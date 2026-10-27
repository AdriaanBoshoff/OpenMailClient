//! Mail accounts: added, listed, changed, removed and tested. A password given to a command
//! goes straight to the credential store and is never given back, put in the status bar or
//! kept in the settings file.

use super::{CommandSpec, always, need_str, p_bool};
use crate::accounts::{self, Account, Kind, MAX_ACCOUNTS};
use crate::{CmdResult, EngineError, Session};
use openmailclient_net::find;
use openmailclient_net::imap::Imap;
use openmailclient_net::pop3::Pop3;
use openmailclient_net::smtp::Smtp;
use openmailclient_net::{Connector, Login, NetError, Protocol, Secret, Vault};
use serde_json::{Value, json};
use std::sync::Arc;

fn has_accounts(s: &Session) -> bool {
    !s.accounts.is_empty()
}

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "account.add",
            label: "Add Account",
            tab: "File",
            group: "Info",
            shortcut: "",
            params: r#"{"email", "kind": "imap" (the default) or "pop3", "incoming": {"host", "port", "security": "tls" (the default), "starttls" or "none"}, "outgoing": the same for SMTP (the incoming host over TLS when not given), "name", "displayName": the name mail is sent under, "user": the sign-in name (the address when not given), "signInToSend" (true), "leaveOnServer" (POP3, true), "caldav": the https: address of a CalDAV server whose calendars are kept in step, signed in to with the same name and password ("" for none), "carddav": the same for a CardDAV server whose address books are kept in step with Contacts folders, "password": kept in the system's credential store, never given back}; returns {"id", "account", "passwordKept"}"#,
            enabled: always,
            run: add,
        },
        CommandSpec {
            id: "account.list",
            label: "Account Settings",
            tab: "File",
            group: "Info",
            shortcut: "",
            params: r#"{}; returns {"accounts": [each account as account.add takes it, with its "id" and "hasPassword"; each server with "trusted" (the fingerprint of a certificate of its own the owner trusted, or null) and "certificates" (those it showed in this run that were not trusted: "host", "fingerprint", "reason", "trustable", "changed", "subject", "issuer", "ownMaking", "names", "notBefore", "notAfter")]}; no password is ever given"#,
            enabled: always,
            run: list,
        },
        CommandSpec {
            id: "account.edit",
            label: "Change Account",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id", and any field account.add takes}; fields not given stay as they were; returns {"account", "passwordKept"}"#,
            enabled: has_accounts,
            run: edit,
        },
        CommandSpec {
            id: "account.remove",
            label: "Remove Account",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id"}; its password is taken out of the credential store as well; mail already taken down stays where it is"#,
            enabled: has_accounts,
            run: remove,
        },
        CommandSpec {
            id: "account.setPassword",
            label: "Set Password",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id", "password": kept in the system's credential store; empty or null forgets it}; returns {"passwordKept"}"#,
            enabled: has_accounts,
            run: set_password,
        },
        CommandSpec {
            id: "account.trustCertificate",
            label: "Trust Certificate",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id", "fingerprint": the SHA-256 of a certificate one of the account's servers showed in this run and that was not trusted, as account.list or a test gives it; null forgets the one trusted, "server": "incoming", "outgoing", "calendar" (its CalDAV server), "contacts" (its CardDAV server) or "both" (the default: each server of the account that showed it)}; the certificate is then taken for that server only, and only while it shows exactly that one; returns {"trusted": [the servers]}"#,
            enabled: has_accounts,
            run: trust_certificate,
        },
        CommandSpec {
            id: "account.test",
            label: "Test Account Settings",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id", "wait": false to test on a thread of its own}; signs in to the incoming and the outgoing server and sends nothing; returns {"incoming": {"ok", "said"}, "outgoing": {"ok", "said"}}"#,
            enabled: has_accounts,
            run: test,
        },
        CommandSpec {
            id: "account.find",
            label: "Find Account Settings",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"email", "wait": false to find them on a thread of its own}; finds the servers for the address from what is known of large providers, the settings its domain publishes, or the usual server names greeted (none signed in to); keeps nothing, and sends the address to no one; returns {"found": the settings as account.add takes them, or null, "from": "known", "published" or "tried", "note", "error"}"#,
            enabled: always,
            run: find_settings,
        },
    ]
}

fn find(s: &Session, p: &Value) -> Result<usize, EngineError> {
    let id = need_str(p, "id")?;
    s.accounts
        .iter()
        .position(|a| a.id == id)
        .ok_or_else(|| EngineError::BadParams(format!("there is no account {}", id.chars().take(64).collect::<String>())))
}

