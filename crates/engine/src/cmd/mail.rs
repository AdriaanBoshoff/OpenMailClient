//! Mail commands: selecting, read state, flags, delete, move, categories.

use super::{CommandSpec, always, clock, has_folder, has_selection, has_selection_writable, need_str, ok, p_str, p_u64};
use crate::undo::Step;
use crate::{CmdResult, EngineError, Session};
use openmailclient_model::task::{self, FollowUp};
use openmailclient_model::{Flag, FolderKind, StoreError};
use serde_json::{Value, json};

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "mail.select",
            label: "Select Message",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"index": row number} or {"id": item id}; optional "folder": path to go to first (alone, it only goes there)"#,
            enabled: has_folder,
            run: select,
        },
        CommandSpec {
            id: "edit.undo",
            label: "Undo",
            tab: "",
            group: "",
            shortcut: "Ctrl+Z",
            params: r#"takes back the last change, move or deletion of a message; returns {"undone": the command's label, "left": steps that remain}"#,
            enabled: can_undo,
            run: undo,
        },
        CommandSpec { id: "mail.next", label: "Next Item", tab: "", group: "", shortcut: "Down", params: "", enabled: has_folder, run: next },
        CommandSpec {
            id: "mail.previous",
            label: "Previous Item",
            tab: "",
            group: "",
            shortcut: "Up",
            params: "",
            enabled: has_folder,
            run: previous,
        },
        CommandSpec {
            id: "mail.markRead",
            label: "Mark as Read",
            tab: "Home",
            group: "Tags",
            shortcut: "Ctrl+Q",
            params: "",
            enabled: has_selection_writable,
            run: mark_read,
        },
        CommandSpec {
            id: "mail.markUnread",
            label: "Mark as Unread",
            tab: "Home",
            group: "Tags",
            shortcut: "Ctrl+U",
            params: "",
            enabled: has_selection_writable,
            run: mark_unread,
        },
        CommandSpec {
            id: "mail.toggleRead",
            label: "Unread/ Read",
            tab: "Home",
            group: "Tags",
            shortcut: "",
            params: "",
            enabled: has_selection_writable,
            run: toggle_read,
        },
        CommandSpec {
            id: "mail.flag",
            label: "Follow Up",
            tab: "Home",
            group: "Tags",
            shortcut: "Insert",
            params: "",
            enabled: has_selection_writable,
            run: toggle_flag,
        },
        CommandSpec {
            id: "mail.flagComplete",
            label: "Mark Complete",
            tab: "Home",
            group: "Tags",
            shortcut: "",
            params: "",
            enabled: has_selection_writable,
            run: flag_complete,
        },
        CommandSpec {
            id: "mail.clearFlag",
            label: "Clear Flag",
            tab: "Home",
            group: "Tags",
            shortcut: "",
            params: "",
            enabled: has_selection_writable,
            run: clear_flag,
        },
        CommandSpec {
            id: "mail.categorize",
            label: "Categorize",
            tab: "Home",
            group: "Tags",
            shortcut: "",
            params: r#"{"category": name}; toggles it. {"clear": true} removes all"#,
            enabled: has_selection_writable,
            run: categorize,
        },
        CommandSpec {
            id: "mail.importance",
            label: "Importance",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"level": "low" | "normal" | "high"}"#,
            enabled: has_selection_writable,
            run: importance,
        },
        CommandSpec {
            id: "mail.delete",
            label: "Delete",
            tab: "Home",
            group: "Delete",
            shortcut: "Ctrl+D",
            params: "",
            enabled: has_selection_writable,
            run: delete,
        },
        CommandSpec {
            id: "mail.archive",
            label: "Archive",
            tab: "Home",
            group: "Delete",
            shortcut: "Backspace",
            params: "",
            enabled: has_selection_writable,
            run: archive,
        },
        CommandSpec {
            id: "mail.junk",
            label: "Junk",
            tab: "Home",
            group: "Delete",
            shortcut: "",
            params: "",
            enabled: has_selection_writable,
            run: junk,
        },
        CommandSpec {
            id: "mail.move",
            label: "Move",
            tab: "Home",
            group: "Move",
            shortcut: "Ctrl+Shift+V",
            params: r#"{"folder": "Inbox/Projects"} or {"folderId": id}"#,
            enabled: has_selection_writable,
            run: move_to,
        },
        CommandSpec { id: "session.inspect", label: "Inspect Session", tab: "", group: "", shortcut: "", params: "", enabled: always, run: inspect },
        CommandSpec {
            id: "mail.properties",
            label: "Properties",
            tab: "",
            group: "",
            shortcut: "Alt+Enter",
            params: "",
            enabled: has_selection,
            run: properties,
        },
    ]
}

