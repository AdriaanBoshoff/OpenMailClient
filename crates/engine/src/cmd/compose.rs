//! Compose commands: new mail, reply, reply all, forward, and the compose window's own actions.
//! Nothing here touches a network: "Send" files the message in the Outbox, and the Outbox is
//! sent by `cmd/send.rs` when there is an account to send it from.

use super::{CommandSpec, always, has_selection, need_str, ok, p_bool, p_u64};
use crate::{CmdResult, Draft, EngineError, Filed, Session, outgoing, signatures};
use openmailclient_model::zone::Offsets;
use openmailclient_model::{
    Attachment, FolderKind, Item, MSGFLAG_READ, MSGFLAG_UNSENT, PropValue, RECIP_BCC, RECIP_CC, RECIP_TO, StoreError, pid, recipient_email,
    recipient_label, time,
};
use serde_json::{Value, json};
use std::collections::HashSet;

/// One attached file is at most this large, and a draft carries at most this many.
const MAX_ATTACHMENT: u64 = 150 * 1024 * 1024;
const MAX_ATTACHMENTS: usize = 500;
/// Messages set aside that are kept; an older one is forgotten when a newer one comes.
const MAX_SET_ASIDE: usize = 20;
/// Field text is clamped: a subject line, an address line, a body.
const MAX_LINE: usize = 32 * 1024;
const MAX_BODY: usize = 16 * 1024 * 1024;

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec { id: "mail.new", label: "New Email", tab: "Home", group: "New", shortcut: "Ctrl+N", params: "", enabled: always, run: new },
        CommandSpec {
            id: "mail.reply",
            label: "Reply",
            tab: "Home",
            group: "Respond",
            shortcut: "Ctrl+R",
            params: "",
            enabled: has_selection,
            run: reply,
        },
        CommandSpec {
            id: "mail.replyAll",
            label: "Reply All",
            tab: "Home",
            group: "Respond",
            shortcut: "Ctrl+Shift+R",
            params: "",
            enabled: has_selection,
            run: reply_all,
        },
        CommandSpec {
            id: "mail.forward",
            label: "Forward",
            tab: "Home",
            group: "Respond",
            shortcut: "Ctrl+F",
            params: "",
            enabled: has_selection,
            run: forward,
        },
        CommandSpec {
            id: "compose.setField",
            label: "Set Field",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"field": "to" | "cc" | "bcc" | "subject" | "body", "value": text}"#,
            enabled: composing,
            run: set_field,
        },
        CommandSpec {
            id: "compose.signature",
            label: "Signature",
            tab: "Message",
            group: "Include",
            shortcut: "",
            params: r#"{"name": a signature's name (signatures.list), or null for none}; takes the place of the signature put in before, above the quoted text of a reply or forward and at the foot of a new message"#,
            enabled: composing,
            run: signature,
        },
        CommandSpec {
            id: "compose.showBcc",
            label: "Bcc",
            tab: "Options",
            group: "Show Fields",
            shortcut: "",
            params: r#"{"show": true | false}; default toggles"#,
            enabled: composing,
            run: show_bcc,
        },
        CommandSpec {
            id: "compose.attach",
            label: "Attach File",
            tab: "Message",
            group: "Include",
            shortcut: "",
            params: r#"{"path": file to attach}"#,
            enabled: composing,
            run: attach,
        },
        CommandSpec {
            id: "compose.removeAttachment",
            label: "Remove Attachment",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"index": attachment number}"#,
            enabled: composing,
            run: remove_attachment,
        },
        CommandSpec {
            id: "compose.check",
            label: "Check Before Sending",
            tab: "Message",
            group: "Message",
            shortcut: "",
            params: r#"returns {"cautions": what the writer should know before the message goes}"#,
            enabled: composing,
            run: check,
        },
        CommandSpec {
            id: "compose.send",
            label: "Send",
            tab: "Message",
            group: "",
            shortcut: "Ctrl+Enter",
            params: r#"{"anyway": true to send it whatever "compose.check" says, "wait": false to send on a thread of its own}; files it in the Outbox, and sends the Outbox when there is an account (see mail.sendAll)"#,
            enabled: composing,
            run: send,
        },
        CommandSpec {
            id: "compose.delayDelivery",
            label: "Delay Delivery",
            tab: "Options",
            group: "More Options",
            shortcut: "",
            params: r#"{"at": "2026-10-17 09:00" on this machine's clock, or "inMinutes": n, or "off": true}; Send leaves it in the Outbox until then, and it goes on its own at that minute; returns {"until": the time or null}"#,
            enabled: composing,
            run: delay_delivery,
        },
        CommandSpec {
            id: "compose.expires",
            label: "Expires After",
            tab: "Options",
            group: "More Options",
            shortcut: "",
            params: r#"{"at": "2026-10-24 17:00" on this machine's clock, or "inMinutes": n, or "off": true}; after then the message is of no use: it is shown struck through, recipients' mail programs are told so, and it is not sent if it is still in the Outbox; returns {"until": the time or null}"#,
            enabled: composing,
            run: expires,
        },
        CommandSpec {
            id: "compose.editDraft",
            label: "Edit Draft",
            tab: "",
            group: "",
            shortcut: "",
            params: "the selected message, which has not been sent, opens to be written on; saving it then takes its place in Drafts, and sending it takes it out of Drafts (on the server too, for an IMAP account)",
            enabled: draft_open,
            run: edit_draft,
        },
        CommandSpec {
            id: "compose.saveDraft",
            label: "Save",
            tab: "Message",
            group: "",
            shortcut: "Ctrl+S",
            params: "",
            enabled: composing,
            run: save_draft,
        },
        CommandSpec {
            id: "compose.discard",
            label: "Discard",
            tab: "Message",
            group: "",
            shortcut: "Esc",
            params: "what was written is set aside, and Undo or compose.restore brings it back",
            enabled: composing,
            run: discard,
        },
        CommandSpec {
            id: "compose.restore",
            label: "Restore Discarded Message",
            tab: "Message",
            group: "",
            shortcut: "",
            params: r#"brings back the message last discarded, or replaced by a new one, as it was; returns {"left": how many more are set aside}"#,
            enabled: has_set_aside,
            run: restore,
        },
    ]
}

