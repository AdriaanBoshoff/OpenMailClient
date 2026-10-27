//! Reminders: those due listed, dismissed, snoozed and opened, and how they are shown. See
//! `crate::reminders`.

use super::{CommandSpec, always, ok, p_u64};
use crate::reminders::{Key, MAX_DUE};
use crate::{CmdResult, EngineError, Module, Session};
use openmailclient_model::FolderId;
use openmailclient_model::calendar::{self, MINUTE};
use openmailclient_model::reminder::{Due, Kind, MAX_SNOOZE_MIN};
use serde_json::{Value, json};

/// Snoozed this long when no time is given, in minutes, as Outlook does.
const SNOOZE_MIN: i64 = 5;

fn has_due(s: &Session) -> bool {
    !s.reminders.due.is_empty()
}

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "view.reminders",
            label: "Reminders Window",
            tab: "View",
            group: "Window",
            shortcut: "",
            params: r#"{"open": true or false (default true)}; the Reminders window, with every reminder due"#,
            enabled: always,
            run: window,
        },
        CommandSpec {
            id: "reminders.list",
            label: "Reminders",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{}; looks through the data files open and returns {"due": [{"store", "folder", "id", "kind": "appointment" | "task" | "message", "due": {"at": Unix seconds} or {"day": days since 1970-01-01} or null, "dueIn": "In 15 minutes", "3 days overdue", "signal": Unix seconds it came due, "occurrence": the day an occurrence of a series was first planned on, "untrusted_content": {"subject", "location"}}], the earliest first, "next": Unix seconds the next comes due, or null}"#,
            enabled: always,
            run: list,
        },
        CommandSpec {
            id: "reminders.dismiss",
            label: "Dismiss",
            tab: "",
            group: "",
            shortcut: "",
            params: concat!(
                r#"{"#,
                r#""store", "folder" and "id" of a reminder reminders.list gives (default: the first)"#,
                r#"}; it is put away: for a series, until its next occurrence. Written into the item as Outlook writes it; in a data file that cannot be written, put away for this run. Returns {"left"}"#
            ),
            enabled: has_due,
            run: dismiss,
        },
        CommandSpec {
            id: "reminders.dismissAll",
            label: "Dismiss All",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{}; every reminder due is dismissed. Returns {"dismissed"}"#,
            enabled: has_due,
            run: dismiss_all,
        },
        CommandSpec {
            id: "reminders.snooze",
            label: "Snooze",
            tab: "",
            group: "",
            shortcut: "",
            params: concat!(
                r#"{"#,
                r#""store", "folder" and "id" of a reminder reminders.list gives (default: the first)"#,
                r#", "minutes": 1 to 10080 (default 5), or "beforeStart": minutes before an appointment starts (0 at its start), "all": true for every one due}; it comes up again then. Returns {"until": Unix seconds}"#
            ),
            enabled: has_due,
            run: snooze,
        },
        CommandSpec {
            id: "reminders.open",
            label: "Open Item",
            tab: "",
            group: "",
            shortcut: "",
            params: concat!(
                r#"{"#,
                r#""store", "folder" and "id" of a reminder reminders.list gives (default: the first)"#,
                r#"}; goes to the item and chooses it: an appointment in the Calendar on its day, a task in Tasks, a message in its folder; the reminder stays. Returns {"kind", "store", "folder", "id", "occurrence"}"#
            ),
            enabled: has_due,
            run: open,
        },
        CommandSpec {
            id: "reminders.options",
            label: "Reminder Options",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"show": the Reminders window comes up when one is due, "sound": a sound is played then}, each true or false, those not given as they are. Kept with the settings; returns {"show", "sound"}"#,
            enabled: always,
            run: options,
        },
    ]
}

fn window(s: &mut Session, p: &Value) -> CmdResult {
    s.reminders.window = match p.get("open") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(EngineError::BadParams("\"open\" is true or false".into())),
    };
    if s.reminders.window {
        s.look_for_reminders(0);
    }
    ok()
}

pub(crate) fn due_json(s: &Session) -> Value {
    let (now, today) = (s.time_now(), s.today());
    json!({
        "due": s.reminders.due.iter().take(MAX_DUE).map(|p| p.to_json(now, today)).collect::<Vec<_>>(),
        "next": s.reminders.next.map(openmailclient_model::time::to_unix),
    })
}

fn list(s: &mut Session, _: &Value) -> CmdResult {
    s.look_for_reminders(0);
    Ok(due_json(s))
}

/// The reminder a command is about: the one named, else the first due.
fn which(s: &Session, p: &Value) -> Result<Key, EngineError> {
    if p.get("id").is_none_or(Value::is_null) {
        return s.reminders.due.first().map(|p| p.key()).ok_or_else(|| EngineError::BadParams("no reminder is due".into()));
    }
    let number = |key: &str| p_u64(p, key).ok_or_else(|| EngineError::BadParams(format!("\"{key}\" must be a number, as reminders.list gives it")));
    let store = usize::try_from(number("store")?).map_err(|_| EngineError::BadParams("no such data file".into()))?;
    let key = (store, FolderId(number("folder")?), number("id")?);
    s.pending(key).map(|p| p.key()).ok_or_else(|| EngineError::BadParams("that reminder is not due; reminders.list gives those that are".into()))
}

