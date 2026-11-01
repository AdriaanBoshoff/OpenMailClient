//! A zone's whole history from the system's own zone database: the tz files Linux, macOS and
//! the BSDs keep (TZif, RFC 8536), and the rules Windows keeps for each year a zone changed
//! (its Dynamic DST, a REG_TZI_FORMAT for each year). Either is turned into the rules a zone
//! keeps each year, which [`Zone::with_history`] lays over a zone this program knows, so that
//! an appointment of 1975 or one in Tehran in 2015 is shown at the time it was.
//!
//! A year is taken as the system has it when it can be said as Outlook says a year: one offset
//! all year, or a standard offset with one period of daylight saving. A year it cannot (two
//! Ramadans, or a zone that moved its standard offset that year) keeps the offset in force at
//! its middle, and a year of the Ramadan rules this program keeps keeps those.
//!
//! What is read is hostile: the file's counts are capped and checked against its length,
//! offsets are held to 18 hours, and years outside 1601 to 2400 are not taken.

use crate::time;
use crate::zone::{Daylight, Era, Rule, Zone};
use std::collections::BTreeMap;

/// Files larger than this are not read (the largest tz file is under 4 KiB in its slim form).
pub const MAX_FILE: usize = 1 << 20;
const MAX_TRANSITIONS: usize = 1 << 16;
const MAX_TYPES: usize = 256;
const MAX_OFFSET_SECONDS: i64 = 18 * 3600;
const FIRST_YEAR: i64 = 1601;
const LAST_YEAR: i64 = 2400;
/// The most eras a zone keeps from the system's history.
const MAX_ERAS: usize = 512;

/// What a zone keeps in a year: its standard offset (minutes east) and daylight saving.
pub type Year = (i32, Option<Daylight>);

/// A tz file read: the rules it gives for each year it changed in, and those its footer gives
/// for the years after.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    pub years: BTreeMap<i64, Year>,
    pub after: Option<Year>,
    /// The standard offset kept before the first transition, minutes east.
    pub before: Option<i32>,
}

#[derive(Clone, Copy)]
struct Type {
    offset: i64,
    dst: bool,
}

struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn u32(&self, at: usize) -> Option<u32> {
        Some(u32::from_be_bytes(self.0.get(at..at.checked_add(4)?)?.try_into().ok()?))
    }
    fn i32(&self, at: usize) -> Option<i32> {
        Some(i32::from_be_bytes(self.0.get(at..at.checked_add(4)?)?.try_into().ok()?))
    }
    fn i64(&self, at: usize) -> Option<i64> {
        Some(i64::from_be_bytes(self.0.get(at..at.checked_add(8)?)?.try_into().ok()?))
    }
}

/// The six counts of a TZif header ([RFC 8536] 3.1): isut, isstd, leap, time, type, char.
fn counts(b: &Bytes, at: usize) -> Option<[usize; 6]> {
    if b.0.get(at..at.checked_add(4)?)? != b"TZif" {
        return None;
    }
    let mut out = [0usize; 6];
    for (i, c) in out.iter_mut().enumerate() {
        *c = usize::try_from(b.u32(at + 20 + i * 4)?).ok()?;
    }
    let [_, _, leap, times, types, chars] = out;
    if times > MAX_TRANSITIONS || types == 0 || types > MAX_TYPES || leap > MAX_TRANSITIONS || chars > MAX_FILE {
        return None;
    }
    Some(out)
}

/// The length of a data block of these counts with times `wide` bytes wide (3.2).
fn block_len(c: &[usize; 6], wide: usize) -> Option<usize> {
    let [isut, isstd, leap, times, types, chars] = *c;
    times
        .checked_mul(wide + 1)?
        .checked_add(types.checked_mul(6)?)?
        .checked_add(chars)?
        .checked_add(leap.checked_mul(wide + 4)?)?
        .checked_add(isstd)?
        .checked_add(isut)
}

