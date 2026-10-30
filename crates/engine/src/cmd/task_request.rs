//! Assign Task: a task given to someone by mail, their answer and how far along they are with
//! it ([MS-OXOTASK] 3.1.4.4 and 3.1.4.5). Each goes as a message of the class Outlook gives it,
//! carrying the task as its first attachment, hidden, and in a `winmail.dat` beside a body
//! anyone can read, so that Outlook takes it as a task request.
//!
//! A request that comes stays a message until it is answered, as an invitation does. An
//! answer or an update that comes is taken in at once, but only by the task it names (its
//! PidLidTaskGlobalId), only into the copy of the one who gave it, and only from someone that
//! task was given to; anything else is left as a message to read.

use super::meeting::{attendees_param, mine};
use super::tasks::{edit_row, load_row, row_key, writable_folder};
use super::{CommandSpec, clock, ok, p_bool, p_str};
use crate::{CmdResult, EngineError, Session, undo::Step};
use openmailclient_model::task::request::{self, Acceptance, Assigner, Assignment, Kind, Ownership};
use openmailclient_model::task::{self, Status, Task};
use openmailclient_model::{FolderId, FolderKind, Item, ItemId, MSGFLAG_READ, PropValue, RECIP_TO, StoreError, pid};
use serde_json::{Value, json};

/// Tasks of a folder looked through for the one an answer names, at most.
const MAX_ITEMS: usize = 20_000;
/// Longest comment sent with an answer, in characters.
const MAX_COMMENT: usize = 4_000;
/// People a task is given to at once, at most.
const MAX_ASSIGNEES: usize = 100;

const ASSIGN_PARAMS: &str = r#"the row (as tasks.markComplete takes it), "to": addresses as one line ("Sam Lee <sam@example.com>; priya@example.net") or a list, "keepCopy": false to keep no copy of it once sent (default true), "updates": false not to be sent each change they make (default true), "statusReport": false not to be sent a report once it is done (default true), "comment": text sent with it, "wait": false to send on a thread of its own; the task is given to them by a task request and waits for their answer; given to several it is theirs to share and none of them answers"#;

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "tasks.assign",
            label: "Assign Task",
            tab: "Task",
            group: "Manage Task",
            shortcut: "",
            params: ASSIGN_PARAMS,
            enabled: has_task,
            run: assign,
        },
        CommandSpec {
            id: "tasks.respond",
            label: "Accept, Decline",
            tab: "Task",
            group: "Respond",
            shortcut: "",
            params: r#"{"response": "accept" | "decline", "send": false to answer without telling the one who gave it (default true), "comment": text sent with the answer, "wait"}; on the task request open: an accepted task is put in the Tasks folder, one's own to work on; the request answered goes to Deleted Items"#,
            enabled: has_request,
            run: respond,
        },
        CommandSpec {
            id: "tasks.request",
            label: "Task Request",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{}; what the open task request, acceptance, rejection or update says: {"kind": "request" | "accept" | "decline" | "update", "from", "assigners": [who gave it on, the most recent last], "owner", "status", "percent", "start", "due", "completed", "inTasks": the task's id when it is in a Tasks folder, "untrusted_content": {"subject", "notes", "comment"}}"#,
            enabled: has_communication,
            run: describe,
        },
        CommandSpec {
            id: "tasks.sendStatusReport",
            label: "Send Status Report",
            tab: "Task",
            group: "Manage Task",
            shortcut: "",
            params: r#"the row (as tasks.markComplete takes it), "to": addresses (default: whoever gave it, or who it was given to), "comment": text sent with it, "wait"; a message saying how far along the task is"#,
            enabled: has_task,
            run: send_status_report,
        },
    ]
}

fn has_task(s: &Session) -> bool {
    s.tasks.selected.is_some() || super::tasks::tasks_folder(s).is_some()
}

fn has_request(s: &Session) -> bool {
    s.open.as_ref().is_some_and(|m| Kind::of(m) == Some(Kind::Request) && request::carried(m).is_some())
}

fn has_communication(s: &Session) -> bool {
    s.open.as_ref().is_some_and(|m| Kind::of(m).is_some())
}

