//! The People module: contacts and contact groups of a Contacts folder ([MS-OXOCNTC]) listed,
//! found, made, changed and deleted; vCard files (`.vcf`) opened and saved; a contact sent on
//! as a card, and mail started to a contact or a group. The model is
//! `openmailclient_model::contact`; the file format `openmailclient_formats::vcf`.
//!
//! A card opened again is known by its UID, or else by its e-mail address, and brings the
//! contact up to date instead of being added twice; what the card does not say is kept.

use super::{CommandSpec, always, need_str, ok, p_bool, p_str, p_u64};
use crate::{CmdResult, Draft, EngineError, Session, io, undo::Step};
use openmailclient_formats::vcf::{self, Card};
use openmailclient_model::contact::{self, Address, Contact, Email, Group, MAX_EMAILS, MAX_MEMBERS, MAX_PICTURE, Member, Phone, Place};
use openmailclient_model::{Attachment, FolderId, FolderKind, Item, ItemId, StoreError};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;

/// Items of a Contacts folder read at most.
const MAX_ITEMS: usize = 20_000;
/// Longest field taken, in characters, and longest notes.
const MAX_TEXT: usize = 4_000;
const MAX_NOTES: usize = 200_000;
/// Longest address taken, in characters (RFC 5321 4.5.3.1 allows 254).
const MAX_ADDRESS: usize = 320;
/// Contacts folders listed at most.
const MAX_FOLDERS: usize = 500;

/// How the People module shows its contacts, as Outlook's Current View offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    /// A list of names beside the chosen contact's card.
    #[default]
    People,
    BusinessCard,
    Card,
    Phone,
    List,
}

impl View {
    pub const ALL: [Self; 5] = [Self::People, Self::BusinessCard, Self::Card, Self::Phone, Self::List];

    pub fn name(self) -> &'static str {
        match self {
            Self::People => "people",
            Self::BusinessCard => "businessCard",
            Self::Card => "card",
            Self::Phone => "phone",
            Self::List => "list",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::People => "People",
            Self::BusinessCard => "Business Card",
            Self::Card => "Card",
            Self::Phone => "Phone",
            Self::List => "List",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.name().eq_ignore_ascii_case(s) || v.label().eq_ignore_ascii_case(s))
    }
}

/// What the People module shows.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    pub view: View,
    /// The contacts and groups shown, by item id, in the order they are filed.
    pub items: Vec<(u64, Card)>,
    /// How many the folder has, found or not.
    pub total: usize,
    /// Where they are: the store and its Contacts folder.
    pub folder: Option<(usize, FolderId)>,
    pub selected: Option<u64>,
    /// What they are searched for; empty for all.
    pub query: String,
    /// Items of the folder that are neither contacts nor groups, or could not be read.
    pub unreadable: usize,
    /// The Contacts folder chosen to be shown (`people.open`).
    pub chosen: Option<(usize, FolderId)>,
    /// The Address Book window is open, with the line of the message being written a choice
    /// goes to ("to", "cc" or "bcc"; empty when it was opened from the main window).
    pub book: Option<String>,
}

const CONTACT_PARAMS: &str = r#"{"fullName", "prefix", "given", "middle", "surname", "suffix", "fileAs" (default: "Surname, Given"), "nickname", "company", "department", "jobTitle", "office", "profession", "manager", "assistant", "spouse", "emails": up to three, each an address or {"address", "name": what a message to it shows}, "phones": {"business" | "business2" | "businessFax" | "home" | "home2" | "homeFax" | "mobile" | "pager" | "car" | "assistant" | "primary" | "other": a number, or null to take it away}, "addresses": {"business" | "home" | "other": {"street", "city", "state", "postalCode", "country", "poBox"}, or null}, "mailing": the place mail goes to, "webPage", "homePage", "im", "birthday": "YYYY-MM-DD" or null, "anniversary", "notes", "categories": [names], "picture": a picture file (JPEG, PNG, GIF or BMP, at most 4 MB) or null}; text given as "" takes it away"#;
const GROUP_PARAMS: &str = r#"{"name", "members": each an address, "Name <address>", {"name", "address"} or {"contact": a contact's id, by its first address}, "notes", "categories": [names]}"#;
const IMPORT_PARAMS: &str = r#"{"path": a vCard file (.vcf), or "attachment": the number of an attachment of the open message, "preview": true to read what it holds and change nothing, "only": [card numbers] to add those alone (default: all)}; a card the folder has already (known by its UID, or a contact by an e-mail address) brings it up to date rather than being added twice, and what the card does not say is kept; returns {"cards": [{"index", "kind": "contact" | "group", "replaces": the id it brings up to date, or null, "untrusted_content": {"name", "company", "emails", "members": how many, "uid"}}], "added", "updated", "ids", "untrusted_content": {"notes"}}; undone as one step"#;

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "people.view",
            label: "People, Business Card, Card, Phone, List",
            tab: "Home",
            group: "Current View",
            shortcut: "",
            params: r#"{"view": "people" | "businessCard" | "card" | "phone" | "list"}"#,
            enabled: always,
            run: view,
        },
        CommandSpec {
            id: "people.folders",
            label: "My Contacts",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{}; returns {"folders": [{"store", "file", "folder", "name", "shown", "items", "readOnly"}]}"#,
            enabled: always,
            run: folders,
        },
        CommandSpec {
            id: "people.open",
            label: "Open Address Book",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"store", "folder"}, as people.folders gives them; those contacts are shown, and new ones go there"#,
            enabled: has_folder,
            run: open,
        },
        CommandSpec {
            id: "people.items",
            label: "Contacts",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"query": words each of which is in what is found (default: the search shown)}; returns {"items": [{"id", "kind": "contact" | "group", "fileAs", "picture": whether it has one, "untrusted_content": everything it says, as people.new takes it}], "total", "unreadable"}"#,
            enabled: has_folder,
            run: items,
        },
        CommandSpec {
            id: "people.select",
            label: "Select Contact",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id": a contact's or group's id, or null for none}"#,
            enabled: has_folder,
            run: select,
        },
        CommandSpec {
            id: "people.search",
            label: "Search People",
            tab: "Home",
            group: "Find",
            shortcut: "Ctrl+E",
            params: r#"{"query": words, each of which is in a name, company, address, number, category or note shown; "" for all}"#,
            enabled: has_folder,
            run: search,
        },
        CommandSpec {
            id: "people.new",
            label: "New Contact",
            tab: "Home",
            group: "New",
            shortcut: "Ctrl+Shift+C",
            params: CONTACT_PARAMS,
            enabled: has_folder_writable,
            run: new,
        },
        CommandSpec {
            id: "people.change",
            label: "Edit Contact",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id" (default: the one chosen), and any of those people.new takes}"#,
            enabled: has_contact,
            run: change,
        },
        CommandSpec {
            id: "people.newGroup",
            label: "New Contact Group",
            tab: "Home",
            group: "New",
            shortcut: "Ctrl+Shift+L",
            params: GROUP_PARAMS,
            enabled: has_folder_writable,
            run: new_group,
        },
        CommandSpec {
            id: "people.changeGroup",
            label: "Edit Contact Group",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id" (default: the one chosen), any of those people.newGroup takes ("members" takes the place of all), "add": members to add, "remove": addresses to take away}"#,
            enabled: has_contact,
            run: change_group,
        },
        CommandSpec {
            id: "people.delete",
            label: "Delete",
            tab: "Home",
            group: "Delete",
            shortcut: "Ctrl+D",
            params: r#"{"id" (default: the one chosen)}; it goes to Deleted Items"#,
            enabled: has_contact,
            run: delete,
        },
        CommandSpec {
            id: "people.import",
            label: "Import a vCard File",
            tab: "File",
            group: "Open & Export",
            shortcut: "",
            params: IMPORT_PARAMS,
            enabled: always,
            run: import,
        },
        CommandSpec {
            id: "people.saveAs",
            label: "Save as vCard",
            tab: "File",
            group: "Save As",
            shortcut: "",
            params: r#"{"path": target ending in .vcf, "ids": [contacts and groups] (default: the one chosen, or with "all": true every one of the folder)}; contacts are written as vCard 3.0, as Outlook reads them, and groups as 4.0; returns {"cards", "untrusted_content": {"path"}}"#,
            enabled: has_folder,
            run: save_as,
        },
        CommandSpec {
            id: "people.forward",
            label: "Forward Contact",
            tab: "Home",
            group: "Share",
            shortcut: "",
            params: r#"{"ids" (default: the one chosen)}; a new message with each attached as a vCard and what it says in the body"#,
            enabled: has_contact,
            run: forward,
        },
        CommandSpec {
            id: "people.email",
            label: "Email",
            tab: "Home",
            group: "Communicate",
            shortcut: "",
            params: r#"{"ids" (default: the one chosen)}; a new message to each contact's first address and to each member of a group"#,
            enabled: has_contact,
            run: email,
        },
        CommandSpec {
            id: "mail.addSenderToContacts",
            label: "Add to Outlook Contacts",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{}; the open message's sender is made a contact, or the contact with that address is chosen; returns {"id", "added": whether it is new}"#,
            enabled: has_sender,
            run: add_sender,
        },
    ]
}

