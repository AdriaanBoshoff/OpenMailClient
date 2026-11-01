//! Saving a mailbox as a `.pst`: what is written is read back through `PstStore` and held
//! against the structure check.

use crate::check::check;
use crate::ndb::Ndb;
use crate::{PstError, PstStore, Written, ltp, write_store};
use openmailclient_model::{
    Attachment, FolderId, FolderInfo, FolderKind, Item, ItemId, MSGFLAG_HASATTACH, MSGFLAG_READ, MemStore, NameId, NamedKey, PropBag, PropValue,
    RECIP_CC, RECIP_TO, Store, StoreError, Summary, pid, pset, ptype,
};
use std::io::Cursor;

pub(super) const UID: [u8; 16] = [0xA5; 16];

pub(super) fn save(store: &mut dyn Store) -> (Vec<u8>, Written) {
    let (out, done) = write_store(Cursor::new(Vec::new()), store, UID).unwrap();
    let bytes = out.into_inner();
    let found = check(&mut Cursor::new(&bytes)).unwrap();
    assert!(found.sound(), "{:?} {:?}", found.problems, found.inside);
    (bytes, done)
}

fn recipient(kind: i32, name: &str, email: &str) -> PropBag {
    let mut r = PropBag::default();
    r.set(pid::RECIPIENT_TYPE, PropValue::Int32(kind));
    r.set_str(pid::DISPLAY_NAME, name);
    r.set_str(pid::ADDRESS_TYPE, "SMTP");
    r.set_str(pid::EMAIL_ADDRESS, email);
    r.set_str(pid::SMTP_ADDRESS, email);
    r
}

pub(super) fn mail(subject: &str, read: bool) -> Item {
    let mut it = Item::new_mail();
    it.set_subject(subject);
    it.set_sender("Robin Vale", "robin@example.org");
    it.props.set_str(pid::BODY, "The quarterly numbers are attached.\r\nRobin");
    it.props.set(pid::MESSAGE_DELIVERY_TIME, PropValue::Time(133_500_000_000_000_000));
    it.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(if read { MSGFLAG_READ } else { 0 }));
    it.props.set_str(pid::DISPLAY_TO, "Alex Morgan");
    it.recipients.push(recipient(RECIP_TO, "Alex Morgan", "alex@example.org"));
    it.recipients.push(recipient(RECIP_CC, "Sam \u{17d}ivkovi\u{107}", "sam@example.org"));
    it
}

