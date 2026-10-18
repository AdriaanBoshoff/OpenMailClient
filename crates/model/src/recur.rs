//! Recurrence patterns: when a recurring appointment falls, and how single occurrences of it
//! were changed or deleted. Read from and written to PidLidAppointmentRecur, the
//! AppointmentRecurrencePattern structure of [MS-OXOCAL] 2.2.1.44.5, whose first part is a
//! RecurrencePattern ([MS-OXOCAL] 2.2.1.44.1) and whose changed occurrences are ExceptionInfo
//! and ExtendedException structures ([MS-OXOCAL] 2.2.1.44.2 and 2.2.1.44.4).
//!
//! Every time in the pattern is local, in the series' own time zone, as minutes since midnight
//! of 1 January 1601. Here a local day is a day number, days since 1970-01-01, as in
//! `calendar`. The bytes come from strangers: every count is held to what the bytes can hold,
//! and every walk over the days of a series is bounded.

use crate::codepage;

/// Days from 1601-01-01 to 1970-01-01.
const DAYS_1601_1970: i64 = 134_774;
/// Minutes in a day.
const DAY_MIN: i64 = 1440;
/// [MS-OXOCAL] 2.2.1.44.1: the EndDate of a series that never ends (31 December 4500, 23:59).
pub const NO_END: u32 = 0x5AE9_80DF;
/// The most occurrences a series is given when it ends after a number of them, as Outlook's
/// own limit is.
pub const MAX_COUNT: u32 = 999;
/// The most occurrences one walk over a series gives.
pub const MAX_WALK: usize = 20_000;

/// How often a series repeats ([MS-OXOCAL] 2.2.1.44.1, RecurFrequency).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Freq {
    fn code(self) -> u16 {
        match self {
            Self::Daily => 0x200A,
            Self::Weekly => 0x200B,
            Self::Monthly => 0x200C,
            Self::Yearly => 0x200D,
        }
    }

    fn of(code: u16) -> Option<Self> {
        match code {
            0x200A => Some(Self::Daily),
            0x200B => Some(Self::Weekly),
            0x200C => Some(Self::Monthly),
            0x200D => Some(Self::Yearly),
            _ => None,
        }
    }

    /// PidLidRecurrenceType ([MS-OXOCAL] 2.2.1.45): 1 daily, 2 weekly, 3 monthly, 4 yearly.
    pub fn recurrence_type(self) -> i32 {
        match self {
            Self::Daily => 1,
            Self::Weekly => 2,
            Self::Monthly => 3,
            Self::Yearly => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::Yearly => "yearly",
        }
    }
}

/// On which days of each period a series falls ([MS-OXOCAL] 2.2.1.44.1, PatternType and
/// PatternTypeSpecific, 2.2.1.44.1.3 to 2.2.1.44.1.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rule {
    /// Every day of the period.
    Day,
    /// On the weekdays of `days`, bit 0 being Sunday.
    Week { days: u8 },
    /// On day `day` of the month, or its last day where the month is shorter.
    Month { day: u32 },
    /// On the `nth` (1 to 4, or 5 for the last) of the days of the month in `days`.
    MonthNth { days: u8, nth: u32 },
    /// On the last day of the month.
    MonthEnd,
    /// A pattern of the Hijri calendar, or one not known here: kept as it was, and shown on
    /// the first day of the series only.
    Other { kind: u16, specific: Vec<u8> },
}

/// When a series ends ([MS-OXOCAL] 2.2.1.44.1, EndType).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Never,
    /// After this many occurrences.
    After(u32),
    /// On or before this local day.
    On(i64),
}

/// One occurrence changed ([MS-OXOCAL] 2.2.1.44.2 and 2.2.1.44.4). Times are local minutes
/// since 1601; a field that is `None` is as the series has it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exception {
    pub start: i64,
    pub end: i64,
    /// When the occurrence would have started.
    pub original: i64,
    pub subject: Option<String>,
    pub meeting_type: Option<u32>,
    pub reminder_delta: Option<i32>,
    pub reminder_set: Option<bool>,
    pub location: Option<String>,
    pub busy: Option<i32>,
    pub attachment: Option<u32>,
    pub sub_type: Option<u32>,
    pub color: Option<u32>,
    /// ARO_EXCEPTIONAL_BODY: the occurrence's own message has a body of its own.
    pub exceptional_body: bool,
    /// PidLidChangeHighlight of the occurrence, when the writer gave it.
    pub change_highlight: Option<u32>,
}

/// A recurring appointment's pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recurrence {
    pub freq: Freq,
    pub rule: Rule,
    /// Days for a daily series (stored as minutes), weeks for a weekly one and months for a
    /// monthly or yearly one; at least 1.
    pub period: u32,
    pub calendar_type: u16,
    /// Used by tasks only ([MS-OXOTASK]); kept as it was.
    pub sliding: u32,
    pub end: End,
    /// OccurrenceCount as written.
    pub count: u32,
    /// The day weeks begin on, 0 being Sunday.
    pub first_dow: u32,
    /// The original local days of occurrences deleted or changed, earliest first.
    pub deleted: Vec<i64>,
    /// The local days changed occurrences now fall on, earliest first.
    pub modified: Vec<i64>,
    /// The local day of the first occurrence, and of the last (`NO_END`'s day when it never ends).
    pub start_day: i64,
    pub end_day: i64,
    /// Minutes after its local midnight each occurrence starts and ends.
    pub start_offset: u32,
    pub end_offset: u32,
    pub writer2: u32,
    pub exceptions: Vec<Exception>,
}

