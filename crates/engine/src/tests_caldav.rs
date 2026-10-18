//! Calendars kept in step with a CalDAV server, against the in-process fake: the first sync,
//! what is made, changed and deleted on either side, a change on both, a read-only calendar,
//! a sync token the server forgot or never kept, and what an account keeps of it.

use crate::Session;
use crate::cmd::caldav::DavPlace;
use openmailclient_model::{FolderKind, ItemId};
use openmailclient_net::fake::{DAV_HOME, DavCalendar, Fake};
use serde_json::{Value, json};
use std::sync::Arc;

const PERSONAL: &str = "/dav/calendars/pat/personal/";

fn event(uid: &str, summary: &str, day: u32) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Elsewhere//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20261001T000000Z\r\nDTSTART:202610{day:02}T090000Z\r\nDTEND:202610{day:02}T100000Z\r\nSUMMARY:{summary}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

/// A session with an IMAP account whose CalDAV server is the fake's; its data file is store 1.
fn account(fake: &Fake, name: &str) -> (Session, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("omc-caldav-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session::new();
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s.tz_offset_min = 0;
    s.connector = Some(Arc::new(fake.clone()));
    s.account_dir = Some(dir.join("Accounts"));
    s.journal_dir = Some(dir.join("journals"));
    s.run(
        "account.add",
        &json!({ "email": "pat@example.test", "kind": "imap",
                 "incoming": { "host": "mail.example.test", "security": "starttls" },
                 "outgoing": { "host": "mail.example.test", "security": "starttls" },
                 "caldav": "https://dav.example.test/", "password": "not a real password" }),
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
    let got = s.run("calendar.sync", &Value::Null).unwrap();
    settle(s);
    assert_eq!(got["ok"], json!(true), "{got}");
    got
}

fn calendars(got: &Value) -> Vec<Value> {
    got["accounts"][0]["calendars"].as_array().cloned().unwrap_or_default()
}

/// The account's Calendar, chosen, showing October 2026.
fn show_account_calendar(s: &mut Session) {
    let listed = s.run("calendar.calendars", &Value::Null).unwrap();
    let mine = listed["calendars"].as_array().unwrap().iter().find(|c| c["store"] == json!(1) && c["name"] == json!("Calendar")).cloned().unwrap();
    s.run("calendar.open", &json!({ "store": 1, "folder": mine["folder"] })).unwrap();
    s.run("calendar.view", &json!({ "view": "month" })).unwrap();
    s.run("calendar.goTo", &json!({ "date": "2026-10-20" })).unwrap();
}

fn subjects(s: &mut Session) -> Vec<String> {
    s.reload_calendar().unwrap();
    let mut all: Vec<String> = s.calendar.items.iter().map(|(_, a)| a.subject.clone()).collect();
    all.sort();
    all
}

fn id_of(s: &mut Session, subject: &str) -> u64 {
    s.reload_calendar().unwrap();
    s.calendar.items.iter().find(|(_, a)| a.subject == subject).map(|(id, _)| *id).unwrap_or_else(|| panic!("no {subject}"))
}

fn on_server(fake: &Fake, href: &str) -> Vec<String> {
    let mut state = fake.state();
    let cal = state.dav_calendar(href).unwrap();
    let mut all: Vec<String> =
        cal.objects.iter().filter_map(|o| o.data.lines().find_map(|l| l.strip_prefix("SUMMARY:")).map(|t| t.trim().to_string())).collect();
    all.sort();
    all
}

#[test]
fn the_first_sync_takes_every_calendar_down_into_a_folder_of_its_own() {
    let fake = Fake::default();
    {
        let mut st = fake.state();
        st.dav_put(PERSONAL, "standup.ics", &event("standup-1", "Standup", 20));
        let mut work = DavCalendar::new(&format!("{DAV_HOME}work/"), "Work");
        work.color = "#CC3333FF".into();
        st.calendars.push(work);
        st.dav_put(&format!("{DAV_HOME}work/"), "review.ics", &event("review-1", "Design review", 21));
    }
    let (mut s, dir) = account(&fake, "first");
    let got = sync(&mut s);
    let cals = calendars(&got);
    assert_eq!(cals.len(), 2, "{got}");
    assert_eq!(cals[0]["folder"], json!("Calendar"), "the first is the data file's own Calendar");
    assert_eq!(cals[1]["folder"], json!("Work"), "the others beside it, by the server's names");
    assert!(cals.iter().all(|c| c["fetched"] == json!(1)), "{got}");

    // Both are listed with the account and their colours; the Calendar is shown as chosen.
    let listed = s.run("calendar.calendars", &Value::Null).unwrap();
    let mine: Vec<&Value> = listed["calendars"].as_array().unwrap().iter().filter(|c| c["store"] == json!(1)).collect();
    assert_eq!(mine.len(), 2, "{listed}");
    assert!(mine.iter().all(|c| c["account"] == json!(s.accounts[0].id)), "{listed}");
    assert_eq!(mine.iter().find(|c| c["name"] == json!("Work")).unwrap()["color"], json!("#CC3333"));
    show_account_calendar(&mut s);
    assert_eq!(subjects(&mut s), vec!["Standup"]);
    let work = mine.iter().find(|c| c["name"] == json!("Work")).unwrap()["folder"].clone();
    s.run("calendar.open", &json!({ "store": 1, "folder": work })).unwrap();
    assert_eq!(subjects(&mut s), vec!["Design review"]);

    // What was kept of the server goes into the settings, and comes back from them.
    let dav = s.accounts[0].caldav.clone().unwrap();
    assert_eq!(dav.calendars.len(), 2);
    assert!(dav.calendars.iter().all(|c| !c.token.is_empty()), "each calendar's token is kept");
    let (back, ignored) = crate::accounts::from_settings(&json!([s.accounts[0].to_settings()]));
    assert_eq!(ignored, 0);
    assert_eq!(back[0].caldav, Some(dav));

    // A sync with nothing changed takes nothing down again.
    let again = sync(&mut s);
    assert!(calendars(&again).iter().all(|c| c["fetched"] == json!(0) && c["put"] == json!(0)), "{again}");
    done(s, dir);
}

#[test]
fn what_is_made_changed_and_deleted_here_goes_to_the_server() {
    let fake = Fake::default();
    fake.state().dav_put(PERSONAL, "standup.ics", &event("standup-1", "Standup", 20));
    let (mut s, dir) = account(&fake, "here");
    sync(&mut s);
    show_account_calendar(&mut s);

    s.run("calendar.new", &json!({ "subject": "Lunch", "start": "2026-10-21 12:00" })).unwrap();
    let got = sync(&mut s);
    assert_eq!(calendars(&got)[0]["put"], json!(1), "{got}");
    assert_eq!(on_server(&fake, PERSONAL), vec!["Lunch", "Standup"]);

    let standup = id_of(&mut s, "Standup");
    let etag_before = fake.state().dav_calendar(PERSONAL).unwrap().objects.iter().find(|o| o.name == "standup.ics").unwrap().etag.clone();
    s.run("calendar.change", &json!({ "id": standup, "subject": "Standup, moved" })).unwrap();
    sync(&mut s);
    assert_eq!(on_server(&fake, PERSONAL), vec!["Lunch", "Standup\\, moved"], "the change went where the event was");
    let etag_after = fake.state().dav_calendar(PERSONAL).unwrap().objects.iter().find(|o| o.name == "standup.ics").unwrap().etag.clone();
    assert_ne!(etag_before, etag_after);

    let lunch = id_of(&mut s, "Lunch");
    s.run("calendar.delete", &json!({ "id": lunch })).unwrap();
    let got = sync(&mut s);
    assert_eq!(calendars(&got)[0]["deleted"], json!(1), "{got}");
    assert_eq!(on_server(&fake, PERSONAL), vec!["Standup\\, moved"]);
    // What is in Deleted Items is known on no server any more.
    let deleted = s.find_folder(1, FolderKind::Deleted).unwrap();
    let st = s.stores.get_mut(1).unwrap();
    for row in st.list(deleted).unwrap() {
        assert!(DavPlace::of(&st.load(deleted, ItemId(row.id)).unwrap()).is_none());
    }
    // Nothing went twice.
    let again = sync(&mut s);
    assert!(calendars(&again).iter().all(|c| c["put"] == json!(0) && c["deleted"] == json!(0)), "{again}");
    done(s, dir);
}

#[test]
fn what_changes_on_the_server_comes_here_and_a_change_on_both_keeps_both() {
    let fake = Fake::default();
    {
        let mut st = fake.state();
        st.dav_put(PERSONAL, "standup.ics", &event("standup-1", "Standup", 20));
        st.dav_put(PERSONAL, "lunch.ics", &event("lunch-1", "Lunch", 21));
        st.dav_put(PERSONAL, "retro.ics", &event("retro-1", "Retro", 22));
    }
    let (mut s, dir) = account(&fake, "there");
    sync(&mut s);
    show_account_calendar(&mut s);
    assert_eq!(subjects(&mut s), vec!["Lunch", "Retro", "Standup"]);

    {
        let mut st = fake.state();
        st.dav_put(PERSONAL, "standup.ics", &event("standup-1", "Standup at ten", 20));
        st.dav_remove(PERSONAL, "lunch.ics");
        st.dav_put(PERSONAL, "new.ics", &event("new-1", "Planning", 23));
        st.dav_put(PERSONAL, "retro.ics", &event("retro-1", "Retro, as the server has it", 22));
    }
    // Retro changed here too, meanwhile.
    let retro = id_of(&mut s, "Retro");
    s.run("calendar.change", &json!({ "id": retro, "subject": "Retro, as we have it" })).unwrap();
    let got = sync(&mut s);
    let c = &calendars(&got)[0];
    assert_eq!(c["removed"], json!(1), "{got}");
    assert_eq!(c["copies"], json!(1), "{got}");
    assert_eq!(subjects(&mut s), vec!["Planning", "Retro, as the server has it", "Retro, as we have it", "Standup at ten"]);
    // What went from the server is in Deleted Items here.
    let deleted = s.find_folder(1, FolderKind::Deleted).unwrap();
    assert!(s.folders[1].iter().any(|f| f.id == deleted && f.total == 1), "{:?}", s.folders[1]);
    // Our copy of the changed one goes to the server as an event of its own.
    sync(&mut s);
    assert_eq!(
        on_server(&fake, PERSONAL),
        vec!["Planning", "Retro, as the server has it", "Retro\\, as we have it", "Standup at ten"],
        "the server's own as it wrote it, ours as we write it"
    );
    done(s, dir);
}

#[test]
fn a_read_only_calendar_takes_nothing_and_what_is_deleted_here_comes_back() {
    let fake = Fake::default();
    {
        let mut st = fake.state();
        st.dav_put(PERSONAL, "holiday.ics", &event("holiday-1", "Holiday", 20));
        st.dav_calendar(PERSONAL).unwrap().read_only = true;
    }
    let (mut s, dir) = account(&fake, "readonly");
    sync(&mut s);
    show_account_calendar(&mut s);
    let listed = s.run("calendar.calendars", &Value::Null).unwrap();
    assert!(listed["calendars"].as_array().unwrap().iter().any(|c| c["store"] == json!(1) && c["readOnly"] == json!(true)), "{listed}");

    let holiday = id_of(&mut s, "Holiday");
    s.run("calendar.change", &json!({ "id": holiday, "location": "Home" })).unwrap();
    s.run("calendar.new", &json!({ "subject": "Mine alone", "start": "2026-10-22 12:00" })).unwrap();
    let got = sync(&mut s);
    assert_eq!(calendars(&got)[0]["notPut"], json!(2), "{got}");
    assert_eq!(calendars(&got)[0]["put"], json!(0), "{got}");
    assert_eq!(on_server(&fake, PERSONAL), vec!["Holiday"]);

    let holiday = id_of(&mut s, "Holiday");
    s.run("calendar.delete", &json!({ "id": holiday })).unwrap();
    sync(&mut s);
    assert_eq!(on_server(&fake, PERSONAL), vec!["Holiday"]);
    assert!(subjects(&mut s).contains(&"Holiday".to_string()), "a read-only calendar's event comes back");
    done(s, dir);
}

#[test]
fn a_token_the_server_forgot_or_never_kept_reads_the_whole_list() {
    let fake = Fake::default();
    {
        let mut st = fake.state();
        st.dav_put(PERSONAL, "standup.ics", &event("standup-1", "Standup", 20));
        st.dav_put(PERSONAL, "lunch.ics", &event("lunch-1", "Lunch", 21));
    }
    let (mut s, dir) = account(&fake, "token");
    sync(&mut s);
    show_account_calendar(&mut s);
    {
        let mut st = fake.state();
        st.dav_remove(PERSONAL, "lunch.ics");
        st.dav_put(PERSONAL, "standup.ics", &event("standup-1", "Standup later", 20));
        st.dav_forget_before = u64::MAX;
    }
    let got = sync(&mut s);
    assert_eq!(calendars(&got)[0]["removed"], json!(1), "{got}");
    assert_eq!(subjects(&mut s), vec!["Standup later"]);

    // A server that keeps no tokens at all: its list is read each time.
    {
        let mut st = fake.state();
        st.dav_sync = false;
        st.dav_forget_before = 0;
        st.dav_put(PERSONAL, "retro.ics", &event("retro-1", "Retro", 22));
    }
    sync(&mut s);
    assert_eq!(subjects(&mut s), vec!["Retro", "Standup later"]);
    let again = sync(&mut s);
    assert_eq!(calendars(&again)[0]["fetched"], json!(0), "{again}");
    done(s, dir);
}

#[test]
fn send_receive_keeps_calendars_in_step_and_bad_settings_are_refused() {
    let fake = Fake::default();
    fake.state().dav_put(PERSONAL, "standup.ics", &event("standup-1", "Standup", 20));
    let (mut s, dir) = account(&fake, "sendreceive");
    let got = s.run("mail.sendReceive", &Value::Null).unwrap();
    settle(&mut s);
    assert!(got["calendars"]["accounts"].is_array(), "{got}");
    show_account_calendar(&mut s);
    assert_eq!(subjects(&mut s), vec!["Standup"]);

    let id = s.accounts[0].id.clone();
    for bad in [json!("http://dav.example.test/"), json!("https://user:pw@dav.example.test/"), json!({}), json!(5)] {
        assert!(s.run("account.edit", &json!({ "id": id, "caldav": bad })).is_err(), "{bad} is refused");
    }
    // The same address keeps what was kept; another host starts over; "" takes it away.
    s.run("account.edit", &json!({ "id": id, "caldav": "https://dav.example.test/" })).unwrap();
    assert_eq!(s.accounts[0].caldav.as_ref().unwrap().calendars.len(), 1);
    s.run("account.edit", &json!({ "id": id, "caldav": "https://other.example.test/dav/" })).unwrap();
    assert!(s.accounts[0].caldav.as_ref().unwrap().calendars.is_empty());
    s.run("account.edit", &json!({ "id": id, "caldav": "" })).unwrap();
    assert!(s.accounts[0].caldav.is_none());
    assert!(s.run("calendar.sync", &json!({ "id": id })).is_err(), "no server, no sync");
    assert!(s.run("calendar.sync", &json!({ "id": 7 })).is_err());
    assert!(s.run("calendar.open", &json!({ "store": 9, "folder": 1 })).is_err());
    done(s, dir);
}