fn composing(s: &Session) -> bool {
    s.compose.is_some()
}

fn draft_open(s: &Session) -> bool {
    s.open.as_ref().is_some_and(Item::is_unsent) && s.current.is_some() && s.selected.is_some()
}

fn has_set_aside(s: &Session) -> bool {
    !s.set_aside.is_empty()
}

/// Close the message being written. What its writer put into it is kept, so that starting
/// another message, or discarding this one, loses nothing. Says whether there was anything.
fn set_aside(s: &mut Session) -> bool {
    match s.compose.take() {
        Some(draft) if draft.touched => {
            if s.set_aside.len() >= MAX_SET_ASIDE {
                s.set_aside.remove(0);
            }
            s.set_aside.push(draft);
            true
        }
        _ => false,
    }
}

/// Start a new message to one mailbox, setting aside any that was being written.
pub(super) fn start_to(s: &mut Session, to: String) {
    set_aside(s);
    let mut draft = Draft { kind: "New", to, ..Draft::default() };
    sign(s, &mut draft);
    s.compose = Some(draft);
}

/// A signature's text for a message from the account `draft` goes from: `{name}` and
/// `{email}` filled with its.
fn signed_as(s: &Session, draft: &Draft, text: &str) -> String {
    let (name, email) = match from_account(s, draft) {
        Some(a) => (if a.display_name.trim().is_empty() { a.name.as_str() } else { a.display_name.as_str() }, a.email.as_str()),
        None => (s.user_name.as_str(), s.user_email.as_str()),
    };
    signatures::fill(text, name, email)
}

/// Put the signature `named` (its name and its text) in as the draft's, in place of the one put
/// in before where it is still there as it was put in: above the quoted text of a reply or
/// forward, else at the foot.
fn place_signature(draft: &mut Draft, named: Option<(String, String)>) {
    if let Some(old) = draft.signature.take() {
        let block = format!("\n\n{old}");
        if let Some(at) = draft.body.find(&block) {
            let after = draft.body.get(at.saturating_add(block.len())..).unwrap_or("");
            draft.body = format!("{}{after}", draft.body.get(..at).unwrap_or(""));
        }
    }
    draft.signature_name = None;
    let Some((name, text)) = named.filter(|(_, t)| !t.is_empty()) else { return };
    let at = draft.body.find(&format!("\n\n{}", outgoing::ORIGINAL)).unwrap_or(draft.body.len());
    let (before, after) = (draft.body.get(..at).unwrap_or(""), draft.body.get(at..).unwrap_or(""));
    draft.body = clamp(&format!("{before}\n\n{text}{after}"), MAX_BODY);
    draft.signature = Some(text);
    draft.signature_name = Some(name);
}

