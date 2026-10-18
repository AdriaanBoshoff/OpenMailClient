//! Appointments: what an item of a Calendar folder says of its time, place and state, read from
//! and written to the properties [MS-OXOCAL] gives them.
//!
//! An appointment's times are named properties of the appointment property set
//! (PidLidAppointmentStartWhole and PidLidAppointmentEndWhole, [MS-OXOCAL] 2.2.1.5 and 2.2.1.6),
//! in UTC. An item that lacks them is read from PidTagStartDate and PidTagEndDate
//! ([MS-OXPROPS] 2.1271 and 2.684), which Outlook writes beside them. What is written here is
//! what [MS-OXOCAL] 3.1.4.1 asks a client to set for an appointment that is not a meeting.
//!
//! A recurring appointment is one item with a pattern (`recur`), shown as one occurrence on
//! each day the pattern falls on. An occurrence changed on its own is an exception: kept in
//! the pattern and, as [MS-OXOCAL] 2.2.10 asks, as a hidden attached item of its own.

use crate::props::{NameId, NamedKey, PropValue, pid, pset};
use crate::recur::{self, Exception, Recurrence};
use crate::{Attachment, Item, time};

/// PSETID_Appointment {00062002-0000-0000-C000-000000000046} ([MS-OXPROPS] 1.3.2).
pub const PSETID_APPOINTMENT: [u8; 16] = [0x02, 0x20, 0x06, 0x00, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46];

/// The message class of an appointment ([MS-OXOCAL] 2.2.1).
pub const CLASS: &str = "IPM.Appointment";

/// Property ids of the appointment property set ([MS-OXOCAL] 2.2.1).
mod lid {
    pub const BUSY_STATUS: u32 = 0x8205;
    pub const LOCATION: u32 = 0x8208;
    pub const START_WHOLE: u32 = 0x820D;
    pub const END_WHOLE: u32 = 0x820E;
    pub const DURATION: u32 = 0x8213;
    pub const SUB_TYPE: u32 = 0x8215;
    pub const RECUR: u32 = 0x8216;
    pub const STATE_FLAGS: u32 = 0x8217;
    pub const RECURRING: u32 = 0x8223;
    pub const IS_RECURRING: u32 = 0x8E2C;
    pub const FEXCEPTIONAL_BODY: u32 = 0x8206;
    pub const EXCEPTION_REPLACE_TIME: u32 = 0x8228;
    pub const RECURRENCE_TYPE: u32 = 0x8231;
    pub const RECURRENCE_PATTERN: u32 = 0x8232;
    pub const TIME_ZONE_STRUCT: u32 = 0x8233;
    pub const TIME_ZONE_DESCRIPTION: u32 = 0x8234;
    pub const CLIP_START: u32 = 0x8235;
    pub const CLIP_END: u32 = 0x8236;
}

/// Properties of an exception's attachment ([MS-OXOCAL] 2.2.10.1, [MS-OXPROPS]).
mod att {
    pub const FLAGS: u16 = 0x3714;
    pub const EXCEPTION_REPLACE_TIME: u16 = 0x7FF9;
    pub const EXCEPTION_START_TIME: u16 = 0x7FFB;
    pub const EXCEPTION_END_TIME: u16 = 0x7FFC;
    /// afException: the attachment is a changed occurrence.
    pub const AF_EXCEPTION: i32 = 0x2;
    /// The message class of an exception's own item ([MS-OXOCAL] 2.2.10.1.1).
    pub const CLASS: &str = "IPM.OLE.CLASS.{00061055-0000-0000-C000-000000000046}";
}

/// [MS-OXOCAL] 2.2.1.14: the ClipEnd of a series that never ends, 31 August 4500 at 23:59.
const CLIP_NEVER: i64 = 0x0CB3_4557_A3DD_4000;

/// Property ids of the common property set ([MS-OXCMSG], [MS-OXORMDR]).
mod common {
    pub const REMINDER_DELTA: u32 = 0x8501;
    pub const REMINDER_TIME: u32 = 0x8502;
    pub const REMINDER_SET: u32 = 0x8503;
    pub const COMMON_START: u32 = 0x8516;
    pub const COMMON_END: u32 = 0x8517;
    pub const REMINDER_SIGNAL_TIME: u32 = 0x8560;
}

/// PidTagStartDate and PidTagEndDate.
const START_DATE: u16 = 0x0060;
const END_DATE: u16 = 0x0061;

/// One minute in FILETIME ticks.
pub const MINUTE: i64 = 60 * 10_000_000;
/// One day in FILETIME ticks.
pub const DAY: i64 = 24 * 60 * MINUTE;
/// An appointment lasts no longer than this many days here; a longer one is cut to it.
pub const MAX_DAYS: i64 = 5 * 366;

/// How the time is shown to others ([MS-OXOCAL] 2.2.1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Busy {
    Free,
    Tentative,
    #[default]
    Busy,
    OutOfOffice,
    WorkingElsewhere,
}

impl Busy {
    pub fn code(self) -> i32 {
        match self {
            Self::Free => 0,
            Self::Tentative => 1,
            Self::Busy => 2,
            Self::OutOfOffice => 3,
            Self::WorkingElsewhere => 4,
        }
    }

    pub fn of(code: i32) -> Self {
        match code {
            0 => Self::Free,
            1 => Self::Tentative,
            3 => Self::OutOfOffice,
            4 => Self::WorkingElsewhere,
            _ => Self::Busy,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Tentative => "tentative",
            Self::Busy => "busy",
            Self::OutOfOffice => "outOfOffice",
            Self::WorkingElsewhere => "workingElsewhere",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Free, Self::Tentative, Self::Busy, Self::OutOfOffice, Self::WorkingElsewhere].into_iter().find(|b| b.name().eq_ignore_ascii_case(s))
    }
}

/// What an appointment says of itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Appointment {
    pub subject: String,
    pub location: String,
    /// UTC FILETIME ticks; `end` is never before `start`.
    pub start: i64,
    pub end: i64,
    /// An all-day event: its times are midnights where it was made ([MS-OXOCAL] 2.2.1.9).
    pub all_day: bool,
    pub busy: Busy,
    /// Minutes before the start a reminder is given, when one is set.
    pub reminder: Option<i32>,
    /// It repeats. Its pattern is `recurrence` when it could be read; when it could not, only
    /// its first time is shown, and the pattern is kept as it was.
    pub recurring: bool,
    /// A meeting: it has attendees ([MS-OXOCAL] 2.2.1.10, asfMeeting).
    pub meeting: bool,
    /// How it repeats ([MS-OXOCAL] 2.2.1.44.5). On a series, `start` and `end` are those of its
    /// first occurrence.
    pub recurrence: Option<Recurrence>,
    /// For one occurrence of a series: the local day it was first planned on.
    pub occurrence: Option<i64>,
}

