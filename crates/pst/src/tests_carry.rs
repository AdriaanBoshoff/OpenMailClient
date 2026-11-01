//! What the model has no place for, carried by a mailbox written from what is read: the hidden
//! items of a folder, its own properties, and entry ids among them naming the source's folders,
//! made to name those folders in the new file. That is how an `.ost` is exported, and how a
//! file that lacks what Outlook asks for is written anew (M3c.6).

use crate::check::check;
use crate::ltp::{Raw, Row};
use crate::tests_edit::{folder, scratch};
use crate::tests_store::{UID, mailbox, save};
use crate::{PstStore, write_store_4k, write_store_until};
use openmailclient_model::{FolderId, FolderKind, ItemId, PropValue, Store, pid, ptype};
use std::io::Cursor;

/// A hidden item's node in the Inbox.
const HIDDEN: u32 = (0x7000 << 5) | 0x08;
/// A property no specification names, on the Inbox and on its hidden item.
const OWN: u16 = 0x6699;
/// PidTagAdditionalRenEntryIds ([MS-OXOSFLD] 2.2.4): entry ids of folders, on the Inbox.
const NAMES: u16 = 0x36D8;
/// The nodes Outlook asks for that files written before M3c.1 lack.
const ASKED: [u32; 6] = [0x62B, 0x64C, 0x6B6, 0x6D7, 0x6F8, 0xEC1];

fn text(id: u16, s: &str) -> Raw {
    Raw { id, ty: ptype::STRING, data: s.encode_utf16().flat_map(u16::to_le_bytes).collect() }
}

/// An entry id of a store with this id naming a node ([MS-PST] 2.4.3.2).
fn entry(uid: [u8; 16], nid: u32) -> Vec<u8> {
    [&[0u8; 4][..], &uid, &nid.to_le_bytes()].concat()
}

/// The sample mailbox as a Unicode file with a hidden item in the Inbox, named in its table of
/// associated items, and two properties of the Inbox's own: one no specification names and
/// one naming the Sent Items folder by an entry id. `leave_out` are nodes left out.
fn planted(leave_out: &[u32]) -> Vec<u8> {
    let mut plain = PstStore::from_bytes(save(&mut mailbox()).0).unwrap();
    let inbox = folder(&mut plain, "Inbox");
    let sent = plain.folders().unwrap().into_iter().find(|f| f.kind == FolderKind::Sent).unwrap();
    let inbox_nid = u32::try_from(inbox.id.0).unwrap();
    let e = *plain.ndb.nbt.get(&inbox_nid).unwrap();
    let mut props: Vec<Raw> = plain.open_pc(e).unwrap().0.into_values().collect();
    props.push(Raw { id: OWN, ty: ptype::INT32, data: vec![7, 0, 0, 0] });
    let one = entry(UID, u32::try_from(sent.id.0).unwrap());
    let multi = crate::ltp_write::encode(NAMES, &PropValue::MultiBinary(vec![vec![1, 2, 3], one]));
    props.push(multi);
    // One of the Inbox's own by name: the first name the file's map has.
    let (at, _) = plain.names.iter().enumerate().find(|(_, k)| k.is_some()).unwrap();
    props.push(text(0x8000 + u16::try_from(at).unwrap(), "the folder's own, by name"));
    let hidden = vec![
        text(pid::MESSAGE_CLASS, "IPM.Configuration.Thing"),
        text(pid::SUBJECT, "hidden"),
        Raw { id: pid::MESSAGE_FLAGS, ty: ptype::INT32, data: 0x40u32.to_le_bytes().to_vec() },
        Raw { id: OWN, ty: ptype::INT32, data: vec![8, 0, 0, 0] },
    ];
    let table = inbox_nid & !0x1F | 0x0F;
    let (columns, _) = plain.table_of(table).unwrap();
    let row = Row { id: HIDDEN, cells: vec![text(pid::MESSAGE_CLASS, "IPM.Configuration.Thing")] };
    crate::edit::TABLES.with(|t| t.borrow_mut().push((table, columns, vec![row])));
    crate::write::LEAVE_OUT.with(|l| *l.borrow_mut() = leave_out.to_vec());
    let put = vec![(HIDDEN, inbox_nid, hidden), (inbox_nid, e.parent, props)];
    let (out, _) = plain.save_with(Cursor::new(Vec::new()), UID, put).unwrap();
    crate::write::LEAVE_OUT.with(|l| l.borrow_mut().clear());
    out.into_inner()
}

