//! FILETIME ticks ↔ calendar dates. The model stores UTC; a local offset is applied only for display.

/// Seconds between 1601-01-01 and 1970-01-01.
const UNIX_DIFF_SECS: i64 = 11_644_473_600;
const TICKS_PER_SEC: i64 = 10_000_000;

pub const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
pub const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

pub fn from_unix(secs: i64) -> i64 {
    secs.saturating_add(UNIX_DIFF_SECS).saturating_mul(TICKS_PER_SEC)
}

pub fn to_unix(filetime: i64) -> i64 {
    filetime.div_euclid(TICKS_PER_SEC).saturating_sub(UNIX_DIFF_SECS)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    /// 1–12
    pub month: u32,
    /// 1–31
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 = Sunday
    pub weekday: u32,
    /// Days since 1970-01-01 in the same offset; handy for "today / yesterday" grouping.
    pub day_number: i64,
}

impl Civil {
    pub fn weekday_name(&self) -> &'static str {
        WEEKDAYS.get(self.weekday as usize).copied().unwrap_or("")
    }

    pub fn month_name(&self) -> &'static str {
        MONTHS.get(self.month.saturating_sub(1) as usize).copied().unwrap_or("")
    }

    /// 12-hour clock: (hour 1–12, "AM"/"PM").
    pub fn hour12(&self) -> (u32, &'static str) {
        let h = match self.hour % 12 {
            0 => 12,
            h => h,
        };
        (h, if self.hour < 12 { "AM" } else { "PM" })
    }

    /// "9:14 AM", as in the United States: [`Region::time`] writes it as the owner's region does.
    pub fn time_short(&self) -> String {
        Region::default().time(self)
    }

    /// "Mon 10/5/2026 9:14 AM", as in the United States ([`Region::stamp`]).
    pub fn long_stamp(&self) -> String {
        Region::default().stamp(self)
    }

    /// "Monday, October 5, 2026 9:14 AM", as in the United States ([`Region::full_stamp`]).
    pub fn full_stamp(&self) -> String {
        Region::default().full_stamp(self)
    }
}

/// Characters of a date or time picture that are read; more is not a date.
const MAX_PICTURE: usize = 64;

/// How dates and times are written where the owner is, as the system's region settings say:
/// pictures as Windows gives them (`M/d/yyyy`, `yyyy/MM/dd`, `dddd, d MMMM yyyy`, `h:mm tt`,
/// `HH:mm`), and the day a week begins on. Outlook writes every date as these say; so do we.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub short_date: String,
    pub long_date: String,
    pub short_time: String,
    /// What `tt` writes before noon and after.
    pub am: String,
    pub pm: String,
    /// 0 = Sunday.
    pub week_start: u32,
}

impl Default for Region {
    /// As in the United States, until the system says otherwise.
    fn default() -> Self {
        Region {
            short_date: "M/d/yyyy".into(),
            long_date: "dddd, MMMM d, yyyy".into(),
            short_time: "h:mm tt".into(),
            am: "AM".into(),
            pm: "PM".into(),
            week_start: 0,
        }
    }
}

impl Region {
    /// A region from what the system says, each part checked: a picture that is empty, too long
    /// or writes no part of a date is left as it is by default.
    pub fn of(short_date: &str, long_date: &str, short_time: &str, am: &str, pm: &str, week_start: u32) -> Region {
        let base = Region::default();
        let picture = |given: &str, or: String| {
            let given: String = given.chars().filter(|c| !c.is_control()).collect();
            let fits = !given.trim().is_empty()
                && given.chars().count() <= MAX_PICTURE
                && given.chars().any(|c| matches!(c, 'd' | 'M' | 'y' | 'h' | 'H' | 'm'));
            if fits { given } else { or }
        };
        let word = |given: &str, or: String| {
            let given: String = given.chars().filter(|c| !c.is_control()).take(8).collect();
            if given.trim().is_empty() { or } else { given }
        };
        Region {
            short_date: picture(short_date, base.short_date),
            long_date: picture(long_date, base.long_date),
            short_time: picture(short_time, base.short_time),
            am: word(am, base.am),
            pm: word(pm, base.pm),
            week_start: week_start % 7,
        }
    }

