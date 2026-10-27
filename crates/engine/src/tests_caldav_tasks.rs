//! Tasks kept in step with a CalDAV server's task lists, against the in-process fake: the
//! first sync, what is made, changed and deleted on either side, a calendar that holds both
//! events and to-dos, what Send/Receive does and what an account keeps of it.

use crate::Session;
use openmailclient_net::fake::{DAV_HOME, DavCalendar, Fake};
use serde_json::{Value, json};
use std::sync::Arc;

const PERSONAL: &str = "/dav/calendars/pat/personal/";

fn chores() -> String {
    format!("{DAV_HOME}chores/")
}

fn todo(uid: &str, summary: &str, due: u32) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Elsewhere//EN\r\nBEGIN:VTODO\r\nUID:{uid}\r\nDTSTAMP:20261001T000000Z\r\nDUE;VALUE=DATE:202610{due:02}\r\nSUMMARY:{summary}\r\nEND:VTODO\r\nEND:VCALENDAR\r\n"
    )
}

fn event(uid: &str, summary: &str, day: u32) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Elsewhere//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20261001T000000Z\r\nDTSTART:202610{day:02}T090000Z\r\nDTEND:202610{day:02}T100000Z\r\nSUMMARY:{summary}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

/// A fake with a task list beside its calendar.
fn fake() -> Fake {
    let fake = Fake::default();
    fake.state().calendars.push(DavCalendar::tasks(&chores(), "Chores"));
    fake
}

/// A session with an IMAP account whose CalDAV server is the fake's; its data file is store 1.
fn account(fake: &Fake, name: &str) -> (Session, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("omc-caldav-tasks-{name}-{}", std::process::id()));
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

fn sync(s: &mut Session, command: &str) -> Value {
    let got = s.run(command, &Value::Null).unwrap();
    settle(s);
    assert_eq!(got["ok"], json!(true), "{got}");
    got
}

fn lists(got: &Value) -> Vec<Value> {
    got["accounts"][0]["taskLists"].as_array().cloned().unwrap_or_default()
}

/// The account's own Tasks folder, shown in a view that has every task.
fn show_account_tasks(s: &mut Session) {
    let listed = s.run("tasks.folders", &Value::Null).unwrap();
    let mine = listed["folders"].as_array().unwrap().iter().find(|f| f["store"] == json!(1) && f["name"] == json!("Tasks")).cloned().unwrap();
    s.run("tasks.open", &json!({ "store": 1, "folder": mine["folder"] })).unwrap();
    s.run("tasks.view", &json!({ "view": "simpleList" })).unwrap();
}

fn rows(s: &mut Session) -> Vec<Value> {
    s.run("tasks.items", &Value::Null).unwrap()["items"].as_array().cloned().unwrap_or_default()
}

fn subjects(s: &mut Session) -> Vec<String> {
    let mut all: Vec<String> =
        rows(s).iter().filter(|r| r["kind"] == json!("task")).map(|r| r["untrusted_content"]["subject"].as_str().unwrap().to_string()).collect();
    all.sort();
    all
}

