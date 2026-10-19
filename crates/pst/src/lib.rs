//! Outlook data files: `.pst` and `.ost` ([MS-PST]).
//!
//! A data file is three layers: the node database (`ndb`: blocks and two B-trees that index
//! them), lists/tables/properties on top of nodes (`ltp`), and the messaging layer here, which
//! says which node is a folder, a message, a recipient table or an attachment ([MS-PST] 2.4).
//!
//! Opening is read-only and leaves the file untouched. Writing (`write_store`) always makes a
//! whole new file from a mailbox, never a change inside an existing one. `check` holds a file's
//! structure against the specification.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod check;
mod check_ltp;
mod crypt;
mod crypt_table;
mod edit;
mod journal;
mod ltp;
mod ltp_write;
mod ndb;
mod store_write;
mod undo;
mod write;

pub use check::{Check, check, check_until};
pub use edit::original_beside;
pub use journal::{journal_beside, journal_elsewhere, set_aside};
pub use store_write::{Written, write_store, write_store_4k, write_store_ansi, write_store_until};
pub use undo::undo_beside;

use ltp::Raw;
use ndb::{Ndb, Subnodes, u32_at};
use openmailclient_model::props::parse_name_map;
use openmailclient_model::{
    Attachment, FolderId, FolderInfo, FolderKind, Item, ItemId, MSGFLAG_HASATTACH, NamedKey, PropBag, PropValue, Store, StoreError, Summary,
    clean_subject, pid, ptype,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PstError {
    #[error("this is not an Outlook data file")]
    NotPst,
    #[error("this data file cannot be opened: {0}")]
    Unsupported(String),
    #[error("the data file is damaged: {0}")]
    Damaged(String),
    #[error("the data file cannot be written: {0}")]
    Write(String),
    #[error("the mailbox being saved could not be read: {0}")]
    Source(String),
    #[error("it was stopped before it was done")]
    Stopped,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

impl From<PstError> for StoreError {
    fn from(e: PstError) -> Self {
        match e {
            PstError::Io(e) => Self::Io(e),
            e => Self::Format(e.to_string()),
        }
    }
}

/// Well-known nodes ([MS-PST] 2.4.1) and node types ([MS-PST] 2.2.2.1).
const NID_MESSAGE_STORE: u32 = 0x21;
const NID_NAME_TO_ID_MAP: u32 = 0x61;
const NID_ROOT_FOLDER: u32 = 0x122;
const NID_RECIPIENT_TABLE: u32 = 0x692;
const NID_ATTACHMENT_TABLE: u32 = 0x671;
const NID_TYPE_NORMAL_MESSAGE: u32 = 0x04;
const NID_TYPE_HIERARCHY_TABLE: u32 = 0x0D;
const NID_TYPE_CONTENTS_TABLE: u32 = 0x0E;

/// Table bookkeeping columns (PidTagLtpRowId, PidTagLtpRowVer and neighbours): not item properties.
const LTP_INTERNAL: std::ops::RangeInclusive<u16> = 0x67F1..=0x67F3;

const MAX_FOLDERS: usize = 100_000;
const MAX_CHILDREN: usize = 20_000;

#[cfg(test)]
fn blind(at: usize) -> bool {
    BLIND.with(|b| b.get()) == Some(at)
}

#[cfg(not(test))]
fn blind(_at: usize) -> bool {
    false
}

#[cfg(test)]
thread_local! {
    /// The attachment, by its place in a message's table, that cannot be read while this
    /// is set: a test's stand-in for a damaged one.
    pub(crate) static BLIND: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}
const MAX_FALLBACK_ITEMS: usize = 200_000;
/// Messages attached to messages are followed this deep.
const MAX_DEPTH: usize = 8;

/// An open `.pst` / `.ost`.
pub struct PstStore {
    ndb: Ndb<'static>,
    path: Option<PathBuf>,
    name: String,
    names: Vec<Option<NamedKey>>,
    root: u32,
    special: HashMap<u32, FolderKind>,
    /// Changes made and not yet saved.
    edits: Option<edit::Edits>,
    /// The file's length and time of writing when it was opened or last saved.
    stamp: Option<(u64, std::time::SystemTime)>,
    /// What the journal of this file is tied to: the file's length and the sum of its header.
    ident: Option<(u64, u32)>,
    journal: journal::Journal,
    /// The file a save that is being done elsewhere writes, from `save_begin` to `save_end`.
    saving: Option<edit::Beside>,
    /// Where a save says how far it is, and where it is told to stop.
    progress: Option<std::sync::Arc<openmailclient_model::Progress>>,
    /// Whether the file as it was stays beside this one the first time a save replaces it.
    keep_before: bool,
    /// Whether a save writes into the data file where that can be done.
    in_place: bool,
    /// What the file held where a save that was cut short wrote: the file is read as it
    /// was, through this, until the next save puts it back.
    torn: Option<undo::Undo>,
    /// How many saves went into the file where it is, and why the last one that was tried
    /// so was undone and written whole.
    onto_done: u64,
    onto_failed: Option<String>,
}

impl PstStore {
    /// Open a data file. Nothing is written to it until changes are saved.
    ///
    /// A save that was cut short (the power went, the program was ended) leaves its undo
    /// file beside the data file. The file is then read as it was before that save, and is
    /// put back by the next save that is asked for; opening still writes nothing.
    pub fn open(path: &Path) -> Result<Self, PstError> {
        let mut file = std::fs::File::open(path)?;
        let torn = undo::Undo::read(&undo_beside(path)).filter(|u| u.fits(&mut file));
        let mut store = match &torn {
            Some(undo) => Self::from_source(Box::new(undo.over(file)))?,
            None => Self::from_source(Box::new(file))?,
        };
        store.torn = torn;
        store.named_after(path);
        Ok(store)
    }

    /// Open a data file as it is, whatever is beside it.
    pub(crate) fn open_plain(path: &Path) -> Result<Self, PstError> {
        let mut store = Self::from_source(Box::new(std::fs::File::open(path)?))?;
        store.named_after(path);
        Ok(store)
    }

    fn named_after(&mut self, path: &Path) {
        let store = self;
        if store.name.is_empty() {
            store.name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        }
        store.path = Some(path.to_path_buf());
        store.note_stamp();
    }

    /// Why the last save that was tried in place was undone and written whole instead.
    pub fn in_place_trouble(&self) -> Option<&str> {
        self.onto_failed.as_deref()
    }

    /// True while the data file is read as it was before a save that was cut short.
    pub fn torn(&self) -> bool {
        self.torn.is_some()
    }

    /// Open a data file held in memory.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, PstError> {
        Self::from_source(Box::new(std::io::Cursor::new(data)))
    }

    fn from_source(src: Box<dyn ndb::Source>) -> Result<Self, PstError> {
        let ndb = Ndb::open(src)?;
        let mut store = Self {
            ndb,
            path: None,
            name: String::new(),
            names: Vec::new(),
            root: NID_ROOT_FOLDER,
            special: HashMap::new(),
            edits: None,
            stamp: None,
            ident: None,
            journal: journal::Journal::default(),
            saving: None,
            progress: None,
            keep_before: true,
            in_place: true,
            torn: None,
            onto_done: 0,
            onto_failed: None,
        };
        // The message store node ([MS-PST] 2.4.3): display name and the entry ids of the special folders.
        if let Ok((bag, _)) = store.node_props(NID_MESSAGE_STORE) {
            store.name = bag.str(pid::DISPLAY_NAME).unwrap_or("").to_string();
            // An EntryID is 4 bytes of flags, the store's 16-byte id, then the node id ([MS-PST] 2.4.3.2).
            let nid_of = |id: u16| bag.bin(id).and_then(|b| u32_at(b, 20));
            if let Some(nid) = nid_of(pid::IPM_SUBTREE_ENTRY_ID).filter(|n| store.ndb.nbt.contains_key(n)) {
                store.root = nid;
            }
            for (id, kind) in [(0x35E2, FolderKind::Outbox), (0x35E3, FolderKind::Deleted), (0x35E4, FolderKind::Sent), (0x35E7, FolderKind::Search)]
            {
                if let Some(nid) = nid_of(id) {
                    store.special.insert(nid, kind);
                }
            }
        }
        // A file whose store does not say where the mail folders begin, as an `.ost` of an IMAP
        // account may not, has them under a folder named IPM_SUBTREE near the root, beside
        // folders of Outlook's own (views, search folders) that it does not show: so do we.
        if store.root == NID_ROOT_FOLDER
            && let Some(nid) = store.named_near(NID_ROOT_FOLDER, "IPM_SUBTREE")
        {
            store.root = nid;
        }
        // The folders of the other modules and the drafts are named by ids on the root
        // ([MS-OXOSFLD] 2.2.3), whatever they are called in the file's language.
        if let Ok((bag, _)) = store.node_props(NID_ROOT_FOLDER) {
            for (id, kind) in store_write::SPECIAL {
                let nid = bag.bin(*id).and_then(|b| u32_at(b, 20)).filter(|n| n & 0x1F == 0x02 && store.ndb.nbt.contains_key(n));
                if let Some(nid) = nid {
                    store.special.entry(nid).or_insert(*kind);
                }
            }
        }
        // The named property map ([MS-PST] 2.4.7): the same three streams a `.msg` has.
        if let Ok((bag, _)) = store.node_props(NID_NAME_TO_ID_MAP) {
            let stream = |id: u16| bag.bin(id).unwrap_or(&[]);
            store.names = parse_name_map(stream(0x0002), stream(0x0003), stream(0x0004));
        }
        if !store.ndb.nbt.contains_key(&store.root) {
            return Err(PstError::Damaged("the file has no folder tree".to_string()));
        }
        Ok(store)
    }

    fn node(&self, nid: u32) -> Result<(u64, u64), PstError> {
        self.ndb.nbt.get(&nid).map(|e| (e.data, e.sub)).ok_or_else(|| PstError::Damaged("a node is missing from the index".to_string()))
    }

    /// A top-level node's properties, ids below 0x8000 only.
    fn node_props(&mut self, nid: u32) -> Result<(PropBag, Subnodes), PstError> {
        let (data, sub) = self.node(nid)?;
        let subs = self.ndb.subnodes(sub)?;
        let raws = ltp::read_pc(&mut self.ndb, data, &subs)?;
        let cp = code_page(&raws);
        let bag = PropBag(raws.into_iter().filter_map(|r| ltp::decode(r, cp)).collect());
        Ok((bag, subs))
    }

    /// A table's rows as property bags.
    fn table(&mut self, data: u64, subs: &Subnodes, cp: u32) -> Result<Vec<(u32, PropBag)>, PstError> {
        let rows = ltp::read_tc(&mut self.ndb, data, subs)?;
        Ok(rows
            .into_iter()
            .map(|row| {
                (row.id, PropBag(row.cells.into_iter().filter(|c| !LTP_INTERNAL.contains(&c.id)).filter_map(|c| ltp::decode(c, cp)).collect()))
            })
            .collect())
    }

    /// A folder of this name in `from`, or in a folder in it; of several, as an `.ost`
    /// has one for the public folders beside the mailbox's, the one with the most folders in it.
    fn named_near(&mut self, from: u32, name: &str) -> Option<u32> {
        /// Folders in the root looked into: Outlook's files have one or two.
        const LOOKED_INTO: usize = 8;
        let is = |bag: &PropBag| bag.str(pid::DISPLAY_NAME).is_some_and(|n| n.trim().eq_ignore_ascii_case(name));
        let top = self.children(from);
        let mut found: Vec<u32> = top.iter().filter(|(_, bag)| is(bag)).map(|(nid, _)| *nid).collect();
        for (child, _) in top.into_iter().take(LOOKED_INTO) {
            if child != from {
                found.extend(self.children(child).into_iter().filter(|(n, bag)| *n != from && is(bag)).map(|(n, _)| n));
            }
        }
        found.truncate(LOOKED_INTO);
        found.into_iter().map(|nid| (self.children(nid).len(), nid)).max_by_key(|(kids, _)| *kids).map(|(_, nid)| nid)
    }

    fn children(&mut self, folder: u32) -> Vec<(u32, PropBag)> {
        // The hierarchy table of a folder ([MS-PST] 2.4.4.4): one row per subfolder, the row id its node.
        let table = (folder & !0x1F) | NID_TYPE_HIERARCHY_TABLE;
        let from_table = self.node(table).and_then(|(data, sub)| {
            let subs = self.ndb.subnodes(sub)?;
            self.table(data, &subs, 1252)
        });
        if let Ok(rows) = from_table {
            return rows;
        }
        // A damaged table: fall back to the node index, which records every node's parent.
        let mut nids: Vec<u32> = self
            .ndb
            .nbt
            .iter()
            .filter(|(nid, e)| e.parent == folder && **nid & 0x1F == folder & 0x1F && **nid != folder)
            .map(|(nid, _)| *nid)
            .collect();
        nids.sort_unstable();
        nids.truncate(MAX_CHILDREN);
        nids.into_iter().filter_map(|nid| Some((nid, self.node_props(nid).ok()?.0))).collect()
    }

    fn message(&mut self, data: u64, sub: u64, depth: usize) -> Result<Item, PstError> {
        let subs = self.ndb.subnodes(sub)?;
        let raws = ltp::read_pc(&mut self.ndb, data, &subs)?;
        let cp = code_page(&raws);
        let mut item = Item::default();
        for (id, v) in raws.into_iter().filter_map(|r| ltp::decode(r, cp)) {
            if id < 0x8000 {
                item.props.set(id, v);
            } else if let Some(Some(key)) = self.names.get(usize::from(id - 0x8000)) {
                match v {
                    PropValue::MultiString(list) if *key == NamedKey::keywords() => item.categories = list,
                    v => item.named.push((key.clone(), v)),
                }
            }
        }
        if let Some(s) = item.props.str(pid::SUBJECT) {
            let clean = clean_subject(s);
            item.props.set_str(pid::SUBJECT, &clean);
        }
        // Recipient and attachment tables are subnodes of the message ([MS-PST] 2.4.5).
        if let Some(&(data, sub)) = subs.get(&NID_RECIPIENT_TABLE) {
            let table_subs = self.ndb.subnodes(sub).unwrap_or_default();
            if let Ok(rows) = self.table(data, &table_subs, cp) {
                item.recipients = rows.into_iter().take(MAX_CHILDREN).map(|(_, bag)| bag).collect();
            }
        }
        item.attachments = self.attachments(&subs, cp, depth).into_iter().map(|(_, att)| att).collect();
        item.normalise_html();
        Ok(item)
    }

    /// The attachments of a message with these subnodes, in the order of its attachment table,
    /// each with the id of the subnode that holds it. One that cannot be read is left out.
    fn attachments(&mut self, subs: &Subnodes, cp: u32, depth: usize) -> Vec<(u32, Attachment)> {
        let mut list = Vec::new();
        if let Some(&(data, sub)) = subs.get(&NID_ATTACHMENT_TABLE) {
            let table_subs = self.ndb.subnodes(sub).unwrap_or_default();
            let rows = self.table(data, &table_subs, cp).unwrap_or_default();
            // Each row's id names the subnode that holds the attachment object ([MS-PST] 2.4.6.2).
            for (at, (nid, _)) in rows.into_iter().take(MAX_CHILDREN).enumerate() {
                if blind(at) {
                    continue;
                }
                if let Some(&(data, sub)) = subs.get(&nid)
                    && let Ok(att) = self.attachment(data, sub, subs, cp, depth)
                {
                    list.push((nid, att));
                }
            }
        }
        list
    }

    fn attachment(&mut self, data: u64, sub: u64, message_subs: &Subnodes, cp: u32, depth: usize) -> Result<Attachment, PstError> {
        let subs = self.ndb.subnodes(sub)?;
        let raws = ltp::read_pc(&mut self.ndb, data, &subs)?;
        let mut att = Attachment {
            props: PropBag(raws.into_iter().filter(|r| r.id < 0x8000).filter_map(|r| ltp::decode(r, cp)).collect()),
            ..Attachment::default()
        };
        match att.props.0.remove(&pid::ATTACH_DATA) {
            Some(PropValue::Binary(b)) => att.data = b,
            // PtypObject: the node that holds the object, then its size ([MS-PST] 2.3.3.5).
            Some(PropValue::Other { ty: ptype::OBJECT, raw }) => {
                let target = u32_at(&raw, 0).and_then(|nid| subs.get(&nid).or_else(|| message_subs.get(&nid)).copied());
                if let Some((data, sub)) = target {
                    if att.props.i32(pid::ATTACH_METHOD) == Some(5) {
                        if depth < MAX_DEPTH {
                            att.embedded = self.message(data, sub, depth + 1).ok().map(Box::new);
                        }
                    } else {
                        att.data = self.ndb.data(data).unwrap_or_default();
                    }
                }
            }
            _ => {}
        }
        Ok(att)
    }

    fn summary(&self, id: u32, mut props: PropBag) -> Summary {
        if let Some(s) = props.str(pid::SUBJECT) {
            let clean = clean_subject(s);
            props.set_str(pid::SUBJECT, &clean);
        }
        let item = Item { props, ..Item::default() };
        let mut s = item.summary(u64::from(id));
        s.has_attachments = item.flags() & MSGFLAG_HASATTACH != 0;
        s
    }

    /// The folders as the file has them.
    fn file_folders(&mut self) -> Result<Vec<FolderInfo>, StoreError> {
        let mut out: Vec<FolderInfo> = Vec::new();
        let mut seen: HashSet<u32> = HashSet::from([self.root]);
        // Each folder is followed by everything inside it, as a folder pane shows them.
        let mut open = vec![(self.children(self.root).into_iter(), None)];
        while let Some((level, parent)) = open.last_mut() {
            let parent = *parent;
            let Some((child, bag)) = level.next() else {
                open.pop();
                continue;
            };
            if !seen.insert(child) || out.len() >= MAX_FOLDERS {
                continue;
            }
            let name = bag.str(pid::DISPLAY_NAME).unwrap_or("").to_string();
            let class = bag.str(pid::CONTAINER_CLASS).unwrap_or("").to_string();
            let count = |id: u16| u32::try_from(bag.i32(id).unwrap_or(0)).unwrap_or(0);
            let id = FolderId(u64::from(child));
            // A hierarchy table's row need not carry the mark: then the folder's own does.
            let hidden = match bag.bool(pid::ATTR_HIDDEN) {
                Some(h) => h,
                None => self.node_props(child).ok().and_then(|(own, _)| own.bool(pid::ATTR_HIDDEN)).unwrap_or(false),
            };
            out.push(FolderInfo {
                id,
                parent,
                kind: self.special.get(&child).copied().unwrap_or_else(|| FolderKind::guess(&name, &class)),
                name,
                container_class: class,
                total: count(pid::CONTENT_COUNT),
                unread: count(pid::CONTENT_UNREAD_COUNT),
                has_children: bag.bool(pid::SUBFOLDERS).unwrap_or(false),
                hidden,
            });
            open.push((self.children(child).into_iter(), Some(id)));
        }
        Ok(out)
    }

    /// A folder's messages as the file has them.
    fn file_list(&mut self, folder: FolderId) -> Result<Vec<Summary>, StoreError> {
        let nid = u32::try_from(folder.0).map_err(|_| StoreError::NoFolder)?;
        if !self.ndb.nbt.contains_key(&nid) {
            return Err(StoreError::NoFolder);
        }
        // The contents table ([MS-PST] 2.4.4.5): one row per message, the row id its node.
        let table = (nid & !0x1F) | NID_TYPE_CONTENTS_TABLE;
        let from_table = self.node(table).and_then(|(data, sub)| {
            let subs = self.ndb.subnodes(sub)?;
            self.table(data, &subs, 1252)
        });
        if let Ok(rows) = from_table {
            return Ok(rows.into_iter().map(|(id, bag)| self.summary(id, bag)).collect());
        }
        // A damaged table: list the message nodes whose parent is this folder.
        let mut nids: Vec<u32> =
            self.ndb.nbt.iter().filter(|(n, e)| e.parent == nid && **n & 0x1F == NID_TYPE_NORMAL_MESSAGE).map(|(n, _)| *n).collect();
        nids.sort_unstable();
        nids.truncate(MAX_FALLBACK_ITEMS);
        Ok(nids.into_iter().filter_map(|n| self.node_props(n).ok().map(|(bag, _)| self.summary(n, bag))).collect())
    }

    fn file_load(&mut self, id: ItemId) -> Result<Item, StoreError> {
        let nid = u32::try_from(id.0).map_err(|_| StoreError::NoItem)?;
        let (data, sub) = self.node(nid).map_err(|_| StoreError::NoItem)?;
        Ok(self.message(data, sub, 0)?)
    }
}

/// The code page of an object's 8-bit strings: its own, else Windows-1252.
fn code_page<'a>(raws: impl IntoIterator<Item = &'a Raw>) -> u32 {
    let (mut message, mut internet) = (None, None);
    for r in raws.into_iter().filter(|r| r.ty == ptype::INT32) {
        let named = u32_at(&r.data, 0).filter(|c| *c != 0);
        match r.id {
            pid::MESSAGE_CODEPAGE if message.is_none() => message = named,
            pid::INTERNET_CODEPAGE if internet.is_none() => internet = named,
            _ => {}
        }
    }
    message.or(internet).unwrap_or(1252)
}

