//! Tasks and mail flagged for follow-up: what an item of a Tasks folder says of a piece of
//! work, read from and written to the properties [MS-OXOTASK] gives it, and the follow-up flag
//! a message carries to be on the to-do list ([MS-OXOFLAG]).
//!
//! A task's status, how far it is done and its start, due and completed dates are named
//! properties of the task property set ([MS-OXOTASK] 2.2.2.2); its reminder, and the start and
//! end every item kept as a to-do has, are of the common property set ([MS-OXOTASK] 2.2.1,
//! [MS-OXORMDR] 2.2.1). A message flagged for follow-up keeps its flag in PidTagFlagStatus and
//! the same task dates beside it, so that it stands on the to-do list as a task does
//! ([MS-OXOFLAG] 2.2.1, 3.1.4.1).
//!
//! The start and due dates are days, not moments: as the specification keeps them, the date's
//! midnight written as though it were UTC is in the task property set, and the local midnight
//! in UTC in the common one ([MS-OXOTASK] 2.2.2.2.4, 2.2.2.2.5; [MS-OXPROPS] PidLidCommonStart).

use crate::calendar::{bool_of, i32_of, named, remove_named, set_named, time_of};
use crate::props::{PropValue, pid, pset};
use crate::zone::Offsets;
use crate::{Flag, Item, time};

/// PSETID_Task {00062003-0000-0000-C000-000000000046} ([MS-OXPROPS] 1.3.2).
pub const PSETID_TASK: [u8; 16] = [0x03, 0x20, 0x06, 0x00, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46];

/// The message class of a task ([MS-OXOTASK] 2.2.1.1).
pub const CLASS: &str = "IPM.Task";

/// Longest subject, owner and other one-line field kept, in characters.
pub const MAX_FIELD: usize = 4_000;
/// Longest notes kept, in characters.
pub const MAX_NOTES: usize = 200_000;
/// Longest a piece of work may be said to take, in minutes: a hundred years.
pub const MAX_WORK: i32 = 60 * 24 * 365 * 100;
/// Companies a task names at most.
pub const MAX_COMPANIES: usize = 100;

/// Named properties of the task property set ([MS-OXOTASK] 2.2.2.2).
mod lid {
    pub const STATUS: u32 = 0x8101;
    pub const PERCENT_COMPLETE: u32 = 0x8102;
    pub const TEAM_TASK: u32 = 0x8103;
    pub const START_DATE: u32 = 0x8104;
    pub const DUE_DATE: u32 = 0x8105;
    pub const RESET_REMINDER: u32 = 0x8107;
    pub const DATE_COMPLETED: u32 = 0x810F;
    pub const ACTUAL_EFFORT: u32 = 0x8110;
    pub const ESTIMATED_EFFORT: u32 = 0x8111;
    pub const STATE: u32 = 0x8113;
    pub const COMPLETE: u32 = 0x811C;
    pub const OWNER: u32 = 0x811F;
    pub const ORDINAL: u32 = 0x8123;
    pub const RECURRING: u32 = 0x8126;
    pub const OWNERSHIP: u32 = 0x8129;
    pub const ACCEPTANCE_STATE: u32 = 0x812A;
}

/// Named properties of the common property set a task or a flagged message keeps
/// ([MS-OXOTASK] 2.2.1, [MS-OXORMDR] 2.2.1, [MS-OXOFLAG] 2.2.1).
mod common {
    pub const REMINDER_TIME: u32 = 0x8502;
    pub const REMINDER_SET: u32 = 0x8503;
    pub const COMMON_START: u32 = 0x8516;
    pub const COMMON_END: u32 = 0x8517;
    pub const TASK_MODE: u32 = 0x8518;
    pub const FLAG_REQUEST: u32 = 0x8530;
    pub const MILEAGE: u32 = 0x8534;
    pub const BILLING: u32 = 0x8535;
    pub const COMPANIES: u32 = 0x8539;
    pub const REMINDER_SIGNAL_TIME: u32 = 0x8560;
    pub const TO_DO_ORDINAL_DATE: u32 = 0x85A0;
    pub const TO_DO_SUB_ORDINAL: u32 = 0x85A1;
    pub const TO_DO_TITLE: u32 = 0x85A4;
}

