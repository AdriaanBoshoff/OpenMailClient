//! Sending: the Outbox goes out through the in-process fake server, what went is kept in Sent
//! Items, and what did not stays in the Outbox. No socket is opened and nothing really goes.

use crate::Session;
use openmailclient_model::{RECIP_BCC, pid};
use openmailclient_net::fake::Fake;
use serde_json::{Value, json};
use std::sync::Arc;

fn session(fake: &Fake) -> Session {
    let mut s = Session::new();
    s.now = openmailclient_model::time::from_civil(2026, 10, 7, 14, 30, 0, 0);
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.connector = Some(Arc::new(fake.clone()));
    s.run(
        "account.add",
        &json!({
            "email": "pat@example.test", "displayName": "Pat Lee",
            "incoming": { "host": "mail.example.test", "security": "starttls" },
            "outgoing": { "host": "mail.example.test", "security": "starttls" },
            "password": "not a real password",
        }),
    )
    .unwrap();
    s
}

fn count(s: &Session, name: &str) -> u32 {
    s.folders[0].iter().find(|f| f.name == name).map_or(0, |f| f.total)
}

fn write(s: &mut Session, fields: &[(&str, &str)]) {
    s.run("mail.new", &Value::Null).unwrap();
    for (field, value) in fields {
        s.run("compose.setField", &json!({ "field": field, "value": value })).unwrap();
    }
}

#[test]
fn send_goes_through_the_account_and_keeps_what_went_in_sent_items() {
    let fake = Fake::default();
    let mut s = session(&fake);
    let sent_before = count(&s, "Sent Items");
    write(
        &mut s,
        &[
            ("to", "June Okafor <june@example.test>"),
            ("cc", "tomas@example.test"),
            ("bcc", "quiet@example.test"),
            ("subject", "Rota"),
            ("body", "Thanks.\n.\n.hidden dot\nPat"),
        ],
    );
    let done = s.run("compose.send", &json!({ "anyway": true })).unwrap();
    assert_eq!(done["sent"], 1, "{done}");
    assert!(s.status.starts_with("Sent 1 message"), "{}", s.status);
    assert_eq!((count(&s, "Outbox"), count(&s, "Sent Items")), (0, sent_before + 1));

    let state = fake.state();
    let went = state.submitted.first().unwrap();
    assert_eq!(went.from, "pat@example.test");
    assert_eq!(went.to, ["june@example.test", "tomas@example.test", "quiet@example.test"]);
    let text = String::from_utf8_lossy(&went.data);
    let head = text.split("\r\n\r\n").next().unwrap();
    assert!(head.contains("From: \"Pat Lee\" <pat@example.test>"), "{head}");
    assert!(!text.contains("quiet@example.test"), "a hidden recipient is not named in what goes: {text}");
    assert!(!head.contains("X-Unsent"), "{head}");
    assert!(head.contains("Message-ID: <") && head.contains("@example.test>"), "{head}");
    // The lines that were only a dot, or began with one, came through whole.
    assert!(text.contains("\r\n.\r\n") && text.contains(".hidden dot"), "{text}");
    drop(state);

    // Sent Items keeps it as it went, the hidden recipient with it.
    s.run("folder.select", &json!({ "folder": "Sent Items" })).unwrap();
    let rows = s.rows.clone();
    let at = rows.iter().position(|r| r.subject == "Rota").unwrap();
    s.run("mail.select", &json!({ "index": at })).unwrap();
    let it = s.open.clone().unwrap();
    assert!(!it.is_unsent());
    assert!(it.recipients.iter().any(|r| r.i32(pid::RECIPIENT_TYPE) == Some(RECIP_BCC)));
    assert!(it.props.str(pid::INTERNET_MESSAGE_ID).is_some_and(|v| v.starts_with('<')));
    assert!(it.props.time(pid::MESSAGE_DELIVERY_TIME).is_some_and(|t| t >= s.now));
}

