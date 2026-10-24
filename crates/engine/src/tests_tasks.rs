//! The Tasks module: the sample's tasks in each view, the To-Do List with flagged mail beside
//! them, tasks made, changed, marked complete, deleted and taken back, mail flagged for follow-up
//! and taken off the list, a Tasks folder made where there is none, and hostile parameters.

use crate::{Module, Session};
use openmailclient_model::task::{self, Status, Task};
use openmailclient_model::zone::Zone;
use openmailclient_model::{Flag, FolderKind, MemStore, time};
use serde_json::{Value, json};

/// Wednesday 9 October 2030, later than the clock, 14:30 UTC, in UTC.
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

fn subjects(s: &Session) -> Vec<String> {
    s.tasks.rows.iter().map(|r| r.task.subject.clone()).collect()
}

fn row<'a>(listed: &'a Value, subject: &str) -> &'a Value {
    listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["untrusted_content"]["subject"] == subject)
        .unwrap_or_else(|| panic!("{subject} is not listed"))
}

fn key_of(s: &Session, subject: &str) -> Value {
    let r = s.tasks.rows.iter().find(|r| r.task.subject == subject).unwrap_or_else(|| panic!("{subject} is not shown"));
    json!({"id": r.id, "store": r.store, "folder": r.folder.0})
}