/// The local day number of local minutes since 1601.
pub fn day_of_minutes(minutes: i64) -> i64 {
    minutes.div_euclid(DAY_MIN).saturating_sub(DAYS_1601_1970)
}

/// Local minutes since 1601 at the midnight beginning day number `day`.
pub fn minutes_of_day(day: i64) -> i64 {
    day.saturating_add(DAYS_1601_1970).saturating_mul(DAY_MIN)
}

/// The weekday of a day number, 0 being Sunday.
pub fn weekday(day: i64) -> u32 {
    (day.rem_euclid(7) as u32 + 4) % 7
}

/// The year and month (1 to 12) of a day number, and its day of the month.
fn ymd(day: i64) -> (i64, u32, u32) {
    let c = crate::time::civil(crate::time::from_unix(day.saturating_mul(86_400)), 0);
    (c.year, c.month, c.day)
}

/// The day number of a date.
fn day_at(year: i64, month: u32, day: u32) -> i64 {
    crate::time::to_unix(crate::time::from_civil(year, month, day, 0, 0, 0, 0)).div_euclid(86_400)
}

/// Months since January of year 0 of a day.
fn month_index(day: i64) -> i64 {
    let (y, m, _) = ymd(day);
    y.saturating_mul(12).saturating_add(i64::from(m) - 1)
}

fn of_month_index(mi: i64) -> (i64, u32) {
    (mi.div_euclid(12), mi.rem_euclid(12) as u32 + 1)
}

/// Bytes read from the front, every read checked.
struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.b.get(self.at..end)?;
        self.at = end;
        Some(s)
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).and_then(|s| s.try_into().ok()).map(u16::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).and_then(|s| s.try_into().ok()).map(u32::from_le_bytes)
    }
    fn left(&self) -> usize {
        self.b.len().saturating_sub(self.at)
    }
    /// A count of 4-byte values, held to what is left.
    fn dates(&mut self) -> Option<Vec<i64>> {
        let n = self.u32()? as usize;
        if n > self.left() / 4 {
            return None;
        }
        (0..n).map(|_| self.u32().map(|v| day_of_minutes(i64::from(v)))).collect()
    }
}

/// Bytes written at the end.
#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn minutes(&mut self, v: i64) {
        self.u32(u32::try_from(v.max(0)).unwrap_or(u32::MAX));
    }
}

/// Longest subject or location kept for one occurrence, in characters.
const MAX_TEXT: usize = 255;

/// Text as the 8-bit string ExceptionInfo holds: Windows-1252, a character it lacks as '?'.
fn narrow(s: &str) -> Vec<u8> {
    s.chars().take(MAX_TEXT).flat_map(|c| codepage::encode(1252, c.encode_utf8(&mut [0; 4])).unwrap_or_else(|| vec![b'?'])).collect()
}

impl Recurrence {
    /// A new series beginning on `start_day` (moved on to its first occurrence by `settle`).
    pub fn new(freq: Freq, rule: Rule, period: u32, end: End, start_day: i64, start_offset: u32, end_offset: u32, first_dow: u32) -> Self {
        let mut r = Self {
            freq,
            rule,
            period: period.max(1),
            calendar_type: 0,
            sliding: 0,
            end,
            count: 10,
            first_dow: first_dow % 7,
            deleted: Vec::new(),
            modified: Vec::new(),
            start_day,
            end_day: day_of_minutes(i64::from(NO_END)),
            start_offset,
            end_offset,
            writer2: 0x3009,
            exceptions: Vec::new(),
        };
        r.settle();
        r
    }

    /// Moves the start on to the first day the pattern falls on, and works out the last day
    /// and the count from how the series ends ([MS-OXOCAL] 2.2.1.44.1, OccurrenceCount and
    /// EndDate).
    pub fn settle(&mut self) {
        if let Some(first) = self.walk_days(self.start_day, self.start_day.saturating_add(800), 1, false).first() {
            self.start_day = *first;
        }
        match self.end {
            End::Never => {
                self.count = 10;
                self.end_day = day_of_minutes(i64::from(NO_END));
            }
            End::After(n) => {
                let n = n.clamp(1, MAX_COUNT);
                self.end = End::After(n);
                self.count = n;
                // The last of `n` lies within 999 periods of 99 months at most.
                let far = self.start_day.saturating_add(366 * 100 * 9);
                self.end_day = self.walk_days(self.start_day, far, n as usize, false).last().copied().unwrap_or(self.start_day);
            }
            End::On(day) => {
                let day = day.max(self.start_day);
                self.end = End::On(day);
                self.end_day = day;
                let n = self.walk_days(self.start_day, day.saturating_add(1), MAX_WALK, false).len();
                self.count = u32::try_from(n).unwrap_or(u32::MAX).max(1);
            }
        }
    }