/// A tz file read ([RFC 8536] 3): its transitions as the rules of each year they fall in, and
/// its footer's rules for the years after the last. `None` when it is no TZif file or does
/// not hold together.
pub fn read(file: &[u8]) -> Option<History> {
    if file.len() > MAX_FILE {
        return None;
    }
    let b = Bytes(file);
    let first = counts(&b, 0)?;
    let version = *file.get(4)?;
    // Version 2 and later repeat the data with 64-bit times after the first block, then a
    // footer; the first block is then passed over.
    let (c, at, wide) = if version >= b'2' {
        let second = 44usize.checked_add(block_len(&first, 4)?)?;
        (counts(&b, second)?, second.checked_add(44)?, 8)
    } else {
        (first, 44, 4)
    };
    let [_, _, _, times, types, _] = c;
    let end = at.checked_add(block_len(&c, wide)?)?;
    if end > file.len() {
        return None;
    }
    let mut at_time = Vec::with_capacity(times);
    for i in 0..times {
        let t = if wide == 8 { b.i64(at + i * 8)? } else { i64::from(b.i32(at + i * 4)?) };
        at_time.push(t);
    }
    let kinds_at = at + times * wide;
    let types_at = kinds_at + times;
    let mut kinds = Vec::with_capacity(types);
    for i in 0..types {
        let offset = i64::from(b.i32(types_at + i * 6)?);
        if offset.abs() > MAX_OFFSET_SECONDS {
            return None;
        }
        kinds.push(Type { offset, dst: *file.get(types_at + i * 6 + 4)? != 0 });
    }
    let mut changes = Vec::with_capacity(times);
    let mut last = i64::MIN;
    for (i, t) in at_time.iter().enumerate() {
        let kind = *kinds.get(usize::from(*file.get(kinds_at + i)?))?;
        // Transitions are in order (3.2); one out of order is left out.
        if *t > last {
            changes.push((*t, kind));
            last = *t;
        }
    }
    let after = if wide == 8 { footer(file.get(end..)?).and_then(|s| posix(&s)) } else { None };
    // Before the first transition, the first type that is not daylight saving (3.2), else the first.
    let before = kinds.iter().copied().find(|k| !k.dst).or(kinds.first().copied())?;
    Some(History { years: yearly(before, &changes), after, before: Some(minutes(before.offset)) })
}

/// The footer's TZ string, between two newlines (3.3).
fn footer(rest: &[u8]) -> Option<String> {
    let rest = rest.strip_prefix(b"\n")?;
    let n = rest.iter().position(|c| *c == b'\n')?;
    let s = std::str::from_utf8(rest.get(..n)?).ok()?;
    (!s.is_empty() && s.len() < 256).then(|| s.to_string())
}

/// The rules of each year from the first transition's to the last's.
fn yearly(before: Type, changes: &[(i64, Type)]) -> BTreeMap<i64, Year> {
    let mut out = BTreeMap::new();
    let (Some(first), Some(last)) = (changes.first(), changes.last()) else { return out };
    let year_of = |t: i64, off: i64| time::civil(time::from_unix(t.saturating_add(off)), 0).year;
    let from = year_of(first.0, before.offset).max(FIRST_YEAR);
    let to = year_of(last.0, last.1.offset).min(LAST_YEAR);
    let mut i = 0usize;
    let mut now = before;
    for year in from..=to {
        // The changes whose local time (by the offset before them) falls in the year.
        let mut within: Vec<(i64, Type, Type)> = Vec::new();
        while let Some((t, kind)) = changes.get(i) {
            if year_of(*t, now.offset) > year {
                break;
            }
            if year_of(*t, now.offset) == year && (kind.offset != now.offset || kind.dst != now.dst) {
                within.push((*t, now, *kind));
            }
            now = *kind;
            i += 1;
        }
        if within.len() > 8 {
            continue;
        }
        let start_kind = within.first().map_or(now, |w| w.1);
        let middle = mid_year(year, start_kind, &within);
        out.insert(year, of_year(start_kind, &within).unwrap_or((minutes(middle.offset), None)));
    }
    out
}

fn minutes(seconds: i64) -> i32 {
    i32::try_from(seconds.div_euclid(60)).unwrap_or(0)
}

/// The type in force on the first of July of `year`.
fn mid_year(year: i64, start: Type, within: &[(i64, Type, Type)]) -> Type {
    let july = time::days_from_civil(year, 7, 1).saturating_mul(86_400);
    within.iter().rev().find(|(t, _, _)| *t <= july).map_or(start, |w| w.2)
}

