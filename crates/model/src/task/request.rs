//! A task given to someone else, and what comes back: task requests, acceptances, rejections and
//! updates ([MS-OXOTASK] 2.2.3, 2.2.4, 3.1.4.3 to 3.1.4.5).
//!
//! The assigner's copy of the task says who it was given to and how they answered; the
//! assignee's copy says who gave it, the most recent last, in PidLidTaskAssigners. Each message
//! carries the task as its first attachment, hidden ([MS-OXOTASK] 3.1.4.3). Both copies know each
//! other by PidLidTaskGlobalId, and an update older than the copy it is for is not taken.

use super::{PSETID_TASK, Status, Task, cut, lid as task_lid};
use crate::calendar::{bool_of, i32_of, named, remove_named, set_named};
use crate::props::{PropValue, pid, pset};
use crate::{Attachment, Item, RECIP_BCC, RECIP_CC, RECIP_TO, codepage};

/// The message classes of task communications ([MS-OXOTASK] 2.2.4.1).
pub mod class {
    pub const REQUEST: &str = "IPM.TaskRequest";
    pub const ACCEPT: &str = "IPM.TaskRequest.Accept";
    pub const DECLINE: &str = "IPM.TaskRequest.Decline";
    pub const UPDATE: &str = "IPM.TaskRequest.Update";
}

/// Named properties of the task property set given to an assigned task ([MS-OXOTASK] 2.2.2.2).
mod lid {
    pub const ACCEPTED: u32 = 0x8108;
    pub const VERSION: u32 = 0x8112;
    pub const LAST_UPDATE: u32 = 0x8115;
    pub const ASSIGNERS: u32 = 0x8117;
    pub const STATUS_ON_COMPLETE: u32 = 0x8119;
    pub const HISTORY: u32 = 0x811A;
    pub const UPDATES: u32 = 0x811B;
    pub const FCREATOR: u32 = 0x811E;
    pub const MULTIPLE_RECIPIENTS: u32 = 0x8120;
    pub const ASSIGNER: u32 = 0x8121;
    pub const LAST_USER: u32 = 0x8122;
    pub const LAST_DELEGATE: u32 = 0x8125;
}

/// Of the common property set ([MS-OXOTASK] 2.2.2.1, 2.2.2.2.32).
mod common {
    pub const TASK_MODE: u32 = 0x8518;
    pub const GLOBAL_ID: u32 = 0x8519;
}

/// PidTagIconIndex of each kind of message ([MS-OXOTASK] 2.2.4.2).
const ICON_REQUEST: i32 = 0x0504;
const ICON_ACCEPT: i32 = 0x0505;
const ICON_DECLINE: i32 = 0x0506;
const ICON_UPDATE: i32 = 0x0500;
/// PidTagProcessed: the message has been acted on ([MS-OXOTASK] 2.2.4.3.1).
const PROCESSED: u16 = 0x7D01;
const ICON_INDEX: u16 = 0x1080;

/// Assigners a task keeps at most.
const MAX_ASSIGNERS: usize = 64;
/// The Address Book provider an assigner's EntryID names ([MS-OXCDATA] 2.2.5.2).
const AB_PROVIDER: [u8; 16] = [0xDC, 0xA7, 0x40, 0xC8, 0xC0, 0x42, 0x10, 0x1A, 0xB4, 0xB9, 0x08, 0x00, 0x2B, 0x2F, 0xE1, 0x82];

/// What kind of task communication a message is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Request,
    Accept,
    Decline,
    Update,
}

impl Kind {
    pub fn of(item: &Item) -> Option<Self> {
        let c = item.message_class();
        [(class::ACCEPT, Self::Accept), (class::DECLINE, Self::Decline), (class::UPDATE, Self::Update), (class::REQUEST, Self::Request)]
            .into_iter()
            .find(|(name, _)| c.eq_ignore_ascii_case(name))
            .map(|(_, k)| k)
    }

