//! Accounts: kept, changed, tested against the in-process fake server, and kept between runs
//! without their passwords. No socket is opened and no real account is touched.

use crate::Session;
use openmailclient_net::fake::Fake;
use serde_json::{Value, json};
use std::sync::Arc;

const PASSWORD: &str = "not a real password";

fn with_fake(fake: Fake) -> Session {
    let mut s = Session::new();
    s.connector = Some(Arc::new(fake));
    s
}

fn imap_account() -> Value {
    json!({
        "email": "pat@example.test", "displayName": "Pat Lee", "kind": "imap",
        "incoming": { "host": "mail.example.test", "security": "starttls" },
        "outgoing": { "host": "mail.example.test", "security": "starttls" },
        "password": PASSWORD,
    })
}

/// Nothing a session says, keeps or shows holds the password.
fn never_shows_the_password(s: &mut Session) {
    let listed = s.run("account.list", &Value::Null).unwrap();
    for text in [listed.to_string(), s.inspect().to_string(), s.settings(None).to_string(), s.status.clone()] {
        assert!(!text.contains(PASSWORD), "{text}");
    }
}

#[test]
fn accounts_are_added_listed_changed_and_removed() {
    let mut s = Session::new();
    let added = s.run("account.add", &imap_account()).unwrap();
    assert_eq!(added["id"], "account-1");
    assert_eq!(added["passwordKept"], true);
    assert!(!added.to_string().contains(PASSWORD));
    // What was not given follows from what was.
    let a = &added["account"];
    assert_eq!((a["name"].as_str(), a["user"].as_str()), (Some("pat@example.test"), Some("pat@example.test")));
    assert_eq!((a["incoming"]["port"].as_u64(), a["outgoing"]["port"].as_u64()), (Some(143), Some(587)));
    never_shows_the_password(&mut s);
    let listed = s.run("account.list", &Value::Null).unwrap();
    assert_eq!(listed["accounts"][0]["hasPassword"], true);

    // A second account for the same address is refused; another address is not.
    assert!(s.run("account.add", &imap_account()).is_err());
    let pop = json!({ "email": "sam@example.test", "kind": "pop3", "incoming": { "host": "pop.example.test" } });
    let added = s.run("account.add", &pop).unwrap();
    assert_eq!(added["id"], "account-2");
    assert_eq!(added["passwordKept"], false);
    assert_eq!(added["account"]["incoming"]["port"], 995);
    assert_eq!(added["account"]["outgoing"]["host"], "pop.example.test");

    // A change keeps what it does not name; a new security brings its own port.
    let changed = s.run("account.edit", &json!({ "id": "account-2", "name": "Home", "incoming": { "security": "starttls" } })).unwrap();
    assert_eq!(changed["account"]["name"], "Home");
    assert_eq!(changed["account"]["incoming"]["host"], "pop.example.test");
    assert_eq!(changed["account"]["incoming"]["port"], 110);
    assert!(s.run("account.edit", &json!({ "id": "account-2", "email": "pat@example.test" })).is_err());

    assert_eq!(s.run("account.setPassword", &json!({ "id": "account-2", "password": "x" })).unwrap()["passwordKept"], true);
    assert_eq!(s.run("account.setPassword", &json!({ "id": "account-2", "password": "" })).unwrap()["passwordKept"], false);
    assert!(s.vault.get("account-2").unwrap().is_none());

    s.run("account.remove", &json!({ "id": "account-1" })).unwrap();
    assert!(s.vault.get("account-1").unwrap().is_none(), "a removed account's password goes with it");
    assert_eq!(s.accounts.len(), 1);
    // A new account does not take the id of one removed while another is kept.
    assert_eq!(s.run("account.add", &imap_account()).unwrap()["id"], "account-1");
}

