//! The Calendar module: which days are shown, and appointments made, changed and deleted.
//! Appointments are items of a Calendar folder ([MS-OXOCAL]); see `openmailclient_model::calendar`.
//! A recurring appointment is shown once on each day it falls on; one of those occurrences
//! is changed or deleted on its own by naming the day it was first planned on.

use super::{CommandSpec, always, need_str, ok, p_bool, p_str, p_u64};
use crate::{CmdResult, EngineError, Session, undo::Step};
use openmailclient_model::calendar::{self, Appointment, Busy, View};
use openmailclient_model::recur::{self, End, Freq, Recurrence, Rule};
use openmailclient_model::zone::Zone;
use openmailclient_model::{FolderId, FolderKind, ItemId, StoreError};
use serde_json::{Value, json};

/// Items of a Calendar folder read for one view at most.
const MAX_ITEMS: usize = 20_000;
/// Longest subject or location taken, in characters.
const MAX_TEXT: usize = 2_000;

/// What the Calendar module shows.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    pub view: View,
    /// The day chosen, as a local day number (days since 1970-01-01); 0 until the module is
    /// first shown, when it becomes today.
    pub day: i64,
    /// The appointments of the days shown, by item id, earliest first.
    pub items: Vec<(u64, Appointment)>,
    /// Where they are: the store and its Calendar folder.
    pub folder: Option<(usize, FolderId)>,
    /// The appointment chosen.
    pub selected: Option<u64>,
    /// When it is one occurrence of a series: the local day it was first planned on.
    pub occurrence: Option<i64>,
    /// Items of the folder that could not be read.
    pub unreadable: usize,
    /// The calendar chosen to be shown (`calendar.open`); while it is there, it is the one
    /// shown and written to.
    pub chosen: Option<(usize, FolderId)>,
}

const VIEW_PARAMS: &str = r#"{"view": "day" | "workWeek" | "week" | "month"}"#;
const APPOINTMENT_PARAMS: &str = r#"{"subject", "start": "YYYY-MM-DD HH:MM" local time (a date alone for an all-day event), "end" (default: half an hour on, or the next day for an all-day event), "location", "allDay": bool, "busy": "free" | "tentative" | "busy" | "outOfOffice" | "workingElsewhere", "reminder": minutes before, or null for none (default 15; none for an all-day event), "repeat": as calendar.recurrence takes it, "zone": the time zone it is kept in and its times are given in, a name calendar.timeZones gives, or null for none (default: the one dates are shown in); a series in a zone keeps its hour there whatever the season, "attendees": those asked, which makes it a meeting one organizes (see calendar.newMeeting)}"#;
const REPEAT_PARAMS: &str = r#"{"id" (default: the one chosen), "repeat": null or "none" for once, or {"every": "day" | "weekday" | "week" | "month" | "year", "interval": days, weeks or months between (default 1), "on": weekday names (a week's days, or with "nth" the days of a month counted), "nth": 1 to 4 or "last", "day": the day of the month (default: the start's), "lastDay": true for the month's last day, "count": occurrences 1 to 999, or "until": "YYYY-MM-DD" (default: no end)}}; the first occurrence is the first day it falls on from the start, and occurrences changed on their own are as planned again"#;

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "calendar.view",
            label: "Day, Work Week, Week, Month",
            tab: "Home",
            group: "Arrange",
            shortcut: "Ctrl+Alt+1…4",
            params: VIEW_PARAMS,
            enabled: always,
            run: view,
        },
        CommandSpec {
            id: "calendar.calendars",
            label: "My Calendars",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{}; returns {"calendars": [{"store", "file", "folder", "name", "shown", "account": the account whose CalDAV server it is kept in step with, or null, "color", "readOnly"}]}"#,
            enabled: always,
            run: list_calendars,
        },
        CommandSpec {
            id: "calendar.open",
            label: "Open Calendar",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"store", "folder"}, as calendar.calendars gives them; that calendar is shown, and new appointments go in it"#,
            enabled: has_calendar,
            run: open,
        },
        CommandSpec { id: "calendar.today", label: "Today", tab: "Home", group: "Go To", shortcut: "", params: "", enabled: always, run: today },
        CommandSpec {
            id: "calendar.goTo",
            label: "Go To Date",
            tab: "Home",
            group: "Go To",
            shortcut: "Ctrl+G",
            params: r#"{"date": "YYYY-MM-DD"}"#,
            enabled: always,
            run: go_to,
        },
        CommandSpec { id: "calendar.next", label: "Forward", tab: "", group: "", shortcut: "Alt+Down", params: "", enabled: always, run: next },
        CommandSpec { id: "calendar.previous", label: "Back", tab: "", group: "", shortcut: "Alt+Up", params: "", enabled: always, run: previous },
        CommandSpec {
            id: "calendar.items",
            label: "Appointments",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"from": "YYYY-MM-DD", "to": "YYYY-MM-DD" (the day after the last; default: the days shown)}; returns {"days", "items": [{"id", "start", "end", "allDay", "busy", "reminder", "recurring", "meeting", "occurrence": the day an occurrence of a series was first planned on, "changed": an occurrence changed on its own, "pattern", "untrusted_content": {"subject", "location"}}], "unreadable"}; a series is one item for each occurrence"#,
            enabled: has_calendar,
            run: items,
        },
        CommandSpec {
            id: "calendar.select",
            label: "Select Appointment",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id": an appointment's id, or null for none, "occurrence": "YYYY-MM-DD" for one occurrence of a series}"#,
            enabled: has_calendar,
            run: select,
        },
        CommandSpec {
            id: "calendar.new",
            label: "New Appointment",
            tab: "Home",
            group: "New",
            shortcut: "Ctrl+Shift+A",
            params: APPOINTMENT_PARAMS,
            enabled: has_calendar_writable,
            run: new,
        },
        CommandSpec {
            id: "calendar.change",
            label: "Change Appointment",
            tab: "",
            group: "",
            shortcut: "",
            params: CHANGE_PARAMS,
            enabled: has_calendar_writable,
            run: change,
        },
        CommandSpec {
            id: "calendar.recurrence",
            label: "Recurrence",
            tab: "Appointment",
            group: "Options",
            shortcut: "",
            params: REPEAT_PARAMS,
            enabled: has_calendar_writable,
            run: recurrence,
        },
        CommandSpec {
            id: "calendar.timeZones",
            label: "Time Zones",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{}; returns {"zones": [{"id": its tz database name, "key": Outlook's name for it, "label", "offsetNow": minutes east of UTC now, "daylight": whether it keeps summer time}], "current", "system", "chosen": the one chosen in place of the system's, or null, "second"}"#,
            enabled: always,
            run: time_zones,
        },
        CommandSpec {
            id: "calendar.timeZone",
            label: "Time Zone",
            tab: "File",
            group: "Options",
            shortcut: "",
            params: r#"{"zone": a name calendar.timeZones gives, or null to follow the system's}; dates everywhere are shown and read in it"#,
            enabled: always,
            run: time_zone,
        },
        CommandSpec {
            id: "calendar.secondTimeZone",
            label: "Show a Second Time Zone",
            tab: "View",
            group: "Arrangement",
            shortcut: "",
            params: r#"{"zone": a name calendar.timeZones gives, or null for none}; the day and week views show its hours beside their own"#,
            enabled: always,
            run: second_time_zone,
        },
        CommandSpec {
            id: "calendar.delete",
            label: "Delete Appointment",
            tab: "Home",
            group: "Actions",
            shortcut: "Ctrl+D",
            params: DELETE_PARAMS,
            enabled: has_appointment,
            run: delete,
        },
    ]
}