fn key(guid: [u8; 16], id: u32) -> NamedKey {
    NamedKey { guid, name: NameId::Id(id) }
}

fn named(item: &Item, guid: [u8; 16], id: u32) -> Option<&PropValue> {
    let want = key(guid, id);
    item.named.iter().find(|(k, _)| *k == want).map(|(_, v)| v)
}

fn set_named(item: &mut Item, guid: [u8; 16], id: u32, value: PropValue) {
    let want = key(guid, id);
    match item.named.iter_mut().find(|(k, _)| *k == want) {
        Some((_, v)) => *v = value,
        None => item.named.push((want, value)),
    }
}

fn remove_named(item: &mut Item, guid: [u8; 16], id: u32) {
    let want = key(guid, id);
    item.named.retain(|(k, _)| *k != want);
}

fn time_of(v: Option<&PropValue>) -> Option<i64> {
    match v {
        Some(PropValue::Time(t)) => Some(*t),
        _ => None,
    }
}

fn bool_of(v: Option<&PropValue>) -> Option<bool> {
    match v {
        Some(PropValue::Bool(b)) => Some(*b),
        Some(PropValue::Int32(n)) => Some(*n != 0),
        Some(PropValue::Int16(n)) => Some(*n != 0),
        _ => None,
    }
}

fn i32_of(v: Option<&PropValue>) -> Option<i32> {
    match v {
        Some(PropValue::Int32(n)) => Some(*n),
        Some(PropValue::Int16(n)) => Some(i32::from(*n)),
        _ => None,
    }
}

/// Whether an item is an appointment or a meeting, by its class.
pub fn is_appointment(item: &Item) -> bool {
    let class = item.message_class().to_ascii_lowercase();
    class == "ipm.appointment" || class.starts_with("ipm.appointment.")
}

impl Appointment {
    /// What `item` says of itself as an appointment; `None` when it gives no start.
    pub fn of(item: &Item) -> Option<Self> {
        let a = PSETID_APPOINTMENT;
        let start = time_of(named(item, a, lid::START_WHOLE)).or_else(|| item.props.time(START_DATE))?;
        let end = time_of(named(item, a, lid::END_WHOLE))
            .or_else(|| item.props.time(END_DATE))
            .or_else(|| i32_of(named(item, a, lid::DURATION)).map(|m| start.saturating_add(i64::from(m.max(0)).saturating_mul(MINUTE))))
            .unwrap_or(start);
        // A hostile or damaged end before its start, or years after it, is held to it.
        let end = end.clamp(start, start.saturating_add(MAX_DAYS.saturating_mul(DAY)));
        let location = match named(item, a, lid::LOCATION) {
            Some(PropValue::String(s)) => s.clone(),
            _ => String::new(),
        };
        let reminder = (bool_of(named(item, pset::PSETID_COMMON, common::REMINDER_SET)) == Some(true))
            .then(|| i32_of(named(item, pset::PSETID_COMMON, common::REMINDER_DELTA)).unwrap_or(15).clamp(0, 60 * 24 * 7 * 52));
        let recurring = bool_of(named(item, a, lid::RECURRING)).or_else(|| bool_of(named(item, a, lid::IS_RECURRING))).unwrap_or(false);
        let meeting = i32_of(named(item, a, lid::STATE_FLAGS)).is_some_and(|f| f & 1 != 0);
        let recurrence = match named(item, a, lid::RECUR) {
            Some(PropValue::Binary(b)) if recurring => Recurrence::read(b),
            _ => None,
        };
        Some(Self {
            subject: item.subject(),
            location,
            start,
            end,
            all_day: bool_of(named(item, a, lid::SUB_TYPE)).unwrap_or(false),
            busy: Busy::of(i32_of(named(item, a, lid::BUSY_STATUS)).unwrap_or(2)),
            reminder,
            recurring,
            meeting,
            recurrence,
            occurrence: None,
        })
    }

    /// Minutes the series' own time is ahead of UTC, as its first occurrence shows it: the
    /// pattern's times are local ([MS-OXOCAL] 2.2.1.44.5), the start is UTC. 0 when there is no
    /// pattern, or the two do not agree on a time zone that can be.
    /// Of the series itself, not one occurrence of it, which may have been moved.
    pub fn series_offset(&self) -> i64 {
        let Some(r) = &self.recurrence else { return 0 };
        let local = recur::minutes_of_day(r.start_day).saturating_add(i64::from(r.start_offset));
        let off = local.saturating_sub(self.start.div_euclid(MINUTE));
        if off.abs() <= 16 * 60 { off } else { 0 }
    }

    /// UTC FILETIME ticks of local minutes since 1601 in the series' time.
    fn utc(&self, local: i64) -> i64 {
        local.saturating_sub(self.series_offset()).saturating_mul(MINUTE)
    }

    /// Local minutes since 1601 in the series' time of UTC FILETIME ticks.
    fn local(&self, utc: i64) -> i64 {
        utc.div_euclid(MINUTE).saturating_add(self.series_offset())
    }

    /// The local day, in the series' time, that `utc` falls on.
    pub fn series_day(&self, utc: i64) -> i64 {
        recur::day_of_minutes(self.local(utc))
    }

    /// When the occurrence first planned on `day` was planned for, UTC; `None` when the series
    /// does not fall on that day.
    pub fn planned(&self, day: i64) -> Option<(i64, i64)> {
        let r = self.recurrence.as_ref()?;
        if !r.falls_on(day) {
            return None;
        }
        let start = self.utc(recur::minutes_of_day(day).saturating_add(i64::from(r.start_offset)));
        Some((start, start.saturating_add(self.end.saturating_sub(self.start))))
    }

    /// The occurrence first planned on `day` as it is now, changed or not; `None` when it was
    /// deleted or the series does not fall on that day.
    pub fn occurrence_of(&self, day: i64) -> Option<Self> {
        let r = self.recurrence.as_ref()?;
        let (from, to) = match r.exception_on(day) {
            Some(e) => return Some(self.changed(e)),
            None if r.taken_out(day) => return None,
            None => {
                let (start, end) = self.planned(day)?;
                (start, end.saturating_add(1))
            }
        };
        self.occurrences(from, to).into_iter().find(|o| o.occurrence == Some(day))
    }

