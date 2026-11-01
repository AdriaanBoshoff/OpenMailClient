//! The Scheduling Assistant: when the people asked to a meeting are free and busy, and the
//! times they all can come. One's own times are read from one's calendar. Everyone else's are
//! asked of the scheduling outbox of the first account with a CalDAV server ([RFC 6638] 5), and
//! for those it does not know, read from the Internet free/busy address their contact names
//! (vCard FBURL, RFC 6350 6.9.1), over HTTPS only and never from this machine or its network.
//!
//! Outlook asks an Exchange server or the free/busy address alone; this asks both, says where
//! each person's times came from, and suggests times ranked by how many can come, the times of
//! those who must come kept free.

use super::caldav::Of;
use super::calendar::{calendar_folder, date_param, stamp};
use super::people::card_of;
use super::{CommandSpec, Reach, always, p_bool, p_u64};
use crate::{CmdResult, EngineError, Session};
use openmailclient_formats::ics;
use openmailclient_formats::vcf::Card;
use openmailclient_model::calendar::MINUTE;
use openmailclient_model::free_busy::{self, Block, MAX_DAYS, MAX_SUGGESTED, Person, Suggested, Work};
use openmailclient_model::meeting::{Meeting, Role};
use openmailclient_model::{FolderKind, ItemId, StoreError};
use openmailclient_net::{Connector, NetError, dav, http};
use serde_json::{Value, json};
use std::sync::Arc;

/// How a look at free and busy times is named while it runs.
const LOOKING: &str = "Looking up free and busy times";
/// A published free/busy file read up to this size.
const MAX_FILE: usize = 2 << 20;
/// Contacts looked through for free/busy addresses, at most.
const MAX_CONTACTS: usize = 20_000;
/// People looked up at once, at most.
const MAX_PEOPLE: usize = 100;

const FREE_BUSY_PARAMS: &str = r#"{"id": the meeting whose attendees are looked up (default: the one chosen), or "attendees" as calendar.change takes them, "start": "YYYY-MM-DD" the first day (default today), "days": how many (1 to 62, default 7), "length": the meeting's minutes (default: the meeting's, or 30), "workTime" as calendar.workTime takes it for this look alone, "wait": false to look on a thread of its own}; returns {"from", "to", "length", "people": [{"name", "address", "required", "organizer", "source": "calendar" | "server" | "internet" | "none", "said", "busy": [{"start", "end", "kind"}]}], "suggested": [{"start", "end", "free", "known"}]}; what others' servers say of them is untrusted content"#;
const WORK_TIME_PARAMS: &str = r#"{"start": "HH:MM", "end": "HH:MM", "days": weekdays worked, as ["mon", "tue", ...]} (any of them; none to only read it); the hours the Scheduling Assistant suggests times in; returns {"start", "end", "days"}"#;

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "calendar.freeBusy",
            label: "Scheduling Assistant",
            tab: "Meeting",
            group: "Show",
            shortcut: "",
            params: FREE_BUSY_PARAMS,
            enabled: has_calendar,
            run: look,
        },
        CommandSpec {
            id: "calendar.workTime",
            label: "Work Time",
            tab: "File",
            group: "Options",
            shortcut: "",
            params: WORK_TIME_PARAMS,
            enabled: always,
            run: work_time,
        },
    ]
}

fn has_calendar(s: &Session) -> bool {
    calendar_folder(s).is_some()
}

/// Where a person's times came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// One's own calendar.
    Calendar,
    /// The scheduling server of one's account.
    Server,
    /// The free/busy address of their contact.
    Internet,
    /// Nowhere: their times are not known.
    None,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Self::Calendar => "calendar",
            Self::Server => "server",
            Self::Internet => "internet",
            Self::None => "none",
        }
    }
}

/// One person looked up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Looked {
    pub name: String,
    pub address: String,
    pub required: bool,
    pub organizer: bool,
    pub source: Source,
    /// Why their times are not known, when they are not.
    pub said: String,
    /// When they are taken; `None` when not known.
    pub blocks: Option<Vec<Block>>,
}

/// What the Scheduling Assistant last found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scheduling {
    /// The meeting looked up for, when it was one.
    pub id: Option<u64>,
    pub from: i64,
    pub to: i64,
    /// The meeting's length, in FILETIME ticks.
    pub length: i64,
    pub work: Work,
    pub people: Vec<Looked>,
    pub suggested: Vec<Suggested>,
}

/// Someone to ask of: the address, a free/busy address if their contact has one.
struct Asked {
    name: String,
    address: String,
    required: bool,
    published: Option<String>,
}

