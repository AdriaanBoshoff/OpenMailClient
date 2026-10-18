//! Mail accounts: where mail comes from (IMAP or POP3) and where it goes out (SMTP), as the
//! owner set them up. An account is kept in the settings file without its password, which is
//! kept in the system's credential store under the account's id (`openmailclient_net::Vault`).
//!
//! What an account says comes from a file or a command and is checked as anything else from
//! outside is: a value that makes no sense is refused, never kept.

use crate::EngineError;
use openmailclient_net::{Endpoint, Fingerprint, Protocol, Security, Seen};
use serde_json::{Map, Value, json};

/// Accounts kept at most.
pub const MAX_ACCOUNTS: usize = 64;

/// How mail is taken from the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Folders stay on the server and are kept in step with it.
    Imap,
    /// New mail is taken down into the Inbox.
    Pop3,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Imap => "imap",
            Self::Pop3 => "pop3",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "imap" => Some(Self::Imap),
            "pop3" | "pop" => Some(Self::Pop3),
            _ => None,
        }
    }

    fn protocol(self) -> Protocol {
        match self {
            Self::Imap => Protocol::Imap,
            Self::Pop3 => Protocol::Pop3,
        }
    }
}

/// One server of an account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub host: String,
    pub port: u16,
    pub security: Security,
    /// A certificate of the server's own that the owner trusted, though this system does not:
    /// taken for this server only, and only while it shows exactly that one. Set only by
    /// `account.trustCertificate` and the settings file, and dropped when the host changes.
    pub trusted: Option<Fingerprint>,
}

/// What was taken down from an account so far, so nothing is taken down twice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Received {
    /// IMAP: the Inbox's UIDVALIDITY when its ids were last read; another value means the
    /// server numbered its messages anew ([RFC 9051] 2.3.1.1).
    pub uid_validity: u32,
    /// IMAP: the highest id taken down.
    pub last_uid: u32,
    /// POP3: the unique ids of the messages taken down that the server still had last time.
    pub seen: Vec<String>,
    /// The data file what was taken down went into; empty for the one new mail is kept in.
    /// Taken into another file, nothing of it is in this one: all of it is taken down again.
    pub into: String,
    /// IMAP: how far each folder of the server other than the Inbox was taken down.
    pub folders: Vec<Mark>,
    /// IMAP: the Message-IDs of messages sent from here and put in the server's Sent folder,
    /// so they are not taken down again beside the copy Sent Items keeps already.
    pub sent_here: Vec<String>,
    /// IMAP: messages deleted here for good whose copy on the server is to go too, at the next
    /// Send/Receive.
    pub gone: Vec<Gone>,
    /// IMAP: the messages taken down before each was marked with its place on the server have
    /// been found there again by their Message-ID; until then nothing taken down earlier is put
    /// on the server as if it were new.
    pub matched: bool,
    /// IMAP: each folder as it was when it was last kept in step, so that only what changed
    /// since is asked for where the server keeps mod-sequences ([RFC 7162]).
    pub steps: Vec<Step>,
    /// IMAP: folders deleted here for good whose folder on the server is to go too, at the next
    /// Send/Receive. Only a folder deleted here by the owner goes there: a folder that is merely
    /// missing here, as in a data file made anew, is taken down again instead.
    pub folders_gone: Vec<GoneFolder>,
}

/// A folder of an IMAP server that is to be deleted there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GoneFolder {
    pub raw: String,
    /// Its UIDVALIDITY as it was known: a folder made there anew by that name since stays.
    pub uid_validity: u32,
}

/// An IMAP folder as it was when it was last kept in step with what is here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Step {
    /// The folder's name as the server knows it.
    pub raw: String,
    pub uid_validity: u32,
    /// Its highest mod-sequence then: every change after it has a higher one.
    pub modseq: u64,
    /// How many messages it held, and the id the next would have.
    pub exists: u64,
    pub uid_next: u32,
}

/// A message of an IMAP folder that is to be deleted from the server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Gone {
    pub raw: String,
    pub uid_validity: u32,
    pub uid: u32,
}

/// How far one IMAP folder was taken down, and the folder here it is kept in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mark {
    /// The folder's name as the server knows it.
    pub raw: String,
    pub uid_validity: u32,
    pub last_uid: u32,
    /// The folder of the account's data file it is kept in, once known; 0 until then.
    pub here: u64,
    /// The folder here's parent and name when the two were last in step: a folder moved or
    /// renamed here since is moved or renamed on the server.
    pub agreed: Option<(Option<u64>, String)>,
    /// The server's own id for the folder, which stays with it when it is renamed ([RFC 8474]).
    pub mailbox_id: String,
}

