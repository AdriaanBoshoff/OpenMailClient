//! The Calendar module: views, going from day to day, and appointments made, changed, deleted
//! and taken back, in the sample mailbox's calendar.

use crate::{Module, Session};
use openmailclient_model::{FolderKind, MemStore, time};
use serde_json::{Value, json};

/// 2026-10-07 14:30 UTC, a Wednesday.
fn now() -> i64 {
    time::from_civil(2026, 10, 7, 14, 30, 0, 0)
}

fn session() -> Session {
    let mut s = Session::new();
    s.now = now();
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.run(id, &params).unwrap_or_else(|e| panic!("{id} failed: {e}"))
}

fn subjects(v: &Value) -> Vec<String> {
    v["items"].as_array().unwrap().iter().map(|i| i["untrusted_content"]["subject"].as_str().unwrap().to_string()).collect()
}

#[test]
fn the_calendar_opens_on_this_week_with_the_sample_plans() {
    let mut s = session();
    run(&mut s, "nav.module", json!({"module": "calendar"}));
    assert_eq!(s.module, Module::Calendar);
    // Today is the system's today; the sample's plans are around the time it was made.
    assert_eq!(s.calendar.day, s.today());
    run(&mut s, "calendar.goTo", json!({"date": "2026-10-07"}));
    let days = s.calendar_days();
    assert_eq!(days.len(), 7);
    // The week of Wednesday 7 October 2026 starts on Sunday the 4th where the week starts on Sunday.
    let first = time::civil(openmailclient_model::calendar::day_start(days[0], 0), 0);
    assert_eq!((first.month, first.day, first.weekday), (10, 4, 0));
    let shown: Vec<&str> = s.calendar.items.iter().map(|(_, a)| a.subject.as_str()).collect();
    for want in ["Team stand-up", "Review the spring catalogue", "Call with the print shop", "Book fair", "Sam's birthday"] {
        assert!(shown.contains(&want), "{want} is not shown: {shown:?}");
    }
    // Earliest first; all-day events start at midnight, so they come before the day's plans.
    let starts: Vec<i64> = s.calendar.items.iter().map(|(_, a)| a.start).collect();
    assert!(starts.windows(2).all(|w| w[0] <= w[1]));

    let today = run(&mut s, "calendar.items", json!({"from": "2026-10-07", "to": "2026-10-08"}));
    assert_eq!(subjects(&today), ["Team stand-up", "Review the spring catalogue", "Call with the print shop"]);
    let review = &today["items"][1];
    assert_eq!(
        (review["start"].as_str(), review["end"].as_str(), review["busy"].as_str()),
        (Some("2026-10-07 14:00"), Some("2026-10-07 15:30"), Some("busy"))
    );
    assert_eq!(today["items"][2]["busy"], "tentative");
}

#[test]
fn views_and_going_from_day_to_day() {
    let mut s = session();
    run(&mut s, "calendar.goTo", json!({"date": "2026-10-07"}));
    run(&mut s, "calendar.view", json!({"view": "day"}));
    assert_eq!(s.calendar_days().len(), 1);
    run(&mut s, "calendar.next", Value::Null);
    let day = time::civil(openmailclient_model::calendar::day_start(s.calendar.day, 0), 0);
    assert_eq!((day.month, day.day), (10, 8));
    assert!(s.calendar.items.iter().any(|(_, a)| a.subject == "Dentist"));

    run(&mut s, "calendar.view", json!({"view": "workWeek"}));
    let days = s.calendar_days();
    assert_eq!(days.len(), 5);
    assert_eq!(time::civil(openmailclient_model::calendar::day_start(days[0], 0), 0).weekday, 1);

    run(&mut s, "calendar.view", json!({"view": "month"}));
    assert_eq!(s.calendar_days().len(), 42);
    run(&mut s, "calendar.next", Value::Null);
    let day = time::civil(openmailclient_model::calendar::day_start(s.calendar.day, 0), 0);
    assert_eq!((day.year, day.month), (2026, 11));
    run(&mut s, "calendar.previous", Value::Null);
    run(&mut s, "calendar.previous", Value::Null);
    let day = time::civil(openmailclient_model::calendar::day_start(s.calendar.day, 0), 0);
    assert_eq!((day.year, day.month), (2026, 9));

    run(&mut s, "calendar.goTo", json!({"date": "2027-02-28"}));
    let day = time::civil(openmailclient_model::calendar::day_start(s.calendar.day, 0), 0);
    assert_eq!((day.year, day.month, day.day), (2027, 2, 28));
    run(&mut s, "calendar.today", Value::Null);
    assert_eq!(s.calendar.day, s.today());

    for bad in [json!({"view": "fortnight"}), json!({}), json!({"view": 3})] {
        assert!(s.run("calendar.view", &bad).is_err());
    }
    for bad in [json!({"date": "2026-13-01"}), json!({"date": "yesterday"}), json!({})] {
        assert!(s.run("calendar.goTo", &bad).is_err());
    }
}

