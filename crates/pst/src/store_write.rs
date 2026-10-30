//! The messaging layer of a `.pst`, written ([MS-PST] 2.4): a mailbox read through the `Store`
//! trait goes out as the message store, the named property map, the folders with their three
//! tables, the messages with their recipients and attachments, and the nodes every data file
//! must have ([MS-PST] 2.7.1).
//!
//! Items are read and written one at a time, so the mailbox never sits in memory whole.

use crate::PstError;
use crate::ltp::{Raw, Row};
use crate::ltp_write::{Late, Subs, encode, write_pc, write_pc_late, write_tc, write_tc_as};
use crate::write::{NdbWriter, Sub, crc};
use openmailclient_model::{
    Attachment, FolderId, FolderKind, Item, ItemId, MSGFLAG_HASATTACH, MSGFLAG_READ, NameId, NamedKey, PropBag, PropValue, Store, codepage, pid,
    pset, ptype,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Seek, Write};

/// Well-known nodes ([MS-PST] 2.4.1, 2.7.1).
const NID_MESSAGE_STORE: u32 = 0x21;
const NID_NAME_TO_ID_MAP: u32 = 0x61;
const NID_ROOT_FOLDER: u32 = 0x122;
use crate::search_queue::NID_SEARCH_MANAGEMENT_QUEUE;
const NID_SEARCH_ACTIVITY_LIST: u32 = 0x201;
const NID_HIERARCHY_TEMPLATE: u32 = 0x60D;
const NID_CONTENTS_TEMPLATE: u32 = 0x60E;
const NID_ASSOC_TEMPLATE: u32 = 0x60F;
const NID_SEARCH_CONTENTS_TEMPLATE: u32 = 0x610;
const NID_ATTACHMENT_TABLE: u32 = 0x671;
const NID_RECIPIENT_TABLE: u32 = 0x692;
const NID_SPAM_SEARCH_FOLDER: u32 = 0x2223;
/// Nodes the list of what a file must hold ([MS-PST] 2.7.1) leaves out and Outlook asks for
/// all the same: its repair tool names each as missing, and Outlook does not load a file
/// that lacks them (seen 2026-10-13, `docs/file-format-parity.md`). The first two are the
/// receive folder table and the outgoing queue, node types of [MS-PST] 2.2.2.1; the three
/// after them are tables new ones are made from.
const NID_RECEIVE_FOLDERS: u32 = 0x62B;
const NID_OUTGOING_QUEUE: u32 = 0x64C;
const NID_TEMPLATE_16: u32 = 0x6B6;
const NID_TEMPLATE_17: u32 = 0x6D7;
/// The update queue of the search folders, an empty node.
const NID_SEARCH_QUEUE: u32 = 0xEC1;
const NID_TEMPLATE_18: u32 = 0x6F8;
/// What a search folder has beside its property list ([MS-PST] 2.4.8.6): the queue of what
/// changed, what it searches for, and its contents table.
const TYPE_SEARCH_UPDATE_QUEUE: u32 = 0x06;
const TYPE_SEARCH_CRITERIA: u32 = 0x07;
const TYPE_SEARCH_CONTENTS: u32 = 0x10;
const NID_IPM_SUBTREE: u32 = 0x8022;
const NID_SEARCH_ROOT: u32 = 0x8042;
const NID_DELETED_ITEMS: u32 = 0x8062;

/// Node types ([MS-PST] 2.2.2.1).
const TYPE_FOLDER: u32 = 0x02;
const TYPE_MESSAGE: u32 = 0x04;
const TYPE_ATTACHMENT: u32 = 0x05;
const TYPE_ASSOC_MESSAGE: u32 = 0x08;
const TYPE_HIERARCHY: u32 = 0x0D;
const TYPE_CONTENTS: u32 = 0x0E;
const TYPE_ASSOC: u32 = 0x0F;

/// The first index of a folder of ours (0x401 to 0x403 are the three fixed ones) and of a message.
const FIRST_FOLDER: u32 = 0x404;
const FIRST_MESSAGE: u32 = 0x1_0000;
/// A node id keeps 27 bits for the index.
pub(crate) const MAX_INDEX: u32 = (1 << 27) - 1;
/// The node an attached message is kept in, among its attachment's subnodes.
const NID_EMBEDDED: u32 = FIRST_MESSAGE << 5 | TYPE_MESSAGE;
/// Messages attached to messages are written this deep; the model never holds more.
const MAX_DEPTH: usize = 32;
/// A value longer than this is not put in a row of a folder's table.
pub(crate) const MAX_CELL: usize = 1024;
/// PidTagNameidBucketCount SHOULD be 251 ([MS-PST] 2.4.7.5).
const BUCKETS: u32 = 251;

const I: u16 = ptype::INT32;
const L: u16 = ptype::INT64;
const S: u16 = ptype::STRING;
const B: u16 = ptype::BOOL;
const T: u16 = ptype::TIME;
const X: u16 = ptype::BINARY;

/// The columns every hierarchy table has ([MS-PST] 2.4.4.4.1). PidTagContainerClass is a
/// string here, as in the files Outlook writes. The list says different versions of Outlook
/// add columns: this one asks for 0x0E30 as a binary too, here and in contents tables, for
/// 0x3013 in contents tables and for 0x682F in tables of associated items (seen in what its
/// repair tool calls missing).
pub(crate) const HIERARCHY: &[(u16, u16)] = &[
    (0x0E30, I),
    (0x0E30, X),
    (0x0E33, L),
    (0x0E34, X),
    (0x0E38, I),
    (0x3001, S),
    (0x3602, I),
    (0x3603, I),
    (0x360A, B),
    (0x3613, S),
    (0x6635, I),
    (0x6636, I),
];
/// The columns every contents table has ([MS-PST] 2.4.4.5.1).
pub(crate) const CONTENTS: &[(u16, u16)] = &[
    (0x0017, I),
    (0x001A, S),
    (0x0036, I),
    (0x0037, S),
    (0x0039, T),
    (0x0042, S),
    (0x0057, B),
    (0x0058, B),
    (0x0070, S),
    (0x0071, X),
    (0x0E03, S),
    (0x0E04, S),
    (0x0E06, T),
    (0x0E07, I),
    (0x0E08, I),
    (0x0E17, I),
    (0x0E30, I),
    (0x0E30, X),
    (0x0E33, L),
    (0x0E34, X),
    (0x0E38, I),
    (0x0E3C, X),
    (0x0E3D, X),
    (0x1097, I),
    (0x3008, T),
    (0x3013, X),
    (0x65C6, I),
];
/// Columns our own contents tables add, so a message list shows who a message is from and
/// its follow-up flag, when a held message goes and when one expires, without each message
/// being opened.
pub(crate) const CONTENTS_MORE: &[(u16, u16)] =
    &[(pid::SENDER_NAME, S), (pid::FLAG_STATUS, I), (pid::DEFERRED_DELIVERY_TIME, T), (pid::EXPIRY_TIME, T)];
