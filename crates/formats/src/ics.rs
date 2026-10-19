//! iCalendar files (RFC 5545): events read into appointments, and appointments written as
//! events, as [MS-OXCICAL] maps the one onto the other.
//!
//! A calendar file comes from anyone: every line, list, component and count is held to a
//! limit, nesting is bounded, and what cannot be kept (a pattern this program cannot repeat, a
//! time zone it does not know) is said in a note rather than guessed at silently. Nothing in a
//! file is run or fetched: an alarm that would play a sound or send mail is read only as a
//! reminder, and attachments and links stay text.

use openmailclient_model::calendar::{self, Appointment, Busy, DAY, MAX_DAYS, MINUTE};
use openmailclient_model::recur::{self, End, Freq, Recurrence, Rule};
use openmailclient_model::zone::{Daylight, Rule as ZoneRule, Zone};
use openmailclient_model::{Item, time};

/// Largest file read, in bytes.
pub const MAX_BYTES: usize = 32 << 20;
/// Longest line kept once unfolded, in bytes; past that a line is cut.
const MAX_LINE: usize = 256 << 10;
/// Most components read from one file, and events taken from it.
const MAX_COMPONENTS: usize = 50_000;
pub const MAX_EVENTS: usize = 20_000;
/// Deepest nesting of components read.
const MAX_DEPTH: usize = 8;
/// Most properties read of one component, and parameters of one property.
const MAX_PROPS: usize = 2_000;
const MAX_PARAMS: usize = 64;
/// Most values read of one list (EXDATE, BYDAY, CATEGORIES…).
const MAX_LIST: usize = 1_000;
/// Longest subject or location kept, in characters, and longest description.
const MAX_TEXT: usize = 2_000;
const MAX_BODY: usize = 200_000;
/// Most notes kept of one file.
const MAX_NOTES: usize = 200;
/// Most occurrences deleted or changed in all the series of one file.
const MAX_CHANGES: usize = 20_000;

/// A property: its name in capitals, its parameters, its value as written.
#[derive(Clone, Debug, Default)]
struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// A component (VCALENDAR, VEVENT, VTIMEZONE, VALARM…) and those within it.
#[derive(Clone, Debug, Default)]
struct Comp {
    name: String,
    props: Vec<Prop>,
    children: Vec<Comp>,
}

impl Comp {
    fn prop(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name == name)
    }
    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Prop> + 'a {
        self.props.iter().filter(move |p| p.name == name)
    }
    fn text(&self, name: &str, most: usize) -> Option<String> {
        self.prop(name).map(|p| cut(&unescape(&p.value), most))
    }
}

/// `s` cut to at most `most` characters.
fn cut(s: &str, most: usize) -> String {
    match s.char_indices().nth(most) {
        Some((at, _)) => s.get(..at).unwrap_or_default().to_string(),
        None => s.to_string(),
    }
}

/// The lines of a file with folded lines joined again (RFC 5545 3.1), each one cut to
/// `MAX_LINE` bytes. Folding is undone on bytes, so a character split across two lines is
/// whole again before the line is read as UTF-8.
fn unfold(bytes: &[u8]) -> Vec<String> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let mut lines: Vec<Vec<u8>> = Vec::new();
    for raw in bytes.split(|b| *b == b'\n') {
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        match (raw.first(), lines.last_mut()) {
            (Some(b' ' | b'\t'), Some(last)) => {
                let more = raw.get(1..).unwrap_or_default();
                let room = MAX_LINE.saturating_sub(last.len());
                last.extend_from_slice(more.get(..more.len().min(room)).unwrap_or_default());
            }
            _ if raw.is_empty() => {}
            _ => lines.push(raw.get(..raw.len().min(MAX_LINE)).unwrap_or_default().to_vec()),
        }
    }
    lines.into_iter().map(|l| String::from_utf8_lossy(&l).into_owned()).collect()
}

/// A content line read (RFC 5545 3.1): `NAME;PARAM=value;PARAM="quoted":value`. `None` when
/// it has no colon outside quotes.
fn content_line(line: &str) -> Option<Prop> {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() && b.get(i).is_some_and(|c| *c != b';' && *c != b':') {
        i += 1;
    }
    let name = line.get(..i)?.trim().to_ascii_uppercase();
    let mut params = Vec::new();
    while b.get(i) == Some(&b';') {
        let start = i + 1;
        let mut j = start;
        while b.get(j).is_some_and(|c| *c != b'=' && *c != b':' && *c != b';') {
            j += 1;
        }
        let pname = line.get(start..j)?.trim().to_ascii_uppercase();
        let mut value = String::new();
        if b.get(j) == Some(&b'=') {
            j += 1;
            // Values, comma-separated, each quoted or not.
            loop {
                if b.get(j) == Some(&b'"') {
                    let open = j + 1;
                    let close = (open..b.len()).find(|k| b.get(*k) == Some(&b'"'))?;
                    value.push_str(line.get(open..close)?);
                    j = close + 1;
                } else {
                    let open = j;
                    while b.get(j).is_some_and(|c| *c != b',' && *c != b':' && *c != b';') {
                        j += 1;
                    }
                    value.push_str(line.get(open..j)?);
                }
                if b.get(j) == Some(&b',') {
                    value.push(',');
                    j += 1;
                } else {
                    break;
                }
            }
        }
        if params.len() < MAX_PARAMS {
            params.push((pname, value));
        }
        i = j;
    }
    if b.get(i) != Some(&b':') {
        return None;
    }
    Some(Prop { name, params, value: line.get(i + 1..)?.to_string() })
}

/// TEXT unescaped (RFC 5545 3.3.11).
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// TEXT escaped (RFC 5545 3.3.11).
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c if c.is_control() && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// A list of TEXT values split at commas that are not escaped, each unescaped.
fn text_list(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut part = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                part.push('\\');
                part.extend(chars.next());
            }
            ',' => out.push(std::mem::take(&mut part)),
            c => part.push(c),
        }
        if out.len() >= MAX_LIST {
            break;
        }
    }
    out.push(part);
    out.into_iter().map(|p| cut(unescape(&p).trim(), MAX_TEXT)).filter(|p| !p.is_empty()).take(MAX_LIST).collect()
}

/// The components of a file, outermost first. Nesting deeper than `MAX_DEPTH` is passed over;
/// an END that closes nothing open is passed over; a file that stops inside a component keeps
/// what it had.
fn components(bytes: &[u8]) -> (Vec<Comp>, bool) {
    let mut roots = Vec::new();
    let mut stack: Vec<Comp> = Vec::new();
    let mut skipped = 0usize;
    let mut count = 0usize;
    for line in unfold(bytes) {
        let Some(p) = content_line(&line) else { continue };
        match p.name.as_str() {
            "BEGIN" => {
                if skipped > 0 || stack.len() >= MAX_DEPTH || count >= MAX_COMPONENTS {
                    skipped += 1;
                } else {
                    count += 1;
                    stack.push(Comp { name: p.value.trim().to_ascii_uppercase(), ..Comp::default() });
                }
            }
            "END" if skipped > 0 => skipped -= 1,
            "END" => {
                let name = p.value.trim().to_ascii_uppercase();
                if !stack.iter().any(|c| c.name == name) {
                    continue;
                }
                while let Some(c) = stack.pop() {
                    let done = c.name == name;
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(c),
                        None => roots.push(c),
                    }
                    if done {
                        break;
                    }
                }
            }
            _ if skipped > 0 => {}
            _ => {
                if let Some(top) = stack.last_mut()
                    && top.props.len() < MAX_PROPS
                {
                    top.props.push(p);
                }
            }
        }
    }
    let cut_short = !stack.is_empty();
    while let Some(c) = stack.pop() {
        match stack.last_mut() {
            Some(parent) => parent.children.push(c),
            None => roots.push(c),
        }
    }
    (roots, cut_short)
}

