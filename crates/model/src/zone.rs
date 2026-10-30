//! Time zones with daylight saving: what offset from UTC a place keeps at each moment, and the
//! local time of a moment there and back.
//!
//! A zone is a standard offset and, where it has one, a daylight-saving period that begins and
//! ends on the same day of the same week of a month each year, as Outlook keeps a zone in an
//! item: TZREG ([MS-OXOCAL] 2.2.1.39, PidLidTimeZoneStruct) and TZDEFINITION ([MS-OXOCAL]
//! 2.2.1.41, PidLidAppointmentTimeZoneDefinitionStartDisplay, EndDisplay and Recur), and as
//! iCalendar writes one, a STANDARD and a DAYLIGHT observance repeating yearly (RFC 5545 3.6.5).
//!
//! A zone whose rules changed over the years keeps the earlier ones with the last year each was
//! in force (`Zone::earlier`), as a TZDEFINITION keeps a rule for each year it changed and
//! iCalendar observances that end (RFC 5545 3.8.5.3, UNTIL). A change may also fall on the first
//! weekday on or after a day of the month (the tz database's "Fri>=23", as Israel's), or follow
//! the Islamic calendar (Morocco's clocks go back for Ramadan): those are written for Outlook as
//! a rule for each year, each said as the week of the month it falls in that year. Ramadan is
//! reckoned by the tabular Islamic calendar, which the tz database's own predictions also follow
//! to the day for the Sundays the clocks change on.
//!
//! What is read is hostile: offsets are held to 18 hours, a rule that names no day of a month
//! leaves the zone without daylight saving, and a definition's rules and name are capped.

use crate::time::{self, Civil};

const MINUTE: i64 = 600_000_000;
/// Offsets further from UTC than this are not taken (RFC 5545 3.3.14 allows ±23:59; no place
/// keeps more than 14 hours).
const MAX_OFFSET: i32 = 18 * 60;

/// The day and local time a change of offset happens each year: the `week`th (1 to 4, or 5 for
/// the last) `weekday` (0 Sunday) of `month` (1 to 12), at `minute` past local midnight, as
/// the SYSTEMTIME of a TZRULE says it with its year 0 ([MS-OXOCAL] 2.2.1.41.1); or, where `day`
/// is not 0 (and `week` is), the first `weekday` on or after that day of the month.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rule {
    pub month: u8,
    pub week: u8,
    pub weekday: u8,
    pub minute: u16,
    pub day: u8,
}

/// The weekday (0 Sunday) of day number `day`; 1970-01-01 was a Thursday.
fn weekday_of(day: i64) -> i64 {
    (day.rem_euclid(7) + 4).rem_euclid(7)
}

/// The day number of the first of the month after `month` of `year`.
fn next_month(year: i64, month: u32) -> i64 {
    if month >= 12 { time::days_from_civil(year.saturating_add(1), 1, 1) } else { time::days_from_civil(year, month + 1, 1) }
}

impl Rule {
    const fn new(month: u8, week: u8, weekday: u8, minute: u16) -> Self {
        Self { month, week, weekday, minute, day: 0 }
    }

    /// The first `weekday` on or after `day` of `month`.
    const fn on_or_after(month: u8, day: u8, weekday: u8, minute: u16) -> Self {
        Self { month, week: 0, weekday, minute, day }
    }

    fn valid(&self) -> bool {
        let when = if self.day == 0 { (1..=5).contains(&self.week) } else { self.week == 0 && (1..=31).contains(&self.day) };
        (1..=12).contains(&self.month) && when && self.weekday <= 6 && self.minute < 24 * 60
    }

    /// The day number (days since 1970-01-01) it falls on in `year`.
    pub fn day_in(&self, year: i64) -> i64 {
        let month = u32::from(self.month.clamp(1, 12));
        let first = time::days_from_civil(year, month, 1);
        let next = next_month(year, month);
        if self.day != 0 {
            let from = (first + i64::from(self.day.clamp(1, 31)) - 1).min(next - 1);
            return from + (i64::from(self.weekday) - weekday_of(from)).rem_euclid(7);
        }
        let first_wanted = first + (i64::from(self.weekday) - weekday_of(first)).rem_euclid(7);
        let mut day = first_wanted + 7 * (i64::from(self.week.clamp(1, 5)) - 1);
        while day >= next {
            day -= 7;
        }
        day
    }

    /// The rule as the week of the month it falls in `year`, as Outlook writes a rule for one
    /// year: itself where it is one already.
    pub fn in_year(&self, year: i64) -> Self {
        if self.day == 0 {
            return *self;
        }
        Self::of_day(self.day_in(year), self.minute)
    }

    /// The rule of the week of its month day number `day` falls in, at `minute`.
    fn of_day(day: i64, minute: u16) -> Self {
        let c = time::civil(time::from_unix(day.saturating_mul(86_400)), 0);
        let last = day.saturating_add(7) >= next_month(c.year, c.month);
        let week = if last { 5 } else { u8::try_from((c.day.saturating_sub(1)) / 7 + 1).unwrap_or(5) };
        Self { month: u8::try_from(c.month).unwrap_or(1), week, weekday: u8::try_from(weekday_of(day)).unwrap_or(0), minute, day: 0 }
    }

    /// Local minutes since 1970 at which it happens in `year`.
    fn local_minutes(&self, year: i64) -> i64 {
        self.day_in(year).saturating_mul(24 * 60).saturating_add(i64::from(self.minute))
    }
}

/// Daylight saving: `save` minutes more (fewer where it is less than 0) from when `begins`
/// (said in standard time) to when `ends` (said in daylight time). Where `ramadan` is set, it is
/// kept through Ramadan instead: from the last `begins.weekday` before its first day, at
/// `begins.minute`, to the first `ends.weekday` after the first day of Shawwal, at `ends.minute`;
/// the months and weeks of the rules are then those of each year's, for writing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Daylight {
    pub save: i32,
    pub begins: Rule,
    pub ends: Rule,
    pub ramadan: bool,
}

impl Daylight {
    pub fn new(save: i32, begins: Rule, ends: Rule) -> Self {
        Self { save, begins, ends, ramadan: false }
    }

    /// Its rules as they fall in `year`, each said by the week of its month.
    pub fn in_year(&self, year: i64) -> Self {
        if !self.ramadan {
            return Self { begins: self.begins.in_year(year), ends: self.ends.in_year(year), ..*self };
        }
        let h = hijri_year_beginning_ramadan_in(year);
        let (begins, ends) = ramadan_days(h, self);
        Self { save: self.save, begins: Rule::of_day(begins, self.begins.minute), ends: Rule::of_day(ends, self.ends.minute), ramadan: false }
    }

    /// Whether its rules are said the same way each year (Outlook can keep them in one rule).
    pub fn yearly(&self) -> bool {
        !self.ramadan && self.begins.day == 0 && self.ends.day == 0
    }
}

/// Rules a zone kept until the end of `until`, from the year after the era before.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Era {
    pub until: i64,
    pub bias: i32,
    pub daylight: Option<Daylight>,
}