/// A mailbox with a little of everything.
pub(super) fn mailbox() -> MemStore {
    let mut s = MemStore::with_default_folders("Synthetic mailbox");
    let inbox = s.folder_by_kind(FolderKind::Inbox).unwrap();
    let sent = s.folder_by_kind(FolderKind::Sent).unwrap();
    let projects = s.push_folder(Some(inbox), "Projects", "IPF.Note");
    let deep = s.push_folder(Some(projects), "2026 \u{2014} \u{6771}\u{4eac}", "IPF.Note");
    s.push_folder(None, "Calendar", "IPF.Appointment");

    let mut first = mail("Quarterly numbers", false);
    first.categories = vec!["Red Category".into(), "Finance".into()];
    first.named.push((NamedKey { guid: pset::PSETID_COMMON, name: NameId::Id(0x8503) }, PropValue::Bool(true)));
    first.named.push((NamedKey { guid: [0x42; 16], name: NameId::Name("x-custom-header".into()) }, PropValue::String("kept".into())));
    first.named.push((NamedKey { guid: pset::PS_PUBLIC_STRINGS, name: NameId::Name("Another".into()) }, PropValue::Int32(7)));
    first.props.set(0x6001, PropValue::Other { ty: 0x00FB, raw: vec![1, 2, 3, 4, 5] });
    first.props.set(pid::FLAG_STATUS, PropValue::Int32(2));
    // Kinds that share their bytes with another kind and keep their own.
    first.props.set(0x6002, PropValue::Float32(2.5));
    first.props.set(0x6003, PropValue::Currency(-12_345_600));
    first.props.set(0x6004, PropValue::FloatTime(45_000.25));
    first.props.set(0x6005, PropValue::Error(0x8004_010Fu32.cast_signed()));
    first.props.set(0x6006, PropValue::Float64(-0.1));
    first.props.set(pid::IMPORTANCE, PropValue::Int32(2));
    first.attachments.push(Attachment::file("numbers.csv", "text/csv", b"a,b\r\n1,2\r\n".to_vec()));
    // Larger than a heap holds, and larger than one block.
    let big: Vec<u8> = (0..40_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
    first.attachments.push(Attachment::file("scan.bin", "", big));
    let mut inner = mail("The message inside", true);
    inner.attachments.push(Attachment::file("inner.txt", "text/plain", b"inside".to_vec()));
    inner.named.push((NamedKey { guid: [0x42; 16], name: NameId::Name("x-custom-header".into()) }, PropValue::String("inner".into())));
    let mut carrier = Attachment::file("The message inside", "", Vec::new());
    carrier.props.set(pid::ATTACH_METHOD, PropValue::Int32(5));
    carrier.embedded = Some(Box::new(inner));
    first.attachments.push(carrier);
    let mut object = Attachment::file("Sheet", "", storage(9, 512));
    object.props.set(pid::ATTACH_METHOD, PropValue::Int32(6));
    first.attachments.push(object);
    // Said to be an object, but its bytes are no storage: kept as the file it is, which
    // Outlook lists (as an object it lists nothing).
    let mut odd = Attachment::file("odd.bin", "", b"not a storage".to_vec());
    odd.props.set(pid::ATTACH_METHOD, PropValue::Int32(6));
    first.attachments.push(odd);
    s.push_item(inbox, first);

    s.push_item(inbox, mail("Read already", true));
    let mut long = mail("A long one", false);
    long.props.set_str(pid::BODY, &"Line of a long body. ".repeat(5_000));
    long.props.set(pid::HTML, PropValue::Binary(format!("<html><body>{}</body></html>", "<p>para</p>".repeat(3_000)).into_bytes()));
    s.push_item(deep, long);
    s.push_item(sent, mail("Sent before", true));
    s
}

/// Bytes that begin as a storage does ([MS-CFB] 2.2), `len` of them.
pub(super) fn storage(fill: u8, len: usize) -> Vec<u8> {
    let mut bytes = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    bytes.resize(len, fill);
    bytes
}

/// Every folder of a store by its path of names, with what it holds.
pub(super) fn tree(store: &mut dyn Store) -> Vec<(String, FolderInfo, Vec<Summary>)> {
    let folders = store.folders().unwrap();
    let mut out = Vec::new();
    for f in &folders {
        let mut path = f.name.clone();
        let mut up = f.parent;
        while let Some(p) = up.and_then(|id| folders.iter().find(|x| x.id == id)) {
            path = format!("{}/{path}", p.name);
            up = p.parent;
        }
        out.push((path, f.clone(), store.list(f.id).unwrap()));
    }
    // One store lists a folder's children after it, another level by level.
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn a_mailbox_saved_as_pst_comes_back() {
    let mut source = mailbox();
    let (bytes, done) = save(&mut source);
    assert_eq!(done, Written { folders: source.folders().unwrap().len() as u64, items: 4, unreadable: 0, hidden: 0 });
    let mut back = PstStore::from_bytes(bytes).unwrap();
    assert_eq!(back.display_name(), "Synthetic mailbox");

    let (want, got) = (tree(&mut source), tree(&mut back));
    assert_eq!(got.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), want.iter().map(|f| f.0.as_str()).collect::<Vec<_>>());
    for ((path, w, w_list), (_, g, g_list)) in want.iter().zip(&got) {
        assert_eq!((g.kind, &g.container_class, g.has_children), (w.kind, &w.container_class, w.has_children), "{path}");
        assert_eq!(g.total as usize, w_list.len(), "{path}");
        assert_eq!(g.unread as usize, w_list.iter().filter(|s| !s.read).count(), "{path}");
        assert_eq!(g_list.len(), w_list.len(), "{path}");
        for (ws, gs) in w_list.iter().zip(g_list) {
            // The list comes from the contents table alone.
            assert_eq!(
                (&gs.subject, &gs.from, &gs.to, gs.time, gs.read, gs.importance, gs.flag),
                (&ws.subject, &ws.from, &ws.to, ws.time, ws.read, ws.importance, ws.flag)
            );
            assert_eq!((gs.has_attachments, &gs.message_class), (ws.has_attachments, &ws.message_class));
            let w_item = source.load(w.id, ItemId(ws.id)).unwrap();
            let g_item = back.load(g.id, ItemId(gs.id)).unwrap();
            same(&w_item, &g_item, path);
        }
    }
    // The deleted items folder is the one the store names, whatever it is called.
    assert!(got.iter().any(|(_, f, _)| f.kind == FolderKind::Deleted && f.id == FolderId(0x8062)));
    assert!(got.iter().any(|(_, f, _)| f.kind == FolderKind::Sent));
}

/// Everything of `want` is in `got`; `got` may have the properties a data file adds.
pub(super) fn same(want: &Item, got: &Item, at: &str) {
    for (id, v) in want.props.iter().filter(|(id, _)| ![pid::MESSAGE_FLAGS, pid::MESSAGE_SIZE].contains(id)) {
        assert_eq!(got.props.get(id), Some(v), "{at}: property {id:#06x}");
    }
    assert_eq!(got.flags() & !MSGFLAG_HASATTACH, want.flags() & !MSGFLAG_HASATTACH, "{at}");
    assert_eq!(got.flags() & MSGFLAG_HASATTACH != 0, !want.attachments.is_empty(), "{at}");
    assert_eq!(got.categories, want.categories, "{at}");
    let sorted = |it: &Item| {
        let mut named = it.named.clone();
        named.sort_by(|a, b| a.0.cmp(&b.0));
        named
    };
    assert_eq!(sorted(got), sorted(want), "{at}");
    // What a message must have is there.
    for id in [pid::MESSAGE_CLASS, 0x0E17, pid::CREATION_TIME, pid::LAST_MODIFICATION_TIME, 0x300B, pid::MESSAGE_SIZE] {
        assert!(got.props.get(id).is_some(), "{at}: {id:#06x} is missing");
    }
    assert_eq!(got.recipients.len(), want.recipients.len(), "{at}");
    for (w, g) in want.recipients.iter().zip(&got.recipients) {
        for (id, v) in w.iter() {
            assert_eq!(g.get(id), Some(v), "{at}: recipient property {id:#06x}");
        }
    }
    assert_eq!(got.attachments.len(), want.attachments.len(), "{at}");
    for (w, g) in want.attachments.iter().zip(&got.attachments) {
        assert_eq!(g.name(), w.name(), "{at}");
        assert!(g.data == w.data, "{at}: the bytes of {}", w.name());
        // An object whose bytes are no storage comes back as the file it is.
        let as_file = w.embedded.is_none() && w.props.i32(pid::ATTACH_METHOD) == Some(6) && !w.is_object();
        for (id, v) in w.props.iter().filter(|(id, _)| *id != pid::ATTACH_SIZE && !(as_file && *id == pid::ATTACH_METHOD)) {
            assert_eq!(g.props.get(id), Some(v), "{at}: attachment property {id:#06x}");
        }
        if as_file {
            assert_eq!(g.props.i32(pid::ATTACH_METHOD), Some(1), "{at}: {}", w.name());
        }
        assert!(g.props.i32(pid::RENDERING_POSITION).is_some() && g.props.i32(pid::ATTACH_SIZE).is_some());
        match (&w.embedded, &g.embedded) {
            (Some(w), Some(g)) => same(w, g, &format!("{at}, attached")),
            (None, None) => {}
            _ => panic!("{at}: an attached message came or went"),
        }
    }
}

#[test]
fn a_saved_pst_saves_again_to_the_same_mailbox() {
    // A data file read by us and written again: nothing is lost on the second trip either.
    let (first, _) = save(&mut mailbox());
    let mut once = PstStore::from_bytes(first).unwrap();
    let (second, done) = save(&mut once);
    assert_eq!((done.items, done.unreadable), (4, 0));
    let mut twice = PstStore::from_bytes(second).unwrap();
    let (a, b) = (tree(&mut once), tree(&mut twice));
    assert_eq!(a.len(), b.len());
    for ((path, fa, la), (_, fb, lb)) in a.iter().zip(&b) {
        assert_eq!((&fa.name, fa.kind, fa.total, fa.unread), (&fb.name, fb.kind, fb.total, fb.unread), "{path}");
        for (sa, sb) in la.iter().zip(lb) {
            let (ia, ib) = (once.load(fa.id, ItemId(sa.id)).unwrap(), twice.load(fb.id, ItemId(sb.id)).unwrap());
            same(&ia, &ib, path);
            // The second trip adds nothing either.
            assert_eq!(ia.props.iter().count(), ib.props.iter().count(), "{path}");
        }
    }
}

#[test]
fn an_empty_mailbox_is_a_whole_data_file() {
    let mut empty = MemStore::new("Nothing yet");
    let (bytes, done) = save(&mut empty);
    assert_eq!(done, Written::default());
    // Every node a data file must have ([MS-PST] 2.7.1).
    let mut ndb = Ndb::open(Box::new(Cursor::new(bytes.clone()))).unwrap();
    for nid in [
        0x21u32, 0x61, 0x122, 0x12D, 0x12E, 0x12F, 0x1E1, 0x201, 0x60D, 0x60E, 0x60F, 0x610, 0x692, 0x671, 0x2223, 0x8022, 0x802D, 0x802E, 0x802F,
        0x8042, 0x804D, 0x804E, 0x804F, 0x8062, 0x806D, 0x806E, 0x806F, // And those Outlook asks for besides (`tests_outlook.rs`).
        0x62B, 0x64C, 0x6B6, 0x6D7, 0x6F8, 0xEC1, 0x2226, 0x2227, 0x2230,
    ] {
        assert!(ndb.nbt.contains_key(&nid), "node {nid:#x} is missing");
    }
    assert_eq!(ndb.nbt.len(), 36);
    // The root is its own parent, and lists the visible tree, the search root and the one
    // search folder.
    let root = *ndb.nbt.get(&0x122).unwrap();
    assert_eq!(root.parent, 0x122);
    let table = *ndb.nbt.get(&0x12D).unwrap();
    let subs = ndb.subnodes(table.sub).unwrap();
    let rows: Vec<u32> = ltp::read_tc(&mut ndb, table.data, &subs).unwrap().iter().map(|r| r.id).collect();
    assert_eq!(rows, [0x8022, 0x8042, 0x2223]);
    // The store names its folders by entry id: flags, the store's id, the node.
    let store = *ndb.nbt.get(&0x21).unwrap();
    let props = ltp::read_pc(&mut ndb, store.data, &Default::default()).unwrap();
    let entry = |id: u16| props.iter().find(|p| p.id == id).map(|p| p.data.clone()).unwrap();
    assert_eq!(entry(0x0FF9), UID);
    for (id, nid) in [(0x35E0u16, 0x8022u32), (0x35E3, 0x8062), (0x35E7, 0x8042)] {
        let mut want = vec![0u8; 4];
        want.extend(UID);
        want.extend(nid.to_le_bytes());
        assert_eq!(entry(id), want);
    }
    // The name map says how many buckets it has, and has its three streams.
    let map = *ndb.nbt.get(&0x61).unwrap();
    let props = ltp::read_pc(&mut ndb, map.data, &Default::default()).unwrap();
    assert_eq!(
        props.iter().map(|p| (p.id, p.ty)).collect::<Vec<_>>(),
        [(1, ptype::INT32), (2, ptype::BINARY), (3, ptype::BINARY), (4, ptype::BINARY)]
    );
    assert_eq!(props.first().map(|p| p.data.clone()), Some(251u32.to_le_bytes().to_vec()));

    let mut back = PstStore::from_bytes(bytes).unwrap();
    let folders = back.folders().unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders.first().map(|f| (f.name.as_str(), f.kind, f.total)), Some(("Deleted Items", FolderKind::Deleted, 0)));
    assert!(back.list(FolderId(0x8062)).unwrap().is_empty());
}