/// Oneself as one who gives a task on: the first account, or the session's name.
fn me(s: &Session) -> Assigner {
    match super::send::account_for(s, "") {
        Some(a) => Assigner { name: if a.display_name.is_empty() { a.email.clone() } else { a.display_name.clone() }, address: a.email.clone() },
        None => Assigner { name: if s.user_name.is_empty() { s.user_email.clone() } else { s.user_name.clone() }, address: s.user_email.clone() },
    }
}

fn is_mine(s: &Session, address: &str) -> bool {
    !address.trim().is_empty() && mine(s).iter().any(|m| m.eq_ignore_ascii_case(address.trim()))
}

fn comment(p: &Value) -> String {
    p_str(p, "comment").map(|c| c.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').take(MAX_COMMENT).collect()).unwrap_or_default()
}

/// A new id for both copies of a task: random enough that two never meet.
fn new_global_id(s: &Session, seed: u64) -> [u8; 16] {
    let mut x = (clock(s) as u64) ^ seed.rotate_left(29) ^ u64::from(std::process::id()).rotate_left(47);
    let mut next = || {
        // SplitMix64.
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&next().to_le_bytes());
    out[8..].copy_from_slice(&next().to_le_bytes());
    out
}

/// What a task has come to, in words, for a message's body one reads without a task list.
fn summary(t: &Task, comment: &str) -> String {
    let day = |d: Option<i64>| d.map_or_else(|| "None".to_string(), super::calendar::day_text);
    let mut out = String::new();
    if !comment.is_empty() {
        out.push_str(comment);
        out.push_str("\n\n");
    }
    out.push_str(&format!("Subject: {}\n", t.subject));
    out.push_str(&format!("Start date: {}\nDue date: {}\n", day(t.start), day(t.due)));
    out.push_str(&format!("Status: {}\n% Complete: {}%\n", t.status.label(), t.percent));
    if t.total_work > 0 || t.actual_work > 0 {
        out.push_str(&format!("Total work: {} hours\nActual work: {} hours\n", t.total_work / 60, t.actual_work / 60));
    }
    if let Some(d) = t.completed {
        out.push_str(&format!("Date completed: {}\n", super::calendar::day_text(d)));
    }
    if !t.notes.is_empty() {
        out.push('\n');
        out.push_str(&t.notes);
    }
    out
}

/// A task's message put in the Outbox from oneself to `to`, and sent when there is an account
/// to send it from.
fn post(s: &mut Session, mut item: Item, to: &[(String, String)], wait: bool) -> CmdResult {
    let store = s.writable_store().ok_or_else(|| EngineError::Format("there is no writable data file to keep the message in".into()))?;
    let outbox = s.find_folder(store, FolderKind::Outbox).ok_or_else(|| EngineError::Format("this data file has no Outbox".into()))?;
    let from = me(s);
    item.set_sender(&from.name, &from.address);
    item.recipients.clear();
    for (name, address) in to {
        item.add_recipient(RECIP_TO, name, address);
    }
    item.refresh_display_recipients();
    let now = clock(s);
    item.props.set(pid::CREATION_TIME, PropValue::Time(now));
    item.props.set(pid::CLIENT_SUBMIT_TIME, PropValue::Time(now));
    item.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(MSGFLAG_READ));
    s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.add(outbox, item)?;
    s.refresh_folders(store);
    if s.accounts.is_empty() || s.connector.is_none() {
        s.status = "The task's message is in the Outbox; it goes when there is an account to send it from".to_string();
        return ok();
    }
    super::send::send_outbox(s, wait).or_else(|e| {
        s.status = format!("The task's message is in the Outbox and was not sent: {e}");
        ok()
    })
}