/// The store and Calendar folder appointments are kept in: the one chosen, else the current
/// store's, or else the first store's that has one.
pub(crate) fn calendar_folder(s: &Session) -> Option<(usize, FolderId)> {
    if let Some((store, folder)) = s.calendar.chosen
        && is_calendar(s, store, folder)
    {
        return Some((store, folder));
    }
    let current = s.current.map(|(store, _)| store);
    current.into_iter().chain(0..s.stores.len()).find_map(|store| s.find_folder(store, FolderKind::Calendar).map(|f| (store, f)))
}

fn is_calendar(s: &Session, store: usize, folder: FolderId) -> bool {
    s.folders.get(store).is_some_and(|f| f.iter().any(|x| x.id == folder && x.kind == FolderKind::Calendar))
}

/// Calendar folders listed at most.
const MAX_CALENDARS: usize = 500;

/// Every calendar of the data files open, as the folder pane lists them under My Calendars.
pub fn calendars(s: &Session) -> Vec<Value> {
    let shown = calendar_folder(s);
    let mut out = Vec::new();
    // Paths are only looked at when some calendar is kept in step with a server: the window
    // asks for this list as it is drawn.
    let davs: Vec<(&crate::accounts::Account, std::path::PathBuf)> = s
        .accounts
        .iter()
        .filter(|a| a.caldav.is_some())
        .filter_map(|a| Some((a, std::fs::canonicalize(super::account::account_path(s, a)?).ok()?)))
        .collect();
    for (store, folders) in s.folders.iter().enumerate() {
        let file = s.stores.get(store).map(|st| st.display_name()).unwrap_or_default();
        let has_calendar = folders.iter().any(|f| f.kind == FolderKind::Calendar);
        let path = if davs.is_empty() || !has_calendar {
            None
        } else {
            s.stores.get(store).and_then(|st| st.path()).and_then(|p| std::fs::canonicalize(p).ok())
        };
        let owner = path.and_then(|path| davs.iter().find(|(_, p)| *p == path).map(|(a, _)| *a));
        for f in folders.iter().filter(|f| f.kind == FolderKind::Calendar).take(MAX_CALENDARS.saturating_sub(out.len())) {
            // Kept in step with a server: whose, and what it lets be changed.
            let dav = owner.and_then(|a| a.caldav.as_ref().and_then(|d| d.calendars.iter().find(|c| c.here == f.id.0)).map(|c| (a, c)));
            out.push(json!({
                "store": store,
                "file": file,
                "folder": f.id.0,
                "name": f.name,
                "shown": shown == Some((store, f.id)),
                "account": dav.map(|(a, _)| a.id.clone()),
                "color": dav.map(|(_, c)| c.color.clone()).filter(|c| !c.is_empty()),
                "readOnly": dav.is_some_and(|(_, c)| c.read_only),
            }));
        }
    }
    out
}