#[test]
fn named_properties_land_in_their_buckets() {
    let mut s = MemStore::new("Names");
    let folder = s.push_folder(None, "Inbox", "IPF.Note");
    let mut it = mail("Named", true);
    // Enough names that buckets are shared, of both kinds and several sets.
    for i in 0..600u32 {
        let guid = if i % 3 == 0 { pset::PSETID_COMMON } else { [(i % 7) as u8 + 1; 16] };
        let name = if i % 2 == 0 { NameId::Id(0x8000 + i) } else { NameId::Name(format!("x-header-{i}")) };
        it.named.push((NamedKey { guid, name }, PropValue::Int32(i as i32)));
    }
    s.push_item(folder, it.clone());
    let (bytes, _) = save(&mut s);
    let mut ndb = Ndb::open(Box::new(Cursor::new(bytes.clone()))).unwrap();
    let map = *ndb.nbt.get(&0x61).unwrap();
    let subs = ndb.subnodes(map.sub).unwrap();
    let props = ltp::read_pc(&mut ndb, map.data, &subs).unwrap();
    let entries = props.iter().find(|p| p.id == 3).unwrap();
    assert_eq!(entries.data.len(), 600 * 8);
    // Every bucket record is in the bucket its first six bytes pick, and there are 600 in all.
    let mut seen = 0;
    for bucket in props.iter().filter(|p| p.id >= 0x1000) {
        assert!(bucket.id < 0x1000 + 251);
        for record in bucket.data.chunks(8) {
            let first = u32::from_le_bytes(record[..4].try_into().unwrap());
            let second = u32::from(u16::from_le_bytes(record[4..6].try_into().unwrap()));
            assert_eq!(u32::from(bucket.id - 0x1000), (first ^ second) % 251);
            seen += 1;
        }
    }
    assert_eq!(seen, 600);
    let mut back = PstStore::from_bytes(bytes).unwrap();
    let id = back.list(FolderId(0x8082)).unwrap().first().map(|s| s.id).unwrap();
    let got = back.load(FolderId(0x8082), ItemId(id)).unwrap();
    assert_eq!(got.named, it.named);
}

