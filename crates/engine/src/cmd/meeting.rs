//! Meetings: appointments others are asked to. The organizer's invitation, an attendee's answer
//! and a meeting called off go by mail as iCalendar messages (RFC 5546 iTIP, carried as RFC 6047
//! iMIP), each a message of the class Outlook gives it ([MS-OXOCAL] 2.2.6.1 IPM.Schedule.
//! Meeting.Request, 2.2.7.1 .Resp.*, 2.2.8.1 .Canceled) with the calendar as another way of
//! reading it, so that Outlook and other mail programs show it as an invitation.
//!
//! An invitation that comes is not put in the calendar until it is answered (Outlook puts it in
//! at once, as tentative, which lets anyone who can send mail write into one's calendar). An
//! answer or a cancellation that comes is taken in at once, but only by the meeting it names
//! (its UID), and only from the person it has to come from: an answer from that attendee, a
//! cancellation from that meeting's organizer. Anything else is left as a message to read.

use super::calendar::{calendar_folder, stamp, writable_calendar};
use super::calendar_file::by_global_id;
use super::{CommandSpec, ok, p_bool, p_str, p_u64};
use crate::{CmdResult, EngineError, Session, undo::Step};
use openmailclient_formats::ics::{self, Method};
use openmailclient_model::calendar::{self, Appointment, Busy};
use openmailclient_model::meeting::{self, Attendee, Meeting, Response, Role};
use openmailclient_model::{Attachment, FolderId, FolderKind, Item, ItemId, MSGFLAG_READ, PropValue, RECIP_CC, RECIP_TO, StoreError, pid};
use serde_json::{Value, json};

/// Appointments that overlap an invitation listed at most.
const MAX_CONFLICTS: usize = 20;
/// Longest comment sent with an answer or a cancellation, in characters.
const MAX_COMMENT: usize = 4_000;
/// What a calendar carried by a message is read as, at most.
const MAX_CALENDAR: usize = 4 << 20;

const ATTENDEES_PARAMS: &str = r#""attendees": addresses as one line ("Sam Lee <sam@example.com>; priya@example.net"), or a list of addresses or of {"name", "address", "role": "required" | "optional" | "resource"}, or lines by role {"required", "optional", "resources"}; an empty list makes it no meeting"#;
const RESPOND_PARAMS: &str = r#"{"response": "accept" | "tentative" | "decline", "send": false to answer without telling the organizer (default true), "comment": text sent with the answer, "id": a meeting of the calendar to answer (default: the invitation open, else the meeting chosen)}; an accepted or tentative meeting is put in the calendar, or brought up to date there; a declined one is taken out; the invitation answered goes to Deleted Items"#;

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "calendar.newMeeting",
            label: "New Meeting",
            tab: "Home",
            group: "New",
            shortcut: "Ctrl+Shift+Q",
            params: NEW_MEETING_PARAMS,
            enabled: has_calendar_writable,
            run: new_meeting,
        },
        CommandSpec {
            id: "calendar.attendees",
            label: "Tracking",
            tab: "Meeting",
            group: "Show",
            shortcut: "",
            params: r#"{"id" (default: the one chosen)}; the organizer and each attendee with their answer and when they gave it, and how many gave each"#,
            enabled: has_calendar,
            run: attendees,
        },
        CommandSpec {
            id: "calendar.sendInvitations",
            label: "Send",
            tab: "Meeting",
            group: "Actions",
            shortcut: "",
            params: r#"{"id" (default: the one chosen), "wait": false to send on a thread of its own}; the invitation sent to every attendee, or an update once one was sent; it is put in the Outbox and goes when there is an account"#,
            enabled: has_calendar_writable,
            run: send_invitations,
        },
        CommandSpec {
            id: "calendar.cancelMeeting",
            label: "Cancel Meeting",
            tab: "Meeting",
            group: "Actions",
            shortcut: "",
            params: r#"{"id" (default: the one chosen), "comment": text sent with it, "wait"}; every attendee told it is called off, and the meeting goes to Deleted Items; only its organizer cancels one"#,
            enabled: has_calendar_writable,
            run: cancel_meeting,
        },
        CommandSpec {
            id: "calendar.respond",
            label: "Accept, Tentative, Decline",
            tab: "Meeting",
            group: "Respond",
            shortcut: "",
            params: RESPOND_PARAMS,
            enabled: has_calendar_writable,
            run: respond,
        },
        CommandSpec {
            id: "calendar.invitation",
            label: "Meeting Invitation",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{}; what the open message asks or tells of a meeting: {"method": "REQUEST" | "REPLY" | "CANCEL", "start", "end", "allDay", "recurring", "pattern", "organizer", "attendees", "response": one's own answer so far, "inCalendar": the appointment's id when it is in the calendar, "outOfDate": a later update is there, "conflicts": [appointments it overlaps], "untrusted_content": {"subject", "location", "description", "comment", "uid", "sequence": a request's, as its sender wrote them}}"#,
            enabled: has_invitation,
            run: invitation,
        },
        CommandSpec {
            id: "mail.replyWithMeeting",
            label: "Meeting",
            tab: "Home",
            group: "Respond",
            shortcut: "Ctrl+Alt+R",
            params: r#"{"start": "YYYY-MM-DD HH:MM" (default: the next whole hour), and any calendar.newMeeting takes}; a meeting with the open message's sender and recipients asked, its subject the message's"#,
            enabled: has_open_and_calendar,
            run: reply_with_meeting,
        },
    ]
}

