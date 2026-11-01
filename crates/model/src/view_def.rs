//! View definitions, as Outlook keeps a folder's views in it ([MS-OXOCFG] 2.2.6): a hidden
//! (FAI) message of class `IPM.Microsoft.FolderDesign.NamedView` in the folder, whose
//! PidTagViewDescriptorBinary holds the columns, the sort and the grouping, and whose
//! PidTagViewDescriptorStrings holds the columns' names. What this program does not use (a
//! view's restriction, the strings that follow the names, flags) is kept as it was read.

use crate::props::{NameId, NamedKey};

/// The message class of a view definition ([MS-OXOCFG] 2.2.6).
pub const CLASS: &str = "IPM.Microsoft.FolderDesign.NamedView";
/// PidTagViewDescriptorBinary ([MS-OXPROPS] 2.1057).
pub const BINARY: u16 = 0x7001;
/// PidTagViewDescriptorStrings ([MS-OXPROPS] 2.1059).
pub const STRINGS: u16 = 0x7002;
/// PidTagViewDescriptorName ([MS-OXPROPS] 2.1058).
pub const NAME: u16 = 0x7006;
/// PidTagViewDescriptorVersion ([MS-OXPROPS] 2.1060), always 8.
pub const VERSION: u16 = 0x7007;
const VERSION_8: u32 = 0x0000_0008;

/// The column's header is a picture ([MS-OXOCFG] 2.2.6.1.1).
pub const VCDF_BITMAP: u32 = 0x0000_0008;
pub const VCDF_NOT_SORTABLE: u32 = 0x0000_0020;
pub const VCDF_SORTDESCENDING: u32 = 0x0000_0040;
pub const VCDF_NAMEDPROP: u32 = 0x0000_1000;
pub const VCDF_MULTIVALUED: u32 = 0x0000_4000;

/// The sort is descending ([MS-OXOCFG] 2.2.6.1, ulFlags).
const SORT_DESCENDING: u32 = 0x0000_0002;
/// The table is arranged by conversation (ivcdSort).
const BY_CONVERSATION: u32 = 0xFFFF_FFFF;
/// "Group by" columns, at most ([MS-OXOCFG] 2.2.6.1, cCat).
const MAX_GROUPS: usize = 4;
/// Columns read, at most: no view has a hundredth of this.
const MAX_COLUMNS: usize = 1024;
/// A named column's name, at most this many bytes.
const MAX_NAME: usize = 1024;

/// One column of a view ([MS-OXOCFG] 2.2.6.1.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    /// The property, as a tag: its id in the high half and its type in the low half. For a
    /// named property the id is the file's own and means nothing elsewhere: `named` says
    /// which property it is.
    pub tag: u32,
    /// Its width, in pixels.
    pub width: u32,
    pub flags: u32,
    pub named: Option<NamedKey>,
    /// The name its header shows, from PidTagViewDescriptorStrings.
    pub title: String,
}

impl Column {
    /// A column of a property by its tag.
    pub fn of(tag: u32, width: u32, title: &str) -> Self {
        Self { tag, width, flags: 0, named: None, title: title.to_string() }
    }

    /// A column of a named property, of property type `ty`.
    pub fn named(key: NamedKey, ty: u16, width: u32, title: &str) -> Self {
        let multi = if ty & 0x1000 != 0 { VCDF_MULTIVALUED } else { 0 };
        Self { tag: u32::from(ty), width, flags: VCDF_NAMEDPROP | multi, named: Some(key), title: title.to_string() }
    }

    /// The "blank" column every view begins with ([MS-OXOCFG] 2.2.6.1, ColumnInfo).
    fn blank() -> Self {
        Self { tag: 0x0004_0001, width: 7, flags: VCDF_BITMAP | VCDF_NOT_SORTABLE, named: None, title: String::new() }
    }

    fn is_blank(&self) -> bool {
        self.tag == 0x0004_0001 && self.named.is_none()
    }

    /// The property's id, for a property that is not named.
    pub fn id(&self) -> Option<u16> {
        u16::try_from(self.tag >> 16).ok().filter(|_| self.named.is_none())
    }
}

/// A view: its columns, what it is sorted and grouped by, and the restriction kept as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewDef {
    /// The columns shown, the blank one left out, then those grouped and sorted by that are
    /// not shown; `groups` of them, from the first, are the "group by" columns.
    pub columns: Vec<Column>,
    /// Which of `columns` the rows are sorted by; `None` when they are arranged by
    /// conversation.
    pub sort: Option<usize>,
    pub descending: bool,
    /// How many of the first columns the rows are grouped by, at most four.
    pub groups: usize,
    /// For each "group by" column, a bit set when it is in ascending order.
    pub groups_ascending: u32,
    /// The RestrictionPacket as it was read ([MS-OXOCFG] 2.2.6.1.2); a restriction that lets
    /// every row through when the view was made here.
    pub restriction: Vec<u8>,
    /// The strings after the column names (those of the restriction), as read.
    pub more_strings: Vec<String>,
}