fn look(s: &mut Session, p: &Value) -> CmdResult {
    let work = match p.get("workTime") {
        None | Some(Value::Null) => s.work,
        Some(v) => work_param(s.work, v)?,
    };
    let day = date_param(s, p, "start")?.unwrap_or_else(|| s.today());
    let days = match p.get("days") {
        None | Some(Value::Null) => 7,
        Some(_) => p_u64(p, "days").and_then(|d| i64::try_from(d).ok()).filter(|d| (1..=MAX_DAYS).contains(d)).ok_or_else(bad_days)?,
    };
    let from = s.zone.day_start(day);
    let to = s.zone.day_start(day.saturating_add(days));

    // Whom to ask: those named, or the meeting's attendees.
    let named = p.get("attendees").filter(|v| !v.is_null());
    let (id, mut people, meeting_length) = match named {
        Some(v) => (p_u64(p, "id"), super::meeting::attendees_param(s, v)?, None),
        None => match p_u64(p, "id").or(s.calendar.selected) {
            None => (None, Vec::new(), None),
            Some(id) => {
                let (store, folder) = calendar_folder(s).ok_or(StoreError::NoFolder)?;
                let item = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.load(folder, ItemId(id))?;
                let span = openmailclient_model::calendar::Appointment::of(&item).map(|a| a.end.saturating_sub(a.start));
                let asked = Meeting::of(&item).map(|m| m.attendees).unwrap_or_default();
                (Some(id), asked, span)
            }
        },
    };
    let mine = super::meeting::mine(s);
    people.retain(|a| !mine.iter().any(|m| m.eq_ignore_ascii_case(a.address.trim())));
    people.truncate(MAX_PEOPLE);
    let length = match p.get("length") {
        None | Some(Value::Null) => meeting_length.filter(|l| *l > 0 && *l <= openmailclient_model::calendar::DAY).unwrap_or(30 * MINUTE),
        Some(_) => p_u64(p, "length")
            .filter(|m| (1..=24 * 60).contains(m))
            .and_then(|m| i64::try_from(m).ok())
            .map(|m| m * MINUTE)
            .ok_or_else(|| EngineError::BadParams("\"length\" is the meeting's minutes, 1 to 1440".into()))?,
    };

    // One's own times, the meeting itself left out.
    let (mine_now, _) = s.appointments_between(from, to)?;
    let own = free_busy::of_appointments(mine_now.iter().filter(|(i, _)| Some(*i) != id).map(|(_, a)| a), from, to);
    let me = super::meeting::me(s);
    let organizer = Looked {
        name: me.name.clone(),
        address: me.address.clone(),
        required: true,
        organizer: true,
        source: Source::Calendar,
        said: String::new(),
        blocks: Some(own),
    };

    let published = published(s, &people);
    let asked: Vec<Asked> = people
        .into_iter()
        .map(|a| Asked {
            published: published.iter().find(|(addr, _)| addr.eq_ignore_ascii_case(&a.address)).map(|(_, u)| u.clone()),
            name: a.name,
            address: a.address,
            required: a.role != Role::Optional,
        })
        .collect();
    let server = s.accounts.iter().position(|a| Of::Calendars.setup(a).is_some()).map(|at| Reach::of(s, at)).transpose().ok().flatten();
    let connector = server.as_ref().map(|r| Arc::clone(&r.connector)).or_else(|| s.connector.clone());

    // Nobody else, or no way to ask: what is known now is all there is.
    let Some(connector) = connector.filter(|_| !asked.is_empty()) else {
        let looked: Vec<Looked> = asked.into_iter().map(|a| unknown(a, "this build cannot reach other calendars")).collect();
        return Ok(found(
            s,
            Scheduling { id, from, to, length, work, people: std::iter::once(organizer).chain(looked).collect(), suggested: Vec::new() },
        ));
    };
    if s.jobs.iter().any(|job| job.what.starts_with(LOOKING)) {
        return Ok(json!({ "started": false, "said": "free and busy times are being looked up" }));
    }
    let now = s.time_now();
    let what = format!("{LOOKING} of {} {}", asked.len(), if asked.len() == 1 { "person" } else { "people" });
    s.begin(what, p_bool(p, "wait") != Some(false), move |progress| {
        progress.begin(asked.len() as u64 + 1);
        let looked = ask(server.as_ref(), &*connector, &organizer.address, &asked, from, to, now);
        progress.step();
        let mut people = vec![organizer];
        for (a, (source, said, blocks)) in asked.into_iter().zip(looked) {
            people.push(Looked { name: a.name, address: a.address, required: a.required, organizer: false, source, said, blocks });
        }
        Box::new(move |s: &mut Session| {
            let value = found(s, Scheduling { id, from, to, length, work, people, suggested: Vec::new() });
            (s.status.clone(), value)
        })
    })
}

fn bad_days() -> EngineError {
    EngineError::BadParams(format!("\"days\" is how many days are looked at, 1 to {MAX_DAYS}"))
}