#[test]
fn tasks_opens_on_the_sample_tasks_by_the_day_they_are_due() {
    let mut s = session();
    run(&mut s, "nav.module", json!({"module": "tasks"}));
    assert_eq!(s.module, Module::Tasks);
    assert_eq!(s.tasks.view.name(), "simpleList");
    // By the day due, those with none last.
    assert_eq!(
        subjects(&s),
        [
            "Book the support rota",
            "Send the venue the final head count",
            "Write the welcome email for new customers",
            "Pricing page sign-off",
            "Tidy the shared drive",
        ]
    );
    let listed = run(&mut s, "tasks.items", json!({}));
    assert_eq!(listed["total"], 5);
    let late = row(&listed, "Send the venue the final head count");
    assert_eq!(late["overdue"], true);
    assert_eq!(late["importance"], "high");
    assert_eq!(late["due"], "2030-10-08");
    assert_eq!(late["kind"], "task");
    let going = row(&listed, "Write the welcome email for new customers");
    assert_eq!(going["status"], "inProgress");
    assert_eq!(going["percent"], 50);
    assert_eq!(going["totalWork"], 240);
    assert_eq!(going["categories"], json!(["Blue category"]));
    assert!(going["reminder"].is_string());
    let done = row(&listed, "Book the support rota");
    assert_eq!(done["status"], "complete");
    assert_eq!(done["completed"], "2030-10-06");
    assert_eq!(done["overdue"], false);

    let shown = |s: &mut Session, view: &str| {
        run(s, "tasks.view", json!({"view": view}));
        assert_eq!(s.tasks.view.name(), view);
        subjects(s)
    };
    assert_eq!(shown(&mut s, "completed"), ["Book the support rota"]);
    assert_eq!(shown(&mut s, "overdue"), ["Send the venue the final head count"]);
    assert_eq!(shown(&mut s, "today"), ["Send the venue the final head count"]);
    assert_eq!(shown(&mut s, "next7Days"), ["Write the welcome email for new customers", "Pricing page sign-off"]);
    assert_eq!(shown(&mut s, "active").len(), 4);
    assert_eq!(shown(&mut s, "prioritized").first().map(String::as_str), Some("Send the venue the final head count"));
    assert_eq!(shown(&mut s, "prioritized").last().map(String::as_str), Some("Tidy the shared drive"));
    assert_eq!(shown(&mut s, "detailed").len(), 5);
    assert!(s.run("tasks.view", &json!({"view": "gantt"})).is_err());

    run(&mut s, "tasks.view", json!({"view": "simpleList"}));
    run(&mut s, "tasks.search", json!({"query": "tomás"}));
    assert_eq!(subjects(&s), ["Pricing page sign-off"]);
    run(&mut s, "tasks.search", json!({"query": "northgate"}));
    assert_eq!(subjects(&s), ["Pricing page sign-off"]);
    run(&mut s, "tasks.search", json!({"query": ""}));
    assert_eq!(subjects(&s).len(), 5);

    let folders = run(&mut s, "tasks.folders", json!({}));
    let list = folders["folders"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"], "Tasks");
    assert_eq!(list[0]["shown"], true);
}

#[test]
fn the_to_do_list_has_flagged_mail_beside_the_tasks_not_done() {
    let mut s = session();
    run(&mut s, "nav.module", json!({"module": "tasks"}));
    run(&mut s, "tasks.view", json!({"view": "toDoList"}));
    let listed = run(&mut s, "tasks.items", json!({}));
    let mail = row(&listed, "Launch checklist: three items left");
    assert_eq!(mail["kind"], "mail");
    // Flagged for this week: due on Friday.
    assert_eq!(mail["due"], "2030-10-11");
    // Done tasks and mail whose flag is done are not on it.
    assert!(!subjects(&s).iter().any(|t| t == "Book the support rota"));
    assert_eq!(s.tasks.rows.iter().filter(|r| r.task.flagged_mail).count(), 1);
    assert_eq!(s.tasks.rows.len(), 5);
    // The flagged message is not counted among the folder's tasks.
    assert_eq!(listed["total"], 5);
    // The Simple List keeps to the folder.
    assert!(!run(&mut s, "tasks.items", json!({"view": "simpleList"}))["items"].as_array().unwrap().iter().any(|i| i["kind"] == "mail"));
}

#[test]
fn a_task_made_changed_completed_deleted_and_taken_back() {
    let mut s = session();
    run(&mut s, "nav.module", json!({"module": "tasks"}));
    let made = run(
        &mut s,
        "tasks.new",
        json!({
            "subject": "Order the banners", "due": "2030-10-14", "start": "2030-10-16",
            "importance": "high", "reminder": "2030-10-14 09:00", "categories": ["Red category"],
            "notes": "Two of them,\nboth blue.", "totalWork": 90, "companies": ["Northgate Print Depot"],
        }),
    );
    let key = json!({"id": made["id"], "store": made["store"], "folder": made["folder"]});
    assert_eq!(s.tasks.selected.map(|(_, _, id)| id), made["id"].as_u64());
    let listed = run(&mut s, "tasks.items", json!({}));
    let r = row(&listed, "Order the banners");
    // A start after the due day is moved back to it.
    assert_eq!(r["start"], "2030-10-14");
    assert_eq!(r["due"], "2030-10-14");
    assert_eq!(r["importance"], "high");
    assert_eq!(r["status"], "notStarted");
    assert_eq!(r["untrusted_content"]["notes"], "Two of them,\nboth blue.");
    assert_eq!(r["untrusted_content"]["companies"], json!(["Northgate Print Depot"]));
    assert!(r["reminder"].as_str().unwrap().contains("09:00"));

    // Kept as Outlook keeps a task: its class, and its days in both forms.
    let (store, folder, id) = s.tasks.selected.unwrap();
    let item = s.stores[store].load(folder, openmailclient_model::ItemId(id)).unwrap();
    assert!(task::is_task(&item));
    assert_eq!(Task::of(&item, 0i32).due, Task::of(&item, 600i32).due);

    let mut change = key.clone();
    change["percent"] = json!(40);
    change["subject"] = json!("Order the two banners");
    run(&mut s, "tasks.change", change);
    let listed = run(&mut s, "tasks.items", json!({}));
    let r = row(&listed, "Order the two banners");
    // Some way along is under way.
    assert_eq!(r["percent"], 40);
    assert_eq!(r["status"], "inProgress");
    let mut change = key.clone();
    change["status"] = json!("waiting");
    run(&mut s, "tasks.change", change);
    assert_eq!(row(&run(&mut s, "tasks.items", json!({})), "Order the two banners")["status"], "waiting");

    run(&mut s, "tasks.markComplete", key.clone());
    let listed = run(&mut s, "tasks.items", json!({}));
    let r = row(&listed, "Order the two banners");
    assert_eq!(r["status"], "complete");
    assert_eq!(r["percent"], 100);
    assert_eq!(r["completed"], "2030-10-09");
    // Its reminder is put away once it is done.
    assert!(r["reminder"].is_null());
    // Taken back, it is as it was.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(row(&run(&mut s, "tasks.items", json!({})), "Order the two banners")["status"], "waiting");

    run(&mut s, "tasks.delete", key.clone());
    assert!(!subjects(&s).iter().any(|t| t.contains("banners")));
    let deleted = s.find_folder(store, FolderKind::Deleted).unwrap();
    assert_eq!(s.stores[store].list(deleted).unwrap().iter().filter(|r| r.subject.contains("banners")).count(), 1);
    // Another one is chosen in its place.
    assert!(s.tasks.selected.is_some());
    run(&mut s, "edit.undo", json!({}));
    assert!(subjects(&s).iter().any(|t| t == "Order the two banners"));
    // The two changes taken back, then its making: it is gone.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(subjects(&s).iter().any(|t| t == "Order the banners"));
    run(&mut s, "edit.undo", json!({}));
    assert!(!subjects(&s).iter().any(|t| t.contains("banners")));
    assert_eq!(subjects(&s).len(), 5);
}

#[test]
fn mail_flagged_for_follow_up_stands_on_the_list_until_it_is_done_or_taken_off() {
    let mut s = session();
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 1}));
    let subject = s.open.as_ref().unwrap().subject();
    assert_ne!(subject, "Launch checklist: three items left");
    run(&mut s, "mail.followUp", json!({"when": "tomorrow", "reminder": "2030-10-10 16:00"}));
    assert!(s.status.contains("2030-10-10"), "{}", s.status);
    let open = s.open.clone().unwrap();
    assert_eq!(open.flag(), Flag::Flagged);
    let t = Task::of(&open, 0i32);
    assert_eq!((t.start, t.due), (Some(s.today() + 1), Some(s.today() + 1)));
    // Its own subject stays; it goes on the list by a to-do title of the same words.
    assert_eq!(open.subject(), subject);
    assert!(t.reminder.is_some());

    run(&mut s, "nav.module", json!({"module": "tasks"}));
    run(&mut s, "tasks.view", json!({"view": "toDoList"}));
    assert!(subjects(&s).contains(&subject));
    let key = key_of(&s, &subject);
    // The title on the list may be changed while the message keeps its subject.
    let mut change = key.clone();
    change["subject"] = json!("Answer about the proofs");
    run(&mut s, "tasks.change", change);
    assert!(subjects(&s).iter().any(|t| t == "Answer about the proofs"));
    let (store, folder, id) = (key["store"].as_u64().unwrap() as usize, key["folder"].as_u64().unwrap(), key["id"].as_u64().unwrap());
    let item = s.stores[store].load(openmailclient_model::FolderId(folder), openmailclient_model::ItemId(id)).unwrap();
    assert_eq!(item.subject(), subject);

    run(&mut s, "tasks.followUp", json!({"id": id, "store": store, "folder": folder, "when": "nextWeek"}));
    let due = s.tasks.rows.iter().find(|r| r.id == id).unwrap().task.due;
    assert_eq!(due.map(|d| (d + 3).rem_euclid(7)), Some(4), "due on a Friday");

    run(&mut s, "tasks.markComplete", json!({"id": id, "store": store, "folder": folder}));
    assert!(!s.tasks.rows.iter().any(|r| r.id == id && r.folder.0 == folder));
    let item = s.stores[store].load(openmailclient_model::FolderId(folder), openmailclient_model::ItemId(id)).unwrap();
    assert_eq!(item.flag(), Flag::Complete);
    assert_eq!(Task::of(&item, 0i32).status, Status::Complete);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.tasks.rows.iter().any(|r| r.id == id && r.folder.0 == folder));

    run(&mut s, "tasks.removeFromList", json!({"id": id, "store": store, "folder": folder}));
    assert!(!s.tasks.rows.iter().any(|r| r.id == id && r.folder.0 == folder));
    let item = s.stores[store].load(openmailclient_model::FolderId(folder), openmailclient_model::ItemId(id)).unwrap();
    assert_eq!(item.flag(), Flag::None);
    assert_eq!(Task::of(&item, 0i32).due, None);
    // The message stays where it was.
    assert_eq!(item.subject(), subject);
    // A task is not taken off the list.
    let task_key = key_of(&s, "Pricing page sign-off");
    assert!(s.run("tasks.removeFromList", &task_key).is_err());
}