/// A DATE or DATE-TIME value (RFC 5545 3.3.4 and 3.3.5) as written: its local day, seconds
/// into that day, and whether it was a date alone or a time in UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    day: i64,
    secs: i64,
    date_only: bool,
    utc: bool,
}

fn digits(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// The day number of a date, `None` when there is no such date or it is outside 1601 to 9999.
fn day_number(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1601..=9999).contains(&year) || !(1..=12).contains(&month) || day < 1 || day > calendar::days_in(year, month) {
        return None;
    }
    Some(time::to_unix(time::from_civil(year, month, day, 0, 0, 0, 0)).div_euclid(86_400))
}

fn stamp(value: &str) -> Option<Stamp> {
    let v = value.trim();
    let date = v.get(..8)?;
    let day = day_number(i64::from(digits(date.get(..4)?)?), digits(date.get(4..6)?)?, digits(date.get(6..8)?)?)?;
    let rest = v.get(8..)?;
    if rest.is_empty() {
        return Some(Stamp { day, secs: 0, date_only: true, utc: false });
    }
    let rest = rest.strip_prefix('T').or_else(|| rest.strip_prefix('t'))?;
    let (clock, utc) = match rest.strip_suffix('Z').or_else(|| rest.strip_suffix('z')) {
        Some(c) => (c, true),
        None => (rest, false),
    };
    let (h, m) = (digits(clock.get(..2)?)?, digits(clock.get(2..4)?)?);
    let s = match clock.get(4..) {
        Some("") | None => 0,
        Some(sec) => digits(sec.get(..2)?)?,
    };
    if h > 23 || m > 59 || s > 60 {
        return None;
    }
    Some(Stamp { day, secs: i64::from(h * 3600 + m * 60 + s.min(59)), date_only: false, utc })
}

/// A UTC offset (RFC 5545 3.3.14), `+HHMM` or `+HHMMSS`, in minutes east; held to 18 hours.
fn utc_offset(v: &str) -> Option<i64> {
    let v = v.trim();
    let (sign, rest) = match v.get(..1)? {
        "+" => (1, v.get(1..)?),
        "-" => (-1, v.get(1..)?),
        _ => return None,
    };
    let h = i64::from(digits(rest.get(..2)?)?);
    let m = i64::from(digits(rest.get(2..4)?)?);
    let total = h * 60 + m;
    (total <= 18 * 60).then_some(sign * total)
}

/// A DURATION (RFC 5545 3.3.6) in seconds, held to a year either way.
fn duration(v: &str) -> Option<i64> {
    let v = v.trim();
    let (sign, v) = match v.get(..1)? {
        "-" => (-1, v.get(1..)?),
        "+" => (1, v.get(1..)?),
        _ => (1, v),
    };
    let v = v.strip_prefix('P').or_else(|| v.strip_prefix('p'))?;
    let mut total: i64 = 0;
    let mut number = String::new();
    let mut in_time = false;
    let mut any = false;
    for c in v.chars() {
        match c.to_ascii_uppercase() {
            'T' => in_time = true,
            d if d.is_ascii_digit() => {
                if number.len() >= 9 {
                    return None;
                }
                number.push(d);
            }
            unit => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                let secs = match (unit, in_time) {
                    ('W', false) => 7 * 86_400,
                    ('D', false) => 86_400,
                    ('H', true) => 3600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
                total = total.saturating_add(n.saturating_mul(secs));
                any = true;
            }
        }
    }
    (any && number.is_empty()).then(|| (sign * total).clamp(-366 * 86_400, 366 * 86_400))
}

/// Weekday codes (RFC 5545 3.3.10), Sunday first.
const WEEKDAYS: [&str; 7] = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"];

fn weekday_code(s: &str) -> Option<u32> {
    WEEKDAYS.iter().position(|w| w.eq_ignore_ascii_case(s)).and_then(|i| u32::try_from(i).ok())
}

/// A BYDAY entry: its ordinal (0 when none) and its weekday.
fn by_day(s: &str) -> Option<(i32, u32)> {
    let s = s.trim();
    let at = s.len().checked_sub(2)?;
    let day = weekday_code(s.get(at..)?)?;
    let n = s.get(..at)?;
    let n = match n {
        "" => 0,
        n => {
            let v: i32 = n.strip_prefix('+').unwrap_or(n).parse().ok()?;
            if v == 0 || !(-53..=53).contains(&v) {
                return None;
            }
            v
        }
    };
    Some((n, day))
}

/// One observance of a time zone (RFC 5545 3.6.5): from when, and the offsets before and after.
#[derive(Clone, Debug)]
struct Onset {
    /// The local day and second it begins, in the offset before it.
    day: i64,
    secs: i64,
    from: i64,
    to: i64,
    /// How it comes again each year: its month, and the weekday and which of them, or the day
    /// of the month; and the last year it does.
    yearly: Option<(u32, Option<(i32, u32)>, Option<i32>, Option<i64>)>,
    /// Other days it begins (RDATE).
    more: Vec<(i64, i64)>,
    /// A DAYLIGHT observance, not a STANDARD one.
    daylight: bool,
}

/// A time zone a file defines (VTIMEZONE), by its TZID.
#[derive(Clone, Debug)]
struct FileZone {
    id: String,
    onsets: Vec<Onset>,
}

fn ymd(day: i64) -> (i64, u32, u32) {
    let c = time::civil(time::from_unix(day.saturating_mul(86_400)), 0);
    (c.year, c.month, c.day)
}

/// The day in `year` and `month` a yearly onset falls on.
fn onset_day(year: i64, month: u32, nth: Option<(i32, u32)>, month_day: Option<i32>) -> Option<i64> {
    let last = calendar::days_in(year, month);
    if let Some(d) = month_day {
        let d = if d < 0 { i64::from(last) + 1 + i64::from(d) } else { i64::from(d) };
        return day_number(year, month, u32::try_from(d).ok()?.clamp(1, last));
    }
    let (n, wd) = nth?;
    let hits: Vec<i64> = (1..=last).filter_map(|d| day_number(year, month, d)).filter(|d| recur::weekday(*d) == wd).collect();
    if n < 0 {
        hits.len().checked_sub(n.unsigned_abs() as usize).and_then(|i| hits.get(i)).copied()
    } else {
        hits.get((n as usize).saturating_sub(1)).copied()
    }
}

impl FileZone {
    fn of(c: &Comp) -> Option<Self> {
        let id = c.prop("TZID")?.value.trim().to_string();
        let mut onsets = Vec::new();
        for o in c.children.iter().filter(|o| o.name == "STANDARD" || o.name == "DAYLIGHT").take(MAX_LIST) {
            let Some(start) = o.prop("DTSTART").and_then(|p| stamp(&p.value)) else { continue };
            let Some(to) = o.prop("TZOFFSETTO").and_then(|p| utc_offset(&p.value)) else { continue };
            let from = o.prop("TZOFFSETFROM").and_then(|p| utc_offset(&p.value)).unwrap_or(to);
            let yearly = o.prop("RRULE").and_then(|p| {
                let rule = rule_parts(&p.value);
                let get = |k: &str| rule.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
                if !get("FREQ").is_some_and(|f| f.eq_ignore_ascii_case("YEARLY")) {
                    return None;
                }
                let month = get("BYMONTH").and_then(digits).filter(|m| (1..=12).contains(m)).unwrap_or(ymd(start.day).1);
                let nth = get("BYDAY").and_then(by_day).map(|(n, d)| (if n == 0 { 1 } else { n }, d));
                let month_day = get("BYMONTHDAY").and_then(|v| v.parse::<i32>().ok()).filter(|d| (-31..=31).contains(d) && *d != 0);
                let until = get("UNTIL").and_then(stamp).map(|s| ymd(s.day).0);
                Some((month, nth, month_day, until))
            });
            let more = o
                .all("RDATE")
                .flat_map(|p| p.value.split(',').map(str::to_string).collect::<Vec<_>>())
                .take(MAX_LIST)
                .filter_map(|v| stamp(&v))
                .map(|s| (s.day, s.secs))
                .collect();
            onsets.push(Onset { day: start.day, secs: start.secs, from, to, yearly, more, daylight: o.name == "DAYLIGHT" });
        }
        (!onsets.is_empty()).then_some(Self { id, onsets })
    }