/// Day number of the first day of `month` of Islamic year `year`, by the tabular (arithmetic)
/// Islamic calendar: 30-year cycles of eleven leap years, months of 30 and 29 days in turn.
fn hijri_day(year: i64, month: i64) -> i64 {
    let month = month.clamp(1, 12);
    // 1 Muharram 1 AH is 16 July 622 (Julian); day number -492148.
    (59 * (month - 1) + 1) / 2 + (year - 1).saturating_mul(354) + (3 + 11 * year).div_euclid(30) - 492_148
}

/// The Islamic year whose Ramadan begins first in Gregorian `year`.
fn hijri_year_beginning_ramadan_in(year: i64) -> i64 {
    let first = time::days_from_civil(year, 1, 1);
    // Near the year of the day; the one asked for is among the two after the one before it.
    let guess = (first + 492_148).saturating_mul(30).div_euclid(10_631);
    (guess.saturating_sub(1)..=guess.saturating_add(2)).find(|h| hijri_day(*h, 9) >= first).unwrap_or(guess)
}

/// The day numbers the clocks change on around Ramadan of Islamic `year`: the last
/// `begins.weekday` before its first day, and the first `ends.weekday` after the first of
/// Shawwal.
fn ramadan_days(year: i64, d: &Daylight) -> (i64, i64) {
    let first = hijri_day(year, 9);
    let before = first - 1 - (weekday_of(first - 1) - i64::from(d.begins.weekday)).rem_euclid(7);
    let shawwal = hijri_day(year, 10);
    let after = shawwal + 1 + (i64::from(d.ends.weekday) - weekday_of(shawwal + 1)).rem_euclid(7);
    (before, after)
}

/// A time zone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Zone {
    /// Its name in the tz database ("Europe/Berlin") when it is one of those known; empty for a
    /// zone known only by its offsets.
    pub id: String,
    /// The name Outlook keys the zone by, written in a TZDEFINITION ([MS-OXOCAL] 2.2.1.41);
    /// empty when not known.
    pub key: String,
    /// The standard offset, minutes east of UTC.
    pub bias: i32,
    pub daylight: Option<Daylight>,
    /// The rules it kept before these, earliest first; each from the year after the one before
    /// it to its `until`, and these from the year after the last.
    pub earlier: Vec<Era>,
}

impl Zone {
    /// A zone that keeps one offset all year.
    pub fn fixed(minutes_east: i32) -> Self {
        Self { bias: minutes_east.clamp(-MAX_OFFSET, MAX_OFFSET), ..Self::default() }
    }

    pub fn utc() -> Self {
        Self::fixed(0)
    }

    /// One of the zones this program knows, by its tz database name or Outlook's key, ignoring
    /// case; `None` for any other.
    pub fn named(name: &str) -> Option<Self> {
        let name = name.trim();
        if name.eq_ignore_ascii_case("UTC") || name.eq_ignore_ascii_case("Etc/UTC") || name.eq_ignore_ascii_case("GMT") {
            return Some(Self { id: "UTC".into(), key: "UTC".into(), ..Self::default() });
        }
        KNOWN.iter().find(|k| k.id.eq_ignore_ascii_case(name)).or_else(|| KNOWN.iter().find(|k| k.key.eq_ignore_ascii_case(name))).map(Known::zone)
    }

    /// The name it is known by: its tz database name, else Outlook's key; empty for an offset
    /// known by no name.
    pub fn name(&self) -> &str {
        if self.id.is_empty() { &self.key } else { &self.id }
    }

    /// Every zone this program knows, west to east.
    pub fn known() -> Vec<Self> {
        let mut all: Vec<Self> =
            std::iter::once(Self { id: "UTC".into(), key: "UTC".into(), ..Self::default() }).chain(KNOWN.iter().map(Known::zone)).collect();
        all.sort_by(|a, b| a.bias.cmp(&b.bias).then_with(|| a.id.cmp(&b.id)));
        all
    }

    /// The zone known that keeps the same offsets at the same times as this one, the first by
    /// name where several do; itself when none does, or when it keeps one offset all year (many
    /// places do, and which is not known).
    pub fn known_as(&self) -> Self {
        if !self.id.is_empty() || self.daylight.is_none() {
            return self.clone();
        }
        Self::known().into_iter().find(|k| k.bias == self.bias && k.daylight == self.daylight).unwrap_or_else(|| self.clone())
    }

    /// Whether it keeps one offset all year.
    pub fn is_fixed(&self) -> bool {
        self.daylight.is_none()
    }

    /// The standard offset and daylight saving it keeps in `year`.
    pub fn rules_in(&self, year: i64) -> (i32, Option<Daylight>) {
        self.earlier.iter().find(|e| year <= e.until).map_or((self.bias, self.daylight), |e| (e.bias, e.daylight))
    }

    /// The first year its rules today are kept from: 1601 where it has kept no others.
    pub fn since(&self) -> i64 {
        self.earlier.iter().map(|e| e.until).max().map_or(1601, |u| u.saturating_add(1))
    }

    /// Minutes east of UTC it keeps at `utc` (FILETIME ticks).
    pub fn offset_at(&self, utc: i64) -> i32 {
        let year = time::civil(utc, self.bias).year;
        let (bias, daylight) = self.rules_in(year);
        let Some(d) = daylight else { return bias };
        let minutes = time::to_unix(utc).div_euclid(60);
        let saving = if d.ramadan {
            // Ramadan moves through the seasons: the one about now, and those either side.
            let h = (minutes.div_euclid(24 * 60) + 492_148).saturating_mul(30).div_euclid(10_631);
            (h.saturating_sub(1)..=h.saturating_add(1)).any(|h| {
                let (from, to) = ramadan_days(h, &d);
                let begins = from.saturating_mul(24 * 60).saturating_add(i64::from(d.begins.minute)).saturating_sub(i64::from(bias));
                let ends = to.saturating_mul(24 * 60).saturating_add(i64::from(d.ends.minute)).saturating_sub(i64::from(bias.saturating_add(d.save)));
                minutes >= begins && minutes < ends
            })
        } else {
            // When it begins and ends that year, in UTC minutes since 1970.
            let begins = d.begins.local_minutes(year).saturating_sub(i64::from(bias));
            let ends = d.ends.local_minutes(year).saturating_sub(i64::from(bias.saturating_add(d.save)));
            if begins <= ends { minutes >= begins && minutes < ends } else { minutes >= begins || minutes < ends }
        };
        if saving { bias.saturating_add(d.save) } else { bias }
    }

    /// The date and time at `utc` (FILETIME ticks) here.
    pub fn civil(&self, utc: i64) -> Civil {
        time::civil(utc, self.offset_at(utc))
    }

    /// UTC FILETIME ticks of a local time here, given as ticks counted as if it were UTC. A time
    /// skipped when the clocks go forward is taken as the standard time it would have been (so
    /// 02:30 on that night is 03:30); one that happens twice when they go back, as the first.
    pub fn utc_of(&self, local: i64) -> i64 {
        let (bias, daylight) = self.rules_in(time::civil(local, 0).year);
        let Some(d) = daylight else { return local.saturating_sub(i64::from(bias).saturating_mul(MINUTE)) };
        let standard = local.saturating_sub(i64::from(bias).saturating_mul(MINUTE));
        let saving = local.saturating_sub(i64::from(bias.saturating_add(d.save)).saturating_mul(MINUTE));
        let as_saving = self.offset_at(saving) == bias.saturating_add(d.save);
        let as_standard = self.offset_at(standard) == bias;
        match (as_saving, as_standard) {
            (true, true) => saving.min(standard),
            (true, false) => saving,
            _ => standard,
        }
    }

