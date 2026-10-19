//! Changes journalled as they are made.
//!
//! A change to an open data file is held in memory until it is saved ([`crate::edit`]). So
//! that a run that ends badly does not lose it, each change is also written, as it is made,
//! to a journal beside the data file. The data file itself is not touched. When the file is
//! opened again the journal is played back and the changes are there, unsaved, as they were.
//!
//! The journal is a list of what was asked for, in order: the same requests on the same file
//! give the same result. It is therefore tied to the file it was made for (its length and
//! header), and a journal for a file that has since been written by anyone is left alone,
//! never applied. A record is a length, a checksum and the request; playing back stops at
//! the first record that is cut short, damaged or cannot be done, and what came before it
//! stands. A journal is read as anything else from disk: every number in it is doubted.
//!
//! A change to a message is written as what changed in it, not the whole message, so marking
//! a message with a large attachment as read costs a few bytes.

use crate::write::crc;
use openmailclient_model::{Attachment, Item, NameId, NamedKey, PropBag, PropValue};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"OMCJRNL1";
/// Magic, the data file's length, the checksum of its start.
pub(crate) const HEADER: u64 = 20;
/// This much of the data file's start is summed to know it again: its header, which changes
/// with every write to the file ([MS-PST] 2.2.2.6).
const IDENT_BYTES: u64 = 1024;
/// A record larger than this is not written, and is not believed when read.
const MAX_RECORD: usize = 256 << 20;
/// A journal is not grown past this; the changes then live in memory only until saved.
const MAX_JOURNAL: u64 = 4 << 30;
/// Messages attached to messages are written and read this deep.
const MAX_DEPTH: usize = 16;
/// The journal is forced to disk after this many records. Each record reaches the system as
/// it is made, which is what a crash of the program needs; a power cut may cost the last few.
const SYNC_EVERY: u32 = 64;

/// The parent journalled for a folder made at the top of the tree: no folder has the id 0.
pub(crate) const TOP: u64 = 0;

/// One request, as it is journalled.
#[derive(Debug, PartialEq)]
pub(crate) enum Op {
    Update {
        folder: u64,
        id: u64,
        diff: Diff,
    },
    Add {
        folder: u64,
        item: Item,
    },
    /// A message taken out of a folder a moment ago, put into one: the same message still.
    Put {
        folder: u64,
        nid: u32,
    },
    Remove {
        folder: u64,
        id: u64,
    },
    /// `parent` is `TOP` for a folder at the top of the tree.
    CreateFolder {
        parent: u64,
        name: String,
    },
    /// A folder of the kind its container class names; `parent` as for `CreateFolder`.
    CreateFolderOf {
        parent: u64,
        name: String,
        class: String,
    },
    RenameFolder {
        folder: u64,
        name: String,
    },
    DeleteFolder {
        folder: u64,
    },
    MoveFolder {
        folder: u64,
        parent: u64,
    },
}

/// What changed in a message.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Diff {
    set: Vec<(u16, PropValue)>,
    gone: Vec<u16>,
    recipients: Option<Vec<PropBag>>,
    attachments: Option<Vec<Attachment>>,
    categories: Option<Vec<String>>,
    named: Option<Vec<(NamedKey, PropValue)>>,
}

impl Diff {
    pub(crate) fn between(was: &Item, now: &Item) -> Self {
        let set = now.props.0.iter().filter(|(id, v)| was.props.0.get(id) != Some(v)).map(|(id, v)| (*id, v.clone())).collect();
        let gone = was.props.0.keys().filter(|id| !now.props.0.contains_key(id)).copied().collect();
        Self {
            set,
            gone,
            recipients: (was.recipients != now.recipients).then(|| now.recipients.clone()),
            attachments: (was.attachments != now.attachments).then(|| now.attachments.clone()),
            categories: (was.categories != now.categories).then(|| now.categories.clone()),
            named: (was.named != now.named).then(|| now.named.clone()),
        }
    }

    pub(crate) fn apply(self, item: &mut Item) {
        for id in self.gone {
            item.props.0.remove(&id);
        }
        item.props.0.extend(self.set);
        if let Some(v) = self.recipients {
            item.recipients = v;
        }
        if let Some(v) = self.attachments {
            item.attachments = v;
        }
        if let Some(v) = self.categories {
            item.categories = v;
        }
        if let Some(v) = self.named {
            item.named = v;
        }
    }
}