impl Store for PstStore {
    fn display_name(&self) -> String {
        if self.name.is_empty() { "Outlook Data File".to_string() } else { self.name.clone() }
    }

    fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn folders(&mut self) -> Result<Vec<FolderInfo>, StoreError> {
        self.e_folders()
    }

    fn list(&mut self, folder: FolderId) -> Result<Vec<Summary>, StoreError> {
        self.e_list(folder)
    }

    fn load(&mut self, folder: FolderId, id: ItemId) -> Result<Item, StoreError> {
        self.e_load(folder, id)
    }

    fn writable(&self) -> bool {
        self.can_edit()
    }

    fn unsaved(&self) -> bool {
        // A file that a save was cut short in is mended by saving.
        self.e_unsaved() || self.torn.is_some()
    }

    fn save(&mut self, seed: [u8; 16]) -> Result<(), StoreError> {
        self.e_save(seed)
    }

    fn save_begin(&mut self, seed: [u8; 16]) -> Result<Option<openmailclient_model::SaveWork>, StoreError> {
        self.e_save_begin(seed)
    }

    fn save_end(&mut self, done: Result<openmailclient_model::SaveDone, StoreError>) -> Result<(), StoreError> {
        self.e_save_end(done)
    }

    fn saving(&self) -> bool {
        self.saving.is_some()
    }

