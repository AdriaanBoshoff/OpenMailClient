//! Reminders of appointments, tasks and messages flagged for follow-up, as [MS-OXORMDR] keeps
//! them: PidLidReminderSet says one is set, PidLidReminderSignalTime when it is given, and
//! PidLidReminderTime (with PidLidReminderDelta on an appointment) what it is for (2.2.1).
//!
//! A reminder is due once its signal time has come. Snoozing it puts the signal time later
//! (3.1.4.6.2); dismissing one of a single item takes the reminder away, and dismissing one of
//! a series puts the signal time on its next occurrence (3.1.4.6.1), so each occurrence is
//! reminded of in its turn and none before it again.

use crate::Item;
use crate::calendar::{self, Appointment, MINUTE, bool_of, named, set_named, time_of};
use crate::props::{PropValue, pset};
use crate::task::{self, Task};
use crate::zone::Zone;

/// PSETID_Common properties of reminders ([MS-OXPROPS] 2.218, 2.220, 2.221, 2.222).
mod lid {
    pub const REMINDER_TIME: u32 = 0x8502;
    pub const REMINDER_SET: u32 = 0x8503;
    pub const REMINDER_SIGNAL_TIME: u32 = 0x8560;
}

const DAY: i64 = 24 * 60 * MINUTE;
/// How far about a series' reminder time the occurrence it is for is looked for: two years,
/// so that a yearly series is found.
const LOOK: i64 = 2 * 366 * DAY;
/// The most minutes a reminder is given before an appointment (a year), as `Appointment` reads.
const MAX_BEFORE: i64 = 60 * 24 * 7 * 52;
/// Snoozed no longer than this many minutes: a week.
pub const MAX_SNOOZE_MIN: i64 = 7 * 24 * 60;

/// What a reminder is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Appointment,
    Task,
    /// A message flagged for follow-up.
    Message,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Appointment => "appointment",
            Self::Task => "task",
            Self::Message => "message",
        }
    }
}

/// When what is reminded of is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    /// The start of an appointment, or of the occurrence of a series reminded of, UTC ticks.
    At(i64),
    /// The day a task or a flagged message is due (days since 1970-01-01).
    Day(i64),
    /// No time or day is given.
    Unknown,
}

/// A reminder set on an item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reminder {
    pub kind: Kind,
    pub subject: String,
    /// Where an appointment is; empty for others.
    pub location: String,
    /// When it is given: once this has come, it is due. UTC ticks.
    pub signal: i64,
    pub due: Due,
    /// For an occurrence of a series, the local day it was first planned on.
    pub occurrence: Option<i64>,
}

impl Reminder {
    /// Whether it is due at `now`.
    pub fn is_due(&self, now: i64) -> bool {
        self.signal <= now
    }
}

fn signal_time(item: &Item) -> Option<i64> {
    time_of(named(item, pset::PSETID_COMMON, lid::REMINDER_SIGNAL_TIME)).filter(|t| *t > 0)
}

fn reminder_set(item: &Item) -> bool {
    bool_of(named(item, pset::PSETID_COMMON, lid::REMINDER_SET)) == Some(true)
}

/// The reminder set on `item`, read as of `now`, or `None` when it has none: none is set, a
/// task is done, or a series has no occurrence left to remind of. `here` is the zone days are
/// read in.
pub fn of(item: &Item, now: i64, here: &Zone) -> Option<Reminder> {
    if !reminder_set(item) {
        return None;
    }
    if calendar::is_appointment(item) {
        let a = Appointment::of(item)?;
        return appointment(item, &a, now);
    }
    let is_task = task::is_task(item);
    if !is_task && !task::is_flagged(item) {
        return None;
    }
    let t = Task::of(item, here);
    if t.is_complete() {
        return None;
    }
    // A snoozed one is given at its signal time, else at its reminder time.
    let signal = signal_time(item).or(t.reminder)?;
    Some(Reminder {
        kind: if is_task { Kind::Task } else { Kind::Message },
        subject: t.subject,
        location: String::new(),
        signal,
        due: t.due.map_or(Due::Unknown, Due::Day),
        occurrence: None,
    })
}

/// An appointment's reminder time: so many minutes before it starts.
fn own(a: &Appointment) -> Option<i64> {
    a.reminder.map(|m| a.start.saturating_sub(i64::from(m).clamp(0, MAX_BEFORE).saturating_mul(MINUTE)))
}