const NEW_MEETING_PARAMS: &str = r#"{any calendar.new takes, "attendees" as calendar.change takes them (required), "send": true to send the invitations at once (default false: calendar.sendInvitations sends them)}; returns {"id"}"#;

fn has_calendar(s: &Session) -> bool {
    calendar_folder(s).is_some()
}

fn has_calendar_writable(s: &Session) -> bool {
    calendar_folder(s).is_some_and(|(store, _)| s.stores.get(store).is_some_and(|st| st.writable()))
}

fn has_invitation(s: &Session) -> bool {
    s.open.as_ref().is_some_and(meeting::is_meeting_message)
}

fn has_open_and_calendar(s: &Session) -> bool {
    s.open.is_some() && has_calendar_writable(s)
}

/// The addresses that are one's own: each account's, and the session's.
pub(crate) fn mine(s: &Session) -> Vec<String> {
    let mut out: Vec<String> = s.accounts.iter().map(|a| a.email.trim().to_string()).collect();
    out.push(s.user_email.trim().to_string());
    out.retain(|a| !a.is_empty());
    out
}

fn is_mine(s: &Session, address: &str) -> bool {
    !address.trim().is_empty() && mine(s).iter().any(|m| m.eq_ignore_ascii_case(address.trim()))
}

/// Oneself as the organizer of a meeting: the first account, or the session's name.
fn me(s: &Session) -> Attendee {
    match super::send::account_for(s, "") {
        Some(a) => Attendee::new(&a.display_name, &a.email, Role::Required),
        None => Attendee::new(&s.user_name, &s.user_email, Role::Required),
    }
}

/// The attendees `v` names: a line of addresses, or a list. None of them oneself.
pub(super) fn attendees_param(s: &Session, v: &Value) -> Result<Vec<Attendee>, EngineError> {
    let bad = || EngineError::BadParams(format!("{ATTENDEES_PARAMS}; each needs an address with an @ in it"));
    let mut out: Vec<Attendee> = Vec::new();
    let mut push = |p: Attendee| {
        if p.address.contains('@') && !out.iter().any(|q| q.is(&p.address)) && out.len() < meeting::MAX_ATTENDEES {
            out.push(p);
        }
    };
    match v {
        Value::Null => {}
        Value::String(line) => {
            for (name, address) in openmailclient_mime::parse_addresses(line).into_iter().take(meeting::MAX_ATTENDEES) {
                push(Attendee::new(&name, &address, Role::Required));
            }
        }
        // Lines by role, as the appointment window has them.
        Value::Object(lines) if !lines.contains_key("address") => {
            for (key, role) in [("required", Role::Required), ("optional", Role::Optional), ("resources", Role::Resource)] {
                match lines.get(key) {
                    None | Some(Value::Null) => {}
                    Some(Value::String(line)) => {
                        for (name, address) in openmailclient_mime::parse_addresses(line).into_iter().take(meeting::MAX_ATTENDEES) {
                            push(Attendee::new(&name, &address, role));
                        }
                    }
                    Some(_) => return Err(bad()),
                }
            }
            if let Some(other) = lines.keys().find(|k| !matches!(k.as_str(), "required" | "optional" | "resources")) {
                return Err(EngineError::BadParams(format!(
                    "\"attendees\" has no line \"{}\"; {ATTENDEES_PARAMS}",
                    other.chars().take(40).collect::<String>()
                )));
            }
        }
        Value::Array(list) => {
            for p in list.iter().take(meeting::MAX_ATTENDEES.saturating_mul(2)) {
                match p {
                    Value::String(address) => {
                        let (name, address) = openmailclient_mime::parse_addresses(address).into_iter().next().ok_or_else(bad)?;
                        push(Attendee::new(&name, &address, Role::Required));
                    }
                    Value::Object(_) => {
                        let address = p_str(p, "address").ok_or_else(bad)?;
                        if !address.contains('@') {
                            return Err(bad());
                        }
                        let role = match p_str(p, "role") {
                            None => Role::Required,
                            Some(r) => Role::parse(r).ok_or_else(bad)?,
                        };
                        push(Attendee::new(p_str(p, "name").unwrap_or(""), address, role));
                    }
                    _ => return Err(bad()),
                }
            }
        }
        _ => return Err(bad()),
    }
    out.retain(|p| !is_mine(s, &p.address));
    Ok(out)
}