/// The columns every table of folder-associated items has ([MS-PST] 2.4.4.6.1).
pub(crate) const ASSOC: &[(u16, u16)] = &[
    (0x001A, S),
    (0x0E07, I),
    (0x0E17, I),
    (0x3001, S),
    (0x6800, S),
    (0x6803, B),
    (0x6805, ptype::MULTI_INT32),
    (0x682F, S),
    (0x7003, I),
    (0x7004, X),
    (0x7005, X),
    (0x7006, S),
    (0x7007, I),
];
/// The columns of a search folder's contents table ([MS-PST] 2.4.8.6.2.1).
const SEARCH_CONTENTS: &[(u16, u16)] = &[
    (0x0017, I),
    (0x001A, S),
    (0x0036, I),
    (0x0037, S),
    (0x0042, S),
    (0x0057, B),
    (0x0058, B),
    (0x0E03, S),
    (0x0E04, S),
    (0x0E05, S),
    (0x0E06, T),
    (0x0E07, I),
    (0x0E08, I),
    (0x0E17, I),
    (0x0E2A, B),
    (0x3008, T),
    (0x67F1, I),
];
/// The columns every recipient table has ([MS-PST] 2.4.5.3.1).
const RECIPIENTS: &[(u16, u16)] = &[
    (0x0C15, I),
    (0x0E0F, B),
    (0x0FF9, X),
    (0x0FFE, I),
    (0x0FFF, X),
    (0x3001, S),
    (0x3002, S),
    (0x3003, S),
    (0x300B, X),
    (0x3900, I),
    (0x39FF, S),
    (0x3A40, B),
];
/// The columns of the tables Outlook asks for beyond [MS-PST] 2.7.1, as its repair tool
/// makes them: the receive folder table (a message class and the folder its mail goes to),
/// the outgoing queue, and three templates.
const RECEIVE_FOLDERS: &[(u16, u16)] = &[(0x001A, S), (0x6605, I)];
const OUTGOING_QUEUE: &[(u16, u16)] = &[(0x0039, T), (0x0E10, I), (0x0E14, I)];
const TEMPLATE_16: &[(u16, u16)] = &[(0x0E33, L), (0x0E37, X), (0x0E38, I)];
const TEMPLATE_17: &[(u16, u16)] = &[(0x001A, S), (0x0E30, X), (0x0E31, X), (0x0E33, L), (0x0E34, X), (0x0E38, I), (0x0E3E, X)];
const TEMPLATE_18: &[(u16, u16)] = &[(0x0E33, L), (0x3007, T)];
/// The columns every attachment table has ([MS-PST] 2.4.6.1.1).
pub(crate) const ATTACHMENTS: &[(u16, u16)] = &[(0x0E20, I), (0x3704, S), (0x3705, I), (0x370B, I)];

/// Bookkeeping columns of a table (PidTagLtpRowId and neighbours): the table writer's own.
const LTP_INTERNAL: std::ops::RangeInclusive<u16> = 0x67F1..=0x67F3;

pub(crate) fn cannot(what: &str) -> PstError {
    PstError::Write(format!("a .pst cannot hold {what}"))
}

/// What went into a data file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Written {
    pub folders: u64,
    pub items: u64,
    /// Items and folder lists the source could not give; they are not in the file.
    pub unreadable: u64,
    /// Hidden items carried from the source, beside `items`.
    pub hidden: u64,
}

/// True when a value may sit in a table that has `columns`: a column of its id has its type.
fn fits(columns: &[(u16, u16)], raw: &Raw) -> bool {
    columns.iter().all(|(id, ty)| *id != raw.id || *ty == raw.ty)
}

pub(crate) fn copy(raw: &Raw) -> Raw {
    Raw { id: raw.id, ty: raw.ty, data: raw.data.clone() }
}

/// A value as a column of another type holds it, when nothing is lost by that: a whole
/// number in a wider one or in a narrower one that has room for it, a 32-bit float as a
/// 64-bit one (and back when it comes back the same), text of 7-bit characters as the
/// other kind of string. A column has one type ([MS-PST] 2.3.4.2), and a list is sorted
/// and shown by it.
pub(crate) fn as_type(r: &Raw, ty: u16) -> Option<Raw> {
    let whole = |r: &Raw| -> Option<i64> {
        Some(match r.ty {
            ptype::INT16 => i64::from(i16::from_le_bytes(r.data.get(..2)?.try_into().ok()?)),
            ptype::INT32 => i64::from(i32::from_le_bytes(r.data.get(..4)?.try_into().ok()?)),
            ptype::INT64 => i64::from_le_bytes(r.data.get(..8)?.try_into().ok()?),
            _ => return None,
        })
    };
    let data = match (r.ty, ty) {
        (ptype::INT16 | ptype::INT32 | ptype::INT64, ptype::INT16) => i16::try_from(whole(r)?).ok()?.to_le_bytes().to_vec(),
        (ptype::INT16 | ptype::INT32 | ptype::INT64, ptype::INT32) => i32::try_from(whole(r)?).ok()?.to_le_bytes().to_vec(),
        (ptype::INT16 | ptype::INT32 | ptype::INT64, ptype::INT64) => whole(r)?.to_le_bytes().to_vec(),
        (ptype::FLOAT32, ptype::FLOAT64) => f64::from(f32::from_le_bytes(r.data.get(..4)?.try_into().ok()?)).to_le_bytes().to_vec(),
        (ptype::FLOAT64, ptype::FLOAT32) => {
            let wide = f64::from_le_bytes(r.data.get(..8)?.try_into().ok()?);
            let narrow = wide as f32;
            if f64::from(narrow).to_bits() != wide.to_bits() {
                return None;
            }
            narrow.to_le_bytes().to_vec()
        }
        (ptype::STRING8, ptype::STRING) if r.data.is_ascii() => r.data.iter().flat_map(|b| [*b, 0]).collect(),
        (ptype::STRING, ptype::STRING8) if r.data.len().is_multiple_of(2) && r.data.chunks_exact(2).all(|c| matches!(c, [low, 0] if *low < 0x80)) => {
            r.data.iter().step_by(2).copied().collect()
        }
        _ => return None,
    };
    Some(Raw { id: r.id, ty, data })
}

/// The cell a value gets in a table with these columns: itself under a column of its
/// type, else what `as_type` makes of it for a column of its id. `None` when the table
/// has no column that holds it; the item keeps the value all the same.
pub(crate) fn cell_for(columns: &[(u16, u16)], r: &Raw) -> Option<Raw> {
    if columns.contains(&(r.id, r.ty)) {
        return Some(copy(r)).filter(|c| c.data.len() <= MAX_CELL);
    }
    columns
        .iter()
        .filter(|(id, _)| *id == r.id)
        .find_map(|(_, ty)| as_type(r, *ty))
        // Unicode text that is not 7-bit goes under a column of 8-bit text as it is: the
        // table is then written with that column as Unicode text, or, in an ANSI file, as
        // 8-bit text when all of it can be (`ltp_write::write_tc`).
        .or_else(|| (r.ty == ptype::STRING && columns.contains(&(r.id, ptype::STRING8))).then(|| copy(r)))
        .filter(|c| c.data.len() <= MAX_CELL)
}

pub(crate) fn int(id: u16, v: i32) -> Raw {
    encode(id, &PropValue::Int32(v))
}

