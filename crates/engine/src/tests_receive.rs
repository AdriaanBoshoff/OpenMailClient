//! Receiving: new mail is taken down from the in-process fake server into the Inbox, nothing
//! twice, and the server is changed only as the account says and only once mail is kept.

use crate::Session;
use openmailclient_net::fake::Fake;
use serde_json::{Value, json};
use std::sync::Arc;

fn message(subject: &str) -> Vec<u8> {
    format!("From: June <june@example.test>\r\nTo: pat@example.test\r\nSubject: {subject}\r\nDate: Tue, 13 Oct 2026 09:00:00 +0000\r\nMessage-ID: <{subject}@example.test>\r\n\r\nHello.\r\n.\r\n..and a dot\r\n").into_bytes()
}

fn with(fake: &Fake, kind: &str, extra: Value) -> Session {
    let mut s = Session::new();
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.connector = Some(Arc::new(fake.clone()));
    let mut account = json!({
        "email": "pat@example.test", "kind": kind,
        "incoming": { "host": "mail.example.test", "security": "starttls" },
        "outgoing": { "host": "mail.example.test", "security": "starttls" },
        "password": "not a real password",
    });
    if let (Some(a), Some(e)) = (account.as_object_mut(), extra.as_object()) {
        a.extend(e.clone());
    }
    s.run("account.add", &account).unwrap();
    s
}

fn inbox(s: &Session) -> (u32, u32) {
    s.folders[0].iter().find(|f| f.name == "Inbox").map_or((0, 0), |f| (f.total, f.unread))
}

fn add(fake: &Fake, subject: &str, flags: &[&str]) {
    fake.state().mailbox("INBOX").unwrap().add(&message(subject), flags);
}

#[test]
fn imap_mail_is_taken_down_once_and_left_as_it_was_on_the_server() {
    let fake = Fake::default();
    add(&fake, "one", &[]);
    add(&fake, "two", &["\\Seen"]);
    let mut s = with(&fake, "imap", json!({}));
    let (total, unread) = inbox(&s);
    let got = s.run("account.receive", &Value::Null).unwrap();
    assert_eq!(got["received"], 2, "{got}");
    assert_eq!(inbox(&s), (total + 2, unread + 1), "what the server has as read comes as read");
    assert!(s.status.starts_with("2 new messages"), "{}", s.status);
    // Read without being marked read, and nothing else said to change the server.
    let state = fake.state();
    assert!(state.mailboxes[0].messages[0].flags.is_empty());
    assert!(!state.heard.iter().any(|l| l.contains("STORE") || l.contains("EXPUNGE") || l.contains("BODY[]")), "{:?}", state.heard);
    drop(state);

    // Again: nothing new. Then one more comes, and only it is taken down.
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 0);
    assert_eq!(s.status, "No new mail");
    add(&fake, "three", &[]);
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 1);
    assert_eq!(inbox(&s).0, total + 3);

    // The message came whole: its dotted lines too.
    s.run("folder.select", &json!({ "folder": "Inbox" })).unwrap();
    let at = s.rows.iter().position(|r| r.subject == "three").unwrap();
    s.run("mail.select", &json!({ "index": at })).unwrap();
    let it = s.open.clone().unwrap();
    assert!(it.body_text().contains(".and a dot"), "{}", it.body_text());

    // The server numbered its messages anew: all of them are new again.
    fake.state().mailbox("INBOX").unwrap().uid_validity = 99;
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 3);
}