/// `item` given these attendees: made a meeting one organizes, its attendees' answers kept
/// where they are asked again; or no meeting, when there are none. Another's meeting cannot
/// be given other attendees here.
pub(super) fn set_attendees(s: &Session, item: &mut Item, attendees: Vec<Attendee>) -> Result<(), EngineError> {
    let was = Meeting::of(item);
    if was.as_ref().is_some_and(|m| m.received) {
        return Err(EngineError::BadParams("only a meeting's organizer changes who is asked to it".into()));
    }
    if attendees.is_empty() {
        if was.is_some() {
            meeting::unmake(item);
        }
        return Ok(());
    }
    let mut m = was.unwrap_or_else(|| Meeting { organizer: me(s), response: Response::Organized, ..Meeting::default() });
    let kept: Vec<Attendee> = attendees
        .into_iter()
        .map(|mut p| {
            if let Some(q) = m.attendee(&p.address) {
                p.response = q.response;
                p.replied = q.replied;
                if p.name.is_empty() {
                    p.name = q.name.clone();
                }
            }
            p
        })
        .collect();
    m.attendees = kept;
    m.write(item);
    Ok(())
}

fn person_json(p: &Attendee) -> Value {
    json!({
        "role": p.role.name(),
        "response": p.response.name(),
        "replied": (p.replied > 0).then_some(p.replied),
        "untrusted_content": { "name": p.name, "address": p.address },
    })
}

fn meeting_json(s: &Session, m: &Meeting) -> Value {
    let [accepted, tentative, declined, none] = m.tally();
    let replied = |p: &Attendee| (p.replied > 0).then(|| stamp(s, p.replied));
    let people: Vec<Value> = m
        .attendees
        .iter()
        .map(|p| {
            let mut v = person_json(p);
            v["replied"] = json!(replied(p));
            v
        })
        .collect();
    json!({
        "organizer": person_json(&m.organizer),
        "attendees": people,
        "response": m.response.name(),
        "organized": !m.received,
        "invited": m.invited,
        "cancelled": m.cancelled,
        "sequence": m.sequence,
        "tally": { "accepted": accepted, "tentative": tentative, "declined": declined, "none": none },
    })
}

fn chosen(s: &Session, p: &Value) -> Result<u64, EngineError> {
    p_u64(p, "id").or(s.calendar.selected).ok_or_else(|| EngineError::BadParams("no meeting is chosen".into()))
}

fn attendees(s: &mut Session, p: &Value) -> CmdResult {
    let (store, folder) = calendar_folder(s).ok_or(StoreError::NoFolder)?;
    let id = chosen(s, p)?;
    let item = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.load(folder, ItemId(id))?;
    let m = Meeting::of(&item).ok_or_else(|| EngineError::BadParams("that appointment is no meeting: no one is asked to it".into()))?;
    Ok(meeting_json(s, &m))
}

fn new_meeting(s: &mut Session, p: &Value) -> CmdResult {
    let asked = attendees_param(s, p.get("attendees").unwrap_or(&Value::Null))?;
    if asked.is_empty() {
        return Err(EngineError::BadParams("a meeting needs \"attendees\": someone other than oneself to ask".into()));
    }
    let made = super::calendar::new(s, p)?;
    let id = made.get("id").and_then(Value::as_u64).ok_or(StoreError::NoItem)?;
    if p_bool(p, "send") == Some(true) {
        send_invitations(s, &json!({ "id": id, "wait": p.get("wait").cloned().unwrap_or(Value::Bool(true)) }))?;
    } else {
        s.status = "Meeting made; send its invitations with Send".to_string();
    }
    Ok(json!({ "id": id }))
}

impl Session {
    /// The meeting appointment `id` of the calendar is; `None` when no one is asked to it.
    pub fn meeting(&mut self, id: u64) -> Result<Option<Meeting>, EngineError> {
        let (store, folder) = calendar_folder(self).ok_or(StoreError::NoFolder)?;
        let item = self.stores.get_mut(store).ok_or(StoreError::NoFolder)?.load(folder, ItemId(id))?;
        Ok(Meeting::of(&item))
    }

    /// Those a meeting in reply to the open message asks: its sender and those it went to,
    /// those it was copied to as optional, oneself left out.
    pub fn reply_attendees(&self) -> Vec<Attendee> {
        let Some(item) = &self.open else { return Vec::new() };
        let mut people: Vec<Attendee> = Vec::new();
        let mut add = |name: String, address: String, role: Role| {
            if address.contains('@') && !is_mine(self, &address) && !people.iter().any(|p| p.is(&address)) && people.len() < meeting::MAX_ATTENDEES {
                people.push(Attendee::new(&name, &address, role));
            }
        };
        add(item.sender_name(), item.sender_email(), Role::Required);
        for r in &item.recipients {
            let role = if r.i32(pid::RECIPIENT_TYPE) == Some(RECIP_CC) { Role::Optional } else { Role::Required };
            if r.i32(pid::RECIPIENT_TYPE).is_some_and(|t| t == RECIP_TO || t == RECIP_CC) {
                add(openmailclient_model::recipient_label(r), openmailclient_model::recipient_email(r), role);
            }
        }
        people
    }
}