    /// UTC FILETIME ticks of a date and time here.
    pub fn from_civil(&self, year: i64, month: u32, day: u32, hour: u32, minute: u32) -> i64 {
        self.utc_of(time::from_civil(year, month, day, hour, minute, 0, 0))
    }

    /// UTC FILETIME ticks at the local midnight that begins day number `day` (days since
    /// 1970-01-01) here.
    pub fn day_start(&self, day: i64) -> i64 {
        self.utc_of(time::from_unix(day.saturating_mul(86_400)))
    }

    /// The local day number `utc` falls on here.
    pub fn day_of(&self, utc: i64) -> i64 {
        self.civil(utc).day_number
    }

    /// Its offset at `utc` as people write it: `UTC+02:00`, or `UTC` itself.
    pub fn offset_text_at(&self, utc: i64) -> String {
        offset_text(self.offset_at(utc))
    }

    /// The name people know it by: the city of its tz database name with its standard offset
    /// ("UTC+01:00 Berlin"), or its offset alone.
    pub fn label(&self) -> String {
        let place = self.id.rsplit('/').next().unwrap_or("").replace('_', " ");
        let offset = offset_text(self.bias);
        match place.as_str() {
            "" => offset,
            "UTC" => "UTC".into(),
            p => format!("{offset} {p}"),
        }
    }

    /// What is said of it beside a time: "Berlin" for a zone known, its offset there otherwise.
    pub fn short(&self, utc: i64) -> String {
        let place = self.id.rsplit('/').next().unwrap_or("").replace('_', " ");
        if place.is_empty() { self.offset_text_at(utc) } else { place }
    }

    /// As a TZREG ([MS-OXOCAL] 2.2.1.39): 48 bytes, of its rules today; one that moves from year
    /// to year as those of the first year they are kept, or of 2026 where that is earlier.
    pub fn tzreg(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(48);
        let daylight = self.daylight.map(|d| d.in_year(self.since().max(2026)));
        biases(&mut out, self.bias, daylight);
        // wStandardYear and stStandardDate: when daylight saving ends; then wDaylightYear and
        // stDaylightDate: when it begins.
        out.extend_from_slice(&0u16.to_le_bytes());
        system_time(&mut out, daylight.map(|d| d.ends));
        out.extend_from_slice(&0u16.to_le_bytes());
        system_time(&mut out, daylight.map(|d| d.begins));
        out
    }

    /// A TZREG read; `None` when it is too short or its offsets cannot be.
    pub fn of_tzreg(b: &[u8]) -> Option<Self> {
        let r = Reader(b);
        let zone = Self::of_biases(r.i32(0)?, r.i32(4)?, r.i32(8)?, r.rule(14)?, r.rule(32)?)?;
        Some(zone.known_as())
    }

    fn of_biases(bias: i32, standard: i32, daylight: i32, ends: Option<Rule>, begins: Option<Rule>) -> Option<Self> {
        let east = bias.checked_add(standard)?.checked_neg()?;
        let saving_east = bias.checked_add(daylight)?.checked_neg()?;
        if east.abs() > MAX_OFFSET || saving_east.abs() > MAX_OFFSET {
            return None;
        }
        let save = saving_east - east;
        let daylight = match (begins, ends) {
            (Some(begins), Some(ends)) if save != 0 && begins.month != ends.month => Some(Daylight::new(save, begins, ends)),
            _ => None,
        };
        Some(Self { bias: east, daylight, ..Self::default() })
    }

    /// Its rules as a TZDEFINITION gives them: each with the first year it is in force, earliest
    /// first. Rules that move from year to year are given for each year from the first they are
    /// kept (1970 at the earliest) to 2040, and from then on as they fall that year.
    pub fn yearly_rules(&self) -> Vec<(i64, i32, Option<Daylight>)> {
        let mut eras: Vec<(i64, i32, Option<Daylight>)> = Vec::new();
        let mut from = 1601;
        for e in self.earlier.iter().take(MAX_RULES) {
            if e.until < from {
                continue;
            }
            eras.push((from, e.bias, e.daylight));
            from = e.until.saturating_add(1);
        }
        eras.push((from, self.bias, self.daylight));
        let mut out: Vec<(i64, i32, Option<Daylight>)> = Vec::new();
        for (n, (from, bias, daylight)) in eras.iter().enumerate() {
            let until = eras.get(n + 1).map(|e| e.0 - 1);
            match daylight {
                Some(d) if !d.yearly() => {
                    let first = (*from).max(1970);
                    let last = until.unwrap_or(LAST_YEAR).min(LAST_YEAR.max(first));
                    if first > *from {
                        out.push((*from, *bias, Some(d.in_year(first))));
                    }
                    for y in first..=last {
                        out.push((y, *bias, Some(d.in_year(y))));
                    }
                }
                _ => out.push((*from, *bias, *daylight)),
            }
        }
        out.truncate(MAX_RULES);
        out
    }