    /// Minutes east of UTC the zone is at the local second `local` (seconds since 1970 on the
    /// zone's own clock): that of the observance begun last before it.
    fn offset_at(&self, local: i64) -> i64 {
        let year = ymd(local.div_euclid(86_400)).0;
        let mut best: Option<(i64, i64)> = None;
        let mut earliest: Option<(i64, i64)> = None;
        for o in &self.onsets {
            let first = o.day.saturating_mul(86_400).saturating_add(o.secs);
            if earliest.is_none_or(|(t, _)| first < t) {
                earliest = Some((first, o.from));
            }
            let mut times = vec![first];
            if let Some((month, nth, month_day, until)) = o.yearly {
                for y in [year - 1, year] {
                    if until.is_some_and(|u| y > u) || y < ymd(o.day).0 {
                        continue;
                    }
                    if let Some(d) = onset_day(y, month, nth, month_day) {
                        times.push(d.saturating_mul(86_400).saturating_add(o.secs));
                    }
                }
            }
            times.extend(o.more.iter().map(|(d, s)| d.saturating_mul(86_400).saturating_add(*s)));
            for t in times.into_iter().filter(|t| *t <= local && *t >= first) {
                if best.is_none_or(|(b, _)| t > b) {
                    best = Some((t, o.to));
                }
            }
        }
        best.or(earliest).map_or(0, |(_, off)| off)
    }

    /// The zone as one this program keeps: one offset, or a standard and a daylight time that
    /// begin on the same weekday of the same month each year (the observances that go on
    /// repeating, where a zone gives its past ones too). Known by name when it is one of those
    /// known; `None` when its rules are of another kind.
    fn model(&self) -> Option<Zone> {
        let in_range = |o: i64| o.abs() <= 18 * 60;
        let current: Vec<&Onset> = self.onsets.iter().filter(|o| o.yearly.is_some_and(|(.., until)| until.is_none())).collect();
        let zone = match current.as_slice() {
            [] => {
                // One offset all along: the observance begun last.
                let last = self.onsets.iter().max_by_key(|o| (o.day, o.secs))?;
                if !in_range(last.to) || self.onsets.iter().any(|o| o.more.iter().any(|(d, _)| *d > last.day)) {
                    return None;
                }
                Zone::fixed(i32::try_from(last.to).ok()?)
            }
            [a, b] if a.daylight != b.daylight => {
                let (standard, daylight) = if a.daylight { (*b, *a) } else { (*a, *b) };
                let rule = |o: &Onset| -> Option<ZoneRule> {
                    let (month, nth, month_day, _) = o.yearly?;
                    let (n, weekday) = nth?;
                    if month_day.is_some() {
                        return None;
                    }
                    let week = match n {
                        -1 | 5 => 5,
                        1..=4 => u8::try_from(n).ok()?,
                        _ => return None,
                    };
                    let minute = u16::try_from(o.secs.div_euclid(60)).ok().filter(|m| *m < 24 * 60)?;
                    Some(ZoneRule { month: u8::try_from(month).ok()?, week, weekday: u8::try_from(weekday).ok()?, minute })
                };
                let save = daylight.to.saturating_sub(standard.to);
                if !in_range(standard.to) || !in_range(daylight.to) || save == 0 {
                    return None;
                }
                Zone {
                    bias: i32::try_from(standard.to).ok()?,
                    daylight: Some(Daylight { save: i32::try_from(save).ok()?, begins: rule(daylight)?, ends: rule(standard)? }),
                    ..Zone::default()
                }
            }
            _ => return None,
        };
        Some(named_as(&self.id, zone))
    }
}

/// `zone` known by `tzid` where that is one of the zones known with the same rules, by its
/// name or Outlook's key; else kept with the TZID as its key.
fn named_as(tzid: &str, zone: Zone) -> Zone {
    let id = tzid.trim().trim_start_matches('/');
    match Zone::named(id) {
        Some(known) if known.bias == zone.bias && known.daylight == zone.daylight => known,
        _ => Zone { key: cut(id, 260), ..zone.known_as() },
    }
}

/// The parts of a recurrence rule (RFC 5545 3.3.10), each name in capitals.
fn rule_parts(v: &str) -> Vec<(String, String)> {
    v.split(';')
        .take(MAX_PARAMS)
        .filter_map(|part| part.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_uppercase(), v.trim().to_string()))
        .collect()
}

/// What times are read in: the zones the file defines, and the offset of where we are for
/// times given in a zone it does not define, or in none.
struct Clock<'a> {
    zones: &'a [FileZone],
    here: &'a Zone,
    /// Occurrences that may still be deleted or changed, in the whole file.
    budget: std::cell::Cell<usize>,
}

impl Clock<'_> {
    /// Whether one more occurrence may be deleted or changed.
    fn spend(&self) -> bool {
        let left = self.budget.get();
        self.budget.set(left.saturating_sub(1));
        left > 0
    }

    /// The offset, in minutes east, of `tzid` at the local second `local`; `None` when the
    /// file does not define that zone (it is then where we are).
    fn offset(&self, tzid: Option<&str>, local: i64) -> Option<i64> {
        let id = tzid?.trim();
        let id = id.strip_prefix('/').unwrap_or(id);
        if ["UTC", "GMT", "Etc/UTC", "Etc/GMT", "Z"].iter().any(|z| z.eq_ignore_ascii_case(id)) {
            return Some(0);
        }
        match self.zones.iter().find(|z| z.id.trim_start_matches('/') == id) {
            Some(z) => Some(z.offset_at(local)),
            // A zone known by name, as Outlook and others write them without defining them.
            None => Zone::named(id).map(|z| offset_of_local(&z, local)),
        }
    }

    /// The zone `tzid` names as one this program keeps, when it can be.
    fn zone(&self, tzid: Option<&str>) -> Option<Zone> {
        let id = tzid?.trim();
        let id = id.strip_prefix('/').unwrap_or(id);
        match self.zones.iter().find(|z| z.id.trim_start_matches('/') == id) {
            Some(z) => z.model(),
            None => Zone::named(id),
        }
        // One offset known by no name says no more than the time given in it does.
        .filter(|z| !(z.is_fixed() && z.id.is_empty()))
    }

    /// A time read: its UTC FILETIME ticks, the offset it was given in, and whether that was
    /// guessed, being of a zone the file does not define.
    fn at(&self, p: &Prop) -> Option<(i64, Stamp, i64, bool)> {
        let s = stamp(p.value.split(',').next().unwrap_or_default())?;
        let (off, guessed) = self.offset_of(p.param("TZID"), s);
        Some((self.utc(s, off), s, off, guessed))
    }

    fn offset_of(&self, tzid: Option<&str>, s: Stamp) -> (i64, bool) {
        if s.utc {
            return (0, false);
        }
        let local = s.day.saturating_mul(86_400).saturating_add(s.secs);
        if s.date_only || tzid.is_none() {
            return (offset_of_local(self.here, local), false);
        }
        match self.offset(tzid, local) {
            Some(off) => (off, false),
            None => (offset_of_local(self.here, local), true),
        }
    }

    fn utc(&self, s: Stamp, off: i64) -> i64 {
        time::from_unix(s.day.saturating_mul(86_400).saturating_add(s.secs).saturating_sub(off.saturating_mul(60)))
    }
}

