//! Folder commands: selecting, creating, renaming, moving, deleting, emptying, marking all read.

use super::{CommandSpec, has_folder, has_folder_writable, need_str, ok, p_bool, p_str, p_u64};
use crate::{CmdResult, EngineError, Session};
use openmailclient_model::{FolderId, FolderKind, ItemId, StoreError};
use serde_json::{Value, json};
use std::collections::HashSet;

const MAX_NAME: usize = 255;
/// FILETIME ticks in a day.
const DAY: i64 = 864_000_000_000;
/// Folders deep that a path is told.
const MAX_DEPTH: usize = 64;

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "folder.select",
            label: "Go to Folder",
            tab: "",
            group: "",
            shortcut: "Ctrl+Y",
            params: r#"{"folder": "Inbox/Projects"} or {"folderId": id}; optional "store": index"#,
            enabled: has_store,
            run: select,
        },
        CommandSpec {
            id: "folder.new",
            label: "New Folder",
            tab: "Folder",
            group: "New",
            shortcut: "Ctrl+Shift+E",
            params: r#"{"name": text}; created inside the current folder"#,
            enabled: has_folder_writable,
            run: new,
        },
        CommandSpec {
            id: "folder.rename",
            label: "Rename Folder",
            tab: "Folder",
            group: "Actions",
            shortcut: "F2",
            params: r#"{"name": text}"#,
            enabled: user_folder,
            run: rename,
        },
        CommandSpec {
            id: "folder.move",
            label: "Move Folder",
            tab: "Folder",
            group: "Actions",
            shortcut: "",
            params: r#"{"to": the path of the folder it goes under, or "toId": its id}"#,
            enabled: user_folder,
            run: move_to,
        },
        CommandSpec {
            id: "folder.copy",
            label: "Copy Folder",
            tab: "Folder",
            group: "Actions",
            shortcut: "",
            params: r#"{"to": the path of the folder the copy goes under, or "toId": its id, "toStore": the data file it goes into (default: this one)}; the folder is copied with the folders inside it and every message, each folder keeping what it holds (mail, appointments, contacts, tasks); a name taken there has a number put after it; returns {"folderId": the copy's id, "name", "folders", "messages", "unreadable": messages that could not be read and were left out}"#,
            enabled: has_folder,
            run: copy,
        },
        CommandSpec {
            id: "folder.delete",
            label: "Delete Folder",
            tab: "Folder",
            group: "Actions",
            shortcut: "",
            params: r#"{"permanently": true to remove it for good; otherwise it goes to Deleted Items, and one already there is removed for good}; returns what went with it: {"permanently", "messages", "unread", "folders"}"#,
            enabled: user_folder,
            run: delete,
        },
        CommandSpec {
            id: "folder.markAllRead",
            label: "Mark All as Read",
            tab: "Folder",
            group: "Clean Up",
            shortcut: "",
            params: r#"{"subfolders": true to mark the folders inside it too}; returns {"marked"}"#,
            enabled: has_folder_writable,
            run: mark_all_read,
        },
        CommandSpec {
            id: "folder.empty",
            label: "Empty Folder",
            tab: "Folder",
            group: "Clean Up",
            shortcut: "",
            params: r#"{"olderThanDays": n to remove only messages older than that and keep the folders inside it}; returns {"removed", "folders", "kept"}"#,
            enabled: can_empty,
            run: empty,
        },
        CommandSpec {
            id: "folder.properties",
            label: "Folder Properties",
            tab: "Folder",
            group: "Properties",
            shortcut: "",
            params: r#"returns the folder's counts and size, alone and with the folders inside it"#,
            enabled: has_folder,
            run: properties,
        },
    ]
}

fn has_store(s: &Session) -> bool {
    !s.stores.is_empty()
}

/// Outlook's own folders (Inbox, Sent Items…) can't be renamed or deleted; folders people made can.
fn user_folder(s: &Session) -> bool {
    has_folder_writable(s) && s.current_folder().is_some_and(|f| matches!(f.kind, FolderKind::Mail | FolderKind::Other))
}

fn can_empty(s: &Session) -> bool {
    has_folder_writable(s) && s.current_folder().is_some_and(|f| matches!(f.kind, FolderKind::Deleted | FolderKind::Junk))
}

fn select(s: &mut Session, p: &Value) -> CmdResult {
    let store = match p_u64(p, "store") {
        Some(i) => usize::try_from(i).unwrap_or(usize::MAX),
        None => s.current.map(|(i, _)| i).unwrap_or(0),
    };
    if store >= s.stores.len() {
        return Err(EngineError::BadParams(format!("there is no data file {store}")));
    }
    let folder = match (p_u64(p, "folderId"), p_str(p, "folder")) {
        (Some(id), _) => FolderId(id),
        (None, Some(path)) => s.folder_by_path(store, path).ok_or_else(|| EngineError::BadParams(format!("there is no folder \"{path}\"")))?,
        _ => return Err(EngineError::BadParams("\"folder\" (path) or \"folderId\" is required".into())),
    };
    s.select_folder(store, folder)?;
    ok()
}