/// A password as a command gives it: `None` to forget it.
fn password(p: &Value) -> Result<Option<Option<Secret>>, EngineError> {
    match p.get("password") {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(s)) if s.is_empty() => Ok(Some(None)),
        Some(Value::String(s)) if s.chars().count() <= 1024 && !s.chars().any(|c| matches!(c, '\r' | '\n' | '\0')) => {
            Ok(Some(Some(Secret::new(s.clone()))))
        }
        Some(_) => Err(EngineError::BadParams("\"password\" is one line of no more than 1024 characters".into())),
    }
}

/// Keep or forget a password; true when one is kept. What the store said goes in the status.
fn keep(s: &mut Session, id: &str, password: Option<Secret>) -> bool {
    let done = match &password {
        Some(secret) => s.vault.set(id, secret),
        None => s.vault.forget(id),
    };
    match done {
        Ok(()) => password.is_some(),
        Err(e) => {
            s.status = format!("The password was not kept: {e}");
            false
        }
    }
}

fn add(s: &mut Session, p: &Value) -> CmdResult {
    if s.accounts.len() >= MAX_ACCOUNTS {
        return Err(EngineError::BadParams(format!("no more than {MAX_ACCOUNTS} accounts are kept")));
    }
    let given = password(p)?;
    let account = Account::from_json(&accounts::changes(p), &accounts::new_id(&s.accounts), None)?;
    if s.accounts.iter().any(|a| a.email.eq_ignore_ascii_case(&account.email)) {
        return Err(EngineError::BadParams(format!("there is an account for {} already", account.email)));
    }
    let id = account.id.clone();
    let name = account.name.clone();
    s.accounts.push(account);
    s.status = match account_store(s, s.accounts.len().saturating_sub(1)) {
        Ok(Some(_)) => format!("Added the account {name}, with a data file of its own for its mail"),
        Ok(None) => format!("Added the account {name}"),
        Err(e) => format!("Added the account {name}; its data file could not be made ({e}), and is tried again when mail comes"),
    };
    let value = s.accounts.last().map(Account::to_json).unwrap_or_default();
    let kept = matches!(given, Some(Some(_))) && keep(s, &id, given.flatten());
    Ok(json!({ "id": id, "account": value, "passwordKept": kept }))
}

fn list(s: &mut Session, _: &Value) -> CmdResult {
    let all: Vec<Value> = s
        .accounts
        .iter()
        .map(|a| {
            let mut v = a.to_json();
            v["hasPassword"] = json!(s.vault.get(&a.id).is_ok_and(|p| p.is_some()));
            for (key, server) in [("incoming", &a.incoming), ("outgoing", &a.outgoing)] {
                let shown: Vec<Value> = s.certificates.of(&server.host).iter().map(accounts::shown).collect();
                v[key]["certificates"] = json!(shown);
            }
            for (key, dav) in [("caldav", &a.caldav), ("carddav", &a.carddav)] {
                if let Some(dav) = dav {
                    let shown: Vec<Value> = s.certificates.of(&dav.host()).iter().map(accounts::shown).collect();
                    v[key]["certificates"] = json!(shown);
                }
            }
            v
        })
        .collect();
    Ok(json!({ "accounts": all }))
}

fn edit(s: &mut Session, p: &Value) -> CmdResult {
    let at = find(s, p)?;
    let given = password(p)?;
    let was = s.accounts.get(at).ok_or_else(|| EngineError::BadParams("there is no such account".into()))?;
    let account = Account::from_json(&accounts::changes(p), &was.id, Some(was))?;
    if s.accounts.iter().enumerate().any(|(i, a)| i != at && a.email.eq_ignore_ascii_case(&account.email)) {
        return Err(EngineError::BadParams(format!("there is an account for {} already", account.email)));
    }
    s.status = format!("Changed the account {}", account.name);
    let (id, value) = (account.id.clone(), account.to_json());
    if let Some(slot) = s.accounts.get_mut(at) {
        *slot = account;
    }
    let kept = match given {
        Some(pw) => keep(s, &id, pw),
        None => s.vault.get(&id).is_ok_and(|p| p.is_some()),
    };
    Ok(json!({ "account": value, "passwordKept": kept }))
}

/// Where the account's own data file is, or is to be made; `None` where the host keeps no folder
/// for them and it has none yet.
pub(crate) fn account_path(s: &Session, account: &Account) -> Option<std::path::PathBuf> {
    match (&account.data_file, &s.account_dir) {
        (Some(p), _) => Some(p.clone()),
        (None, Some(dir)) => Some(dir.join(accounts::data_file_name(account))),
        (None, None) => None,
    }
}

