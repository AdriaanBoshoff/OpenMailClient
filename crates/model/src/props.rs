//! Typed properties ([MS-OXCDATA] 2.11.1) and the property ids OpenMailClient reads by name ([MS-OXPROPS]).

use std::collections::BTreeMap;

/// Property ids ([MS-OXPROPS]). Only the ones the code names; every other id is still carried.
pub mod pid {
    /// When a message held in the Outbox may go (Delay Delivery) ([MS-OXPROPS] PidTagDeferredDeliveryTime).
    pub const DEFERRED_DELIVERY_TIME: u16 = 0x000F;
    /// When a message stops being of use ([MS-OXPROPS] PidTagExpiryTime).
    pub const EXPIRY_TIME: u16 = 0x0015;
    pub const IMPORTANCE: u16 = 0x0017;
    /// The automatic answers a message asks not to be sent to it ([MS-OXPROPS]
    /// PidTagAutoResponseSuppress, [MS-OXCMAIL] 2.2.3.2.20); set on what a rule writes.
    pub const AUTO_RESPONSE_SUPPRESS: u16 = 0x3FDF;
    pub const MESSAGE_CLASS: u16 = 0x001A;
    pub const SUBJECT: u16 = 0x0037;
    pub const CLIENT_SUBMIT_TIME: u16 = 0x0039;
    pub const SENT_REPRESENTING_NAME: u16 = 0x0042;
    pub const SENT_REPRESENTING_ADDRESS_TYPE: u16 = 0x0064;
    pub const SENT_REPRESENTING_EMAIL_ADDRESS: u16 = 0x0065;
    pub const CONVERSATION_TOPIC: u16 = 0x0070;
    pub const TRANSPORT_MESSAGE_HEADERS: u16 = 0x007D;
    pub const RECIPIENT_TYPE: u16 = 0x0C15;
    pub const SENDER_NAME: u16 = 0x0C1A;
    pub const SENDER_ADDRESS_TYPE: u16 = 0x0C1E;
    pub const SENDER_EMAIL_ADDRESS: u16 = 0x0C1F;
    pub const DISPLAY_BCC: u16 = 0x0E02;
    pub const DISPLAY_CC: u16 = 0x0E03;
    pub const DISPLAY_TO: u16 = 0x0E04;
    pub const MESSAGE_DELIVERY_TIME: u16 = 0x0E06;
    pub const MESSAGE_FLAGS: u16 = 0x0E07;
    pub const MESSAGE_SIZE: u16 = 0x0E08;
    pub const HASATTACH: u16 = 0x0E1B;
    pub const NORMALIZED_SUBJECT: u16 = 0x0E1D;
    pub const RTF_IN_SYNC: u16 = 0x0E1F;
    pub const ATTACH_SIZE: u16 = 0x0E20;
    pub const ATTACH_NUM: u16 = 0x0E21;
    pub const BODY: u16 = 0x1000;
    /// PidTagPreview ([MS-OXPROPS] 2.871): the start of the body as Outlook keeps it for its list.
    pub const PREVIEW: u16 = 0x3FD9;
    pub const RTF_COMPRESSED: u16 = 0x1009;
    pub const HTML: u16 = 0x1013;
    pub const INTERNET_MESSAGE_ID: u16 = 0x1035;
    pub const IN_REPLY_TO_ID: u16 = 0x1042;
    pub const INTERNET_REFERENCES: u16 = 0x1039;
    pub const FLAG_STATUS: u16 = 0x1090;
    pub const ROWID: u16 = 0x3000;
    pub const DISPLAY_NAME: u16 = 0x3001;
    pub const ADDRESS_TYPE: u16 = 0x3002;
    pub const EMAIL_ADDRESS: u16 = 0x3003;
    pub const CREATION_TIME: u16 = 0x3007;
    pub const LAST_MODIFICATION_TIME: u16 = 0x3008;
    pub const STORE_SUPPORT_MASK: u16 = 0x340D;
    pub const IPM_SUBTREE_ENTRY_ID: u16 = 0x35E0;
    pub const CONTENT_COUNT: u16 = 0x3602;
    pub const CONTENT_UNREAD_COUNT: u16 = 0x3603;
    pub const SUBFOLDERS: u16 = 0x360A;
    pub const CONTAINER_CLASS: u16 = 0x3613;
    /// PidTagAttributeHidden: a folder or item not shown ([MS-OXPROPS] 2.609).
    pub const ATTR_HIDDEN: u16 = 0x10F4;
    pub const ATTACH_DATA: u16 = 0x3701;
    pub const ATTACH_EXTENSION: u16 = 0x3703;
    pub const ATTACH_FILENAME: u16 = 0x3704;
    pub const ATTACH_METHOD: u16 = 0x3705;
    pub const ATTACH_LONG_FILENAME: u16 = 0x3707;
    pub const ATTACH_RENDERING: u16 = 0x3709;
    pub const RENDERING_POSITION: u16 = 0x370B;
    pub const ATTACH_MIME_TAG: u16 = 0x370E;
    pub const ATTACH_CONTENT_ID: u16 = 0x3712;
    pub const SMTP_ADDRESS: u16 = 0x39FE;
    pub const ATTACHMENT_HIDDEN: u16 = 0x7FFE;
    pub const INTERNET_CODEPAGE: u16 = 0x3FDE;
    pub const MESSAGE_CODEPAGE: u16 = 0x3FFD;
    pub const SENDER_SMTP_ADDRESS: u16 = 0x5D01;
    pub const SENT_REPRESENTING_SMTP_ADDRESS: u16 = 0x5D02;
}