#[test]
fn an_answer_goes_from_the_account_it_reached_and_names_what_it_answers() {
    let fake = Fake::default();
    let mut s = session(&fake);
    s.run("account.add", &json!({ "email": "sam@example.test", "incoming": { "host": "mail.example.test" } })).unwrap();
    // A message that came to the second account.
    write(&mut s, &[("to", "sam@example.test"), ("subject", "Question")]);
    s.run("compose.setField", &json!({ "field": "from", "value": "sam@example.test" })).unwrap();
    assert!(s.run("compose.setField", &json!({ "field": "from", "value": "nobody@example.test" })).is_err());
    s.connector = None;
    s.run("compose.send", &json!({ "anyway": true })).unwrap();
    assert!(s.status.contains("cannot reach"), "{}", s.status);
    s.run("folder.select", &json!({ "folder": "Outbox" })).unwrap();
    s.run("mail.select", &json!({ "index": 0 })).unwrap();
    let mut it = s.open.clone().unwrap();
    assert_eq!(it.sender_email(), "sam@example.test");
    it.props.set(pid::INTERNET_MESSAGE_ID, openmailclient_model::PropValue::String("<q1@example.test>".into()));
    it.props.set(pid::INTERNET_REFERENCES, openmailclient_model::PropValue::String("<q0@example.test> not-an-id".into()));
    s.open = Some(it);
    s.run("mail.reply", &Value::Null).unwrap();
    let d = s.compose.clone().unwrap();
    assert_eq!(d.account, "account-2");
    assert_eq!((d.in_reply_to.as_str(), d.references.as_str()), ("<q1@example.test>", "<q0@example.test> <q1@example.test>"));
}

#[test]
fn what_the_server_does_not_take_stays_in_the_outbox() {
    let fake = Fake::default();
    let mut s = session(&fake);
    // Every recipient refused: the message stays, and says why.
    write(&mut s, &[("to", "gone@refused.test"), ("subject", "Lost?"), ("body", "x")]);
    let done = s.run("compose.send", &json!({ "anyway": true })).unwrap();
    assert_eq!(done["sent"], 0, "{done}");
    assert!(s.status.contains("Nothing was sent") && s.status.contains("no such user"), "{}", s.status);
    assert_eq!(count(&s, "Outbox"), 1);
    // One of two refused: it goes to the other, and the one refused is said.
    write(&mut s, &[("to", "gone@refused.test; june@example.test"), ("subject", "Half"), ("body", "x")]);
    let done = s.run("compose.send", &json!({ "anyway": true })).unwrap();
    assert_eq!(done["sent"], 1, "{done}");
    assert_eq!(done["refused"][0]["to"], "gone@refused.test");
    assert!(s.status.contains("not taken"), "{}", s.status);
    assert_eq!(count(&s, "Outbox"), 1, "the first stays; the second went");

    // A wrong password: nothing goes, everything stays.
    s.run("account.setPassword", &json!({ "id": "account-1", "password": "wrong" })).unwrap();
    let done = s.run("mail.sendAll", &Value::Null).unwrap();
    assert_eq!(done["sent"], 0);
    assert!(s.status.contains("not accepted"), "{}", s.status);
    assert!(!s.status.contains("wrong") && !done.to_string().contains("wrong"));
    assert_eq!(count(&s, "Outbox"), 1);
    assert_eq!(fake.state().submitted.len(), 1);
}

#[test]
fn a_send_runs_on_a_thread_of_its_own_and_one_asked_for_meanwhile_follows() {
    let fake = Fake::default();
    let mut s = session(&fake);
    write(&mut s, &[("to", "june@example.test"), ("subject", "One"), ("body", "x")]);
    let started = s.run("compose.send", &json!({ "anyway": true, "wait": false })).unwrap();
    assert_eq!(started["started"], true, "{started}");
    // A data file is not closed from under a send.
    assert!(s.run("file.closeStore", &json!({ "store": 0, "discard": true })).is_err());
    write(&mut s, &[("to", "june@example.test"), ("subject", "Two"), ("body", "x")]);
    let queued = s.run("compose.send", &json!({ "anyway": true, "wait": false })).unwrap();
    assert_eq!(queued["queued"], true, "{queued}");
    let mut waited = 0;
    while !(s.jobs.is_empty() && count(&s, "Outbox") == 0) {
        s.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
        assert!(waited < 1000, "the send never ended: {}", s.status);
    }
    let subjects: Vec<String> = fake
        .state()
        .submitted
        .iter()
        .map(|m| String::from_utf8_lossy(&m.data).lines().find(|l| l.starts_with("Subject:")).unwrap_or("").to_string())
        .collect();
    assert_eq!(subjects, ["Subject: One", "Subject: Two"], "each went once");
}

#[test]
fn without_an_account_a_message_waits_in_the_outbox() {
    let mut s = Session::new();
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    write(&mut s, &[("to", "june@example.test"), ("subject", "Later"), ("body", "x")]);
    s.run("compose.send", &json!({ "anyway": true })).unwrap();
    assert!(s.status.contains("account"), "{}", s.status);
    assert_eq!(count(&s, "Outbox"), 1);
    assert!(!s.is_enabled("mail.sendAll"));
    assert!(s.run("mail.sendAll", &Value::Null).is_err());
}