    fn copy_to(&mut self, to: &Path, seed: [u8; 16]) -> Result<Option<openmailclient_model::Copied>, StoreError> {
        self.e_copy_to(to, seed)
    }

    fn keep_original(&mut self, keep: bool) {
        self.keep_before = keep;
    }

    fn save_in_place(&mut self, on: bool) {
        self.in_place = on;
    }

    fn saved_in_place(&self) -> u64 {
        self.onto_done
    }

    fn apart(&mut self) -> Result<Option<openmailclient_model::Apart>, StoreError> {
        self.e_apart()
    }

    fn hidden(&mut self, folder: FolderId) -> Result<Vec<(ItemId, Result<Item, StoreError>)>, StoreError> {
        self.e_hidden(folder)
    }

    fn folder_props(&mut self, folder: FolderId) -> Result<PropBag, StoreError> {
        self.e_folder_props(folder)
    }

    fn record_key(&mut self) -> Option<[u8; 16]> {
        // PidTagRecordKey of the message store ([MS-PST] 2.4.3.1).
        let (bag, _) = self.node_props(0x21).ok()?;
        match bag.get(0x0FF9) {
            Some(PropValue::Binary(b)) => b.as_slice().try_into().ok(),
            _ => None,
        }
    }

    fn recover(&mut self) -> Result<openmailclient_model::Recovery, StoreError> {
        PstStore::recover(self).map_err(StoreError::from)
    }

