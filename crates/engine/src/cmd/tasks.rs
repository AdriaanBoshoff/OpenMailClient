//! The Tasks module: tasks of a Tasks folder ([MS-OXOTASK]) listed in Outlook's views, found,
//! made, changed, marked complete and deleted; and mail flagged for follow-up ([MS-OXOFLAG]),
//! which stands beside the tasks on the To-Do List and is followed up, done and taken off it
//! from there or from the message list. The model is `openmailclient_model::task`.

use super::people::{categories_param, line_param};
use super::{CommandSpec, always, clock, has_selection_writable, need_str, ok, p_u64};
use crate::{CmdResult, EngineError, Module, Session, undo::Step};
use openmailclient_model::calendar;
use openmailclient_model::task::{self, FollowUp, MAX_COMPANIES, MAX_NOTES, MAX_WORK, Status, Task};
use openmailclient_model::zone::Zone;
use openmailclient_model::{FolderId, FolderKind, Item, ItemId, StoreError};
use serde_json::{Value, json};

/// Items of a Tasks folder read at most.
const MAX_ITEMS: usize = 20_000;
/// Flagged messages put on the To-Do List at most, the newest flagged first.
const MAX_FLAGGED: usize = 2_000;
/// Tasks folders listed at most.
const MAX_FOLDERS: usize = 500;
/// Longest search taken, in characters.
const MAX_QUERY: usize = 4_000;

/// How the Tasks module shows its tasks, as Outlook's Current View offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    /// Tasks not done and mail flagged for follow-up, by when they are due.
    ToDoList,
    /// Every task, with how far along it is.
    #[default]
    SimpleList,
    Detailed,
    /// The most important first.
    Prioritized,
    Active,
    Completed,
    /// Due today, or before and not done.
    Today,
    Next7Days,
    Overdue,
}

impl View {
    pub const ALL: [Self; 9] = [
        Self::ToDoList,
        Self::SimpleList,
        Self::Detailed,
        Self::Prioritized,
        Self::Active,
        Self::Completed,
        Self::Today,
        Self::Next7Days,
        Self::Overdue,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::ToDoList => "toDoList",
            Self::SimpleList => "simpleList",
            Self::Detailed => "detailed",
            Self::Prioritized => "prioritized",
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Today => "today",
            Self::Next7Days => "next7Days",
            Self::Overdue => "overdue",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ToDoList => "To-Do List",
            Self::SimpleList => "Simple List",
            Self::Detailed => "Detailed",
            Self::Prioritized => "Prioritized",
            Self::Active => "Active",
            Self::Completed => "Completed",
            Self::Today => "Today",
            Self::Next7Days => "Next 7 Days",
            Self::Overdue => "Overdue",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.name().eq_ignore_ascii_case(s) || v.label().eq_ignore_ascii_case(s))
    }

    /// Whether mail flagged for follow-up stands among its tasks.
    pub fn with_mail(self) -> bool {
        self == Self::ToDoList
    }

    /// Whether `t` is shown in it on day `today`.
    pub fn shows(self, t: &Task, today: i64) -> bool {
        match self {
            Self::SimpleList | Self::Detailed | Self::Prioritized => true,
            Self::ToDoList | Self::Active => !t.is_complete(),
            Self::Completed => t.is_complete(),
            Self::Today => !t.is_complete() && t.due.is_some_and(|d| d <= today),
            Self::Next7Days => !t.is_complete() && t.due.is_some_and(|d| d >= today && d < today.saturating_add(7)),
            Self::Overdue => t.is_overdue(today),
        }
    }
}

/// A task or a flagged message as a view shows it, and where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub store: usize,
    pub folder: FolderId,
    pub id: u64,
    pub task: Task,
    /// Whose copy it is, when it was given to someone or by someone.
    pub assignment: task::request::Assignment,
}

impl Row {
    pub fn key(&self) -> (usize, FolderId, u64) {
        (self.store, self.folder, self.id)
    }
}

/// What the Tasks module shows.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    pub view: View,
    /// The tasks and messages shown, in the order the view puts them.
    pub rows: Vec<Row>,
    /// How many tasks the folder has, shown or not.
    pub total: usize,
    /// Where they are: the store and its Tasks folder.
    pub folder: Option<(usize, FolderId)>,
    /// The one chosen: its data file, folder and id.
    pub selected: Option<(usize, FolderId, u64)>,
    /// What they are searched for; empty for all.
    pub query: String,
    /// Items of the folder that are not tasks, or could not be read.
    pub unreadable: usize,
    /// The Tasks folder chosen to be shown (`tasks.open`).
    pub chosen: Option<(usize, FolderId)>,
    /// Flagged messages left off the To-Do List because it had as many as it holds.
    pub left_out: usize,
}

impl Shown {
    pub fn chosen_row(&self) -> Option<&Row> {
        let key = self.selected?;
        self.rows.iter().find(|r| r.key() == key)
    }
}

const TASK_PARAMS: &str = r#"{"subject", "start": "YYYY-MM-DD" or null, "due": "YYYY-MM-DD" or null (a start after it is moved back to it), "status": "notStarted" | "inProgress" | "complete" | "waiting" | "deferred", "percent": 0 to 100 (100 is done, and done is 100), "importance": "low" | "normal" | "high", "reminder": "YYYY-MM-DD HH:MM" local time, or null for none (put away once it is done), "completed": "YYYY-MM-DD", the day it was done (default: today), "categories": [names], "notes", "owner", "private": bool, "totalWork" and "actualWork": minutes, "mileage", "billing", "companies": [names], "repeat" and "regenerate" as tasks.recurrence takes them}; text given as "" takes it away. A message on the To-Do List takes "subject" (its to-do title; its own subject stays), the days, "status", "percent", "completed" and "reminder""#;
const ROW_PARAMS: &str =
    r#"{"id", "folder" and "store" of a row tasks.items gives (default: the one chosen; "folder" and "store" default to the Tasks folder shown)}"#;