/// The Contacts folder shown: the one chosen, else the current data file's, else the first.
pub(crate) fn people_folder(s: &Session) -> Option<(usize, FolderId)> {
    if let Some((store, folder)) = s.people.chosen
        && is_contacts(s, store, folder)
    {
        return Some((store, folder));
    }
    let current = s.current.map(|(store, _)| store);
    current.into_iter().chain(0..s.stores.len()).find_map(|store| s.find_folder(store, FolderKind::Contacts).map(|f| (store, f)))
}

fn is_contacts(s: &Session, store: usize, folder: FolderId) -> bool {
    s.folders.get(store).is_some_and(|f| f.iter().any(|x| x.id == folder && x.kind == FolderKind::Contacts))
}

fn has_folder(s: &Session) -> bool {
    people_folder(s).is_some()
}

fn has_folder_writable(s: &Session) -> bool {
    people_folder(s).is_some_and(|(store, _)| s.stores.get(store).is_some_and(|st| st.writable()))
}

/// A contact or group is chosen, or may be named by its id.
fn has_contact(s: &Session) -> bool {
    has_folder(s)
}

fn has_sender(s: &Session) -> bool {
    s.open.as_ref().is_some_and(|m| m.sender_email().contains('@')) && has_folder_writable(s)
}

fn writable_folder(s: &Session) -> Result<(usize, FolderId), EngineError> {
    let (store, folder) = people_folder(s).ok_or_else(|| EngineError::BadParams("no data file open has a Contacts folder".into()))?;
    if !s.stores.get(store).is_some_and(|st| st.writable()) {
        return Err(EngineError::BadParams("the data file with the contacts cannot be written".into()));
    }
    Ok((store, folder))
}

/// What an item of a Contacts folder is, when it is a contact or a group.
pub fn card_of(item: &Item, s: &Session) -> Option<Card> {
    if contact::is_group(item) {
        Some(Card::Group(contact::Group::of(item)))
    } else if contact::is_contact(item) {
        Some(Card::Contact(Box::new(Contact::of(item, &s.zone))))
    } else {
        None
    }
}

fn card_matches(card: &Card, query: &str) -> bool {
    match card {
        Card::Contact(c) => c.matches(query),
        Card::Group(g) => g.matches(query),
    }
}

fn sort_key(card: &Card) -> String {
    match card {
        Card::Contact(c) => c.sort_key(),
        Card::Group(g) => g.name.to_lowercase(),
    }
}

impl Session {
    /// Read again the contacts and groups of the Contacts folder shown.
    pub fn reload_people(&mut self) -> Result<(), EngineError> {
        let folder = people_folder(self);
        self.people.folder = folder;
        let Some((store, folder)) = folder else {
            self.people.items.clear();
            self.people.total = 0;
            self.people.unreadable = 0;
            self.people.selected = None;
            return Ok(());
        };
        let (found, total, unreadable) = self.cards_of(store, folder, &self.people.query.clone())?;
        // As Outlook does, someone is always chosen: the one that was, else the one that took its
        // place in the list (as after a delete), else the first.
        if !self.people.selected.is_some_and(|id| found.iter().any(|(i, _)| *i == id)) {
            let at = self.people.selected.and_then(|id| self.people.items.iter().position(|(i, _)| *i == id)).unwrap_or(0);
            self.people.selected = found.get(at.min(found.len().saturating_sub(1))).map(|(id, _)| *id);
        }
        self.people.items = found;
        self.people.total = total;
        self.people.unreadable = unreadable;
        Ok(())
    }

    /// The contacts and groups of a folder that `query` finds, in the order they are filed;
    /// how many the folder has, and how many items of it are neither.
    fn cards_of(&mut self, store: usize, folder: FolderId, query: &str) -> Result<(Vec<(u64, Card)>, usize, usize), EngineError> {
        let zone = self.zone.clone();
        let st = self.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
        let mut found = Vec::new();
        let (mut total, mut unreadable) = (0usize, 0usize);
        for row in st.list(folder)?.into_iter().take(MAX_ITEMS) {
            let Ok(item) = st.load(folder, ItemId(row.id)) else {
                unreadable = unreadable.saturating_add(1);
                continue;
            };
            let card = if contact::is_group(&item) {
                Card::Group(Group::of(&item))
            } else if contact::is_contact(&item) {
                Card::Contact(Box::new(Contact::of(&item, &zone)))
            } else {
                unreadable = unreadable.saturating_add(1);
                continue;
            };
            total = total.saturating_add(1);
            if query.trim().is_empty() || card_matches(&card, query) {
                found.push((row.id, card));
            }
        }
        found.sort_by_cached_key(|(id, c)| (sort_key(c), *id));
        Ok((found, total, unreadable))
    }

