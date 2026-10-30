//! Changes to an open `.pst`: made, saved to a new file, read back.

use crate::check::check;
use crate::ltp::Raw;
use crate::tests_store::{UID, mail, mailbox, same, save, tree};
use crate::{PstStore, Written};
use openmailclient_model::{Attachment, Flag, FolderId, FolderInfo, FolderKind, Item, ItemId, PropValue, Store, StoreError, Summary, pid, ptype};
use std::io::Cursor;

pub(super) type Whole = Vec<(String, FolderInfo, Vec<(Summary, Item)>)>;

fn open() -> PstStore {
    PstStore::from_bytes(save(&mut mailbox()).0).unwrap()
}

pub(super) fn folder(store: &mut PstStore, name: &str) -> FolderInfo {
    store.folders().unwrap().into_iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no folder {name}"))
}

/// Save the changes to memory and open what was written.
fn saved(store: &mut PstStore) -> PstStore {
    let (out, _) = store.save_to(Cursor::new(Vec::new()), UID).unwrap();
    let bytes = out.into_inner();
    let found = check(&mut Cursor::new(&bytes)).unwrap();
    assert!(found.sound(), "{:?} {:?}", found.problems, found.inside);
    PstStore::from_bytes(bytes).unwrap()
}

/// A store's folders and messages, each message loaded.
pub(super) fn whole(store: &mut PstStore) -> Whole {
    tree(store)
        .into_iter()
        .map(|(path, f, list)| {
            let items = list.into_iter().map(|s| (s.clone(), store.load(f.id, ItemId(s.id)).unwrap())).collect();
            (path, f, items)
        })
        .collect()
}

/// What was saved reads back as what was shown before saving.
pub(super) fn same_mailbox(want: &Whole, got: &Whole) {
    assert_eq!(got.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), want.iter().map(|f| f.0.as_str()).collect::<Vec<_>>());
    for ((path, w, w_items), (_, g, g_items)) in want.iter().zip(got) {
        assert_eq!((g.id, g.parent, g.total, g.unread, g.has_children), (w.id, w.parent, w.total, w.unread, w.has_children), "{path}");
        assert_eq!(&g.container_class, &w.container_class, "{path}");
        assert_eq!(g_items.len(), w_items.len(), "{path}");
        assert_eq!(g.total as usize, g_items.len(), "{path}");
        for ((ws, wi), (gs, gi)) in w_items.iter().zip(g_items) {
            assert_eq!(
                (gs.id, &gs.subject, &gs.from, &gs.to, gs.time, gs.read, gs.flag),
                (ws.id, &ws.subject, &ws.from, &ws.to, ws.time, ws.read, ws.flag),
                "{path}"
            );
            assert_eq!((gs.importance, &gs.message_class), (ws.importance, &ws.message_class), "{path}");
            same(wi, gi, path);
        }
    }
}

#[test]
fn a_file_copied_without_changes_is_the_same_mailbox() {
    let mut store = open();
    assert!(store.writable() && !store.unsaved());
    let want = whole(&mut store);
    let mut back = saved(&mut store);
    same_mailbox(&want, &whole(&mut back));
    assert!(!store.unsaved());
}

#[test]
fn changes_are_kept_when_saved() {
    let mut store = open();
    let (inbox, sent) = (folder(&mut store, "Inbox"), folder(&mut store, "Sent Items"));
    let listed = store.list(inbox.id).unwrap();
    let (first, second) = (ItemId(listed[0].id), ItemId(listed[1].id));
    assert_eq!((inbox.total, inbox.unread), (2, 1));

    // A message is read and flagged, another goes to Sent Items and keeps who it is.
    let mut item = store.load(inbox.id, first).unwrap();
    item.set_read(true);
    item.set_flag(Flag::Complete);
    item.categories.push("Blue Category".into());
    store.update(inbox.id, first, item).unwrap();
    assert!(store.unsaved());
    let moved = store.remove(inbox.id, second).unwrap();
    assert_eq!(store.add(sent.id, moved).unwrap(), second);
    assert_eq!(store.list(sent.id).unwrap().len(), 2);
    assert!(matches!(store.load(inbox.id, second), Err(StoreError::NoItem)));

    // A message is deleted for good, a new one arrives.
    let old_sent = ItemId(store.list(sent.id).unwrap()[0].id);
    store.remove(sent.id, old_sent).unwrap();
    let mut new = mail("Written here", false);
    new.attachments.push(Attachment::file("note.txt", "text/plain", b"new".to_vec()));
    let new_id = store.add(inbox.id, new).unwrap();
    assert_ne!(new_id, old_sent);

    // Folders: one made, one renamed, one moved with what is in it, one deleted.
    let made = store.create_folder(inbox.id, "Made here").unwrap();
    let projects = folder(&mut store, "Projects");
    store.rename_folder(projects.id, "Work").unwrap();
    let deep = store.folders().unwrap().into_iter().find(|f| f.parent == Some(projects.id)).unwrap();
    store.move_folder(deep.id, made).unwrap();
    assert!(store.move_folder(inbox.id, deep.id).is_err());
    let calendar = folder(&mut store, "Calendar");
    store.delete_folder(calendar.id).unwrap();
    let in_made = store.add(made, mail("In the new folder", true)).unwrap();

    let now = store.folders().unwrap();
    let by = |name: &str| now.iter().find(|f| f.name == name).unwrap();
    assert_eq!((by("Inbox").total, by("Inbox").unread, by("Inbox").has_children), (2, 1, true));
    assert_eq!((by("Sent Items").total, by("Work").has_children, by("Made here").has_children), (1, false, true));
    assert_eq!(by(&deep.name).parent, Some(made));
    assert!(!now.iter().any(|f| f.name == "Calendar" || f.name == "Projects"));
    // Each folder is followed by what is inside it.
    let at = |id: FolderId| now.iter().position(|f| f.id == id).unwrap();
    assert!(at(inbox.id) < at(made) && at(made) < at(deep.id));

    let want = whole(&mut store);
    let mut back = saved(&mut store);
    same_mailbox(&want, &whole(&mut back));
    assert!(back.list(made).unwrap().iter().any(|s| s.id == in_made.0));
    let read = back.load(inbox.id, first).unwrap();
    assert!(read.is_read() && read.flag() == Flag::Complete && read.categories.contains(&"Blue Category".to_string()));
    assert_eq!(read.attachments.len(), 4);
    assert_eq!(back.load(inbox.id, new_id).unwrap().attachments.len(), 1);
    // The deleted folder's nodes and the deleted message are not in the new file.
    assert!(!back.ndb.nbt.contains_key(&(calendar.id.0 as u32)) && !back.ndb.nbt.contains_key(&(old_sent.0 as u32)));
    // The special folders are still the ones the store names.
    assert!(back.folders().unwrap().iter().any(|f| f.kind == FolderKind::Deleted));

    // The file that was saved can be changed and saved again.
    let again = back.create_folder(made, "Deeper").unwrap();
    let it = back.remove(inbox.id, first).unwrap();
    back.add(again, it).unwrap();
    let want = whole(&mut back);
    let mut third = saved(&mut back);
    same_mailbox(&want, &whole(&mut third));
    assert_eq!(third.load(again, first).unwrap().attachments.len(), 4);
}

