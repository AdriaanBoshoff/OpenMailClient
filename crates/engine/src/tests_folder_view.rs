//! Each folder's own view: kept in the settings for every store, in the data file where Outlook
//! keeps a folder's views for one that can be written, and read from a view Outlook wrote.

use crate::folder_view::{FolderView, NAME};
use crate::print::Field;
use crate::{ReadingPane, Session, SortKey};
use openmailclient_model::view_def::{self, Column, ViewDef};
use openmailclient_model::{FolderKind, Item, PropValue, pid, time};
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

fn open(s: &mut Session, folder: &str) {
    run(s, "folder.select", json!({ "folder": folder }));
}

#[test]
fn each_folder_keeps_its_own_view() {
    let mut s = session();
    open(&mut s, "Inbox");
    run(&mut s, "view.sortBy", json!({ "key": "subject" }));
    run(&mut s, "view.columns", json!({ "hide": "size" }));
    run(&mut s, "view.readingPane", json!({ "position": "bottom" }));
    // A folder with no view of its own shows the one the last had.
    open(&mut s, "Sent Items");
    assert_eq!((s.sort, s.reading_pane), (SortKey::Subject, ReadingPane::Bottom));
    run(&mut s, "view.sortBy", json!({ "key": "size" }));
    run(&mut s, "view.readingPane", json!({ "position": "off" }));

    open(&mut s, "Inbox");
    assert_eq!((s.sort, s.sort_desc, s.reading_pane), (SortKey::Subject, false, ReadingPane::Bottom));
    assert!(!s.list_columns.contains(&Field::Size) && !s.print_list.columns.contains(&Field::Size));
    let subjects: Vec<String> = s.rows.iter().map(|r| r.subject.to_lowercase()).collect();
    assert!(subjects.windows(2).all(|w| w[0] <= w[1]), "{subjects:?}");
    open(&mut s, "Sent Items");
    assert_eq!((s.sort, s.sort_desc, s.reading_pane), (SortKey::Size, true, ReadingPane::Off));

    // Kept between runs.
    let saved = crate::cmd::settings::current(&s, None);
    assert_eq!(saved["folderViews"].as_object().unwrap().len(), 2, "{saved}");
    let dir = std::env::temp_dir().join(format!("omc-folder-view-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    std::fs::write(&path, saved.to_string()).unwrap();
    let mut again = session();
    let loaded = run(&mut again, "settings.load", json!({ "path": path }));
    assert_eq!(loaded["ignored"], 0, "{loaded}");
    open(&mut again, "Inbox");
    assert_eq!((again.sort, again.reading_pane), (SortKey::Subject, ReadingPane::Bottom));

    // Reset View: as a folder first is, and kept so.
    run(&mut again, "view.resetView", json!({}));
    assert_eq!(FolderView::of(&again), FolderView::default());
    open(&mut again, "Sent Items");
    open(&mut again, "Inbox");
    assert_eq!(FolderView::of(&again), FolderView::default());

    // Settings that make no sense are left out, a part at a time.
    std::fs::write(&path, json!({ "folderViews": { "a": { "sort": "nowhere" }, "b": 5 }, "version": 1 }).to_string()).unwrap();
    let mut third = session();
    run(&mut third, "settings.load", json!({ "path": path }));
    assert!(third.folder_views.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The views a data file's folder holds, by name.
fn views(s: &mut Session, folder: FolderKind) -> Vec<String> {
    let id = s.find_folder(0, folder).unwrap();
    s.stores[0]
        .hidden(id)
        .unwrap()
        .into_iter()
        .filter_map(|(_, item)| item.ok())
        .filter(|item| item.props.str(pid::MESSAGE_CLASS) == Some(view_def::CLASS))
        .filter_map(|item| item.props.str(view_def::NAME).map(str::to_string))
        .collect()
}

#[test]
fn a_folders_view_is_kept_in_its_data_file_and_one_outlook_wrote_is_read() {
    let dir = std::env::temp_dir().join(format!("omc-folder-view-pst-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("views.pst");
    run(&mut session(), "file.export", json!({ "path": path }));

    let mut s = Session::new();
    s.journal_dir = Some(dir.join("journals"));
    run(&mut s, "file.open", json!({ "path": path }));
    open(&mut s, "Inbox");
    run(&mut s, "view.sortBy", json!({ "key": "subject", "descending": true }));
    run(&mut s, "view.columns", json!({ "show": "preview", "at": 0 }));
    run(&mut s, "view.readingPane", json!({ "position": "bottom" }));
    // Looking changes nothing: the view waits in the settings for the file's next save.
    assert!(!s.stores[0].unsaved());
    assert!(views(&mut s, FolderKind::Inbox).is_empty());
    assert!(!dir.join("views.pst.openmailclient-journal").exists());

    // A view Outlook wrote: its own name, no word of ours, sorted by size, ascending.
    let sent = s.find_folder(0, FolderKind::Sent).unwrap();
    let def = ViewDef {
        columns: vec![Column::of(0x0037_001F, 300, "Subject"), Column::of(0x0E08_0003, 60, "Size"), Column::of(0x7D01_001F, 60, "Something else")],
        sort: Some(1),
        ..ViewDef::default()
    };
    let (bin, strings) = view_def::write(&def);
    let mut theirs = Item::default();
    theirs.props.set(pid::MESSAGE_CLASS, PropValue::String(view_def::CLASS.into()));
    theirs.props.set(view_def::NAME, PropValue::String("Compact".into()));
    theirs.props.set(view_def::VERSION, PropValue::Int32(8));
    theirs.props.set(view_def::BINARY, PropValue::Binary(bin));
    theirs.props.set(view_def::STRINGS, PropValue::String(strings));
    s.stores[0].put_hidden(sent, None, theirs).unwrap();
    run(&mut s, "file.save", json!({}));
    assert_eq!(views(&mut s, FolderKind::Inbox), [NAME]);
    assert!(s.views_unsaved.is_empty());
    let found = crate::io::check_structure(&path).unwrap().unwrap();
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    run(&mut s, "file.closeStore", json!({ "store": 0 }));

    // Another run with no settings: the file says it all.
    let mut other = Session::new();
    other.journal_dir = Some(dir.join("journals"));
    run(&mut other, "file.open", json!({ "path": path }));
    open(&mut other, "Inbox");
    assert_eq!((other.sort, other.sort_desc, other.reading_pane), (SortKey::Subject, true, ReadingPane::Bottom));
    assert_eq!(other.list_columns.first(), Some(&Field::Preview), "the column with no property of its own too");
    open(&mut other, "Sent Items");
    assert_eq!((other.sort, other.sort_desc), (SortKey::Size, false));
    assert_eq!(other.list_columns, [Field::Subject, Field::Size]);
    // Ours is changed in its place; Outlook's is left as it was.
    run(&mut other, "view.sortBy", json!({ "key": "date" }));
    open(&mut other, "Inbox");
    run(&mut other, "view.reverseSort", json!({}));
    // Asked to save, views alone are saved.
    assert!(!other.stores[0].unsaved());
    assert_eq!(run(&mut other, "file.save", json!({}))["saved"], 1);
    assert_eq!(views(&mut other, FolderKind::Inbox), [NAME]);
    let mut sent_views = views(&mut other, FolderKind::Sent);
    sent_views.sort();
    assert_eq!(sent_views, ["Compact", NAME]);
    run(&mut other, "file.closeStore", json!({ "store": 0 }));

    let mut last = Session::new();
    last.journal_dir = Some(dir.join("journals"));
    run(&mut last, "file.open", json!({ "path": path }));
    open(&mut last, "Sent Items");
    assert_eq!(last.sort, SortKey::Date, "ours before Outlook's");
    open(&mut last, "Inbox");
    assert_eq!((last.sort, last.sort_desc), (SortKey::Subject, false));
    run(&mut last, "file.closeStore", json!({ "store": 0 }));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_view_that_makes_no_sense_is_not_shown() {
    let base = FolderView::default();
    let mut item = crate::folder_view::to_item(&FolderView {
        sort: SortKey::Flag,
        descending: false,
        columns: vec![Field::Notes],
        reading_pane: ReadingPane::Off,
    });
    let back = crate::folder_view::from_item(&item, &base).unwrap();
    assert_eq!((back.sort, back.columns.as_slice(), back.reading_pane), (SortKey::Flag, [Field::Notes].as_slice(), ReadingPane::Off));
    // Our words damaged: what the binary says, over the view shown.
    item.named[0].1 = PropValue::String("{ not json".into());
    let back = crate::folder_view::from_item(&item, &base).unwrap();
    assert_eq!((back.sort, back.columns.as_slice(), back.reading_pane), (SortKey::Flag, [Field::Flag].as_slice(), ReadingPane::Right));
    item.props.set(view_def::BINARY, PropValue::Binary(vec![1, 2, 3]));
    assert!(crate::folder_view::from_item(&item, &base).is_none());
    item.props.set(pid::MESSAGE_CLASS, PropValue::String("IPM.Note".into()));
    assert!(crate::folder_view::from_item(&item, &base).is_none());
}