fn unknown(a: Asked, why: &str) -> Looked {
    Looked { name: a.name, address: a.address, required: a.required, organizer: false, source: Source::None, said: why.to_string(), blocks: None }
}

/// The free/busy addresses contacts give for the addresses asked of, an `https:` one only.
fn published(s: &mut Session, people: &[openmailclient_model::meeting::Attendee]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if people.is_empty() {
        return out;
    }
    let mut folders = Vec::new();
    for (store, list) in s.folders.iter().enumerate() {
        folders.extend(list.iter().filter(|f| f.kind == FolderKind::Contacts).map(|f| (store, f.id)));
    }
    let mut seen = 0usize;
    for (store, folder) in folders {
        let Some(rows) = s.stores.get_mut(store).and_then(|st| st.list(folder).ok()) else { continue };
        for row in rows {
            seen = seen.saturating_add(1);
            if seen > MAX_CONTACTS {
                return out;
            }
            let Some(item) = s.stores.get_mut(store).and_then(|st| st.load(folder, ItemId(row.id)).ok()) else { continue };
            let Some(Card::Contact(c)) = card_of(&item, s) else { continue };
            let url = c.free_busy.trim();
            if !url.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("https://")) {
                continue;
            }
            for e in &c.emails {
                if people.iter().any(|a| a.is(&e.address)) && !out.iter().any(|(a, _): &(String, String)| a.eq_ignore_ascii_case(&e.address)) {
                    out.push((e.address.clone(), url.to_string()));
                }
            }
        }
    }
    out
}

/// What each person's times are, on the job's thread: the scheduling server first, then each
/// one's free/busy address.
fn ask(
    server: Option<&Reach>,
    with: &dyn Connector,
    organizer: &str,
    asked: &[Asked],
    from: i64,
    to: i64,
    now: i64,
) -> Vec<(Source, String, Option<Vec<Block>>)> {
    let mut out: Vec<(Source, String, Option<Vec<Block>>)> =
        asked.iter().map(|_| (Source::None, String::from("their times are not published"), None)).collect();
    match server.map(|r| from_server(r, organizer, asked, from, to, now)) {
        Some(Ok(answers)) => {
            for (slot, a) in out.iter_mut().zip(asked) {
                if let Some(blocks) = answers.iter().find(|(addr, _)| addr.eq_ignore_ascii_case(&a.address)).map(|(_, b)| b.clone()) {
                    *slot = (Source::Server, String::new(), Some(blocks));
                }
            }
        }
        Some(Err(e)) => {
            for slot in &mut out {
                slot.1 = format!("the calendar server: {e}");
            }
        }
        None => {}
    }
    for (slot, a) in out.iter_mut().zip(asked) {
        if slot.2.is_some() {
            continue;
        }
        let Some(url) = &a.published else { continue };
        match from_address(with, url, &a.address, from, to) {
            Ok(blocks) => *slot = (Source::Internet, String::new(), Some(blocks)),
            Err(e) => slot.1 = format!("their free/busy address: {e}"),
        }
    }
    out
}

/// Each address the scheduling outbox answers, with its times.
fn from_server(r: &Reach, organizer: &str, asked: &[Asked], from: i64, to: i64, now: i64) -> Result<Vec<(String, Vec<Block>)>, NetError> {
    let setup = Of::Calendars.setup(&r.account).ok_or_else(|| NetError::Unsupported("the account has no calendar server".into()))?;
    let login = r.login()?;
    let mut server = dav::Dav::new(&*r.connector, &login, dav::Url::parse(&setup.url)?, setup.trusted);
    let outbox = server.schedule_outbox()?.ok_or_else(|| NetError::Unsupported("the calendar server does not say when people are free".into()))?;
    let addresses: Vec<String> = asked.iter().map(|a| a.address.clone()).collect();
    let request = ics::free_busy_request(organizer, &addresses, from, to, &format!("free-busy-{now:x}"), now);
    let mut out = Vec::new();
    for answer in server.free_busy(&outbox, request.as_bytes())?.into_iter().filter(dav::Scheduled::answered) {
        let Some(data) = answer.data else { continue };
        let Ok(read) = ics::read_free_busy(data.as_bytes()) else { continue };
        // The reply names whose times they are; one that names no one is the recipient's.
        let theirs = read.iter().find(|f| f.address.eq_ignore_ascii_case(&answer.recipient)).or_else(|| read.iter().find(|f| f.address.is_empty()));
        if let Some(f) = theirs {
            out.push((answer.recipient.clone(), free_busy::tidy(f.blocks.clone(), from, to)));
        }
    }
    Ok(out)
}