    /// Reads PidLidAppointmentRecur. `None` for bytes that are not one, cut short, or that
    /// claim more than they hold.
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader { b: bytes, at: 0 };
        let (_reader, _writer) = (r.u16()?, r.u16()?);
        let freq = Freq::of(r.u16()?)?;
        let kind = r.u16()?;
        let calendar_type = r.u16()?;
        let _first = r.u32()?;
        let period = r.u32()?;
        let sliding = r.u32()?;
        let rule = match kind {
            0x0 => Rule::Day,
            0x1 => Rule::Week { days: (r.u32()? & 0x7F) as u8 },
            0x2 => Rule::Month { day: r.u32()?.clamp(1, 31) },
            0x3 => {
                let days = (r.u32()? & 0x7F) as u8;
                Rule::MonthNth { days, nth: r.u32()?.clamp(1, 5) }
            }
            0x4 => {
                let _day = r.u32()?;
                Rule::MonthEnd
            }
            0xB => Rule::Other { kind, specific: r.take(8)?.to_vec() },
            0xA | 0xC => Rule::Other { kind, specific: r.take(4)?.to_vec() },
            _ => return None,
        };
        // A Gregorian pattern of a calendar this program does not count in is shown once.
        let rule = if matches!(calendar_type, 0..=2 | 9..=12) { rule } else { Rule::Other { kind, specific: rule_specific(&rule) } };
        let end_type = r.u32()?;
        let count = r.u32()?;
        let first_dow = r.u32()? % 7;
        let deleted = r.dates()?;
        let modified = r.dates()?;
        let start_day = day_of_minutes(i64::from(r.u32()?));
        let end_raw = r.u32()?;
        let end_day = day_of_minutes(i64::from(end_raw));
        let end = match end_type {
            0x2021 => End::On(end_day),
            0x2022 => End::After(count.max(1)),
            _ => End::Never,
        };
        let period = match freq {
            Freq::Daily => (period / 1440).max(1),
            Freq::Yearly => 12,
            _ => period.max(1),
        };
        let _reader2 = r.u32()?;
        let writer2 = r.u32()?;
        let start_offset = r.u32()?;
        let end_offset = r.u32()?;
        let n = usize::from(r.u16()?);
        // Each ExceptionInfo is at least 14 bytes.
        if n > r.left() / 14 {
            return None;
        }
        let mut exceptions = Vec::with_capacity(n);
        for _ in 0..n {
            exceptions.push(read_exception(&mut r)?);
        }
        // What follows is newer writers' own; a pattern cut short there is read without it.
        if let Some(size) = r.u32()
            && r.take(size as usize).is_some()
        {
            for e in &mut exceptions {
                if read_extended(&mut r, e, writer2).is_none() {
                    break;
                }
            }
        }
        Some(Self {
            freq,
            rule,
            period,
            calendar_type,
            sliding,
            end,
            count,
            first_dow,
            deleted,
            modified,
            start_day,
            end_day,
            start_offset: start_offset.min(DAY_MIN as u32 - 1),
            end_offset: end_offset.min(DAY_MIN as u32 * 2 + 5 * 366 * DAY_MIN as u32),
            writer2,
            exceptions,
        })
    }

    /// The bytes of PidLidAppointmentRecur, written as [MS-OXOCAL] 2.2.1.44.5 gives them with
    /// WriterVersion2 0x3009.
    pub fn write(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u16(0x3004);
        w.u16(0x3004);
        w.u16(self.freq.code());
        let (kind, specific) = match &self.rule {
            Rule::Day => (0u16, Vec::new()),
            Rule::Week { .. } => (1, rule_specific(&self.rule)),
            Rule::Month { .. } => (2, rule_specific(&self.rule)),
            Rule::MonthNth { .. } => (3, rule_specific(&self.rule)),
            Rule::MonthEnd => (4, rule_specific(&self.rule)),
            Rule::Other { kind, specific } => (*kind, specific.clone()),
        };
        w.u16(kind);
        w.u16(self.calendar_type);
        w.minutes(self.first_date_time());
        w.u32(match self.freq {
            Freq::Daily => self.period.max(1).saturating_mul(1440),
            Freq::Yearly => 12,
            _ => self.period.max(1),
        });
        w.u32(self.sliding);
        w.0.extend_from_slice(&specific);
        w.u32(match self.end {
            End::On(_) => 0x2021,
            End::After(_) => 0x2022,
            End::Never => 0x2023,
        });
        w.u32(self.count);
        w.u32(self.first_dow % 7);
        let mut deleted = self.deleted.clone();
        deleted.sort_unstable();
        deleted.dedup();
        w.u32(u32::try_from(deleted.len()).unwrap_or(0));
        for d in &deleted {
            w.minutes(minutes_of_day(*d));
        }
        let mut modified = self.modified.clone();
        modified.sort_unstable();
        modified.dedup();
        w.u32(u32::try_from(modified.len()).unwrap_or(0));
        for d in &modified {
            w.minutes(minutes_of_day(*d));
        }
        w.minutes(minutes_of_day(self.start_day));
        w.u32(match self.end {
            End::Never => NO_END,
            _ => u32::try_from(minutes_of_day(self.end_day).max(0)).unwrap_or(NO_END),
        });
        w.u32(0x3006);
        w.u32(0x3009);
        w.u32(self.start_offset);
        w.u32(self.end_offset);
        let exceptions: Vec<&Exception> = self.exceptions.iter().take(usize::from(u16::MAX)).collect();
        w.u16(u16::try_from(exceptions.len()).unwrap_or(0));
        for e in &exceptions {
            write_exception(&mut w, e);
        }
        w.u32(0);
        for e in &exceptions {
            write_extended(&mut w, e);
        }
        w.u32(0);
        w.0
    }

    /// FirstDateTime ([MS-OXOCAL] 2.2.1.44.1.1).
    fn first_date_time(&self) -> i64 {
        let start = minutes_of_day(self.start_day);
        match self.freq {
            Freq::Daily => start.rem_euclid(i64::from(self.period.max(1)).saturating_mul(DAY_MIN)),
            Freq::Weekly => {
                let back = (i64::from(weekday(self.start_day)) - i64::from(self.first_dow % 7)).rem_euclid(7);
                minutes_of_day(self.start_day.saturating_sub(back)).rem_euclid(i64::from(self.period.max(1)).saturating_mul(7 * DAY_MIN))
            }
            Freq::Monthly | Freq::Yearly => {
                let months = month_index(self.start_day).saturating_sub(1601 * 12);
                let period = if self.freq == Freq::Yearly { 12 } else { i64::from(self.period.max(1)) };
                let (y, m) = of_month_index(1601 * 12 + months.rem_euclid(period));
                minutes_of_day(day_at(y, m, 1))
            }
        }
    }

    /// The day the pattern falls on in the month `mi`, if any.
    fn day_in_month(&self, mi: i64) -> Option<i64> {
        let (y, m) = of_month_index(mi);
        let last = crate::calendar::days_in(y, m);
        match &self.rule {
            Rule::Month { day } => Some(day_at(y, m, (*day).clamp(1, last))),
            Rule::MonthEnd => Some(day_at(y, m, last)),
            Rule::MonthNth { days, nth } => {
                let first = day_at(y, m, 1);
                let hits: Vec<i64> = (0..i64::from(last)).map(|i| first.saturating_add(i)).filter(|d| days & (1 << weekday(*d)) != 0).collect();
                if *nth >= 5 { hits.last().copied() } else { hits.get((*nth as usize).saturating_sub(1)).copied() }
            }
            Rule::Day | Rule::Week { .. } | Rule::Other { .. } => None,
        }
    }

    /// The days the pattern falls on in `[from, to)`, earliest first, at most `cap` of them,
    /// with each one's place in the series (0 for the first). Deleted occurrences are among
    /// them; how the series ends is kept to when `ended` is true.
    fn walk(&self, from: i64, to: i64, cap: usize, ended: bool) -> Vec<(u64, i64)> {
        let mut out = Vec::new();
        let start = self.start_day;
        let cap = cap.min(MAX_WALK);
        let last = match self.end {
            End::On(d) if ended => d,
            End::After(_) if ended => self.end_day.max(start),
            _ => i64::MAX,
        };
        let count = match self.end {
            End::After(n) if ended => u64::from(n),
            _ => u64::MAX,
        };
        let to = to.min(last.saturating_add(1));
        let from = from.max(start);
        if to <= from {
            return out;
        }
        let keep = |i: u64, d: i64, out: &mut Vec<(u64, i64)>| -> bool {
            if i >= count || d >= to || out.len() >= cap {
                return false;
            }
            if d >= from {
                out.push((i, d));
            }
            true
        };
        match (&self.rule, self.freq) {
            (Rule::Other { .. }, _) => {
                keep(0, start, &mut out);
            }
            (Rule::Day, _) => {
                let every = i64::from(self.period.max(1));
                let k0 = (from - start + every - 1).div_euclid(every).max(0);
                let mut k = k0;
                while keep(k as u64, start.saturating_add(k.saturating_mul(every)), &mut out) {
                    k = k.saturating_add(1);
                }
            }
            (Rule::Week { days }, _) => {
                let days = *days & 0x7F;
                if days == 0 {
                    return out;
                }
                let ws = start - (i64::from(weekday(start)) - i64::from(self.first_dow % 7)).rem_euclid(7);
                let span = 7 * i64::from(self.period.max(1));
                let per_week = u64::from(days.count_ones());
                let first_week = (0..7).filter(|o| days & (1 << weekday(ws + o)) != 0 && ws + o >= start).count() as u64;
                let mut w = (from - ws).div_euclid(span).max(0);
                'weeks: loop {
                    let begin = ws.saturating_add(w.saturating_mul(span));
                    if begin >= to {
                        break;
                    }
                    let mut i = if w == 0 { 0 } else { first_week.saturating_add((w as u64 - 1).saturating_mul(per_week)) };
                    for o in 0..7 {
                        let d = begin + o;
                        if days & (1 << weekday(d)) != 0 && d >= start {
                            if !keep(i, d, &mut out) {
                                break 'weeks;
                            }
                            i = i.saturating_add(1);
                        }
                    }
                    w = w.saturating_add(1);
                }
            }
            (_, Freq::Daily | Freq::Weekly) => {}
            (_, Freq::Monthly | Freq::Yearly) => {
                let every = if self.freq == Freq::Yearly { 12 } else { i64::from(self.period.max(1)) };
                let mi0 = month_index(start);
                let skip_first = self.day_in_month(mi0).is_none_or(|d| d < start);
                let mut k = (month_index(from) - mi0).div_euclid(every).saturating_sub(1).max(0);
                loop {
                    let mi = mi0.saturating_add(k.saturating_mul(every));
                    let (y, m) = of_month_index(mi);
                    if day_at(y, m, 1) >= to {
                        break;
                    }
                    if let Some(d) = self.day_in_month(mi).filter(|d| *d >= start) {
                        let i = (k as u64).saturating_sub(u64::from(skip_first));
                        if !keep(i, d, &mut out) {
                            break;
                        }
                    }
                    k = k.saturating_add(1);
                }
            }
        }
        out
    }

    fn walk_days(&self, from: i64, to: i64, cap: usize, ended: bool) -> Vec<i64> {
        self.walk(from, to, cap, ended).into_iter().map(|(_, d)| d).collect()
    }

    /// The local days the series falls on in `[from, to)`, earliest first, deleted and changed
    /// ones among them.
    pub fn days(&self, from: i64, to: i64) -> Vec<i64> {
        self.walk_days(from, to, MAX_WALK, true)
    }

    /// The occurrence first planned on `day` changed as `e` says, in the place of any change
    /// made to it before ([MS-OXOCAL] 2.2.1.44.5: its day among the deleted ones, the day it
    /// now falls on among the modified ones).
    pub fn set_exception(&mut self, e: Exception) {
        let day = day_of_minutes(e.original);
        self.exceptions.retain(|x| day_of_minutes(x.original) != day);
        self.exceptions.push(e);
        self.exceptions.sort_by_key(|x| x.start);
        if !self.deleted.contains(&day) {
            self.deleted.push(day);
            self.deleted.sort_unstable();
        }
        self.remark();
    }

    /// The occurrence first planned on `day` deleted, with any change made to it.
    pub fn delete_occurrence(&mut self, day: i64) {
        self.exceptions.retain(|x| day_of_minutes(x.original) != day);
        if !self.deleted.contains(&day) {
            self.deleted.push(day);
            self.deleted.sort_unstable();
        }
        self.remark();
    }

    /// Every occurrence as first planned again.
    pub fn clear_exceptions(&mut self) {
        self.exceptions.clear();
        self.deleted.clear();
        self.modified.clear();
    }

    fn remark(&mut self) {
        let mut days: Vec<i64> = self.exceptions.iter().map(|x| day_of_minutes(x.start)).collect();
        days.sort_unstable();
        days.dedup();
        self.modified = days;
    }

    /// Whether the occurrence of `day` was deleted or changed.
    pub fn taken_out(&self, day: i64) -> bool {
        self.deleted.contains(&day)
    }

    /// The change made to the occurrence that first fell on `day`.
    pub fn exception_on(&self, day: i64) -> Option<&Exception> {
        self.exceptions.iter().find(|e| day_of_minutes(e.original) == day)
    }

    /// Whether the series falls on `day`, as first planned.
    pub fn falls_on(&self, day: i64) -> bool {
        self.walk(day, day.saturating_add(1), 1, true).first().is_some_and(|(_, d)| *d == day)
    }

    /// The days before and after `day` the series falls on, as planned, deleted ones left out.
    pub fn around(&self, day: i64) -> (Option<i64>, Option<i64>) {
        let before = self
            .walk_days(day.saturating_sub(800), day, MAX_WALK, true)
            .into_iter()
            .rev()
            .find(|d| !self.deleted.contains(d) || self.exception_on(*d).is_some());
        let after = self
            .walk_days(day.saturating_add(1), day.saturating_add(800), MAX_WALK, true)
            .into_iter()
            .find(|d| !self.deleted.contains(d) || self.exception_on(*d).is_some());
        let moved = |d: i64| self.exception_on(d).map_or(d, |e| day_of_minutes(e.start));
        (before.map(moved), after.map(moved))
    }

    /// The pattern in words, as a sentence's start: "every weekday", "every 2 weeks on Monday
    /// and Wednesday", "day 15 of every month", "the last Friday of every 3 months",
    /// "every 7 October".
    pub fn words(&self) -> String {
        const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
        const NTH: [&str; 5] = ["first", "second", "third", "fourth", "last"];
        let list = |mask: u8| -> String {
            if mask == 0x3E {
                return "weekday".into();
            }
            if mask == 0x41 {
                return "weekend day".into();
            }
            if mask == 0x7F {
                return "day".into();
            }
            let names: Vec<&str> =
                (0..7).map(|i| (i + self.first_dow as usize) % 7).filter(|i| mask & (1 << i) != 0).filter_map(|i| DAYS.get(i).copied()).collect();
            match names.split_last() {
                Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
                Some((last, _)) => (*last).to_string(),
                None => String::new(),
            }
        };
        let every = |n: u32, one: &str| if n <= 1 { format!("every {one}") } else { format!("every {n} {one}s") };
        let (_, month, _) = ymd(self.start_day);
        let month_name = crate::time::civil(crate::time::from_civil(2001, month, 1, 0, 0, 0, 0), 0).month_name();
        let nth = |n: u32| NTH.get(n.clamp(1, 5) as usize - 1).copied().unwrap_or("last");
        match (&self.rule, self.freq) {
            (Rule::Day, _) => every(self.period, "day"),
            (Rule::Week { days }, _) if *days == 0x3E && self.period <= 1 => "every weekday".into(),
            (Rule::Week { days }, _) => format!("{} on {}", every(self.period, "week"), list(*days)),
            (Rule::Month { day }, Freq::Yearly) => format!("every {day} {month_name}"),
            (Rule::Month { day }, _) => format!("day {day} of {}", every(self.period, "month")),
            (Rule::MonthNth { days, nth: n }, Freq::Yearly) => format!("the {} {} of {month_name}", nth(*n), list(*days)),
            (Rule::MonthNth { days, nth: n }, _) => format!("the {} {} of {}", nth(*n), list(*days), every(self.period, "month")),
            (Rule::MonthEnd, Freq::Yearly) => format!("the last day of {month_name}"),
            (Rule::MonthEnd, _) => format!("the last day of {}", every(self.period, "month")),
            (Rule::Other { .. }, _) => "in a calendar not counted here".into(),
        }
    }
}