/// Property set GUIDs ([MS-OXPROPS] 1.3.2), in their on-disk byte order.
pub mod pset {
    pub const PS_MAPI: [u8; 16] = [0x28, 0x03, 0x02, 0x00, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46];
    pub const PS_PUBLIC_STRINGS: [u8; 16] = [0x29, 0x03, 0x02, 0x00, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46];
    pub const PSETID_COMMON: [u8; 16] = [0x08, 0x20, 0x06, 0x00, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46];
    pub const PS_INTERNET_HEADERS: [u8; 16] = [0x86, 0x03, 0x02, 0x00, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46];
}

/// The name half of a named property ([MS-OXCDATA] 2.6.1): a number or a string.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NameId {
    Id(u32),
    Name(String),
}

/// A named property's identity. Its 0x8000+ property id differs per store, so named properties are
/// carried by name and given ids again when a file is written.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NamedKey {
    pub guid: [u8; 16],
    pub name: NameId,
}

impl NamedKey {
    /// PidNameKeywords: the categories of an item.
    pub fn keywords() -> Self {
        Self { guid: pset::PS_PUBLIC_STRINGS, name: NameId::Name("Keywords".to_string()) }
    }
}

/// Property types ([MS-OXCDATA] 2.11.1).
pub mod ptype {
    pub const INT16: u16 = 0x0002;
    pub const INT32: u16 = 0x0003;
    pub const FLOAT32: u16 = 0x0004;
    pub const FLOAT64: u16 = 0x0005;
    pub const CURRENCY: u16 = 0x0006;
    pub const FLOATTIME: u16 = 0x0007;
    pub const ERROR: u16 = 0x000A;
    pub const BOOL: u16 = 0x000B;
    pub const OBJECT: u16 = 0x000D;
    pub const INT64: u16 = 0x0014;
    pub const STRING8: u16 = 0x001E;
    pub const STRING: u16 = 0x001F;
    pub const TIME: u16 = 0x0040;
    pub const GUID: u16 = 0x0048;
    pub const BINARY: u16 = 0x0102;
    pub const MULTI_INT32: u16 = 0x1003;
    pub const MULTI_STRING8: u16 = 0x101E;
    pub const MULTI_STRING: u16 = 0x101F;
    pub const MULTI_BINARY: u16 = 0x1102;