/// Put in the signature the account a new message, reply or forward goes from has chosen.
fn sign(s: &Session, draft: &mut Draft) {
    let id = from_account(s, draft).map_or("", |a| a.id.as_str());
    let choice = s.signatures.choice_for(id);
    let name = match draft.kind {
        "New" => choice.new.as_deref(),
        "Reply" | "Reply All" | "Forward" => choice.replies.as_deref(),
        _ => return,
    };
    let named = name.and_then(|n| s.signatures.get(n)).map(|g| (g.name.clone(), signed_as(s, draft, &g.text)));
    place_signature(draft, named);
    draft.signature_auto = true;
}

fn signature(s: &mut Session, p: &Value) -> CmdResult {
    let name = match p.get("name") {
        None | Some(Value::Null) => None,
        Some(Value::String(n)) => Some(n.as_str()),
        Some(_) => return Err(EngineError::BadParams("\"name\" is a signature's name, or null for none".into())),
    };
    let draft = s.compose.as_ref().ok_or_else(|| EngineError::Disabled("Signature".into()))?;
    let text = match name {
        None => None,
        Some(n) => {
            let g = s.signatures.get(n).ok_or_else(|| {
                EngineError::BadParams(format!("there is no signature called {}", n.chars().take(signatures::MAX_NAME).collect::<String>()))
            })?;
            Some((g.name.clone(), signed_as(s, draft, &g.text)))
        }
    };
    if let Some(d) = s.compose.as_mut() {
        place_signature(d, text);
        d.signature_auto = false;
        d.touched = true;
    }
    ok()
}

/// Start writing `draft`, setting aside any message that was being written.
pub(super) fn start_with(s: &mut Session, draft: Draft) {
    set_aside(s);
    s.compose = Some(draft);
}

fn new(s: &mut Session, _: &Value) -> CmdResult {
    set_aside(s);
    let mut draft = Draft { kind: "New", ..Draft::default() };
    sign(s, &mut draft);
    s.compose = Some(draft);
    ok()
}

/// "Name <address>", or just the address.
fn mailbox(name: &str, email: &str) -> String {
    if name.is_empty() || name == email {
        email.to_string()
    } else if email.is_empty() {
        name.to_string()
    } else {
        format!("{name} <{email}>")
    }
}

/// Add `prefix` unless the subject already starts with it (Outlook does not stack "RE: RE:").
fn prefixed(prefix: &str, subject: &str) -> String {
    let has = subject.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix));
    if has { subject.to_string() } else { format!("{prefix} {subject}").trim_end().to_string() }
}

/// The header block Outlook puts above the quoted text of a plain-text reply or forward.
fn quoted(s: &Session, item: &Item) -> String {
    let mut out = format!("\n\n{}\n", outgoing::ORIGINAL);
    out.push_str(&format!("From: {}\n", mailbox(&item.sender_name(), &item.sender_email())));
    if item.time() > 0 {
        out.push_str(&format!("Sent: {}\n", s.region.full_stamp(&s.zone.civil(item.time()))));
    }
    out.push_str(&format!("To: {}\n", item.display_recipients(RECIP_TO)));
    let cc = item.display_recipients(RECIP_CC);
    if !cc.is_empty() {
        out.push_str(&format!("Cc: {cc}\n"));
    }
    out.push_str(&format!("Subject: {}\n\n", item.subject()));
    out.push_str(&item.body_text());
    out
}

/// The recipients of `item` of one kind, each address once: one that is in `seen` already is
/// left out, and the others are added to it.
fn recipients_of(item: &Item, kind: i32, seen: &mut HashSet<String>) -> Vec<String> {
    item.recipients
        .iter()
        .filter(|r| r.i32(pid::RECIPIENT_TYPE).unwrap_or(RECIP_TO) == kind)
        .filter(|r| {
            let address = recipient_email(r).trim().to_lowercase();
            address.is_empty() || seen.insert(address)
        })
        .map(|r| mailbox(&recipient_label(r), &recipient_email(r)))
        .collect()
}