#[test]
fn accounts_that_make_no_sense_are_refused() {
    let mut s = Session::new();
    let base = imap_account();
    let with = |key: &str, v: Value| {
        let mut p = base.clone();
        p[key] = v;
        p
    };
    for bad in [
        with("email", json!("not an address")),
        with("email", json!("a@b@c")),
        with("email", json!("pat@example.test\r\nRCPT TO:<x@y>")),
        with("kind", json!("exchange")),
        with("incoming", json!({ "host": "" })),
        with("incoming", json!({ "host": "mail example" })),
        with("incoming", json!({ "host": "mail.example.test", "port": 0 })),
        with("incoming", json!({ "host": "mail.example.test", "port": 70000 })),
        with("incoming", json!({ "host": "mail.example.test", "security": "maybe" })),
        with("incoming", json!("mail.example.test")),
        with("name", json!("n".repeat(101))),
        with("password", json!("two\nlines")),
        with("leaveOnServer", json!("yes")),
        json!([]),
        Value::Null,
    ] {
        assert!(s.run("account.add", &bad).is_err(), "{bad}");
    }
    assert!(s.accounts.is_empty());
    assert!(s.run("account.edit", &json!({ "id": "account-9" })).is_err());
}

#[test]
fn a_test_signs_in_to_both_servers_and_sends_nothing() {
    for kind in ["imap", "pop3"] {
        let fake = Fake::default();
        let mut s = with_fake(fake.clone());
        let mut account = imap_account();
        account["kind"] = json!(kind);
        let id = s.run("account.add", &account).unwrap()["id"].as_str().unwrap().to_string();
        let tested = s.run("account.test", &json!({ "id": id })).unwrap();
        assert_eq!(tested["incoming"]["ok"], true, "{kind}: {tested}");
        assert_eq!(tested["outgoing"]["ok"], true, "{kind}: {tested}");
        assert!(s.status.contains("nothing was sent"), "{}", s.status);
        assert!(fake.state().submitted.is_empty());
        never_shows_the_password(&mut s);
    }
}

#[test]
fn a_test_that_fails_says_which_server_and_why() {
    let mut s = with_fake(Fake::default());
    let mut account = imap_account();
    account["password"] = json!("wrong");
    s.run("account.add", &account).unwrap();
    let tested = s.run("account.test", &json!({ "id": "account-1" })).unwrap();
    assert_eq!((tested["incoming"]["ok"].as_bool(), tested["outgoing"]["ok"].as_bool()), (Some(false), Some(false)));
    assert!(tested["incoming"]["said"].as_str().unwrap().contains("not accepted"), "{tested}");
    assert!(!tested.to_string().contains("wrong"), "{tested}");
    // No password at all is said as that, before any server is asked.
    s.run("account.setPassword", &json!({ "id": "account-1", "password": null })).unwrap();
    let tested = s.run("account.test", &json!({ "id": "account-1" })).unwrap();
    assert!(tested["incoming"]["said"].as_str().unwrap().contains("no password"), "{tested}");
    // Without a way to reach servers, nothing is tried.
    let mut alone = Session::new();
    alone.run("account.add", &imap_account()).unwrap();
    assert!(alone.run("account.test", &json!({ "id": "account-1" })).is_err());
}

#[test]
fn a_test_runs_on_a_thread_of_its_own_and_ends_in_poll() {
    let mut s = with_fake(Fake::default());
    s.run("account.add", &imap_account()).unwrap();
    let started = s.run("account.test", &json!({ "id": "account-1", "wait": false })).unwrap();
    assert_eq!(started["started"], true);
    assert!(s.working().is_some());
    let mut waited = 0;
    while !s.poll() {
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
        assert!(waited < 1000, "the test never ended");
    }
    assert!(s.jobs.is_empty());
    let last = s.finished.last().unwrap();
    assert_eq!(last["result"]["incoming"]["ok"], true, "{last}");
    let (events, _) = s.events.since(0);
    assert_eq!(events.last().map(|e| e["kind"].clone()), Some(json!("jobEnded")));
    assert!(s.status.contains("nothing was sent"), "{}", s.status);
}