/// What a file holds beside the model, read back: the Inbox's hidden items (id, class, the
/// property of its own), the Inbox's property of its own, and the folders its entry ids name
/// with the store id they carry.
fn carried(store: &mut PstStore) -> (Vec<(u64, String, Option<i32>)>, Option<i32>, Vec<Option<([u8; 16], u32)>>) {
    let inbox = folder(store, "Inbox");
    let hidden = store
        .hidden(inbox.id)
        .unwrap()
        .into_iter()
        .map(|(id, item)| {
            let item = item.unwrap();
            (id.0, item.message_class().to_string(), item.props.i32(OWN))
        })
        .collect();
    let props = store.folder_props(inbox.id).unwrap();
    let names = match props.get(NAMES) {
        Some(PropValue::MultiBinary(list)) => {
            list.iter().map(|b| (b.len() == 24).then(|| (b[4..20].try_into().unwrap(), u32::from_le_bytes(b[20..24].try_into().unwrap())))).collect()
        }
        _ => Vec::new(),
    };
    (hidden, props.i32(OWN), names)
}

fn sent_nid(store: &mut PstStore) -> u32 {
    let sent = store.folders().unwrap().into_iter().find(|f| f.kind == FolderKind::Sent).unwrap();
    u32::try_from(sent.id.0).unwrap()
}