fn respond(s: &mut Session, kind: &'static str) -> CmdResult {
    let item = s.open.clone().ok_or(StoreError::NoItem)?;
    let sender = mailbox(&item.sender_name(), &item.sender_email());
    let mut draft = Draft { kind, body: quoted(s, &item), account: account_reached(s, &item), ..Draft::default() };
    match kind {
        "Forward" => {
            draft.subject = prefixed("FW:", &item.subject());
            draft.attachments = item.attachments.clone();
        }
        _ => {
            draft.subject = prefixed("RE:", &item.subject());
            (draft.in_reply_to, draft.references) = thread_of(&item);
            // The writer is the account the answer goes from, where there are accounts.
            let own = from_account(s, &draft).map_or(s.user_email.as_str(), |a| a.email.as_str()).trim().to_lowercase();
            let from = item.sender_email().trim().to_lowercase();
            // Nobody is named twice, and the writer not at all, however the message listed them.
            let mut seen: HashSet<String> = [own.clone(), from.clone()].into_iter().filter(|a| !a.is_empty()).collect();
            if !own.is_empty() && own == from {
                // An answer to one's own message goes to those it went to, not back to oneself.
                draft.to = recipients_of(&item, RECIP_TO, &mut seen).join("; ");
                if kind == "Reply All" {
                    draft.cc = recipients_of(&item, RECIP_CC, &mut seen).join("; ");
                }
            } else {
                draft.to = sender.clone();
                if kind == "Reply All" {
                    let named = |kind: i32| {
                        item.recipients
                            .iter()
                            .filter(|r| r.i32(pid::RECIPIENT_TYPE).unwrap_or(RECIP_TO) == kind)
                            .any(|r| recipient_email(r).trim().to_lowercase() == own)
                    };
                    draft.was_hidden = !own.is_empty() && !item.recipients.is_empty() && !named(RECIP_TO) && !named(RECIP_CC);
                    let others = recipients_of(&item, RECIP_TO, &mut seen);
                    if !others.is_empty() {
                        draft.to = format!("{sender}; {}", others.join("; "));
                    }
                    draft.cc = recipients_of(&item, RECIP_CC, &mut seen).join("; ");
                }
            }
        }
    }
    sign(s, &mut draft);
    set_aside(s);
    s.compose = Some(draft);
    ok()
}

fn reply(s: &mut Session, _: &Value) -> CmdResult {
    respond(s, "Reply")
}

fn reply_all(s: &mut Session, _: &Value) -> CmdResult {
    respond(s, "Reply All")
}

fn forward(s: &mut Session, _: &Value) -> CmdResult {
    respond(s, "Forward")
}

fn clamp(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("").to_string()
}

fn set_field(s: &mut Session, p: &Value) -> CmdResult {
    let field = need_str(p, "field")?.to_ascii_lowercase();
    let value = need_str(p, "value")?;
    let draft = s.compose.as_mut().ok_or_else(|| EngineError::Disabled("Set Field".into()))?;
    let one_line = clamp(&value.replace("\r\n", " ").replace(['\r', '\n'], " "), MAX_LINE);
    match field.as_str() {
        "to" => draft.to = one_line,
        "cc" => draft.cc = one_line,
        "bcc" => {
            draft.bcc = one_line;
            draft.show_bcc = true;
        }
        "subject" => draft.subject = one_line,
        "body" => draft.body = clamp(value, MAX_BODY),
        "from" | "account" => {
            // An account by its id or its address; empty for the first account.
            let wanted = value.trim();
            draft.account = if wanted.is_empty() {
                String::new()
            } else {
                s.accounts
                    .iter()
                    .find(|a| a.id == wanted || a.email.eq_ignore_ascii_case(wanted))
                    .map(|a| a.id.clone())
                    .ok_or_else(|| EngineError::BadParams(format!("there is no account {}", clamp(wanted, 64))))?
            };
        }
        other => return Err(EngineError::BadParams(format!("\"{other}\" is not to, cc, bcc, subject, body or from"))),
    }
    draft.touched = true;
    // The signature put in as the account's choice becomes the new account's.
    if matches!(field.as_str(), "from" | "account")
        && draft.signature_auto
        && let Some(mut d) = s.compose.take()
    {
        sign(s, &mut d);
        s.compose = Some(d);
    }
    ok()
}

fn show_bcc(s: &mut Session, p: &Value) -> CmdResult {
    if let Some(d) = s.compose.as_mut() {
        d.show_bcc = p_bool(p, "show").unwrap_or(!d.show_bcc);
    }
    ok()
}