#[test]
fn a_folder_of_many_messages_is_saved() {
    let mut s = MemStore::new("Many");
    let folder = s.push_folder(None, "Archive", "IPF.Note");
    for i in 0..1_500 {
        let mut it = mail(&format!("Message {i}"), i % 4 != 0);
        it.props.set(pid::MESSAGE_DELIVERY_TIME, PropValue::Time(133_500_000_000_000_000 + i64::from(i)));
        s.push_item(folder, it);
    }
    let (bytes, done) = save(&mut s);
    assert_eq!(done.items, 1_500);
    let mut back = PstStore::from_bytes(bytes).unwrap();
    let f = back.folders().unwrap().into_iter().find(|f| f.name == "Archive").unwrap();
    assert_eq!((f.total, f.unread), (1_500, 375));
    let list = back.list(f.id).unwrap();
    assert_eq!(list.len(), 1_500);
    assert!(list.iter().enumerate().all(|(i, s)| s.subject == format!("Message {i}")));
    let last = back.load(f.id, ItemId(list.last().unwrap().id)).unwrap();
    assert_eq!(last.subject(), "Message 1499");
}

/// A store that cannot give some of what it lists.
struct Flaky(MemStore);

impl Store for Flaky {
    fn display_name(&self) -> String {
        self.0.display_name()
    }
    fn folders(&mut self) -> Result<Vec<FolderInfo>, StoreError> {
        let mut f = self.0.folders()?;
        // A folder that claims to be inside itself, and one inside a folder nobody listed.
        let id = f.last().map(|f| f.id).unwrap();
        f.push(FolderInfo { id: FolderId(9_001), parent: Some(FolderId(9_001)), ..f.last().cloned().unwrap() });
        f.push(FolderInfo { id: FolderId(9_002), parent: Some(FolderId(77_777)), ..f.last().cloned().unwrap() });
        // And one id twice.
        f.push(FolderInfo { id, ..f.last().cloned().unwrap() });
        Ok(f)
    }
    fn list(&mut self, folder: FolderId) -> Result<Vec<Summary>, StoreError> {
        if folder.0 >= 9_000 {
            return Err(StoreError::NoFolder);
        }
        let mut list = self.0.list(folder)?;
        list.push(Summary { id: u64::MAX, ..Summary::default() });
        Ok(list)
    }
    fn load(&mut self, folder: FolderId, id: ItemId) -> Result<Item, StoreError> {
        self.0.load(folder, id)
    }
}

