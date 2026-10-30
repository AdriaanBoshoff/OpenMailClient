//! TNEF, the `winmail.dat` Outlook sends an item's own properties in ([MS-OXTNEF]).
//!
//! Read: a stream becomes an item with its properties, named properties, recipients and
//! attachments, an attached item among them read as a stream of its own. Every length, count and
//! depth comes from the sender and is checked against what is there, capped and bounded.
//! Written: an item becomes a stream as Outlook reads one, its properties in `attMsgProps` and
//! each attachment's in `attAttachment` ([MS-OXTNEF] 2.1.3.4).

use openmailclient_model::{Attachment, Item, NameId, NamedKey, PropBag, PropValue, codepage, pid, ptype, time};

/// The stream's signature, 0x223E9F78 ([MS-OXTNEF] 2.1.3.2).
const SIGNATURE: [u8; 4] = [0x78, 0x9F, 0x3E, 0x22];
/// Attached items inside attached items deeper than this are kept as data.
const MAX_DEPTH: usize = 8;
/// Properties in one list, rows in a table, values in one property and attachments beyond these
/// are not read.
const MAX_PROPS: usize = 4_096;
const MAX_ROWS: usize = 2_048;
const MAX_VALUES: usize = 4_096;
const MAX_ATTACHMENTS: usize = 1_024;

/// Levels and attribute ids ([MS-OXTNEF] 2.1.3.2), as the stream has them, little-endian.
const LEVEL_MESSAGE: u8 = 1;
const LEVEL_ATTACHMENT: u8 = 2;
mod att {
    pub const TNEF_VERSION: u32 = 0x0008_9006;
    pub const OEM_CODEPAGE: u32 = 0x0006_9007;
    pub const MESSAGE_CLASS: u32 = 0x0007_8008;
    pub const SUBJECT: u32 = 0x0001_8004;
    pub const DATE_SENT: u32 = 0x0003_8005;
    pub const DATE_RECEIVED: u32 = 0x0003_8006;
    pub const MESSAGE_STATUS: u32 = 0x0006_8007;
    pub const BODY: u32 = 0x0002_800C;
    pub const PRIORITY: u32 = 0x0004_800D;
    pub const DATE_MODIFIED: u32 = 0x0003_8020;
    pub const MSG_PROPS: u32 = 0x0006_9003;
    pub const RECIP_TABLE: u32 = 0x0006_9004;
    pub const ATTACH_DATA: u32 = 0x0006_800F;
    pub const ATTACH_TITLE: u32 = 0x0001_8010;
    pub const ATTACH_CREATE_DATE: u32 = 0x0003_8012;
    pub const ATTACH_MODIFY_DATE: u32 = 0x0003_8013;
    pub const ATTACH_TRANSPORT_FILENAME: u32 = 0x0006_9001;
    pub const ATTACH_REND_DATA: u32 = 0x0006_9002;
    pub const ATTACHMENT: u32 = 0x0006_9005;
}

/// A multi-valued type is its single type with this bit ([MS-OXCDATA] 2.11.1.2).
const MV: u16 = 0x1000;
/// The interface an attached item's value is given as: IMessage ([MS-OXTNEF] 2.1.3.4).
const IID_MESSAGE: [u8; 16] = [0x07, 0x03, 0x02, 0x00, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46];
/// IStorage, which an object another program laid out is given as.
const IID_STORAGE: [u8; 16] = [0x0B, 0, 0, 0, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46];
const CREATION_TIME: u16 = 0x3007;
const LAST_MODIFICATION_TIME: u16 = 0x3008;
const ATTACH_TRANSPORT_NAME: u16 = 0x370C;

/// Whether `data` begins as a TNEF stream does.
pub fn is_tnef(data: &[u8]) -> bool {
    data.starts_with(&SIGNATURE)
}

/// The item a TNEF stream holds, or `None` when it is not one. What cannot be read of it is left
/// out; what can is kept.
pub fn read(data: &[u8]) -> Option<Item> {
    read_at(data, 0)
}