/// The name a folder of the server has once `from` is renamed `to`: `from` itself, or a folder
/// inside it. None for any other.
pub(crate) fn renamed_raw(raw: &str, from: &str, to: &str, delimiter: Option<char>) -> Option<String> {
    if raw == from {
        return Some(to.to_string());
    }
    let d = delimiter?;
    let rest = raw.strip_prefix(from)?.strip_prefix(d)?;
    Some(format!("{to}{d}{rest}"))
}

/// A folder id as a server gives it ([RFC 8474] 4): letters, digits, `-` and `_`.
fn kept_object_id(v: Option<&Value>) -> String {
    v.and_then(Value::as_str)
        .filter(|t| !t.is_empty() && t.len() <= 255 && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
        .unwrap_or_default()
        .to_string()
}

/// POP3 ids remembered at most; the server's list is the bound in practice.
pub const MAX_SEEN: usize = 100_000;
/// IMAP folders followed at most, and Message-IDs of what was put in Sent remembered.
pub const MAX_FOLDERS: usize = 1000;
pub const MAX_SENT_HERE: usize = 1000;
/// Deletions waiting to go to a server.
pub const MAX_GONE: usize = 100_000;

/// A name or id as kept in the settings: not empty, not too long, nothing that is not text.
fn kept_text(v: &Value, max: usize) -> Option<&str> {
    v.as_str().filter(|t| !t.is_empty() && t.len() <= max && !t.chars().any(char::is_control))
}

impl Received {
    pub(crate) fn to_json(&self) -> Value {
        let folders: Vec<Value> = self
            .folders
            .iter()
            .map(|m| {
                let mut v = json!({ "raw": m.raw, "uidValidity": m.uid_validity, "lastUid": m.last_uid });
                if m.here != 0 {
                    v["here"] = json!(m.here);
                }
                if let Some((parent, name)) = &m.agreed {
                    v["agreedParent"] = json!(parent);
                    v["agreedName"] = json!(name);
                }
                if !m.mailbox_id.is_empty() {
                    v["mailboxId"] = json!(m.mailbox_id);
                }
                v
            })
            .collect();
        let folders_gone: Vec<Value> = self.folders_gone.iter().map(|g| json!({ "raw": g.raw, "uidValidity": g.uid_validity })).collect();
        let gone: Vec<Value> = self.gone.iter().map(|g| json!({ "raw": g.raw, "uidValidity": g.uid_validity, "uid": g.uid })).collect();
        // A mod-sequence may be past what a JSON number holds exactly: kept as text.
        let steps: Vec<Value> = self
            .steps
            .iter()
            .map(
                |t| json!({ "raw": t.raw, "uidValidity": t.uid_validity, "modseq": t.modseq.to_string(), "exists": t.exists, "uidNext": t.uid_next }),
            )
            .collect();
        json!({ "uidValidity": self.uid_validity, "lastUid": self.last_uid, "seen": self.seen, "into": self.into,
                "folders": folders, "sentHere": self.sent_here, "gone": gone, "matched": self.matched, "steps": steps,
                "foldersGone": folders_gone })
    }

    /// As a settings file gives it: what makes no sense is left as nothing taken down yet.
    pub(crate) fn from_json(v: Option<&Value>) -> Self {
        let Some(v) = v else { return Self::default() };
        let number = |v: &Value, key: &str| v.get(key).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
        let n = |key: &str| number(v, key);
        let mut folders: Vec<Mark> = Vec::new();
        for f in v.get("folders").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().take(MAX_FOLDERS) {
            let Some(raw) = f.get("raw").and_then(|r| kept_text(r, 1000)) else { continue };
            if !folders.iter().any(|m| m.raw == raw) {
                let agreed = f
                    .get("agreedName")
                    .and_then(|n| kept_text(n, 1000))
                    .map(|name| (f.get("agreedParent").and_then(Value::as_u64), name.to_string()));
                folders.push(Mark {
                    raw: raw.to_string(),
                    uid_validity: number(f, "uidValidity"),
                    last_uid: number(f, "lastUid"),
                    here: f.get("here").and_then(Value::as_u64).unwrap_or(0),
                    agreed,
                    mailbox_id: kept_object_id(f.get("mailboxId")),
                });
            }
        }
        let sent_here = v
            .get("sentHere")
            .and_then(Value::as_array)
            .map(|all| all.iter().take(MAX_SENT_HERE).filter_map(|m| kept_text(m, 998)).map(str::to_string).collect())
            .unwrap_or_default();
        let seen = v
            .get("seen")
            .and_then(Value::as_array)
            .map(|all| {
                all.iter()
                    .take(MAX_SEEN)
                    .filter_map(Value::as_str)
                    .filter(|u| !u.is_empty() && u.len() <= 70 && u.bytes().all(|b| (0x21..=0x7E).contains(&b)))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let mut gone: Vec<Gone> = Vec::new();
        for g in v.get("gone").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().take(MAX_GONE) {
            let Some(raw) = g.get("raw").and_then(|r| kept_text(r, 1000)) else { continue };
            let one = Gone { raw: raw.to_string(), uid_validity: number(g, "uidValidity"), uid: number(g, "uid") };
            if one.uid > 0 && one.uid_validity > 0 && !gone.contains(&one) {
                gone.push(one);
            }
        }
        let mut steps: Vec<Step> = Vec::new();
        for t in v.get("steps").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().take(MAX_FOLDERS) {
            let Some(raw) = t.get("raw").and_then(|r| kept_text(r, 1000)) else { continue };
            let modseq = t.get("modseq").and_then(Value::as_str).and_then(|m| m.parse::<u64>().ok()).filter(|m| *m > 0 && *m < 1 << 63);
            let (Some(modseq), uid_validity) = (modseq, number(t, "uidValidity")) else { continue };
            if uid_validity > 0 && !steps.iter().any(|x| x.raw == raw) {
                let exists = t.get("exists").and_then(Value::as_u64).unwrap_or(0);
                steps.push(Step { raw: raw.to_string(), uid_validity, modseq, exists, uid_next: number(t, "uidNext") });
            }
        }
        let mut folders_gone: Vec<GoneFolder> = Vec::new();
        for g in v.get("foldersGone").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().take(MAX_FOLDERS) {
            let Some(raw) = g.get("raw").and_then(|r| kept_text(r, 1000)) else { continue };
            if !raw.eq_ignore_ascii_case("INBOX") && !folders_gone.iter().any(|x| x.raw == raw) {
                folders_gone.push(GoneFolder { raw: raw.to_string(), uid_validity: number(g, "uidValidity") });
            }
        }
        let matched = v.get("matched").and_then(Value::as_bool).unwrap_or(false);
        let into = v.get("into").and_then(Value::as_str).filter(|p| p.len() <= 1024 && !p.chars().any(char::is_control)).unwrap_or_default();
        Self {
            uid_validity: n("uidValidity"),
            last_uid: n("lastUid"),
            seen,
            into: into.to_string(),
            folders,
            sent_here,
            gone,
            matched,
            steps,
            folders_gone,
        }
    }

    /// What was taken down into the data file `into`: nothing, when it went into another.
    pub fn kept_in(&self, into: &str) -> Self {
        if self.into == into { self.clone() } else { Self { into: into.to_string(), sent_here: self.sent_here.clone(), ..Self::default() } }
    }

    /// Where a folder was left when it was last kept in step.
    pub fn step(&self, raw: &str) -> Option<&Step> {
        self.steps.iter().find(|t| t.raw == raw)
    }

    /// Remember where a folder was left, in place of what was remembered of it.
    pub fn stepped(&mut self, step: Step) {
        if let Some(t) = self.steps.iter_mut().find(|t| t.raw == step.raw) {
            *t = step;
        } else if self.steps.len() < MAX_FOLDERS {
            self.steps.push(step);
        }
    }

    /// A folder of the server was renamed `from` to `to`, with the folders inside it: what is
    /// remembered of each goes with it.
    pub fn renamed(&mut self, from: &str, to: &str, delimiter: Option<char>) {
        // What was remembered by the new name is not the folder now there: it goes.
        let stale = |raw: &str| renamed_raw(raw, to, to, delimiter).is_some();
        self.folders.retain(|m| !stale(&m.raw));
        self.steps.retain(|t| !stale(&t.raw));
        self.gone.retain(|g| !stale(&g.raw));
        self.folders_gone.retain(|g| !stale(&g.raw));
        let new = |raw: &mut String| {
            if let Some(r) = renamed_raw(raw, from, to, delimiter) {
                *raw = r;
            }
        };
        self.folders.iter_mut().for_each(|m| new(&mut m.raw));
        self.steps.iter_mut().for_each(|t| new(&mut t.raw));
        self.gone.iter_mut().for_each(|g| new(&mut g.raw));
        self.folders_gone.iter_mut().for_each(|g| new(&mut g.raw));
    }

    /// Remember that a folder of the server is to be deleted there.
    pub fn delete_folder_on_server(&mut self, g: GoneFolder) {
        if g.raw.is_empty() || g.raw.eq_ignore_ascii_case("INBOX") || self.folders_gone.iter().any(|x| x.raw == g.raw) {
            return;
        }
        self.folders_gone.push(g);
        if self.folders_gone.len() > MAX_FOLDERS {
            let over = self.folders_gone.len() - MAX_FOLDERS;
            self.folders_gone.drain(..over);
        }
    }

    /// Remember that a message of the server is to be deleted there.
    pub fn delete_on_server(&mut self, g: Gone) {
        if g.uid == 0 || self.gone.contains(&g) {
            return;
        }
        self.gone.push(g);
        if self.gone.len() > MAX_GONE {
            let over = self.gone.len() - MAX_GONE;
            self.gone.drain(..over);
        }
    }

    /// Remember a message put in the server's Sent folder, the oldest forgotten past the bound.
    pub fn put_in_sent(&mut self, message_id: &str) {
        let id = message_id.trim();
        if id.is_empty() || self.sent_here.iter().any(|m| m == id) {
            return;
        }
        self.sent_here.push(id.to_string());
        if self.sent_here.len() > MAX_SENT_HERE {
            let over = self.sent_here.len() - MAX_SENT_HERE;
            self.sent_here.drain(..over);
        }
    }
}

/// Calendars kept in step with a CalDAV server ([RFC 4791]), signed in to with the account's
/// own name and password.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DavSetup {
    /// Where the server is, as the owner named it: `https:` only.
    pub url: String,
    /// A certificate of the server's own the owner trusted, for its host only.
    pub trusted: Option<Fingerprint>,
    /// The server's calendars as they were when last kept in step.
    pub calendars: Vec<DavCal>,
}

/// A calendar of a CalDAV server, and the folder here it is kept in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DavCal {
    /// Its path on the server.
    pub href: String,
    pub name: String,
    /// `#RRGGBB`, or empty.
    pub color: String,
    /// Its sync token when last kept in step ([RFC 6578]); empty when the server keeps none.
    pub token: String,
    /// The folder of the account's data file it is kept in.
    pub here: u64,
    /// Nothing changed here is put there.
    pub read_only: bool,
}

/// Calendars of one server kept in step at most.
pub const MAX_DAV_CALENDARS: usize = 100;

impl DavSetup {
    pub fn to_json(&self) -> Value {
        let calendars: Vec<Value> = self
            .calendars
            .iter()
            .map(|c| json!({ "href": c.href, "name": c.name, "color": c.color, "token": c.token, "here": c.here, "readOnly": c.read_only }))
            .collect();
        json!({ "url": self.url, "trusted": self.trusted.map(|f| f.to_string()), "calendars": calendars })
    }

    /// As the settings file keeps it; what makes no sense is left out.
    fn from_settings(v: Option<&Value>) -> Option<Self> {
        let v = v?;
        let url = openmailclient_net::dav::Url::parse(v.get("url").and_then(Value::as_str)?).ok()?.text();
        let mut calendars: Vec<DavCal> = Vec::new();
        for c in v.get("calendars").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().take(MAX_DAV_CALENDARS) {
            let Some(href) = c.get("href").and_then(Value::as_str) else { continue };
            let plain = href.starts_with('/') && href.len() <= 2048 && href.bytes().all(|b| (0x21..0x7F).contains(&b));
            if !plain || calendars.iter().any(|x| x.href == href) {
                continue;
            }
            let short = |key: &str, max: usize| c.get(key).and_then(|t| kept_text(t, max)).unwrap_or_default().to_string();
            calendars.push(DavCal {
                href: href.to_string(),
                name: short("name", 200),
                color: short("color", 16),
                token: short("token", 1000),
                here: c.get("here").and_then(Value::as_u64).unwrap_or(0),
                read_only: c.get("readOnly").and_then(Value::as_bool).unwrap_or(false),
            });
        }
        let trusted = v.get("trusted").and_then(Value::as_str).and_then(Fingerprint::parse);
        Some(Self { url, trusted, calendars })
    }

    /// The server's host.
    pub fn host(&self) -> String {
        openmailclient_net::dav::Url::parse(&self.url).map(|u| u.host).unwrap_or_default()
    }
}

/// The CalDAV server an edit leaves: the one given (`"caldav"`: its address, or
/// `{"url"}`; `""` or `false` for none), else the one before. Another address starts over;
/// a certificate trusted stays only for the same host.
fn caldav(p: &Value, was: Option<&DavSetup>) -> Result<Option<DavSetup>, EngineError> {
    let url = match p.get("caldav") {
        None | Some(Value::Null) => return Ok(was.cloned()),
        Some(Value::Bool(false)) => return Ok(None),
        Some(Value::String(u)) if u.trim().is_empty() => return Ok(None),
        Some(Value::String(u)) => u.as_str(),
        Some(v) => match v.get("url").and_then(Value::as_str) {
            Some(u) if u.trim().is_empty() => return Ok(None),
            Some(u) => u,
            None => return Err(bad("\"caldav\" is a calendar server's https: address, or {\"url\"}")),
        },
    };
    let parsed = openmailclient_net::dav::Url::parse(url).map_err(|e| bad(e.to_string()))?;
    let url = parsed.text();
    Ok(Some(match was {
        Some(w) if w.url == url => w.clone(),
        Some(w) if w.host() == parsed.host => DavSetup { url, trusted: w.trusted, calendars: Vec::new() },
        _ => DavSetup { url, ..DavSetup::default() },
    }))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    /// What its password is kept under; never changes once made.
    pub id: String,
    /// What the account is called in the folder pane and lists: its address unless named.
    pub name: String,
    pub email: String,
    /// The name mail from it is sent under.
    pub display_name: String,
    pub kind: Kind,
    /// The name the account signs in with: its address unless given.
    pub user: String,
    pub incoming: Server,
    pub outgoing: Server,
    /// Whether the outgoing server is signed in to, with the same name and password.
    pub sign_in_to_send: bool,
    /// POP3: a copy of each message is left on the server once taken down.
    pub leave_on_server: bool,
    /// IMAP: a copy of what is sent is put in the server's Sent folder, so the owner's other
    /// mail programs see it too.
    pub sent_on_server: bool,
    /// What was taken down so far; kept in the settings, not given by `account.list`.
    pub received: Received,
    /// The data file the account's mail is kept in, as Outlook keeps one for each account:
    /// made in the program's own folder the first time it is needed. Set only by the program
    /// and the settings file, never by a command.
    pub data_file: Option<std::path::PathBuf>,
    /// The CalDAV server the account's calendars are kept in step with, when it has one.
    pub caldav: Option<DavSetup>,
}

pub(crate) fn security_name(s: Security) -> &'static str {
    match s {
        Security::Tls => "tls",
        Security::StartTls => "starttls",
        Security::Plain => "none",
    }
}

