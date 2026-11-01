//! ANSI data files from our own writer: written, read back, changed and saved as the kind
//! they are, and held against the structure check each time.

use crate::check::check;
use crate::ltp::{self, Raw};
use crate::ndb::{Ndb, Ver};
use crate::tests_edit::{folder, same_mailbox, whole};
use crate::tests_store::{UID, mail, mailbox, save};
use crate::write::NdbWriter;
use crate::{PstError, PstStore, write_store_ansi};
use openmailclient_model::{Attachment, FolderKind, ItemId, MemStore, PropValue, Store, codepage, pid, ptype};
use std::io::Cursor;

fn ansi(store: &mut dyn Store) -> Vec<u8> {
    let (out, _) = write_store_ansi(Cursor::new(Vec::new()), store, UID, None).unwrap();
    let bytes = out.into_inner();
    let found = check(&mut Cursor::new(&bytes)).unwrap();
    assert!(found.sound(), "{:?} {:?}", found.problems, found.inside);
    bytes
}

/// Save the changes to memory, hold the file against the check and open it.
fn saved(store: &mut PstStore) -> PstStore {
    let (out, _) = store.save_to(Cursor::new(Vec::new()), UID).unwrap();
    let bytes = out.into_inner();
    let found = check(&mut Cursor::new(&bytes)).unwrap();
    assert!(found.sound(), "{:?} {:?}", found.problems, found.inside);
    PstStore::from_bytes(bytes).unwrap()
}

/// The properties of a top-level node as they are stored.
fn stored(store: &mut PstStore, nid: u32) -> Vec<Raw> {
    let e = *store.ndb.nbt.get(&nid).unwrap();
    let subs = store.ndb.subnodes(e.sub).unwrap();
    ltp::read_pc(&mut store.ndb, e.data, &subs).unwrap()
}

fn kind_of(raws: &[Raw], id: u16) -> u16 {
    raws.iter().find(|r| r.id == id).map(|r| r.ty).unwrap()
}

#[test]
fn a_mailbox_written_as_an_ansi_file_is_the_same_mailbox() {
    let bytes = ansi(&mut mailbox());
    // HEADER, ANSI: wVer 14, the root's file end at 168, the encoding at 461.
    assert_eq!((bytes[10], bytes[11], bytes[461]), (14, 0, 1));
    assert_eq!(u32::from_le_bytes(bytes[168..172].try_into().unwrap()) as usize, bytes.len());
    let mut back = PstStore::from_bytes(bytes).unwrap();
    assert_eq!(back.ndb.ver, Ver::Ansi);
    assert_eq!(back.display_name(), "Synthetic mailbox");
    // The same mailbox as the Unicode file made from it, to the last property.
    let mut wide = PstStore::from_bytes(save(&mut mailbox()).0).unwrap();
    same_mailbox(&whole(&mut wide), &whole(&mut back));

    // Text is 8-bit where Windows-1252 has it, and Unicode text where it has not.
    let inbox = folder(&mut back, "Inbox");
    let first = back.list(inbox.id).unwrap()[0].id as u32;
    let props = stored(&mut back, first);
    assert_eq!(kind_of(&props, pid::SUBJECT), ptype::STRING8);
    assert_eq!(kind_of(&props, pid::BODY), ptype::STRING8);
    let deep = back.folders().unwrap().into_iter().find(|f| f.name.contains("\u{6771}")).unwrap();
    let props = stored(&mut back, deep.id.0 as u32);
    assert_eq!(kind_of(&props, pid::DISPLAY_NAME), ptype::STRING);
    let props = stored(&mut back, inbox.id.0 as u32);
    assert_eq!(kind_of(&props, pid::DISPLAY_NAME), ptype::STRING8);
    // A table has one kind of text in a column: the folders under Inbox are all 8-bit, the
    // one under Projects cannot be.
    let names = |store: &mut PstStore, parent: u32| -> Vec<u16> {
        let (columns, _) = store.table_of(parent & !0x1F | 0x0D).unwrap();
        columns.into_iter().filter(|c| c.0 == pid::DISPLAY_NAME).map(|c| c.1).collect()
    };
    assert_eq!(names(&mut back, inbox.id.0 as u32), [ptype::STRING8]);
    assert_eq!(names(&mut back, deep.parent.unwrap().0 as u32), [ptype::STRING]);
}