impl Default for ViewDef {
    fn default() -> Self {
        Self {
            columns: Vec::new(),
            sort: None,
            descending: false,
            groups: 0,
            groups_ascending: 0,
            restriction: every_row(),
            more_strings: Vec::new(),
        }
    }
}

/// A LogicalAndRestriction of nothing, which every row meets ([MS-OXOCFG] 2.2.6.1.2.1).
fn every_row() -> Vec<u8> {
    vec![0; 16]
}

struct Rd<'a>(&'a [u8]);

impl Rd<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let (a, b) = (self.0.get(..n)?, self.0.get(n..)?);
        self.0 = b;
        Some(a)
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).and_then(|b| b.try_into().ok()).map(u16::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes)
    }
}

/// A view read from its two properties: `None` when the binary is not one ([MS-OXOCFG]
/// 2.2.6.1). Names that are missing are left empty.
pub fn read(binary: &[u8], strings: Option<&str>) -> Option<ViewDef> {
    let mut r = Rd(binary);
    r.take(8)?;
    if r.u32()? != VERSION_8 {
        return None;
    }
    let flags = r.u32()?;
    r.u32()?;
    let count = usize::try_from(r.u32()?).ok()?;
    let sort = r.u32()?;
    let groups = usize::try_from(r.u32()?).ok()?;
    let groups_ascending = r.u32()?;
    r.take(24)?;
    if count > MAX_COLUMNS {
        return None;
    }
    let mut columns = Vec::with_capacity(count);
    for _ in 0..count {
        let ty = r.u16()?;
        let id = r.u16()?;
        let width = r.u32()?;
        r.u32()?;
        let flags = r.u32()?;
        r.take(12)?;
        let kind = r.u32()?;
        let long_id = r.u32()?;
        let named = if flags & VCDF_NAMEDPROP != 0 {
            let guid: [u8; 16] = r.take(16)?.try_into().ok()?;
            let name = match kind {
                1 => {
                    let len = usize::try_from(r.u32()?).ok()?;
                    if len > MAX_NAME || len % 2 != 0 {
                        return None;
                    }
                    let units: Vec<u16> =
                        r.take(len)?.chunks_exact(2).filter_map(|c| c.try_into().ok().map(u16::from_le_bytes)).take_while(|u| *u != 0).collect();
                    NameId::Name(String::from_utf16_lossy(&units))
                }
                _ => NameId::Id(long_id),
            };
            Some(NamedKey { guid, name })
        } else {
            None
        };
        columns.push(Column { tag: u32::from(id) << 16 | u32::from(ty), width, flags, named, title: String::new() });
    }
    let restriction = r.0.to_vec();
    // The names, one after another, each ended by a new line ([MS-OXOCFG] 2.2.6.3).
    let mut names = strings.unwrap_or("").split('\n').map(str::to_string).collect::<Vec<_>>();
    if names.last().is_some_and(String::is_empty) {
        names.pop();
    }
    let mut names = names.into_iter();
    for c in &mut columns {
        c.title = names.next().unwrap_or_default();
    }
    let more_strings = names.collect();
    // The blank column is not one of the view's own.
    let blank = columns.first().is_some_and(Column::is_blank);
    let shift = usize::from(blank);
    if blank {
        columns.remove(0);
    }
    let sort = match sort {
        BY_CONVERSATION => None,
        i => usize::try_from(i).ok().and_then(|i| i.checked_sub(shift)).filter(|i| *i < columns.len()),
    };
    Some(ViewDef {
        columns,
        sort,
        descending: flags & SORT_DESCENDING != 0,
        groups: groups.min(MAX_GROUPS).min(count.saturating_sub(shift)),
        groups_ascending,
        restriction,
        more_strings,
    })
}