/// PatternTypeSpecific of a rule ([MS-OXOCAL] 2.2.1.44.1.3 to 2.2.1.44.1.6).
fn rule_specific(rule: &Rule) -> Vec<u8> {
    match rule {
        Rule::Day => Vec::new(),
        Rule::Week { days } => u32::from(*days).to_le_bytes().to_vec(),
        Rule::Month { day } => day.to_le_bytes().to_vec(),
        Rule::MonthNth { days, nth } => [u32::from(*days).to_le_bytes(), nth.to_le_bytes()].concat(),
        Rule::MonthEnd => 31u32.to_le_bytes().to_vec(),
        Rule::Other { specific, .. } => specific.clone(),
    }
}

mod aro {
    pub const SUBJECT: u16 = 0x0001;
    pub const MEETINGTYPE: u16 = 0x0002;
    pub const REMINDERDELTA: u16 = 0x0004;
    pub const REMINDER: u16 = 0x0008;
    pub const LOCATION: u16 = 0x0010;
    pub const BUSYSTATUS: u16 = 0x0020;
    pub const ATTACHMENT: u16 = 0x0040;
    pub const SUBTYPE: u16 = 0x0080;
    pub const APPTCOLOR: u16 = 0x0100;
    pub const EXCEPTIONAL_BODY: u16 = 0x0200;
}