fn valid_name(p: &Value) -> Result<String, EngineError> {
    let name = need_str(p, "name")?.trim().to_string();
    if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(|c| c.is_control() || c == '/' || c == '\\') {
        return Err(EngineError::BadParams("a folder name is 1 to 255 characters without slashes".into()));
    }
    Ok(name)
}

/// `folder` and every folder under it, parents first, each once however the parents point.
pub(super) fn subtree(s: &Session, store: usize, folder: FolderId) -> Vec<FolderId> {
    let list = s.folders.get(store).map(Vec::as_slice).unwrap_or(&[]);
    let mut seen = HashSet::from([folder]);
    let mut found = vec![folder];
    let mut at = 0;
    while let Some(&cur) = found.get(at) {
        found.extend(list.iter().filter(|f| f.parent == Some(cur)).map(|f| f.id).filter(|id| seen.insert(*id)));
        at += 1;
    }
    found
}

/// Whether a folder named `name` stands under `parent` already, other than `except`. Names that
/// differ only in the case of their letters are one name, in any alphabet.
pub(super) fn clash(s: &Session, store: usize, parent: Option<FolderId>, name: &str, except: Option<FolderId>) -> bool {
    let name = name.to_lowercase();
    s.folders.get(store).is_some_and(|l| l.iter().any(|f| f.parent == parent && Some(f.id) != except && f.name.to_lowercase() == name))
}

fn taken(name: &str) -> EngineError {
    EngineError::Format(format!("a folder named \"{name}\" already exists here"))
}

/// What a set of folders holds.
#[derive(Default)]
struct Held {
    messages: u64,
    unread: u64,
    size: u64,
    oldest: Option<i64>,
    newest: Option<i64>,
}

fn held(s: &mut Session, store: usize, folders: &[FolderId]) -> Result<Held, EngineError> {
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    let mut held = Held::default();
    for folder in folders {
        for row in st.list(*folder)? {
            held.messages = held.messages.saturating_add(1);
            held.unread = held.unread.saturating_add(u64::from(!row.read));
            held.size = held.size.saturating_add(u64::from(row.size));
            if row.time > 0 {
                held.oldest = Some(held.oldest.map_or(row.time, |t| t.min(row.time)));
                held.newest = Some(held.newest.map_or(row.time, |t| t.max(row.time)));
            }
        }
    }
    Ok(held)
}

fn new(s: &mut Session, p: &Value) -> CmdResult {
    let name = valid_name(p)?;
    let (store, parent) = s.current.ok_or(StoreError::NoFolder)?;
    if clash(s, store, Some(parent), &name, None) {
        return Err(taken(&name));
    }
    let id = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.create_folder(parent, &name)?;
    s.refresh_folders(store);
    Ok(json!({ "folderId": id.0 }))
}

fn rename(s: &mut Session, p: &Value) -> CmdResult {
    let name = valid_name(p)?;
    let (store, folder) = s.current.ok_or(StoreError::NoFolder)?;
    let parent = s.folder_info(store, folder).and_then(|f| f.parent);
    if clash(s, store, parent, &name, Some(folder)) {
        return Err(taken(&name));
    }
    s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.rename_folder(folder, &name)?;
    s.refresh_folders(store);
    ok()
}

fn move_to(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = s.current.ok_or(StoreError::NoFolder)?;
    let to = match (p_u64(p, "toId"), p_str(p, "to")) {
        (Some(id), _) => FolderId(id),
        (None, Some(path)) => s.folder_by_path(store, path).ok_or_else(|| EngineError::BadParams(format!("there is no folder \"{path}\"")))?,
        _ => return Err(EngineError::BadParams("\"to\" (path) or \"toId\" is required".into())),
    };
    let info = s.folder_info(store, folder).cloned().ok_or(StoreError::NoFolder)?;
    if s.folder_info(store, to).is_none() {
        return Err(EngineError::BadParams("there is no such folder to move it to".into()));
    }
    if info.parent == Some(to) {
        return ok();
    }
    if subtree(s, store, folder).contains(&to) {
        return Err(EngineError::BadParams("a folder cannot be moved into itself or a folder inside it".into()));
    }
    if clash(s, store, Some(to), &info.name, None) {
        return Err(taken(&info.name));
    }
    s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.move_folder(folder, to)?;
    s.refresh_folders(store);
    ok()
}

