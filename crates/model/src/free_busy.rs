//! Free and busy times: when each person asked to a meeting is taken, from their calendar or a
//! server's answer, and the times when those who must come are all free (the Scheduling
//! Assistant's suggested times). Times are FILETIMEs in UTC.

use crate::calendar::{Appointment, Busy, DAY, MINUTE};
use crate::zone::Zone;

/// Blocks kept of one person; more say nothing a page of hours can show.
pub const MAX_BLOCKS: usize = 5000;
/// Times suggested at most.
pub const MAX_SUGGESTED: usize = 20;
/// Days looked across at most.
pub const MAX_DAYS: i64 = 62;

/// A time someone is taken, and how ([RFC 5545] 3.2.9 FBTYPE; Outlook's Show As).
///
/// [RFC 5545]: https://www.rfc-editor.org/rfc/rfc5545
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub start: i64,
    pub end: i64,
    pub busy: Busy,
}

/// How much a kind of busy keeps one from a meeting: free and working elsewhere do not.
pub fn takes(b: Busy) -> bool {
    matches!(b, Busy::Busy | Busy::Tentative | Busy::OutOfOffice)
}

/// Which of two kinds says more: out of office over busy over tentative over the rest.
fn weight(b: Busy) -> u8 {
    match b {
        Busy::Free => 0,
        Busy::WorkingElsewhere => 1,
        Busy::Tentative => 2,
        Busy::Busy => 3,
        Busy::OutOfOffice => 4,
    }
}

/// Blocks in order, cut to `from`..`to`, empty ones and free ones left out, and those of one
/// kind that touch or overlap made one; at most [`MAX_BLOCKS`].
pub fn tidy(mut blocks: Vec<Block>, from: i64, to: i64) -> Vec<Block> {
    blocks.retain(|b| b.busy != Busy::Free);
    for b in &mut blocks {
        b.start = b.start.max(from);
        b.end = b.end.min(to);
    }
    blocks.retain(|b| b.end > b.start);
    blocks.sort_by_key(|b| (b.start, b.end));
    let mut out: Vec<Block> = Vec::new();
    for b in blocks {
        match out.last_mut() {
            Some(last) if last.busy == b.busy && b.start <= last.end => last.end = last.end.max(b.end),
            _ => out.push(b),
        }
        if out.len() >= MAX_BLOCKS {
            break;
        }
    }
    out
}

/// One's own blocks, from the appointments (occurrences already made) of one's calendar.
pub fn of_appointments<'a>(items: impl IntoIterator<Item = &'a Appointment>, from: i64, to: i64) -> Vec<Block> {
    let blocks =
        items.into_iter().filter(|a| a.overlaps(from, to)).map(|a| Block { start: a.start, end: a.end, busy: a.busy }).take(MAX_BLOCKS * 2).collect();
    tidy(blocks, from, to)
}

/// The kind of busy a person is between two times: the strongest of the blocks that cover
/// any of it.
pub fn at(blocks: &[Block], start: i64, end: i64) -> Busy {
    blocks.iter().filter(|b| b.start < end && b.end > start).map(|b| b.busy).max_by_key(|b| weight(*b)).unwrap_or(Busy::Free)
}

/// When a working day is: its hours in minutes from midnight, and the weekdays worked
/// (0 = Sunday), as Outlook's Work time options keep them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Work {
    pub start: u32,
    pub end: u32,
    /// Bit `n` for weekday `n`.
    pub days: u8,
}

impl Default for Work {
    /// 8:00 to 17:00, Monday to Friday.
    fn default() -> Self {
        Self { start: 8 * 60, end: 17 * 60, days: 0b0011_1110 }
    }
}

impl Work {
    pub fn works_on(self, weekday: u32) -> bool {
        weekday < 7 && self.days & (1 << weekday) != 0
    }

    /// The weekdays worked, from Sunday.
    pub fn weekdays(self) -> Vec<u32> {
        (0..7).filter(|d| self.works_on(*d)).collect()
    }

    /// A sensible one: the day ends after it starts, within the day, on some weekday.
    pub fn valid(self) -> bool {
        self.start < self.end && self.end <= 24 * 60 && self.days & 0x7F != 0
    }
}

/// Whose free and busy times are looked at, and whether they must come.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub required: bool,
    /// `None` when their times could not be had: they do not hold a time back.
    pub blocks: Option<Vec<Block>>,
}

/// A suggested time: when, and how many of those who may come are free then.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Suggested {
    pub start: i64,
    pub end: i64,
    /// Of everyone whose times are known.
    pub free: usize,
    pub known: usize,
}