/// A year said as Outlook says one: one offset all year, or a standard offset with one period
/// of daylight saving. `None` for any other.
fn of_year(start: Type, within: &[(i64, Type, Type)]) -> Option<Year> {
    match within {
        [] if !start.dst => Some((minutes(start.offset), None)),
        [] => None,
        [a, b] => {
            let ((on, std, dst), (off, dst2, std2)) = if !a.1.dst && a.2.dst {
                ((a.0, a.1, a.2), (b.0, b.1, b.2))
            } else if a.1.dst && !a.2.dst {
                ((b.0, b.1, b.2), (a.0, a.1, a.2))
            } else {
                return None;
            };
            if std.offset != std2.offset || dst.offset != dst2.offset || !dst.dst || dst2.dst != dst.dst || std.dst || std2.dst {
                return None;
            }
            let save = minutes(dst.offset - std.offset);
            if save == 0 || save.abs() > 180 {
                return None;
            }
            // Begins said in standard time, ends in daylight time.
            let begins = rule_at(on, std.offset)?;
            let ends = rule_at(off, dst.offset)?;
            (begins.month != ends.month).then(|| (minutes(std.offset), Some(Daylight::new(save, begins, ends))))
        }
        _ => None,
    }
}

fn rule_at(t: i64, offset: i64) -> Option<Rule> {
    let local = t.checked_add(offset)?;
    let day = local.div_euclid(86_400);
    let minute = u16::try_from(local.rem_euclid(86_400) / 60).ok()?;
    Some(Rule::of_day(day, minute))
}

/// A POSIX TZ string's rules ([RFC 8536] 3.3, POSIX.1 8.3), as "CET-1CEST,M3.5.0,M10.5.0/3":
/// its standard offset and, where it names one by month, week and day, its daylight saving.
pub fn posix(s: &str) -> Option<Year> {
    let mut p = Posix { s: s.as_bytes(), at: 0 };
    p.name()?;
    let std = p.offset()?.checked_neg()?;
    if p.done() {
        return Some((minutes(std), None));
    }
    p.name()?;
    let dst = if p.peek() == Some(b',') { std + 3600 } else { p.offset()?.checked_neg()? };
    if !p.eat(b',') {
        return None;
    }
    let begins = p.rule()?;
    if !p.eat(b',') {
        return None;
    }
    let ends = p.rule()?;
    if !p.done() || std.abs() > MAX_OFFSET_SECONDS || dst.abs() > MAX_OFFSET_SECONDS {
        return None;
    }
    let save = minutes(dst - std);
    (save != 0 && save.abs() <= 180 && begins.month != ends.month).then(|| (minutes(std), Some(Daylight::new(save, begins, ends))))
}

struct Posix<'a> {
    s: &'a [u8],
    at: usize,
}

impl Posix<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.at).copied()
    }
    fn done(&self) -> bool {
        self.at >= self.s.len()
    }
    fn eat(&mut self, c: u8) -> bool {
        let yes = self.peek() == Some(c);
        self.at += usize::from(yes);
        yes
    }
    /// A zone's abbreviation: three or more letters, or anything within `<` and `>`.
    fn name(&mut self) -> Option<()> {
        let from = self.at;
        if self.eat(b'<') {
            while self.peek().is_some_and(|c| c != b'>') {
                self.at += 1;
            }
            return self.eat(b'>').then_some(());
        }
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.at += 1;
        }
        (self.at - from >= 3).then_some(())
    }
    fn number(&mut self) -> Option<i64> {
        let from = self.at;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) && self.at - from < 3 {
            self.at += 1;
        }
        std::str::from_utf8(self.s.get(from..self.at)?).ok()?.parse().ok()
    }
    /// `[+-]hh[:mm[:ss]]` in seconds, west of UTC as POSIX says it.
    fn offset(&mut self) -> Option<i64> {
        let sign = if self.eat(b'-') {
            -1
        } else {
            self.eat(b'+');
            1
        };
        let mut seconds = self.number()?.checked_mul(3600)?;
        if self.eat(b':') {
            seconds += self.number()?.checked_mul(60)?;
            if self.eat(b':') {
                seconds += self.number()?;
            }
        }
        Some(sign * seconds)
    }
    /// `Mm.w.d[/time]`; the day-of-year forms are not taken.
    fn rule(&mut self) -> Option<Rule> {
        if !self.eat(b'M') {
            return None;
        }
        let month = u8::try_from(self.number()?).ok().filter(|m| (1..=12).contains(m))?;
        let week = if self.eat(b'.') { u8::try_from(self.number()?).ok().filter(|w| (1..=5).contains(w))? } else { return None };
        let weekday = if self.eat(b'.') { u8::try_from(self.number()?).ok().filter(|d| *d <= 6)? } else { return None };
        let minute = if self.eat(b'/') { self.offset()? / 60 } else { 120 };
        // RFC 8536 3.3.1 allows hours from -167 to 167; this program keeps a change within its day.
        let minute = u16::try_from(minute).ok().filter(|m| *m < 24 * 60)?;
        Some(Rule { month, week, weekday, minute, day: 0 })
    }
}