/// Copy Folder: the folder and those inside it made again under `to`, in this data file or
/// another, and each message put into its copy. The original is not touched.
fn copy(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = s.current.ok_or(StoreError::NoFolder)?;
    let into = match p_u64(p, "toStore") {
        Some(n) => {
            usize::try_from(n).ok().filter(|n| *n < s.stores.len()).ok_or_else(|| EngineError::BadParams("there is no such data file".into()))?
        }
        None => store,
    };
    let to = match (p_u64(p, "toId"), p_str(p, "to")) {
        (Some(id), _) => FolderId(id),
        (None, Some(path)) => s
            .folder_by_path(into, path)
            .ok_or_else(|| EngineError::BadParams(format!("there is no folder \"{}\"", path.chars().take(MAX_NAME).collect::<String>())))?,
        _ => return Err(EngineError::BadParams("\"to\" (path) or \"toId\" is required".into())),
    };
    if s.folder_info(into, to).is_none() {
        return Err(EngineError::BadParams("there is no such folder to copy it into".into()));
    }
    if !s.stores.get(into).is_some_and(|st| st.writable()) {
        return Err(EngineError::Format("that data file cannot be written".into()));
    }
    let tree = subtree(s, store, folder);
    if into == store && tree.contains(&to) {
        return Err(EngineError::BadParams("a folder cannot be copied into itself or a folder inside it".into()));
    }
    let info = s.folder_info(store, folder).cloned().ok_or(StoreError::NoFolder)?;
    // Each folder of the tree is made under the copy of its parent; the tree lists a parent
    // before those inside it.
    let mut made: std::collections::HashMap<FolderId, FolderId> = std::collections::HashMap::new();
    let (mut messages, mut unreadable) = (0u64, 0u64);
    let mut name_of_top = String::new();
    for f in &tree {
        let Some(fi) = s.folder_info(store, *f).cloned() else { continue };
        let parent = if *f == folder { to } else { fi.parent.and_then(|p| made.get(&p).copied()).ok_or(StoreError::NoFolder)? };
        let name = if *f == folder { free_name(s, into, parent, &fi.name) } else { fi.name.clone() };
        let dst = s.stores.get_mut(into).ok_or(StoreError::NoFolder)?;
        let copy = if fi.container_class.is_empty() || fi.container_class == "IPF.Note" {
            dst.create_folder(parent, &name)?
        } else {
            dst.create_folder_of(Some(parent), &name, &fi.container_class)?
        };
        if *f == folder {
            name_of_top = name;
        }
        made.insert(*f, copy);
        let rows = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.list(*f)?;
        for row in rows {
            let item = match s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.load(*f, ItemId(row.id)) {
                Ok(item) => item,
                Err(_) => {
                    unreadable = unreadable.saturating_add(1);
                    continue;
                }
            };
            match s.stores.get_mut(into).ok_or(StoreError::NoFolder)?.add(copy, item) {
                Ok(_) => messages = messages.saturating_add(1),
                Err(_) => unreadable = unreadable.saturating_add(1),
            }
        }
    }
    s.refresh_folders(into);
    let top = made.get(&folder).copied().ok_or(StoreError::NoFolder)?;
    s.status = format!(
        "\"{}\" copied, {} folder{} and {messages} message{}",
        info.name,
        made.len(),
        if made.len() == 1 { "" } else { "s" },
        if messages == 1 { "" } else { "s" }
    );
    Ok(json!({ "folderId": top.0, "name": name_of_top, "folders": made.len(), "messages": messages, "unreadable": unreadable }))
}

/// A name for `name` that no folder under `parent` has: itself, or itself with a number.
pub(super) fn free_name(s: &Session, store: usize, parent: FolderId, name: &str) -> String {
    if !clash(s, store, Some(parent), name, None) {
        return name.to_string();
    }
    let base: String = name.chars().take(MAX_NAME.saturating_sub(12)).collect();
    (2..10_000u32).map(|n| format!("{base} ({n})")).find(|c| !clash(s, store, Some(parent), c, None)).unwrap_or(base)
}