    fn discard(&mut self) {
        self.e_discard();
    }

    fn journal_elsewhere(&mut self, dir: &Path) {
        PstStore::journal_elsewhere(self, dir);
    }

    fn settle(&mut self) -> bool {
        PstStore::settle(self)
    }

    fn journal_trouble(&self) -> Option<String> {
        self.e_journal_trouble()
    }

    fn update(&mut self, folder: FolderId, id: ItemId, item: Item) -> Result<(), StoreError> {
        self.e_update(folder, id, item)
    }

    fn add(&mut self, folder: FolderId, item: Item) -> Result<ItemId, StoreError> {
        self.e_add(folder, item)
    }

    fn remove(&mut self, folder: FolderId, id: ItemId) -> Result<Item, StoreError> {
        self.e_remove(folder, id)
    }

    fn create_folder(&mut self, parent: FolderId, name: &str) -> Result<FolderId, StoreError> {
        self.e_create_folder(parent, name)
    }

    fn create_top_folder(&mut self, name: &str) -> Result<FolderId, StoreError> {
        self.e_create_top_folder(name)
    }

    fn create_folder_of(&mut self, parent: Option<FolderId>, name: &str, class: &str) -> Result<FolderId, StoreError> {
        self.e_create_of(parent, name, class)
    }

    fn rename_folder(&mut self, folder: FolderId, name: &str) -> Result<(), StoreError> {
        self.e_rename_folder(folder, name)
    }

    fn delete_folder(&mut self, folder: FolderId) -> Result<(), StoreError> {
        self.e_delete_folder(folder)
    }

    fn move_folder(&mut self, folder: FolderId, parent: FolderId) -> Result<(), StoreError> {
        self.e_move_folder(folder, parent)
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_4k;
#[cfg(test)]
mod tests_ansi;
#[cfg(test)]
mod tests_carry;
#[cfg(test)]
mod tests_edit;
#[cfg(test)]
mod tests_journal;
#[cfg(test)]
mod tests_onto;
#[cfg(test)]
mod tests_outlook;
#[cfg(test)]
mod tests_store;
#[cfg(test)]
mod tests_write;