fn reply_with_meeting(s: &mut Session, p: &Value) -> CmdResult {
    let item = s.open.clone().ok_or(StoreError::NoItem)?;
    let people: Vec<Value> = s.reply_attendees().iter().map(|a| json!({ "name": a.name, "address": a.address, "role": a.role.name() })).collect();
    let mut asked = match p {
        Value::Object(m) => m.clone(),
        _ => serde_json::Map::new(),
    };
    asked.entry("attendees").or_insert(Value::Array(people));
    asked.entry("subject").or_insert_with(|| Value::String(item.subject()));
    if !asked.contains_key("start") {
        // The next whole hour, where we are.
        let now = s.time_now();
        let next = now.saturating_sub(now.rem_euclid(60 * calendar::MINUTE)).saturating_add(60 * calendar::MINUTE);
        asked.insert("start".into(), Value::String(stamp(s, next)));
    }
    new_meeting(s, &Value::Object(asked))
}

/// Where a meeting is: the calendar, and the item.
struct Found {
    store: usize,
    folder: FolderId,
    id: u64,
    item: Item,
}

fn load_meeting(s: &mut Session, p: &Value) -> Result<(Found, Meeting), EngineError> {
    let (store, folder) = writable_calendar(s)?;
    let id = chosen(s, p)?;
    let item = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.load(folder, ItemId(id))?;
    let m = Meeting::of(&item).ok_or_else(|| EngineError::BadParams("that appointment is no meeting: give it attendees first".into()))?;
    Ok((Found { store, folder, id, item }, m))
}

/// When and where, as the body of a meeting's message says it to one who reads it without a
/// calendar.
fn when_where(s: &Session, a: &Appointment) -> String {
    let when = if a.all_day { stamp(s, a.start).chars().take(10).collect() } else { format!("{} – {}", stamp(s, a.start), stamp(s, a.end)) };
    let mut out = format!("When: {when} ({})\n", s.zone.label());
    if a.recurring {
        out.push_str(&format!("Repeats: {}\n", a.pattern_words()));
    }
    if !a.location.is_empty() {
        out.push_str(&format!("Where: {}\n", a.location));
    }
    out
}

/// A meeting's message put in the Outbox, and sent when there is an account to send it from.
fn post(s: &mut Session, class: &str, subject: &str, body: &str, to: &[&Attendee], cc: &[&Attendee], calendar: String, wait: bool) -> CmdResult {
    let store = s.writable_store().ok_or_else(|| EngineError::Format("there is no writable data file to keep the message in".into()))?;
    let outbox = s.find_folder(store, FolderKind::Outbox).ok_or_else(|| EngineError::Format("this data file has no Outbox".into()))?;
    let mut item = Item::new_mail();
    item.props.set_str(pid::MESSAGE_CLASS, class);
    item.set_subject(subject);
    item.set_body_text(body);
    let from = me(s);
    item.set_sender(&from.name, &from.address);
    for (kind, list) in [(RECIP_TO, to), (RECIP_CC, cc)] {
        for p in list {
            item.add_recipient(kind, &p.name, &p.address);
        }
    }
    item.refresh_display_recipients();
    item.attachments.push(Attachment::file("invite.ics", "text/calendar", calendar.into_bytes()));
    dress(&mut item, &s.zone, false);
    item.props.set(pid::CREATION_TIME, PropValue::Time(s.now));
    item.props.set(pid::CLIENT_SUBMIT_TIME, PropValue::Time(s.now));
    item.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(MSGFLAG_READ));
    s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.add(outbox, item)?;
    s.refresh_folders(store);
    if s.accounts.is_empty() || s.connector.is_none() {
        s.status = "The meeting's message is in the Outbox; it goes when there is an account to send it from".to_string();
        return ok();
    }
    super::send::send_outbox(s, wait).or_else(|e| {
        s.status = format!("The meeting's message is in the Outbox and was not sent: {e}");
        ok()
    })
}

fn send_invitations(s: &mut Session, p: &Value) -> CmdResult {
    let (mut f, mut m) = load_meeting(s, p)?;
    if m.received {
        return Err(EngineError::BadParams("only a meeting's organizer sends its invitations; answer it with Accept, Tentative or Decline".into()));
    }
    if m.cancelled {
        return Err(EngineError::BadParams("that meeting was called off".into()));
    }
    if m.attendees.is_empty() {
        return Err(EngineError::BadParams("no one is asked to that meeting".into()));
    }
    let update = m.invited;
    if update {
        m.sequence = m.sequence.saturating_add(1);
    }
    m.invited = true;
    let before = Box::new(f.item.clone());
    m.write(&mut f.item);
    let a = Appointment::of(&f.item).ok_or(StoreError::NoItem)?;
    let text = ics::write_as(std::slice::from_ref(&f.item), &s.zone, s.time_now(), None, &Method::Request);
    s.stores.get_mut(f.store).ok_or(StoreError::NoFolder)?.update(f.folder, ItemId(f.id), f.item.clone())?;
    s.did(Step::Edited { what: "Send Meeting", store: f.store, folder: f.folder, id: f.id, before });
    let body = format!("{}\n{}", when_where(s, &a), f.item.body_text());
    let to: Vec<&Attendee> = m.attendees.iter().filter(|p| p.role != Role::Optional).collect();
    let cc: Vec<&Attendee> = m.attendees.iter().filter(|p| p.role == Role::Optional).collect();
    post(s, meeting::class::REQUEST, &a.subject, &body, &to, &cc, text, p_bool(p, "wait") != Some(false))?;
    s.reload_calendar()?;
    if !s.status.contains("Outbox") {
        s.status = format!("{} sent to {} attendees", if update { "Update" } else { "Invitation" }, m.attendees.len());
    }
    Ok(json!({ "sent": m.attendees.len(), "update": update, "sequence": m.sequence }))
}