/// What Outlook makes beside an account's mail in its data file: a calendar, people and tasks,
/// kept on this computer only.
const BESIDE_MAIL: [(&str, &str); 3] = [("Calendar", "IPF.Appointment"), ("Contacts", "IPF.Contact"), ("Tasks", "IPF.Task")];

/// An account's data file made before it was given a calendar, people and tasks is given those
/// it lacks, so a meeting can be accepted into it and contacts and tasks kept in it. A folder of
/// that kind already there, by any name, is kept as it is.
fn given_beside_mail(s: &mut Session, idx: usize) -> Result<(), EngineError> {
    let mut made = false;
    for (name, class) in BESIDE_MAIL {
        let kind = openmailclient_model::FolderKind::guess(name, class);
        if s.find_folder(idx, kind).is_some() {
            continue;
        }
        let Some(store) = s.stores.get_mut(idx) else { return Ok(()) };
        store.create_folder_of(None, name, class)?;
        made = true;
    }
    if made {
        s.refresh_folders(idx);
    }
    Ok(())
}

/// The data file the account `at` keeps its mail in, opened, as Outlook keeps one for each
/// account so that its mail is never mixed with another's: made, with the folders a mailbox
/// has, the first time it is needed, in the program's own folder. `None` where the host keeps
/// no such folder and the account has no data file yet: its mail then goes where new mail goes.
pub(crate) fn account_store(s: &mut Session, at: usize) -> Result<Option<usize>, EngineError> {
    let Some(account) = s.accounts.get(at) else { return Ok(None) };
    let Some(path) = account_path(s, account) else { return Ok(None) };
    let name = account.name.clone();
    let real = std::fs::canonicalize(&path).ok();
    let open = s.stores.iter().position(|st| real.is_some() && st.path().and_then(|p| std::fs::canonicalize(p).ok()) == real);
    let idx = match open {
        Some(idx) => idx,
        None => {
            if !path.exists() {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                let mut mailbox = openmailclient_model::MemStore::with_default_folders(&name);
                for (folder, class) in BESIDE_MAIL {
                    mailbox.push_folder(None, folder, class);
                }
                crate::io::save_store(&mut mailbox, &path, crate::io::new_store_id(s.now))?;
            }
            match crate::io::open_path(&path)? {
                crate::io::Opened::Store(mut store) => {
                    if let Some(dir) = &s.journal_dir {
                        store.journal_elsewhere(dir);
                    }
                    // Unsaved mail of a run that ended comes back, as with any data file.
                    let _ = store.recover();
                    s.add_store(store)
                }
                crate::io::Opened::Item(_) => return Err(EngineError::Format(format!("{} is not a data file", path.display()))),
            }
        }
    };
    if !s.stores.get(idx).is_some_and(|st| st.writable()) {
        return Err(EngineError::Format(format!("the data file of {name} cannot be written: {}", path.display())));
    }
    given_beside_mail(s, idx)?;
    if let Some(a) = s.accounts.get_mut(at) {
        a.data_file = Some(path);
    }
    Ok(Some(idx))
}

/// Open the data file of each account, as the settings left it. Where the host keeps an
/// account folder, an account without one yet (one added before accounts had their own) is
/// given it now, so its mailbox is there from the start; elsewhere a file no longer there is
/// made again when mail next comes.
pub(crate) fn open_account_stores(s: &mut Session) -> Vec<String> {
    let mut trouble = Vec::new();
    for at in 0..s.accounts.len() {
        let there = s.accounts.get(at).and_then(|a| a.data_file.as_ref()).is_some_and(|p| p.exists());
        let made_here = s.account_dir.is_some() && s.accounts.get(at).is_some_and(|a| a.data_file.is_none());
        if (there || made_here)
            && let Err(e) = account_store(s, at)
        {
            trouble.push(e.to_string());
        }
    }
    trouble
}

fn remove(s: &mut Session, p: &Value) -> CmdResult {
    let at = find(s, p)?;
    let account = s.accounts.remove(at);
    s.status = match &account.data_file {
        Some(file) => format!("Removed the account {}; its mail stays in {}", account.name, file.display()),
        None => format!("Removed the account {}", account.name),
    };
    if let Err(e) = s.vault.forget(&account.id) {
        s.status = format!("Removed the account {}; its password could not be taken out of the credential store: {e}", account.name);
    }
    Ok(json!({ "removed": account.id }))
}

