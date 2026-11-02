//! View commands: module switching, arrangement, panes, filter and search.

use super::{CommandSpec, always, has_folder, need_str, ok, p_bool, p_str};
use crate::{CmdResult, EngineError, Module, ReadingPane, Session, SortKey};
use serde_json::Value;

const MAX_QUERY: usize = 512;

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "nav.module",
            label: "Switch Module",
            tab: "",
            group: "",
            shortcut: "Ctrl+1…4",
            params: r#"{"module": "mail" | "calendar" | "people" | "tasks"}"#,
            enabled: always,
            run: module,
        },
        CommandSpec {
            id: "view.sortBy",
            label: "Arrange By",
            tab: "View",
            group: "Arrangement",
            shortcut: "",
            params: r#"{"key": "date" | "from" | "to" | "subject" | "size" | "importance" | "flag" | "attachments"}; optional "descending""#,
            enabled: has_folder,
            run: sort_by,
        },
        CommandSpec {
            id: "view.reverseSort",
            label: "Reverse Sort",
            tab: "View",
            group: "Arrangement",
            shortcut: "",
            params: "",
            enabled: has_folder,
            run: reverse_sort,
        },
        CommandSpec {
            id: "view.columns",
            label: "Add Columns",
            tab: "View",
            group: "Arrangement",
            shortcut: "",
            params: r#"{"columns": names from the left, or "default"} or {"show": a name, optional "at": its place from 0} or {"hide": a name} or {"move": a name, "at": its place}; the names: who, from, to, subject, date, size, notes, categories, importance, flag, attachment, preview. Table Style prints the same columns. Returns {"columns"}"#,
            enabled: always,
            run: columns,
        },
        CommandSpec {
            id: "view.resetView",
            label: "Reset View",
            tab: "View",
            group: "Current View",
            shortcut: "",
            params: "; the open folder's list arranged by date, newest first, with the first columns and the reading pane at the right, kept as its view",
            enabled: has_folder,
            run: reset_view,
        },
        CommandSpec {
            id: "view.readingPane",
            label: "Reading Pane",
            tab: "View",
            group: "Layout",
            shortcut: "",
            params: r#"{"position": "right" | "bottom" | "off"}"#,
            enabled: always,
            run: reading_pane,
        },
        CommandSpec {
            id: "view.folderPane",
            label: "Folder Pane",
            tab: "View",
            group: "Layout",
            shortcut: "",
            params: r#"{"show": true | false}; default toggles"#,
            enabled: always,
            run: folder_pane,
        },
        CommandSpec {
            id: "view.animations",
            label: "Animated Pictures",
            tab: "View",
            group: "Layout",
            shortcut: "",
            params: r#"{"on": true | false}; default toggles. Pictures that move in a message are played, or each stands still on its first frame"#,
            enabled: always,
            run: animations,
        },
        CommandSpec {
            id: "view.filterUnread",
            label: "Unread",
            tab: "Home",
            group: "Find",
            shortcut: "",
            params: r#"{"on": true | false}; default toggles"#,
            enabled: has_folder,
            run: filter_unread,
        },
        CommandSpec {
            id: "view.filter",
            label: "Filter Email",
            tab: "Home",
            group: "Find",
            shortcut: "",
            params: r#"{"by": "unread" | "attachments" | "flagged" | "important" | "categorized", "on": true | false (default toggles)}, or {"clear": true} for every message; filters chosen together show only what is each of them; returns {"filter": [those on], "count": messages shown}"#,
            enabled: has_folder,
            run: filter,
        },
        CommandSpec {
            id: "search.query",
            label: "Search",
            tab: "",
            group: "",
            shortcut: "Ctrl+E",
            params: r#"{"text": what to find in the current folder: words, "quoted words", -word, OR, from: to: subject: category: has:attachment is:unread is:flagged is:important before: after: on:2026-10-05 size:>2mb}; returns {"count"}"#,
            enabled: has_folder,
            run: search,
        },
        CommandSpec {
            id: "search.clear",
            label: "Close Search",
            tab: "Search",
            group: "Close",
            shortcut: "Esc",
            params: "",
            enabled: has_folder,
            run: clear_search,
        },
    ]
}

fn module(s: &mut Session, p: &Value) -> CmdResult {
    let name = need_str(p, "module")?;
    s.module = Module::parse(name).ok_or_else(|| EngineError::BadParams(format!("\"{name}\" is not mail, calendar, people or tasks")))?;
    if s.module == Module::Calendar {
        s.reload_calendar()?;
    }
    if s.module == Module::People {
        s.reload_people()?;
    }
    if s.module == Module::Tasks {
        s.reload_tasks()?;
    }
    ok()
}

fn sort_by(s: &mut Session, p: &Value) -> CmdResult {
    let name = need_str(p, "key")?;
    let key = SortKey::parse(name).ok_or_else(|| EngineError::BadParams(format!("\"{name}\" is not a column to arrange by")))?;
    // Choosing the current arrangement again flips its order, like clicking a column header.
    s.sort_desc = match p_bool(p, "descending") {
        Some(d) => d,
        None if key == s.sort => !s.sort_desc,
        None => matches!(key, SortKey::Date | SortKey::Size | SortKey::Importance | SortKey::Flag | SortKey::Attachments),
    };
    s.sort = key;
    s.reload_rows()?;
    crate::folder_view::keep(s);
    ok()
}