/// The occurrences of a series whose reminder times fall in `[from, to)`, each with it,
/// earliest first.
fn reminded_between(a: &Appointment, from: i64, to: i64) -> Vec<(i64, Appointment)> {
    // An occurrence starts up to so long after its reminder.
    let before = i64::from(a.reminder.unwrap_or(0)).clamp(0, MAX_BEFORE).saturating_mul(MINUTE).saturating_add(DAY);
    let mut list: Vec<_> = a
        .occurrences(from, to.saturating_add(before))
        .into_iter()
        .filter_map(|o| Some((own(&o)?, o)))
        .filter(|(at, _)| (from..to).contains(at))
        .collect();
    list.sort_by_key(|(at, o)| (*at, o.start));
    list
}

/// An appointment's reminder. A single one's is for itself. A series' is for the latest
/// occurrence whose own reminder time has come by its signal time or by now, whichever is
/// later: one dismissed waits for the next, one snoozed stays with the one it was, and one
/// left untouched for a while is for the latest occurrence, not the first.
fn appointment(item: &Item, a: &Appointment, now: i64) -> Option<Reminder> {
    let stored = signal_time(item);
    let (at, o) = if a.recurrence.is_none() {
        (stored.or_else(|| own(a))?, a.clone())
    } else {
        let by = stored.map_or(now, |s| s.max(now));
        let came = reminded_between(a, by.saturating_sub(LOOK), by.saturating_add(1)).pop();
        // None has come yet: the next one.
        let (at, o) = came.or_else(|| reminded_between(a, by, by.saturating_add(LOOK)).into_iter().next())?;
        (stored.map_or(at, |s| s.max(at)), o)
    };
    Some(Reminder {
        kind: Kind::Appointment,
        subject: o.subject.clone(),
        location: o.location.clone(),
        signal: at,
        due: Due::At(o.start),
        occurrence: o.occurrence,
    })
}

/// Dismisses the reminder of `item` ([MS-OXORMDR] 3.1.4.6.1): taken away from a single item,
/// put on the next occurrence of a series, or taken away when none is left. Returns whether
/// one was set.
pub fn dismiss(item: &mut Item, now: i64, here: &Zone) -> bool {
    let Some(r) = of(item, now, here) else { return false };
    let c = pset::PSETID_COMMON;
    let series = if calendar::is_appointment(item) { Appointment::of(item).filter(|a| a.recurrence.is_some()) } else { None };
    // The next after the one dismissed: its own reminder time, not a snoozed one, is counted from.
    let next = series.and_then(|a| {
        let Due::At(start) = r.due else { return None };
        let at = own(&Appointment { start, ..a.clone() })?;
        reminded_between(&a, at.saturating_add(1), at.saturating_add(LOOK)).into_iter().next()
    });
    match next {
        Some((at, o)) => {
            set_named(item, c, lid::REMINDER_SIGNAL_TIME, PropValue::Time(at));
            set_named(item, c, lid::REMINDER_TIME, PropValue::Time(o.start));
        }
        None => set_named(item, c, lid::REMINDER_SET, PropValue::Bool(false)),
    }
    true
}

/// Snoozes the reminder of `item` until `until` ([MS-OXORMDR] 3.1.4.6.2): it is given again
/// then, never before now nor more than a week on, and what it is for stays as it was. Returns
/// whether one was set.
pub fn snooze(item: &mut Item, until: i64, now: i64, here: &Zone) -> bool {
    if of(item, now, here).is_none() {
        return false;
    }
    let until = until.clamp(now, now.saturating_add(MAX_SNOOZE_MIN.saturating_mul(MINUTE)));
    set_named(item, pset::PSETID_COMMON, lid::REMINDER_SIGNAL_TIME, PropValue::Time(until));
    true
}

