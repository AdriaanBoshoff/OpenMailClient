//! Each folder's own view: what its list is arranged by, its columns and where its reading
//! pane is. Kept in the data file where Outlook keeps a folder's views, a hidden message of
//! the folder ([MS-OXOCFG] 2.2.6), under a name of our own so that Outlook's own views are
//! never written over; and in the settings, so that a folder of a file that cannot be
//! written keeps its view too. A folder that has none shows the view the last one had.
//!
//! Looking at a mailbox does not change it: a view goes into the data file when the file is
//! next saved, and until then the settings keep it, marked as not yet written.

use crate::print::Field;
use crate::{ReadingPane, Session, SortKey};
use openmailclient_model::props::{NameId, NamedKey, PropValue, pid};
use openmailclient_model::view_def::{self, Column, ViewDef};
use openmailclient_model::{FolderId, Item, ItemId};
use serde_json::{Value, json};

/// The name our view goes by among a folder's views.
pub const NAME: &str = "OpenMailClient";
/// Our own property set, for what a view binary has no place for: columns with no property
/// of their own (Notes, Preview) and the reading pane.
const OURS: [u8; 16] = [0x4F, 0x4D, 0x43, 0x56, 0x69, 0x65, 0x77, 0x31, 0x9A, 0x2C, 0x5B, 0x11, 0x7E, 0x40, 0xD3, 0x06];
/// Folders whose views the settings keep, at most.
pub const MAX_KEPT: usize = 4096;
/// A folder's hidden items looked through for a view, at most.
const MAX_LOOKED: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderView {
    pub sort: SortKey,
    pub descending: bool,
    /// Never none.
    pub columns: Vec<Field>,
    pub reading_pane: ReadingPane,
}

impl Default for FolderView {
    /// Outlook's single-line view as a folder first has it.
    fn default() -> Self {
        Self { sort: SortKey::Date, descending: true, columns: Field::LIST.to_vec(), reading_pane: ReadingPane::Right }
    }
}

impl FolderView {
    /// The view the session shows now.
    pub fn of(s: &Session) -> Self {
        Self { sort: s.sort, descending: s.sort_desc, columns: s.list_columns.clone(), reading_pane: s.reading_pane }
    }

    /// Show this view: the list's columns and Table Style's the same, as `view.columns` does.
    pub fn show(&self, s: &mut Session) {
        s.sort = self.sort;
        s.sort_desc = self.descending;
        // Table Style's columns follow the list's when these change, and are else its own.
        if !self.columns.is_empty() && self.columns != s.list_columns {
            s.list_columns = self.columns.clone();
            s.print_list.columns = self.columns.clone();
        }
        s.reading_pane = self.reading_pane;
    }

    pub fn to_json(&self) -> Value {
        json!({
            "sort": self.sort.name(),
            "descending": self.descending,
            "columns": self.columns.iter().map(|f| f.name()).collect::<Vec<_>>(),
            "readingPane": pane_name(self.reading_pane),
        })
    }

    /// As [`FolderView::to_json`] wrote it; none when any part makes no sense.
    pub fn from_json(v: &Value) -> Option<Self> {
        let columns: Vec<Field> =
            v.get("columns")?.as_array()?.iter().take(Field::ALL.len()).map(|c| c.as_str().and_then(Field::named)).collect::<Option<_>>()?;
        let mut seen = Vec::with_capacity(columns.len());
        for c in columns {
            if !seen.contains(&c) {
                seen.push(c);
            }
        }
        (!seen.is_empty()).then_some(())?;
        Some(Self {
            sort: SortKey::parse(v.get("sort")?.as_str()?)?,
            descending: v.get("descending")?.as_bool()?,
            columns: seen,
            reading_pane: pane_named(v.get("readingPane")?.as_str()?)?,
        })
    }
}

fn pane_name(p: ReadingPane) -> &'static str {
    match p {
        ReadingPane::Right => "right",
        ReadingPane::Bottom => "bottom",
        ReadingPane::Off => "off",
    }
}

fn pane_named(name: &str) -> Option<ReadingPane> {
    match name {
        "right" => Some(ReadingPane::Right),
        "bottom" => Some(ReadingPane::Bottom),
        "off" => Some(ReadingPane::Off),
        _ => None,
    }
}

fn ours() -> NamedKey {
    NamedKey { guid: OURS, name: NameId::Name("View".to_string()) }
}