fn assign(s: &mut Session, p: &Value) -> CmdResult {
    let key = row_key(s, p)?;
    let item = load_row(s, key)?;
    if !task::is_task(&item) {
        return Err(EngineError::BadParams("a message flagged for follow-up is not given to anyone; make a task of it first".into()));
    }
    let a = Assignment::of(&item);
    if a.ownership == Ownership::Assignee && a.acceptance != Acceptance::Accepted {
        return Err(EngineError::BadParams("answer the task request first; a task accepted may be given on".into()));
    }
    if a.ownership == Ownership::Assigner && a.acceptance != Acceptance::Declined {
        return Err(EngineError::BadParams("that task is given to someone already".into()));
    }
    let to: Vec<(String, String)> = attendees_param(s, p.get("to").unwrap_or(&Value::Null))?
        .into_iter()
        .filter(|q| !is_mine(s, &q.address))
        .take(MAX_ASSIGNEES)
        .map(|q| (q.name, q.address))
        .collect();
    if to.is_empty() {
        return Err(EngineError::BadParams("\"to\" names no one but oneself to give it to".into()));
    }
    let updates = p_bool(p, "updates") != Some(false);
    let report = p_bool(p, "statusReport") != Some(false);
    let keep = p_bool(p, "keepCopy") != Some(false);
    let words = comment(p);
    let from = me(s);
    let global = new_global_id(s, key.2);
    let now = clock(s);
    let mut carried = None;
    edit_row(s, key, "Assign Task", |_, item| {
        carried = Some(request::assign(item, &to, &from, global, updates, report, now));
        Ok(())
    })?;
    let carried = carried.ok_or(StoreError::NoItem)?;
    let t = Task::of(&carried, &s.zone);
    let msg = request::message(Kind::Request, &carried, &summary(&t, &words));
    post(s, msg, &to, p_bool(p, "wait") != Some(false))?;
    if !keep {
        let (store, folder, id) = key;
        s.run("tasks.delete", &json!({ "id": id, "store": store, "folder": folder.0 }))?;
    }
    if !s.status.contains("Outbox") {
        s.status = format!(
            "Task request sent to {}",
            to.iter().map(|(n, e)| if n.is_empty() { e.as_str() } else { n.as_str() }).collect::<Vec<_>>().join("; ")
        );
    }
    Ok(json!({ "sent": to.len(), "kept": keep, "shared": to.len() > 1 }))
}

/// The task of a Tasks folder of any data file that can be written whose
/// PidLidTaskGlobalId is `want`, with whose copy it must be.
fn find(s: &mut Session, want: [u8; 16], whose: Ownership) -> Option<(usize, FolderId, u64, Item)> {
    let places: Vec<(usize, FolderId)> = s
        .folders
        .iter()
        .enumerate()
        .flat_map(|(store, list)| list.iter().filter(|f| f.kind == FolderKind::Tasks).map(move |f| (store, f.id)))
        .filter(|(store, _)| s.stores.get(*store).is_some_and(|st| st.writable()))
        .collect();
    for (store, folder) in places {
        let Some(st) = s.stores.get_mut(store) else { continue };
        let Ok(rows) = st.list(folder) else { continue };
        for row in rows.into_iter().take(MAX_ITEMS) {
            if let Ok(item) = st.load(folder, ItemId(row.id))
                && request::global_id(&item) == Some(want)
                && Assignment::of(&item).ownership == whose
            {
                return Some((store, folder, row.id, item));
            }
        }
    }
    None
}

