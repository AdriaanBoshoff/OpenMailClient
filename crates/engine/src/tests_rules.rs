//! Rules: made, ordered, turned off and kept between runs; run on mail as it comes in, and on
//! a folder by Run Rules Now, looked at first and then done.

use crate::Session;
use openmailclient_model::FolderKind;
use openmailclient_net::fake::Fake;
use serde_json::{Value, json};
use std::sync::Arc;

fn message(from: &str, subject: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\nTo: pat@example.test\r\nSubject: {subject}\r\nDate: Tue, 13 Oct 2026 09:00:00 +0000\r\nMessage-ID: <{}@example.test>\r\n\r\nHello.\r\n",
        subject.replace(' ', ".")
    )
    .into_bytes()
}

fn count_in(s: &Session, store: usize, name: &str) -> (u32, u32) {
    s.folders[store].iter().find(|f| f.name == name).map_or((0, 0), |f| (f.total, f.unread))
}

fn count(s: &Session, name: &str) -> (u32, u32) {
    count_in(s, 0, name)
}

fn settle(s: &mut Session) {
    let mut waited = 0;
    while !(s.jobs.is_empty() && s.saves.is_empty()) {
        s.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
        assert!(waited < 1000, "never settled: {}", s.status);
    }
}

fn receive(s: &mut Session) -> Value {
    let got = s.run("account.receive", &Value::Null).unwrap();
    settle(s);
    got
}