/// The property a column shows ([MS-OXPROPS]): its tag, width in pixels and heading. Notes and
/// Preview have none.
fn column(f: Field) -> Option<Column> {
    let (tag, width, title) = match f {
        Field::Importance => (0x0017_0003, 20, "Importance"),
        Field::Attachment => (0x0E1B_000B, 20, "Attachment"),
        Field::Who | Field::From => (0x0042_001F, 140, "From"),
        Field::To => (0x0E04_001F, 140, "To"),
        Field::Subject => (0x0037_001F, 300, "Subject"),
        Field::Date => (0x0E06_0040, 120, "Received"),
        Field::Size => (0x0E08_0003, 60, "Size"),
        Field::Flag => (0x1090_0003, 20, "Flag Status"),
        Field::Categories => return Some(Column::named(NamedKey::keywords(), 0x101F, 90, "Categories")),
        Field::Notes | Field::Preview => return None,
    };
    Some(Column::of(tag, width, title))
}

/// The field a column shows, if it is one the list has.
fn field(c: &Column) -> Option<Field> {
    if let Some(key) = &c.named {
        return (*key == NamedKey::keywords()).then_some(Field::Categories);
    }
    Some(match c.id()? {
        0x0017 => Field::Importance,
        0x0E1B => Field::Attachment,
        0x0042 => Field::Who,
        0x0C1A => Field::From,
        0x0E04 => Field::To,
        0x0037 => Field::Subject,
        0x0E06 | 0x0039 => Field::Date,
        0x0E08 => Field::Size,
        0x1090 => Field::Flag,
        _ => return None,
    })
}

/// The column the list is arranged by.
fn sort_field(key: SortKey, columns: &[Field]) -> Field {
    match key {
        SortKey::Date => Field::Date,
        SortKey::From if columns.contains(&Field::From) => Field::From,
        SortKey::From => Field::Who,
        SortKey::To => Field::To,
        SortKey::Subject => Field::Subject,
        SortKey::Size => Field::Size,
        SortKey::Importance => Field::Importance,
        SortKey::Flag => Field::Flag,
        SortKey::Attachments => Field::Attachment,
    }
}

/// The view as Outlook keeps one: the columns shown that have a property, the one sorted by
/// after them when it is not shown, and which of them that is.
fn binary(v: &FolderView) -> ViewDef {
    let mut columns: Vec<Column> = Vec::new();
    for c in v.columns.iter().filter_map(|f| column(*f)) {
        if !columns.contains(&c) {
            columns.push(c);
        }
    }
    let want = column(sort_field(v.sort, &v.columns));
    let sort = match want {
        Some(want) => match columns.iter().position(|c| *c == want) {
            Some(i) => Some(i),
            None => {
                columns.push(want);
                Some(columns.len().saturating_sub(1))
            }
        },
        None => None,
    };
    ViewDef { columns, sort, descending: v.descending, ..ViewDef::default() }
}

/// The hidden message that keeps `v` in a folder.
pub fn to_item(v: &FolderView) -> Item {
    let (bin, strings) = view_def::write(&binary(v));
    let mut item = Item::default();
    item.props.set(pid::MESSAGE_CLASS, PropValue::String(view_def::CLASS.to_string()));
    item.props.set(view_def::NAME, PropValue::String(NAME.to_string()));
    item.props.set(view_def::VERSION, PropValue::Int32(8));
    item.props.set(view_def::BINARY, PropValue::Binary(bin));
    item.props.set(view_def::STRINGS, PropValue::String(strings));
    item.named.push((ours(), PropValue::String(v.to_json().to_string())));
    item
}

/// What a view message says, put over `base`: all of it from one of ours, unless Outlook
/// has changed it since; from another, what is sorted by and the columns it has a field for.
/// None when it is not a view, or not one that can be read.
pub fn from_item(item: &Item, base: &FolderView) -> Option<FolderView> {
    (item.props.str(pid::MESSAGE_CLASS)? == view_def::CLASS).then_some(())?;
    let Some(PropValue::Binary(bin)) = item.props.get(view_def::BINARY) else { return None };
    let def = view_def::read(bin, item.props.str(view_def::STRINGS))?;
    let kept = item.named.iter().find(|(k, _)| *k == ours()).and_then(|(_, v)| match v {
        PropValue::String(s) => serde_json::from_str::<Value>(s).ok().as_ref().and_then(FolderView::from_json),
        _ => None,
    });
    // Ours, unless Outlook has since changed what it shows or is sorted by.
    if let Some(kept) = kept
        && summary(&binary(&kept)) == summary(&def)
    {
        return Some(kept);
    }
    let mut view = base.clone();
    let sort = def.sort.and_then(|i| def.columns.get(i)).and_then(field);
    let mut shown: Vec<Field> = Vec::new();
    for f in def.columns.iter().filter_map(field) {
        if !shown.contains(&f) {
            shown.push(f);
        }
    }
    if !shown.is_empty() {
        view.columns = shown;
    }
    if let Some(key) = sort.and_then(|f| f.sort_key(false)).and_then(SortKey::parse) {
        view.sort = key;
        view.descending = def.descending;
    }
    Some(view)
}

