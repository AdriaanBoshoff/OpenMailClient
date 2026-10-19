//! Time zones with daylight saving: what offset from UTC a place keeps at each moment, and the
//! local time of a moment there and back.
//!
//! A zone is a standard offset and, where it has one, a daylight-saving period that begins and
//! ends on the same day of the same week of a month each year, as Outlook keeps a zone in an
//! item: TZREG ([MS-OXOCAL] 2.2.1.39, PidLidTimeZoneStruct) and TZDEFINITION ([MS-OXOCAL]
//! 2.2.1.41, PidLidAppointmentTimeZoneDefinitionStartDisplay, EndDisplay and Recur), and as
//! iCalendar writes one, a STANDARD and a DAYLIGHT observance repeating yearly (RFC 5545 3.6.5).
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
/// the SYSTEMTIME of a TZRULE says it with its year 0 ([MS-OXOCAL] 2.2.1.41.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rule {
    pub month: u8,
    pub week: u8,
    pub weekday: u8,
    pub minute: u16,
}

impl Rule {
    const fn new(month: u8, week: u8, weekday: u8, minute: u16) -> Self {
        Self { month, week, weekday, minute }
    }

    fn valid(&self) -> bool {
        (1..=12).contains(&self.month) && (1..=5).contains(&self.week) && self.weekday <= 6 && self.minute < 24 * 60
    }

    /// The day number (days since 1970-01-01) it falls on in `year`.
    pub fn day_in(&self, year: i64) -> i64 {
        let month = u32::from(self.month.clamp(1, 12));
        let first = time::days_from_civil(year, month, 1);
        let next = if month == 12 { time::days_from_civil(year.saturating_add(1), 1, 1) } else { time::days_from_civil(year, month + 1, 1) };
        // 1970-01-01 was a Thursday.
        let first_weekday = (first.rem_euclid(7) + 4).rem_euclid(7);
        let first_wanted = first + (i64::from(self.weekday) - first_weekday).rem_euclid(7);
        let mut day = first_wanted + 7 * (i64::from(self.week.clamp(1, 5)) - 1);
        while day >= next {
            day -= 7;
        }
        day
    }

    /// Local minutes since 1970 at which it happens in `year`.
    fn local_minutes(&self, year: i64) -> i64 {
        self.day_in(year).saturating_mul(24 * 60).saturating_add(i64::from(self.minute))
    }
}