fn flags_of(e: &Exception) -> u16 {
    let mut f = 0;
    let mut set = |on: bool, bit: u16| {
        if on {
            f |= bit;
        }
    };
    set(e.subject.is_some(), aro::SUBJECT);
    set(e.meeting_type.is_some(), aro::MEETINGTYPE);
    set(e.reminder_delta.is_some(), aro::REMINDERDELTA);
    set(e.reminder_set.is_some(), aro::REMINDER);
    set(e.location.is_some(), aro::LOCATION);
    set(e.busy.is_some(), aro::BUSYSTATUS);
    set(e.attachment.is_some(), aro::ATTACHMENT);
    set(e.sub_type.is_some(), aro::SUBTYPE);
    set(e.color.is_some(), aro::APPTCOLOR);
    set(e.exceptional_body, aro::EXCEPTIONAL_BODY);
    f
}

/// An 8-bit string of ExceptionInfo: a length one more than the second, then the second's
/// count of bytes.
fn read_narrow(r: &mut Reader) -> Option<String> {
    let _len = r.u16()?;
    let len2 = r.u16()?;
    Some(codepage::decode(1252, r.take(usize::from(len2))?))
}

fn read_exception(r: &mut Reader) -> Option<Exception> {
    let mut e = Exception { start: i64::from(r.u32()?), end: i64::from(r.u32()?), original: i64::from(r.u32()?), ..Exception::default() };
    let f = r.u16()?;
    if f & aro::SUBJECT != 0 {
        e.subject = Some(read_narrow(r)?);
    }
    if f & aro::MEETINGTYPE != 0 {
        e.meeting_type = Some(r.u32()?);
    }
    if f & aro::REMINDERDELTA != 0 {
        e.reminder_delta = Some(i32::try_from(r.u32()?).unwrap_or(i32::MAX));
    }
    if f & aro::REMINDER != 0 {
        e.reminder_set = Some(r.u32()? != 0);
    }
    if f & aro::LOCATION != 0 {
        e.location = Some(read_narrow(r)?);
    }
    if f & aro::BUSYSTATUS != 0 {
        e.busy = Some(i32::try_from(r.u32()?).unwrap_or(2));
    }
    if f & aro::ATTACHMENT != 0 {
        e.attachment = Some(r.u32()?);
    }
    if f & aro::SUBTYPE != 0 {
        e.sub_type = Some(r.u32()?);
    }
    if f & aro::APPTCOLOR != 0 {
        e.color = Some(r.u32()?);
    }
    e.exceptional_body = f & aro::EXCEPTIONAL_BODY != 0;
    e.end = e.end.max(e.start);
    Some(e)
}