    pub fn class(self) -> &'static str {
        match self {
            Self::Request => class::REQUEST,
            Self::Accept => class::ACCEPT,
            Self::Decline => class::DECLINE,
            Self::Update => class::UPDATE,
        }
    }

    /// The subject's prefix, as Outlook gives it.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Request => "Task Request",
            Self::Accept => "Task Accepted",
            Self::Decline => "Task Declined",
            Self::Update => "Task Update",
        }
    }

    /// PidLidTaskMode on the task it carries ([MS-OXOTASK] 2.2.2.1.2).
    fn mode(self) -> i32 {
        match self {
            Self::Request => 1,
            Self::Accept => 2,
            Self::Decline => 3,
            Self::Update => 4,
        }
    }

    fn icon(self) -> i32 {
        match self {
            Self::Request => ICON_REQUEST,
            Self::Accept => ICON_ACCEPT,
            Self::Decline => ICON_DECLINE,
            Self::Update => ICON_UPDATE,
        }
    }
}

/// Whose copy of a task this is (PidLidTaskOwnership, [MS-OXOTASK] 2.2.2.2.29).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ownership {
    /// One's own, given to no one.
    #[default]
    Own,
    /// The copy of one who gave it to someone.
    Assigner,
    /// The copy of one it was given to.
    Assignee,
}

impl Ownership {
    pub fn name(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::Assigner => "assigner",
            Self::Assignee => "assignee",
        }
    }
}

/// How the one it was given to answered (PidLidTaskAcceptanceState, 2.2.2.2.30).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Acceptance {
    #[default]
    NotAssigned,
    Unknown,
    Accepted,
    Declined,
}

impl Acceptance {
    pub fn name(self) -> &'static str {
        match self {
            Self::NotAssigned => "notAssigned",
            Self::Unknown => "waiting",
            Self::Accepted => "accepted",
            Self::Declined => "declined",
        }
    }
}

/// One who gave a task on, as PidLidTaskAssigners keeps them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Assigner {
    pub name: String,
    pub address: String,
}

/// What an item says of a task given to someone, or taken from someone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Assignment {
    pub ownership: Ownership,
    pub acceptance: Acceptance,
    /// The one it was given to, by name (PidLidTaskOwner).
    pub owner: String,
    /// Who gave it on, the most recent last.
    pub assigners: Vec<Assigner>,
    pub global_id: Option<[u8; 16]>,
    pub version: i32,
    /// The assigner is sent an update of each change (PidLidTaskUpdates).
    pub updates: bool,
    /// The assigner is sent a report once it is done (PidLidTaskStatusOnComplete).
    pub status_on_complete: bool,
}

impl Assignment {
    pub fn of(item: &Item) -> Self {
        let t = PSETID_TASK;
        let ownership = match i32_of(named(item, t, task_lid::OWNERSHIP)) {
            Some(1) => Ownership::Assigner,
            Some(2) => Ownership::Assignee,
            _ => Ownership::Own,
        };
        let acceptance = match i32_of(named(item, t, task_lid::ACCEPTANCE_STATE)) {
            Some(1) => Acceptance::Unknown,
            Some(2) => Acceptance::Accepted,
            Some(3) => Acceptance::Declined,
            _ => Acceptance::NotAssigned,
        };
        Self {
            ownership,
            acceptance,
            owner: match named(item, t, task_lid::OWNER) {
                Some(PropValue::String(s)) => cut(s, super::MAX_FIELD),
                _ => String::new(),
            },
            assigners: match named(item, t, lid::ASSIGNERS) {
                Some(PropValue::Binary(b)) => read_assigners(b),
                _ => Vec::new(),
            },
            global_id: global_id(item),
            version: i32_of(named(item, t, lid::VERSION)).unwrap_or(1),
            updates: bool_of(named(item, t, lid::UPDATES)) == Some(true),
            status_on_complete: bool_of(named(item, t, lid::STATUS_ON_COMPLETE)) == Some(true),
        }
    }