// ---- writing a record ----

struct Enc(Vec<u8>);

impl Enc {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    /// A count or a length; `None` when it is more than a record can say.
    fn len(&mut self, n: usize) -> Option<()> {
        self.u32(u32::try_from(n).ok()?);
        Some(())
    }
    fn bytes(&mut self, b: &[u8]) -> Option<()> {
        self.len(b.len())?;
        self.0.extend_from_slice(b);
        (self.0.len() <= MAX_RECORD).then_some(())
    }
    fn str(&mut self, s: &str) -> Option<()> {
        self.bytes(s.as_bytes())
    }

    fn value(&mut self, v: &PropValue) -> Option<()> {
        match v {
            PropValue::Int16(n) => {
                self.u8(0);
                self.u16(n.cast_unsigned());
            }
            PropValue::Int32(n) => {
                self.u8(1);
                self.u32(n.cast_unsigned());
            }
            PropValue::Int64(n) => {
                self.u8(2);
                self.u64(n.cast_unsigned());
            }
            PropValue::Bool(b) => {
                self.u8(3);
                self.u8(u8::from(*b));
            }
            PropValue::Float64(f) => {
                self.u8(4);
                self.u64(f.to_bits());
            }
            PropValue::Float32(f) => {
                self.u8(13);
                self.u32(f.to_bits());
            }
            PropValue::Currency(n) => {
                self.u8(14);
                self.u64(n.cast_unsigned());
            }
            PropValue::FloatTime(f) => {
                self.u8(15);
                self.u64(f.to_bits());
            }
            PropValue::Error(n) => {
                self.u8(16);
                self.u32(n.cast_unsigned());
            }
            PropValue::String(s) => {
                self.u8(5);
                self.str(s)?;
            }
            PropValue::Binary(b) => {
                self.u8(6);
                self.bytes(b)?;
            }
            PropValue::Time(t) => {
                self.u8(7);
                self.u64(t.cast_unsigned());
            }
            PropValue::Guid(g) => {
                self.u8(8);
                self.0.extend_from_slice(g);
            }
            PropValue::MultiInt32(all) => {
                self.u8(9);
                self.len(all.len())?;
                for n in all {
                    self.u32(n.cast_unsigned());
                }
            }
            PropValue::MultiString(all) => {
                self.u8(10);
                self.len(all.len())?;
                for s in all {
                    self.str(s)?;
                }
            }
            PropValue::MultiBinary(all) => {
                self.u8(11);
                self.len(all.len())?;
                for b in all {
                    self.bytes(b)?;
                }
            }
            PropValue::Other { ty, raw } => {
                self.u8(12);
                self.u16(*ty);
                self.bytes(raw)?;
            }
        }
        Some(())
    }

    fn bag(&mut self, bag: &PropBag) -> Option<()> {
        self.len(bag.0.len())?;
        for (id, v) in &bag.0 {
            self.u16(*id);
            self.value(v)?;
        }
        Some(())
    }

    fn bags(&mut self, all: &[PropBag]) -> Option<()> {
        self.len(all.len())?;
        all.iter().try_for_each(|b| self.bag(b))
    }

    fn strings(&mut self, all: &[String]) -> Option<()> {
        self.len(all.len())?;
        all.iter().try_for_each(|s| self.str(s))
    }

    fn named(&mut self, all: &[(NamedKey, PropValue)]) -> Option<()> {
        self.len(all.len())?;
        for (key, v) in all {
            self.0.extend_from_slice(&key.guid);
            match &key.name {
                NameId::Id(id) => {
                    self.u8(0);
                    self.u32(*id);
                }
                NameId::Name(name) => {
                    self.u8(1);
                    self.str(name)?;
                }
            }
            self.value(v)?;
        }
        Some(())
    }

    fn attachments(&mut self, all: &[Attachment], depth: usize) -> Option<()> {
        self.len(all.len())?;
        for a in all {
            self.bag(&a.props)?;
            self.bytes(&a.data)?;
            match &a.embedded {
                Some(item) => {
                    self.u8(1);
                    self.item(item, depth.checked_add(1)?)?;
                }
                None => self.u8(0),
            }
        }
        Some(())
    }