/// A mailbox in another language: the folders keep their kinds and not their English names.
struct Renamed(MemStore);

impl Store for Renamed {
    fn display_name(&self) -> String {
        self.0.display_name()
    }
    fn folders(&mut self) -> Result<Vec<FolderInfo>, StoreError> {
        let mut folders = self.0.folders()?;
        for f in &mut folders {
            f.name = match f.name.as_str() {
                "Drafts" => "Concepten",
                "Calendar" => "Agenda",
                "Tasks" => "Taken",
                other => other,
            }
            .to_string();
        }
        Ok(folders)
    }
    fn list(&mut self, folder: FolderId) -> Result<Vec<Summary>, StoreError> {
        self.0.list(folder)
    }
    fn load(&mut self, folder: FolderId, id: ItemId) -> Result<Item, StoreError> {
        self.0.load(folder, id)
    }
}

#[test]
fn the_folders_of_the_other_modules_are_named_by_id_on_the_root_and_the_inbox() {
    let mut s = MemStore::new("Kinds");
    s.push_folder(None, "Inbox", "IPF.Note");
    s.push_folder(None, "Drafts", "IPF.Note");
    s.push_folder(None, "Calendar", "IPF.Appointment");
    // The first of a kind is the one named; a second calendar is a folder like any other.
    s.push_folder(None, "Holidays", "IPF.Appointment");
    s.push_folder(None, "Contacts", "IPF.Contact");
    s.push_folder(None, "Tasks", "IPF.Task");
    s.push_folder(None, "Notes", "IPF.StickyNote");
    s.push_folder(None, "Journal", "IPF.Journal");
    let (bytes, _) = save(&mut Renamed(s));
    let mut back = PstStore::from_bytes(bytes).unwrap();
    let folders = back.folders().unwrap();
    let nid = |name: &str| folders.iter().find(|f| f.name == name).map(|f| f.id.0 as u32).unwrap();
    // A folder with no English name is known for what it is by the id that names it.
    assert_eq!(folders.iter().find(|f| f.name == "Concepten").map(|f| f.kind), Some(FolderKind::Drafts));
    let want = [(0x36D0u16, "Agenda"), (0x36D1, "Contacts"), (0x36D2, "Journal"), (0x36D3, "Notes"), (0x36D4, "Taken"), (0x36D7, "Concepten")];
    for holder in [crate::NID_ROOT_FOLDER, nid("Inbox")] {
        let (bag, _) = back.node_props(holder).unwrap();
        for (id, name) in want {
            // An EntryID: four bytes of flags, the store's id, the folder's node.
            let entry = bag.bin(id).unwrap_or_else(|| panic!("{holder:#x} has no {id:#x}"));
            assert_eq!(entry.len(), 24);
            assert_eq!((&entry[..4], &entry[4..20], &entry[20..]), (&[0u8; 4][..], &UID[..], &nid(name).to_le_bytes()[..]), "{id:#x}");
        }
    }
    // No other folder carries them, and a mailbox without those folders has none.
    assert!(back.node_props(nid("Holidays")).unwrap().0.bin(0x36D0).is_none());
    let mut plain = MemStore::new("Plain");
    plain.push_folder(None, "Inbox", "IPF.Note");
    let mut back = PstStore::from_bytes(save(&mut plain).0).unwrap();
    let (bag, _) = back.node_props(crate::NID_ROOT_FOLDER).unwrap();
    assert!(want.iter().all(|(id, _)| bag.bin(*id).is_none()));
}