    /// As a TZDEFINITION ([MS-OXOCAL] 2.2.1.41): a rule for each year its rules changed (one, in
    /// force from 1601, for a zone whose rules never did), the one of today marked as in force;
    /// `recur` marks that one as the one a series' pattern is in (TZRULE_FLAG_RECUR_CURRENT_TZREG).
    pub fn tzdefinition(&self, recur: bool) -> Vec<u8> {
        let key: Vec<u16> = self.key_name().encode_utf16().take(MAX_KEY).collect();
        let rules = self.yearly_rules();
        // The one in force: that of today's rules for 2026, or the first of them where later.
        let since = self.since().max(2026);
        let effective = rules.iter().rposition(|(y, ..)| *y <= since).unwrap_or(0);
        let mut out = Vec::with_capacity(16 + key.len() * 2 + 66 * rules.len());
        out.extend_from_slice(&[0x02, 0x01]);
        let header = 2 + 2 + key.len() * 2 + 2;
        out.extend_from_slice(&u16::try_from(header).unwrap_or(u16::MAX).to_le_bytes());
        out.extend_from_slice(&0x0002u16.to_le_bytes());
        out.extend_from_slice(&u16::try_from(key.len()).unwrap_or(0).to_le_bytes());
        for unit in &key {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out.extend_from_slice(&u16::try_from(rules.len()).unwrap_or(0).to_le_bytes());
        for (n, (year, bias, daylight)) in rules.iter().enumerate() {
            // The rule ([MS-OXOCAL] 2.2.1.41.1).
            out.extend_from_slice(&[0x02, 0x01]);
            out.extend_from_slice(&0x003Eu16.to_le_bytes());
            let flags: u16 = if n == effective { 0x0002 | if recur { 0x0001 } else { 0 } } else { 0 };
            out.extend_from_slice(&flags.to_le_bytes());
            out.extend_from_slice(&u16::try_from(*year).unwrap_or(1601).to_le_bytes());
            out.extend_from_slice(&[0u8; 14]);
            biases(&mut out, *bias, *daylight);
            system_time(&mut out, daylight.map(|d| d.ends));
            system_time(&mut out, daylight.map(|d| d.begins));
        }
        out
    }

    /// The name written in a TZDEFINITION: Outlook's key where it is known, else its own.
    fn key_name(&self) -> String {
        if !self.key.is_empty() {
            self.key.clone()
        } else if !self.id.is_empty() {
            self.id.clone()
        } else {
            offset_text(self.bias)
        }
    }

    /// A TZDEFINITION read: its rules each from the year it gives, the last kept from then on;
    /// `None` when it cannot be read.
    pub fn of_tzdefinition(b: &[u8]) -> Option<Self> {
        let r = Reader(b);
        if r.u8(0)? != 0x02 {
            return None;
        }
        let header = usize::from(r.u16(2)?);
        let key_len = usize::from(r.u16(6)?).min(MAX_KEY);
        let key: Vec<u16> = (0..key_len).filter_map(|i| r.u16(8 + i * 2)).collect();
        let key = String::from_utf16_lossy(&key).chars().filter(|c| !c.is_control()).collect::<String>();
        let rules_at = 4usize.checked_add(header)?.checked_sub(2)?;
        let count = usize::from(r.u16(rules_at)?).min(MAX_RULES);
        let mut read: Vec<(i64, Zone)> = Vec::new();
        for i in 0..count {
            let at = rules_at + 2 + i * 66;
            if r.u8(at)? != 0x02 {
                break;
            }
            let year = i64::from(r.u16(at + 6)?);
            let zone = Self::of_biases(r.i32(at + 22)?, r.i32(at + 26)?, r.i32(at + 30)?, r.rule(at + 34)?, r.rule(at + 50)?);
            if let Some(z) = zone {
                read.push((year, z));
            }
        }
        // Earliest first; of two for one year, the last; and a year that keeps the year
        // before's rules is part of its era.
        read.sort_by_key(|(y, _)| *y);
        let mut eras: Vec<(i64, Zone)> = Vec::new();
        for (year, z) in read {
            match eras.last_mut() {
                Some((y, last)) if *y == year => *last = z,
                Some((_, last)) if last.bias == z.bias && last.daylight == z.daylight => {}
                _ => eras.push((year, z)),
            }
        }
        let (_, mut zone) = eras.last()?.clone();
        zone.earlier = eras
            .windows(2)
            .filter_map(|w| match w {
                [(_, a), (next, _)] => Some(Era { until: next - 1, bias: a.bias, daylight: a.daylight }),
                _ => None,
            })
            .collect();
        // A key this program knows brings its tz database name where it keeps the same
        // offsets; any other is kept as it is.
        let known = KNOWN.iter().map(Known::zone).find(|k| k.key.eq_ignore_ascii_case(&key) && k.keeps_as(&zone));
        match known {
            Some(k) => zone = k,
            None if key.eq_ignore_ascii_case("UTC") && zone.bias == 0 && zone.daylight.is_none() && zone.earlier.is_empty() => {
                zone = Self::named("UTC")?
            }
            None => {
                if zone.earlier.is_empty() {
                    zone = zone.known_as();
                }
                zone.key = key;
            }
        }
        Some(zone)
    }

    /// Whether `other` keeps the offsets this zone does in the years it says something of:
    /// from 1970 where it gives rules it kept before, else from when this zone's rules today
    /// are kept (a definition of one rule says what is kept now), to 2040.
    pub fn keeps_as(&self, other: &Zone) -> bool {
        let first = if other.earlier.is_empty() { self.since().max(1970) } else { 1970 };
        self.bias == other.bias && (first..=LAST_YEAR.max(first)).filter(|y| self.one_rule_in(*y)).all(|y| self.same_in(other, y))
    }

    /// Whether its rules in `year` can be said as one of that year: not where Ramadan comes
    /// twice in it or runs into the next, which no rule for one year says.
    pub fn one_rule_in(&self, year: i64) -> bool {
        let Some(d) = self.rules_in(year).1.filter(|d| d.ramadan) else { return true };
        let h = hijri_year_beginning_ramadan_in(year);
        let (_, after) = ramadan_days(h, &d);
        let (next, _) = ramadan_days(h.saturating_add(1), &d);
        let (_, before_end) = ramadan_days(h.saturating_sub(1), &d);
        let (start, end) = (time::days_from_civil(year, 1, 1), time::days_from_civil(year.saturating_add(1), 1, 1));
        after < end && next >= end && before_end < start
    }

    /// Whether `other` keeps the rules this zone does in `year`, each said as it falls that
    /// year.
    pub fn same_in(&self, other: &Zone, year: i64) -> bool {
        let (a, da) = self.rules_in(year);
        let (b, db) = other.rules_in(year);
        a == b && da.map(|d| d.in_year(year)) == db.map(|d| d.in_year(year))
    }

    /// The zone a place keeps, from what offset it keeps at each moment (as the system says):
    /// the year of `now` is looked at for the moments its offset changes, and those taken as
    /// happening on the same day of the same week each year. One of the zones known when it is
    /// the same.
    pub fn from_system(now: i64, offset_at: impl Fn(i64) -> i32) -> Self {
        let year = time::civil(now, offset_at(now)).year;
        let start = time::from_civil(year, 1, 1, 0, 0, 0, 0);
        let end = time::from_civil(year.saturating_add(1), 1, 1, 0, 0, 0, 0);
        let step = MINUTE.saturating_mul(60 * 24 * 7);
        let clamp = |o: i32| o.clamp(-MAX_OFFSET, MAX_OFFSET);
        // Each change of offset in the year, found to the minute.
        let mut changes: Vec<(i64, i32, i32)> = Vec::new();
        let mut at = start;
        let mut was = clamp(offset_at(at));
        while at < end && changes.len() < 4 {
            let next = at.saturating_add(step).min(end);
            let now_off = clamp(offset_at(next));
            if now_off != was {
                let (mut lo, mut hi) = (at, next);
                while hi.saturating_sub(lo) > MINUTE {
                    let mid = lo + (hi - lo) / 2 / MINUTE * MINUTE;
                    let mid = if mid <= lo { lo + MINUTE } else { mid };
                    if clamp(offset_at(mid)) == was { lo = mid } else { hi = mid }
                }
                changes.push((hi, was, now_off));
                was = now_off;
            }
            at = next;
        }
        let rule = |utc: i64, offset: i32| {
            let c = time::civil(utc, offset);
            let last = time::days_from_civil(c.year, c.month, c.day).saturating_add(7) > {
                let (y, m) = if c.month == 12 { (c.year + 1, 1) } else { (c.year, c.month + 1) };
                time::days_from_civil(y, m, 1) - 1
            };
            Rule {
                month: c.month as u8,
                week: if last { 5 } else { ((c.day - 1) / 7 + 1) as u8 },
                weekday: c.weekday as u8,
                minute: (c.hour * 60 + c.minute) as u16,
                day: 0,
            }
        };
        let zone = match changes.as_slice() {
            [(a, from_a, to_a), (b, from_b, to_b)] if to_a == from_b && from_a == to_b => {
                // The one to the larger offset begins daylight saving.
                let ((begins, std), (ends, saving)) = if to_a > from_a { ((*a, *from_a), (*b, *from_b)) } else { ((*b, *from_b), (*a, *from_a)) };
                Self { bias: std, daylight: Some(Daylight::new(saving - std, rule(begins, std), rule(ends, saving))), ..Self::default() }
            }
            _ => Self::fixed(clamp(offset_at(now))),
        };
        zone.known_as()
    }
}

/// What a place's clocks say of moments: a zone, or one offset (minutes east) all year.
pub trait Offsets {
    /// Minutes east of UTC kept at `utc` (FILETIME ticks).
    fn offset_at(&self, utc: i64) -> i32;
    /// UTC ticks of local ticks counted as if they were UTC.
    fn utc_of(&self, local: i64) -> i64;
    /// The date and time at `utc` there.
    fn civil(&self, utc: i64) -> Civil {
        time::civil(utc, self.offset_at(utc))
    }
}

impl Offsets for i32 {
    fn offset_at(&self, _: i64) -> i32 {
        *self
    }

