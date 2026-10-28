//! The To-Do Bar: off until a part is shown, each module keeping its own parts; the
//! appointments of today and the next days not ended yet, the To-Do List under its headings and
//! a task ticked off from it, favourite people by their contacts' names; read again soon after
//! something was done and not on every frame; kept with the settings.

use crate::Session;
use crate::cmd::todo_bar::{Parts, heading};
use openmailclient_model::time;
use openmailclient_model::zone::Zone;
use serde_json::{Value, json};

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

fn subjects(list: &Value) -> Vec<String> {
    list.as_array().unwrap().iter().map(|v| v["untrusted_content"]["subject"].as_str().unwrap().to_string()).collect()
}

#[test]
fn it_is_off_until_a_part_is_shown_and_each_module_keeps_its_own() {
    let mut s = session();
    assert_eq!(s.todo_bar_parts(), Parts::default());
    assert_eq!(s.todo_bar_due(1_000), None, "nothing is read while it is not shown");
    let set = run(&mut s, "view.todoBar", json!({"part": "tasks"}));
    assert_eq!(set, json!({"module": "Mail", "calendar": false, "people": false, "tasks": true}));
    run(&mut s, "view.todoBar", json!({"part": "calendar", "show": true}));
    run(&mut s, "view.todoBar", json!({"part": "people", "module": "calendar"}));
    assert_eq!(s.todo_bar_parts(), Parts { calendar: true, people: false, tasks: true });
    run(&mut s, "nav.module", json!({"module": "calendar"}));
    assert_eq!(s.todo_bar_parts(), Parts { calendar: false, people: true, tasks: false }, "the Calendar keeps its own");
    run(&mut s, "view.todoBar", json!({"part": "people"}));
    assert!(!s.todo_bar_parts().any());
    run(&mut s, "view.todoBar", json!({"part": "off", "module": "mail"}));
    assert_eq!(s.todo_bar[0], Parts::default());
    for bad in [
        json!({}),
        json!({"part": "weather"}),
        json!({"part": "tasks", "show": "yes"}),
        json!({"part": "tasks", "module": "garden"}),
        json!({"part": 3}),
    ] {
        assert!(s.run("view.todoBar", &bad).is_err(), "{bad} is refused");
    }
}

#[test]
fn the_calendar_part_has_the_appointments_still_to_come_the_soonest_first() {
    let mut s = session();
    let bar = run(&mut s, "todo.bar", json!({}));
    let names = subjects(&bar["appointments"]);
    // 14:00 to 15:30 is under way at 14:30, and stays; lunch was yesterday and the stand-up
    // ended at 9:45.
    assert_eq!(names.first().map(String::as_str), Some("Review the spring catalogue"));
    assert!(names.contains(&"Call with the print shop".to_string()));
    assert!(names.contains(&"Dentist".to_string()) && names.contains(&"Volunteer shift at the library".to_string()));
    assert!(!names.contains(&"Lunch with Priya".to_string()) && !names.contains(&"Sam's birthday".to_string()));
    let list = bar["appointments"].as_array().unwrap();
    let first_standup = list.iter().find(|a| a["untrusted_content"]["subject"] == "Team stand-up").unwrap();
    assert_eq!(first_standup["day"], "2030-10-10", "today's has ended; tomorrow's is the next");
    let starts: Vec<&str> = list.iter().map(|a| a["start"].as_str().unwrap()).collect();
    assert!(starts.windows(2).all(|w| w[0] <= w[1]), "the soonest first: {starts:?}");
    assert!(starts.iter().all(|d| *d < "2030-10-16"), "today and the six days after it");
}

#[test]
fn the_tasks_part_is_the_to_do_list_and_a_task_is_ticked_off_from_it() {
    let mut s = session();
    let bar = run(&mut s, "todo.bar", json!({}));
    let tasks = bar["tasks"].as_array().unwrap();
    let late = tasks.iter().find(|t| t["untrusted_content"]["subject"] == "Send the venue the final head count").unwrap();
    assert_eq!(late["heading"], "Overdue");
    let someday = tasks.iter().find(|t| t["untrusted_content"]["subject"] == "Tidy the shared drive").unwrap();
    assert_eq!(someday["heading"], "No Date");
    assert!(!subjects(&bar["tasks"]).contains(&"Book the support rota".to_string()), "a task done is not on it");

    // Shown in Mail and ticked off there, the Tasks module never opened.
    run(&mut s, "view.todoBar", json!({"part": "tasks"}));
    assert_eq!(s.todo_bar_due(10_000), Some(60_000));
    assert!(s.todo.tasks.iter().any(|r| r.task.subject == "Send the venue the final head count"));
    let at = json!({"id": late["id"], "store": late["store"], "folder": late["folder"]});
    run(&mut s, "tasks.markComplete", at);
    s.todo_bar_due(10_500);
    assert!(s.todo.tasks.iter().any(|r| r.task.subject == "Send the venue the final head count"), "not read on every frame");
    let wait = s.todo_bar_due(10_600).unwrap();
    assert!(wait <= 1_000, "asked again soon: {wait}");
    s.todo_bar_due(11_100);
    assert!(!s.todo.tasks.iter().any(|r| r.task.subject == "Send the venue the final head count"), "read again a moment after");
    // A task made there is due today when no day is said.
    run(&mut s, "tasks.new", json!({"subject": "Ring the caterer", "due": "2030-10-09"}));
    s.todo_bar_due(12_200);
    assert!(s.todo.tasks.iter().any(|r| r.task.subject == "Ring the caterer"));
}