/// Tagged properties of a flag ([MS-OXOFLAG] 2.2.1).
mod tag {
    pub const SENSITIVITY: u16 = 0x0036;
    /// PidTagToDoItemFlags: 1 says the message is on the to-do list by its own owner's flag.
    pub const TO_DO_ITEM_FLAGS: u16 = 0x0E2B;
    pub const ICON_INDEX: u16 = 0x1080;
    pub const FLAG_COMPLETE_TIME: u16 = 0x1091;
    pub const FOLLOWUP_ICON: u16 = 0x1095;
}

/// PidTagIconIndex of a task ([MS-OXOTASK] 2.2.1.2).
const TASK_ICON: i32 = 0x0500;
/// PidTagFollowupIcon of a red flag ([MS-OXOFLAG] 2.2.1.4).
const RED_FLAG: i32 = 6;
/// PidTagSensitivity of a private item ([MS-OXCMSG] 2.2.1.13).
const PRIVATE: i32 = 2;
/// PidLidFlagRequest written on a message flagged for follow-up.
pub const FOLLOW_UP: &str = "Follow up";

/// How far along a task is, as PidLidTaskStatus says ([MS-OXOTASK] 2.2.2.2.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    #[default]
    NotStarted,
    InProgress,
    Complete,
    Waiting,
    Deferred,
}

impl Status {
    pub const ALL: [Self; 5] = [Self::NotStarted, Self::InProgress, Self::Complete, Self::Waiting, Self::Deferred];

    fn code(self) -> i32 {
        match self {
            Self::NotStarted => 0,
            Self::InProgress => 1,
            Self::Complete => 2,
            Self::Waiting => 3,
            Self::Deferred => 4,
        }
    }

    fn of_code(n: i32) -> Self {
        match n {
            1 => Self::InProgress,
            2 => Self::Complete,
            3 => Self::Waiting,
            4 => Self::Deferred,
            _ => Self::NotStarted,
        }
    }

    /// The name commands give it.
    pub fn name(self) -> &'static str {
        match self {
            Self::NotStarted => "notStarted",
            Self::InProgress => "inProgress",
            Self::Complete => "complete",
            Self::Waiting => "waiting",
            Self::Deferred => "deferred",
        }
    }

    /// The name the window shows.
    pub fn label(self) -> &'static str {
        match self {
            Self::NotStarted => "Not Started",
            Self::InProgress => "In Progress",
            Self::Complete => "Completed",
            Self::Waiting => "Waiting on someone else",
            Self::Deferred => "Deferred",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.name().eq_ignore_ascii_case(s) || v.label().eq_ignore_ascii_case(s))
    }
}

/// When something flagged is to be followed up, as the Follow Up menu offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FollowUp {
    Today,
    Tomorrow,
    ThisWeek,
    NextWeek,
    NoDate,
}

impl FollowUp {
    pub const ALL: [Self; 5] = [Self::Today, Self::Tomorrow, Self::ThisWeek, Self::NextWeek, Self::NoDate];

    pub fn name(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Tomorrow => "tomorrow",
            Self::ThisWeek => "thisWeek",
            Self::NextWeek => "nextWeek",
            Self::NoDate => "noDate",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Tomorrow => "Tomorrow",
            Self::ThisWeek => "This Week",
            Self::NextWeek => "Next Week",
            Self::NoDate => "No Date",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.name().eq_ignore_ascii_case(s) || v.label().eq_ignore_ascii_case(s))
    }

    /// The start and due days it gives, from the day it is today. A week is the work week,
    /// Monday to Friday: this week's work is due on its Friday (today, once Friday is past),
    /// next week's starts on its Monday and is due on its Friday.
    pub fn days(self, today: i64) -> (Option<i64>, Option<i64>) {
        // 1970-01-01 was a Thursday; 0 is Monday here.
        let weekday = today.saturating_add(3).rem_euclid(7);
        let friday = today.saturating_add(4 - weekday);
        match self {
            Self::Today => (Some(today), Some(today)),
            Self::Tomorrow => (Some(today.saturating_add(1)), Some(today.saturating_add(1))),
            Self::ThisWeek => (Some(today), Some(friday.max(today))),
            Self::NextWeek => {
                let monday = today.saturating_add(7 - weekday);
                (Some(monday), Some(monday.saturating_add(4)))
            }
            Self::NoDate => (None, None),
        }
    }
}