/// The rules Windows keeps for one year of a zone's Dynamic DST: a REG_TZI_FORMAT, 44 bytes
/// (Bias, StandardBias, DaylightBias, StandardDate, DaylightDate), as a TZREG without its years.
pub fn of_tzi(b: &[u8]) -> Option<Year> {
    let b = b.get(..44)?;
    let tzreg: Vec<u8> = [b.get(..12)?, &[0, 0], b.get(12..28)?, &[0, 0], b.get(28..44)?].concat();
    let zone = Zone::of_tzreg_alone(&tzreg)?;
    Some((zone.bias, zone.daylight))
}

impl Zone {
    /// This zone with the rules the system's history gives each year laid over its own: every
    /// year from 1601 to the last the history names is as the history says (one offset all year
    /// before its first change), but for a year this zone keeps by Ramadan. The years after keep its own rules; a zone that has none of
    /// its own (`self` known only by name) takes those the history gives for after.
    pub fn with_history(&self, history: &History) -> Zone {
        let mut base = self.clone();
        if base.bias == 0
            && base.daylight.is_none()
            && base.earlier.is_empty()
            && let Some((bias, daylight)) = history.after
        {
            base.bias = bias;
            base.daylight = daylight;
        }
        let (Some(first), Some(last)) = (history.years.keys().next().copied(), history.years.keys().next_back().copied()) else {
            return base;
        };
        let hi = last.max(base.since().saturating_sub(1)).min(LAST_YEAR);
        let mut eras: Vec<Era> = Vec::new();
        for year in FIRST_YEAR..=hi {
            let own = base.rules_in(year);
            let ramadan = own.1.is_some_and(|d| d.ramadan);
            let (bias, daylight) = match (history.years.get(&year), history.before) {
                _ if ramadan => own,
                (Some(y), _) => *y,
                (None, Some(before)) if year < first => (before, None),
                _ => own,
            };
            let full = eras.len() >= MAX_ERAS;
            match eras.last_mut() {
                Some(e) if e.bias == bias && e.daylight == daylight => e.until = year,
                _ if !full => eras.push(Era { until: year, bias, daylight }),
                _ => {}
            }
        }
        // The last era, where it keeps the rules of today, is today's.
        if eras.last().is_some_and(|e| e.bias == base.bias && e.daylight == base.daylight) {
            eras.pop();
        }
        Zone { earlier: eras, ..base }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUN: u8 = 0;

    /// A version 2 tz file of our own: types (offset seconds, daylight saving), transitions
    /// (unix seconds, type) and a footer.
    fn tzif(types: &[(i32, bool)], changes: &[(i64, u8)], footer: &str) -> Vec<u8> {
        let header = |times: usize, types: usize, chars: usize| {
            let mut h = b"TZif2".to_vec();
            h.extend_from_slice(&[0; 15]);
            for c in [0, 0, 0, times, types, chars] {
                h.extend_from_slice(&u32::try_from(c).unwrap().to_be_bytes());
            }
            h
        };
        // The first block: one type, as slim files have it.
        let mut out = header(0, 1, 1);
        out.extend_from_slice(&[0; 6]);
        out.push(0);
        out.extend(header(changes.len(), types.len(), 1));
        for (t, _) in changes {
            out.extend_from_slice(&t.to_be_bytes());
        }
        out.extend(changes.iter().map(|(_, k)| *k));
        for (offset, dst) in types {
            out.extend_from_slice(&offset.to_be_bytes());
            out.extend_from_slice(&[u8::from(*dst), 0]);
        }
        out.push(0);
        out.push(b'\n');
        out.extend_from_slice(footer.as_bytes());
        out.push(b'\n');
        out
    }

    fn unix(year: i64, month: u32, day: u32, hour: i64) -> i64 {
        time::days_from_civil(year, month, day) * 86_400 + hour * 3600
    }

    fn at(zone: &Zone, year: i64, month: u32, day: u32) -> i32 {
        zone.offset_at(time::from_unix(unix(year, month, day, 12)))
    }

    /// Tehran's summer time of 2008 to 2022, from the first day of spring to that of autumn
    /// by the Persian calendar (21 or 22 March and September, at midnight local time).
    fn tehran() -> Vec<u8> {
        let mut changes = Vec::new();
        for year in 2008..=2022 {
            let day = if year % 4 == 0 || year % 4 == 1 { 21 } else { 22 };
            // 00:00 standard (+03:30) is 20:30 the day before in UTC; 00:00 daylight (+04:30) back.
            changes.push((unix(year, 3, day, 0) - 12_600, 1));
            changes.push((unix(year, 9, day, 0) - 16_200, 0));
        }
        tzif(&[(12_600, false), (16_200, true)], &changes, "<+0330>-3:30")
    }

    #[test]
    fn irans_summer_time_comes_from_the_system() {
        let history = read(&tehran()).unwrap();
        assert_eq!(history.after, Some((210, None)));
        assert_eq!(history.years.len(), 15);
        let built_in = Zone::named("Asia/Tehran").unwrap();
        assert_eq!(at(&built_in, 2015, 7, 1), 210, "what we knew alone");
        let zone = built_in.with_history(&history);
        assert_eq!((zone.id.as_str(), zone.key.as_str()), ("Asia/Tehran", "Iran Standard Time"));
        assert_eq!(at(&zone, 2015, 7, 1), 270);
        assert_eq!(at(&zone, 2015, 12, 1), 210);
        assert_eq!(at(&zone, 2007, 7, 1), 210);
        assert_eq!(at(&zone, 2023, 7, 1), 210);
        // The days move with the Persian calendar: each year said as it falls.
        let y2012 = zone.rules_in(2012).1.unwrap();
        assert_eq!((y2012.begins.month, y2012.save), (3, 60));
        // Written for Outlook and read back, each of those years kept.
        let back = Zone::of_tzdefinition(&zone.tzdefinition(false)).unwrap();
        for year in [2008, 2015, 2022] {
            assert_eq!(at(&back, year, 7, 1), 270, "{year}");
        }
        assert_eq!(at(&back, 2023, 7, 1), 210);
    }

    #[test]
    fn rules_before_the_1980s_and_the_footers_for_after() {
        // A place at +01:00 with summer time from the last Sunday of March to the last of
        // September in 1980 to 1995, none before, and the European rules from 1996.
        let mut changes = Vec::new();
        for year in 1980..=2000 {
            let last_sunday = |month: u32, len: u32| {
                let last = time::days_from_civil(year, month, len);
                last - (last.rem_euclid(7) + 4).rem_euclid(7)
            };
            let ends = if year <= 1995 { last_sunday(9, 30) } else { last_sunday(10, 31) };
            changes.push((last_sunday(3, 31) * 86_400 + 3600, 1));
            changes.push((ends * 86_400 + 3600, 0));
        }
        let file = tzif(&[(3600, false), (7200, true)], &changes, "CET-1CEST,M3.5.0,M10.5.0/3");
        let history = read(&file).unwrap();
        let zone = Zone::named("Europe/Berlin").unwrap().with_history(&history);
        assert_eq!(at(&zone, 1975, 7, 1), 60, "no summer time before 1980");
        assert_eq!(at(&zone, 1985, 7, 1), 120);
        assert_eq!(at(&zone, 1985, 10, 10), 60, "it ended in September then");
        assert_eq!(at(&zone, 1999, 10, 10), 120);
        assert_eq!(at(&zone, 2026, 10, 10), 120);
        assert_eq!(zone.rules_in(1985).1.unwrap().ends.month, 9);
        // A zone known by no name of ours takes the footer's rules for today.
        let none = Zone { id: "Somewhere/Else".into(), ..Zone::default() }.with_history(&history);
        assert_eq!(at(&none, 2026, 7, 1), 120);
        assert_eq!(at(&none, 2026, 12, 1), 60);
    }

    #[test]
    fn a_ramadan_kept_by_its_own_rules() {
        // A file that says Casablanca kept +01:00 all year from 2018: the Ramadan rules of ours stay.
        let file = tzif(&[(0, false), (3600, false)], &[(unix(2018, 10, 28, 2), 1), (unix(2030, 1, 1, 0), 1)], "<+01>-1");
        let ours = Zone::named("Africa/Casablanca").unwrap();
        let zone = ours.with_history(&read(&file).unwrap());
        for year in 2020..=2030 {
            assert_eq!(zone.rules_in(year), ours.rules_in(year), "{year}");
        }
    }

    #[test]
    fn posix_rules_read() {
        let eu = posix("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        let rule = |month, minute| Rule { month, week: 5, weekday: SUN, minute, day: 0 };
        assert_eq!(eu, (60, Some(Daylight::new(60, rule(3, 120), rule(10, 180)))));
        assert_eq!(posix("EST5EDT,M3.2.0,M11.1.0").unwrap().0, -300);
        assert_eq!(posix("<-03>3"), Some((-180, None)));
        assert_eq!(posix("<+0545>-5:45"), Some((345, None)));
        let south = posix("AEST-10AEDT,M10.1.0,M4.1.0/3").unwrap();
        assert_eq!((south.0, south.1.unwrap().begins.month), (600, 10));
        assert_eq!(posix("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0").unwrap().1.unwrap().save, 30);
        // Forms this program does not keep, and nonsense.
        for s in ["", "UT", "EST", "CET-1CEST,M3.5.0", "CET-1CEST,M13.5.0,M10.5.0", "<abc", "XXX-99:99", "CET-1CEST,M3.5.0/-1,M10.5.0/25"] {
            let _ = posix(s);
        }
        assert_eq!(posix("CET-1CEST,J60,J300"), None);
        assert_eq!(posix("CET-1CEST,M3.5.0/26,M10.5.0"), None);
    }

    #[test]
    fn windows_years_read() {
        // Pacific time of 2006: Bias 480, StandardBias 0, DaylightBias -60; ends on the last
        // Sunday of October at 02:00, begins on the first Sunday of April at 02:00.
        let mut tzi = Vec::new();
        for v in [480i32, 0, -60] {
            tzi.extend_from_slice(&v.to_le_bytes());
        }
        for (month, week) in [(10u16, 5u16), (4, 1)] {
            for v in [0u16, month, 0, week, 2, 0, 0, 0] {
                tzi.extend_from_slice(&v.to_le_bytes());
            }
        }
        let (bias, d) = of_tzi(&tzi).unwrap();
        let d = d.unwrap();
        assert_eq!((bias, d.save, d.begins.month, d.begins.week, d.ends.month), (-480, 60, 4, 1, 10));
        assert_eq!(of_tzi(&tzi[..40]), None);
    }

    #[test]
    fn hostile_tz_files_are_turned_away() {
        let good = tehran();
        for n in 0..good.len() {
            let _ = read(&good[..n]);
        }
        assert!(read(&good[..good.len() - 1]).is_some_and(|h| h.after.is_none()), "a footer without its end");
        // The second header, after the first and its block of one type and one character.
        let second = 44 + 6 + 1;
        let mut huge = good.clone();
        huge[second + 32..second + 36].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(read(&huge), None);
        let mut types = good.clone();
        types[second + 36..second + 40].copy_from_slice(&0u32.to_be_bytes());
        assert_eq!(read(&types), None);
        // A transition naming a type there is not.
        let mut kind = good.clone();
        kind[second + 44 + 30 * 8] = 9;
        assert_eq!(read(&kind), None);
        // An offset beyond 18 hours.
        assert_eq!(read(&tzif(&[(90_000, false)], &[(0, 0)], "UTC0")), None);
        // Out of order, far outside the years kept, and garbage.
        let odd = tzif(&[(0, false), (3600, true)], &[(i64::MAX, 1), (i64::MIN, 0), (0, 1)], "");
        let _ = read(&odd).map(|h| Zone::utc().with_history(&h));
        let far = tzif(&[(0, false), (3600, true)], &[(i64::MIN / 2, 1), (i64::MAX / 2, 0)], "UTC0");
        assert!(read(&far).unwrap().years.len() <= 800, "the years kept, and no more");
        let mut x = 0x1234_5678u32;
        for _ in 0..200 {
            let mut junk = good.clone();
            for _ in 0..8 {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let i = x as usize % junk.len();
                junk[i] = (x >> 8) as u8;
            }
            if let Some(h) = read(&junk) {
                let _ = Zone::named("Asia/Tehran").unwrap().with_history(&h).tzdefinition(false);
            }
        }
        assert_eq!(read(b"TZif"), None);
        assert_eq!(read(&vec![0; MAX_FILE + 1]), None);
    }
}