    /// The most recent one who gave it on: the one an answer or an update goes to.
    pub fn last_assigner(&self) -> Option<&Assigner> {
        self.assigners.last()
    }
}

/// PidLidTaskGlobalId, the id both copies of an assigned task share.
pub fn global_id(item: &Item) -> Option<[u8; 16]> {
    match named(item, pset::PSETID_COMMON, common::GLOBAL_ID) {
        Some(PropValue::Binary(b)) => b.get(..16).and_then(|b| b.try_into().ok()),
        _ => None,
    }
}

/// The task a task communication carries: its first attachment, an attached item that is a
/// task ([MS-OXOTASK] 3.1.4.3).
pub fn carried(msg: &Item) -> Option<&Item> {
    msg.attachments.first().and_then(|a| a.embedded.as_deref()).filter(|t| super::is_task(t))
}

/// The stack of assigners ([MS-OXOTASK] 2.2.2.2.16): a count, then for each its size, an
/// Address Book EntryID, its name in 8 bits and in UTF-16. Entries that cannot be read end it.
fn read_assigners(b: &[u8]) -> Vec<Assigner> {
    let u32_at = |at: usize| b.get(at..at.checked_add(4)?).and_then(|s| s.try_into().ok()).map(u32::from_le_bytes);
    let Some(count) = u32_at(0) else { return Vec::new() };
    let mut out = Vec::new();
    let mut at = 4usize;
    for _ in 0..(count as usize).min(MAX_ASSIGNERS) {
        let Some(size) = u32_at(at) else { break };
        let Some(entry) = at.checked_add(4).and_then(|from| b.get(from..from.checked_add(size as usize)?)) else { break };
        at = at.saturating_add(4).saturating_add(size as usize);
        // Flags (4), the provider (16), version (4) and type (4), then the X500 DN, the name in
        // 8 bits and in UTF-16, each ending in a zero.
        let rest = entry.get(28..).unwrap_or_default();
        let (dn, rest) = until_zero(rest);
        let (name8, rest) = until_zero(rest);
        let mut wide = Vec::new();
        for pair in rest.chunks_exact(2) {
            if pair == [0, 0] {
                break;
            }
            wide.extend_from_slice(pair);
        }
        let name = if wide.is_empty() { codepage::decode(1252, name8) } else { codepage::utf16le(&wide) };
        let address = String::from_utf8_lossy(dn).trim().to_string();
        out.push(Assigner { name: cut(&name, super::MAX_FIELD), address: cut(address.rsplit("cn=").next().unwrap_or(&address), super::MAX_FIELD) });
    }
    out
}

fn until_zero(b: &[u8]) -> (&[u8], &[u8]) {
    match b.iter().position(|&c| c == 0) {
        Some(i) => (b.get(..i).unwrap_or_default(), b.get(i + 1..).unwrap_or_default()),
        None => (b, &[]),
    }
}

fn write_assigners(list: &[Assigner]) -> Vec<u8> {
    let list = list.get(list.len().saturating_sub(MAX_ASSIGNERS)..).unwrap_or(list);
    let mut out = (list.len() as u32).to_le_bytes().to_vec();
    for a in list {
        let mut entry = Vec::new();
        entry.extend_from_slice(&0u32.to_le_bytes());
        entry.extend_from_slice(&AB_PROVIDER);
        entry.extend_from_slice(&1u32.to_le_bytes());
        entry.extend_from_slice(&0u32.to_le_bytes());
        // An Internet address in place of an Exchange one: it is all that is known of them.
        entry.extend(a.address.bytes().filter(|c| c.is_ascii_graphic()));
        entry.push(0);
        entry.extend(codepage::encode(1252, &a.name).unwrap_or_else(|| a.name.bytes().filter(u8::is_ascii).collect()));
        entry.push(0);
        entry.extend(a.name.encode_utf16().chain([0]).flat_map(u16::to_le_bytes));
        out.extend_from_slice(&(entry.len() as u32).to_le_bytes());
        out.extend_from_slice(&entry);
    }
    out
}

