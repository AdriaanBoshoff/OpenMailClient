//! The AutoComplete list: the names and addresses written to before, suggested first as an
//! address is typed. Each send adds to it; one is taken off it alone (the X beside a
//! suggestion) or all at once (File > Options > Mail > Empty Auto-Complete List). Outlook's own
//! list is brought from a data file it keeps it in: a hidden message of class
//! `IPM.Configuration.Autocomplete` in the Inbox, its PidTagRoamingBinary the stream
//! [MS-OXOCFG] 2.2.4.1 lays out.

use openmailclient_model::{FolderKind, Item, Store};
use serde_json::{Value, json};

/// How many it keeps; Outlook keeps about a thousand, and the least used go first.
pub const MAX_NAMES: usize = 2000;
/// The longest name or address kept.
pub const MAX_TEXT: usize = 320;

/// PidTagRoamingBinary ([MS-OXPROPS] 2.920), where a configuration message keeps its stream.
const ROAMING_BINARY: u16 = 0x7C09;
const CLASS: &str = "IPM.Configuration.Autocomplete";
/// The words a stream begins and ends with ([MS-OXOCFG] 2.2.4.1.1).
const MARK: u32 = 0xBAAD_F00D;
/// The most rows and properties read from one stream: a stream can say any number.
const MAX_ROWS: u32 = 10_000;
const MAX_PROPS: u32 = 256;

/// One name of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    pub name: String,
    pub address: String,
    /// How many times it was written to: the more, the higher it is suggested.
    pub uses: u32,
    /// When it was last written to, in seconds since 1970; 0 when not known.
    pub last: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutoComplete {
    pub list: Vec<Name>,
}

impl AutoComplete {
    /// The place of `address` in the list, its case aside.
    fn place(&self, address: &str) -> Option<usize> {
        self.list.iter().position(|n| n.address.eq_ignore_ascii_case(address))
    }

    /// `name <address>` was written to at `now`: added, or used once more. A name it had is
    /// kept where the new one is empty.
    pub fn learn(&mut self, name: &str, address: &str, now: i64) {
        let address = cut(address.trim());
        if !address.contains('@') {
            return;
        }
        let name = cut(name.trim());
        match self.place(&address) {
            Some(i) => {
                if let Some(n) = self.list.get_mut(i) {
                    n.uses = n.uses.saturating_add(1);
                    n.last = n.last.max(now);
                    if !name.is_empty() {
                        n.name = name;
                    }
                }
            }
            None => self.list.push(Name { name, address, uses: 1, last: now }),
        }
        self.trim();
    }

    /// Names brought from elsewhere: those it has already keep theirs, and the rest are added
    /// as used once. Returns how many were added.
    pub fn bring(&mut self, names: Vec<Name>) -> usize {
        let mut added = 0;
        for n in names {
            if !n.address.contains('@') {
                continue;
            }
            match self.place(&n.address) {
                Some(i) => {
                    if let Some(had) = self.list.get_mut(i) {
                        if had.name.is_empty() {
                            had.name = n.name;
                        }
                        had.last = had.last.max(n.last);
                    }
                }
                None => {
                    self.list.push(Name { uses: n.uses.max(1), ..n });
                    added += 1;
                }
            }
        }
        self.trim();
        added
    }

    /// Take `address` off the list; false when it was not on it.
    pub fn forget(&mut self, address: &str) -> bool {
        match self.place(address.trim()) {
            Some(i) => {
                self.list.remove(i);
                true
            }
            None => false,
        }
    }

    /// The most used first, then the latest; past the limit the rest go.
    fn trim(&mut self) {
        self.list.sort_by(|a, b| b.uses.cmp(&a.uses).then(b.last.cmp(&a.last)));
        self.list.truncate(MAX_NAMES);
    }

    pub fn to_json(&self) -> Value {
        Value::Array(self.list.iter().map(|n| json!({ "name": n.name, "address": n.address, "uses": n.uses, "last": n.last })).collect())
    }

    /// The list as settings keep it, and how many entries were left out as not understood.
    pub fn of_json(v: &Value) -> (Self, u32) {
        let mut out = Self::default();
        let mut left_out = 0u32;
        for e in v.as_array().map(Vec::as_slice).unwrap_or_default() {
            let address = e.get("address").and_then(Value::as_str).map(|a| cut(a.trim())).unwrap_or_default();
            if !address.contains('@') || out.place(&address).is_some() {
                left_out = left_out.saturating_add(1);
                continue;
            }
            out.list.push(Name {
                name: e.get("name").and_then(Value::as_str).map(|n| cut(n.trim())).unwrap_or_default(),
                address,
                uses: e.get("uses").and_then(Value::as_u64).and_then(|u| u32::try_from(u).ok()).unwrap_or(1).max(1),
                last: e.get("last").and_then(Value::as_i64).unwrap_or(0),
            });
        }
        out.trim();
        (out, left_out)
    }
}