fn attach(s: &mut Session, p: &Value) -> CmdResult {
    let path = std::path::Path::new(need_str(p, "path")?);
    let draft = s.compose.as_mut().ok_or_else(|| EngineError::Disabled("Attach File".into()))?;
    if draft.attachments.len() >= MAX_ATTACHMENTS {
        return Err(EngineError::Format("this message already has as many attachments as it can carry".into()));
    }
    let about = std::fs::metadata(path)?;
    if about.is_dir() {
        return Err(EngineError::BadParams(format!("{} is a folder, not a file", path.display())));
    }
    if about.len() > MAX_ATTACHMENT {
        return Err(EngineError::Format("this file is too large to attach".into()));
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "attachment".to_string());
    let data = std::fs::read(path)?;
    // The same file a second time is a slip, not a wish.
    if draft.attachments.iter().any(|a| a.embedded.is_none() && a.name() == name && a.data == data) {
        return Err(EngineError::Format(format!("{name} is attached already")));
    }
    let size = data.len();
    draft.attachments.push(Attachment::file(&name, "", data));
    draft.touched = true;
    let total = draft.attachments.iter().fold(0usize, |sum, a| sum.saturating_add(a.size()));
    Ok(json!({ "index": draft.attachments.len().saturating_sub(1), "size": size, "total": total }))
}

fn remove_attachment(s: &mut Session, p: &Value) -> CmdResult {
    let index = usize::try_from(p_u64(p, "index").unwrap_or(0)).unwrap_or(usize::MAX);
    let draft = s.compose.as_mut().ok_or_else(|| EngineError::Disabled("Remove Attachment".into()))?;
    if index >= draft.attachments.len() {
        return Err(EngineError::BadParams(format!("there is no attachment {index}")));
    }
    draft.attachments.remove(index);
    draft.touched = true;
    ok()
}

/// A time as it is given: `"at"`, a date and time on this machine's clock, or `"inMinutes"`
/// from now; `None` for `"off"`.
fn when_given(s: &Session, p: &Value) -> Result<Option<i64>, EngineError> {
    const TICKS_PER_MINUTE: i64 = 600_000_000;
    /// Five years, in minutes: further off than that is a slip.
    const MAX_MINUTES: u64 = 5 * 366 * 24 * 60;
    if p_bool(p, "off") == Some(true) {
        return Ok(None);
    }
    if let Some(m) = p_u64(p, "inMinutes") {
        let m = i64::try_from(m.clamp(1, MAX_MINUTES)).unwrap_or(1);
        return Ok(Some(super::clock(s).saturating_add(m.saturating_mul(TICKS_PER_MINUTE))));
    }
    let text =
        need_str(p, "at").map_err(|_| EngineError::BadParams("give \"at\": \"2026-10-17 09:00\", \"inMinutes\": n, or \"off\": true".into()))?;
    parse_local(text, &s.zone)
        .map(Some)
        .ok_or_else(|| EngineError::BadParams(format!("\"{}\" is not a date and time such as 2026-10-17 09:00", clamp(text, 64))))
}

/// "2026-10-17 09:00", "2026-10-17T09:00:30": a date and time where `here` is (a zone, or
/// minutes ahead of UTC).
pub(crate) fn parse_local(text: &str, here: impl Offsets) -> Option<i64> {
    let (date, at) = text.trim().split_once(['T', ' '])?;
    let mut d = date.split('-');
    let (year, month, day) = (d.next()?.parse::<i64>().ok()?, d.next()?.parse::<u32>().ok()?, d.next()?.parse::<u32>().ok()?);
    let mut c = at.trim().split(':');
    let (hour, minute) = (c.next()?.parse::<u32>().ok()?, c.next()?.parse::<u32>().ok()?);
    let second = c.next().map_or(Some(0), |v| v.parse::<u32>().ok())?;
    if d.next().is_some() || c.next().is_some() || !(1601..=9999).contains(&year) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let local = time::from_civil(year, month, day, hour, minute, second, 0);
    // A day the month does not have (31 April) comes back as another day: not a date.
    let back = time::civil(local, 0);
    (back.month == month && back.day == day).then(|| here.utc_of(local))
}

/// A time as the reading pane says it: "Fri 10/17/2026 9:00 AM", on this machine's clock.
pub(crate) fn when_shown(s: &Session, t: i64) -> String {
    s.region.stamp(&s.zone.civil(t))
}

impl Session {
    /// A time as the reading pane says it, on this machine's clock.
    pub fn time_shown(&self, t: i64) -> String {
        when_shown(self, t)
    }

    /// Bring the session's clock up to the machine's; it never goes back. A window that stays
    /// open past midnight says "Today" of the new day, and sees a message expire when it does.
    pub fn tick_clock(&mut self) {
        self.now = super::clock(self);
    }
}