#[test]
fn text_goes_by_the_code_page_of_its_message() {
    let mut s = MemStore::with_default_folders("Почта");
    let inbox = s.folder_by_kind(FolderKind::Inbox).unwrap();
    let mut it = mail("Привет, мир", false);
    it.props.set(pid::INTERNET_CODEPAGE, PropValue::Int32(1251));
    it.props.set_str(pid::BODY, "Отчёт во вложении.");
    it.recipients[0].set_str(pid::DISPLAY_NAME, "Алексей");
    // One name the code page has, one it has not, one that is plain.
    it.attachments.push(Attachment::file("отчёт.txt", "text/plain", b"1".to_vec()));
    it.attachments.push(Attachment::file("café.txt", "text/plain", b"2".to_vec()));
    it.attachments.push(Attachment::file("plain.txt", "text/plain", b"3".to_vec()));
    it.categories = vec!["Красная".into(), "Red".into()];
    let mut inner = mail("Grüße aus Köln", true);
    inner.props.set(pid::INTERNET_CODEPAGE, PropValue::Int32(1252));
    inner.attachments.push(Attachment::file("Übersicht.txt", "text/plain", b"4".to_vec()));
    let mut carrier = Attachment::file("Grüße aus Köln", "", Vec::new());
    carrier.props.set(pid::ATTACH_METHOD, PropValue::Int32(5));
    carrier.embedded = Some(Box::new(inner));
    it.attachments.push(carrier);
    s.push_item(inbox, it);
    // A message that names no code page is read in Windows-1252.
    s.push_item(inbox, mail("Smörgåsbord \u{2713}", true));
    s.push_item(inbox, mail("Smörgåsbord", true));

    let mut back = PstStore::from_bytes(ansi(&mut s)).unwrap();
    assert_eq!(back.display_name(), "Почта");
    let mut wide = PstStore::from_bytes(save(&mut s).0).unwrap();
    same_mailbox(&whole(&mut wide), &whole(&mut back));
    let inbox = folder(&mut back, "Inbox").id;
    let listed = back.list(inbox).unwrap();
    assert_eq!(listed.iter().map(|l| l.subject.as_str()).collect::<Vec<_>>(), ["Привет, мир", "Smörgåsbord \u{2713}", "Smörgåsbord"]);
    let got = back.load(inbox, ItemId(listed[0].id)).unwrap();
    assert_eq!(got.attachments.iter().map(|a| a.name()).collect::<Vec<_>>(), ["отчёт.txt", "café.txt", "plain.txt", "Grüße aus Köln"]);
    assert_eq!(got.attachments[3].embedded.as_ref().unwrap().attachments[0].name(), "Übersicht.txt");
    assert_eq!(got.recipients[0].str(pid::DISPLAY_NAME), Some("Алексей"));

    // As stored: the subject in the bytes of Windows-1251, and so the body.
    let props = stored(&mut back, listed[0].id as u32);
    let subject = props.iter().find(|r| r.id == pid::SUBJECT).unwrap();
    assert_eq!((subject.ty, &subject.data), (ptype::STRING8, &codepage::encode(1251, "Привет, мир").unwrap()));
    assert_eq!(kind_of(&props, pid::BODY), ptype::STRING8);
    // Text with a character Windows-1252 has not stays Unicode text; its neighbour does not.
    let props = stored(&mut back, listed[1].id as u32);
    assert_eq!(kind_of(&props, pid::SUBJECT), ptype::STRING);
    let props = stored(&mut back, listed[2].id as u32);
    assert_eq!(kind_of(&props, pid::SUBJECT), ptype::STRING8);
    // The list of a folder is read in Windows-1252, which has no Cyrillic: its column of
    // subjects is Unicode text, and one column only.
    let (columns, _) = back.table_of(inbox.0 as u32 & !0x1F | 0x0E).unwrap();
    assert_eq!(columns.iter().filter(|c| c.0 == pid::SUBJECT).map(|c| c.1).collect::<Vec<_>>(), [ptype::STRING]);
    assert_eq!(columns.iter().filter(|c| c.0 == pid::MESSAGE_CLASS).map(|c| c.1).collect::<Vec<_>>(), [ptype::STRING8]);
}