    /// "9/10/2026", or "2026/09/10".
    pub fn date(&self, c: &Civil) -> String {
        self.write(&self.short_date, c)
    }

    /// "Thursday, September 10, 2026".
    pub fn long_date(&self, c: &Civil) -> String {
        self.write(&self.long_date, c)
    }

    /// "5:04 PM", or "17:04".
    pub fn time(&self, c: &Civil) -> String {
        self.write(&self.short_time, c)
    }

    /// "Thu 9/10/2026 5:04 PM", the reading pane's date.
    pub fn stamp(&self, c: &Civil) -> String {
        format!("{} {} {}", c.weekday_name().get(..3).unwrap_or(""), self.date(c), self.time(c))
    }

    /// "Thursday, September 10, 2026 5:04 PM", the "Sent:" line of a reply header.
    pub fn full_stamp(&self, c: &Civil) -> String {
        format!("{} {}", self.long_date(c), self.time(c))
    }

    /// The day number of the first day of the week `c` is in.
    pub fn week_begun(&self, c: &Civil) -> i64 {
        let into = (c.weekday % 7 + 7 - self.week_start % 7) % 7;
        c.day_number.saturating_sub(i64::from(into))
    }

    /// A picture written out: letters repeated stand for a part of the date, text in single
    /// quotes stands for itself, and so does anything else.
    fn write(&self, picture: &str, c: &Civil) -> String {
        let mut out = String::new();
        let mut chars = picture.chars().take(MAX_PICTURE).peekable();
        while let Some(ch) = chars.next() {
            if ch == '\'' {
                for quoted in chars.by_ref() {
                    if quoted == '\'' {
                        break;
                    }
                    out.push(quoted);
                }
                continue;
            }
            if !matches!(ch, 'd' | 'M' | 'y' | 'h' | 'H' | 'm' | 's' | 't' | 'g') {
                out.push(ch);
                continue;
            }
            let mut n = 1usize;
            while chars.next_if_eq(&ch).is_some() {
                n = n.saturating_add(1);
            }
            let (h12, _) = c.hour12();
            let noon = if c.hour < 12 { &self.am } else { &self.pm };
            match (ch, n) {
                ('d', 1) => out.push_str(&c.day.to_string()),
                ('d', 2) => out.push_str(&format!("{:02}", c.day)),
                ('d', 3) => out.push_str(c.weekday_name().get(..3).unwrap_or("")),
                ('d', _) => out.push_str(c.weekday_name()),
                ('M', 1) => out.push_str(&c.month.to_string()),
                ('M', 2) => out.push_str(&format!("{:02}", c.month)),
                ('M', 3) => out.push_str(c.month_name().get(..3).unwrap_or("")),
                ('M', _) => out.push_str(c.month_name()),
                ('y', 1) => out.push_str(&c.year.rem_euclid(100).to_string()),
                ('y', 2) => out.push_str(&format!("{:02}", c.year.rem_euclid(100))),
                ('y', _) => out.push_str(&c.year.to_string()),
                ('h', 1) => out.push_str(&h12.to_string()),
                ('h', _) => out.push_str(&format!("{h12:02}")),
                ('H', 1) => out.push_str(&c.hour.to_string()),
                ('H', _) => out.push_str(&format!("{:02}", c.hour)),
                ('m', 1) => out.push_str(&c.minute.to_string()),
                ('m', _) => out.push_str(&format!("{:02}", c.minute)),
                ('s', 1) => out.push_str(&c.second.to_string()),
                ('s', _) => out.push_str(&format!("{:02}", c.second)),
                ('t', 1) => out.extend(noon.chars().next()),
                ('t', _) => out.push_str(noon),
                // The era is not written: the Gregorian calendar has one.
                _ => {}
            }
        }
        out.trim().to_string()
    }
}

