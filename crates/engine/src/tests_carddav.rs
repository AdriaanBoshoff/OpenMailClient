//! Contacts kept in step with a CardDAV server, against the in-process fake: the first sync,
//! what is made, changed and deleted on either side, a change on both, a group, a read-only
//! address book, what Send/Receive does and what an account keeps of it.

use crate::Session;
use openmailclient_formats::vcf::Card;
use openmailclient_net::fake::{DAV_BOOK_HOME, DavCalendar, Fake};
use serde_json::{Value, json};
use std::sync::Arc;

fn book() -> String {
    format!("{DAV_BOOK_HOME}contacts/")
}

fn card(uid: &str, given: &str, surname: &str, email: &str) -> String {
    format!(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:{uid}\r\nN:{surname};{given};;;\r\nFN:{given} {surname}\r\nEMAIL;TYPE=INTERNET:{email}\r\nEND:VCARD\r\n"
    )
}

/// A fake with one address book beside its calendar.
fn fake() -> Fake {
    let fake = Fake::default();
    fake.state().calendars.push(DavCalendar::book(&book(), "Contacts"));
    fake
}

/// A session with an IMAP account whose CardDAV server is the fake's; its data file is store 1.
fn account(fake: &Fake, name: &str) -> (Session, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("omc-carddav-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session::new();
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.zone = openmailclient_model::zone::Zone::fixed(0);
    s.connector = Some(Arc::new(fake.clone()));
    s.account_dir = Some(dir.join("Accounts"));
    s.journal_dir = Some(dir.join("journals"));
    s.run(
        "account.add",
        &json!({ "email": "pat@example.test", "kind": "imap",
                 "incoming": { "host": "mail.example.test", "security": "starttls" },
                 "outgoing": { "host": "mail.example.test", "security": "starttls" },
                 "carddav": "https://dav.example.test/", "password": "not a real password" }),
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

fn sync(s: &mut Session) -> Value {
    let got = s.run("people.sync", &Value::Null).unwrap();
    settle(s);
    assert_eq!(got["ok"], json!(true), "{got}");
    got
}

fn books(got: &Value) -> Vec<Value> {
    got["accounts"][0]["addressBooks"].as_array().cloned().unwrap_or_default()
}

/// The account's own Contacts folder, shown.
fn show_account_contacts(s: &mut Session) {
    let listed = s.run("people.folders", &Value::Null).unwrap();
    let mine = listed["folders"].as_array().unwrap().iter().find(|f| f["store"] == json!(1) && f["name"] == json!("Contacts")).cloned().unwrap();
    s.run("people.open", &json!({ "store": 1, "folder": mine["folder"] })).unwrap();
}

fn names(s: &mut Session) -> Vec<String> {
    s.reload_people().unwrap();
    let mut all: Vec<String> = s.people.items.iter().map(|(_, c)| c.name()).collect();
    all.sort();
    all
}

fn id_of(s: &mut Session, name: &str) -> u64 {
    s.reload_people().unwrap();
    s.people.items.iter().find(|(_, c)| c.name() == name).map(|(id, _)| *id).unwrap_or_else(|| panic!("no {name}"))
}

fn on_server(fake: &Fake) -> Vec<String> {
    let mut state = fake.state();
    let b = state.dav_calendar(&book()).unwrap();
    let mut all: Vec<String> =
        b.objects.iter().filter_map(|o| o.data.lines().find_map(|l| l.strip_prefix("FN:")).map(|t| t.trim().to_string())).collect();
    all.sort();
    all
}

#[test]
fn the_first_sync_takes_each_address_book_down_into_a_contacts_folder() {
    let fake = fake();
    {
        let mut st = fake.state();
        st.dav_put(&book(), "ana.vcf", &card("ana-1", "Ana", "Silva", "ana@example.org"));
        st.calendars.push(DavCalendar::book(&format!("{DAV_BOOK_HOME}work/"), "Work"));
        st.dav_put(&format!("{DAV_BOOK_HOME}work/"), "ben.vcf", &card("ben-1", "Ben", "Okafor", "ben@example.org"));
    }
    let (mut s, dir) = account(&fake, "first");
    let got = sync(&mut s);
    let all = books(&got);
    assert_eq!(all.len(), 2, "only address books, never the calendar beside them: {got}");
    assert_eq!(all[0]["folder"], json!("Contacts"), "the first is the data file's own Contacts");
    assert_eq!(all[1]["folder"], json!("Work"));
    assert!(all.iter().all(|b| b["fetched"] == json!(1)), "{got}");
    show_account_contacts(&mut s);
    assert_eq!(names(&mut s), vec!["Ana Silva"]);
    let ana = id_of(&mut s, "Ana Silva");
    let Card::Contact(ana) = s.person(ana).unwrap() else { panic!("not a contact") };
    assert_eq!(ana.emails[0].address, "ana@example.org");
    assert_eq!(ana.uid, "ana-1", "known by the server's UID");

    // What was kept of the server goes into the settings, and comes back from them; the
    // calendar setting stays empty.
    let dav = s.accounts[0].carddav.clone().unwrap();
    assert_eq!(dav.calendars.len(), 2);
    assert!(s.accounts[0].caldav.is_none());
    let (back, ignored) = crate::accounts::from_settings(&json!([s.accounts[0].to_settings()]));
    assert_eq!(ignored, 0);
    assert_eq!(back[0].carddav, Some(dav));

    let again = sync(&mut s);
    assert!(books(&again).iter().all(|b| b["fetched"] == json!(0) && b["put"] == json!(0)), "{again}");
    done(s, dir);
}

#[test]
fn contacts_and_groups_made_changed_and_deleted_here_go_to_the_server() {
    let fake = fake();
    fake.state().dav_put(&book(), "ana.vcf", &card("ana-1", "Ana", "Silva", "ana@example.org"));
    let (mut s, dir) = account(&fake, "here");
    sync(&mut s);
    show_account_contacts(&mut s);

    s.run("people.new", &json!({ "given": "Chen", "surname": "Wei", "emails": ["chen@example.org"] })).unwrap();
    s.run("people.newGroup", &json!({ "name": "Book club", "members": ["chen@example.org"] })).unwrap();
    let got = sync(&mut s);
    assert_eq!(books(&got)[0]["put"], json!(2), "{got}");
    assert_eq!(on_server(&fake), vec!["Ana Silva", "Book club", "Chen Wei"]);
    {
        let mut st = fake.state();
        let b = st.dav_calendar(&book()).unwrap();
        assert!(b.objects.iter().all(|o| o.name.ends_with(".vcf")), "new ones are put as .vcf");
        let group = b.objects.iter().find(|o| o.data.contains("FN:Book club")).unwrap();
        assert!(group.data.contains("chen@example.org"), "a group goes with its members: {}", group.data);
    }

    let ana = id_of(&mut s, "Ana Silva");
    s.run("people.change", &json!({ "id": ana, "jobTitle": "Editor" })).unwrap();
    sync(&mut s);
    {
        let mut st = fake.state();
        let b = st.dav_calendar(&book()).unwrap();
        let o = b.objects.iter().find(|o| o.name == "ana.vcf").unwrap();
        assert!(o.data.contains("TITLE:Editor"), "the change went where the card was: {}", o.data);
        assert!(o.data.contains("UID:ana-1"));
    }

    let chen = id_of(&mut s, "Chen Wei");
    s.run("people.delete", &json!({ "id": chen })).unwrap();
    let got = sync(&mut s);
    assert_eq!(books(&got)[0]["deleted"], json!(1), "{got}");
    assert_eq!(on_server(&fake), vec!["Ana Silva", "Book club"]);
    let again = sync(&mut s);
    assert!(books(&again).iter().all(|b| b["put"] == json!(0) && b["deleted"] == json!(0)), "nothing goes twice: {again}");
    done(s, dir);
}

#[test]
fn what_changes_on_the_server_comes_here_and_a_change_on_both_keeps_both() {
    let fake = fake();
    {
        let mut st = fake.state();
        st.dav_put(&book(), "ana.vcf", &card("ana-1", "Ana", "Silva", "ana@example.org"));
        st.dav_put(&book(), "ben.vcf", &card("ben-1", "Ben", "Okafor", "ben@example.org"));
        st.dav_put(&book(), "dee.vcf", &card("dee-1", "Dee", "Park", "dee@example.org"));
    }
    let (mut s, dir) = account(&fake, "there");
    sync(&mut s);
    show_account_contacts(&mut s);
    assert_eq!(names(&mut s), vec!["Ana Silva", "Ben Okafor", "Dee Park"]);

    {
        let mut st = fake.state();
        st.dav_put(&book(), "ana.vcf", &card("ana-1", "Ana", "Silva-Reyes", "ana@example.org"));
        st.dav_remove(&book(), "ben.vcf");
        st.dav_put(&book(), "eve.vcf", &card("eve-1", "Eve", "Adams", "eve@example.org"));
        st.dav_put(&book(), "dee.vcf", &card("dee-1", "Dee", "Park-Server", "dee@example.org"));
    }
    // Dee changed here too, meanwhile.
    let dee = id_of(&mut s, "Dee Park");
    s.run("people.change", &json!({ "id": dee, "surname": "Park-Here" })).unwrap();
    let got = sync(&mut s);
    let b = &books(&got)[0];
    assert_eq!(b["removed"], json!(1), "{got}");
    assert_eq!(b["copies"], json!(1), "{got}");
    assert_eq!(names(&mut s), vec!["Ana Silva-Reyes", "Dee Park-Here", "Dee Park-Server", "Eve Adams"]);
    // Our copy goes to the server as a card of its own, with a UID of its own.
    sync(&mut s);
    assert_eq!(on_server(&fake), vec!["Ana Silva-Reyes", "Dee Park-Here", "Dee Park-Server", "Eve Adams"]);
    let uids: Vec<String> = fake
        .state()
        .dav_calendar(&book())
        .unwrap()
        .objects
        .iter()
        .filter_map(|o| o.data.lines().find_map(|l| l.strip_prefix("UID:")).map(str::to_string))
        .collect();
    let mut once = uids.clone();
    once.sort();
    once.dedup();
    assert_eq!(once.len(), uids.len(), "no UID twice: {uids:?}");
    done(s, dir);
}

#[test]
fn a_read_only_address_book_takes_nothing_and_send_receive_keeps_contacts_in_step() {
    let fake = fake();
    {
        let mut st = fake.state();
        st.dav_put(&book(), "ana.vcf", &card("ana-1", "Ana", "Silva", "ana@example.org"));
        st.dav_calendar(&book()).unwrap().read_only = true;
    }
    let (mut s, dir) = account(&fake, "readonly");
    sync(&mut s);
    show_account_contacts(&mut s);
    let ana = id_of(&mut s, "Ana Silva");
    s.run("people.change", &json!({ "id": ana, "jobTitle": "Editor" })).unwrap();
    let got = sync(&mut s);
    assert_eq!(books(&got)[0]["readOnly"], json!(true));
    assert_eq!(books(&got)[0]["notPut"], json!(1), "{got}");
    assert!(!fake.state().dav_calendar(&book()).unwrap().objects[0].data.contains("Editor"));

    // Send/Receive keeps contacts in step with the mail.
    fake.state().dav_calendar(&book()).unwrap().read_only = false;
    fake.state().dav_put(&book(), "eve.vcf", &card("eve-1", "Eve", "Adams", "eve@example.org"));
    let all = s.run("mail.sendReceive", &Value::Null).unwrap();
    settle(&mut s);
    assert!(all["contacts"].is_object(), "{all}");
    assert!(names(&mut s).contains(&"Eve Adams".to_string()));
    done(s, dir);
}

#[test]
fn a_carddav_server_is_set_and_taken_away_and_junk_is_refused() {
    let fake = fake();
    let (mut s, dir) = account(&fake, "settings");
    let id = s.accounts[0].id.clone();
    for bad in [json!("http://dav.example.test/"), json!("ftp://x"), json!(7), json!({ "nope": 1 }), json!("https://user:pw@dav.example.test/")] {
        assert!(s.run("account.edit", &json!({ "id": id, "carddav": bad })).is_err(), "{bad} is refused");
    }
    sync(&mut s);
    assert_eq!(s.accounts[0].carddav.as_ref().unwrap().calendars.len(), 1);
    // Another server starts over; none stops the sync.
    s.run("account.edit", &json!({ "id": id, "carddav": "https://other.example.test/dav/" })).unwrap();
    assert!(s.accounts[0].carddav.as_ref().unwrap().calendars.is_empty());
    s.run("account.edit", &json!({ "id": id, "carddav": "" })).unwrap();
    assert!(s.accounts[0].carddav.is_none());
    assert!(s.run("people.sync", &json!({ "id": id })).is_err(), "no server, no sync");
    assert!(s.run("people.sync", &json!({ "id": 7 })).is_err());
    done(s, dir);
}