/// What changed, as PidLidTaskHistory keeps it; setting it sets when, too (2.2.2.2.18).
#[derive(Clone, Copy)]
pub enum History {
    Accepted = 1,
    Declined = 2,
    Changed = 3,
    DueChanged = 4,
    Assigned = 5,
}

fn history(item: &mut Item, h: History, now: i64) {
    set_named(item, PSETID_TASK, lid::HISTORY, PropValue::Int32(h as i32));
    set_named(item, PSETID_TASK, lid::LAST_UPDATE, PropValue::Time(now));
}

fn set_i32(item: &mut Item, id: u32, v: i32) {
    set_named(item, PSETID_TASK, id, PropValue::Int32(v));
}

fn set_text(item: &mut Item, id: u32, v: &str) {
    set_named(item, PSETID_TASK, id, PropValue::String(cut(v, super::MAX_FIELD)));
}

/// Makes `task` the assigner's copy of a task given to `to` (name, address), by `me`
/// ([MS-OXOTASK] 3.1.4.4, task request): its owner is who it was given to, its recipients
/// them, and it waits for their answer. `global_id` is the id both copies will share, kept
/// when it has one. Returns the copy the request carries.
pub fn assign(task: &mut Item, to: &[(String, String)], me: &Assigner, global: [u8; 16], updates: bool, status_report: bool, now: i64) -> Item {
    let owner = to.iter().map(|(n, e)| if n.is_empty() { e.as_str() } else { n.as_str() }).collect::<Vec<_>>().join("; ");
    if global_id(task).is_none() {
        set_named(task, pset::PSETID_COMMON, common::GLOBAL_ID, PropValue::Binary(global.to_vec()));
    }
    task.recipients.retain(|r| r.i32(pid::RECIPIENT_TYPE).unwrap_or(RECIP_TO) != RECIP_TO);
    for (n, e) in to {
        task.add_recipient(RECIP_TO, n, e);
    }
    task.refresh_display_recipients();
    set_text(task, task_lid::OWNER, &owner);
    set_text(task, lid::ASSIGNER, &me.name);
    set_text(task, lid::LAST_USER, &me.name);
    set_text(task, lid::LAST_DELEGATE, &me.name);
    set_i32(task, task_lid::STATE, 3);
    set_i32(task, task_lid::OWNERSHIP, 1);
    set_i32(task, task_lid::ACCEPTANCE_STATE, 1);
    set_i32(task, lid::MULTIPLE_RECIPIENTS, i32::from(to.len() > 1));
    set_named(task, PSETID_TASK, lid::UPDATES, PropValue::Bool(updates));
    set_named(task, PSETID_TASK, lid::STATUS_ON_COMPLETE, PropValue::Bool(status_report));
    set_named(task, PSETID_TASK, lid::FCREATOR, PropValue::Bool(true));
    set_named(task, PSETID_TASK, lid::ACCEPTED, PropValue::Bool(false));
    if named(task, PSETID_TASK, lid::VERSION).is_none() {
        set_i32(task, lid::VERSION, 1);
    }
    history(task, History::Assigned, now);
    let mut carried = task.clone();
    // Who gave it on: the stack the one it goes to takes it with, oneself on top.
    let mut stack = Assignment::of(task).assigners;
    stack.push(me.clone());
    set_named(&mut carried, PSETID_TASK, lid::ASSIGNERS, PropValue::Binary(write_assigners(&stack)));
    carried
}