const FOLLOW_UP_PARAMS: &str = r#"{"when": "today" | "tomorrow" | "thisWeek" | "nextWeek" | "noDate", or "start" and "due": "YYYY-MM-DD" (a custom flag), "reminder": "YYYY-MM-DD HH:MM" or null}"#;

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "tasks.view",
            label: "To-Do List, Detailed, Simple List",
            tab: "Home",
            group: "Current View",
            shortcut: "",
            params: r#"{"view": "toDoList" | "simpleList" | "detailed" | "prioritized" | "active" | "completed" | "today" | "next7Days" | "overdue"}; the To-Do List has mail flagged for follow-up beside the tasks not done"#,
            enabled: always,
            run: view,
        },
        CommandSpec {
            id: "tasks.folders",
            label: "My Tasks",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{}; returns {"folders": [{"store", "file", "folder", "name", "shown", "items", "readOnly"}]}"#,
            enabled: always,
            run: folders,
        },
        CommandSpec {
            id: "tasks.open",
            label: "Open Tasks Folder",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"store", "folder"}, as tasks.folders gives them; those tasks are shown, and new ones go there"#,
            enabled: always,
            run: open,
        },
        CommandSpec {
            id: "tasks.items",
            label: "Tasks",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"view" (default: the one shown), "query": words each of which is in what is found (default: the search shown)}; returns {"items": [{"id", "store", "folder", "kind": "task" | "mail", "status", "percent", "importance", "start", "due", "completed", "reminder", "overdue", "recurring", "private", "categories", "totalWork", "actualWork", "untrusted_content": {"subject", "notes", "owner", "mileage", "billing", "companies"}}], "total", "unreadable", "leftOut"}"#,
            enabled: always,
            run: items,
        },
        CommandSpec {
            id: "tasks.select",
            label: "Select Task",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id", "folder", "store" of a row shown, or "id": null for none}"#,
            enabled: has_rows,
            run: select,
        },
        CommandSpec {
            id: "tasks.search",
            label: "Search Tasks",
            tab: "Home",
            group: "Find",
            shortcut: "Ctrl+E",
            params: r#"{"query": words, each of which is in a subject, owner, category, company, note or status shown; "" for all}"#,
            enabled: always,
            run: search,
        },
        CommandSpec {
            id: "tasks.new",
            label: "New Task",
            tab: "Home",
            group: "New",
            shortcut: "Ctrl+Shift+K",
            params: TASK_PARAMS,
            enabled: has_writable_store,
            run: new,
        },
        CommandSpec {
            id: "tasks.change",
            label: "Edit Task",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"the row (as tasks.markComplete takes it), and any of those tasks.new takes"#,
            enabled: has_rows,
            run: change,
        },
        CommandSpec {
            id: "tasks.markComplete",
            label: "Mark Complete",
            tab: "Home",
            group: "Manage Task",
            shortcut: "",
            params: ROW_PARAMS,
            enabled: has_rows,
            run: mark_complete,
        },
        CommandSpec {
            id: "tasks.recurrence",
            label: "Recurrence",
            tab: "Task",
            group: "Recurrence",
            shortcut: "",
            params: r#"the row (as tasks.markComplete takes it), "repeat": as calendar.new takes it ({"every": "day" | "weekday" | "week" | "month" | "year", "interval", "on": [weekday names], "nth": 1 to 4 or "last", "day", "lastDay", "count", "until"}), or null to stop it recurring, "regenerate": true for a new task a period after each one is completed rather than on the pattern's days; the series starts on the task's due day (else its start, else today), and the task is due on its first occurrence; returns {"pattern": in words, "due"}"#,
            enabled: has_rows,
            run: recurrence,
        },
        CommandSpec {
            id: "tasks.skipOccurrence",
            label: "Skip Occurrence",
            tab: "Task",
            group: "Manage Task",
            shortcut: "",
            params: r#"the row (as tasks.markComplete takes it); a recurring task moves on to its next occurrence without this one being kept as done; returns {"due"}"#,
            enabled: has_rows,
            run: skip_occurrence,
        },
        CommandSpec {
            id: "tasks.followUp",
            label: "Today, Tomorrow, This Week",
            tab: "Home",
            group: "Follow Up",
            shortcut: "",
            params: r#"the row (as tasks.markComplete takes it), and when, as mail.followUp takes it; a task takes the days, a message is flagged again"#,
            enabled: has_rows,
            run: follow_up,
        },
        CommandSpec {
            id: "tasks.removeFromList",
            label: "Remove from List",
            tab: "Home",
            group: "Manage Task",
            shortcut: "",
            params: r#"the row (as tasks.markComplete takes it); a message's flag is taken away and the message stays where it is"#,
            enabled: has_rows,
            run: remove_from_list,
        },
        CommandSpec {
            id: "tasks.delete",
            label: "Delete",
            tab: "Home",
            group: "Delete",
            shortcut: "Ctrl+D",
            params: r#"the row (as tasks.markComplete takes it); it goes to Deleted Items"#,
            enabled: has_rows,
            run: delete,
        },
        CommandSpec {
            id: "calendar.fromTask",
            label: "Schedule Task",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"the row (as tasks.markComplete takes it), "start": "YYYY-MM-DD HH:MM" local time, or a date alone for an all-day event, "minutes": how long (default: the work the task is thought to take, from a quarter of an hour to a day, else half an hour); an appointment with the task's subject and notes, as a task dragged onto the Calendar makes; the task stays on its list"#,
            enabled: has_rows,
            run: schedule,
        },
        CommandSpec {
            id: "mail.followUp",
            label: "Follow Up",
            tab: "",
            group: "",
            shortcut: "",
            params: FOLLOW_UP_PARAMS,
            enabled: has_selection_writable,
            run: mail_follow_up,
        },
    ]
}