fn parse_security(s: &str) -> Option<Security> {
    match s.to_ascii_lowercase().as_str() {
        "tls" | "ssl" | "ssl/tls" => Some(Security::Tls),
        "starttls" => Some(Security::StartTls),
        "none" | "plain" => Some(Security::Plain),
        _ => None,
    }
}

fn bad(what: impl Into<String>) -> EngineError {
    EngineError::BadParams(what.into())
}

/// Text of a field: one line, no more than `max` characters.
fn text<'a>(p: &'a Value, key: &str, max: usize) -> Result<Option<&'a str>, EngineError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.chars().count() <= max && !s.chars().any(char::is_control) => Ok(Some(s.trim())),
        Some(_) => Err(bad(format!("\"{key}\" is one line of no more than {max} characters"))),
    }
}

/// An address mail can be sent from: one `@` with something either side, no spaces.
pub fn is_address(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else { return false };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && s.len() <= 320
        && !s.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | ',' | ';'))
}

/// A server's name, or an address in brackets.
fn is_host(s: &str) -> bool {
    !s.is_empty() && s.len() <= 253 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']'))
}

fn server(p: &Value, key: &str, protocol: Protocol, was: Option<&Server>) -> Result<Server, EngineError> {
    let Some(v) = p.get(key).filter(|v| !v.is_null()) else {
        return was.cloned().ok_or_else(|| bad(format!("\"{key}\" ({{\"host\", \"port\", \"security\"}}) is required")));
    };
    if !v.is_object() {
        return Err(bad(format!("\"{key}\" is {{\"host\", \"port\", \"security\"}}")));
    }
    let host = match text(v, "host", 253)? {
        Some(h) => h.to_ascii_lowercase(),
        None => was.map(|w| w.host.clone()).ok_or_else(|| bad(format!("\"{key}\" needs a \"host\"")))?,
    };
    if !is_host(&host) {
        return Err(bad(format!("\"{key}\": {} is not a server name", host.chars().take(80).collect::<String>())));
    }
    let security = match text(v, "security", 16)? {
        Some(s) => parse_security(s).ok_or_else(|| bad(format!("\"{key}\": \"security\" is \"tls\", \"starttls\" or \"none\"")))?,
        None => was.map_or(Security::Tls, |w| w.security),
    };
    let port = match v.get("port").filter(|v| !v.is_null()) {
        Some(n) => {
            n.as_u64().and_then(|n| u16::try_from(n).ok()).filter(|n| *n > 0).ok_or_else(|| bad(format!("\"{key}\": \"port\" is from 1 to 65535")))?
        }
        // A port not given follows the security, unless one was set before and the security
        // stays as it was.
        None => match was {
            Some(w) if w.security == security => w.port,
            _ => protocol.port(security),
        },
    };
    // A certificate trusted for one server says nothing of another.
    let trusted = was.filter(|w| w.host == host).and_then(|w| w.trusted);
    Ok(Server { host, port, security, trusted })
}

