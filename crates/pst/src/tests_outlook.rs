//! What Outlook Classic asks of a data file beyond what [MS-PST] 2.7.1 lists. Its Inbox
//! Repair Tool named each of these when it was run on the files this crate writes, and
//! Outlook did not load a file that lacked the nodes (seen 2026-10-13,
//! `docs/file-format-parity.md`). The tests hold the writer to what was learned.

use crate::check::check;
use crate::ltp::{self, Raw};
use crate::ndb::{Ndb, u16_at, u32_at};
use crate::tests_store::{UID, mailbox, save};
use crate::{PstStore, write_store_ansi};
use openmailclient_model::{Store, pid, ptype};
use std::io::Cursor;

/// The mailbox with a little of everything, as a Unicode file and as an ANSI one.
fn files() -> Vec<(Vec<u8>, bool)> {
    let (unicode, _) = save(&mut mailbox());
    let (out, _) = write_store_ansi(Cursor::new(Vec::new()), &mut mailbox(), UID, None).unwrap();
    let ansi = out.into_inner();
    assert!(check(&mut Cursor::new(&ansi)).unwrap().sound());
    vec![(unicode, false), (ansi, true)]
}

fn open(bytes: &[u8]) -> Ndb<'static> {
    Ndb::open(Box::new(Cursor::new(bytes.to_vec()))).unwrap()
}

fn props(ndb: &mut Ndb, data: u64, sub: u64) -> Vec<Raw> {
    let subs = if sub == 0 { Default::default() } else { ndb.subnodes(sub).unwrap() };
    ltp::read_pc(ndb, data, &subs).unwrap()
}

fn table(ndb: &mut Ndb, data: u64, sub: u64) -> Vec<ltp::Row> {
    let subs = if sub == 0 { Default::default() } else { ndb.subnodes(sub).unwrap() };
    ltp::read_tc(ndb, data, &subs).unwrap()
}

fn int(list: &[Raw], id: u16) -> Option<u32> {
    list.iter().find(|p| p.id == id).and_then(|p| u32_at(&p.data, 0))
}

/// The bytes of a node's data and of all its subnodes' data.
fn stored(ndb: &mut Ndb, data: u64, sub: u64) -> usize {
    let mut n = ndb.data(data).unwrap().len();
    if sub != 0 {
        for (data, sub) in ndb.subnodes(sub).unwrap().into_values() {
            n += stored(ndb, data, sub);
        }
    }
    n
}

/// An allocation of a heap that is one block ([MS-PST] 2.3.1): HNHDR, then HNPAGEMAP.
fn alloc(block: &[u8], hid: u32) -> &[u8] {
    let map = usize::from(u16_at(block, 0).unwrap());
    let index = (hid >> 5) as usize;
    let at = |i: usize| usize::from(u16_at(block, map + 4 + 2 * i).unwrap());
    &block[at(index - 1)..at(index)]
}

#[test]
fn the_nodes_outlook_asks_for_are_there() {
    for (bytes, ansi) in files() {
        let mut ndb = open(&bytes);
        // The receive folder table, the outgoing queue, three more tables new ones are made
        // from, the search folders' update queue, and the parts of the one search folder.
        for nid in [0x62Bu32, 0x64C, 0x6B6, 0x6D7, 0x6F8, 0xEC1, 0x2226, 0x2227, 0x2230] {
            assert!(ndb.nbt.contains_key(&nid), "node {nid:#x} is missing (ansi: {ansi})");
        }
        // The store says it has no password, by having the property.
        let store = *ndb.nbt.get(&0x21).unwrap();
        assert_eq!(int(&props(&mut ndb, store.data, store.sub), 0x67FF), Some(0));
        // The search folder is one of the root's rows and on the list of search folders.
        let root = *ndb.nbt.get(&0x12D).unwrap();
        assert!(table(&mut ndb, root.data, root.sub).iter().any(|r| r.id == 0x2223));
        let listed = *ndb.nbt.get(&0x201).unwrap();
        assert_eq!(ndb.data(listed.data).unwrap(), 0x2223u32.to_le_bytes());
        let criteria = *ndb.nbt.get(&0x2227).unwrap();
        assert_eq!(int(&props(&mut ndb, criteria.data, criteria.sub), 0x660B), Some(0));
        // Mail of any class is received in the Inbox.
        let receive = *ndb.nbt.get(&0x62B).unwrap();
        let rows = table(&mut ndb, receive.data, receive.sub);
        assert_eq!(rows.len(), 1);
        let inbox = int(&rows[0].cells, 0x6605).unwrap();
        let node = *ndb.nbt.get(&inbox).unwrap();
        let name = props(&mut ndb, node.data, node.sub).into_iter().find(|p| p.id == pid::DISPLAY_NAME).unwrap();
        let name = if ansi {
            String::from_utf8(name.data).unwrap()
        } else {
            String::from_utf16(&name.data.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()).unwrap()
        };
        assert_eq!(name, "Inbox");
        // A search folder is not one of the mailbox's folders.
        let mut back = PstStore::from_bytes(bytes.clone()).unwrap();
        assert!(back.folders().unwrap().iter().all(|f| !f.name.contains("SPAM")));
    }
}