/// The Tasks folder shown: the one chosen, else the current data file's, else the first.
pub(crate) fn tasks_folder(s: &Session) -> Option<(usize, FolderId)> {
    if let Some((store, folder)) = s.tasks.chosen
        && is_tasks(s, store, folder)
    {
        return Some((store, folder));
    }
    let current = s.current.map(|(store, _)| store);
    current.into_iter().chain(0..s.stores.len()).find_map(|store| s.find_folder(store, FolderKind::Tasks).map(|f| (store, f)))
}

fn is_tasks(s: &Session, store: usize, folder: FolderId) -> bool {
    s.folders.get(store).is_some_and(|f| f.iter().any(|x| x.id == folder && x.kind == FolderKind::Tasks))
}

fn writable(s: &Session, store: usize) -> bool {
    s.stores.get(store).is_some_and(|st| st.writable())
}

fn has_rows(s: &Session) -> bool {
    !s.tasks.rows.is_empty() || !s.todo.tasks.is_empty() || tasks_folder(s).is_some()
}

fn has_writable_store(s: &Session) -> bool {
    tasks_folder(s).map_or_else(|| (0..s.stores.len()).any(|i| writable(s, i)), |(store, _)| writable(s, store))
}

/// The Tasks folder new tasks go to. A data file that has none is given one the first time a
/// task is made in it, as Outlook keeps one in every mailbox.
pub(crate) fn writable_folder(s: &mut Session) -> Result<(usize, FolderId), EngineError> {
    if let Some((store, folder)) = tasks_folder(s) {
        if !writable(s, store) {
            return Err(EngineError::BadParams("the data file with the tasks cannot be written".into()));
        }
        return Ok((store, folder));
    }
    let current = s.current.map(|(store, _)| store);
    let store = current
        .into_iter()
        .chain(0..s.stores.len())
        .find(|&i| writable(s, i))
        .ok_or_else(|| EngineError::BadParams("no data file open can be written".into()))?;
    let folder = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.create_folder_of(None, "Tasks", "IPF.Task")?;
    s.refresh_folders(store);
    Ok((store, folder))
}

/// The folders of a data file mail flagged for follow-up is looked for in: those of mail,
/// not Deleted Items, Junk Email or a search's.
fn mail_folders(s: &Session, store: usize) -> Vec<FolderId> {
    s.folders
        .get(store)
        .map(|list| {
            list.iter()
                .filter(|f| {
                    matches!(
                        f.kind,
                        FolderKind::Inbox
                            | FolderKind::Drafts
                            | FolderKind::Sent
                            | FolderKind::Outbox
                            | FolderKind::Archive
                            | FolderKind::Mail
                            | FolderKind::Other
                    )
                })
                .map(|f| f.id)
                .collect()
        })
        .unwrap_or_default()
}

/// Rows sorted as `view` puts them: by the day they are due (those with none last), the most
/// important first among those due alike, then by subject; the most important first of all in
/// Prioritized, and the most lately done first in Completed.
fn sort(rows: &mut [Row], view: View) {
    rows.sort_by_cached_key(|r| {
        let t = &r.task;
        let due = t.due.unwrap_or(i64::MAX);
        let first = match view {
            View::Prioritized => i64::from(2 - t.importance.clamp(0, 2)),
            View::Completed => t.completed.map_or(i64::MAX, |d| d.saturating_neg()),
            _ => 0,
        };
        (first, due, 2 - t.importance.clamp(0, 2), t.subject.to_lowercase(), r.store, r.folder.0, r.id)
    });
}

impl Session {
    /// Read again what the Tasks module shows.
    pub fn reload_tasks(&mut self) -> Result<(), EngineError> {
        let view = self.tasks.view;
        let query = self.tasks.query.clone();
        let (rows, total, unreadable, left_out) = self.task_rows(view, &query)?;
        self.tasks.folder = tasks_folder(self);
        // As Outlook does, one is chosen while there are any: the one that was, else the one
        // that took its place in the list (as after a delete), else the first.
        if !self.tasks.selected.is_some_and(|key| rows.iter().any(|r| r.key() == key)) {
            let at = self.tasks.selected.and_then(|key| self.tasks.rows.iter().position(|r| r.key() == key)).unwrap_or(0);
            self.tasks.selected = rows.get(at.min(rows.len().saturating_sub(1))).map(Row::key);
        }
        self.tasks.rows = rows;
        self.tasks.total = total;
        self.tasks.unreadable = unreadable;
        self.tasks.left_out = left_out;
        Ok(())
    }

    /// The rows `view` shows that `query` finds; how many tasks the folder has, how many of its
    /// items are not tasks, and how many flagged messages were left out.
    pub(crate) fn task_rows(&mut self, view: View, query: &str) -> Result<(Vec<Row>, usize, usize, usize), EngineError> {
        let zone = self.zone.clone();
        let today = self.today();
        let mut rows = Vec::new();
        let (mut total, mut unreadable, mut left_out) = (0usize, 0usize, 0usize);
        let wanted = |t: &Task| view.shows(t, today) && (query.trim().is_empty() || t.matches(query));
        if let Some((store, folder)) = tasks_folder(self) {
            let st = self.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
            for row in st.list(folder)?.into_iter().take(MAX_ITEMS) {
                let item = match st.load(folder, ItemId(row.id)) {
                    Ok(item) if task::is_task(&item) => item,
                    _ => {
                        unreadable = unreadable.saturating_add(1);
                        continue;
                    }
                };
                total = total.saturating_add(1);
                let t = Task::of(&item, &zone);
                if wanted(&t) {
                    rows.push(Row { store, folder, id: row.id, task: t, assignment: task::request::Assignment::of(&item) });
                }
            }
        }
        if view.with_mail() {
            // Only what the list rows say is flagged is read whole, the newest first.
            let mut flagged = Vec::new();
            for store in 0..self.stores.len() {
                for folder in mail_folders(self, store) {
                    let Some(st) = self.stores.get_mut(store) else { continue };
                    let Ok(list) = st.list(folder) else { continue };
                    flagged.extend(list.into_iter().filter(|r| r.flag != openmailclient_model::Flag::None).map(|r| (r.time, store, folder, r.id)));
                }
            }
            flagged.sort_by_key(|(time, ..)| std::cmp::Reverse(*time));
            left_out = flagged.len().saturating_sub(MAX_FLAGGED);
            for (_, store, folder, id) in flagged.into_iter().take(MAX_FLAGGED) {
                let Some(st) = self.stores.get_mut(store) else { continue };
                let Ok(item) = st.load(folder, ItemId(id)) else { continue };
                if task::is_task(&item) || !task::is_flagged(&item) {
                    continue;
                }
                let t = Task::of(&item, &zone);
                if wanted(&t) {
                    rows.push(Row { store, folder, id, task: t, assignment: task::request::Assignment::default() });
                }
            }
        }
        sort(&mut rows, view);
        Ok((rows, total, unreadable, left_out))
    }
}