    /// The contact or group `id` of the folder shown, as it is kept.
    pub fn person(&mut self, id: u64) -> Result<Card, EngineError> {
        let (store, folder) = people_folder(self).ok_or(StoreError::NoFolder)?;
        let item = self.stores.get_mut(store).ok_or(StoreError::NoFolder)?.load(folder, ItemId(id))?;
        card_of(&item, self).ok_or_else(|| EngineError::BadParams("that item is not a contact or a group".into()))
    }
}

fn view(s: &mut Session, p: &Value) -> CmdResult {
    let name = need_str(p, "view")?;
    s.people.view =
        View::parse(name).ok_or_else(|| EngineError::BadParams(format!("\"{name}\" is not people, businessCard, card, phone or list")))?;
    ok()
}

fn folders(s: &mut Session, _: &Value) -> CmdResult {
    let shown = people_folder(s);
    let mut out = Vec::new();
    for (store, list) in s.folders.iter().enumerate() {
        let file = s.stores.get(store).map(|st| st.display_name()).unwrap_or_default();
        let read_only = !s.stores.get(store).is_some_and(|st| st.writable());
        for f in list.iter().filter(|f| f.kind == FolderKind::Contacts).take(MAX_FOLDERS.saturating_sub(out.len())) {
            out.push(json!({
                "store": store,
                "file": file,
                "folder": f.id.0,
                "name": f.name,
                "shown": shown == Some((store, f.id)),
                "items": f.total,
                "readOnly": read_only,
            }));
        }
    }
    Ok(json!({ "folders": out }))
}

fn open(s: &mut Session, p: &Value) -> CmdResult {
    let store = p_u64(p, "store")
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| EngineError::BadParams("\"store\" is a data file's number, as people.folders gives it".into()))?;
    let folder = FolderId(
        p_u64(p, "folder").ok_or_else(|| EngineError::BadParams("\"folder\" is a Contacts folder's id, as people.folders gives it".into()))?,
    );
    if !is_contacts(s, store, folder) {
        return Err(EngineError::BadParams("that is not a Contacts folder of a data file open".into()));
    }
    s.people.chosen = Some((store, folder));
    s.people.selected = None;
    s.reload_people()?;
    s.status = format!("Showing {}", s.folder_info(store, folder).map(|f| f.name.clone()).unwrap_or_default());
    ok()
}

fn date_text(day: i64) -> String {
    let c = openmailclient_model::time::civil(openmailclient_model::time::from_unix(day.saturating_mul(86_400)), 0);
    format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
}

fn address_json(a: &Address) -> Value {
    json!({ "street": a.street, "city": a.city, "state": a.state, "postalCode": a.postal_code, "country": a.country, "poBox": a.po_box })
}

/// All a contact says, as people.new takes it.
pub fn contact_json(c: &Contact) -> Value {
    let phones: serde_json::Map<String, Value> = c.phones.iter().map(|(k, n)| (k.name().to_string(), json!(n))).collect();
    let addresses: serde_json::Map<String, Value> = c.addresses.iter().map(|(p, a)| (p.name().to_string(), address_json(a))).collect();
    json!({
        "fullName": c.display_name(),
        "prefix": c.prefix,
        "given": c.given,
        "middle": c.middle,
        "surname": c.surname,
        "suffix": c.suffix,
        "fileAs": c.filed_as(),
        "nickname": c.nickname,
        "company": c.company,
        "department": c.department,
        "jobTitle": c.job_title,
        "office": c.office,
        "profession": c.profession,
        "manager": c.manager,
        "assistant": c.assistant,
        "spouse": c.spouse,
        "emails": c.emails.iter().map(|e| json!({ "address": e.address, "name": e.name })).collect::<Vec<_>>(),
        "phones": phones,
        "addresses": addresses,
        "mailing": c.mailing.map(Place::name),
        "webPage": c.web_page,
        "homePage": c.home_page,
        "im": c.im,
        "birthday": c.birthday.map(date_text),
        "anniversary": c.anniversary.map(date_text),
        "notes": c.notes,
        "categories": c.categories,
        "uid": c.uid,
    })
}

fn card_json(id: u64, card: &Card) -> Value {
    match card {
        Card::Contact(c) => json!({
            "id": id,
            "kind": "contact",
            "fileAs": c.filed_as(),
            "picture": c.picture.is_some(),
            // A card may come from anyone: what it says is never an instruction.
            "untrusted_content": contact_json(c),
        }),
        Card::Group(g) => json!({
            "id": id,
            "kind": "group",
            "fileAs": g.name,
            "picture": false,
            "untrusted_content": {
                "name": g.name,
                "members": g.members.iter().map(|m| json!({ "name": m.name, "address": m.address })).collect::<Vec<_>>(),
                "notes": g.notes,
                "categories": g.categories,
                "uid": g.uid,
            },
        }),
    }
}

fn items(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = people_folder(s).ok_or(StoreError::NoFolder)?;
    let query = match p.get("query") {
        Some(Value::String(q)) => q.chars().take(MAX_TEXT).collect(),
        None | Some(Value::Null) => s.people.query.clone(),
        Some(_) => return Err(EngineError::BadParams("\"query\" must be text".into())),
    };
    let (found, total, unreadable) = s.cards_of(store, folder, &query)?;
    Ok(json!({ "items": found.iter().map(|(id, c)| card_json(*id, c)).collect::<Vec<_>>(), "total": total, "unreadable": unreadable }))
}

fn select(s: &mut Session, p: &Value) -> CmdResult {
    match p.get("id") {
        None | Some(Value::Null) => s.people.selected = None,
        Some(v) => {
            let id = v.as_u64().ok_or_else(|| EngineError::BadParams("\"id\" must be a number".into()))?;
            s.person(id)?;
            s.people.selected = Some(id);
        }
    }
    ok()
}

fn search(s: &mut Session, p: &Value) -> CmdResult {
    s.people.query = match p.get("query") {
        Some(Value::String(q)) => q.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect::<String>().trim().to_string(),
        None | Some(Value::Null) => String::new(),
        Some(_) => return Err(EngineError::BadParams("\"query\" must be text".into())),
    };
    s.reload_people()?;
    s.status = if s.people.query.is_empty() { String::new() } else { format!("{} found", s.people.items.len()) };
    ok()
}

/// One line of text: control characters left out, cut, trimmed.
pub(super) fn line_param(p: &Value, key: &str) -> Result<Option<String>, EngineError> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(String::new())),
        Some(Value::String(t)) => Ok(Some(t.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect::<String>().trim().to_string())),
        Some(_) => Err(EngineError::BadParams(format!("\"{key}\" must be text"))),
    }
}