    /// The occurrence `e` changed, as it is now.
    fn changed(&self, e: &Exception) -> Self {
        let mut a = self.instance(recur::day_of_minutes(e.original), self.utc(e.start), self.utc(e.end));
        if let Some(s) = &e.subject {
            a.subject = s.clone();
        }
        if let Some(l) = &e.location {
            a.location = l.clone();
        }
        if let Some(b) = e.busy {
            a.busy = Busy::of(b);
        }
        if let Some(all) = e.sub_type {
            a.all_day = all != 0;
        }
        a.reminder = match (e.reminder_set, e.reminder_delta) {
            (Some(false), _) => None,
            (Some(true), delta) => Some(delta.or(self.reminder).unwrap_or(15).clamp(0, 60 * 24 * 7 * 52)),
            (None, Some(delta)) if self.reminder.is_some() => Some(delta.clamp(0, 60 * 24 * 7 * 52)),
            (None, _) => self.reminder,
        };
        a
    }

    /// One occurrence of the series, first planned on `day`.
    fn instance(&self, day: i64, start: i64, end: i64) -> Self {
        let mut a = self.clone();
        a.occurrence = Some(day);
        a.recurring = true;
        a.start = start;
        a.end = end.clamp(start, start.saturating_add(MAX_DAYS.saturating_mul(DAY)));
        a
    }

    /// What is shown of it in `[from, to)`: itself, or each occurrence of a series as it falls
    /// now, earliest first, those deleted left out and those changed as they were changed.
    pub fn occurrences(&self, from: i64, to: i64) -> Vec<Self> {
        let Some(r) = &self.recurrence else {
            return if self.overlaps(from, to) { vec![self.clone()] } else { Vec::new() };
        };
        let length = self.end.saturating_sub(self.start);
        let first = self.series_day(from.saturating_sub(length));
        let last = self.series_day(to).saturating_add(1);
        let mut out = Vec::new();
        for day in r.days(first, last) {
            if r.taken_out(day) {
                continue;
            }
            let start = self.utc(recur::minutes_of_day(day).saturating_add(i64::from(r.start_offset)));
            let a = self.instance(day, start, start.saturating_add(length));
            if a.overlaps(from, to) {
                out.push(a);
            }
        }
        for e in &r.exceptions {
            // Only one that may fall in the window is made: making each is a copy of the series.
            let (start, end) = (self.utc(e.start), self.utc(e.end));
            if start >= to || (end <= from && !(end <= start && start >= from)) {
                continue;
            }
            let a = self.changed(e);
            if a.overlaps(from, to) {
                out.push(a);
            }
        }
        out.sort_by_key(|a| (a.start, std::cmp::Reverse(a.end)));
        out
    }

    /// The occurrence first planned on `day` changed to be as `to` is: its times, subject,
    /// place, how it is shown and its reminder, each kept as its own only where it differs
    /// from the series'. `false` when this is not a series that falls on `day`.
    pub fn change_occurrence(&mut self, day: i64, to: &Self) -> bool {
        let Some((planned, _)) = self.planned(day) else { return false };
        let (start, end, original) = (self.local(to.start), self.local(to.end.max(to.start)), self.local(planned));
        let differs = |a: &str, b: &str| (a != b).then(|| b.to_string());
        let e = Exception {
            start,
            end,
            original,
            subject: differs(&self.subject, &to.subject),
            location: differs(&self.location, &to.location),
            busy: (self.busy != to.busy).then(|| to.busy.code()),
            sub_type: (self.all_day != to.all_day).then(|| u32::from(to.all_day)),
            reminder_set: (self.reminder.is_some() != to.reminder.is_some()).then_some(to.reminder.is_some()),
            reminder_delta: to.reminder.filter(|m| Some(*m) != self.reminder),
            ..Exception::default()
        };
        match self.recurrence.as_mut() {
            Some(r) => {
                r.set_exception(e);
                true
            }
            None => false,
        }
    }

    /// The occurrence first planned on `day` deleted. `false` when this is not a series that
    /// falls on `day`.
    pub fn delete_occurrence(&mut self, day: i64) -> bool {
        if self.planned(day).is_none() {
            return false;
        }
        match self.recurrence.as_mut() {
            Some(r) => {
                r.delete_occurrence(day);
                true
            }
            None => false,
        }
    }

    /// Minutes it lasts.
    pub fn minutes(&self) -> i64 {
        self.end.saturating_sub(self.start) / MINUTE
    }

    /// Whether any of it falls in `[from, to)`. One with no length falls where it starts.
    pub fn overlaps(&self, from: i64, to: i64) -> bool {
        self.start < to && (self.end > from || (self.end == self.start && self.start >= from))
    }

    /// A new appointment item saying this. Anything else of `item` (its body, attachments and
    /// properties this program does not know) is kept as it was.
    pub fn write(&self, item: &mut Item) {
        let a = PSETID_APPOINTMENT;
        let c = pset::PSETID_COMMON;
        if !is_appointment(item) {
            item.props.set_str(pid::MESSAGE_CLASS, CLASS);
        }
        item.set_subject(&self.subject);
        item.props.set(START_DATE, PropValue::Time(self.start));
        item.props.set(END_DATE, PropValue::Time(self.end));
        set_named(item, a, lid::START_WHOLE, PropValue::Time(self.start));
        set_named(item, a, lid::END_WHOLE, PropValue::Time(self.end));
        set_named(item, c, common::COMMON_START, PropValue::Time(self.start));
        set_named(item, c, common::COMMON_END, PropValue::Time(self.end));
        set_named(item, a, lid::DURATION, PropValue::Int32(i32::try_from(self.minutes()).unwrap_or(i32::MAX)));
        set_named(item, a, lid::LOCATION, PropValue::String(self.location.clone()));
        set_named(item, a, lid::SUB_TYPE, PropValue::Bool(self.all_day));
        set_named(item, a, lid::BUSY_STATUS, PropValue::Int32(self.busy.code()));
        if named(item, a, lid::STATE_FLAGS).is_none() {
            set_named(item, a, lid::STATE_FLAGS, PropValue::Int32(0));
        }
        match &self.recurrence {
            Some(r) => self.write_recurrence(item, r),
            // A pattern not read here is kept as it was.
            None if self.recurring && named(item, a, lid::RECUR).is_some() => {}
            None => {
                for id in [lid::RECUR, lid::RECURRENCE_TYPE, lid::RECURRENCE_PATTERN, lid::CLIP_START, lid::CLIP_END] {
                    remove_named(item, a, id);
                }
                set_named(item, a, lid::RECURRING, PropValue::Bool(false));
                item.attachments.retain(|at| !is_exception(at));
            }
        }
        match self.reminder {
            Some(minutes) => {
                let minutes = minutes.clamp(0, 60 * 24 * 7 * 52);
                let at = self.start.saturating_sub(i64::from(minutes).saturating_mul(MINUTE));
                set_named(item, c, common::REMINDER_SET, PropValue::Bool(true));
                set_named(item, c, common::REMINDER_DELTA, PropValue::Int32(minutes));
                set_named(item, c, common::REMINDER_TIME, PropValue::Time(self.start));
                set_named(item, c, common::REMINDER_SIGNAL_TIME, PropValue::Time(at));
            }
            None => {
                set_named(item, c, common::REMINDER_SET, PropValue::Bool(false));
                remove_named(item, c, common::REMINDER_SIGNAL_TIME);
            }
        }
    }