#[test]
fn a_saved_file_goes_on_from_the_counters_of_the_one_it_replaces() {
    let mut store = open();
    let before = store.ndb.counters;
    // The newest message is deleted: its id is free in the tree, and is not given out again.
    let inbox = folder(&mut store, "Inbox");
    let last = store.list(inbox.id).unwrap().iter().map(|s| s.id).max().unwrap();
    let highest = store.ndb.nbt.keys().filter(|n| *n & 0x1F == 0x04).map(|n| n >> 5).max().unwrap();
    assert!(before.1[0x04] > highest, "{} {highest}", before.1[0x04]);
    store.remove(inbox.id, ItemId(last)).unwrap();
    let mut after = saved(&mut store);
    assert_eq!(after.ndb.counters.0, before.0.wrapping_add(1), "the header says it was written once more");
    for (kind, (was, is)) in before.1.iter().zip(after.ndb.counters.1).enumerate() {
        assert!(is >= *was, "kind {kind:#x}: {is} < {was}");
    }
    // A message added now takes an id no message of the file ever had.
    let inbox = folder(&mut after, "Inbox");
    let mut item = Item::default();
    item.props.set_str(pid::SUBJECT, "New");
    let new = after.add(inbox.id, item).unwrap();
    let mut again = saved(&mut after);
    let inbox = folder(&mut again, "Inbox");
    let ids: Vec<u64> = again.list(inbox.id).unwrap().iter().map(|s| s.id).collect();
    assert!(ids.iter().all(|id| (*id as u32) >> 5 != (last as u32) >> 5 || *id == last), "{ids:?}");
    assert!(ids.iter().any(|id| (*id as u32) >> 5 > highest), "{ids:?} {highest} {new:?}");
    // Counters that make no sense carry nothing a file cannot hold.
    let mut wild = open();
    wild.ndb.counters = (u32::MAX, [u32::MAX; 32]);
    let item = Item::default();
    let inbox = folder(&mut wild, "Inbox");
    let _ = wild.add(inbox.id, item);
    let _ = wild.save_to(Cursor::new(Vec::new()), UID);
}

#[test]
fn what_this_program_does_not_understand_is_kept() {
    // A file with what our own writer never makes: a hidden (associated) message in the Inbox,
    // a property with an id no name stands for, and a folder property the model has no place for.
    let mut plain = open();
    let inbox = folder(&mut plain, "Inbox");
    let listed = plain.list(inbox.id).unwrap();
    let (inbox_nid, msg) = (inbox.id.0 as u32, listed[1].id as u32);
    let hidden = (0x7000 << 5) | 0x08;
    let text = |id: u16, s: &str| Raw { id, ty: ptype::STRING, data: s.encode_utf16().flat_map(u16::to_le_bytes).collect() };
    let entry = *plain.ndb.nbt.get(&msg).unwrap();
    let mut props: Vec<Raw> = plain.open_pc(entry).unwrap().0.into_values().collect();
    props.push(Raw { id: 0x9F00, ty: ptype::INT32, data: vec![7, 0, 0, 0] });
    let folder_entry = *plain.ndb.nbt.get(&inbox_nid).unwrap();
    let mut folder_props: Vec<Raw> = plain.open_pc(folder_entry).unwrap().0.into_values().collect();
    folder_props.push(Raw { id: 0x36DA, ty: ptype::BINARY, data: vec![9; 300] });
    let put = vec![
        (hidden, inbox_nid, vec![text(pid::MESSAGE_CLASS, "IPM.Configuration.Thing"), text(pid::SUBJECT, "hidden")]),
        (msg, inbox_nid, props),
        (inbox_nid, folder_entry.parent, folder_props),
    ];
    let (out, _) = plain.save_with(Cursor::new(Vec::new()), UID, put).unwrap();
    let mut store = PstStore::from_bytes(out.into_inner()).unwrap();
    assert!(store.load(inbox.id, ItemId(u64::from(msg))).unwrap().props.get(0x9F00).is_none());

    // The message is changed, another is added to its folder, the folder is renamed.
    let mut item = store.load(inbox.id, ItemId(u64::from(msg))).unwrap();
    item.set_read(false);
    item.props.set_str(pid::BODY, "A new body.");
    item.props.0.remove(&pid::DISPLAY_TO);
    store.update(inbox.id, ItemId(u64::from(msg)), item).unwrap();
    store.add(inbox.id, mail("One more", false)).unwrap();
    store.rename_folder(inbox.id, "In").unwrap();
    let mut back = saved(&mut store);

    let raw = |store: &mut PstStore, nid: u32| {
        let e = *store.ndb.nbt.get(&nid).unwrap();
        (e.parent, store.open_pc(e).unwrap().0)
    };
    let (parent, props) = raw(&mut back, hidden);
    assert_eq!((parent, props.len()), (inbox_nid, 2));
    let (_, props) = raw(&mut back, msg);
    assert_eq!(props.get(&0x9F00).map(|r| r.data.clone()), Some(vec![7, 0, 0, 0]));
    assert!(!props.contains_key(&pid::DISPLAY_TO));
    let (_, props) = raw(&mut back, inbox_nid);
    assert_eq!(props.get(&0x36DA).map(|r| r.data.len()), Some(300));
    let now = folder(&mut back, "In");
    assert_eq!((now.total, now.unread), (3, 3));
    let got = back.load(inbox.id, ItemId(u64::from(msg))).unwrap();
    assert_eq!((got.props.str(pid::BODY), got.is_read(), got.recipients.len()), (Some("A new body."), false, 0));
    // The list shows the change, and no longer what the message lost.
    let row = back.list(inbox.id).unwrap().into_iter().find(|s| s.id == u64::from(msg)).unwrap();
    assert!(!row.read && row.to.is_empty());
}

