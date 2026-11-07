//! File I/O dispatch: which reader or writer a path goes to.

use crate::EngineError;
use openmailclient_model::{Item, Store};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Loose message files larger than this are refused rather than loaded into memory.
const MAX_ITEM_FILE: u64 = 512 * 1024 * 1024;

pub enum Opened {
    /// A data file with folders (`.pst`, `.ost`).
    Store(Box<dyn Store>),
    /// One item (`.msg`, `.oft`, `.eml`).
    Item(Box<Item>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Pst,
    Msg,
    Template,
    Eml,
}

fn kind_by_extension(path: &Path) -> Option<Kind> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "pst" | "ost" => Some(Kind::Pst),
        "msg" => Some(Kind::Msg),
        "oft" => Some(Kind::Template),
        "eml" | "mht" | "mhtml" | "nws" => Some(Kind::Eml),
        _ => None,
    }
}

/// What a look at a data file's structure found (`openmailclient_pst::check`).
pub struct Structure {
    pub pages: u64,
    pub blocks: u64,
    pub nodes: u64,
    pub free: u64,
    pub problems: Vec<String>,
    /// Problems beyond the ones spelled out.
    pub more: u64,
}

/// Hold a data file's structure against the format: checksums, allocation maps, the two indexes
/// and the reference counts. Read-only. `None` for a kind of data file that is not checked yet.
pub fn check_structure(path: &Path) -> Result<Option<Structure>, EngineError> {
    let mut file = std::io::BufReader::new(std::fs::File::open(path)?);
    match openmailclient_pst::check(&mut file) {
        Ok(mut c) => {
            c.problems.append(&mut c.inside);
            Ok(Some(Structure { pages: c.pages, blocks: c.blocks, nodes: c.nodes, free: c.free, problems: c.problems, more: c.more }))
        }
        Err(openmailclient_pst::PstError::Unsupported(_)) => Ok(None),
        Err(openmailclient_pst::PstError::Io(e)) => Err(EngineError::Io(e)),
        Err(e) => Err(EngineError::Format(e.to_string())),
    }
}

/// Open a file read-only. The extension decides; without a known one, the first bytes do.
pub fn open_path(path: &Path) -> Result<Opened, EngineError> {
    let mut file = std::fs::File::open(path)?;
    let mut magic = [0u8; 8];
    let got = file.read(&mut magic)?;
    let magic = magic.get(..got).unwrap_or(&[]);
    let kind = kind_by_extension(path).unwrap_or(if magic.starts_with(b"!BDN") {
        Kind::Pst
    } else if magic.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        Kind::Msg
    } else {
        Kind::Eml
    });
    drop(file);
    if kind == Kind::Pst {
        let store = openmailclient_pst::PstStore::open(path).map_err(|e| EngineError::Format(e.to_string()))?;
        return Ok(Opened::Store(Box::new(store)));
    }
    if std::fs::metadata(path)?.len() > MAX_ITEM_FILE {
        return Err(EngineError::Format("this file is too large to open as one message".into()));
    }
    let data = std::fs::read(path)?;
    let item = match kind {
        Kind::Eml => openmailclient_mime::parse(&data).map_err(|e| EngineError::Format(e.to_string()))?,
        _ => openmailclient_msg::read(&data).map_err(|e| EngineError::Format(e.to_string()))?,
    };
    Ok(Opened::Item(Box::new(item)))
}

/// Open a file that is held in memory, as one chosen in a browser (M6.6): `name` is the name it
/// came under. The name's extension decides, as for `open_path`; without one, the first bytes.
pub fn open_bytes(name: &str, data: Vec<u8>) -> Result<Opened, EngineError> {
    let kind = kind_by_extension(Path::new(name)).unwrap_or(if data.starts_with(b"!BDN") {
        Kind::Pst
    } else if data.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        Kind::Msg
    } else {
        Kind::Eml
    });
    if kind == Kind::Pst {
        let store = openmailclient_pst::PstStore::from_bytes_named(data, name).map_err(|e| EngineError::Format(e.to_string()))?;
        return Ok(Opened::Store(Box::new(store)));
    }
    if u64::try_from(data.len()).unwrap_or(u64::MAX) > MAX_ITEM_FILE {
        return Err(EngineError::Format("this file is too large to open as one message".into()));
    }
    let item = match kind {
        Kind::Eml => openmailclient_mime::parse(&data).map_err(|e| EngineError::Format(e.to_string()))?,
        _ => openmailclient_msg::read(&data).map_err(|e| EngineError::Format(e.to_string()))?,
    };
    Ok(Opened::Item(Box::new(item)))
}