/// Times of `length` within working hours, from `from` (rounded up to the next half hour) for
/// up to [`MAX_DAYS`] days until `to`, when everyone who must come and whose times are known
/// is free: those when most of the rest are free first, earliest first among them, at most
/// `most`. Each day is a working day where `here` is. A length longer than a working day is
/// looked for across the whole of each day.
pub fn suggest(people: &[Person], from: i64, to: i64, length: i64, work: Work, here: &Zone, most: usize) -> Vec<Suggested> {
    let step = 30 * MINUTE;
    let most = most.min(MAX_SUGGESTED);
    if length <= 0 || length > DAY || to <= from || most == 0 || !work.valid() {
        return Vec::new();
    }
    let known: Vec<&Person> = people.iter().filter(|p| p.blocks.is_some()).collect();
    let mut found: Vec<Suggested> = Vec::new();
    let first = here.day_of(from);
    let last = here.day_of(to.saturating_sub(1)).min(first.saturating_add(MAX_DAYS - 1));
    let whole_day = length > i64::from(work.end - work.start) * MINUTE;
    for day in first..=last {
        let midnight = here.day_start(day);
        if !work.works_on(here.civil(midnight).weekday) {
            continue;
        }
        let (open, close) = if whole_day {
            (midnight, here.day_start(day.saturating_add(1)))
        } else {
            (midnight.saturating_add(i64::from(work.start) * MINUTE), midnight.saturating_add(i64::from(work.end) * MINUTE))
        };
        // From the first half hour at or after `from`.
        let mut start = open.max(from);
        let off = start.saturating_sub(midnight).rem_euclid(step);
        if off != 0 {
            start = start.saturating_add(step - off);
        }
        while start.saturating_add(length) <= close.min(to) {
            let end = start.saturating_add(length);
            let mut ok = true;
            let mut free = 0;
            for p in &known {
                let taken = p.blocks.as_deref().is_some_and(|b| takes(at(b, start, end)));
                if !taken {
                    free += 1;
                } else if p.required {
                    ok = false;
                    break;
                }
            }
            if ok {
                found.push(Suggested { start, end, free, known: known.len() });
            }
            start = start.saturating_add(step);
        }
        // Enough of the best are found once a whole day of everyone free has given them.
        if found.iter().filter(|s| s.free == known.len()).count() >= most {
            break;
        }
    }
    found.sort_by_key(|s| (std::cmp::Reverse(s.free), s.start));
    found.truncate(most);
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::from_civil;

    fn t(day: u32, hour: u32, minute: u32) -> i64 {
        from_civil(2026, 10, day, hour, minute, 0, 0)
    }

    fn block(day: u32, h1: u32, m1: u32, h2: u32, m2: u32, busy: Busy) -> Block {
        Block { start: t(day, h1, m1), end: t(day, h2, m2), busy }
    }

    #[test]
    fn blocks_are_put_in_order_and_joined() {
        let blocks = vec![
            block(12, 10, 0, 11, 0, Busy::Busy),
            block(12, 9, 0, 10, 0, Busy::Busy),
            block(12, 13, 0, 14, 0, Busy::Free),
            block(12, 15, 0, 15, 0, Busy::Busy),
            block(12, 16, 0, 17, 0, Busy::Tentative),
            block(11, 7, 0, 23, 0, Busy::OutOfOffice),
        ];
        let tidy = tidy(blocks, t(12, 0, 0), t(13, 0, 0));
        assert_eq!(tidy, vec![block(12, 9, 0, 11, 0, Busy::Busy), block(12, 16, 0, 17, 0, Busy::Tentative)]);
        assert_eq!(at(&tidy, t(12, 10, 30), t(12, 16, 30)), Busy::Busy);
        assert_eq!(at(&tidy, t(12, 11, 0), t(12, 16, 0)), Busy::Free);
        // Hostile: the end before the start, and far too many.
        let many: Vec<Block> = (0..20_000).map(|n| Block { start: n * MINUTE, end: n * MINUTE + 30, busy: Busy::Busy }).collect();
        assert_eq!(super::tidy(many, 0, i64::MAX).len(), MAX_BLOCKS);
        assert!(super::tidy(vec![Block { start: 9, end: 3, busy: Busy::Busy }], 0, 100).is_empty());
    }

    #[test]
    fn times_are_suggested_when_those_who_must_come_are_free() {
        let utc = Zone::utc();
        // Monday 12 October 2026.
        let me = Person { required: true, blocks: Some(vec![block(12, 8, 0, 10, 0, Busy::Busy)]) };
        let sam = Person { required: true, blocks: Some(vec![block(12, 10, 0, 11, 0, Busy::Tentative), block(12, 13, 0, 17, 0, Busy::OutOfOffice)]) };
        let ola = Person { required: false, blocks: Some(vec![block(12, 11, 0, 12, 0, Busy::Busy)]) };
        let unknown = Person { required: true, blocks: None };
        let people = [me, sam, ola, unknown];
        let found = suggest(&people, t(12, 0, 0), t(13, 0, 0), 60 * MINUTE, Work::default(), &utc, 5);
        // 11:00 to 13:00 is free for both who must come; 12:00 is free for Ola too.
        assert_eq!(found.first().map(|s| (s.start, s.free, s.known)), Some((t(12, 12, 0), 3, 3)));
        assert_eq!(found.iter().map(|s| s.start).collect::<Vec<_>>(), vec![t(12, 12, 0), t(12, 11, 0), t(12, 11, 30)]);
        // Begun at 11:10: the next half hour.
        let later = suggest(&people, t(12, 11, 10), t(13, 0, 0), 30 * MINUTE, Work::default(), &utc, 5);
        assert_eq!(later.iter().map(|s| s.start).min(), Some(t(12, 11, 30)));
        // Saturday and Sunday are no working days; Monday is.
        let weekend = suggest(&[], t(10, 0, 0), t(13, 0, 0), 60 * MINUTE, Work::default(), &utc, 1);
        assert_eq!(weekend.first().map(|s| s.start), Some(t(12, 8, 0)));
        // Nonsense asks for nothing.
        assert!(suggest(&people, t(12, 0, 0), t(13, 0, 0), 0, Work::default(), &utc, 5).is_empty());
        assert!(suggest(&people, t(13, 0, 0), t(12, 0, 0), 60 * MINUTE, Work::default(), &utc, 5).is_empty());
        assert!(suggest(&people, i64::MIN, i64::MAX, 60 * MINUTE, Work { start: 0, end: 0, days: 0 }, &utc, 5).is_empty());
        let wide = suggest(&[], i64::MIN / 2, i64::MAX / 2, DAY, Work::default(), &utc, 500);
        assert!(wide.len() <= MAX_SUGGESTED);
    }
}