/// Daylight saving: `save` minutes more from when `begins` (said in standard time) to when
/// `ends` (said in daylight time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Daylight {
    pub save: i32,
    pub begins: Rule,
    pub ends: Rule,
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

    /// Minutes east of UTC it keeps at `utc` (FILETIME ticks).
    pub fn offset_at(&self, utc: i64) -> i32 {
        let Some(d) = self.daylight else { return self.bias };
        let minutes = time::to_unix(utc).div_euclid(60);
        let year = time::civil(utc, self.bias).year;
        // When it begins and ends that year, in UTC minutes since 1970.
        let begins = d.begins.local_minutes(year).saturating_sub(i64::from(self.bias));
        let ends = d.ends.local_minutes(year).saturating_sub(i64::from(self.bias.saturating_add(d.save)));
        let saving = if begins <= ends { minutes >= begins && minutes < ends } else { minutes >= begins || minutes < ends };
        if saving { self.bias.saturating_add(d.save) } else { self.bias }
    }

    /// The date and time at `utc` (FILETIME ticks) here.
    pub fn civil(&self, utc: i64) -> Civil {
        time::civil(utc, self.offset_at(utc))
    }

    /// UTC FILETIME ticks of a local time here, given as ticks counted as if it were UTC. A time
    /// skipped when the clocks go forward is taken as the standard time it would have been (so
    /// 02:30 on that night is 03:30); one that happens twice when they go back, as the first.
    pub fn utc_of(&self, local: i64) -> i64 {
        let Some(d) = self.daylight else { return local.saturating_sub(i64::from(self.bias).saturating_mul(MINUTE)) };
        let standard = local.saturating_sub(i64::from(self.bias).saturating_mul(MINUTE));
        let saving = local.saturating_sub(i64::from(self.bias.saturating_add(d.save)).saturating_mul(MINUTE));
        let as_saving = self.offset_at(saving) == self.bias.saturating_add(d.save);
        let as_standard = self.offset_at(standard) == self.bias;
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

    /// As a TZREG ([MS-OXOCAL] 2.2.1.39): 48 bytes.
    pub fn tzreg(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(48);
        self.biases(&mut out);
        // wStandardYear and stStandardDate: when daylight saving ends; then wDaylightYear and
        // stDaylightDate: when it begins.
        out.extend_from_slice(&0u16.to_le_bytes());
        system_time(&mut out, self.daylight.map(|d| d.ends));
        out.extend_from_slice(&0u16.to_le_bytes());
        system_time(&mut out, self.daylight.map(|d| d.begins));
        out
    }

    /// lBias, lStandardBias and lDaylightBias: minutes west, as Windows counts.
    fn biases(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&(-self.bias).to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        out.extend_from_slice(&(-self.daylight.map_or(0, |d| d.save)).to_le_bytes());
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
            (Some(begins), Some(ends)) if save != 0 && begins.month != ends.month => Some(Daylight { save, begins, ends }),
            _ => None,
        };
        Some(Self { bias: east, daylight, ..Self::default() })
    }

    /// As a TZDEFINITION ([MS-OXOCAL] 2.2.1.41) of one rule, in force from 1601; `recur` marks it
    /// as the one a series' pattern is in (TZRULE_FLAG_RECUR_CURRENT_TZREG).
    pub fn tzdefinition(&self, recur: bool) -> Vec<u8> {
        let key: Vec<u16> = self.key_name().encode_utf16().take(MAX_KEY).collect();
        let mut out = Vec::with_capacity(16 + key.len() * 2 + 66);
        out.extend_from_slice(&[0x02, 0x01]);
        let header = 2 + 2 + key.len() * 2 + 2;
        out.extend_from_slice(&u16::try_from(header).unwrap_or(u16::MAX).to_le_bytes());
        out.extend_from_slice(&0x0002u16.to_le_bytes());
        out.extend_from_slice(&u16::try_from(key.len()).unwrap_or(0).to_le_bytes());
        for unit in &key {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out.extend_from_slice(&1u16.to_le_bytes());
        // The rule ([MS-OXOCAL] 2.2.1.41.1).
        out.extend_from_slice(&[0x02, 0x01]);
        out.extend_from_slice(&0x003Eu16.to_le_bytes());
        let flags: u16 = 0x0002 | if recur { 0x0001 } else { 0 };
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&1601u16.to_le_bytes());
        out.extend_from_slice(&[0u8; 14]);
        self.biases(&mut out);
        system_time(&mut out, self.daylight.map(|d| d.ends));
        system_time(&mut out, self.daylight.map(|d| d.begins));
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

    /// A TZDEFINITION read: the rule it marks as in force (TZRULE_FLAG_EFFECTIVE_TZREG), else
    /// its last; `None` when it cannot be read.
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
        let mut chosen = None;
        for i in 0..count {
            let at = rules_at + 2 + i * 66;
            if r.u8(at)? != 0x02 {
                break;
            }
            let flags = r.u16(at + 4)?;
            let zone = Self::of_biases(r.i32(at + 22)?, r.i32(at + 26)?, r.i32(at + 30)?, r.rule(at + 34)?, r.rule(at + 50)?);
            if let Some(z) = zone {
                let effective = flags & 0x0002 != 0;
                chosen = Some(z);
                if effective {
                    break;
                }
            }
        }
        let mut zone = chosen?;
        // A key this program knows brings its tz database name; any other is kept as it is.
        match KNOWN.iter().find(|k| k.key.eq_ignore_ascii_case(&key) && k.zone().bias == zone.bias && k.zone().daylight == zone.daylight) {
            Some(k) => zone = k.zone(),
            None if key.eq_ignore_ascii_case("UTC") && zone.bias == 0 && zone.daylight.is_none() => zone = Self::named("UTC")?,
            None => {
                zone = zone.known_as();
                zone.key = key;
            }
        }
        Some(zone)
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
            }
        };
        let zone = match changes.as_slice() {
            [(a, from_a, to_a), (b, from_b, to_b)] if to_a == from_b && from_a == to_b => {
                // The one to the larger offset begins daylight saving.
                let ((begins, std), (ends, saving)) = if to_a > from_a { ((*a, *from_a), (*b, *from_b)) } else { ((*b, *from_b), (*a, *from_a)) };
                Self {
                    bias: std,
                    daylight: Some(Daylight { save: saving - std, begins: rule(begins, std), ends: rule(ends, saving) }),
                    ..Self::default()
                }
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
        };
        Some(rule.valid().then_some(rule))
    }
}

/// A zone of the tz database, with the key Outlook knows it by and its rules today.
struct Known {
    id: &'static str,
    key: &'static str,
    bias: i32,
    daylight: Option<(i32, Rule, Rule)>,
}