/// Everything a store holds as the bytes of a new `.pst`, held against the structure check,
/// for a place that has no files: a download in a browser (M6.6). It is written from the
/// model, with the changes not yet saved, each folder's hidden items and own properties.
pub fn store_bytes(store: &mut dyn Store, id: [u8; 16], kind: PstKind) -> Result<(Vec<u8>, Saved), EngineError> {
    let out = std::io::Cursor::new(Vec::new());
    let written = match kind {
        PstKind::Ansi => openmailclient_pst::write_store_ansi(out, store, id, None),
        PstKind::Big => openmailclient_pst::write_store_4k(out, store, id, None),
        _ => openmailclient_pst::write_store_until(out, store, id, None),
    };
    let (mut out, done) = written.map_err(pst_error)?;
    out.set_position(0);
    let found = openmailclient_pst::check_until(&mut out, None).map_err(pst_error)?;
    if !found.sound() {
        let first = found.problems.first().or(found.inside.first()).map(String::as_str).unwrap_or("no detail");
        return Err(EngineError::Format(format!("the data file that was written did not pass its own check ({first})")));
    }
    let data = out.into_inner();
    let bytes = u64::try_from(data.len()).unwrap_or(u64::MAX);
    Ok((data, Saved { folders: done.folders, items: done.items, unreadable: done.unreadable, bytes, copied: false, not_copied: None }))
}

/// The bytes of an item in the format the path's extension names.
pub fn encode_item(item: &Item, path: &Path) -> Result<Vec<u8>, EngineError> {
    match kind_by_extension(path) {
        Some(Kind::Msg) => openmailclient_msg::write(item).map_err(|e| EngineError::Format(e.to_string())),
        Some(Kind::Template) => {
            openmailclient_msg::write_as(item, openmailclient_msg::Kind::Template).map_err(|e| EngineError::Format(e.to_string()))
        }
        Some(Kind::Eml) => Ok(openmailclient_mime::write(item)),
        Some(Kind::Pst) => {
            Err(EngineError::BadParams("one message is saved as .msg, .oft or .eml; a .pst takes a whole data file (file.export)".into()))
        }
        None => Err(EngineError::BadParams("the file name must end in .msg, .oft or .eml".into())),
    }
}

/// Write `data` to `path` so that a failure leaves whatever was there before intact: the bytes go
/// to a temporary file beside the target, which then replaces it in one step.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), EngineError> {
    let tmp = tmp_beside(path)?;
    let written = std::fs::write(&tmp, data).and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(written?)
}

/// The temporary file a write to `path` goes through: beside it, so the swap is one step.
fn tmp_beside(path: &Path) -> Result<PathBuf, EngineError> {
    let name = path.file_name().ok_or_else(|| EngineError::BadParams("the path has no file name".into()))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    Ok(path.with_file_name(tmp_name))
}

/// What `save_store` wrote.
pub struct Saved {
    pub folders: u64,
    pub items: u64,
    /// Items and folder lists the source could not give; they are not in the new file.
    pub unreadable: u64,
    pub bytes: u64,
    /// True when the source was a data file copied node for node: hidden items, a folder's
    /// own properties and properties unknown here went with it.
    pub copied: bool,
    /// Why the source could not be copied so, when it was tried and the new file holds
    /// what the model gives instead.
    pub not_copied: Option<String>,
}

/// An id for a new data file. Not a secret: only unlikely to be another file's.
pub fn new_store_id(now: i64) -> [u8; 16] {
    use std::hash::{BuildHasher, Hasher};
    let mut id = [0u8; 16];
    for (n, half) in id.chunks_mut(8).enumerate() {
        // Each `RandomState` is keyed from the system's random source.
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_i64(now);
        h.write_usize(n);
        for (to, from) in half.iter_mut().zip(h.finish().to_le_bytes()) {
            *to = from;
        }
    }
    id
}

fn pst_error(e: openmailclient_pst::PstError) -> EngineError {
    match e {
        openmailclient_pst::PstError::Io(e) => EngineError::Io(e),
        e => EngineError::Format(e.to_string()),
    }
}

/// Save everything a store holds as a new `.pst`. The store is only read. The new file is
/// written beside the target, held against the structure check, and only then put in the
/// target's place in one step: whatever goes wrong, a file that was there stays as it was.
pub fn save_store(store: &mut dyn Store, path: &Path, id: [u8; 16]) -> Result<Saved, EngineError> {
    save_store_as(store, path, id, PstKind::AsItIs)
}