/// What a task, or a message flagged for follow-up, says of the work to be done.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Task {
    pub subject: String,
    /// Day numbers (days since 1970-01-01) it starts and is due on.
    pub start: Option<i64>,
    pub due: Option<i64>,
    pub status: Status,
    /// From 0 to 100.
    pub percent: u8,
    /// 0 low, 1 normal, 2 high (PidTagImportance).
    pub importance: i32,
    /// The day it was done on.
    pub completed: Option<i64>,
    /// When it reminds, UTC FILETIME ticks.
    pub reminder: Option<i64>,
    pub categories: Vec<String>,
    pub notes: String,
    pub owner: String,
    pub private: bool,
    /// What it was thought to take and what it took, in minutes.
    pub total_work: i32,
    pub actual_work: i32,
    pub mileage: String,
    pub billing: String,
    pub companies: Vec<String>,
    /// It recurs; its pattern is kept as it is.
    pub recurring: bool,
    /// It is a message flagged for follow-up, not a task.
    pub flagged_mail: bool,
}

fn cut(s: &str, most: usize) -> String {
    let s = s.trim_matches(|c: char| c.is_whitespace() && c != '\n');
    if s.chars().count() <= most { s.to_string() } else { s.chars().take(most).collect() }
}

fn named_text(item: &Item, guid: [u8; 16], id: u32) -> String {
    match named(item, guid, id) {
        Some(PropValue::String(s)) => cut(s, MAX_FIELD),
        _ => String::new(),
    }
}

fn put_named(item: &mut Item, guid: [u8; 16], id: u32, value: &str) {
    if value.is_empty() {
        remove_named(item, guid, id);
    } else {
        set_named(item, guid, id, PropValue::String(value.to_string()));
    }
}

/// No date before 1601 or after 9999.
fn in_range(day: i64) -> bool {
    (-134_774..=2_932_896).contains(&day)
}

/// A day number at its midnight written as though it were UTC, as the task dates are kept.
fn floating(day: i64) -> i64 {
    time::from_unix(day.saturating_mul(86_400))
}

fn day_floating(t: i64) -> i64 {
    time::to_unix(t).div_euclid(86_400)
}

/// A day read from the task property set, or from the common one's local midnight.
fn day_of(item: &Item, task_id: u32, common_id: u32, here: impl Offsets + Copy) -> Option<i64> {
    time_of(named(item, PSETID_TASK, task_id))
        .map(day_floating)
        .or_else(|| time_of(named(item, pset::PSETID_COMMON, common_id)).map(|t| crate::calendar::day_of(t, here)))
        .filter(|d| in_range(*d))
}

/// Whether an item is a task, by its class.
pub fn is_task(item: &Item) -> bool {
    let class = item.message_class().to_ascii_lowercase();
    class == "ipm.task" || class.starts_with("ipm.task.")
}

/// Whether a message stands on the to-do list by a flag: flagged, or flagged and done.
pub fn is_flagged(item: &Item) -> bool {
    item.flag() != Flag::None
}

/// Sets the start and due days of a task or of a message, in both the forms they are kept;
/// `None` takes a day away.
fn write_days(item: &mut Item, start: Option<i64>, due: Option<i64>, here: impl Offsets + Copy) {
    let (t, c) = (PSETID_TASK, pset::PSETID_COMMON);
    for (day, task_id, common_id) in [(start, lid::START_DATE, common::COMMON_START), (due, lid::DUE_DATE, common::COMMON_END)] {
        match day.filter(|d| in_range(*d)) {
            Some(d) => {
                set_named(item, t, task_id, PropValue::Time(floating(d)));
                set_named(item, c, common_id, PropValue::Time(crate::calendar::day_start(d, here)));
            }
            None => {
                remove_named(item, t, task_id);
                remove_named(item, c, common_id);
            }
        }
    }
}