/// An e-mail address as it is taken: one line, with an `@`, and not longer than any can be.
fn address_param(v: &str) -> Result<String, EngineError> {
    let a = v.trim();
    let bad = a.is_empty()
        || a.chars().count() > MAX_ADDRESS
        || !a.contains('@')
        || a.chars().any(|c| c.is_control() || c.is_whitespace() || "<>,;\"".contains(c));
    if bad {
        return Err(EngineError::BadParams(format!("\"{}\" is not an e-mail address", a.chars().take(80).collect::<String>())));
    }
    Ok(a.to_string())
}

pub(super) fn categories_param(v: &Value) -> Result<Vec<String>, EngineError> {
    let bad = || EngineError::BadParams("\"categories\" is a list of names".into());
    let list = v.as_array().ok_or_else(bad)?;
    let mut out = Vec::new();
    for c in list.iter().take(200) {
        let name: String = c.as_str().ok_or_else(bad)?.chars().filter(|c| !c.is_control()).take(255).collect();
        let name = name.trim().to_string();
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    }
    Ok(out)
}

fn day_param(s: &Session, p: &Value, key: &str) -> Result<Option<Option<i64>>, EngineError> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(t)) if t.trim().is_empty() => Ok(Some(None)),
        Some(Value::String(_)) => Ok(Some(super::calendar::date_param(s, p, key)?)),
        Some(_) => Err(EngineError::BadParams(format!("\"{key}\" is a date written YYYY-MM-DD, or null"))),
    }
}

/// A picture file read, no larger than a contact keeps, and one of the kinds it shows.
fn picture_param(v: &Value) -> Result<Option<Vec<u8>>, EngineError> {
    let path = match v {
        Value::Null => return Ok(None),
        Value::String(p) if p.trim().is_empty() => return Ok(None),
        Value::String(p) => Path::new(p.as_str()),
        _ => return Err(EngineError::BadParams("\"picture\" is a picture file's path, or null".into())),
    };
    let file = std::fs::File::open(path)?;
    if file.metadata()?.is_dir() {
        return Err(EngineError::BadParams("\"picture\" is a folder, not a picture".into()));
    }
    let mut data = Vec::new();
    file.take(u64::try_from(MAX_PICTURE).unwrap_or(u64::MAX).saturating_add(1)).read_to_end(&mut data)?;
    if data.len() > MAX_PICTURE {
        return Err(EngineError::BadParams("the picture is larger than 4 MB".into()));
    }
    let known =
        data.starts_with(&[0xFF, 0xD8, 0xFF]) || data.starts_with(&[0x89, b'P', b'N', b'G']) || data.starts_with(b"GIF8") || data.starts_with(b"BM");
    if !known {
        return Err(EngineError::BadParams("the picture is not a JPEG, PNG, GIF or BMP file".into()));
    }
    Ok(Some(data))
}

fn address_object(v: &Value, was: Option<&Address>) -> Result<Address, EngineError> {
    let o = v
        .as_object()
        .ok_or_else(|| EngineError::BadParams("an address is {\"street\", \"city\", \"state\", \"postalCode\", \"country\", \"poBox\"}".into()))?;
    let mut a = was.cloned().unwrap_or_default();
    for (key, field) in [
        ("street", &mut a.street),
        ("city", &mut a.city),
        ("state", &mut a.state),
        ("postalCode", &mut a.postal_code),
        ("country", &mut a.country),
        ("poBox", &mut a.po_box),
    ] {
        match o.get(key) {
            None => {}
            Some(Value::Null) => field.clear(),
            // A street may have more than one line.
            Some(Value::String(t)) => {
                *field = t.chars().filter(|c| !c.is_control() || *c == '\n').take(MAX_TEXT).collect::<String>().trim().to_string()
            }
            Some(_) => return Err(EngineError::BadParams(format!("\"{key}\" of an address must be text"))),
        }
    }
    Ok(a)
}

/// The contact `p` makes of `c`: each field given takes the place of what it was.
fn changed_contact(s: &Session, p: &Value, mut c: Contact) -> Result<Contact, EngineError> {
    if !p.is_object() {
        return Err(EngineError::BadParams("the parameters are an object".into()));
    }
    let fields: [(&str, fn(&mut Contact) -> &mut String); 20] = [
        ("fullName", |c| &mut c.full_name),
        ("prefix", |c| &mut c.prefix),
        ("given", |c| &mut c.given),
        ("middle", |c| &mut c.middle),
        ("surname", |c| &mut c.surname),
        ("suffix", |c| &mut c.suffix),
        ("fileAs", |c| &mut c.file_as),
        ("nickname", |c| &mut c.nickname),
        ("company", |c| &mut c.company),
        ("department", |c| &mut c.department),
        ("jobTitle", |c| &mut c.job_title),
        ("office", |c| &mut c.office),
        ("profession", |c| &mut c.profession),
        ("manager", |c| &mut c.manager),
        ("assistant", |c| &mut c.assistant),
        ("spouse", |c| &mut c.spouse),
        ("webPage", |c| &mut c.web_page),
        ("homePage", |c| &mut c.home_page),
        ("im", |c| &mut c.im),
        ("uid", |c| &mut c.uid),
    ];
    // The parts of a name given without the whole make it again.
    let parts_given = ["prefix", "given", "middle", "surname", "suffix"].iter().any(|k| p.get(*k).is_some());
    for (key, field) in fields {
        if let Some(t) = line_param(p, key)? {
            *field(&mut c) = t;
        }
    }
    if parts_given && p.get("fullName").is_none() {
        c.full_name = c.name_from_parts();
    }
    if let Some(v) = p.get("notes") {
        c.notes = match v {
            Value::Null => String::new(),
            Value::String(t) => {
                t.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).take(MAX_NOTES).collect::<String>().trim_end().to_string()
            }
            _ => return Err(EngineError::BadParams("\"notes\" must be text".into())),
        };
    }
    if let Some(v) = p.get("emails") {
        let bad = || EngineError::BadParams("\"emails\" is a list of up to three addresses, each text or {\"address\", \"name\"}".into());
        let list = v.as_array().ok_or_else(bad)?;
        if list.len() > MAX_EMAILS {
            return Err(bad());
        }
        let mut emails = Vec::new();
        for e in list {
            let (address, name) = match e {
                Value::String(a) => (a.as_str(), ""),
                Value::Object(o) => (o.get("address").and_then(Value::as_str).ok_or_else(bad)?, o.get("name").and_then(Value::as_str).unwrap_or("")),
                _ => return Err(bad()),
            };
            let name: String = name.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect();
            emails.push(Email { address: address_param(address)?, name: name.trim().to_string() });
        }
        c.emails = emails;
    }
    if let Some(v) = p.get("phones") {
        let o = v.as_object().ok_or_else(|| EngineError::BadParams("\"phones\" is {\"mobile\": a number, …}".into()))?;
        for (kind, number) in o {
            let k = Phone::parse(kind).ok_or_else(|| {
                EngineError::BadParams(format!("\"{}\" is not a kind of telephone number", kind.chars().take(40).collect::<String>()))
            })?;
            match number {
                Value::Null => c.set_phone(k, ""),
                Value::String(n) => c.set_phone(k, &n.chars().filter(|c| !c.is_control()).take(100).collect::<String>()),
                _ => return Err(EngineError::BadParams("a telephone number must be text".into())),
            }
        }
    }
    if let Some(v) = p.get("addresses") {
        let o = v.as_object().ok_or_else(|| EngineError::BadParams("\"addresses\" is {\"home\": {…}, …}".into()))?;
        for (place, a) in o {
            let at = Place::parse(place).ok_or_else(|| EngineError::BadParams("an address is \"business\", \"home\" or \"other\"".into()))?;
            match a {
                Value::Null => c.set_address(at, Address::default()),
                v => {
                    let was = c.address(at).cloned();
                    c.set_address(at, address_object(v, was.as_ref())?);
                }
            }
        }
    }
    match p.get("mailing") {
        None => {}
        Some(Value::Null) => c.mailing = None,
        Some(Value::String(m)) => {
            let at = Place::parse(m).ok_or_else(|| EngineError::BadParams("\"mailing\" is \"business\", \"home\" or \"other\"".into()))?;
            if c.address(at).is_none() {
                return Err(EngineError::BadParams(format!("the contact has no {} address", at.name())));
            }
            c.mailing = Some(at);
        }
        Some(_) => return Err(EngineError::BadParams("\"mailing\" must be text".into())),
    }
    if let Some(d) = day_param(s, p, "birthday")? {
        c.birthday = d;
    }
    if let Some(d) = day_param(s, p, "anniversary")? {
        c.anniversary = d;
    }
    if let Some(v) = p.get("categories") {
        c.categories = categories_param(v)?;
    }
    if let Some(v) = p.get("picture") {
        c.picture = picture_param(v)?;
    }
    if c.display_name().is_empty() {
        return Err(EngineError::BadParams("a contact needs a name, a company or an e-mail address".into()));
    }
    Ok(c)
}