    /// The pattern and what [MS-OXOCAL] 3.1.4.1.2 asks be set beside it, and an attached item
    /// for each changed occurrence ([MS-OXOCAL] 2.2.10.1), those of occurrences no longer
    /// changed taken away.
    fn write_recurrence(&self, item: &mut Item, r: &Recurrence) {
        let a = PSETID_APPOINTMENT;
        let off = self.series_offset();
        set_named(item, a, lid::RECUR, PropValue::Binary(r.write()));
        set_named(item, a, lid::RECURRING, PropValue::Bool(true));
        set_named(item, a, lid::RECURRENCE_TYPE, PropValue::Int32(r.freq.recurrence_type()));
        set_named(item, a, lid::RECURRENCE_PATTERN, PropValue::String(self.pattern_words()));
        set_named(item, a, lid::CLIP_START, PropValue::Time(self.utc(recur::minutes_of_day(r.start_day))));
        let clip_end = match r.end {
            recur::End::Never => CLIP_NEVER,
            _ => self.utc(recur::minutes_of_day(r.end_day)),
        };
        set_named(item, a, lid::CLIP_END, PropValue::Time(clip_end));
        // The zone the pattern's times are in, as a TimeZoneStruct ([MS-OXOCAL] 2.2.1.39) with
        // no daylight saving: only where the item does not already say which zone it is.
        if named(item, a, lid::TIME_ZONE_STRUCT).is_none() {
            let bias = i32::try_from(-off).unwrap_or(0);
            let mut tz = Vec::with_capacity(48);
            tz.extend_from_slice(&bias.to_le_bytes());
            tz.extend_from_slice(&[0u8; 44]);
            set_named(item, a, lid::TIME_ZONE_STRUCT, PropValue::Binary(tz));
            let (h, m) = (off.abs() / 60, off.abs() % 60);
            let sign = if off < 0 { '-' } else { '+' };
            set_named(item, a, lid::TIME_ZONE_DESCRIPTION, PropValue::String(format!("(UTC{sign}{h:02}:{m:02})")));
        }
        // An attachment for each changed occurrence, found again by when it was first planned.
        let replaced: Vec<i64> = r.exceptions.iter().map(|e| self.utc(e.original)).collect();
        item.attachments.retain(|at| !is_exception(at) || at.props.time(att::EXCEPTION_REPLACE_TIME).is_some_and(|t| replaced.contains(&t)));
        for (e, at_time) in r.exceptions.iter().zip(&replaced) {
            let shown = self.changed(e);
            let found = item.attachments.iter().position(|at| is_exception(at) && at.props.time(att::EXCEPTION_REPLACE_TIME) == Some(*at_time));
            let at = match found.and_then(|i| item.attachments.get_mut(i)) {
                Some(at) => at,
                None => {
                    item.attachments.push(Attachment::default());
                    let Some(at) = item.attachments.last_mut() else { continue };
                    at
                }
            };
            at.props.set(pid::ATTACH_METHOD, PropValue::Int32(5));
            at.props.set(pid::ATTACHMENT_HIDDEN, PropValue::Bool(true));
            at.props.set(att::FLAGS, PropValue::Int32(att::AF_EXCEPTION));
            at.props.set(pid::RENDERING_POSITION, PropValue::Int32(-1));
            at.props.set_str(pid::DISPLAY_NAME, &shown.subject);
            at.props.set(att::EXCEPTION_REPLACE_TIME, PropValue::Time(*at_time));
            // [MS-OXOCAL] 2.2.10.1.2 and 2.2.10.1.3: in the series' own time.
            at.props.set(att::EXCEPTION_START_TIME, PropValue::Time(e.start.saturating_mul(MINUTE)));
            at.props.set(att::EXCEPTION_END_TIME, PropValue::Time(e.end.saturating_mul(MINUTE)));
            let mut inner = at.embedded.take().map_or_else(Item::default, |b| *b);
            inner.props.set_str(pid::MESSAGE_CLASS, att::CLASS);
            let mut one = shown.clone();
            one.recurrence = None;
            one.recurring = false;
            one.occurrence = None;
            one.write(&mut inner);
            inner.props.set_str(pid::MESSAGE_CLASS, att::CLASS);
            set_named(&mut inner, a, lid::EXCEPTION_REPLACE_TIME, PropValue::Time(*at_time));
            set_named(&mut inner, a, lid::FEXCEPTIONAL_BODY, PropValue::Bool(e.exceptional_body));
            at.embedded = Some(Box::new(inner));
        }
    }

    /// The pattern as PidLidRecurrencePattern says it ([MS-OXOCAL] 2.2.1.46).
    pub fn pattern_words(&self) -> String {
        let Some(r) = &self.recurrence else { return String::new() };
        // The series' own times, kept in its pattern: an occurrence changed on its own may start
        // at another.
        let clock = |minutes: u32| (minutes / 60 % 24, minutes % 60);
        let (first, last) = (clock(r.start_offset), clock(r.end_offset));
        let date = |d: i64| {
            let c = time::civil(time::from_unix(d.saturating_mul(86_400)), 0);
            format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
        };
        let mut words = format!("Occurs {} effective {}", r.words(), date(r.start_day));
        match r.end {
            recur::End::Never => {}
            recur::End::After(n) => words.push_str(&format!(" {n} times")),
            recur::End::On(d) => words.push_str(&format!(" until {}", date(d))),
        }
        if !self.all_day {
            words.push_str(&format!(" from {:02}:{:02} to {:02}:{:02}", first.0, first.1, last.0, last.1));
        }
        words
    }