/// The attachments of a message as they are stored, in the order of its table: the name of
/// each and the value of a property no name stands for.
fn stored_attachments(store: &mut PstStore, msg: ItemId) -> Vec<(String, Option<Vec<u8>>, i32)> {
    let e = *store.ndb.nbt.get(&(msg.0 as u32)).unwrap();
    let subs = store.ndb.subnodes(e.sub).unwrap();
    let list = store.attachments(&subs, 1252, 0);
    list.into_iter()
        .map(|(nid, att)| {
            let (data, sub) = *subs.get(&nid).unwrap();
            let props = store.open_pc(crate::ndb::NbtEntry { data, sub, parent: 0 }).unwrap().0;
            (att.name(), props.get(&0x9F01).map(|r| r.data.clone()), att.props.i32(pid::ATTACH_NUM).unwrap())
        })
        .collect()
}

#[test]
fn what_is_not_understood_on_an_attachment_is_kept_when_the_list_changes() {
    use crate::store_write::PLANTED;
    let plant = |n: Option<u8>| {
        PLANTED.with(|p| *p.borrow_mut() = n.map(|n| Raw { id: 0x9F01, ty: ptype::INT32, data: vec![n, 0, 0, 0] }).into_iter().collect());
    };
    // A message with three attachments, each with a property the model has no place for: a
    // large file, a message, and a file added by a later save.
    let mut store = open();
    let inbox = folder(&mut store, "Inbox");
    let mut item = mail("With attachments", false);
    item.attachments.push(Attachment::file("large.bin", "application/octet-stream", (0..40_000u32).map(|i| (i % 251) as u8).collect()));
    let mut inner = Attachment::file("", "", Vec::new());
    inner.props.set_str(pid::DISPLAY_NAME, "A message inside");
    inner.embedded = Some(Box::new(mail("Inside", true)));
    item.attachments.push(inner);
    let id = store.add(inbox.id, item).unwrap();
    plant(Some(7));
    let mut store = saved(&mut store);
    let mut item = store.load(inbox.id, id).unwrap();
    assert!(item.attachments.iter().all(|a| a.props.get(0x9F01).is_none()));
    item.attachments.push(Attachment::file("late.txt", "text/plain", b"late".to_vec()));
    store.update(inbox.id, id, item).unwrap();
    plant(Some(8));
    let mut store = saved(&mut store);
    plant(None);
    let seven = Some(vec![7, 0, 0, 0]);
    let eight = Some(vec![8, 0, 0, 0]);
    assert_eq!(
        stored_attachments(&mut store, id),
        [("large.bin".to_string(), seven.clone(), 0), ("A message inside".to_string(), seven.clone(), 1), ("late.txt".to_string(), eight.clone(), 2)]
    );

    // The list is turned round, one attachment is renamed, one is added, and the message
    // inside is left alone.
    let mut item = store.load(inbox.id, id).unwrap();
    let before = item.clone();
    item.attachments.reverse();
    item.attachments[2].props.set_str(pid::ATTACH_LONG_FILENAME, "renamed.bin");
    item.attachments[2].props.0.remove(&pid::ATTACH_FILENAME);
    item.attachments.insert(1, Attachment::file("new.txt", "text/plain", b"new".to_vec()));
    store.update(inbox.id, id, item.clone()).unwrap();
    let mut store = saved(&mut store);
    assert_eq!(
        stored_attachments(&mut store, id),
        [
            ("late.txt".to_string(), eight.clone(), 0),
            ("new.txt".to_string(), None, 1),
            ("A message inside".to_string(), seven.clone(), 2),
            ("renamed.bin".to_string(), seven.clone(), 3)
        ]
    );
    let got = store.load(inbox.id, id).unwrap();
    assert_eq!(got.attachments.len(), 4);
    assert_eq!(got.attachments[3].data, before.attachments[0].data);
    assert!(got.attachments[3].props.get(pid::ATTACH_FILENAME).is_none());
    assert_eq!(got.attachments[2].embedded, before.attachments[1].embedded);
    assert_eq!(got.attachments[1].data, b"new");
    assert!(got.flags() & openmailclient_model::MSGFLAG_HASATTACH != 0);

    // Attachments are taken away: what is left keeps what it had, and with none left the
    // message has no table and says it has none.
    let mut item = got;
    item.attachments.retain(|a| a.name() == "renamed.bin");
    store.update(inbox.id, id, item.clone()).unwrap();
    let mut store = saved(&mut store);
    assert_eq!(stored_attachments(&mut store, id), [("renamed.bin".to_string(), seven, 0)]);
    item.attachments.clear();
    store.update(inbox.id, id, item).unwrap();
    let mut store = saved(&mut store);
    assert!(stored_attachments(&mut store, id).is_empty());
    let got = store.load(inbox.id, id).unwrap();
    assert!(got.attachments.is_empty() && got.flags() & openmailclient_model::MSGFLAG_HASATTACH == 0);
    let e = *store.ndb.nbt.get(&(id.0 as u32)).unwrap();
    assert!(store.ndb.subnodes(e.sub).unwrap().keys().all(|nid| nid & 0x1F != 0x05 && *nid != 0x671));
}