/// Break UTC FILETIME ticks into a calendar date at `offset_minutes` east of UTC.
pub fn civil(filetime: i64, offset_minutes: i32) -> Civil {
    let secs = to_unix(filetime).saturating_add(i64::from(offset_minutes).saturating_mul(60));
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    Civil {
        year,
        month,
        day,
        hour: (rem / 3600) as u32,
        minute: (rem % 3600 / 60) as u32,
        second: (rem % 60) as u32,
        weekday: (days.rem_euclid(7) + 4).rem_euclid(7) as u32,
        day_number: days,
    }
}

/// UTC FILETIME ticks for a calendar date given at `offset_minutes` east of UTC.
pub fn from_civil(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32, offset_minutes: i32) -> i64 {
    let days = days_from_civil(year, month, day);
    let secs = days
        .saturating_mul(86_400)
        .saturating_add(i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second))
        .saturating_sub(i64::from(offset_minutes).saturating_mul(60));
    from_unix(secs)
}

// Days ↔ civil date, proleptic Gregorian (Howard Hinnant's public-domain algorithms).
pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_date() {
        // 2026-10-05 09:14:00 UTC was a Monday.
        let ft = from_civil(2026, 10, 5, 9, 14, 0, 0);
        let c = civil(ft, 0);
        assert_eq!((c.year, c.month, c.day, c.hour, c.minute, c.weekday), (2026, 10, 5, 9, 14, 1));
        assert_eq!(c.long_stamp(), "Mon 10/5/2026 9:14 AM");
        assert_eq!(c.full_stamp(), "Monday, October 5, 2026 9:14 AM");
    }

    #[test]
    fn dates_are_written_as_the_region_says() {
        let c = civil(from_civil(2026, 9, 10, 8, 5, 7, 0), 0);
        let r = Region::of("yyyy/MM/dd", "dddd, dd MMMM yyyy", "HH:mm", "", "", 0);
        assert_eq!((r.date(&c), r.time(&c), r.stamp(&c)), ("2026/09/10".into(), "08:05".into(), "Thu 2026/09/10 08:05".into()));
        assert_eq!(r.full_stamp(&c), "Thursday, 10 September 2026 08:05");
        let r = Region::of("d.M.yy 'r.'", "ddd d MMM", "h:mm:ss t", "vm", "nm", 1);
        assert_eq!((r.date(&c), r.long_date(&c), r.time(&c)), ("10.9.26 r.".into(), "Thu 10 Sep".into(), "8:05:07 v".into()));
        // What is not a picture of a date is not taken: the default stands.
        let r = Region::of("", "no", &"d".repeat(500), "\u{0}", "PM", 13);
        assert_eq!(r, Region { week_start: 6, ..Region::default() });
        // A quote left open, and letters repeated without end, are only text.
        let r = Region::of("d 'open", "dddddddddd", "HH:mm", "", "", 0);
        assert_eq!((r.date(&c), r.long_date(&c)), ("10 open".into(), "Thursday".into()));
        // 10 September 2026 was a Thursday: its week began on Sunday the 6th, or Monday the 7th.
        assert_eq!(Region::default().week_begun(&c), c.day_number - 4);
        assert_eq!(Region::of("", "", "", "", "", 1).week_begun(&c), c.day_number - 3);
        assert_eq!(Region::of("", "", "", "", "", 4).week_begun(&c), c.day_number);
    }

    #[test]
    fn unix_epoch_and_offset() {
        assert_eq!(to_unix(from_unix(0)), 0);
        let c = civil(from_unix(0), 120);
        assert_eq!((c.year, c.month, c.day, c.hour, c.weekday), (1970, 1, 1, 2, 4));
        assert_eq!(civil(from_unix(0), -60).day, 31);
    }

    #[test]
    fn extreme_ticks_do_not_panic() {
        for ft in [i64::MIN, -1, 0, i64::MAX] {
            let _ = civil(ft, i32::MAX);
            let _ = civil(ft, i32::MIN);
        }
        assert_eq!(civil(from_civil(2024, 2, 29, 23, 59, 59, 0), 0).day, 29);
    }
}