    fn item(&mut self, item: &Item, depth: usize) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.bag(&item.props)?;
        self.bags(&item.recipients)?;
        self.attachments(&item.attachments, depth)?;
        self.strings(&item.categories)?;
        self.named(&item.named)
    }

    /// A part that may be left out: a mark, then the part when it is there.
    fn maybe<T: ?Sized>(&mut self, part: Option<&T>, put: impl FnOnce(&mut Self, &T) -> Option<()>) -> Option<()> {
        match part {
            Some(part) => {
                self.u8(1);
                put(self, part)
            }
            None => {
                self.u8(0);
                Some(())
            }
        }
    }
}

impl Op {
    /// The request as the bytes of a record; `None` when it is more than a record holds.
    pub(crate) fn encode(&self) -> Option<Vec<u8>> {
        let mut e = Enc(Vec::new());
        match self {
            Self::Update { folder, id, diff } => {
                e.u8(1);
                e.u64(*folder);
                e.u64(*id);
                e.len(diff.set.len())?;
                for (id, v) in &diff.set {
                    e.u16(*id);
                    e.value(v)?;
                }
                e.len(diff.gone.len())?;
                for id in &diff.gone {
                    e.u16(*id);
                }
                e.maybe(diff.recipients.as_deref(), Enc::bags)?;
                e.maybe(diff.attachments.as_deref(), |e, all| e.attachments(all, 0))?;
                e.maybe(diff.categories.as_deref(), Enc::strings)?;
                e.maybe(diff.named.as_deref(), Enc::named)?;
            }
            Self::Add { folder, item } => {
                e.u8(2);
                e.u64(*folder);
                e.item(item, 0)?;
            }
            Self::Put { folder, nid } => {
                e.u8(3);
                e.u64(*folder);
                e.u32(*nid);
            }
            Self::Remove { folder, id } => {
                e.u8(4);
                e.u64(*folder);
                e.u64(*id);
            }
            Self::CreateFolder { parent, name } => {
                e.u8(5);
                e.u64(*parent);
                e.str(name)?;
            }
            Self::CreateFolderOf { parent, name, class } => {
                e.u8(9);
                e.u64(*parent);
                e.str(name)?;
                e.str(class)?;
            }
            Self::RenameFolder { folder, name } => {
                e.u8(6);
                e.u64(*folder);
                e.str(name)?;
            }
            Self::DeleteFolder { folder } => {
                e.u8(7);
                e.u64(*folder);
            }
            Self::MoveFolder { folder, parent } => {
                e.u8(8);
                e.u64(*folder);
                e.u64(*parent);
            }
        }
        (e.0.len() <= MAX_RECORD).then_some(e.0)
    }

    /// A record's bytes as a request; `None` when they are not one, whole and nothing over.
    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        let mut d = Dec(bytes);
        let op = match d.u8()? {
            1 => {
                let (folder, id) = (d.u64()?, d.u64()?);
                let mut diff = Diff::default();
                for _ in 0..d.count(3)? {
                    diff.set.push((d.u16()?, d.value()?));
                }
                for _ in 0..d.count(2)? {
                    diff.gone.push(d.u16()?);
                }
                diff.recipients = d.maybe(Dec::bags)?;
                diff.attachments = d.maybe(|d| d.attachments(0))?;
                diff.categories = d.maybe(Dec::strings)?;
                diff.named = d.maybe(Dec::named)?;
                Self::Update { folder, id, diff }
            }
            2 => Self::Add { folder: d.u64()?, item: d.item(0)? },
            3 => Self::Put { folder: d.u64()?, nid: d.u32()? },
            4 => Self::Remove { folder: d.u64()?, id: d.u64()? },
            5 => Self::CreateFolder { parent: d.u64()?, name: d.string()? },
            6 => Self::RenameFolder { folder: d.u64()?, name: d.string()? },
            7 => Self::DeleteFolder { folder: d.u64()? },
            8 => Self::MoveFolder { folder: d.u64()?, parent: d.u64()? },
            9 => Self::CreateFolderOf { parent: d.u64()?, name: d.string()?, class: d.string()? },
            _ => return None,
        };
        d.0.is_empty().then_some(op)
    }
}