fn view(s: &mut Session, p: &Value) -> CmdResult {
    let name = need_str(p, "view")?;
    s.tasks.view = View::parse(name).ok_or_else(|| {
        EngineError::BadParams(format!("\"{}\" is not a view of tasks; tasks.view lists them", name.chars().take(40).collect::<String>()))
    })?;
    s.reload_tasks()?;
    ok()
}

fn folders(s: &mut Session, _: &Value) -> CmdResult {
    let shown = tasks_folder(s);
    let mut out = Vec::new();
    for (store, list) in s.folders.iter().enumerate() {
        let file = s.stores.get(store).map(|st| st.display_name()).unwrap_or_default();
        let read_only = !writable(s, store);
        for f in list.iter().filter(|f| f.kind == FolderKind::Tasks).take(MAX_FOLDERS.saturating_sub(out.len())) {
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
        .ok_or_else(|| EngineError::BadParams("\"store\" is a data file's number, as tasks.folders gives it".into()))?;
    let folder =
        FolderId(p_u64(p, "folder").ok_or_else(|| EngineError::BadParams("\"folder\" is a Tasks folder's id, as tasks.folders gives it".into()))?);
    if !is_tasks(s, store, folder) {
        return Err(EngineError::BadParams("that is not a Tasks folder of a data file open".into()));
    }
    s.tasks.chosen = Some((store, folder));
    s.tasks.selected = None;
    s.reload_tasks()?;
    s.status = format!("Showing {}", s.folder_info(store, folder).map(|f| f.name.clone()).unwrap_or_default());
    ok()
}

/// All a row says, as tasks.new takes it; what a stranger may have written is marked so.
pub fn row_json(s: &Session, r: &Row) -> Value {
    let t = &r.task;
    let day = |d: Option<i64>| d.map(super::calendar::day_text);
    json!({
        "id": r.id,
        "store": r.store,
        "folder": r.folder.0,
        "kind": if t.flagged_mail { "mail" } else { "task" },
        "status": t.status.name(),
        "percent": t.percent,
        "importance": importance_name(t.importance),
        "start": day(t.start),
        "due": day(t.due),
        "completed": day(t.completed),
        "reminder": t.reminder.map(|r| super::calendar::stamp(s, r)),
        "overdue": t.is_overdue(s.today()),
        "recurring": t.recurring,
        "pattern": t.recurrence.as_ref().map(task::recurrence_words),
        "private": t.private,
        "categories": t.categories,
        "totalWork": t.total_work,
        "actualWork": t.actual_work,
        // Given to someone, or by someone: whose copy it is and how it was answered.
        "assigned": (r.assignment.ownership != task::request::Ownership::Own)
            .then(|| json!({ "copy": r.assignment.ownership.name(), "answer": r.assignment.acceptance.name() })),
        // A task may have come from anyone, a flagged message surely did.
        "untrusted_content": {
            "subject": t.subject,
            "notes": t.notes,
            "owner": t.owner,
            "mileage": t.mileage,
            "billing": t.billing,
            "companies": t.companies,
        },
    })
}

fn query_param(p: &Value, or: &str) -> Result<String, EngineError> {
    match p.get("query") {
        Some(Value::String(q)) => Ok(q.chars().filter(|c| !c.is_control()).take(MAX_QUERY).collect::<String>().trim().to_string()),
        None | Some(Value::Null) => Ok(or.to_string()),
        Some(_) => Err(EngineError::BadParams("\"query\" must be text".into())),
    }
}

fn items(s: &mut Session, p: &Value) -> CmdResult {
    let view = match p.get("view") {
        None | Some(Value::Null) => s.tasks.view,
        Some(Value::String(v)) => View::parse(v).ok_or_else(|| EngineError::BadParams("that is not a view of tasks".into()))?,
        Some(_) => return Err(EngineError::BadParams("\"view\" must be text".into())),
    };
    let query = query_param(p, &s.tasks.query.clone())?;
    let (rows, total, unreadable, left_out) = s.task_rows(view, &query)?;
    Ok(json!({
        "items": rows.iter().map(|r| row_json(s, r)).collect::<Vec<_>>(),
        "total": total,
        "unreadable": unreadable,
        "leftOut": left_out,
    }))
}

/// The row a command is about: the one named by "id" (with its "folder" and "store", which
/// default to the Tasks folder shown), else the one chosen.
pub(super) fn row_key(s: &Session, p: &Value) -> Result<(usize, FolderId, u64), EngineError> {
    let Some(v) = p.get("id").filter(|v| !v.is_null()) else {
        return s.tasks.selected.ok_or_else(|| EngineError::BadParams("no task is chosen".into()));
    };
    let id = v.as_u64().ok_or_else(|| EngineError::BadParams("\"id\" must be a number".into()))?;
    let shown = tasks_folder(s);
    let store = match p.get("store") {
        None | Some(Value::Null) => shown.map(|(st, _)| st),
        Some(v) => Some(v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| EngineError::BadParams("\"store\" must be a number".into()))?),
    };
    let folder = match p.get("folder") {
        None | Some(Value::Null) => shown.filter(|(st, _)| Some(*st) == store).map(|(_, f)| f),
        Some(v) => Some(FolderId(v.as_u64().ok_or_else(|| EngineError::BadParams("\"folder\" must be a number".into()))?)),
    };
    match (store, folder) {
        (Some(store), Some(folder)) if s.folder_info(store, folder).is_some() => Ok((store, folder, id)),
        _ => Err(EngineError::BadParams("no such folder of a data file open".into())),
    }
}

/// The item a row is, loaded, and whether it is one the To-Do List can hold.
pub(super) fn load_row(s: &mut Session, (store, folder, id): (usize, FolderId, u64)) -> Result<Item, EngineError> {
    let item = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.load(folder, ItemId(id))?;
    if !task::is_task(&item) && !task::is_flagged(&item) {
        return Err(EngineError::BadParams("that is neither a task nor a message flagged for follow-up".into()));
    }
    Ok(item)
}

fn select(s: &mut Session, p: &Value) -> CmdResult {
    if p.get("id").is_some_and(Value::is_null) {
        s.tasks.selected = None;
        return ok();
    }
    let key = row_key(s, p)?;
    load_row(s, key)?;
    s.tasks.selected = Some(key);
    ok()
}

fn search(s: &mut Session, p: &Value) -> CmdResult {
    s.tasks.query = query_param(p, "")?;
    s.reload_tasks()?;
    s.status = if s.tasks.query.is_empty() { String::new() } else { format!("{} found", s.tasks.rows.len()) };
    ok()
}

fn date_param(s: &Session, p: &Value, key: &str) -> Result<Option<Option<i64>>, EngineError> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(t)) if t.trim().is_empty() => Ok(Some(None)),
        Some(Value::String(_)) => Ok(Some(super::calendar::date_param(s, p, key)?)),
        Some(_) => Err(EngineError::BadParams(format!("\"{key}\" is a date written YYYY-MM-DD, or null"))),
    }
}

