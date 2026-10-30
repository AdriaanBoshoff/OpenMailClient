//! Reminders: the sample opens on none due, an appointment's comes its minutes before, is
//! snoozed and dismissed and written so; a series' moves on to the next occurrence; a task's
//! and a flagged message's; one opened goes to its item; a data file that cannot be written
//! keeps them put away for the run; the options are kept with the settings.

use crate::{Module, Session};
use openmailclient_model::calendar::{Appointment, MINUTE};
use openmailclient_model::reminder;
use openmailclient_model::zone::Zone;
use openmailclient_model::{FolderId, FolderInfo, Item, ItemId, MemStore, Store, StoreError, Summary, time};
use serde_json::{Value, json};

const HOUR: i64 = 60 * MINUTE;

/// Wednesday 9 October 2030, 14:30 UTC, in UTC.
fn session() -> Session {
    let mut s = Session::new();
    s.now = time::from_civil(2030, 10, 9, 14, 30, 0, 0);
    s.zone = Zone::fixed(0);
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.run(id, &params).unwrap_or_else(|e| panic!("{id} failed: {e}"))
}

fn subjects(listed: &Value) -> Vec<String> {
    listed["due"].as_array().unwrap().iter().map(|d| d["untrusted_content"]["subject"].as_str().unwrap().to_string()).collect()
}

/// The sample's appointment named `subject`, as it is kept now.
fn appointment(s: &mut Session, subject: &str) -> Appointment {
    let (store, folder) = crate::cmd::calendar::calendar_folder(s).unwrap();
    let st = s.stores.get_mut(store).unwrap();
    let rows = st.list(folder).unwrap();
    rows.iter().map(|r| Appointment::of(&st.load(folder, ItemId(r.id)).unwrap()).unwrap()).find(|a| a.subject == subject).unwrap()
}

#[test]
fn the_sample_opens_on_none_due_and_an_appointment_comes_its_minutes_before() {
    let mut s = session();
    let listed = run(&mut s, "reminders.list", json!({}));
    assert_eq!(subjects(&listed), Vec::<String>::new(), "those whose time came were dismissed as the sample was made");
    assert!(listed["next"].is_i64());
    // The dentist is tomorrow at 8:00; its reminder comes at 7:45.
    let dentist = appointment(&mut s, "Dentist").start;
    s.now = dentist - 16 * MINUTE;
    assert!(subjects(&run(&mut s, "reminders.list", json!({}))).is_empty());
    s.now = dentist - 15 * MINUTE;
    let listed = run(&mut s, "reminders.list", json!({}));
    assert_eq!(subjects(&listed), ["Dentist"]);
    let due = &listed["due"][0];
    assert_eq!((due["kind"].as_str(), due["dueIn"].as_str()), (Some("appointment"), Some("In 15 minutes")));
    assert_eq!(due["untrusted_content"]["location"], "Bridge Street Clinic");
    let fresh = s.reminders.fresh;
    assert!(fresh > 0, "a reminder that comes is counted, so the window comes up");

    // Snoozed for 5 minutes, written into the item: gone until then.
    let until = run(&mut s, "reminders.snooze", json!({}));
    assert_eq!(until["until"], time::to_unix(dentist - 10 * MINUTE));
    let item = {
        let (store, folder) = crate::cmd::calendar::calendar_folder(&s).unwrap();
        let st = s.stores.get_mut(store).unwrap();
        let id = st.list(folder).unwrap().iter().find(|r| r.subject == "Dentist").unwrap().id;
        st.load(folder, ItemId(id)).unwrap()
    };
    let stored = reminder::of(&item, s.now, &s.zone).unwrap();
    assert_eq!(stored.signal, dentist - 10 * MINUTE, "written into the item, so Outlook reads it too");
    assert!(subjects(&run(&mut s, "reminders.list", json!({}))).is_empty());
    s.now = dentist - 10 * MINUTE;
    assert_eq!(subjects(&run(&mut s, "reminders.list", json!({}))), ["Dentist"]);
    assert!(s.reminders.fresh > fresh, "it comes up again");

    // Snoozed to its start, then dismissed for good.
    run(&mut s, "reminders.snooze", json!({"beforeStart": 0}));
    s.now = dentist;
    let listed = run(&mut s, "reminders.list", json!({}));
    assert_eq!(listed["due"][0]["dueIn"], "Now");
    assert_eq!(run(&mut s, "reminders.dismiss", json!({}))["left"], 0);
    s.now = dentist + 3 * HOUR;
    let listed = subjects(&run(&mut s, "reminders.list", json!({})));
    assert!(!listed.contains(&"Dentist".to_string()), "{listed:?}");
    assert_eq!(appointment(&mut s, "Dentist").reminder, None, "dismissed for good, its reminder is off, as Outlook writes it");
}