// ---- reading a record ----

struct Dec<'a>(&'a [u8]);

impl Dec<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }
    fn u8(&mut self) -> Option<u8> {
        self.array().map(u8::from_le_bytes)
    }
    fn u16(&mut self) -> Option<u16> {
        self.array().map(u16::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.array().map(u64::from_le_bytes)
    }
    /// How many of something follow, each at least `each` bytes: never more than there is
    /// room for in what is left, so a count cannot ask for memory the record does not have.
    fn count(&mut self, each: usize) -> Option<usize> {
        let n = usize::try_from(self.u32()?).ok()?;
        (n.checked_mul(each.max(1))? <= self.0.len()).then_some(n)
    }
    fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = self.count(1)?;
        self.take(n).map(<[u8]>::to_vec)
    }
    fn string(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?).ok()
    }

    fn value(&mut self) -> Option<PropValue> {
        Some(match self.u8()? {
            0 => PropValue::Int16(self.u16()?.cast_signed()),
            1 => PropValue::Int32(self.u32()?.cast_signed()),
            2 => PropValue::Int64(self.u64()?.cast_signed()),
            3 => PropValue::Bool(self.u8()? != 0),
            4 => PropValue::Float64(f64::from_bits(self.u64()?)),
            5 => PropValue::String(self.string()?),
            6 => PropValue::Binary(self.bytes()?),
            7 => PropValue::Time(self.u64()?.cast_signed()),
            8 => PropValue::Guid(self.array()?),
            9 => {
                let n = self.count(4)?;
                PropValue::MultiInt32((0..n).map(|_| self.u32().map(u32::cast_signed)).collect::<Option<_>>()?)
            }
            10 => PropValue::MultiString(self.strings()?),
            11 => {
                let n = self.count(4)?;
                PropValue::MultiBinary((0..n).map(|_| self.bytes()).collect::<Option<_>>()?)
            }
            12 => PropValue::Other { ty: self.u16()?, raw: self.bytes()? },
            13 => PropValue::Float32(f32::from_bits(self.u32()?)),
            14 => PropValue::Currency(self.u64()?.cast_signed()),
            15 => PropValue::FloatTime(f64::from_bits(self.u64()?)),
            16 => PropValue::Error(self.u32()?.cast_signed()),
            _ => return None,
        })
    }

    fn bag(&mut self) -> Option<PropBag> {
        let mut bag = PropBag::default();
        for _ in 0..self.count(3)? {
            bag.0.insert(self.u16()?, self.value()?);
        }
        Some(bag)
    }

    fn bags(&mut self) -> Option<Vec<PropBag>> {
        let n = self.count(4)?;
        (0..n).map(|_| self.bag()).collect()
    }

    fn strings(&mut self) -> Option<Vec<String>> {
        let n = self.count(4)?;
        (0..n).map(|_| self.string()).collect()
    }

    fn named(&mut self) -> Option<Vec<(NamedKey, PropValue)>> {
        let n = self.count(18)?;
        let mut all = Vec::new();
        for _ in 0..n {
            let guid = self.array()?;
            let name = match self.u8()? {
                0 => NameId::Id(self.u32()?),
                1 => NameId::Name(self.string()?),
                _ => return None,
            };
            all.push((NamedKey { guid, name }, self.value()?));
        }
        Some(all)
    }

    fn attachments(&mut self, depth: usize) -> Option<Vec<Attachment>> {
        let n = self.count(9)?;
        let mut all = Vec::new();
        for _ in 0..n {
            let (props, data) = (self.bag()?, self.bytes()?);
            let embedded = match self.u8()? {
                0 => None,
                1 => Some(Box::new(self.item(depth.checked_add(1)?)?)),
                _ => return None,
            };
            all.push(Attachment { props, data, embedded });
        }
        Some(all)
    }

    fn item(&mut self, depth: usize) -> Option<Item> {
        if depth > MAX_DEPTH {
            return None;
        }
        Some(Item {
            props: self.bag()?,
            recipients: self.bags()?,
            attachments: self.attachments(depth)?,
            categories: self.strings()?,
            named: self.named()?,
        })
    }

    fn maybe<T>(&mut self, get: impl FnOnce(&mut Self) -> Option<T>) -> Option<Option<T>> {
        match self.u8()? {
            0 => Some(None),
            1 => get(self).map(Some),
            _ => None,
        }
    }
}