fn delay_delivery(s: &mut Session, p: &Value) -> CmdResult {
    let at = when_given(s, p)?;
    let now = super::clock(s);
    let draft = s.compose.as_ref().ok_or_else(|| EngineError::Disabled("Delay Delivery".into()))?;
    if let Some(at) = at {
        if at <= now {
            return Err(EngineError::BadParams(format!("{} has gone by already", when_shown(s, at))));
        }
        if draft.expires > 0 && at >= draft.expires {
            return Err(EngineError::BadParams(format!("it would expire ({}) before it went", when_shown(s, draft.expires))));
        }
    }
    let shown = at.map(|t| when_shown(s, t));
    if let Some(d) = s.compose.as_mut() {
        d.deliver_after = at.unwrap_or(0);
        d.touched = true;
    }
    s.status = match &shown {
        Some(t) => format!("Send leaves it in the Outbox until {t}, and it goes on its own then"),
        None => "It goes as soon as it is sent".to_string(),
    };
    Ok(json!({ "until": shown }))
}

fn expires(s: &mut Session, p: &Value) -> CmdResult {
    let at = when_given(s, p)?;
    let now = super::clock(s);
    let draft = s.compose.as_ref().ok_or_else(|| EngineError::Disabled("Expires After".into()))?;
    if let Some(at) = at {
        if at <= now {
            return Err(EngineError::BadParams(format!("{} has gone by already", when_shown(s, at))));
        }
        if draft.deliver_after > 0 && at <= draft.deliver_after {
            return Err(EngineError::BadParams(format!("it would expire before it went ({})", when_shown(s, draft.deliver_after))));
        }
    }
    let shown = at.map(|t| when_shown(s, t));
    if let Some(d) = s.compose.as_mut() {
        d.expires = at.unwrap_or(0);
        d.touched = true;
    }
    s.status = match &shown {
        Some(t) => format!("It expires {t}"),
        None => "It does not expire".to_string(),
    };
    Ok(json!({ "until": shown }))
}

/// Turn the draft into an item. `sent` stamps the submit time and clears the unsent flag.
pub(super) fn build(s: &Session, draft: &Draft, sent: bool) -> Item {
    let mut item = Item::new_mail();
    item.set_subject(&draft.subject);
    item.set_body_text(&draft.body);
    match from_account(s, draft) {
        Some(a) => item.set_sender(&a.display_name, &a.email),
        None => item.set_sender(&s.user_name, &s.user_email),
    }
    for (id, text) in [(pid::IN_REPLY_TO_ID, &draft.in_reply_to), (pid::INTERNET_REFERENCES, &draft.references)] {
        if !text.is_empty() {
            item.props.set(id, PropValue::String(text.clone()));
        }
    }
    for (kind, line) in [(RECIP_TO, &draft.to), (RECIP_CC, &draft.cc), (RECIP_BCC, &draft.bcc)] {
        for (name, email) in openmailclient_mime::parse_addresses(line) {
            item.add_recipient(kind, &name, &email);
        }
    }
    item.refresh_display_recipients();
    item.attachments = draft.attachments.clone();
    for (id, t) in [(pid::DEFERRED_DELIVERY_TIME, draft.deliver_after), (pid::EXPIRY_TIME, draft.expires)] {
        if t > 0 {
            item.props.set(id, PropValue::Time(t));
        }
    }
    item.props.set(pid::CREATION_TIME, PropValue::Time(s.now));
    item.props.set(pid::LAST_MODIFICATION_TIME, PropValue::Time(s.now));
    if sent {
        item.props.set(pid::CLIENT_SUBMIT_TIME, PropValue::Time(s.now));
        item.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(MSGFLAG_READ));
    } else {
        item.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(MSGFLAG_READ | MSGFLAG_UNSENT));
    }
    item
}

/// The account a draft goes from: the one it names, or the first.
fn from_account<'a>(s: &'a Session, draft: &Draft) -> Option<&'a crate::accounts::Account> {
    s.accounts.iter().find(|a| a.id == draft.account).or_else(|| s.accounts.first())
}

/// The account a message reached: the first whose address it went to, or that sent it.
fn account_reached(s: &Session, item: &Item) -> String {
    let to: Vec<String> = item.recipients.iter().map(recipient_email).chain([item.sender_email()]).collect();
    s.accounts.iter().find(|a| to.iter().any(|t| t.trim().eq_ignore_ascii_case(&a.email))).map(|a| a.id.clone()).unwrap_or_default()
}

