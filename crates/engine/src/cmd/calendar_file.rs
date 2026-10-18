//! Calendar files (iCalendar, RFC 5545): one opened and its events put in the calendar, and the
//! calendar, a stretch of it or one appointment saved or mailed as one. The format itself is
//! `openmailclient_formats::ics`; an event's UID is kept as its appointment's GlobalObjectId
//! ([MS-OXCICAL] 2.1.3.1.1.20.26), so a file opened again brings what it added up to date
//! rather than adding it twice, as Outlook does.

use super::calendar::{calendar_folder, date_param, day_text, stamp, writable_calendar};
use super::{CommandSpec, always, need_str, p_bool, p_str, p_u64};
use crate::{CmdResult, Draft, EngineError, Session, io, undo::Step};
use openmailclient_formats::ics;
use openmailclient_model::calendar::{self, Appointment};
use openmailclient_model::{Attachment, FolderId, Item, ItemId, StoreError};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;

/// Items of a Calendar folder looked at for one file at most.
const MAX_ITEMS: usize = 20_000;
/// Longest description shown of an event before it is added, in characters.
const MAX_SHOWN: usize = 500;
/// Appointments listed in the body of a calendar sent by mail.
const MAX_LISTED: usize = 200;

const IMPORT_PARAMS: &str = r#"{"path": a calendar file (.ics), or "attachment": the number of an attachment of the open message, "preview": true to read what it holds and change nothing, "only": [event numbers] to add those alone (default: all)}; an event the calendar has already (known by its UID) brings that appointment up to date rather than being added twice; returns {"method", "events": [{"index", "start", "end", "allDay", "lastDay": the last day of an all-day event, "busy", "reminder", "recurring", "pattern", "replaces": the id of the appointment it brings up to date, or null, "untrusted_content": {"subject", "location", "description", "uid", "notes": what of it could not be kept}}], "added", "updated", "untrusted_content": {"name", "notes"}}; undone as one step"#;
const SAVE_PARAMS: &str = r#"{"path": target ending in .ics, "id": one appointment (a series whole), or "from": "YYYY-MM-DD" and "to": "YYYY-MM-DD" (the day after the last) for those that fall between (default: the whole calendar), "name": the calendar's name in the file (default: the folder's)}; a series is written with its pattern, the occurrences deleted from it and those changed; returns {"events", "unreadable", "untrusted_content": {"path"}}"#;
const EMAIL_PARAMS: &str = r#"{"id", or "from" and "to", as calendar.saveAs takes them (default: the days shown), "name"}; a new message with the calendar attached as an .ics file and its appointments listed in the body; returns {"events"}"#;

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "calendar.import",
            label: "Open Calendar",
            tab: "Home",
            group: "Manage Calendars",
            shortcut: "",
            params: IMPORT_PARAMS,
            enabled: always,
            run: import,
        },
        CommandSpec {
            id: "calendar.saveAs",
            label: "Save Calendar",
            tab: "File",
            group: "Save As",
            shortcut: "",
            params: SAVE_PARAMS,
            enabled: has_calendar,
            run: save_as,
        },
        CommandSpec {
            id: "calendar.email",
            label: "E-mail Calendar",
            tab: "Home",
            group: "Share",
            shortcut: "",
            params: EMAIL_PARAMS,
            enabled: has_calendar,
            run: email,
        },
    ]
}

fn has_calendar(s: &Session) -> bool {
    calendar_folder(s).is_some()
}

fn cut(text: &str, most: usize) -> String {
    text.chars().take(most).collect()
}

/// A calendar's name made fit to be a file's: no character a file system keeps for itself.
fn file_safe(name: &str) -> String {
    let safe: String = name.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).take(100).collect();
    let safe = safe.trim().trim_end_matches('.').to_string();
    if safe.is_empty() { "Calendar".into() } else { safe }
}

