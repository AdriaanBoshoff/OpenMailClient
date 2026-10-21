//! IMAP kept in step both ways, against the in-process fake server: read and flagged each way,
//! moves, deletions for good, messages put into the account from elsewhere, the first pass that
//! finds the messages taken down before, and a folder numbered anew.

use crate::Session;
use crate::cmd::imap_sync::Place;
use openmailclient_model::{FolderKind, ItemId};
use openmailclient_net::fake::{Fake, Mailbox};
use serde_json::{Value, json};
use std::sync::Arc;

fn message(subject: &str) -> Vec<u8> {
    format!("From: June <june@example.test>\r\nTo: pat@example.test\r\nSubject: {subject}\r\nDate: Tue, 13 Oct 2026 09:00:00 +0000\r\nMessage-ID: <{subject}@example.test>\r\n\r\nHello.\r\n").into_bytes()
}

/// A session with an IMAP account whose mail is in a data file of its own, store 1, and the
/// folder it was made in, to be cleared away at the end.
fn account(fake: &Fake, name: &str) -> (Session, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("omc-sync-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session::new();
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.connector = Some(Arc::new(fake.clone()));
    s.account_dir = Some(dir.join("Accounts"));
    s.journal_dir = Some(dir.join("journals"));
    s.run(
        "account.add",
        &json!({ "email": "pat@example.test", "kind": "imap",
                 "incoming": { "host": "mail.example.test", "security": "starttls" },
                 "outgoing": { "host": "mail.example.test", "security": "starttls" }, "password": "not a real password" }),
    )
    .unwrap();
    assert_eq!(s.stores.len(), 2);
    (s, dir)
}

fn done(mut s: Session, dir: std::path::PathBuf) {
    settle(&mut s);
    let _ = s.run("file.closeStore", &json!({ "store": 1, "discard": true }));
    let _ = std::fs::remove_dir_all(&dir);
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

fn folder(s: &Session, name: &str) -> openmailclient_model::FolderId {
    s.folders[1].iter().find(|f| f.name == name).map(|f| f.id).unwrap_or_else(|| panic!("no {name}: {:?}", s.folders[1]))
}

fn count(s: &Session, name: &str) -> u32 {
    s.folders[1].iter().find(|f| f.name == name).map_or(0, |f| f.total)
}

/// Select the message of that subject in a folder of the account's data file.
fn pick(s: &mut Session, name: &str, subject: &str) -> u64 {
    let f = folder(s, name);
    s.select_folder(1, f).unwrap();
    let id = s.rows.iter().find(|r| r.subject == subject).map(|r| r.id).unwrap_or_else(|| panic!("no {subject} in {name}"));
    s.select_item(id).unwrap();
    id
}

fn flags(fake: &Fake, mailbox: &str, subject: &str) -> Vec<String> {
    let mut state = fake.state();
    let m = state.mailbox(mailbox).unwrap();
    let wanted = format!("Subject: {subject}\r\n");
    m.messages
        .iter()
        .find(|m| String::from_utf8_lossy(&m.data).contains(&wanted))
        .map(|m| m.flags.clone())
        .unwrap_or_else(|| vec!["(not there)".into()])
}

fn on_server(fake: &Fake, mailbox: &str) -> usize {
    fake.state().mailbox(mailbox).map_or(0, |m| m.messages.len())
}

#[test]
fn read_and_flagged_go_both_ways() {
    let fake = Fake::default();
    fake.state().mailbox("INBOX").unwrap().add(&message("one"), &[]);
    fake.state().mailbox("INBOX").unwrap().add(&message("two"), &[]);
    let (mut s, dir) = account(&fake, "flags");
    assert_eq!(receive(&mut s)["received"], 2, "{}", s.status);
    assert!(s.accounts[0].received.matched, "the first pass looked through every folder");
    let id = pick(&mut s, "Inbox", "one");
    let inbox = folder(&s, "Inbox");
    let item = s.stores[1].load(inbox, ItemId(id)).unwrap();
    assert!(matches!(Place::of(&item), Some(Place::On { ref raw, uid: 1, seen: false, .. }) if raw == "INBOX"), "{:?}", Place::of(&item));

    // Read here, and flagged there.
    s.run("mail.markRead", &Value::Null).unwrap();
    {
        let mut state = fake.state();
        let inbox = state.mailbox("INBOX").unwrap();
        inbox.messages[1].flags.push("\\Flagged".into());
    }
    let got = s.run("account.receive", &Value::Null).unwrap();
    assert!(s.status.contains("2 changes kept in step with the server"), "{}", s.status);
    settle(&mut s);
    assert_eq!(flags(&fake, "INBOX", "one"), ["\\Seen"], "{}", s.status);
    assert_eq!(got["accounts"][0]["inStep"]["pushed"], 1, "{got}");
    assert_eq!(got["accounts"][0]["inStep"]["changed"], 1, "{got}");
    pick(&mut s, "Inbox", "two");
    assert_eq!(s.open.as_ref().unwrap().flag(), openmailclient_model::Flag::Flagged);
    // Opened, as in Outlook, it is read: that goes too.
    receive(&mut s);
    assert_eq!(flags(&fake, "INBOX", "two"), ["\\Flagged", "\\Seen"]);

    // Nothing changed: nothing goes either way, and no message is taken down twice.
    let heard = fake.state().heard.len();
    let got = receive(&mut s);
    assert_eq!((got["received"].as_u64(), got["accounts"][0]["inStep"]["pushed"].as_u64()), (Some(0), Some(0)), "{got}");
    assert!(!fake.state().heard[heard..].iter().any(|c| c.contains("STORE")), "{:?}", &fake.state().heard[heard..]);
    assert_eq!(count(&s, "Inbox"), 2);

    // Unread there and unflagged here: each side has the other's change.
    s.run("mail.flag", &Value::Null).unwrap();
    assert_eq!(s.open.as_ref().unwrap().flag(), openmailclient_model::Flag::None);
    fake.state().mailbox("INBOX").unwrap().messages[0].flags.clear();
    receive(&mut s);
    assert_eq!(flags(&fake, "INBOX", "two"), ["\\Seen"]);
    s.select_folder(1, folder(&s, "Inbox")).unwrap();
    assert!(!s.rows.iter().find(|r| r.subject == "one").unwrap().read, "unread there, so unread here");
    done(s, dir);
}

#[test]
fn a_move_here_is_a_move_there() {
    for with_move in [true, false] {
        let fake = Fake::default();
        if with_move {
            fake.state().imap_caps.push("MOVE".into());
        }
        {
            let mut state = fake.state();
            state.mailbox("INBOX").unwrap().add(&message("keep"), &[]);
            state.mailbox("INBOX").unwrap().add(&message("file"), &[]);
            state.mailboxes.push(Mailbox::new("Projects", &[]));
        }
        let (mut s, dir) = account(&fake, if with_move { "move" } else { "copy" });
        receive(&mut s);
        pick(&mut s, "Inbox", "file");
        s.run("mail.move", &json!({ "folderId": folder(&s, "Projects").0 })).unwrap();
        let got = receive(&mut s);
        assert_eq!((on_server(&fake, "INBOX"), on_server(&fake, "Projects")), (1, 1), "{} {got}", s.status);
        assert_eq!(got["received"], 0, "the moved message is not taken down again: {got}");
        assert_eq!((count(&s, "Inbox"), count(&s, "Projects")), (1, 1));
        let heard = fake.state().heard.join("\n");
        assert_eq!(heard.contains("UID MOVE"), with_move, "{heard}");
        assert_eq!(heard.contains("UID COPY"), !with_move, "{heard}");
        // Its new place is known: a change to it goes to the folder it is in now.
        pick(&mut s, "Projects", "file");
        s.run("mail.markRead", &Value::Null).unwrap();
        receive(&mut s);
        assert_eq!(flags(&fake, "Projects", "file"), ["\\Seen"]);
        assert_eq!(count(&s, "Projects"), 1);

        // To a folder made here: it is made there too.
        let projects = folder(&s, "Projects");
        let st = &mut s.stores[1];
        st.create_folder(projects, "Plans").unwrap();
        s.refresh_folders(1);
        pick(&mut s, "Projects", "file");
        s.run("mail.move", &json!({ "folderId": folder(&s, "Plans").0 })).unwrap();
        let got = receive(&mut s);
        assert_eq!(got["accounts"][0]["inStep"]["created"], json!(["Projects/Plans"]), "{got}");
        assert_eq!((on_server(&fake, "Projects"), on_server(&fake, "Projects/Plans")), (0, 1));
        assert_eq!(count(&s, "Plans"), 1);
        done(s, dir);
    }
}

#[test]
fn a_message_deleted_for_good_goes_from_the_server_and_one_the_server_lost_goes_from_here() {
    let fake = Fake::default();
    {
        let mut state = fake.state();
        state.mailbox("INBOX").unwrap().add(&message("unwanted"), &[]);
        state.mailbox("INBOX").unwrap().add(&message("lost there"), &[]);
        state.mailbox("INBOX").unwrap().add(&message("stays"), &[]);
    }
    let (mut s, dir) = account(&fake, "delete");
    receive(&mut s);
    // Deleted: it moves to Deleted Items here, and to the server's Trash.
    pick(&mut s, "Inbox", "unwanted");
    s.run("mail.delete", &Value::Null).unwrap();
    receive(&mut s);
    assert_eq!((on_server(&fake, "INBOX"), on_server(&fake, "Trash")), (2, 1), "{}", s.status);
    // Deleted from Deleted Items: gone for good, there too.
    pick(&mut s, "Deleted Items", "unwanted");
    s.run("mail.delete", &Value::Null).unwrap();
    assert_eq!(s.accounts[0].received.gone.len(), 1);
    // Taken back before the next receive, it stays on the server.
    s.run("edit.undo", &Value::Null).unwrap();
    assert!(s.accounts[0].received.gone.is_empty());
    pick(&mut s, "Deleted Items", "unwanted");
    s.run("mail.delete", &Value::Null).unwrap();
    receive(&mut s);
    assert_eq!(on_server(&fake, "Trash"), 0, "{}", s.status);
    assert!(s.accounts[0].received.gone.is_empty(), "done, so forgotten");

    // The server lost one: it goes from here, and only it.
    fake.state().mailbox("INBOX").unwrap().messages.retain(|m| !String::from_utf8_lossy(&m.data).contains("lost there"));
    let got = receive(&mut s);
    assert_eq!(got["accounts"][0]["inStep"]["removed"], 1, "{got}");
    assert_eq!(count(&s, "Inbox"), 1);
    pick(&mut s, "Inbox", "stays");
    done(s, dir);
}

#[test]
fn a_message_put_into_the_account_from_elsewhere_goes_on_the_server_once() {
    let fake = Fake::default();
    fake.state().mailbox("INBOX").unwrap().add(&message("first"), &[]);
    let (mut s, dir) = account(&fake, "upload");
    receive(&mut s);
    // From the sample's Inbox into the account's.
    let from = s.find_folder(0, FolderKind::Inbox).unwrap();
    let row = s.stores[0].list(from).unwrap()[0].clone();
    let item = s.stores[0].load(from, ItemId(row.id)).unwrap();
    let inbox = folder(&s, "Inbox");
    s.stores[1].add(inbox, item).unwrap();
    s.refresh_folders(1);
    let got = receive(&mut s);
    assert_eq!(on_server(&fake, "INBOX"), 2, "{} {got}", s.status);
    assert_eq!(got["received"], 0, "{got}");
    assert_eq!(count(&s, "Inbox"), 2);
    let again = receive(&mut s);
    assert_eq!((on_server(&fake, "INBOX"), count(&s, "Inbox"), again["received"].as_u64()), (2, 2, Some(0)));
    done(s, dir);
}

#[test]
fn mail_taken_down_before_is_found_on_the_server_and_none_of_it_is_put_there_again() {
    let fake = Fake::default();
    fake.state().mailbox("INBOX").unwrap().add(&message("old"), &[]);
    fake.state().mailbox("Sent").unwrap().add(&message("went"), &["\\Seen"]);
    let (mut s, dir) = account(&fake, "match");
    receive(&mut s);
    // As a receive before this program knew places left it.
    for f in [folder(&s, "Inbox"), folder(&s, "Sent Items")] {
        for r in s.stores[1].list(f).unwrap() {
            let mut item = s.stores[1].load(f, ItemId(r.id)).unwrap();
            Place::clear(&mut item);
            s.stores[1].update(f, ItemId(r.id), item).unwrap();
        }
    }
    s.accounts[0].received.matched = false;
    s.places = Default::default();
    // One kept only here: it stays so.
    let inbox = folder(&s, "Inbox");
    let mut only = openmailclient_model::Item::new_mail();
    only.set_subject("only here");
    s.stores[1].add(inbox, only).unwrap();
    s.refresh_folders(1);
    let got = receive(&mut s);
    assert!(s.accounts[0].received.matched, "{} {got}", s.status);
    assert_eq!((on_server(&fake, "INBOX"), on_server(&fake, "Sent")), (1, 1));
    let id = pick(&mut s, "Inbox", "old");
    assert!(matches!(Place::of(&s.stores[1].load(inbox, ItemId(id)).unwrap()), Some(Place::On { uid: 1, .. })));
    let id = pick(&mut s, "Inbox", "only here");
    assert_eq!(Place::of(&s.stores[1].load(inbox, ItemId(id)).unwrap()), Some(Place::Here));
    // Found, a change goes to it.
    pick(&mut s, "Inbox", "old");
    s.run("mail.markRead", &Value::Null).unwrap();
    receive(&mut s);
    assert_eq!(flags(&fake, "INBOX", "old"), ["\\Seen"]);
    assert_eq!(on_server(&fake, "INBOX"), 1);
    done(s, dir);
}

#[test]
fn a_folder_numbered_anew_is_found_again_by_message_id() {
    let fake = Fake::default();
    fake.state().mailbox("INBOX").unwrap().add(&message("one"), &[]);
    let (mut s, dir) = account(&fake, "validity");
    receive(&mut s);
    pick(&mut s, "Inbox", "one");
    s.run("mail.markRead", &Value::Null).unwrap();
    fake.state().mailbox("INBOX").unwrap().uid_validity = 4242;
    let got = receive(&mut s);
    assert_eq!(count(&s, "Inbox"), 1, "not kept twice: {} {got}", s.status);
    let id = pick(&mut s, "Inbox", "one");
    let inbox = folder(&s, "Inbox");
    let place = Place::of(&s.stores[1].load(inbox, ItemId(id)).unwrap());
    assert!(matches!(place, Some(Place::On { uid_validity: 4242, .. })), "{place:?}");
    // The change made before goes with the next receive.
    receive(&mut s);
    assert_eq!(flags(&fake, "INBOX", "one"), ["\\Seen"]);
    done(s, dir);
}

#[test]
fn places_that_make_no_sense_are_no_places() {
    let mut item = openmailclient_model::Item::new_mail();
    for junk in ["", "on", "on 1", "on x 2 S- INBOX", "on 1 0 S- INBOX", "on 1 2 S-", "nowhere", "on 1 2 S- IN\u{7}BOX", &"x".repeat(5000)] {
        item.named.retain(|_| false);
        item.named.push((
            openmailclient_model::NamedKey {
                guid: crate::cmd::imap_sync::PS_OPENMAILCLIENT,
                name: openmailclient_model::NameId::Name("ImapPlace".into()),
            },
            openmailclient_model::PropValue::String(junk.to_string()),
        ));
        assert_eq!(Place::of(&item), None, "{junk:?}");
    }
    for place in [
        Place::On { raw: "Projects/Two words".into(), uid_validity: 7, uid: 9, seen: true, flagged: false },
        Place::Pending { raw: String::new() },
        Place::Here,
    ] {
        place.put(&mut item);
        assert_eq!(Place::of(&item), Some(place));
    }
}

#[test]
fn a_flag_set_in_a_folder_written_empty_shows_in_the_list_after_a_save() {
    // The account's data file is made with empty folders, whose tables have only the columns
    // every table has; a save into it in place gives them the flag's column.
    let fake = Fake::default();
    let (mut s, dir) = account(&fake, "flag-column");
    let inbox = folder(&s, "Inbox");
    let mut it = openmailclient_model::Item::new_mail();
    it.set_subject("x");
    let id = s.stores[1].add(inbox, it).unwrap();
    s.run("file.save", &json!({ "store": 1 })).unwrap();
    let flags = |s: &mut Session| s.stores[1].list(inbox).unwrap().iter().map(|r| r.flag).collect::<Vec<_>>();
    let mut it = s.stores[1].load(inbox, id).unwrap();
    it.set_flag(openmailclient_model::Flag::Flagged);
    s.stores[1].update(inbox, id, it).unwrap();
    s.run("file.save", &json!({ "store": 1 })).unwrap();
    assert_eq!(flags(&mut s), [openmailclient_model::Flag::Flagged]);
    let mut it = s.stores[1].load(inbox, id).unwrap();
    it.set_read(true);
    s.stores[1].update(inbox, id, it).unwrap();
    s.run("file.save", &json!({ "store": 1 })).unwrap();
    assert_eq!(flags(&mut s), [openmailclient_model::Flag::Flagged]);
    done(s, dir);
}

/// The commands a receive sent the server, from `from` on.
fn heard_since(fake: &Fake, from: usize) -> Vec<String> {
    fake.state().heard[from..].to_vec()
}

fn whole_lists(heard: &[String]) -> usize {
    heard.iter().filter(|c| c.contains("UID FETCH 1:* (UID FLAGS RFC822.SIZE)")).count()
}

#[test]
fn with_mod_sequences_only_what_changed_is_asked_for() {
    for caps in [&["CONDSTORE"][..], &["CONDSTORE", "QRESYNC", "ENABLE"][..]] {
        let qresync = caps.contains(&"QRESYNC");
        let fake = Fake::default();
        {
            let mut state = fake.state();
            state.imap_caps.extend(caps.iter().map(|c| c.to_string()));
            for n in ["one", "two", "three", "four"] {
                state.mailbox("INBOX").unwrap().add(&message(n), &[]);
            }
            state.mailbox("Sent").unwrap().add(&message("went"), &["\\Seen"]);
        }
        let (mut s, dir) = account(&fake, if qresync { "qresync" } else { "condstore" });
        receive(&mut s);
        assert_eq!(count(&s, "Inbox"), 4);
        let steps = &s.accounts[0].received.steps;
        assert!(steps.iter().any(|t| t.raw == "INBOX" && t.modseq > 0 && t.exists == 4), "{steps:?}");

        // Nothing changed: each folder costs its SELECT, and no list is read.
        let from = fake.state().heard.len();
        let got = receive(&mut s);
        let heard = heard_since(&fake, from);
        assert_eq!(whole_lists(&heard), 0, "{heard:?}");
        assert!(!heard.iter().any(|c| c.contains("CHANGEDSINCE")), "{heard:?}");
        assert!(got["accounts"][0]["inStep"]["unchanged"].as_u64().unwrap() >= 2, "{got}");

        // Flagged and read there, and one taken away there: only that is asked for, and comes.
        {
            let mut state = fake.state();
            let inbox = state.mailbox("INBOX").unwrap();
            inbox.set_flags(2, &["\\Flagged"]);
            inbox.set_flags(3, &["\\Seen"]);
            inbox.remove(4);
        }
        let from = fake.state().heard.len();
        let got = receive(&mut s);
        let heard = heard_since(&fake, from);
        // Without QRESYNC nothing says what went; the count does not add up, so the whole list
        // of the Inbox is read, and nothing is asked for first that it would tell again.
        assert_eq!(whole_lists(&heard), usize::from(!qresync), "{heard:?}");
        assert!(!heard.iter().any(|c| c.contains("CHANGEDSINCE")), "{heard:?}");
        assert_eq!(heard.iter().any(|c| c.contains("(QRESYNC (")), qresync, "{heard:?}");
        assert_eq!(got["accounts"][0]["inStep"]["changed"], 2, "{got}");
        if qresync {
            // What the server said went was made sure of before it went here.
            assert!(heard.iter().any(|c| c.contains("UID FETCH 4 (UID)")), "{heard:?}");
            assert_eq!(got["accounts"][0]["inStep"]["removed"], 1, "{got}");
            assert_eq!(count(&s, "Inbox"), 3);
        } else {
            assert_eq!(count(&s, "Inbox"), 3, "{got}");
        }
        pick(&mut s, "Inbox", "two");
        assert_eq!(s.open.as_ref().unwrap().flag(), openmailclient_model::Flag::Flagged);
        s.select_folder(1, folder(&s, "Inbox")).unwrap();
        assert!(s.rows.iter().find(|r| r.subject == "three").unwrap().read);

        // New mail with a flag changed: the new one is taken down, the change comes, and no
        // list of the whole folder is read, as the count adds up.
        {
            let mut state = fake.state();
            let inbox = state.mailbox("INBOX").unwrap();
            inbox.add(&message("five"), &[]);
            inbox.set_flags(1, &["\\Seen"]);
        }
        let from = fake.state().heard.len();
        let got = receive(&mut s);
        let heard = heard_since(&fake, from);
        assert_eq!(whole_lists(&heard), 0, "{heard:?}");
        assert_eq!(heard.iter().any(|c| c.contains("CHANGEDSINCE")), !qresync, "{heard:?}");
        assert_eq!((got["received"].as_u64(), got["accounts"][0]["inStep"]["changed"].as_u64()), (Some(1), Some(1)), "{got}");
        assert_eq!(count(&s, "Inbox"), 4);

        // A change made here goes there, and is not taken back as the server's next time.
        pick(&mut s, "Inbox", "five");
        s.run("mail.markRead", &Value::Null).unwrap();
        receive(&mut s);
        assert_eq!(flags(&fake, "INBOX", "five"), ["\\Seen"]);
        let got = receive(&mut s);
        assert_eq!(got["accounts"][0]["inStep"]["changed"], 0, "{got}");
        done(s, dir);
    }
}

#[test]
fn a_server_that_says_a_message_went_that_is_still_there_is_not_believed() {
    let fake = Fake::default();
    {
        let mut state = fake.state();
        state.imap_caps.extend(["CONDSTORE", "QRESYNC", "ENABLE"].map(String::from));
        for n in ["kept", "also kept"] {
            state.mailbox("INBOX").unwrap().add(&message(n), &[]);
        }
    }
    let (mut s, dir) = account(&fake, "vanished-wrongly");
    receive(&mut s);
    // The server names both as gone, and every id there might be, though it has them still.
    {
        let mut state = fake.state();
        let inbox = state.mailbox("INBOX").unwrap();
        inbox.modseq += 1;
        let at = inbox.modseq;
        inbox.vanished.extend([(1, at), (2, at), (3, at)]);
    }
    let got = receive(&mut s);
    assert_eq!(got["accounts"][0]["inStep"]["removed"], 0, "{got}");
    assert_eq!(count(&s, "Inbox"), 2);
    done(s, dir);
}

#[test]
fn where_each_folder_was_left_is_kept_in_the_settings_and_nonsense_is_dropped() {
    use crate::accounts::{Received, Step};
    let mut r = Received::default();
    r.stepped(Step { raw: "INBOX".into(), uid_validity: 7, modseq: 9_007_199_254_740_993, exists: 3, uid_next: 4 });
    r.stepped(Step { raw: "INBOX".into(), uid_validity: 7, modseq: 9_007_199_254_740_995, exists: 3, uid_next: 4 });
    let json = r.to_json();
    assert_eq!(json["steps"][0]["modseq"], "9007199254740995", "a mod-sequence past what a JSON number holds is kept whole");
    let back = Received::from_json(Some(&json));
    assert_eq!(back.steps, r.steps);
    for bad in [
        json!({"raw": "INBOX", "uidValidity": 7, "modseq": "0"}),
        json!({"raw": "INBOX", "uidValidity": 0, "modseq": "5"}),
        json!({"raw": "", "uidValidity": 7, "modseq": "5"}),
        json!({"raw": "INBOX", "uidValidity": 7, "modseq": "9223372036854775808"}),
        json!({"raw": "INBOX", "uidValidity": 7, "modseq": 5}),
    ] {
        let r = Received::from_json(Some(&json!({ "steps": [bad] })));
        assert!(r.steps.is_empty(), "{bad} was taken: {:?}", r.steps);
    }
}

/// Ask the session as the host does until `ok`, for no more than a while.
fn until(s: &mut Session, what: &str, mut ok: impl FnMut(&mut Session) -> bool) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        s.send_receive_due(0);
        s.poll();
        if ok(s) {
            return;
        }
        assert!(std::time::Instant::now() < end, "waited too long for {what}: {:?}", s.watching());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn logouts(fake: &Fake) -> usize {
    fake.state().heard.iter().filter(|l| l.ends_with("LOGOUT")).count()
}

#[test]
fn new_mail_is_received_the_moment_the_server_says_it_came() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let fake = Fake::default();
    fake.state().imap_caps.push("IDLE".into());
    fake.state().mailbox("INBOX").unwrap().add(&message("before"), &[]);
    let (mut s, dir) = account(&fake, "idle");
    s.send_receive_min = 0;
    receive(&mut s);
    assert_eq!(count(&s, "Inbox"), 1);
    let woken = Arc::new(AtomicUsize::new(0));
    let theirs = Arc::clone(&woken);
    s.waker = Some(Arc::new(move || {
        theirs.fetch_add(1, Ordering::Relaxed);
    }));
    until(&mut s, "the Inbox to be watched", |s| s.watching().first().is_some_and(|(_, said)| said.starts_with("Inbox watched")));
    while fake.state().idling == 0 {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Looked at, not opened for changing: nothing on the server changes for being watched.
    assert!(fake.state().heard.iter().any(|l| l.ends_with("EXAMINE \"INBOX\"")));
    assert_eq!(s.watching()[0].0, "pat@example.test");

    fake.state().mailbox("INBOX").unwrap().add(&message("as it comes"), &[]);
    until(&mut s, "the new message", |s| count(s, "Inbox") == 2);
    assert!(woken.load(Ordering::Relaxed) > 0, "the host was not woken");
    assert!(s.arrived.list.iter().any(|a| a.subject == "as it comes"), "{:?}", s.arrived.list);

    // A flag changed elsewhere comes too, with no Send/Receive asked for.
    fake.state().mailbox("INBOX").unwrap().set_flags(1, &["\\Flagged"]);
    until(&mut s, "the flag", |s| {
        pick(s, "Inbox", "before");
        s.open.as_ref().is_some_and(|m| m.flag() == openmailclient_model::Flag::Flagged)
    });

    // Turned off, the watch signs out and none is begun again.
    let before = logouts(&fake);
    let got = s.run("mail.newMailAsItComes", &json!({ "on": false })).unwrap();
    assert_eq!(got, json!({ "on": false, "watching": [] }));
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while logouts(&fake) == before {
        assert!(std::time::Instant::now() < end, "the watch did not sign out");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    s.send_receive_due(0);
    assert!(s.watching().is_empty());
    assert_eq!(s.settings(None)["newMailAsItComes"], false);
    done(s, dir);
}

#[test]
fn a_server_without_idle_is_received_from_on_the_timer_and_a_wrong_password_is_not_tried_again_soon() {
    let fake = Fake::default();
    let (mut s, dir) = account(&fake, "no-idle");
    s.send_receive_min = 0;
    until(&mut s, "the server to be found without IDLE", |s| s.watching().first().is_some_and(|(_, said)| said.contains("on the timer")));
    assert_eq!(fake.state().idling, 0);
    assert!(s.jobs.is_empty());
    done(s, dir);

    let fake = Fake::default();
    fake.state().imap_caps.push("IDLE".into());
    fake.state().password = "another".into();
    let (mut s, dir) = account(&fake, "idle-refused");
    s.send_receive_min = 0;
    until(&mut s, "the sign-in to be refused", |s| s.watching().first().is_some_and(|(_, said)| said.contains("tried again in 30 minutes")));
    assert_eq!(fake.state().idling, 0);
    done(s, dir);
}

#[test]
fn whether_new_mail_comes_as_it_arrives_is_kept_in_the_settings() {
    let dir = std::env::temp_dir().join(format!("omc-as-it-comes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    let mut s = Session::new();
    assert!(s.new_mail_as_it_comes);
    assert_eq!(s.run("mail.newMailAsItComes", &Value::Null).unwrap()["on"], false);
    assert!(s.run("mail.newMailAsItComes", &json!({ "on": "yes" })).is_err());
    s.run("settings.save", &json!({ "path": path.display().to_string() })).unwrap();
    let mut again = Session::new();
    again.run("settings.load", &json!({ "path": path.display().to_string() })).unwrap();
    assert!(!again.new_mail_as_it_comes);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A folder of the fake server, numbered as one of its own, holding messages.
fn server_folder(fake: &Fake, name: &str, uid_validity: u32, subjects: &[&str]) {
    let mut m = Mailbox::new(name, &[]);
    m.uid_validity = uid_validity;
    for s in subjects {
        m.add(&message(s), &[]);
    }
    fake.state().mailboxes.push(m);
}

/// Rename a folder on the fake server, with the folders inside it, as another program would.
fn rename_there(fake: &Fake, from: &str, to: &str) {
    let mut state = fake.state();
    let inside = format!("{from}/");
    for m in state.mailboxes.iter_mut() {
        if m.name == from {
            m.name = to.to_string();
        } else if let Some(rest) = m.name.strip_prefix(&inside) {
            m.name = format!("{to}/{rest}");
        }
    }
}

fn parent_of(s: &Session, name: &str) -> Option<String> {
    let f = s.folders[1].iter().find(|f| f.name == name)?;
    s.folders[1].iter().find(|p| Some(p.id) == f.parent).map(|p| p.name.clone())
}

fn names_there(fake: &Fake) -> Vec<String> {
    let mut all: Vec<String> = fake.state().mailboxes.iter().map(|m| m.name.clone()).collect();
    all.sort();
    all
}

#[test]
fn a_folder_renamed_or_moved_on_the_server_is_renamed_or_moved_here_and_nothing_comes_twice() {
    let fake = Fake::default();
    server_folder(&fake, "Projects", 200, &["plan"]);
    server_folder(&fake, "Projects/Old", 201, &["old plan"]);
    server_folder(&fake, "Clients", 202, &[]);
    let (mut s, dir) = account(&fake, "server-rename");
    assert_eq!(receive(&mut s)["received"], 2, "{}", s.status);
    assert_eq!(parent_of(&s, "Old").as_deref(), Some("Projects"));

    rename_there(&fake, "Projects", "Work");
    let got = receive(&mut s);
    assert_eq!(got["received"], 0, "the folder's mail is not taken down again: {got}");
    assert_eq!(got["accounts"][0]["inStep"]["renamed"], json!([{ "from": "Projects", "to": "Work" }]), "{got}");
    assert!(!s.folders[1].iter().any(|f| f.name == "Projects"), "{:?}", s.folders[1]);
    assert_eq!((count(&s, "Work"), count(&s, "Old")), (1, 1));
    assert_eq!(parent_of(&s, "Old").as_deref(), Some("Work"));
    let id = folder(&s, "Work");
    assert!(s.accounts[0].received.folders.iter().any(|m| m.raw == "Work" && m.here == id.0), "{:?}", s.accounts[0].received.folders);

    // The messages know their place by the new name: a change goes there, and one made there
    // comes here.
    pick(&mut s, "Work", "plan");
    s.run("mail.markRead", &Value::Null).unwrap();
    fake.state().mailbox("Work/Old").unwrap().messages[0].flags.push("\\Flagged".into());
    receive(&mut s);
    assert_eq!(flags(&fake, "Work", "plan"), ["\\Seen"], "{}", s.status);
    pick(&mut s, "Old", "old plan");
    assert_eq!(s.open.as_ref().unwrap().flag(), openmailclient_model::Flag::Flagged);

    // Moved into another folder there: moved here too.
    rename_there(&fake, "Work/Old", "Clients/Old");
    let got = receive(&mut s);
    assert_eq!(got["received"], 0, "{got}");
    assert_eq!(parent_of(&s, "Old").as_deref(), Some("Clients"));
    assert_eq!(count(&s, "Old"), 1);
    done(s, dir);
}

#[test]
fn a_folder_renamed_moved_and_deleted_here_is_renamed_moved_and_deleted_there() {
    let fake = Fake::default();
    server_folder(&fake, "Projects", 200, &["plan"]);
    server_folder(&fake, "Clients", 201, &[]);
    let (mut s, dir) = account(&fake, "local-rename");
    receive(&mut s);

    s.select_folder(1, folder(&s, "Projects")).unwrap();
    s.run("folder.rename", &json!({ "name": "Work" })).unwrap();
    let got = receive(&mut s);
    assert_eq!(names_there(&fake), ["Clients", "Drafts", "INBOX", "Sent", "Trash", "Work"], "{got}");
    assert_eq!((got["received"].as_u64(), on_server(&fake, "Work")), (Some(0), 1), "{got}");
    assert_eq!(count(&s, "Work"), 1);

    s.select_folder(1, folder(&s, "Work")).unwrap();
    s.run("folder.move", &json!({ "toId": folder(&s, "Clients").0 })).unwrap();
    receive(&mut s);
    assert!(names_there(&fake).contains(&"Clients/Work".to_string()), "{:?} {}", names_there(&fake), s.status);
    pick(&mut s, "Work", "plan");
    s.run("mail.markRead", &Value::Null).unwrap();
    receive(&mut s);
    assert_eq!(flags(&fake, "Clients/Work", "plan"), ["\\Seen"], "the message's place went with its folder");

    // Deleted, it is put with the deleted mail, here and there.
    s.select_folder(1, folder(&s, "Work")).unwrap();
    s.run("folder.delete", &Value::Null).unwrap();
    assert_eq!(parent_of(&s, "Work").as_deref(), Some("Deleted Items"));
    receive(&mut s);
    assert!(names_there(&fake).contains(&"Trash/Work".to_string()), "{:?} {}", names_there(&fake), s.status);
    assert_eq!(count(&s, "Work"), 1, "nothing came twice");

    // Deleted for good here: from the server as well.
    s.select_folder(1, folder(&s, "Work")).unwrap();
    s.run("folder.delete", &Value::Null).unwrap();
    assert_eq!(s.accounts[0].received.folders_gone.len(), 1);
    let got = receive(&mut s);
    assert_eq!(got["accounts"][0]["inStep"]["foldersDeleted"], json!(["Trash/Work"]), "{got}");
    assert!(!names_there(&fake).iter().any(|n| n.contains("Work")), "{:?}", names_there(&fake));
    assert!(s.accounts[0].received.folders_gone.is_empty());
    done(s, dir);
}

#[test]
fn a_folder_the_server_no_longer_has_is_kept_here_with_the_deleted_mail_and_not_made_again() {
    let fake = Fake::default();
    server_folder(&fake, "Projects", 200, &["plan"]);
    let (mut s, dir) = account(&fake, "server-delete");
    receive(&mut s);
    fake.state().mailboxes.retain(|m| m.name != "Projects");
    let got = receive(&mut s);
    assert_eq!(got["accounts"][0]["inStep"]["keptHere"], 1, "{got}");
    assert_eq!(parent_of(&s, "Projects").as_deref(), Some("Deleted Items"));
    assert_eq!(count(&s, "Projects"), 1, "what it held is kept");
    receive(&mut s);
    assert!(!names_there(&fake).iter().any(|n| n.contains("Projects")), "{:?}", names_there(&fake));
    assert_eq!(count(&s, "Projects"), 1);
    done(s, dir);
}

#[test]
fn a_folder_made_anew_there_by_the_name_of_one_deleted_here_stays() {
    let fake = Fake::default();
    server_folder(&fake, "Projects", 200, &["plan"]);
    let (mut s, dir) = account(&fake, "only-asked");
    receive(&mut s);
    s.select_folder(1, folder(&s, "Projects")).unwrap();
    s.run("folder.delete", &json!({ "permanently": true })).unwrap();
    fake.state().mailboxes.retain(|m| m.name != "Projects");
    server_folder(&fake, "Projects", 300, &["new plan"]);
    let got = receive(&mut s);
    assert!(names_there(&fake).contains(&"Projects".to_string()), "{got}");
    assert!(s.accounts[0].received.folders_gone.is_empty(), "forgotten once it cannot be done");
    assert_eq!(count(&s, "Projects"), 1, "the new folder's mail comes here: {got}");
    done(s, dir);
}

#[test]
fn a_folder_made_here_is_made_on_the_server_but_not_one_with_the_deleted_mail() {
    let fake = Fake::default();
    let (mut s, dir) = account(&fake, "made-here");
    receive(&mut s);
    // The server's top is where the Inbox stands.
    let bin = folder(&s, "Deleted Items");
    match s.folders[1].iter().find(|f| f.kind == FolderKind::Inbox).and_then(|f| f.parent) {
        Some(top) => s.stores[1].create_folder(top, "Ideas").unwrap(),
        None => s.stores[1].create_top_folder("Ideas").unwrap(),
    };
    s.stores[1].create_folder(bin, "Thrown away").unwrap();
    s.refresh_folders(1);
    let ideas = folder(&s, "Ideas");
    s.stores[1].create_folder(ideas, "Big").unwrap();
    s.refresh_folders(1);
    let got = receive(&mut s);
    assert_eq!(got["accounts"][0]["inStep"]["created"], json!(["Ideas", "Ideas/Big"]), "{got}");
    assert!(!names_there(&fake).iter().any(|n| n.contains("Thrown")), "{:?}", names_there(&fake));
    // Once there, they are not made again.
    let got = receive(&mut s);
    assert_eq!(got["accounts"][0]["inStep"]["created"], json!([]), "{got}");
    done(s, dir);
}

#[test]
fn where_the_server_gives_folders_ids_a_rename_is_followed_by_the_id() {
    let fake = Fake::default();
    fake.state().imap_caps.push("OBJECTID".into());
    // The same UIDVALIDITY in both, as servers may give: only the id tells them apart.
    server_folder(&fake, "Projects", 1, &["plan"]);
    server_folder(&fake, "Clients", 1, &["client"]);
    let (mut s, dir) = account(&fake, "ids");
    receive(&mut s);
    assert!(s.accounts[0].received.folders.iter().any(|m| m.raw == "Projects" && !m.mailbox_id.is_empty()), "{:?}", s.accounts[0].received.folders);
    rename_there(&fake, "Projects", "Work");
    let got = receive(&mut s);
    assert_eq!(got["received"], 0, "{got}");
    assert_eq!((count(&s, "Work"), count(&s, "Clients")), (1, 1));
    assert!(fake.state().heard.iter().any(|c| c.contains("MAILBOXID")));
    done(s, dir);
}

#[test]
fn where_two_folders_cannot_be_told_apart_nothing_is_followed() {
    let fake = Fake::default();
    server_folder(&fake, "Projects", 7, &["plan"]);
    let (mut s, dir) = account(&fake, "ambiguous");
    receive(&mut s);
    rename_there(&fake, "Projects", "Work");
    server_folder(&fake, "Other", 7, &[]);
    let got = receive(&mut s);
    assert_eq!(got["accounts"][0]["inStep"]["renamed"], json!([]), "{got}");
    assert!(s.folders[1].iter().any(|f| f.name == "Projects"), "kept as it was: {:?}", s.folders[1]);
    assert!(names_there(&fake).contains(&"Work".to_string()), "nothing done there: {:?}", names_there(&fake));
    done(s, dir);
}

#[test]
fn what_is_kept_of_folders_round_trips_and_nonsense_is_dropped() {
    use crate::accounts::{GoneFolder, Mark, Received};
    let r = Received {
        folders: vec![Mark {
            raw: "Work".into(),
            uid_validity: 5,
            last_uid: 9,
            here: 77,
            agreed: Some((Some(3), "Work".into())),
            mailbox_id: "F12-ab_c".into(),
        }],
        folders_gone: vec![GoneFolder { raw: "Trash/Old".into(), uid_validity: 4 }],
        ..Received::default()
    };
    assert_eq!(Received::from_json(Some(&r.to_json())), r);
    let top = Received { folders: vec![Mark { raw: "A".into(), agreed: Some((None, "A".into())), ..Mark::default() }], ..Received::default() };
    assert_eq!(Received::from_json(Some(&top.to_json())), top, "a folder at the very top keeps having no parent");
    let bad = json!({
        "folders": [
            { "raw": "A", "mailboxId": "has spaces", "agreedName": "", "here": -4 },
            { "raw": "B", "mailboxId": "x".repeat(256), "agreedName": 5, "agreedParent": "3" },
        ],
        "foldersGone": [{ "raw": "INBOX", "uidValidity": 3 }, { "raw": "" }, { "raw": 4 }, { "raw": "D", "uidValidity": -1 }, { "raw": "D" }],
    });
    let r = Received::from_json(Some(&bad));
    assert!(r.folders.iter().all(|m| m.mailbox_id.is_empty() && m.agreed.is_none() && m.here == 0), "{:?}", r.folders);
    assert_eq!(r.folders_gone, [GoneFolder { raw: "D".into(), uid_validity: 0 }]);
    // A rename takes the folders inside with it, and what was kept by the new name goes.
    let mut r = Received {
        folders: vec![
            Mark { raw: "A".into(), ..Mark::default() },
            Mark { raw: "A/B".into(), ..Mark::default() },
            Mark { raw: "AB".into(), ..Mark::default() },
            Mark { raw: "Z".into(), last_uid: 4, ..Mark::default() },
        ],
        ..Received::default()
    };
    r.renamed("A", "Z", Some('/'));
    let raws: Vec<&str> = r.folders.iter().map(|m| m.raw.as_str()).collect();
    assert_eq!(raws, ["Z", "Z/B", "AB"]);
}

fn write(s: &mut Session, fields: &[(&str, &str)]) {
    for (field, value) in fields {
        s.run("compose.setField", &json!({ "field": field, "value": value })).unwrap();
    }
}

fn is_draft(fake: &Fake, subject: &str) -> bool {
    flags(fake, "Drafts", subject).iter().any(|f| f == "\\Draft")
}

#[test]
fn a_draft_is_kept_on_the_server_as_it_is_written_and_goes_from_there_when_sent() {
    let fake = Fake::default();
    let (mut s, dir) = account(&fake, "drafts");
    receive(&mut s);
    s.run("mail.new", &Value::Null).unwrap();
    write(&mut s, &[("to", "june@example.test"), ("subject", "Plan"), ("body", "First go.")]);
    s.run("compose.saveDraft", &Value::Null).unwrap();
    assert!(s.compose.is_some(), "saved, it stays open to be written on");
    assert_eq!(count(&s, "Drafts"), 1, "in the account's own Drafts");
    receive(&mut s);
    assert_eq!(on_server(&fake, "Drafts"), 1, "{}", s.status);
    assert!(is_draft(&fake, "Plan"), "{:?}", flags(&fake, "Drafts", "Plan"));

    // Written on and saved again: it takes its own place, here and there.
    write(&mut s, &[("subject", "Plan B")]);
    s.run("compose.saveDraft", &Value::Null).unwrap();
    s.run("compose.saveDraft", &Value::Null).unwrap();
    assert_eq!(count(&s, "Drafts"), 1);
    receive(&mut s);
    assert_eq!(on_server(&fake, "Drafts"), 1, "{}", s.status);
    assert!(is_draft(&fake, "Plan B"));

    // Closed, opened again from Drafts and sent: out of Drafts here and there.
    s.run("compose.discard", &Value::Null).unwrap();
    assert!(s.compose.is_none() && s.set_aside.is_empty(), "nothing was written since it was saved");
    pick(&mut s, "Drafts", "Plan B");
    s.run("compose.editDraft", &Value::Null).unwrap();
    let d = s.compose.clone().unwrap();
    assert_eq!((d.to.as_str(), d.subject.as_str(), d.body.trim()), ("june@example.test", "Plan B", "First go."));
    s.run("compose.send", &json!({ "anyway": true })).unwrap();
    settle(&mut s);
    assert_eq!(count(&s, "Drafts"), 0);
    assert_eq!(fake.state().submitted.len(), 1);
    receive(&mut s);
    assert_eq!(on_server(&fake, "Drafts"), 0, "{}", s.status);
    done(s, dir);
}

#[test]
fn a_draft_written_elsewhere_comes_here_as_a_draft_to_be_written_on() {
    let fake = Fake::default();
    fake.state().mailbox("Drafts").unwrap().add(&message("Elsewhere"), &["\\Draft", "\\Seen"]);
    fake.state().mailbox("INBOX").unwrap().add(&message("Mail"), &[]);
    let (mut s, dir) = account(&fake, "drafts-there");
    receive(&mut s);
    pick(&mut s, "Inbox", "Mail");
    assert!(!s.is_enabled("compose.editDraft"), "only a draft opens to be written on");
    pick(&mut s, "Drafts", "Elsewhere");
    assert!(s.open.as_ref().unwrap().is_unsent());
    s.run("compose.editDraft", &Value::Null).unwrap();
    write(&mut s, &[("body", "Finished here.")]);
    s.run("compose.saveDraft", &Value::Null).unwrap();
    assert_eq!(count(&s, "Drafts"), 1);
    receive(&mut s);
    assert_eq!(on_server(&fake, "Drafts"), 1, "{}", s.status);
    let there = fake.state().mailbox("Drafts").unwrap().messages[0].clone();
    assert!(String::from_utf8_lossy(&there.data).contains("Finished here."));
    assert!(there.flags.iter().any(|f| f == "\\Draft"));
    // Nothing comes back twice.
    receive(&mut s);
    assert_eq!(count(&s, "Drafts"), 1);
    done(s, dir);
}

#[test]
fn a_server_without_a_drafts_folder_is_given_one() {
    let fake = Fake::default();
    fake.state().mailboxes.retain(|m| m.name != "Drafts");
    let (mut s, dir) = account(&fake, "no-drafts");
    receive(&mut s);
    s.run("mail.new", &Value::Null).unwrap();
    write(&mut s, &[("subject", "Note")]);
    s.run("compose.saveDraft", &Value::Null).unwrap();
    let got = receive(&mut s);
    assert_eq!(got["accounts"][0]["inStep"]["created"], json!(["Drafts"]), "{got}");
    assert!(is_draft(&fake, "Note"));
    done(s, dir);
}

#[test]
fn the_places_of_messages_are_read_apart_from_the_window_and_remembered_after() {
    let fake = Fake::default();
    for n in 0..5 {
        fake.state().mailbox("INBOX").unwrap().add(&message(&format!("Mail {n}")), &[]);
    }
    let (mut s, dir) = account(&fake, "apart");
    receive(&mut s);
    assert!(!s.stores[1].unsaved(), "the account's data file is saved by the receive");
    assert_eq!(count(&s, "Inbox"), 5);

    // As if the program were started again: nothing is remembered of the data file.
    s.places = Default::default();
    s.run("account.receive", &json!({ "wait": false })).unwrap();
    assert!(s.places.is_empty(), "the window read none of the data file's messages");
    settle(&mut s);
    assert!(s.places.len() >= 5, "what was read apart is remembered: {}", s.places.len());
    // Kept in step as before: nothing came twice and nothing went up again.
    assert_eq!(count(&s, "Inbox"), 5);
    assert_eq!(on_server(&fake, "INBOX"), 5);

    // Read and changed here meanwhile: the next receive takes the change up.
    pick(&mut s, "Inbox", "Mail 0");
    s.run("mail.flag", &Value::Null).unwrap();
    receive(&mut s);
    assert!(flags(&fake, "INBOX", "Mail 0").iter().any(|f| f == "\\Flagged"), "{}", s.status);
    done(s, dir);
}

#[test]
fn a_data_file_that_cannot_be_read_apart_is_not_kept_in_step_and_nothing_goes_from_the_server() {
    let fake = Fake::default();
    fake.state().mailbox("INBOX").unwrap().add(&message("Kept"), &[]);
    let (mut s, dir) = account(&fake, "apart-gone");
    receive(&mut s);
    assert!(!s.stores[1].unsaved(), "the account's data file is saved by the receive");
    let path = s.stores[1].path().unwrap().to_path_buf();

    // Another program seems to have changed the file, so it is not opened again apart.
    s.places = Default::default();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(later).unwrap();
    let _ = s.run("account.receive", &json!({ "wait": false }));
    settle(&mut s);
    assert!(s.status.contains("could not be read"), "{}", s.status);
    assert_eq!(on_server(&fake, "INBOX"), 1, "nothing went from the server");
    done(s, dir);
}

fn subjects(s: &mut Session) -> Vec<String> {
    s.run("folder.select", &json!({ "store": 1, "folder": "Inbox" })).unwrap();
    s.rows.iter().map(|r| r.subject.clone()).collect()
}

#[test]
fn into_the_account_s_own_data_file_the_newest_come_first_and_a_stop_loses_nothing() {
    let fake = Fake::default();
    {
        let mut state = fake.state();
        let inbox = state.mailbox("INBOX").unwrap();
        for i in 0..450 {
            inbox.add(&message(&format!("m{i}")), &[]);
        }
    }
    let (mut s, dir) = account(&fake, "newest");
    s.run("account.receive", &json!({ "wait": false })).unwrap();
    let mut waited = 0;
    while count(&s, "Inbox") == 0 {
        s.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
        assert!(waited < 3000, "no part came: {}", s.status);
    }
    assert!(!s.jobs.is_empty(), "the first part came before the receive ended");
    let here = subjects(&mut s);
    assert!(here.iter().any(|t| t == "m449") && !here.iter().any(|t| t == "m0"), "the newest come first");
    // Stopped, as when the window is closed: what came is kept, and the rest comes next time.
    s.leave_jobs(std::time::Duration::from_secs(5));
    settle(&mut s);
    let kept = count(&s, "Inbox");
    assert!(kept > 0 && kept <= 450, "{kept}");
    receive(&mut s);
    assert_eq!(count(&s, "Inbox"), 450, "the rest came, none of it twice");
    let here = subjects(&mut s);
    for i in [0, 1, 224, 448, 449] {
        assert_eq!(here.iter().filter(|t| **t == format!("m{i}")).count(), 1, "m{i}");
    }
    // Then only what is new.
    fake.state().mailbox("INBOX").unwrap().add(&message("later"), &[]);
    assert_eq!(receive(&mut s)["received"], 1);
    assert_eq!(count(&s, "Inbox"), 451);
    done(s, dir);
}