/// The headers of an answer that name what it answers ([RFC 5322] 3.6.4): the message's own
/// id, after those it named itself, of which no more than the last 20 are kept.
fn thread_of(item: &Item) -> (String, String) {
    let Some(id) = item.props.str(pid::INTERNET_MESSAGE_ID).map(str::trim).filter(|v| v.starts_with('<') && v.is_ascii()) else {
        return (String::new(), String::new());
    };
    let before = item.props.str(pid::INTERNET_REFERENCES).or(item.props.str(pid::IN_REPLY_TO_ID)).unwrap_or_default();
    let mut all: Vec<&str> = before.split_whitespace().filter(|r| r.starts_with('<') && r.ends_with('>') && r.is_ascii()).collect();
    all.push(id);
    let from = all.len().saturating_sub(20);
    (id.to_string(), all.get(from..).unwrap_or_default().join(" "))
}

/// The path a data file is known by, or nothing for one kept only in memory.
fn path_of(s: &Session, store: usize) -> String {
    s.stores.get(store).and_then(|st| st.path()).map(|p| p.display().to_string()).unwrap_or_default()
}

/// The data file a draft is kept in: the one of the account it goes from, as Outlook keeps a
/// Drafts folder for each account, or the one new mail goes to.
pub(super) fn drafts_store(s: &mut Session, draft: &Draft) -> Result<Option<usize>, EngineError> {
    let at = s.accounts.iter().position(|a| a.id == draft.account).or_else(|| (!s.accounts.is_empty()).then_some(0));
    if let Some(at) = at
        && let Some(store) = super::account::account_store(s, at)?
        && s.stores.get(store).is_some_and(|st| st.writable())
    {
        return Ok(Some(store));
    }
    Ok(s.writable_store())
}

/// File what is being written in the Drafts folder or the Outbox. The draft it was opened from,
/// or saved as before, makes way for it once it is kept: taken out for good, and from the IMAP
/// server too at the next Send/Receive. Saved to Drafts, it stays open to be written on.
fn file_draft(s: &mut Session, kind: FolderKind, sent: bool) -> Result<(), EngineError> {
    let draft = s.compose.clone().ok_or_else(|| EngineError::Disabled("Send".into()))?;
    let store = match kind {
        FolderKind::Drafts => drafts_store(s, &draft)?,
        _ => s.writable_store(),
    }
    .ok_or_else(|| EngineError::Format("there is no writable data file to keep the message in".into()))?;
    let folder = s.find_folder(store, kind).ok_or_else(|| EngineError::Format(format!("this data file has no {kind:?} folder")))?;
    let item = build(s, &draft, sent);
    let id = s.stores.get_mut(store).ok_or(StoreError::NoFolder)?.add(folder, item)?;
    if let Some(was) = &draft.filed
        && !(was.store == store && was.folder == folder && was.item == id)
    {
        take_out(s, was);
    }
    if kind == FolderKind::Drafts {
        let path = path_of(s, store);
        if let Some(d) = s.compose.as_mut() {
            d.filed = Some(Filed { path, store, folder, item: id });
            d.touched = false;
        }
    } else {
        s.compose = None;
    }
    s.refresh_folders(store);
    if let Some(was) = draft.filed.as_ref().filter(|w| w.store != store) {
        s.refresh_folders(was.store);
    }
    s.reload_rows()
}

/// A draft kept before, taken out for good where it still is and still a draft: never another
/// message that came to have its place.
fn take_out(s: &mut Session, was: &Filed) {
    if path_of(s, was.store) != was.path {
        return;
    }
    let Some(st) = s.stores.get_mut(was.store) else { return };
    if !st.load(was.folder, was.item).is_ok_and(|i| i.is_unsent()) {
        return;
    }
    if let Ok(item) = st.remove(was.folder, was.item) {
        super::imap_sync::forget(s, was.store, &item);
        if s.current == Some((was.store, was.folder)) && s.selected == Some(was.item.0) {
            s.selected = None;
            s.open = None;
        }
    }
}