/// The view's two properties: PidTagViewDescriptorBinary and PidTagViewDescriptorStrings.
pub fn write(view: &ViewDef) -> (Vec<u8>, String) {
    let mut columns = Vec::with_capacity(view.columns.len().saturating_add(1));
    columns.push(Column::blank());
    columns.extend(view.columns.iter().take(MAX_COLUMNS - 1).cloned());
    let count = u32::try_from(columns.len()).unwrap_or(u32::MAX);
    let sort = view.sort.filter(|i| *i < view.columns.len()).and_then(|i| u32::try_from(i + 1).ok()).unwrap_or(BY_CONVERSATION);
    let mut b = Vec::with_capacity(64 + columns.len() * 48 + view.restriction.len());
    b.extend_from_slice(&[0; 8]);
    b.extend_from_slice(&VERSION_8.to_le_bytes());
    b.extend_from_slice(&(if view.descending { SORT_DESCENDING } else { 0 }).to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&count.to_le_bytes());
    b.extend_from_slice(&sort.to_le_bytes());
    let groups = view.groups.min(MAX_GROUPS).min(view.columns.len());
    b.extend_from_slice(&u32::try_from(groups).unwrap_or(0).to_le_bytes());
    b.extend_from_slice(&view.groups_ascending.to_le_bytes());
    b.extend_from_slice(&[0; 24]);
    for c in &columns {
        let ty = (c.tag & 0xFFFF) as u16;
        b.extend_from_slice(&ty.to_le_bytes());
        b.extend_from_slice(&((c.tag >> 16) as u16).to_le_bytes());
        b.extend_from_slice(&c.width.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        let flags = if c.named.is_some() { c.flags | VCDF_NAMEDPROP } else { c.flags & !VCDF_NAMEDPROP };
        b.extend_from_slice(&flags.to_le_bytes());
        b.extend_from_slice(&[0; 12]);
        match &c.named {
            None => {
                b.extend_from_slice(&0u32.to_le_bytes());
                b.extend_from_slice(&(c.tag >> 16).to_le_bytes());
            }
            Some(key) => {
                let kind = u32::from(matches!(key.name, NameId::Name(_)));
                b.extend_from_slice(&kind.to_le_bytes());
                b.extend_from_slice(&(if let NameId::Id(id) = key.name { id } else { 0 }).to_le_bytes());
                b.extend_from_slice(&key.guid);
                if let NameId::Name(name) = &key.name {
                    let mut units: Vec<u16> = name.encode_utf16().take(MAX_NAME / 2 - 1).collect();
                    units.push(0);
                    b.extend_from_slice(&u32::try_from(units.len() * 2).unwrap_or(0).to_le_bytes());
                    for u in units {
                        b.extend_from_slice(&u.to_le_bytes());
                    }
                }
            }
        }
    }
    b.extend_from_slice(if view.restriction.is_empty() { &[0; 16] } else { &view.restriction });
    let mut strings = String::new();
    for name in columns.iter().map(|c| c.title.as_str()).chain(view.more_strings.iter().map(String::as_str)) {
        // A name holds no new line: that ends it.
        strings.extend(name.chars().filter(|c| *c != '\n'));
        strings.push('\n');
    }
    (b, strings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::props::pset;

    fn view() -> ViewDef {
        ViewDef {
            columns: vec![
                Column::of(0x0017_0003, 18, "Importance"),
                Column::of(0x0042_001F, 140, "From"),
                Column::of(0x0037_001F, 300, "Subject"),
                Column::named(NamedKey { guid: pset::PS_PUBLIC_STRINGS, name: NameId::Name("Keywords".into()) }, 0x101F, 90, "Categories"),
                Column::of(0x0E06_0040, 120, "Received"),
            ],
            sort: Some(4),
            descending: true,
            groups: 1,
            groups_ascending: 1,
            ..ViewDef::default()
        }
    }

    #[test]
    fn a_view_reads_back_as_it_was_written() {
        let v = view();
        let (b, s) = write(&v);
        assert_eq!(&b[8..12], &[8, 0, 0, 0]);
        assert_eq!(s, "\nImportance\nFrom\nSubject\nCategories\nReceived\n");
        let back = read(&b, Some(&s)).unwrap();
        assert_eq!(back, v);
        // The restriction and the strings after the names are kept as read.
        let mut odd = v.clone();
        odd.restriction = vec![8, 0, 0, 0, 0x1F, 0, 0x37, 0];
        odd.more_strings = vec!["hello".into()];
        let (b, s) = write(&odd);
        assert_eq!(read(&b, Some(&s)).unwrap(), odd);
        // Arranged by conversation.
        let conv = ViewDef { sort: None, ..v };
        let (b, s) = write(&conv);
        assert_eq!(read(&b, Some(&s)).unwrap().sort, None);
    }

    #[test]
    fn a_view_that_is_not_one_is_refused() {
        let (b, s) = write(&view());
        for n in 0..b.len().min(200) {
            let _ = read(&b[..n], Some(&s));
        }
        let mut wrong = b.clone();
        wrong[8] = 7;
        assert!(read(&wrong, None).is_none(), "another version");
        let mut many = b.clone();
        many[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read(&many, None).is_none());
        // A sort past the columns, and a count of groups past them, are kept within them.
        let mut far = b.clone();
        far[24..28].copy_from_slice(&999u32.to_le_bytes());
        far[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        let v = read(&far, None).unwrap();
        assert_eq!((v.sort, v.groups), (None, 4));
        assert!(v.columns.iter().all(|c| c.title.is_empty()));
        // Garbage of every length is only ever refused.
        let mut seed = 7u32;
        for len in 0..300 {
            let junk: Vec<u8> = (0..len)
                .map(|_| {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    (seed >> 16) as u8
                })
                .collect();
            let _ = read(&junk, Some("a\nb"));
        }
    }
}
