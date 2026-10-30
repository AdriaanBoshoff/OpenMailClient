//! Several messages selected at once: Ctrl and Shift with a click or an arrow, Select All, and
//! what changes, moves and deletes the selected message done to each, as one step of Undo.

use crate::Session;
use openmailclient_model::time;
use serde_json::{Value, json};

fn session() -> Session {
    let mut s = Session::new();
    s.now = time::from_civil(2026, 10, 7, 14, 30, 0, 0);
    s.add_store(Box::new(crate::sample::sample_store(s.now)));
    s
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.run(id, &params).unwrap_or_else(|e| panic!("{id} failed: {e}"))
}

fn count(s: &Session, name: &str) -> u32 {
    s.folders[0].iter().find(|f| f.name == name).map_or(0, |f| f.total)
}

#[test]
fn ctrl_shift_and_select_all_choose_rows_and_the_last_clicked_is_shown() {
    let mut s = session();
    let ids: Vec<u64> = s.rows.iter().map(|r| r.id).collect();
    run(&mut s, "mail.select", json!({ "index": 1 }));
    let got = run(&mut s, "mail.select", json!({ "id": ids[3], "add": true }));
    assert_eq!(got["selected"], 2);
    assert_eq!((s.selected, s.chosen()), (Some(ids[3]), vec![ids[1], ids[3]]));
    // Ctrl and a click on one selected takes it away, and the other is shown.
    run(&mut s, "mail.select", json!({ "id": ids[3], "add": true }));
    assert_eq!((s.selected, s.chosen()), (Some(ids[1]), vec![ids[1]]));
    // Shift and a click: from where the selection began, either way.
    run(&mut s, "mail.select", json!({ "id": ids[1] }));
    run(&mut s, "mail.select", json!({ "id": ids[4], "through": true }));
    assert_eq!(s.chosen(), ids[1..=4].to_vec());
    run(&mut s, "mail.select", json!({ "id": ids[0], "through": true }));
    assert_eq!(s.chosen(), ids[0..=1].to_vec(), "the range starts again from the same row");
    // Shift and Down selects on; a click alone selects one again.
    run(&mut s, "mail.select", json!({ "id": ids[2] }));
    run(&mut s, "mail.next", json!({ "extend": true }));
    run(&mut s, "mail.next", json!({ "extend": true }));
    assert_eq!((s.selected, s.chosen()), (Some(ids[4]), ids[2..=4].to_vec()));
    run(&mut s, "mail.previous", json!({ "extend": true }));
    assert_eq!(s.chosen(), ids[2..=3].to_vec());
    run(&mut s, "mail.next", Value::Null);
    assert_eq!(s.chosen(), vec![ids[4]]);
    // Select All keeps the one shown; only that one is marked read.
    let unread = s.rows.iter().filter(|r| !r.read).count();
    assert_eq!(run(&mut s, "mail.selectAll", Value::Null)["selected"], ids.len());
    assert_eq!((s.selected, s.rows.iter().filter(|r| !r.read).count()), (Some(ids[4]), unread));
    // Another folder starts with nothing selected.
    run(&mut s, "mail.select", json!({ "folder": "Sent Items" }));
    assert!(s.chosen().is_empty() && s.also.is_empty());
    assert!(s.run("mail.select", &json!({ "id": 999_999, "add": true })).is_err());
}

#[test]
fn a_change_is_made_to_each_selected_and_taken_back_at_once() {
    let mut s = session();
    let ids: Vec<u64> = s.rows.iter().map(|r| r.id).collect();
    run(&mut s, "mail.select", json!({ "index": 0 }));
    run(&mut s, "mail.select", json!({ "id": ids[2], "through": true }));
    run(&mut s, "mail.markUnread", Value::Null);
    assert!(s.rows[..3].iter().all(|r| !r.read), "each one selected is unread");
    run(&mut s, "mail.markRead", Value::Null);
    assert!(s.rows[..3].iter().all(|r| r.read));
    let steps = s.undo.len();
    // A category: given to each, as the shown one lacks it, then taken from each.
    run(&mut s, "mail.categorize", json!({ "category": "Blue" }));
    assert!(s.rows[..3].iter().all(|r| r.categories.iter().any(|c| c == "Blue")), "{:?}", s.rows[0].categories);
    assert!(!s.rows[3].categories.iter().any(|c| c == "Blue"));
    assert_eq!(s.undo.len(), steps + 1, "one step for the three");
    // Unread/Read goes the way the one shown goes.
    run(&mut s, "mail.toggleRead", Value::Null);
    assert!(s.rows[..3].iter().all(|r| !r.read));
    let undone = run(&mut s, "edit.undo", Value::Null);
    assert_eq!(undone["undone"], "Unread/ Read");
    assert!(s.rows[..3].iter().all(|r| r.read));
    run(&mut s, "edit.undo", Value::Null);
    assert!(s.rows[..3].iter().all(|r| !r.categories.iter().any(|c| c == "Blue")));
    assert_eq!(s.chosen(), ids[..3].to_vec(), "those taken back are selected again");
}

#[test]
fn several_deleted_go_together_and_come_back_together() {
    let mut s = session();
    let ids: Vec<u64> = s.rows.iter().map(|r| r.id).collect();
    let (inbox, deleted) = (count(&s, "Inbox"), count(&s, "Deleted Items"));
    run(&mut s, "mail.select", json!({ "index": 1 }));
    run(&mut s, "mail.select", json!({ "id": ids[3], "add": true }));
    run(&mut s, "mail.select", json!({ "id": ids[4], "add": true }));
    run(&mut s, "mail.delete", Value::Null);
    assert_eq!((count(&s, "Inbox"), count(&s, "Deleted Items")), (inbox - 3, deleted + 3));
    assert_eq!((s.selected, s.chosen().len()), (Some(ids[5]), 1), "on to the row after the last of them");
    run(&mut s, "edit.undo", Value::Null);
    assert_eq!((count(&s, "Inbox"), count(&s, "Deleted Items")), (inbox, deleted));
    assert_eq!(s.chosen().len(), 3);

    // From Deleted Items they go for good, and still come back.
    run(&mut s, "mail.select", json!({ "folder": "Deleted Items", "index": 0 }));
    run(&mut s, "mail.selectAll", Value::Null);
    let there = count(&s, "Deleted Items");
    run(&mut s, "mail.delete", Value::Null);
    assert_eq!(count(&s, "Deleted Items"), 0);
    assert!(s.selected.is_none() && s.also.is_empty());
    run(&mut s, "edit.undo", Value::Null);
    assert_eq!(count(&s, "Deleted Items"), there);

    // Moved to a folder, all of them, and the move taken back.
    run(&mut s, "mail.select", json!({ "folder": "Inbox", "index": 0 }));
    run(&mut s, "mail.select", json!({ "index": 1, "through": true }));
    run(&mut s, "mail.move", json!({ "folder": "Archive" }));
    assert_eq!(count(&s, "Inbox"), inbox - 2);
    run(&mut s, "edit.undo", Value::Null);
    assert_eq!(count(&s, "Inbox"), inbox);
}