pub(crate) fn text(id: u16, s: &str) -> Raw {
    encode(id, &PropValue::String(s.to_string()))
}

pub(crate) fn flag(id: u16, v: bool) -> Raw {
    encode(id, &PropValue::Bool(v))
}

pub(crate) fn count(n: u64) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// A property bag as stored values: named ids of another store and table bookkeeping left out.
fn raws(bag: &PropBag) -> BTreeMap<u16, Raw> {
    bag.iter().filter(|(id, _)| *id < 0x8000 && !LTP_INTERNAL.contains(id)).map(|(id, v)| (id, encode(id, v))).collect()
}

/// Named properties get ids 0x8000, 0x8001, … in order of first use ([MS-PST] 2.4.7).
#[derive(Default)]
pub(crate) struct Names {
    /// How many ids the data file had given out before: new names come after them.
    base: usize,
    /// The names that are new here.
    keys: Vec<NamedKey>,
    ids: BTreeMap<NamedKey, u16>,
}

impl Names {
    /// The names a data file already has, by their place: the id of each is 0x8000 and that.
    pub(crate) fn seeded(known: &[Option<NamedKey>]) -> Self {
        let mut names = Self { base: known.len(), ..Self::default() };
        for (i, key) in known.iter().enumerate().take(0x7FFE) {
            if let Some(key) = key {
                names.ids.entry(key.clone()).or_insert(0x8000 + i as u16);
            }
        }
        names
    }

    /// The id a name has, when it has one.
    pub(crate) fn known(&self, key: &NamedKey) -> Option<u16> {
        self.ids.get(key).copied()
    }

    /// True when names were added to those the data file had.
    pub(crate) fn grew(&self) -> bool {
        !self.keys.is_empty()
    }

    pub(crate) fn id_for(&mut self, key: &NamedKey) -> Result<u16, PstError> {
        if let Some(id) = self.ids.get(key) {
            return Ok(*id);
        }
        let id = u16::try_from(self.base.saturating_add(self.keys.len()))
            .ok()
            .filter(|n| *n < 0x7FFE)
            .map(|n| 0x8000 + n)
            .ok_or_else(|| cannot("that many named properties"))?;
        self.keys.push(key.clone());
        self.ids.insert(key.clone(), id);
        Ok(id)
    }

    /// The property list of the name-to-id map: the bucket count, the GUID, entry and string
    /// streams, and the hash buckets ([MS-PST] 2.4.7.1 to 2.4.7.5).
    fn props(&self) -> Vec<Raw> {
        let mut out = Vec::new();
        self.add_to(&mut out);
        out
    }

    /// Add the new names to the property list of a name-to-id map: to the end of its streams
    /// and to the buckets they fall in. What is there stays as it is.
    pub(crate) fn add_to(&self, map: &mut Vec<Raw>) {
        let mut take = |id: u16| map.iter().position(|r| r.id == id).map(|at| map.swap_remove(at).data).unwrap_or_default();
        let count = take(0x0001).first_chunk::<4>().map_or(0, |b| u32::from_le_bytes(*b));
        let count = if count == 0 { BUCKETS } else { count };
        let mut guids: Vec<[u8; 16]> = take(0x0002).chunks_exact(16).filter_map(|g| <[u8; 16]>::try_from(g).ok()).collect();
        let (mut entries, mut strings) = (take(0x0003), take(0x0004));
        let mut buckets: BTreeMap<u16, Vec<u8>> = BTreeMap::new();
        for (index, key) in self.keys.iter().enumerate() {
            let index = self.base + index;
            // wGuid: 1 and 2 are the two well-known sets, 3 and up an index into the GUID stream.
            let guid_index: u32 = if key.guid == pset::PS_MAPI {
                1
            } else if key.guid == pset::PS_PUBLIC_STRINGS {
                2
            } else {
                let at = guids.iter().position(|g| *g == key.guid).unwrap_or_else(|| {
                    guids.push(key.guid);
                    guids.len() - 1
                });
                u32::try_from(at).unwrap_or(0).saturating_add(3)
            };
            // A numbered name is its number; a string name is where it starts in the string
            // stream, and in its bucket the checksum of the string.
            let (first, kind, hashed) = match &key.name {
                NameId::Id(n) => (*n, 0u32, *n),
                NameId::Name(s) => {
                    let utf16 = codepage::to_utf16le(s);
                    let offset = u32::try_from(strings.len()).unwrap_or(u32::MAX);
                    strings.extend(u32::try_from(utf16.len()).unwrap_or(0).to_le_bytes());
                    strings.extend(&utf16);
                    strings.resize(strings.len().next_multiple_of(4), 0);
                    (offset, 1u32, crc(&utf16))
                }
            };
            let guid_kind = (guid_index << 1 | kind) as u16;
            let tail: Vec<u8> = guid_kind.to_le_bytes().into_iter().chain((index as u16).to_le_bytes()).collect();
            entries.extend(first.to_le_bytes());
            entries.extend(&tail);
            // The bucket: the first four bytes of the record XOR the next two, modulo the count.
            let bucket = 0x1000u16.saturating_add((hashed ^ u32::from(guid_kind)).checked_rem(count).unwrap_or(0) as u16);
            let record = buckets.entry(bucket).or_default();
            record.extend(hashed.to_le_bytes());
            record.extend(&tail);
        }
        for (id, more) in buckets {
            let mut bytes = map.iter().position(|r| r.id == id).map(|at| map.swap_remove(at).data).unwrap_or_default();
            bytes.extend(more);
            map.push(Raw { id, ty: X, data: bytes });
        }
        map.push(int(0x0001, count as i32));
        for (id, bytes) in [(0x0002, guids.concat()), (0x0003, entries), (0x0004, strings)] {
            map.push(Raw { id, ty: X, data: bytes });
        }
    }
}

/// One folder on its way out.
struct Folder {
    nid: u32,
    parent: u32,
    name: String,
    class: String,
    children: Vec<usize>,
    total: u64,
    unread: u64,
    /// The folder's own properties the source keeps, carried into the new file.
    kept: BTreeMap<u16, Raw>,
    /// Its hidden items' rows in its table of associated items.
    hidden: Vec<Row>,
}

/// Properties of a folder the writer gives itself, which a folder's carried properties do
/// not stand in for: what its row in the hierarchy table says, and the ids naming the other
/// modules' folders, which are made anew.
fn written_here(id: u16) -> bool {
    matches!(id, pid::DISPLAY_NAME | pid::CONTENT_COUNT | pid::CONTENT_UNREAD_COUNT | pid::SUBFOLDERS | pid::CONTAINER_CLASS)
        || SPECIAL.iter().any(|(s, _)| *s == id)
        || LTP_INTERNAL.contains(&id)
}

/// Entry ids of the source's folders made to name the same folders in the new file: an
/// entry id of this store is four bytes of flags, the store's id and the node
/// ([MS-PST] 2.4.3.2), so one that names a folder of the source by the source's id is given
/// the new file's id and the folder's node there.
struct Renamed {
    from: Option<[u8; 16]>,
    to: [u8; 16],
    nids: HashMap<u32, u32>,
}