/// The kind of `.pst` an export writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PstKind {
    /// The kind the source is when it is copied as it is stored, a Unicode file otherwise.
    AsItIs,
    Unicode,
    /// The kind older programs read ([MS-PST] 2.2.2.6, wVer 14), of at most 2 GB.
    Ansi,
    /// Pages of 4096 bytes and blocks of up to 64 KB (wVer 36), the kind newer versions of
    /// Outlook make; in no specification, measured on files Outlook wrote.
    Big,
}

impl PstKind {
    /// What `"kind"` names; nothing named is `AsItIs`.
    pub fn named(name: Option<&str>) -> Result<PstKind, EngineError> {
        match name {
            None => Ok(PstKind::AsItIs),
            Some("unicode") => Ok(PstKind::Unicode),
            Some("ansi") => Ok(PstKind::Ansi),
            Some("4k") => Ok(PstKind::Big),
            Some(other) => Err(EngineError::BadParams(format!("a data file is written as \"unicode\", \"ansi\" or \"4k\", not {other:?}"))),
        }
    }

    /// True when a copy of the store as it is stored would be of this kind. A copy keeps the
    /// kind of its source: its blocks are of that kind's size.
    fn is_kept_by_a_copy_of(self, store: &dyn Store) -> bool {
        if self == PstKind::AsItIs {
            return true;
        }
        // HEADER, wVer at 10 ([MS-PST] 2.2.2.6): 14 and 15 are ANSI files, 36 one with 4K
        // pages (measured).
        let mut head = [0u8; 12];
        let read = store.path().and_then(|p| std::fs::File::open(p).ok()).is_some_and(|mut f| f.read_exact(&mut head).is_ok());
        let source = match u16::from_le_bytes([head[10], head[11]]) {
            14 | 15 => PstKind::Ansi,
            36 => PstKind::Big,
            _ => PstKind::Unicode,
        };
        read && source == self
    }
}

/// `save_store`, as a data file of the kind asked for.
pub fn save_store_as(store: &mut dyn Store, path: &Path, id: [u8; 16], kind: PstKind) -> Result<Saved, EngineError> {
    if !path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pst")) {
        return Err(EngineError::BadParams("a data file is saved under a name ending in .pst".into()));
    }
    // The file being read cannot be the file being replaced.
    if let Some(from) = store.path().and_then(|p| std::fs::canonicalize(p).ok())
        && std::fs::canonicalize(path).is_ok_and(|to| to == from)
    {
        return Err(EngineError::BadParams("a data file cannot be saved over itself; choose another name".into()));
    }
    let tmp = tmp_beside(path)?;
    let saved = write_checked(store, &tmp, id, None, kind).and_then(|saved| Ok(std::fs::rename(&tmp, path).map(|()| saved)?));
    if saved.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    saved
}

/// The long part of `save_store` for a store that can be had apart from the caller: the
/// work, and the file beside the target it writes. The work takes the mailbox as it stood
/// when this was asked, writes it as `save_store` does (copied as it is stored when it can
/// be, from what is read otherwise) and checks it; what it gives goes to `export_end`.
/// `None` for a store that cannot: `save_store` is for those.
pub fn export_begin(store: &mut dyn Store, path: &Path, id: [u8; 16], kind: PstKind) -> Result<Option<(ExportWork, PathBuf)>, EngineError> {
    if cfg!(target_arch = "wasm32") {
        return Ok(None);
    }
    if !path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pst")) {
        return Err(EngineError::BadParams("a data file is saved under a name ending in .pst".into()));
    }
    if let Some(from) = store.path().and_then(|p| std::fs::canonicalize(p).ok())
        && std::fs::canonicalize(path).is_ok_and(|to| to == from)
    {
        return Err(EngineError::BadParams("a data file cannot be saved over itself; choose another name".into()));
    }
    let tmp = tmp_beside(path)?;
    let Some(apart) = store.apart()? else { return Ok(None) };
    let beside = tmp.clone();
    let work: ExportWork = Box::new(move |progress| {
        let saved = apart(progress.clone())
            .map_err(EngineError::from)
            .and_then(|mut store| write_checked(store.as_mut(), &beside, id, Some(&progress), kind));
        if saved.is_err() {
            let _ = std::fs::remove_file(&beside);
        }
        saved
    });
    Ok(Some((work, tmp)))
}

/// What `export_begin` gives to be run elsewhere.
pub type ExportWork = Box<dyn FnOnce(std::sync::Arc<openmailclient_model::Progress>) -> Result<Saved, EngineError> + Send>;