fn reminder_param(s: &Session, p: &Value) -> Result<Option<Option<i64>>, EngineError> {
    match p.get("reminder") {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(t)) if t.trim().is_empty() => Ok(Some(None)),
        Some(Value::String(t)) => calendar::parse_local(t, &s.zone)
            .map(|t| Some(Some(t)))
            .ok_or_else(|| EngineError::BadParams("\"reminder\" is a local time written YYYY-MM-DD HH:MM, or null".into())),
        Some(_) => Err(EngineError::BadParams("\"reminder\" is a local time written YYYY-MM-DD HH:MM, or null".into())),
    }
}

fn minutes_param(p: &Value, key: &str) -> Result<Option<i32>, EngineError> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(0)),
        Some(v) => v
            .as_u64()
            .and_then(|n| i32::try_from(n).ok())
            .filter(|n| *n <= MAX_WORK)
            .map(Some)
            .ok_or_else(|| EngineError::BadParams(format!("\"{key}\" is a number of minutes, at most a hundred years of them"))),
    }
}

/// The task `p` makes of `t`: each field given takes the place of what it was.
fn changed_task(s: &Session, p: &Value, mut t: Task) -> Result<Task, EngineError> {
    if !p.is_object() {
        return Err(EngineError::BadParams("the parameters are an object".into()));
    }
    let fields: [(&str, fn(&mut Task) -> &mut String); 4] =
        [("subject", |t| &mut t.subject), ("owner", |t| &mut t.owner), ("mileage", |t| &mut t.mileage), ("billing", |t| &mut t.billing)];
    for (key, field) in fields {
        if let Some(text) = line_param(p, key)? {
            *field(&mut t) = text;
        }
    }
    if let Some(v) = p.get("notes") {
        t.notes = match v {
            Value::Null => String::new(),
            Value::String(text) => {
                text.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).take(MAX_NOTES).collect::<String>().trim_end().to_string()
            }
            _ => return Err(EngineError::BadParams("\"notes\" must be text".into())),
        };
    }
    if let Some(d) = date_param(s, p, "start")? {
        t.start = d;
    }
    if let Some(d) = date_param(s, p, "due")? {
        t.due = d;
    }
    if let Some(d) = date_param(s, p, "completed")? {
        t.completed = d;
    }
    if let Some(r) = reminder_param(s, p)? {
        t.reminder = r;
    }
    let percent_given = match p.get("percent") {
        None => false,
        Some(v) => {
            t.percent = v
                .as_u64()
                .filter(|n| *n <= 100)
                .and_then(|n| u8::try_from(n).ok())
                .ok_or_else(|| EngineError::BadParams("\"percent\" is a number from 0 to 100".into()))?;
            true
        }
    };
    if let Some(v) = p.get("status") {
        let name = v.as_str().ok_or_else(|| EngineError::BadParams("\"status\" must be text".into()))?;
        t.status = Status::parse(name).ok_or_else(|| {
            EngineError::BadParams("\"status\" is \"notStarted\", \"inProgress\", \"complete\", \"waiting\" or \"deferred\"".into())
        })?;
        // A status of done, or of not done, says how far along it is unless that is given too.
        if !percent_given {
            if t.status == Status::Complete {
                t.percent = 100;
            } else if t.percent == 100 {
                t.percent = 0;
            }
        }
    } else if percent_given && t.percent < 100 && t.status == Status::Complete {
        t.status = Status::InProgress;
    }
    if let Some(v) = p.get("importance") {
        t.importance = match v {
            Value::String(i) if i.eq_ignore_ascii_case("low") => 0,
            Value::String(i) if i.eq_ignore_ascii_case("normal") => 1,
            Value::String(i) if i.eq_ignore_ascii_case("high") => 2,
            Value::Number(n) if n.as_u64().is_some_and(|n| n <= 2) => i32::try_from(n.as_u64().unwrap_or(1)).unwrap_or(1),
            _ => return Err(EngineError::BadParams("\"importance\" is \"low\", \"normal\" or \"high\"".into())),
        };
    }
    if let Some(v) = p.get("categories") {
        t.categories = categories_param(v)?;
    }
    if let Some(v) = p.get("companies") {
        let mut list = categories_param(v).map_err(|_| EngineError::BadParams("\"companies\" is a list of names".into()))?;
        list.truncate(MAX_COMPANIES);
        t.companies = list;
    }
    if let Some(v) = p.get("private") {
        t.private = v.as_bool().ok_or_else(|| EngineError::BadParams("\"private\" is true or false".into()))?;
    }
    if let Some(m) = minutes_param(p, "totalWork")? {
        t.total_work = m;
    }
    if let Some(m) = minutes_param(p, "actualWork")? {
        t.actual_work = m;
    }
    Ok(t)
}

