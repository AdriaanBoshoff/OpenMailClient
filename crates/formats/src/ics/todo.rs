//! To-dos (VTODO, RFC 5545 3.6.2) read into tasks and tasks written as to-dos.
//!
//! Outlook keeps a task's start and due date as days, and so are they read: a time given with
//! one is taken on the day it falls in where we are. A status RFC 5545 has no word for
//! (Waiting on someone else, Deferred) is written beside the nearest one it has, and read back
//! from there, so that a file written here comes back as it was.

use super::{Clock, Comp, MAX_BODY, MAX_LIST, MAX_TEXT, Prop, cut, date_text, duration, escape, line, note, text_list, utc_text};
use openmailclient_model::calendar;
use openmailclient_model::task::{Status, Task};
use openmailclient_model::zone::Zone;
use openmailclient_model::{Item, time};

/// The status a task of ours was in, where RFC 5545 has no word for it.
const OWN_STATUS: &str = "X-OPENMAILCLIENT-TASK-STATUS";
/// Outlook's own word for how much a task is thought to take and took, in minutes.
const TOTAL_WORK: &str = "X-OPENMAILCLIENT-TOTAL-WORK";
const ACTUAL_WORK: &str = "X-OPENMAILCLIENT-ACTUAL-WORK";

/// A to-do of a calendar file, read as a task.
#[derive(Clone, Debug, PartialEq)]
pub struct Todo {
    /// Its UID (RFC 5545 3.8.4.7), by which a copy of it is known again.
    pub uid: String,
    pub task: Task,
    /// How many times it was changed (SEQUENCE).
    pub sequence: i32,
    /// What of it could not be kept.
    pub notes: Vec<String>,
}

impl Todo {
    /// A new task item saying this to-do, its UID kept as its GlobalObjectId.
    pub fn item(&self, today: i64, now: i64, here: &Zone) -> Item {
        let mut it = self.task.item(today, now, here);
        calendar::set_global_id(&mut it, &calendar::global_id_of_uid(&self.uid));
        it
    }

    /// `item` made to say this to-do; what else it has is kept.
    pub fn write(&self, item: &mut Item, today: i64, now: i64, here: &Zone) {
        self.task.write(item, today, now, here);
        calendar::set_global_id(item, &calendar::global_id_of_uid(&self.uid));
    }
}

/// The day a date or time falls on where we are.
fn day_of(p: &Prop, clock: &Clock) -> Option<i64> {
    let (t, s, ..) = clock.at(p)?;
    let day = if s.date_only || (!s.utc && p.param("TZID").is_none()) { s.day } else { calendar::day_of(t, clock.here) };
    // From 1601, where FILETIME starts, to the year 9999.
    (-134_774..=2_932_896).contains(&day).then_some(day)
}

/// The UTC ticks a local midnight falls at where we are.
fn midnight(day: i64, here: &Zone) -> i64 {
    here.utc_of(time::from_unix(day.saturating_mul(86_400)))
}

/// A to-do read as a task.
pub(super) fn todo_of(c: &Comp, clock: &Clock, uid: &str) -> Todo {
    let mut notes = Vec::new();
    let value = |name: &str| c.prop(name).map(|p| p.value.trim().to_ascii_uppercase()).unwrap_or_default();
    let start = c.prop("DTSTART").and_then(|p| day_of(p, clock));
    let due = c.prop("DUE").and_then(|p| day_of(p, clock)).or_else(|| {
        // A start and how long it lasts, in place of a due date.
        let d = duration(&c.prop("DURATION")?.value)?;
        start.map(|s| s.saturating_add(d.div_euclid(86_400)))
    });
    let mut status = match (value(OWN_STATUS).as_str(), value("STATUS").as_str()) {
        ("WAITING", _) => Status::Waiting,
        ("DEFERRED", _) => Status::Deferred,
        (_, "IN-PROCESS") => Status::InProgress,
        (_, "COMPLETED") => Status::Complete,
        (_, "CANCELLED") => {
            note(&mut notes, "it was cancelled: it is kept as Deferred");
            Status::Deferred
        }
        _ => Status::NotStarted,
    };
    let completed = c.prop("COMPLETED").and_then(|p| day_of(p, clock));
    if completed.is_some() && status != Status::Complete && value("STATUS").is_empty() {
        status = Status::Complete;
    }
    let percent = c.prop("PERCENT-COMPLETE").and_then(|p| p.value.trim().parse::<u8>().ok()).unwrap_or(0).min(100);
    // RFC 5545 3.8.1.9: 1 to 4 high, 5 (or none) normal, 6 to 9 low.
    let importance = match c.prop("PRIORITY").and_then(|p| p.value.trim().parse::<u32>().ok()).unwrap_or(0) {
        1..=4 => 2,
        6..=9 => 0,
        _ => 1,
    };
    let reminder = c.children.iter().find(|a| a.name == "VALARM").and_then(|a| {
        let trigger = a.prop("TRIGGER")?;
        if trigger.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE-TIME")) {
            return clock.at(trigger).map(|(t, ..)| t);
        }
        let to_end = trigger.param("RELATED").is_some_and(|r| r.eq_ignore_ascii_case("END"));
        let base = if to_end { due.or(start) } else { start.or(due) }?;
        Some(midnight(base, clock.here).saturating_add(duration(&trigger.value)?.saturating_mul(10_000_000)))
    });
    if c.prop("RRULE").is_some() {
        note(&mut notes, "it repeats: it is added once, as its first time");
    }
    let class = value("CLASS");
    let work = |name: &str| c.prop(name).and_then(|p| p.value.trim().parse::<i32>().ok()).unwrap_or(0).clamp(0, 1 << 24);
    let task = Task {
        subject: c.text("SUMMARY", MAX_TEXT).unwrap_or_default(),
        start,
        due,
        percent: if status == Status::Complete { 100 } else { percent },
        status,
        importance,
        completed: completed.filter(|_| status == Status::Complete),
        reminder: reminder.filter(|t| *t > 0),
        categories: c.all("CATEGORIES").flat_map(|p| text_list(&p.value)).take(MAX_LIST).collect(),
        notes: c.text("DESCRIPTION", MAX_BODY).unwrap_or_default(),
        private: class == "PRIVATE" || class == "CONFIDENTIAL",
        total_work: work(TOTAL_WORK),
        actual_work: work(ACTUAL_WORK),
        ..Task::default()
    };
    Todo { uid: uid.to_string(), task, sequence: super::sequence_of(c), notes }
}