fn list_calendars(s: &mut Session, _: &Value) -> CmdResult {
    Ok(json!({ "calendars": calendars(s) }))
}

fn open(s: &mut Session, p: &Value) -> CmdResult {
    let store = p_u64(p, "store")
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| EngineError::BadParams("\"store\" is a data file's number, as calendar.calendars gives it".into()))?;
    let folder = FolderId(
        p_u64(p, "folder").ok_or_else(|| EngineError::BadParams("\"folder\" is a Calendar folder's id, as calendar.calendars gives it".into()))?,
    );
    if !is_calendar(s, store, folder) {
        return Err(EngineError::BadParams("that is not a calendar of a data file open".into()));
    }
    s.calendar.chosen = Some((store, folder));
    s.calendar.selected = None;
    s.calendar.occurrence = None;
    s.reload_calendar()?;
    s.status =
        format!("Showing {}", s.folders.get(store).and_then(|f| f.iter().find(|x| x.id == folder)).map(|f| f.name.clone()).unwrap_or_default());
    ok()
}

const CHANGE_PARAMS: &str = r#"{"id" (default: the one chosen), any of those calendar.new takes, and "occurrence": "YYYY-MM-DD", the day one occurrence of a series was first planned on, to change that one alone (default: the one chosen, when "id" is not given), or "series": true; an occurrence keeps its place among the others, and a series whose time changes has its occurrences as planned again"}"#;
const DELETE_PARAMS: &str = r#"{"id" (default: the one chosen), "occurrence": "YYYY-MM-DD" to delete one occurrence of a series alone (default: the one chosen, when "id" is not given), or "series": true}; a whole appointment goes to Deleted Items"#;

fn has_calendar(s: &Session) -> bool {
    calendar_folder(s).is_some()
}

fn has_appointment(s: &Session) -> bool {
    s.calendar.selected.is_some() && has_calendar_writable(s)
}

fn has_calendar_writable(s: &Session) -> bool {
    calendar_folder(s).is_some_and(|(store, _)| s.stores.get(store).is_some_and(|st| st.writable()))
}

impl Session {
    /// The time now, as the store keeps times.
    pub fn time_now(&self) -> i64 {
        super::clock(self)
    }

    /// The local day number of today.
    pub fn today(&self) -> i64 {
        calendar::day_of(super::clock(self), &self.zone)
    }

    /// The local day numbers the calendar shows.
    pub fn calendar_days(&self) -> Vec<i64> {
        let day = if self.calendar.day == 0 { self.today() } else { self.calendar.day };
        self.calendar.view.days(day, self.region.week_start)
    }

    /// Read again the appointments of the days shown.
    pub fn reload_calendar(&mut self) -> Result<(), EngineError> {
        if self.calendar.day == 0 {
            self.calendar.day = self.today();
        }
        let days = self.calendar_days();
        let (Some(first), Some(last)) = (days.first(), days.last()) else { return Ok(()) };
        let from = calendar::day_start(*first, &self.zone);
        let to = calendar::day_start(last.saturating_add(1), &self.zone);
        let (items, unreadable) = self.appointments_between(from, to)?;
        self.calendar.folder = calendar_folder(self);
        let (chosen, occurrence) = (self.calendar.selected, self.calendar.occurrence);
        if chosen.is_some_and(|id| !items.iter().any(|(i, a)| *i == id && a.occurrence == occurrence)) {
            self.calendar.selected = None;
            self.calendar.occurrence = None;
        }
        self.calendar.items = items;
        self.calendar.unreadable = unreadable;
        Ok(())
    }

    /// The pattern `repeat` (as `calendar.recurrence` takes it) makes for an appointment that
    /// starts as `a` does, not saved: what the appointment window shows before it is.
    pub fn repeat_preview(&self, repeat: &Value, a: &Appointment) -> Result<Option<Recurrence>, EngineError> {
        let zone = if a.recurrence.is_some() { a.series_zone().into_owned() } else { a.zone.clone().unwrap_or_else(|| self.zone.clone()) };
        Ok(repeat_param(self, &json!({ "repeat": repeat }), a, &zone)?.flatten())
    }

    /// The appointment `id` of the calendar as it is kept: a series as a whole.
    pub fn appointment(&mut self, id: u64) -> Result<Appointment, EngineError> {
        let (store, folder) = calendar_folder(self).ok_or(StoreError::NoFolder)?;
        let item = self.stores.get_mut(store).ok_or(StoreError::NoFolder)?.load(folder, ItemId(id))?;
        Appointment::of(&item).ok_or_else(|| EngineError::BadParams("that item is not an appointment".into()))
    }

    /// The appointments any of which falls in `[from, to)`, earliest first, each occurrence of
    /// a series apart, and how many items could not be read.
    pub fn appointments_between(&mut self, from: i64, to: i64) -> Result<(Vec<(u64, Appointment)>, usize), EngineError> {
        let Some((store, folder)) = calendar_folder(self) else { return Ok((Vec::new(), 0)) };
        let st = self.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
        let mut found = Vec::new();
        let mut unreadable = 0usize;
        for row in st.list(folder)?.into_iter().take(MAX_ITEMS) {
            match st.load(folder, ItemId(row.id)) {
                Ok(item) => match Appointment::of(&item) {
                    Some(a) => {
                        let room = MAX_ITEMS.saturating_sub(found.len());
                        found.extend(a.occurrences(from, to).into_iter().take(room).map(|o| (row.id, o)));
                    }
                    None => unreadable = unreadable.saturating_add(1),
                },
                Err(_) => unreadable = unreadable.saturating_add(1),
            }
        }
        found.sort_by_key(|(id, a)| (a.start, std::cmp::Reverse(a.end), *id));
        Ok((found, unreadable))
    }
}