#[test]
fn a_delayed_message_waits_in_the_outbox_and_goes_at_its_minute_and_an_expired_one_stays() {
    const MINUTE: i64 = 600_000_000;
    let fake = Fake::default();
    let mut s = session(&fake);
    // The session's clock is set ahead of the machine's, so the test moves time itself.
    s.now = crate::cmd::clock(&s).saturating_add(24 * 60 * MINUTE);
    s.run("mail.sendReceiveEvery", &json!({ "minutes": 0 })).unwrap();
    let sent_before = count(&s, "Sent Items");
    write(&mut s, &[("to", "june@example.test"), ("subject", "Monday plan"), ("body", "See you then.")]);

    // Times that are not times, or that make no sense together, are said so and change nothing.
    for bad in [json!({ "at": "2026-02-30 09:00" }), json!({ "at": "tomorrow" }), json!({ "at": "2026-10-17 25:00" }), json!({}), json!({ "at": 5 })]
    {
        assert!(s.run("compose.delayDelivery", &bad).is_err(), "{bad}");
    }
    assert!(s.run("compose.delayDelivery", &json!({ "at": "2001-01-01 09:00" })).unwrap_err().to_string().contains("gone by"));
    let until = s.run("compose.delayDelivery", &json!({ "inMinutes": 60 })).unwrap();
    assert!(until["until"].as_str().unwrap().contains(':'), "{until}");
    assert!(s.run("compose.expires", &json!({ "inMinutes": 30 })).unwrap_err().to_string().contains("expire before it went"));
    s.run("compose.expires", &json!({ "inMinutes": 600 })).unwrap();
    assert!(s.run("compose.delayDelivery", &json!({ "inMinutes": 700 })).is_err(), "not after it expires");

    // Send leaves it in the Outbox, waiting.
    let done = s.run("compose.send", &json!({ "anyway": true })).unwrap();
    assert_eq!((done["sent"].as_u64(), done["waiting"].as_array().map(Vec::len)), (Some(0), Some(1)), "{done}");
    assert!(s.status.starts_with("It waits in the Outbox until"), "{}", s.status);
    assert_eq!(count(&s, "Outbox"), 1);
    assert!(fake.state().submitted.is_empty());

    // The host is told to ask again within a minute, and nothing goes before its time.
    let wait = s.send_receive_due(1_000).unwrap();
    assert!((50..=60_000).contains(&wait), "{wait}");
    s.now = s.now.saturating_add(59 * MINUTE);
    s.send_receive_due(61_000);
    s.finish_saves();
    assert!(fake.state().submitted.is_empty());
    assert!(s.send_receive_due(62_000).unwrap() <= 60_000);

    // At its minute it goes on its own, with the Send/Receive timer off.
    s.now = s.now.saturating_add(2 * MINUTE);
    s.send_receive_due(63_000);
    s.finish_saves();
    let state = fake.state();
    let went = state.submitted.first().expect("it went at its time");
    let text = String::from_utf8_lossy(&went.data).to_string();
    let head = text.split("\r\n\r\n").next().unwrap().to_string();
    drop(state);
    assert!(head.contains("\r\nExpires: "), "recipients are told when it expires: {head}");
    let back = openmailclient_mime::parse(went_data(&fake).as_slice()).unwrap();
    assert!(back.expires() > 0 && back.deliver_after() == 0, "when it was held is not told to anyone");
    assert_eq!((count(&s, "Outbox"), count(&s, "Sent Items")), (0, sent_before + 1));
    // Nothing is held now: the host has nothing more to wait for.
    assert_eq!(s.send_receive_due(200_000), None);

    // One that expired while it waited is not sent, and stays to be seen to.
    write(&mut s, &[("to", "june@example.test"), ("subject", "Lunch today")]);
    s.run("compose.expires", &json!({ "inMinutes": 10 })).unwrap();
    s.now = s.now.saturating_add(20 * MINUTE);
    let done = s.run("compose.send", &json!({ "anyway": true })).unwrap();
    assert_eq!(done["sent"], 0, "{done}");
    assert!(done["failed"][0]["said"].as_str().unwrap().contains("expired"), "{done}");
    assert_eq!(count(&s, "Outbox"), 1);
    assert_eq!(fake.state().submitted.len(), 1);
}

fn went_data(fake: &Fake) -> Vec<u8> {
    fake.state().submitted.first().map(|m| m.data.clone()).unwrap_or_default()
}