/// Flags a message for follow-up from `start` to `due`, as the Follow Up menu does: it goes
/// on the to-do list under its own subject unless it was given another ([MS-OXOFLAG] 3.1.4.1.1).
/// A task flagged so takes the days alone.
pub fn follow_up(item: &mut Item, start: Option<i64>, due: Option<i64>, now: i64, here: impl Offsets + Copy) {
    // A due day before the start moves the start back to it, as Outlook keeps them in order.
    let start = match (start, due) {
        (Some(s), Some(d)) if s > d => Some(d),
        _ => start,
    };
    write_days(item, start, due, here);
    let (t, c) = (PSETID_TASK, pset::PSETID_COMMON);
    if is_task(item) {
        return;
    }
    item.set_flag(Flag::Flagged);
    item.props.set(tag::FOLLOWUP_ICON, PropValue::Int32(RED_FLAG));
    item.props.set(tag::TO_DO_ITEM_FLAGS, PropValue::Int32(1));
    item.props.remove(tag::FLAG_COMPLETE_TIME);
    if named_text(item, c, common::FLAG_REQUEST).is_empty() {
        set_named(item, c, common::FLAG_REQUEST, PropValue::String(FOLLOW_UP.to_string()));
    }
    if named_text(item, c, common::TO_DO_TITLE).is_empty() {
        let subject = cut(&item.subject(), MAX_FIELD);
        set_named(item, c, common::TO_DO_TITLE, PropValue::String(subject));
    }
    // Where it stands among the to-do list's items: by when it was flagged.
    set_named(item, c, common::TO_DO_ORDINAL_DATE, PropValue::Time(now));
    if named(item, c, common::TO_DO_SUB_ORDINAL).is_none() {
        set_named(item, c, common::TO_DO_SUB_ORDINAL, PropValue::String("5555555".to_string()));
    }
    set_named(item, t, lid::STATUS, PropValue::Int32(Status::NotStarted.code()));
    set_named(item, t, lid::PERCENT_COMPLETE, PropValue::Float64(0.0));
    set_named(item, t, lid::COMPLETE, PropValue::Bool(false));
    remove_named(item, t, lid::DATE_COMPLETED);
}

/// Marks a task or a flagged message done on day `today`: complete, all of it done, the day
/// it was done kept and its reminder put away ([MS-OXOTASK] 3.1.4.1.2, [MS-OXOFLAG] 3.1.4.1.2).
pub fn mark_complete(item: &mut Item, today: i64, now: i64) {
    let (t, c) = (PSETID_TASK, pset::PSETID_COMMON);
    set_named(item, t, lid::STATUS, PropValue::Int32(Status::Complete.code()));
    set_named(item, t, lid::PERCENT_COMPLETE, PropValue::Float64(1.0));
    set_named(item, t, lid::COMPLETE, PropValue::Bool(true));
    set_named(item, t, lid::DATE_COMPLETED, PropValue::Time(floating(today)));
    set_named(item, c, common::REMINDER_SET, PropValue::Bool(false));
    if !is_task(item) {
        item.set_flag(Flag::Complete);
        item.props.set(tag::FLAG_COMPLETE_TIME, PropValue::Time(now));
        item.props.remove(tag::FOLLOWUP_ICON);
    }
}

/// Takes a message's follow-up flag away, and with it everything that put it on the to-do
/// list ([MS-OXOFLAG] 3.1.4.1.3). A task is not a flag, and is left as it is.
pub fn clear_flag(item: &mut Item) {
    if is_task(item) {
        return;
    }
    let (t, c) = (PSETID_TASK, pset::PSETID_COMMON);
    item.set_flag(Flag::None);
    for id in [tag::FOLLOWUP_ICON, tag::TO_DO_ITEM_FLAGS, tag::FLAG_COMPLETE_TIME] {
        item.props.remove(id);
    }
    for id in [lid::START_DATE, lid::DUE_DATE, lid::STATUS, lid::PERCENT_COMPLETE, lid::COMPLETE, lid::DATE_COMPLETED] {
        remove_named(item, t, id);
    }
    for id in [
        common::COMMON_START,
        common::COMMON_END,
        common::FLAG_REQUEST,
        common::TO_DO_TITLE,
        common::TO_DO_ORDINAL_DATE,
        common::TO_DO_SUB_ORDINAL,
        common::REMINDER_SET,
        common::REMINDER_TIME,
        common::REMINDER_SIGNAL_TIME,
    ] {
        remove_named(item, c, id);
    }
}