/// The Unicode subject and location of ExtendedException, which stand for the 8-bit ones.
fn read_extended(r: &mut Reader, e: &mut Exception, writer2: u32) -> Option<()> {
    if writer2 >= 0x3009 {
        let size = r.u32()? as usize;
        let block = r.take(size)?;
        e.change_highlight = block.get(..4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes);
    }
    let reserved = r.u32()? as usize;
    r.take(reserved)?;
    if e.subject.is_some() || e.location.is_some() {
        let (_start, _end, _original) = (r.u32()?, r.u32()?, r.u32()?);
        if e.subject.is_some() {
            let n = usize::from(r.u16()?);
            e.subject = Some(codepage::utf16le(r.take(n.checked_mul(2)?)?));
        }
        if e.location.is_some() {
            let n = usize::from(r.u16()?);
            e.location = Some(codepage::utf16le(r.take(n.checked_mul(2)?)?));
        }
        let reserved = r.u32()? as usize;
        r.take(reserved)?;
    }
    Some(())
}

fn write_narrow(w: &mut Writer, s: &str) {
    let b = narrow(s);
    let n = u16::try_from(b.len()).unwrap_or(u16::MAX - 1);
    w.u16(n.saturating_add(1));
    w.u16(n);
    w.0.extend_from_slice(b.get(..usize::from(n)).unwrap_or(&b));
}