/// The bytes of the calendar file asked for: one on disk, or an attachment of the open message.
fn calendar_bytes(s: &Session, p: &Value) -> Result<Vec<u8>, EngineError> {
    match (p_str(p, "path"), p.get("attachment")) {
        (Some(_), Some(_)) => Err(EngineError::BadParams("a calendar is opened from \"path\" or \"attachment\", not both".into())),
        (Some(path), None) => {
            let file = std::fs::File::open(Path::new(path))?;
            if file.metadata()?.is_dir() {
                return Err(EngineError::BadParams(format!("{path} is a folder, not a calendar file")));
            }
            // Read no further than a calendar file may be, however long the file says it is.
            let mut data = Vec::new();
            file.take(u64::try_from(ics::MAX_BYTES).unwrap_or(u64::MAX).saturating_add(1)).read_to_end(&mut data)?;
            if data.len() > ics::MAX_BYTES {
                return Err(EngineError::Format("this calendar file is too large to open".into()));
            }
            Ok(data)
        }
        (None, Some(v)) => {
            let index = v
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .ok_or_else(|| EngineError::BadParams("\"attachment\" is the number of an attachment of the open message".into()))?;
            let item = s.open.as_ref().ok_or(StoreError::NoItem)?;
            let att = item
                .attachments
                .get(index)
                .ok_or_else(|| EngineError::BadParams(format!("there is no attachment {index}; the message has {}", item.attachments.len())))?;
            if att.embedded.is_some() {
                return Err(EngineError::Format("that attachment is a message, not a calendar file".into()));
            }
            Ok(att.data.clone())
        }
        (None, None) => Err(EngineError::BadParams("\"path\" or \"attachment\" is required".into())),
    }
}

/// The appointments of a Calendar folder by the id each keeps for life, the first of any two
/// that share one.
fn by_global_id(s: &mut Session, store: usize, folder: FolderId) -> Result<HashMap<Vec<u8>, u64>, EngineError> {
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    let mut found = HashMap::new();
    for row in st.list(folder)?.into_iter().take(MAX_ITEMS) {
        if let Ok(item) = st.load(folder, ItemId(row.id))
            && let Some(id) = calendar::global_id(&item)
        {
            found.entry(id).or_insert(row.id);
        }
    }
    Ok(found)
}

fn event_json(s: &Session, index: usize, e: &ics::Event, replaces: Option<u64>) -> Value {
    let a = &e.appointment;
    json!({
        "index": index,
        "start": stamp(s, a.start),
        "end": stamp(s, a.end),
        "allDay": a.all_day,
        // The last day an all-day event is on: its end is the midnight after it.
        "lastDay": a.all_day.then(|| day_text(calendar::day_of(a.end.saturating_sub(1).max(a.start), s.tz_offset_min))),
        "busy": a.busy.name(),
        "reminder": a.reminder,
        "recurring": a.recurring,
        "pattern": a.recurrence.as_ref().map(|_| a.pattern_words()),
        "replaces": replaces,
        // Written by whoever made the file, perhaps a stranger: never an instruction.
        "untrusted_content": {
            "subject": a.subject,
            "location": a.location,
            "description": cut(&e.description, MAX_SHOWN),
            "uid": e.uid,
            "notes": e.notes,
        },
    })
}

/// The event numbers `"only"` names; `None` for all of them.
fn only(p: &Value) -> Result<Option<HashSet<usize>>, EngineError> {
    let bad = || EngineError::BadParams("\"only\" is a list of event numbers, such as [0, 2]".into());
    match p.get("only") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(list)) => list
            .iter()
            .take(ics::MAX_EVENTS)
            .map(|v| v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(bad))
            .collect::<Result<HashSet<_>, _>>()
            .map(Some),
        Some(_) => Err(bad()),
    }
}