fn new(s: &mut Session, p: &Value) -> CmdResult {
    let t = changed_task(s, p, Task { importance: 1, ..Task::default() })?;
    if t.subject.trim().is_empty() {
        return Err(EngineError::BadParams("a task needs a \"subject\"".into()));
    }
    let pattern = recurrence_param(s, p, &t)?;
    let (store, folder) = writable_folder(s)?;
    let mut item = t.item(s.today(), clock(s), &s.zone);
    if let Some(Some((r, start, due))) = pattern {
        let mut moved = Task::of(&item, &s.zone);
        (moved.start, moved.due) = (start, Some(due));
        moved.write(&mut item, s.today(), clock(s), &s.zone);
        task::set_recurrence(&mut item, Some(&r));
    }
    let id = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.add(folder, item)?.0;
    s.did(Step::Imported { what: "New Task", store, folder, added: vec![id], updated: Vec::new() });
    s.refresh_folders(store);
    // A search or a view that would hide it is let go, so what was made is seen.
    s.tasks.query.clear();
    if !s.tasks.view.shows(&t, s.today()) {
        s.tasks.view = View::SimpleList;
    }
    s.tasks.selected = Some((store, folder, id));
    s.reload_tasks()?;
    s.status = format!("Task made: {}", t.subject);
    Ok(json!({ "id": id, "store": store, "folder": folder.0 }))
}

/// Longest appointment a task is given by the work it is thought to take, in minutes.
const LONGEST_FROM_WORK: i32 = 24 * 60;

/// How long the appointment made from a task lasts: `minutes` when asked for, else the work the
/// task is thought to take when that is from a quarter of an hour to a day, else half an hour.
pub(crate) fn scheduled_minutes(asked: Option<i64>, total_work: i32) -> Result<i64, EngineError> {
    match asked {
        Some(m) if (1..=i64::from(LONGEST_FROM_WORK) * 14).contains(&m) => Ok(m),
        Some(_) => Err(EngineError::BadParams("\"minutes\" must be from 1 to 20160 (two weeks)".into())),
        None if (15..=LONGEST_FROM_WORK).contains(&total_work) => Ok(i64::from(total_work)),
        None => Ok(30),
    }
}

/// A task or a flagged message given a time in the calendar: an appointment with its subject
/// and notes, as Outlook makes one when a task is dragged onto the Calendar. Better than
/// Outlook, it lasts as long as the work the task is thought to take, and the notes say which
/// task and when it is due.
fn schedule(s: &mut Session, p: &Value) -> CmdResult {
    let key = row_key(s, p)?;
    let item = load_row(s, key)?;
    let t = Task::of(&item, &s.zone);
    let start = need_str(p, "start")?.trim().to_string();
    let minutes = match p.get("minutes") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_i64().ok_or_else(|| EngineError::BadParams("\"minutes\" must be a number".into()))?),
    };
    let length = scheduled_minutes(minutes, t.total_work)?;
    let off = s.zone.clone();
    let all_day = !start.contains([' ', 'T']);
    let mut asked = json!({ "subject": t.subject, "start": start });
    if !all_day {
        let at =
            calendar::parse_local(&start, &off).ok_or_else(|| EngineError::BadParams("\"start\" is not a time written YYYY-MM-DD HH:MM".into()))?;
        let end = off.civil(at.saturating_add(length.saturating_mul(calendar::MINUTE)));
        asked["end"] = json!(format!("{:04}-{:02}-{:02} {:02}:{:02}", end.year, end.month, end.day, end.hour, end.minute));
    }
    let mut head = vec![if t.flagged_mail { "Flagged for follow-up".to_string() } else { "From the task".to_string() }];
    if let Some(due) = t.due {
        head.push(format!("due {}", super::calendar::day_text(due)));
    }
    let notes = if t.notes.trim().is_empty() { format!("{}.", head.join(", ")) } else { format!("{}.\n\n{}", head.join(", "), t.notes) };
    asked["notes"] = json!(notes);
    let made = super::calendar::new(s, &asked)?;
    s.status = format!("Scheduled: {}", if t.subject.is_empty() { "(No subject)" } else { &t.subject });
    Ok(json!({ "id": made["id"], "minutes": if all_day { Value::Null } else { json!(length) }, "allDay": all_day }))
}

/// Writes `f`'s change to the row's item, undone as one step, and shows it again wherever it
/// is shown: among the tasks, and in the message list when it is a message there.
pub(super) fn edit_row(
    s: &mut Session,
    key: (usize, FolderId, u64),
    what: &'static str,
    f: impl FnOnce(&Session, &mut Item) -> Result<(), EngineError>,
) -> CmdResult {
    let (store, folder, id) = key;
    if !writable(s, store) {
        return Err(EngineError::BadParams("that data file cannot be written".into()));
    }
    let mut item = load_row(s, key)?;
    let before = Box::new(item.clone());
    f(s, &mut item)?;
    s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.update(folder, ItemId(id), item.clone())?;
    s.did(Step::Edited { what, store, folder, id, before });
    s.refresh_folders(store);
    s.tasks.selected = Some(key);
    s.reload_tasks()?;
    if s.current == Some((store, folder)) {
        if s.selected == Some(id) {
            s.open = Some(item);
        }
        s.reload_rows()?;
    }
    ok()
}