fn write_exception(w: &mut Writer, e: &Exception) {
    w.minutes(e.start);
    w.minutes(e.end);
    w.minutes(e.original);
    w.u16(flags_of(e));
    if let Some(s) = &e.subject {
        write_narrow(w, s);
    }
    if let Some(v) = e.meeting_type {
        w.u32(v);
    }
    if let Some(v) = e.reminder_delta {
        w.u32(u32::try_from(v.max(0)).unwrap_or(0));
    }
    if let Some(v) = e.reminder_set {
        w.u32(u32::from(v));
    }
    if let Some(s) = &e.location {
        write_narrow(w, s);
    }
    if let Some(v) = e.busy {
        w.u32(u32::try_from(v.max(0)).unwrap_or(2));
    }
    if let Some(v) = e.attachment {
        w.u32(v);
    }
    if let Some(v) = e.sub_type {
        w.u32(v);
    }
    if let Some(v) = e.color {
        w.u32(v);
    }
}

fn write_wide(w: &mut Writer, s: &str) {
    let units: Vec<u16> = s.chars().take(MAX_TEXT).collect::<String>().encode_utf16().collect();
    w.u16(u16::try_from(units.len()).unwrap_or(0));
    for u in units {
        w.u16(u);
    }
}

fn write_extended(w: &mut Writer, e: &Exception) {
    w.u32(4);
    w.u32(e.change_highlight.unwrap_or(0));
    w.u32(0);
    if e.subject.is_some() || e.location.is_some() {
        w.minutes(e.start);
        w.minutes(e.end);
        w.minutes(e.original);
        if let Some(s) = &e.subject {
            write_wide(w, s);
        }
        if let Some(s) = &e.location {
            write_wide(w, s);
        }
        w.u32(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i64, m: u32, d: u32) -> i64 {
        day_at(y, m, d)
    }

    fn dates(days: &[i64]) -> Vec<String> {
        days.iter()
            .map(|d| {
                let (y, m, dd) = ymd(*d);
                format!("{y:04}-{m:02}-{dd:02}")
            })
            .collect()
    }

    #[test]
    fn days_and_minutes_since_1601() {
        assert_eq!(minutes_of_day(0), 134_774 * 1440);
        assert_eq!(day_of_minutes(minutes_of_day(20_000) + 600), 20_000);
        assert_eq!(weekday(0), 4); // 1970-01-01 was a Thursday.
        assert_eq!(weekday(day(2026, 10, 7)), 3);
    }

    #[test]
    fn a_daily_series_every_other_day_ending_after_five() {
        let r = Recurrence::new(Freq::Daily, Rule::Day, 2, End::After(5), day(2026, 10, 5), 9 * 60, 9 * 60 + 30, 0);
        assert_eq!(r.end_day, day(2026, 10, 13));
        assert_eq!(dates(&r.days(day(2026, 10, 1), day(2026, 11, 1))), ["2026-10-05", "2026-10-07", "2026-10-09", "2026-10-11", "2026-10-13"]);
        // From the middle the places are counted from the start.
        assert_eq!(dates(&r.days(day(2026, 10, 10), day(2026, 11, 1))), ["2026-10-11", "2026-10-13"]);
        assert_eq!(r.words(), "every 2 days");
    }

    #[test]
    fn a_weekly_series_moves_its_start_to_its_first_day() {
        // Made on a Tuesday for Mondays and Wednesdays: it starts on the Wednesday.
        let r = Recurrence::new(Freq::Weekly, Rule::Week { days: 0b1010 }, 1, End::After(4), day(2026, 10, 6), 600, 660, 0);
        assert_eq!(r.start_day, day(2026, 10, 7));
        assert_eq!(dates(&r.days(day(2026, 10, 1), day(2026, 12, 1))), ["2026-10-07", "2026-10-12", "2026-10-14", "2026-10-19"]);
        assert_eq!(r.words(), "every week on Monday and Wednesday");
        let every_other = Recurrence::new(Freq::Weekly, Rule::Week { days: 0b10 }, 2, End::Never, day(2026, 10, 5), 600, 660, 1);
        assert_eq!(dates(&every_other.days(day(2026, 10, 1), day(2026, 11, 10))), ["2026-10-05", "2026-10-19", "2026-11-02"]);
        let weekdays = Recurrence::new(Freq::Weekly, Rule::Week { days: 0x3E }, 1, End::Never, day(2026, 10, 9), 600, 660, 0);
        assert_eq!(dates(&weekdays.days(day(2026, 10, 9), day(2026, 10, 14))), ["2026-10-09", "2026-10-12", "2026-10-13"]);
        assert_eq!(weekdays.words(), "every weekday");
    }

    #[test]
    fn monthly_series_by_date_by_weekday_and_at_the_end() {
        let by_date = Recurrence::new(Freq::Monthly, Rule::Month { day: 31 }, 1, End::On(day(2027, 4, 30)), day(2027, 1, 31), 0, 60, 0);
        // The 31st, or the month's last day where it is shorter.
        assert_eq!(dates(&by_date.days(day(2027, 1, 1), day(2027, 6, 1))), ["2027-01-31", "2027-02-28", "2027-03-31", "2027-04-30"]);
        assert_eq!(by_date.count, 4);
        let second_tuesday = Recurrence::new(Freq::Monthly, Rule::MonthNth { days: 1 << 2, nth: 2 }, 2, End::Never, day(2026, 10, 1), 0, 60, 0);
        assert_eq!(dates(&second_tuesday.days(day(2026, 10, 1), day(2027, 3, 1))), ["2026-10-13", "2026-12-08", "2027-02-09"]);
        assert_eq!(second_tuesday.words(), "the second Tuesday of every 2 months");
        let last_weekday = Recurrence::new(Freq::Monthly, Rule::MonthNth { days: 0x3E, nth: 5 }, 1, End::After(2), day(2026, 10, 1), 0, 60, 0);
        assert_eq!(dates(&last_weekday.days(day(2026, 10, 1), day(2027, 3, 1))), ["2026-10-30", "2026-11-30"]);
    }

    #[test]
    fn a_yearly_series_on_a_birthday() {
        let r = Recurrence::new(Freq::Yearly, Rule::Month { day: 29 }, 12, End::Never, day(2028, 2, 29), 0, 1440, 0);
        assert_eq!(dates(&r.days(day(2028, 1, 1), day(2032, 12, 31))), ["2028-02-29", "2029-02-28", "2030-02-28", "2031-02-28", "2032-02-29"]);
        assert_eq!(r.words(), "every 29 February");
    }

    #[test]
    fn a_pattern_with_its_exceptions_round_trips() {
        let mut r = Recurrence::new(Freq::Weekly, Rule::Week { days: 0b100 }, 1, End::After(10), day(2026, 10, 6), 540, 570, 1);
        let moved = day(2026, 10, 14);
        r.deleted = vec![day(2026, 10, 13), day(2026, 10, 20)];
        r.modified = vec![moved];
        r.exceptions.push(Exception {
            start: minutes_of_day(moved) + 600,
            end: minutes_of_day(moved) + 660,
            original: minutes_of_day(day(2026, 10, 13)) + 540,
            subject: Some("Stand-up in the garden ☀".into()),
            location: Some("Café".into()),
            busy: Some(1),
            reminder_set: Some(false),
            ..Exception::default()
        });
        let bytes = r.write();
        let back = Recurrence::read(&bytes).unwrap();
        assert_eq!(back.exceptions[0].subject.as_deref(), Some("Stand-up in the garden ☀"));
        assert_eq!(back.exceptions[0].change_highlight, Some(0));
        let mut want = r.clone();
        want.exceptions[0].change_highlight = Some(0);
        assert_eq!(back, want);
        assert_eq!(back.write(), bytes);
        assert!(back.taken_out(day(2026, 10, 20)));
        assert_eq!(back.exception_on(day(2026, 10, 13)).map(|e| e.start), Some(minutes_of_day(moved) + 600));
        // The 8-bit subject has a stand-in for what Windows-1252 lacks.
        assert!(bytes.windows(24).any(|w| w == b"Stand-up in the garden ?"));
    }

    #[test]
    fn first_date_time_is_as_the_spec_counts_it() {
        // [MS-OXOCAL] 2.2.1.44.1.1: weekly, from the start of the week StartDate is in.
        let r = Recurrence::new(Freq::Weekly, Rule::Week { days: 1 << 4 }, 1, End::Never, day(2007, 4, 12), 0, 60, 0);
        assert_eq!(r.first_date_time(), minutes_of_day(day(2007, 4, 8)) % 10080);
        let daily = Recurrence::new(Freq::Daily, Rule::Day, 3, End::Never, day(2026, 10, 5), 0, 60, 0);
        assert_eq!(daily.first_date_time(), minutes_of_day(day(2026, 10, 5)) % (3 * 1440));
        let monthly = Recurrence::new(Freq::Monthly, Rule::Month { day: 5 }, 5, End::Never, day(2026, 10, 5), 0, 60, 0);
        let months = (2026 - 1601) * 12 + 9;
        assert_eq!(monthly.first_date_time(), minutes_of_day(day(1601, (months % 5) as u32 + 1, 1)));
    }

    #[test]
    fn hostile_patterns_are_refused_or_bounded() {
        let good = Recurrence::new(Freq::Daily, Rule::Day, 1, End::Never, day(2026, 10, 5), 0, 60, 0).write();
        // Every cut is refused, never a panic.
        for n in 0..good.len() {
            let _ = Recurrence::read(&good[..n]);
        }
        assert!(Recurrence::read(&good).is_some());
        // Counts claiming more than the bytes hold.
        let mut huge = good.clone();
        huge[34..38].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Recurrence::read(&huge).is_none());
        // Garbage, and every byte changed in turn.
        assert!(Recurrence::read(&[0xFF; 200]).is_none());
        for i in 0..good.len() {
            let mut b = good.clone();
            b[i] ^= 0xA5;
            if let Some(r) = Recurrence::read(&b) {
                assert!(r.days(day(2026, 1, 1), day(2027, 12, 31)).len() <= MAX_WALK);
                let _ = r.words();
                let _ = r.write();
            }
        }
        // A weekly pattern on no day, and a series from 1601 every day, are bounded walks.
        let none = Recurrence::new(Freq::Weekly, Rule::Week { days: 0 }, 1, End::Never, day(2026, 10, 5), 0, 60, 0);
        assert!(none.days(day(2026, 1, 1), day(2030, 1, 1)).is_empty());
        let old = Recurrence::new(Freq::Daily, Rule::Day, 1, End::Never, day(1601, 1, 1), 0, 60, 0);
        assert_eq!(old.days(day(2026, 10, 1), day(2026, 10, 8)).len(), 7);
        let far = old.days(day(1601, 1, 1), day(9000, 1, 1));
        assert_eq!(far.len(), MAX_WALK);
    }
}