fn dismiss(s: &mut Session, p: &Value) -> CmdResult {
    let key = which(s, p)?;
    s.put_off(key, None)?;
    s.status = "Reminder dismissed".into();
    Ok(json!({ "left": s.reminders.due.len() }))
}

fn dismiss_all(s: &mut Session, _: &Value) -> CmdResult {
    let keys: Vec<Key> = s.reminders.due.iter().map(|p| p.key()).collect();
    let mut dismissed = 0usize;
    let mut failed = None;
    for key in keys {
        match s.put_off(key, None) {
            Ok(()) => dismissed = dismissed.saturating_add(1),
            Err(e) => failed = Some(e),
        }
    }
    if let (Some(e), 0) = (&failed, dismissed) {
        return Err(EngineError::BadParams(e.to_string()));
    }
    s.status = match dismissed {
        1 => "1 reminder dismissed".into(),
        n => format!("{n} reminders dismissed"),
    };
    Ok(json!({ "dismissed": dismissed }))
}

/// When a snooze ends: so many minutes from now, or so many before the appointment starts
/// (never before now, as `put_off` holds it).
fn until(s: &Session, p: &Value, due: Due) -> Result<i64, EngineError> {
    let now = s.time_now();
    if let Some(v) = p.get("beforeStart").filter(|v| !v.is_null()) {
        let minutes =
            v.as_u64().filter(|m| *m <= 60 * 24 * 7).ok_or_else(|| EngineError::BadParams("\"beforeStart\" is minutes, 0 to 10080".into()))?;
        let Due::At(start) = due else {
            return Err(EngineError::BadParams("only an appointment has a start to snooze until".into()));
        };
        let at = start.saturating_sub(i64::try_from(minutes).unwrap_or(0).saturating_mul(MINUTE));
        if at <= now {
            return Err(EngineError::BadParams("that time has passed; snooze for minutes instead".into()));
        }
        return Ok(at);
    }
    let minutes = match p.get("minutes") {
        None | Some(Value::Null) => SNOOZE_MIN,
        Some(v) => v
            .as_i64()
            .filter(|m| (1..=MAX_SNOOZE_MIN).contains(m))
            .ok_or_else(|| EngineError::BadParams(format!("\"minutes\" is a whole number from 1 to {MAX_SNOOZE_MIN}")))?,
    };
    Ok(now.saturating_add(minutes.saturating_mul(MINUTE)))
}

fn snooze(s: &mut Session, p: &Value) -> CmdResult {
    let keys: Vec<Key> =
        if p.get("all").and_then(Value::as_bool) == Some(true) { s.reminders.due.iter().map(|p| p.key()).collect() } else { vec![which(s, p)?] };
    let mut last = None;
    for key in keys {
        let Some(due) = s.pending(key).map(|p| p.reminder.due) else { continue };
        let at = until(s, p, due)?;
        s.put_off(key, Some(at))?;
        last = Some(at);
    }
    let at = last.ok_or_else(|| EngineError::BadParams("no reminder is due".into()))?;
    let minutes = at.saturating_sub(s.time_now()) / MINUTE;
    s.status = match minutes {
        0 | 1 => "Snoozed for a minute".into(),
        m => format!("Snoozed for {m} minutes"),
    };
    Ok(json!({ "until": openmailclient_model::time::to_unix(at) }))
}

fn open(s: &mut Session, p: &Value) -> CmdResult {
    let key = which(s, p)?;
    let pending = s.pending(key).cloned().ok_or_else(|| EngineError::BadParams("no reminder is due".into()))?;
    let (store, folder, id) = key;
    let r = &pending.reminder;
    match r.kind {
        Kind::Appointment => {
            s.module = Module::Calendar;
            s.calendar.chosen = Some((store, folder));
            if let Due::At(start) = r.due {
                s.calendar.day = calendar::day_of(start, &s.zone);
            }
            s.reload_calendar()?;
            s.calendar.selected = Some(id);
            s.calendar.occurrence = r.occurrence;
        }
        Kind::Task => {
            s.module = Module::Tasks;
            s.tasks.chosen = Some((store, folder));
            s.tasks.query.clear();
            s.reload_tasks()?;
            if !s.tasks.rows.iter().any(|row| row.key() == key) {
                s.tasks.view = super::tasks::View::SimpleList;
                s.reload_tasks()?;
            }
            s.tasks.selected = Some(key);
        }
        Kind::Message => {
            s.module = Module::Mail;
            s.select_folder(store, folder)?;
            if !s.rows.iter().any(|row| row.id == id) {
                return Err(EngineError::BadParams("that message is not shown in its folder; a search or filter hides it".into()));
            }
            s.select_item(id)?;
        }
    }
    Ok(json!({ "kind": r.kind.name(), "store": store, "folder": folder.0, "id": id, "occurrence": r.occurrence }))
}

fn options(s: &mut Session, p: &Value) -> CmdResult {
    let part = |key: &str, now: bool| match p.get(key) {
        None | Some(Value::Null) => Ok(now),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(EngineError::BadParams(format!("\"{key}\" is true or false"))),
    };
    let (show, sound) = (part("show", s.reminders.show)?, part("sound", s.reminders.sound)?);
    s.reminders.show = show;
    s.reminders.sound = sound;
    Ok(s.reminders.options_json())
}