fn change(s: &mut Session, p: &Value) -> CmdResult {
    let key = row_key(s, p)?;
    let before = Task::of(&load_row(s, key)?, &s.zone);
    let after = changed_task(s, p, before.clone())?;
    // A recurring task made complete moves on to its next occurrence, as Mark Complete does;
    // what else was changed goes with the occurrence done.
    if after.is_complete() && !before.is_complete() && before.recurrence.is_some() && p.get("repeat").is_none() {
        let mut rest = p.clone();
        if let Some(o) = rest.as_object_mut() {
            for k in ["status", "percent", "completed"] {
                o.remove(k);
            }
        }
        if rest.as_object().is_some_and(|o| o.keys().any(|k| !matches!(k.as_str(), "id" | "folder" | "store"))) {
            edit_row(s, key, "Edit Task", |s, item| {
                let t = changed_task(s, &rest, Task::of(item, &s.zone))?;
                t.write(item, s.today(), clock(s), &s.zone);
                Ok(())
            })?;
        }
        return complete_row(s, key, "Edit Task");
    }
    edit_row(s, key, "Edit Task", |s, item| {
        let t = changed_task(s, p, Task::of(item, &s.zone))?;
        t.write(item, s.today(), clock(s), &s.zone);
        if let Some(pattern) = recurrence_param(s, p, &Task::of(item, &s.zone))? {
            set_pattern(s, item, pattern);
        }
        Ok(())
    })?;
    // A task given to oneself tells the one who gave it what changed, as they asked.
    super::task_request::changed(s, key, &before)?;
    ok()
}

fn mark_complete(s: &mut Session, p: &Value) -> CmdResult {
    let key = row_key(s, p)?;
    complete_row(s, key, "Mark Complete")
}

/// Marks the row complete. A recurring task's occurrence done is kept as a completed copy
/// beside it and the task moves on to its next occurrence, both taken back as one step.
fn complete_row(s: &mut Session, key: (usize, FolderId, u64), what: &'static str) -> CmdResult {
    let (store, folder, id) = key;
    if !writable(s, store) {
        return Err(EngineError::BadParams("that data file cannot be written".into()));
    }
    let mut item = load_row(s, key)?;
    let before = Box::new(item.clone());
    let Some(next) = task::complete_occurrence(&mut item, s.today(), clock(s), &s.zone) else {
        let was = Task::of(&item, &s.zone);
        edit_row(s, key, what, |s, item| {
            task::mark_complete(item, s.today(), clock(s));
            Ok(())
        })?;
        s.status = "Marked complete".into();
        super::task_request::changed(s, key, &was)?;
        return ok();
    };
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    st.update(folder, ItemId(id), item)?;
    let done = match st.add(folder, next.done) {
        Ok(done) => done.0,
        Err(e) => {
            // Nothing is half done: the task is put back as it was.
            let _ = st.update(folder, ItemId(id), *before);
            return Err(e.into());
        }
    };
    s.did(Step::Imported { what, store, folder, added: vec![done], updated: vec![(id, before)] });
    s.refresh_folders(store);
    s.tasks.selected = Some(key);
    s.reload_tasks()?;
    let due = super::calendar::day_text(next.due);
    s.status = format!("Marked complete; the next one is due {due}");
    Ok(json!({ "done": done, "due": due }))
}

/// The pattern `p` asks a task to recur by, with the days the task then starts and is due on:
/// `None` when "repeat" is not given, `Some(None)` to stop it recurring.
type Pattern = Option<(openmailclient_model::recur::Recurrence, Option<i64>, i64)>;

fn recurrence_param(s: &Session, p: &Value, t: &Task) -> Result<Option<Pattern>, EngineError> {
    let regenerate = match p.get("regenerate") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| EngineError::BadParams("\"regenerate\" is true or false".into()))?),
    };
    if t.flagged_mail && p.get("repeat").is_some_and(|v| !v.is_null()) {
        return Err(EngineError::BadParams("a message flagged for follow-up does not recur; make a task of it".into()));
    }
    let from = t.due.or(t.start).unwrap_or_else(|| s.today());
    let made = match super::calendar::pattern_param(s, p, from, 0, 0)? {
        None if regenerate.is_some() => match t.recurrence.clone() {
            // Only "regenerate" given: the pattern there is kept, made to regenerate or not.
            Some(r) => Some(r),
            None => return Err(EngineError::BadParams("give \"repeat\" too; the task does not recur".into())),
        },
        None => return Ok(None),
        Some(None) => return Ok(Some(None)),
        Some(Some(r)) => Some(r),
    };
    let Some(mut r) = made else { return Ok(Some(None)) };
    if let Some(regenerate) = regenerate {
        r.sliding = u32::from(regenerate);
    }
    // A task that regenerates is due on its own day; one on a pattern on the pattern's first.
    let due = if r.sliding != 0 { from } else { r.start_day };
    if r.sliding != 0 {
        r.start_day = from;
        r.settle();
        r.start_day = from;
    }
    let start = t.start.map(|st| due.saturating_sub(t.due.map_or(0, |d| d.saturating_sub(st).max(0))));
    Ok(Some(Some((r, start, due))))
}

/// Writes a pattern `recurrence_param` gave into the task.
fn set_pattern(s: &Session, item: &mut Item, pattern: Pattern) {
    match pattern {
        Some((r, start, due)) => {
            let mut t = Task::of(item, &s.zone);
            (t.start, t.due) = (start, Some(due));
            t.write(item, s.today(), clock(s), &s.zone);
            task::set_recurrence(item, Some(&r));
        }
        None => task::set_recurrence(item, None),
    }
}