/// Minutes east `zone` keeps at the local second `local` (since 1970 on its own clock).
fn offset_of_local(zone: &Zone, local: i64) -> i64 {
    i64::from(zone.offset_at(zone.utc_of(time::from_unix(local))))
}

/// An event of a calendar file, read as an appointment.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// Its UID (RFC 5545 3.8.4.7), by which a copy of it is known again.
    pub uid: String,
    pub appointment: Appointment,
    pub description: String,
    pub categories: Vec<String>,
    /// What of it could not be kept, or was read with a guess.
    pub notes: Vec<String>,
}

impl Event {
    /// A new appointment item saying this event, its UID kept as its GlobalObjectId.
    pub fn item(&self) -> Item {
        let mut it = self.appointment.item();
        self.write(&mut it);
        it
    }

    /// `item` made to say this event; what else it has (its attachments, properties this
    /// program does not know) is kept.
    pub fn write(&self, item: &mut Item) {
        self.appointment.write(item);
        item.set_body_text(&self.description);
        item.categories = self.categories.clone();
        calendar::set_global_id(item, &calendar::global_id_of_uid(&self.uid));
    }
}

/// What a calendar file holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Calendar {
    /// Its name (X-WR-CALNAME), when it gives one.
    pub name: Option<String>,
    /// Its METHOD (RFC 5546), when it gives one: PUBLISH, REQUEST, REPLY, CANCEL…
    pub method: Option<String>,
    pub events: Vec<Event>,
    /// What of the file as a whole could not be kept.
    pub notes: Vec<String>,
}

fn note(notes: &mut Vec<String>, text: impl Into<String>) {
    let text = text.into();
    if notes.len() < MAX_NOTES && !notes.contains(&text) {
        notes.push(text);
    }
}

/// A file's events read. `here` is the zone we are in: times given in no zone, or in one the
/// file does not define and this program does not know, are taken to be here. An error when it
/// is not a calendar file at all.
pub fn read(bytes: &[u8], here: &Zone) -> Result<Calendar, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("a calendar file is read up to {} MB", MAX_BYTES >> 20));
    }
    let (roots, cut_short) = components(bytes);
    let calendars: Vec<&Comp> = roots.iter().filter(|c| c.name == "VCALENDAR").collect();
    if calendars.is_empty() {
        return Err("this is not an iCalendar file: it has no VCALENDAR".into());
    }
    let mut out = Calendar::default();
    if cut_short {
        note(&mut out.notes, "the file stops part of the way through; what came before is read");
    }
    let mut zones = Vec::new();
    let mut events: Vec<&Comp> = Vec::new();
    for cal in &calendars {
        if out.name.is_none() {
            out.name = cal.text("X-WR-CALNAME", MAX_TEXT).filter(|n| !n.trim().is_empty());
        }
        if out.method.is_none() {
            out.method = cal.prop("METHOD").map(|p| p.value.trim().to_ascii_uppercase());
        }
        zones.extend(cal.children.iter().filter(|c| c.name == "VTIMEZONE").filter_map(FileZone::of));
        events.extend(cal.children.iter().filter(|c| c.name == "VEVENT"));
        let todos = cal.children.iter().filter(|c| c.name == "VTODO" || c.name == "VJOURNAL").count();
        if todos > 0 {
            note(&mut out.notes, format!("{todos} tasks or journal entries are not read here; only events are"));
        }
    }
    if events.len() > MAX_EVENTS {
        note(&mut out.notes, format!("only the first {MAX_EVENTS} events are read"));
        events.truncate(MAX_EVENTS);
    }
    let clock = Clock { zones: &zones, here, budget: std::cell::Cell::new(MAX_CHANGES) };
    let uids: Vec<String> = events.iter().enumerate().map(|(i, e)| uid_of(e, i)).collect();
    // The occurrences changed in each series (RECURRENCE-ID), by UID.
    let mut changes: std::collections::HashMap<&str, Vec<&Comp>> = std::collections::HashMap::new();
    for (ev, uid) in events.iter().zip(&uids) {
        if ev.prop("RECURRENCE-ID").is_some() {
            changes.entry(uid.as_str()).or_default().push(ev);
        }
    }
    let mut done: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (ev, uid) in events.iter().zip(&uids) {
        if ev.prop("RECURRENCE-ID").is_some() || !done.insert(uid.as_str()) {
            continue;
        }
        let mut notes = Vec::new();
        let Some(mut e) = event_of(ev, &clock, uid, &mut notes) else {
            note(&mut out.notes, format!("an event with no start was left out ({})", cut(uid, 60)));
            continue;
        };
        if ev.prop("STATUS").is_some_and(|p| p.value.trim().eq_ignore_ascii_case("CANCELLED")) {
            note(&mut out.notes, format!("“{}” was cancelled and is left out", cut(&e.appointment.subject, 60)));
            continue;
        }
        for o in changes.get(uid.as_str()).into_iter().flatten() {
            changed_occurrence(&mut e, o, &clock, &mut notes);
        }
        for n in notes {
            note(&mut e.notes, n);
        }
        out.events.push(e);
    }
    if clock.budget.get() == 0 {
        note(&mut out.notes, format!("only {MAX_CHANGES} occurrences changed or deleted are kept in one file"));
    }
    // Changed occurrences whose series is not in the file are added as appointments of their own.
    for (o, uid) in events.iter().zip(&uids) {
        let Some(rid) = o.prop("RECURRENCE-ID") else { continue };
        if done.contains(uid.as_str()) {
            continue;
        }
        let mut notes = vec!["one occurrence of a series that is not in this file: added on its own".to_string()];
        let own = cut(&format!("{uid}-{}", rid.value.trim()), calendar::MAX_UID);
        if let Some(mut e) = event_of(o, &clock, &own, &mut notes) {
            e.notes = notes;
            out.events.push(e);
        }
    }
    Ok(out)
}