#[test]
fn appointments_are_made_changed_deleted_and_taken_back() {
    let mut s = session();
    s.zone = openmailclient_model::zone::Zone::fixed(120);
    run(&mut s, "nav.module", json!({"module": "calendar"}));
    let made = run(&mut s, "calendar.new", json!({"subject": "Design review", "start": "2026-10-09 11:00", "location": "Room 1", "reminder": 10}));
    let id = made["id"].as_u64().unwrap();
    assert_eq!(s.calendar.selected, Some(id));
    let (_, a) = s.calendar.items.iter().find(|(i, _)| *i == id).unwrap().clone();
    // Written in local time, two hours east of UTC; half an hour long when no end is given.
    assert_eq!(a.start, time::from_civil(2026, 10, 9, 9, 0, 0, 0));
    assert_eq!(a.minutes(), 30);
    assert_eq!((a.location.as_str(), a.reminder, a.all_day), ("Room 1", Some(10), false));

    // Moving the start keeps the length; a later end makes it longer.
    run(&mut s, "calendar.change", json!({"start": "2026-10-09 13:00"}));
    run(&mut s, "calendar.change", json!({"id": id, "end": "2026-10-09 14:30", "busy": "outOfOffice", "reminder": null}));
    let got = run(&mut s, "calendar.items", json!({"from": "2026-10-09", "to": "2026-10-10"}));
    let item = got["items"].as_array().unwrap().iter().find(|i| i["id"] == id).unwrap().clone();
    assert_eq!((item["start"].as_str(), item["end"].as_str()), (Some("2026-10-09 13:00"), Some("2026-10-09 14:30")));
    assert_eq!((item["busy"].as_str(), item["reminder"].is_null()), (Some("outOfOffice"), true));
    assert!(s.run("calendar.change", &json!({"id": id, "end": "2026-10-09 12:00"})).is_err(), "it cannot end before it starts");

    // Undone, the change before it is back.
    assert_eq!(run(&mut s, "edit.undo", Value::Null)["undone"], "Change Appointment");
    let (_, a) = s.calendar.items.iter().find(|(i, _)| *i == id).unwrap().clone();
    assert_eq!(a.minutes(), 30);

    // Deleted, it goes to Deleted Items, and comes back when undone.
    run(&mut s, "calendar.delete", json!({"id": id}));
    assert!(!s.calendar.items.iter().any(|(_, a)| a.subject == "Design review"));
    let deleted = s.find_folder(0, FolderKind::Deleted).unwrap();
    assert!(s.stores[0].list(deleted).unwrap().iter().any(|r| r.subject == "Design review"));
    run(&mut s, "edit.undo", Value::Null);
    assert!(s.calendar.items.iter().any(|(_, a)| a.subject == "Design review"));
}

#[test]
fn an_all_day_event_runs_midnight_to_midnight() {
    let mut s = session();
    s.zone = openmailclient_model::zone::Zone::fixed(-300);
    run(&mut s, "calendar.view", json!({"view": "week"}));
    let id = run(&mut s, "calendar.new", json!({"subject": "Away", "start": "2026-10-12", "end": "2026-10-14"}))["id"].as_u64().unwrap();
    let (_, a) = s.calendar.items.iter().find(|(i, _)| *i == id).unwrap().clone();
    assert!(a.all_day);
    assert_eq!(a.start, time::from_civil(2026, 10, 12, 0, 0, 0, -300));
    // The last day given is the last day of it: three days.
    assert_eq!(a.end, time::from_civil(2026, 10, 15, 0, 0, 0, -300));
    assert_eq!((a.reminder, a.busy.name()), (None, "free"));
}