/// The list's columns as `p` asks, then Table Style's the same, as Outlook prints the view.
fn columns(s: &mut Session, p: &Value) -> CmdResult {
    use crate::print::Field;
    let named =
        |key: &str| -> Result<Option<Field>, EngineError> {
            match p.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => v.as_str().and_then(Field::named).map(Some).ok_or_else(|| {
                    EngineError::BadParams(format!("\"{key}\" names no column; there are: {}", Field::ALL.map(Field::name).join(", ")))
                }),
            }
        };
    let at = match p.get("at") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().ok_or_else(|| EngineError::BadParams("\"at\" is a place from 0".into()))?),
    };
    let mut now = s.list_columns.clone();
    if p.get("columns").is_some_and(|v| !v.is_null()) {
        let base = crate::print::Listing { columns: now.clone(), ..crate::print::Listing::default() };
        let asked = crate::print::listing_with(&base, &serde_json::json!({ "columns": p.get("columns") }))?;
        // "default" is the list's own first columns, not the printed page's.
        now = match p.get("columns").and_then(Value::as_str) {
            Some(_) => Field::LIST.to_vec(),
            None => asked.columns,
        };
    }
    let place = |len: usize| at.map_or(len, |a| usize::try_from(a).unwrap_or(usize::MAX).min(len));
    if let Some(f) = named("show")? {
        now.retain(|c| *c != f);
        let i = place(now.len());
        now.insert(i, f);
    }
    if let Some(f) = named("hide")? {
        if now == [f] {
            return Err(EngineError::BadParams("the list keeps one column at least".into()));
        }
        now.retain(|c| *c != f);
    }
    if let Some(f) = named("move")? {
        let from = now.iter().position(|c| *c == f).ok_or_else(|| EngineError::BadParams(format!("{} is not a column of the list", f.name())))?;
        now.remove(from);
        let i = place(now.len());
        now.insert(i, f);
    }
    s.list_columns = now;
    s.print_list.columns = s.list_columns.clone();
    crate::folder_view::keep(s);
    Ok(serde_json::json!({ "columns": s.list_columns.iter().map(|f| f.name()).collect::<Vec<_>>() }))
}

fn reverse_sort(s: &mut Session, _: &Value) -> CmdResult {
    s.sort_desc = !s.sort_desc;
    s.reload_rows()?;
    crate::folder_view::keep(s);
    ok()
}

fn reset_view(s: &mut Session, _: &Value) -> CmdResult {
    crate::folder_view::FolderView::default().show(s);
    s.reload_rows()?;
    crate::folder_view::keep(s);
    ok()
}

fn reading_pane(s: &mut Session, p: &Value) -> CmdResult {
    s.reading_pane = match need_str(p, "position")?.to_ascii_lowercase().as_str() {
        "right" => ReadingPane::Right,
        "bottom" => ReadingPane::Bottom,
        "off" => ReadingPane::Off,
        other => return Err(EngineError::BadParams(format!("\"{other}\" is not right, bottom or off"))),
    };
    crate::folder_view::keep(s);
    ok()
}

fn folder_pane(s: &mut Session, p: &Value) -> CmdResult {
    s.folder_pane = p_bool(p, "show").unwrap_or(!s.folder_pane);
    ok()
}

fn animations(s: &mut Session, p: &Value) -> CmdResult {
    s.animate = p_bool(p, "on").unwrap_or(!s.animate);
    Ok(serde_json::json!({ "on": s.animate }))
}

fn filter_unread(s: &mut Session, p: &Value) -> CmdResult {
    s.unread_only = p_bool(p, "on").unwrap_or(!s.unread_only);
    s.reload_rows()?;
    ok()
}

fn filter(s: &mut Session, p: &Value) -> CmdResult {
    if p_bool(p, "clear").unwrap_or(false) {
        s.unread_only = false;
        s.filter = crate::MailFilter::default();
    } else {
        let by = need_str(p, "by")?;
        let now = if by == "unread" { Some(s.unread_only) } else { s.filter.get(by) };
        let Some(now) = now else {
            return Err(EngineError::BadParams(format!(
                "\"{}\" is not unread, attachments, flagged, important or categorized",
                by.chars().take(32).collect::<String>()
            )));
        };
        let on = p_bool(p, "on").unwrap_or(!now);
        if by == "unread" {
            s.unread_only = on;
        } else {
            s.filter.set(by, on);
        }
    }
    s.reload_rows()?;
    let on = s.filters();
    let words: Vec<&str> = on
        .iter()
        .map(|f| match *f {
            "unread" => "unread",
            "attachments" => "with attachments",
            "flagged" => "flagged",
            "important" => "of high importance",
            _ => "with a category",
        })
        .collect();
    s.status = if words.is_empty() { "Every message is shown".to_string() } else { format!("Only messages {}", words.join(" and ")) };
    Ok(serde_json::json!({ "filter": on, "count": s.rows.len() }))
}

fn search(s: &mut Session, p: &Value) -> CmdResult {
    s.search = p_str(p, "text").unwrap_or("").chars().take(MAX_QUERY).collect();
    s.reload_rows()?;
    if s.search.trim().is_empty() {
        return ok();
    }
    let count = s.rows.len();
    s.status = match count {
        0 => "Nothing found".to_string(),
        1 => "1 item found".to_string(),
        n => format!("{n} items found"),
    };
    Ok(serde_json::json!({ "count": count }))
}

fn clear_search(s: &mut Session, _: &Value) -> CmdResult {
    s.search.clear();
    s.reload_rows()?;
    ok()
}