fn flag(p: &Value, key: &str) -> Result<Option<bool>, EngineError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(format!("\"{key}\" is true or false"))),
    }
}

/// The sign-in name an edit leaves: the one given, else the one before.
fn p_user<'a>(p: &'a Value, was: &'a Account) -> &'a str {
    p.get("user").and_then(Value::as_str).map(str::trim).filter(|u| !u.is_empty()).unwrap_or(&was.user)
}

impl Account {
    /// An account made from what is given, or `was` changed by it.
    pub fn from_json(p: &Value, id: &str, was: Option<&Account>) -> Result<Self, EngineError> {
        if !p.is_object() {
            return Err(bad("an account is an object"));
        }
        let email = match text(p, "email", 320)? {
            Some(e) => e.to_string(),
            None => was.map(|w| w.email.clone()).ok_or_else(|| bad("\"email\" (the account's address) is required"))?,
        };
        if !is_address(&email) {
            return Err(bad(format!("{} is not an address mail can be sent from", email.chars().take(80).collect::<String>())));
        }
        let kind = match text(p, "kind", 8)? {
            Some(k) => Kind::parse(k).ok_or_else(|| bad("\"kind\" is \"imap\" or \"pop3\""))?,
            None => was.map_or(Kind::Imap, |w| w.kind),
        };
        // A changed kind leaves nothing of the incoming server it had.
        let was_in = was.filter(|w| w.kind == kind).map(|w| &w.incoming);
        let incoming = server(p, "incoming", kind.protocol(), was_in)?;
        // The outgoing server is the incoming one's host when not given.
        let fallback = Server { host: incoming.host.clone(), port: Protocol::Smtp.port(Security::Tls), security: Security::Tls, trusted: None };
        let outgoing = server(p, "outgoing", Protocol::Smtp, Some(was.map_or(&fallback, |w| &w.outgoing)))?;
        let pick = |key: &str, max: usize, before: Option<&str>, or: &str| -> Result<String, EngineError> {
            Ok(match text(p, key, max)? {
                Some(t) if !t.is_empty() => t.to_string(),
                Some(_) => or.to_string(),
                None => before.map_or_else(|| or.to_string(), str::to_string),
            })
        };
        Ok(Self {
            id: id.to_string(),
            name: pick("name", 100, was.map(|w| w.name.as_str()), &email)?,
            display_name: pick("displayName", 200, was.map(|w| w.display_name.as_str()), "")?,
            user: pick("user", 320, was.map(|w| w.user.as_str()), &email)?,
            sign_in_to_send: flag(p, "signInToSend")?.unwrap_or(was.is_none_or(|w| w.sign_in_to_send)),
            leave_on_server: flag(p, "leaveOnServer")?.unwrap_or(was.is_none_or(|w| w.leave_on_server)),
            sent_on_server: flag(p, "sentOnServer")?.unwrap_or(was.is_none_or(|w| w.sent_on_server)),
            // What was taken down stays with the account while it reads from the same place.
            received: was
                .filter(|w| w.kind == kind && w.incoming.host == incoming.host && w.user == p_user(p, w))
                .map(|w| w.received.clone())
                .unwrap_or_default(),
            data_file: was.and_then(|w| w.data_file.clone()),
            caldav: caldav(p, was.and_then(|w| w.caldav.as_ref()))?,
            email,
            kind,
            incoming,
            outgoing,
        })
    }