// ---- the file ----

/// Where the journal of a data file is kept: beside it.
pub fn journal_beside(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(std::ffi::OsString::from).unwrap_or_default();
    name.push(".openmailclient-journal");
    path.with_file_name(name)
}

/// Where the journal of a data file is kept when it cannot be kept beside it (the folder is
/// not to be written, as on a disc or a share that is only read): in `dir`, under the data
/// file's name and a number made of the whole of where the file is, so that two files of
/// one name in two folders do not share a journal.
pub fn journal_elsewhere(dir: &Path, path: &Path) -> PathBuf {
    let whole = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    // FNV-1a, 64 bits: a name, not a secret.
    let mut sum = 0xcbf2_9ce4_8422_2325u64;
    for b in whole.to_string_lossy().bytes() {
        sum = (sum ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    let mut name = path.file_name().map(std::ffi::OsString::from).unwrap_or_default();
    name.push(format!(".{sum:016x}.openmailclient-journal"));
    dir.join(name)
}

/// Where a journal that could not be played back, or not all of it, is kept.
pub fn set_aside(path: &Path) -> PathBuf {
    aside_of(&journal_beside(path))
}

/// The other name of the journal kept at `at`.
pub(crate) fn aside_of(at: &Path) -> PathBuf {
    let mut name = at.as_os_str().to_os_string();
    name.push(".old");
    PathBuf::from(name)
}

/// Keep what a journal holds under the other name before any of it is cut off. Read through
/// the handle that holds it, which is the only one that may. `at` is where the journal is.
pub(crate) fn keep_aside(file: &mut std::fs::File, at: &Path) {
    let copied = file.seek(SeekFrom::Start(0)).and_then(|_| {
        let mut out = std::fs::File::create(aside_of(at))?;
        std::io::copy(file, &mut out)?;
        out.sync_all()
    });
    if copied.is_err() {
        let _ = std::fs::remove_file(aside_of(at));
    }
}

/// What tells a data file from itself after a write: its length and the sum of its header.
pub(crate) fn ident(path: &Path) -> Option<(u64, u32)> {
    let file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut head = Vec::new();
    file.take(IDENT_BYTES).read_to_end(&mut head).ok()?;
    Some((len, crc(&head)))
}

fn header(ident: (u64, u32)) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&ident.0.to_le_bytes());
    out.extend_from_slice(&ident.1.to_le_bytes());
    out
}

/// Hold the journal for this program alone. `false` when another has it.
fn hold(file: &std::fs::File) -> bool {
    // Where files cannot be held at all there is no one to hold it against.
    !matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock))
}

fn held() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::WouldBlock, "another program holds it")
}

/// The journal of one open data file.
#[derive(Default)]
pub(crate) struct Journal {
    file: Option<std::fs::File>,
    /// Where that file is: beside the data file, or in the other place.
    at: Option<PathBuf>,
    records: u32,
    /// Why changes are not journalled any more, when they are not.
    pub(crate) trouble: Option<String>,
    /// True while the journal is played back: what is played is not written again.
    pub(crate) playing: bool,
    /// True when the journal file is this run's to write and to take away.
    mine: bool,
    /// A folder to keep the journal in when it cannot be kept beside the data file.
    pub(crate) elsewhere: Option<PathBuf>,
    /// Records were written that the disk has not been told to keep yet.
    loose: bool,
}

/// What reading a journal found.
pub(crate) enum Found {
    /// There is none.
    Nothing,
    /// It is for another file, or for this one before it was written again; it is left alone.
    Stale,
    /// Another running program holds it.
    Held,
    /// Its records, each with where it ends in the file, and where the journal is.
    Records { file: std::fs::File, at: PathBuf, records: Vec<(Vec<u8>, u64)>, after: After },
}