impl Renamed {
    fn bytes(&self, b: &mut [u8]) {
        let Some(from) = self.from else { return };
        if b.len() != 24 || b.get(..4) != Some(&[0u8; 4][..]) || b.get(4..20) != Some(&from[..]) {
            return;
        }
        let Some(n) = crate::ndb::u32_at(b, 20).and_then(|nid| self.nids.get(&nid)).copied() else { return };
        if let Some(uid) = b.get_mut(4..20) {
            uid.copy_from_slice(&self.to);
        }
        if let Some(place) = b.get_mut(20..24) {
            place.copy_from_slice(&n.to_le_bytes());
        }
    }

    fn value(&self, v: &mut PropValue) {
        match v {
            PropValue::Binary(b) => self.bytes(b),
            PropValue::MultiBinary(list) => list.iter_mut().for_each(|b| self.bytes(b)),
            _ => {}
        }
    }

    fn bag(&self, bag: &mut PropBag) {
        bag.0.values_mut().for_each(|v| self.value(v));
    }
}

impl Folder {
    /// The properties a folder must have ([MS-PST] 2.4.4.1.1), which are also its row in its
    /// parent's hierarchy table.
    fn props(&self, subfolders: bool) -> Vec<Raw> {
        let mut out =
            vec![text(pid::DISPLAY_NAME, &self.name), int(pid::CONTENT_COUNT, count(self.total)), int(pid::CONTENT_UNREAD_COUNT, count(self.unread))];
        out.push(flag(pid::SUBFOLDERS, subfolders));
        if !self.class.is_empty() {
            out.push(text(pid::CONTAINER_CLASS, &self.class));
        }
        out
    }

    /// Its row in its parent's hierarchy table: what `props` gives, and those of its carried
    /// properties the table has columns for. Outlook's repair tool takes a row that does not
    /// show what the folder holds under a column for a damaged one (seen 2026-10-14).
    fn row(&self, subfolders: bool) -> Row {
        let mut cells = self.props(subfolders);
        cells.extend(self.kept.values().filter_map(|r| cell_for(HIERARCHY, r)));
        Row { id: self.nid, cells }
    }
}

/// The ids under which the root and the Inbox name the folders of the other modules and
/// the drafts ([MS-OXOSFLD] 2.2.3: PidTagIpmAppointmentEntryId, PidTagIpmContactEntryId,
/// PidTagIpmJournalEntryId, PidTagIpmNoteEntryId, PidTagIpmTaskEntryId,
/// PidTagIpmDraftsEntryId).
pub(crate) const SPECIAL: &[(u16, FolderKind)] = &[
    (0x36D0, FolderKind::Calendar),
    (0x36D1, FolderKind::Contacts),
    (0x36D2, FolderKind::Journal),
    (0x36D3, FolderKind::Notes),
    (0x36D4, FolderKind::Tasks),
    (0x36D7, FolderKind::Drafts),
];

pub(crate) struct Out<W: Read + Write + Seek> {
    pub(crate) w: NdbWriter<W>,
    pub(crate) names: Names,
    pub(crate) uid: [u8; 16],
    pub(crate) serial: u64,
}

impl<W: Read + Write + Seek> Out<W> {
    /// A top-level node holding a property list.
    pub(crate) fn pc_node(&mut self, nid: u32, parent: u32, props: &[Raw]) -> Result<(), PstError> {
        let mut subs = Subs::new(1);
        let data = write_pc(&mut self.w, props, &mut subs)?;
        let sub = self.w.subnodes(subs.list)?;
        self.w.node(nid, parent, data, sub);
        Ok(())
    }

    /// A table as a node's data and subnodes.
    fn table(&mut self, nid: u32, columns: &[(u16, u16)], rows: &[Row]) -> Result<(u64, u64), PstError> {
        let mut subs = Subs::new(1);
        let data = write_tc(&mut self.w, columns, rows, &mut subs)?;
        contents_extra(&mut self.w, nid, &mut subs)?;
        Ok((data, self.w.subnodes(subs.list)?))
    }

    /// A top-level node holding a table. Tables have no parent.
    pub(crate) fn tc_node(&mut self, nid: u32, columns: &[(u16, u16)], rows: &[Row]) -> Result<(), PstError> {
        let (data, sub) = self.table(nid, columns, rows)?;
        self.w.node(nid, 0, data, sub);
        Ok(())
    }

    /// A folder's node and its three tables ([MS-PST] 2.4.4). `rows` are its contents table.
    fn folder(&mut self, folders: &[Folder], f: &Folder, contents: Option<&[Row]>, more: &[Raw]) -> Result<(), PstError> {
        let kids: Vec<&Folder> = f.children.iter().filter_map(|i| folders.get(*i)).collect();
        let mut props: BTreeMap<u16, Raw> = f.kept.values().map(|r| (r.id, copy(r))).collect();
        props.extend(f.props(!kids.is_empty()).into_iter().chain(more.iter().map(copy)).map(|r| (r.id, r)));
        let props: Vec<Raw> = props.into_values().collect();
        self.pc_node(f.nid, f.parent, &props)?;
        let rows: Vec<Row> = kids.iter().map(|k| k.row(!k.children.is_empty())).collect();
        self.tc_node(f.nid & !0x1F | TYPE_HIERARCHY, HIERARCHY, &rows)?;
        if let Some(rows) = contents {
            self.contents(f.nid, rows)?;
        }
        self.tc_node(f.nid & !0x1F | TYPE_ASSOC, ASSOC, &f.hidden)
    }

    fn contents(&mut self, folder: u32, rows: &[Row]) -> Result<(), PstError> {
        let more = if rows.is_empty() { &[] } else { CONTENTS_MORE };
        let columns: Vec<(u16, u16)> = CONTENTS.iter().chain(more).copied().collect();
        self.tc_node(folder & !0x1F | TYPE_CONTENTS, &columns, rows)
    }

    /// An EntryID of this store: four bytes of flags, the store's id, the node ([MS-PST] 2.4.3.2).
    fn entry_id(&self, id: u16, nid: u32) -> Raw {
        let mut data = vec![0u8; 4];
        data.extend(self.uid);
        data.extend(nid.to_le_bytes());
        Raw { id, ty: X, data }
    }