fn view(s: &mut Session, p: &Value) -> CmdResult {
    let name = need_str(p, "view")?;
    s.calendar.view = View::parse(name).ok_or_else(|| EngineError::BadParams(format!("\"{name}\" is not day, workWeek, week or month")))?;
    s.reload_calendar()?;
    ok()
}

fn today(s: &mut Session, _: &Value) -> CmdResult {
    s.calendar.day = s.today();
    s.reload_calendar()?;
    ok()
}

pub(super) fn date_param(s: &Session, p: &Value, key: &str) -> Result<Option<i64>, EngineError> {
    match p_str(p, key) {
        None => Ok(None),
        Some(text) => calendar::parse_local(text, &s.zone)
            .map(|t| Some(calendar::day_of(t, &s.zone)))
            .ok_or_else(|| EngineError::BadParams(format!("\"{key}\" is not a date written YYYY-MM-DD"))),
    }
}

fn go_to(s: &mut Session, p: &Value) -> CmdResult {
    let day = date_param(s, p, "date")?.ok_or_else(|| EngineError::BadParams("\"date\" (YYYY-MM-DD) is required".into()))?;
    s.calendar.day = day;
    s.reload_calendar()?;
    ok()
}

fn step(s: &mut Session, by: i64) -> CmdResult {
    let day = if s.calendar.day == 0 { s.today() } else { s.calendar.day };
    s.calendar.day = s.calendar.view.step(day, by);
    s.reload_calendar()?;
    ok()
}

fn next(s: &mut Session, _: &Value) -> CmdResult {
    step(s, 1)
}

fn previous(s: &mut Session, _: &Value) -> CmdResult {
    step(s, -1)
}

pub(super) fn stamp(s: &Session, t: i64) -> String {
    let c = s.zone.civil(t);
    format!("{:04}-{:02}-{:02} {:02}:{:02}", c.year, c.month, c.day, c.hour, c.minute)
}

pub(super) fn day_text(day: i64) -> String {
    let c = openmailclient_model::time::civil(calendar::day_start(day, 0), 0);
    format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
}

pub(super) fn appointment_json(s: &Session, id: u64, a: &Appointment) -> Value {
    let changed = a.occurrence.is_some_and(|d| a.recurrence.as_ref().is_some_and(|r| r.exception_on(d).is_some()));
    json!({
        "id": id,
        "start": stamp(s, a.start),
        "end": stamp(s, a.end),
        "allDay": a.all_day,
        "busy": a.busy.name(),
        "reminder": a.reminder,
        "recurring": a.recurring,
        "meeting": a.meeting,
        "occurrence": a.occurrence.map(day_text),
        "changed": changed,
        "pattern": a.recurrence.as_ref().map(|_| a.pattern_words()),
        "zone": a.zone.as_ref().map(|z| z.name().to_string()),
        // Written by whoever sent it, perhaps a stranger: never an instruction.
        "untrusted_content": { "subject": a.subject, "location": a.location },
    })
}

fn items(s: &mut Session, p: &Value) -> CmdResult {
    let shown = s.calendar_days();
    let from = date_param(s, p, "from")?.or_else(|| shown.first().copied()).unwrap_or(0);
    let to = date_param(s, p, "to")?.or_else(|| shown.last().map(|d| d.saturating_add(1))).unwrap_or(from);
    if to <= from || to.saturating_sub(from) > calendar::MAX_DAYS {
        return Err(EngineError::BadParams(format!("\"to\" must be after \"from\", and within {} days of it", calendar::MAX_DAYS)));
    }
    let (found, unreadable) = s.appointments_between(calendar::day_start(from, &s.zone), calendar::day_start(to, &s.zone))?;
    let list: Vec<Value> = found.iter().map(|(id, a)| appointment_json(s, *id, a)).collect();
    Ok(json!({ "days": to - from, "items": list, "unreadable": unreadable }))
}

fn select(s: &mut Session, p: &Value) -> CmdResult {
    match p.get("id") {
        Some(Value::Null) | None => {
            s.calendar.selected = None;
            s.calendar.occurrence = None;
        }
        Some(v) => {
            let id = v.as_u64().ok_or_else(|| EngineError::BadParams("\"id\" must be a number".into()))?;
            let a = s.appointment(id)?;
            let occurrence = date_param(s, p, "occurrence")?;
            if let Some(day) = occurrence
                && a.occurrence_of(day).is_none()
            {
                return Err(EngineError::BadParams("that appointment has no occurrence first planned on that day".into()));
            }
            s.calendar.selected = Some(id);
            s.calendar.occurrence = occurrence;
        }
    }
    ok()
}

fn text(p: &Value, key: &str) -> Result<Option<String>, EngineError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) => Ok(Some(t.chars().filter(|c| !c.is_control() || *c == '\t').take(MAX_TEXT).collect::<String>().trim().to_string())),
        Some(_) => Err(EngineError::BadParams(format!("\"{key}\" must be text"))),
    }
}