/// What follows the last whole record of a journal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct After {
    /// Records that are there but not to be believed: one whose sum is wrong, and each that
    /// follows it (a record is played only after all before it).
    pub(crate) damaged: u32,
    /// One more record was begun and cut short: the last thing written before the end.
    pub(crate) cut: bool,
    /// Bytes follow that cannot be told into records: how many requests they were is not
    /// known.
    pub(crate) unknown: bool,
}

impl Journal {
    /// Read the journal of a data file: the one beside it, or, when there is none there, the
    /// one in the other place.
    pub(crate) fn read(path: &Path, ident: (u64, u32), elsewhere: Option<&Path>) -> std::io::Result<Found> {
        match Self::read_at(journal_beside(path), ident) {
            Ok(Found::Nothing) | Err(_) if elsewhere.is_some() => {}
            found => return found,
        }
        match elsewhere {
            Some(dir) => Self::read_at(journal_elsewhere(dir, path), ident),
            None => Ok(Found::Nothing),
        }
    }

    fn read_at(at: PathBuf, ident: (u64, u32)) -> std::io::Result<Found> {
        let mut file = match std::fs::File::options().read(true).write(true).open(&at) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Found::Nothing),
            Err(e) => return Err(e),
        };
        if !hold(&file) {
            return Ok(Found::Held);
        }
        let len = file.metadata()?.len();
        let mut head = Vec::new();
        (&file).take(HEADER).read_to_end(&mut head)?;
        if head != header(ident) {
            return Ok(Found::Stale);
        }
        let mut reader = std::io::BufReader::new(&file);
        let (mut at_byte, mut good, mut records, mut after) = (HEADER, HEADER, Vec::new(), After::default());
        while at_byte < len {
            let mut lead = [0u8; 8];
            let (Ok(()), Some(size), Some(sum)) = (
                reader.read_exact(&mut lead),
                lead.first_chunk::<4>().map(|b| u32::from_le_bytes(*b)),
                lead.last_chunk::<4>().map(|b| u32::from_le_bytes(*b)),
            ) else {
                // Less than the start of a record: one that was cut short as it was begun.
                after.cut = true;
                break;
            };
            let end = at_byte.saturating_add(8).saturating_add(u64::from(size));
            // The length is believed only as far as it can be one.
            if usize::try_from(size).map_or(true, |n| n > MAX_RECORD) {
                after.unknown = true;
                break;
            }
            if end > len {
                // A record that says it goes on past the end: after whole records, the one
                // that was being written; after a damaged one, nothing that can be trusted.
                (after.cut, after.unknown) = (after.damaged == 0, after.damaged > 0);
                break;
            }
            let mut payload = Vec::new();
            if (&mut reader).take(u64::from(size)).read_to_end(&mut payload).is_err() || payload.len() as u64 != u64::from(size) {
                after.unknown = true;
                break;
            }
            if after.damaged > 0 || crc(&payload) != sum {
                // Counted, not played: the records still follow one another.
                after.damaged = after.damaged.saturating_add(1);
            } else {
                records.push((payload, end));
                good = end;
            }
            at_byte = end;
        }
        drop(reader);
        file.seek(SeekFrom::Start(good))?;
        Ok(Found::Records { file, at, records, after })
    }

    /// Go on writing a journal that was played back, from where its last good record ends.
    pub(crate) fn resume(&mut self, mut file: std::fs::File, at: PathBuf, end: u64, records: u32) -> std::io::Result<()> {
        file.set_len(end)?;
        file.seek(SeekFrom::Start(end))?;
        (self.file, self.at, self.records, self.mine) = (Some(file), Some(at), records, true);
        Ok(())
    }

    /// True when requests are being written down.
    pub(crate) fn on(&self) -> bool {
        !self.playing && self.trouble.is_none()
    }

    /// True when this run has a journal open.
    pub(crate) fn open(&self) -> bool {
        self.file.is_some()
    }

    /// Where this run's journal is, when it has one.
    pub(crate) fn place(&self) -> Option<&Path> {
        self.at.as_deref().filter(|_| self.file.is_some())
    }

    /// Write one request. When that cannot be done the journal ends there: what it holds is
    /// still the start of what was asked for, and the rest lives in memory only.
    pub(crate) fn log(&mut self, path: Option<&Path>, ident: Option<(u64, u32)>, op: &Op) {
        if self.playing || self.trouble.is_some() {
            return;
        }
        let (Some(path), Some(ident)) = (path, ident) else { return };
        if let Err(why) = self.append(path, ident, op) {
            self.file = None;
            self.trouble = Some(why);
        }
    }

    fn append(&mut self, path: &Path, ident: (u64, u32), op: &Op) -> Result<(), String> {
        let payload = op.encode().ok_or("a change was too large to journal")?;
        let size = u32::try_from(payload.len()).map_err(|_| "a change was too large to journal")?;
        if self.file.is_none() {
            let (file, at) = start(path, ident, self.elsewhere.as_deref()).map_err(|e| format!("the journal could not be started ({e})"))?;
            (self.file, self.at, self.mine) = (Some(file), Some(at), true);
        }
        let file = self.file.as_mut().ok_or("the journal is not open")?;
        let mut record = Vec::with_capacity(payload.len().saturating_add(8));
        record.extend_from_slice(&size.to_le_bytes());
        record.extend_from_slice(&crc(&payload).to_le_bytes());
        record.extend_from_slice(&payload);
        let at = file.stream_position().map_err(|e| e.to_string())?;
        if at.saturating_add(record.len() as u64) > MAX_JOURNAL {
            return Err("the journal is full; save the changes to start it again".to_string());
        }
        if let Err(e) = file.write_all(&record) {
            // Half a record must not stay in front of later ones.
            let _ = file.set_len(at);
            return Err(format!("the journal could not be written ({e})"));
        }
        self.records = self.records.saturating_add(1);
        self.loose = true;
        if self.records.is_multiple_of(SYNC_EVERY) {
            self.settle();
        }
        Ok(())
    }

    /// Have the disk keep what was written so far. Asked for now and then by whoever has a
    /// clock, so that a power cut costs the last moments and not the last 63 changes.
    /// Returns true when there was something to keep.
    pub(crate) fn settle(&mut self) -> bool {
        let loose = std::mem::take(&mut self.loose);
        if loose && let Some(file) = &self.file {
            let _ = file.sync_data();
        }
        loose && self.file.is_some()
    }

    /// The changes are in the data file, or were thrown away: the journal has done its work.
    pub(crate) fn finish(&mut self) {
        let (mine, at, elsewhere) = (self.mine, self.at.take(), self.elsewhere.take());
        *self = Self { elsewhere, ..Self::default() };
        if let Some(at) = at
            && mine
        {
            let _ = std::fs::remove_file(at);
        }
    }
}