    /// The account as the settings file keeps it and commands give it: no password.
    pub fn to_json(&self) -> Value {
        let server = |s: &Server| json!({ "host": s.host, "port": s.port, "security": security_name(s.security), "trusted": s.trusted.map(|f| f.to_string()) });
        json!({
            "id": self.id,
            "name": self.name,
            "email": self.email,
            "displayName": self.display_name,
            "kind": self.kind.name(),
            "user": self.user,
            "incoming": server(&self.incoming),
            "outgoing": server(&self.outgoing),
            "signInToSend": self.sign_in_to_send,
            "leaveOnServer": self.leave_on_server,
            "sentOnServer": self.sent_on_server,
            "dataFile": self.data_file.as_ref().map(|p| p.display().to_string()),
            "caldav": self.caldav.as_ref().map(DavSetup::to_json),
        })
    }

    /// The account as the settings file keeps it: with what was taken down so far.
    pub fn to_settings(&self) -> Value {
        let mut v = self.to_json();
        v["received"] = self.received.to_json();
        v
    }

    pub fn incoming_endpoint(&self) -> Endpoint {
        Endpoint {
            protocol: self.kind.protocol(),
            host: self.incoming.host.clone(),
            port: self.incoming.port,
            security: self.incoming.security,
            trusted: self.incoming.trusted,
        }
    }

