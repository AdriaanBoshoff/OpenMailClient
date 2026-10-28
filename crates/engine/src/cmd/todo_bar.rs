//! The To-Do Bar: a strip at the right of every module with a date navigator and the
//! appointments to come, the people marked as favourites, and the To-Do List with a box that
//! adds a task to it. Each part is shown or not for each module on its own, as Outlook keeps it
//! (View, To-Do Bar). What it shows is read again only while it is shown: soon after something
//! was done, once a minute, when a data file comes or goes and when the day changes, as the
//! reminders are (`crate::reminders`). `todo.bar` reads it for agents and the CLI, shown or not.

use super::tasks::{Row, View};
use super::{CommandSpec, always, p_bool, p_str};
use crate::{CmdResult, EngineError, Module, Session};
use openmailclient_model::calendar::{self, Appointment};
use serde_json::{Value, json};

/// Days of appointments to come it shows, today the first.
pub const DAYS: i64 = 7;
/// Appointments it shows at most, the soonest first.
const MAX_APPOINTMENTS: usize = 60;
/// Tasks and flagged messages it shows at most, in the To-Do List's order.
const MAX_TASKS: usize = 300;
/// Favourites kept at most.
pub const MAX_FAVORITES: usize = 100;
/// An address longer than this is none (RFC 5321 4.5.3.1 allows 64 + 1 + 255).
const MAX_ADDRESS: usize = 320;
/// What it shows is read again this often while it is shown.
const LOOK_MS: u64 = 60_000;
/// After something was done, it is read again this soon at the least.
const AFTER_MS: u64 = 1_000;

/// The parts of the bar one module shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Parts {
    pub calendar: bool,
    pub people: bool,
    pub tasks: bool,
}

impl Parts {
    pub const ALL: Self = Self { calendar: true, people: true, tasks: true };

    pub fn any(self) -> bool {
        self.calendar || self.people || self.tasks
    }

    pub fn to_json(self) -> Value {
        json!({ "calendar": self.calendar, "people": self.people, "tasks": self.tasks })
    }

    /// The parts an object of booleans names; a key it lacks is off.
    pub fn of_json(v: &Value) -> Option<Self> {
        let o = v.as_object()?;
        if !o.values().all(Value::is_boolean) {
            return None;
        }
        let part = |key: &str| o.get(key).and_then(Value::as_bool).unwrap_or(false);
        Some(Self { calendar: part("calendar"), people: part("people"), tasks: part("tasks") })
    }
}

/// The modules, in the order `Session::todo_bar` keeps their parts.
pub const MODULES: [Module; 4] = [Module::Mail, Module::Calendar, Module::People, Module::Tasks];

pub fn index(m: Module) -> usize {
    MODULES.iter().position(|x| *x == m).unwrap_or(0)
}

/// Someone marked as a favourite: the name their contact has, else the address alone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Favorite {
    pub name: String,
    pub address: String,
    /// Whether a contact of an open data file has the address.
    pub known: bool,
}

/// What the bar shows, and when it was read.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    /// From now through the next days, the soonest first; one that has ended is left out.
    pub appointments: Vec<(u64, Appointment)>,
    pub tasks: Vec<Row>,
    pub people: Vec<Favorite>,
    /// Items of the calendar or the tasks that could not be read.
    pub unreadable: usize,
    /// Tasks and flagged messages left out because it shows as many as it holds.
    pub left_out: usize,
    /// The day it was read on.
    pub day: i64,
    looked: Option<u64>,
    runs: u64,
    stores: usize,
}

pub fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "view.todoBar",
            label: "To-Do Bar",
            tab: "View",
            group: "Layout",
            shortcut: "",
            params: r#"{"part": "calendar" | "people" | "tasks" | "off", "show": true or false (default: the other way), "module": "mail" | "calendar" | "people" | "tasks" (default the one shown)}; each module keeps its own; returns the parts it shows"#,
            enabled: always,
            run: set_parts,
        },
        CommandSpec {
            id: "todo.bar",
            label: "To-Do Bar Items",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{}; returns {"shown": the parts the module shown shows, "appointments" of today and the next days not ended yet, "tasks" of the To-Do List with their "heading", "people" marked as favourites, "unreadable", "leftOut"}, all parts whether shown or not"#,
            enabled: always,
            run: items,
        },
        CommandSpec {
            id: "people.favorite",
            label: "Add to Favorites",
            tab: "Home",
            group: "Favorites",
            shortcut: "",
            params: r#"{"address": an e-mail address, "on": true to add it (default) or false to take it off}; the favourites stand in the To-Do Bar's People part"#,
            enabled: always,
            run: favorite,
        },
    ]
}