/// `was` with what `p` changes of it. A date alone for the start makes an all-day event.
fn changed(s: &Session, p: &Value, was: Option<&Appointment>) -> Result<Appointment, EngineError> {
    let mut a = match was {
        Some(a) => a.clone(),
        None => Appointment {
            subject: String::new(),
            location: String::new(),
            start: 0,
            end: 0,
            all_day: false,
            busy: Busy::Busy,
            reminder: Some(15),
            recurring: false,
            meeting: false,
            recurrence: None,
            occurrence: None,
            // Made where this session is, when that is a zone known by name.
            zone: (!s.zone.is_fixed() || !s.zone.key.is_empty()).then(|| s.zone.clone()),
        },
    };
    // The zone it is in, and the one its times are given in: that one, or this session's.
    let off = match p.get("zone") {
        None => s.zone.clone(),
        Some(v) => {
            a.zone = zone_param(v)?;
            a.zone.clone().unwrap_or_else(|| s.zone.clone())
        }
    };
    let off = &off;
    if let Some(subject) = text(p, "subject")? {
        a.subject = subject;
    }
    if let Some(location) = text(p, "location")? {
        a.location = location;
    }
    let all_day_asked = p_bool(p, "allDay");
    let start_text = p_str(p, "start");
    if let Some(t) = start_text {
        let start = calendar::parse_local(t, off).ok_or_else(|| EngineError::BadParams("\"start\" is not a time written YYYY-MM-DD HH:MM".into()))?;
        let length = a.end.saturating_sub(a.start);
        a.start = start;
        a.end = start.saturating_add(length);
    } else if was.is_none() {
        return Err(EngineError::BadParams("\"start\" (YYYY-MM-DD HH:MM) is required".into()));
    }
    let date_only = start_text.is_some_and(|t| !t.trim().contains([' ', 'T']));
    a.all_day = all_day_asked.unwrap_or(if start_text.is_some() { date_only || (a.all_day && was.is_some() && date_only) } else { a.all_day });
    if a.all_day {
        // An all-day event runs from midnight to midnight where it is made.
        let first = calendar::day_of(a.start, off);
        let last = calendar::day_of(a.end.saturating_sub(1).max(a.start), off).max(first);
        a.start = calendar::day_start(first, off);
        a.end = calendar::day_start(last.saturating_add(1), off);
        if was.is_none() && p.get("reminder").is_none() {
            a.reminder = None;
        }
        if was.is_none() && p.get("busy").is_none() {
            a.busy = Busy::Free;
        }
    }
    match p_str(p, "end") {
        Some(t) => {
            let end = calendar::parse_local(t, off).ok_or_else(|| EngineError::BadParams("\"end\" is not a time written YYYY-MM-DD HH:MM".into()))?;
            if end < a.start {
                return Err(EngineError::BadParams("an appointment cannot end before it starts".into()));
            }
            a.end = if a.all_day && !t.trim().contains([' ', 'T']) {
                // The last day given is the last day of the event.
                calendar::day_start(calendar::day_of(end, off).saturating_add(1), off)
            } else {
                end
            };
            if a.all_day && a.end <= a.start {
                a.end = a.start.saturating_add(calendar::DAY);
            }
        }
        None if was.is_none() || a.end <= a.start && !a.all_day => {
            a.end = a.start.saturating_add(if a.all_day { calendar::DAY } else { 30 * calendar::MINUTE });
        }
        None => {}
    }
    if a.end.saturating_sub(a.start) > calendar::MAX_DAYS.saturating_mul(calendar::DAY) {
        return Err(EngineError::BadParams(format!("an appointment lasts at most {} days", calendar::MAX_DAYS)));
    }
    if let Some(name) = p_str(p, "busy") {
        a.busy = Busy::parse(name)
            .ok_or_else(|| EngineError::BadParams(format!("\"{name}\" is not free, tentative, busy, outOfOffice or workingElsewhere")))?;
    }
    match p.get("reminder") {
        None => {}
        Some(Value::Null) => a.reminder = None,
        Some(v) => {
            let minutes = v
                .as_i64()
                .filter(|m| (0..=60 * 24 * 7 * 52).contains(m))
                .ok_or_else(|| EngineError::BadParams("\"reminder\" is minutes before the start, up to a year, or null".into()))?;
            a.reminder = i32::try_from(minutes).ok();
        }
    }
    Ok(a)
}

pub(super) fn writable_calendar(s: &Session) -> Result<(usize, FolderId), EngineError> {
    let (store, folder) = calendar_folder(s).ok_or_else(|| EngineError::BadParams("no data file open has a Calendar folder".into()))?;
    if !s.stores.get(store).is_some_and(|st| st.writable()) {
        return Err(EngineError::BadParams("the data file with the calendar cannot be written".into()));
    }
    Ok((store, folder))
}