    pub fn outgoing_endpoint(&self) -> Endpoint {
        Endpoint {
            protocol: Protocol::Smtp,
            host: self.outgoing.host.clone(),
            port: self.outgoing.port,
            security: self.outgoing.security,
            trusted: self.outgoing.trusted,
        }
    }
}

/// An id for a new account: the first `account-N` not taken.
pub fn new_id(accounts: &[Account]) -> String {
    (1..).map(|n: u32| format!("account-{n}")).find(|id| !accounts.iter().any(|a| &a.id == id)).unwrap_or_else(|| "account".to_string())
}

/// An id as the settings file gives it: short, plain and not yet taken.
pub fn kept_id(id: &str, accounts: &[Account]) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !accounts.iter().any(|a| a.id == id)
}

/// The accounts a settings file keeps; those that make no sense are counted, not taken.
pub fn from_settings(v: &Value) -> (Vec<Account>, u32) {
    let mut out: Vec<Account> = Vec::new();
    let mut ignored = 0;
    for a in v.as_array().map(Vec::as_slice).unwrap_or(&[]).iter().take(MAX_ACCOUNTS) {
        let id = a.get("id").and_then(Value::as_str).unwrap_or_default();
        match kept_id(id, &out).then(|| Account::from_json(a, id, None)) {
            Some(Ok(mut account)) => {
                account.received = Received::from_json(a.get("received"));
                account.data_file = a.get("dataFile").and_then(Value::as_str).and_then(data_file_of);
                let trusted = |key: &str| a.get(key).and_then(|s| s.get("trusted")).and_then(Value::as_str).and_then(Fingerprint::parse);
                account.incoming.trusted = trusted("incoming");
                account.outgoing.trusted = trusted("outgoing");
                account.caldav = DavSetup::from_settings(a.get("caldav"));
                out.push(account);
            }
            _ => ignored += 1,
        }
    }
    (out, ignored)
}