#[test]
fn an_ost_is_exported_with_its_hidden_items_and_folder_properties() {
    let dir = scratch("carry-ost");
    let ost = dir.join("box.ost");
    std::fs::write(&ost, planted(&[])).unwrap();
    let mut source = PstStore::open(&ost).unwrap();
    assert!(!source.writable());
    let sent_before = sent_nid(&mut source);
    let before = carried(&mut source);
    assert_eq!(before.0.len(), 1);
    assert_eq!(before.2, vec![None, Some((UID, sent_before))]);
    // An `.ost` is not copied as it is stored: Outlook refuses what that makes.
    assert!(source.copy_to(&dir.join("copy.pst"), UID).unwrap().is_none());
    let new = [0x5A; 16];
    for four_k in [false, true] {
        let written = if four_k {
            write_store_4k(Cursor::new(Vec::new()), &mut source, new, None)
        } else {
            write_store_until(Cursor::new(Vec::new()), &mut source, new, None)
        };
        let (out, done) = written.unwrap();
        assert_eq!((done.hidden, done.unreadable), (1, 0));
        let bytes = out.into_inner();
        let found = check(&mut Cursor::new(&bytes)).unwrap();
        assert!(found.sound(), "{:?} {:?}", found.problems, found.inside);
        let mut back = PstStore::from_bytes(bytes).unwrap();
        let sent = sent_nid(&mut back);
        let (hidden, own, names) = carried(&mut back);
        assert_eq!(hidden.len(), 1);
        assert_eq!(hidden[0].0 as u32 & 0x1F, 0x08);
        assert_eq!((hidden[0].1.as_str(), hidden[0].2), ("IPM.Configuration.Thing", Some(8)));
        assert_eq!(own, Some(7));
        // The entry id names the Sent Items of the new file, by the new file's id; a value
        // that is no entry id of the store is left as it was.
        assert_eq!(names, vec![None, Some((new, sent))]);
        let inbox = folder(&mut back, "Inbox");
        match back.folder_props(inbox.id).unwrap().get(NAMES) {
            Some(PropValue::MultiBinary(list)) => assert_eq!(list[0], vec![1, 2, 3]),
            other => panic!("{other:?}"),
        }
        // The named one under the id the new file gives its name.
        let key = source.names.iter().flatten().next().unwrap().clone();
        let named = back.folder_named(inbox.id).unwrap();
        assert_eq!(named, vec![(key, PropValue::String("the folder's own, by name".into()))]);
        // The name and counts are those the writer gives, once each.
        assert_eq!(back.folder_props(inbox.id).unwrap().str(pid::DISPLAY_NAME), Some("Inbox"));
    }
    drop(source);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_without_what_outlook_asks_for_is_written_whole_with_its_hidden_items() {
    let dir = scratch("carry-renew");
    let path = dir.join("old.pst");
    std::fs::write(&path, planted(&ASKED)).unwrap();
    let mut store = PstStore::open(&path).unwrap();
    store.keep_original(false);
    assert!(store.renews());
    let before = carried(&mut store);
    let inbox = folder(&mut store, "Inbox");
    let first = ItemId(store.list(inbox.id).unwrap()[0].id);
    let mut item = store.load(inbox.id, first).unwrap();
    item.set_read(!item.is_read());
    store.update(inbox.id, first, item).unwrap();
    store.save([0x33; 16]).unwrap();
    assert_eq!(store.saved_in_place(), 0);
    drop(store);

    let bytes = std::fs::read(&path).unwrap();
    let found = check(&mut Cursor::new(&bytes)).unwrap();
    assert!(found.sound(), "{:?} {:?}", found.problems, found.inside);
    let mut back = PstStore::from_bytes(bytes).unwrap();
    assert!(ASKED.iter().all(|nid| back.ndb.nbt.contains_key(nid)));
    // Ids, the store's own among them, are kept: the hidden item and the entry id are as
    // they were.
    assert_eq!(Store::record_key(&mut back), Some(UID));
    assert_eq!(carried(&mut back), before);
    assert_eq!(before.0, vec![(u64::from(HIDDEN), "IPM.Configuration.Thing".to_string(), Some(8))]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_store_that_keeps_no_hidden_items_gives_none() {
    let mut memory = mailbox();
    let inbox = memory.folders().unwrap().into_iter().find(|f| f.name == "Inbox").unwrap();
    assert!(memory.hidden(inbox.id).unwrap().is_empty());
    assert!(memory.folder_props(inbox.id).unwrap().0.is_empty());
    let mut store = PstStore::from_bytes(planted(&[])).unwrap();
    for id in [0, 1, u64::MAX, u64::from(u32::MAX)] {
        let _ = store.hidden(FolderId(id));
        let _ = store.folder_props(FolderId(id));
    }
}

#[test]
fn a_4k_data_file_is_copied_with_its_hidden_items() {
    let dir = scratch("carry-4k");
    let (from, to) = (dir.join("box.pst"), dir.join("copy.pst"));
    let mut planted = PstStore::from_bytes(planted(&[])).unwrap();
    let (out, done) = write_store_4k(Cursor::new(Vec::new()), &mut planted, UID, None).unwrap();
    assert_eq!(done.hidden, 1);
    std::fs::write(&from, out.into_inner()).unwrap();
    let mut store = PstStore::open(&from).unwrap();
    let want = carried(&mut store);
    assert!(store.copy_to(&to, UID).unwrap().is_some());
    drop(store);
    let bytes = std::fs::read(&to).unwrap();
    assert_eq!(u16::from_le_bytes([bytes[10], bytes[11]]), 36);
    let found = check(&mut Cursor::new(&bytes)).unwrap();
    assert!(found.sound(), "{:?} {:?}", found.problems, found.inside);
    assert_eq!(carried(&mut PstStore::from_bytes(bytes).unwrap()), want);
    let _ = std::fs::remove_dir_all(&dir);
}