fn skip_occurrence(s: &mut Session, p: &Value) -> CmdResult {
    let key = row_key(s, p)?;
    let mut due = None;
    edit_row(s, key, "Skip Occurrence", |s, item| {
        let next = task::complete_occurrence(item, s.today(), clock(s), &s.zone)
            .ok_or_else(|| EngineError::BadParams("that task does not recur, is done, or this is the last of its series".into()))?;
        due = Some(next.due);
        Ok(())
    })?;
    let due = due.map(super::calendar::day_text);
    s.status = format!("Skipped; the next one is due {}", due.as_deref().unwrap_or(""));
    Ok(json!({ "due": due }))
}

fn recurrence(s: &mut Session, p: &Value) -> CmdResult {
    let key = row_key(s, p)?;
    let item = load_row(s, key)?;
    let t = Task::of(&item, &s.zone);
    if p.get("repeat").is_none() && p.get("regenerate").is_none() {
        return Err(EngineError::BadParams("give \"repeat\", an object or null".into()));
    }
    let Some(pattern) = recurrence_param(s, p, &t)? else {
        return Err(EngineError::BadParams("give \"repeat\", an object or null".into()));
    };
    let words = pattern.as_ref().map(|(r, _, _)| task::recurrence_words(r));
    let due = pattern.as_ref().map(|(_, _, d)| super::calendar::day_text(*d));
    edit_row(s, key, "Recurrence", |s, item| {
        set_pattern(s, item, pattern);
        Ok(())
    })?;
    s.status = match &words {
        Some(w) => format!("Recurs {w}"),
        None => "No longer recurs".into(),
    };
    Ok(json!({ "pattern": words, "due": due }))
}

/// The start, due day and reminder a follow-up asks for: one of the menu's, or days of one's
/// own.
fn when_param(s: &Session, p: &Value) -> Result<(Option<i64>, Option<i64>, Option<Option<i64>>), EngineError> {
    let reminder = reminder_param(s, p)?;
    if let Some(when) = p.get("when").filter(|v| !v.is_null()) {
        let name = when.as_str().ok_or_else(|| EngineError::BadParams("\"when\" must be text".into()))?;
        let f = FollowUp::parse(name)
            .ok_or_else(|| EngineError::BadParams("\"when\" is \"today\", \"tomorrow\", \"thisWeek\", \"nextWeek\" or \"noDate\"".into()))?;
        let (start, due) = f.days(s.today());
        return Ok((start, due, reminder));
    }
    let start = date_param(s, p, "start")?.flatten();
    let due = date_param(s, p, "due")?.flatten();
    if start.is_none() && due.is_none() && p.get("start").is_none() && p.get("due").is_none() {
        return Err(EngineError::BadParams("say \"when\", or give \"start\" and \"due\"".into()));
    }
    Ok((start.or(due), due.or(start), reminder))
}

/// What a follow-up needs of the session while a message is changed: the day, the clock and
/// the zone.
struct Clock {
    today: i64,
    now: i64,
    zone: Zone,
}

impl Clock {
    fn of(s: &Session) -> Self {
        Self { today: s.today(), now: clock(s), zone: s.zone.clone() }
    }

    fn follow_up(&self, item: &mut Item, start: Option<i64>, due: Option<i64>, reminder: Option<Option<i64>>) {
        task::follow_up(item, start, due, self.now, &self.zone);
        if let Some(r) = reminder {
            let mut t = Task::of(item, &self.zone);
            t.reminder = r;
            t.write(item, self.today, self.now, &self.zone);
        }
    }
}

fn follow_up(s: &mut Session, p: &Value) -> CmdResult {
    let key = row_key(s, p)?;
    let (start, due, reminder) = when_param(s, p)?;
    edit_row(s, key, "Follow Up", |s, item| {
        Clock::of(s).follow_up(item, start, due, reminder);
        Ok(())
    })
}

fn mail_follow_up(s: &mut Session, p: &Value) -> CmdResult {
    let (start, due, reminder) = when_param(s, p)?;
    let mut done = Item::default();
    let at = Clock::of(s);
    s.edit_selected("Follow Up", |item| {
        at.follow_up(item, start, due, reminder);
        done = item.clone();
    })?;
    if s.module == Module::Tasks {
        s.reload_tasks()?;
    }
    let t = Task::of(&done, &s.zone);
    s.status = match t.due {
        Some(d) => format!("Flagged for follow-up by {}", super::calendar::day_text(d)),
        None => "Flagged for follow-up".into(),
    };
    ok()
}

fn remove_from_list(s: &mut Session, p: &Value) -> CmdResult {
    let key = row_key(s, p)?;
    edit_row(s, key, "Remove from List", |_, item| {
        if task::is_task(item) {
            return Err(EngineError::BadParams("a task is not taken off the list; it is marked complete or deleted".into()));
        }
        task::clear_flag(item);
        Ok(())
    })
}

fn delete(s: &mut Session, p: &Value) -> CmdResult {
    let key = row_key(s, p)?;
    let (store, folder, id) = key;
    if !writable(s, store) {
        return Err(EngineError::BadParams("that data file cannot be written".into()));
    }
    load_row(s, key)?;
    let deleted = s.find_folder(store, FolderKind::Deleted).filter(|d| *d != folder);
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    match deleted {
        Some(to) => {
            let item = st.load(folder, ItemId(id))?;
            let moved = st.add(to, item)?;
            st.remove(folder, ItemId(id))?;
            s.did(Step::Moved { what: "Delete Task", store, from: folder, was: id, to, id: moved.0 });
        }
        None => {
            let item = Box::new(st.remove(folder, ItemId(id))?);
            s.did(Step::Removed { what: "Delete Task", store, folder, was: id, item });
        }
    }
    // The one deleted stays chosen until the list is read again, which chooses the one that
    // took its place.
    s.refresh_folders(store);
    s.reload_tasks()?;
    if s.current.is_some() {
        s.reload_rows()?;
    }
    ok()
}

fn importance_name(i: i32) -> &'static str {
    match i {
        0 => "low",
        2 => "high",
        _ => "normal",
    }
}