/// The short part: the file the work wrote takes the target's place.
pub fn export_end(tmp: &Path, path: &Path) -> Result<(), EngineError> {
    let placed = std::fs::rename(tmp, path);
    if placed.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    Ok(placed?)
}

impl Saved {
    /// What the status says of an export to `path`.
    pub fn says(&self, path: &Path) -> String {
        let mut says = if self.unreadable == 0 {
            format!("Exported {} items in {} folders to {}", self.items, self.folders, path.display())
        } else {
            format!(
                "Exported {} items in {} folders to {}; {} could not be read and are not in the file",
                self.items,
                self.folders,
                path.display(),
                self.unreadable
            )
        };
        if self.copied {
            says.push_str(", as it is stored");
        } else if let Some(why) = &self.not_copied {
            says.push_str(&format!(
                "; it could not be copied as it is stored ({why}), so what lies outside its folders (search folders, say) is not in it"
            ));
        }
        says
    }
}

fn write_checked(
    store: &mut dyn Store,
    tmp: &Path,
    id: [u8; 16],
    progress: Option<&openmailclient_model::Progress>,
    kind: PstKind,
) -> Result<Saved, EngineError> {
    // A data file is copied node for node when it can be. When that fails, or what comes of
    // it does not pass the check (the source has damage), the model's way is taken: it
    // leaves out what cannot be read and says how much. A copy is of the kind its source
    // is, so a file asked for as the other kind is written the model's way from the start.
    let copy = if kind.is_kept_by_a_copy_of(store) { copy_checked(store, tmp, id, progress) } else { Ok(None) };
    let not_copied = match copy {
        Ok(Some(saved)) => return Ok(saved),
        Ok(None) => None,
        // A copy that was stopped is not tried another way.
        Err(e) if progress.is_some_and(|p| p.stopped()) => return Err(e),
        Err(e) => Some(e.to_string()),
    };
    let file = std::fs::File::options().read(true).write(true).create(true).truncate(true).open(tmp)?;
    let written = match kind {
        PstKind::Ansi => openmailclient_pst::write_store_ansi(file, store, id, progress),
        PstKind::Big => openmailclient_pst::write_store_4k(file, store, id, progress),
        _ => openmailclient_pst::write_store_until(file, store, id, progress),
    };
    let (mut file, done) = written.map_err(pst_error)?;
    file.sync_all()?;
    let bytes = file.seek(SeekFrom::End(0))?;
    file.seek(SeekFrom::Start(0))?;
    let found = openmailclient_pst::check_until(&mut std::io::BufReader::new(&mut file), progress).map_err(pst_error)?;
    if let Some(p) = progress {
        p.part(2);
    }
    if !found.sound() {
        let first = found.problems.first().or(found.inside.first()).map(String::as_str).unwrap_or("no detail");
        return Err(EngineError::Format(format!("the data file that was written did not pass its own check ({first}); nothing was replaced")));
    }
    Ok(Saved { folders: done.folders, items: done.items, unreadable: done.unreadable, bytes, copied: false, not_copied })
}

fn copy_checked(
    store: &mut dyn Store,
    tmp: &Path,
    id: [u8; 16],
    stop: Option<&openmailclient_model::Progress>,
) -> Result<Option<Saved>, EngineError> {
    let Some(done) = store.copy_to(tmp, id)? else { return Ok(None) };
    copied(tmp, done, stop).map(Some)
}

/// Hold a copy that was written to `tmp` against the structure check.
fn copied(tmp: &Path, done: openmailclient_model::Copied, stop: Option<&openmailclient_model::Progress>) -> Result<Saved, EngineError> {
    let mut file = std::fs::File::open(tmp)?;
    let bytes = file.seek(SeekFrom::End(0))?;
    file.seek(SeekFrom::Start(0))?;
    let found = openmailclient_pst::check_until(&mut std::io::BufReader::new(&mut file), stop).map_err(pst_error)?;
    if !found.sound() {
        let first = found.problems.first().or(found.inside.first()).map(String::as_str).unwrap_or("no detail");
        return Err(EngineError::Format(format!("the copy did not pass its own check ({first})")));
    }
    Ok(Saved { folders: done.folders, items: done.items, unreadable: 0, bytes, copied: true, not_copied: None })
}

pub fn save_item(item: &Item, path: &Path) -> Result<(), EngineError> {
    let data = encode_item(item, path)?;
    write_atomic(path, &data)
}