impl Task {
    /// What `item` says of itself as a task, or as a flagged message. `here` is where a day
    /// kept only as a local midnight is read.
    pub fn of(item: &Item, here: impl Offsets + Copy) -> Self {
        let (t, c) = (PSETID_TASK, pset::PSETID_COMMON);
        let flagged_mail = !is_task(item);
        let mut subject = if flagged_mail { named_text(item, c, common::TO_DO_TITLE) } else { String::new() };
        if subject.is_empty() {
            subject = cut(&item.subject(), MAX_FIELD);
        }
        let mut status = i32_of(named(item, t, lid::STATUS)).map_or(Status::NotStarted, Status::of_code);
        let percent = match named(item, t, lid::PERCENT_COMPLETE) {
            Some(PropValue::Float64(f)) if f.is_finite() => (f.clamp(0.0, 1.0) * 100.0).round() as u8,
            Some(PropValue::Float32(f)) if f.is_finite() => (f.clamp(0.0, 1.0) * 100.0).round() as u8,
            _ => 0,
        };
        // Done however it says so: by its status, by PidLidTaskComplete or by its flag.
        let done =
            status == Status::Complete || bool_of(named(item, t, lid::COMPLETE)) == Some(true) || (flagged_mail && item.flag() == Flag::Complete);
        if done {
            status = Status::Complete;
        }
        let completed = time_of(named(item, t, lid::DATE_COMPLETED))
            .map(day_floating)
            .or_else(|| item.props.time(tag::FLAG_COMPLETE_TIME).map(|t| crate::calendar::day_of(t, here)))
            .filter(|d| in_range(*d) && done);
        let reminder = (bool_of(named(item, c, common::REMINDER_SET)) == Some(true))
            .then(|| time_of(named(item, c, common::REMINDER_TIME)).or_else(|| time_of(named(item, c, common::REMINDER_SIGNAL_TIME))))
            .flatten()
            .filter(|t| *t > 0);
        let work = |id| i32_of(named(item, t, id)).unwrap_or(0).clamp(0, MAX_WORK);
        let companies = match named(item, c, common::COMPANIES) {
            Some(PropValue::MultiString(list)) => list.iter().map(|s| cut(s, MAX_FIELD)).filter(|s| !s.is_empty()).take(MAX_COMPANIES).collect(),
            _ => Vec::new(),
        };
        Self {
            subject,
            start: day_of(item, lid::START_DATE, common::COMMON_START, here),
            due: day_of(item, lid::DUE_DATE, common::COMMON_END, here),
            status,
            percent: if done { 100 } else { percent.min(100) },
            importance: item.importance().clamp(0, 2),
            completed,
            reminder,
            categories: item.categories.clone(),
            notes: cut(&item.body_text(), MAX_NOTES),
            owner: named_text(item, t, lid::OWNER),
            private: item.props.i32(tag::SENSITIVITY) == Some(PRIVATE),
            total_work: work(lid::ESTIMATED_EFFORT),
            actual_work: work(lid::ACTUAL_EFFORT),
            mileage: named_text(item, c, common::MILEAGE),
            billing: named_text(item, c, common::BILLING),
            companies,
            recurring: bool_of(named(item, t, lid::RECURRING)) == Some(true),
            flagged_mail,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.status == Status::Complete
    }

    /// Not done, and due before `today`.
    pub fn is_overdue(&self, today: i64) -> bool {
        !self.is_complete() && self.due.is_some_and(|d| d < today)
    }

    /// Whether every word of `query` is somewhere in its subject, owner, categories, companies
    /// or notes, without case.
    pub fn matches(&self, query: &str) -> bool {
        let mut hay = String::new();
        for s in [&self.subject, &self.owner, &self.notes, &self.billing, &self.mileage] {
            hay.push_str(s);
            hay.push('\n');
        }
        for s in self.categories.iter().chain(&self.companies) {
            hay.push_str(s);
            hay.push('\n');
        }
        hay.push_str(self.status.label());
        let hay = hay.to_lowercase();
        query.split_whitespace().all(|w| hay.contains(&w.to_lowercase()))
    }

    /// Writes it into `item`, keeping every property it does not speak of. A flagged message
    /// takes the subject as its to-do title, its own subject staying as it came, and keeps its
    /// flag; the rest of a task is not written on a message. `today` is the day a task made
    /// complete here was done on.
    pub fn write(&self, item: &mut Item, today: i64, now: i64, here: impl Offsets + Copy) {
        let (t, c) = (PSETID_TASK, pset::PSETID_COMMON);
        let mail = !is_task(item);
        let start = match (self.start, self.due) {
            (Some(s), Some(d)) if s > d => Some(d),
            _ => self.start,
        };
        if mail {
            put_named(item, c, common::TO_DO_TITLE, &cut(&self.subject, MAX_FIELD));
            if !is_flagged(item) {
                follow_up(item, start, self.due, now, here);
            } else {
                write_days(item, start, self.due, here);
            }
        } else {
            item.set_subject(&cut(&self.subject, MAX_FIELD));
            write_days(item, start, self.due, here);
            item.set_body_text(&cut(&self.notes, MAX_NOTES));
            item.categories.clone_from(&self.categories);
            item.props.set(pid::IMPORTANCE, PropValue::Int32(self.importance.clamp(0, 2)));
            if self.private {
                item.props.set(tag::SENSITIVITY, PropValue::Int32(PRIVATE));
            } else if item.props.i32(tag::SENSITIVITY) == Some(PRIVATE) {
                item.props.set(tag::SENSITIVITY, PropValue::Int32(0));
            }
            put_named(item, t, lid::OWNER, &cut(&self.owner, MAX_FIELD));
            for (id, minutes) in [(lid::ESTIMATED_EFFORT, self.total_work), (lid::ACTUAL_EFFORT, self.actual_work)] {
                set_named(item, t, id, PropValue::Int32(minutes.clamp(0, MAX_WORK)));
            }
            put_named(item, c, common::MILEAGE, &cut(&self.mileage, MAX_FIELD));
            put_named(item, c, common::BILLING, &cut(&self.billing, MAX_FIELD));
            if self.companies.is_empty() {
                remove_named(item, c, common::COMPANIES);
            } else {
                let list = self.companies.iter().map(|s| cut(s, MAX_FIELD)).take(MAX_COMPANIES).collect();
                set_named(item, c, common::COMPANIES, PropValue::MultiString(list));
            }
            // A task of one's own, not one given to someone or taken from them.
            for (id, v) in [(lid::STATE, 1), (lid::OWNERSHIP, 0), (lid::ACCEPTANCE_STATE, 0)] {
                if named(item, t, id).is_none() {
                    set_named(item, t, id, PropValue::Int32(v));
                }
            }
            if named(item, c, common::TASK_MODE).is_none() {
                set_named(item, c, common::TASK_MODE, PropValue::Int32(0));
            }
            if named(item, t, lid::TEAM_TASK).is_none() {
                set_named(item, t, lid::TEAM_TASK, PropValue::Bool(false));
            }
            if item.props.i32(tag::ICON_INDEX).is_none() {
                item.props.set(tag::ICON_INDEX, PropValue::Int32(TASK_ICON));
            }
        }
        // How far along it is: done is all of it, and all of it is done.
        let (status, percent) = match (self.status, self.percent.min(100)) {
            (Status::Complete, _) | (_, 100) => (Status::Complete, 100),
            (Status::NotStarted, p) if p > 0 => (Status::InProgress, p),
            (s, p) => (s, p),
        };
        let was_done = Task::of(item, here).is_complete();
        set_named(item, t, lid::STATUS, PropValue::Int32(status.code()));
        set_named(item, t, lid::PERCENT_COMPLETE, PropValue::Float64(f64::from(percent) / 100.0));
        set_named(item, t, lid::COMPLETE, PropValue::Bool(status == Status::Complete));
        if status == Status::Complete {
            let on = self.completed.filter(|d| in_range(*d)).unwrap_or(today);
            set_named(item, t, lid::DATE_COMPLETED, PropValue::Time(floating(on)));
            if mail && !was_done {
                item.set_flag(Flag::Complete);
                item.props.set(tag::FLAG_COMPLETE_TIME, PropValue::Time(now));
                item.props.remove(tag::FOLLOWUP_ICON);
            }
        } else {
            remove_named(item, t, lid::DATE_COMPLETED);
            if mail && was_done {
                item.set_flag(Flag::Flagged);
                item.props.remove(tag::FLAG_COMPLETE_TIME);
                item.props.set(tag::FOLLOWUP_ICON, PropValue::Int32(RED_FLAG));
            }
        }
        // A reminder, put away once the work is done.
        match self.reminder.filter(|r| *r > 0 && status != Status::Complete) {
            Some(r) => {
                set_named(item, c, common::REMINDER_SET, PropValue::Bool(true));
                set_named(item, c, common::REMINDER_TIME, PropValue::Time(r));
                set_named(item, c, common::REMINDER_SIGNAL_TIME, PropValue::Time(r));
                set_named(item, t, lid::RESET_REMINDER, PropValue::Bool(false));
            }
            None => {
                set_named(item, c, common::REMINDER_SET, PropValue::Bool(false));
                remove_named(item, c, common::REMINDER_SIGNAL_TIME);
            }
        }
    }

    /// A new task item of what it says.
    pub fn item(&self, today: i64, now: i64, here: impl Offsets + Copy) -> Item {
        let mut it = Item::default();
        it.props.set_str(pid::MESSAGE_CLASS, CLASS);
        it.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(crate::MSGFLAG_READ));
        it.props.set(pid::CREATION_TIME, PropValue::Time(now));
        set_named(&mut it, PSETID_TASK, lid::ORDINAL, PropValue::Int32(0));
        self.write(&mut it, today, now, here);
        it
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-22, a Thursday.
    const TODAY: i64 = 20_748;
    const NOW: i64 = 134_000_000_000_000_000;

    #[test]
    fn a_task_reads_back_as_written_and_keeps_what_it_does_not_speak_of() {
        let t = Task {
            subject: "Write the report".into(),
            start: Some(TODAY),
            due: Some(TODAY + 3),
            status: Status::InProgress,
            percent: 40,
            importance: 2,
            reminder: Some(NOW + 1),
            categories: vec!["Work".into()],
            notes: "Two pages.\nWith a table.".into(),
            owner: "Me".into(),
            private: true,
            total_work: 120,
            actual_work: 30,
            mileage: "12 km".into(),
            billing: "Client A".into(),
            companies: vec!["Contoso".into()],
            ..Task::default()
        };
        let mut item = t.item(TODAY, NOW, 120);
        set_named(&mut item, PSETID_TASK, 0x8999, PropValue::Int32(7));
        assert_eq!(item.message_class(), CLASS);
        assert_eq!(Task::of(&item, 120), t);
        // Read back in another zone, the days are the same days.
        assert_eq!(Task::of(&item, -300).due, Some(TODAY + 3));
        let mut changed = Task::of(&item, 120);
        changed.subject = "Write the long report".into();
        changed.write(&mut item, TODAY, NOW, 120);
        assert_eq!(named(&item, PSETID_TASK, 0x8999), Some(&PropValue::Int32(7)));
        assert_eq!(item.subject(), "Write the long report");
    }

    #[test]
    fn done_is_all_of_it_and_the_reminder_is_put_away() {
        let mut t = Task { subject: "x".into(), percent: 100, reminder: Some(NOW), ..Task::default() };
        let item = t.item(TODAY, NOW, 0);
        let back = Task::of(&item, 0);
        assert_eq!(back.status, Status::Complete);
        assert_eq!(back.completed, Some(TODAY));
        assert_eq!(back.reminder, None);
        t.percent = 10;
        assert_eq!(Task::of(&t.item(TODAY, NOW, 0), 0).status, Status::InProgress);
    }

    #[test]
    fn a_flagged_message_stands_on_the_to_do_list_and_comes_off_it_whole() {
        let mut m = Item::new_mail();
        m.set_subject("Invoice 42");
        let before = m.clone();
        let (start, due) = FollowUp::ThisWeek.days(TODAY);
        follow_up(&mut m, start, due, NOW, 60);
        assert_eq!(m.flag(), Flag::Flagged);
        let t = Task::of(&m, 60);
        assert!(t.flagged_mail);
        assert_eq!(t.subject, "Invoice 42");
        assert_eq!((t.start, t.due), (Some(TODAY), Some(TODAY + 1)));
        // Renamed on the to-do list, the message keeps its own subject.
        let mut renamed = t.clone();
        renamed.subject = "Pay invoice 42".into();
        renamed.write(&mut m, TODAY, NOW, 60);
        assert_eq!(m.subject(), "Invoice 42");
        assert_eq!(Task::of(&m, 60).subject, "Pay invoice 42");
        mark_complete(&mut m, TODAY, NOW);
        assert_eq!(m.flag(), Flag::Complete);
        assert!(Task::of(&m, 60).is_complete());
        clear_flag(&mut m);
        assert_eq!(m, before);
    }

    #[test]
    fn the_follow_up_menu_gives_the_work_week() {
        // Thursday: due Friday this week; next week Monday to Friday.
        assert_eq!(FollowUp::ThisWeek.days(TODAY), (Some(TODAY), Some(TODAY + 1)));
        assert_eq!(FollowUp::NextWeek.days(TODAY), (Some(TODAY + 4), Some(TODAY + 8)));
        // Saturday: this week's Friday is gone, so today.
        assert_eq!(FollowUp::ThisWeek.days(TODAY + 2), (Some(TODAY + 2), Some(TODAY + 2)));
        assert_eq!(FollowUp::Tomorrow.days(TODAY), (Some(TODAY + 1), Some(TODAY + 1)));
        assert_eq!(FollowUp::NoDate.days(TODAY), (None, None));
        for f in FollowUp::ALL {
            assert_eq!(FollowUp::parse(f.name()), Some(f));
            assert_eq!(FollowUp::parse(f.label()), Some(f));
        }
    }

    #[test]
    fn hostile_values_are_held_to_what_a_task_can_be() {
        let mut item = Item::default();
        item.props.set_str(pid::MESSAGE_CLASS, "IPM.Task");
        set_named(&mut item, PSETID_TASK, lid::STATUS, PropValue::Int32(i32::MIN));
        set_named(&mut item, PSETID_TASK, lid::PERCENT_COMPLETE, PropValue::Float64(f64::NAN));
        set_named(&mut item, PSETID_TASK, lid::DUE_DATE, PropValue::Time(i64::MAX));
        set_named(&mut item, PSETID_TASK, lid::START_DATE, PropValue::Time(i64::MIN));
        set_named(&mut item, PSETID_TASK, lid::ACTUAL_EFFORT, PropValue::Int32(i32::MAX));
        set_named(&mut item, PSETID_TASK, lid::OWNER, PropValue::Binary(vec![0; 9]));
        set_named(&mut item, pset::PSETID_COMMON, common::COMPANIES, PropValue::MultiString(vec!["x".repeat(9_000); 500]));
        item.props.set(pid::IMPORTANCE, PropValue::Int32(99));
        let t = Task::of(&item, 0);
        assert_eq!(t.status, Status::NotStarted);
        assert_eq!(t.percent, 0);
        assert_eq!((t.start, t.due), (None, None));
        assert_eq!(t.actual_work, MAX_WORK);
        assert_eq!(t.importance, 2);
        assert_eq!(t.companies.len(), MAX_COMPANIES);
        assert!(t.companies.iter().all(|c| c.chars().count() == MAX_FIELD));
        // Written back, it is a task as any other.
        let mut again = item.clone();
        t.write(&mut again, TODAY, NOW, 0);
        assert_eq!(Task::of(&again, 0).status, Status::NotStarted);
        // Days past what a FILETIME holds are not written.
        let far = Task { due: Some(i64::MAX), start: Some(i64::MIN), ..Task::default() };
        assert_eq!(Task::of(&far.item(TODAY, NOW, 0), 0).due, None);
    }

    #[test]
    fn statuses_are_named_both_ways() {
        for s in Status::ALL {
            assert_eq!(Status::parse(s.name()), Some(s));
            assert_eq!(Status::of_code(s.code()), s);
        }
    }
}