/// A data file as the settings file names it: a whole path to a `.pst`, nothing else.
fn data_file_of(s: &str) -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(s);
    let pst = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pst"));
    (s.len() <= 1024 && !s.chars().any(char::is_control) && path.is_absolute() && pst).then_some(path)
}

/// The name of an account's data file: its address, with what a file name cannot hold left out.
pub fn data_file_name(account: &Account) -> String {
    let plain: String = account.email.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '@' | '-' | '_' | '+')).take(120).collect();
    let plain = plain.trim_matches('.');
    if plain.is_empty() { format!("{}.pst", account.id) } else { format!("{plain}.pst") }
}

/// What a test of one server came to, in words fit to show; with the certificate it showed,
/// when that was not trusted.
pub(crate) fn outcome(r: Result<String, openmailclient_net::NetError>) -> Value {
    match r {
        Ok(said) => json!({ "ok": true, "said": said }),
        Err(openmailclient_net::NetError::Untrusted(seen)) => {
            json!({ "ok": false, "said": format!("the certificate of {} is not trusted: {}", seen.host, seen.reason), "certificate": shown(&seen) })
        }
        Err(e) => json!({ "ok": false, "said": e.to_string() }),
    }
}

/// A certificate a server showed, as commands give it.
pub fn shown(seen: &Seen) -> Value {
    json!({
        "host": seen.host,
        "fingerprint": seen.fingerprint.to_string(),
        "reason": seen.reason,
        "trustable": seen.trustable,
        "changed": seen.changed,
        "subject": seen.about.subject,
        "issuer": seen.about.issuer,
        "ownMaking": seen.about.own(),
        "names": seen.about.names,
        "notBefore": seen.about.not_before,
        "notAfter": seen.about.not_after,
    })
}