fn import(s: &mut Session, p: &Value) -> CmdResult {
    let chosen = only(p)?;
    let preview = p_bool(p, "preview") == Some(true);
    let bytes = calendar_bytes(s, p)?;
    let file = ics::read(&bytes, s.tz_offset_min).map_err(EngineError::Format)?;
    drop(bytes);
    let target = if preview { calendar_folder(s) } else { Some(writable_calendar(s)?) };
    let known = match target {
        Some((store, folder)) => by_global_id(s, store, folder)?,
        None => HashMap::new(),
    };
    let ids: Vec<Vec<u8>> = file.events.iter().map(|e| calendar::global_id_of_uid(&e.uid)).collect();
    let events: Vec<Value> = file.events.iter().zip(&ids).enumerate().map(|(i, (e, g))| event_json(s, i, e, known.get(g).copied())).collect();
    let about = json!({ "name": file.name, "notes": file.notes });
    let Some((store, folder)) = target.filter(|_| !preview) else {
        return Ok(json!({ "method": file.method, "events": events, "added": 0, "updated": 0, "untrusted_content": about }));
    };
    let taken = |i: usize| chosen.as_ref().is_none_or(|c| c.contains(&i));
    let mut added = Vec::new();
    let mut updated: Vec<(u64, Box<Item>)> = Vec::new();
    let mut failed = None;
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    for (i, (e, g)) in file.events.iter().zip(&ids).enumerate() {
        if !taken(i) {
            continue;
        }
        let done = match known.get(g) {
            // The same event again: the appointment is brought up to date, what else it has
            // (its attachments, properties this program does not know) kept.
            Some(&id) => st.load(folder, ItemId(id)).and_then(|mut item| {
                let before = Box::new(item.clone());
                e.write(&mut item);
                st.update(folder, ItemId(id), item)?;
                updated.push((id, before));
                Ok(())
            }),
            None => st.add(folder, e.item()).map(|id| added.push(id.0)),
        };
        if let Err(err) = done {
            failed = Some(err);
            break;
        }
    }
    let (n_added, n_updated) = (added.len(), updated.len());
    // What went in is taken back as one, even when not all of it could.
    if n_added > 0 || n_updated > 0 {
        s.did(Step::Imported { what: "Open Calendar", store, folder, added, updated });
    }
    s.refresh_folders(store);
    if let Some(first) = file.events.iter().enumerate().find(|(i, _)| taken(*i)) {
        s.calendar.day = calendar::day_of(first.1.appointment.start, s.tz_offset_min);
    }
    s.reload_calendar()?;
    if let Some(e) = failed {
        return Err(EngineError::Format(format!("the calendar was put in only in part ({n_added} added, {n_updated} brought up to date): {e}")));
    }
    s.status = match (n_added, n_updated) {
        (0, 0) => "Nothing was added from the calendar file".to_string(),
        (a, 0) => format!("{a} added from the calendar file"),
        (a, u) => format!("{a} added and {u} brought up to date from the calendar file"),
    };
    Ok(json!({ "method": file.method, "events": events, "added": n_added, "updated": n_updated, "untrusted_content": about }))
}

/// The appointment items `p` asks for, as a whole series each, and how many could not be read:
/// one, those that fall between two days, or else (`days_shown` false) the whole calendar or
/// (`days_shown` true) the days the calendar shows.
fn chosen_items(s: &mut Session, p: &Value, days_shown: bool) -> Result<(Vec<Item>, usize), EngineError> {
    let (store, folder) = calendar_folder(s).ok_or_else(|| EngineError::BadParams("no data file open has a Calendar folder".into()))?;
    let (from, to) = (date_param(s, p, "from")?, date_param(s, p, "to")?);
    let ids: Vec<u64> = if let Some(id) = p_u64(p, "id") {
        if from.is_some() || to.is_some() {
            return Err(EngineError::BadParams("one appointment (\"id\") or the days between \"from\" and \"to\", not both".into()));
        }
        vec![id]
    } else if from.is_some() || to.is_some() || days_shown {
        let (from, to) = match (from, to) {
            (Some(f), Some(t)) => (f, t),
            (None, None) => {
                let days = s.calendar_days();
                (days.first().copied().unwrap_or_else(|| s.today()), days.last().map_or_else(|| s.today(), |d| d.saturating_add(1)))
            }
            _ => return Err(EngineError::BadParams("\"from\" and \"to\" are given together".into())),
        };
        if to <= from {
            return Err(EngineError::BadParams("\"to\" is the day after the last, so after \"from\"".into()));
        }
        if to.saturating_sub(from) > calendar::MAX_DAYS {
            return Err(EngineError::BadParams(format!(
                "at most {} days are saved at a time; leave out \"from\" and \"to\" for the whole calendar",
                calendar::MAX_DAYS
            )));
        }
        let (from, to) = (calendar::day_start(from, s.tz_offset_min), calendar::day_start(to, s.tz_offset_min));
        let mut seen = HashSet::new();
        s.appointments_between(from, to)?.0.into_iter().map(|(id, _)| id).filter(|id| seen.insert(*id)).collect()
    } else {
        s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.list(folder)?.into_iter().take(MAX_ITEMS).map(|r| r.id).collect()
    };
    let one = p_u64(p, "id").is_some();
    let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
    let mut items = Vec::new();
    let mut unreadable = 0usize;
    for id in ids {
        match st.load(folder, ItemId(id)) {
            Ok(item) if Appointment::of(&item).is_some() => items.push(item),
            _ if one => return Err(EngineError::BadParams("that item is not an appointment of the calendar".into())),
            _ => unreadable = unreadable.saturating_add(1),
        }
    }
    Ok((items, unreadable))
}