impl Session {
    /// The parts of the To-Do Bar the module shown shows.
    pub fn todo_bar_parts(&self) -> Parts {
        self.todo_bar.get(index(self.module)).copied().unwrap_or_default()
    }

    /// Read again what the To-Do Bar shows when it is time to, and say in how many
    /// milliseconds to ask again; `None` while it is not shown. `now_ms` is the host's clock,
    /// which only goes forward.
    pub fn todo_bar_due(&mut self, now_ms: u64) -> Option<u64> {
        let parts = self.todo_bar_parts();
        if !parts.any() {
            return None;
        }
        let t = &self.todo;
        let since = t.looked.map(|at| now_ms.saturating_sub(at));
        let done = t.runs != self.auto.runs;
        let stale = since.is_none_or(|ms| ms >= LOOK_MS) || t.stores != self.stores.len() || t.day != self.today();
        if stale || (done && since.is_none_or(|ms| ms >= AFTER_MS)) {
            self.todo = self.todo_bar_read(Parts::ALL);
            self.todo.looked = Some(now_ms);
            return Some(LOOK_MS);
        }
        Some(if done { AFTER_MS.saturating_sub(since.unwrap_or(0)).max(50) } else { LOOK_MS.saturating_sub(since.unwrap_or(0)).max(50) })
    }

    /// Have what the To-Do Bar shows read again the next time it is asked.
    pub fn todo_bar_stale(&mut self) {
        self.todo.looked = None;
    }

    /// What the To-Do Bar's `parts` show, read now; the parts left out are empty.
    pub(crate) fn todo_bar_read(&mut self, parts: Parts) -> Shown {
        let mut out = Shown { day: self.today(), runs: self.auto.runs, stores: self.stores.len(), ..Shown::default() };
        if parts.calendar {
            let now = self.time_now();
            let from = calendar::day_start(out.day, &self.zone);
            let to = calendar::day_start(out.day.saturating_add(DAYS), &self.zone);
            match self.appointments_between(from, to) {
                Ok((mut found, unreadable)) => {
                    found.retain(|(_, a)| a.end > now || (a.end <= a.start && a.start >= from));
                    found.sort_by_key(|(_, a)| (a.start, !a.all_day));
                    found.truncate(MAX_APPOINTMENTS);
                    out.appointments = found;
                    out.unreadable = out.unreadable.saturating_add(unreadable);
                }
                Err(_) => out.unreadable = out.unreadable.saturating_add(1),
            }
        }
        if parts.tasks {
            match self.task_rows(View::ToDoList, "") {
                Ok((mut rows, _, unreadable, left_out)) => {
                    let over = rows.len().saturating_sub(MAX_TASKS);
                    rows.truncate(MAX_TASKS);
                    out.tasks = rows;
                    out.unreadable = out.unreadable.saturating_add(unreadable);
                    out.left_out = left_out.saturating_add(over);
                }
                Err(_) => out.unreadable = out.unreadable.saturating_add(1),
            }
        }
        if parts.people && !self.favorites.is_empty() {
            let entries = self.address_entries(None);
            out.people = self
                .favorites
                .iter()
                .map(|address| match entries.iter().find(|e| e.address.eq_ignore_ascii_case(address)) {
                    Some(e) if !e.name.trim().is_empty() => Favorite { name: e.name.clone(), address: address.clone(), known: true },
                    found => Favorite { name: address.clone(), address: address.clone(), known: found.is_some() },
                })
                .collect();
        }
        out
    }
}

fn module_param(s: &Session, p: &Value) -> Result<Module, EngineError> {
    match p.get("module") {
        None | Some(Value::Null) => Ok(s.module),
        Some(Value::String(m)) => Module::parse(m).ok_or_else(|| EngineError::BadParams("that is not a module".into())),
        Some(_) => Err(EngineError::BadParams("\"module\" must be text".into())),
    }
}