#[test]
fn a_series_reminds_of_its_next_occurrence_once_one_is_dismissed() {
    let mut s = session();
    // Thursday 10 October 2030, 9:15: the stand-up at 9:30 is due.
    s.now = time::from_civil(2030, 10, 10, 9, 15, 0, 0);
    let listed = run(&mut s, "reminders.list", json!({}));
    assert_eq!(subjects(&listed), ["Dentist", "Team stand-up"], "the dentist's at 7:45 is still there, an hour and a half on");
    run(&mut s, "reminders.dismiss", json!({}));
    let listed = run(&mut s, "reminders.list", json!({}));
    assert_eq!(subjects(&listed), ["Team stand-up"]);
    let first = listed["due"][0]["occurrence"].as_i64().unwrap();
    run(&mut s, "reminders.dismiss", json!({}));
    assert!(subjects(&run(&mut s, "reminders.list", json!({}))).is_empty());
    // Friday's comes in its turn (beside the sample task's, come due meanwhile).
    s.now = time::from_civil(2030, 10, 11, 9, 15, 0, 0);
    let listed = run(&mut s, "reminders.list", json!({}));
    let standup = listed["due"].as_array().unwrap().iter().find(|d| d["untrusted_content"]["subject"] == "Team stand-up").unwrap().clone();
    assert_eq!(standup["occurrence"].as_i64(), Some(first + 1));
    // Opened: the Calendar on that day, with it chosen.
    let opened = run(&mut s, "reminders.open", json!({"store": standup["store"], "folder": standup["folder"], "id": standup["id"]}));
    assert_eq!(s.module, Module::Calendar);
    assert_eq!(s.calendar.selected, opened["id"].as_u64());
    assert_eq!(s.calendar.occurrence, Some(first + 1));
    assert_eq!(s.calendar.day, first + 1);
    assert!(subjects(&run(&mut s, "reminders.list", json!({}))).contains(&"Team stand-up".to_string()), "opening keeps the reminder");
}

#[test]
fn a_task_and_a_flagged_message_remind_and_open_where_they_are() {
    let mut s = session();
    let made = run(&mut s, "tasks.new", json!({"subject": "Ring the printer", "due": "2030-10-10", "reminder": "2030-10-09 16:00"}));
    run(&mut s, "nav.module", json!({"module": "mail"}));
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 0}));
    run(&mut s, "mail.followUp", json!({"when": "today", "reminder": "2030-10-09 16:30"}));
    let message = s.open.as_ref().unwrap().subject();
    s.now = time::from_civil(2030, 10, 9, 17, 0, 0, 0);
    let listed = run(&mut s, "reminders.list", json!({}));
    assert_eq!(subjects(&listed), ["Ring the printer".to_string(), message.clone()]);
    assert_eq!(listed["due"][0]["kind"], "task");
    assert_eq!(listed["due"][0]["dueIn"], "Tomorrow");
    assert_eq!(listed["due"][1]["kind"], "message");
    run(&mut s, "reminders.open", json!({}));
    assert_eq!(s.module, Module::Tasks);
    assert_eq!(s.tasks.selected.map(|k| k.2), made["id"].as_u64());
    let second = &listed["due"][1];
    run(&mut s, "reminders.open", json!({"store": second["store"], "folder": second["folder"], "id": second["id"]}));
    assert_eq!(s.module, Module::Mail);
    assert_eq!(s.selected, second["id"].as_u64());
    // Done is done: the task's reminder goes; Dismiss All takes the message's.
    run(&mut s, "tasks.markComplete", json!({"id": made["id"], "store": made["store"], "folder": made["folder"]}));
    assert_eq!(subjects(&run(&mut s, "reminders.list", json!({}))), [message]);
    assert_eq!(run(&mut s, "reminders.dismissAll", json!({}))["dismissed"], 1);
    assert!(subjects(&run(&mut s, "reminders.list", json!({}))).is_empty());
    assert!(s.open.as_ref().is_some_and(openmailclient_model::task::is_flagged), "dismissed, still flagged");
}

/// A data file that cannot be written: the sample, read only.
struct ReadOnly(MemStore);

impl Store for ReadOnly {
    fn display_name(&self) -> String {
        self.0.display_name()
    }
    fn writable(&self) -> bool {
        false
    }
    fn folders(&mut self) -> Result<Vec<FolderInfo>, StoreError> {
        self.0.folders()
    }
    fn list(&mut self, folder: FolderId) -> Result<Vec<Summary>, StoreError> {
        self.0.list(folder)
    }
    fn load(&mut self, folder: FolderId, id: ItemId) -> Result<Item, StoreError> {
        self.0.load(folder, id)
    }
}