#[test]
fn a_file_put_in_again_keeps_what_is_stored_on_it_and_what_cannot_be_read_stays() {
    use crate::store_write::PLANTED;
    let plant = |n: Option<u8>| {
        PLANTED.with(|p| *p.borrow_mut() = n.map(|n| Raw { id: 0x9F01, ty: ptype::INT32, data: vec![n, 0, 0, 0] }).into_iter().collect());
    };
    let mut store = open();
    let inbox = folder(&mut store, "Inbox");
    let mut item = mail("Worked on", false);
    item.attachments.push(Attachment::file("report.txt", "text/plain", b"first draft".to_vec()));
    item.attachments.push(Attachment::file("figures.bin", "", (0..30_000u32).map(|i| (i % 249) as u8).collect()));
    let mut sheet = Attachment::file("Sheet", "", vec![0xD0, 0xCF, 0x11, 0xE0, 1, 2, 3]);
    sheet.props.set(pid::ATTACH_METHOD, PropValue::Int32(6));
    item.attachments.push(sheet);
    let id = store.add(inbox.id, item).unwrap();
    plant(Some(7));
    let mut store = saved(&mut store);
    plant(Some(9));
    let seven = Some(vec![7, 0, 0, 0]);

    // Each file is put in again with other contents: one as it was otherwise, one with
    // another kind of contents said of it too, and the object of another program.
    let mut item = store.load(inbox.id, id).unwrap();
    item.attachments[0].data = b"second draft, longer".to_vec();
    let grown: Vec<u8> = (0..50_000u32).map(|i| (i % 241) as u8).collect();
    item.attachments[1].data = grown.clone();
    item.attachments[1].props.set_str(pid::ATTACH_MIME_TAG, "application/octet-stream");
    item.attachments[2].data = vec![0xD0, 0xCF, 0x11, 0xE0, 9, 9, 9, 9, 9];
    store.update(inbox.id, id, item).unwrap();
    let mut store = saved(&mut store);
    assert_eq!(
        stored_attachments(&mut store, id),
        [("report.txt".to_string(), seven.clone(), 0), ("figures.bin".to_string(), seven.clone(), 1), ("Sheet".to_string(), seven.clone(), 2)]
    );
    let got = store.load(inbox.id, id).unwrap();
    assert_eq!(got.attachments[0].data, b"second draft, longer");
    // The size of the whole attachment: the twenty bytes and its other properties.
    assert_eq!(got.attachments[0].props.i32(pid::ATTACH_SIZE), Some(120));
    assert_eq!((got.attachments[1].data == grown, got.attachments[1].props.str(pid::ATTACH_MIME_TAG)), (true, Some("application/octet-stream")));
    assert_eq!((got.attachments[2].data.len(), got.attachments[2].props.i32(pid::ATTACH_METHOD)), (9, Some(6)));
    // Another name and other contents: nothing says it is the file that was there.
    let mut item = got.clone();
    item.attachments[0].data = b"another file".to_vec();
    item.attachments[0].props.set_str(pid::ATTACH_LONG_FILENAME, "other.txt");
    store.update(inbox.id, id, item).unwrap();
    let mut store = saved(&mut store);
    assert_eq!(stored_attachments(&mut store, id)[0], ("other.txt".to_string(), Some(vec![9, 0, 0, 0]), 0));
    // An object carried through a save of the list has its bytes still.
    assert_eq!(store.load(inbox.id, id).unwrap().attachments[2].data, [0xD0, 0xCF, 0x11, 0xE0, 9, 9, 9, 9, 9]);

    // The second attachment cannot be read: the message shows two. One of those is taken
    // away and the message is saved; the one nobody saw is still there afterwards.
    crate::BLIND.with(|b| b.set(Some(1)));
    let mut item = store.load(inbox.id, id).unwrap();
    assert_eq!(item.attachments.iter().map(Attachment::name).collect::<Vec<_>>(), ["other.txt", "Sheet"]);
    item.attachments.remove(0);
    store.update(inbox.id, id, item).unwrap();
    let mut store = saved(&mut store);
    crate::BLIND.with(|b| b.set(None));
    let names: Vec<(String, Option<Vec<u8>>)> = stored_attachments(&mut store, id).into_iter().map(|(name, kept, _)| (name, kept)).collect();
    assert_eq!(names, [("Sheet".to_string(), seven.clone()), ("figures.bin".to_string(), seven.clone())]);
    assert!(store.load(inbox.id, id).unwrap().attachments[1].data == grown);
    // With every attachment that was shown taken away, the message still has the other.
    crate::BLIND.with(|b| b.set(Some(1)));
    let mut item = store.load(inbox.id, id).unwrap();
    item.attachments.clear();
    store.update(inbox.id, id, item).unwrap();
    let mut store = saved(&mut store);
    crate::BLIND.with(|b| b.set(None));
    plant(None);
    let got = store.load(inbox.id, id).unwrap();
    assert!(got.attachments[0].data == grown);
    assert_eq!(
        (got.attachments.len(), got.attachments[0].name(), got.flags() & openmailclient_model::MSGFLAG_HASATTACH != 0),
        (1, "figures.bin".to_string(), true)
    );
}