#[test]
fn what_the_source_cannot_give_is_counted_not_fatal() {
    let mut s = MemStore::new("Flaky");
    let folder = s.push_folder(None, "Inbox", "IPF.Note");
    s.push_item(folder, mail("Still here", false));
    let mut flaky = Flaky(s);
    let (bytes, done) = save(&mut flaky);
    // One message that will not load in each of the two listings of the folder, and two
    // folders that will not list.
    assert_eq!((done.folders, done.items, done.unreadable), (4, 2, 4));
    let mut back = PstStore::from_bytes(bytes).unwrap();
    let folders = back.folders().unwrap();
    // Every folder is reachable: the odd ones hang from the top.
    assert_eq!(folders.len(), 5);
    assert!(folders.iter().all(|f| f.parent.is_none()));
    let inbox = folders.iter().find(|f| f.total == 1).unwrap();
    assert_eq!(back.list(inbox.id).unwrap().first().map(|s| s.subject.clone()), Some("Still here".to_string()));
}

#[test]
fn hostile_items_are_written_or_refused_never_a_panic() {
    let mut s = MemStore::new("\u{0}\u{FFFF}");
    let folder = s.push_folder(None, "", "");
    let mut it = Item::default();
    // Ids a data file keeps for itself, a foreign named id, values of the wrong type in
    // columns of every table, and empty values of each kind.
    for (id, v) in [
        (0x67F2u16, PropValue::Int32(5)),
        (0x67F3, PropValue::Binary(vec![1])),
        (0x8123, PropValue::String("foreign".into())),
        (0x0017, PropValue::String("high".into())),
        (0x0037, PropValue::Int32(3)),
        (0x0E06, PropValue::Bool(true)),
        (0x0E07, PropValue::String("flags".into())),
        (0x0E08, PropValue::Int32(-5)),
        (0x1000, PropValue::String(String::new())),
        (0x1013, PropValue::Binary(Vec::new())),
        (0x7000, PropValue::MultiString(Vec::new())),
        (0x7001, PropValue::Other { ty: 0x000D, raw: vec![0xFF; 3] }),
        (0x7002, PropValue::Other { ty: 0x0003, raw: Vec::new() }),
    ] {
        it.props.set(id, v);
    }
    let mut r = PropBag::default();
    r.set(0x0C15, PropValue::String("to".into()));
    r.set(0x67F2, PropValue::Int32(99));
    r.set(0x3000, PropValue::String("row".into()));
    it.recipients.push(r);
    let mut a = Attachment::default();
    a.props.set(0x0E20, PropValue::String("big".into()));
    a.props.set(0x3705, PropValue::Bool(true));
    a.props.set(0x3701, PropValue::Int32(1));
    it.attachments.push(a);
    it.categories = vec![String::new()];
    it.named.push((NamedKey { guid: [0; 16], name: NameId::Name(String::new()) }, PropValue::Binary(Vec::new())));
    s.push_item(folder, it);
    let (bytes, done) = save(&mut s);
    assert_eq!(done.items, 1);
    let mut back = PstStore::from_bytes(bytes).unwrap();
    let f = back.folders().unwrap().into_iter().find(|f| f.kind != FolderKind::Deleted).unwrap();
    let list = back.list(f.id).unwrap();
    let got = back.load(f.id, ItemId(list.first().unwrap().id)).unwrap();
    assert_eq!(got.props.get(0x0037), Some(&PropValue::Int32(3)));
    assert_eq!(got.props.get(0x8123), None);
    assert_eq!((got.recipients.len(), got.attachments.len()), (1, 1));

    // Messages nested deeper than a data file is written are refused, not recursed into for ever.
    let mut deep = mail("bottom", true);
    for _ in 0..40 {
        let mut outer = mail("outer", true);
        let carrier = Attachment { embedded: Some(Box::new(deep)), ..Default::default() };
        outer.attachments.push(carrier);
        deep = outer;
    }
    let mut s = MemStore::new("Deep");
    let folder = s.push_folder(None, "Inbox", "IPF.Note");
    s.push_item(folder, deep);
    assert!(matches!(write_store(Cursor::new(Vec::new()), &mut s, UID), Err(PstError::Write(_))));
}