/// A task communication: the message, its class and icon, carrying `task` as its first
/// attachment, hidden, marked with the kind it is ([MS-OXOTASK] 3.1.4.3).
pub fn message(kind: Kind, task: &Item, body: &str) -> Item {
    let mut carried = task.clone();
    set_named(&mut carried, pset::PSETID_COMMON, common::TASK_MODE, PropValue::Int32(kind.mode()));
    // Who it goes to is the message's to say; the task keeps none.
    carried.recipients.retain(|r| r.i32(pid::RECIPIENT_TYPE).unwrap_or(RECIP_TO) == RECIP_TO);
    let subject = Task::of(task, 0i32).subject;
    let mut msg = Item::new_mail();
    msg.props.set_str(pid::MESSAGE_CLASS, kind.class());
    msg.set_subject(&format!("{}: {subject}", kind.prefix()));
    msg.set_body_text(body);
    msg.props.set(ICON_INDEX, PropValue::Int32(kind.icon()));
    if let Some(g) = global_id(task) {
        set_named(&mut msg, pset::PSETID_COMMON, common::GLOBAL_ID, PropValue::Binary(g.to_vec()));
    }
    set_named(&mut msg, pset::PSETID_COMMON, common::TASK_MODE, PropValue::Int32(0));
    let mut a = Attachment::default();
    a.props.set(pid::ATTACH_METHOD, PropValue::Int32(5));
    a.props.set(pid::RENDERING_POSITION, PropValue::Int32(-1));
    a.props.set(pid::ATTACHMENT_HIDDEN, PropValue::Bool(true));
    a.props.set_str(pid::ATTACH_LONG_FILENAME, &subject);
    a.props.set_str(pid::ATTACH_FILENAME, &subject);
    a.embedded = Some(Box::new(carried));
    msg.attachments.insert(0, a);
    msg
}

/// The assignee's own copy of a task a request carries, from `from` (who sent it) to `me`
/// ([MS-OXOTASK] 3.1.4.5, task request): the sender on top of who gave it on, oneself its
/// owner, and the assigner among its recipients to be sent what they asked for.
pub fn take_request(carried: &Item, from: &Assigner, me: &str) -> Item {
    let mut task = carried.clone();
    let a = Assignment::of(carried);
    let mut stack = a.assigners;
    // The one who sent it is the one who gave it on, whatever the task says.
    if stack.last().is_none_or(|l| !l.address.eq_ignore_ascii_case(&from.address)) {
        stack.push(from.clone());
    } else if let Some(l) = stack.last_mut() {
        l.name.clone_from(&from.name);
    }
    set_named(&mut task, PSETID_TASK, lid::ASSIGNERS, PropValue::Binary(write_assigners(&stack)));
    set_text(&mut task, task_lid::OWNER, me);
    set_i32(&mut task, task_lid::STATE, 2);
    set_i32(&mut task, task_lid::OWNERSHIP, 2);
    set_named(&mut task, PSETID_TASK, lid::FCREATOR, PropValue::Bool(false));
    set_named(&mut task, pset::PSETID_COMMON, common::TASK_MODE, PropValue::Int32(0));
    task.recipients.clear();
    if a.updates {
        task.add_recipient(RECIP_CC, &from.name, &from.address);
    }
    if a.status_on_complete {
        task.add_recipient(RECIP_BCC, &from.name, &from.address);
    }
    task.refresh_display_recipients();
    task
}

