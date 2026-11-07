//! Mail read without a window (M6.3): the folders of every data file, a folder's messages and a
//! message's text, for agents, the CLI and the MCP server. None of these change what is shown,
//! the selection or whether a message is read. What strangers wrote (names, addresses,
//! subjects, previews, bodies, attachment names) is under `untrusted_content`.

use super::calendar::stamp;
use super::{CommandSpec, always, p_bool, p_str, p_u64};
use crate::{CmdResult, EngineError, Session};
use openmailclient_model::{FolderId, FolderInfo, FolderKind, RECIP_CC, RECIP_TO, StoreError, Summary};
use serde_json::{Value, json};

/// Folders listed at most, over every data file.
const MAX_FOLDERS: usize = 5_000;
/// Messages listed at most in one answer.
const MAX_ITEMS: u64 = 500;
/// Messages listed when no limit is given.
const DEFAULT_ITEMS: u64 = 50;
/// Characters of a body given when no limit is given, and at most.
const DEFAULT_CHARS: u64 = 20_000;
const MAX_CHARS: u64 = 200_000;
/// Folders above one another followed at most when a path is made: a cycle ends there.
const MAX_DEPTH: usize = 64;

pub fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "mail.folders",
            label: "Folder List",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{}; returns {"folders": [{"store", "file", "id", "path", "name", "kind", "items", "unread", "current"}]} of every data file open, hidden folders left out; the selection does not change"#,
            enabled: always,
            run: folders,
        },
        CommandSpec {
            id: "mail.items",
            label: "Message List",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"folder": a path as mail.folders gives it, or "folderId", with "store" (default: the folder shown), "offset" (default 0), "limit" (1 to 500, default 50), "unreadOnly": true}; returns {"store", "folder", "path", "total", "offset", "items": [{"id", "time", "read", "flag", "importance", "attachments", "size", "untrusted_content": {"from", "to", "subject", "preview"}}]}, the newest first; nothing is marked read"#,
            enabled: always,
            run: items,
        },
        CommandSpec {
            id: "mail.read",
            label: "Read Message",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id" (default: the selected message), "folder" or "folderId" and "store" (default: the folder shown), "maxChars" (default 20000, at most 200000)}; returns {"id", "time", "read", "flag", "importance", "html": whether it has an HTML body, "cut": whether the body was cut, "untrusted_content": {"from", "fromAddress", "to", "cc", "subject", "body": its text, "attachments": [{"index", "name", "size"}]}}; it is not marked read (mail.markRead does that)"#,
            enabled: always,
            run: read,
        },
        CommandSpec {
            id: "mail.events",
            label: "Events",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"since": the "last" of the answer before (default 0)}; returns {"last", "missed": whether some after "since" are no longer kept, "events": [{"seq", "kind": "newMail" (with "store", "folder", "item", "account", "untrusted_content": {"from", "subject", "preview"}), "jobEnded" (with "job", "said", "ok", "stopped") or "reminder" (with "reminder")}], "folders": [[store, folder id, items, unread]] of every folder mail.folders lists}; nothing changes"#,
            enabled: always,
            run: events,
        },
    ]
}

/// What came since an agent last asked, and every folder's counts, for it to see what changed.
fn events(s: &mut Session, p: &Value) -> CmdResult {
    let since = match p.get("since") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_u64().ok_or_else(|| EngineError::BadParams("\"since\" is the \"last\" of an answer before".into()))?,
    };
    let (events, missed) = s.events.since(since);
    let mut folders = Vec::new();
    for (store, list) in s.folders.iter().enumerate() {
        for f in list.iter().filter(|f| !f.hidden && f.kind != FolderKind::Root).take(MAX_FOLDERS.saturating_sub(folders.len())) {
            folders.push(json!([store, f.id.0, f.total, f.unread]));
        }
    }
    Ok(json!({ "last": s.events.last(), "missed": missed, "events": events, "folders": folders }))
}

/// A folder's path from the top of its data file, as `mail.select` and `folder.select` take it.
fn path_of(list: &[FolderInfo], f: &FolderInfo) -> String {
    let mut parts = vec![f.name.as_str()];
    let mut cur = f.parent;
    while let Some(p) = cur.and_then(|id| list.iter().find(|x| x.id == id)) {
        // The top of a data file has no name of its own in a path.
        if p.kind == FolderKind::Root || parts.len() >= MAX_DEPTH {
            break;
        }
        parts.push(p.name.as_str());
        cur = p.parent;
    }
    parts.reverse();
    parts.join("/")
}