/// How long until `due` from `now` (on day `today`), or how long past it, as the Reminders
/// window says it: "Now", "In 15 minutes", "2 hours overdue", "Tomorrow".
pub fn due_in(due: Due, now: i64, today: i64) -> String {
    let plural = |n: i64, what: &str| if n == 1 { format!("1 {what}") } else { format!("{n} {what}s") };
    let span = |minutes: i64| match minutes {
        m if m < 60 => plural(m, "minute"),
        m if m < 24 * 60 => plural(m / 60, "hour"),
        m if m < 7 * 24 * 60 => plural(m / (24 * 60), "day"),
        m => plural(m / (7 * 24 * 60), "week"),
    };
    match due {
        Due::At(at) => match at.saturating_sub(now) / MINUTE {
            0 => "Now".into(),
            m if m > 0 => format!("In {}", span(m)),
            m => format!("{} overdue", span(m.saturating_neg())),
        },
        Due::Day(day) => match day.saturating_sub(today) {
            0 => "Today".into(),
            1 => "Tomorrow".into(),
            d if d > 0 => format!("In {}", span(d.saturating_mul(24 * 60))),
            d => format!("{} overdue", span(d.saturating_neg().saturating_mul(24 * 60))),
        },
        Due::Unknown => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recur::{self, Recurrence};
    use crate::time;

    const DAY0: i64 = 20_388;

    fn at(day: i64, hour: i64) -> i64 {
        time::from_unix(day * 86_400 + hour * 3_600)
    }

    fn appt(start: i64, reminder: Option<i32>) -> Item {
        Appointment { subject: "Review".into(), start, end: start + 30 * MINUTE, reminder, ..Appointment::default() }.item()
    }

    #[test]
    fn a_single_appointment_is_due_its_minutes_before_and_dismissed_for_good() {
        let utc = Zone::utc();
        let mut item = appt(at(DAY0, 9), Some(15));
        let r = of(&item, at(DAY0, 8), &utc).unwrap();
        assert_eq!((r.kind, r.signal, r.due), (Kind::Appointment, at(DAY0, 9) - 15 * MINUTE, Due::At(at(DAY0, 9))));
        assert!(!r.is_due(at(DAY0, 8)));
        assert!(r.is_due(at(DAY0, 9) - 15 * MINUTE));
        assert!(snooze(&mut item, at(DAY0, 9) - 10 * MINUTE, at(DAY0, 9) - 15 * MINUTE, &utc));
        let r = of(&item, at(DAY0, 9) - 15 * MINUTE, &utc).unwrap();
        assert_eq!((r.signal, r.due), (at(DAY0, 9) - 10 * MINUTE, Due::At(at(DAY0, 9))), "snoozed, still for the same");
        assert!(dismiss(&mut item, at(DAY0, 9), &utc));
        assert_eq!(of(&item, at(DAY0, 9), &utc), None);
        assert!(!dismiss(&mut item, at(DAY0, 9), &utc), "nothing left to dismiss");
        assert!(!snooze(&mut item, at(DAY0, 10), at(DAY0, 9), &utc));
        assert_eq!(of(&appt(at(DAY0, 9), None), at(DAY0, 9), &utc), None);
    }

    #[test]
    fn a_series_reminds_of_each_occurrence_in_its_turn() {
        let utc = Zone::utc();
        let mut a = Appointment {
            subject: "Standup".into(),
            start: at(DAY0, 9),
            end: at(DAY0, 9) + 30 * MINUTE,
            reminder: Some(15),
            ..Appointment::default()
        };
        a.recurring = true;
        a.recurrence = Some(Recurrence::new(recur::Freq::Daily, recur::Rule::Day, 1, recur::End::Never, DAY0, 540, 570, 1));
        let mut item = a.item();
        // Left untouched for days: the latest occurrence whose time came, not the first.
        let now = at(DAY0 + 3, 12);
        let r = of(&item, now, &utc).unwrap();
        assert_eq!(r.due, Due::At(at(DAY0 + 3, 9)));
        assert!(r.is_due(now));
        // Dismissed: the next day's, not yet due.
        assert!(dismiss(&mut item, now, &utc));
        let r = of(&item, now, &utc).unwrap();
        assert_eq!((r.due, r.signal), (Due::At(at(DAY0 + 4, 9)), at(DAY0 + 4, 9) - 15 * MINUTE));
        assert!(!r.is_due(now));
        // Its time comes; snoozed past the start, it stays with that occurrence.
        let then = at(DAY0 + 4, 9) - 15 * MINUTE;
        assert!(snooze(&mut item, then + 20 * MINUTE, then, &utc));
        let r = of(&item, then + 5 * MINUTE, &utc).unwrap();
        assert_eq!((r.due, r.is_due(then + 5 * MINUTE)), (Due::At(at(DAY0 + 4, 9)), false));
        assert!(of(&item, then + 20 * MINUTE, &utc).unwrap().is_due(then + 20 * MINUTE));
        assert!(dismiss(&mut item, then + 20 * MINUTE, &utc));
        assert_eq!(of(&item, then + 20 * MINUTE, &utc).unwrap().due, Due::At(at(DAY0 + 5, 9)));
    }

    #[test]
    fn the_last_occurrence_dismissed_leaves_no_reminder() {
        let utc = Zone::utc();
        let mut a = Appointment { subject: "Course".into(), start: at(DAY0, 9), end: at(DAY0, 10), reminder: Some(0), ..Appointment::default() };
        a.recurring = true;
        a.recurrence = Some(Recurrence::new(recur::Freq::Daily, recur::Rule::Day, 1, recur::End::After(2), DAY0, 540, 600, 1));
        let mut item = a.item();
        let now = at(DAY0 + 1, 9);
        assert_eq!(of(&item, now, &utc).unwrap().due, Due::At(at(DAY0 + 1, 9)));
        assert!(dismiss(&mut item, now, &utc));
        assert_eq!(of(&item, at(DAY0 + 30, 9), &utc), None);
    }

    #[test]
    fn a_task_reminds_until_it_is_done_and_a_flagged_message_too() {
        let utc = Zone::utc();
        let t = Task { subject: "Report".into(), due: Some(DAY0 + 1), reminder: Some(at(DAY0, 9)), importance: 1, ..Task::default() };
        let mut item = t.item(DAY0, at(DAY0, 8), &utc);
        let r = of(&item, at(DAY0, 9), &utc).unwrap();
        assert_eq!((r.kind, r.signal, r.due, r.subject.as_str()), (Kind::Task, at(DAY0, 9), Due::Day(DAY0 + 1), "Report"));
        assert!(snooze(&mut item, at(DAY0, 10), at(DAY0, 9), &utc));
        assert_eq!(of(&item, at(DAY0, 9), &utc).unwrap().signal, at(DAY0, 10));
        task::mark_complete(&mut item, DAY0, at(DAY0, 9));
        assert_eq!(of(&item, at(DAY0, 11), &utc), None, "done");

        let mut mail = Item::default();
        mail.props.set_str(crate::props::pid::MESSAGE_CLASS, "IPM.Note");
        mail.set_subject("Please answer");
        task::follow_up(&mut mail, Some(DAY0), Some(DAY0), at(DAY0, 8), &utc);
        assert_eq!(of(&mail, at(DAY0, 9), &utc), None, "flagged without a reminder");
        let mut t = Task::of(&mail, &utc);
        t.reminder = Some(at(DAY0, 16));
        t.write(&mut mail, DAY0, at(DAY0, 8), &utc);
        let r = of(&mail, at(DAY0, 9), &utc).unwrap();
        assert_eq!((r.kind, r.signal, r.due), (Kind::Message, at(DAY0, 16), Due::Day(DAY0)));
        assert!(dismiss(&mut mail, at(DAY0, 16), &utc));
        assert_eq!(of(&mail, at(DAY0, 17), &utc), None);
        assert!(task::is_flagged(&mail), "dismissed, still flagged");
    }

    #[test]
    fn a_snooze_is_held_to_a_week_and_never_before_now() {
        let utc = Zone::utc();
        let mut item = appt(at(DAY0, 9), Some(0));
        assert!(snooze(&mut item, i64::MAX, at(DAY0, 9), &utc));
        assert_eq!(of(&item, at(DAY0, 9), &utc).unwrap().signal, at(DAY0, 9) + MAX_SNOOZE_MIN * MINUTE);
        assert!(snooze(&mut item, i64::MIN, at(DAY0, 9), &utc));
        assert_eq!(of(&item, at(DAY0, 9), &utc).unwrap().signal, at(DAY0, 9));
    }

    #[test]
    fn how_long_is_said_plainly() {
        let now = at(DAY0, 9);
        assert_eq!(due_in(Due::At(now), now, DAY0), "Now");
        assert_eq!(due_in(Due::At(now + 15 * MINUTE), now, DAY0), "In 15 minutes");
        assert_eq!(due_in(Due::At(now - 61 * MINUTE), now, DAY0), "1 hour overdue");
        assert_eq!(due_in(Due::At(now - 3 * DAY), now, DAY0), "3 days overdue");
        assert_eq!(due_in(Due::At(now - 15 * DAY), now, DAY0), "2 weeks overdue");
        assert_eq!(due_in(Due::Day(DAY0 + 1), now, DAY0), "Tomorrow");
        assert_eq!(due_in(Due::Day(DAY0 - 2), now, DAY0), "2 days overdue");
        assert_eq!(due_in(Due::Unknown, now, DAY0), "");
        assert!(!due_in(Due::At(i64::MIN), i64::MAX, 0).is_empty());
        assert!(!due_in(Due::Day(i64::MIN), 0, i64::MAX).is_empty());
    }
}