/// A new UID for a card made here (RFC 6350 6.7.6): random enough not to be made twice.
fn new_uid(s: &Session, seed: u64) -> String {
    let mut x = (s.time_now() as u64) ^ seed.rotate_left(29) ^ (std::process::id() as u64).rotate_left(47);
    let mut next = || {
        // SplitMix64.
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let (a, b) = (next(), next());
    let b = (b & 0x3FFF_FFFF_FFFF_FFFF) | 0x8000_0000_0000_0000;
    let a = (a & 0xFFFF_FFFF_FFFF_0FFF) | 0x4000;
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", a >> 32, (a >> 16) & 0xFFFF, a & 0xFFFF, b >> 48, b & 0xFFFF_FFFF_FFFF)
}

/// Puts a new item in the folder shown, undone as one step, and chooses it.
fn add_one(s: &mut Session, what: &'static str, item: Item) -> Result<u64, EngineError> {
    let (store, folder) = writable_folder(s)?;
    let id = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.add(folder, item)?.0;
    s.did(Step::Imported { what, store, folder, added: vec![id], updated: Vec::new() });
    s.refresh_folders(store);
    // A search that would hide it is let go.
    s.people.query.clear();
    s.people.selected = Some(id);
    s.reload_people()?;
    Ok(id)
}

fn new(s: &mut Session, p: &Value) -> CmdResult {
    let mut c = changed_contact(s, p, Contact::default())?;
    if c.uid.is_empty() {
        c.uid = new_uid(s, s.people.total as u64);
    }
    let name = c.display_name();
    let id = add_one(s, "New Contact", c.item(&s.zone))?;
    s.status = format!("Contact made: {name}");
    Ok(json!({ "id": id }))
}

fn chosen(s: &Session, p: &Value) -> Result<u64, EngineError> {
    p_u64(p, "id").or(s.people.selected).ok_or_else(|| EngineError::BadParams("no contact is chosen".into()))
}

/// Writes `f`'s change to item `id`, undone as one step.
fn edit_item(s: &mut Session, id: u64, what: &'static str, f: impl FnOnce(&Session, &mut Item) -> Result<(), EngineError>) -> CmdResult {
    let (store, folder) = writable_folder(s)?;
    let mut item = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.load(folder, ItemId(id))?;
    let before = Box::new(item.clone());
    f(s, &mut item)?;
    s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.update(folder, ItemId(id), item)?;
    s.did(Step::Edited { what, store, folder, id, before });
    s.refresh_folders(store);
    s.reload_people()?;
    ok()
}

fn change(s: &mut Session, p: &Value) -> CmdResult {
    let id = chosen(s, p)?;
    edit_item(s, id, "Edit Contact", |s, item| {
        if !contact::is_contact(item) {
            return Err(EngineError::BadParams("that is not a contact; a group is changed with people.changeGroup".into()));
        }
        let c = changed_contact(s, p, Contact::of(item, &s.zone))?;
        c.write(item, &s.zone);
        Ok(())
    })
}

/// The members a list of them names: addresses, "Name <address>", {"name", "address"} or a
/// contact by its id.
fn members_param(s: &mut Session, v: &Value) -> Result<Vec<Member>, EngineError> {
    let bad =
        || EngineError::BadParams("members are a list, each an address, \"Name <address>\", {\"name\", \"address\"} or {\"contact\": id}".into());
    let list = v.as_array().ok_or_else(bad)?;
    if list.len() > MAX_MEMBERS {
        return Err(EngineError::BadParams(format!("a group has at most {MAX_MEMBERS} members")));
    }
    let mut out = Vec::new();
    for m in list {
        match m {
            Value::String(text) => {
                let found = openmailclient_mime::parse_addresses(text);
                if found.is_empty() {
                    return Err(EngineError::BadParams(format!("\"{}\" is not an e-mail address", text.chars().take(80).collect::<String>())));
                }
                for (name, address) in found {
                    let address = address_param(&address)?;
                    out.push(Member {
                        name: if name.trim().is_empty() { address.clone() } else { name.trim().chars().take(MAX_TEXT).collect() },
                        address,
                    });
                }
            }
            Value::Object(o) if o.contains_key("contact") => {
                let id = o.get("contact").and_then(Value::as_u64).ok_or_else(bad)?;
                let Card::Contact(c) = s.person(id)? else {
                    return Err(EngineError::BadParams("a group is not put in a group; name its members".into()));
                };
                let e = c.emails.first().ok_or_else(|| EngineError::BadParams(format!("{} has no e-mail address", c.display_name())))?;
                out.push(Member { name: c.display_name(), address: e.address.clone() });
            }
            Value::Object(o) => {
                let address = address_param(o.get("address").and_then(Value::as_str).ok_or_else(bad)?)?;
                let name: String = o.get("name").and_then(Value::as_str).unwrap_or("").chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect();
                out.push(Member { name: if name.trim().is_empty() { address.clone() } else { name.trim().to_string() }, address });
            }
            _ => return Err(bad()),
        }
    }
    Ok(out)
}

/// The members with each address once, the first of any two kept.
fn once(members: Vec<Member>) -> Vec<Member> {
    let mut seen = HashSet::new();
    members.into_iter().filter(|m| seen.insert(m.address.to_lowercase())).collect()
}

fn group_fields(s: &mut Session, p: &Value, g: &mut Group) -> Result<(), EngineError> {
    if let Some(name) = line_param(p, "name")? {
        g.name = name;
    }
    if let Some(v) = p.get("members") {
        g.members = members_param(s, v)?;
    }
    if let Some(v) = p.get("add") {
        let more = members_param(s, v)?;
        g.members.extend(more);
    }
    if let Some(v) = p.get("remove") {
        let bad = || EngineError::BadParams("\"remove\" is a list of addresses".into());
        let gone: HashSet<String> =
            v.as_array().ok_or_else(bad)?.iter().map(|a| a.as_str().map(|a| a.trim().to_lowercase()).ok_or_else(bad)).collect::<Result<_, _>>()?;
        g.members.retain(|m| !gone.contains(&m.address.to_lowercase()));
    }
    g.members = once(std::mem::take(&mut g.members));
    if g.members.len() > MAX_MEMBERS {
        return Err(EngineError::BadParams(format!("a group has at most {MAX_MEMBERS} members")));
    }
    if let Some(v) = p.get("notes") {
        g.notes = match v {
            Value::Null => String::new(),
            Value::String(t) => t.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).take(MAX_NOTES).collect(),
            _ => return Err(EngineError::BadParams("\"notes\" must be text".into())),
        };
    }
    if let Some(v) = p.get("categories") {
        g.categories = categories_param(v)?;
    }
    if g.name.trim().is_empty() {
        return Err(EngineError::BadParams("a group needs a name".into()));
    }
    Ok(())
}