    /// A new appointment item.
    pub fn item(&self) -> Item {
        let mut it = Item::default();
        it.props.set_str(pid::MESSAGE_CLASS, CLASS);
        it.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(crate::MSGFLAG_READ));
        it.props.set(pid::IMPORTANCE, PropValue::Int32(1));
        self.write(&mut it);
        it
    }
}

/// PSETID_Meeting {6ED8DA90-450B-101B-98DA-00AA003F1305} ([MS-OXPROPS] 1.3.2).
pub const PSETID_MEETING: [u8; 16] = [0x90, 0xDA, 0xD8, 0x6E, 0x0B, 0x45, 0x1B, 0x10, 0x98, 0xDA, 0x00, 0xAA, 0x00, 0x3F, 0x13, 0x05];

/// PidLidGlobalObjectId and PidLidCleanGlobalObjectId ([MS-OXOCAL] 2.2.1.27 and 2.2.1.28).
mod meeting {
    pub const GLOBAL_OBJECT_ID: u32 = 0x0003;
    pub const CLEAN_GLOBAL_OBJECT_ID: u32 = 0x0023;
}

/// The Byte Array ID that begins every GlobalObjectId ([MS-OXOCAL] 2.2.1.27).
const GOID_HEAD: [u8; 16] = [0x04, 0x00, 0x00, 0x00, 0x82, 0x00, 0xE0, 0x00, 0x74, 0xC5, 0xB7, 0x10, 0x1A, 0x82, 0xE0, 0x08];
/// What the data of a GlobalObjectId made from an iCalendar UID begins with
/// ([MS-OXCICAL] 2.1.3.1.1.20.26): "vCal-Uid" and the version 1.
const VCAL_UID: &[u8; 12] = b"vCal-Uid\x01\x00\x00\x00";
/// Longest UID kept, in bytes.
pub const MAX_UID: usize = 1_000;

/// A GlobalObjectId ([MS-OXOCAL] 2.2.1.27) with no instance date and no creation time: the
/// head, the date, the creation time, the reserved bytes, the size, then `data`.
fn goid(data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(40 + data.len());
    v.extend_from_slice(&GOID_HEAD);
    v.extend_from_slice(&[0u8; 4 + 8 + 8]);
    v.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_le_bytes());
    v.extend_from_slice(data);
    v
}

/// The id an appointment keeps for the whole of its life, by which a copy of it elsewhere (a
/// calendar file, a server, a meeting request) is known again: PidLidCleanGlobalObjectId, or
/// PidLidGlobalObjectId with its instance date taken out ([MS-OXOCAL] 2.2.1.28).
pub fn global_id(item: &Item) -> Option<Vec<u8>> {
    let read = |id| match named(item, PSETID_MEETING, id) {
        Some(PropValue::Binary(b)) if b.len() >= 40 && b.starts_with(&GOID_HEAD) => Some(b.clone()),
        _ => None,
    };
    if let Some(clean) = read(meeting::CLEAN_GLOBAL_OBJECT_ID) {
        return Some(clean);
    }
    let mut id = read(meeting::GLOBAL_OBJECT_ID)?;
    for b in id.get_mut(16..20).into_iter().flatten() {
        *b = 0;
    }
    Some(id)
}

/// `id` kept as the appointment's own, as both PidLidGlobalObjectId and
/// PidLidCleanGlobalObjectId: an appointment that is not one occurrence has no instance date
/// in either.
pub fn set_global_id(item: &mut Item, id: &[u8]) {
    set_named(item, PSETID_MEETING, meeting::GLOBAL_OBJECT_ID, PropValue::Binary(id.to_vec()));
    set_named(item, PSETID_MEETING, meeting::CLEAN_GLOBAL_OBJECT_ID, PropValue::Binary(id.to_vec()));
}

/// A new GlobalObjectId, made at `now` (FILETIME ticks): sixteen bytes of data from `now` and
/// `seed`, as Outlook's own are sixteen bytes that are not repeated.
pub fn new_global_id(now: i64, seed: u64) -> Vec<u8> {
    let mut data = [0u8; 16];
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (i, b) in data.iter_mut().enumerate() {
        for x in now.to_le_bytes().iter().chain(seed.to_le_bytes().iter()).chain(std::iter::once(&(i as u8))) {
            h = (h ^ u64::from(*x)).wrapping_mul(0x0100_0000_01b3);
        }
        *b = (h >> 24) as u8;
    }
    let mut id = goid(&data);
    if let Some(at) = id.get_mut(24..32) {
        at.copy_from_slice(&now.to_le_bytes());
    }
    id
}

/// The iCalendar UID of a GlobalObjectId ([MS-OXCICAL] 2.1.3.1.1.20.26): the UID it was made
/// from, when it was made from one; else its bytes in hexadecimal.
pub fn uid_of_global_id(id: &[u8]) -> String {
    let data = id.get(40..).unwrap_or_default();
    if let Some(rest) = data.strip_prefix(VCAL_UID.as_slice()) {
        let text = rest.split(|b| *b == 0).next().unwrap_or_default();
        if let Ok(t) = std::str::from_utf8(text)
            && !t.is_empty()
        {
            return t.to_string();
        }
    }
    id.iter().map(|b| format!("{b:02X}")).collect()
}

/// The GlobalObjectId of an iCalendar UID ([MS-OXCICAL] 2.1.3.1.1.20.26): the bytes it spells
/// when it is one written in hexadecimal, its instance date taken out; else one that wraps it.
pub fn global_id_of_uid(uid: &str) -> Vec<u8> {
    let hex = uid.trim();
    if hex.len() >= 80 && hex.len().is_multiple_of(2) && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        let bytes: Vec<u8> = (0..hex.len() / 2).filter_map(|i| hex.get(i * 2..i * 2 + 2)).filter_map(|h| u8::from_str_radix(h, 16).ok()).collect();
        if bytes.starts_with(&GOID_HEAD) {
            let mut bytes = bytes;
            for b in bytes.get_mut(16..20).into_iter().flatten() {
                *b = 0;
            }
            return bytes;
        }
    }
    let mut end = uid.len().min(MAX_UID);
    while !uid.is_char_boundary(end) {
        end -= 1;
    }
    let mut data = VCAL_UID.to_vec();
    data.extend_from_slice(uid.get(..end).unwrap_or_default().as_bytes());
    data.push(0);
    goid(&data)
}