    /// A message as a node's data and subnodes ([MS-PST] 2.4.5), and the cells of its row in a
    /// contents table.
    pub(crate) fn message(&mut self, item: &Item, depth: usize) -> Result<(u64, u64, Vec<Raw>), PstError> {
        if depth > MAX_DEPTH {
            return Err(cannot("messages attached to messages that deep"));
        }
        let from = self.w.counted();
        let mut props = raws(&item.props);
        // What a message must have ([MS-PST] 2.4.5.1.1), where the item does not say.
        self.serial = self.serial.wrapping_add(1);
        let mut key = self.uid;
        for (to, from) in key.iter_mut().zip(self.serial.to_le_bytes()) {
            *to ^= from;
        }
        let time = item.time();
        for raw in [
            text(pid::MESSAGE_CLASS, "IPM.Note"),
            int(0x0E17, 0),
            encode(pid::CREATION_TIME, &PropValue::Time(time)),
            encode(pid::LAST_MODIFICATION_TIME, &PropValue::Time(time)),
            Raw { id: 0x300B, ty: X, data: key.to_vec() },
        ] {
            props.entry(raw.id).or_insert(raw);
        }
        let flags = if item.attachments.is_empty() { item.flags() & !MSGFLAG_HASATTACH } else { item.flags() | MSGFLAG_HASATTACH };
        props.insert(pid::MESSAGE_FLAGS, int(pid::MESSAGE_FLAGS, flags));
        props.insert(pid::HASATTACH, flag(pid::HASATTACH, !item.attachments.is_empty()));
        if !item.categories.is_empty() {
            let id = self.names.id_for(&NamedKey::keywords())?;
            props.insert(id, encode(id, &PropValue::MultiString(item.categories.clone())));
        }
        for (key, v) in &item.named {
            let id = self.names.id_for(key)?;
            props.insert(id, encode(id, v));
        }
        // PidTagMessageSize is how much of the file the message takes, its recipients and
        // attachments too: Outlook's repair tool takes a row whose size is far from that for
        // a damaged one (seen 2026-10-13). It is filled in when the rest is written.
        props.insert(pid::MESSAGE_SIZE, int(pid::MESSAGE_SIZE, 0));
        // Short values only: these become the message's row in its folder's table.
        let mut cells: Vec<Raw> = props.values().filter(|r| r.data.len() <= MAX_CELL).map(copy).collect();

        let props: Vec<Raw> = props.into_values().collect();
        let mut subs = Subs::new(1);
        // A message's own text is read in its own code page, and so are its recipients and
        // attachments; a message inside it has its own again.
        let cp = crate::code_page(&props);
        let outer = self.w.within(Some(cp));
        subs.list.push(self.recipients(item)?);
        subs.list.extend(self.attachments(item, depth)?);
        self.w.within(None);
        let (data, size) = write_pc_late(&mut self.w, &props, &mut subs, Some(Late::Stored { id: pid::MESSAGE_SIZE, from }))?;
        drop(props);
        self.w.within(outer);
        if let Some(cell) = cells.iter_mut().find(|c| c.id == pid::MESSAGE_SIZE) {
            *cell = int(pid::MESSAGE_SIZE, size);
        }
        let sub = self.w.subnodes(subs.list)?;
        Ok((data, sub, cells))
    }

    /// The recipient table of a message, one row a recipient ([MS-PST] 2.4.5.3), as a subnode.
    pub(crate) fn recipients(&mut self, item: &Item) -> Result<Sub, PstError> {
        let mut rows = Vec::with_capacity(item.recipients.len());
        for (i, r) in item.recipients.iter().enumerate() {
            // Rows are numbered from 1: Outlook's repair tool takes a table whose first row
            // has the id 0 for a damaged one (seen 2026-10-13).
            let id = u32::try_from(i).ok().and_then(|i| i.checked_add(1)).ok_or_else(|| cannot("that many recipients"))?;
            let mut cells = raws(r);
            cells.insert(pid::ROWID, int(pid::ROWID, count(u64::from(id))));
            rows.push(Row { id, cells: cells.into_values().filter(|c| fits(RECIPIENTS, c)).collect() });
        }
        let (table, table_sub) = self.table(0, RECIPIENTS, &rows)?;
        Ok(Sub { nid: NID_RECIPIENT_TABLE, data: table, sub: table_sub })
    }

    /// The attachment table of a message and each attachment in a subnode its row names
    /// ([MS-PST] 2.4.6), as subnodes; none when the message has no attachments.
    pub(crate) fn attachments(&mut self, item: &Item, depth: usize) -> Result<Vec<Sub>, PstError> {
        let mut list = Vec::new();
        if !item.attachments.is_empty() {
            let mut rows = Vec::with_capacity(item.attachments.len());
            for (i, a) in item.attachments.iter().enumerate() {
                let nid = u32::try_from(i)
                    .ok()
                    .and_then(|i| i.checked_add(0x401))
                    .filter(|i| *i <= MAX_INDEX)
                    .map(|i| i << 5 | TYPE_ATTACHMENT)
                    .ok_or_else(|| cannot("that many attachments"))?;
                let (data, sub, cells) = self.attachment(a, i, depth)?;
                list.push(Sub { nid, data, sub });
                rows.push(Row { id: nid, cells });
            }
            list.push(self.attachment_table(&rows)?);
        }
        Ok(list)
    }

    /// The attachment table of a message ([MS-PST] 2.4.6.1), as a subnode.
    pub(crate) fn attachment_table(&mut self, rows: &[Row]) -> Result<Sub, PstError> {
        let (data, sub) = self.table(0, ATTACHMENTS, rows)?;
        Ok(Sub { nid: NID_ATTACHMENT_TABLE, data, sub })
    }

    /// An attachment as a subnode's data and subnodes ([MS-PST] 2.4.6.2), and the cells of its
    /// row in the attachment table.
    pub(crate) fn attachment(&mut self, a: &Attachment, index: usize, depth: usize) -> Result<(u64, u64, Vec<Raw>), PstError> {
        let mut props = raws(&a.props);
        #[cfg(test)]
        PLANTED.with(|p| props.extend(p.borrow().iter().map(|r| (r.id, copy(r)))));
        props.remove(&pid::ATTACH_DATA);
        props.insert(pid::ATTACH_NUM, int(pid::ATTACH_NUM, count(index as u64)));
        // What an attachment must have ([MS-PST] 2.4.6.2.1), where it does not say.
        for raw in [int(pid::ATTACH_METHOD, 1), int(pid::RENDERING_POSITION, -1)] {
            props.entry(raw.id).or_insert(raw);
        }
        let mut subs = Subs::new(1);
        // PtypObject: the subnode that holds the object, then its size ([MS-PST] 2.3.3.5).
        let object = |nid: u32, size: usize| {
            let mut data = nid.to_le_bytes().to_vec();
            data.extend(u32::try_from(size).unwrap_or(u32::MAX).to_le_bytes());
            Raw { id: pid::ATTACH_DATA, ty: ptype::OBJECT, data }
        };
        let size = if let Some(inner) = &a.embedded {
            // An attached message is a whole message in a subnode ([MS-PST] 2.4.6.2.2).
            let (data, sub, cells) = self.message(inner, depth + 1)?;
            subs.list.push(Sub { nid: NID_EMBEDDED, data, sub });
            props.insert(pid::ATTACH_METHOD, int(pid::ATTACH_METHOD, 5));
            let size = cells.iter().find(|c| c.id == pid::MESSAGE_SIZE).and_then(|c| crate::ndb::u32_at(&c.data, 0)).map_or(0, |n| n as usize);
            props.insert(pid::ATTACH_DATA, object(NID_EMBEDDED, size));
            size
        } else if a.props.i32(pid::ATTACH_METHOD) == Some(6) && !a.data.is_empty() {
            // An object of another program is kept as an object too, its storage as bytes.
            let most = self.w.max_data();
            let nid = subs.object(&mut self.w, a.data.chunks(most))?;
            props.insert(pid::ATTACH_DATA, object(nid, a.data.len()));
            a.data.len()
        } else {
            props.insert(pid::ATTACH_DATA, Raw { id: pid::ATTACH_DATA, ty: X, data: a.data.clone() });
            0
        };
        // PidTagAttachSize is the size of the whole attachment, its data and every other
        // property of it as they are stored: Outlook's repair tool calls another number
        // invalid (seen 2026-10-13). It is filled in as the properties are written.
        props.insert(pid::ATTACH_SIZE, int(pid::ATTACH_SIZE, 0));
        let list: Vec<Raw> = props.into_values().collect();
        let (data, size) = write_pc_late(&mut self.w, &list, &mut subs, Some(Late::Values { id: pid::ATTACH_SIZE, object: size }))?;
        let mut props: BTreeMap<u16, Raw> = list.into_iter().map(|r| (r.id, r)).collect();
        props.insert(pid::ATTACH_SIZE, int(pid::ATTACH_SIZE, size));
        let cells = attachment_cells(&props);
        let sub = self.w.subnodes(subs.list)?;
        Ok((data, sub, cells))
    }
}

