//! Hidden items put in folders (a folder's view, say): kept with the other unsaved changes,
//! journalled, and written with the folder's table of hidden items by a save into the file
//! where it is and by one that writes it whole.

use crate::PstStore;
use crate::check::check;
use crate::tests_edit::{folder, scratch};
use crate::tests_store::{UID, mail, mailbox, save};
use openmailclient_model::{FolderId, Item, ItemId, PropValue, Recovery, Store, StoreError, pid, view_def};
use std::path::{Path, PathBuf};

fn on_disk(name: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(name);
    let path = dir.join("box.pst");
    std::fs::write(&path, save(&mut mailbox()).0).unwrap();
    (dir, path)
}

fn open(path: &Path) -> PstStore {
    let mut store = PstStore::open(path).unwrap();
    store.keep_original(false);
    store
}

fn sound(path: &Path) {
    let found = check(&mut std::fs::File::open(path).unwrap()).unwrap();
    assert!(found.sound(), "{:?} {:?}", found.problems, found.inside);
}

fn a_view(name: &str, binary: &[u8]) -> Item {
    let mut item = Item::default();
    item.props.set(pid::MESSAGE_CLASS, PropValue::String(view_def::CLASS.to_string()));
    item.props.set(view_def::NAME, PropValue::String(name.to_string()));
    item.props.set(view_def::VERSION, PropValue::Int32(8));
    item.props.set(view_def::BINARY, PropValue::Binary(binary.to_vec()));
    item
}

/// The views a folder holds, by name, with their binaries.
fn views(store: &mut PstStore, folder: FolderId) -> Vec<(ItemId, String, Vec<u8>)> {
    store
        .hidden(folder)
        .unwrap()
        .into_iter()
        .filter_map(|(id, item)| {
            let item = item.ok()?;
            (item.props.str(pid::MESSAGE_CLASS) == Some(view_def::CLASS)).then_some(())?;
            let name = item.props.str(view_def::NAME)?.to_string();
            let Some(PropValue::Binary(b)) = item.props.get(view_def::BINARY) else { return None };
            Some((id, name, b.clone()))
        })
        .collect()
}

#[test]
fn a_hidden_item_is_saved_into_the_file_and_replaced_there() {
    let (dir, path) = on_disk("hidden-onto");
    let mut store = open(&path);
    let inbox = folder(&mut store, "Inbox").id;
    let before = store.hidden(inbox).unwrap().len();
    let id = store.put_hidden(inbox, None, a_view("Ours", b"first")).unwrap();
    assert!(store.unsaved());
    assert_eq!(views(&mut store, inbox), [(id, "Ours".to_string(), b"first".to_vec())]);
    store.save(UID).unwrap();
    assert_eq!(store.saved_in_place(), 1, "{:?}", store.in_place_trouble());
    sound(&path);
    let mut back = open(&path);
    assert_eq!(views(&mut back, inbox), [(id, "Ours".to_string(), b"first".to_vec())]);
    assert_eq!(back.hidden(inbox).unwrap().len(), before + 1);

    // In place of itself: still one, now as changed; the messages are untouched.
    let listed = back.list(inbox).unwrap().len();
    back.put_hidden(inbox, Some(id), a_view("Ours", b"second, longer than the first")).unwrap();
    back.save(UID).unwrap();
    sound(&path);
    let mut again = open(&path);
    assert_eq!(views(&mut again, inbox), [(id, "Ours".to_string(), b"second, longer than the first".to_vec())]);
    assert_eq!(again.list(inbox).unwrap().len(), listed);

    // Written whole: what was put in and what is new are both there.
    // (Those still open would keep the file from being swapped.)
    drop((store, back));
    again.save_in_place(false);
    let sent = folder(&mut again, "Sent Items").id;
    let other = again.put_hidden(sent, None, a_view("Theirs", b"x")).unwrap();
    again.put_hidden(inbox, Some(id), a_view("Ours", b"third")).unwrap();
    again.save(UID).unwrap();
    assert_eq!(again.saved_in_place(), 0);
    sound(&path);
    let mut whole = open(&path);
    assert_eq!(views(&mut whole, inbox), [(id, "Ours".to_string(), b"third".to_vec())]);
    assert_eq!(views(&mut whole, sent), [(other, "Theirs".to_string(), b"x".to_vec())]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_hidden_item_comes_back_after_a_crash_and_goes_with_a_new_folder() {
    let (dir, path) = on_disk("hidden-journal");
    let mut store = open(&path);
    let inbox = folder(&mut store, "Inbox").id;
    let made = store.create_folder(inbox, "Made here").unwrap();
    store.add(made, mail("In it", false)).unwrap();
    let in_made = store.put_hidden(made, None, a_view("New", b"in a new folder")).unwrap();
    let in_inbox = store.put_hidden(inbox, None, a_view("Ours", b"journalled")).unwrap();
    drop(store);

    let mut back = open(&path);
    assert_eq!(back.recover().unwrap().applied, 4);
    let made = folder(&mut back, "Made here").id;
    assert_eq!(views(&mut back, inbox), [(in_inbox, "Ours".to_string(), b"journalled".to_vec())]);
    assert_eq!(views(&mut back, made), [(in_made, "New".to_string(), b"in a new folder".to_vec())]);
    // A new id is not one a hidden item was given.
    let next = back.add(inbox, mail("After", true)).unwrap();
    assert!(next != in_inbox && next != in_made);
    back.save(UID).unwrap();
    sound(&path);
    let mut saved = open(&path);
    assert_eq!(saved.recover().unwrap(), Recovery::default());
    assert_eq!(views(&mut saved, made).len(), 1);
    assert_eq!(saved.list(made).unwrap().len(), 1);
    assert_eq!(views(&mut saved, inbox).len(), 1);

    // Only a hidden item of that folder is replaced; a message is not one.
    let message = ItemId(saved.list(inbox).unwrap()[0].id);
    assert!(matches!(saved.put_hidden(inbox, Some(message), a_view("No", b"")), Err(StoreError::NoItem)));
    assert!(matches!(saved.put_hidden(made, Some(in_inbox), a_view("No", b"")), Err(StoreError::NoItem)));
    assert!(saved.put_hidden(FolderId(0xFFFF_FFE2), None, a_view("No", b"")).is_err());
    assert!(!saved.unsaved());
    std::fs::remove_dir_all(&dir).unwrap();
}