/// Open the selected draft to be written on, as a double-click on it does.
fn edit_draft(s: &mut Session, _: &Value) -> CmdResult {
    let item = s.open.clone().filter(Item::is_unsent).ok_or_else(|| EngineError::Disabled("Edit Draft".into()))?;
    let ((store, folder), id) = s.current.zip(s.selected).ok_or(StoreError::NoItem)?;
    let path = path_of(s, store);
    // The account it goes from: the one it is written from, or the one whose data file has it.
    let sender = item.sender_email().trim().to_lowercase();
    let account = s
        .accounts
        .iter()
        .find(|a| !sender.is_empty() && a.email.trim().eq_ignore_ascii_case(&sender))
        .or_else(|| s.accounts.iter().find(|a| !path.is_empty() && a.received.into == path))
        .map(|a| a.id.clone())
        .unwrap_or_default();
    let line = |kind: i32| {
        let mut seen = HashSet::new();
        recipients_of(&item, kind, &mut seen).join("; ")
    };
    let text = |id: u16| item.props.str(id).map(|t| clamp(t, MAX_LINE)).unwrap_or_default();
    let time = |id: u16| item.props.time(id).filter(|t| *t > 0).unwrap_or(0);
    let (to, cc, bcc) = (line(RECIP_TO), line(RECIP_CC), line(RECIP_BCC));
    let draft = Draft {
        show_bcc: !bcc.is_empty(),
        to,
        cc,
        bcc,
        subject: clamp(&item.subject(), MAX_LINE),
        body: clamp(&item.body_text(), MAX_BODY),
        attachments: item.attachments.iter().take(MAX_ATTACHMENTS).cloned().collect(),
        kind: "Draft",
        account,
        in_reply_to: text(pid::IN_REPLY_TO_ID),
        references: text(pid::INTERNET_REFERENCES),
        deliver_after: time(pid::DEFERRED_DELIVERY_TIME),
        expires: time(pid::EXPIRY_TIME),
        filed: Some(Filed { path, store, folder, item: openmailclient_model::ItemId(id) }),
        ..Draft::default()
    };
    set_aside(s);
    s.compose = Some(draft);
    ok()
}

fn check(s: &mut Session, _: &Value) -> CmdResult {
    let draft = s.compose.as_ref().ok_or_else(|| EngineError::Disabled("Check Before Sending".into()))?;
    Ok(json!({"cautions": outgoing::cautions(draft).iter().map(|c| c.words()).collect::<Vec<_>>()}))
}

fn send(s: &mut Session, p: &Value) -> CmdResult {
    let has_recipient =
        s.compose.as_ref().is_some_and(|d| [&d.to, &d.cc, &d.bcc].iter().any(|l| !openmailclient_mime::parse_addresses(l).is_empty()));
    if !has_recipient {
        return Err(EngineError::Format("there must be at least one name or address in the To, Cc or Bcc box".into()));
    }
    // As Outlook does on Send, a name the address book has once is put in as its address; one
    // it has several times or not at all is left for the cautions below to speak of.
    let _ = super::names::resolve_draft(s, &["to", "cc", "bcc"], true);
    // What the writer should know is said before the message goes, not after; it goes as it is
    // when that is asked for.
    let cautions = s.compose.as_ref().map(outgoing::cautions).unwrap_or_default();
    if !cautions.is_empty() && !p_bool(p, "anyway").unwrap_or(false) {
        let words: Vec<String> = cautions.iter().map(|c| c.words()).collect();
        return Err(EngineError::Format(format!("{} It was not sent; send it with \"anyway\": true to send it as it is.", words.join(" "))));
    }
    file_draft(s, FolderKind::Outbox, true)?;
    if s.accounts.is_empty() {
        s.status = "The message is in the Outbox; it goes when an account is added (account.add) and Send All is asked for".to_string();
        return ok();
    }
    if s.connector.is_none() {
        s.status = "The message is in the Outbox; this build cannot reach mail servers".to_string();
        return ok();
    }
    // The message is kept whatever comes of sending it.
    super::send::send_outbox(s, p_bool(p, "wait") != Some(false)).or_else(|e| {
        s.status = format!("The message is in the Outbox and was not sent: {e}");
        ok()
    })
}

fn save_draft(s: &mut Session, _: &Value) -> CmdResult {
    file_draft(s, FolderKind::Drafts, false)?;
    s.status = "Saved to Drafts".to_string();
    ok()
}

fn discard(s: &mut Session, _: &Value) -> CmdResult {
    if set_aside(s) {
        s.did(crate::undo::Step::Discarded);
        s.status = "Discarded. Undo (Ctrl+Z) brings it back.".to_string();
    }
    ok()
}

fn restore(s: &mut Session, _: &Value) -> CmdResult {
    let draft = s.set_aside.pop().ok_or_else(|| EngineError::Disabled("Restore Discarded Message".into()))?;
    // What is being written now makes way, and is kept in its turn.
    set_aside(s);
    s.compose = Some(draft);
    Ok(json!({ "left": s.set_aside.len() }))
}
