//! New mail told of, and Send/Receive on a timer, against the in-process fake server.

use crate::Session;
use openmailclient_net::fake::Fake;
use serde_json::{Value, json};
use std::sync::Arc;

fn message(subject: &str, body: &str) -> Vec<u8> {
    format!(
        "From: June Okafor <june@example.test>\r\nTo: pat@example.test\r\nSubject: {subject}\r\nDate: Tue, 13 Oct 2026 09:00:00 +0000\r\nMessage-ID: <{subject}@example.test>\r\n\r\n{body}\r\n"
    )
    .into_bytes()
}

fn add(fake: &Fake, subject: &str, flags: &[&str]) {
    fake.state().mailbox("INBOX").unwrap().add(&message(subject, "First line.\r\n\r\n   Second    line."), flags);
}

fn with(fake: &Fake, kind: &str) -> Session {
    let mut s = Session::new();
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.connector = Some(Arc::new(fake.clone()));
    let account = json!({
        "email": "pat@example.test", "kind": kind, "name": "Work",
        "incoming": { "host": "mail.example.test", "security": "starttls" },
        "password": "not a real password",
    });
    s.run("account.add", &account).unwrap();
    s
}

#[test]
fn new_unread_mail_is_told_of_and_opened_but_not_what_was_there_first() {
    for kind in ["imap", "pop3"] {
        let fake = Fake::default();
        add(&fake, "old", &[]);
        let mut s = with(&fake, kind);
        assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 1);
        assert!(s.arrived.list.is_empty(), "{kind}: mail on the server the first time is not new: {:?}", s.arrived.list);

        add(&fake, "fresh", &[]);
        add(&fake, "read already", &["\\Seen"]);
        assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 2, "{kind}");
        // POP3 knows nothing of read and unread: all that comes is new.
        let told = if kind == "imap" { 1 } else { 2 };
        assert_eq!(s.arrived.list.len(), told, "{kind}: only the unread one: {:?}", s.arrived.list);
        let a = s.arrived.list[0].clone();
        assert_eq!((a.from.as_str(), a.subject.as_str(), a.account.as_str()), ("June Okafor", "fresh", "Work"), "{kind}");
        assert_eq!(a.preview, "First line. Second line.", "{kind}");
        // What the commands give of it is marked as the message's own.
        let state = s.inspect();
        assert_eq!(state["arrived"][0]["untrusted_content"]["subject"], "fresh", "{kind}");

        // Opened: its folder is shown, it is selected, and it is no longer new.
        s.run("folder.select", &json!({ "folder": "Sent Items" })).unwrap();
        s.run("mail.openArrived", &json!({ "seq": a.seq })).unwrap();
        assert_eq!(s.open.as_ref().map(|i| i.subject()), Some("fresh".to_string()), "{kind}");
        assert_eq!(s.arrived.list.len(), told - 1);
        s.run("mail.dismissArrived", &Value::Null).unwrap();
        assert!(s.run("mail.openArrived", &json!({ "seq": a.seq })).is_err(), "{kind}: only once");

        // Dismissed one at a time or all at once.
        add(&fake, "third", &[]);
        add(&fake, "fourth", &[]);
        s.run("account.receive", &Value::Null).unwrap();
        assert_eq!(s.arrived.list.len(), 2, "{kind}");
        let first = s.arrived.list[0].seq;
        assert_eq!(s.run("mail.dismissArrived", &json!({ "seq": first })).unwrap()["left"], 1);
        assert_eq!(s.arrived.list[0].subject, "fourth");
        assert_eq!(s.run("mail.dismissArrived", &Value::Null).unwrap()["left"], 0);
        // Agents hear of each, whatever was opened or put away meanwhile (M6.5).
        let events = s.run("mail.events", &json!({})).unwrap();
        let heard: Vec<String> = events["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "newMail")
            .map(|e| e["untrusted_content"]["subject"].as_str().unwrap().to_string())
            .collect();
        assert!(heard.ends_with(&["third".to_string(), "fourth".to_string()]), "{kind}: {heard:?}");
        assert!(s.run("mail.dismissArrived", &json!({ "seq": "x" })).is_err());
    }
}

#[test]
fn a_message_moved_away_before_it_is_opened_is_said_to_be_gone() {
    let fake = Fake::default();
    let mut s = with(&fake, "pop3");
    s.run("account.receive", &Value::Null).unwrap();
    add(&fake, "moved", &[]);
    s.run("account.receive", &Value::Null).unwrap();
    let seq = s.arrived.list[0].seq;
    s.run("folder.select", &json!({ "folder": "Inbox" })).unwrap();
    let at = s.rows.iter().position(|r| r.subject == "moved").unwrap();
    s.run("mail.select", &json!({ "index": at })).unwrap();
    s.run("mail.delete", &Value::Null).unwrap();
    assert!(s.run("mail.openArrived", &json!({ "seq": seq })).is_err());
    assert!(s.arrived.list.is_empty(), "not told of again");
}

