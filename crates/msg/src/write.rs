//! Writing a `.msg` / `.oft`.

use crate::{
    ATTACH_PREFIX, EMBEDDED_STORAGE, HEADER_CHILD, HEADER_EMBEDDED, HEADER_TOP, MsgError, NAMEID_STORAGE, PROPS_STREAM, RECIP_PREFIX, crc32,
    value_stream,
};
use openmailclient_model::{Item, MSGFLAG_HASATTACH, NameId, NamedKey, PropBag, PropValue, codepage, pid, pset, ptype};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

/// PROPATTR_READABLE | PROPATTR_WRITABLE ([MS-OXMSG] 2.4.2).
const FLAGS: u32 = 0x6;
/// STORE_UNICODE_OK: every string in the file is Unicode ([MS-OXMSG] 2.1.1.1).
const STORE_UNICODE_OK: i32 = 0x0004_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A saved message (`.msg`).
    Message,
    /// An Outlook template (`.oft`).
    Template,
}

type Cf = cfb::CompoundFile<Cursor<Vec<u8>>>;

/// Write a `.msg`.
pub fn write(item: &Item) -> Result<Vec<u8>, MsgError> {
    write_as(item, Kind::Message)
}

pub fn write_as(item: &Item, kind: Kind) -> Result<Vec<u8>, MsgError> {
    // Version 3 (512-byte sectors) is what Outlook itself writes.
    let mut cf = cfb::CompoundFile::create_with_version(cfb::Version::V3, Cursor::new(Vec::new()))?;
    let clsid = match kind {
        Kind::Message => 0x0002_0D0B_0000_0000_C000_0000_0000_0046u128,
        Kind::Template => 0x0006_F046_0000_0000_C000_0000_0000_0046u128,
    };
    cf.set_storage_clsid("/", uuid::Uuid::from_u128(clsid))?;
    let mut names = Names::default();
    write_message(&mut cf, Path::new("/"), item, true, &mut names)?;
    names.write(&mut cf)?;
    cf.flush()?;
    Ok(cf.into_inner().into_inner())
}

fn put(cf: &mut Cf, path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut s = cf.create_stream(path)?;
    s.write_all(data)?;
    s.flush()
}

/// Named properties get ids 0x8000, 0x8001, … in order of first use ([MS-OXMSG] 2.2.3).
#[derive(Default)]
struct Names {
    keys: Vec<NamedKey>,
}

impl Names {
    fn id_for(&mut self, key: &NamedKey) -> Option<u16> {
        let index = match self.keys.iter().position(|k| k == key) {
            Some(i) => i,
            None => {
                if self.keys.len() >= 0x7FFE {
                    return None;
                }
                self.keys.push(key.clone());
                self.keys.len() - 1
            }
        };
        u16::try_from(index).ok().map(|i| 0x8000 + i)
    }

    fn write(&self, cf: &mut Cf) -> std::io::Result<()> {
        let base = PathBuf::from("/").join(NAMEID_STORAGE);
        cf.create_storage(&base)?;
        let mut guids: Vec<[u8; 16]> = Vec::new();
        let (mut entries, mut strings) = (Vec::new(), Vec::new());
        let mut buckets: std::collections::BTreeMap<u32, Vec<u8>> = std::collections::BTreeMap::new();
        for (index, key) in self.keys.iter().enumerate() {
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
            let (first, kind, hash_id) = match &key.name {
                NameId::Id(n) => (*n, 0u32, *n),
                NameId::Name(s) => {
                    let utf16 = codepage::to_utf16le(s);
                    let offset = u32::try_from(strings.len()).unwrap_or(u32::MAX);
                    strings.extend_from_slice(&u32::try_from(utf16.len()).unwrap_or(0).to_le_bytes());
                    strings.extend_from_slice(&utf16);
                    while strings.len() % 4 != 0 {
                        strings.push(0);
                    }
                    (offset, 1u32, crc32(&utf16))
                }
            };
            let guid_kind = (guid_index << 1 | kind) as u16;
            let prop_index = u16::try_from(index).unwrap_or(u16::MAX);
            let mut entry = first.to_le_bytes().to_vec();
            entry.extend_from_slice(&guid_kind.to_le_bytes());
            entry.extend_from_slice(&prop_index.to_le_bytes());
            entries.extend_from_slice(&entry);
            // The reverse lookup streams: 0x1000 + (id XOR (guid << 1 | kind)) mod 0x1F ([MS-OXMSG] 2.2.3.2).
            let bucket = 0x1000 + (hash_id ^ u32::from(guid_kind)) % 0x1F;
            let mut hashed = hash_id.to_le_bytes().to_vec();
            hashed.extend_from_slice(entry.get(4..).unwrap_or(&[]));
            buckets.entry(bucket).or_default().extend_from_slice(&hashed);
        }
        put(cf, &base.join(value_stream(0x0002, ptype::BINARY)), &guids.concat())?;
        put(cf, &base.join(value_stream(0x0003, ptype::BINARY)), &entries)?;
        put(cf, &base.join(value_stream(0x0004, ptype::BINARY)), &strings)?;
        for (bucket, data) in buckets {
            put(cf, &base.join(value_stream(bucket as u16, ptype::BINARY)), &data)?;
        }
        Ok(())
    }
}