fn comment(p: &Value) -> String {
    p_str(p, "comment").map(|c| c.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').take(MAX_COMMENT).collect()).unwrap_or_default()
}

fn cancel_meeting(s: &mut Session, p: &Value) -> CmdResult {
    let (mut f, mut m) = load_meeting(s, p)?;
    if m.received {
        return Err(EngineError::BadParams("only its organizer cancels a meeting; decline it instead".into()));
    }
    let told = m.invited && !m.attendees.is_empty();
    if told {
        m.sequence = m.sequence.saturating_add(1);
        m.cancelled = true;
        m.write(&mut f.item);
        let a = Appointment::of(&f.item).ok_or(StoreError::NoItem)?;
        let text = ics::write_as(std::slice::from_ref(&f.item), &s.zone, s.time_now(), None, &Method::Cancel);
        let note = comment(p);
        let body =
            format!("This meeting is cancelled.\n\n{}{}", when_where(s, &a), if note.is_empty() { String::new() } else { format!("\n{note}\n") });
        let everyone: Vec<&Attendee> = m.attendees.iter().collect();
        post(s, meeting::class::CANCELED, &format!("Canceled: {}", a.subject), &body, &everyone, &[], text, p_bool(p, "wait") != Some(false))?;
    }
    super::calendar::delete(s, &json!({ "id": f.id, "series": true }))?;
    s.status =
        if told { format!("Meeting cancelled; {} attendees told", m.attendees.len()) } else { "Meeting deleted; no one had been asked yet".into() };
    Ok(json!({ "told": if told { m.attendees.len() } else { 0 } }))
}

/// The calendar a meeting's message carries, read: its iCalendar part, as it comes by mail,
/// or else what its properties say, as Outlook keeps a meeting's messages in its data files and
/// `.msg` files with no calendar in them ([MS-OXOCAL] 2.2.6 to 2.2.8).
fn carried(item: &Item, zone: &openmailclient_model::zone::Zone) -> Option<ics::Calendar> {
    match item.attachments.iter().find(|a| a.embedded.is_none() && a.mime().eq_ignore_ascii_case("text/calendar")) {
        Some(a) => ics::read(a.data.get(..a.data.len().min(MAX_CALENDAR))?, zone).ok(),
        None => of_properties(item),
    }
}

/// The longest description or comment read from a meeting's message.
const MAX_WORDS: usize = 20_000;

/// A meeting's message as Outlook keeps one, read from its properties: the meeting it is about
/// by its PidLidGlobalObjectId ([MS-OXOCAL] 2.2.1.27); for a request ([MS-OXOCAL] 2.2.6.1) the
/// appointment, its organizer and its attendees; for an answer (2.2.7.1) who gave it, and how,
/// by its class; for a cancellation (2.2.8.1) who called it off. One about a single occurrence
/// is read as about the meeting.
fn of_properties(item: &Item) -> Option<ics::Calendar> {
    let uid = calendar::uid_of_global_id(&calendar::global_id(item)?);
    let class = item.message_class().to_ascii_lowercase();
    let m = Meeting::of(item);
    let sequence = m.as_ref().map_or(0, |m| m.sequence);
    let sender = Attendee::new(&item.sender_name(), &item.sender_email(), Role::Required);
    let words = || item.body_text().trim_end().chars().take(MAX_WORDS).collect::<String>();
    let mut cal = ics::Calendar::default();
    if class == meeting::class::REQUEST.to_ascii_lowercase() {
        let appointment = Appointment::of(item)?;
        let (organizer, attendees) = match m {
            Some(m) => (m.organizer, m.attendees),
            None => (sender, Vec::new()),
        };
        cal.method = Some("REQUEST".into());
        cal.events.push(ics::Event {
            uid,
            appointment,
            description: words(),
            categories: Vec::new(),
            organizer: Some(organizer),
            attendees,
            sequence,
            notes: Vec::new(),
        });
    } else if let Some(kind) = class.strip_prefix("ipm.schedule.meeting.resp.") {
        let mut attendee = sender;
        attendee.response = match kind {
            "pos" => Response::Accepted,
            "tent" => Response::Tentative,
            "neg" => Response::Declined,
            _ => return None,
        };
        cal.method = Some("REPLY".into());
        cal.answers.push(ics::Answer { uid, occurrence: None, attendee, sequence, comment: words() });
    } else if class == meeting::class::CANCELED.to_ascii_lowercase() {
        cal.method = Some("CANCEL".into());
        cal.cancelled.push(ics::Cancelled { uid, occurrence: None, sequence, organizer: Some(sender) });
    } else {
        return None;
    }
    Some(cal)
}

/// A meeting's message given the properties Outlook gives one ([MS-OXOCAL] 2.2.6.1, 2.2.7.1,
/// 2.2.8.1), from the calendar it carries: the meeting's GlobalObjectId, when and where it is,
/// its state and sequence and, for a request that came, that it is not answered yet. Outlook
/// reads a meeting's message in a data file by these alone, so one we keep opens in Outlook as
/// the meeting it is. Its class, subject, body and recipients are left as they are.
pub(crate) fn dress(item: &mut Item, zone: &openmailclient_model::zone::Zone, received: bool) {
    if !meeting::is_meeting_message(item) || calendar::global_id(item).is_some() {
        return;
    }
    let Some(cal) = carried(item, zone) else { return };
    let (uid, sequence) = match (cal.events.first(), cal.answers.first(), cal.cancelled.first()) {
        (Some(e), _, _) => (e.uid.clone(), e.sequence),
        (None, Some(a), _) => (a.uid.clone(), a.sequence),
        (None, None, Some(c)) => (c.uid.clone(), c.sequence),
        _ => return,
    };
    let (class, subject) = (item.message_class().to_string(), item.subject());
    if let Some(e) = cal.events.first() {
        e.appointment.write(item);
        item.props.set_str(pid::MESSAGE_CLASS, &class);
        item.set_subject(&subject);
    }
    let response = if received && !cal.events.is_empty() { Response::NotResponded } else { Response::None };
    meeting::dress(item, &calendar::global_id_of_uid(&uid), sequence, received, response);
}

fn response_param(p: &Value) -> Result<Response, EngineError> {
    match p_str(p, "response").map(str::to_ascii_lowercase).as_deref() {
        Some("accept" | "accepted") => Ok(Response::Accepted),
        Some("tentative") => Ok(Response::Tentative),
        Some("decline" | "declined") => Ok(Response::Declined),
        _ => Err(EngineError::BadParams("\"response\" is accept, tentative or decline".into())),
    }
}

fn respond(s: &mut Session, p: &Value) -> CmdResult {
    let answer = response_param(p)?;
    let (store, folder) = writable_calendar(s)?;
    let mine: Vec<String> = mine(s);
    let me_refs: Vec<&str> = mine.iter().map(String::as_str).collect();
    let open_request =
        s.open.as_ref().filter(|i| p_u64(p, "id").is_none() && i.message_class().eq_ignore_ascii_case(meeting::class::REQUEST)).cloned();
    // The meeting as it is to be kept: from the invitation, or as the calendar has it.
    let (existing, mut item, from_message) = match &open_request {
        Some(msg) => {
            let cal = carried(msg, &s.zone).ok_or_else(|| EngineError::Format("the invitation carries no meeting that can be read".into()))?;
            let e = cal.events.first().ok_or_else(|| EngineError::Format("the invitation carries no meeting".into()))?;
            let m = e.meeting(&me_refs).ok_or_else(|| EngineError::Format("the invitation asks no one".into()))?;
            if !m.received {
                return Err(EngineError::BadParams("that is one's own meeting".into()));
            }
            let known = by_global_id(s, store, folder)?;
            let existing = known.get(&calendar::global_id_of_uid(&e.uid)).copied();
            let mut item = match existing {
                Some(id) => {
                    let item = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.load(folder, ItemId(id))?;
                    if Meeting::of(&item).is_some_and(|was| was.sequence > e.sequence) {
                        return Err(EngineError::BadParams("a later update of this meeting has come; answer that one".into()));
                    }
                    item
                }
                None => e.appointment.item(),
            };
            e.write_as(&mut item, &me_refs);
            (existing, item, true)
        }
        None => {
            let id = chosen(s, p)?;
            let item = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.load(folder, ItemId(id))?;
            (Some(id), item, false)
        }
    };
    let mut m = Meeting::of(&item).ok_or_else(|| EngineError::BadParams("that appointment is no meeting".into()))?;
    if !m.received {
        return Err(EngineError::BadParams("one does not answer one's own meeting".into()));
    }
    if m.cancelled {
        return Err(EngineError::BadParams("that meeting was called off".into()));
    }
    let now = s.time_now();
    m.response = answer;
    m.replied = now;
    // Oneself among those asked; one asked through a list is not named, and answers as oneself.
    let me_now = m.attendees.iter().find(|q| mine.iter().any(|a| q.is(a))).cloned().unwrap_or_else(|| me(s));
    let mut from = me_now.clone();
    from.response = answer;
    from.replied = now;
    if let Some(q) = m.attendees.iter_mut().find(|q| q.is(&from.address)) {
        q.response = answer;
        q.replied = now;
    }
    m.write(&mut item);
    if let Some(mut a) = Appointment::of(&item) {
        // Tentative is shown as such; an accepted one as busy as the organizer asked.
        if answer == Response::Tentative {
            a.busy = Busy::Tentative;
        } else if a.busy == Busy::Tentative && answer == Response::Accepted {
            a.busy = Busy::Busy;
        }
        a.write(&mut item);
    }
    // What is sent back is made before the meeting is taken out on a decline.
    let reply = (p_bool(p, "send") != Some(false)).then(|| {
        let text = ics::write_as(std::slice::from_ref(&item), &s.zone, now, None, &Method::Reply { from: &from, comment: &comment(p) });
        (text, Appointment::of(&item).map(|a| a.subject).unwrap_or_default())
    });
    let kept = match (answer, existing) {
        (Response::Declined, Some(id)) => {
            super::calendar::delete(s, &json!({ "id": id, "series": true }))?;
            None
        }
        (Response::Declined, None) => None,
        (_, Some(id)) => {
            let st = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
            let before = Box::new(st.load(folder, ItemId(id))?);
            st.update(folder, ItemId(id), item)?;
            s.did(Step::Edited { what: answer.prefix(), store, folder, id, before });
            Some(id)
        }
        (_, None) => {
            let id = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.add(folder, item)?;
            s.did(Step::Imported { what: answer.prefix(), store, folder, added: vec![id.0], updated: Vec::new() });
            Some(id.0)
        }
    };
    s.refresh_folders(store);
    s.reload_calendar()?;
    let sent = reply.is_some();
    if let Some((text, subject)) = reply {
        let organizer = m.organizer.clone();
        let words = comment(p);
        let body = if words.is_empty() { format!("{} {}.", from.label(), verb(answer)) } else { words };
        post(s, answer.class(), &format!("{}: {subject}", answer.prefix()), &body, &[&organizer], &[], text, p_bool(p, "wait") != Some(false))?;
    }
    // The invitation answered is done with, as Outlook leaves it.
    if from_message && s.is_enabled("mail.delete") {
        s.run("mail.delete", &json!({}))?;
    }
    s.status = format!("{}{}", answer.prefix(), if sent { "; the organizer is told" } else { "; the organizer is not told" });
    Ok(json!({ "id": kept, "sent": sent }))
}

/// An answer as a sentence says it was given.
fn verb(r: Response) -> &'static str {
    match r {
        Response::Accepted => "accepted",
        Response::Tentative => "tentatively accepted",
        Response::Declined => "declined",
        _ => "answered",
    }
}