/// Certificates kept of those servers showed in this run that were not trusted.
pub const MAX_SHOWN: usize = 16;

/// The certificates servers showed that were not trusted, the newest last: only these may be
/// trusted, so what the owner trusts is what a server really showed. Shared with the jobs that
/// reach servers, which note them as they are refused.
#[derive(Clone, Default)]
pub struct Shown(std::sync::Arc<std::sync::Mutex<Vec<Seen>>>);

impl Shown {
    pub fn note(&self, seen: &Seen) {
        let Ok(mut all) = self.0.lock() else { return };
        all.retain(|s| !(s.host == seen.host && s.fingerprint == seen.fingerprint));
        all.push(seen.clone());
        if all.len() > MAX_SHOWN {
            let over = all.len() - MAX_SHOWN;
            all.drain(..over);
        }
    }

    /// Those shown by a server.
    pub fn of(&self, host: &str) -> Vec<Seen> {
        self.0.lock().map(|all| all.iter().filter(|s| s.host.eq_ignore_ascii_case(host)).cloned().collect()).unwrap_or_default()
    }

    pub fn forget(&self, host: &str, fingerprint: Fingerprint) {
        if let Ok(mut all) = self.0.lock() {
            all.retain(|s| !(s.host.eq_ignore_ascii_case(host) && s.fingerprint == fingerprint));
        }
    }
}

/// A connector that notes each certificate refused, then gives the refusal on.
pub(crate) struct Noting {
    pub inner: std::sync::Arc<dyn openmailclient_net::Connector>,
    pub shown: Shown,
}

impl Noting {
    fn noted<T>(&self, r: Result<T, openmailclient_net::NetError>) -> Result<T, openmailclient_net::NetError> {
        if let Err(openmailclient_net::NetError::Untrusted(seen)) = &r {
            self.shown.note(seen);
        }
        r
    }
}

impl openmailclient_net::Connector for Noting {
    fn lookup(&self, name: &str, kind: openmailclient_net::dns::Kind) -> Result<Vec<openmailclient_net::dns::Record>, openmailclient_net::NetError> {
        self.inner.lookup(name, kind)
    }

    fn connect(&self, to: &Endpoint) -> Result<Box<dyn openmailclient_net::Stream>, openmailclient_net::NetError> {
        self.noted(self.inner.connect(to))
    }

    fn secure(
        &self,
        plain: Box<dyn openmailclient_net::Stream>,
        to: &Endpoint,
    ) -> Result<Box<dyn openmailclient_net::Stream>, openmailclient_net::NetError> {
        self.noted(self.inner.secure(plain, to))
    }
}

/// Fields of an account given to `account.edit`, less those it does not take. A server's
/// trusted certificate is never taken from them: `account.trustCertificate` alone sets it.
pub(crate) fn changes(p: &Value) -> Value {
    let mut m = Map::new();
    if let Some(all) = p.as_object() {
        for (k, v) in all {
            if !matches!(k.as_str(), "id" | "password" | "received" | "dataFile") {
                m.insert(k.clone(), v.clone());
            }
        }
    }
    Value::Object(m)
}