/// An event's UID, or one made from where it stands in the file when it has none.
fn uid_of(ev: &Comp, index: usize) -> String {
    match ev.prop("UID").map(|p| p.value.trim()).filter(|u| !u.is_empty()) {
        Some(u) => cut(u, calendar::MAX_UID),
        None => {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            let key = format!("{index}|{}|{}", ev.prop("DTSTART").map_or("", |p| &p.value), ev.prop("SUMMARY").map_or("", |p| &p.value));
            for b in key.bytes() {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
            format!("openmailclient-{h:016x}")
        }
    }
}

/// How busy an event shows its time: Outlook's own word for it, else TRANSP ([MS-OXCICAL]
/// 2.1.3.1.1.20.20 and 2.1.3.1.1.20.41).
fn busy_of(ev: &Comp) -> Busy {
    let value = |name: &str| ev.prop(name).map(|p| p.value.trim().to_ascii_uppercase());
    match value("X-MICROSOFT-CDO-BUSYSTATUS").as_deref() {
        Some("FREE") => return Busy::Free,
        Some("TENTATIVE") => return Busy::Tentative,
        Some("BUSY") => return Busy::Busy,
        Some("OOF") => return Busy::OutOfOffice,
        Some("WORKINGELSEWHERE") => return Busy::WorkingElsewhere,
        _ => {}
    }
    if value("TRANSP").as_deref() == Some("TRANSPARENT") {
        return Busy::Free;
    }
    if value("STATUS").as_deref() == Some("TENTATIVE") {
        return Busy::Tentative;
    }
    Busy::Busy
}

/// The minutes before its start an event's first alarm comes (RFC 5545 3.8.6.3); `None` when
/// it has none.
fn reminder_of(ev: &Comp, clock: &Clock, start: i64, end: i64) -> Option<i32> {
    let alarm = ev.children.iter().find(|c| c.name == "VALARM")?;
    let trigger = alarm.prop("TRIGGER")?;
    let before_secs = if trigger.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE-TIME")) {
        let (at, ..) = clock.at(trigger)?;
        start.saturating_sub(at) / 10_000_000
    } else {
        let d = duration(&trigger.value)?;
        let base = if trigger.param("RELATED").is_some_and(|r| r.eq_ignore_ascii_case("END")) { end.saturating_sub(start) / 10_000_000 } else { 0 };
        -(base.saturating_add(d))
    };
    i32::try_from((before_secs / 60).clamp(0, 60 * 24 * 7 * 52)).ok()
}

/// An event read as an appointment, without its changed occurrences; `None` when it gives no
/// start.
fn event_of(ev: &Comp, clock: &Clock, uid: &str, notes: &mut Vec<String>) -> Option<Event> {
    let dtstart = ev.prop("DTSTART")?;
    let (start, s, off, guessed) = clock.at(dtstart)?;
    if guessed {
        note(notes, format!("its time zone “{}” is not in the file: taken as this computer's", cut(dtstart.param("TZID").unwrap_or(""), 60)));
    }
    let all_day = s.date_only;
    let start = if all_day { calendar::day_start(s.day, clock.here) } else { start };
    let end = match (ev.prop("DTEND").and_then(|p| clock.at(p)), ev.prop("DURATION").and_then(|p| duration(&p.value))) {
        (Some((_, e, _, _)), _) if all_day && e.date_only => calendar::day_start(e.day, clock.here),
        (Some((end, ..)), _) => end,
        (None, Some(d)) => start.saturating_add(d.saturating_mul(10_000_000)),
        (None, None) if all_day => start.saturating_add(DAY),
        (None, None) => start,
    };
    let end = if all_day && end <= start { start.saturating_add(DAY) } else { end };
    let end = end.clamp(start, start.saturating_add(MAX_DAYS.saturating_mul(DAY)));
    let mut a = Appointment {
        subject: ev.text("SUMMARY", MAX_TEXT).unwrap_or_default(),
        location: ev.text("LOCATION", MAX_TEXT).unwrap_or_default(),
        start,
        end,
        all_day,
        busy: busy_of(ev),
        reminder: None,
        recurring: false,
        meeting: false,
        recurrence: None,
        occurrence: None,
        // The zone it was given in; an all-day series is kept where we are, so its days begin
        // at midnight here whatever the season.
        zone: if s.utc || all_day { None } else { clock.zone(dtstart.param("TZID")) },
    };
    if all_day && ev.prop("RRULE").is_some() && !clock.here.is_fixed() {
        a.zone = Some(clock.here.clone());
    }
    a.reminder = reminder_of(ev, clock, a.start, a.end);
    if ev.prop("ATTENDEE").is_some() {
        note(notes, "its attendees are not kept: it is added as an appointment");
    }
    if let Some(rule) = ev.prop("RRULE") {
        let series_off = if all_day { i64::from(clock.here.offset_at(start)) } else { off };
        match pattern(&rule.value, &a, s, series_off, clock) {
            Ok((r, words)) => {
                if let Some(w) = words {
                    note(notes, w);
                }
                let length = a.end.saturating_sub(a.start);
                let first = recur::minutes_of_day(r.start_day).saturating_add(i64::from(r.start_offset));
                a.start = match &a.zone {
                    Some(z) => z.utc_of(first.saturating_mul(MINUTE)),
                    None => first.saturating_sub(series_off).saturating_mul(MINUTE),
                };
                a.end = a.start.saturating_add(length);
                a.recurrence = Some(r);
                a.recurring = true;
                let gone: Vec<i64> = ev.all("EXDATE").flat_map(|p| exdates(p, clock, &a)).take(recur::MAX_COUNT as usize).collect();
                for value in gone {
                    if !clock.spend() {
                        break;
                    }
                    a.delete_occurrence(value);
                }
            }
            Err(why) => note(notes, format!("it repeats {why}, which is not kept: only its first time is added")),
        }
        if ev.prop("RDATE").is_some() {
            note(notes, "the extra days it is given (RDATE) are not kept");
        }
    }
    let categories = ev.all("CATEGORIES").flat_map(|p| text_list(&p.value)).take(MAX_LIST).collect();
    Some(Event {
        uid: uid.to_string(),
        appointment: a,
        description: ev.text("DESCRIPTION", MAX_BODY).unwrap_or_default(),
        categories,
        notes: Vec::new(),
    })
}

/// The local days, in the series' time, a date property (EXDATE, RECURRENCE-ID) names.
fn exdates(p: &Prop, clock: &Clock, a: &Appointment) -> Vec<i64> {
    p.value
        .split(',')
        .take(MAX_LIST)
        .filter_map(stamp)
        .map(|s| {
            if s.date_only {
                s.day
            } else {
                let (off, _) = clock.offset_of(p.param("TZID"), s);
                a.series_day(clock.utc(s, off))
            }
        })
        .collect()
}

/// One occurrence of `e` changed or deleted as the event `o` says (RFC 5545 3.8.4.4).
fn changed_occurrence(e: &mut Event, o: &Comp, clock: &Clock, notes: &mut Vec<String>) {
    let Some(rid) = o.prop("RECURRENCE-ID") else { return };
    let Some(day) = exdates(rid, clock, &e.appointment).first().copied() else { return };
    if e.appointment.recurrence.is_none() || !clock.spend() {
        return;
    }
    if o.prop("STATUS").is_some_and(|p| p.value.trim().eq_ignore_ascii_case("CANCELLED")) {
        e.appointment.delete_occurrence(day);
        return;
    }
    let mut ignored = Vec::new();
    let Some(mut one) = event_of(o, clock, &e.uid, &mut ignored) else { return };
    if o.prop("SUMMARY").is_none() {
        one.appointment.subject = e.appointment.subject.clone();
    }
    if o.prop("LOCATION").is_none() {
        one.appointment.location = e.appointment.location.clone();
    }
    let changed = e.appointment.recurrence.as_ref().is_some_and(|r| r.exceptions.len() < recur::MAX_COUNT as usize)
        && e.appointment.change_occurrence(day, &one.appointment);
    if !changed {
        note(notes, "an occurrence changed on a day the series does not fall on is left out");
    }
}