/// Trust a certificate a server showed, for that server only; or forget the one trusted.
fn trust_certificate(s: &mut Session, p: &Value) -> CmdResult {
    let at = find(s, p)?;
    let which = match p.get("server").and_then(Value::as_str) {
        None | Some("both") => &["incoming", "outgoing", "calendar", "contacts"][..],
        Some("incoming") => &["incoming"][..],
        Some("outgoing") => &["outgoing"][..],
        Some("calendar") => &["calendar"][..],
        Some("contacts") => &["contacts"][..],
        Some(_) => return Err(EngineError::BadParams("\"server\" is \"incoming\", \"outgoing\", \"calendar\", \"contacts\" or \"both\"".into())),
    };
    let fingerprint = match p.get("fingerprint") {
        Some(Value::Null) => None,
        Some(Value::String(f)) => Some(
            openmailclient_net::Fingerprint::parse(f)
                .ok_or_else(|| EngineError::BadParams("\"fingerprint\" is a SHA-256 as 64 hex digits, colons between pairs or not".into()))?,
        ),
        _ => return Err(EngineError::BadParams("\"fingerprint\" (as account.list gives it, or null to forget) is required".into())),
    };
    let account = s.accounts.get(at).cloned().ok_or_else(|| EngineError::BadParams("there is no such account".into()))?;
    let mut done = Vec::new();
    let mut refused = None;
    for key in which {
        let (host, trusted) = match *key {
            "incoming" => (account.incoming.host.clone(), account.incoming.trusted),
            "outgoing" => (account.outgoing.host.clone(), account.outgoing.trusted),
            key => match if key == "contacts" { &account.carddav } else { &account.caldav } {
                Some(dav) => (dav.host(), dav.trusted),
                None => continue,
            },
        };
        let ok = match fingerprint {
            // Only what the server itself showed, and was not revoked, can be trusted.
            Some(f) => match s.certificates.of(&host).into_iter().find(|seen| seen.fingerprint == f) {
                Some(seen) if seen.trustable => true,
                Some(seen) => {
                    refused = Some(format!("the certificate of {} cannot be trusted: {}", seen.host, seen.reason));
                    false
                }
                None => false,
            },
            None => trusted.is_some(),
        };
        if ok {
            done.push((*key, host));
        }
    }
    if done.is_empty() {
        return Err(EngineError::BadParams(refused.unwrap_or_else(|| match fingerprint {
            Some(_) => format!("no server of {} showed that certificate in this run; test the account to be shown it", account.name),
            None => format!("no certificate is trusted for a server of {}", account.name),
        })));
    }
    if let Some(a) = s.accounts.get_mut(at) {
        for (key, host) in &done {
            match *key {
                "incoming" => a.incoming.trusted = fingerprint,
                "outgoing" => a.outgoing.trusted = fingerprint,
                key => {
                    let dav = if key == "contacts" { a.carddav.as_mut() } else { a.caldav.as_mut() };
                    if let Some(dav) = dav {
                        dav.trusted = fingerprint;
                    }
                }
            }
            if let Some(f) = fingerprint {
                s.certificates.forget(host, f);
            }
        }
    }
    let hosts: Vec<&str> = done.iter().map(|(_, h)| h.as_str()).collect();
    let mut hosts_said = hosts.clone();
    hosts_said.dedup();
    s.status = match fingerprint {
        Some(_) => format!("Trusted the certificate of {} for {} only", hosts_said.join(" and "), account.name),
        None => format!("{} is checked against what this system trusts again", hosts_said.join(" and ")),
    };
    let trusted: Vec<&str> = done.iter().map(|(k, _)| *k).collect();
    Ok(json!({ "trusted": trusted }))
}

fn set_password(s: &mut Session, p: &Value) -> CmdResult {
    let at = find(s, p)?;
    let given = password(p)?.ok_or_else(|| EngineError::BadParams("\"password\" is required (empty or null forgets it)".into()))?;
    let (id, name) = s.accounts.get(at).map(|a| (a.id.clone(), a.name.clone())).unwrap_or_default();
    let forgetting = given.is_none();
    s.status = if forgetting { format!("Forgot the password of {name}") } else { format!("Kept the password of {name}") };
    let kept = keep(s, &id, given);
    Ok(json!({ "passwordKept": kept }))
}

/// What a session needs to reach an account's servers, taken away from it for a job.
pub(crate) struct Reach {
    pub account: Account,
    pub connector: Arc<dyn Connector>,
    pub vault: Arc<dyn Vault>,
}