#[test]
fn a_data_file_that_cannot_be_written_keeps_them_put_away_for_the_run() {
    let mut s = Session::new();
    s.now = time::from_civil(2030, 10, 9, 14, 30, 0, 0);
    s.zone = Zone::fixed(0);
    s.add_store(Box::new(ReadOnly(crate::sample::sample_store(s.now))));
    let dentist = appointment(&mut s, "Dentist").start;
    s.now = dentist - 15 * MINUTE;
    assert_eq!(subjects(&run(&mut s, "reminders.list", json!({}))), ["Dentist"]);
    run(&mut s, "reminders.snooze", json!({"minutes": 10}));
    assert!(subjects(&run(&mut s, "reminders.list", json!({}))).is_empty());
    s.now = dentist - 5 * MINUTE;
    assert_eq!(subjects(&run(&mut s, "reminders.list", json!({}))), ["Dentist"], "the snooze held ends");
    run(&mut s, "reminders.dismiss", json!({}));
    s.now = dentist + HOUR;
    assert!(subjects(&run(&mut s, "reminders.list", json!({}))).is_empty());
    assert!(s.reminders_due(10 * 60 * 60 * 1000).is_some());
    assert!(subjects(&run(&mut s, "reminders.list", json!({}))).is_empty(), "still put away after everything is read again");
}

#[test]
fn the_host_is_asked_back_when_the_next_one_comes() {
    let mut s = session();
    let dentist = appointment(&mut s, "Dentist").start;
    s.now = dentist - 15 * MINUTE - 30 * 10_000_000;
    let wait = s.reminders_due(1_000).unwrap();
    assert!((250..=30_000).contains(&wait), "{wait}");
    s.now = dentist - 15 * MINUTE;
    s.reminders_due(1_000 + wait);
    assert_eq!(s.reminders.due.len(), 1);
    assert!(s.reminders_due(1_000 + wait + 10).unwrap() <= 60_000);
}

#[test]
fn the_options_are_kept_with_the_settings_and_bad_ones_refused() {
    let mut s = session();
    assert_eq!(run(&mut s, "reminders.options", json!({})), json!({"show": true, "sound": true, "soundFile": null}));
    assert_eq!(run(&mut s, "reminders.options", json!({"sound": false})), json!({"show": true, "sound": false, "soundFile": null}));
    assert!(s.run("reminders.options", &json!({"show": "yes"})).is_err());
    // A sound of the owner's: made sure of as it is chosen, kept with the settings.
    let dir = std::env::temp_dir();
    let wav = dir.join(format!("omc-reminder-{}.wav", std::process::id()));
    let sound = openmailclient_formats::wav::Wav { rate: 8_000, channels: 1, samples: vec![0.25; 800] };
    std::fs::write(&wav, openmailclient_formats::wav::write(&sound)).unwrap();
    let not = dir.join(format!("omc-reminder-{}.txt", std::process::id()));
    std::fs::write(&not, "not a sound").unwrap();
    let w = wav.display().to_string();
    assert_eq!(run(&mut s, "reminders.options", json!({"soundFile": w}))["soundFile"], json!(w));
    for bad in [
        json!({"soundFile": not.display().to_string()}),
        json!({"soundFile": dir.join("nowhere.wav").display().to_string()}),
        json!({"soundFile": 3}),
    ] {
        assert!(s.run("reminders.options", &bad).is_err(), "{bad} is refused");
    }
    assert_eq!(s.reminders.sound_file.as_deref(), Some(w.as_str()), "a refused one changes nothing");
    let _ = std::fs::remove_file(&not);
    let path = std::env::temp_dir().join(format!("omc-reminders-{}.json", std::process::id()));
    let p = path.display().to_string();
    run(&mut s, "settings.save", json!({"path": p}));
    let mut again = session();
    run(&mut again, "settings.load", json!({"path": p}));
    let _ = std::fs::remove_file(&path);
    assert!(!again.reminders.sound);
    assert!(again.reminders.show);
    assert_eq!(again.reminders.sound_file.as_deref(), Some(w.as_str()));
    let _ = std::fs::remove_file(&wav);
    assert_eq!(run(&mut again, "reminders.options", json!({"soundFile": ""}))["soundFile"], json!(null), "back to our own chime");
    run(&mut s, "view.reminders", json!({}));
    assert!(s.reminders.window);
    run(&mut s, "view.reminders", json!({"open": false}));
    assert!(!s.reminders.window);
}

#[test]
fn hostile_reminder_params_are_refused() {
    let mut s = session();
    let dentist = appointment(&mut s, "Dentist").start;
    s.now = dentist - 15 * MINUTE;
    run(&mut s, "reminders.list", json!({}));
    for bad in [
        json!({"minutes": 0}),
        json!({"minutes": -5}),
        json!({"minutes": 1e30}),
        json!({"minutes": 99_999_999}),
        json!({"beforeStart": 999_999}),
        json!({"beforeStart": 60}),
        json!({"id": 1, "store": 99, "folder": 1}),
        json!({"id": u64::MAX, "store": 0, "folder": u64::MAX}),
        json!({"id": "x"}),
    ] {
        assert!(s.run("reminders.snooze", &bad).is_err(), "{bad}");
    }
    assert_eq!(s.reminders.due.len(), 1, "nothing was put off");
}