#[test]
fn the_alerts_are_chosen_and_kept() {
    let mut s = Session::new();
    assert_eq!(s.run("mail.newMailAlerts", &json!({ "sound": false })).unwrap(), json!({ "alert": true, "sound": false, "flash": true }));
    assert!(s.run("mail.newMailAlerts", &json!({ "alert": "yes" })).is_err());
    assert!(s.run("mail.newMailAlerts", &json!([1])).is_err());
    assert!(s.alerts.alert && !s.alerts.sound, "a refused change changes nothing");
    assert_eq!(s.run("mail.sendReceiveEvery", &json!({ "minutes": 10 })).unwrap()["minutes"], 10);
    assert!(s.run("mail.sendReceiveEvery", &json!({ "minutes": 1441 })).is_err());
    assert_eq!(s.run("mail.sendReceiveEvery", &Value::Null).unwrap()["minutes"], 0);
    assert_eq!(s.run("mail.sendReceiveEvery", &Value::Null).unwrap()["minutes"], 30);
    s.run("mail.sendReceiveEvery", &json!({ "minutes": 10 })).unwrap();

    let kept = crate::cmd::settings::current(&s, None);
    let mut again = Session::new();
    let path = std::env::temp_dir().join(format!("omc-arrival-{}.json", std::process::id()));
    std::fs::write(&path, kept.to_string()).unwrap();
    again.run("settings.load", &json!({ "path": path })).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(again.send_receive_min, 10);
    assert_eq!(again.alerts, s.alerts);
}

#[test]
fn send_receive_comes_on_its_own_counted_from_the_end_of_the_last() {
    let fake = Fake::default();
    let mut s = with(&fake, "imap");
    s.run("mail.sendReceiveEvery", &json!({ "minutes": 1 })).unwrap();
    let receives = |fake: &Fake| fake.state().heard.iter().filter(|l| l.contains("SELECT")).count();

    // The first comes a few seconds after the host first asks.
    let wait = s.send_receive_due(1_000).unwrap();
    assert!((1..=5_000).contains(&wait), "{wait}");
    assert_eq!(receives(&fake), 0);
    s.send_receive_due(1_000 + wait).unwrap();
    s.finish_saves();
    assert!(receives(&fake) > 0, "{:?}", fake.state().heard);
    let after_first = receives(&fake);

    // Nothing more until a minute has gone by since it ended.
    let at = 1_000 + wait;
    assert!(s.send_receive_due(at + 30_000).unwrap() > 0);
    assert_eq!(receives(&fake), after_first);
    // One asked for by the owner puts the next off: counted from its end.
    s.run("mail.sendReceive", &json!({ "wait": false })).unwrap();
    assert_eq!(s.send_receive_due(at + 50_000), Some(1_000), "while it runs");
    s.finish_saves();
    let owners = receives(&fake);
    assert_eq!(s.send_receive_due(at + 60_500).unwrap(), 49_500);
    assert_eq!(receives(&fake), owners);
    s.send_receive_due(at + 110_000);
    s.finish_saves();
    assert!(receives(&fake) > owners);

    // Off, or no account: nothing is waited for.
    s.run("mail.sendReceiveEvery", &json!({ "minutes": 0 })).unwrap();
    assert_eq!(s.send_receive_due(at + 500_000), None);
    let mut none = Session::new();
    assert_eq!(none.send_receive_due(0), None);
}

#[test]
fn show_progress_lists_what_runs_stops_one_and_keeps_what_came_of_it() {
    let fake = Fake::default();
    let mut s = with(&fake, "imap");
    let shown = s.run("mail.showProgress", &Value::Null).unwrap();
    assert_eq!(shown["running"], json!([]));
    assert!(s.run("mail.cancelTask", &Value::Null).is_err(), "nothing to cancel");

    s.run("mail.sendReceive", &json!({ "wait": false })).unwrap();
    let shown = s.run("mail.showProgress", &Value::Null).unwrap();
    let running = shown["running"].as_array().unwrap();
    assert_eq!(running.len(), 1, "{shown}");
    let id = running[0]["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("job-") && running[0]["what"].as_str().unwrap().contains("Receiving"), "{shown}");
    assert!(s.run("mail.cancelTask", &json!({ "id": "job-999" })).is_err());
    assert!(s.run("mail.cancelTask", &json!({ "id": 3 })).is_err());
    assert_eq!(s.run("mail.cancelTask", &json!({ "id": id })).unwrap()["stopping"], 1);
    assert!(s.running()[0].stopping);
    s.finish_saves();

    let shown = s.run("mail.showProgress", &Value::Null).unwrap();
    assert_eq!(shown["running"], json!([]));
    let last = shown["finished"].as_array().unwrap().last().unwrap().clone();
    assert!(last["job"].as_str().unwrap().contains("Receiving") && last["said"].is_string(), "{last}");
    assert_eq!(last["stopped"], true);

    // When Send/Receive comes next, once the host has asked.
    assert_eq!(s.run("mail.showProgress", &Value::Null).unwrap()["nextSendReceiveInSeconds"], Value::Null);
    s.send_receive_due(10_000);
    assert_eq!(s.run("mail.showProgress", &Value::Null).unwrap()["nextSendReceiveInSeconds"], 5);
}