#[test]
fn accounts_are_kept_between_runs_without_their_passwords() {
    let dir = std::env::temp_dir().join(format!("omc-accounts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    let mut s = Session::new();
    s.run("account.add", &imap_account()).unwrap();
    s.run(
        "account.add",
        &json!({ "email": "sam@example.test", "kind": "pop3", "incoming": { "host": "pop.example.test", "port": 1995 }, "leaveOnServer": false }),
    )
    .unwrap();
    s.run("settings.save", &json!({ "path": path.display().to_string() })).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains(PASSWORD));

    let mut again = Session::new();
    again.run("settings.load", &json!({ "path": path.display().to_string() })).unwrap();
    assert_eq!(again.accounts, s.accounts);

    // A file written by someone else: what makes no sense is left out, the rest is taken.
    let mut all: Value = serde_json::from_str(&text).unwrap();
    let good = all["accounts"][0].clone();
    all["accounts"] = json!([
        good, 7, null, {}, { "id": "account-1", "email": "dup@example.test", "incoming": { "host": "x.test" } },
        { "id": "../../etc", "email": "k@example.test", "incoming": { "host": "x.test" } },
        { "id": "account-5", "email": "k@example.test", "incoming": { "host": "x.test", "port": -1 } },
    ]);
    std::fs::write(&path, all.to_string()).unwrap();
    let mut third = Session::new();
    let loaded = third.run("settings.load", &json!({ "path": path.display().to_string() })).unwrap();
    assert_eq!(third.accounts.len(), 1);
    assert!(loaded["ignored"].as_u64().unwrap() >= 6, "{loaded}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A server with a certificate of its own: refused and shown, trusted on the owner's word for
/// that server only, kept between runs, and dropped when the account points elsewhere.
#[test]
fn a_servers_own_certificate_is_trusted_on_the_owners_word() {
    let own = openmailclient_net::Fingerprint([0x5A; 32]);
    let fake = Fake::default();
    fake.state().own_certificate = Some(own);
    let mut s = with_fake(fake.clone());
    s.run("account.add", &imap_account()).unwrap();
    let id = json!({ "id": "account-1" });
    // Nothing that was not shown can be trusted, and nothing is trusted by an edit.
    let made_up = json!({ "id": "account-1", "fingerprint": own.to_string() });
    assert!(s.run("account.trustCertificate", &made_up).is_err());
    let mut edit = json!({ "id": "account-1", "incoming": { "trusted": own.to_string() } });
    s.run("account.edit", &edit).unwrap();
    assert_eq!(s.accounts[0].incoming.trusted, None);
    let tested = s.run("account.test", &id).unwrap();
    assert_eq!(tested["incoming"]["ok"], false, "{tested}");
    assert_eq!(tested["incoming"]["certificate"]["fingerprint"], own.to_string(), "{tested}");
    assert_eq!(tested["incoming"]["certificate"]["ownMaking"], true, "{tested}");
    assert!(fake.state().logins == 0);
    let listed = s.run("account.list", &Value::Null).unwrap();
    assert_eq!(listed["accounts"][0]["incoming"]["certificates"][0]["fingerprint"], own.to_string(), "{listed}");
    assert_eq!(listed["accounts"][0]["incoming"]["trusted"], Value::Null);
    // Another fingerprint than the one shown is not trusted; the one shown is, for both servers
    // on that host.
    let other = json!({ "id": "account-1", "fingerprint": "00".repeat(32) });
    assert!(s.run("account.trustCertificate", &other).is_err());
    assert!(s.run("account.trustCertificate", &json!({ "id": "account-1", "fingerprint": "junk" })).is_err());
    let trusted = s.run("account.trustCertificate", &made_up).unwrap();
    assert_eq!(trusted["trusted"], json!(["incoming", "outgoing"]), "{trusted}");
    assert!(s.status.contains("for pat@example.test only"), "{}", s.status);
    let tested = s.run("account.test", &id).unwrap();
    assert_eq!((tested["incoming"]["ok"].as_bool(), tested["outgoing"]["ok"].as_bool()), (Some(true), Some(true)), "{tested}");
    let listed = s.run("account.list", &Value::Null).unwrap();
    assert_eq!(listed["accounts"][0]["incoming"]["trusted"], own.to_string(), "{listed}");
    assert_eq!(listed["accounts"][0]["incoming"]["certificates"], json!([]), "{listed}");
    // Kept between runs.
    let (again, _) = crate::accounts::from_settings(&s.settings(None)["accounts"]);
    assert_eq!(again[0].incoming.trusted, Some(own));
    assert_eq!(again[0].outgoing.trusted, Some(own));
    // A server that now shows another certificate is refused again, and said to have changed.
    fake.state().own_certificate = Some(openmailclient_net::Fingerprint([0x77; 32]));
    let tested = s.run("account.test", &id).unwrap();
    assert_eq!(tested["incoming"]["certificate"]["changed"], true, "{tested}");
    fake.state().own_certificate = Some(own);
    // Forgotten: checked against what this system trusts again.
    s.run("account.trustCertificate", &json!({ "id": "account-1", "fingerprint": null, "server": "outgoing" })).unwrap();
    assert_eq!((s.accounts[0].incoming.trusted, s.accounts[0].outgoing.trusted), (Some(own), None));
    // Another server for the account: what was trusted for the old one does not follow it.
    edit["incoming"] = json!({ "host": "imap.example.test" });
    s.run("account.edit", &edit).unwrap();
    assert_eq!(s.accounts[0].incoming.trusted, None);
    for bad in [json!({ "id": "account-1", "fingerprint": null, "server": "sideways" }), json!({ "id": "account-1" })] {
        assert!(s.run("account.trustCertificate", &bad).is_err(), "{bad}");
    }
}

#[test]
fn an_accounts_settings_are_found_from_its_address_and_work_as_found() {
    use openmailclient_net::fake::State;
    let settings = r#"<clientConfig version="1.1"><emailProvider id="example.test">
        <incomingServer type="imap"><hostname>mail.%EMAILDOMAIN%</hostname><port>143</port><socketType>STARTTLS</socketType><username>%EMAILADDRESS%</username></incomingServer>
        <outgoingServer type="smtp"><hostname>mail.example.test</hostname><port>587</port><socketType>STARTTLS</socketType></outgoingServer>
    </emailProvider></clientConfig>"#;
    let page = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{settings}", settings.len()).into_bytes();
    let state = State { pages: vec![("autoconfig.example.test/mail/config-v1.1.xml".into(), page)], ..State::default() };
    let mut s = with_fake(Fake::new(state));
    let found = s.run("account.find", &json!({ "email": "pat@example.test" })).unwrap();
    assert_eq!(found["from"], "published", "{found}");
    assert!(s.status.contains("as its domain publishes them"), "{}", s.status);
    assert!(s.accounts.is_empty(), "finding keeps nothing");
    // What was found is what account.add takes, and it works.
    let mut add = found["found"].clone();
    add["password"] = json!(PASSWORD);
    s.run("account.add", &add).unwrap();
    let tested = s.run("account.test", &json!({ "id": "account-1" })).unwrap();
    assert_eq!(tested["incoming"]["ok"], true, "{tested}");
    assert_eq!(tested["outgoing"]["ok"], true, "{tested}");
    never_shows_the_password(&mut s);

    // A large provider's are known without asking anyone; on a thread of its own as well.
    let started = s.run("account.find", &json!({ "email": "sam@gmail.com", "wait": false })).unwrap();
    assert_eq!(started["started"], true);
    assert!(s.end_jobs(true));
    assert!(s.status.contains("imap.gmail.com"), "{}", s.status);

    // Nothing found is said, and is no error of the command's.
    let mut none = with_fake(Fake::new(State { answers_only: Some(Vec::new()), ..State::default() }));
    let found = none.run("account.find", &json!({ "email": "sam@bluebird.example" })).unwrap();
    assert!(found["found"].is_null());
    assert!(none.status.contains("were not found"), "{}", none.status);
    assert!(none.run("account.find", &json!({ "email": "not an address" })).is_err());
    assert!(Session::new().run("account.find", &json!({ "email": "sam@bluebird.example" })).is_err(), "a build that cannot reach servers says so");

    // A domain whose mail goes to Google is given Google's servers, through the connector's
    // lookup as the window's Noting connector passes it on.
    let mx = openmailclient_net::dns::Record::Mx { preference: 1, exchange: "aspmx.l.google.com".into() };
    let mut by_mx =
        with_fake(Fake::new(State { records: vec![("bluebird.example".into(), mx)], answers_only: Some(Vec::new()), ..State::default() }));
    let found = by_mx.run("account.find", &json!({ "email": "sam@bluebird.example" })).unwrap();
    assert_eq!(found["from"], "exchanger", "{found}");
    assert_eq!(found["found"]["incoming"]["host"], "imap.gmail.com", "{found}");
    assert!(by_mx.status.contains("for the provider that takes its mail"), "{}", by_mx.status);
}
