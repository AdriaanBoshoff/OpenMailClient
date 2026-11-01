//! Free and busy times (VFREEBUSY, RFC 5545 3.6.4): a request asking a server when people are
//! taken (RFC 5546 4.3.2, as CalDAV scheduling sends it, RFC 6638 5), and the times read from
//! its reply or from a file someone publishes at their free/busy address.

use super::{MAX_TEXT, Prop, components, cut, duration, line, mailto, stamp, utc_text};
use openmailclient_model::calendar::Busy;
use openmailclient_model::free_busy::{Block, MAX_BLOCKS, tidy};
use openmailclient_model::time;

/// People's times read from one file at most.
const MAX_PEOPLE: usize = 500;

/// One person's free and busy times, as a VFREEBUSY gives them.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FreeBusy {
    /// Whose times they are: the attendee of a reply, or the organizer of a published file.
    pub address: String,
    /// The span the times were given for, when the file says.
    pub start: Option<i64>,
    pub end: Option<i64>,
    /// When they are taken, in order; free times are left out.
    pub blocks: Vec<Block>,
}

/// An address of a CAL-ADDRESS, without its `mailto:`.
fn address_of(p: &Prop) -> String {
    let v = p.value.trim();
    let v = v.get(..7).filter(|s| s.eq_ignore_ascii_case("mailto:")).map_or(v, |_| v.get(7..).unwrap_or(""));
    cut(v.trim(), MAX_TEXT)
}

/// A DATE-TIME as UTC ticks; one with no zone is taken as UTC, as free and busy times are
/// always written (RFC 5545 3.8.2.6).
fn utc_at(v: &str) -> Option<i64> {
    let s = stamp(v)?;
    Some(time::from_unix(s.day.saturating_mul(86_400).saturating_add(s.secs)))
}

/// A PERIOD (RFC 5545 3.3.9), `start/end` or `start/duration`.
fn period(v: &str) -> Option<(i64, i64)> {
    let (a, b) = v.split_once('/')?;
    let start = utc_at(a)?;
    let end = match utc_at(b) {
        Some(end) => end,
        None => start.saturating_add(duration(b)?.saturating_mul(10_000_000)),
    };
    (end > start).then_some((start, end))
}

/// The kind of busy an FBTYPE says (RFC 5545 3.2.9); one not known is busy, as it says.
fn kind(p: &Prop) -> Busy {
    match p.param("FBTYPE").map(|v| v.trim().to_ascii_uppercase()).as_deref() {
        Some("FREE") => Busy::Free,
        Some("BUSY-TENTATIVE") => Busy::Tentative,
        Some("BUSY-UNAVAILABLE") => Busy::OutOfOffice,
        _ => Busy::Busy,
    }
}

/// Every person's times in a calendar file: each VFREEBUSY of each VCALENDAR.
pub fn read_free_busy(bytes: &[u8]) -> Result<Vec<FreeBusy>, String> {
    if bytes.len() > super::MAX_BYTES {
        return Err(format!("a calendar file is read up to {} MB", super::MAX_BYTES >> 20));
    }
    let (roots, _) = components(bytes);
    if !roots.iter().any(|c| c.name == "VCALENDAR") {
        return Err("this is not an iCalendar file: it has no VCALENDAR".into());
    }
    let mut out = Vec::new();
    for fb in roots.iter().filter(|c| c.name == "VCALENDAR").flat_map(|c| c.children.iter()).filter(|c| c.name == "VFREEBUSY") {
        if out.len() >= MAX_PEOPLE {
            break;
        }
        let address = fb.prop("ATTENDEE").or_else(|| fb.prop("ORGANIZER")).map(address_of).unwrap_or_default();
        let start = fb.prop("DTSTART").and_then(|p| utc_at(&p.value));
        let end = fb.prop("DTEND").and_then(|p| utc_at(&p.value));
        let mut blocks = Vec::new();
        'all: for p in fb.all("FREEBUSY") {
            let busy = kind(p);
            for one in p.value.split(',') {
                if blocks.len() >= MAX_BLOCKS * 2 {
                    break 'all;
                }
                if let Some((s, e)) = period(one) {
                    blocks.push(Block { start: s, end: e, busy });
                }
            }
        }
        let blocks = tidy(blocks, start.unwrap_or(i64::MIN), end.filter(|e| start.is_none_or(|s| *e > s)).unwrap_or(i64::MAX));
        out.push(FreeBusy { address, start, end, blocks });
    }
    Ok(out)
}