pub(super) fn new(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = writable_calendar(s)?;
    let mut a = changed(s, p, None)?;
    let zone = a.zone.clone().unwrap_or_else(|| s.zone.clone());
    if let Some(pattern) = repeat_param(s, p, &a, &zone)? {
        set_pattern(&mut a, pattern, &zone);
    }
    // Its id for life, by which a copy of it (a calendar file, a server's) is known again.
    let mut item = a.item();
    if let Some(v) = p.get("attendees") {
        let asked = super::meeting::attendees_param(s, v)?;
        super::meeting::set_attendees(s, &mut item, asked)?;
    }
    let seed = s.calendar.items.len() as u64 ^ (a.start as u64).rotate_left(17) ^ a.subject.len() as u64;
    calendar::set_global_id(&mut item, &calendar::new_global_id(s.time_now(), seed));
    let id = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.add(folder, item)?;
    s.calendar.day = calendar::day_of(a.start, &s.zone);
    s.calendar.selected = Some(id.0);
    s.calendar.occurrence = a.recurrence.as_ref().map(|r| r.start_day);
    s.refresh_folders(store);
    s.reload_calendar()?;
    s.status = format!("Appointment made: {}", if a.subject.is_empty() { "(No subject)" } else { &a.subject });
    Ok(json!({ "id": id.0 }))
}

fn chosen(s: &Session, p: &Value) -> Result<u64, EngineError> {
    p_u64(p, "id").or(s.calendar.selected).ok_or_else(|| EngineError::BadParams("no appointment is chosen".into()))
}

/// The appointment asked for, and the occurrence of it when one occurrence of a series is.
fn which(s: &Session, p: &Value) -> Result<(u64, Option<i64>), EngineError> {
    let id = chosen(s, p)?;
    if p_bool(p, "series") == Some(true) {
        return Ok((id, None));
    }
    match date_param(s, p, "occurrence")? {
        Some(day) => Ok((id, Some(day))),
        None if p_u64(p, "id").is_none() => Ok((id, s.calendar.occurrence)),
        None => Ok((id, None)),
    }
}

/// Weekday bits ([MS-OXOCAL] 2.2.1.44.1.2, Sunday the lowest) of names such as "monday" or "Mon".
fn weekdays(v: &Value) -> Result<u8, EngineError> {
    const NAMES: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
    let bad = || EngineError::BadParams("\"on\" is a list of weekday names, such as [\"monday\", \"thursday\"]".into());
    let list = v.as_array().ok_or_else(bad)?;
    let mut mask = 0u8;
    for name in list.iter().take(7) {
        let name = name.as_str().ok_or_else(bad)?.trim().to_ascii_lowercase();
        let at = NAMES.iter().position(|n| name.len() >= 3 && n.starts_with(name.get(..3).unwrap_or(""))).ok_or_else(bad)?;
        mask |= 1 << at;
    }
    if mask == 0 { Err(bad()) } else { Ok(mask) }
}

/// The pattern `p`'s "repeat" asks for, for an appointment that starts as `a` does, in a time
/// `zone`: `None` when it is not given, `Some(None)` for once only.
fn repeat_param(s: &Session, p: &Value, a: &Appointment, zone: &Zone) -> Result<Option<Option<Recurrence>>, EngineError> {
    let r = match p.get("repeat") {
        None => return Ok(None),
        Some(Value::Null) => return Ok(Some(None)),
        Some(Value::String(t)) if t.eq_ignore_ascii_case("none") => return Ok(Some(None)),
        Some(r @ Value::Object(_)) => r,
        Some(_) => return Err(EngineError::BadParams("\"repeat\" is null, \"none\" or an object such as {\"every\": \"week\"}".into())),
    };
    let local = local_minutes(a.start, zone);
    let day = recur::day_of_minutes(local);
    let start_offset = u32::try_from(local.saturating_sub(recur::minutes_of_day(day))).unwrap_or(0);
    let length = u32::try_from(a.minutes().max(0)).unwrap_or(u32::MAX);
    let end_offset = start_offset.saturating_add(length);
    let n = |key: &str, most: u64| -> Result<Option<u32>, EngineError> {
        match r.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_u64()
                .filter(|n| (1..=most).contains(n))
                .and_then(|n| u32::try_from(n).ok())
                .map(Some)
                .ok_or_else(|| EngineError::BadParams(format!("\"{key}\" is from 1 to {most}"))),
        }
    };
    let every = r.get("every").and_then(Value::as_str).unwrap_or("week").to_ascii_lowercase();
    let freq = match every.as_str() {
        "day" | "daily" => Freq::Daily,
        "weekday" | "week" | "weekly" => Freq::Weekly,
        "month" | "monthly" => Freq::Monthly,
        "year" | "yearly" => Freq::Yearly,
        other => return Err(EngineError::BadParams(format!("\"{other}\" is not day, weekday, week, month or year"))),
    };
    let interval = n("interval", if freq == Freq::Daily { 999 } else { 99 })?.unwrap_or(1);
    let on = r.get("on").map(weekdays).transpose()?;
    let this_weekday = 1u8 << recur::weekday(day);
    let rule = match freq {
        Freq::Daily => Rule::Day,
        Freq::Weekly if every == "weekday" => Rule::Week { days: 0x3E },
        Freq::Weekly => Rule::Week { days: on.unwrap_or(this_weekday) },
        Freq::Monthly | Freq::Yearly => {
            let nth = match r.get("nth") {
                None | Some(Value::Null) => None,
                Some(Value::String(t)) if t.eq_ignore_ascii_case("last") => Some(5),
                Some(v) => {
                    Some(v.as_u64().filter(|n| (1..=4).contains(n)).ok_or_else(|| EngineError::BadParams("\"nth\" is 1 to 4 or \"last\"".into()))?
                        as u32)
                }
            };
            if p_bool(r, "lastDay") == Some(true) {
                Rule::MonthEnd
            } else if let Some(nth) = nth {
                Rule::MonthNth { days: on.unwrap_or(this_weekday), nth }
            } else {
                let of_month = openmailclient_model::time::civil(calendar::day_start(day, 0), 0).day;
                Rule::Month { day: n("day", 31)?.unwrap_or(of_month) }
            }
        }
    };
    let end = match (n("count", u64::from(recur::MAX_COUNT))?, date_param(s, r, "until")?) {
        (Some(_), Some(_)) => return Err(EngineError::BadParams("a series ends after a count or on a day, not both".into())),
        (Some(count), None) => End::After(count),
        (None, Some(until)) if until < day => return Err(EngineError::BadParams("a series cannot end before it starts".into())),
        (None, Some(until)) => End::On(until),
        (None, None) => End::Never,
    };
    let period = if freq == Freq::Yearly { 12 } else { interval };
    let made = Recurrence::new(freq, rule, period, end, day, start_offset, end_offset, s.region.week_start);
    if made.days(made.start_day, made.start_day.saturating_add(1)).is_empty() {
        return Err(EngineError::BadParams("that pattern never falls on a day".into()));
    }
    Ok(Some(Some(made)))
}