/// Write one object's property stream and value streams ([MS-OXMSG] 2.4).
fn write_props(cf: &mut Cf, base: &Path, header: &[u8], props: &[(u16, PropValue)]) -> std::io::Result<()> {
    let mut stream = header.to_vec();
    for (id, v) in props {
        let ty = v.ty();
        let mut entry = (u32::from(*id) << 16 | u32::from(ty)).to_le_bytes().to_vec();
        entry.extend_from_slice(&FLAGS.to_le_bytes());
        let name = value_stream(*id, ty);
        let variable = |cf: &mut Cf, data: &[u8], size: usize| -> std::io::Result<[u8; 8]> {
            put(cf, &base.join(&name), data)?;
            let mut value = [0u8; 8];
            value[..4].copy_from_slice(&u32::try_from(size).unwrap_or(u32::MAX).to_le_bytes());
            Ok(value)
        };
        let value: [u8; 8] = match v {
            PropValue::String(s) => {
                // The size counts the terminating null that the stream itself leaves out.
                let data = codepage::to_utf16le(s);
                variable(cf, &data, data.len() + 2)?
            }
            PropValue::Binary(b) => variable(cf, b, b.len())?,
            PropValue::Guid(g) => variable(cf, g, 16)?,
            PropValue::MultiInt32(list) => {
                let data: Vec<u8> = list.iter().flat_map(|n| n.to_le_bytes()).collect();
                variable(cf, &data, data.len())?
            }
            PropValue::MultiString(list) => {
                let values: Vec<Vec<u8>> = list.iter().map(|s| codepage::to_utf16le(&format!("{s}\0"))).collect();
                let lengths: Vec<u8> = values.iter().flat_map(|v| u32::try_from(v.len()).unwrap_or(0).to_le_bytes()).collect();
                for (i, value) in values.iter().enumerate() {
                    put(cf, &base.join(format!("{name}-{i:08X}")), value)?;
                }
                variable(cf, &lengths, lengths.len())?
            }
            PropValue::MultiBinary(list) => {
                let lengths: Vec<u8> = list.iter().flat_map(|v| u64::from(u32::try_from(v.len()).unwrap_or(0)).to_le_bytes()).collect();
                for (i, value) in list.iter().enumerate() {
                    put(cf, &base.join(format!("{name}-{i:08X}")), value)?;
                }
                variable(cf, &lengths, lengths.len())?
            }
            PropValue::Other { ty, raw } => match ptype::fixed_size(*ty).filter(|n| *n <= 8).or((*ty == ptype::OBJECT).then_some(8)) {
                // An object entry is written in place: it has no value stream of its own.
                Some(_) => {
                    let mut value = [0u8; 8];
                    for (slot, b) in value.iter_mut().zip(raw) {
                        *slot = *b;
                    }
                    value
                }
                None => variable(cf, raw, raw.len())?,
            },
            fixed => fixed.fixed_bytes().unwrap_or_default(),
        };
        entry.extend_from_slice(&value);
        stream.extend_from_slice(&entry);
    }
    put(cf, &base.join(PROPS_STREAM), &stream)
}

fn bag(props: &PropBag) -> Vec<(u16, PropValue)> {
    // 0x8000 and up are another store's named property ids; ours are assigned from `Names`.
    props.iter().filter(|(id, _)| *id < 0x8000).map(|(id, v)| (id, v.clone())).collect()
}

fn set(list: &mut Vec<(u16, PropValue)>, id: u16, v: PropValue) {
    list.retain(|(i, _)| *i != id);
    list.push((id, v));
}

/// An attached object's bytes as the compound file they should be; `None` when they are not one.
fn object(data: &[u8]) -> Option<cfb::CompoundFile<Cursor<&[u8]>>> {
    // The container library is not ours; a panic inside it on damaged bytes must not escape.
    std::panic::catch_unwind(|| cfb::CompoundFile::open(Cursor::new(data)).ok()).ok().flatten()
}