fn select(s: &mut Session, p: &Value) -> CmdResult {
    let went = p_str(p, "folder");
    if let Some(path) = went {
        let store = s.current.map(|(i, _)| i).unwrap_or(0);
        let folder = s.folder_by_path(store, path).ok_or_else(|| EngineError::BadParams(format!("there is no folder \"{path}\"")))?;
        s.select_folder(store, folder)?;
    }
    let id = if let Some(id) = p_u64(p, "id") {
        id
    } else if let Some(i) = p_u64(p, "index") {
        let i = usize::try_from(i).map_err(|_| EngineError::BadParams("index is too large".into()))?;
        s.rows.get(i).map(|r| r.id).ok_or_else(|| EngineError::BadParams(format!("there is no row {i}; the list has {}", s.rows.len())))?
    } else if went.is_some() {
        return ok();
    } else {
        return Err(EngineError::BadParams("\"index\" or \"id\" is required".into()));
    };
    if !s.rows.iter().any(|r| r.id == id) {
        return Err(StoreError::NoItem.into());
    }
    s.select_item(id)?;
    ok()
}

fn step(s: &mut Session, forward: bool) -> CmdResult {
    let target = match s.selected_index() {
        None => s.rows.first(),
        Some(i) if forward => s.rows.get(i.saturating_add(1)),
        Some(i) => i.checked_sub(1).and_then(|p| s.rows.get(p)),
    }
    .map(|r| r.id);
    if let Some(id) = target {
        s.select_item(id)?;
    }
    ok()
}

fn next(s: &mut Session, _: &Value) -> CmdResult {
    step(s, true)
}

fn previous(s: &mut Session, _: &Value) -> CmdResult {
    step(s, false)
}

fn mark_read(s: &mut Session, _: &Value) -> CmdResult {
    s.edit_selected("Mark as Read", |it| it.set_read(true))?;
    ok()
}

fn mark_unread(s: &mut Session, _: &Value) -> CmdResult {
    s.edit_selected("Mark as Unread", |it| it.set_read(false))?;
    ok()
}

fn toggle_read(s: &mut Session, _: &Value) -> CmdResult {
    s.edit_selected("Unread/ Read", |it| it.set_read(!it.is_read()))?;
    ok()
}

/// Insert flags a message for today, as a quick click on the flag does, and takes a flag
/// that is there away; the flag carries its days and goes on the to-do list
/// ([MS-OXOFLAG] 3.1.4.1.1).
fn toggle_flag(s: &mut Session, _: &Value) -> CmdResult {
    let (today, now, zone) = (s.today(), clock(s), s.zone.clone());
    let (start, due) = FollowUp::Today.days(today);
    s.edit_selected("Follow Up", |it| {
        if it.flag() == Flag::Flagged {
            task::clear_flag(it);
        } else {
            task::follow_up(it, start, due, now, &zone);
        }
    })?;
    ok()
}

fn flag_complete(s: &mut Session, _: &Value) -> CmdResult {
    let (today, now) = (s.today(), clock(s));
    s.edit_selected("Mark Complete", |it| task::mark_complete(it, today, now))?;
    ok()
}

fn clear_flag(s: &mut Session, _: &Value) -> CmdResult {
    s.edit_selected("Clear Flag", task::clear_flag)?;
    ok()
}

fn categorize(s: &mut Session, p: &Value) -> CmdResult {
    if p.get("clear").and_then(Value::as_bool) == Some(true) {
        s.edit_selected("Categorize", |it| it.categories.clear())?;
        return ok();
    }
    let cat = need_str(p, "category")?.trim().to_string();
    if cat.is_empty() || cat.chars().count() > 255 {
        return Err(EngineError::BadParams("a category name is 1 to 255 characters".into()));
    }
    s.edit_selected("Categorize", |it| match it.categories.iter().position(|c| c.eq_ignore_ascii_case(&cat)) {
        Some(i) => {
            it.categories.remove(i);
        }
        None => it.categories.push(cat),
    })?;
    ok()
}