#[test]
fn folder_tables_have_the_columns_outlook_asks_for() {
    for (bytes, _) in files() {
        let mut ndb = open(&bytes);
        let mut columns = |nid: u32| {
            let e = *ndb.nbt.get(&nid).unwrap();
            ltp::read_table(&mut ndb, e.data, &Default::default()).unwrap().0
        };
        // The hierarchy, contents and associated contents tables new ones are made from.
        assert!(columns(0x60D).contains(&(0x0E30, ptype::BINARY)));
        let contents = columns(0x60E);
        assert!(contents.contains(&(0x0E30, ptype::BINARY)) && contents.contains(&(0x3013, ptype::BINARY)));
        assert!(columns(0x60F).contains(&(0x682F, ptype::STRING)) || columns(0x60F).contains(&(0x682F, ptype::STRING8)));
    }
}

#[test]
fn two_tables_of_an_ansi_file_have_a_row_index_four_bytes_wide() {
    let (bytes, _) = files().remove(1);
    let mut ndb = open(&bytes);
    // BTHHEADER ([MS-PST] 2.3.2.1): cbEnt is its third byte; TCINFO names it at byte 10.
    let mut entry = |nid: u32| {
        let e = *ndb.nbt.get(&nid).unwrap();
        let block = ndb.data(e.data).unwrap();
        let info = alloc(&block, u32_at(&block, 4).unwrap());
        alloc(&block, u32_at(info, 10).unwrap())[2]
    };
    assert_eq!((entry(0x6D7), entry(0x6F8)), (4, 4));
    // The others have the two bytes of an ANSI file ([MS-PST] 2.3.4.3.1).
    assert_eq!((entry(0x60D), entry(0x6B6), entry(0x692)), (2, 2, 2));
}

#[test]
fn sizes_are_what_is_stored_and_rows_say_the_same() {
    for (bytes, ansi) in files() {
        let mut ndb = open(&bytes);
        let mut messages: Vec<u32> = ndb.nbt.keys().copied().filter(|n| n & 0x1F == 0x04).collect();
        messages.sort_unstable();
        assert_eq!(messages.len(), 4);
        let (mut files_seen, mut objects, mut inside) = (0, 0, 0);
        for nid in messages {
            let e = *ndb.nbt.get(&nid).unwrap();
            // PidTagMessageSize: the message's own bytes and those of its subnodes.
            let size = int(&props(&mut ndb, e.data, e.sub), pid::MESSAGE_SIZE).unwrap();
            assert_eq!(size as usize, stored(&mut ndb, e.data, e.sub), "message {nid:#x} (ansi: {ansi})");
            // Its row in the folder's contents table says the same.
            let contents = e.parent & !0x1F | 0x0E;
            let t = *ndb.nbt.get(&contents).unwrap();
            let row = table(&mut ndb, t.data, t.sub).into_iter().find(|r| r.id == nid).unwrap();
            assert_eq!(int(&row.cells, pid::MESSAGE_SIZE), Some(size));

            let subs = ndb.subnodes(e.sub).unwrap();
            // Recipients are numbered from 1.
            let (data, sub) = *subs.get(&0x692).unwrap();
            let ids: Vec<u32> = table(&mut ndb, data, sub).iter().map(|r| r.id).collect();
            assert_eq!(ids, [1, 2]);
            let Some(&(data, sub)) = subs.get(&0x671) else { continue };
            for row in table(&mut ndb, data, sub) {
                let (data, sub) = *subs.get(&row.id).unwrap();
                let list = props(&mut ndb, data, sub);
                // PidTagAttachSize: the sizes of all the values added up, a Boolean one byte
                // and an object the size it says it has.
                let want: usize = list
                    .iter()
                    .map(|p| match p.ty {
                        ptype::OBJECT => u32_at(&p.data, 4).unwrap() as usize,
                        ptype::BOOL => 1,
                        _ => p.data.len(),
                    })
                    .sum();
                let size = int(&list, pid::ATTACH_SIZE).unwrap();
                assert_eq!(size as usize, want, "attachment {:#x} of {nid:#x} (ansi: {ansi})", row.id);
                assert_eq!(int(&row.cells, pid::ATTACH_SIZE), Some(size));
                files_seen += 1;
                // An object's storage is in a subnode of a kind of its own; a message inside
                // is a message.
                let Some(object) = list.iter().find(|p| p.id == pid::ATTACH_DATA && p.ty == ptype::OBJECT) else { continue };
                let (named, said) = (u32_at(&object.data, 0).unwrap(), u32_at(&object.data, 4).unwrap() as usize);
                let (data, sub) = *ndb.subnodes(sub).unwrap().get(&named).unwrap();
                match int(&list, pid::ATTACH_METHOD) {
                    Some(6) => {
                        assert_eq!((named & 0x1F, said, ndb.data(data).unwrap().len()), (0x09, 512, 512));
                        objects += 1;
                    }
                    _ => {
                        assert_eq!(named & 0x1F, 0x04);
                        assert_eq!(said, int(&props(&mut ndb, data, sub), pid::MESSAGE_SIZE).unwrap() as usize);
                        assert_eq!(said, stored(&mut ndb, data, sub));
                        inside += 1;
                    }
                }
            }
        }
        assert_eq!((files_seen, objects, inside), (5, 1, 1));
    }
}