/// Whether an attachment is a changed occurrence of its series ([MS-OXOCAL] 2.2.10.1.1).
pub fn is_exception(at: &Attachment) -> bool {
    at.props.i32(att::FLAGS).is_some_and(|f| f & att::AF_EXCEPTION != 0)
        || at.embedded.as_ref().is_some_and(|e| e.message_class().eq_ignore_ascii_case(att::CLASS))
}

/// How many days a view shows, and from which.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum View {
    Day,
    /// Monday to Friday, as Outlook's default work week is.
    WorkWeek,
    #[default]
    Week,
    Month,
}

impl View {
    pub fn name(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::WorkWeek => "workWeek",
            Self::Week => "week",
            Self::Month => "month",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Day, Self::WorkWeek, Self::Week, Self::Month].into_iter().find(|v| v.name().eq_ignore_ascii_case(s))
    }

    /// The days shown (local day numbers, days since 1970-01-01) when `day` is the day chosen
    /// and weeks begin on `week_start` (0 = Sunday).
    pub fn days(self, day: i64, week_start: u32) -> Vec<i64> {
        let weekday = |d: i64| (d.rem_euclid(7) + 4).rem_euclid(7) as u32;
        let begun = |d: i64| d.saturating_sub(i64::from((weekday(d) + 7 - week_start % 7) % 7));
        match self {
            Self::Day => vec![day],
            Self::Week => (0..7).map(|i| begun(day).saturating_add(i)).collect(),
            Self::WorkWeek => {
                // Monday to Friday of the week `day` is in, whichever day weeks begin on.
                let monday = day.saturating_sub(i64::from((weekday(day) + 6) % 7));
                (0..5).map(|i| monday.saturating_add(i)).collect()
            }
            Self::Month => {
                let c = time::civil(day_start(day, 0), 0);
                let first = day.saturating_sub(i64::from(c.day.saturating_sub(1)));
                let from = begun(first);
                (0..42).map(|i| from.saturating_add(i)).collect()
            }
        }
    }

    /// The day chosen after moving one view forward (`by` 1) or back (-1).
    pub fn step(self, day: i64, by: i64) -> i64 {
        match self {
            Self::Day => day.saturating_add(by),
            Self::WorkWeek | Self::Week => day.saturating_add(by.saturating_mul(7)),
            Self::Month => {
                let c = time::civil(day_start(day, 0), 0);
                let months = c.year.saturating_mul(12).saturating_add(i64::from(c.month) - 1).saturating_add(by);
                let (year, month) = (months.div_euclid(12), months.rem_euclid(12) as u32 + 1);
                let last = days_in(year, month);
                time::to_unix(time::from_civil(year, month, c.day.min(last), 0, 0, 0, 0)).div_euclid(86_400)
            }
        }
    }
}

/// Days in a month.
pub fn days_in(year: i64, month: u32) -> u32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// UTC FILETIME ticks at the local midnight beginning day number `day`.
pub fn day_start(day: i64, offset_minutes: i32) -> i64 {
    time::from_unix(day.saturating_mul(86_400).saturating_sub(i64::from(offset_minutes).saturating_mul(60)))
}

/// The local day number `filetime` falls on.
pub fn day_of(filetime: i64, offset_minutes: i32) -> i64 {
    time::civil(filetime, offset_minutes).day_number
}

/// A local date and time read from text: `YYYY-MM-DD`, then optionally `HH:MM` after a space or
/// a `T`. `None` for anything else, or a date that does not exist.
pub fn parse_local(text: &str, offset_minutes: i32) -> Option<i64> {
    let text = text.trim();
    let (date, clock) = match text.split_once(['T', ' ']) {
        Some((d, c)) => (d, Some(c.trim())),
        None => (text, None),
    };
    let mut parts = date.splitn(3, '-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if !(1601..=9999).contains(&year) || !(1..=12).contains(&month) || day == 0 || day > days_in(year, month) {
        return None;
    }
    let (hour, minute) = match clock {
        Some(c) => {
            let c = c.trim_end_matches('Z');
            let mut hm = c.splitn(3, ':');
            let h: u32 = hm.next()?.parse().ok()?;
            let m: u32 = hm.next()?.parse().ok()?;
            if h > 23 || m > 59 {
                return None;
            }
            (h, m)
        }
        None => (0, 0),
    };
    Some(time::from_civil(year, month, day, hour, minute, 0, offset_minutes))
}