    /// Bytes a fixed-size value takes, or `None` for variable-size and multi-valued types.
    pub fn fixed_size(ty: u16) -> Option<usize> {
        match ty {
            INT16 | BOOL => Some(2),
            INT32 | FLOAT32 | ERROR => Some(4),
            FLOAT64 | CURRENCY | FLOATTIME | INT64 | TIME => Some(8),
            GUID => Some(16),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PropValue {
    Int16(i16),
    Int32(i32),
    Int64(i64),
    Bool(bool),
    Float64(f64),
    /// A 32-bit float, kept as one so that it is written as it was read.
    Float32(f32),
    /// Currency: a count of ten-thousandths ([MS-OXCDATA] 2.11.1).
    Currency(i64),
    /// A time as days since the end of 1899, with the time of day as a fraction.
    FloatTime(f64),
    /// An error code standing where a value could not be given.
    Error(i32),
    String(String),
    Binary(Vec<u8>),
    /// UTC FILETIME ticks: 100 ns since 1601-01-01.
    Time(i64),
    Guid([u8; 16]),
    MultiInt32(Vec<i32>),
    MultiString(Vec<String>),
    MultiBinary(Vec<Vec<u8>>),
    /// A type this code does not interpret, kept as the bytes the file had.
    Other {
        ty: u16,
        raw: Vec<u8>,
    },
}

impl PropValue {
    pub fn ty(&self) -> u16 {
        match self {
            Self::Int16(_) => ptype::INT16,
            Self::Int32(_) => ptype::INT32,
            Self::Int64(_) => ptype::INT64,
            Self::Bool(_) => ptype::BOOL,
            Self::Float64(_) => ptype::FLOAT64,
            Self::Float32(_) => ptype::FLOAT32,
            Self::Currency(_) => ptype::CURRENCY,
            Self::FloatTime(_) => ptype::FLOATTIME,
            Self::Error(_) => ptype::ERROR,
            Self::String(_) => ptype::STRING,
            Self::Binary(_) => ptype::BINARY,
            Self::Time(_) => ptype::TIME,
            Self::Guid(_) => ptype::GUID,
            Self::MultiInt32(_) => ptype::MULTI_INT32,
            Self::MultiString(_) => ptype::MULTI_STRING,
            Self::MultiBinary(_) => ptype::MULTI_BINARY,
            Self::Other { ty, .. } => *ty,
        }
    }

    /// Decode a fixed-size value from its little-endian bytes ([MS-OXCDATA] 2.11.1).
    pub fn from_fixed(ty: u16, b: &[u8]) -> Option<Self> {
        let le32 = |b: &[u8]| b.get(..4).and_then(|s| <[u8; 4]>::try_from(s).ok());
        let le64 = |b: &[u8]| b.get(..8).and_then(|s| <[u8; 8]>::try_from(s).ok());
        Some(match ty {
            ptype::INT16 => Self::Int16(i16::from_le_bytes(b.get(..2)?.try_into().ok()?)),
            ptype::BOOL => Self::Bool(*b.first()? != 0),
            ptype::INT32 => Self::Int32(i32::from_le_bytes(le32(b)?)),
            ptype::ERROR => Self::Error(i32::from_le_bytes(le32(b)?)),
            ptype::FLOAT32 => Self::Float32(f32::from_le_bytes(le32(b)?)),
            ptype::FLOAT64 => Self::Float64(f64::from_le_bytes(le64(b)?)),
            ptype::FLOATTIME => Self::FloatTime(f64::from_le_bytes(le64(b)?)),
            ptype::INT64 => Self::Int64(i64::from_le_bytes(le64(b)?)),
            ptype::CURRENCY => Self::Currency(i64::from_le_bytes(le64(b)?)),
            ptype::TIME => Self::Time(i64::from_le_bytes(le64(b)?)),
            ptype::GUID => Self::Guid(b.get(..16)?.try_into().ok()?),
            _ => return None,
        })
    }

    /// The little-endian bytes of a fixed-size value, padded to 8 bytes; `None` for the rest.
    pub fn fixed_bytes(&self) -> Option<[u8; 8]> {
        let mut out = [0u8; 8];
        match self {
            Self::Int16(v) => out.get_mut(..2)?.copy_from_slice(&v.to_le_bytes()),
            Self::Int32(v) | Self::Error(v) => out.get_mut(..4)?.copy_from_slice(&v.to_le_bytes()),
            Self::Float32(v) => out.get_mut(..4)?.copy_from_slice(&v.to_le_bytes()),
            Self::Bool(v) => out[0] = u8::from(*v),
            Self::Int64(v) | Self::Time(v) | Self::Currency(v) => out = v.to_le_bytes(),
            Self::Float64(v) | Self::FloatTime(v) => out = v.to_le_bytes(),
            _ => return None,
        }
        Some(out)
    }
}

/// An item's properties, by property id. One value per id, as in a MAPI object.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropBag(pub BTreeMap<u16, PropValue>);

impl PropBag {
    pub fn get(&self, id: u16) -> Option<&PropValue> {
        self.0.get(&id)
    }

    pub fn set(&mut self, id: u16, v: PropValue) {
        self.0.insert(id, v);
    }

    pub fn remove(&mut self, id: u16) {
        self.0.remove(&id);
    }

    pub fn set_str(&mut self, id: u16, s: &str) {
        self.0.insert(id, PropValue::String(s.to_string()));
    }

    pub fn str(&self, id: u16) -> Option<&str> {
        match self.0.get(&id) {
            Some(PropValue::String(s)) => Some(s),
            _ => None,
        }
    }

    pub fn i32(&self, id: u16) -> Option<i32> {
        match self.0.get(&id) {
            Some(PropValue::Int32(v)) => Some(*v),
            Some(PropValue::Int16(v)) => Some(i32::from(*v)),
            Some(PropValue::Bool(v)) => Some(i32::from(*v)),
            _ => None,
        }
    }

    pub fn bool(&self, id: u16) -> Option<bool> {
        match self.0.get(&id) {
            Some(PropValue::Bool(v)) => Some(*v),
            Some(PropValue::Int32(v)) => Some(*v != 0),
            _ => None,
        }
    }

    pub fn time(&self, id: u16) -> Option<i64> {
        match self.0.get(&id) {
            Some(PropValue::Time(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn bin(&self, id: u16) -> Option<&[u8]> {
        match self.0.get(&id) {
            Some(PropValue::Binary(v)) => Some(v),
            _ => None,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (u16, &PropValue)> {
        self.0.iter().map(|(k, v)| (*k, v))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Decode a named property map: the GUID, entry and string streams that `.msg` files
/// ([MS-OXMSG] 2.2.3.1) and `.pst` files ([MS-PST] 2.4.7) share. Slot n is property id 0x8000 + n.
pub fn parse_name_map(guids: &[u8], entries: &[u8], strings: &[u8]) -> Vec<Option<NamedKey>> {
    let u32_at = |b: &[u8], at: usize| Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?));
    let u16_at = |b: &[u8], at: usize| Some(u16::from_le_bytes(b.get(at..at.checked_add(2)?)?.try_into().ok()?));
    let mut out: Vec<Option<NamedKey>> = Vec::new();
    for e in entries.chunks_exact(8).take(0x7FFF) {
        let (Some(name_or_offset), Some(kind), Some(index)) = (u32_at(e, 0), u16_at(e, 4), u16_at(e, 6)) else { continue };
        let guid = match kind >> 1 {
            1 => Some(pset::PS_MAPI),
            2 => Some(pset::PS_PUBLIC_STRINGS),
            n => usize::from(n)
                .checked_sub(3)
                .and_then(|i| guids.get(i.checked_mul(16)?..i.checked_mul(16)?.checked_add(16)?))
                .and_then(|g| <[u8; 16]>::try_from(g).ok()),
        };
        let name = if kind & 1 == 0 {
            Some(NameId::Id(name_or_offset))
        } else {
            let at = name_or_offset as usize;
            u32_at(strings, at)
                .and_then(|len| strings.get(at.checked_add(4)?..at.checked_add(4)?.checked_add(len as usize)?))
                .map(|b| NameId::Name(crate::codepage::utf16le(b)))
        };
        let slot = usize::from(index);
        if out.len() <= slot {
            out.resize(slot + 1, None);
        }
        if let (Some(guid), Some(name), Some(cell)) = (guid, name, out.get_mut(slot)) {
            *cell = Some(NamedKey { guid, name });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_values_round_trip() {
        for v in [
            PropValue::Int32(-7),
            PropValue::Bool(true),
            PropValue::Time(133_000_000_000_000_000),
            PropValue::Int16(-2),
            PropValue::Int64(1 << 40),
            PropValue::Float64(-0.1),
            // The kinds that share their bytes with another keep their own type.
            PropValue::Float32(1.5),
            PropValue::Currency(12_345_600),
            PropValue::FloatTime(45_000.25),
            PropValue::Error(0x8004_010Fu32.cast_signed()),
        ] {
            let b = v.fixed_bytes().unwrap();
            assert_eq!(PropValue::from_fixed(v.ty(), &b), Some(v));
        }
    }

    #[test]
    fn short_input_is_none_not_a_panic() {
        assert_eq!(PropValue::from_fixed(ptype::INT32, &[1, 2]), None);
        assert_eq!(PropValue::from_fixed(ptype::GUID, &[0; 8]), None);
        assert_eq!(PropValue::from_fixed(ptype::BOOL, &[]), None);
    }
}