/// What a view shows and is sorted by, its widths and headings aside.
fn summary(def: &ViewDef) -> (Vec<Option<Field>>, Option<usize>, bool) {
    (def.columns.iter().map(field).collect(), def.sort, def.descending)
}

/// What the settings know a folder by: its data file, or the store's name, and its id.
pub fn key(s: &Session, store: usize, folder: FolderId) -> Option<String> {
    let st = s.stores.get(store)?;
    let file = st.path().map(|p| p.display().to_string()).unwrap_or_else(|| format!("({})", st.display_name()));
    Some(format!("{file}#{:x}", folder.0))
}

/// The view a folder's data file keeps, ours first, else the first of Outlook's, with the
/// hidden message ours is in.
fn in_file(s: &mut Session, store: usize, folder: FolderId, base: &FolderView) -> (Option<FolderView>, Option<ItemId>) {
    let Some(st) = s.stores.get_mut(store) else { return (None, None) };
    let Ok(hidden) = st.hidden(folder) else { return (None, None) };
    let mut theirs = None;
    for (id, item) in hidden.into_iter().take(MAX_LOOKED) {
        let Ok(item) = item else { continue };
        let Some(view) = from_item(&item, base) else { continue };
        if item.props.str(view_def::NAME) == Some(NAME) {
            return (Some(view), Some(id));
        }
        theirs = theirs.or(Some(view));
    }
    (theirs, None)
}

/// Show the view a folder keeps, as it is opened: from its data file the first time it is
/// opened in a run, else as the settings keep it. One with none keeps the view shown.
pub fn open(s: &mut Session, store: usize, folder: FolderId) {
    let Some(key) = key(s, store, folder) else { return };
    // A view changed here and not yet written goes before what the file says.
    if s.views_looked.insert(key.clone()) && !s.views_unsaved.contains(&key) {
        let base = s.folder_views.get(&key).cloned().unwrap_or_else(|| FolderView::of(s));
        if let (Some(view), _) = in_file(s, store, folder, &base) {
            remember(s, key.clone(), view);
        }
    }
    if let Some(view) = s.folder_views.get(&key).cloned() {
        view.show(s);
    }
}

fn remember(s: &mut Session, key: String, view: FolderView) {
    if !s.folder_views.contains_key(&key) && s.folder_views.len() >= MAX_KEPT {
        s.folder_views.pop_first();
    }
    s.folder_views.insert(key, view);
}

/// Keep the view shown as the open folder's, in the settings, to be written into its data
/// file with the file's next save.
pub fn keep(s: &mut Session) {
    let Some((store, folder)) = s.current else { return };
    let Some(key) = key(s, store, folder) else { return };
    let view = FolderView::of(s);
    s.views_looked.insert(key.clone());
    if s.folder_views.get(&key) == Some(&view) {
        return;
    }
    remember(s, key.clone(), view);
    if s.views_unsaved.len() < MAX_KEPT {
        s.views_unsaved.insert(key);
    }
}

/// The folders of data file `store` whose views are not yet written into it, by key and id.
fn unwritten(s: &Session, store: usize) -> Vec<(String, FolderId)> {
    let Some(st) = s.stores.get(store) else { return Vec::new() };
    if st.path().is_none() {
        return Vec::new();
    }
    let Some(file) = key(s, store, FolderId(0)).and_then(|k| k.strip_suffix("#0").map(|f| format!("{f}#"))) else { return Vec::new() };
    s.views_unsaved
        .iter()
        .filter_map(|k| Some((k.clone(), FolderId(u64::from_str_radix(k.strip_prefix(&file)?, 16).ok()?))))
        .filter(|(_, f)| s.folder_info(store, *f).is_some())
        .collect()
}

/// Whether data file `store` has views not yet written into it.
pub fn unsaved_in(s: &Session, store: usize) -> bool {
    !unwritten(s, store).is_empty()
}

/// Write the views of data file `store` not yet written into it, as it is saved: in place of
/// ours where the folder has one. One that cannot be written stays in the settings.
pub fn write_into(s: &mut Session, store: usize) {
    for (key, folder) in unwritten(s, store) {
        let Some(view) = s.folder_views.get(&key).cloned() else { continue };
        let (stored, id) = in_file(s, store, folder, &view);
        let done = (id.is_some() && stored.as_ref() == Some(&view))
            || s.stores.get_mut(store).is_some_and(|st| st.put_hidden(folder, id, to_item(&view)).is_ok());
        if done {
            s.views_unsaved.remove(&key);
        }
    }
}