#[test]
fn headings_follow_the_week_the_region_begins_on() {
    let s = session();
    let today = s.today();
    // Wednesday 9 October 2030; the Sunday after is the 13th.
    let sunday = today + 4;
    assert_eq!(heading(None, today, 0), "No Date");
    assert_eq!(heading(Some(today - 1), today, 0), "Overdue");
    assert_eq!(heading(Some(today), today, 0), "Today");
    assert_eq!(heading(Some(today + 1), today, 0), "Tomorrow");
    assert_eq!(heading(Some(today + 3), today, 0), "This Week");
    assert_eq!(heading(Some(sunday), today, 0), "Next Week", "a week from Sunday begins on it");
    assert_eq!(heading(Some(sunday), today, 1), "This Week", "a week from Monday ends on it");
    assert_eq!(heading(Some(sunday + 1), today, 1), "Next Week");
    assert_eq!(heading(Some(sunday + 7), today, 1), "Next Week", "the Sunday a week on ends the next");
    assert_eq!(heading(Some(sunday + 8), today, 1), "Later");
    assert_eq!(heading(Some(sunday + 6), today, 0), "Next Week");
    assert_eq!(heading(Some(sunday + 7), today, 0), "Later");
    assert_eq!(heading(Some(i64::MAX), i64::MAX, 6), "Today", "no overflow at the end of time");
}

#[test]
fn favourites_are_shown_by_their_contacts_names() {
    let mut s = session();
    run(&mut s, "people.favorite", json!({"address": "  Priya.Raman@Example.org "}));
    run(&mut s, "people.favorite", json!({"address": "priya.raman@example.org"}));
    run(&mut s, "people.favorite", json!({"address": "someone@elsewhere.example"}));
    assert_eq!(s.favorites, ["priya.raman@example.org", "someone@elsewhere.example"], "kept once, in lower case");
    let bar = run(&mut s, "todo.bar", json!({}));
    let people = &bar["untrusted_content"]["people"];
    assert_eq!(people[0], json!({"name": "Priya Raman", "address": "priya.raman@example.org", "known": true}));
    assert_eq!(people[1], json!({"name": "someone@elsewhere.example", "address": "someone@elsewhere.example", "known": false}));
    run(&mut s, "people.favorite", json!({"address": "PRIYA.RAMAN@example.org", "on": false}));
    assert_eq!(s.favorites, ["someone@elsewhere.example"]);
    for bad in ["", "nobody", "@example.org", "a@", "a@b@c", "a b@example.org", "<a@b.example>", "a@b.ex\u{7}ample"] {
        assert!(s.run("people.favorite", &json!({"address": bad})).is_err(), "{bad:?} is refused");
    }
    assert!(s.run("people.favorite", &json!({"address": "a@b.example", "on": "no"})).is_err());
    assert!(s.run("people.favorite", &json!({"address": format!("{}@b.example", "a".repeat(400))})).is_err());
    for n in 1..crate::cmd::todo_bar::MAX_FAVORITES {
        run(&mut s, "people.favorite", json!({"address": format!("p{n}@example.org")}));
    }
    assert!(s.run("people.favorite", &json!({"address": "one.more@example.org"})).is_err(), "as many as it keeps");
}

#[test]
fn the_parts_and_the_favourites_are_kept_with_the_settings() {
    let mut s = session();
    run(&mut s, "view.todoBar", json!({"part": "tasks"}));
    run(&mut s, "view.todoBar", json!({"part": "calendar", "module": "people"}));
    run(&mut s, "people.favorite", json!({"address": "june.okafor@example.org"}));
    let path = std::env::temp_dir().join(format!("omc-todo-bar-{}.json", std::process::id()));
    let p = path.display().to_string();
    run(&mut s, "settings.save", json!({"path": p}));
    let mut again = session();
    let loaded = run(&mut again, "settings.load", json!({"path": p}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(loaded["ignored"], 0);
    assert_eq!(again.todo_bar, s.todo_bar);
    assert_eq!(again.favorites, ["june.okafor@example.org"]);

    // Junk in a settings file costs only itself.
    std::fs::write(&path, r#"{"toDoBar": {"mail": {"tasks": true}, "garden": {"tasks": true}, "people": {"tasks": "yes"}}, "favorites": ["x@y.example", 4, "nobody", "x@y.example"]}"#).unwrap();
    let mut third = session();
    let loaded = run(&mut third, "settings.load", json!({"path": p}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(third.todo_bar[0], Parts { tasks: true, ..Parts::default() });
    assert_eq!(third.todo_bar[2], Parts::default());
    assert_eq!(third.favorites, ["x@y.example"]);
    assert!(loaded["ignored"].as_u64().unwrap() >= 4);
}