#[test]
fn no_calendar_and_hostile_appointments_are_errors_not_crashes() {
    let mut s = Session::new();
    s.now = now();
    s.add_store(Box::new(MemStore::with_default_folders("Test")));
    assert!(!s.is_enabled("calendar.new") && !s.is_enabled("calendar.items"));
    run(&mut s, "nav.module", json!({"module": "calendar"}));
    assert!(s.calendar.items.is_empty());
    assert!(s.run("calendar.new", &json!({"subject": "x", "start": "2026-10-09 11:00"})).is_err());

    let mut s = session();
    let long = "x".repeat(100_000);
    for bad in [
        json!({"subject": "no start"}),
        json!({"start": "2026-10-09 25:00"}),
        json!({"start": "2026-10-09 11:00", "end": "9999-12-31 00:00"}),
        json!({"start": "2026-10-09 11:00", "reminder": -5}),
        json!({"start": "2026-10-09 11:00", "reminder": 1e12}),
        json!({"start": "2026-10-09 11:00", "busy": "asleep"}),
        json!({"start": "2026-10-09 11:00", "subject": 7}),
        json!({"start": "\u{0}\u{ffff}"}),
    ] {
        assert!(s.run("calendar.new", &bad).is_err(), "{bad} was taken");
    }
    // A long subject is cut, and control characters are left out.
    let id = run(&mut s, "calendar.new", json!({"subject": format!("a\u{7}b{long}"), "start": "2026-10-09 11:00"}))["id"].as_u64().unwrap();
    let (_, a) = s.calendar.items.iter().find(|(i, _)| *i == id).unwrap().clone();
    assert!(a.subject.starts_with("abx") && a.subject.chars().count() == 2_000);
    assert!(s.run("calendar.items", &json!({"from": "2026-10-09", "to": "2026-10-01"})).is_err());
    assert!(s.run("calendar.items", &json!({"from": "2000-01-01", "to": "2030-01-01"})).is_err());
    assert!(s.run("calendar.select", &json!({"id": 999_999})).is_err());
    assert!(s.run("calendar.delete", &json!({"id": 999_999})).is_err());
}