fn respond(s: &mut Session, p: &Value) -> CmdResult {
    let accept = match p_str(p, "response").map(str::to_ascii_lowercase).as_deref() {
        Some("accept" | "accepted") => true,
        Some("decline" | "declined") => false,
        _ => return Err(EngineError::BadParams("\"response\" is accept or decline".into())),
    };
    let msg =
        s.open.clone().filter(|m| Kind::of(m) == Some(Kind::Request)).ok_or_else(|| EngineError::BadParams("no task request is open".into()))?;
    let carried = request::carried(&msg).ok_or_else(|| EngineError::Format("the task request carries no task that can be read".into()))?;
    let from = Assigner { name: msg.sender_name(), address: msg.sender_email() };
    if is_mine(s, &from.address) {
        return Err(EngineError::BadParams("that request is one's own".into()));
    }
    let me = me(s);
    let now = clock(s);
    let mut local = request::take_request(carried, &from, &me.name);
    let shared = Assignment::of(carried).owner.contains(';');
    let sent = request::answer(&mut local, accept, &me.name, now);
    let mut kept = None;
    if accept {
        let (store, folder) = writable_folder(s)?;
        let existing = request::global_id(&local).and_then(|g| find(s, g, Ownership::Assignee)).filter(|(st, f, _, _)| (*st, *f) == (store, folder));
        let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
        kept = Some(match existing {
            Some((_, _, id, before)) => {
                st.update(folder, ItemId(id), local)?;
                s.did(Step::Edited { what: "Accept Task", store, folder, id, before: Box::new(before) });
                id
            }
            None => {
                let id = st.add(folder, local)?.0;
                s.did(Step::Imported { what: "Accept Task", store, folder, added: vec![id], updated: Vec::new() });
                id
            }
        });
        s.refresh_folders(store);
        s.reload_tasks()?;
    }
    // Given to several to share, none of them answers ([MS-OXOTASK] 3.1.4.4.1).
    let send = p_bool(p, "send") != Some(false) && !shared && from.address.contains('@');
    if send {
        let kind = if accept { Kind::Accept } else { Kind::Decline };
        let words = comment(p);
        let body = if words.is_empty() { format!("{} {} the task.", me.name, if accept { "accepted" } else { "declined" }) } else { words };
        let reply = request::message(kind, &sent, &body);
        post(s, reply, &[(from.name.clone(), from.address.clone())], p_bool(p, "wait") != Some(false))?;
    }
    if s.is_enabled("mail.delete") {
        s.run("mail.delete", &json!({}))?;
    }
    s.status = format!("Task {}{}", if accept { "accepted" } else { "declined" }, if send { "; the one who gave it is told" } else { "" });
    Ok(json!({ "id": kept, "sent": send }))
}

fn describe(s: &mut Session, _: &Value) -> CmdResult {
    let msg = s.open.clone().ok_or_else(|| EngineError::BadParams("no message is open".into()))?;
    let kind = Kind::of(&msg).ok_or_else(|| EngineError::BadParams("the open message is no task request".into()))?;
    let Some(carried) = request::carried(&msg) else {
        return Ok(json!({ "kind": kind_name(kind), "from": msg.sender_email(), "readable": false }));
    };
    let a = Assignment::of(carried);
    let t = Task::of(carried, &s.zone);
    let whose = if kind == Kind::Request { Ownership::Assignee } else { Ownership::Assigner };
    let in_tasks = request::global_id(carried).and_then(|g| find(s, g, whose)).map(|(_, _, id, _)| id);
    let day = |d: Option<i64>| d.map(super::calendar::day_text);
    Ok(json!({
        "kind": kind_name(kind),
        "from": msg.sender_email(),
        "assigners": a.assigners.iter().map(|x| json!({ "name": x.name, "address": x.address })).collect::<Vec<_>>(),
        "owner": a.owner,
        "status": t.status.name(),
        "percent": t.percent,
        "start": day(t.start),
        "due": day(t.due),
        "completed": day(t.completed),
        "inTasks": in_tasks,
        // Whoever sent it wrote these.
        "untrusted_content": { "subject": t.subject, "notes": t.notes, "comment": msg.body_text() },
    }))
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Request => "request",
        Kind::Accept => "accept",
        Kind::Decline => "decline",
        Kind::Update => "update",
    }
}