/// A recurrence rule (RFC 5545 3.3.10) as a pattern Outlook keeps ([MS-OXCICAL] 2.1.3.2.1),
/// with a note when it is kept nearly but not exactly; `Err` says how it repeats when it
/// cannot be kept.
fn pattern(rule: &str, a: &Appointment, s: Stamp, off: i64, clock: &Clock) -> Result<(Recurrence, Option<String>), String> {
    let parts = rule_parts(rule);
    let get = |k: &str| parts.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    const KNOWN: [&str; 9] = ["FREQ", "INTERVAL", "COUNT", "UNTIL", "BYDAY", "BYMONTHDAY", "BYMONTH", "BYSETPOS", "WKST"];
    if let Some((k, _)) = parts.iter().find(|(k, _)| !KNOWN.contains(&k.as_str())) {
        return Err(format!("by {k}"));
    }
    let freq = get("FREQ").unwrap_or("").to_ascii_uppercase();
    let interval = match get("INTERVAL") {
        None => 1,
        Some(v) => digits(v).filter(|n| *n >= 1).ok_or("with an interval that cannot be read")?,
    };
    let list = |k: &str| get(k).map(|v| v.split(',').take(MAX_LIST).map(str::trim).filter(|x| !x.is_empty()).collect::<Vec<_>>());
    let days = match list("BYDAY") {
        Some(v) => v.into_iter().map(by_day).collect::<Option<Vec<_>>>().ok_or("on days that cannot be read")?,
        None => Vec::new(),
    };
    let mask = |ds: &[(i32, u32)]| ds.iter().fold(0u8, |m, (_, d)| m | (1 << d));
    let plain = days.iter().all(|(n, _)| *n == 0);
    let setpos = match get("BYSETPOS") {
        None => None,
        Some(v) => {
            Some(v.trim().parse::<i32>().ok().filter(|n| (1..=4).contains(n) || *n == -1).ok_or("on a set position other than 1 to 4 or the last")?)
        }
    };
    let month_days = match list("BYMONTHDAY") {
        Some(v) => v
            .into_iter()
            .map(|d| d.parse::<i32>().ok().filter(|d| (-31..=31).contains(d) && *d != 0))
            .collect::<Option<Vec<_>>>()
            .ok_or("on days of the month that cannot be read")?,
        None => Vec::new(),
    };
    let first_dow = get("WKST").and_then(weekday_code).unwrap_or(1);
    let local = a.start.div_euclid(MINUTE).saturating_add(off);
    let start_offset =
        if a.all_day { 0 } else { u32::try_from(local.saturating_sub(recur::minutes_of_day(recur::day_of_minutes(local)))).unwrap_or(0) };
    let mut start_day = s.day;
    let (_, month, month_day) = ymd(start_day);
    let mut warn = None;
    let monthly = |warn: &mut Option<String>| -> Result<Rule, String> {
        match (days.as_slice(), month_days.as_slice(), setpos) {
            ([], [], None) => Ok(Rule::Month { day: month_day }),
            ([], [-1], None) => Ok(Rule::MonthEnd),
            ([], [d], None) if *d > 0 => {
                if *d > 28 {
                    *warn = Some(format!("in months shorter than {d} days it falls on their last day, as Outlook keeps it"));
                }
                Ok(Rule::Month { day: (*d).unsigned_abs() })
            }
            // Day 29, 30 or 31, or the month's last day where it is shorter: as written below.
            ([], ds, Some(-1)) if ds.first() == Some(&28) && ds.windows(2).all(|w| w.get(1).copied() == w.first().map(|x| x + 1)) => {
                Ok(Rule::Month { day: ds.last().copied().unwrap_or(28).unsigned_abs() })
            }
            ([(n, d)], [], None) if *n != 0 && (*n == -1 || (1..=4).contains(n)) => {
                Ok(Rule::MonthNth { days: 1 << d, nth: if *n < 0 { 5 } else { n.unsigned_abs() } })
            }
            (ds, [], Some(pos)) if !ds.is_empty() && plain => {
                Ok(Rule::MonthNth { days: mask(ds), nth: if pos < 0 { 5 } else { pos.unsigned_abs() } })
            }
            _ => Err("on days of the month Outlook cannot keep".to_string()),
        }
    };
    let (freq, rule, period) = match freq.as_str() {
        "DAILY" if days.is_empty() && month_days.is_empty() && setpos.is_none() => (Freq::Daily, Rule::Day, interval.min(1000)),
        "DAILY" if interval == 1 && plain && month_days.is_empty() && setpos.is_none() => (Freq::Weekly, Rule::Week { days: mask(&days) }, 1),
        "WEEKLY" if plain && month_days.is_empty() && setpos.is_none() => {
            let m = if days.is_empty() { 1 << recur::weekday(start_day) } else { mask(&days) };
            (Freq::Weekly, Rule::Week { days: m }, interval)
        }
        "MONTHLY" if get("BYMONTH").is_none() => (Freq::Monthly, monthly(&mut warn)?, interval),
        "YEARLY" => {
            let months = list("BYMONTH").unwrap_or_default();
            let m = match months.as_slice() {
                [] => month,
                [m] => digits(m).filter(|m| (1..=12).contains(m)).ok_or("in a month that cannot be read")?,
                _ => return Err("in more than one month of the year".into()),
            };
            if m != month {
                // The series begins in the first of its months from its start.
                let (y, _, _) = ymd(start_day);
                let y = if m < month { y + 1 } else { y };
                start_day = day_number(y, m, 1).ok_or("in a year that cannot be")?;
            }
            let rule = if days.is_empty() && month_days.is_empty() && m != month { Rule::Month { day: month_day } } else { monthly(&mut warn)? };
            if interval == 1 { (Freq::Yearly, rule, 12) } else { (Freq::Monthly, rule, interval.saturating_mul(12)) }
        }
        "" => return Err("with no frequency".into()),
        other => return Err(format!("{} with these parts", other.to_ascii_lowercase())),
    };
    let most = if freq == Freq::Daily { 999 } else { 99 * 12 };
    if period > most {
        return Err(format!("every {period} periods"));
    }
    let end = match (get("COUNT"), get("UNTIL")) {
        (Some(_), Some(_)) => return Err("with both a count and an end".into()),
        (Some(c), None) => {
            let n = digits(c).filter(|n| *n >= 1).ok_or("a number of times that cannot be read")?;
            if n > recur::MAX_COUNT {
                warn = Some(format!("{n} times is more than Outlook keeps: kept to {}", recur::MAX_COUNT));
            }
            End::After(n.min(recur::MAX_COUNT))
        }
        (None, Some(u)) => {
            let until = stamp(u).ok_or("until a date that cannot be read")?;
            // The last day whose occurrence starts at or before it (RFC 5545 3.3.10: UNTIL is
            // inclusive), in the series' own time.
            let day = if until.date_only {
                until.day
            } else {
                let at = if until.utc {
                    clock.utc(until, 0).div_euclid(MINUTE).saturating_add(off)
                } else {
                    recur::minutes_of_day(until.day).saturating_add(until.secs / 60)
                };
                recur::day_of_minutes(at.saturating_sub(i64::from(start_offset)))
            };
            if day < start_day {
                return Err("until before it begins".into());
            }
            End::On(day)
        }
        (None, None) => End::Never,
    };
    let length = u32::try_from(a.minutes().max(0)).unwrap_or(u32::MAX);
    let r = Recurrence::new(freq, rule, period, end, start_day, start_offset, start_offset.saturating_add(length), first_dow);
    if r.days(r.start_day, r.start_day.saturating_add(1)).is_empty() {
        return Err("on no day at all".into());
    }
    Ok((r, warn))
}

// ---- Writing ----

/// A line written, folded at 75 octets (RFC 5545 3.1) between characters.
fn line(out: &mut String, text: &str) {
    let mut used = 0;
    for c in text.chars() {
        let n = c.len_utf8();
        if used + n > 75 {
            out.push_str("\r\n ");
            used = 1;
        }
        out.push(c);
        used += n;
    }
    out.push_str("\r\n");
}

fn date_text(day: i64) -> String {
    let (y, m, d) = ymd(day);
    format!("{y:04}{m:02}{d:02}")
}