fn folders(s: &mut Session, _: &Value) -> CmdResult {
    let mut out = Vec::new();
    for (store, list) in s.folders.iter().enumerate() {
        let file = s.stores.get(store).map(|st| st.display_name()).unwrap_or_default();
        for f in list.iter().filter(|f| !f.hidden && f.kind != FolderKind::Root).take(MAX_FOLDERS.saturating_sub(out.len())) {
            out.push(json!({
                "store": store,
                "file": file,
                "id": f.id.0,
                "path": path_of(list, f),
                "name": f.name,
                "kind": format!("{:?}", f.kind).to_lowercase(),
                "items": f.total,
                "unread": f.unread,
                "current": s.current == Some((store, f.id)),
            }));
        }
    }
    Ok(json!({ "folders": out }))
}

/// The folder a command names, or the one shown.
fn folder_of(s: &Session, p: &Value) -> Result<(usize, FolderId), EngineError> {
    let named = p.get("folder").is_some() || p.get("folderId").is_some();
    let store = match p.get("store") {
        None | Some(Value::Null) => s.current.map(|(st, _)| st).unwrap_or(0),
        Some(v) => {
            v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| EngineError::BadParams("\"store\" is a data file's number".into()))?
        }
    };
    if !named {
        return match s.current {
            Some((st, f)) if p.get("store").is_none() || st == store => Ok((st, f)),
            _ => Err(StoreError::NoFolder.into()),
        };
    }
    let folder = match (p_str(p, "folder"), p_u64(p, "folderId")) {
        (_, Some(id)) => Some(FolderId(id)).filter(|id| s.folder_info(store, *id).is_some()),
        (Some(path), None) => s.folder_by_path(store, path),
        (None, None) => return Err(EngineError::BadParams("\"folder\" is a path and \"folderId\" a number".into())),
    };
    folder.map(|f| (store, f)).ok_or_else(|| EngineError::BadParams("there is no such folder in that data file".into()))
}

fn row_json(s: &Session, r: &Summary) -> Value {
    json!({
        "id": r.id,
        "time": (r.time != 0).then(|| stamp(s, r.time)),
        "read": r.read,
        "flag": format!("{:?}", r.flag).to_lowercase(),
        "importance": r.importance,
        "attachments": r.has_attachments,
        "size": r.size,
        "untrusted_content": { "from": r.from, "to": r.to, "subject": r.subject, "preview": r.preview },
    })
}

fn items(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = folder_of(s, p)?;
    let offset = p_u64(p, "offset").unwrap_or(0);
    let limit = p_u64(p, "limit").unwrap_or(DEFAULT_ITEMS).clamp(1, MAX_ITEMS);
    let unread_only = p_bool(p, "unreadOnly").unwrap_or(false);
    let mut rows = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.list(folder)?;
    if unread_only {
        rows.retain(|r| !r.read);
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.time));
    let skip = usize::try_from(offset).unwrap_or(usize::MAX);
    let take = usize::try_from(limit).unwrap_or(1);
    let listed: Vec<Value> = rows.iter().skip(skip).take(take).map(|r| row_json(s, r)).collect();
    let path = s.folders.get(store).and_then(|list| list.iter().find(|f| f.id == folder).map(|f| path_of(list, f)));
    Ok(json!({ "store": store, "folder": folder.0, "path": path, "total": rows.len(), "offset": offset, "items": listed }))
}

fn read(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = folder_of(s, p)?;
    let id = match p.get("id") {
        None | Some(Value::Null) => s
            .selected
            .filter(|_| s.current == Some((store, folder)))
            .ok_or_else(|| EngineError::BadParams("no message is selected; give \"id\"".into()))?,
        Some(v) => v.as_u64().ok_or_else(|| EngineError::BadParams("\"id\" must be a number".into()))?,
    };
    let max = usize::try_from(p_u64(p, "maxChars").unwrap_or(DEFAULT_CHARS).clamp(1, MAX_CHARS)).unwrap_or(1);
    let item = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.load(folder, openmailclient_model::ItemId(id))?;
    let body = item.body_text();
    let cut = body.chars().nth(max).is_some();
    let attachments: Vec<Value> =
        item.attachments.iter().enumerate().map(|(i, a)| json!({ "index": i, "name": a.name(), "size": a.data.len() })).collect();
    let time = item.time();
    Ok(json!({
        "id": id,
        "time": (time != 0).then(|| stamp(s, time)),
        "read": item.is_read(),
        "flag": format!("{:?}", item.flag()).to_lowercase(),
        "importance": item.importance(),
        "html": item.body_html().is_some(),
        "cut": cut,
        "untrusted_content": {
            "from": item.sender_name(),
            "fromAddress": item.sender_email(),
            "to": item.display_recipients(RECIP_TO),
            "cc": item.display_recipients(RECIP_CC),
            "subject": item.subject(),
            "body": body.chars().take(max).collect::<String>(),
            "attachments": attachments,
        },
    }))
}

#[cfg(test)]
mod tests {
    use crate::Session;
    use serde_json::{Value, json};

    fn sample() -> Session {
        let mut s = Session::new();
        s.run("file.openSample", &Value::Null).unwrap();
        s
    }