fn importance(s: &mut Session, p: &Value) -> CmdResult {
    let level = match need_str(p, "level")?.to_ascii_lowercase().as_str() {
        "low" => 0,
        "normal" => 1,
        "high" => 2,
        other => return Err(EngineError::BadParams(format!("\"{other}\" is not low, normal or high"))),
    };
    s.edit_selected("Importance", |it| it.props.set(openmailclient_model::pid::IMPORTANCE, openmailclient_model::PropValue::Int32(level)))?;
    ok()
}

fn move_to_kind(s: &mut Session, what: &'static str, kind: FolderKind, name: &str) -> CmdResult {
    let (store, _) = s.current.ok_or(StoreError::NoFolder)?;
    let target = s.find_folder(store, kind).ok_or_else(|| EngineError::Format(format!("this data file has no {name} folder")))?;
    s.move_selected(what, target)?;
    ok()
}

/// Delete moves to Deleted Items; deleting from Deleted Items removes the item for good.
fn delete(s: &mut Session, _: &Value) -> CmdResult {
    let (store, folder) = s.current.ok_or(StoreError::NoFolder)?;
    let in_deleted = s.folder_info(store, folder).is_some_and(|f| f.kind == FolderKind::Deleted);
    let deleted = s.find_folder(store, FolderKind::Deleted);
    match deleted {
        Some(target) if !in_deleted => s.move_selected("Delete", target)?,
        _ => {
            let id = s.selected.ok_or(StoreError::NoItem)?;
            let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
            let item = Box::new(st.remove(folder, openmailclient_model::ItemId(id))?);
            super::imap_sync::forget(s, store, &item);
            // Gone from the data file, and kept by the session so that it can be taken back.
            s.did(Step::Removed { what: "Delete", store, folder, was: id, item });
            s.selected = None;
            s.open = None;
            s.refresh_folders(store);
            s.reload_rows()?;
        }
    }
    ok()
}

fn archive(s: &mut Session, _: &Value) -> CmdResult {
    move_to_kind(s, "Archive", FolderKind::Archive, "Archive")
}

fn junk(s: &mut Session, _: &Value) -> CmdResult {
    move_to_kind(s, "Junk", FolderKind::Junk, "Junk Email")
}

fn move_to(s: &mut Session, p: &Value) -> CmdResult {
    let (store, _) = s.current.ok_or(StoreError::NoFolder)?;
    let target = match (p_u64(p, "folderId"), p_str(p, "folder")) {
        (Some(id), _) => openmailclient_model::FolderId(id),
        (None, Some(path)) => s.folder_by_path(store, path).ok_or_else(|| EngineError::BadParams(format!("there is no folder \"{path}\"")))?,
        _ => return Err(EngineError::BadParams("\"folder\" (path) or \"folderId\" is required".into())),
    };
    s.move_selected("Move", target)?;
    ok()
}

fn can_undo(s: &Session) -> bool {
    !s.undo.is_empty() && s.compose.is_none()
}

fn undo(s: &mut Session, _: &Value) -> CmdResult {
    let undone = s.undo_last()?;
    Ok(json!({ "undone": undone, "left": s.undo.len() }))
}

fn inspect(s: &mut Session, _: &Value) -> CmdResult {
    Ok(s.inspect())
}

/// Every property of the selected item, for inspection: id, type and a short rendering.
fn properties(s: &mut Session, _: &Value) -> CmdResult {
    let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
    let props: Vec<Value> = item
        .props
        .iter()
        .map(|(id, v)| {
            let text: String = match v {
                openmailclient_model::PropValue::String(t) => t.chars().take(120).collect(),
                openmailclient_model::PropValue::Binary(b) => format!("{} bytes", b.len()),
                other => format!("{other:?}").chars().take(120).collect(),
            };
            json!({ "id": format!("0x{id:04X}"), "type": format!("0x{:04X}", v.ty()), "value": text })
        })
        .collect();
    Ok(json!({ "properties": props, "recipients": item.recipients.len(), "attachments": item.attachments.len() }))
}