impl Reach {
    pub(crate) fn of(s: &Session, at: usize) -> Result<Self, EngineError> {
        let account = s.accounts.get(at).cloned().ok_or_else(|| EngineError::BadParams("there is no such account".into()))?;
        let connector = s.connector.clone().ok_or_else(|| EngineError::Format("this build cannot reach mail servers".into()))?;
        let connector: Arc<dyn Connector> = Arc::new(accounts::Noting { inner: connector, shown: s.certificates.clone() });
        Ok(Self { account, connector, vault: Arc::clone(&s.vault) })
    }

    /// The name and password to sign in with; read from the store only when needed.
    pub(crate) fn login(&self) -> Result<Login, NetError> {
        match self.vault.get(&self.account.id)? {
            Some(password) => Ok(Login { user: self.account.user.clone(), password }),
            None => Err(NetError::Unsupported(format!("no password is kept for {}; set one with account.setPassword", self.account.name))),
        }
    }
}

fn test_incoming(r: &Reach) -> Result<String, NetError> {
    let login = r.login()?;
    let to = r.account.incoming_endpoint();
    match r.account.kind {
        Kind::Imap => {
            let mut imap = Imap::open(&*r.connector, &to, &login)?;
            let folders = imap.folders()?.len();
            imap.logout()?;
            Ok(format!("signed in; {folders} folders on the server"))
        }
        Kind::Pop3 => {
            let mut pop = Pop3::open(&*r.connector, &to, &login)?;
            let messages = pop.list()?.len();
            pop.quit()?;
            Ok(format!("signed in; {messages} messages on the server"))
        }
    }
}

fn test_outgoing(r: &Reach) -> Result<String, NetError> {
    let login = if r.account.sign_in_to_send { Some(r.login()?) } else { None };
    let smtp = Smtp::open(&*r.connector, &r.account.outgoing_endpoint(), login.as_ref())?;
    smtp.quit()?;
    Ok(if login.is_some() { "signed in".to_string() } else { "answered".to_string() })
}

fn server_json(server: &find::Server) -> Value {
    json!({ "host": server.host, "port": server.port, "security": accounts::security_name(server.security) })
}

fn find_settings(s: &mut Session, p: &Value) -> CmdResult {
    let email = need_str(p, "email")?.trim().to_string();
    let Some((_, domain)) = find::parts(&email) else {
        return Err(EngineError::BadParams("\"email\" is not an email address".into()));
    };
    let connector = s.connector.clone().ok_or_else(|| EngineError::Format("this build cannot reach mail servers".into()))?;
    let wait = p_bool(p, "wait") != Some(false);
    s.begin(format!("Finding the settings for {domain}"), wait, move |progress| {
        let found = find::find(&*connector, &email, &|| progress.stopped());
        let (status, value) = match found {
            Ok(f) => {
                let kind = if f.protocol == Protocol::Pop3 { "pop3" } else { "imap" };
                let whence = match f.from {
                    find::Source::Known => "as they are known for its provider",
                    find::Source::Published => "as its domain publishes them",
                    find::Source::Records => "as its domain's DNS records name them",
                    find::Source::Exchanger => "for the provider that takes its mail",
                    find::Source::Tried => "from servers that answered",
                };
                let settings = json!({ "email": email, "kind": kind, "incoming": server_json(&f.incoming), "outgoing": server_json(&f.outgoing), "user": f.user });
                let status = format!("Found the settings for {domain}, {whence}: {} and {}", f.incoming.host, f.outgoing.host);
                (status, json!({ "found": settings, "from": f.from.word(), "note": f.note }))
            }
            Err(e) => (format!("The settings for {domain} were not found: {e}"), json!({ "found": null, "error": e.to_string() })),
        };
        Box::new(move |_| (status, value))
    })
}

fn test(s: &mut Session, p: &Value) -> CmdResult {
    let at = find(s, p)?;
    let reach = Reach::of(s, at)?;
    let wait = p_bool(p, "wait") != Some(false);
    let name = reach.account.name.clone();
    s.begin(format!("Testing {name}"), wait, move |progress| {
        progress.begin(2);
        let incoming = test_incoming(&reach);
        progress.step();
        let outgoing = test_outgoing(&reach);
        progress.step();
        let status = match (&incoming, &outgoing) {
            (Ok(_), Ok(_)) => format!("{name}: both servers answered, and nothing was sent"),
            (Err(e), Ok(_)) => format!("{name}: the incoming server failed: {e}"),
            (Ok(_), Err(e)) => format!("{name}: the outgoing server failed: {e}"),
            (Err(e), Err(_)) => format!("{name}: neither server could be used: {e}"),
        };
        let value = json!({ "incoming": accounts::outcome(incoming), "outgoing": accounts::outcome(outgoing) });
        Box::new(move |_| (status, value))
    })
}