fn cut(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect()
}

/// The AutoComplete list a data file keeps in its Inbox, as Outlook wrote it; empty where it
/// keeps none. A stream that cannot be read to its end gives the names read before.
pub fn from_store(store: &mut dyn Store) -> Vec<Name> {
    let Ok(folders) = store.folders() else { return Vec::new() };
    let Some(inbox) = folders.iter().find(|f| f.kind == FolderKind::Inbox).map(|f| f.id) else { return Vec::new() };
    let Ok(hidden) = store.hidden(inbox) else { return Vec::new() };
    let mut out = Vec::new();
    for (_, item) in hidden {
        let Ok(item) = item else { continue };
        if let Some(b) = stream_of(&item) {
            out.extend(read_stream(b));
        }
    }
    out
}

fn stream_of(item: &Item) -> Option<&[u8]> {
    if !item.message_class().eq_ignore_ascii_case(CLASS) {
        return None;
    }
    match item.props.get(ROAMING_BINARY) {
        Some(openmailclient_model::PropValue::Binary(b)) => Some(b),
        _ => None,
    }
}

/// Reads through a stream, never past its end.
struct Bytes<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Bytes<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let out = self.b.get(self.at..end)?;
        self.at = end;
        Some(out)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
}

/// The names of an AutoComplete stream ([MS-OXOCFG] 2.2.4.1): a header (the mark, major
/// version 12, minor version 0), the count of rows, and each row its count of properties
/// followed by them. Rows past one that cannot be read are not looked at.
pub fn read_stream(b: &[u8]) -> Vec<Name> {
    let mut r = Bytes { b, at: 0 };
    let mut out = Vec::new();
    if r.u32() != Some(MARK) || r.u32() != Some(12) {
        return out;
    }
    let _minor = r.u32();
    let Some(rows) = r.u32() else { return out };
    for _ in 0..rows.min(MAX_ROWS) {
        let Some(row) = read_row(&mut r) else { break };
        if let Some(n) = row.into_name() {
            out.push(n);
        }
    }
    out
}

/// What a row says of the one written to.
#[derive(Default)]
struct Row {
    display: Option<String>,
    kind: Option<String>,
    email: Option<String>,
    smtp: Option<String>,
}

impl Row {
    fn into_name(self) -> Option<Name> {
        let smtp_kind = self.kind.as_deref().is_none_or(|k| k.eq_ignore_ascii_case("SMTP"));
        let address = self.smtp.filter(|a| a.contains('@')).or_else(|| self.email.filter(|a| smtp_kind && a.contains('@')))?;
        let address = cut(address.trim());
        let name = self.display.map(|d| cut(d.trim())).filter(|d| !d.eq_ignore_ascii_case(&address)).unwrap_or_default();
        Some(Name { name, address, uses: 1, last: 0 })
    }
}