#[test]
fn a_search_folder_shows_a_changed_message_as_it_is_now() {
    // A search folder's table, as another program keeps one, with a row for each of two
    // messages of the Inbox and a column the messages have nothing for.
    let mut plain = open();
    let inbox = folder(&mut plain, "Inbox");
    let listed = plain.list(inbox.id).unwrap();
    let (first, second) = (listed[0].id as u32, listed[1].id as u32);
    let table = (0x7100 << 5) | 0x10;
    let text = |id: u16, s: &str| Raw { id, ty: ptype::STRING, data: s.encode_utf16().flat_map(u16::to_le_bytes).collect() };
    let int = |id: u16, n: i32| Raw { id, ty: ptype::INT32, data: n.to_le_bytes().to_vec() };
    let columns = vec![(pid::SUBJECT, ptype::STRING), (pid::DISPLAY_TO, ptype::STRING), (pid::MESSAGE_FLAGS, ptype::INT32), (0x0E05, ptype::STRING)];
    let row = |plain: &mut PstStore, nid: u32| {
        let e = *plain.ndb.nbt.get(&nid).unwrap();
        let props = plain.open_pc(e).unwrap().0;
        let mut cells: Vec<Raw> = props.into_values().filter(|r| columns.contains(&(r.id, r.ty))).collect();
        cells.push(text(0x0E05, "Inbox"));
        crate::ltp::Row { id: nid, cells }
    };
    let rows = vec![row(&mut plain, first), row(&mut plain, second)];
    crate::edit::TABLES.with(|t| t.borrow_mut().push((table, columns.clone(), rows)));
    let (out, _) = plain.save_with(Cursor::new(Vec::new()), UID, Vec::new()).unwrap();
    let mut store = PstStore::from_bytes(out.into_inner()).unwrap();
    let cells = |store: &mut PstStore| -> Vec<(u32, std::collections::BTreeMap<u16, Vec<u8>>)> {
        let (_, rows) = store.table_of(table).unwrap();
        rows.into_iter().map(|r| (r.id, r.cells.into_iter().map(|c| (c.id, c.data)).collect())).collect()
    };
    let before = cells(&mut store);
    assert_eq!(before.iter().map(|r| r.0).collect::<Vec<_>>(), [first, second]);

    // Nothing changed that the table shows: it goes across block for block.
    let unread = folder(&mut store, "Sent Items");
    store.add(unread.id, mail("Elsewhere", true)).unwrap();
    let mut store = saved(&mut store);
    assert_eq!(cells(&mut store), before);

    // One message is changed: its row says what it says now, and no longer what it lost;
    // what the table knows and the message does not is still there. The other row is as it was.
    let id = ItemId(u64::from(second));
    let mut item = store.load(inbox.id, id).unwrap();
    item.props.set_str(pid::SUBJECT, "A new subject");
    item.set_read(!item.is_read());
    item.props.0.remove(&pid::DISPLAY_TO);
    let flags = item.flags();
    store.update(inbox.id, id, item).unwrap();
    let mut store = saved(&mut store);
    let after = cells(&mut store);
    assert_eq!(after[0], before[0]);
    let (nid, row) = &after[1];
    assert_eq!(*nid, second);
    assert_eq!(row.get(&pid::SUBJECT), Some(&text(pid::SUBJECT, "A new subject").data));
    assert_eq!(row.get(&pid::MESSAGE_FLAGS), Some(&int(pid::MESSAGE_FLAGS, flags).data));
    assert_eq!(row.get(&0x0E05), Some(&text(0x0E05, "Inbox").data));
    assert!(before[1].1.contains_key(&pid::DISPLAY_TO) && !row.contains_key(&pid::DISPLAY_TO));

    // A message deleted for good leaves the table; the changed one stays as it is now.
    store.remove(inbox.id, ItemId(u64::from(first))).unwrap();
    let mut store = saved(&mut store);
    assert_eq!(cells(&mut store), [after[1].clone()]);
}

/// What waits in a file's Search Management Queue, as (type, the first three words of its data).
fn queue_of(store: &mut PstStore) -> Vec<(u16, [u32; 3])> {
    let e = *store.ndb.nbt.get(&crate::search_queue::NID_SEARCH_MANAGEMENT_QUEUE).unwrap();
    let data = store.ndb.data(e.data).unwrap();
    assert_eq!((e.parent, data.len() % crate::search_queue::SUD_SIZE), (0, 0), "the head is at the first, and each SUD whole");
    let word = |c: &[u8], at: usize| u32::from_le_bytes(c[at..at + 4].try_into().unwrap());
    data.chunks(20).map(|c| (u16::from_le_bytes([c[2], c[3]]), [word(c, 4), word(c, 8), word(c, 12)])).collect()
}

#[test]
fn what_a_save_changed_is_queued_for_the_search_folders_to_be_worked_out_again() {
    // [MS-PST] 2.4.8.4.1: each change is queued, so that the program that keeps the search
    // folders works out again which searches each message belongs to.
    let mut store = open();
    assert!(queue_of(&mut store).is_empty());
    let (inbox, sent) = (folder(&mut store, "Inbox"), folder(&mut store, "Sent Items"));
    let listed = store.list(inbox.id).unwrap();
    let (first, second) = (ItemId(listed[0].id), ItemId(listed[1].id));
    let (n, i, s) = (|id: ItemId| id.0 as u32, inbox.id.0 as u32, sent.id.0 as u32);
    let mut item = store.load(inbox.id, first).unwrap();
    item.set_read(!item.is_read());
    store.update(inbox.id, first, item).unwrap();
    let moved = store.remove(inbox.id, second).unwrap();
    store.add(sent.id, moved).unwrap();
    let gone = ItemId(store.list(sent.id).unwrap()[0].id);
    store.remove(sent.id, gone).unwrap();
    let new = store.add(inbox.id, mail("Written here", false)).unwrap();
    let made = store.create_folder(inbox.id, "Made here").unwrap();
    let projects = folder(&mut store, "Projects");
    store.rename_folder(projects.id, "Work").unwrap();
    let calendar = folder(&mut store, "Calendar");
    store.delete_folder(calendar.id).unwrap();

    let mut back = saved(&mut store);
    let q = queue_of(&mut back);
    let (made, projects, calendar) = (made.0 as u32, projects.id.0 as u32, calendar.id.0 as u32);
    let has = |t: u16, w: [u32; 3]| q.contains(&(t, w));
    assert!(has(0x0002, [i, n(first), 0]), "the message changed: {q:x?}");
    assert!(has(0x0001, [i, n(new), 0]), "the message put in");
    assert!(has(0x0004, [s, n(second), i]), "the message moved, from the Inbox to Sent Items");
    assert!(has(0x0003, [s, n(gone), 0]), "the message deleted");
    assert!(has(0x0005, [i, made, 0]) && has(0x0006, [projects, 0, 0]) && has(0x0007, [calendar, 0, 0]), "the folders");
    // Folders made come before the messages put in them, and deleted ones last.
    let at = |t: u16| q.iter().position(|(u, _)| *u == t).unwrap();
    assert!(at(0x0005) < at(0x0001) && at(0x0007) == q.len() - 1);
    assert_eq!(q.len(), 7, "nothing else: {q:x?}");

    // What waits is kept, and what the next save changed goes behind it; a save of nothing
    // changed adds nothing.
    let mut item = back.load(inbox.id, first).unwrap();
    item.set_flag(Flag::Flagged);
    back.update(inbox.id, first, item).unwrap();
    let mut third = saved(&mut back);
    let q3 = queue_of(&mut third);
    assert_eq!((&q3[..7], q3.get(7)), (&q[..], Some(&(0x0002, [i, n(first), 0]))));
    assert_eq!(q3.len(), 8);
    let mut fourth = saved(&mut third);
    assert_eq!(queue_of(&mut fourth), q3);
}