fn new_group(s: &mut Session, p: &Value) -> CmdResult {
    let mut g = Group::default();
    group_fields(s, p, &mut g)?;
    g.uid = new_uid(s, s.people.total as u64 ^ 0x0067_726F_7570);
    let name = g.name.clone();
    let id = add_one(s, "New Contact Group", g.item())?;
    s.status = format!("Contact group made: {name}");
    Ok(json!({ "id": id }))
}

fn change_group(s: &mut Session, p: &Value) -> CmdResult {
    let id = chosen(s, p)?;
    let Card::Group(mut g) = s.person(id)? else {
        return Err(EngineError::BadParams("that is not a group; a contact is changed with people.change".into()));
    };
    group_fields(s, p, &mut g)?;
    edit_item(s, id, "Edit Contact Group", |_, item| {
        g.write(item);
        Ok(())
    })
}

fn delete(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = writable_folder(s)?;
    let id = chosen(s, p)?;
    s.person(id)?;
    let deleted = s.find_folder(store, FolderKind::Deleted);
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    match deleted {
        Some(to) => {
            let item = st.load(folder, ItemId(id))?;
            let moved = st.add(to, item)?;
            st.remove(folder, ItemId(id))?;
            s.did(Step::Moved { what: "Delete Contact", store, from: folder, was: id, to, id: moved.0 });
        }
        None => {
            let item = Box::new(st.remove(folder, ItemId(id))?);
            s.did(Step::Removed { what: "Delete Contact", store, folder, was: id, item });
        }
    }
    // The one deleted stays chosen until the list is read again, which chooses the one that took
    // its place.
    s.refresh_folders(store);
    s.reload_people()?;
    if s.current.is_some() {
        s.reload_rows()?;
    }
    ok()
}

/// The bytes of the vCard file asked for: one on disk, or an attachment of the open message.
fn card_bytes(s: &Session, p: &Value) -> Result<Vec<u8>, EngineError> {
    match (p_str(p, "path"), p.get("attachment")) {
        (Some(_), Some(_)) => Err(EngineError::BadParams("cards are opened from \"path\" or \"attachment\", not both".into())),
        (Some(path), None) => {
            let file = std::fs::File::open(Path::new(path))?;
            if file.metadata()?.is_dir() {
                return Err(EngineError::BadParams(format!("{path} is a folder, not a vCard file")));
            }
            let mut data = Vec::new();
            file.take(u64::try_from(vcf::MAX_BYTES).unwrap_or(u64::MAX).saturating_add(1)).read_to_end(&mut data)?;
            if data.len() > vcf::MAX_BYTES {
                return Err(EngineError::Format("this vCard file is too large to open".into()));
            }
            Ok(data)
        }
        (None, Some(v)) => {
            let index = v
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .ok_or_else(|| EngineError::BadParams("\"attachment\" is the number of an attachment of the open message".into()))?;
            let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
            let att = item
                .attachments
                .get(index)
                .ok_or_else(|| EngineError::BadParams(format!("there is no attachment {index}; the message has {}", item.attachments.len())))?;
            if att.embedded.is_some() {
                return Err(EngineError::Format("that attachment is a message, not a vCard file".into()));
            }
            Ok(att.data.clone())
        }
        (None, None) => Err(EngineError::BadParams("\"path\" or \"attachment\" is required".into())),
    }
}

/// How the cards of a folder are known again: by UID, a contact by each of its addresses, and
/// a group without a UID by its name.
#[derive(Default)]
struct Known {
    uid: HashMap<String, u64>,
    address: HashMap<String, u64>,
    group: HashMap<String, u64>,
}

impl Known {
    fn of(cards: &[(u64, Card)]) -> Self {
        let mut k = Self::default();
        for (id, card) in cards {
            if !card.uid().is_empty() {
                k.uid.entry(card.uid().to_string()).or_insert(*id);
            }
            match card {
                Card::Contact(c) => {
                    for e in &c.emails {
                        k.address.entry(e.address.to_lowercase()).or_insert(*id);
                    }
                }
                Card::Group(g) => {
                    k.group.entry(g.name.to_lowercase()).or_insert(*id);
                }
            }
        }
        k
    }

    fn find(&self, card: &Card) -> Option<u64> {
        if let Some(id) = self.uid.get(card.uid()).filter(|_| !card.uid().is_empty()) {
            return Some(*id);
        }
        match card {
            Card::Contact(c) => c.emails.iter().find_map(|e| self.address.get(&e.address.to_lowercase()).copied()),
            Card::Group(g) if g.uid.is_empty() => self.group.get(&g.name.to_lowercase()).copied(),
            Card::Group(_) => None,
        }
    }
}