/// UTC FILETIME ticks as a DATE-TIME in UTC.
fn utc_text(t: i64) -> String {
    let c = time::civil(t, 0);
    format!("{:04}{:02}{:02}T{:02}{:02}{:02}Z", c.year, c.month, c.day, c.hour, c.minute, c.second)
}

/// Local minutes since 1601 as a DATE-TIME of no zone.
fn local_text(minutes: i64) -> String {
    let day = recur::day_of_minutes(minutes);
    let m = minutes.saturating_sub(recur::minutes_of_day(day));
    format!("{}T{:02}{:02}00", date_text(day), m / 60, m % 60)
}

/// `off` minutes east as a UTC offset (RFC 5545 3.3.14), `+0200`.
fn offset_text(off: i64) -> String {
    let sign = if off < 0 { '-' } else { '+' };
    format!("{sign}{:02}{:02}", off.abs() / 60, off.abs() % 60)
}

/// The TZID written for a zone: its tz database name, Outlook's key, or for one offset with
/// no name `UTC+0200`. It has no colon, semicolon, comma or quote, which a parameter value
/// could not hold unquoted.
fn zone_id(zone: &Zone) -> String {
    let safe = |t: &str| t.chars().filter(|c| !matches!(c, ':' | ';' | ',' | '"') && !c.is_control()).take(200).collect::<String>();
    let id = if !zone.id.is_empty() { safe(&zone.id) } else { safe(&zone.key) };
    if id.trim().is_empty() { format!("UTC{}", offset_text(i64::from(zone.bias))) } else { id }
}

/// Whether times in `zone` are written in UTC: it is UTC itself, by no other name.
fn is_utc(zone: &Zone) -> bool {
    zone.is_fixed() && zone.bias == 0 && (zone.id.is_empty() || zone.id == "UTC")
}

/// A zone as a VTIMEZONE (RFC 5545 3.6.5): one STANDARD observance, and a DAYLIGHT one
/// beside it repeating each year where it saves daylight.
fn write_zone(out: &mut String, zone: &Zone) {
    line(out, "BEGIN:VTIMEZONE");
    line(out, &format!("TZID:{}", zone_id(zone)));
    let bias = i64::from(zone.bias);
    match zone.daylight {
        None => {
            let text = offset_text(bias);
            line(out, "BEGIN:STANDARD");
            line(out, "DTSTART:16010101T000000");
            line(out, &format!("TZOFFSETFROM:{text}"));
            line(out, &format!("TZOFFSETTO:{text}"));
            line(out, "END:STANDARD");
        }
        Some(d) => {
            let saving = bias.saturating_add(i64::from(d.save));
            let observance = |out: &mut String, name: &str, rule: ZoneRule, from: i64, to: i64| {
                // First in 1970, said in the time before it, as RFC 5545 3.6.5 asks.
                let day = rule.day_in(1970);
                line(out, &format!("BEGIN:{name}"));
                line(out, &format!("DTSTART:{}T{:02}{:02}00", date_text(day), rule.minute / 60, rule.minute % 60));
                line(out, &format!("TZOFFSETFROM:{}", offset_text(from)));
                line(out, &format!("TZOFFSETTO:{}", offset_text(to)));
                let nth = if rule.week >= 5 { -1 } else { i32::from(rule.week) };
                let weekday = WEEKDAYS.get(usize::from(rule.weekday) % 7).copied().unwrap_or("SU");
                line(out, &format!("RRULE:FREQ=YEARLY;BYMONTH={};BYDAY={nth}{weekday}", rule.month));
                line(out, &format!("END:{name}"));
            };
            observance(out, "STANDARD", d.ends, saving, bias);
            observance(out, "DAYLIGHT", d.begins, bias, saving);
        }
    }
    line(out, "END:VTIMEZONE");
}

/// How a series' times are written: in its zone, or in UTC.
struct Writer {
    zone: Zone,
}

impl Writer {
    /// UTC ticks of local minutes in the series' time.
    fn utc(&self, local: i64) -> i64 {
        self.zone.utc_of(local.saturating_mul(MINUTE))
    }

    /// The parameters and value of local minutes in the series' time: a date, a time in its
    /// zone, or one in UTC.
    fn parts(&self, local: i64, all_day: bool) -> (String, String) {
        if all_day {
            (";VALUE=DATE".into(), date_text(recur::day_of_minutes(local)))
        } else if is_utc(&self.zone) {
            (String::new(), utc_text(self.utc(local)))
        } else {
            (format!(";TZID={}", zone_id(&self.zone)), local_text(local))
        }
    }

    fn at(&self, name: &str, local: i64, all_day: bool) -> String {
        let (params, value) = self.parts(local, all_day);
        format!("{name}{params}:{value}")
    }
}

/// A pattern as a recurrence rule ([MS-OXCICAL] 2.1.3.2.1); `None` for one that has none.
fn rule_text(r: &Recurrence, w: &Writer, all_day: bool) -> Option<String> {
    let names = |mask: u8| (0..7).filter(|d| mask & (1 << d) != 0).filter_map(|d| WEEKDAYS.get(d).copied()).collect::<Vec<_>>().join(",");
    let interval = |n: u32| if n > 1 { format!(";INTERVAL={n}") } else { String::new() };
    let month_rule = |rule: &Rule| -> Option<String> {
        Some(match rule {
            Rule::Month { day } if *day <= 28 => format!(";BYMONTHDAY={day}"),
            // Day 29 to 31 falls on a shorter month's last day: the last of 28 to that day.
            Rule::Month { day } => format!(";BYMONTHDAY={};BYSETPOS=-1", (28..=(*day).min(31)).map(|d| d.to_string()).collect::<Vec<_>>().join(",")),
            Rule::MonthEnd => ";BYMONTHDAY=-1".into(),
            Rule::MonthNth { days, nth } if days.count_ones() == 1 => {
                let n = if *nth >= 5 { -1 } else { *nth as i32 };
                format!(";BYDAY={n}{}", names(*days))
            }
            Rule::MonthNth { days, nth } => format!(";BYDAY={};BYSETPOS={}", names(*days), if *nth >= 5 { -1 } else { *nth as i32 }),
            _ => return None,
        })
    };
    let mut text = match (&r.rule, r.freq) {
        (Rule::Day, _) => format!("FREQ=DAILY{}", interval(r.period)),
        (Rule::Week { days }, _) => format!("FREQ=WEEKLY{};BYDAY={}", interval(r.period), names(*days)),
        (Rule::Other { .. }, _) => return None,
        (rule, Freq::Yearly) => format!("FREQ=YEARLY;BYMONTH={}{}", ymd(r.start_day).1, month_rule(rule)?),
        (rule, _) => format!("FREQ=MONTHLY{}{}", interval(r.period), month_rule(rule)?),
    };
    text.push_str(&format!(";WKST={}", WEEKDAYS.get(r.first_dow as usize % 7).unwrap_or(&"SU")));
    match r.end {
        End::Never => {}
        End::After(n) => text.push_str(&format!(";COUNT={n}")),
        // UNTIL is a date for an all-day series, else the UTC time of the last occurrence.
        End::On(day) if all_day => text.push_str(&format!(";UNTIL={}", date_text(day))),
        End::On(day) => {
            let last = recur::minutes_of_day(day).saturating_add(i64::from(r.start_offset));
            text.push_str(&format!(";UNTIL={}", utc_text(w.utc(last))));
        }
    }
    Some(text)
}