/// A zone as `calendar.timeZones` gives it.
pub(crate) fn zone_json(zone: &Zone, now: i64) -> Value {
    json!({
        "id": zone.id,
        "key": zone.key,
        "label": zone.label(),
        "offsetNow": zone.offset_at(now),
        "daylight": zone.daylight.is_some(),
    })
}

fn time_zones(s: &mut Session, _: &Value) -> CmdResult {
    let now = s.now;
    let zones: Vec<Value> = Zone::known().iter().map(|z| zone_json(z, now)).collect();
    Ok(json!({
        "zones": zones,
        "current": zone_json(&s.zone, now),
        "system": zone_json(&s.system_zone, now),
        "chosen": s.zone_chosen,
        "second": s.second_zone.as_ref().map(|z| zone_json(z, now)),
    }))
}

fn time_zone(s: &mut Session, p: &Value) -> CmdResult {
    let name = match p.get("zone") {
        None => return Err(EngineError::BadParams("\"zone\" is needed: a time zone's name, or null to follow the system's".into())),
        Some(v) => zone_param(v)?,
    };
    s.choose_zone(name.as_ref().map(Zone::name))?;
    s.status = format!("Dates are shown in {} time, {} now.", s.zone.short(s.now), s.zone.offset_text_at(s.now));
    Ok(json!({ "current": zone_json(&s.zone, s.now) }))
}

fn second_time_zone(s: &mut Session, p: &Value) -> CmdResult {
    s.second_zone = match p.get("zone") {
        None => return Err(EngineError::BadParams("\"zone\" is needed: a time zone's name, or null for none".into())),
        Some(v) => zone_param(v)?,
    };
    Ok(json!({ "second": s.second_zone.as_ref().map(|z| zone_json(z, s.now)) }))
}

/// Local minutes since 1601 in `zone` of UTC FILETIME ticks.
fn local_minutes(utc: i64, zone: &Zone) -> i64 {
    utc.div_euclid(calendar::MINUTE).saturating_add(i64::from(zone.offset_at(utc)))
}

/// The zone `v` names: one of those known by its name or Outlook's key, or null for none.
pub(super) fn zone_param(v: &Value) -> Result<Option<Zone>, EngineError> {
    match v {
        Value::Null => Ok(None),
        Value::String(t) if t.trim().is_empty() || t.eq_ignore_ascii_case("none") => Ok(None),
        Value::String(t) => Zone::named(t).map(Some).ok_or_else(|| {
            EngineError::BadParams(format!("\"{}\" is not a time zone known; calendar.timeZones lists them", t.chars().take(80).collect::<String>()))
        }),
        _ => Err(EngineError::BadParams("\"zone\" is a time zone's name, such as \"Europe/Berlin\", or null".into())),
    }
}

/// `a` repeating as `pattern` says, or once only; a series starts at its first occurrence.
fn set_pattern(a: &mut Appointment, pattern: Option<Recurrence>, zone: &Zone) {
    match pattern {
        None => {
            a.recurrence = None;
            a.recurring = false;
        }
        Some(r) => {
            let length = a.end.saturating_sub(a.start);
            let first = recur::minutes_of_day(r.start_day).saturating_add(i64::from(r.start_offset));
            a.start = zone.utc_of(first.saturating_mul(calendar::MINUTE));
            a.end = a.start.saturating_add(length);
            a.recurrence = Some(r);
            a.recurring = true;
        }
    }
}