/// Every message in the file has as its size what is stored of it, and its row says the same.
fn sizes_hold(bytes: &[u8], what: &str) -> usize {
    let mut ndb = open(bytes);
    let messages: Vec<u32> = ndb.nbt.keys().copied().filter(|n| n & 0x1F == 0x04).collect();
    for nid in &messages {
        let e = *ndb.nbt.get(nid).unwrap();
        let size = int(&props(&mut ndb, e.data, e.sub), pid::MESSAGE_SIZE).unwrap();
        assert_eq!(size as usize, stored(&mut ndb, e.data, e.sub), "message {nid:#x} ({what})");
        let t = *ndb.nbt.get(&(e.parent & !0x1F | 0x0E)).unwrap();
        let row = table(&mut ndb, t.data, t.sub).into_iter().find(|r| r.id == *nid).unwrap();
        assert_eq!(int(&row.cells, pid::MESSAGE_SIZE), Some(size), "the row of {nid:#x} ({what})");
    }
    messages.len()
}

#[test]
fn a_message_changed_by_a_save_has_the_size_now_stored() {
    use crate::tests_edit::{folder, scratch};
    use openmailclient_model::{Attachment, ItemId};
    for (bytes, ansi) in files() {
        for in_place in [true, false] {
            let what = format!("ansi: {ansi}, in place: {in_place}");
            let dir = scratch(&format!("sizes-{ansi}-{in_place}"));
            let path = dir.join("box.pst");
            std::fs::write(&path, &bytes).unwrap();
            let mut store = PstStore::open(&path).unwrap();
            store.keep_original(false);
            store.save_in_place(in_place);
            let inbox = folder(&mut store, "Inbox");
            let listed = store.list(inbox.id).unwrap();
            // One message only marked read: its recipients and attachments stay as stored.
            let first = ItemId(listed[0].id);
            let mut item = store.load(inbox.id, first).unwrap();
            item.set_read(!item.is_read());
            store.update(inbox.id, first, item).unwrap();
            // One given a file: it is larger by about as much.
            let second = ItemId(listed[1].id);
            let was = store.list(inbox.id).unwrap()[1].size;
            let mut item = store.load(inbox.id, second).unwrap();
            item.attachments.push(Attachment::file("added.bin", "application/octet-stream", vec![7; 20_000]));
            store.update(inbox.id, second, item).unwrap();
            store.save(UID).unwrap();
            assert_eq!(store.saved_in_place(), u64::from(in_place), "{what}");
            let now = std::fs::read(&path).unwrap();
            assert!(sizes_hold(&now, &what) >= 4);
            let size = PstStore::open(&path).unwrap().list(inbox.id).unwrap()[1].size;
            assert!(size >= was + 20_000, "{was} then {size} ({what})");
            // Once more, on what the first save wrote.
            let mut item = store.load(inbox.id, first).unwrap();
            item.set_read(!item.is_read());
            store.update(inbox.id, first, item).unwrap();
            store.save(UID).unwrap();
            sizes_hold(&std::fs::read(&path).unwrap(), &what);
            drop(store);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// The nodes the files this program wrote before M3c.1 lack.
const ASKED: [u32; 6] = [0x62B, 0x64C, 0x6B6, 0x6D7, 0x6F8, 0xEC1];

/// The mailbox as a Unicode and an ANSI file without the nodes Outlook asks for.
fn old_files() -> Vec<(Vec<u8>, bool)> {
    crate::write::LEAVE_OUT.with(|l| *l.borrow_mut() = ASKED.to_vec());
    // Blocks the nodes left out shared are counted as they were: the file is as sound as the
    // ones written then, which the structure check does not ask about here.
    let (unicode, _) = crate::write_store(Cursor::new(Vec::new()), &mut mailbox(), UID).unwrap();
    let (out, _) = write_store_ansi(Cursor::new(Vec::new()), &mut mailbox(), UID, None).unwrap();
    crate::write::LEAVE_OUT.with(|l| l.borrow_mut().clear());
    vec![(unicode.into_inner(), false), (out.into_inner(), true)]
}

#[test]
fn a_file_without_what_outlook_asks_for_is_written_whole_keeping_its_ids() {
    use crate::tests_edit::{folder, same_mailbox, scratch, whole};
    use openmailclient_model::ItemId;
    for (bytes, ansi) in old_files() {
        let ndb = open(&bytes);
        assert!(ASKED.iter().all(|n| !ndb.nbt.contains_key(n)));
        drop(ndb);
        let dir = scratch(&format!("renew-{ansi}"));
        let path = dir.join("old.pst");
        std::fs::write(&path, &bytes).unwrap();
        let mut store = PstStore::open(&path).unwrap();
        store.keep_original(false);
        assert!(store.renews());
        let inbox = folder(&mut store, "Inbox");
        let first = ItemId(store.list(inbox.id).unwrap()[0].id);
        let mut item = store.load(inbox.id, first).unwrap();
        item.set_read(!item.is_read());
        store.update(inbox.id, first, item).unwrap();
        let want = whole(&mut store);
        store.save(UID).unwrap();
        // Written whole, not into the file where it is: it now has what Outlook asks for.
        assert_eq!(store.saved_in_place(), 0);
        let now = std::fs::read(&path).unwrap();
        assert_eq!(now.get(10), Some(if ansi { &14 } else { &23 }));
        let mut ndb = open(&now);
        assert!(ASKED.iter().all(|n| ndb.nbt.contains_key(n)), "ansi: {ansi}");
        assert!(ndb.nbt.contains_key(&0x2226) && !ndb.data(*ndb.nbt.get(&0x201).map(|e| &e.data).unwrap()).unwrap().is_empty());
        drop(ndb);
        assert!(check(&mut Cursor::new(&now)).unwrap().sound());
        sizes_hold(&now, "renewed");
        // Every folder and item has the id it had, and what it held.
        same_mailbox(&want, &whole(&mut store));
        same_mailbox(&want, &whole(&mut PstStore::open(&path).unwrap()));
        assert!(!store.renews());
        // The next save goes into the file where it is.
        let mut item = store.load(inbox.id, first).unwrap();
        item.set_read(!item.is_read());
        store.update(inbox.id, first, item).unwrap();
        store.save(UID).unwrap();
        assert_eq!(store.saved_in_place(), 1);
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_file_without_what_outlook_asks_for_and_a_hidden_row_that_cannot_be_read_is_copied_as_stored() {
    use crate::tests_edit::{folder, scratch};
    use openmailclient_model::ItemId;
    let (bytes, _) = old_files().remove(0);
    let mut plain = PstStore::from_bytes(bytes).unwrap();
    // A row in the Inbox's table of associated items for a hidden item that is not there:
    // writing the file anew would lose the row, so it is copied as it is stored.
    let inbox = folder(&mut plain, "Inbox");
    let table = u32::try_from(inbox.id.0).unwrap() & !0x1F | 0x0F;
    let (columns, _) = plain.table_of(table).unwrap();
    let hidden = ltp::Row { id: 0x7FFF_0004 & !0x1F | 0x04, cells: Vec::new() };
    crate::edit::TABLES.with(|t| t.borrow_mut().push((table, columns, vec![hidden])));
    crate::write::LEAVE_OUT.with(|l| *l.borrow_mut() = ASKED.to_vec());
    let (out, _) = plain.save_with(Cursor::new(Vec::new()), UID, Vec::new()).unwrap();
    crate::write::LEAVE_OUT.with(|l| l.borrow_mut().clear());
    let dir = scratch("renew-hidden");
    let path = dir.join("old.pst");
    std::fs::write(&path, out.into_inner()).unwrap();
    let mut store = PstStore::open(&path).unwrap();
    store.keep_original(false);
    assert!(store.renews());
    let first = ItemId(store.list(inbox.id).unwrap()[0].id);
    let mut item = store.load(inbox.id, first).unwrap();
    item.set_read(!item.is_read());
    store.update(inbox.id, first, item).unwrap();
    store.save(UID).unwrap();
    let mut back = PstStore::open(&path).unwrap();
    assert_eq!(back.table_of(table).unwrap().1.len(), 1);
    drop((store, back));
    let _ = std::fs::remove_dir_all(&dir);
}