#[test]
fn pop3_mail_leaves_the_server_only_once_it_is_saved_here() {
    let dir = std::env::temp_dir().join(format!("omc-receive-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("box.pst");
    let mut sample = Session::new();
    sample.add_store(Box::new(crate::sample::sample_store(sample.now)));
    sample.run("file.export", &json!({ "path": path })).unwrap();

    let fake = Fake::default();
    add(&fake, "one", &[]);
    add(&fake, "two", &[]);
    let mut s = Session::new();
    s.connector = Some(Arc::new(fake.clone()));
    s.run("file.open", &json!({ "path": path })).unwrap();
    s.run(
        "account.add",
        &json!({ "email": "pat@example.test", "kind": "pop3", "incoming": { "host": "mail.example.test", "security": "starttls" }, "leaveOnServer": false, "password": "not a real password" }),
    )
    .unwrap();
    let (total, _) = inbox(&s);
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 2);
    assert_eq!(inbox(&s).0, total + 2);
    // Not yet saved: the server keeps them, and they are not taken down again.
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 0);
    assert_eq!(fake.state().mailboxes[0].messages.len(), 2);
    s.run("file.save", &Value::Null).unwrap();
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 0);
    assert!(fake.state().mailboxes[0].messages.is_empty(), "saved here, so taken off the server");
    assert!(s.accounts[0].received.seen.is_empty(), "ids the server no longer has are forgotten");
    assert_eq!(inbox(&s).0, total + 2);
    s.run("file.closeStore", &json!({ "discard": true })).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pop3_mail_left_on_the_server_is_remembered_between_runs() {
    let fake = Fake::default();
    add(&fake, "one", &[]);
    let mut s = with(&fake, "pop3", json!({}));
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 1);
    let settings = s.settings(None);
    assert_eq!(settings["accounts"][0]["received"]["seen"][0], "uid-1", "{settings}");
    // account.list does not give what was taken down; the settings file keeps it.
    assert!(s.run("account.list", &Value::Null).unwrap()["accounts"][0].get("received").is_none());

    let dir = std::env::temp_dir().join(format!("omc-receive-settings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("settings.json");
    s.run("settings.save", &json!({ "path": file.display().to_string() })).unwrap();
    let mut again = with(&fake, "imap", json!({ "email": "other@example.test" }));
    again.run("settings.load", &json!({ "path": file.display().to_string() })).unwrap();
    again.run("account.setPassword", &json!({ "id": "account-1", "password": "not a real password" })).unwrap();
    assert_eq!(again.run("account.receive", &Value::Null).unwrap()["received"], 0, "taken down before, so not again");
    assert_eq!(fake.state().mailboxes[0].messages.len(), 1, "left on the server, as the account says");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_receive_that_fails_says_why_and_takes_nothing() {
    let fake = Fake::default();
    add(&fake, "one", &[]);
    let mut s = with(&fake, "imap", json!({ "password": "wrong" }));
    let got = s.run("account.receive", &Value::Null).unwrap();
    assert_eq!(got["received"], 0);
    assert!(s.status.contains("not accepted"), "{}", s.status);
    assert!(!s.status.contains("wrong") && !got.to_string().contains("wrong"));
    assert_eq!(s.accounts[0].received, crate::accounts::Received::default());
    assert!(s.run("account.receive", &json!({ "id": "account-9" })).is_err());
    assert!(s.run("account.receive", &json!({ "id": 7 })).is_err());
}

#[test]
fn a_message_that_cannot_be_read_is_kept_whole() {
    let fake = Fake::default();
    fake.state().mailbox("INBOX").unwrap().add(b"\x00\x01\x02 not mail at all", &[]);
    let mut s = with(&fake, "imap", json!({}));
    let (total, _) = inbox(&s);
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 1);
    assert_eq!(inbox(&s).0, total + 1);
    s.run("folder.select", &json!({ "folder": "Inbox" })).unwrap();
    let at = s.rows.iter().position(|r| r.subject.starts_with("A message that could not be read")).unwrap();
    s.run("mail.select", &json!({ "index": at })).unwrap();
    assert_eq!(s.open.as_ref().unwrap().attachments[0].data, b"\x00\x01\x02 not mail at all");
}

#[test]
fn send_and_receive_runs_on_threads_of_their_own() {
    let fake = Fake::default();
    add(&fake, "one", &[]);
    let mut s = with(&fake, "imap", json!({}));
    s.run("mail.new", &Value::Null).unwrap();
    s.run("compose.setField", &json!({ "field": "to", "value": "june@example.test" })).unwrap();
    s.run("compose.setField", &json!({ "field": "subject", "value": "Hi" })).unwrap();
    s.run("compose.setField", &json!({ "field": "body", "value": "x" })).unwrap();
    s.run("compose.saveDraft", &Value::Null).unwrap();
    let total = inbox(&s).0;
    // From the window: F9, not waited for.
    let started = s.run("mail.sendReceive", &json!({ "wait": false })).unwrap();
    assert_eq!(started["receive"]["started"], true, "{started}");
    assert!(s.run("account.receive", &json!({ "wait": false })).is_err(), "one receive at a time");
    let mut waited = 0;
    while !s.jobs.is_empty() {
        s.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
        assert!(waited < 1000, "the receive never ended: {}", s.status);
    }
    assert_eq!(inbox(&s).0, total + 1);
    assert!(fake.state().submitted.is_empty(), "a draft is not sent");
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

fn count_in(s: &Session, store: usize, name: &str) -> u32 {
    s.folders[store].iter().find(|f| f.name == name).map_or(0, |f| f.total)
}

#[test]
fn an_account_keeps_its_mail_in_a_data_file_of_its_own() {
    let dir = std::env::temp_dir().join(format!("omc-account-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let fake = Fake::default();
    add(&fake, "one", &[]);
    add(&fake, "two", &[]);
    let mut s = Session::new();
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.connector = Some(Arc::new(fake.clone()));
    s.account_dir = Some(dir.join("Accounts"));
    s.journal_dir = Some(dir.join("journals"));
    let sample_inbox = inbox(&s);
    s.run(
        "account.add",
        &json!({ "email": "pat@example.test", "kind": "pop3", "leaveOnServer": false,
                 "incoming": { "host": "mail.example.test", "security": "starttls" },
                 "outgoing": { "host": "mail.example.test", "security": "starttls" }, "password": "not a real password" }),
    )
    .unwrap();
    assert!(s.status.contains("data file of its own"), "{}", s.status);
    let file = s.accounts[0].data_file.clone().unwrap();
    assert_eq!(file, dir.join("Accounts").join("pat@example.test.pst"));
    assert!(file.exists() && s.stores.len() == 2);

    // The mail goes in the account's own Inbox, never in the data file that was open.
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 2);
    assert_eq!(inbox(&s), sample_inbox, "the sample keeps only its own mail");
    assert_eq!(count_in(&s, 1, "Inbox"), 2);
    // Saved on its own, so POP3 mail can leave the server on the next receive.
    settle(&mut s);
    assert!(!s.stores[1].unsaved(), "{}", s.status);
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 0);
    assert!(fake.state().mailboxes[0].messages.is_empty());

    // What went is kept in the account's own Sent Items.
    s.run("mail.new", &Value::Null).unwrap();
    s.run("compose.setField", &json!({ "field": "to", "value": "june@example.test" })).unwrap();
    s.run("compose.setField", &json!({ "field": "subject", "value": "Mine" })).unwrap();
    let sent_before = count_in(&s, 0, "Sent Items");
    assert_eq!(s.run("compose.send", &json!({ "anyway": true })).unwrap()["sent"], 1);
    assert_eq!((count_in(&s, 0, "Sent Items"), count_in(&s, 1, "Sent Items")), (sent_before, 1));
    settle(&mut s);

    // Next run: the settings name the file, and it opens again with what it holds.
    let settings = s.settings(None);
    assert_eq!(settings["accounts"][0]["dataFile"], file.display().to_string(), "{settings}");
    let path = dir.join("settings.json");
    s.run("settings.save", &json!({ "path": path.display().to_string() })).unwrap();
    let mut again = Session::new();
    again.account_dir = Some(dir.join("Accounts"));
    again.run("settings.load", &json!({ "path": path.display().to_string() })).unwrap();
    assert_eq!(again.stores.len(), 1, "{}", again.status);
    assert_eq!((count_in(&again, 0, "Inbox"), count_in(&again, 0, "Sent Items")), (2, 1));

    // An account kept before accounts had files of their own is given one when it is loaded.
    let older = dir.join("older.json");
    let mut kept = settings.clone();
    kept["accounts"][0]["email"] = json!("sam@example.test");
    kept["accounts"][0].as_object_mut().unwrap().remove("dataFile");
    std::fs::write(&older, kept.to_string()).unwrap();
    let mut third = Session::new();
    third.account_dir = Some(dir.join("Accounts"));
    third.run("settings.load", &json!({ "path": older.display().to_string() })).unwrap();
    assert_eq!(third.stores.len(), 1, "{}", third.status);
    assert!(dir.join("Accounts").join("sam@example.test.pst").exists());
    third.run("file.closeStore", &json!({ "discard": true })).unwrap();

    // Removing the account keeps its mail.
    again.run("account.remove", &json!({ "id": "account-1" })).unwrap();
    assert!(again.status.contains("its mail stays"), "{}", again.status);
    assert!(file.exists());
    again.run("file.closeStore", &json!({ "discard": true })).unwrap();
    s.run("file.closeStore", &json!({ "store": 1, "discard": true })).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_data_file_named_in_the_settings_must_be_a_pst_by_its_full_path() {
    let mut s = Session::new();
    for bad in ["relative.pst", "C:/x/mail.txt", "/x/mail.txt", "\u{0}.pst"] {
        let settings = json!({ "accounts": [{ "id": "account-1", "email": "pat@example.test", "kind": "imap", "incoming": { "host": "h.test" }, "dataFile": bad }] });
        let dir = std::env::temp_dir().join(format!("omc-bad-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, settings.to_string()).unwrap();
        s.run("settings.load", &json!({ "path": path.display().to_string() })).unwrap();
        assert!(s.accounts.first().is_none_or(|a| a.data_file.is_none()), "{bad}");
        assert!(s.stores.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn mail_taken_into_another_data_file_is_taken_down_again_into_the_account_s_own() {
    let dir = std::env::temp_dir().join(format!("omc-account-again-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let fake = Fake::default();
    add(&fake, "one", &[]);
    add(&fake, "two", &[]);
    // First into the sample, as before accounts had data files of their own.
    let mut s = with(&fake, "imap", json!({}));
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 2);
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 0);
    // Then the account has its own: none of that mail is in it, so all of it comes again.
    s.account_dir = Some(dir.join("Accounts"));
    s.journal_dir = Some(dir.join("journals"));
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 2, "{}", s.status);
    assert_eq!(count_in(&s, 1, "Inbox"), 2);
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 0, "and only once");
    settle(&mut s);
    s.run("file.closeStore", &json!({ "store": 1, "discard": true })).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn imap_folders_come_into_the_account_s_own_data_file_as_the_server_has_them() {
    use openmailclient_net::fake::Mailbox;
    let dir = std::env::temp_dir().join(format!("omc-imap-folders-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let fake = Fake::default();
    add(&fake, "new", &[]);
    {
        let mut state = fake.state();
        state.mailbox("Sent").unwrap().add(&message("went"), &["\\Seen"]);
        state.mailbox("Trash").unwrap().add(&message("gone"), &["\\Seen"]);
        state.mailboxes.push(Mailbox::new("Projects", &["\\HasChildren"]));
        let mut plans = Mailbox::new("Projects/Plans", &[]);
        plans.add(&message("plan"), &[]);
        state.mailboxes.push(plans);
        // A view of mail that is in other folders too, and a folder that holds none.
        let mut all = Mailbox::new("All Mail", &["\\All"]);
        all.add(&message("new"), &[]);
        state.mailboxes.push(all);
        state.mailboxes.push(Mailbox::new("Shared", &["\\Noselect"]));
        // A folder named as the Outbox: its mail would be sent from here, so it is not kept.
        let mut outbox = Mailbox::new("Outbox", &[]);
        outbox.add(&message("not to go"), &[]);
        state.mailboxes.push(outbox);
    }
    let mut s = with(&fake, "imap", json!({}));
    s.account_dir = Some(dir.join("Accounts"));
    s.journal_dir = Some(dir.join("journals"));
    let got = s.run("account.receive", &Value::Null).unwrap();
    assert_eq!(got["received"], 4, "{}", s.status);
    assert_eq!(
        (count_in(&s, 1, "Inbox"), count_in(&s, 1, "Sent Items"), count_in(&s, 1, "Deleted Items"), count_in(&s, 1, "Plans")),
        (1, 1, 1, 1),
        "{:?}",
        s.folders[1]
    );
    assert_eq!(count_in(&s, 1, "Outbox"), 0, "nothing put in the Outbox to be sent");
    assert!(!s.folders[1].iter().any(|f| f.name == "All Mail" || f.name == "Shared"));
    let projects = s.folders[1].iter().find(|f| f.name == "Projects").unwrap().id;
    assert_eq!(s.folders[1].iter().find(|f| f.name == "Plans").unwrap().parent, Some(projects));
    // Read on the server, read here.
    assert_eq!(s.folders[1].iter().find(|f| f.name == "Sent Items").unwrap().unread, 0);
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 0, "and only once");

    // What is sent from here is put in the server's Sent folder, read, and not taken down again.
    s.run("mail.new", &Value::Null).unwrap();
    s.run("compose.setField", &json!({ "field": "to", "value": "june@example.test" })).unwrap();
    s.run("compose.setField", &json!({ "field": "bcc", "value": "sam@example.test" })).unwrap();
    s.run("compose.setField", &json!({ "field": "subject", "value": "From here" })).unwrap();
    let sent = s.run("compose.send", &json!({ "anyway": true })).unwrap();
    assert_eq!((sent["sent"].as_u64(), sent["onServer"].as_u64()), (Some(1), Some(1)), "{sent} {}", s.status);
    {
        let mut state = fake.state();
        let on_server = state.mailbox("Sent").unwrap();
        assert_eq!(on_server.messages.len(), 2);
        let copy = on_server.messages.last().unwrap();
        assert_eq!(copy.flags, ["\\Seen"]);
        let text = String::from_utf8_lossy(&copy.data);
        assert!(text.contains("Subject: From here") && text.contains("sam@example.test"), "the sender's copy keeps the Bcc: {text}");
    }
    assert_eq!(count_in(&s, 1, "Sent Items"), 2, "{}", s.status);
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 0, "{}", s.status);
    assert_eq!(count_in(&s, 1, "Sent Items"), 2, "the copy on the server is not kept twice");
    assert!(s.accounts[0].received.sent_here.is_empty(), "known once seen");

    // What the account remembers is kept between runs.
    let kept = s.accounts[0].to_settings();
    let (again, ignored) = crate::accounts::from_settings(&json!([kept]));
    assert_eq!((ignored, &again[0].received), (0, &s.accounts[0].received));
    assert!(again[0].received.folders.iter().any(|m| m.raw == "Projects/Plans" && m.last_uid == 1));

    // Asked not to, nothing is put on the server.
    s.run("account.edit", &json!({ "id": "account-1", "sentOnServer": false })).unwrap();
    s.run("mail.new", &Value::Null).unwrap();
    s.run("compose.setField", &json!({ "field": "to", "value": "june@example.test" })).unwrap();
    s.run("compose.setField", &json!({ "field": "subject", "value": "Only here" })).unwrap();
    assert_eq!(s.run("compose.send", &json!({ "anyway": true })).unwrap()["onServer"], 0);
    assert_eq!(fake.state().mailbox("Sent").unwrap().messages.len(), 2);
    settle(&mut s);
    s.run("file.closeStore", &json!({ "store": 1, "discard": true })).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_server_without_a_sent_folder_is_given_one() {
    let fake = Fake::default();
    fake.state().mailboxes.retain(|m| m.name == "INBOX");
    let mut s = with(&fake, "imap", json!({}));
    s.run("mail.new", &Value::Null).unwrap();
    s.run("compose.setField", &json!({ "field": "to", "value": "june@example.test" })).unwrap();
    s.run("compose.setField", &json!({ "field": "subject", "value": "First" })).unwrap();
    assert_eq!(s.run("compose.send", &json!({ "anyway": true })).unwrap()["onServer"], 1, "{}", s.status);
    assert_eq!(fake.state().mailbox("Sent").unwrap().messages.len(), 1);
}

#[test]
fn a_sent_copy_the_server_refuses_leaves_what_went_as_it_went() {
    let fake = Fake::default();
    // The IMAP server offers no secure connection: the copy cannot go, the message has gone
    // all the same.
    fake.state().imap_without.push("STARTTLS".into());
    let mut s = with(&fake, "imap", json!({}));
    s.run("mail.new", &Value::Null).unwrap();
    s.run("compose.setField", &json!({ "field": "to", "value": "june@example.test" })).unwrap();
    s.run("compose.setField", &json!({ "field": "subject", "value": "Gone" })).unwrap();
    let sent = s.run("compose.send", &json!({ "anyway": true })).unwrap();
    assert_eq!((sent["sent"].as_u64(), sent["onServer"].as_u64()), (Some(1), Some(0)), "{sent}");
    assert!(s.status.contains("could not be put in the server's Sent folder"), "{}", s.status);
    assert_eq!(fake.state().submitted.len(), 1);
}

fn many(fake: &Fake, n: usize) {
    let mut state = fake.state();
    let inbox = state.mailbox("INBOX").unwrap();
    for i in 0..n {
        inbox.add(&message(&format!("m{i}")), &[]);
    }
}

fn subjects(s: &mut Session) -> Vec<String> {
    s.run("folder.select", &json!({ "folder": "Inbox" })).unwrap();
    s.rows.iter().map(|r| r.subject.clone()).collect()
}

#[test]
fn mail_shows_in_the_inbox_as_it_comes_down() {
    let fake = Fake::default();
    many(&fake, 450);
    let mut s = with(&fake, "imap", json!({}));
    let total = inbox(&s).0;
    s.run("account.receive", &json!({ "wait": false })).unwrap();
    // A part is taken while the receive still runs: one part a time round, so the end is
    // never taken in the same round as the first part.
    let mut waited = 0;
    while inbox(&s).0 == total {
        s.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
        assert!(waited < 3000, "no part came: {}", s.status);
    }
    assert!(!s.jobs.is_empty(), "the first part came before the receive ended");
    let part = inbox(&s).0 - total;
    assert!(part > 0 && part < 450, "{part}");
    assert!(s.status.contains("so far"), "{}", s.status);
    let here = subjects(&mut s);
    // Into a data file that is not the account's own, mail comes in the server's order.
    assert!(here.iter().any(|t| t == "m0") && !here.iter().any(|t| t == "m449"));
    settle(&mut s);
    assert_eq!(inbox(&s).0, total + 450);
    assert!(s.status.starts_with("450 new messages"), "{}", s.status);
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 0, "nothing twice");
}

#[test]
fn a_receive_stopped_part_way_takes_the_rest_next_time_and_nothing_twice() {
    let fake = Fake::default();
    many(&fake, 450);
    let mut s = with(&fake, "imap", json!({}));
    let total = inbox(&s).0;
    s.run("account.receive", &json!({ "wait": false })).unwrap();
    let mut waited = 0;
    while inbox(&s).0 == total && !s.jobs.is_empty() {
        s.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
        assert!(waited < 3000, "no part came: {}", s.status);
    }
    // As when the window is closed.
    s.leave_jobs(std::time::Duration::from_secs(5));
    assert!(s.jobs.is_empty());
    let kept = inbox(&s).0 - total;
    assert!(kept > 0 && kept <= 450, "{kept}");
    s.run("account.receive", &Value::Null).unwrap();
    assert_eq!(inbox(&s).0, total + 450, "the rest came, none of it twice");
    let here = subjects(&mut s);
    for i in [0, 1, 224, 448, 449] {
        assert_eq!(here.iter().filter(|t| **t == format!("m{i}")).count(), 1, "m{i}");
    }
    assert_eq!(s.run("account.receive", &Value::Null).unwrap()["received"], 0);
}

#[test]
fn leaving_is_never_kept_waiting_for_a_server() {
    let fake = Fake::default();
    many(&fake, 300);
    let mut s = with(&fake, "imap", json!({}));
    s.run("account.receive", &json!({ "wait": false })).unwrap();
    let began = std::time::Instant::now();
    s.leave_jobs(std::time::Duration::ZERO);
    assert!(s.jobs.is_empty());
    assert!(began.elapsed() < std::time::Duration::from_secs(2), "{:?}", began.elapsed());
    // The thread left behind ends alone, and what it had not kept comes again.
    s.run("account.receive", &Value::Null).unwrap();
    assert_eq!(subjects(&mut s).iter().filter(|t| t.starts_with('m')).count(), 300);
}