fn write_message(cf: &mut Cf, base: &Path, item: &Item, top: bool, names: &mut Names) -> std::io::Result<()> {
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX).to_le_bytes();
    let mut header = vec![0u8; 8];
    for n in [item.recipients.len(), item.attachments.len(), item.recipients.len(), item.attachments.len()] {
        header.extend_from_slice(&count(n));
    }
    if top {
        header.extend_from_slice(&[0u8; 8]);
    }
    debug_assert_eq!(header.len(), if top { HEADER_TOP } else { HEADER_EMBEDDED });

    let mut props = bag(&item.props);
    if item.props.get(pid::MESSAGE_CLASS).is_none() {
        set(&mut props, pid::MESSAGE_CLASS, PropValue::String("IPM.Note".into()));
    }
    let flags = if item.attachments.is_empty() { item.flags() & !MSGFLAG_HASATTACH } else { item.flags() | MSGFLAG_HASATTACH };
    set(&mut props, pid::MESSAGE_FLAGS, PropValue::Int32(flags));
    set(&mut props, pid::HASATTACH, PropValue::Bool(!item.attachments.is_empty()));
    set(&mut props, pid::STORE_SUPPORT_MASK, PropValue::Int32(STORE_UNICODE_OK));
    if !item.categories.is_empty()
        && let Some(id) = names.id_for(&NamedKey::keywords())
    {
        props.push((id, PropValue::MultiString(item.categories.clone())));
    }
    for (key, v) in &item.named {
        if let Some(id) = names.id_for(key) {
            set(&mut props, id, v.clone());
        }
    }
    props.sort_by_key(|(id, _)| *id);
    write_props(cf, base, &header, &props)?;

    for (i, r) in item.recipients.iter().enumerate() {
        let dir = base.join(format!("{RECIP_PREFIX}{i:08X}"));
        cf.create_storage(&dir)?;
        let mut props = bag(r);
        set(&mut props, pid::ROWID, PropValue::Int32(i32::try_from(i).unwrap_or(i32::MAX)));
        props.sort_by_key(|(id, _)| *id);
        write_props(cf, &dir, &[0u8; HEADER_CHILD], &props)?;
    }
    for (i, a) in item.attachments.iter().enumerate() {
        let dir = base.join(format!("{ATTACH_PREFIX}{i:08X}"));
        cf.create_storage(&dir)?;
        let mut props = bag(&a.props);
        set(&mut props, pid::ATTACH_NUM, PropValue::Int32(i32::try_from(i).unwrap_or(i32::MAX)));
        if a.props.get(pid::RENDERING_POSITION).is_none() {
            set(&mut props, pid::RENDERING_POSITION, PropValue::Int32(-1));
        }
        props.retain(|(id, _)| *id != pid::ATTACH_DATA);
        // An object of another program goes back as the storage it was ([MS-OXMSG] 2.2.2.2).
        let object = (a.embedded.is_none() && a.props.i32(pid::ATTACH_METHOD) == Some(6)).then(|| object(&a.data)).flatten();
        match (&a.embedded, object) {
            (None, Some(mut from)) => {
                // As for an attached message, but the object kind is 4 ([MS-OXMSG] 2.4.2.2).
                let mut raw = 0xFFFF_FFFFu32.to_le_bytes().to_vec();
                raw.extend_from_slice(&4u32.to_le_bytes());
                props.push((pid::ATTACH_DATA, PropValue::Other { ty: ptype::OBJECT, raw }));
                let sub = dir.join(EMBEDDED_STORAGE);
                cf.create_storage(&sub)?;
                crate::ole::copy(&mut from, Path::new("/"), cf, &sub, &mut crate::ole::Budget::new(), 0)?;
            }
            (Some(inner), _) => {
                // An attached message is a storage; its entry carries 0xFFFFFFFF and the object
                // kind 1 in place of a size ([MS-OXMSG] 2.4.2.2).
                set(&mut props, pid::ATTACH_METHOD, PropValue::Int32(5));
                let mut raw = 0xFFFF_FFFFu32.to_le_bytes().to_vec();
                raw.extend_from_slice(&1u32.to_le_bytes());
                props.push((pid::ATTACH_DATA, PropValue::Other { ty: ptype::OBJECT, raw }));
                let sub = dir.join(EMBEDDED_STORAGE);
                cf.create_storage(&sub)?;
                write_message(cf, &sub, inner, false, names)?;
            }
            (None, None) => {
                // An object whose bytes are no storage goes as the file it is: Outlook lists
                // an object that is not a storage as no attachment.
                if a.props.i32(pid::ATTACH_METHOD) == Some(6) {
                    set(&mut props, pid::ATTACH_METHOD, PropValue::Int32(1));
                }
                set(&mut props, pid::ATTACH_SIZE, PropValue::Int32(i32::try_from(a.data.len()).unwrap_or(i32::MAX)));
                props.push((pid::ATTACH_DATA, PropValue::Binary(a.data.clone())));
            }
        }
        props.sort_by_key(|(id, _)| *id);
        write_props(cf, &dir, &[0u8; HEADER_CHILD], &props)?;
    }
    Ok(())
}