/// The UID an item is written with: its GlobalObjectId's, or one made from what it says.
pub fn uid(item: &Item) -> String {
    if let Some(id) = calendar::global_id(item) {
        return calendar::uid_of_global_id(&id);
    }
    let a = Appointment::of(item);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let key = format!("{}|{}", a.as_ref().map_or(0, |a| a.start), item.subject());
    for b in key.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("openmailclient-{h:016x}")
}

/// Writes the parts of one event both a series and its changed occurrences carry.
fn event_body(out: &mut String, a: &Appointment, item: &Item, uid: &str, now: i64) {
    line(out, &format!("UID:{uid}"));
    line(out, &format!("DTSTAMP:{}", utc_text(now)));
    line(out, &format!("SUMMARY:{}", escape(&a.subject)));
    if !a.location.is_empty() {
        line(out, &format!("LOCATION:{}", escape(&a.location)));
    }
    let body = item.body_text();
    if !body.trim().is_empty() {
        line(out, &format!("DESCRIPTION:{}", escape(&cut(&body, MAX_BODY))));
    }
    if !item.categories.is_empty() {
        line(out, &format!("CATEGORIES:{}", item.categories.iter().take(MAX_LIST).map(|c| escape(c)).collect::<Vec<_>>().join(",")));
    }
    line(out, &format!("TRANSP:{}", if a.busy == Busy::Free { "TRANSPARENT" } else { "OPAQUE" }));
    let busy = match a.busy {
        Busy::Free => "FREE",
        Busy::Tentative => "TENTATIVE",
        Busy::Busy => "BUSY",
        Busy::OutOfOffice => "OOF",
        Busy::WorkingElsewhere => "WORKINGELSEWHERE",
    };
    line(out, &format!("X-MICROSOFT-CDO-BUSYSTATUS:{busy}"));
    line(out, &format!("X-MICROSOFT-CDO-ALLDAYEVENT:{}", if a.all_day { "TRUE" } else { "FALSE" }));
}

fn alarm(out: &mut String, a: &Appointment) {
    if let Some(m) = a.reminder {
        line(out, "BEGIN:VALARM");
        line(out, "ACTION:DISPLAY");
        line(out, "DESCRIPTION:Reminder");
        line(out, &format!("TRIGGER:-PT{}M", m.max(0)));
        line(out, "END:VALARM");
    }
}

/// Appointment items written as one calendar file (RFC 5545), each as [MS-OXCICAL] 2.1.3.1.1
/// writes it: a series with its rule, the occurrences deleted from it and one event for each
/// occurrence changed. Each is written in its own zone, defined in the file. `here` is the
/// zone we are in, for an all-day event that is not a series; `now` the time written as each
/// event's stamp; `name` the calendar's.
pub fn write(items: &[Item], here: &Zone, now: i64, name: Option<&str>) -> String {
    let mut out = String::new();
    line(&mut out, "BEGIN:VCALENDAR");
    line(&mut out, "PRODID:-//OpenMailClient//OpenMailClient//EN");
    line(&mut out, "VERSION:2.0");
    line(&mut out, "METHOD:PUBLISH");
    line(&mut out, "CALSCALE:GREGORIAN");
    if let Some(n) = name {
        line(&mut out, &format!("X-WR-CALNAME:{}", escape(n)));
    }
    let shown: Vec<(&Item, Appointment)> = items.iter().filter_map(|it| Appointment::of(it).map(|a| (it, a))).collect();
    // The zone each event is written in: a series' own; an appointment's that says it was
    // made in one known by name; else UTC.
    let zone_of = |a: &Appointment| -> Option<Zone> {
        match (&a.recurrence, &a.zone) {
            (_, _) if a.all_day => None,
            (Some(_), _) => Some(a.series_zone().into_owned()),
            (None, Some(z)) if !z.id.is_empty() || !z.key.is_empty() => Some(z.clone()),
            (None, _) => None,
        }
        .filter(|z| !is_utc(z))
    };
    // Each zone once, by its TZID.
    let mut zones: Vec<Zone> = Vec::new();
    for (_, a) in &shown {
        if let Some(z) = zone_of(a)
            && !zones.iter().any(|w| zone_id(w) == zone_id(&z))
        {
            zones.push(z);
        }
    }
    for z in &zones {
        write_zone(&mut out, z);
    }
    for (item, a) in &shown {
        let uid = uid(item);
        line(&mut out, "BEGIN:VEVENT");
        event_body(&mut out, a, item, &uid, now);
        let rule = a.recurrence.as_ref().and_then(|r| {
            let w = Writer { zone: if a.all_day { Zone::utc() } else { a.series_zone().into_owned() } };
            rule_text(r, &w, a.all_day).map(|t| (r, w, t))
        });
        match &rule {
            Some((r, w, text)) => {
                let first = recur::minutes_of_day(r.start_day).saturating_add(i64::from(r.start_offset));
                let length = a.end.saturating_sub(a.start).div_euclid(MINUTE);
                line(&mut out, &w.at("DTSTART", first, a.all_day));
                line(&mut out, &w.at("DTEND", first.saturating_add(length), a.all_day));
                line(&mut out, &format!("RRULE:{text}"));
                let gone: Vec<i64> = r.deleted.iter().filter(|d| r.exception_on(**d).is_none()).copied().collect();
                let parts: Vec<(String, String)> =
                    gone.iter().map(|d| w.parts(recur::minutes_of_day(*d).saturating_add(i64::from(r.start_offset)), a.all_day)).collect();
                if let Some((params, _)) = parts.first() {
                    let values: Vec<&str> = parts.iter().map(|(_, v)| v.as_str()).collect();
                    line(&mut out, &format!("EXDATE{params}:{}", values.join(",")));
                }
            }
            None if a.all_day => {
                let day = calendar::day_of(a.start, here);
                let last = calendar::day_of(a.end, here).max(day.saturating_add(1));
                line(&mut out, &format!("DTSTART;VALUE=DATE:{}", date_text(day)));
                line(&mut out, &format!("DTEND;VALUE=DATE:{}", date_text(last)));
            }
            None => match zone_of(a) {
                Some(z) => {
                    let id = zone_id(&z);
                    let local = |t: i64| local_text(t.div_euclid(MINUTE).saturating_add(i64::from(z.offset_at(t))));
                    line(&mut out, &format!("DTSTART;TZID={id}:{}", local(a.start)));
                    line(&mut out, &format!("DTEND;TZID={id}:{}", local(a.end)));
                }
                None => {
                    line(&mut out, &format!("DTSTART:{}", utc_text(a.start)));
                    line(&mut out, &format!("DTEND:{}", utc_text(a.end)));
                }
            },
        }
        alarm(&mut out, a);
        line(&mut out, "END:VEVENT");
        // Each occurrence changed on its own, by the time it was first planned for.
        if let Some((r, w, _)) = &rule {
            for e in r.exceptions.iter().take(recur::MAX_COUNT as usize) {
                let day = recur::day_of_minutes(e.original);
                let Some(one) = a.occurrence_of(day) else { continue };
                line(&mut out, "BEGIN:VEVENT");
                event_body(&mut out, &one, item, &uid, now);
                let planned = recur::minutes_of_day(day).saturating_add(i64::from(r.start_offset));
                line(&mut out, &w.at("RECURRENCE-ID", planned, a.all_day));
                line(&mut out, &w.at("DTSTART", e.start, one.all_day && a.all_day));
                line(&mut out, &w.at("DTEND", e.end.max(e.start), one.all_day && a.all_day));
                alarm(&mut out, &one);
                line(&mut out, "END:VEVENT");
            }
        }
    }
    line(&mut out, "END:VCALENDAR");
    out
}

#[cfg(test)]
mod tests;