/// The row of a task, as tasks.change and tasks.delete take it.
fn row_of(s: &mut Session, subject: &str) -> Value {
    let r = rows(s).into_iter().find(|r| r["untrusted_content"]["subject"] == json!(subject)).unwrap_or_else(|| panic!("no {subject}"));
    json!({ "id": r["id"], "folder": r["folder"], "store": r["store"] })
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
fn the_first_sync_takes_each_task_list_down_into_a_tasks_folder_and_calendars_leave_them() {
    let fake = fake();
    {
        let mut st = fake.state();
        st.dav_put(&chores(), "bins.ics", &todo("bins-1", "Put the bins out", 20));
        st.dav_put(&chores(), "plants.ics", &todo("plants-1", "Water the plants", 21));
        st.dav_put(PERSONAL, "standup.ics", &event("standup-1", "Standup", 20));
        st.calendars.push(DavCalendar::tasks(&format!("{DAV_HOME}shopping/"), "Shopping"));
        st.dav_put(&format!("{DAV_HOME}shopping/"), "milk.ics", &todo("milk-1", "Milk", 22));
    }
    let (mut s, dir) = account(&fake, "first");
    // The calendar sync keeps the calendar of events alone.
    let cal = sync(&mut s, "calendar.sync");
    let names: Vec<&Value> = cal["accounts"][0]["calendars"].as_array().unwrap().iter().map(|c| &c["name"]).collect();
    assert_eq!(names, vec![&json!("Personal")], "{cal}");

    let got = sync(&mut s, "tasks.sync");
    let l = lists(&got);
    assert_eq!(l.len(), 2, "{got}");
    assert_eq!(l[0]["folder"], json!("Tasks"), "the first is the account's Tasks: {got}");
    assert_eq!(l[0]["fetched"], json!(2));
    assert_eq!(l[1]["folder"], json!("Shopping"), "the others are folders under it: {got}");
    show_account_tasks(&mut s);
    assert_eq!(subjects(&mut s), vec!["Put the bins out", "Water the plants"]);
    let bins = rows(&mut s).into_iter().find(|r| r["untrusted_content"]["subject"] == json!("Put the bins out")).unwrap();
    assert_eq!(bins["due"], json!("2026-10-20"));
    // Kept apart from the calendars, each with its own folder.
    let dav = s.accounts[0].caldav.clone().unwrap();
    assert_eq!(dav.calendars.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["Personal"]);
    assert_eq!(dav.tasks.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["Chores", "Shopping"]);
    assert!(dav.tasks.iter().all(|t| dav.calendars.iter().all(|c| c.here != t.here)));
    // A second sync with nothing changed changes nothing.
    let again = sync(&mut s, "tasks.sync");
    assert!(lists(&again).iter().all(|l| l["fetched"] == json!(0) && l["put"] == json!(0)), "{again}");
    done(s, dir);
}

#[test]
fn tasks_made_changed_and_deleted_on_either_side_go_to_the_other() {
    let fake = fake();
    fake.state().dav_put(&chores(), "bins.ics", &todo("bins-1", "Put the bins out", 20));
    fake.state().dav_put(&chores(), "plants.ics", &todo("plants-1", "Water the plants", 21));
    let (mut s, dir) = account(&fake, "both");
    sync(&mut s, "tasks.sync");
    show_account_tasks(&mut s);

    // Here: one made, one changed, one deleted.
    s.run("tasks.new", &json!({ "subject": "Book the dentist", "due": "2026-10-24" })).unwrap();
    let bins = row_of(&mut s, "Put the bins out");
    let mut change = bins.clone();
    change["subject"] = json!("Put the bins out tonight");
    s.run("tasks.change", &change).unwrap();
    let plants = row_of(&mut s, "Water the plants");
    s.run("tasks.delete", &plants).unwrap();
    let got = sync(&mut s, "tasks.sync");
    assert_eq!(lists(&got)[0]["put"], json!(2), "{got}");
    assert_eq!(lists(&got)[0]["deleted"], json!(1), "{got}");
    assert_eq!(on_server(&fake, &chores()), vec!["Book the dentist", "Put the bins out tonight"]);
    let dentist = fake.state().dav_calendar(&chores()).unwrap().objects.iter().find(|o| o.data.contains("Book the dentist")).cloned().unwrap();
    assert!(dentist.data.contains("BEGIN:VTODO") && !dentist.data.contains("BEGIN:VEVENT"), "{}", dentist.data);

    // There: one changed, one taken away, one new.
    {
        let mut st = fake.state();
        let changed = dentist.data.replace("SUMMARY:Book the dentist", "SUMMARY:Book the dentist for Monday");
        st.dav_put(&chores(), &dentist.name, &changed);
        let bins = st.dav_calendar(&chores()).unwrap().objects.iter().find(|o| o.data.contains("bins out")).map(|o| o.name.clone()).unwrap();
        st.dav_remove(&chores(), &bins);
        st.dav_put(&chores(), "oil.ics", &todo("oil-1", "Check the oil", 25));
    }
    let got = sync(&mut s, "tasks.sync");
    assert_eq!(lists(&got)[0]["removed"], json!(1), "{got}");
    assert_eq!(subjects(&mut s), vec!["Book the dentist for Monday", "Check the oil"]);
    done(s, dir);
}

#[test]
fn a_calendar_of_events_and_to_dos_keeps_each_with_its_own_kind() {
    let fake = Fake::default();
    let both = format!("{DAV_HOME}home/");
    {
        let mut st = fake.state();
        let mut home = DavCalendar::new(&both, "Home");
        home.comps = vec!["VEVENT".into(), "VTODO".into()];
        st.calendars.push(home);
        st.dav_put(&both, "party.ics", &event("party-1", "Party", 24));
        st.dav_put(&both, "cake.ics", &todo("cake-1", "Order the cake", 23));
    }
    let (mut s, dir) = account(&fake, "mixed");
    let cal = sync(&mut s, "calendar.sync");
    let tasks = sync(&mut s, "tasks.sync");
    let home_of = |got: &Value, key: &str| got["accounts"][0][key].as_array().unwrap().iter().find(|c| c["name"] == json!("Home")).cloned().unwrap();
    assert_eq!(home_of(&cal, "calendars")["fetched"], json!(1), "the event alone: {cal}");
    assert_eq!(lists(&tasks)[0]["name"], json!("Home"));
    assert_eq!(lists(&tasks)[0]["fetched"], json!(1), "the to-do alone: {tasks}");
    show_account_tasks(&mut s);
    assert_eq!(subjects(&mut s), vec!["Order the cake"]);

    // A task made here goes there; neither sync takes away the other's.
    s.run("tasks.new", &json!({ "subject": "Buy candles" })).unwrap();
    sync(&mut s, "tasks.sync");
    sync(&mut s, "calendar.sync");
    sync(&mut s, "tasks.sync");
    assert_eq!(on_server(&fake, &both), vec!["Buy candles", "Order the cake", "Party"]);
    assert_eq!(subjects(&mut s), vec!["Buy candles", "Order the cake"]);
    done(s, dir);
}

#[test]
fn send_receive_keeps_task_lists_in_step_and_the_account_keeps_them() {
    let fake = fake();
    fake.state().dav_put(&chores(), "bins.ics", &todo("bins-1", "Put the bins out", 20));
    let (mut s, dir) = account(&fake, "receive");
    let got = s.run("mail.sendReceive", &Value::Null).unwrap();
    settle(&mut s);
    assert_eq!(got["tasks"]["accounts"][0]["taskLists"][0]["fetched"], json!(1), "{got}");
    show_account_tasks(&mut s);
    assert_eq!(subjects(&mut s), vec!["Put the bins out"]);
    // Kept in the settings beside the calendars, and read back.
    let kept = s.accounts[0].caldav.clone().unwrap().to_json();
    assert_eq!(kept["tasks"][0]["href"], json!(chores()), "{kept}");
    assert_eq!(kept["calendars"][0]["href"], json!(PERSONAL), "{kept}");
    // An account without a CalDAV server is told so.
    let err = s.run("tasks.sync", &json!({ "id": 7 })).unwrap_err().to_string();
    assert!(err.contains("account's id"), "{err}");
    done(s, dir);
}