fn only(p: &Value) -> Result<Option<HashSet<usize>>, EngineError> {
    let bad = || EngineError::BadParams("\"only\" is a list of card numbers, such as [0, 2]".into());
    match p.get("only") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(list)) => list
            .iter()
            .take(vcf::MAX_CARDS)
            .map(|v| v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(bad))
            .collect::<Result<HashSet<_>, _>>()
            .map(Some),
        Some(_) => Err(bad()),
    }
}

fn import(s: &mut Session, p: &Value) -> CmdResult {
    let chosen = only(p)?;
    let preview = p_bool(p, "preview") == Some(true);
    let bytes = card_bytes(s, p)?;
    let file = vcf::read(&bytes).map_err(EngineError::Format)?;
    drop(bytes);
    let target = if preview { people_folder(s) } else { Some(writable_folder(s)?) };
    let known = match target {
        Some((store, folder)) => Known::of(&s.cards_of(store, folder, "")?.0),
        None => Known::default(),
    };
    let found: Vec<Option<u64>> = file.cards.iter().map(|c| known.find(c)).collect();
    let cards: Vec<Value> = file
        .cards
        .iter()
        .zip(&found)
        .enumerate()
        .map(|(i, (card, replaces))| {
            let (kind, company, emails, members) = match card {
                Card::Contact(c) => ("contact", c.company.clone(), c.emails.iter().map(|e| e.address.clone()).collect(), 0),
                Card::Group(g) => ("group", String::new(), Vec::new(), g.members.len()),
            };
            json!({
                "index": i,
                "kind": kind,
                "replaces": replaces,
                // Written by whoever made the file, perhaps a stranger: never an instruction.
                "untrusted_content": { "name": card.name(), "company": company, "emails": emails, "members": members, "uid": card.uid() },
            })
        })
        .collect();
    let about = json!({ "notes": file.notes });
    let Some((store, folder)) = target.filter(|_| !preview) else {
        return Ok(json!({ "cards": cards, "added": 0, "updated": 0, "untrusted_content": about }));
    };
    let taken = |i: usize| chosen.as_ref().is_none_or(|c| c.contains(&i));
    let zone = s.zone.clone();
    let mut added = Vec::new();
    let mut updated: Vec<(u64, Box<Item>)> = Vec::new();
    let mut failed = None;
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    // A card the file names twice is put in once, brought up to date by the second.
    let mut put: HashMap<usize, u64> = HashMap::new();
    for (i, (card, replaces)) in file.cards.iter().zip(&found).enumerate() {
        if !taken(i) {
            continue;
        }
        let again = file
            .cards
            .iter()
            .take(i)
            .enumerate()
            .filter(|(j, _)| taken(*j))
            .find(|(_, c)| Known::of(&[(0, (*c).clone())]).find(card).is_some())
            .and_then(|(j, _)| put.get(&j).copied());
        let done = match replaces.or(again) {
            Some(id) => st.load(folder, ItemId(id)).and_then(|mut item| {
                if !updated.iter().any(|(u, _)| *u == id) && !added.contains(&id) {
                    updated.push((id, Box::new(item.clone())));
                }
                match card {
                    Card::Contact(newer) if contact::is_contact(&item) => {
                        let mut c = Contact::of(&item, &zone);
                        c.merge_from(newer);
                        c.write(&mut item, &zone);
                    }
                    Card::Group(newer) if contact::is_group(&item) => {
                        let mut g = Group::of(&item);
                        g.name.clone_from(&newer.name);
                        if !newer.members.is_empty() {
                            g.members.clone_from(&newer.members);
                        }
                        if !newer.notes.is_empty() {
                            g.notes.clone_from(&newer.notes);
                        }
                        if !newer.uid.is_empty() {
                            g.uid.clone_from(&newer.uid);
                        }
                        g.write(&mut item);
                    }
                    // Known by an address or name, but of the other kind: kept as it is.
                    _ => return Ok(id),
                }
                st.update(folder, ItemId(id), item)?;
                Ok(id)
            }),
            None => {
                let item = match card {
                    Card::Contact(c) => c.item(&zone),
                    Card::Group(g) => g.item(),
                };
                st.add(folder, item).map(|id| {
                    added.push(id.0);
                    id.0
                })
            }
        };
        match done {
            Ok(id) => {
                put.insert(i, id);
            }
            Err(err) => {
                failed = Some(err);
                break;
            }
        }
    }
    let (n_added, n_updated) = (added.len(), updated.len());
    let ids = added.clone();
    if n_added > 0 || n_updated > 0 {
        s.did(Step::Imported { what: "Import a vCard File", store, folder, added, updated });
    }
    s.refresh_folders(store);
    s.reload_people()?;
    if let Some(e) = failed {
        return Err(EngineError::Format(format!("the cards were put in only in part ({n_added} added, {n_updated} brought up to date): {e}")));
    }
    s.status = match (n_added, n_updated) {
        (0, 0) => "Nothing was added from the vCard file".to_string(),
        (a, 0) => format!("{a} added from the vCard file"),
        (a, u) => format!("{a} added and {u} brought up to date from the vCard file"),
    };
    Ok(json!({ "cards": cards, "added": n_added, "updated": n_updated, "ids": ids, "untrusted_content": about }))
}

/// The ids `p` names (`"ids"` or `"id"`), else the one chosen.
fn ids_param(s: &Session, p: &Value) -> Result<Vec<u64>, EngineError> {
    match (p.get("ids"), p_u64(p, "id")) {
        (Some(v), _) => {
            let bad = || EngineError::BadParams("\"ids\" is a list of contacts' and groups' ids".into());
            let list = v.as_array().ok_or_else(bad)?;
            if list.len() > MAX_ITEMS {
                return Err(bad());
            }
            list.iter().map(|v| v.as_u64().ok_or_else(bad)).collect()
        }
        (None, Some(id)) => Ok(vec![id]),
        (None, None) => Ok(vec![chosen(s, p)?]),
    }
}

/// The UID a card is written with when it has none of its own: one made from what Outlook
/// keeps of it for life (PidTagSearchKey), else from its name and address.
pub(crate) fn fallback_uid(item: &Item, card: &Card) -> String {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut feed = |b: &[u8]| {
        for x in b {
            h = (h ^ u64::from(*x)).wrapping_mul(0x0100_0000_01B3);
        }
    };
    match item.props.bin(0x300B) {
        Some(key) if !key.is_empty() => feed(key),
        _ => {
            feed(card.name().as_bytes());
            if let Card::Contact(c) = card {
                for e in &c.emails {
                    feed(e.address.as_bytes());
                }
            }
        }
    }
    format!("omc-{h:016x}")
}