/// A request for the free and busy times of `attendees` from `start` to `end`, sent by
/// `organizer` to a scheduling outbox (RFC 6638 5; RFC 5546 4.3.2).
pub fn free_busy_request(organizer: &str, attendees: &[String], start: i64, end: i64, uid: &str, now: i64) -> String {
    let mut out = String::new();
    for l in ["BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//OpenMailClient//EN", "METHOD:REQUEST", "BEGIN:VFREEBUSY"] {
        line(&mut out, l);
    }
    let uid: String = uid.chars().filter(|c| !c.is_control()).collect();
    line(&mut out, &format!("UID:{uid}"));
    line(&mut out, &format!("DTSTAMP:{}", utc_text(now)));
    line(&mut out, &format!("DTSTART:{}", utc_text(start)));
    line(&mut out, &format!("DTEND:{}", utc_text(end)));
    line(&mut out, &format!("ORGANIZER:{}", mailto(organizer)));
    for a in attendees {
        line(&mut out, &format!("ATTENDEE:{}", mailto(a)));
    }
    line(&mut out, "END:VFREEBUSY");
    line(&mut out, "END:VCALENDAR");
    out
}

/// `address`'s times from `start` to `end`, as a file published at a free/busy address
/// (METHOD:PUBLISH) or, with `reply`, as a scheduling server answers (METHOD:REPLY).
pub fn write_free_busy(address: &str, start: i64, end: i64, blocks: &[Block], now: i64, reply: bool) -> String {
    let mut out = String::new();
    line(&mut out, "BEGIN:VCALENDAR");
    line(&mut out, "VERSION:2.0");
    line(&mut out, "PRODID:-//OpenMailClient//EN");
    line(&mut out, if reply { "METHOD:REPLY" } else { "METHOD:PUBLISH" });
    line(&mut out, "BEGIN:VFREEBUSY");
    line(&mut out, &format!("DTSTAMP:{}", utc_text(now)));
    line(&mut out, &format!("DTSTART:{}", utc_text(start)));
    line(&mut out, &format!("DTEND:{}", utc_text(end)));
    line(&mut out, &format!("{}:{}", if reply { "ATTENDEE" } else { "ORGANIZER" }, mailto(address)));
    for b in tidy(blocks.to_vec(), start, end) {
        let fbtype = match b.busy {
            Busy::Tentative => "BUSY-TENTATIVE",
            Busy::OutOfOffice => "BUSY-UNAVAILABLE",
            // Working elsewhere holds no time back; it is written as free and so left out.
            Busy::Free | Busy::WorkingElsewhere => continue,
            Busy::Busy => "BUSY",
        };
        line(&mut out, &format!("FREEBUSY;FBTYPE={fbtype}:{}/{}", utc_text(b.start), utc_text(b.end)));
    }
    line(&mut out, "END:VFREEBUSY");
    line(&mut out, "END:VCALENDAR");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use openmailclient_model::calendar::MINUTE;

    fn minutes(n: i64) -> i64 {
        n * MINUTE
    }

    fn t(day: u32, h: u32, m: u32) -> i64 {
        time::from_civil(2026, 10, day, h, m, 0, 0)
    }

    #[test]
    fn free_and_busy_times_are_read_and_written() {
        let blocks = vec![
            Block { start: t(12, 9, 0), end: t(12, 10, 0), busy: Busy::Busy },
            Block { start: t(12, 11, 0), end: t(12, 11, 30), busy: Busy::Tentative },
            Block { start: t(13, 0, 0), end: t(14, 0, 0), busy: Busy::OutOfOffice },
            Block { start: t(12, 15, 0), end: t(12, 16, 0), busy: Busy::WorkingElsewhere },
        ];
        let file = write_free_busy("sam@example.org", t(12, 0, 0), t(15, 0, 0), &blocks, t(1, 0, 0), true);
        assert!(file.contains("METHOD:REPLY") && file.contains("ATTENDEE:mailto:sam@example.org"), "{file}");
        let read = read_free_busy(file.as_bytes()).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].address, "sam@example.org");
        assert_eq!((read[0].start, read[0].end), (Some(t(12, 0, 0)), Some(t(15, 0, 0))));
        assert_eq!(read[0].blocks, blocks[..3].to_vec());

        // Periods by duration, several on a line, one kind by default, free ones left out.
        let published = "BEGIN:VCALENDAR\r\nBEGIN:VFREEBUSY\r\nORGANIZER:MAILTO:ola@example.org\r\n\
FREEBUSY:20261012T090000Z/PT1H,20261012T140000Z/20261012T143000Z\r\n\
FREEBUSY;FBTYPE=FREE:20261012T100000Z/PT1H\r\nFREEBUSY;FBTYPE=X-OWN:20261012T160000Z/PT15M\r\n\
END:VFREEBUSY\r\nEND:VCALENDAR\r\n";
        let read = read_free_busy(published.as_bytes()).unwrap();
        assert_eq!(read[0].address, "ola@example.org");
        assert_eq!(
            read[0].blocks,
            vec![
                Block { start: t(12, 9, 0), end: t(12, 10, 0), busy: Busy::Busy },
                Block { start: t(12, 14, 0), end: t(12, 14, 30), busy: Busy::Busy },
                Block { start: t(12, 16, 0), end: t(12, 16, 15), busy: Busy::Busy },
            ]
        );

        let ask = free_busy_request(
            "me@example.org",
            &["sam@example.org".into(), "ola@example.org\r\nX:y".into()],
            t(12, 0, 0),
            t(13, 0, 0),
            "fb-1",
            t(1, 0, 0),
        );
        assert!(ask.contains("METHOD:REQUEST") && ask.contains("ORGANIZER:mailto:me@example.org") && ask.contains("DTEND:20261013T000000Z"), "{ask}");
        assert_eq!(ask.matches("ATTENDEE:").count(), 2);
        assert!(!ask.contains("\r\nX:y"), "{ask}");
    }

    #[test]
    fn hostile_free_busy_files_are_read_safely() {
        assert!(read_free_busy(b"").is_err());
        assert!(read_free_busy(b"\xff\xfe garbage").is_err());
        assert!(read_free_busy(&vec![b'A'; super::super::MAX_BYTES + 1]).is_err());
        // Truncated, periods of nonsense, the end before the start, a span backwards.
        let bad = "BEGIN:VCALENDAR\r\nBEGIN:VFREEBUSY\r\nDTSTART:20261013T000000Z\r\nDTEND:20261012T000000Z\r\n\
FREEBUSY:20261012T090000Z/20261012T080000Z,garbage,/,20261012T090000Z/P99999999W,99999999T999999Z/PT1H\r\nFREEBUSY";
        let read = read_free_busy(bad.as_bytes()).unwrap();
        assert_eq!(read.len(), 1);
        assert!(read[0].blocks.len() <= 1);
        // Very many periods are held to a bound.
        let mut many = String::from("BEGIN:VCALENDAR\r\nBEGIN:VFREEBUSY\r\nFREEBUSY:");
        for n in 0..30_000 {
            let s = t(1, 0, 0) + minutes(n * 2);
            many.push_str(&format!("{}/PT1M,", utc_text(s)));
        }
        many.push_str("\r\nEND:VFREEBUSY\r\nEND:VCALENDAR\r\n");
        let read = read_free_busy(many.as_bytes()).unwrap();
        assert!(read[0].blocks.len() <= MAX_BLOCKS);
    }
}