    fn utc_of(&self, local: i64) -> i64 {
        local.saturating_sub(i64::from(*self).saturating_mul(MINUTE))
    }
}

impl Offsets for Zone {
    fn offset_at(&self, utc: i64) -> i32 {
        Zone::offset_at(self, utc)
    }

    fn utc_of(&self, local: i64) -> i64 {
        Zone::utc_of(self, local)
    }
}

impl<T: Offsets + ?Sized> Offsets for &T {
    fn offset_at(&self, utc: i64) -> i32 {
        (**self).offset_at(utc)
    }

    fn utc_of(&self, local: i64) -> i64 {
        (**self).utc_of(local)
    }
}

/// `UTC+02:00`; `UTC` for no offset.
pub fn offset_text(minutes_east: i32) -> String {
    if minutes_east == 0 {
        return "UTC".into();
    }
    let sign = if minutes_east < 0 { '-' } else { '+' };
    let m = minutes_east.unsigned_abs();
    format!("UTC{sign}{:02}:{:02}", m / 60, m % 60)
}

/// Longest key name read or written.
const MAX_KEY: usize = 260;
/// Rules of a TZDEFINITION looked at.
const MAX_RULES: usize = 1024;
/// The last year rules that move from year to year are written for one by one.
const LAST_YEAR: i64 = 2040;

/// lBias, lStandardBias and lDaylightBias: minutes west, as Windows counts.
fn biases(out: &mut Vec<u8>, bias: i32, daylight: Option<Daylight>) {
    out.extend_from_slice(&(-bias).to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&(-daylight.map_or(0, |d| d.save)).to_le_bytes());
}

/// A SYSTEMTIME for a rule, or all zeros for none.
fn system_time(out: &mut Vec<u8>, rule: Option<Rule>) {
    let words: [u16; 8] = match rule {
        Some(r) => [0, u16::from(r.month), u16::from(r.weekday), u16::from(r.week), r.minute / 60, r.minute % 60, 0, 0],
        None => [0; 8],
    };
    for w in words {
        out.extend_from_slice(&w.to_le_bytes());
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn u8(&self, at: usize) -> Option<u8> {
        self.0.get(at).copied()
    }

    fn u16(&self, at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(self.0.get(at..at.checked_add(2)?)?.try_into().ok()?))
    }

    fn i32(&self, at: usize) -> Option<i32> {
        Some(i32::from_le_bytes(self.0.get(at..at.checked_add(4)?)?.try_into().ok()?))
    }

    /// A SYSTEMTIME at `at` as a yearly rule: `Some(None)` when it names none (a month of 0, or
    /// a date of one year only, which this program does not keep).
    fn rule(&self, at: usize) -> Option<Option<Rule>> {
        let w = |i: usize| self.u16(at + i * 2);
        let (year, month, weekday, week, hour, minute) = (w(0)?, w(1)?, w(2)?, w(3)?, w(4)?, w(5)?);
        if year != 0 || month == 0 {
            return Some(None);
        }
        // An hour of 24, as some zones end at the end of a day, is the next day's midnight
        // less a minute.
        let minutes = (u32::from(hour) * 60 + u32::from(minute)).min(24 * 60 - 1);
        let rule = Rule {
            month: u8::try_from(month).unwrap_or(0),
            week: u8::try_from(week).unwrap_or(0),
            weekday: u8::try_from(weekday).unwrap_or(9),
            minute: u16::try_from(minutes).unwrap_or(0),
            day: 0,
        };
        Some(rule.valid().then_some(rule))
    }
}

/// A zone of the tz database, with the key Outlook knows it by, its rules today and those it
/// kept before: the last year of each, its standard offset as so many minutes more than today's,
/// and its daylight saving.
struct Known {
    id: &'static str,
    key: &'static str,
    bias: i32,
    daylight: Option<(i32, Rule, Rule)>,
    earlier: &'static [(i64, i32, Option<(i32, Rule, Rule)>)],
    ramadan: bool,
}

impl Known {
    fn zone(&self) -> Zone {
        let daylight = |d: Option<(i32, Rule, Rule)>| d.map(|(save, begins, ends)| Daylight { save, begins, ends, ramadan: self.ramadan });
        Zone {
            id: self.id.into(),
            key: self.key.into(),
            bias: self.bias,
            daylight: daylight(self.daylight),
            earlier: self
                .earlier
                .iter()
                .map(|(until, more, d)| Era {
                    until: *until,
                    bias: self.bias.saturating_add(*more),
                    daylight: d.map(|(save, begins, ends)| Daylight::new(save, begins, ends)),
                })
                .collect(),
        }
    }
}

const SUN: u8 = 0;
/// The European Union's: the last Sunday of March to the last Sunday of October, at 01:00 UTC.
const fn eu(bias: i32) -> Option<(i32, Rule, Rule)> {
    let begins = (60 + bias) as u16;
    Some((60, Rule::new(3, 5, SUN, begins), Rule::new(10, 5, SUN, begins + 60)))
}
/// The United States' and Canada's: the second Sunday of March to the first of November, 02:00.
const US: Option<(i32, Rule, Rule)> = Some((60, Rule::new(3, 2, SUN, 120), Rule::new(11, 1, SUN, 120)));
/// South-east Australia's: the first Sunday of October to the first of April.
const AU: Option<(i32, Rule, Rule)> = Some((60, Rule::new(10, 1, SUN, 120), Rule::new(4, 1, SUN, 180)));

const fn k(id: &'static str, key: &'static str, bias: i32, daylight: Option<(i32, Rule, Rule)>) -> Known {
    Known { id, key, bias, daylight, earlier: &[], ramadan: false }
}

/// A zone that kept other rules before.
const fn h(
    id: &'static str,
    key: &'static str,
    bias: i32,
    daylight: Option<(i32, Rule, Rule)>,
    earlier: &'static [(i64, i32, Option<(i32, Rule, Rule)>)],
) -> Known {
    Known { id, key, bias, daylight, earlier, ramadan: false }
}

const FRI: u8 = 5;
/// The United States' and Canada's from 1987 to 2006: the first Sunday of April to the last of
/// October, 02:00.
const US_1987: Option<(i32, Rule, Rule)> = Some((60, Rule::new(4, 1, SUN, 120), Rule::new(10, 5, SUN, 120)));
const US_EARLIER: &[(i64, i32, Option<(i32, Rule, Rule)>)] = &[(2006, 0, US_1987)];
/// The European Union's from 1981 to 1995, ending on the last Sunday of September.
const fn eu_1981(bias: i32) -> Option<(i32, Rule, Rule)> {
    let begins = (60 + bias) as u16;
    Some((60, Rule::new(3, 5, SUN, begins), Rule::new(9, 5, SUN, begins + 60)))
}
const CET_EARLIER: &[(i64, i32, Option<(i32, Rule, Rule)>)] = &[(1995, 0, eu_1981(60))];
const EET_EARLIER: &[(i64, i32, Option<(i32, Rule, Rule)>)] = &[(1995, 0, eu_1981(120))];
/// South-east Australia's to 2007: the last Sunday of October to the last of March.
const AU_EARLIER: &[(i64, i32, Option<(i32, Rule, Rule)>)] = &[(2007, 0, Some((60, Rule::new(10, 5, SUN, 120), Rule::new(3, 5, SUN, 180))))];
/// New Zealand's to 2006: the first Sunday of October to the third of March.
const NZ_EARLIER: &[(i64, i32, Option<(i32, Rule, Rule)>)] = &[(2006, 0, Some((60, Rule::new(10, 1, SUN, 120), Rule::new(3, 3, SUN, 180))))];
/// Mexico's to 2022: the first Sunday of April to the last of October.
const MX_EARLIER: &[(i64, i32, Option<(i32, Rule, Rule)>)] = &[(2022, 0, Some((60, Rule::new(4, 1, SUN, 120), Rule::new(10, 5, SUN, 120))))];
/// Turkey's to 2015: the European Union's, two hours east; then three all year.
const TR_EARLIER: &[(i64, i32, Option<(i32, Rule, Rule)>)] = &[(2015, -60, eu(120))];
/// Russia's to 2010: the last Sunday of March to the last of October; then four hours east
/// all year to 2014, and three since.
const RU_EARLIER: &[(i64, i32, Option<(i32, Rule, Rule)>)] =
    &[(2010, 0, Some((60, Rule::new(3, 5, SUN, 120), Rule::new(10, 5, SUN, 180)))), (2014, 60, None)];
/// Israel's: the Friday before the last Sunday of March (the first on or after the 23rd) to the
/// last Sunday of October, 02:00.
const IL: Option<(i32, Rule, Rule)> = Some((60, Rule::on_or_after(3, 23, FRI, 120), Rule::new(10, 5, SUN, 120)));
/// Morocco's since 2019: an hour east, and back to UTC from the Sunday before Ramadan at 03:00 to
/// the Sunday after it at 02:00.
const MA: Known = Known {
    id: "Africa/Casablanca",
    key: "Morocco Standard Time",
    bias: 60,
    daylight: Some((-60, Rule::new(3, 1, SUN, 180), Rule::new(4, 1, SUN, 120))),
    earlier: &[],
    ramadan: true,
};

/// Zones as the tz database has them now, with the rules the larger ones kept before since the
/// 1980s; Outlook's key for each as the Unicode CLDR maps them (windowsZones).
const KNOWN: &[Known] = &[
    k("Pacific/Honolulu", "Hawaiian Standard Time", -600, None),
    h("America/Anchorage", "Alaskan Standard Time", -540, US, US_EARLIER),
    h("America/Los_Angeles", "Pacific Standard Time", -480, US, US_EARLIER),
    h("America/Vancouver", "Pacific Standard Time", -480, US, US_EARLIER),
    h("America/Denver", "Mountain Standard Time", -420, US, US_EARLIER),
    h("America/Edmonton", "Mountain Standard Time", -420, US, US_EARLIER),
    k("America/Phoenix", "US Mountain Standard Time", -420, None),
    h("America/Chicago", "Central Standard Time", -360, US, US_EARLIER),
    h("America/Winnipeg", "Central Standard Time", -360, US, US_EARLIER),
    h("America/Mexico_City", "Central Standard Time (Mexico)", -360, None, MX_EARLIER),
    h("America/New_York", "Eastern Standard Time", -300, US, US_EARLIER),
    h("America/Toronto", "Eastern Standard Time", -300, US, US_EARLIER),
    k("America/Bogota", "SA Pacific Standard Time", -300, None),
    k("America/Lima", "SA Pacific Standard Time", -300, None),
    h("America/Halifax", "Atlantic Standard Time", -240, US, US_EARLIER),
    k("America/Caracas", "Venezuela Standard Time", -240, None),
    k("America/Santiago", "Pacific SA Standard Time", -240, Some((60, Rule::new(9, 1, SUN, 0), Rule::new(4, 1, SUN, 0)))),
    k("America/St_Johns", "Newfoundland Standard Time", -210, US),
    k("America/Sao_Paulo", "E. South America Standard Time", -180, None),
    k("America/Argentina/Buenos_Aires", "Argentina Standard Time", -180, None),
    k("Atlantic/Reykjavik", "Greenwich Standard Time", 0, None),
    k("Europe/London", "GMT Standard Time", 0, eu(0)),
    k("Europe/Dublin", "GMT Standard Time", 0, eu(0)),
    k("Europe/Lisbon", "GMT Standard Time", 0, eu(0)),
    MA,
    k("Africa/Lagos", "W. Central Africa Standard Time", 60, None),
    h("Europe/Berlin", "W. Europe Standard Time", 60, eu(60), CET_EARLIER),
    h("Europe/Amsterdam", "W. Europe Standard Time", 60, eu(60), CET_EARLIER),
    h("Europe/Rome", "W. Europe Standard Time", 60, eu(60), CET_EARLIER),
    h("Europe/Stockholm", "W. Europe Standard Time", 60, eu(60), CET_EARLIER),
    h("Europe/Vienna", "W. Europe Standard Time", 60, eu(60), CET_EARLIER),
    h("Europe/Zurich", "W. Europe Standard Time", 60, eu(60), CET_EARLIER),
    h("Europe/Oslo", "W. Europe Standard Time", 60, eu(60), CET_EARLIER),
    h("Europe/Paris", "Romance Standard Time", 60, eu(60), CET_EARLIER),
    h("Europe/Madrid", "Romance Standard Time", 60, eu(60), CET_EARLIER),
    h("Europe/Brussels", "Romance Standard Time", 60, eu(60), CET_EARLIER),
    h("Europe/Copenhagen", "Romance Standard Time", 60, eu(60), CET_EARLIER),
    h("Europe/Prague", "Central Europe Standard Time", 60, eu(60), CET_EARLIER),
    h("Europe/Budapest", "Central Europe Standard Time", 60, eu(60), CET_EARLIER),
    h("Europe/Warsaw", "Central European Standard Time", 60, eu(60), CET_EARLIER),
    k("Africa/Johannesburg", "South Africa Standard Time", 120, None),
    k("Asia/Jerusalem", "Israel Standard Time", 120, IL),
    h("Europe/Athens", "GTB Standard Time", 120, eu(120), EET_EARLIER),
    h("Europe/Bucharest", "GTB Standard Time", 120, eu(120), EET_EARLIER),
    h("Europe/Helsinki", "FLE Standard Time", 120, eu(120), EET_EARLIER),
    k("Europe/Kyiv", "FLE Standard Time", 120, eu(120)),
    h("Europe/Sofia", "FLE Standard Time", 120, eu(120), EET_EARLIER),
    h("Europe/Istanbul", "Turkey Standard Time", 180, None, TR_EARLIER),
    h("Europe/Moscow", "Russian Standard Time", 180, None, RU_EARLIER),
    k("Asia/Riyadh", "Arab Standard Time", 180, None),
    k("Africa/Nairobi", "E. Africa Standard Time", 180, None),
    k("Asia/Tehran", "Iran Standard Time", 210, None),
    k("Asia/Dubai", "Arabian Standard Time", 240, None),
    k("Asia/Karachi", "Pakistan Standard Time", 300, None),
    k("Asia/Kolkata", "India Standard Time", 330, None),
    k("Asia/Kathmandu", "Nepal Standard Time", 345, None),
    k("Asia/Dhaka", "Bangladesh Standard Time", 360, None),
    k("Asia/Bangkok", "SE Asia Standard Time", 420, None),
    k("Asia/Jakarta", "SE Asia Standard Time", 420, None),
    k("Asia/Shanghai", "China Standard Time", 480, None),
    k("Asia/Hong_Kong", "China Standard Time", 480, None),
    k("Asia/Singapore", "Singapore Standard Time", 480, None),
    k("Asia/Manila", "Singapore Standard Time", 480, None),
    k("Asia/Taipei", "Taipei Standard Time", 480, None),
    k("Australia/Perth", "W. Australia Standard Time", 480, None),
    k("Asia/Tokyo", "Tokyo Standard Time", 540, None),
    k("Asia/Seoul", "Korea Standard Time", 540, None),
    k("Australia/Darwin", "AUS Central Standard Time", 570, None),
    h("Australia/Adelaide", "Cen. Australia Standard Time", 570, AU, AU_EARLIER),
    k("Australia/Brisbane", "E. Australia Standard Time", 600, None),
    h("Australia/Sydney", "AUS Eastern Standard Time", 600, AU, AU_EARLIER),
    h("Australia/Melbourne", "AUS Eastern Standard Time", 600, AU, AU_EARLIER),
    k("Australia/Hobart", "Tasmania Standard Time", 600, AU),
    h("Pacific/Auckland", "New Zealand Standard Time", 720, Some((60, Rule::new(9, 5, SUN, 120), Rule::new(4, 1, SUN, 180))), NZ_EARLIER),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn at(z: &Zone, y: i64, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        z.from_civil(y, mo, d, h, mi)
    }

    #[test]
    fn europe_changes_at_one_in_the_morning_utc_on_the_last_sundays() {
        let berlin = Zone::named("Europe/Berlin").unwrap();
        // 2026: 29 March and 25 October.
        assert_eq!(berlin.daylight.unwrap().begins.day_in(2026), time::days_from_civil(2026, 3, 29));
        assert_eq!(berlin.daylight.unwrap().ends.day_in(2026), time::days_from_civil(2026, 10, 25));
        let before = time::from_civil(2026, 3, 29, 0, 59, 0, 0);
        assert_eq!(berlin.offset_at(before), 60);
        assert_eq!(berlin.offset_at(before + MINUTE), 120);
        let back = time::from_civil(2026, 10, 25, 0, 59, 0, 0);
        assert_eq!(berlin.offset_at(back), 120);
        assert_eq!(berlin.offset_at(back + MINUTE), 60);
        // 09:00 is 09:00 on either side.
        assert_eq!(berlin.civil(at(&berlin, 2026, 3, 28, 9, 0)).hour, 9);
        assert_eq!(berlin.civil(at(&berlin, 2026, 3, 30, 9, 0)).hour, 9);
        assert_eq!(at(&berlin, 2026, 3, 30, 9, 0) - at(&berlin, 2026, 3, 28, 9, 0), MINUTE * 60 * (48 - 1));
    }

    #[test]
    fn a_time_skipped_moves_on_and_one_twice_is_the_first() {
        let ny = Zone::named("America/New_York").unwrap();
        // 8 March 2026, 02:30 does not happen: 03:30.
        let skipped = at(&ny, 2026, 3, 8, 2, 30);
        let c = ny.civil(skipped);
        assert_eq!((c.hour, c.minute), (3, 30));
        // 1 November 2026, 01:30 happens twice: the first, in daylight time.
        let twice = at(&ny, 2026, 11, 1, 1, 30);
        assert_eq!(ny.offset_at(twice), -240);
        assert_eq!(ny.civil(twice).hour, 1);
    }

    #[test]
    fn the_south_saves_daylight_across_the_new_year() {
        let sydney = Zone::named("Australia/Sydney").unwrap();
        assert_eq!(sydney.offset_at(time::from_civil(2026, 1, 15, 0, 0, 0, 0)), 660);
        assert_eq!(sydney.offset_at(time::from_civil(2026, 7, 15, 0, 0, 0, 0)), 600);
        assert_eq!(sydney.offset_at(time::from_civil(2026, 12, 31, 23, 0, 0, 0)), 660);
        let santiago = Zone::named("America/Santiago").unwrap();
        assert_eq!(santiago.offset_at(time::from_civil(2026, 1, 15, 12, 0, 0, 0)), -180);
        assert_eq!(santiago.offset_at(time::from_civil(2026, 6, 15, 12, 0, 0, 0)), -240);
    }

    #[test]
    fn a_zone_is_read_back_as_it_was_written() {
        for z in Zone::known() {
            let back = Zone::of_tzdefinition(&z.tzdefinition(true)).unwrap();
            assert_eq!((back.bias, back.daylight), (z.bias, z.daylight), "{}", z.id);
            assert_eq!(back.key, z.key, "{}", z.id);
            // A TZREG holds one rule: today's, as it falls in 2026 where it moves.
            let reg = Zone::of_tzreg(&z.tzreg()).unwrap();
            assert_eq!((reg.bias, reg.daylight), (z.bias, z.daylight.map(|d| d.in_year(2026))), "{}", z.id);
            // Read back as the first zone of its key keeping its offsets (Kyiv as Helsinki).
            assert!(back.keeps_as(&z), "{}", z.id);
            assert_eq!(z.tzreg().len(), 48);
        }
        // A key not known is kept, with its rules.
        let mut own = Zone::fixed(5 * 60 + 30);
        own.key = "Somewhere Standard Time".into();
        assert_eq!(Zone::of_tzdefinition(&own.tzdefinition(false)).unwrap().key, "Somewhere Standard Time");
    }

    #[test]
    fn rules_changed_over_the_years_are_kept_with_each_year() {
        let ny = Zone::named("America/New_York").unwrap();
        // 20 March: standard time in 2006, under the rules of 1987; daylight time in 2010.
        assert_eq!(ny.offset_at(time::from_civil(2006, 3, 20, 12, 0, 0, 0)), -300);
        assert_eq!(ny.offset_at(time::from_civil(2010, 3, 20, 12, 0, 0, 0)), -240);
        assert_eq!(ny.offset_at(time::from_civil(2006, 4, 2, 6, 59, 0, 0)), -300);
        assert_eq!(ny.offset_at(time::from_civil(2006, 4, 2, 7, 0, 0, 0)), -240);
        // Moscow: summer time to 2010, four hours east all year to 2014, three since.
        let moscow = Zone::named("Europe/Moscow").unwrap();
        let mid = |z: &Zone, y, m| z.offset_at(time::from_civil(y, m, 15, 12, 0, 0, 0));
        assert_eq!((mid(&moscow, 2009, 1), mid(&moscow, 2009, 7)), (180, 240));
        assert_eq!((mid(&moscow, 2012, 1), mid(&moscow, 2012, 7)), (240, 240));
        assert_eq!((mid(&moscow, 2020, 1), mid(&moscow, 2020, 7)), (180, 180));
        // A local time is read in the rules of its own year.
        let c = moscow.civil(moscow.from_civil(2012, 1, 15, 9, 0));
        assert_eq!((c.hour, moscow.offset_at(moscow.from_civil(2012, 1, 15, 9, 0))), (9, 240));
        // Europe ended summer time in September to 1995.
        let berlin = Zone::named("Europe/Berlin").unwrap();
        assert_eq!((mid(&berlin, 1990, 10), mid(&berlin, 2000, 10)), (60, 120));
        // Written for Outlook with a rule for each year it changed, and read back the same.
        let def = moscow.tzdefinition(true);
        assert_eq!(Reader(&def).u16(4 + usize::from(Reader(&def).u16(2).unwrap()) - 2), Some(3));
        let back = Zone::of_tzdefinition(&def).unwrap();
        assert_eq!(back, moscow);
        // Under a key not known, its eras are kept as read.
        let mut own = moscow.clone();
        own.id.clear();
        own.key = "Elsewhere Standard Time".into();
        let back = Zone::of_tzdefinition(&own.tzdefinition(false)).unwrap();
        assert_eq!((back.key.as_str(), back.earlier.len()), ("Elsewhere Standard Time", 2));
        assert!(back.keeps_as(&moscow));
    }

    #[test]
    fn zones_that_move_by_other_reckonings_are_followed_each_year() {
        // Israel: the Friday before the last Sunday of March.
        let jerusalem = Zone::named("Asia/Jerusalem").unwrap();
        for (y, d) in [(2026, 27), (2027, 26), (2025, 28), (2030, 29)] {
            let begins = jerusalem.daylight.unwrap().begins.day_in(y);
            assert_eq!(begins, time::days_from_civil(y, 3, d), "{y}");
            assert_eq!(weekday_of(begins), 5);
            assert_eq!(jerusalem.offset_at(jerusalem.from_civil(y, 3, d, 1, 59)), 120);
            assert_eq!(jerusalem.offset_at(jerusalem.from_civil(y, 3, d, 3, 0)), 180);
        }
        // Morocco: back to UTC from the Sunday before Ramadan to the Sunday after it.
        let casablanca = Zone::named("Africa/Casablanca").unwrap();
        let on = |y, m, d, h| casablanca.offset_at(time::from_civil(y, m, d, h, 0, 0, 0));
        // 2025: Ramadan from 1 March; the clocks went back on 23 February and on again on 6 April.
        assert_eq!((on(2025, 2, 23, 1), on(2025, 2, 23, 2)), (60, 0));
        assert_eq!((on(2025, 4, 6, 1), on(2025, 4, 6, 2)), (0, 60));
        assert_eq!((on(2025, 3, 15, 12), on(2025, 7, 1, 12)), (0, 60));
        // 2024: 10 March to 14 April.
        assert_eq!((on(2024, 3, 9, 12), on(2024, 3, 11, 12), on(2024, 4, 13, 12), on(2024, 4, 15, 12)), (60, 0, 0, 60));
        // 2030 has two Ramadans: early January and late December.
        assert_eq!((on(2030, 1, 15, 12), on(2030, 12, 30, 12), on(2030, 6, 1, 12)), (0, 0, 60));
        // Written for each year, as Outlook keeps such a zone.
        let rules = casablanca.yearly_rules();
        assert!(rules.len() > 60 && rules.len() < 80, "{}", rules.len());
        assert_eq!(rules.iter().find(|(y, ..)| *y == 2025).and_then(|r| r.2).map(|d| (d.begins.month, d.ends.month)), Some((2, 4)));
        assert_eq!(Zone::of_tzdefinition(&casablanca.tzdefinition(false)).unwrap(), casablanca);
    }

    #[test]
    fn the_systems_zone_is_found_from_its_offsets() {
        let now = time::from_civil(2026, 6, 1, 0, 0, 0, 0);
        for id in ["Europe/London", "America/Denver", "Australia/Sydney", "Asia/Kolkata", "Pacific/Auckland"] {
            let z = Zone::named(id).unwrap();
            let found = Zone::from_system(now, |t| z.offset_at(t));
            assert_eq!((found.bias, found.daylight), (z.bias, z.daylight), "{id}");
            assert_eq!(found.id.is_empty(), z.is_fixed(), "{id}: known by name when it saves daylight");
        }
        // A place no zone known matches keeps its offsets.
        let odd = Zone { daylight: Some(Daylight::new(30, Rule::new(4, 2, 3, 60), Rule::new(9, 3, 3, 90))), ..Zone::fixed(195) };
        let found = Zone::from_system(now, |t| odd.offset_at(t));
        assert_eq!((found.bias, found.daylight), (odd.bias, odd.daylight));
        assert!(found.id.is_empty());
        // A hostile system: anything at all is clamped and never loops for long.
        let wild = Zone::from_system(now, |t| ((t / MINUTE) % 100_000) as i32);
        assert!(wild.bias.abs() <= MAX_OFFSET);
    }

    #[test]
    fn hostile_zones_are_read_safely() {
        let mut seed = 0x1234_5678u32;
        for len in 0..400 {
            let bytes: Vec<u8> = (0..len)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed as u8
                })
                .collect();
            for z in [Zone::of_tzreg(&bytes), Zone::of_tzdefinition(&bytes)].into_iter().flatten() {
                assert!(z.bias.abs() <= MAX_OFFSET);
                let _ = z.offset_at(time::from_civil(2026, 5, 5, 0, 0, 0, 0));
                let _ = z.utc_of(i64::MAX);
                let _ = z.utc_of(i64::MIN);
            }
        }
        // A definition that claims many rules and a long name, and has neither.
        let mut lying = vec![0x02, 0x01, 0xFF, 0xFF, 0x02, 0x00, 0xFF, 0xFF];
        lying.extend_from_slice(&[0xFF; 64]);
        assert!(Zone::of_tzdefinition(&lying).is_none());
        // Extreme times.
        let berlin = Zone::named("Europe/Berlin").unwrap();
        for t in [i64::MIN, -1, 0, i64::MAX] {
            let _ = berlin.civil(t);
            let _ = berlin.utc_of(t);
            let _ = berlin.day_start(t / MINUTE);
        }
    }
}