/// The assignee's answer written on their copy ([MS-OXOTASK] 3.1.4.4, task acceptance and
/// rejection): accepted, the stack cut to the last who gave it on; declined, that one taken
/// off and the owner again. Returns the copy the answer carries.
pub fn answer(task: &mut Item, accept: bool, me: &str, now: i64) -> Item {
    let a = Assignment::of(task);
    let mut stack = a.assigners;
    set_text(task, lid::LAST_USER, me);
    set_i32(task, lid::VERSION, a.version.saturating_add(1));
    if accept {
        let keep = stack.len().saturating_sub(1);
        stack.drain(..keep);
        set_named(task, PSETID_TASK, lid::ASSIGNERS, PropValue::Binary(write_assigners(&stack)));
        set_i32(task, task_lid::OWNERSHIP, 2);
        set_i32(task, task_lid::ACCEPTANCE_STATE, 2);
        set_named(task, PSETID_TASK, lid::ACCEPTED, PropValue::Bool(true));
        history(task, History::Accepted, now);
        return task.clone();
    }
    set_i32(task, task_lid::ACCEPTANCE_STATE, 3);
    set_named(task, PSETID_TASK, lid::ACCEPTED, PropValue::Bool(false));
    history(task, History::Declined, now);
    // What goes back still names the one declining as its owner; the copy kept, if it is,
    // is the last assigner's again.
    let sent = task.clone();
    if let Some(last) = stack.pop() {
        set_text(task, task_lid::OWNER, &last.name);
    }
    set_named(task, PSETID_TASK, lid::ASSIGNERS, PropValue::Binary(write_assigners(&stack)));
    sent
}

/// The assignee's copy marked as changed by `me`, to go in an update ([MS-OXOTASK] 3.1.4.4,
/// task update): its version moved on and what changed said.
pub fn changed(task: &mut Item, me: &str, due_changed: bool, now: i64) -> Item {
    let a = Assignment::of(task);
    set_text(task, lid::LAST_USER, me);
    set_i32(task, lid::VERSION, a.version.saturating_add(1));
    history(task, if due_changed { History::DueChanged } else { History::Changed }, now);
    task.clone()
}

/// What came back of a task given away, taken into the assigner's copy `local`
/// ([MS-OXOTASK] 3.1.4.5): an acceptance, a rejection or an update, by how far along the one
/// it was given to has it. `false` when it is older than what is known, and nothing is taken.
pub fn take_answer(local: &mut Item, kind: Kind, carried: &Item, now: i64) -> bool {
    let mine = Assignment::of(local);
    let theirs = Assignment::of(carried);
    if theirs.version < mine.version {
        return false;
    }
    let got = Task::of(carried, 0i32);
    let mut t = Task::of(local, 0i32);
    // Only what the work has come to: who gave it and when it reminds the assigner is theirs.
    t.status = got.status;
    t.percent = got.percent;
    t.completed = got.completed;
    t.start = got.start;
    t.due = got.due;
    t.total_work = got.total_work;
    t.actual_work = got.actual_work;
    t.reminder = None;
    t.write(local, got.completed.unwrap_or_default(), now, 0i32);
    set_i32(local, lid::VERSION, theirs.version);
    match kind {
        Kind::Accept => {
            set_i32(local, task_lid::ACCEPTANCE_STATE, 2);
            set_named(local, PSETID_TASK, lid::ACCEPTED, PropValue::Bool(true));
            history(local, History::Accepted, now);
        }
        Kind::Decline => {
            set_i32(local, task_lid::STATE, 4);
            set_i32(local, task_lid::ACCEPTANCE_STATE, 3);
            set_named(local, PSETID_TASK, lid::ACCEPTED, PropValue::Bool(false));
            history(local, History::Declined, now);
        }
        Kind::Update | Kind::Request => history(local, History::Changed, now),
    }
    if got.status == Status::Complete {
        remove_named(local, pset::PSETID_COMMON, 0x8503);
    }
    true
}

/// Marks a task communication as acted on (PidTagProcessed).
pub fn processed(msg: &mut Item) {
    msg.props.set(PROCESSED, PropValue::Bool(true));
}

pub fn is_processed(msg: &Item) -> bool {
    msg.props.bool(PROCESSED) == Some(true)
}