/// Whether two spans of time overlap.
fn overlaps(a: (i64, i64), b: (i64, i64)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

fn invitation(s: &mut Session, _: &Value) -> CmdResult {
    let msg = s.open.clone().filter(meeting::is_meeting_message).ok_or(StoreError::NoItem)?;
    let cal = carried(&msg, &s.zone).ok_or_else(|| EngineError::Format("the message carries no meeting that can be read".into()))?;
    let mine = mine(s);
    let me_refs: Vec<&str> = mine.iter().map(String::as_str).collect();
    let method = cal.method.clone().unwrap_or_default();
    if let Some(answer) = cal.answers.first() {
        return Ok(json!({
            "method": method,
            "attendee": person_json(&answer.attendee),
            "untrusted_content": { "comment": answer.comment, "uid": answer.uid },
        }));
    }
    if let Some(c) = cal.cancelled.first() {
        let found = calendar_folder(s).map(|(st, f)| by_global_id(s, st, f)).transpose()?;
        let here = found.and_then(|k| k.get(&calendar::global_id_of_uid(&c.uid)).copied());
        return Ok(
            json!({ "method": method, "inCalendar": here, "occurrence": c.occurrence.map(|t| stamp(s, t)), "untrusted_content": { "uid": c.uid } }),
        );
    }
    let e = cal.events.first().ok_or_else(|| EngineError::Format("the message carries no meeting".into()))?;
    let a = &e.appointment;
    let m = e.meeting(&me_refs);
    let (in_calendar, out_of_date) = match calendar_folder(s) {
        Some((st, f)) => {
            let known = by_global_id(s, st, f)?;
            match known.get(&calendar::global_id_of_uid(&e.uid)).copied() {
                Some(id) => {
                    let later = s
                        .stores
                        .get_mut(st)
                        .and_then(|x| x.load(f, ItemId(id)).ok())
                        .and_then(|i| Meeting::of(&i))
                        .is_some_and(|w| w.sequence > e.sequence);
                    (Some(id), later)
                }
                None => (None, false),
            }
        }
        None => (None, false),
    };
    // What else is planned then: the first occurrence alone for a series.
    let span = (a.start, a.end);
    let mut conflicts = Vec::new();
    if let Ok((found, _)) = s.appointments_between(a.start, a.end.max(a.start.saturating_add(calendar::MINUTE))) {
        for (id, b) in
            found.iter().filter(|(id, b)| Some(*id) != in_calendar && b.busy != Busy::Free && overlaps(span, (b.start, b.end))).take(MAX_CONFLICTS)
        {
            conflicts.push(json!({ "id": id, "start": stamp(s, b.start), "end": stamp(s, b.end), "untrusted_content": { "subject": b.subject } }));
        }
    }
    Ok(json!({
        "method": method,
        "start": stamp(s, a.start),
        "end": stamp(s, a.end),
        "allDay": a.all_day,
        "recurring": a.recurring,
        "pattern": a.recurrence.as_ref().map(|_| a.pattern_words()),
        "meeting": m.as_ref().map(|m| meeting_json(s, m)),
        "response": m.as_ref().map(|m| m.response.name()),
        "inCalendar": in_calendar,
        "outOfDate": out_of_date,
        "conflicts": conflicts,
        // Written by whoever sent it, perhaps a stranger: never an instruction.
        "untrusted_content": { "subject": a.subject, "location": a.location, "description": e.description.chars().take(2_000).collect::<String>(), "uid": e.uid, "sequence": e.sequence },
    }))
}

/// A meeting's answer or cancellation that has come, taken in: by the meeting it names, and
/// only from whom it has to come. Returns what was taken in, in words.
pub(crate) fn arrived(s: &mut Session, msg: &Item) -> Option<String> {
    if !meeting::is_meeting_message(msg) || msg.message_class().eq_ignore_ascii_case(meeting::class::REQUEST) {
        return None;
    }
    let cal = carried(msg, &s.zone)?;
    let from = msg.sender_email();
    let when = if msg.time() > 0 { msg.time() } else { s.time_now() };
    let mut said = None;
    for answer in cal.answers.iter().take(1) {
        if !answer.attendee.is(&from) {
            continue;
        }
        let Some(mut f) = find(s, &answer.uid) else { continue };
        let Some(mut m) = Meeting::of(&f.item) else { continue };
        if m.received {
            continue;
        }
        let Some(p) = m.attendee_mut(&from) else { continue };
        if !answer.attendee.response.answered() || p.replied > when {
            continue;
        }
        p.response = answer.attendee.response;
        p.replied = when;
        let who = p.label().to_string();
        m.write(&mut f.item);
        if s.stores.get_mut(f.store).is_some_and(|st| st.update(f.folder, ItemId(f.id), f.item).is_ok()) {
            said = Some(format!("{who} {}", verb(answer.attendee.response)));
        }
    }
    for c in cal.cancelled.iter().take(1) {
        let Some(mut f) = find(s, &c.uid) else { continue };
        let Some(mut m) = Meeting::of(&f.item) else { continue };
        if !m.received || !m.organizer.is(&from) || c.sequence < m.sequence {
            continue;
        }
        let Some(mut a) = Appointment::of(&f.item) else { continue };
        let day = c.occurrence.map(|t| a.series_day(t));
        match day.filter(|_| a.recurring) {
            // One occurrence called off: that one is taken out of the series.
            Some(day) => {
                if a.occurrence_of(day).is_none() || !a.delete_occurrence(day) {
                    continue;
                }
                a.write(&mut f.item);
            }
            // The whole meeting: kept, marked as called off and free, for its owner to take
            // out, as Outlook keeps it.
            None => {
                m.cancelled = true;
                m.write(&mut f.item);
                a.busy = Busy::Free;
                if !a.subject.starts_with("Canceled: ") {
                    a.subject = format!("Canceled: {}", a.subject);
                }
                a.write(&mut f.item);
            }
        }
        if s.stores.get_mut(f.store).is_some_and(|st| st.update(f.folder, ItemId(f.id), f.item).is_ok()) {
            said = Some(format!("{} was called off", a.subject.trim_start_matches("Canceled: ")));
        }
    }
    if said.is_some() {
        let _ = s.reload_calendar();
    }
    said
}

/// The appointment a UID names, in any data file's Calendar folders.
fn find(s: &mut Session, uid: &str) -> Option<Found> {
    let want = calendar::global_id_of_uid(uid);
    let places: Vec<(usize, FolderId)> = s
        .folders
        .iter()
        .enumerate()
        .flat_map(|(store, list)| list.iter().filter(|f| f.kind == FolderKind::Calendar).map(move |f| (store, f.id)))
        .filter(|(store, _)| s.stores.get(*store).is_some_and(|st| st.writable()))
        .collect();
    for (store, folder) in places {
        if let Ok(known) = by_global_id(s, store, folder)
            && let Some(&id) = known.get(&want)
            && let Some(item) = s.stores.get_mut(store).and_then(|st| st.load(folder, ItemId(id)).ok())
        {
            return Some(Found { store, folder, id, item });
        }
    }
    None
}