/// Begin a journal beside the data file, or in the other place when that cannot be done.
fn start(path: &Path, ident: (u64, u32), elsewhere: Option<&Path>) -> std::io::Result<(std::fs::File, PathBuf)> {
    let beside = journal_beside(path);
    match (start_at(&beside, ident), elsewhere) {
        (Ok(file), _) => Ok((file, beside)),
        // Held by another running program: its changes are not to be written beside in
        // another place, where neither would find the other's.
        (Err(e), _) if e.kind() == std::io::ErrorKind::WouldBlock => Err(e),
        (Err(e), None) => Err(e),
        (Err(_), Some(dir)) => {
            std::fs::create_dir_all(dir)?;
            let at = journal_elsewhere(dir, path);
            start_at(&at, ident).map(|file| (file, at))
        }
    }
}

/// Begin a journal at `at`. One that is already there was not played back (it is for this
/// file as it once was, or the person chose not to): it is kept under another name, not
/// written over.
fn start_at(at: &Path, ident: (u64, u32)) -> std::io::Result<std::fs::File> {
    if at.exists() {
        // Not from under another running program that is writing it.
        if !hold(&std::fs::File::options().read(true).write(true).open(at)?) {
            return Err(held());
        }
        std::fs::rename(at, aside_of(at))?;
    }
    let mut file = std::fs::File::options().read(true).write(true).create_new(true).open(at)?;
    if !hold(&file) {
        return Err(held());
    }
    file.write_all(&header(ident))?;
    file.sync_all()?;
    Ok(file)
}
