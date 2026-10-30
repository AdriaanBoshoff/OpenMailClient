//! Changes to an open data file.
//!
//! Nothing is written while changes are made: they are kept beside the open file, which stays
//! as it was. Saving writes a whole new file, or into the file where it is (`save_onto.rs`). Every node the changes do not touch is copied as
//! it is stored, block for block, so what this program does not understand (hidden items,
//! views, search folders, values of types the model has no place for) goes across untouched.
//! A changed message keeps every stored property that was not changed. The new file is checked,
//! read back and only then put in the old one's place.

use crate::journal::{self, Diff, Found, Journal, Op, TOP};
use crate::ltp::{self, Raw, Row};
use crate::ltp_write::{Late, NID_TYPE_OBJECT, Subs, encode, write_pc_late};
use crate::ndb::{NbtEntry, Ndb, Subnodes, u32_at};
use crate::search_queue::{self, NID_SEARCH_MANAGEMENT_QUEUE, Sud};
use crate::store_write::{
    ASSOC, ATTACHMENTS, CONTENTS, CONTENTS_MORE, HIERARCHY, MAX_CELL, MAX_INDEX, Names, Out, Written, attachment_cells, cannot, cell_for, copy,
    count, flag, int, text,
};
use crate::write::{NdbWriter, Sub};
use crate::{LTP_INTERNAL, MAX_CHILDREN, NID_ATTACHMENT_TABLE, NID_NAME_TO_ID_MAP, NID_RECIPIENT_TABLE, PstError, PstStore, code_page};
use openmailclient_model::{
    Attachment, FolderId, FolderInfo, FolderKind, Item, ItemId, MSGFLAG_HASATTACH, NamedKey, PropBag, PropValue, Recovery, StoreError, Summary, pid,
    ptype,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Node types ([MS-PST] 2.2.2.1).
const TYPE_FOLDER: u32 = 0x02;
const TYPE_MESSAGE: u32 = 0x04;
const TYPE_ATTACHMENT: u32 = 0x05;
const TYPE_HIERARCHY: u32 = 0x0D;
const TYPE_CONTENTS: u32 = 0x0E;
const TYPE_ASSOC: u32 = 0x0F;
const TYPE_SEARCH_FOLDER: u32 = 0x03;
const TYPE_ASSOC_MESSAGE: u32 = 0x08;
/// Nodes Outlook asks for that the files this program wrote before M3c.1 lack (see
/// `store_write`): the receive folder table, the outgoing queue, three templates and the
/// search update queue.
const OUTLOOK_ASKS: [u32; 6] = [0x62B, 0x64C, 0x6B6, 0x6D7, 0x6F8, 0xEC1];
/// The folders every data file has that are not listed: the root, the top of the visible
/// tree, the search root and the search folder ([MS-PST] 2.7.1).
const EVERY_FILE: [u32; 4] = [0x122, 0x8022, 0x8042, 0x2223];
const TYPE_SEARCH_CONTENTS: u32 = 0x10;
const TYPE_LTP: u32 = 0x1F;

/// Subnodes are copied this deep; a message attached to a message goes two levels down.
const MAX_COPY_DEPTH: usize = 40;
/// At most this many hidden items of one folder are read for a mailbox written anew.
const MAX_HIDDEN: usize = 100_000;
#[cfg(test)]
thread_local! {
    /// Tables a test has the next save write (node, columns, rows) in place of what the file
    /// has there: what another program keeps and our own writer never makes.
    pub(crate) static TABLES: std::cell::RefCell<Vec<(u32, Vec<(u16, u16)>, Vec<Row>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// What says where an attachment is and how large, not what it is: its place in the list and
/// its size are worked out when it is written.
const PLACED: [u16; 3] = [pid::ATTACH_NUM, pid::ATTACH_SIZE, pid::ATTACH_DATA];

/// An attachment's properties without those that only say where it is.
fn settled(props: &PropBag) -> Vec<(u16, &PropValue)> {
    props.iter().filter(|(id, _)| !PLACED.contains(id)).collect()
}

/// The file a save that is done elsewhere writes beside the data file. Should the store go
/// away before the save is ended, the file goes with it.
pub(crate) struct Beside(PathBuf);

impl Drop for Beside {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

#[path = "save_onto.rs"]
pub(crate) mod onto;

/// What the work of a save that is done elsewhere gives back.
struct Saved {
    done: Written,
    /// The changes went into the data file where it is: there is no new file to put in its place.
    in_place: bool,
    /// The data file's length and time of writing when the work was done with it.
    stamp: Option<(u64, std::time::SystemTime)>,
}

const BEING_SAVED: &str = "the data file is being saved; changes are taken again when that is done";
const STOPPED: &str = "the save was stopped; nothing was replaced, and the changes are still unsaved";

/// Messages taken out of a folder are remembered this many at a time, so that one put into
/// another folder is the same message still.
const MAX_LIMBO: usize = 32;

/// One message of a folder that was looked into.
#[derive(Clone)]
struct Entry {
    nid: u32,
    summary: Summary,
    /// The message as changed here; `None` while it is as the file has it.
    item: Option<Box<Item>>,
    /// True when the file has no such message yet.
    new: bool,
}

#[derive(Clone)]
struct EFolder {
    info: FolderInfo,
    nid: u32,
    new: bool,
    /// The messages, once the folder was looked into; `None` while they are as the file has them.
    items: Option<Vec<Entry>>,
    /// What the file lists in this folder, when `items` is there.
    had: Vec<u32>,
}

impl EFolder {
    fn recount(&mut self) {
        if let Some(items) = &self.items {
            self.info.total = u32::try_from(items.len()).unwrap_or(u32::MAX);
            self.info.unread = u32::try_from(items.iter().filter(|e| !e.summary.read).count()).unwrap_or(u32::MAX);
        }
    }

    /// True when the folder's list of messages is not what the file has.
    fn contents_changed(&self) -> bool {
        self.new
            || self.items.as_ref().is_some_and(|items| {
                items.len() != self.had.len() || items.iter().zip(&self.had).any(|(e, had)| e.nid != *had || e.item.is_some() || e.new)
            })
    }
}

/// What a folder was when the file was opened.
#[derive(Clone)]
struct Was {
    parent: Option<u32>,
    name: String,
    has_children: bool,
}

/// The changes made to an open data file and not yet saved.
#[derive(Clone)]
pub(crate) struct Edits {
    /// The visible folders, each followed by what is inside it.
    folders: Vec<EFolder>,
    was: HashMap<u32, Was>,
    limbo: Vec<(Entry, Item)>,
    /// The next index for the node of a new folder or message.
    next: u32,
    pub(crate) dirty: bool,
}

impl Edits {
    fn at(&self, folder: FolderId) -> Result<usize, StoreError> {
        self.folders.iter().position(|f| f.info.id == folder).ok_or(StoreError::NoFolder)
    }

    fn fresh(&mut self, ty: u32) -> Result<u32, StoreError> {
        let index = self.next;
        if index > MAX_INDEX {
            return Err(StoreError::Format("the data file has no room for another item".into()));
        }
        self.next = index + 1;
        Ok(index << 5 | ty)
    }

    /// A folder and every folder inside it, as places in the list, in list order.
    fn subtree(&self, folder: FolderId) -> Vec<FolderId> {
        let mut all = vec![folder];
        let mut i = 0;
        while let Some(&cur) = all.get(i) {
            let inside: Vec<FolderId> =
                self.folders.iter().filter(|f| f.info.parent == Some(cur) && !all.contains(&f.info.id)).map(|f| f.info.id).collect();
            all.extend(inside);
            i += 1;
        }
        all
    }

    fn mark_parents(&mut self) {
        let parents: HashSet<FolderId> = self.folders.iter().filter_map(|f| f.info.parent).collect();
        for f in &mut self.folders {
            f.info.has_children = parents.contains(&f.info.id);
        }
    }

    /// Where a folder put under `parent` goes in the list: after the last folder inside it.
    fn end_of(&self, parent: FolderId) -> usize {
        let mut under = vec![parent];
        let mut after = 0;
        for (at, f) in self.folders.iter().enumerate() {
            if f.info.id == parent || f.info.parent.is_some_and(|p| under.contains(&p)) {
                under.push(f.info.id);
                after = at + 1;
            }
        }
        after.min(self.folders.len())
    }
}

fn nid_of(id: u64) -> Option<u32> {
    u32::try_from(id).ok()
}

/// A file's length and when it was last written: enough to see that another program wrote it.
fn stamp(path: &Path) -> Option<(u64, std::time::SystemTime)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

impl PstStore {
    /// True when changes to this data file can be kept: a `.pst` of any kind, and it is
    /// saved as the kind it is (one with 4K pages is written whole). An `.ost` is a copy of
    /// a mailbox on a server and is only ever read.
    pub(crate) fn can_edit(&self) -> bool {
        !self.path.as_deref().is_some_and(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ost")))
    }

    pub(crate) fn note_stamp(&mut self) {
        self.stamp = self.path.as_deref().and_then(stamp);
        // A file a save was cut short in is known as the file it was.
        self.ident = match &self.torn {
            Some(undo) => undo.ident(),
            None => self.path.as_deref().and_then(journal::ident),
        };
    }

    /// Write a request that was done into the journal.
    fn log(&mut self, op: &Op) {
        self.journal.log(self.path.as_deref(), self.ident, op);
    }

    pub(crate) fn e_journal_trouble(&self) -> Option<String> {
        self.journal.trouble.clone().filter(|_| self.e_unsaved())
    }

    /// A folder to keep the journal in when it cannot be kept beside the data file.
    pub fn journal_elsewhere(&mut self, dir: &Path) {
        self.journal.elsewhere = Some(dir.to_path_buf());
    }

    /// Where this run's journal of unsaved changes is, when it has one.
    pub fn journal_place(&self) -> Option<&Path> {
        self.journal.place()
    }

    /// Have the disk keep the journal as far as it is written. True when there was something
    /// to keep.
    pub fn settle(&mut self) -> bool {
        self.journal.settle()
    }

    /// Throw the unsaved changes away, and their journal with them.
    pub(crate) fn e_discard(&mut self) {
        if self.saving.is_some() {
            return;
        }
        self.edits = None;
        self.journal.finish();
    }

    /// Bring back the changes a run that ended without saving left in the journal beside the
    /// file. They are unsaved again, as they were; the data file is not written. Playing back
    /// stops at the first change that is damaged or cannot be made; when any is lost that
    /// way, the journal as it was found is kept beside the file under another name.
    pub fn recover(&mut self) -> Result<Recovery, PstError> {
        let mut out = Recovery::default();
        let (Some(path), Some(ident)) = (self.path.clone(), self.ident) else { return Ok(out) };
        if !self.can_edit() || self.e_unsaved() || self.journal.open() || self.saving.is_some() {
            return Ok(out);
        }
        match Journal::read(&path, ident, self.journal.elsewhere.as_deref())? {
            Found::Nothing => {}
            Found::Stale => out.stale = true,
            Found::Held => {
                out.held = true;
                self.journal.trouble = Some("another running OpenMailClient has this data file's journal".to_string());
            }
            Found::Records { mut file, at, records, after } => {
                let total = records.len();
                let (mut end, mut applied) = (journal::HEADER, 0usize);
                self.journal.playing = true;
                for (payload, ends) in records {
                    if Op::decode(&payload).and_then(|op| self.play(op).ok()).is_none() {
                        break;
                    }
                    (end, applied) = (ends, applied.saturating_add(1));
                }
                self.journal.playing = false;
                out.applied = u32::try_from(applied).unwrap_or(u32::MAX);
                // What could not be made again and all after it, the records that are there
                // but damaged, and the one that was cut short as it was written.
                out.lost = u32::try_from(total.saturating_sub(applied))
                    .unwrap_or(u32::MAX)
                    .saturating_add(after.damaged)
                    .saturating_add(u32::from(after.cut || after.unknown));
                out.more = after.unknown;
                if out.lost > 0 || out.more {
                    journal::keep_aside(&mut file, &at);
                }
                if applied == 0 {
                    // Nothing to go on from: no journal until there is a change to write.
                    self.edits = None;
                    drop(file);
                    let _ = std::fs::remove_file(&at);
                } else if let Err(e) = self.journal.resume(file, at, end, out.applied) {
                    self.journal.trouble = Some(format!("the journal could not be gone on with ({e})"));
                }
            }
        }
        Ok(out)
    }

    /// Do one journalled request again.
    fn play(&mut self, op: Op) -> Result<(), StoreError> {
        match op {
            Op::Update { folder, id, diff } => {
                let (folder, id) = (FolderId(folder), ItemId(id));
                self.looked_into(folder)?;
                let mut item = self.e_load(folder, id)?;
                diff.apply(&mut item);
                self.e_update(folder, id, item)
            }
            Op::Add { folder, item } => self.e_add(FolderId(folder), item).map(|_| ()),
            Op::Put { folder, nid } => {
                let at = self.looked_into(FolderId(folder))?;
                let edits = self.edits()?;
                let i = edits.limbo.iter().position(|(e, _)| e.nid == nid).ok_or(StoreError::NoItem)?;
                let entry = edits.limbo.swap_remove(i).0;
                self.entries(at)?.push(entry);
                self.touched(at);
                Ok(())
            }
            Op::Remove { folder, id } => self.e_remove(FolderId(folder), ItemId(id)).map(|_| ()),
            Op::CreateFolder { parent: TOP, name } => self.e_create_top_folder(&name).map(|_| ()),
            Op::CreateFolder { parent, name } => self.e_create_folder(FolderId(parent), &name).map(|_| ()),
            Op::CreateFolderOf { parent, name, class } => self.e_create_of((parent != TOP).then_some(FolderId(parent)), &name, &class).map(|_| ()),
            Op::RenameFolder { folder, name } => self.e_rename_folder(FolderId(folder), &name),
            Op::DeleteFolder { folder } => self.e_delete_folder(FolderId(folder)),
            Op::MoveFolder { folder, parent } => self.e_move_folder(FolderId(folder), FolderId(parent)),
        }
    }

    fn edits(&mut self) -> Result<&mut Edits, StoreError> {
        if !self.can_edit() {
            return Err(StoreError::ReadOnly);
        }
        if self.saving.is_some() {
            return Err(StoreError::Format(BEING_SAVED.to_string()));
        }
        if self.edits.is_none() {
            let listed = self.file_folders()?;
            let top = self.ndb.nbt.keys().map(|n| n >> 5).max().unwrap_or(0);
            let mut was = HashMap::new();
            let mut folders = Vec::with_capacity(listed.len());
            for info in listed {
                let Some(nid) = nid_of(info.id.0) else { continue };
                was.insert(nid, Was { parent: info.parent.and_then(|p| nid_of(p.0)), name: info.name.clone(), has_children: info.has_children });
                folders.push(EFolder { info, nid, new: false, items: None, had: Vec::new() });
            }
            let mut edits = Edits { folders, was, limbo: Vec::new(), next: top.saturating_add(1).max(0x1_0000), dirty: false };
            // What the file says of subfolders may be stale; the list is what counts.
            edits.mark_parents();
            for f in &edits.folders {
                if let Some(w) = edits.was.get_mut(&f.nid) {
                    w.has_children = f.info.has_children;
                }
            }
            self.edits = Some(edits);
        }
        self.edits.as_mut().ok_or(StoreError::ReadOnly)
    }

    /// The place of a folder in the list of changes, its messages looked into.
    fn looked_into(&mut self, folder: FolderId) -> Result<usize, StoreError> {
        let at = self.edits()?.at(folder)?;
        let known = self.edits.as_ref().and_then(|e| e.folders.get(at)).is_some_and(|f| f.items.is_some());
        if !known {
            let new = self.edits.as_ref().and_then(|e| e.folders.get(at)).is_some_and(|f| f.new);
            let listed = if new { Vec::new() } else { self.file_list(folder)? };
            if let Some(f) = self.edits.as_mut().and_then(|e| e.folders.get_mut(at)) {
                f.had = listed.iter().filter_map(|s| nid_of(s.id)).collect();
                f.items = Some(
                    listed.into_iter().filter_map(|summary| Some(Entry { nid: nid_of(summary.id)?, summary, item: None, new: false })).collect(),
                );
            }
        }
        Ok(at)
    }

    fn entries(&mut self, at: usize) -> Result<&mut Vec<Entry>, StoreError> {
        self.edits.as_mut().and_then(|e| e.folders.get_mut(at)).and_then(|f| f.items.as_mut()).ok_or(StoreError::NoFolder)
    }

    fn touched(&mut self, at: usize) {
        if let Some(e) = self.edits.as_mut() {
            e.dirty = true;
            if let Some(f) = e.folders.get_mut(at) {
                f.recount();
            }
        }
    }

    pub(crate) fn e_unsaved(&self) -> bool {
        self.edits.as_ref().is_some_and(|e| e.dirty)
    }

    pub(crate) fn e_folders(&mut self) -> Result<Vec<FolderInfo>, StoreError> {
        match &self.edits {
            Some(e) => Ok(e.folders.iter().map(|f| f.info.clone()).collect()),
            None => self.file_folders(),
        }
    }

    pub(crate) fn e_list(&mut self, folder: FolderId) -> Result<Vec<Summary>, StoreError> {
        let Some(e) = &self.edits else { return self.file_list(folder) };
        match &e.folders.get(e.at(folder)?).ok_or(StoreError::NoFolder)?.items {
            Some(items) => Ok(items.iter().map(|e| e.summary.clone()).collect()),
            None => self.file_list(folder),
        }
    }

    pub(crate) fn e_load(&mut self, folder: FolderId, id: ItemId) -> Result<Item, StoreError> {
        let Some(e) = &self.edits else { return self.file_load(id) };
        if let Some(items) = &e.folders.get(e.at(folder)?).ok_or(StoreError::NoFolder)?.items {
            let entry = items.iter().find(|e| u64::from(e.nid) == id.0).ok_or(StoreError::NoItem)?;
            if let Some(item) = &entry.item {
                return Ok((**item).clone());
            }
        } else if e.folders.iter().any(|f| f.items.is_some() && f.had.iter().any(|n| u64::from(*n) == id.0)) {
            // It was in a folder that was looked into, and is there no longer.
            return Err(StoreError::NoItem);
        }
        self.file_load(id)
    }

    pub(crate) fn e_update(&mut self, folder: FolderId, id: ItemId, item: Item) -> Result<(), StoreError> {
        let at = self.looked_into(folder)?;
        let on = self.journal.on();
        let entry = self.entries(at)?.iter().find(|e| u64::from(e.nid) == id.0).ok_or(StoreError::NoItem)?;
        // The journal takes what changed in the message, not the message.
        let op = if on {
            let diff = match entry.item.as_deref().map(|was| Diff::between(was, &item)) {
                Some(diff) => Ok(diff),
                None => self.file_load(id).map(|was| Diff::between(&was, &item)),
            };
            match diff {
                Ok(diff) => Some(Op::Update { folder: folder.0, id: id.0, diff }),
                Err(e) => {
                    self.journal.trouble = Some(format!("a message could not be read to journal a change to it ({e})"));
                    None
                }
            }
        } else {
            None
        };
        let entry = self.entries(at)?.iter_mut().find(|e| u64::from(e.nid) == id.0).ok_or(StoreError::NoItem)?;
        entry.summary = item.summary(id.0);
        entry.item = Some(Box::new(item));
        self.touched(at);
        if let Some(op) = op {
            self.log(&op);
        }
        Ok(())
    }

    pub(crate) fn e_add(&mut self, folder: FolderId, item: Item) -> Result<ItemId, StoreError> {
        let at = self.looked_into(folder)?;
        let edits = self.edits()?;
        // A message just taken out of a folder is the same message when it is put into one:
        // it keeps its node, and with it everything the file holds for it.
        let (entry, op) = match edits.limbo.iter().position(|(_, was)| *was == item) {
            Some(i) => {
                let entry = edits.limbo.swap_remove(i).0;
                let op = Op::Put { folder: folder.0, nid: entry.nid };
                (entry, op)
            }
            None => {
                let nid = edits.fresh(TYPE_MESSAGE)?;
                let summary = item.summary(u64::from(nid));
                // The journal's copy is made only when there is a journal to write it to.
                let op = Op::Add { folder: folder.0, item: if self.journal.on() { item.clone() } else { Item::default() } };
                (Entry { nid, summary, item: Some(Box::new(item)), new: true }, op)
            }
        };
        let id = ItemId(u64::from(entry.nid));
        self.entries(at)?.push(entry);
        self.touched(at);
        self.log(&op);
        Ok(id)
    }

    pub(crate) fn e_remove(&mut self, folder: FolderId, id: ItemId) -> Result<Item, StoreError> {
        let at = self.looked_into(folder)?;
        let items = self.entries(at)?;
        let pos = items.iter().position(|e| u64::from(e.nid) == id.0).ok_or(StoreError::NoItem)?;
        let item = match items.get(pos).and_then(|e| e.item.as_deref()) {
            Some(item) => item.clone(),
            None => self.file_load(id)?,
        };
        let entry = self.entries(at)?.remove(pos);
        let edits = self.edits()?;
        if edits.limbo.len() >= MAX_LIMBO {
            edits.limbo.remove(0);
        }
        edits.limbo.push((entry, item.clone()));
        self.touched(at);
        self.log(&Op::Remove { folder: folder.0, id: id.0 });
        Ok(item)
    }

    pub(crate) fn e_create_folder(&mut self, parent: FolderId, name: &str) -> Result<FolderId, StoreError> {
        self.e_create_in(Some(parent), name)
    }

    /// A mail folder at the top of the folder tree, under the root of the mailbox as the Inbox
    /// is ([MS-PST] 2.4.3).
    pub(crate) fn e_create_top_folder(&mut self, name: &str) -> Result<FolderId, StoreError> {
        self.e_create_in(None, name)
    }

    fn e_create_in(&mut self, parent: Option<FolderId>, name: &str) -> Result<FolderId, StoreError> {
        let edits = self.edits()?;
        let class = match parent {
            Some(parent) => edits.folders.get(edits.at(parent)?).map(|f| f.info.container_class.clone()).unwrap_or_default(),
            None => "IPF.Note".to_string(),
        };
        let id = self.e_make(parent, name, class)?;
        self.log(&Op::CreateFolder { parent: parent.map_or(TOP, |p| p.0), name: name.to_string() });
        Ok(id)
    }

    /// A folder of the kind `class` names, inside `parent` or at the top of the tree.
    pub(crate) fn e_create_of(&mut self, parent: Option<FolderId>, name: &str, class: &str) -> Result<FolderId, StoreError> {
        if class.is_empty() || class.chars().count() > 255 || class.chars().any(char::is_control) {
            return Err(StoreError::Format(format!("{class:?} is no folder class")));
        }
        if let Some(p) = parent {
            let edits = self.edits()?;
            edits.at(p)?;
        }
        let id = self.e_make(parent, name, class.to_string())?;
        self.log(&Op::CreateFolderOf { parent: parent.map_or(TOP, |p| p.0), name: name.to_string(), class: class.to_string() });
        Ok(id)
    }

    fn e_make(&mut self, parent: Option<FolderId>, name: &str, class: String) -> Result<FolderId, StoreError> {
        let edits = self.edits()?;
        let nid = edits.fresh(TYPE_FOLDER)?;
        let id = FolderId(u64::from(nid));
        let info = FolderInfo {
            id,
            parent,
            name: name.to_string(),
            kind: FolderKind::guess(name, &class),
            container_class: class,
            total: 0,
            unread: 0,
            has_children: false,
            hidden: false,
        };
        let at = parent.map_or(edits.folders.len(), |p| edits.end_of(p));
        edits.folders.insert(at, EFolder { info, nid, new: true, items: Some(Vec::new()), had: Vec::new() });
        edits.mark_parents();
        edits.dirty = true;
        Ok(id)
    }

    pub(crate) fn e_rename_folder(&mut self, folder: FolderId, name: &str) -> Result<(), StoreError> {
        let edits = self.edits()?;
        let at = edits.at(folder)?;
        if let Some(f) = edits.folders.get_mut(at) {
            f.info.name = name.to_string();
        }
        edits.dirty = true;
        self.log(&Op::RenameFolder { folder: folder.0, name: name.to_string() });
        Ok(())
    }

    pub(crate) fn e_delete_folder(&mut self, folder: FolderId) -> Result<(), StoreError> {
        let edits = self.edits()?;
        edits.at(folder)?;
        let doomed = edits.subtree(folder);
        edits.folders.retain(|f| !doomed.contains(&f.info.id));
        edits.mark_parents();
        edits.dirty = true;
        self.log(&Op::DeleteFolder { folder: folder.0 });
        Ok(())
    }

    pub(crate) fn e_move_folder(&mut self, folder: FolderId, parent: FolderId) -> Result<(), StoreError> {
        let edits = self.edits()?;
        let at = edits.at(folder)?;
        edits.at(parent)?;
        let moved = edits.subtree(folder);
        if moved.contains(&parent) {
            return Err(StoreError::Format("a folder cannot be moved into itself or a folder inside it".into()));
        }
        if let Some(f) = edits.folders.get_mut(at) {
            f.info.parent = Some(parent);
        }
        let (mut taken, rest): (Vec<EFolder>, Vec<EFolder>) =
            std::mem::take(&mut edits.folders).into_iter().partition(|f| moved.contains(&f.info.id));
        taken.sort_by_key(|f| moved.iter().position(|id| *id == f.info.id));
        edits.folders = rest;
        let after = edits.end_of(parent);
        let tail = edits.folders.split_off(after);
        edits.folders.extend(taken);
        edits.folders.extend(tail);
        edits.mark_parents();
        edits.dirty = true;
        self.log(&Op::MoveFolder { folder: folder.0, parent: parent.0 });
        Ok(())
    }

    /// Write the data file with the changes made so far to `out`, a whole new file. The open
    /// file is only read, and the changes stay as they are.
    pub fn save_to<W: Read + Write + Seek>(&mut self, out: W, seed: [u8; 16]) -> Result<(W, Written), PstError> {
        self.save_with(out, seed, Vec::new())
    }

    /// As `save_to`. `put` are nodes (id, parent, properties) written in place of what the
    /// file has under those ids: the tests make files with content the model cannot make.
    pub(crate) fn save_with<W: Read + Write + Seek>(
        &mut self,
        out: W,
        seed: [u8; 16],
        put: Vec<(u32, u32, Vec<Raw>)>,
    ) -> Result<(W, Written), PstError> {
        let edits = self.edits.take();
        let done = self.write(out, seed, edits.as_ref(), put);
        self.edits = edits;
        done
    }

    fn write<W: Read + Write + Seek>(
        &mut self,
        out: W,
        seed: [u8; 16],
        edits: Option<&Edits>,
        put: Vec<(u32, u32, Vec<Raw>)>,
    ) -> Result<(W, Written), PstError> {
        // Data blocks are permuted, as in the data files Outlook makes ([MS-PST] 2.2.2.6).
        // The file is written as the kind it is: what is copied block for block has blocks
        // of that kind's size.
        let mut o = Out {
            w: NdbWriter::of(out, crate::write::crypt_of(self.ndb.ver), self.ndb.ver)?,
            names: Names::seeded(&self.names),
            uid: seed,
            serial: 0,
        };
        o.w.carry(self.ndb.counters);
        let (o, done) = self.write_into(o, edits, put)?;
        Ok((o.w.finish()?, done))
    }

    /// Give the writer every node of the mailbox as it stands: those the changes do not
    /// touch as they are stored, the others anew.
    fn write_into<W: Read + Write + Seek>(
        &mut self,
        mut o: Out<W>,
        edits: Option<&Edits>,
        put: Vec<(u32, u32, Vec<Raw>)>,
    ) -> Result<(Out<W>, Written), PstError> {
        let mut budget = self.ndb.block_count().saturating_mul(16).saturating_add(4096);
        let root = self.root;
        let none: &[EFolder] = &[];
        let folders = edits.map_or(none, |e| e.folders.as_slice());
        let parent_of = |f: &EFolder| f.info.parent.and_then(|p| nid_of(p.0)).unwrap_or(root);

        // Where each message of a folder that was looked into goes, and what is gone.
        let view: HashMap<u32, &EFolder> = folders.iter().map(|f| (f.nid, f)).collect();
        let mut place: HashMap<u32, (u32, &Entry)> = HashMap::new();
        for f in folders {
            for e in f.items.iter().flatten() {
                place.insert(e.nid, (f.nid, e));
            }
        }
        let mut gone: HashSet<u32> = folders.iter().flat_map(|f| &f.had).filter(|n| !place.contains_key(n)).copied().collect();
        let dead: HashSet<u32> = edits.map(|e| e.was.keys().filter(|n| !view.contains_key(n)).copied().collect()).unwrap_or_default();
        for (nid, e) in &self.ndb.nbt {
            let ty = nid & 0x1F;
            let own = matches!(ty, TYPE_FOLDER | TYPE_HIERARCHY | TYPE_CONTENTS | TYPE_ASSOC) && dead.contains(&(nid & !0x1F | TYPE_FOLDER));
            if !place.contains_key(nid) && (own || (ty != TYPE_FOLDER && dead.contains(&e.parent))) {
                gone.insert(*nid);
            }
        }
        // The folders whose own node changes, and those whose table of subfolders does.
        let changed = |f: &EFolder| {
            f.new
                || f.contents_changed()
                || edits.and_then(|e| e.was.get(&f.nid)).is_none_or(|w| {
                    w.name != f.info.name || w.parent != f.info.parent.and_then(|p| nid_of(p.0)) || w.has_children != f.info.has_children
                })
        };
        let mut tables: HashSet<u32> = HashSet::new();
        for f in folders {
            if f.new {
                tables.insert(f.nid);
            }
            if changed(f) {
                tables.insert(parent_of(f));
                tables.insert(edits.and_then(|e| e.was.get(&f.nid)).and_then(|w| w.parent).unwrap_or(root));
            }
        }
        for nid in &dead {
            tables.insert(edits.and_then(|e| e.was.get(nid)).and_then(|w| w.parent).unwrap_or(root));
        }
        #[allow(unused_mut)]
        let mut put_ids: HashSet<u32> = put.iter().map(|p| p.0).collect();
        #[cfg(test)]
        for (nid, columns, rows) in TABLES.take() {
            o.tc_node(nid, &columns, &rows)?;
            put_ids.insert(nid);
        }

        // What a changed or new message shows in its folder's table: its short values, and
        // the properties it no longer has.
        let mut shown: HashMap<u32, (Vec<Raw>, Vec<u16>)> = HashMap::new();
        // Search folders' tables wait until every changed message has been written.
        let mut searches = Vec::new();
        let mut nids: Vec<u32> = self.ndb.nbt.keys().copied().collect();
        nids.sort_unstable();
        let progress = self.progress.clone();
        if let Some(p) = &progress {
            // The nodes to write, and as much again for checking the new file and reading it back.
            p.begin((nids.len() as u64).saturating_mul(2));
        }
        for nid in nids {
            if let Some(p) = &progress {
                if p.stopped() {
                    return Err(PstError::Write(STOPPED.to_string()));
                }
                p.step();
            }
            let Some(e) = self.ndb.nbt.get(&nid).copied() else { continue };
            if put_ids.contains(&nid) {
                continue;
            }
            if let Some((folder, entry)) = place.get(&nid) {
                match &entry.item {
                    Some(item) => {
                        o.w.near(e.data, e.sub);
                        let (data, sub, cells, removed) = self.merged(&mut o, e, item, &mut budget)?;
                        o.w.node(nid, *folder, data, sub);
                        shown.insert(nid, (cells, removed));
                    }
                    None => self.copy_node(&mut o, nid, *folder, e, &mut budget)?,
                }
                continue;
            }
            if gone.contains(&nid) {
                continue;
            }
            let ty = nid & 0x1F;
            let folder = nid & !0x1F | TYPE_FOLDER;
            if edits.is_some() {
                // Written last, with what this save changed queued in it.
                if nid == NID_NAME_TO_ID_MAP || nid == NID_SEARCH_MANAGEMENT_QUEUE {
                    continue;
                }
                if ty == TYPE_HIERARCHY && tables.contains(&folder) {
                    continue;
                }
                if ty == TYPE_CONTENTS && view.get(&folder).is_some_and(|f| f.contents_changed()) {
                    continue;
                }
                if ty == TYPE_FOLDER
                    && let Some(f) = view.get(&nid).filter(|f| changed(f))
                {
                    o.w.near(e.data, e.sub);
                    let (mut props, kept) = self.open_pc(e)?;
                    for raw in folder_cells(f) {
                        props.insert(raw.id, raw);
                    }
                    let (data, sub, _) = self.close_pc(&mut o, props, &kept, Vec::new(), None, &mut budget)?;
                    o.w.node(nid, parent_of(f), data, sub);
                    continue;
                }
                if ty == TYPE_SEARCH_CONTENTS {
                    searches.push((nid, e));
                    continue;
                }
            }
            self.copy_node(&mut o, nid, e.parent, e, &mut budget)?;
        }
        for (nid, e) in searches {
            if !self.search_table(&mut o, nid, e, &gone, &shown)? {
                self.copy_node(&mut o, nid, e.parent, e, &mut budget)?;
            }
        }
        for (nid, parent, props) in &put {
            o.pc_node(*nid, *parent, props)?;
        }

        // What is new: messages, then folders.
        for f in folders {
            for entry in f.items.iter().flatten().filter(|e| !self.ndb.nbt.contains_key(&e.nid)) {
                let Some(item) = &entry.item else { continue };
                let (data, sub, cells) = o.message(item, 0)?;
                o.w.node(entry.nid, f.nid, data, sub);
                shown.insert(entry.nid, (cells, Vec::new()));
            }
            if f.new {
                o.pc_node(f.nid, parent_of(f), &folder_cells(f))?;
                o.tc_node(f.nid & !0x1F | TYPE_ASSOC, ASSOC, &[])?;
            }
        }
        // The tables of messages that changed ([MS-PST] 2.4.4.5).
        for f in folders.iter().filter(|f| f.contents_changed()) {
            let table = f.nid & !0x1F | TYPE_CONTENTS;
            let (mut columns, rows) = self.table_of(table).unwrap_or_else(|| (CONTENTS.iter().chain(CONTENTS_MORE).copied().collect(), Vec::new()));
            // A table written while its folder was empty has only the columns every table has;
            // once it holds messages it has the sender and the flag too, as a whole save writes
            // it, or a flag set on a message would never show in the list.
            let mut grown = Vec::new();
            if f.items.iter().flatten().next().is_some() {
                for c in CONTENTS_MORE {
                    if !columns.iter().any(|(id, _)| *id == c.0) {
                        columns.push(*c);
                        grown.push(*c);
                    }
                }
            }
            let mut by: HashMap<u32, Vec<Raw>> = rows.into_iter().map(|r| (r.id, r.cells)).collect();
            let mut rows = Vec::new();
            for entry in f.items.iter().flatten() {
                let cells = match (shown.get(&entry.nid), by.remove(&entry.nid)) {
                    // A changed message: its row as it was, less what it lost, under what it has now.
                    (Some((now, removed)), was) => {
                        let mut cells: BTreeMap<u16, Raw> =
                            was.unwrap_or_default().into_iter().filter(|c| !removed.contains(&c.id)).map(|c| (c.id, c)).collect();
                        show(&mut cells, &columns, now);
                        cells.into_values().collect()
                    }
                    (None, Some(was)) if grown.is_empty() => was,
                    // The columns the table was given are filled from the message.
                    (None, Some(mut was)) => {
                        let stored = self.ndb.nbt.get(&entry.nid).copied().and_then(|e| self.open_pc(e).ok()).map(|p| p.0).unwrap_or_default();
                        was.extend(stored.values().filter_map(|r| cell_for(&grown, r)));
                        was
                    }
                    // A message that came from another folder: what its own properties give.
                    (None, None) => {
                        let stored = self.ndb.nbt.get(&entry.nid).copied().and_then(|e| self.open_pc(e).ok()).map(|p| p.0).unwrap_or_default();
                        stored.values().filter_map(|r| cell_for(&columns, r)).collect()
                    }
                };
                rows.push(Row { id: entry.nid, cells });
            }
            self.near(&mut o, table);
            o.tc_node(table, &columns, &rows)?;
        }
        // The tables of subfolders that changed ([MS-PST] 2.4.4.4).
        for parent in tables.iter().filter(|n| !dead.contains(n)) {
            let table = parent & !0x1F | TYPE_HIERARCHY;
            let (columns, rows) = self.table_of(table).unwrap_or_else(|| (HIERARCHY.to_vec(), Vec::new()));
            let mut by: HashMap<u32, Vec<Raw>> = rows.into_iter().map(|r| (r.id, r.cells)).collect();
            let mut rows = Vec::new();
            for f in folders.iter().filter(|f| parent_of(f) == *parent) {
                // A folder that came from elsewhere brings what its own properties give.
                let was = match by.remove(&f.nid) {
                    Some(was) => was,
                    None => {
                        let stored = self.ndb.nbt.get(&f.nid).copied().and_then(|e| self.open_pc(e).ok()).map(|p| p.0).unwrap_or_default();
                        stored.values().filter_map(|r| cell_for(&columns, r)).collect()
                    }
                };
                let mut cells: BTreeMap<u16, Raw> = was.into_iter().map(|c| (c.id, c)).collect();
                cells.extend(folder_cells(f).into_iter().map(|r| (r.id, r)));
                rows.push(Row { id: f.nid, cells: cells.into_values().collect() });
            }
            // A folder this program does not list (it is hidden, or its row is damaged) stays.
            rows.extend(by.into_iter().filter(|(nid, _)| !dead.contains(nid) && !view.contains_key(nid)).map(|(id, cells)| Row { id, cells }));
            rows.sort_by_key(|r| folders.iter().position(|f| f.nid == r.id).unwrap_or(usize::MAX));
            self.near(&mut o, table);
            o.tc_node(table, &columns, &rows)?;
        }
        // The names of named properties, with those that are new ([MS-PST] 2.4.7).
        if edits.is_some()
            && !put_ids.contains(&NID_NAME_TO_ID_MAP)
            && let Some(e) = self.ndb.nbt.get(&NID_NAME_TO_ID_MAP).copied()
        {
            if o.names.grew() {
                o.w.near(e.data, e.sub);
                let (props, kept) = self.open_pc(e)?;
                let mut props: Vec<Raw> = props.into_values().collect();
                o.names.add_to(&mut props);
                let props: BTreeMap<u16, Raw> = props.into_iter().map(|r| (r.id, r)).collect();
                let (data, sub, _) = self.close_pc(&mut o, props, &kept, Vec::new(), None, &mut budget)?;
                o.w.node(NID_NAME_TO_ID_MAP, e.parent, data, sub);
            } else {
                self.copy_node(&mut o, NID_NAME_TO_ID_MAP, e.parent, e, &mut budget)?;
            }
        }
        if let Some(edits) = edits {
            let suds = self.updates(edits, &gone, &dead);
            self.queue(&mut o, &suds, &mut budget)?;
        }
        let done = match edits {
            Some(_) => {
                Written { folders: folders.len() as u64, items: folders.iter().map(|f| u64::from(f.info.total)).sum(), unreadable: 0, hidden: 0 }
            }
            None => Written::default(),
        };
        Ok((o, done))
    }
}

impl PstStore {
    /// What a save of `edits` changes, in the order it is told to the program that keeps the
    /// search folders ([MS-PST] 2.4.8.1): folders made, moved and changed, then each message
    /// moved, changed, put in and taken out, then folders deleted. A folder's change is one
    /// of its name, its place or whether it has subfolders; a message's own counts too.
    fn updates(&self, edits: &Edits, gone: &HashSet<u32>, dead: &HashSet<u32>) -> Vec<Sud> {
        let root = self.root;
        let (mut folders, mut messages, mut deleted) = (Vec::new(), Vec::new(), Vec::new());
        for f in &edits.folders {
            let parent = f.info.parent.and_then(|p| nid_of(p.0)).unwrap_or(root);
            match edits.was.get(&f.nid) {
                _ if f.new => folders.push(Sud::FldAdd { parent, fld: f.nid }),
                None => {}
                Some(w) => {
                    if w.parent.unwrap_or(root) != parent {
                        folders.push(Sud::FldMov { parent, fld: f.nid });
                    }
                    if w.name != f.info.name || w.has_children != f.info.has_children {
                        folders.push(Sud::FldMod { fld: f.nid });
                    }
                }
            }
            for e in f.items.iter().flatten() {
                match self.ndb.nbt.get(&e.nid) {
                    Some(was) => {
                        if was.parent != f.nid {
                            messages.push(Sud::MsgMov { new: f.nid, msg: e.nid, old: was.parent });
                        }
                        if e.item.is_some() {
                            messages.push(Sud::MsgMod { parent: f.nid, msg: e.nid });
                        }
                    }
                    None => messages.push(Sud::MsgAdd { parent: f.nid, msg: e.nid }),
                }
            }
        }
        let mut lost: Vec<(u32, u32)> =
            gone.iter().filter(|n| *n & 0x1F == TYPE_MESSAGE).filter_map(|n| self.ndb.nbt.get(n).map(|e| (e.parent, *n))).collect();
        lost.sort_unstable();
        messages.extend(lost.into_iter().map(|(parent, msg)| Sud::MsgDel { parent, msg }));
        let mut dead: Vec<u32> = dead.iter().copied().collect();
        dead.sort_unstable();
        deleted.extend(dead.into_iter().map(|fld| Sud::FldDel { fld }));
        folders.into_iter().chain(messages).chain(deleted).collect()
    }

    /// The Search Management Queue with `suds` added behind what waits in it ([MS-PST]
    /// 2.4.8.4.1); a queue that may not be read, or a file without one, keeps what it has.
    fn queue<W: Read + Write + Seek>(&mut self, o: &mut Out<W>, suds: &[Sud], budget: &mut usize) -> Result<(), PstError> {
        let Some(e) = self.ndb.nbt.get(&NID_SEARCH_MANAGEMENT_QUEUE).copied() else {
            return Ok(());
        };
        let joined = match suds {
            [] => None,
            _ => search_queue::queued(&self.ndb.data(e.data)?, e.parent, suds),
        };
        match joined {
            Some(bytes) => {
                self.near(o, NID_SEARCH_MANAGEMENT_QUEUE);
                let max = o.w.max_data();
                let data = o.w.data(bytes.chunks(max))?;
                // The head is back at the first SUD (2.4.8.3).
                o.w.node(NID_SEARCH_MANAGEMENT_QUEUE, 0, data, 0);
                Ok(())
            }
            None => self.copy_node(o, NID_SEARCH_MANAGEMENT_QUEUE, e.parent, e, budget),
        }
    }
}

/// Put what a message has now into its row of a table with these columns. A value of
/// another type than its column's goes in as the column's type when nothing is lost by
/// that; one that cannot leaves its cell empty, and not saying what the message said before.
fn show(cells: &mut BTreeMap<u16, Raw>, columns: &[(u16, u16)], now: &[Raw]) {
    for r in now {
        match cell_for(columns, r) {
            Some(cell) => {
                cells.insert(r.id, cell);
            }
            None if columns.iter().any(|(id, _)| *id == r.id) => {
                cells.remove(&r.id);
            }
            None => {}
        }
    }
}

impl PstStore {
    /// A folder's hidden items: the messages its associated contents table lists ([MS-PST]
    /// 2.4.4.6), in its order. None for a folder the file does not have yet.
    pub(crate) fn e_hidden(&mut self, folder: FolderId) -> Result<Vec<(ItemId, Result<Item, StoreError>)>, StoreError> {
        let nid = nid_of(folder.0).ok_or(StoreError::NoFolder)?;
        let Some((_, rows)) = self.table_of(nid & !0x1F | TYPE_ASSOC) else { return Ok(Vec::new()) };
        let mut out = Vec::with_capacity(rows.len().min(MAX_HIDDEN));
        for row in rows.into_iter().take(MAX_HIDDEN) {
            let id = ItemId(u64::from(row.id));
            out.push((id, self.file_load(id)));
        }
        Ok(out)
    }

    /// A folder's own properties as the file has them; none for a folder it does not have
    /// yet. Named properties are left out: their names are the file's own.
    pub(crate) fn e_folder_props(&mut self, folder: FolderId) -> Result<PropBag, StoreError> {
        let nid = nid_of(folder.0).ok_or(StoreError::NoFolder)?;
        if !self.ndb.nbt.contains_key(&nid) {
            return Ok(PropBag::default());
        }
        let (mut bag, _) = self.node_props(nid)?;
        bag.0.retain(|id, _| *id < 0x8000);
        Ok(bag)
    }

    /// A table's columns and rows, the table's own bookkeeping left out; `None` when the
    /// file has no such table or it cannot be read.
    pub(crate) fn table_of(&mut self, nid: u32) -> Option<(Vec<(u16, u16)>, Vec<Row>)> {
        let e = self.ndb.nbt.get(&nid).copied()?;
        let subs = self.ndb.subnodes(e.sub).ok()?;
        let (mut columns, mut rows) = ltp::read_table(&mut self.ndb, e.data, &subs).ok()?;
        let own = |id: u16| id == 0x67F2 || id == 0x67F3;
        columns.retain(|c| !own(c.0));
        for row in &mut rows {
            row.cells.retain(|c| !own(c.id));
        }
        Some((columns, rows))
    }

    /// Write a search folder's table ([MS-PST] 2.4.8.6.2) with what changed: without the rows
    /// of messages that are gone, and with the rows of changed messages showing what those
    /// have now, as their folders' tables do. False when it has no row of either kind (or
    /// cannot be read) and is to be copied as it is.
    fn search_table<W: Read + Write + Seek>(
        &mut self,
        o: &mut Out<W>,
        nid: u32,
        e: NbtEntry,
        gone: &HashSet<u32>,
        shown: &HashMap<u32, (Vec<Raw>, Vec<u16>)>,
    ) -> Result<bool, PstError> {
        if gone.is_empty() && shown.is_empty() {
            return Ok(false);
        }
        let Some((columns, mut rows)) = self.table_of(nid) else { return Ok(false) };
        if !rows.iter().any(|r| gone.contains(&r.id) || shown.contains_key(&r.id)) {
            return Ok(false);
        }
        rows.retain(|r| !gone.contains(&r.id));
        for row in &mut rows {
            let Some((now, removed)) = shown.get(&row.id) else { continue };
            let mut cells: BTreeMap<u16, Raw> =
                std::mem::take(&mut row.cells).into_iter().filter(|c| !removed.contains(&c.id)).map(|c| (c.id, c)).collect();
            show(&mut cells, &columns, now);
            row.cells = cells.into_values().collect();
        }
        o.w.near(e.data, e.sub);
        let mut subs = Subs::new(1);
        let data = crate::ltp_write::write_tc(&mut o.w, &columns, &rows, &mut subs)?;
        crate::store_write::contents_extra(&mut o.w, nid, &mut subs)?;
        let sub = o.w.subnodes(subs.list)?;
        o.w.node(nid, e.parent, data, sub);
        Ok(true)
    }

    /// Tell a writer that goes into the file where it is that the node `nid` is about to be
    /// written again: what of it comes out as it was is not written a second time.
    fn near<W: Read + Write + Seek>(&self, o: &mut Out<W>, nid: u32) {
        if let Some(e) = self.ndb.nbt.get(&nid) {
            o.w.near(e.data, e.sub);
        }
    }

    /// Copy a node's data as it is stored, block for block.
    fn copy_data<W: Read + Write + Seek>(&mut self, w: &mut NdbWriter<W>, bid: u64, budget: &mut usize) -> Result<u64, PstError> {
        if bid == 0 {
            return Ok(0);
        }
        // In the file itself, the blocks are there: they are named once more, and counted as
        // if written.
        if w.in_place() {
            let n = self.ndb.data_len(bid)?;
            w.count(n);
            return w.keep(bid);
        }
        let blocks = self.ndb.data_blocks(bid)?;
        *budget = budget
            .checked_sub(blocks.len().max(1))
            .ok_or_else(|| PstError::Damaged("the file's blocks point at each other without end".to_string()))?;
        w.data(&blocks)
    }

    /// Copy a node's subnodes, and theirs.
    fn copy_subs<W: Read + Write + Seek>(&mut self, w: &mut NdbWriter<W>, bid: u64, depth: usize, budget: &mut usize) -> Result<u64, PstError> {
        if bid == 0 {
            return Ok(0);
        }
        if depth > MAX_COPY_DEPTH {
            return Err(PstError::Damaged("items are nested deeper than this version follows".to_string()));
        }
        if w.in_place() {
            let n = self.stored_under(bid, depth, budget)?;
            w.count(n);
            return w.keep(bid);
        }
        let mut subs: Vec<(u32, (u64, u64))> = self.ndb.subnodes(bid)?.into_iter().collect();
        subs.sort_unstable();
        let mut list = Vec::with_capacity(subs.len());
        for (nid, (data, sub)) in subs {
            list.push(Sub { nid, data: self.copy_data(w, data, budget)?, sub: self.copy_subs(w, sub, depth + 1, budget)? });
        }
        w.subnodes(list)
    }

    /// The bytes of data stored in the subnodes under `bid`, and theirs.
    fn stored_under(&mut self, bid: u64, depth: usize, budget: &mut usize) -> Result<u64, PstError> {
        if bid == 0 {
            return Ok(0);
        }
        if depth > MAX_COPY_DEPTH {
            return Err(PstError::Damaged("items are nested deeper than this version follows".to_string()));
        }
        let mut n = 0u64;
        for (data, sub) in self.ndb.subnodes(bid)?.into_values() {
            *budget = budget.checked_sub(1).ok_or_else(|| PstError::Damaged("the file's blocks point at each other without end".to_string()))?;
            n = n.saturating_add(self.ndb.data_len(data)?).saturating_add(self.stored_under(sub, depth + 1, budget)?);
        }
        Ok(n)
    }

    fn copy_node<W: Read + Write + Seek>(&mut self, o: &mut Out<W>, nid: u32, parent: u32, e: NbtEntry, budget: &mut usize) -> Result<(), PstError> {
        let data = self.copy_data(&mut o.w, e.data, budget)?;
        let sub = self.copy_subs(&mut o.w, e.sub, 0, budget)?;
        o.w.node(nid, parent, data, sub);
        Ok(())
    }

    /// A node's properties as they are stored, and its subnodes other than those that only
    /// hold the values of those properties.
    pub(crate) fn open_pc(&mut self, e: NbtEntry) -> Result<(BTreeMap<u16, Raw>, Subnodes), PstError> {
        let mut subs = self.ndb.subnodes(e.sub)?;
        let props = ltp::read_pc(&mut self.ndb, e.data, &subs)?;
        for nid in ltp::pc_value_subnodes(&mut self.ndb, e.data)? {
            subs.remove(&nid);
        }
        Ok((props.into_iter().map(|r| (r.id, r)).collect(), subs))
    }

    /// The attachments of a changed message, as subnodes, and its attachment table. An
    /// attachment the file already holds stays in the subnode it has, with every property
    /// stored on it: only what the model says differently is changed. `kept` loses the
    /// attachments and the table the file had.
    fn changed_attachments<W: Read + Write + Seek>(
        &mut self,
        o: &mut Out<W>,
        e: NbtEntry,
        new: &Item,
        kept: &mut Subnodes,
        budget: &mut usize,
    ) -> Result<Vec<Sub>, PstError> {
        let subs = self.ndb.subnodes(e.sub)?;
        let cp = code_page(&ltp::read_pc(&mut self.ndb, e.data, &subs)?);
        let mut stored: Vec<Option<(u32, Attachment)>> = self.attachments(&subs, cp, 0).into_iter().map(Some).collect();
        // Attachments the table lists and this program could not read: nobody was shown
        // them, so nobody took them away. They stay, as they are stored, after the others.
        let mut seen: HashSet<u32> = stored.iter().flatten().map(|(nid, _)| *nid).collect();
        let listed = match subs.get(&NID_ATTACHMENT_TABLE) {
            Some(&(data, sub)) => {
                let inside = self.ndb.subnodes(sub).unwrap_or_default();
                ltp::read_tc(&mut self.ndb, data, &inside).unwrap_or_default()
            }
            None => Vec::new(),
        };
        let unread: Vec<Row> = listed
            .into_iter()
            .filter(|r| r.id & 0x1F == TYPE_ATTACHMENT && subs.contains_key(&r.id) && seen.insert(r.id))
            .take(MAX_CHILDREN)
            .collect();
        kept.remove(&NID_ATTACHMENT_TABLE);
        kept.retain(|nid, _| nid & 0x1F != TYPE_ATTACHMENT);
        // Which stored attachment each one is: first those that are the same but for their
        // place in the list, then those with the same contents and other properties (a new
        // name, say), then files with other contents and the same properties (a file put in
        // again after it was worked on), and last files with other contents under the same
        // name. A message inside a message is matched by what it holds alone.
        let holds = |a: &Attachment, b: &Attachment| a.data == b.data && a.embedded == b.embedded;
        let same = |a: &Attachment, b: &Attachment| holds(a, b) && settled(&a.props) == settled(&b.props);
        let files = |a: &Attachment, b: &Attachment| a.embedded.is_none() && b.embedded.is_none();
        let named = |a: &Attachment, b: &Attachment| {
            !a.name().is_empty() && a.name() == b.name() && a.props.i32(pid::ATTACH_METHOD) == b.props.i32(pid::ATTACH_METHOD)
        };
        let mut was: Vec<Option<(u32, Attachment)>> = new.attachments.iter().map(|_| None).collect();
        for pass in 0..4 {
            for (a, slot) in new.attachments.iter().zip(was.iter_mut()).filter(|(_, slot)| slot.is_none()) {
                let fits = |b: &Attachment| match pass {
                    0 => same(a, b),
                    1 => holds(a, b) && (!a.data.is_empty() || a.embedded.is_some()),
                    2 => files(a, b) && settled(&a.props) == settled(&b.props),
                    _ => files(a, b) && named(a, b),
                };
                *slot = stored.iter_mut().find(|s| s.as_ref().is_some_and(|(_, b)| fits(b))).and_then(Option::take);
            }
        }
        // New attachments get subnode ids past every one the file had.
        let mut next = subs.keys().filter(|nid| *nid & 0x1F == TYPE_ATTACHMENT).map(|nid| nid >> 5).max().unwrap_or(0).max(0x400);
        let mut list = Vec::new();
        let mut rows = Vec::with_capacity(new.attachments.len());
        for (i, (a, was)) in new.attachments.iter().zip(was).enumerate() {
            let at = was.and_then(|(nid, b)| Some((nid, *subs.get(&nid)?, b)));
            let merged = match at {
                Some((nid, (data, sub), b)) => self.merged_attachment(o, data, sub, &b, a, i, budget).ok().map(|m| (nid, m)),
                None => None,
            };
            let (nid, (data, sub, cells)) = match merged {
                Some(m) => m,
                None => {
                    next = next.checked_add(1).filter(|n| *n <= MAX_INDEX).ok_or_else(|| cannot("that many attachments"))?;
                    (next << 5 | TYPE_ATTACHMENT, o.attachment(a, i, 0)?)
                }
            };
            list.push(Sub { nid, data, sub });
            rows.push(Row { id: nid, cells });
        }
        for row in unread {
            let Some(&(data, sub)) = subs.get(&row.id) else { continue };
            list.push(Sub { nid: row.id, data: self.copy_data(&mut o.w, data, budget)?, sub: self.copy_subs(&mut o.w, sub, 1, budget)? });
            // Its row as the table had it, less what the table of a new file has no column for.
            let cells = row.cells.into_iter().filter_map(|c| cell_for(ATTACHMENTS, &c)).collect();
            rows.push(Row { id: row.id, cells });
        }
        if !rows.is_empty() {
            list.push(o.attachment_table(&rows)?);
        }
        Ok(list)
    }

    /// A stored attachment with what the model now says of it: its data and subnodes, and the
    /// cells of its row. Contents that are the same stay as they are stored; a file's new
    /// contents take the place of the old, and what else is stored on it stays.
    #[allow(clippy::too_many_arguments)]
    fn merged_attachment<W: Read + Write + Seek>(
        &mut self,
        o: &mut Out<W>,
        data: u64,
        sub: u64,
        old: &Attachment,
        new: &Attachment,
        index: usize,
        budget: &mut usize,
    ) -> Result<(u64, u64, Vec<Raw>), PstError> {
        let (mut props, mut kept) = self.open_pc(NbtEntry { data, sub, parent: 0 })?;
        let ids: HashSet<u16> = old
            .props
            .iter()
            .chain(new.props.iter())
            .map(|(id, _)| id)
            .filter(|id| *id < 0x8000 && !PLACED.contains(id) && !LTP_INTERNAL.contains(id))
            .collect();
        for id in ids {
            match (old.props.get(id), new.props.get(id)) {
                (was, Some(now)) if was != Some(now) => {
                    props.insert(id, encode(id, now));
                }
                (Some(_), None) => {
                    props.remove(&id);
                }
                _ => {}
            }
        }
        props.insert(pid::ATTACH_NUM, int(pid::ATTACH_NUM, i32::try_from(index).unwrap_or(i32::MAX)));
        let mut more = Vec::new();
        if old.embedded.is_none() && new.embedded.is_none() && old.data != new.data {
            // An object's bytes were in a subnode of their own: that goes with them.
            if let Some(nid) = props.remove(&pid::ATTACH_DATA).filter(|r| r.ty == ptype::OBJECT).and_then(|r| u32_at(&r.data, 0)) {
                kept.remove(&nid);
            }
            let size = u32::try_from(new.data.len()).unwrap_or(u32::MAX);
            let contents = if new.props.i32(pid::ATTACH_METHOD) == Some(6) && !new.data.is_empty() {
                // PtypObject: the subnode that holds the object, then its size ([MS-PST] 2.3.3.5).
                let used = kept.keys().filter(|n| *n & 0x1F == NID_TYPE_OBJECT).map(|n| n >> 5).max().unwrap_or(0);
                let mut subs = Subs::new(used.saturating_add(1));
                let most = o.w.max_data();
                let nid = subs.object(&mut o.w, new.data.chunks(most))?;
                more = subs.list;
                Raw { id: pid::ATTACH_DATA, ty: ptype::OBJECT, data: [nid.to_le_bytes(), size.to_le_bytes()].concat() }
            } else {
                Raw { id: pid::ATTACH_DATA, ty: ptype::BINARY, data: new.data.clone() }
            };
            props.insert(pid::ATTACH_DATA, contents);
        }
        // The size of the whole attachment as it is stored now (`Out::attachment`).
        let object = props.get(&pid::ATTACH_DATA).filter(|r| r.ty == ptype::OBJECT).and_then(|r| u32_at(&r.data, 4)).map_or(0, |n| n as usize);
        props.insert(pid::ATTACH_SIZE, int(pid::ATTACH_SIZE, 0));
        let mut cells = attachment_cells(&props);
        let (data, sub, size) = self.close_pc(o, props, &kept, more, Some(Late::Values { id: pid::ATTACH_SIZE, object }), budget)?;
        if let Some(cell) = cells.iter_mut().find(|c| c.id == pid::ATTACH_SIZE) {
            *cell = int(pid::ATTACH_SIZE, size);
        }
        Ok((data, sub, cells))
    }

    /// Write properties as a node's data and subnodes: `kept` are copied from the file as they
    /// are, `more` were written already.
    fn close_pc<W: Read + Write + Seek>(
        &mut self,
        o: &mut Out<W>,
        props: BTreeMap<u16, Raw>,
        kept: &Subnodes,
        more: Vec<Sub>,
        late: Option<Late>,
        budget: &mut usize,
    ) -> Result<(u64, u64, i32), PstError> {
        // Values too large for the heap get subnodes of their own, numbered past those kept.
        let used = kept.keys().copied().chain(more.iter().map(|s| s.nid)).filter(|n| n & 0x1F == TYPE_LTP).map(|n| n >> 5).max().unwrap_or(0);
        let mut subs = Subs::new(used.saturating_add(1));
        let props: Vec<Raw> = props.into_values().collect();
        // The subnodes that stay come first: a value that is an object names one of them, and
        // the property list is written knowing which are there.
        let mut kept: Vec<(u32, (u64, u64))> = kept.iter().map(|(n, e)| (*n, *e)).collect();
        kept.sort_unstable();
        for (nid, (data, sub)) in kept {
            subs.list.push(Sub { nid, data: self.copy_data(&mut o.w, data, budget)?, sub: self.copy_subs(&mut o.w, sub, 1, budget)? });
        }
        subs.list.extend(more);
        let (data, late) = write_pc_late(&mut o.w, &props, &mut subs, late)?;
        Ok((data, o.w.subnodes(subs.list)?, late))
    }

    /// A changed message: every stored property that was not changed stays as it is stored,
    /// and so do the recipients and the attachments when they are the same. Gives the node's
    /// data and subnodes, its short values, and the properties taken away.
    fn merged<W: Read + Write + Seek>(
        &mut self,
        o: &mut Out<W>,
        e: NbtEntry,
        new: &Item,
        budget: &mut usize,
    ) -> Result<(u64, u64, Vec<Raw>, Vec<u16>), PstError> {
        let old = self.message(e.data, e.sub, 0)?;
        let (mut props, mut kept) = self.open_pc(e)?;
        // Its size is what is stored of it once it is written ([MS-PST] 2.4.4.5.1 lists
        // PidTagMessageSize in each row): counted from here.
        let from = o.w.counted();
        let mut removed = Vec::new();
        let ids: HashSet<u16> =
            old.props.iter().chain(new.props.iter()).map(|(id, _)| id).filter(|id| *id < 0x8000 && !LTP_INTERNAL.contains(id)).collect();
        for id in ids {
            match (old.props.get(id), new.props.get(id)) {
                (was, Some(now)) if was != Some(now) => {
                    props.insert(id, encode(id, now));
                }
                (Some(_), None) => {
                    props.remove(&id);
                    removed.push(id);
                }
                _ => {}
            }
        }
        if old.categories != new.categories {
            let id = o.names.id_for(&NamedKey::keywords())?;
            if new.categories.is_empty() {
                props.remove(&id);
            } else {
                props.insert(id, encode(id, &PropValue::MultiString(new.categories.clone())));
            }
        }
        for (key, _) in &old.named {
            if !new.named.iter().any(|(k, _)| k == key)
                && let Some(id) = o.names.known(key)
            {
                props.remove(&id);
            }
        }
        for (key, now) in &new.named {
            if old.named.iter().find(|(k, _)| k == key).map(|(_, v)| v) != Some(now) {
                let id = o.names.id_for(key)?;
                props.insert(id, encode(id, now));
            }
        }
        let mut more = Vec::new();
        // The recipients and attachments are read in the message's code page.
        let outer = o.w.within(Some(code_page(props.values())));
        if old.recipients != new.recipients {
            kept.remove(&NID_RECIPIENT_TABLE);
            more.push(o.recipients(new)?);
        }
        if old.attachments != new.attachments {
            let list = self.changed_attachments(o, e, new, &mut kept, budget)?;
            // Those that could not be read are still there, and the message still has them.
            let has = !list.is_empty();
            more.extend(list);
            let flags = if has { new.flags() | MSGFLAG_HASATTACH } else { new.flags() & !MSGFLAG_HASATTACH };
            props.insert(pid::MESSAGE_FLAGS, int(pid::MESSAGE_FLAGS, flags));
            props.insert(pid::HASATTACH, flag(pid::HASATTACH, has));
        }
        o.w.within(outer);
        props.insert(pid::MESSAGE_SIZE, int(pid::MESSAGE_SIZE, 0));
        let mut cells: Vec<Raw> = props.values().filter(|r| r.data.len() <= MAX_CELL).map(copy).collect();
        let (data, sub, size) = self.close_pc(o, props, &kept, more, Some(Late::Stored { id: pid::MESSAGE_SIZE, from }), budget)?;
        if let Some(cell) = cells.iter_mut().find(|c| c.id == pid::MESSAGE_SIZE) {
            *cell = int(pid::MESSAGE_SIZE, size);
        }
        Ok((data, sub, cells, removed))
    }

    /// Put the changes made so far into the data file. Where it can be done they are
    /// written into the file where it is, with what is written over kept beside it until
    /// the save is checked and done (`save_onto.rs`). Otherwise a new file is written beside
    /// it, held against the structure check, read back and compared with what was meant, and
    /// only then put in the old one's place in one step. Whatever goes wrong, the data file
    /// stays as it was and the changes stay to be saved again.
    pub fn save(&mut self, seed: [u8; 16]) -> Result<Written, PstError> {
        if self.saving.is_some() {
            return Err(PstError::Write(BEING_SAVED.to_string()));
        }
        self.restore_torn()?;
        if !self.e_unsaved() {
            return Ok(Written::default());
        }
        let (path, tmp) = self.places()?;
        if let Some(done) = self.in_place(&path, seed)? {
            self.journal.finish();
            return Ok(done);
        }
        let done = self.prepare(&tmp, seed).and_then(|(fresh, done)| self.replace(&path, &tmp, fresh, done));
        match done {
            // The changes are in the file: the journal of them has done its work.
            Ok(_) => self.journal.finish(),
            Err(_) => {
                let _ = std::fs::remove_file(&tmp);
            }
        }
        done
    }

    /// The data file and the file a save writes beside it; an error when the data file is not
    /// a file, or is no longer the file that was opened.
    fn places(&self) -> Result<(PathBuf, PathBuf), PstError> {
        let path = self.path.clone().ok_or_else(|| PstError::Write("this data file is not kept in a file".to_string()))?;
        if stamp(&path) != self.stamp {
            return Err(PstError::Write("another program changed the data file since it was opened; nothing was saved".to_string()));
        }
        let name = path.file_name().ok_or_else(|| PstError::Write("the path has no file name".to_string()))?;
        let mut tmp_name = std::ffi::OsString::from(".");
        tmp_name.push(name);
        tmp_name.push(format!(".{}.tmp", std::process::id()));
        let tmp = path.with_file_name(tmp_name);
        Ok((path, tmp))
    }

    /// Begin a save whose writing is done elsewhere. The work opens the data file for itself
    /// and writes the mailbox as it stands now to a new file beside it, checks that file and
    /// reads it back; `e_save_end` then puts it in the data file's place. Between the two the
    /// store is read as before and takes no changes, so what is saved is what is shown.
    pub(crate) fn e_save_begin(&mut self, seed: [u8; 16]) -> Result<Option<openmailclient_model::SaveWork>, StoreError> {
        if self.saving.is_some() {
            return Err(StoreError::Format(BEING_SAVED.to_string()));
        }
        // A file a save was cut short in is put back here: it is quick, and what the work
        // then opens is the file as it is shown.
        self.restore_torn()?;
        let Some(edits) = self.edits.as_ref().filter(|e| e.dirty).cloned() else { return Ok(None) };
        let (path, tmp) = self.places()?;
        let was = self.stamp;
        let (keep, in_place) = (self.keep_before, self.in_place);
        self.saving = Some(Beside(tmp.clone()));
        Ok(Some(Box::new(move |progress: std::sync::Arc<openmailclient_model::Progress>| {
            let work = || -> Result<Saved, PstError> {
                let mut store = Self::open(&path)?;
                if store.stamp != was {
                    return Err(PstError::Write("another program changed the data file since it was opened; nothing was saved".to_string()));
                }
                store.edits = Some(edits);
                store.progress = Some(progress.clone());
                (store.keep_before, store.in_place) = (keep, in_place);
                if let Some(done) = store.in_place(&path, seed)? {
                    return Ok(Saved { done, in_place: true, stamp: store.stamp });
                }
                let done = store.prepare(&tmp, seed).map(|(_, done)| done)?;
                Ok(Saved { done, in_place: false, stamp: store.stamp })
            };
            let done = work();
            if done.is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
            Ok(Box::new(done?) as openmailclient_model::SaveDone)
        })))
    }

    /// End a save begun with `e_save_begin`: the new file its work wrote takes the data
    /// file's place. With an error, or when the data file is no longer what it was, nothing
    /// is replaced and the changes stay to be saved again.
    pub(crate) fn e_save_end(&mut self, done: Result<openmailclient_model::SaveDone, StoreError>) -> Result<(), StoreError> {
        let Some(mut tmp) = self.saving.take() else { return Ok(()) };
        let tmp = std::mem::take(&mut tmp.0);
        let ended = (|| -> Result<(), StoreError> {
            let saved = *done?.downcast::<Saved>().map_err(|_| StoreError::Format("the save gave nothing to end it with".to_string()))?;
            if saved.in_place {
                // The changes are in the data file: it is opened as it is now.
                let path = self.path.clone().ok_or_else(|| PstError::Write("this data file is not kept in a file".to_string()))?;
                if stamp(&path) != saved.stamp {
                    return Err(PstError::Write("another program changed the data file while it was saved".to_string()).into());
                }
                let fresh = Self::open(&path)?;
                self.ndb = fresh.ndb;
                self.finish_swap(fresh.name, fresh.names, fresh.root, fresh.special);
                self.onto_done += 1;
                return Ok(());
            }
            // A save that was tried in the file and undone left the file as it was, written anew.
            if saved.stamp.is_some() && saved.stamp == self.path.as_deref().and_then(stamp) {
                self.stamp = saved.stamp;
            }
            let (path, _) = self.places()?;
            let fresh = Self::open(&tmp)?;
            self.replace(&path, &tmp, fresh, saved.done)?;
            Ok(())
        })();
        match ended {
            Ok(()) => self.journal.finish(),
            Err(_) => {
                let _ = std::fs::remove_file(&tmp);
            }
        }
        ended
    }

    /// Write the mailbox as it stands to `tmp`, hold that file against the structure check,
    /// read it back and compare it with what was meant. Gives the new file, open.
    fn prepare(&mut self, tmp: &Path, seed: [u8; 16]) -> Result<(Self, Written), PstError> {
        // A file that lacks what Outlook asks for is written anew; where that cannot be done
        // it is saved as it is stored, as before.
        if self.renews() {
            match self.prepare_as(tmp, seed, true) {
                Ok(done) => return Ok(done),
                Err(e) if self.progress.as_ref().is_some_and(|p| p.stopped()) => return Err(e),
                Err(_) => {}
            }
        }
        self.prepare_as(tmp, seed, false)
    }

    /// `prepare`, the new file written by copying what is stored, or with `renew` by the
    /// writer of today from the mailbox as it is read (`renewed`).
    fn prepare_as(&mut self, tmp: &Path, seed: [u8; 16], renew: bool) -> Result<(Self, Written), PstError> {
        let file = std::fs::File::options().read(true).write(true).create(true).truncate(true).open(tmp)?;
        let (mut file, done) = if renew { self.renewed(file, seed)? } else { self.save_to(file, seed)? };
        file.sync_all()?;
        file.seek(SeekFrom::Start(0))?;
        // The check hears a stop as the writing does.
        let found = match crate::check_until(&mut std::io::BufReader::new(&mut file), self.progress.as_deref()) {
            Err(PstError::Stopped) => return Err(PstError::Write(STOPPED.to_string())),
            found => found?,
        };
        if let Some(problem) = found.problems.first() {
            return Err(PstError::Write(format!("the new file did not pass its own check ({problem}); nothing was replaced")));
        }
        drop(file);
        // Inside the nodes, what was wrong in the file as it was is not this save's doing:
        // such a node went across as stored. Anything else stops the save.
        if !found.inside.is_empty() {
            let was: HashSet<String> = match &self.path {
                Some(path) => {
                    crate::check(&mut std::io::BufReader::new(std::fs::File::open(path)?)).map(|c| c.inside.into_iter().collect()).unwrap_or_default()
                }
                None => HashSet::new(),
            };
            if let Some(problem) = found.inside.iter().find(|p| !was.contains(*p)) {
                return Err(PstError::Write(format!("the new file did not pass its own check ({problem}); nothing was replaced")));
            }
        }
        if let Some(p) = &self.progress {
            p.part(4);
        }
        let mut fresh = Self::open(tmp)?;
        if !self.reads_back(&mut fresh)? {
            return Err(PstError::Write("the new file does not read back as the mailbox that was saved; nothing was replaced".to_string()));
        }
        if let Some(p) = &self.progress {
            p.part(4);
        }
        if self.progress.as_ref().is_some_and(|p| p.stopped()) {
            return Err(PstError::Write(STOPPED.to_string()));
        }
        Ok((fresh, done))
    }

    /// Whether the file `fresh` reads back as the mailbox as it stands here: the folders
    /// and their counts are what was meant, and each folder that was looked into lists the
    /// messages it should.
    fn reads_back(&mut self, fresh: &mut Self) -> Result<bool, PstError> {
        let key = |f: &FolderInfo| (f.id, f.parent, f.name.clone(), f.total, f.unread);
        let mut want: Vec<_> = self.e_folders().map_err(|e| PstError::Source(e.to_string()))?.iter().map(key).collect();
        let mut got: Vec<_> = fresh.file_folders().map_err(|e| PstError::Source(e.to_string()))?.iter().map(key).collect();
        want.sort();
        got.sort();
        let mut same = want == got;
        for f in self.edits.iter().flat_map(|e| &e.folders).filter(|f| f.items.is_some()) {
            let listed: Vec<u64> = fresh.file_list(f.info.id).map(|l| l.iter().map(|s| s.id).collect()).unwrap_or_default();
            same &= f.items.iter().flatten().map(|e| u64::from(e.nid)).eq(listed);
        }
        Ok(same)
    }

    /// Put the new file, written and checked, in the data file's place in one step.
    fn replace(&mut self, path: &Path, tmp: &Path, fresh: Self, done: Written) -> Result<Written, PstError> {
        // Let go of the old file, then put the new one in its place.
        let old = std::mem::replace(&mut self.ndb, fresh.ndb);
        drop(old);
        // The first time a data file is replaced, the file as it was stays beside it under
        // another name: a second name for the same bytes, so it costs no copying.
        let keep = self.keep_before;
        let moved = free_to_replace(path).and_then(|()| {
            let kept = original_beside(path);
            if keep && !kept.exists() {
                let _ = std::fs::hard_link(path, &kept);
            }
            std::fs::rename(tmp, path)
        });
        if let Err(e) = moved {
            let back = std::fs::File::open(path).map_err(PstError::Io).and_then(|f| Ndb::open(Box::new(f)));
            return match back {
                Ok(ndb) => {
                    self.ndb = ndb;
                    Err(PstError::Write(format!("the data file could not be replaced ({e}); it is as it was, and the changes are still unsaved")))
                }
                // The old file cannot be read any more: the new one stays open under its own name.
                Err(_) => {
                    self.path = Some(tmp.to_path_buf());
                    self.finish_swap(fresh.name, fresh.names, fresh.root, fresh.special);
                    Ok(done)
                }
            };
        }
        self.finish_swap(fresh.name, fresh.names, fresh.root, fresh.special);
        Ok(done)
    }

    fn finish_swap(&mut self, name: String, names: Vec<Option<NamedKey>>, root: u32, special: HashMap<u32, FolderKind>) {
        if !name.is_empty() {
            self.name = name;
        }
        (self.names, self.root, self.special) = (names, root, special);
        self.edits = None;
        self.note_stamp();
    }
}

/// Where the file as it was before its first save here is kept: beside it.
pub fn original_beside(path: &Path) -> PathBuf {
    let mut name = path.file_stem().map(std::ffi::OsString::from).unwrap_or_default();
    name.push(".before-openmailclient.pst");
    path.with_file_name(name)
}

/// An error when another program has the file open: its data file is not replaced under it.
#[cfg(windows)]
fn free_to_replace(path: &Path) -> std::io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    // Opening for writing and sharing with no one succeeds only when nobody else has it.
    std::fs::File::options().write(true).share_mode(0).open(path).map(|_| ())
}

#[cfg(not(windows))]
fn free_to_replace(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// What a folder's own node and its row in its parent's table say of it ([MS-PST] 2.4.4.1.1).
fn folder_cells(f: &EFolder) -> Vec<Raw> {
    let mut out = vec![text(pid::DISPLAY_NAME, &f.info.name), flag(pid::SUBFOLDERS, f.info.has_children)];
    if f.items.is_some() {
        out.push(int(pid::CONTENT_COUNT, count(u64::from(f.info.total))));
        out.push(int(pid::CONTENT_UNREAD_COUNT, count(u64::from(f.info.unread))));
    }
    if f.new && !f.info.container_class.is_empty() {
        out.push(text(pid::CONTAINER_CLASS, &f.info.container_class));
    }
    out
}

impl PstStore {
    /// The data file as it stands, unsaved changes with it, written node for node to a new
    /// file: what is stored and the model does not hold goes with it. The new file keeps the
    /// id of the one it was copied from, as a copy made of the file itself would.
    pub(crate) fn e_copy_to(&mut self, to: &Path, seed: [u8; 16]) -> Result<Option<openmailclient_model::Copied>, StoreError> {
        // An `.ost` is not copied as it is stored: Outlook refuses a `.pst` made of an offline
        // store's nodes (seen 2026-10-14). It is written from what is read, which carries its
        // hidden items and folder properties all the same (`write_store_until`).
        if !self.can_edit() {
            return Ok(None);
        }
        let listed = self.e_folders()?;
        let open = || std::fs::File::options().read(true).write(true).create(true).truncate(true).open(to).map_err(PstError::Io);
        // A copy of a file that lacks what Outlook asks for is written anew, as a save is.
        let renewed = match self.renews() {
            true => match self.renewed(open()?, seed) {
                Err(e) if self.progress.as_ref().is_some_and(|p| p.stopped()) => return Err(e.into()),
                done => done.ok(),
            },
            false => None,
        };
        let (file, _) = match renewed {
            Some(done) => done,
            None => self.save_to(open()?, seed)?,
        };
        file.sync_all().map_err(PstError::Io)?;
        let items = listed.iter().fold(0u64, |n, f| n.saturating_add(u64::from(f.total)));
        Ok(Some(openmailclient_model::Copied { folders: listed.len() as u64, items }))
    }

    /// True when the data file lacks nodes Outlook asks for beyond [MS-PST] 2.7.1 (see
    /// `store_write`), as those this program wrote before M3c.1 do, and all it holds is in
    /// what is read of it or carried beside it (hidden items and folder properties): no
    /// folder that is not listed but those every file has. Such a file is written whole by
    /// the writer of today when it is saved or copied, every folder and item keeping its id;
    /// any other is copied as it is stored.
    pub(crate) fn renews(&mut self) -> bool {
        if !self.can_edit() || OUTLOOK_ASKS.iter().all(|nid| self.ndb.nbt.contains_key(nid)) {
            return false;
        }
        let Ok(listed) = self.file_folders() else { return false };
        let listed: HashSet<u32> = listed.iter().filter_map(|f| nid_of(f.id.0)).chain(EVERY_FILE).collect();
        let nodes: Vec<u32> = self.ndb.nbt.keys().copied().collect();
        // The hidden items the listed folders' tables name: those are carried.
        let mut named: HashSet<u32> = HashSet::new();
        for folder in &listed {
            if let Some((_, rows)) = self.table_of(folder & !0x1F | TYPE_ASSOC) {
                named.extend(rows.iter().map(|r| r.id));
            }
        }
        nodes.into_iter().all(|nid| match nid & 0x1F {
            TYPE_FOLDER | TYPE_SEARCH_FOLDER => listed.contains(&nid),
            // A hidden item no listed folder's table names would be lost.
            TYPE_ASSOC_MESSAGE => named.contains(&nid),
            _ => true,
        })
    }

    /// The mailbox written to `out` by the writer of today, as a file of the kind this one
    /// is: every folder and item keeps its id, and the file goes on from the header's
    /// counters. An error when an item cannot be read or an id cannot be kept.
    fn renewed<W: Read + Write + Seek>(&mut self, out: W, seed: [u8; 16]) -> Result<(W, Written), PstError> {
        let progress = self.progress.clone();
        let (ver, counters) = (self.ndb.ver, self.ndb.counters);
        // The file keeps the id it has, as a copy of it would.
        let uid = openmailclient_model::Store::record_key(self).unwrap_or(seed);
        let (out, done) = match crate::store_write::write_store_keeping(out, self, uid, progress.as_deref(), ver, counters) {
            Err(PstError::Stopped) => return Err(PstError::Write(STOPPED.to_string())),
            done => done?,
        };
        if done.unreadable > 0 {
            return Err(PstError::Write("an item could not be read, and the file is not written anew".to_string()));
        }
        Ok((out, done))
    }

    /// The mailbox for another thread: a store of its own on the same file, with the
    /// changes as they are now. A data file of any kind can be had so; one that is in no
    /// file cannot.
    pub(crate) fn e_apart(&mut self) -> Result<Option<openmailclient_model::Apart>, StoreError> {
        let Some(path) = self.path.clone() else { return Ok(None) };
        let (edits, was) = (self.edits.clone(), self.stamp);
        Ok(Some(Box::new(move |progress: std::sync::Arc<openmailclient_model::Progress>| {
            let mut store = Self::open(&path)?;
            if store.stamp != was {
                return Err(StoreError::Format("another program changed the data file since it was opened; nothing was copied".to_string()));
            }
            store.edits = edits;
            store.progress = Some(progress);
            Ok(Box::new(store) as Box<dyn openmailclient_model::Store>)
        })))
    }

    pub(crate) fn e_save(&mut self, seed: [u8; 16]) -> Result<(), StoreError> {
        self.save(seed).map(|_| ()).map_err(StoreError::from)
    }
}