/// The cells of an attachment's row in the attachment table: those of its properties the table
/// has a column for.
pub(crate) fn attachment_cells(props: &BTreeMap<u16, Raw>) -> Vec<Raw> {
    ATTACHMENTS.iter().filter_map(|(id, ty)| props.get(id).filter(|r| r.ty == *ty)).map(copy).collect()
}

#[cfg(test)]
thread_local! {
    /// Properties a test has every attachment written with, as another program might write
    /// what the model has no place for.
    pub(crate) static PLANTED: std::cell::RefCell<Vec<Raw>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Write everything `store` holds to `out` as a Unicode `.pst`. `uid` becomes the id of the new
/// data file (PidTagRecordKey); no two data files should share one.
///
/// The source is only read. An item it cannot give is left out and counted, so a mailbox with
/// damage is saved as far as it can be read.
pub fn write_store<W: Read + Write + Seek>(out: W, store: &mut dyn Store, uid: [u8; 16]) -> Result<(W, Written), PstError> {
    write_store_until(out, store, uid, None)
}

/// `write_store`, saying how far it is and stopping when asked: half of the progress is the
/// items written, the other half is left to whoever checks the file afterwards. A write
/// that was stopped gives `PstError::Stopped`, and what it wrote is no data file.
pub fn write_store_until<W: Read + Write + Seek>(
    out: W,
    store: &mut dyn Store,
    uid: [u8; 16],
    progress: Option<&openmailclient_model::Progress>,
) -> Result<(W, Written), PstError> {
    write_store_of(out, store, uid, progress, crate::ndb::Ver::Unicode, None)
}

/// `write_store_until` for an ANSI data file ([MS-PST] 2.2.2.6, wVer 14): the kind older
/// programs read, of at most 2 GB. Text is 8-bit in the code page of its message where that
/// code page has all of it, and Unicode text where it has not, so that nothing is replaced
/// by a question mark; what a program that only knows 8-bit text makes of the latter has not
/// been tried.
pub fn write_store_ansi<W: Read + Write + Seek>(
    out: W,
    store: &mut dyn Store,
    uid: [u8; 16],
    progress: Option<&openmailclient_model::Progress>,
) -> Result<(W, Written), PstError> {
    write_store_of(out, store, uid, progress, crate::ndb::Ver::Ansi, None)
}

/// A contents table in a file with 4K pages has two subnodes more, 0x521 and 0x541, each a
/// heap with the client signature 0xA5, which [MS-PST] 2.3.1.2 calls reserved (measured). In
/// the empty tables of the files measured both are heaps with nothing in them; Outlook's Inbox
/// Repair Tool reads a contents table without them, the template 0x60E among them, as broken.
/// What they hold for a table with rows is not known, and they are written empty.
pub(crate) fn contents_extra<W: Read + Write + Seek>(w: &mut NdbWriter<W>, nid: u32, subs: &mut Subs) -> Result<(), PstError> {
    if w.big() && nid & 0x1F == TYPE_CONTENTS {
        for at in [0x521, 0x541] {
            let empty = crate::ltp_write::Heap::new(0xA5, w.max_data()).finish(0);
            let data = w.data(empty)?;
            subs.list.push(Sub { nid: at, data, sub: 0 });
        }
    }
    Ok(())
}

/// `write_store_until` for a data file with 4K pages (wVer 36), the kind newer versions of
/// Outlook make: pages of 4096 bytes and blocks of up to 64 KB. The layout is in no
/// specification; it was measured on files Outlook wrote (see `write::big`).
pub fn write_store_4k<W: Read + Write + Seek>(
    out: W,
    store: &mut dyn Store,
    uid: [u8; 16],
    progress: Option<&openmailclient_model::Progress>,
) -> Result<(W, Written), PstError> {
    write_store_of(out, store, uid, progress, crate::ndb::Ver::Unicode4k, None)
}

/// `write_store_until` for a data file written whole again as the kind `ver` it is: every
/// folder and message keeps the node id it has in `store`, and the file goes on from the
/// header's `counters`. An error when an id cannot be kept (one of another kind, one given
/// twice, or one this writer keeps for a node of its own).
pub(crate) fn write_store_keeping<W: Read + Write + Seek>(
    out: W,
    store: &mut dyn Store,
    uid: [u8; 16],
    progress: Option<&openmailclient_model::Progress>,
    ver: crate::ndb::Ver,
    counters: (u32, [u32; 32]),
) -> Result<(W, Written), PstError> {
    write_store_of(out, store, uid, progress, ver, Some(counters))
}

/// The folders this writer gives node ids of their own.
const OWN_FOLDERS: [u32; 5] = [NID_ROOT_FOLDER, NID_IPM_SUBTREE, NID_SEARCH_ROOT, NID_DELETED_ITEMS, NID_SPAM_SEARCH_FOLDER];

/// The node id an item has, kept: one of the kind `ty`, not given twice, and not one of the
/// folders this writer numbers itself, but for the deleted items folder, which keeps that one.
fn kept_id(id: u64, ty: u32, deleted: bool, taken: &mut HashSet<u32>) -> Result<u32, PstError> {
    u32::try_from(id)
        .ok()
        .filter(|n| n & 0x1F == ty && n >> 5 != 0)
        .filter(|n| if deleted { *n == NID_DELETED_ITEMS } else { !OWN_FOLDERS.contains(n) })
        .filter(|n| taken.insert(*n))
        .ok_or_else(|| PstError::Write(format!("the id {id:#x} cannot be kept in a file written whole")))
}

fn write_store_of<W: Read + Write + Seek>(
    out: W,
    store: &mut dyn Store,
    uid: [u8; 16],
    progress: Option<&openmailclient_model::Progress>,
    ver: crate::ndb::Ver,
    keep: Option<(u32, [u32; 32])>,
) -> Result<(W, Written), PstError> {
    let source = |e: openmailclient_model::StoreError| PstError::Source(e.to_string());
    let listed = store.folders().map_err(source)?;
    if let Some(p) = progress {
        p.begin(listed.iter().map(|f| u64::from(f.total)).sum::<u64>().saturating_mul(2));
    }
    // Data blocks are permuted, as in the data files Outlook makes ([MS-PST] 2.2.2.6).
    let mut o = Out { w: NdbWriter::of(out, crate::write::crypt_of(ver), ver)?, names: Names::default(), uid, serial: 0 };
    if let Some(counters) = keep {
        o.w.carry(counters);
    }
    let mut taken: HashSet<u32> = HashSet::new();
    let mut done = Written::default();

    // 0: the top of the visible tree; 1…: the source's folders in its order.
    let mut folders = vec![Folder {
        nid: NID_IPM_SUBTREE,
        parent: NID_ROOT_FOLDER,
        name: "Top of Data File".to_string(),
        class: String::new(),
        children: Vec::new(),
        total: 0,
        unread: 0,
        kept: BTreeMap::new(),
        hidden: Vec::new(),
    }];
    let mut place: HashMap<u64, usize> = HashMap::new();
    let mut ids: Vec<FolderId> = Vec::with_capacity(listed.len());
    let mut special: Vec<(u16, u32)> = Vec::new();
    // The folders other programs find by an id and not by a name, and the Inbox, which
    // carries those ids as the root does ([MS-OXOSFLD] 2.2.3).
    let mut found: Vec<(u16, u32)> = Vec::new();
    let mut inbox: Option<u32> = None;
    let mut next = FIRST_FOLDER;
    // The deleted items folder has a node of its own; the first folder of that kind takes it.
    let deleted = listed.iter().position(|f| f.kind == FolderKind::Deleted && f.parent.is_none());
    for (i, f) in listed.iter().enumerate() {
        let nid = if keep.is_some() {
            kept_id(f.id.0, TYPE_FOLDER, Some(i) == deleted, &mut taken)?
        } else if Some(i) == deleted {
            NID_DELETED_ITEMS
        } else {
            let index = next;
            next = next.checked_add(1).filter(|n| *n <= MAX_INDEX).ok_or_else(|| cannot("that many folders"))?;
            index << 5 | TYPE_FOLDER
        };
        // A parent comes before its children; a folder that names none, or one not seen yet,
        // goes to the top.
        let parent = f.parent.and_then(|p| place.get(&p.0).copied()).unwrap_or(0);
        let at = folders.len();
        place.entry(f.id.0).or_insert(at);
        ids.push(f.id);
        if let Some(p) = folders.get_mut(parent) {
            p.children.push(at);
        }
        let parent = folders.get(parent).map_or(NID_IPM_SUBTREE, |p| p.nid);
        // PidTagIpmOutboxEntryId and PidTagIpmSentMailEntryId: the first folder of each kind.
        for (id, kind) in [(0x35E2u16, FolderKind::Outbox), (0x35E4, FolderKind::Sent)] {
            if f.kind == kind && special.iter().all(|(seen, _)| *seen != id) {
                special.push((id, nid));
            }
        }
        for (id, kind) in SPECIAL {
            if f.kind == *kind && found.iter().all(|(seen, _)| seen != id) {
                found.push((*id, nid));
            }
        }
        if f.kind == FolderKind::Inbox && inbox.is_none() {
            inbox = Some(nid);
        }
        folders.push(Folder {
            nid,
            parent,
            name: f.name.clone(),
            class: f.container_class.clone(),
            children: Vec::new(),
            total: 0,
            unread: 0,
            kept: BTreeMap::new(),
            hidden: Vec::new(),
        });
    }
    if deleted.is_none() {
        let at = folders.len();
        if let Some(top) = folders.first_mut() {
            top.children.push(at);
        }
        folders.push(Folder {
            nid: NID_DELETED_ITEMS,
            parent: NID_IPM_SUBTREE,
            name: "Deleted Items".to_string(),
            class: String::new(),
            children: Vec::new(),
            total: 0,
            unread: 0,
            kept: BTreeMap::new(),
            hidden: Vec::new(),
        });
    }

    // What the source keeps beside the model: each folder's own properties and its hidden
    // items, entry ids naming its folders made to name them in the new file.
    let renamed = Renamed {
        from: store.record_key(),
        to: uid,
        nids: ids
            .iter()
            .enumerate()
            .filter_map(|(i, id)| Some((u32::try_from(id.0).ok()?, folders.get(i + 1)?.nid)))
            .chain([(NID_ROOT_FOLDER, NID_ROOT_FOLDER), (NID_IPM_SUBTREE, NID_IPM_SUBTREE), (NID_SEARCH_ROOT, NID_SEARCH_ROOT)])
            .collect(),
    };
    for (i, id) in ids.iter().enumerate() {
        let Ok(mut bag) = store.folder_props(*id) else {
            done.unreadable += 1;
            continue;
        };
        renamed.bag(&mut bag);
        if let Some(f) = folders.get_mut(i + 1) {
            f.kept = raws(&bag).into_iter().filter(|(id, _)| !written_here(*id)).collect();
        }
    }

    // The messages, folder by folder, and each folder's contents table.
    let mut next = FIRST_MESSAGE;
    for (i, id) in ids.iter().enumerate() {
        let Some(nid) = folders.get(i + 1).map(|f| f.nid) else { continue };
        let summaries = store.list(*id).unwrap_or_else(|_| {
            done.unreadable += 1;
            Vec::new()
        });
        let mut rows = Vec::with_capacity(summaries.len());
        let (mut total, mut unread) = (0u64, 0u64);
        for s in summaries {
            if let Some(p) = progress {
                if p.stopped() {
                    return Err(PstError::Stopped);
                }
                p.step();
            }
            let Ok(item) = store.load(*id, ItemId(s.id)) else {
                done.unreadable += 1;
                continue;
            };
            let message = if keep.is_some() {
                kept_id(s.id, TYPE_MESSAGE, false, &mut taken)?
            } else {
                let index = next;
                next = next.checked_add(1).filter(|n| *n <= MAX_INDEX).ok_or_else(|| cannot("that many messages"))?;
                index << 5 | TYPE_MESSAGE
            };
            let (data, sub, cells) = o.message(&item, 0)?;
            o.w.node(message, nid, data, sub);
            let columns: Vec<(u16, u16)> = CONTENTS.iter().chain(CONTENTS_MORE).copied().collect();
            let cells = cells.iter().filter_map(|c| cell_for(&columns, c)).collect();
            rows.push(Row { id: message, cells });
            total += 1;
            unread += u64::from(item.flags() & MSGFLAG_READ == 0);
        }
        o.contents(nid, &rows)?;
        // The folder's hidden items, after its messages.
        let hidden = store.hidden(*id).unwrap_or_else(|_| {
            done.unreadable += 1;
            Vec::new()
        });
        let mut assoc = Vec::with_capacity(hidden.len());
        for (hid, item) in hidden {
            if progress.is_some_and(|p| p.stopped()) {
                return Err(PstError::Stopped);
            }
            let Ok(mut item) = item else {
                done.unreadable += 1;
                continue;
            };
            renamed.bag(&mut item.props);
            let message = if keep.is_some() {
                kept_id(hid.0, TYPE_ASSOC_MESSAGE, false, &mut taken)?
            } else {
                let index = next;
                next = next.checked_add(1).filter(|n| *n <= MAX_INDEX).ok_or_else(|| cannot("that many messages"))?;
                index << 5 | TYPE_ASSOC_MESSAGE
            };
            let (data, sub, cells) = o.message(&item, 0)?;
            o.w.node(message, nid, data, sub);
            assoc.push(Row { id: message, cells: cells.iter().filter_map(|c| cell_for(ASSOC, c)).collect() });
            done.hidden += 1;
        }
        if let Some(f) = folders.get_mut(i + 1) {
            (f.total, f.unread, f.hidden) = (total, unread, assoc);
        }
        done.items += total;
    }
    done.folders = ids.len() as u64;

    // The folders, with the tables of their subfolders and of their associated items.
    let none: &[Row] = &[];
    let found: Vec<Raw> = found.into_iter().map(|(id, nid)| o.entry_id(id, nid)).collect();
    for (i, f) in folders.iter().enumerate() {
        // The top and a deleted items folder made here have no contents table yet.
        let made = i == 0 || i > ids.len();
        let more: &[Raw] = if Some(f.nid) == inbox { &found } else { &[] };
        o.folder(&folders, f, made.then_some(none), more)?;
    }

    // The root, which is its own parent, over the visible tree and the search root
    // ([MS-PST] 2.4.4.2, 2.7.1).
    let plain = |nid: u32, name: &str, children: Vec<usize>| Folder {
        nid,
        parent: NID_ROOT_FOLDER,
        name: name.to_string(),
        class: String::new(),
        children,
        total: 0,
        unread: 0,
        kept: BTreeMap::new(),
        hidden: Vec::new(),
    };
    let search = plain(NID_SEARCH_ROOT, "Search Root", Vec::new());
    o.folder(&folders, &search, Some(none), &[])?;
    let top = folders.first().map(|f| Row { id: f.nid, cells: f.props(!f.children.is_empty()) });
    let spam = plain(NID_SPAM_SEARCH_FOLDER, "SPAM Search Folder 2", Vec::new());
    // Three rows: the visible tree, the search root and the search folder ([MS-PST] 2.7.3.4.1).
    let rows: Vec<Row> =
        top.into_iter().chain([Row { id: search.nid, cells: search.props(false) }, Row { id: spam.nid, cells: spam.props(false) }]).collect();
    let root = plain(NID_ROOT_FOLDER, "", Vec::new());
    let mut props = root.props(true);
    props.extend(found);
    o.pc_node(NID_ROOT_FOLDER, NID_ROOT_FOLDER, &props)?;
    o.tc_node(NID_ROOT_FOLDER & !0x1F | TYPE_HIERARCHY, HIERARCHY, &rows)?;
    o.contents(NID_ROOT_FOLDER, &[])?;
    o.tc_node(NID_ROOT_FOLDER & !0x1F | TYPE_ASSOC, ASSOC, &[])?;
    // The search folder every data file has; the name is the one the format gives it. With
    // it go an empty queue of what changed, a search for nothing and an empty contents table
    // ([MS-PST] 2.4.8.6), and its place in the list of folders that are searched (2.4.8.4.2):
    // Outlook does not load a file whose search folder lacks them.
    o.pc_node(NID_SPAM_SEARCH_FOLDER, NID_ROOT_FOLDER, &spam.props(false))?;
    o.w.node(NID_SPAM_SEARCH_FOLDER & !0x1F | TYPE_SEARCH_UPDATE_QUEUE, 0, 0, 0);
    o.w.node(NID_SEARCH_QUEUE, 0, 0, 0);
    o.pc_node(NID_SPAM_SEARCH_FOLDER & !0x1F | TYPE_SEARCH_CRITERIA, 0, &[Raw { id: 0x660B, ty: I, data: 0u32.to_le_bytes().to_vec() }])?;
    o.tc_node(NID_SPAM_SEARCH_FOLDER & !0x1F | TYPE_SEARCH_CONTENTS, SEARCH_CONTENTS, &[])?;
    let searched = o.w.data([NID_SPAM_SEARCH_FOLDER.to_le_bytes()])?;
    o.w.node(NID_SEARCH_ACTIVITY_LIST, 0, searched, 0);

    // The message store ([MS-PST] 2.4.3.1).
    let mut props = vec![Raw { id: 0x0FF9, ty: X, data: uid.to_vec() }, text(pid::DISPLAY_NAME, &store.display_name())];
    for (id, nid) in [(pid::IPM_SUBTREE_ENTRY_ID, NID_IPM_SUBTREE), (0x35E3, NID_DELETED_ITEMS), (0x35E7, NID_SEARCH_ROOT)].into_iter().chain(special)
    {
        props.push(o.entry_id(id, nid));
    }
    // PidTagPstPassword: none ([MS-PST] 2.4.3.3). Outlook asks for the property itself.
    props.push(Raw { id: 0x67FF, ty: I, data: 0u32.to_le_bytes().to_vec() });
    o.pc_node(NID_MESSAGE_STORE, 0, &props)?;
    let names = o.names.props();
    o.pc_node(NID_NAME_TO_ID_MAP, 0, &names)?;

    // The tables new tables are made from, and the two search lists, empty ([MS-PST] 2.7.1).
    for (nid, columns) in [
        (NID_HIERARCHY_TEMPLATE, HIERARCHY),
        (NID_CONTENTS_TEMPLATE, CONTENTS),
        (NID_ASSOC_TEMPLATE, ASSOC),
        (NID_SEARCH_CONTENTS_TEMPLATE, SEARCH_CONTENTS),
        (NID_RECIPIENT_TABLE, RECIPIENTS),
        (NID_ATTACHMENT_TABLE, ATTACHMENTS),
        (NID_OUTGOING_QUEUE, OUTGOING_QUEUE),
        (NID_TEMPLATE_16, TEMPLATE_16),
        (NID_TEMPLATE_17, TEMPLATE_17),
        (NID_TEMPLATE_18, TEMPLATE_18),
    ] {
        // Outlook's repair tool wants a four-byte row place in these two, in an ANSI file too.
        let wide = nid == NID_TEMPLATE_17 || nid == NID_TEMPLATE_18;
        let mut subs = Subs::new(1);
        let data = write_tc_as(&mut o.w, columns, &[], &mut subs, wide)?;
        contents_extra(&mut o.w, nid, &mut subs)?;
        let sub = o.w.subnodes(subs.list)?;
        o.w.node(nid, 0, data, sub);
    }
    // Mail of any class is received in the Inbox, or at the root where there is none.
    let receive = Row {
        id: 1,
        cells: vec![
            Raw { id: 0x001A, ty: S, data: Vec::new() },
            Raw { id: 0x6605, ty: I, data: inbox.unwrap_or(NID_ROOT_FOLDER).to_le_bytes().to_vec() },
        ],
    };
    o.tc_node(NID_RECEIVE_FOLDERS, RECEIVE_FOLDERS, &[receive])?;
    o.w.node(NID_SEARCH_MANAGEMENT_QUEUE, 0, 0, 0);
    Ok((o.w.finish()?, done))
}