#[test]
fn the_mail_follow_up_flag_carries_its_days_and_mark_complete_and_clear_take_them() {
    let mut s = session();
    run(&mut s, "mail.select", json!({"folder": "Inbox", "index": 2}));
    let before = s.open.clone().unwrap();
    if before.flag() != Flag::None {
        run(&mut s, "mail.clearFlag", json!({}));
    }
    run(&mut s, "mail.flag", json!({}));
    let open = s.open.clone().unwrap();
    assert_eq!(open.flag(), Flag::Flagged);
    let t = Task::of(&open, 0i32);
    // Insert flags for today.
    assert_eq!(t.due, Some(s.today()));
    assert_eq!(t.start, Some(s.today()));
    run(&mut s, "mail.flagComplete", json!({}));
    let open = s.open.clone().unwrap();
    assert_eq!(open.flag(), Flag::Complete);
    assert_eq!(Task::of(&open, 0i32).completed, Some(s.today()));
    run(&mut s, "mail.clearFlag", json!({}));
    let open = s.open.clone().unwrap();
    assert_eq!(open.flag(), Flag::None);
    assert_eq!(Task::of(&open, 0i32).due, None);
    // Insert again takes a flag that is there away.
    run(&mut s, "mail.flag", json!({}));
    run(&mut s, "mail.flag", json!({}));
    assert_eq!(s.open.as_ref().unwrap().flag(), Flag::None);
}