#[test]
fn delivery_times_are_read_as_given_on_this_machines_clock() {
    use crate::cmd::compose::parse_local;
    use openmailclient_model::time;
    assert_eq!(parse_local("2026-10-17 09:00", 0), Some(time::from_civil(2026, 10, 17, 9, 0, 0, 0)));
    assert_eq!(parse_local(" 2026-10-17T09:00:30 ", 120), Some(time::from_civil(2026, 10, 17, 9, 0, 30, 120)));
    assert_eq!(parse_local("2028-02-29 00:00", 0), Some(time::from_civil(2028, 2, 29, 0, 0, 0, 0)));
    for bad in [
        "",
        "2026-10-17",
        "09:00",
        "2026-13-01 09:00",
        "2027-02-29 09:00",
        "2026-04-31 09:00",
        "2026-10-17 9:60",
        "1600-01-01 00:00",
        "2026-10-17 09:00:00:00",
        "-1-1-1 1:1",
        "2026-10-17 ９:00",
        "99999999999999999999-01-01 00:00",
    ] {
        assert_eq!(parse_local(bad, 0), None, "{bad:?}");
    }
}

#[test]
fn rules_on_mail_as_it_is_sent_keep_a_copy_mark_it_and_hold_it_back() {
    let fake = Fake::default();
    let mut s = session(&fake);
    s.run("folder.select", &json!({ "folder": "Inbox" })).unwrap();
    s.run("folder.new", &json!({ "name": "Kept" })).unwrap();
    // What went stays in Sent Items; only what is sent is held back.
    for bad in [
        json!({ "name": "x", "sent": true, "then": [{ "do": "move", "folder": "Inbox/Kept" }] }),
        json!({ "name": "x", "sent": true, "then": [{ "do": "reply", "words": "Hi" }] }),
        json!({ "name": "x", "then": [{ "do": "defer", "minutes": 5 }] }),
        json!({ "name": "x", "sent": true, "then": [{ "do": "defer", "minutes": 500 }] }),
    ] {
        assert!(s.run("rules.save", &json!({ "rule": bad })).is_err(), "{bad}");
    }
    s.run(
        "rules.save",
        &json!({ "rule": { "name": "June", "sent": true, "when": [{ "test": "recipientAddress", "words": ["june@"] }],
            "then": [{ "do": "copy", "folder": "Inbox/Kept" }, { "do": "category", "name": "To June" }] } }),
    )
    .unwrap();
    s.run(
        "rules.save",
        &json!({ "rule": { "name": "Think twice", "sent": true, "when": [{ "test": "subject", "words": ["angry"] }], "then": [{ "do": "defer", "minutes": 5 }] } }),
    )
    .unwrap();
    // One that asks nothing takes everything sent.
    s.run("rules.save", &json!({ "rule": { "name": "All sent", "sent": true, "then": [{ "do": "category", "name": "Sent" }] } })).unwrap();
    assert!(s.rules[0].words(None).starts_with("Apply this rule after I send the message"));
    let sent_before = count(&s, "Sent Items");

    write(&mut s, &[("to", "June Okafor <june@example.test>"), ("subject", "Rota"), ("body", "Thanks.")]);
    let done = s.run("compose.send", &json!({ "anyway": true })).unwrap();
    assert_eq!(done["sent"], 1, "{done}");
    assert_eq!((count(&s, "Sent Items"), count(&s, "Kept")), (sent_before + 1, 1));
    s.run("folder.select", &json!({ "folder": "Sent Items" })).unwrap();
    let at = s.rows.iter().position(|r| r.subject == "Rota").unwrap();
    s.run("mail.select", &json!({ "index": at })).unwrap();
    assert_eq!(s.open.as_ref().unwrap().categories, ["To June", "Sent"]);
    // What went did not carry them.
    assert!(!String::from_utf8_lossy(&fake.state().submitted[0].data).contains("Keywords"));

    write(&mut s, &[("to", "tomas@example.test"), ("subject", "An angry note"), ("body", "No.")]);
    let held = s.run("compose.send", &json!({ "anyway": true })).unwrap();
    assert_eq!((held["sent"].as_u64(), held["waiting"].as_array().map(Vec::len)), (Some(0), Some(1)), "{held}");
    assert!(s.status.contains("held 5 minutes by the rule Think twice"), "{}", s.status);
    assert_eq!((count(&s, "Outbox"), count(&s, "Kept")), (1, 1));
    assert_eq!(fake.state().submitted.len(), 1);
}