    #[test]
    fn folders_are_listed_with_paths_that_select_them() {
        let mut s = sample();
        let out = s.run("mail.folders", &json!({})).unwrap();
        let folders = out["folders"].as_array().unwrap();
        let inbox = folders.iter().find(|f| f["name"] == "Inbox").unwrap();
        assert_eq!(inbox["path"], "Inbox");
        assert_eq!(inbox["kind"], "inbox");
        // Every path given is one the engine finds again.
        for f in folders {
            let store = f["store"].as_u64().unwrap() as usize;
            assert_eq!(s.folder_by_path(store, f["path"].as_str().unwrap()).map(|id| id.0), f["id"].as_u64(), "{f}");
        }
    }

    #[test]
    fn a_folder_is_listed_and_a_message_read_without_changing_anything() {
        let mut s = sample();
        s.run("mail.select", &json!({ "folder": "Inbox", "index": 0 })).unwrap();
        let before = (s.current, s.selected, s.unsaved());
        let list = s.run("mail.items", &json!({ "folder": "Inbox", "limit": 2 })).unwrap();
        let items = list["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert!(list["total"].as_u64().unwrap() >= 2);
        assert!(items[0]["untrusted_content"]["subject"].is_string());
        // The newest first.
        assert!(items[0]["time"].as_str() >= items[1]["time"].as_str());
        let unread = s.run("mail.items", &json!({ "folder": "Inbox", "unreadOnly": true, "limit": 500 })).unwrap();
        assert!(unread["items"].as_array().unwrap().iter().all(|i| i["read"] == false));
        let id = items.iter().find(|i| i["read"] == false).unwrap_or(&items[0])["id"].clone();
        let was_read = s.run("mail.read", &json!({ "folder": "Inbox", "id": id })).unwrap();
        assert!(!was_read["untrusted_content"]["body"].as_str().unwrap().is_empty());
        let again = s.run("mail.read", &json!({ "folder": "Inbox", "id": id })).unwrap();
        assert_eq!(was_read["read"], again["read"]);
        // The selected message by default, cut to what is asked.
        let short = s.run("mail.read", &json!({ "maxChars": 5 })).unwrap();
        assert_eq!(short["id"].as_u64(), s.selected);
        assert!(short["untrusted_content"]["body"].as_str().unwrap().chars().count() <= 5);
        assert_eq!((s.current, s.selected, s.unsaved()), before);
    }

    #[test]
    fn events_are_told_once_with_every_folder_s_counts() {
        let mut s = sample();
        let first = s.run("mail.events", &json!({})).unwrap();
        let folders = first["folders"].as_array().unwrap();
        assert_eq!(folders.len(), s.run("mail.folders", &json!({})).unwrap()["folders"].as_array().unwrap().len());
        let last = first["last"].as_u64().unwrap();
        s.events.note("jobEnded", json!({ "job": "Sending", "ok": true }));
        let next = s.run("mail.events", &json!({ "since": last })).unwrap();
        assert_eq!(next["events"].as_array().unwrap().len(), 1);
        assert_eq!(next["events"][0]["kind"], "jobEnded");
        assert_eq!(next["missed"], false);
        let again = s.run("mail.events", &json!({ "since": next["last"] })).unwrap();
        assert!(again["events"].as_array().unwrap().is_empty());
        assert!(s.run("mail.events", &json!({ "since": "x" })).is_err());
        // A message marked read is seen in its folder's counts.
        s.run("mail.select", &json!({ "folder": "Inbox", "index": 0 })).unwrap();
        s.run("mail.markRead", &Value::Null).unwrap();
        let before = s.run("mail.events", &json!({})).unwrap()["folders"].clone();
        s.run("mail.markUnread", &Value::Null).unwrap();
        let after = s.run("mail.events", &json!({})).unwrap()["folders"].clone();
        assert_ne!(before, after);
    }

    #[test]
    fn what_is_not_there_is_an_error() {
        let mut s = sample();
        assert!(s.run("mail.items", &json!({ "folder": "No Such Folder" })).is_err());
        assert!(s.run("mail.items", &json!({ "folderId": 999_999_999u64 })).is_err());
        assert!(s.run("mail.items", &json!({ "folder": "Inbox", "store": 99 })).is_err());
        assert!(s.run("mail.read", &json!({ "folder": "Inbox", "id": 999_999_999u64 })).is_err());
        assert!(s.run("mail.read", &json!({ "folder": "Inbox", "id": "x" })).is_err());
        // Far past the end is an empty page, not an error.
        let past = s.run("mail.items", &json!({ "folder": "Inbox", "offset": u64::MAX, "limit": u64::MAX })).unwrap();
        assert_eq!(past["items"].as_array().unwrap().len(), 0);
        let empty = Session::new().run("mail.items", &json!({})).is_err();
        assert!(empty);
    }
}