/// The items of `from` to `to` (YYYY-MM-DD, the day after the last) whose subject starts so,
/// as (start, subject, occurrence).
fn shown(s: &mut Session, from: &str, to: &str, subject: &str) -> Vec<(String, String, String)> {
    let got = run(s, "calendar.items", json!({"from": from, "to": to}));
    got["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["untrusted_content"]["subject"].as_str().unwrap().starts_with(subject))
        .map(|i| {
            let text = |v: &Value| v.as_str().unwrap_or("").to_string();
            (text(&i["start"]), text(&i["untrusted_content"]["subject"]), text(&i["occurrence"]))
        })
        .collect()
}

#[test]
fn the_sample_stand_up_is_on_every_weekday() {
    let mut s = session();
    let week = shown(&mut s, "2026-10-04", "2026-10-11", "Team stand-up");
    let days: Vec<&str> = week.iter().map(|(start, _, _)| &start[..10]).collect();
    assert_eq!(days, ["2026-10-05", "2026-10-06", "2026-10-07", "2026-10-08", "2026-10-09"]);
    assert!(week.iter().all(|(start, _, occurrence)| start.starts_with(occurrence.as_str()) && start.ends_with("09:30")));
    let got = run(&mut s, "calendar.items", json!({"from": "2026-10-07", "to": "2026-10-08"}));
    let one = got["items"].as_array().unwrap().iter().find(|i| i["untrusted_content"]["subject"] == "Team stand-up").unwrap().clone();
    assert_eq!(one["recurring"], true);
    assert!(one["pattern"].as_str().unwrap().starts_with("Occurs every weekday"), "{}", one["pattern"]);
}

#[test]
fn a_series_is_made_and_its_occurrences_changed_one_by_one() {
    let mut s = session();
    s.zone = openmailclient_model::zone::Zone::fixed(120);
    run(&mut s, "nav.module", json!({"module": "calendar"}));
    // Mondays and Wednesdays at 10, four times, from Sunday: the first is on Monday.
    let repeat = json!({"every": "week", "on": ["monday", "Wed"], "count": 4});
    let id =
        run(&mut s, "calendar.new", json!({"subject": "Pottery", "start": "2026-10-11 10:00", "end": "2026-10-11 11:30", "repeat": repeat}))["id"]
            .as_u64()
            .unwrap();
    let starts = |s: &mut Session| {
        let all = shown(s, "2026-10-01", "2026-11-30", "");
        all.into_iter().filter(|(_, b, _)| b.starts_with("Pottery") || b.starts_with("Clay")).map(|(a, b, _)| format!("{a} {b}")).collect::<Vec<_>>()
    };
    assert_eq!(starts(&mut s), ["2026-10-12 10:00 Pottery", "2026-10-14 10:00 Pottery", "2026-10-19 10:00 Pottery", "2026-10-21 10:00 Pottery"]);
    assert_eq!(s.calendar.occurrence.map(|d| d % 7), Some(openmailclient_model::calendar::day_of(time::from_civil(2026, 10, 12, 0, 0, 0, 0), 0) % 7));

    // One occurrence moved and renamed; the others are as they were.
    run(&mut s, "calendar.change", json!({"id": id, "occurrence": "2026-10-14", "start": "2026-10-15 16:00", "subject": "Pottery, glazing"}));
    assert_eq!(starts(&mut s)[1], "2026-10-15 16:00 Pottery, glazing");
    assert_eq!(starts(&mut s)[2], "2026-10-19 10:00 Pottery");
    // It cannot be moved past the next one.
    assert!(s.run("calendar.change", &json!({"id": id, "occurrence": "2026-10-14", "start": "2026-10-20 10:00"})).is_err());
    assert!(s.run("calendar.change", &json!({"id": id, "occurrence": "2026-10-13", "subject": "x"})).is_err(), "not a day it falls on");
    assert!(s.run("calendar.change", &json!({"id": id, "occurrence": "2026-10-14", "repeat": null})).is_err());

    // The chosen occurrence is deleted alone, and comes back when undone.
    run(&mut s, "calendar.select", json!({"id": id, "occurrence": "2026-10-19"}));
    run(&mut s, "calendar.delete", Value::Null);
    assert_eq!(starts(&mut s).len(), 3);
    assert_eq!(run(&mut s, "edit.undo", Value::Null)["undone"], "Delete Occurrence");
    assert_eq!(starts(&mut s).len(), 4);

    // The series renamed: the occurrence renamed on its own keeps its name.
    run(&mut s, "calendar.change", json!({"id": id, "subject": "Clay class"}));
    let names: Vec<String> = shown(&mut s, "2026-10-01", "2026-11-30", "").into_iter().filter(|(_, _, o)| !o.is_empty()).map(|(_, b, _)| b).collect();
    assert!(names.contains(&"Pottery, glazing".to_string()) && names.iter().filter(|n| *n == "Clay class").count() == 3, "{names:?}");

    // Its time moved: every occurrence as planned again, an hour later.
    run(&mut s, "calendar.change", json!({"id": id, "series": true, "start": "2026-10-12 11:00", "end": "2026-10-12 12:30"}));
    assert_eq!(
        starts(&mut s),
        ["2026-10-12 11:00 Clay class", "2026-10-14 11:00 Clay class", "2026-10-19 11:00 Clay class", "2026-10-21 11:00 Clay class"]
    );

    // Every other month on the last Friday, until the new year; then once only.
    run(
        &mut s,
        "calendar.recurrence",
        json!({"id": id, "repeat": {"every": "month", "interval": 2, "nth": "last", "on": ["friday"], "until": "2027-01-01"}}),
    );
    let monthly = shown(&mut s, "2026-10-01", "2027-03-01", "Clay");
    assert_eq!(monthly.iter().map(|(a, _, _)| &a[..10]).collect::<Vec<_>>(), ["2026-10-30", "2026-12-25"]);
    run(&mut s, "calendar.recurrence", json!({"id": id, "repeat": null}));
    let once = shown(&mut s, "2026-10-01", "2027-03-01", "Clay");
    assert_eq!(once.len(), 1);
    assert_eq!(once[0].2, "", "no longer an occurrence");
}

#[test]
fn hostile_patterns_are_errors_not_crashes() {
    let mut s = session();
    let id = run(&mut s, "calendar.new", json!({"subject": "x", "start": "2026-10-12 10:00"}))["id"].as_u64().unwrap();
    for bad in [
        json!(5),
        json!("weekly"),
        json!({"every": "fortnight"}),
        json!({"every": "day", "interval": 0}),
        json!({"every": "day", "interval": 1000}),
        json!({"every": "week", "interval": 100}),
        json!({"every": "week", "on": []}),
        json!({"every": "week", "on": ["xx"]}),
        json!({"every": "week", "on": "monday"}),
        json!({"every": "month", "nth": 9, "on": ["monday"]}),
        json!({"every": "month", "day": 32}),
        json!({"every": "day", "count": 1000}),
        json!({"every": "day", "count": 3, "until": "2027-01-01"}),
        json!({"every": "day", "until": "2026-10-01"}),
        json!({"every": "day", "until": "never"}),
    ] {
        assert!(s.run("calendar.recurrence", &json!({"id": id, "repeat": bad})).is_err(), "{bad}");
    }
    assert!(s.run("calendar.recurrence", &json!({"id": id})).is_err());
    // The longest a series can be is walked in bounded time, and so is the widest range.
    run(&mut s, "calendar.recurrence", json!({"id": id, "repeat": {"every": "day", "count": 999}}));
    let got = run(&mut s, "calendar.items", json!({"from": "2026-10-01", "to": "2031-10-01"}));
    assert_eq!(got["items"].as_array().unwrap().iter().filter(|i| i["id"] == id).count(), 999);
    assert!(s.run("calendar.select", &json!({"id": id, "occurrence": "2020-01-01"})).is_err());
}

#[test]
fn a_series_with_changed_and_deleted_occurrences_is_saved_to_a_pst_and_read_again() {
    let dir = std::env::temp_dir().join(format!("openmailclient-series-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = session();
    let id = run(
        &mut s,
        "calendar.new",
        json!({"subject": "Rowing", "start": "2026-10-12 07:00", "end": "2026-10-12 08:00", "repeat": {"every": "week", "on": ["monday", "thursday"], "count": 6}}),
    )["id"]
        .as_u64()
        .unwrap();
    run(
        &mut s,
        "calendar.change",
        json!({"id": id, "occurrence": "2026-10-15", "subject": "Rowing (river closed: gym)", "start": "2026-10-15 18:00", "end": "2026-10-15 19:00"}),
    );
    run(&mut s, "calendar.select", json!({"id": id, "occurrence": "2026-10-19"}));
    run(&mut s, "calendar.delete", json!({}));
    let before = shown(&mut s, "2026-10-01", "2026-12-01", "Rowing");
    assert_eq!(before.len(), 5, "{before:?}");
    // Each occurrence, the one moved too, says the series' own pattern and times.
    let got = run(&mut s, "calendar.items", json!({"from": "2026-10-01", "to": "2026-12-01"}));
    let patterns: std::collections::BTreeSet<_> =
        got["items"].as_array().unwrap().iter().filter(|i| i["id"] == id).map(|i| i["pattern"].as_str().unwrap().to_string()).collect();
    assert_eq!(patterns.len(), 1, "{patterns:?}");
    assert!(patterns.iter().all(|p| p.ends_with("from 07:00 to 08:00")), "{patterns:?}");

    let path = dir.join("series.pst");
    run(&mut s, "file.export", json!({"path": path}));
    let found = crate::io::check_structure(&path).unwrap().unwrap();
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let mut other = Session::new();
    other.now = now();
    run(&mut other, "file.open", json!({"path": path}));
    run(&mut other, "nav.module", json!({"module": "calendar"}));
    assert_eq!(shown(&mut other, "2026-10-01", "2026-12-01", "Rowing"), before);
    // Read again, it is changed as any series is.
    let opened = other.calendar.items.iter().find(|(_, a)| a.subject == "Rowing").map(|(i, _)| *i).unwrap();
    run(&mut other, "calendar.select", json!({"id": opened, "occurrence": "2026-10-22"}));
    run(&mut other, "calendar.delete", json!({}));
    assert_eq!(shown(&mut other, "2026-10-01", "2026-12-01", "Rowing").len(), 4);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_series_in_a_zone_keeps_its_hour_there_across_the_change_to_summer_time() {
    let dir = std::env::temp_dir().join(format!("openmailclient-zone-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = session();
    run(&mut s, "calendar.timeZone", json!({"zone": "Europe/Berlin"}));
    assert_eq!(s.zone_chosen.as_deref(), Some("Europe/Berlin"));
    // Mondays at 09:00 in Berlin; summer time begins there on Sunday 28 March 2027.
    let made = run(
        &mut s,
        "calendar.new",
        json!({"subject": "Planning", "start": "2027-03-15 09:00", "end": "2027-03-15 10:00", "repeat": {"every": "week", "on": ["monday"], "count": 4}}),
    );
    let id = made["id"].as_u64().unwrap();
    let starts = |s: &mut Session| shown(s, "2027-03-01", "2027-04-30", "Planning").into_iter().map(|(start, ..)| start).collect::<Vec<_>>();
    assert_eq!(starts(&mut s), ["2027-03-15 09:00", "2027-03-22 09:00", "2027-03-29 09:00", "2027-04-05 09:00"]);
    let got = run(&mut s, "calendar.items", json!({"from": "2027-03-15", "to": "2027-03-16"}));
    assert_eq!(got["items"][0]["zone"], "Europe/Berlin");
    // Seen from UTC it is an hour earlier from the change on.
    run(&mut s, "calendar.timeZone", json!({"zone": "UTC"}));
    let utc = ["2027-03-15 08:00", "2027-03-22 08:00", "2027-03-29 07:00", "2027-04-05 07:00"];
    assert_eq!(starts(&mut s), utc);

    // Saved to a data file and read again, it is in its zone still.
    let path = dir.join("zone.pst");
    run(&mut s, "file.export", json!({"path": path}));
    let mut other = Session::new();
    other.now = now();
    run(&mut other, "file.open", json!({"path": path}));
    run(&mut other, "nav.module", json!({"module": "calendar"}));
    assert_eq!(starts(&mut other), utc);
    let got = run(&mut other, "calendar.items", json!({"from": "2027-03-15", "to": "2027-03-16"}));
    assert_eq!(got["items"][0]["zone"], "Europe/Berlin");

    // And in a calendar file, with the zone defined in it.
    let ics = dir.join("zone.ics");
    run(&mut s, "calendar.saveAs", json!({"path": ics.to_str().unwrap(), "from": "2027-03-01", "to": "2027-04-30"}));
    let text = std::fs::read_to_string(&ics).unwrap();
    assert!(text.contains("TZID:Europe/Berlin") && text.contains("DTSTART;TZID=Europe/Berlin:20270315T090000"), "{text}");
    let mut third = Session::new();
    third.now = now();
    let mut store = MemStore::new("Mine");
    store.push_folder(None, "Deleted Items", "IPF.Note");
    store.push_folder(None, "Calendar", "IPF.Appointment");
    third.add_store(Box::new(store));
    third.module = crate::Module::Calendar;
    run(&mut third, "calendar.import", json!({"path": ics.to_str().unwrap()}));
    assert_eq!(starts(&mut third), utc);
    let _ = id;

    // The zone chosen is kept in the settings; null follows the system's again.
    let kept = crate::cmd::settings::current(&s, None);
    assert_eq!(kept["timeZone"], "UTC");
    run(&mut s, "calendar.timeZone", json!({"zone": null}));
    assert_eq!((s.zone_chosen.clone(), s.zone.clone()), (None, s.system_zone.clone()));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn time_zones_are_listed_and_chosen_and_a_second_one_shown() {
    let mut s = session();
    let got = run(&mut s, "calendar.timeZones", json!({}));
    let zones = got["zones"].as_array().unwrap();
    assert!(zones.len() > 50);
    let ny = zones.iter().find(|z| z["id"] == "America/New_York").unwrap();
    // In October New York keeps summer time, four hours behind UTC.
    assert_eq!((ny["offsetNow"].as_i64(), ny["daylight"].as_bool()), (Some(-240), Some(true)));
    // Asked for by Outlook's name, it is known by its own.
    run(&mut s, "calendar.timeZone", json!({"zone": "Tokyo Standard Time"}));
    assert_eq!(s.zone_chosen.as_deref(), Some("Asia/Tokyo"));
    run(&mut s, "calendar.secondTimeZone", json!({"zone": "America/New_York"}));
    assert_eq!(run(&mut s, "calendar.timeZones", json!({}))["second"]["id"], "America/New_York");
    run(&mut s, "calendar.secondTimeZone", json!({"zone": null}));
    assert!(s.second_zone.is_none());
    for bad in [json!({}), json!({"zone": "Mars/Olympus"}), json!({"zone": 5}), json!({"zone": "x".repeat(100_000)})] {
        assert!(s.run("calendar.timeZone", &bad).is_err());
        assert!(s.run("calendar.secondTimeZone", &bad).is_err());
    }
    assert_eq!(s.zone_chosen.as_deref(), Some("Asia/Tokyo"), "a bad name changes nothing");
}