fn read_at(data: &[u8], depth: usize) -> Option<Item> {
    let mut r = Reader { data: data.strip_prefix(&SIGNATURE)?, at: 2 };
    let mut item = Item::default();
    let mut cp = 1252u32;
    let mut attachments: Vec<Attachment> = Vec::new();
    // Properties given by an attribute only stand when the encapsulated ones do not say
    // otherwise ([MS-OXTNEF] 2.1.3): they are gathered apart and laid under them.
    let mut mapped = PropBag::default();
    while let Some((level, id, value)) = r.attribute() {
        match (level, id) {
            // 0x00010000 is the only version there is (2.3.3.1); another is not this format.
            (_, att::TNEF_VERSION) if value != [0, 0, 1, 0] => return None,
            (_, att::OEM_CODEPAGE) => {
                cp = u32_at(value, 0).filter(|c| *c != 0).unwrap_or(cp);
            }
            (LEVEL_MESSAGE, att::MESSAGE_CLASS) => {
                let class = legacy_class(&string8(value, cp));
                if !class.is_empty() {
                    mapped.set_str(pid::MESSAGE_CLASS, &class);
                }
            }
            (LEVEL_MESSAGE, att::SUBJECT) => mapped.set_str(pid::SUBJECT, &string8(value, cp)),
            (LEVEL_MESSAGE, att::BODY) => mapped.set_str(pid::BODY, &string8(value, cp)),
            (LEVEL_MESSAGE, att::DATE_SENT) => set_date(&mut mapped, pid::CLIENT_SUBMIT_TIME, value),
            (LEVEL_MESSAGE, att::DATE_RECEIVED) => set_date(&mut mapped, pid::MESSAGE_DELIVERY_TIME, value),
            (LEVEL_MESSAGE, att::DATE_MODIFIED) => set_date(&mut mapped, LAST_MODIFICATION_TIME, value),
            (LEVEL_MESSAGE, att::MESSAGE_STATUS) => {
                if let Some(s) = u32_at(value, 0) {
                    mapped.set(pid::MESSAGE_FLAGS, PropValue::Int32(flags_of_status(s)));
                }
            }
            (LEVEL_MESSAGE, att::PRIORITY) => {
                // 1 high, 2 normal, 3 low (2.1.3.3.10).
                let importance = match value.first() {
                    Some(1) => 2,
                    Some(3) => 0,
                    _ => 1,
                };
                mapped.set(pid::IMPORTANCE, PropValue::Int32(importance));
            }
            (LEVEL_MESSAGE, att::MSG_PROPS) => {
                for (key, v) in props(value, cp, depth) {
                    put(&mut item, key, v);
                }
            }
            (LEVEL_MESSAGE, att::RECIP_TABLE) => {
                let mut p = Reader { data: value, at: 0 };
                let rows = p.u32().map_or(0, |n| n as usize).min(MAX_ROWS);
                for _ in 0..rows {
                    let Some(list) = p.prop_list(cp, depth) else { break };
                    let mut row = PropBag::default();
                    for (key, v) in list {
                        if let Key::Id(id) = key {
                            row.set(id, v);
                        }
                    }
                    item.recipients.push(row);
                }
            }
            // Each attachment begins with how it is shown (2.1.3.3.15).
            (LEVEL_ATTACHMENT, att::ATTACH_REND_DATA) => {
                if attachments.len() >= MAX_ATTACHMENTS {
                    break;
                }
                let mut a = Attachment::default();
                a.props.set(pid::ATTACH_METHOD, PropValue::Int32(1));
                if let Some(at) = u32_at(value, 2) {
                    a.props.set(pid::RENDERING_POSITION, PropValue::Int32(at as i32));
                }
                attachments.push(a);
            }
            (LEVEL_ATTACHMENT, _) => {
                let Some(a) = attachments.last_mut() else { continue };
                match id {
                    att::ATTACH_DATA => a.data = value.to_vec(),
                    att::ATTACH_TITLE => {
                        let name = string8(value, cp);
                        a.props.set_str(pid::ATTACH_FILENAME, &name);
                        a.props.set_str(pid::ATTACH_LONG_FILENAME, &name);
                    }
                    att::ATTACH_TRANSPORT_FILENAME => a.props.set_str(ATTACH_TRANSPORT_NAME, &string8(value, cp)),
                    att::ATTACH_CREATE_DATE => set_date(&mut a.props, CREATION_TIME, value),
                    att::ATTACH_MODIFY_DATE => set_date(&mut a.props, LAST_MODIFICATION_TIME, value),
                    att::ATTACHMENT => {
                        for (key, v) in props(value, cp, depth) {
                            match (key, v) {
                                (Key::Id(pid::ATTACH_DATA), Value::Object(iid, bytes)) => {
                                    if iid == IID_MESSAGE
                                        && depth < MAX_DEPTH
                                        && let Some(inner) = read_at(&bytes, depth + 1)
                                    {
                                        a.embedded = Some(Box::new(inner));
                                    } else {
                                        a.data = bytes;
                                    }
                                }
                                (Key::Id(pid::ATTACH_DATA), Value::Plain(PropValue::Binary(b))) => a.data = b,
                                (Key::Id(id), Value::Plain(v)) => a.props.set(id, v),
                                // An attachment's named properties have nowhere to go here.
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    for (id, v) in mapped.0 {
        if item.props.get(id).is_none() {
            item.props.set(id, v);
        }
    }
    for a in &mut attachments {
        // An attached item's data is the item (2.3.3.7).
        if a.embedded.is_some() {
            a.data.clear();
        }
        if a.props.get(pid::ATTACH_SIZE).is_none() {
            a.props.set(pid::ATTACH_SIZE, PropValue::Int32(i32::try_from(a.data.len()).unwrap_or(i32::MAX)));
        }
    }
    item.attachments = attachments;
    Some(item)
}

/// Where a property read from a list goes.
enum Key {
    Id(u16),
    Named(NamedKey),
}

/// A value read from a list: a property, or an object's interface and bytes.
enum Value {
    Plain(PropValue),
    Object([u8; 16], Vec<u8>),
}

fn props(data: &[u8], cp: u32, depth: usize) -> Vec<(Key, Value)> {
    Reader { data, at: 0 }.prop_list_values(cp, depth).unwrap_or_default()
}

/// A property read from a message's list put into the item; an object there has no place.
fn put(item: &mut Item, key: Key, v: Value) {
    let Value::Plain(v) = v else { return };
    match key {
        Key::Id(id) => item.props.set(id, v),
        Key::Named(k) => match v {
            PropValue::MultiString(list) if k == NamedKey::keywords() => item.categories = list,
            v => {
                item.named.retain(|(n, _)| *n != k);
                item.named.push((k, v));
            }
        },
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.data.get(self.at..end)?;
        self.at = end;
        Some(s)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    /// Skips the padding that brings `n` bytes up to a multiple of four.
    fn pad(&mut self, n: usize) -> Option<()> {
        let extra = (4 - n % 4) % 4;
        // A value at the very end may be given without its padding.
        if self.at.saturating_add(extra) > self.data.len() {
            self.at = self.data.len();
        } else {
            self.take(extra)?;
        }
        Some(())
    }

    /// One attribute: its level, id and data. The checksum is not held against it: some writers
    /// got it wrong (2.3.3.4), and the data is checked as it is read.
    fn attribute(&mut self) -> Option<(u8, u32, &'a [u8])> {
        let level = *self.take(1)?.first()?;
        let id = self.u32()?;
        let len = self.u32()? as usize;
        let value = self.take(len)?;
        self.u16()?;
        Some((level, id, value))
    }

    fn prop_list(&mut self, cp: u32, depth: usize) -> Option<Vec<(Key, PropValue)>> {
        Some(
            self.prop_list_values(cp, depth)?
                .into_iter()
                .filter_map(|(k, v)| match v {
                    Value::Plain(v) => Some((k, v)),
                    Value::Object(..) => None,
                })
                .collect(),
        )
    }

    /// A counted list of properties ([MS-OXTNEF] 2.1.3.4). A list cut short keeps what was read.
    fn prop_list_values(&mut self, cp: u32, depth: usize) -> Option<Vec<(Key, Value)>> {
        let count = (self.u32()? as usize).min(MAX_PROPS);
        let mut out = Vec::with_capacity(count.min(256));
        for _ in 0..count {
            let Some(p) = self.property(cp, depth) else { break };
            if let Some(p) = p {
                out.push(p);
            }
        }
        Some(out)
    }

    /// One property; `Some(None)` for one read past but not kept, `None` when the list cannot be
    /// read on.
    fn property(&mut self, cp: u32, depth: usize) -> Option<Option<(Key, Value)>> {
        let ty = self.u16()?;
        let id = self.u16()?;
        let key = if id >= 0x8000 {
            let guid: [u8; 16] = self.take(16)?.try_into().ok()?;
            let name = match self.u32()? {
                0 => NameId::Id(self.u32()?),
                1 => {
                    let len = self.u32()? as usize;
                    let bytes = self.take(len)?;
                    self.pad(len)?;
                    NameId::Name(codepage::utf16le(bytes).trim_end_matches('\0').to_string())
                }
                _ => return None,
            };
            Key::Named(NamedKey { guid, name })
        } else {
            Key::Id(id)
        };
        let value = self.value(ty, cp, depth)?;
        Some(value.map(|v| (key, v)))
    }

    fn value(&mut self, ty: u16, cp: u32, depth: usize) -> Option<Option<Value>> {
        let base = ty & !MV;
        let many = ty & MV != 0;
        if let Some(size) = ptype::fixed_size(base) {
            // Two-byte values take four, as every value is padded to four (2.1.3.4).
            let width = size.max(4);
            if !many {
                let b = self.take(width)?;
                return Some(PropValue::from_fixed(base, b).map(Value::Plain));
            }
            let n = (self.u32()? as usize).min(MAX_VALUES);
            let mut raw = Vec::new();
            let mut ints = Vec::new();
            for _ in 0..n {
                let b = self.take(width)?;
                raw.extend_from_slice(b.get(..size)?);
                if base == ptype::INT32 {
                    ints.push(i32::from_le_bytes(b.get(..4)?.try_into().ok()?));
                }
            }
            return Some(Some(Value::Plain(if base == ptype::INT32 { PropValue::MultiInt32(ints) } else { PropValue::Other { ty, raw } })));
        }
        match base {
            ptype::STRING | ptype::STRING8 | ptype::BINARY | ptype::OBJECT => {
                let n = (self.u32()? as usize).min(MAX_VALUES);
                let mut values = Vec::with_capacity(n.min(64));
                for _ in 0..n {
                    let len = self.u32()? as usize;
                    let b = self.take(len)?;
                    self.pad(len)?;
                    values.push(b);
                }
                let text =
                    |b: &[u8]| if base == ptype::STRING { codepage::utf16le(b) } else { codepage::decode(cp, b) }.trim_end_matches('\0').to_string();
                Some(Some(match (base, many) {
                    (ptype::OBJECT, _) => {
                        let b = values.first()?;
                        let iid: [u8; 16] = b.get(..16)?.try_into().ok()?;
                        // An attached item is read only so deep; the bytes are what it was given as.
                        let _ = depth;
                        Value::Object(iid, b.get(16..).unwrap_or_default().to_vec())
                    }
                    (ptype::BINARY, false) => Value::Plain(PropValue::Binary(values.first().map(|b| b.to_vec()).unwrap_or_default())),
                    (ptype::BINARY, true) => Value::Plain(PropValue::MultiBinary(values.iter().map(|b| b.to_vec()).collect())),
                    (_, false) => Value::Plain(PropValue::String(values.first().map(|b| text(b)).unwrap_or_default())),
                    (_, true) => Value::Plain(PropValue::MultiString(values.iter().map(|b| text(b)).collect())),
                }))
            }
            // Null and unspecified carry nothing; any other type cannot be stepped over.
            0x0000 | 0x0001 if !many => Some(None),
            _ => None,
        }
    }
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// A string attribute: 8-bit text in the stream's code page, ending in a zero.
fn string8(b: &[u8], cp: u32) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    codepage::decode(cp, b.get(..end).unwrap_or_default())
}

/// The message classes of old mail programs, as they are now (2.3.3.4).
fn legacy_class(class: &str) -> String {
    let c = class.strip_prefix("Microsoft Mail v3.0 ").unwrap_or(class);
    match c {
        "IPM.Microsoft Mail.Note" => "IPM.Note",
        "IPM.Microsoft Mail.Read Receipt" => "Report.IPM.Note.IPNRN",
        "IPM.Microsoft Mail.Non-Delivery" => "Report.IPM.Note.NDR",
        "IPM.Microsoft Schedule.MtgRespP" => "IPM.Schedule.Meeting.Resp.Pos",
        "IPM.Microsoft Schedule.MtgRespN" => "IPM.Schedule.Meeting.Resp.Neg",
        "IPM.Microsoft Schedule.MtgRespA" => "IPM.Schedule.Meeting.Resp.Tent",
        "IPM.Microsoft Schedule.MtgReq" => "IPM.Schedule.Meeting.Request",
        "IPM.Microsoft Schedule.MtgCncl" => "IPM.Schedule.Meeting.Canceled",
        other => other,
    }
    .to_string()
}

/// PidTagMessageFlags from attMessageStatus (2.1.3.3.8).
fn flags_of_status(s: u32) -> i32 {
    let mut f = 0;
    for (status, flag) in [(0x20, 0x01), (0x04, 0x04), (0x02, 0x08), (0x80, 0x10)] {
        if s & status != 0 {
            f |= flag;
        }
    }
    // Unmodified is the other way round.
    if s & 0x01 == 0 {
        f |= 0x02;
    }
    f
}

fn status_of_flags(f: i32) -> u32 {
    let mut s = 0;
    for (status, flag) in [(0x20, 0x01), (0x04, 0x04), (0x02, 0x08), (0x80, 0x10)] {
        if f & flag != 0 {
            s |= status;
        }
    }
    if f & 0x02 == 0 {
        s |= 0x01;
    }
    s
}

/// A date attribute: year, month, day, hour, minute, second and weekday (2.1.3.3.4), in UTC.
fn set_date(props: &mut PropBag, id: u16, b: &[u8]) {
    let w = |i: usize| b.get(i * 2..i * 2 + 2).and_then(|s| s.try_into().ok()).map(u16::from_le_bytes);
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(s)) = (w(0), w(1), w(2), w(3), w(4), w(5)) else { return };
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return;
    }
    let t = time::from_civil(i64::from(y), u32::from(mo), u32::from(d), u32::from(h), u32::from(mi), u32::from(s.min(59)), 0);
    props.set(id, PropValue::Time(t));
}

fn date_bytes(t: i64) -> Vec<u8> {
    let c = time::civil(t, 0);
    let weekday = u16::try_from(c.weekday).unwrap_or(0);
    let mut out = Vec::with_capacity(14);
    for v in [u16::try_from(c.year).unwrap_or(1601), c.month as u16, c.day as u16, c.hour as u16, c.minute as u16, c.second as u16, weekday] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// The item as a TNEF stream: its message class as an attribute, its properties, named ones by
/// name, in `attMsgProps`, and each attachment after it, an attached item as a stream of its own.
/// Strings go as UTF-16, so the code page given, 1252, loses nothing (2.2.3.2).
pub fn write(item: &Item) -> Vec<u8> {
    write_at(item, 0)
}

fn write_at(item: &Item, depth: usize) -> Vec<u8> {
    let mut w = Writer(Vec::new());
    w.0.extend_from_slice(&SIGNATURE);
    // The key is of no use now (2.1.3.2); any number does.
    w.0.extend_from_slice(&1u16.to_le_bytes());
    w.attribute(LEVEL_MESSAGE, att::TNEF_VERSION, &[0, 0, 1, 0]);
    w.attribute(LEVEL_MESSAGE, att::OEM_CODEPAGE, &[0xE4, 0x04, 0, 0, 0, 0, 0, 0]);
    let class = item.message_class();
    let mut class8 = codepage::encode(1252, class).unwrap_or_else(|| class.bytes().filter(u8::is_ascii).collect());
    class8.push(0);
    w.attribute(LEVEL_MESSAGE, att::MESSAGE_CLASS, &class8);
    if let Some(f) = item.props.i32(pid::MESSAGE_FLAGS) {
        w.attribute(LEVEL_MESSAGE, att::MESSAGE_STATUS, &status_of_flags(f).to_le_bytes());
    }
    for (id, att_id) in [(pid::CLIENT_SUBMIT_TIME, att::DATE_SENT), (pid::MESSAGE_DELIVERY_TIME, att::DATE_RECEIVED)] {
        if let Some(t) = item.props.time(id).filter(|t| *t > 0) {
            w.attribute(LEVEL_MESSAGE, att_id, &date_bytes(t));
        }
    }
    let mut list = PropList::default();
    for (id, v) in item.props.iter() {
        // What the mail around it says of where it came from is not repeated in it.
        if id == pid::TRANSPORT_MESSAGE_HEADERS {
            continue;
        }
        list.add(id, None, v);
    }
    if !item.categories.is_empty() {
        list.add(0x8000, Some(&NamedKey::keywords()), &PropValue::MultiString(item.categories.clone()));
    }
    for (k, v) in &item.named {
        list.add(0x8000, Some(k), v);
    }
    w.attribute(LEVEL_MESSAGE, att::MSG_PROPS, &list.bytes());
    // An attached item says who it was for: its recipients go with it (2.2.3.16).
    if depth > 0 && !item.recipients.is_empty() {
        let mut rows = (item.recipients.len() as u32).to_le_bytes().to_vec();
        for r in &item.recipients {
            let mut row = PropList::default();
            for (id, v) in r.iter() {
                row.add(id, None, v);
            }
            rows.extend_from_slice(&row.bytes());
        }
        w.attribute(LEVEL_MESSAGE, att::RECIP_TABLE, &rows);
    }
    for a in &item.attachments {
        let method = a.props.i32(pid::ATTACH_METHOD).unwrap_or(1);
        let position = a.props.i32(pid::RENDERING_POSITION).unwrap_or(-1);
        let mut rend = Vec::with_capacity(14);
        rend.extend_from_slice(&(if method == 6 { 2u16 } else { 1u16 }).to_le_bytes());
        rend.extend_from_slice(&position.to_le_bytes());
        let size: i16 = if method == 1 { 32 } else { -1 };
        rend.extend_from_slice(&size.to_le_bytes());
        rend.extend_from_slice(&size.to_le_bytes());
        rend.extend_from_slice(&0u32.to_le_bytes());
        w.attribute(LEVEL_ATTACHMENT, att::ATTACH_REND_DATA, &rend);
        let name = a.name();
        let mut title = codepage::encode(1252, &name).unwrap_or_else(|| name.bytes().filter(u8::is_ascii).collect());
        title.push(0);
        w.attribute(LEVEL_ATTACHMENT, att::ATTACH_TITLE, &title);
        let embedded = a.embedded.as_ref().filter(|_| depth < MAX_DEPTH);
        if embedded.is_none() && method != 6 {
            w.attribute(LEVEL_ATTACHMENT, att::ATTACH_DATA, &a.data);
        }
        let mut list = PropList::default();
        for (id, v) in a.props.iter() {
            if id != pid::ATTACH_DATA {
                list.add(id, None, v);
            }
        }
        match embedded {
            Some(inner) => list.object(pid::ATTACH_DATA, &IID_MESSAGE, &write_at(inner, depth + 1)),
            None if method == 6 => list.object(pid::ATTACH_DATA, &IID_STORAGE, &a.data),
            None => list.add(pid::ATTACH_DATA, None, &PropValue::Binary(a.data.clone())),
        }
        w.attribute(LEVEL_ATTACHMENT, att::ATTACHMENT, &list.bytes());
    }
    w.0
}

struct Writer(Vec<u8>);

impl Writer {
    /// Level, id, length, data and the sum of the data's bytes (2.1.3.2).
    fn attribute(&mut self, level: u8, id: u32, data: &[u8]) {
        self.0.push(level);
        self.0.extend_from_slice(&id.to_le_bytes());
        self.0.extend_from_slice(&(data.len() as u32).to_le_bytes());
        self.0.extend_from_slice(data);
        let sum = data.iter().fold(0u16, |s, &b| s.wrapping_add(u16::from(b)));
        self.0.extend_from_slice(&sum.to_le_bytes());
    }
}

/// A counted list of properties being written; named ones are given ids from 0x8000 up.
#[derive(Default)]
struct PropList {
    count: u32,
    body: Vec<u8>,
    next_named: u16,
}

impl PropList {
    fn tag(&mut self, ty: u16, id: u16, named: Option<&NamedKey>) {
        let id = match named {
            Some(_) => {
                let id = 0x8000u16.saturating_add(self.next_named);
                self.next_named = self.next_named.saturating_add(1);
                id
            }
            None => id,
        };
        self.body.extend_from_slice(&ty.to_le_bytes());
        self.body.extend_from_slice(&id.to_le_bytes());
        if let Some(k) = named {
            self.body.extend_from_slice(&k.guid);
            match &k.name {
                NameId::Id(n) => {
                    self.body.extend_from_slice(&0u32.to_le_bytes());
                    self.body.extend_from_slice(&n.to_le_bytes());
                }
                NameId::Name(s) => {
                    let mut b: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
                    b.extend_from_slice(&[0, 0]);
                    self.body.extend_from_slice(&1u32.to_le_bytes());
                    self.body.extend_from_slice(&(b.len() as u32).to_le_bytes());
                    self.padded(&b);
                }
            }
        }
        self.count = self.count.saturating_add(1);
    }

    fn padded(&mut self, b: &[u8]) {
        self.body.extend_from_slice(b);
        self.body.resize(self.body.len() + (4 - b.len() % 4) % 4, 0);
    }

    fn variable(&mut self, values: &[Vec<u8>]) {
        self.body.extend_from_slice(&(values.len() as u32).to_le_bytes());
        for v in values {
            self.body.extend_from_slice(&(v.len() as u32).to_le_bytes());
            self.padded(v);
        }
    }

    /// One property; a value of a type there is no way to write is left out.
    fn add(&mut self, id: u16, named: Option<&NamedKey>, v: &PropValue) {
        let utf16 = |s: &str| -> Vec<u8> { s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect() };
        match v {
            PropValue::String(s) => {
                self.tag(ptype::STRING, id, named);
                self.variable(&[utf16(s)]);
            }
            PropValue::Binary(b) => {
                self.tag(ptype::BINARY, id, named);
                self.variable(std::slice::from_ref(b));
            }
            PropValue::Guid(g) => {
                self.tag(ptype::GUID, id, named);
                self.body.extend_from_slice(g);
            }
            PropValue::MultiInt32(list) => {
                self.tag(ptype::MULTI_INT32, id, named);
                self.body.extend_from_slice(&(list.len() as u32).to_le_bytes());
                for n in list {
                    self.body.extend_from_slice(&n.to_le_bytes());
                }
            }
            PropValue::MultiString(list) => {
                self.tag(ptype::MULTI_STRING, id, named);
                self.variable(&list.iter().map(|s| utf16(s)).collect::<Vec<_>>());
            }
            PropValue::MultiBinary(list) => {
                self.tag(ptype::MULTI_BINARY, id, named);
                self.variable(list);
            }
            PropValue::Other { ty, raw } => {
                // A multi-valued fixed-size type read as its values laid end to end.
                let base = ty & !MV;
                let Some(size) = ptype::fixed_size(base).filter(|_| ty & MV != 0) else { return };
                self.tag(*ty, id, named);
                let values: Vec<&[u8]> = raw.chunks_exact(size).collect();
                self.body.extend_from_slice(&(values.len() as u32).to_le_bytes());
                for v in values {
                    self.padded(v);
                }
            }
            fixed => {
                let Some(bytes) = fixed.fixed_bytes() else { return };
                let size = ptype::fixed_size(fixed.ty()).unwrap_or(8);
                self.tag(fixed.ty(), id, named);
                self.padded(bytes.get(..size).unwrap_or(&bytes));
            }
        }
    }

    /// An object: its interface, then its bytes (2.1.3.4).
    fn object(&mut self, id: u16, iid: &[u8; 16], data: &[u8]) {
        self.tag(ptype::OBJECT, id, None);
        let mut v = iid.to_vec();
        v.extend_from_slice(data);
        self.variable(&[v]);
    }

    fn bytes(&self) -> Vec<u8> {
        let mut out = self.count.to_le_bytes().to_vec();
        out.extend_from_slice(&self.body);
        out
    }
}