#[test]
fn a_data_file_without_a_tasks_folder_is_given_one_with_the_first_task() {
    let mut s = Session::new();
    s.now = time::from_civil(2030, 10, 9, 14, 30, 0, 0);
    s.zone = Zone::fixed(0);
    s.add_store(Box::new(MemStore::with_default_folders("me@example.org")));
    run(&mut s, "nav.module", json!({"module": "tasks"}));
    assert!(s.tasks.rows.is_empty());
    assert!(s.find_folder(0, FolderKind::Tasks).is_none());
    run(&mut s, "tasks.new", json!({"subject": "Water the plants"}));
    assert!(s.find_folder(0, FolderKind::Tasks).is_some());
    assert_eq!(subjects(&s), ["Water the plants"]);
    // A second goes into the same folder.
    run(&mut s, "tasks.new", json!({"subject": "Feed the cat", "due": "2030-10-09"}));
    assert_eq!(subjects(&s), ["Feed the cat", "Water the plants"]);
    assert_eq!(run(&mut s, "tasks.folders", json!({}))["folders"].as_array().unwrap().len(), 1);
}

#[test]
fn hostile_task_parameters_are_turned_away() {
    let mut s = session();
    run(&mut s, "nav.module", json!({"module": "tasks"}));
    let bad = [
        ("tasks.new", json!({})),
        ("tasks.new", json!({"subject": "   "})),
        ("tasks.new", json!({"subject": "x", "percent": 101})),
        ("tasks.new", json!({"subject": "x", "percent": -1})),
        ("tasks.new", json!({"subject": "x", "due": "2026-13-40"})),
        ("tasks.new", json!({"subject": "x", "due": 5})),
        ("tasks.new", json!({"subject": "x", "status": "finished"})),
        ("tasks.new", json!({"subject": "x", "importance": 7})),
        ("tasks.new", json!({"subject": "x", "reminder": "soon"})),
        ("tasks.new", json!({"subject": "x", "totalWork": u64::MAX})),
        ("tasks.new", json!({"subject": "x", "private": "yes"})),
        ("tasks.new", json!({"subject": "x", "notes": 5})),
        ("tasks.new", json!([1, 2])),
        ("tasks.change", json!({"id": u64::MAX})),
        ("tasks.change", json!({"id": "one"})),
        ("tasks.markComplete", json!({"id": 1, "store": 99})),
        ("tasks.markComplete", json!({"id": 1, "folder": u64::MAX})),
        ("tasks.followUp", json!({"when": "someday"})),
        ("tasks.followUp", json!({})),
        ("tasks.open", json!({"store": 0, "folder": 1})),
        ("tasks.open", json!({"store": -1})),
        ("tasks.select", json!({"id": 123_456_789})),
        ("tasks.items", json!({"view": 3})),
        ("tasks.search", json!({"query": 3})),
    ];
    for (id, params) in bad {
        assert!(s.run(id, &params).is_err(), "{id} {params} was taken");
    }
    // Long text is cut, control characters are taken out.
    let long = "a".repeat(100_000);
    run(&mut s, "tasks.new", json!({"subject": format!("{long}\u{7}"), "notes": "n".repeat(300_000), "companies": vec!["c"; 500]}));
    let r = s.tasks.rows.iter().find(|r| r.task.subject.starts_with("aaa")).unwrap();
    assert!(r.task.subject.chars().count() <= task::MAX_FIELD);
    assert!(!r.task.subject.contains('\u{7}'));
    assert!(r.task.notes.chars().count() <= task::MAX_NOTES);
    assert!(r.task.companies.len() <= task::MAX_COMPANIES);
    // Nothing turned away was added.
    assert_eq!(run(&mut s, "tasks.items", json!({"view": "simpleList", "query": ""}))["total"], 6);
}