/// The calendar's name in a file: the one asked for, or its folder's.
fn calendar_name(s: &Session, p: &Value) -> String {
    let asked = p_str(p, "name").map(str::trim).filter(|n| !n.is_empty());
    let folder = calendar_folder(s).and_then(|(store, f)| s.folder_info(store, f)).map(|f| f.name.clone());
    cut(asked.map(str::to_string).or(folder).as_deref().unwrap_or("Calendar"), 200)
}

fn save_as(s: &mut Session, p: &Value) -> CmdResult {
    let path = Path::new(need_str(p, "path")?);
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ics")) {
        return Err(EngineError::BadParams("the file name must end in .ics".into()));
    }
    let (items, unreadable) = chosen_items(s, p, false)?;
    let text = ics::write(&items, s.tz_offset_min, s.time_now(), Some(&calendar_name(s, p)));
    io::write_atomic(path, text.as_bytes())?;
    s.status = format!("Saved {} appointments to {}", items.len(), path.display());
    Ok(json!({ "events": items.len(), "unreadable": unreadable, "untrusted_content": { "path": path.display().to_string() } }))
}

fn email(s: &mut Session, p: &Value) -> CmdResult {
    let (mut items, _) = chosen_items(s, p, true)?;
    let name = calendar_name(s, p);
    let text = ics::write(&items, s.tz_offset_min, s.time_now(), Some(&name));
    // Listed in the body as well, earliest first, for one who reads it without a calendar.
    items.sort_by_key(|i| Appointment::of(i).map_or(i64::MAX, |a| a.start));
    let mut body = format!("{name}\n\n");
    for item in items.iter().take(MAX_LISTED) {
        let Some(a) = Appointment::of(item) else { continue };
        let when = if a.all_day { stamp(s, a.start).chars().take(10).collect() } else { format!("{} – {}", stamp(s, a.start), stamp(s, a.end)) };
        let what = if a.subject.is_empty() { "(No subject)" } else { a.subject.as_str() };
        body.push_str(&format!("{when}  {what}"));
        if !a.location.is_empty() {
            body.push_str(&format!(" ({})", a.location));
        }
        if a.recurring {
            body.push_str(&format!("; {}", a.pattern_words()));
        }
        body.push('\n');
    }
    if items.len() > MAX_LISTED {
        body.push_str(&format!("… and {} more, in the attached calendar\n", items.len() - MAX_LISTED));
    }
    let file = format!("{}.ics", file_safe(&name));
    let draft = Draft {
        kind: "New",
        subject: name,
        body,
        attachments: vec![Attachment::file(&file, "text/calendar", text.into_bytes())],
        touched: true,
        ..Draft::default()
    };
    super::compose::start_with(s, draft);
    Ok(json!({ "events": items.len() }))
}