fn delete(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = s.current.ok_or(StoreError::NoFolder)?;
    let info = s.folder_info(store, folder).cloned().ok_or(StoreError::NoFolder)?;
    let parent = info.parent;
    let tree = subtree(s, store, folder);
    let went = held(s, store, &tree)?;
    // A folder is put with the deleted mail, where it can be taken back from. One that is there
    // already, or that is asked to go for good, is removed.
    let bin = s.folders.get(store).and_then(|l| l.iter().find(|f| f.kind == FolderKind::Deleted)).map(|f| f.id);
    let in_bin = bin.is_some_and(|bin| subtree(s, store, bin).contains(&folder));
    let permanently = p_bool(p, "permanently").unwrap_or(false) || in_bin;
    match bin {
        Some(bin) if !permanently => {
            let name = free_name(s, store, bin, &info.name);
            let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
            st.move_folder(folder, bin)?;
            if name != info.name {
                st.rename_folder(folder, &name)?;
            }
        }
        None if !permanently => {
            return Err(EngineError::Format(
                "this data file has no Deleted Items folder to put it in; \"permanently\": true removes the folder and what it holds for good".into(),
            ));
        }
        _ => {
            s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.delete_folder(folder)?;
            // Deleted for good here: the IMAP server's folders go too, at the next Send/Receive.
            super::imap_sync::forget_folders(s, store, &tree);
        }
    }
    s.refresh_folders(store);
    s.current = None;
    s.selected = None;
    s.open = None;
    s.rows.clear();
    match parent {
        Some(p) => s.select_folder(store, p)?,
        None => s.select_default_folder(store),
    }
    Ok(json!({ "permanently": permanently, "messages": went.messages, "unread": went.unread, "folders": tree.len() }))
}

fn mark_all_read(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = s.current.ok_or(StoreError::NoFolder)?;
    let folders = if p_bool(p, "subfolders").unwrap_or(false) { subtree(s, store, folder) } else { vec![folder] };
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    let mut marked = 0u64;
    for folder in folders {
        let unread: Vec<u64> = st.list(folder)?.into_iter().filter(|r| !r.read).map(|r| r.id).collect();
        for id in unread {
            let mut item = st.load(folder, ItemId(id))?;
            item.set_read(true);
            st.update(folder, ItemId(id), item)?;
            marked = marked.saturating_add(1);
        }
    }
    if let Some(open) = s.open.as_mut() {
        open.set_read(true);
    }
    s.refresh_folders(store);
    s.reload_rows()?;
    Ok(json!({ "marked": marked }))
}

fn empty(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = s.current.ok_or(StoreError::NoFolder)?;
    // With an age, only what is older goes: undated messages and the folders inside stay.
    let cutoff = match p.get("olderThanDays") {
        None | Some(Value::Null) => None,
        Some(days) => {
            let days = days.as_u64().ok_or_else(|| EngineError::BadParams("\"olderThanDays\" is a whole number of days".into()))?;
            Some(s.now.saturating_sub(i64::try_from(days).unwrap_or(i64::MAX).saturating_mul(DAY)))
        }
    };
    let inside: Vec<FolderId> = if cutoff.is_none() { subtree(s, store, folder).into_iter().skip(1).collect() } else { Vec::new() };
    let in_folders = held(s, store, &inside)?.messages;
    let children: Vec<FolderId> =
        s.folders.get(store).map(|l| l.iter().filter(|f| f.parent == Some(folder)).map(|f| f.id).collect()).unwrap_or_default();
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    let rows = st.list(folder)?;
    let ids: Vec<u64> = rows.iter().filter(|r| cutoff.is_none_or(|cutoff| r.time > 0 && r.time < cutoff)).map(|r| r.id).collect();
    let kept = rows.len().saturating_sub(ids.len());
    let mut removed = in_folders;
    let mut gone = Vec::new();
    for id in ids {
        gone.push(st.remove(folder, ItemId(id))?);
        removed = removed.saturating_add(1);
    }
    if cutoff.is_none() {
        for child in children {
            st.delete_folder(child)?;
        }
    }
    super::imap_sync::forget_folders(s, store, &inside);
    for item in &gone {
        super::imap_sync::forget(s, store, item);
    }
    s.selected = None;
    s.open = None;
    s.refresh_folders(store);
    s.reload_rows()?;
    Ok(json!({ "removed": removed, "folders": inside.len(), "kept": kept }))
}

fn properties(s: &mut Session, _: &Value) -> CmdResult {
    let (store, folder) = s.current.ok_or(StoreError::NoFolder)?;
    let f = s.current_folder().cloned().ok_or(StoreError::NoFolder)?;
    let mut path = vec![f.name.clone()];
    let mut up = f.parent;
    while let Some(info) = up.and_then(|id| s.folder_info(store, id))
        && path.len() < MAX_DEPTH
    {
        path.push(info.name.clone());
        up = info.parent;
    }
    path.reverse();
    let tree = subtree(s, store, folder);
    let own = held(s, store, &[folder])?;
    let all = held(s, store, &tree)?;
    Ok(json!({
        "store": store, "id": f.id.0, "name": f.name, "kind": format!("{:?}", f.kind), "class": f.container_class,
        "total": f.total, "unread": f.unread, "size": own.size, "oldest": own.oldest, "newest": own.newest,
        "subfolders": tree.len().saturating_sub(1),
        "withSubfolders": { "total": all.messages, "unread": all.unread, "size": all.size },
        "untrusted_content": { "path": path.join("/") },
    }))
}