#[test]
fn rules_are_run_on_mail_that_comes_in() {
    let fake = Fake::default();
    fake.state().mailbox("INBOX").unwrap().add(&message("news@example.test", "Already there"), &[]);
    let dir = std::env::temp_dir().join(format!("omc-rules-arrival-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session::new();
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.connector = Some(Arc::new(fake.clone()));
    s.account_dir = Some(dir.join("Accounts"));
    s.journal_dir = Some(dir.join("journals"));
    s.run(
        "account.add",
        &json!({ "email": "pat@example.test", "kind": "imap", "password": "not a real password",
            "incoming": { "host": "mail.example.test", "security": "starttls" }, "outgoing": { "host": "mail.example.test", "security": "starttls" } }),
    )
    .unwrap();
    // What a first receive finds on the server is not ruled on.
    s.run("rules.save", &json!({ "rule": { "name": "News", "when": [{ "test": "from", "words": ["news@"] }], "then": [{ "do": "move", "folder": "Inbox/News" }, { "do": "markRead" }] } }))
        .unwrap();
    receive(&mut s);
    let inbox = s.find_folder(1, FolderKind::Inbox).unwrap();
    assert_eq!(count_in(&s, 1, "Inbox").0, 1);
    s.select_folder(1, inbox).unwrap();
    s.run("folder.new", &json!({ "name": "News" })).unwrap();
    s.run("rules.save", &json!({ "rule": { "name": "Junk", "when": [{ "test": "subject", "words": ["WIN"] }], "except": [{ "test": "ccMe" }, { "test": "from", "words": ["friend"] }], "then": [{ "do": "delete" }] } }))
        .unwrap();
    s.run("rules.save", &json!({ "rule": { "name": "Tag", "when": [{ "test": "toMe" }], "then": [{ "do": "category", "name": "Mine" }] } })).unwrap();

    let deleted = count_in(&s, 1, "Deleted Items").0;
    {
        let mut st = fake.state();
        let m = st.mailbox("INBOX").unwrap();
        m.add(&message("news@example.test", "Weekly news"), &[]);
        m.add(&message("someone@example.test", "WIN a prize"), &[]);
        m.add(&message("kim@example.test", "Lunch"), &[]);
    }
    let got = receive(&mut s);
    assert_eq!(got["received"], 3, "{got}");
    // Moved and read; deleted to Deleted Items, not for good; the rest stays and is tagged.
    assert_eq!(count_in(&s, 1, "News"), (1, 0));
    assert_eq!(count_in(&s, 1, "Deleted Items").0, deleted + 1);
    assert_eq!(count_in(&s, 1, "Inbox").0, 2);
    s.select_folder(1, inbox).unwrap();
    let lunch = s.rows.iter().position(|r| r.subject == "Lunch").unwrap();
    s.run("mail.select", &json!({ "index": lunch })).unwrap();
    assert_eq!(s.open.as_ref().unwrap().categories, ["Mine"]);
    // Only the one left in the Inbox, still unread, is told of as new.
    assert_eq!(s.arrived.list.len(), 1, "{:?}", s.arrived.list);

    // The next Send/Receive takes the moves to the server.
    receive(&mut s);
    {
        let st = fake.state();
        let on = |name: &str| -> Vec<usize> { st.mailboxes.iter().filter(|m| m.name.ends_with(name)).map(|m| m.messages.len()).collect() };
        assert_eq!(on("News"), [1], "{:?}", st.mailboxes.iter().map(|m| (&m.name, m.messages.len())).collect::<Vec<_>>());
    }
    let _ = s.run("file.closeStore", &json!({ "store": 1, "discard": true }));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_rules_now_looks_first_and_then_does() {
    let mut s = Session::with_sample();
    s.run("folder.select", &json!({ "folder": "Inbox" })).unwrap();
    s.run("folder.new", &json!({ "name": "Import" })).unwrap();
    s.run(
        "rules.save",
        &json!({ "rule": { "name": "Import tool", "when": [{ "test": "subject", "words": ["import tool"] }], "then": [{ "do": "move", "folder": "Inbox/Import" }, { "do": "flag" }, { "do": "importance", "level": "high" }] } }),
    )
    .unwrap();
    let before = count(&s, "Projects").0;
    let look = s.run("rules.runNow", &json!({ "preview": true, "subfolders": true })).unwrap();
    let matched = look["matched"].as_u64().unwrap();
    assert!(matched >= 1, "{look}");
    assert_eq!(look["messages"][0]["does"], "moved to Inbox/Import, flagged, high importance");
    assert_eq!(count(&s, "Projects").0, before, "a look changes nothing");

    // A rule turned off is not run, unless it is named.
    s.run("rules.turn", &json!({ "index": 0, "on": false })).unwrap();
    assert_eq!(s.run("rules.runNow", &json!({ "preview": true, "subfolders": true })).unwrap()["matched"], 0);
    let done = s.run("rules.runNow", &json!({ "rules": ["import TOOL"], "folder": "Inbox/Projects" })).unwrap();
    assert_eq!((done["matched"].as_u64(), done["moved"].as_u64()), (Some(matched), Some(matched)), "{done}");
    assert_eq!(count(&s, "Import").0, u32::try_from(matched).unwrap());
    assert_eq!(count(&s, "Projects").0, before - u32::try_from(matched).unwrap());
    s.run("folder.select", &json!({ "folder": "Inbox/Import" })).unwrap();
    s.run("mail.select", &json!({ "index": 0 })).unwrap();
    let it = s.open.clone().unwrap();
    assert_eq!((it.importance(), it.flag()), (2, openmailclient_model::Flag::Flagged));
    // Run again from the Inbox with its subfolders: nothing is moved twice.
    s.run("folder.select", &json!({ "folder": "Inbox" })).unwrap();
    let again = s.run("rules.runNow", &json!({ "rules": ["Import tool"], "subfolders": true })).unwrap();
    assert_eq!(again["moved"], 0, "{again}");
    // A rule whose folder is gone says so, and does what it still can.
    s.run("rules.save", &json!({ "index": 0, "rule": { "name": "Gone", "then": [{ "do": "move", "folder": "Nowhere" }, { "do": "markRead" }] } }))
        .unwrap();
    let gone = s.run("rules.runNow", &json!({ "folder": "Inbox/Import" })).unwrap();
    assert!(gone["trouble"][0].as_str().unwrap().contains("no folder Nowhere"), "{gone}");
    assert_eq!(count(&s, "Import").1, 0);
    assert!(s.run("rules.runNow", &json!({ "rules": ["nothing"] })).is_err());
    assert!(s.run("rules.runNow", &json!({ "folder": "Nowhere" })).is_err());
    assert!(s.find_folder(0, FolderKind::Deleted).is_some());
}

#[test]
fn rules_are_made_ordered_and_kept() {
    let mut s = Session::with_sample();
    s.run("folder.select", &json!({ "folder": "Inbox" })).unwrap();
    s.run("mail.select", &json!({ "index": 0 })).unwrap();
    // Create Rule starts from the selected message, and keeps nothing yet.
    let made = s.run("rules.create", &Value::Null).unwrap();
    let mut rule = made["rule"].clone();
    assert_eq!(rule["when"][0]["test"], "from");
    assert!(s.rules.is_empty());
    rule["then"] = json!([{ "do": "copy", "folder": "Inbox" }]);
    s.run("rules.save", &json!({ "rule": rule })).unwrap();
    s.run(
        "rules.save",
        &json!({ "rule": { "name": "Second", "when": [{ "test": "size", "min": 1 }], "then": [{ "do": "markRead" }, { "do": "stop" }] } }),
    )
    .unwrap();
    s.run("rules.move", &json!({ "index": 1, "to": 0 })).unwrap();
    let listed = s.run("rules.list", &Value::Null).unwrap();
    assert_eq!(listed["rules"][0]["name"], "Second");
    assert_eq!(
        listed["rules"][0]["words"],
        "Apply this rule after the message arrives, of 1 KB or more: mark it as read, stop processing more rules"
    );

    for bad in [
        json!({}),
        json!({ "rule": { "name": "x" } }),
        json!({ "rule": { "name": "x", "then": [{ "do": "explode" }] } }),
        json!({ "rule": { "name": "x", "account": "nobody", "then": [{ "do": "markRead" }] } }),
        json!({ "index": 9, "rule": { "name": "x", "then": [{ "do": "markRead" }] } }),
    ] {
        assert!(s.run("rules.save", &bad).is_err(), "{bad}");
    }
    assert!(s.run("rules.delete", &json!({ "index": 5 })).is_err());

    let dir = std::env::temp_dir().join(format!("omc-rules-settings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json").display().to_string();
    s.run("settings.save", &json!({ "path": path })).unwrap();
    let mut again = Session::new();
    again.run("settings.load", &json!({ "path": path })).unwrap();
    assert_eq!(again.rules, s.rules);
    s.run("rules.delete", &json!({ "index": 0 })).unwrap();
    assert_eq!(s.rules.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