/// One row: its count of properties, then each as [MS-OXOCFG] 2.2.4.1.3 lays them out: the
/// property tag, four reserved bytes and an eight-byte value; for a string or binary the
/// value's first four bytes are its length, and its bytes follow.
fn read_row(r: &mut Bytes) -> Option<Row> {
    let count = r.u32()?;
    if count > MAX_PROPS {
        return None;
    }
    let mut row = Row::default();
    for _ in 0..count {
        let tag = r.u32()?;
        let _reserved = r.u32()?;
        let union = r.take(8)?;
        let size = u32::from_le_bytes(union.get(..4)?.try_into().ok()?);
        let ty = (tag & 0xFFFF) as u16;
        let id = (tag >> 16) as u16;
        let data = match ty {
            // Fixed-size values are in the union itself.
            0x0002 | 0x0003 | 0x0004 | 0x0005 | 0x0006 | 0x0007 | 0x000A | 0x000B | 0x0014 | 0x0040 => None,
            0x001F | 0x001E | 0x0102 => Some(r.take(usize::try_from(size).ok()?)?),
            0x0048 => {
                r.take(16)?;
                None
            }
            // A kind it does not know the size of: nothing past it can be read.
            _ => return None,
        };
        let text = |d: &[u8]| -> String { if ty == 0x001F { crate::outlook::utf16(d) } else { d.iter().map(|&c| char::from(c)).collect() } };
        let Some(d) = data else { continue };
        if ty == 0x0102 {
            continue;
        }
        let t = text(d).trim_end_matches('\0').to_string();
        match id {
            // PidTagDisplayName, PidTagAddressType, PidTagEmailAddress, PidTagSmtpAddress.
            0x3001 => row.display = Some(t),
            0x3002 => row.kind = Some(t),
            0x3003 => row.email = Some(t),
            0x39FE => row.smtp = Some(t),
            _ => {}
        }
    }
    Some(row)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A stream as [MS-OXOCFG] 2.2.4.1 lays one out, of rows of (property id, text) and a
    /// number at the end of each, written by us.
    pub(crate) fn stream(rows: &[&[(u16, &str)]]) -> Vec<u8> {
        let mut b = Vec::new();
        for w in [MARK, 12, 0, u32::try_from(rows.len()).unwrap()] {
            b.extend(w.to_le_bytes());
        }
        for row in rows {
            b.extend(u32::try_from(row.len() + 1).unwrap().to_le_bytes());
            for (id, text) in *row {
                let data: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
                b.extend(((u32::from(*id) << 16) | 0x001F).to_le_bytes());
                b.extend([0; 4]);
                b.extend(u32::try_from(data.len()).unwrap().to_le_bytes());
                b.extend([0; 4]);
                b.extend(data);
            }
            // A number of its own, as a row has (its weight, say), in the union alone.
            b.extend(0x6001_0003u32.to_le_bytes());
            b.extend([0; 4]);
            b.extend(5u64.to_le_bytes());
        }
        b.extend(0u32.to_le_bytes());
        b.extend(MARK.to_le_bytes());
        b
    }

    #[test]
    fn a_stream_is_read() {
        let b = stream(&[
            &[(0x3001, "Kim Lee"), (0x3002, "SMTP"), (0x3003, "kim@example.test")],
            &[(0x3001, "Exchange person"), (0x3002, "EX"), (0x3003, "/o=Org/cn=x"), (0x39FE, "ex@example.test")],
            &[(0x3001, "x500 only"), (0x3002, "EX"), (0x3003, "/o=Org/cn=y")],
            &[(0x3001, "sam@example.test"), (0x3003, "sam@example.test")],
        ]);
        let got = read_stream(&b);
        let pairs: Vec<(&str, &str)> = got.iter().map(|n| (n.name.as_str(), n.address.as_str())).collect();
        assert_eq!(pairs, [("Kim Lee", "kim@example.test"), ("Exchange person", "ex@example.test"), ("", "sam@example.test")]);
    }

    #[test]
    fn a_hostile_stream_never_panics() {
        let good = stream(&[&[(0x3001, "Kim"), (0x3003, "kim@example.test")], &[(0x3003, "two@example.test")]]);
        for n in 0..good.len() {
            let _ = read_stream(&good[..n]);
        }
        // Counts far past what is there, and lengths past the end.
        let mut huge = good.clone();
        huge[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(read_stream(&huge).len(), 2);
        let mut long = good.clone();
        long[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_stream(&long).is_empty());
        assert!(read_stream(&[0xFF; 64]).is_empty());
        let mut odd = good;
        odd[20..24].copy_from_slice(&0x3001_00FEu32.to_le_bytes());
        assert!(read_stream(&odd).is_empty());
    }

    #[test]
    fn the_most_used_come_first_and_the_list_is_bounded() {
        let mut a = AutoComplete::default();
        a.learn("Kim", "kim@example.test", 10);
        a.learn("", "sam@example.test", 20);
        a.learn("", "KIM@example.test", 30);
        assert_eq!(a.list[0].name, "Kim");
        assert_eq!((a.list[0].uses, a.list[0].last), (2, 30));
        a.learn("no address", "nobody", 40);
        assert_eq!(a.list.len(), 2);
        for i in 0..MAX_NAMES + 10 {
            a.learn("", &format!("p{i}@example.test"), 50);
        }
        assert_eq!(a.list.len(), MAX_NAMES);
        assert!(a.place("kim@example.test").is_some(), "the most used stay");
        let (again, left_out) = AutoComplete::of_json(&a.to_json());
        assert_eq!((again, left_out), (a.clone(), 0));
        assert!(a.forget("Kim@Example.test"));
        assert!(!a.forget("kim@example.test"));
    }
}