pub(super) fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("omc-edit-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_file_on_disk_is_replaced_whole_or_not_at_all() {
    let dir = scratch("save");
    let path = dir.join("box.pst");
    let bytes = save(&mut mailbox()).0;
    std::fs::write(&path, &bytes).unwrap();
    let mut store = PstStore::open(&path).unwrap();
    assert!(store.writable());
    // Looking does not write.
    let inbox = folder(&mut store, "Inbox");
    let first = ItemId(store.list(inbox.id).unwrap()[0].id);
    assert_eq!(store.save(UID).unwrap(), Written::default());
    assert!(std::fs::read(&path).unwrap() == bytes);

    let mut item = store.load(inbox.id, first).unwrap();
    item.set_read(true);
    store.update(inbox.id, first, item).unwrap();
    assert!(std::fs::read(&path).unwrap() == bytes, "a change is not written until it is saved");
    let done = store.save(UID).unwrap();
    assert!(done.folders > 0 && !store.unsaved());
    // Beside the file there is now the file as it was, and nothing else.
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    assert!(std::fs::read(crate::original_beside(&path)).unwrap() == bytes);
    // The open store goes on from the new file, and so does anyone who opens it.
    assert!(store.load(inbox.id, first).unwrap().is_read());
    assert_eq!(folder(&mut store, "Inbox").unread, 0);
    let mut fresh = PstStore::open(&path).unwrap();
    assert!(fresh.load(inbox.id, first).unwrap().is_read());
    assert_eq!(store.save(UID).unwrap(), Written::default());
    drop(fresh);

    // Another program writes the file: the next save is refused and the file is left alone.
    let mut item = store.load(inbox.id, first).unwrap();
    item.set_read(false);
    store.update(inbox.id, first, item).unwrap();
    let theirs = std::fs::read(&path).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(120);
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(later).unwrap();
    assert!(store.save(UID).is_err());
    assert!(store.unsaved() && std::fs::read(&path).unwrap() == theirs);
    // Beside the file: the file as it first was, and the journal of what is not saved.
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
    assert!(crate::journal_beside(&path).exists());
    // The file as it first was is not written over by later saves.
    assert!(std::fs::read(crate::original_beside(&path)).unwrap() == bytes);
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_save_done_elsewhere_replaces_the_file_whole_or_not_at_all() {
    use openmailclient_model::Progress;
    use std::sync::Arc;
    let dir = scratch("save-elsewhere");
    let path = dir.join("box.pst");
    let bytes = save(&mut mailbox()).0;
    std::fs::write(&path, &bytes).unwrap();
    let mut store = PstStore::open(&path).unwrap();
    let inbox = folder(&mut store, "Inbox");
    let first = ItemId(store.list(inbox.id).unwrap()[0].id);
    // Nothing to save: no work is given.
    assert!(store.save_begin(UID).unwrap().is_none() && !store.saving());
    let change = |store: &mut PstStore, read: bool| {
        let mut item = store.load(inbox.id, first).unwrap();
        item.set_read(read);
        store.update(inbox.id, first, item)
    };
    change(&mut store, true).unwrap();

    // The work is done on another thread while the store is read; it takes no changes then.
    let work = store.save_begin(UID).unwrap().unwrap();
    assert!(store.saving() && store.unsaved());
    let progress = Arc::new(Progress::default());
    let theirs = progress.clone();
    let thread = std::thread::spawn(move || work(theirs));
    assert!(store.load(inbox.id, first).unwrap().is_read());
    assert!(change(&mut store, false).is_err() && store.save(UID).is_err() && store.save_begin(UID).is_err());
    assert!(store.create_folder(inbox.id, "No").is_err());
    let done = thread.join().unwrap();
    assert!(std::fs::read(&path).unwrap() == bytes, "the data file is as it was until the save is ended");
    assert!(progress.fraction() > 0.99);
    store.save_end(done).unwrap();
    assert!(!store.saving() && !store.unsaved());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    assert!(std::fs::read(crate::original_beside(&path)).unwrap() == bytes);
    assert!(PstStore::open(&path).unwrap().load(inbox.id, first).unwrap().is_read());
    assert_eq!(folder(&mut store, "Inbox").unread, 0);
    let saved = std::fs::read(&path).unwrap();

    // A save that is stopped leaves the file and the changes as they were, and nothing beside them
    // but the journal; the store takes changes again.
    change(&mut store, false).unwrap();
    let work = store.save_begin(UID).unwrap().unwrap();
    let progress = Arc::new(Progress::default());
    progress.stop();
    let done = work(progress);
    assert!(done.is_err());
    assert!(store.save_end(done).is_err());
    assert!(!store.saving() && store.unsaved() && std::fs::read(&path).unwrap() == saved);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
    store.create_folder(inbox.id, "Yes").unwrap();

    // Another program writes the file while the work runs: nothing is replaced. (The work
    // is a whole new file here; one that goes into the file is in `tests_onto.rs`.)
    store.save_in_place(false);
    let work = store.save_begin(UID).unwrap().unwrap();
    let done = work(Arc::new(Progress::default()));
    assert!(done.is_ok());
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(120);
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(later).unwrap();
    assert!(store.save_end(done).is_err());
    assert!(!store.saving() && store.unsaved() && std::fs::read(&path).unwrap() == saved);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
    // Ending a save that was not begun does nothing.
    store.save_end(Err(StoreError::NoItem)).unwrap();
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn a_file_another_program_has_open_is_not_replaced() {
    let dir = scratch("held");
    let path = dir.join("box.pst");
    std::fs::write(&path, save(&mut mailbox()).0).unwrap();
    let mut store = PstStore::open(&path).unwrap();
    let inbox = folder(&mut store, "Inbox");
    let first = ItemId(store.list(inbox.id).unwrap()[0].id);
    let mut item = store.load(inbox.id, first).unwrap();
    item.set_read(true);
    store.update(inbox.id, first, item).unwrap();
    let before = std::fs::read(&path).unwrap();
    let theirs = std::fs::File::open(&path).unwrap();
    assert!(store.save(UID).is_err());
    assert!(store.unsaved() && std::fs::read(&path).unwrap() == before);
    // Nothing is left of the save that failed; the journal of the change is still there.
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    assert!(crate::journal_beside(&path).exists());
    // The store still reads the file it had, with the change on top.
    assert!(store.load(inbox.id, first).unwrap().is_read());
    drop(theirs);
    store.save(UID).unwrap();
    assert!(PstStore::open(&path).unwrap().load(inbox.id, first).unwrap().is_read());
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_ost_is_only_read() {
    let dir = scratch("ost");
    let path = dir.join("copy.ost");
    std::fs::write(&path, save(&mut mailbox()).0).unwrap();
    let mut store = PstStore::open(&path).unwrap();
    assert!(!store.writable());
    let inbox = folder(&mut store, "Inbox");
    let first = ItemId(store.list(inbox.id).unwrap()[0].id);
    let item = store.load(inbox.id, first).unwrap();
    assert!(matches!(store.update(inbox.id, first, item), Err(StoreError::ReadOnly)));
    assert!(matches!(store.create_folder(inbox.id, "No"), Err(StoreError::ReadOnly)));
    assert!(matches!(store.remove(inbox.id, first), Err(StoreError::ReadOnly)));
    assert!(!store.unsaved());
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_ids_are_refused() {
    let mut store = open();
    let inbox = folder(&mut store, "Inbox");
    for id in [0, 1, u64::from(u32::MAX), u64::MAX, inbox.id.0] {
        assert!(store.update(inbox.id, ItemId(id), Item::default()).is_err());
        assert!(store.remove(inbox.id, ItemId(id)).is_err());
        if id != inbox.id.0 {
            assert!(store.add(FolderId(id), Item::default()).is_err());
            assert!(store.rename_folder(FolderId(id), "x").is_err());
            assert!(store.delete_folder(FolderId(id)).is_err());
            assert!(store.move_folder(FolderId(id), inbox.id).is_err());
            assert!(store.move_folder(inbox.id, FolderId(id)).is_err());
            assert!(store.create_folder(FolderId(id), "x").is_err());
        }
    }
    assert!(store.move_folder(inbox.id, inbox.id).is_err());
    // A message of junk is written or refused; either way the save does not fall over.
    let mut junk = Item::default();
    junk.props.set(pid::SUBJECT, PropValue::Binary(vec![0xFF; 70_000]));
    junk.props.set(0x7FFF, PropValue::Other { ty: 0xFFFF, raw: vec![1; 9] });
    store.add(inbox.id, junk).unwrap();
    let _ = store.save_to(Cursor::new(Vec::new()), UID);
}

#[test]
fn a_damaged_file_is_changed_and_saved_or_refused_never_a_panic() {
    let bytes = save(&mut mailbox()).0;
    let step = (bytes.len() / 240).max(1);
    let mut opened = 0;
    for at in (0..bytes.len()).step_by(step) {
        let mut bad = bytes.clone();
        for b in bad.iter_mut().skip(at).take(3) {
            *b ^= 0xA5;
        }
        let Ok(mut store) = PstStore::from_bytes(bad) else { continue };
        opened += 1;
        let Ok(folders) = store.folders() else { continue };
        for f in folders.iter().take(6) {
            let Ok(list) = store.list(f.id) else { continue };
            if let Some(s) = list.first() {
                if let Ok(mut item) = store.load(f.id, ItemId(s.id)) {
                    item.set_read(!item.is_read());
                    if !item.attachments.is_empty() {
                        item.attachments.rotate_left(1);
                        item.attachments.push(Attachment::file("more.txt", "text/plain", b"more".to_vec()));
                    }
                    let _ = store.update(f.id, ItemId(s.id), item);
                }
                if let (Some(s), Some(to)) = (list.get(1), folders.last())
                    && let Ok(item) = store.remove(f.id, ItemId(s.id))
                {
                    let _ = store.add(to.id, item);
                }
            }
            let _ = store.create_folder(f.id, "New");
        }
        if let Some(f) = folders.get(2) {
            let _ = store.delete_folder(f.id);
        }
        if let Ok((out, _)) = store.save_to(Cursor::new(Vec::new()), UID) {
            // What comes out may be missing what the damage took; it is still read without a panic.
            if let Ok(mut back) = PstStore::from_bytes(out.into_inner()) {
                let folders = back.folders().unwrap_or_default();
                for f in folders {
                    let _ = back.list(f.id);
                }
            }
        }
    }
    assert!(opened > 20);
}

#[test]
fn a_copy_of_a_data_file_has_what_this_program_does_not_understand() {
    // The source has a hidden message, an unknown property and a folder property of its own.
    let mut plain = open();
    let inbox = folder(&mut plain, "Inbox");
    let (inbox_nid, msg) = (inbox.id.0 as u32, plain.list(inbox.id).unwrap()[1].id as u32);
    let hidden = (0x7000 << 5) | 0x08;
    let text = |id: u16, s: &str| Raw { id, ty: ptype::STRING, data: s.encode_utf16().flat_map(u16::to_le_bytes).collect() };
    let entry = *plain.ndb.nbt.get(&msg).unwrap();
    let mut props: Vec<Raw> = plain.open_pc(entry).unwrap().0.into_values().collect();
    props.push(Raw { id: 0x9F00, ty: ptype::INT32, data: vec![7, 0, 0, 0] });
    let folder_entry = *plain.ndb.nbt.get(&inbox_nid).unwrap();
    let mut folder_props: Vec<Raw> = plain.open_pc(folder_entry).unwrap().0.into_values().collect();
    folder_props.push(Raw { id: 0x36DA, ty: ptype::BINARY, data: vec![9; 300] });
    let put = vec![
        (hidden, inbox_nid, vec![text(pid::MESSAGE_CLASS, "IPM.Configuration.Thing"), text(pid::SUBJECT, "hidden")]),
        (msg, inbox_nid, props),
        (inbox_nid, folder_entry.parent, folder_props),
    ];
    let (out, _) = plain.save_with(Cursor::new(Vec::new()), UID, put).unwrap();
    let dir = scratch("copy");
    let (from, to) = (dir.join("from.pst"), dir.join("to.pst"));
    std::fs::write(&from, out.into_inner()).unwrap();

    // Changes that are not saved go into the copy; the source stays as it is, unsaved still.
    let mut store = PstStore::open(&from).unwrap();
    store.add(inbox.id, mail("Not saved yet", false)).unwrap();
    store.create_folder(inbox.id, "Under the Inbox").unwrap();
    let want = whole(&mut store);
    let done = store.copy_to(&to, UID).unwrap().unwrap();
    assert_eq!(done.folders, want.len() as u64);
    assert_eq!(done.items, want.iter().map(|f| f.2.len() as u64).sum::<u64>());
    assert!(store.unsaved());
    let first = std::fs::read(&from).unwrap();
    drop(store);
    assert_eq!(std::fs::read(&from).unwrap(), first);

    let found = check(&mut Cursor::new(std::fs::read(&to).unwrap())).unwrap();
    assert!(found.sound(), "{:?} {:?}", found.problems, found.inside);
    let mut back = PstStore::open(&to).unwrap();
    assert!(!back.unsaved());
    same_mailbox(&want, &whole(&mut back));
    let raw = |store: &mut PstStore, nid: u32| {
        let e = *store.ndb.nbt.get(&nid).unwrap();
        (e.parent, store.open_pc(e).unwrap().0)
    };
    let (parent, props) = raw(&mut back, hidden);
    assert_eq!((parent, props.len()), (inbox_nid, 2));
    assert_eq!(raw(&mut back, msg).1.get(&0x9F00).map(|r| r.data.clone()), Some(vec![7, 0, 0, 0]));
    assert_eq!(raw(&mut back, inbox_nid).1.get(&0x36DA).map(|r| r.data.len()), Some(300));
    drop(back);

    // A data file that is only ever read is not copied this way: the caller takes the model's.
    let ost = dir.join("from.ost");
    std::fs::copy(&from, &ost).unwrap();
    assert_eq!(PstStore::open(&ost).unwrap().copy_to(&dir.join("no.pst"), UID).unwrap(), None);
    assert!(!dir.join("no.pst").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_value_of_another_type_than_its_column_is_shown_as_the_column_holds_it() {
    use crate::store_write::{as_type, cell_for};
    let mut store = open();
    let inbox = folder(&mut store, "Inbox");
    let id = ItemId(store.list(inbox.id).unwrap()[0].id);
    let table = (inbox.id.0 as u32 & !0x1F) | 0x0E;
    // A column of 32-bit numbers ([MS-PST] 2.4.4.5.1), and a message that has the value as
    // a number of another width.
    const COLUMN: u16 = 0x1097;
    let put = |store: &mut PstStore, v: PropValue| -> (PstStore, Option<(u16, Vec<u8>)>) {
        let mut item = store.load(inbox.id, id).unwrap();
        item.props.0.insert(COLUMN, v.clone());
        store.update(inbox.id, id, item).unwrap();
        let mut next = saved(store);
        // The message has the value as it was given, whatever the list can show of it.
        assert_eq!(next.load(inbox.id, id).unwrap().props.0.get(&COLUMN), Some(&v));
        let (columns, rows) = next.table_of(table).unwrap();
        assert_eq!(columns.iter().filter(|c| c.0 == COLUMN).collect::<Vec<_>>(), [&(COLUMN, ptype::INT32)], "one column, of the type it had");
        let cell = rows.into_iter().find(|r| u64::from(r.id) == id.0).unwrap().cells.into_iter().find(|c| c.id == COLUMN);
        (next, cell.map(|c| (c.ty, c.data)))
    };
    let (mut store, cell) = put(&mut store, PropValue::Int32(5));
    assert_eq!(cell, Some((ptype::INT32, 5i32.to_le_bytes().to_vec())));
    // Narrower and wider numbers go in as the column's kind.
    let (mut store, cell) = put(&mut store, PropValue::Int16(-7));
    assert_eq!(cell, Some((ptype::INT32, (-7i32).to_le_bytes().to_vec())));
    let (mut store, cell) = put(&mut store, PropValue::Int64(9));
    assert_eq!(cell, Some((ptype::INT32, 9i32.to_le_bytes().to_vec())));
    // One the column has no room for leaves the cell empty: not the 9 it said before.
    let (mut store, cell) = put(&mut store, PropValue::Int64(1 << 40));
    assert_eq!(cell, None);
    // Nor is a value of a kind that is no number made into one.
    let (_, cell) = put(&mut store, PropValue::Binary(vec![1, 2, 3, 4]));
    assert_eq!(cell, None);

    let raw = |ty: u16, data: &[u8]| Raw { id: 1, ty, data: data.to_vec() };
    let made = |r: Raw, ty: u16| as_type(&r, ty).map(|c| (c.ty, c.data));
    assert_eq!(made(raw(ptype::STRING8, b"Ab"), ptype::STRING), Some((ptype::STRING, vec![0x41, 0, 0x62, 0])));
    assert_eq!(made(raw(ptype::STRING, &[0x41, 0, 0x62, 0]), ptype::STRING8), Some((ptype::STRING8, b"Ab".to_vec())));
    // Text beyond 7 bits depends on a code page, and is not guessed at.
    assert_eq!(made(raw(ptype::STRING8, &[0x41, 0xE9]), ptype::STRING), None);
    assert_eq!(made(raw(ptype::STRING, &[0xE9, 0, 0x41, 0]), ptype::STRING8), None);
    assert_eq!(made(raw(ptype::STRING, &[0x41, 0, 0x62]), ptype::STRING8), None);
    assert_eq!(made(raw(ptype::FLOAT32, &1.5f32.to_le_bytes()), ptype::FLOAT64), Some((ptype::FLOAT64, 1.5f64.to_le_bytes().to_vec())));
    assert_eq!(made(raw(ptype::FLOAT64, &1.5f64.to_le_bytes()), ptype::FLOAT32), Some((ptype::FLOAT32, 1.5f32.to_le_bytes().to_vec())));
    assert_eq!(made(raw(ptype::FLOAT64, &0.1f64.to_le_bytes()), ptype::FLOAT32), None);
    assert_eq!(made(raw(ptype::INT32, &70_000i32.to_le_bytes()), ptype::INT16), None);
    // Values cut short, as a damaged file may have them, are no value.
    assert_eq!(made(raw(ptype::INT32, &[1, 2]), ptype::INT64), None);
    assert_eq!(made(raw(ptype::FLOAT64, &[1, 2, 3]), ptype::FLOAT32), None);
    // A table without a column of that id gets no cell for it.
    assert!(cell_for(&[(2, ptype::INT32)], &raw(ptype::INT16, &[1, 0])).is_none());
    assert!(cell_for(&[(1, ptype::INT32)], &raw(ptype::INT16, &[1, 0])).is_some());
}