#[test]
fn changes_to_an_ansi_file_are_kept_and_it_stays_one() {
    let mut store = PstStore::from_bytes(ansi(&mut mailbox())).unwrap();
    assert!(store.writable() && !store.unsaved());
    // Copied without a change it is the same mailbox, and an ANSI file.
    let want = whole(&mut store);
    let mut copy = saved(&mut store);
    assert_eq!(copy.ndb.ver, Ver::Ansi);
    same_mailbox(&want, &whole(&mut copy));

    let (inbox, sent) = (folder(&mut store, "Inbox"), folder(&mut store, "Sent Items"));
    let listed = store.list(inbox.id).unwrap();
    let (first, second) = (ItemId(listed[0].id), ItemId(listed[1].id));
    // A subject the code page has, then one it has not, on a message with attachments.
    let mut item = store.load(inbox.id, first).unwrap();
    item.set_subject("Zahlen für März");
    item.set_read(true);
    item.categories.push("Blau \u{2713}".into());
    item.attachments.pop();
    item.attachments.push(Attachment::file("später.txt", "text/plain", b"new".to_vec()));
    item.recipients[0].set_str(pid::DISPLAY_NAME, "Алексей");
    item.refresh_display_recipients();
    store.update(inbox.id, first, item).unwrap();
    let mut item = store.load(inbox.id, second).unwrap();
    item.set_subject("\u{6771}\u{4eac} report");
    store.update(inbox.id, second, item).unwrap();
    // Messages come and go, and so do folders.
    let gone = ItemId(store.list(sent.id).unwrap()[0].id);
    store.remove(sent.id, gone).unwrap();
    let new_id = store.add(inbox.id, mail("Written here", false)).unwrap();
    let made = store.create_folder(inbox.id, "Ordner für später").unwrap();
    let other = store.create_folder(made, "Папка").unwrap();
    store.add(other, mail("In the new folder", true)).unwrap();
    let calendar = folder(&mut store, "Calendar");
    store.delete_folder(calendar.id).unwrap();

    let want = whole(&mut store);
    let mut back = saved(&mut store);
    assert_eq!(back.ndb.ver, Ver::Ansi);
    same_mailbox(&want, &whole(&mut back));
    let got = back.load(inbox.id, first).unwrap();
    assert_eq!((got.subject().as_str(), got.attachments.len()), ("Zahlen für März", 5));
    assert_eq!(got.recipients[0].str(pid::DISPLAY_NAME), Some("Алексей"));
    assert!(got.categories.contains(&"Blau \u{2713}".to_string()));
    assert_eq!(back.load(inbox.id, second).unwrap().subject(), "\u{6771}\u{4eac} report");
    let subjects: Vec<String> = back.list(inbox.id).unwrap().into_iter().map(|s| s.subject).collect();
    assert_eq!(subjects, ["Zahlen für März", "\u{6771}\u{4eac} report", "Written here"]);
    assert!(back.load(inbox.id, new_id).is_ok());
    // The changed message has its subject as 8-bit text; the one beside it could not.
    assert_eq!(kind_of(&stored(&mut back, first.0 as u32), pid::SUBJECT), ptype::STRING8);
    assert_eq!(kind_of(&stored(&mut back, second.0 as u32), pid::SUBJECT), ptype::STRING);
    // And saved again from there.
    let it = back.remove(inbox.id, first).unwrap();
    back.add(other, it).unwrap();
    let want = whole(&mut back);
    let mut third = saved(&mut back);
    assert_eq!(third.ndb.ver, Ver::Ansi);
    same_mailbox(&want, &whole(&mut third));
}