/// Appointments laid side by side where they overlap, as a day of a calendar shows them: for
/// each, the column it stands in and how many columns its group of overlapping ones needs.
/// `spans` are (start, end) pairs; one with no length is given a short one so it can be seen.
pub fn columns(spans: &[(i64, i64)], least: i64) -> Vec<(usize, usize)> {
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by_key(|&i| spans.get(i).map(|s| (s.0, std::cmp::Reverse(s.1))));
    let end_of = |i: usize| spans.get(i).map_or(0, |&(s, e)| e.max(s.saturating_add(least)));
    let mut out = vec![(0usize, 1usize); spans.len()];
    // A group goes on while some appointment in it has not yet ended.
    let mut group: Vec<usize> = Vec::new();
    let mut ends: Vec<i64> = Vec::new();
    let mut group_end = i64::MIN;
    let close = |group: &mut Vec<usize>, ends: &mut Vec<i64>, out: &mut Vec<(usize, usize)>| {
        let n = ends.len().max(1);
        for &i in group.iter() {
            if let Some(slot) = out.get_mut(i) {
                slot.1 = n;
            }
        }
        group.clear();
        ends.clear();
    };
    for i in order {
        let start = spans.get(i).map_or(0, |s| s.0);
        if start >= group_end && !group.is_empty() {
            close(&mut group, &mut ends, &mut out);
        }
        let column = match ends.iter().position(|&e| e <= start) {
            Some(c) => c,
            None => {
                ends.push(i64::MIN);
                ends.len() - 1
            }
        };
        if let Some(e) = ends.get_mut(column) {
            *e = end_of(i);
        }
        if let Some(slot) = out.get_mut(i) {
            slot.0 = column;
        }
        group.push(i);
        group_end = if group.len() == 1 { end_of(i) } else { group_end.max(end_of(i)) };
    }
    close(&mut group, &mut ends, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> i64 {
        parse_local(text, 0).unwrap()
    }

    #[test]
    fn an_icalendar_uid_and_a_global_object_id_are_each_made_from_the_other() {
        let id = global_id_of_uid("abc-123@example.test");
        assert!(id.starts_with(&GOID_HEAD));
        assert_eq!(uid_of_global_id(&id), "abc-123@example.test");
        let mut item = Item::default();
        set_global_id(&mut item, &id);
        assert_eq!(global_id(&item), Some(id));
        // An Outlook-made id goes out as hexadecimal and comes back as the same bytes.
        let made = new_global_id(at("2026-10-18 09:00"), 7);
        assert_ne!(made, new_global_id(at("2026-10-18 09:00"), 8));
        let hex = uid_of_global_id(&made);
        assert!(hex.starts_with("040000008200E00074C5B7101A82E008"));
        assert_eq!(global_id_of_uid(&hex), made);
        // One occurrence's id is known by its series' once its date is taken out.
        let mut one = made.clone();
        one[16..20].copy_from_slice(&[0x07, 0xEA, 10, 18]);
        let mut item = Item::default();
        set_named(&mut item, PSETID_MEETING, meeting::GLOBAL_OBJECT_ID, PropValue::Binary(one));
        assert_eq!(global_id(&item), Some(made));
        // Hostile ones: too long, not a GlobalObjectId, cut short.
        assert_eq!(uid_of_global_id(&global_id_of_uid(&"é".repeat(MAX_UID))).len(), MAX_UID);
        assert_eq!(uid_of_global_id(&[1, 2]), "0102");
        assert!(global_id_of_uid(&"04".repeat(50)).starts_with(&GOID_HEAD));
    }

    #[test]
    fn an_appointment_is_written_and_read_back_as_mapi_lays_it_out() {
        let a = Appointment {
            subject: "Dentist".into(),
            location: "12 High Street".into(),
            start: at("2026-10-20 09:30"),
            end: at("2026-10-20 10:15"),
            all_day: false,
            busy: Busy::OutOfOffice,
            reminder: Some(30),
            recurring: false,
            meeting: false,
            recurrence: None,
            occurrence: None,
        };
        let item = a.item();
        assert!(is_appointment(&item));
        assert_eq!(item.props.time(START_DATE), Some(a.start));
        assert_eq!(named(&item, PSETID_APPOINTMENT, lid::DURATION), Some(&PropValue::Int32(45)));
        assert_eq!(Appointment::of(&item), Some(a.clone()));
        // Written again, nothing is doubled.
        let mut again = item.clone();
        a.write(&mut again);
        assert_eq!(again.named.len(), item.named.len());
        let mut no_reminder = a.clone();
        no_reminder.reminder = None;
        no_reminder.write(&mut again);
        assert_eq!(Appointment::of(&again).unwrap().reminder, None);
    }

    #[test]
    fn an_item_with_only_the_plain_dates_or_odd_values_is_read_safely() {
        let mut it = Item::default();
        it.props.set_str(pid::MESSAGE_CLASS, "IPM.Appointment");
        assert!(Appointment::of(&it).is_none());
        it.props.set(START_DATE, PropValue::Time(at("2026-01-01 10:00")));
        it.props.set(END_DATE, PropValue::Time(at("2025-01-01 10:00")));
        let a = Appointment::of(&it).unwrap();
        assert_eq!(a.end, a.start, "an end before the start is held to it");
        it.props.set(END_DATE, PropValue::Time(i64::MAX));
        assert_eq!(Appointment::of(&it).unwrap().end, a.start + MAX_DAYS * DAY);
        set_named(&mut it, PSETID_APPOINTMENT, lid::BUSY_STATUS, PropValue::Int32(99));
        set_named(&mut it, PSETID_APPOINTMENT, lid::LOCATION, PropValue::Int32(5));
        let a = Appointment::of(&it).unwrap();
        assert_eq!((a.busy, a.location.as_str()), (Busy::Busy, ""));
    }

    #[test]
    fn views_show_the_days_outlook_shows() {
        let day = |t: &str| day_of(at(t), 0);
        let wed = day("2026-10-21");
        assert_eq!(View::Day.days(wed, 0), vec![wed]);
        assert_eq!(View::Week.days(wed, 0).first(), Some(&day("2026-10-18")), "a week from Sunday");
        assert_eq!(View::Week.days(wed, 1).first(), Some(&day("2026-10-19")), "or from Monday");
        assert_eq!(View::WorkWeek.days(day("2026-10-18"), 0), (0..5).map(|i| day("2026-10-12") + i).collect::<Vec<_>>());
        let month = View::Month.days(wed, 0);
        assert_eq!((month.len(), month[0]), (42, day("2026-09-27")));
        assert_eq!(View::Month.step(day("2026-01-31"), 1), day("2026-02-28"));
        assert_eq!(View::Month.step(day("2026-01-15"), -1), day("2025-12-15"));
        assert_eq!(View::Week.step(wed, -1), wed - 7);
    }

    #[test]
    fn local_times_are_read_and_nonsense_is_not() {
        assert_eq!(parse_local("2026-10-20T09:30", 120), Some(time::from_civil(2026, 10, 20, 7, 30, 0, 0)));
        for bad in ["", "2026-13-01", "2026-02-30", "2026-10-20 24:00", "20261020", "2026-10-20 9", "x-y-z", "99999-01-01"] {
            assert!(parse_local(bad, 0).is_none(), "{bad}");
        }
        assert!(parse_local("2028-02-29", 0).is_some());
    }

    /// A weekly series on Tuesdays at 09:00 for half an hour, at UTC+02:00, from 2026-10-06.
    fn weekly() -> Appointment {
        let off = 120;
        let start = parse_local("2026-10-06 09:00", off).unwrap();
        let day = day_of(start, off);
        let r = Recurrence::new(recur::Freq::Weekly, recur::Rule::Week { days: 1 << 2 }, 1, recur::End::After(5), day, 540, 570, 1);
        Appointment {
            subject: "Stand-up".into(),
            location: "Room 1".into(),
            start,
            end: start + 30 * MINUTE,
            all_day: false,
            busy: Busy::Busy,
            reminder: Some(10),
            recurring: true,
            meeting: false,
            recurrence: Some(r),
            occurrence: None,
        }
    }

    #[test]
    fn a_series_is_shown_on_each_day_it_falls_on() {
        let off = 120;
        let a = weekly();
        assert_eq!(a.series_offset(), 120);
        let all = a.occurrences(parse_local("2026-10-01", off).unwrap(), parse_local("2026-12-01", off).unwrap());
        let days: Vec<i64> = all.iter().map(|o| day_of(o.start, off)).collect();
        let tue = |d: &str| day_of(at(&format!("{d} 12:00")), 0);
        assert_eq!(days, ["2026-10-06", "2026-10-13", "2026-10-20", "2026-10-27", "2026-11-03"].map(tue).to_vec(), "five, as it ends");
        assert!(all.iter().all(|o| o.recurring && o.occurrence == Some(day_of(o.start, off)) && o.minutes() == 30));
        assert_eq!(time::civil(all[2].start, off).hour, 9, "at nine where it was made");
        // Only those in the range, one ending at its start is not in it.
        let one = a.occurrences(parse_local("2026-10-13 09:30", off).unwrap(), parse_local("2026-10-20 09:01", off).unwrap());
        assert_eq!(one.len(), 1);
        // Read back from the item as written.
        let item = a.item();
        let back = Appointment::of(&item).unwrap();
        assert_eq!(back, a);
        assert!(
            back.pattern_words().starts_with("Occurs every week on Tuesday effective 2026-10-06 5 times from 09:00 to 09:30"),
            "{}",
            back.pattern_words()
        );
        assert!(named(&item, PSETID_APPOINTMENT, lid::TIME_ZONE_STRUCT).is_some());
    }

    #[test]
    fn a_series_with_the_most_changed_occurrences_all_at_one_time_is_written_and_shown_quickly() {
        // Regression: each changed occurrence was found by making every other one, a copy of
        // the series each, so writing this took over a minute.
        let started = std::time::Instant::now();
        let mut a = weekly();
        let first = a.recurrence.as_ref().unwrap().start_day;
        a.recurrence = Some(Recurrence::new(recur::Freq::Daily, recur::Rule::Day, 1, recur::End::Never, first, 540, 570, 1));
        let mut to = a.clone();
        to.recurrence = None;
        to.subject = "All at once".into();
        for i in 0..i64::from(recur::MAX_COUNT) {
            assert!(a.change_occurrence(first + i, &to));
        }
        let item = a.item();
        let back = Appointment::of(&item).unwrap();
        assert_eq!(back.recurrence.as_ref().unwrap().exceptions.len(), recur::MAX_COUNT as usize);
        assert_eq!(back.occurrences(a.start, a.end).len(), recur::MAX_COUNT as usize);
        assert_eq!(back.occurrence_of(first + 500).unwrap().subject, "All at once");
        assert!(started.elapsed().as_secs() < 20, "{:?}", started.elapsed());
    }

    #[test]
    fn one_occurrence_is_changed_or_deleted_and_the_rest_are_not() {
        let off = 120;
        let mut a = weekly();
        let second = day_of(parse_local("2026-10-13", off).unwrap(), off);
        let third = second + 7;
        let mut moved = a.occurrences(parse_local("2026-10-13", off).unwrap(), parse_local("2026-10-14", off).unwrap()).remove(0);
        moved.start = parse_local("2026-10-14 15:00", off).unwrap();
        moved.end = moved.start + 60 * MINUTE;
        moved.subject = "Stand-up, moved".into();
        assert!(a.change_occurrence(second, &moved));
        assert!(a.delete_occurrence(third));
        assert!(!a.delete_occurrence(second + 1), "not a day it falls on");
        let mut item = a.item();
        let shown = Appointment::of(&item).unwrap().occurrences(parse_local("2026-10-01", off).unwrap(), parse_local("2026-12-01", off).unwrap());
        let said: Vec<(String, u32, &str)> =
            shown.iter().map(|o| (format!("{}", time::civil(o.start, off).day), time::civil(o.start, off).hour, o.subject.as_str())).collect();
        assert_eq!(
            said,
            [("6".into(), 9, "Stand-up"), ("14".into(), 15, "Stand-up, moved"), ("27".into(), 9, "Stand-up"), ("3".into(), 9, "Stand-up")]
        );
        assert_eq!(shown[1].location, "Room 1", "what was not changed is the series'");
        assert_eq!(shown[1].occurrence, Some(second));
        // The changed one has a hidden attached item of its own, as Outlook keeps it.
        let ex: Vec<&Attachment> = item.attachments.iter().filter(|at| is_exception(at)).collect();
        assert_eq!(ex.len(), 1);
        let inner = ex[0].embedded.as_ref().unwrap();
        assert_eq!(inner.subject(), "Stand-up, moved");
        assert_eq!(ex[0].props.time(att::EXCEPTION_REPLACE_TIME), a.planned(second).map(|p| p.0));
        assert_eq!(Appointment::of(inner).map(|i| i.start), Some(shown[1].start));
        // Changed back to a plain appointment, the pattern and the attachment go.
        let mut plain = Appointment::of(&item).unwrap();
        plain.recurrence = None;
        plain.recurring = false;
        plain.write(&mut item);
        assert!(item.attachments.is_empty());
        assert_eq!(Appointment::of(&item).unwrap().occurrences(0, i64::MAX).len(), 1);
    }

    #[test]
    fn a_pattern_that_cannot_be_read_is_kept_and_shown_once() {
        let mut item = weekly().item();
        set_named(&mut item, PSETID_APPOINTMENT, lid::RECUR, PropValue::Binary(vec![0xFF; 7]));
        let a = Appointment::of(&item).unwrap();
        assert!(a.recurring && a.recurrence.is_none());
        assert_eq!(a.occurrences(0, i64::MAX).len(), 1);
        a.write(&mut item);
        assert_eq!(named(&item, PSETID_APPOINTMENT, lid::RECUR), Some(&PropValue::Binary(vec![0xFF; 7])));
        // A start that does not agree with its pattern on any time zone is taken as UTC.
        let mut odd = weekly();
        odd.start += 5 * DAY;
        assert_eq!(odd.series_offset(), 0);
        assert!(odd.occurrences(i64::MIN, i64::MAX).len() <= 5);
    }

    #[test]
    fn overlapping_appointments_stand_side_by_side() {
        let h = |n: i64| n * 60 * MINUTE;
        let spans = [(h(9), h(11)), (h(10), h(12)), (h(10), h(10)), (h(13), h(14)), (h(11), h(12))];
        let cols = columns(&spans, h(1) / 2);
        assert_eq!(cols[0], (0, 3));
        assert_eq!(cols[1].1, 3);
        assert_ne!(cols[1].0, cols[2].0);
        assert_eq!(cols[4], (0, 3), "the first column is free again at 11");
        assert_eq!(cols[3], (0, 1));
        assert!(columns(&[], 1).is_empty());
    }
}