/// The series `was` with its first occurrence moved as `a`'s start and length say, or put in
/// the zone `a` is: the same pattern from the new day, its occurrences as planned again.
fn moved_series(was: &Appointment, a: &mut Appointment) {
    let Some(r) = was.recurrence.clone() else { return };
    if a.start == was.start && a.end == was.end && a.zone == was.zone {
        return;
    }
    let zone = match &a.zone {
        Some(z) if a.zone != was.zone => z.clone(),
        _ => was.series_zone().into_owned(),
    };
    let off = &zone;
    let local = local_minutes(a.start, off);
    let day = recur::day_of_minutes(local);
    let start_offset = u32::try_from(local.saturating_sub(recur::minutes_of_day(day))).unwrap_or(0);
    let end_offset = start_offset.saturating_add(u32::try_from(a.minutes().max(0)).unwrap_or(u32::MAX));
    let end = match r.end {
        End::On(d) if d < day => End::On(day),
        other => other,
    };
    set_pattern(a, Some(Recurrence::new(r.freq, r.rule.clone(), r.period, end, day, start_offset, end_offset, r.first_dow)), off);
}

fn change(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = writable_calendar(s)?;
    let (id, occurrence) = which(s, p)?;
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    let mut item = st.load(folder, ItemId(id))?;
    let mut was = Appointment::of(&item).ok_or_else(|| EngineError::BadParams("that item is not an appointment".into()))?;
    let before = Box::new(item.clone());
    match occurrence {
        Some(day) => {
            if p.get("repeat").is_some() {
                return Err(EngineError::BadParams("one occurrence cannot repeat on its own; change the series".into()));
            }
            if p.get("attendees").is_some() {
                return Err(EngineError::BadParams("those asked are asked to the whole series; change the series".into()));
            }
            let now = was.occurrence_of(day).ok_or_else(|| EngineError::BadParams("that appointment has no such occurrence".into()))?;
            let to = changed(s, p, Some(&now))?;
            // It stays between the occurrences before and after it, as Outlook keeps it.
            let (before_day, after_day) = was.recurrence.as_ref().map_or((None, None), |r| r.around(day));
            let lands = was.series_day(to.start);
            if before_day.is_some_and(|b| lands < b) || after_day.is_some_and(|a| lands > a) {
                return Err(EngineError::BadParams("an occurrence cannot be moved past the occurrence before or after it".into()));
            }
            was.change_occurrence(day, &to);
            was.write(&mut item);
        }
        None => {
            let mut a = changed(s, p, Some(&was))?;
            // The series' own time: the zone asked for, that of the pattern it had, or this
            // session's.
            let zone = match (&a.zone, was.recurrence.is_some()) {
                (Some(z), _) if a.zone != was.zone => z.clone(),
                (_, true) => was.series_zone().into_owned(),
                (z, false) => z.clone().unwrap_or_else(|| s.zone.clone()),
            };
            match repeat_param(s, p, &a, &zone)? {
                Some(pattern) => set_pattern(&mut a, pattern, &zone),
                None => moved_series(&was, &mut a),
            }
            a.write(&mut item);
            if let Some(v) = p.get("attendees") {
                let asked = super::meeting::attendees_param(s, v)?;
                super::meeting::set_attendees(s, &mut item, asked)?;
            }
        }
    }
    s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.update(folder, ItemId(id), item)?;
    s.did(Step::Edited { what: "Change Appointment", store, folder, id, before });
    s.refresh_folders(store);
    s.reload_calendar()?;
    ok()
}

fn recurrence(s: &mut Session, p: &Value) -> CmdResult {
    if p.get("repeat").is_none() {
        return Err(EngineError::BadParams("\"repeat\" is required: null for once, or a pattern".into()));
    }
    let mut asked = serde_json::Map::new();
    asked.insert("id".into(), json!(chosen(s, p)?));
    asked.insert("series".into(), json!(true));
    asked.insert("repeat".into(), p.get("repeat").cloned().unwrap_or(Value::Null));
    change(s, &Value::Object(asked))
}

pub(super) fn delete(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = writable_calendar(s)?;
    let (id, occurrence) = which(s, p)?;
    let deleted = s.find_folder(store, FolderKind::Deleted);
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    let mut item = st.load(folder, ItemId(id))?;
    let Some(mut a) = Appointment::of(&item) else {
        return Err(EngineError::BadParams("that item is not an appointment".into()));
    };
    if let Some(day) = occurrence {
        // One occurrence alone: the series is changed, and is undone as a change is.
        if a.occurrence_of(day).is_none() || !a.delete_occurrence(day) {
            return Err(EngineError::BadParams("that appointment has no such occurrence".into()));
        }
        let before = Box::new(item.clone());
        a.write(&mut item);
        st.update(folder, ItemId(id), item)?;
        s.did(Step::Edited { what: "Delete Occurrence", store, folder, id, before });
        s.calendar.selected = None;
        s.calendar.occurrence = None;
        s.refresh_folders(store);
        s.reload_calendar()?;
        return ok();
    }
    match deleted {
        Some(to) => {
            let moved = st.add(to, item)?;
            st.remove(folder, ItemId(id))?;
            s.did(Step::Moved { what: "Delete Appointment", store, from: folder, was: id, to, id: moved.0 });
        }
        None => {
            let item = Box::new(st.remove(folder, ItemId(id))?);
            s.did(Step::Removed { what: "Delete Appointment", store, folder, was: id, item });
        }
    }
    s.calendar.selected = None;
    s.calendar.occurrence = None;
    s.refresh_folders(store);
    s.reload_calendar()?;
    if s.current.is_some() {
        s.reload_rows()?;
    }
    ok()
}