#[test]
fn an_ansi_file_on_disk_is_saved_in_place_as_an_ansi_file() {
    let dir = std::env::temp_dir().join(format!("omc-ansi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.pst");
    let bytes = ansi(&mut mailbox());
    std::fs::write(&path, &bytes).unwrap();
    let mut store = PstStore::open(&path).unwrap();
    let inbox = folder(&mut store, "Inbox");
    let first = ItemId(store.list(inbox.id).unwrap()[0].id);
    let mut item = store.load(inbox.id, first).unwrap();
    item.set_subject("Geändert");
    store.update(inbox.id, first, item).unwrap();
    assert!(std::fs::read(&path).unwrap() == bytes, "a change is not written until it is saved");
    assert!(store.save(UID).unwrap().folders > 0);
    let now = std::fs::read(&path).unwrap();
    assert_eq!((now[10], now[11]), (14, 0));
    assert!(std::fs::read(crate::original_beside(&path)).unwrap() == bytes);
    let mut fresh = PstStore::open(&path).unwrap();
    assert_eq!(fresh.load(inbox.id, first).unwrap().subject(), "Geändert");
    // A copy of it elsewhere is an ANSI file too, with all of it.
    let to = dir.join("copy.pst");
    let copied = fresh.copy_to(&to, UID).unwrap().unwrap();
    assert!(copied.items >= 4);
    let mut copy = PstStore::open(&to).unwrap();
    assert_eq!(copy.ndb.ver, Ver::Ansi);
    same_mailbox(&whole(&mut fresh), &whole(&mut copy));
    drop((store, fresh, copy));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_trees_of_a_large_ansi_file_come_back() {
    let pattern = |len: usize, seed: u32| -> Vec<u8> {
        let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect()
    };
    let mut w = NdbWriter::of(Cursor::new(Vec::new()), 2, Ver::Ansi).unwrap();
    assert_eq!(w.max_data(), 8180);
    assert!(matches!(w.data([vec![0u8; 8181]]), Err(PstError::Write(_))));
    // More blocks than one XBLOCK names (2,043 in an ANSI file), each a full one.
    let blocks: Vec<Vec<u8>> = (0..2_100u32).map(|i| pattern(8180, i + 10)).collect();
    let data = w.data(blocks.iter()).unwrap();
    // More subnodes than one SLBLOCK holds (681), and more nodes than one page of the
    // node tree (31), so both trees have a level above their leaves.
    let small = w.data([pattern(40, 1)]).unwrap();
    let mut subs = Vec::new();
    for i in 0..700u32 {
        subs.push(crate::write::Sub { nid: (0x1000 + i) << 5 | 0x1F, data: w.data([pattern(40, 1)]).unwrap(), sub: 0 });
    }
    let sub = w.subnodes(subs).unwrap();
    w.node(0x20_0024, 0x122, data, sub);
    for i in 0..200u32 {
        let d = w.data([pattern(64, 500 + i)]).unwrap();
        w.node((0x2000 + i) << 5 | 4, 0x122, d, 0);
    }
    w.node(0x61, 0, small, 0);
    let bytes = w.finish().unwrap().into_inner();
    let found = check(&mut Cursor::new(&bytes)).unwrap();
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    assert_eq!(found.nodes, 202);
    let mut ndb = Ndb::open(Box::new(Cursor::new(bytes))).unwrap();
    assert_eq!((ndb.ver, ndb.nbt.len()), (Ver::Ansi, 202));
    let n = *ndb.nbt.get(&0x20_0024).unwrap();
    assert!(ndb.data_blocks(n.data).unwrap() == blocks);
    let back = ndb.subnodes(n.sub).unwrap();
    assert_eq!(back.len(), 700);
    assert!(back.keys().all(|nid| nid & 0x1F == 0x1F));
    for i in [0u32, 57, 199] {
        let e = *ndb.nbt.get(&((0x2000 + i) << 5 | 4)).unwrap();
        assert_eq!(ndb.data(e.data).unwrap(), pattern(64, 500 + i));
    }
}

#[test]
fn a_cut_or_smashed_ansi_file_is_read_or_refused_never_a_panic() {
    let exercise = |bytes: Vec<u8>| {
        let _ = check(&mut Cursor::new(&bytes));
        let Ok(mut store) = PstStore::from_bytes(bytes) else { return };
        let Ok(folders) = store.folders() else { return };
        for f in folders {
            for row in store.list(f.id).unwrap_or_default() {
                let _ = store.load(f.id, ItemId(row.id));
            }
        }
        // What can be opened can be asked to save: it does, or says why not.
        let _ = store.save_to(Cursor::new(Vec::new()), UID);
    };
    let mut s = MemStore::with_default_folders("Small");
    let inbox = s.folder_by_kind(FolderKind::Inbox).unwrap();
    let mut it = mail("Grüße", false);
    it.attachments.push(Attachment::file("a.txt", "text/plain", b"abc".to_vec()));
    s.push_item(inbox, it);
    let file = ansi(&mut s);
    // The part of the file that is used: the header, and the first section as far as it goes.
    let used = file.iter().rposition(|b| *b != 0).unwrap() + 1;
    for len in (0..used).step_by(997) {
        exercise(file[..len].to_vec());
    }
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..150 {
        let mut bad = file.clone();
        for _ in 0..(1 + next() % 4) {
            let at = (next() as usize) % used;
            bad[at] = next() as u8;
        }
        exercise(bad);
    }
}