#[test]
fn where_a_store_does_not_say_where_its_mail_begins_the_ipm_subtree_with_folders_in_it_is_taken() {
    // As an `.ost` has them: one IPM_SUBTREE for the public folders, empty, and the mailbox's.
    let mut s = MemStore::new("Synthetic cache");
    let public = s.push_folder(None, "Root - Public", "");
    s.push_folder(Some(public), "IPM_SUBTREE", "");
    let mailbox = s.push_folder(None, "Root - Mailbox", "");
    s.push_folder(Some(mailbox), "Finder", "");
    let subtree = s.push_folder(Some(mailbox), "IPM_SUBTREE", "");
    s.push_folder(Some(subtree), "Inbox", "IPF.Note");
    s.push_folder(Some(subtree), "Sent Items", "IPF.Note");
    let (bytes, _) = save(&mut s);
    let mut back = PstStore::from_bytes(bytes).unwrap();
    let from = back.root;
    let found = back.named_near(from, "ipm_subtree").unwrap();
    let names: Vec<String> = back.children(found).iter().filter_map(|(_, bag)| bag.str(pid::DISPLAY_NAME).map(str::to_string)).collect();
    assert_eq!(names, ["Inbox", "Sent Items"]);
    assert_eq!(back.named_near(from, "Nowhere"), None);
}