impl Known {
    fn zone(&self) -> Zone {
        Zone {
            id: self.id.into(),
            key: self.key.into(),
            bias: self.bias,
            daylight: self.daylight.map(|(save, begins, ends)| Daylight { save, begins, ends }),
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
    Known { id, key, bias, daylight }
}

/// Zones whose rules are the same each year, as the tz database has them now; Outlook's key for
/// each as the Unicode CLDR maps them (windowsZones). Zones whose daylight saving moves by
/// other rules than these are not here.
const KNOWN: &[Known] = &[
    k("Pacific/Honolulu", "Hawaiian Standard Time", -600, None),
    k("America/Anchorage", "Alaskan Standard Time", -540, US),
    k("America/Los_Angeles", "Pacific Standard Time", -480, US),
    k("America/Vancouver", "Pacific Standard Time", -480, US),
    k("America/Denver", "Mountain Standard Time", -420, US),
    k("America/Edmonton", "Mountain Standard Time", -420, US),
    k("America/Phoenix", "US Mountain Standard Time", -420, None),
    k("America/Chicago", "Central Standard Time", -360, US),
    k("America/Winnipeg", "Central Standard Time", -360, US),
    k("America/Mexico_City", "Central Standard Time (Mexico)", -360, None),
    k("America/New_York", "Eastern Standard Time", -300, US),
    k("America/Toronto", "Eastern Standard Time", -300, US),
    k("America/Bogota", "SA Pacific Standard Time", -300, None),
    k("America/Lima", "SA Pacific Standard Time", -300, None),
    k("America/Halifax", "Atlantic Standard Time", -240, US),
    k("America/Caracas", "Venezuela Standard Time", -240, None),
    k("America/Santiago", "Pacific SA Standard Time", -240, Some((60, Rule::new(9, 1, SUN, 0), Rule::new(4, 1, SUN, 0)))),
    k("America/St_Johns", "Newfoundland Standard Time", -210, US),
    k("America/Sao_Paulo", "E. South America Standard Time", -180, None),
    k("America/Argentina/Buenos_Aires", "Argentina Standard Time", -180, None),
    k("Atlantic/Reykjavik", "Greenwich Standard Time", 0, None),
    k("Europe/London", "GMT Standard Time", 0, eu(0)),
    k("Europe/Dublin", "GMT Standard Time", 0, eu(0)),
    k("Europe/Lisbon", "GMT Standard Time", 0, eu(0)),
    k("Africa/Lagos", "W. Central Africa Standard Time", 60, None),
    k("Europe/Berlin", "W. Europe Standard Time", 60, eu(60)),
    k("Europe/Amsterdam", "W. Europe Standard Time", 60, eu(60)),
    k("Europe/Rome", "W. Europe Standard Time", 60, eu(60)),
    k("Europe/Stockholm", "W. Europe Standard Time", 60, eu(60)),
    k("Europe/Vienna", "W. Europe Standard Time", 60, eu(60)),
    k("Europe/Zurich", "W. Europe Standard Time", 60, eu(60)),
    k("Europe/Oslo", "W. Europe Standard Time", 60, eu(60)),
    k("Europe/Paris", "Romance Standard Time", 60, eu(60)),
    k("Europe/Madrid", "Romance Standard Time", 60, eu(60)),
    k("Europe/Brussels", "Romance Standard Time", 60, eu(60)),
    k("Europe/Copenhagen", "Romance Standard Time", 60, eu(60)),
    k("Europe/Prague", "Central Europe Standard Time", 60, eu(60)),
    k("Europe/Budapest", "Central Europe Standard Time", 60, eu(60)),
    k("Europe/Warsaw", "Central European Standard Time", 60, eu(60)),
    k("Africa/Johannesburg", "South Africa Standard Time", 120, None),
    k("Europe/Athens", "GTB Standard Time", 120, eu(120)),
    k("Europe/Bucharest", "GTB Standard Time", 120, eu(120)),
    k("Europe/Helsinki", "FLE Standard Time", 120, eu(120)),
    k("Europe/Kyiv", "FLE Standard Time", 120, eu(120)),
    k("Europe/Sofia", "FLE Standard Time", 120, eu(120)),
    k("Europe/Istanbul", "Turkey Standard Time", 180, None),
    k("Europe/Moscow", "Russian Standard Time", 180, None),
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
    k("Australia/Adelaide", "Cen. Australia Standard Time", 570, AU),
    k("Australia/Brisbane", "E. Australia Standard Time", 600, None),
    k("Australia/Sydney", "AUS Eastern Standard Time", 600, AU),
    k("Australia/Melbourne", "AUS Eastern Standard Time", 600, AU),
    k("Australia/Hobart", "Tasmania Standard Time", 600, AU),
    k("Pacific/Auckland", "New Zealand Standard Time", 720, Some((60, Rule::new(9, 5, SUN, 120), Rule::new(4, 1, SUN, 180)))),
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
            let reg = Zone::of_tzreg(&z.tzreg()).unwrap();
            assert_eq!((reg.bias, reg.daylight), (z.bias, z.daylight), "{}", z.id);
            assert_eq!(z.tzreg().len(), 48);
        }
        // A key not known is kept, with its rules.
        let mut own = Zone::fixed(5 * 60 + 30);
        own.key = "Somewhere Standard Time".into();
        assert_eq!(Zone::of_tzdefinition(&own.tzdefinition(false)).unwrap().key, "Somewhere Standard Time");
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
        let odd = Zone { daylight: Some(Daylight { save: 30, begins: Rule::new(4, 2, 3, 60), ends: Rule::new(9, 3, 3, 90) }), ..Zone::fixed(195) };
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