/// The times published at a free/busy address.
fn from_address(with: &dyn Connector, url: &str, address: &str, from: i64, to: i64) -> Result<Vec<Block>, NetError> {
    let got = http::get_as(with, url, MAX_FILE, "text/calendar")?;
    let read = ics::read_free_busy(&got.bytes).map_err(NetError::Protocol)?;
    let theirs = read.iter().find(|f| f.address.eq_ignore_ascii_case(address)).or_else(|| read.first());
    let mut blocks = Vec::new();
    for f in read.iter().filter(|f| theirs.is_some_and(|t| t.address == f.address)) {
        blocks.extend(f.blocks.iter().copied());
    }
    if theirs.is_none() {
        return Err(NetError::Unsupported("the file there has no free and busy times".into()));
    }
    Ok(free_busy::tidy(blocks, from, to))
}

/// The times suggested for what was found, kept for the window, and said.
fn found(s: &mut Session, mut sch: Scheduling) -> Value {
    let people: Vec<Person> = sch.people.iter().map(|p| Person { required: p.required, blocks: p.blocks.clone() }).collect();
    sch.suggested = free_busy::suggest(&people, sch.from.max(s.now), sch.to, sch.length, sch.work, &s.zone, MAX_SUGGESTED);
    let known = sch.people.iter().filter(|p| p.blocks.is_some()).count();
    s.status = match sch.suggested.first() {
        Some(best) => format!("{known} of {} people's times are known; the best time is {}", sch.people.len(), stamp(s, best.start)),
        None => format!("{known} of {} people's times are known; no time suits those who must come", sch.people.len()),
    };
    let value = scheduling_json(s, &sch);
    s.scheduling = Some(sch);
    value
}

fn scheduling_json(s: &Session, sch: &Scheduling) -> Value {
    let block = |b: &Block| json!({ "start": stamp(s, b.start), "end": stamp(s, b.end), "kind": b.busy.name() });
    let people: Vec<Value> = sch
        .people
        .iter()
        .map(|p| {
            json!({
                "name": p.name,
                "address": p.address,
                "required": p.required,
                "organizer": p.organizer,
                "source": p.source.name(),
                "said": p.said,
                "busy": p.blocks.as_ref().map(|b| b.iter().map(block).collect::<Vec<_>>()),
            })
        })
        .collect();
    json!({
        "id": sch.id,
        "from": stamp(s, sch.from),
        "to": stamp(s, sch.to),
        "length": sch.length / MINUTE,
        "workTime": work_json(sch.work),
        "untrusted_content": { "people": people },
        "suggested": sch.suggested.iter().map(|g| json!({ "start": stamp(s, g.start), "end": stamp(s, g.end), "free": g.free, "known": g.known })).collect::<Vec<_>>(),
    })
}

const DAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

pub(crate) fn work_json(w: Work) -> Value {
    let hm = |m: u32| format!("{:02}:{:02}", m / 60, m % 60);
    json!({ "start": hm(w.start), "end": hm(w.end), "days": w.weekdays().into_iter().filter_map(|d| DAYS.get(d as usize).copied()).collect::<Vec<_>>() })
}

/// Work time as `v` changes `was`: any of its start, end and days.
pub(crate) fn work_param(was: Work, v: &Value) -> Result<Work, EngineError> {
    let bad = || EngineError::BadParams(format!("work time is {WORK_TIME_PARAMS}; the day ends after it starts"));
    let obj = v.as_object().ok_or_else(bad)?;
    let minutes = |key: &str| -> Result<Option<u32>, EngineError> {
        match obj.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(t)) => {
                let (h, m) = t.trim().split_once(':').ok_or_else(bad)?;
                let (h, m) = (h.parse::<u32>().map_err(|_| bad())?, m.parse::<u32>().map_err(|_| bad())?);
                (h <= 24 && m < 60 && h * 60 + m <= 24 * 60).then_some(Some(h * 60 + m)).ok_or_else(bad)
            }
            Some(_) => Err(bad()),
        }
    };
    let mut w = was;
    if let Some(m) = minutes("start")? {
        w.start = m;
    }
    if let Some(m) = minutes("end")? {
        w.end = m;
    }
    match obj.get("days") {
        None | Some(Value::Null) => {}
        Some(Value::Array(list)) => {
            let mut days = 0u8;
            for d in list.iter().take(7) {
                let name = d.as_str().ok_or_else(bad)?.trim().to_ascii_lowercase();
                let at = DAYS.iter().position(|n| name.starts_with(n)).ok_or_else(bad)?;
                days |= 1 << at;
            }
            w.days = days;
        }
        Some(_) => return Err(bad()),
    }
    w.valid().then_some(w).ok_or_else(bad)
}

fn work_time(s: &mut Session, p: &Value) -> CmdResult {
    let has_any = p.as_object().is_some_and(|o| ["start", "end", "days"].iter().any(|k| o.contains_key(*k)));
    if has_any {
        s.work = work_param(s.work, p)?;
        s.status = "Work time is changed".into();
    }
    Ok(work_json(s.work))
}