fn send_status_report(s: &mut Session, p: &Value) -> CmdResult {
    let key = row_key(s, p)?;
    let item = load_row(s, key)?;
    let a = Assignment::of(&item);
    let to: Vec<(String, String)> = match p.get("to").filter(|v| !v.is_null()) {
        Some(v) => attendees_param(s, v)?.into_iter().map(|q| (q.name, q.address)).collect(),
        None if a.ownership == Ownership::Assigner => item
            .recipients
            .iter()
            .filter(|r| r.i32(pid::RECIPIENT_TYPE).unwrap_or(RECIP_TO) == RECIP_TO)
            .map(|r| (r.str(pid::DISPLAY_NAME).unwrap_or_default().to_string(), r.str(pid::EMAIL_ADDRESS).unwrap_or_default().to_string()))
            .collect(),
        None => a.assigners.iter().map(|x| (x.name.clone(), x.address.clone())).collect(),
    };
    let to: Vec<(String, String)> = to.into_iter().filter(|(_, e)| e.contains('@') && !is_mine(s, e)).take(MAX_ASSIGNEES).collect();
    if to.is_empty() {
        return Err(EngineError::BadParams("no one to send it to: give \"to\"".into()));
    }
    let t = Task::of(&item, &s.zone);
    let mut msg = Item::new_mail();
    msg.set_subject(&format!("Task Status Report: {}", t.subject));
    msg.set_body_text(&summary(&t, &comment(p)));
    post(s, msg, &to, p_bool(p, "wait") != Some(false))?;
    if !s.status.contains("Outbox") {
        s.status = "Status report sent".into();
    }
    Ok(json!({ "sent": to.len() }))
}

/// After a change to a task given to oneself, the one who gave it is sent what they asked for:
/// each change when they asked for updates, and a report once it is done when they asked for
/// that ([MS-OXOTASK] 3.1.4.4, task update). Nothing is sent for a task of one's own.
pub(crate) fn changed(s: &mut Session, key: (usize, FolderId, u64), was: &Task) -> Result<(), EngineError> {
    let (store, folder, id) = key;
    let Ok(mut item) = load_row(s, key) else { return Ok(()) };
    let a = Assignment::of(&item);
    if a.ownership != Ownership::Assignee || a.acceptance != Acceptance::Accepted {
        return Ok(());
    }
    let Some(to) = a.last_assigner().filter(|x| x.address.contains('@')).cloned() else { return Ok(()) };
    let now_t = Task::of(&item, &s.zone);
    let done = now_t.status == Status::Complete && was.status != Status::Complete;
    if !(a.updates || done && a.status_on_complete) {
        return Ok(());
    }
    let me = me(s);
    let carried = request::changed(&mut item, &me.name, now_t.due != was.due, clock(s));
    // The version moved on is the copy's own, not a change to take back.
    s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.update(folder, ItemId(id), item)?;
    let mut msg = request::message(Kind::Update, &carried, &summary(&now_t, ""));
    if done {
        msg.set_subject(&format!("Task Completed: {}", now_t.subject));
    }
    let status = s.status.clone();
    post(s, msg, &[(to.name.clone(), to.address.clone())], true)?;
    if !s.status.contains("Outbox") {
        s.status = format!("{status}; {} is sent an update", to.name);
    }
    Ok(())
}

/// An acceptance, rejection or update of a task one gave, taken in: by the task it names, and
/// only from someone it was given to. Returns what was taken in, in words.
pub(crate) fn arrived(s: &mut Session, msg: &Item) -> Option<String> {
    let kind = Kind::of(msg).filter(|k| *k != Kind::Request)?;
    let carried = request::carried(msg)?;
    let want = request::global_id(carried)?;
    let from = msg.sender_email();
    let (store, folder, id, mut local) = find(s, want, Ownership::Assigner)?;
    // Only one it was given to answers for it.
    let given_to = local.recipients.iter().any(|r| {
        r.i32(pid::RECIPIENT_TYPE).unwrap_or(RECIP_TO) == RECIP_TO
            && r.str(pid::EMAIL_ADDRESS).is_some_and(|e| !from.is_empty() && e.trim().eq_ignore_ascii_case(from.trim()))
    });
    if !given_to {
        return None;
    }
    let subject = Task::of(&local, &s.zone).subject;
    if !request::take_answer(&mut local, kind, carried, clock(s)) {
        return None;
    }
    s.stores.get_mut(store)?.update(folder, ItemId(id), local).ok()?;
    let _ = s.reload_tasks();
    let who = Some(msg.sender_name()).filter(|n| !n.is_empty()).unwrap_or(from);
    Some(match kind {
        Kind::Accept => format!("{who} accepted {subject}"),
        Kind::Decline => format!("{who} declined {subject}"),
        _ => format!("{subject} was brought up to date by {who}"),
    })
}