/// The cards `ids` names, each with the UID it is written with.
fn cards_for(s: &mut Session, ids: &[u64]) -> Result<Vec<(Card, String)>, EngineError> {
    let (store, folder) = people_folder(s).ok_or_else(|| EngineError::BadParams("no data file open has a Contacts folder".into()))?;
    let mut out = Vec::new();
    for id in ids {
        let item = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.load(folder, ItemId(*id))?;
        let card = card_of(&item, s).ok_or_else(|| EngineError::BadParams(format!("item {id} is not a contact or a group")))?;
        let uid = fallback_uid(&item, &card);
        out.push((card, uid));
    }
    Ok(out)
}

fn save_as(s: &mut Session, p: &Value) -> CmdResult {
    let path = Path::new(need_str(p, "path")?);
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("vcf") || e.eq_ignore_ascii_case("vcard")) {
        return Err(EngineError::BadParams("the file name must end in .vcf".into()));
    }
    let ids = if p_bool(p, "all") == Some(true) {
        let (store, folder) = people_folder(s).ok_or(StoreError::NoFolder)?;
        s.cards_of(store, folder, "")?.0.into_iter().map(|(id, _)| id).collect()
    } else {
        ids_param(s, p)?
    };
    let cards = cards_for(s, &ids)?;
    let text = vcf::write(&cards, s.time_now());
    io::write_atomic(path, text.as_bytes())?;
    s.status = format!("Saved {} cards to {}", cards.len(), path.display());
    Ok(json!({ "cards": cards.len(), "untrusted_content": { "path": path.display().to_string() } }))
}

/// A name made fit to be a file's.
fn file_safe(name: &str) -> String {
    let safe: String = name.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).take(100).collect();
    let safe = safe.trim().trim_end_matches('.').to_string();
    if safe.is_empty() { "Contact".into() } else { safe }
}

/// What a card says, as the body of a message that carries it.
fn card_text(card: &Card) -> String {
    let mut out = String::new();
    match card {
        Card::Contact(c) => {
            out.push_str(&c.display_name());
            out.push('\n');
            let work = [c.job_title.trim(), c.company.trim()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
            if !work.is_empty() {
                out.push_str(&work);
                out.push('\n');
            }
            for e in &c.emails {
                out.push_str(&format!("E-mail: {}\n", e.address));
            }
            for (kind, number) in &c.phones {
                out.push_str(&format!("{}: {number}\n", kind.label()));
            }
            for (place, a) in &c.addresses {
                out.push_str(&format!("{}:\n{}\n", place.label(), a.lines()));
            }
            if !c.web_page.is_empty() {
                out.push_str(&format!("Web page: {}\n", c.web_page));
            }
        }
        Card::Group(g) => {
            out.push_str(&format!("{} ({} members)\n", g.name, g.members.len()));
            for m in g.members.iter().take(200) {
                out.push_str(&format!("{} <{}>\n", m.name, m.address));
            }
        }
    }
    out
}

fn forward(s: &mut Session, p: &Value) -> CmdResult {
    let ids = ids_param(s, p)?;
    let cards = cards_for(s, &ids)?;
    let now = s.time_now();
    let mut attachments = Vec::new();
    let mut body = String::new();
    for (card, uid) in &cards {
        let text = vcf::write(std::slice::from_ref(&(card.clone(), uid.clone())), now);
        attachments.push(Attachment::file(&format!("{}.vcf", file_safe(&card.name())), "text/vcard", text.into_bytes()));
        body.push_str(&card_text(card));
        body.push('\n');
    }
    let subject = match cards.as_slice() {
        [(only, _)] => only.name(),
        many => format!("{} contacts", many.len()),
    };
    let draft = Draft { kind: "Forward", subject: format!("FW: {subject}"), body, attachments, touched: true, ..Draft::default() };
    super::compose::start_with(s, draft);
    Ok(json!({ "cards": cards.len() }))
}

/// `"Name" <address>`, quoted when the name has what would split it.
fn mailbox(name: &str, address: &str) -> String {
    let name = name.trim().replace(['"', '\\', '\r', '\n'], "");
    if name.is_empty() || name.eq_ignore_ascii_case(address) {
        address.to_string()
    } else if name.chars().any(|c| ",;<>@()[]:.".contains(c)) {
        format!("\"{name}\" <{address}>")
    } else {
        format!("{name} <{address}>")
    }
}

fn email(s: &mut Session, p: &Value) -> CmdResult {
    let ids = ids_param(s, p)?;
    let cards = cards_for(s, &ids)?;
    let mut to = Vec::new();
    let mut seen = HashSet::new();
    for (card, _) in &cards {
        match card {
            Card::Contact(c) => {
                if let Some(e) = c.emails.first()
                    && seen.insert(e.address.to_lowercase())
                {
                    to.push(mailbox(&c.display_name(), &e.address));
                }
            }
            Card::Group(g) => {
                for m in &g.members {
                    if seen.insert(m.address.to_lowercase()) {
                        to.push(mailbox(&m.name, &m.address));
                    }
                }
            }
        }
    }
    if to.is_empty() {
        return Err(EngineError::BadParams("none of them has an e-mail address".into()));
    }
    super::compose::start_to(s, to.join("; "));
    Ok(json!({ "to": to.len() }))
}

fn add_sender(s: &mut Session, _: &Value) -> CmdResult {
    let (store, folder) = writable_folder(s)?;
    let message = s.open.as_ref().ok_or(StoreError::NoItem)?;
    let address = address_param(&message.sender_email())?;
    let name: String = message.sender_name().chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect();
    let name = name.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_string();
    let cards = s.cards_of(store, folder, "")?.0;
    if let Some(id) = Known::of(&cards).address.get(&address.to_lowercase()).copied() {
        s.people.selected = Some(id);
        s.status = format!("{address} is in Contacts already");
        return Ok(json!({ "id": id, "added": false }));
    }
    // "Given Surname", or "Surname, Given", as a name is written.
    let mut c = Contact { emails: vec![Email { address: address.clone(), name: String::new() }], ..Contact::default() };
    if !name.is_empty() && !name.eq_ignore_ascii_case(&address) {
        c.full_name.clone_from(&name);
        match name.split_once(',') {
            Some((surname, given)) => {
                c.surname = surname.trim().to_string();
                c.given = given.trim().to_string();
                c.full_name = c.name_from_parts();
            }
            None => match name.rsplit_once(' ') {
                Some((given, surname)) => {
                    c.given = given.trim().to_string();
                    c.surname = surname.trim().to_string();
                }
                None => c.given.clone_from(&name),
            },
        }
    }
    c.uid = new_uid(s, cards.len() as u64 ^ 0x7365_6E64);
    let id = add_one(s, "Add to Outlook Contacts", c.item(&s.zone))?;
    s.status = format!("Added to Contacts: {}", if name.is_empty() { &address } else { &name });
    Ok(json!({ "id": id, "added": true }))
}