/// A task that is, or was, given to someone: not one's own to give again or to answer.
pub fn is_assigned(item: &Item) -> bool {
    Assignment::of(item).ownership != Ownership::Own
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> Assigner {
        Assigner { name: "Ana Costa".into(), address: "ana@example.org".into() }
    }

    #[test]
    fn a_task_goes_to_its_assignee_and_their_answer_comes_back() {
        let mut task = Task { subject: "Write the report".into(), due: Some(20_000), ..Task::default() }.item(19_990, 5, 0i32);
        let to = vec![("Bo Lind".to_string(), "bo@example.org".to_string())];
        let carried = assign(&mut task, &to, &me(), [7; 16], true, true, 10);
        let mine = Assignment::of(&task);
        assert_eq!((mine.ownership, mine.acceptance, mine.owner.as_str()), (Ownership::Assigner, Acceptance::Unknown, "Bo Lind"));
        assert_eq!(mine.global_id, Some([7; 16]));
        let request = message(Kind::Request, &carried, "Please.");
        assert_eq!(request.message_class(), class::REQUEST);
        assert_eq!(request.subject(), "Task Request: Write the report");
        assert_eq!(request.attachments[0].props.bool(pid::ATTACHMENT_HIDDEN), Some(true));
        let carried = carried_of(&request);
        assert_eq!(Assignment::of(&carried).assigners, vec![me()]);

        // Taken by the one it was given to, from whoever sent it.
        let mut theirs = take_request(&carried, &me(), "Bo Lind");
        let a = Assignment::of(&theirs);
        assert_eq!((a.ownership, a.owner.as_str(), a.assigners.len()), (Ownership::Assignee, "Bo Lind", 1));
        assert_eq!(theirs.recipients.len(), 2, "updates and the report on completing go to the assigner");
        let sent = answer(&mut theirs, true, "Bo Lind", 20);
        assert_eq!(Assignment::of(&theirs).acceptance, Acceptance::Accepted);
        let back = message(Kind::Accept, &sent, "");
        assert!(take_answer(&mut task, Kind::Accept, &carried_of(&back), 30));
        assert_eq!(Assignment::of(&task).acceptance, Acceptance::Accepted);

        // An update says how far along it is; an older one is not taken.
        let mut t = Task::of(&theirs, 0i32);
        t.percent = 50;
        t.write(&mut theirs, 19_995, 40, 0i32);
        let update = message(Kind::Update, &changed(&mut theirs, "Bo Lind", false, 40), "");
        let stale = message(Kind::Update, &sent, "");
        assert!(take_answer(&mut task, Kind::Update, &carried_of(&update), 50));
        assert_eq!(Task::of(&task, 0i32).percent, 50);
        assert!(!take_answer(&mut task, Kind::Update, &carried_of(&stale), 60));
        assert_eq!(Task::of(&task, 0i32).percent, 50);

        // Declined, it is the assigner's own again.
        let mut other = take_request(&carried, &me(), "Bo Lind");
        let no = answer(&mut other, false, "Bo Lind", 70);
        assert_eq!(Assignment::of(&other).owner, "Ana Costa");
        let mut fresh = Task { subject: "x".into(), ..Task::default() }.item(1, 1, 0i32);
        let _ = assign(&mut fresh, &to, &me(), [8; 16], false, false, 1);
        assert!(take_answer(&mut fresh, Kind::Decline, &no, 80));
        assert_eq!(Assignment::of(&fresh).acceptance, Acceptance::Declined);
    }

    fn carried_of(msg: &Item) -> Item {
        carried(msg).cloned().unwrap_or_default()
    }

    #[test]
    fn a_stack_of_assigners_that_cannot_be_read_is_cut_short() {
        let list = vec![me(), Assigner { name: "Zöe".into(), address: "zoe@example.org".into() }];
        let b = write_assigners(&list);
        assert_eq!(read_assigners(&b), list);
        for n in 0..b.len() {
            let _ = read_assigners(&b[..n]);
        }
        let mut huge = u32::MAX.to_le_bytes().to_vec();
        huge.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_assigners(&huge).is_empty());
    }
}