fn set_parts(s: &mut Session, p: &Value) -> CmdResult {
    let module = module_param(s, p)?;
    let part = p_str(p, "part").ok_or_else(|| EngineError::BadParams("\"part\" must be calendar, people, tasks or off".into()))?;
    let show = match p.get("show") {
        None | Some(Value::Null) => None,
        Some(_) => Some(p_bool(p, "show").ok_or_else(|| EngineError::BadParams("\"show\" must be true or false".into()))?),
    };
    let slot = s.todo_bar.get_mut(index(module)).ok_or_else(|| EngineError::Internal("view.todoBar".into()))?;
    match part.to_ascii_lowercase().as_str() {
        "calendar" => slot.calendar = show.unwrap_or(!slot.calendar),
        "people" => slot.people = show.unwrap_or(!slot.people),
        "tasks" => slot.tasks = show.unwrap_or(!slot.tasks),
        "off" => *slot = Parts::default(),
        _ => return Err(EngineError::BadParams("\"part\" must be calendar, people, tasks or off".into())),
    }
    let now = *slot;
    s.todo_bar_stale();
    let mut out = now.to_json();
    out["module"] = json!(module.name());
    Ok(out)
}

fn items(s: &mut Session, _: &Value) -> CmdResult {
    let read = s.todo_bar_read(Parts::ALL);
    let today = s.today();
    let appointments: Vec<Value> = read
        .appointments
        .iter()
        .map(|(id, a)| {
            let mut v = super::calendar::appointment_json(s, *id, a);
            v["day"] = json!(super::calendar::day_text(calendar::day_of(a.start, &s.zone)));
            v
        })
        .collect();
    let tasks: Vec<Value> = read
        .tasks
        .iter()
        .map(|r| {
            let mut v = super::tasks::row_json(s, r);
            v["heading"] = json!(heading(r.task.due, today, s.region.week_start));
            v
        })
        .collect();
    let people: Vec<Value> = read.people.iter().map(|f| json!({ "name": f.name, "address": f.address, "known": f.known })).collect();
    Ok(json!({
        "shown": s.todo_bar_parts().to_json(),
        "appointments": appointments,
        "tasks": tasks,
        // Names are as a contact has them, and a contact may have come from anyone.
        "untrusted_content": { "people": people },
        "unreadable": read.unreadable,
        "leftOut": read.left_out,
    }))
}

/// The heading the To-Do List puts a task under, by the day it is due: Overdue, Today,
/// Tomorrow, This Week, Next Week, Later or No Date. Weeks begin on `week_start` (0 Sunday), as
/// the system's region says; Outlook's own weeks begin on Sunday.
pub fn heading(due: Option<i64>, today: i64, week_start: u32) -> &'static str {
    let Some(due) = due else { return "No Date" };
    // Day 0 (1970-01-01) was a Thursday, so day `d` is weekday (d + 4) % 7 counted from Sunday.
    let weekday = today.saturating_add(4).rem_euclid(7);
    let into_week = (weekday - i64::from(week_start % 7)).rem_euclid(7);
    let week_end = today.saturating_add(7 - into_week);
    match due {
        d if d < today => "Overdue",
        d if d == today => "Today",
        d if d == today.saturating_add(1) => "Tomorrow",
        d if d < week_end => "This Week",
        d if d < week_end.saturating_add(7) => "Next Week",
        _ => "Later",
    }
}

/// An address as a favourite is kept: trimmed and in lower case, or `None` if it is none.
pub fn favorite_address(text: &str) -> Option<String> {
    let a = text.trim();
    let (local, domain) = a.split_once('@')?;
    let bad = |c: char| c.is_control() || c.is_whitespace() || "<>,;\"".contains(c);
    if a.len() > MAX_ADDRESS || local.is_empty() || domain.is_empty() || domain.contains('@') || a.chars().any(bad) {
        return None;
    }
    Some(a.to_lowercase())
}

fn favorite(s: &mut Session, p: &Value) -> CmdResult {
    let text = p_str(p, "address").ok_or_else(|| EngineError::BadParams("\"address\" must be text".into()))?;
    let address = favorite_address(text).ok_or_else(|| EngineError::BadParams("that is not an e-mail address".into()))?;
    let on = match p.get("on") {
        None | Some(Value::Null) => true,
        Some(_) => p_bool(p, "on").ok_or_else(|| EngineError::BadParams("\"on\" must be true or false".into()))?,
    };
    let had = s.favorites.contains(&address);
    if on && !had {
        if s.favorites.len() >= MAX_FAVORITES {
            return Err(EngineError::BadParams(format!("there are {MAX_FAVORITES} favourites already; take one off first")));
        }
        s.favorites.push(address);
    } else if !on {
        s.favorites.retain(|f| *f != address);
    }
    s.todo_bar_stale();
    s.status = if on { "Added to Favorites." } else { "Removed from Favorites." }.to_string();
    Ok(json!({ "favorite": on, "favorites": s.favorites.len() }))
}