/// A task item written as a to-do.
pub(super) fn write_todo(out: &mut String, item: &Item, here: &Zone, now: i64) {
    let t = Task::of(item, here);
    line(out, "BEGIN:VTODO");
    line(out, &format!("UID:{}", super::uid(item)));
    line(out, &format!("DTSTAMP:{}", utc_text(now)));
    line(out, &format!("SUMMARY:{}", escape(&t.subject)));
    if let Some(d) = t.start {
        line(out, &format!("DTSTART;VALUE=DATE:{}", date_text(d)));
    }
    if let Some(d) = t.due {
        // A due date before the start is not one RFC 5545 allows (3.8.2.3): it is the start.
        line(out, &format!("DUE;VALUE=DATE:{}", date_text(d.max(t.start.unwrap_or(d)))));
    }
    let (status, own) = match t.status {
        Status::NotStarted => ("NEEDS-ACTION", None),
        Status::InProgress => ("IN-PROCESS", None),
        Status::Complete => ("COMPLETED", None),
        Status::Waiting => ("NEEDS-ACTION", Some("WAITING")),
        Status::Deferred => ("NEEDS-ACTION", Some("DEFERRED")),
    };
    line(out, &format!("STATUS:{status}"));
    if let Some(own) = own {
        line(out, &format!("{OWN_STATUS}:{own}"));
    }
    if t.percent > 0 {
        line(out, &format!("PERCENT-COMPLETE:{}", t.percent.min(100)));
    }
    if let Some(d) = t.completed.filter(|_| t.is_complete()) {
        // COMPLETED is a time in UTC (RFC 5545 3.8.2.1): midnight of the day it was done.
        line(out, &format!("COMPLETED:{}", utc_text(midnight(d, here))));
    }
    line(out, &format!("PRIORITY:{}", [9, 5, 1].get(usize::try_from(t.importance).unwrap_or(1)).unwrap_or(&5)));
    if t.private {
        line(out, "CLASS:PRIVATE");
    }
    if !t.notes.trim().is_empty() {
        line(out, &format!("DESCRIPTION:{}", escape(&cut(&t.notes, MAX_BODY))));
    }
    if !t.categories.is_empty() {
        line(out, &format!("CATEGORIES:{}", t.categories.iter().take(MAX_LIST).map(|c| escape(c)).collect::<Vec<_>>().join(",")));
    }
    if t.total_work > 0 {
        line(out, &format!("{TOTAL_WORK}:{}", t.total_work));
    }
    if t.actual_work > 0 {
        line(out, &format!("{ACTUAL_WORK}:{}", t.actual_work));
    }
    if let Some(at) = t.reminder {
        line(out, "BEGIN:VALARM");
        line(out, "ACTION:DISPLAY");
        line(out, "DESCRIPTION:Reminder");
        line(out, &format!("TRIGGER;VALUE=DATE-TIME:{}", utc_text(at)));
        line(out, "END:VALARM");
    }
    line(out, "END:VTODO");
}
