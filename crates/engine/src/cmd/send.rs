//! Sending: what is in the Outbox goes out through its account's outgoing server, on a thread
//! of its own, and what went out is kept in Sent Items. Nothing is sent unless it was put in
//! the Outbox to be sent; what the server did not take stays in the Outbox with the reason.
//! An IMAP account puts a copy of what went in the server's Sent folder too, as the owner's
//! other mail programs expect (`Account::sent_on_server`); the receive that next reads that
//! folder knows those copies by their Message-ID and does not keep them twice.
//!
//! The work on the thread gets copies of the messages as they are to go, written on the
//! session's thread before it starts; the mailbox is changed only when the job is taken back
//! by `Session::poll`.

use super::account::Reach;
use super::imap_sync::Place;
use super::{CommandSpec, p_bool};
use crate::accounts::Account;
use crate::jobs::Ended;
use crate::{CmdResult, EngineError, Session};
use openmailclient_model::{FolderId, FolderKind, Item, ItemId, MSGFLAG_READ, MSGFLAG_UNSENT, PropValue, RECIP_BCC, pid, recipient_email};
use openmailclient_net::NetError;
use openmailclient_net::imap::Imap;
use openmailclient_net::smtp::Smtp;
use serde_json::{Value, json};

/// Messages taken from the Outbox in one go; the rest wait for the next.
const MAX_AT_ONCE: usize = 500;

fn can_send(s: &Session) -> bool {
    !s.accounts.is_empty()
}

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "mail.sendAll",
        label: "Send All",
        tab: "Send / Receive",
        group: "Send & Receive",
        shortcut: "",
        params: r#"{"wait": false to send on a thread of its own}; sends what is in the Outbox, each message from the account its sender names (the first account otherwise), and keeps what went in Sent Items; returns {"sent", "failed": [{"subject", "said"}], "refused": [{"subject", "to", "said"}]}"#,
        enabled: can_send,
        run: send_all,
    }]
}

fn send_all(s: &mut Session, p: &Value) -> CmdResult {
    send_outbox(s, p_bool(p, "wait") != Some(false))
}

/// A message on its way: where it is, what goes on the wire and what is kept once it went.
struct Going {
    /// The account it goes from, whose own data file keeps it once it went.
    account: String,
    folder: FolderId,
    id: u64,
    subject: String,
    /// The envelope: the account's address, and everyone the message is for, Bcc too.
    from: String,
    to: Vec<String>,
    /// The message as it goes out: no Bcc line, so hidden recipients stay hidden.
    data: Vec<u8>,
    /// The message as Sent Items keeps it: every recipient, stamped as sent.
    kept: Item,
}

/// Messages from one account, said over one connection.
struct Batch {
    reach: Reach,
    going: Vec<Going>,
    /// A copy of what went is put in the server's Sent folder.
    to_server: bool,
}

/// How putting the copies of what went in an account's Sent folder on the server went.
struct Copies {
    account: String,
    put: Result<usize, NetError>,
}

/// Put a copy of each message that went in the server's Sent folder, read, made when the
/// server has none.
fn put_in_sent(reach: &Reach, copies: &[Vec<u8>]) -> Result<usize, NetError> {
    let login = reach.login()?;
    let mut imap = Imap::open(&*reach.connector, &reach.account.incoming_endpoint(), &login)?;
    let folders = imap.folders()?;
    let sent = folders.iter().filter(|f| f.selectable()).find(|f| f.has("\\Sent")).or_else(|| {
        folders.iter().filter(|f| f.selectable()).find(|f| super::receive::kind_of(f, &super::receive::path_of(f)) == Some(FolderKind::Sent))
    });
    let raw = match sent {
        Some(f) => f.raw.clone(),
        None => {
            imap.create("Sent")?;
            "Sent".to_string()
        }
    };
    let mut put = 0;
    for copy in copies {
        imap.append(&raw, &["\\Seen"], copy)?;
        put += 1;
    }
    imap.logout()?;
    Ok(put)
}

/// What came of one message.
type Outcome = Result<openmailclient_net::smtp::Sent, NetError>;

/// The time now, as the store keeps times; the session's own clock where there is none.
pub(crate) fn clock(s: &Session) -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .map_or(s.now, openmailclient_model::time::from_unix)
        .max(s.now)
}

/// The account a message goes from: the one whose address is its sender's, or the first.
pub(crate) fn account_for<'a>(s: &'a Session, sender: &str) -> Option<&'a Account> {
    let sender = sender.trim();
    s.accounts.iter().find(|a| a.email.eq_ignore_ascii_case(sender)).or_else(|| s.accounts.first())
}

/// A Message-ID no other message has ([RFC 5322] 3.6.4): the time, a count and random bits,
/// at the sending address's domain.
fn message_id(now: i64, n: usize, email: &str) -> String {
    let id = crate::io::new_store_id(now);
    let random: String = id.iter().take(8).map(|b| format!("{b:02x}")).collect();
    let domain = email.rsplit_once('@').map(|(_, d)| d).filter(|d| !d.is_empty() && d.is_ascii()).unwrap_or("openmailclient.invalid");
    format!("<{:x}.{n:x}.{random}@{domain}>", openmailclient_model::time::to_unix(now).max(0))
}

/// The message as it is to go, from `account`.
fn prepare(item: &Item, account: &Account, now: i64, n: usize) -> (Vec<String>, Vec<u8>, Item) {
    let mut kept = item.clone();
    kept.set_sender(&account.display_name, &account.email);
    if kept.props.str(pid::INTERNET_MESSAGE_ID).is_none_or(|v| v.trim().is_empty()) {
        kept.props.set(pid::INTERNET_MESSAGE_ID, PropValue::String(message_id(now, n, &account.email)));
    }
    kept.props.set(pid::CLIENT_SUBMIT_TIME, PropValue::Time(now));
    kept.props.set(pid::MESSAGE_DELIVERY_TIME, PropValue::Time(now));
    let flags = kept.props.i32(pid::MESSAGE_FLAGS).unwrap_or(0);
    kept.props.set(pid::MESSAGE_FLAGS, PropValue::Int32((flags | MSGFLAG_READ) & !MSGFLAG_UNSENT));
    let mut to: Vec<String> = Vec::new();
    for r in &kept.recipients {
        let email = recipient_email(r).trim().to_string();
        if !email.is_empty() && !to.iter().any(|t| t.eq_ignore_ascii_case(&email)) {
            to.push(email);
        }
    }
    let mut wire = kept.clone();
    wire.recipients.retain(|r| r.i32(pid::RECIPIENT_TYPE) != Some(RECIP_BCC));
    (to, openmailclient_mime::write(&wire), kept)
}

/// When the first message held in the Outbox by Delay Delivery may go, of those whose time is
/// after `after` (any time when `None`). Once its time has come and a send was tried, a message
/// is one like any other in the Outbox, sent by the next Send/Receive and not tried again and
/// again on its own.
pub(crate) fn held_in_outbox(s: &mut Session, after: Option<i64>) -> Option<i64> {
    let (store, folder) = outbox(s)?;
    let st = s.stores.get_mut(store)?;
    let rows = st.list(folder).ok()?;
    let mut first: Option<i64> = None;
    for r in rows.iter().take(MAX_AT_ONCE).filter(|r| r.deliver_after > after.unwrap_or(0)) {
        // Read to be sure: one that went already, or would have expired first, is not waited for.
        let Ok(item) = st.load(folder, ItemId(r.id)) else { continue };
        let at = item.deliver_after();
        let gone = item.props.time(pid::MESSAGE_DELIVERY_TIME).is_some_and(|t| t > 0);
        if at > after.unwrap_or(0) && !gone && !(item.expires() > 0 && item.expires() <= at) {
            first = Some(first.map_or(at, |f| f.min(at)));
        }
    }
    first
}

/// The store and Outbox messages are sent from: the one new mail is kept in.
fn outbox(s: &Session) -> Option<(usize, FolderId)> {
    let store = s.writable_store()?;
    Some((store, s.find_folder(store, FolderKind::Outbox)?))
}

/// Send what is in the Outbox: on a thread of its own unless `wait`. When a send is already
/// under way, the Outbox is sent again once it ends.
pub(crate) fn send_outbox(s: &mut Session, wait: bool) -> CmdResult {
    let sent = send_now(s, wait);
    // What still waits for its time (Delay Delivery) goes on its own when that comes.
    let now = clock(s);
    let held = held_in_outbox(s, Some(now));
    s.held_changed(held);
    sent
}

fn send_now(s: &mut Session, wait: bool) -> CmdResult {
    if s.accounts.is_empty() {
        return Err(EngineError::Format("there is no account to send from; add one with account.add".into()));
    }
    if s.jobs.iter().any(|j| j.what.starts_with(SENDING)) {
        s.send_again = true;
        s.status = "The message waits in the Outbox and goes when the send under way ends".to_string();
        return Ok(json!({ "queued": true }));
    }
    let (store, folder) = outbox(s).ok_or_else(|| EngineError::Format("there is no Outbox in a data file that can be written".into()))?;
    let now = clock(s);
    let summaries = s.stores.get_mut(store).ok_or(openmailclient_model::StoreError::NoFolder)?.list(folder)?;
    let mut batches: Vec<Batch> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    let mut waiting: Vec<Value> = Vec::new();
    for (n, summary) in summaries.iter().take(MAX_AT_ONCE).enumerate() {
        let Some(st) = s.stores.get_mut(store) else { break };
        let item = match st.load(folder, ItemId(summary.id)) {
            Ok(item) => item,
            Err(e) => {
                skipped.push(json!({ "subject": summary.subject, "said": e.to_string() }));
                continue;
            }
        };
        // What went before and could not be kept in Sent Items is not sent again.
        if item.props.time(pid::MESSAGE_DELIVERY_TIME).is_some_and(|t| t > 0) {
            skipped.push(json!({ "subject": summary.subject, "said": "it was sent already" }));
            continue;
        }
        // Delay Delivery: it waits for its time, and goes on its own then.
        if item.deliver_after() > now {
            waiting.push(json!({ "subject": summary.subject, "until": super::compose::when_shown(s, item.deliver_after()) }));
            continue;
        }
        // Of no use any more by the time it would go: it is not sent, and stays to be seen to.
        if item.expires() > 0 && item.expires() <= now {
            skipped.push(json!({ "subject": summary.subject, "said": "it expired before it went; change when it expires, or delete it" }));
            continue;
        }
        // Something that is not a message or a meeting's request, answer or cancellation (a
        // task request, say) waits; a meeting's goes with its calendar, [MS-OXOCAL] 2.2.6.
        let class = item.message_class().to_ascii_uppercase();
        if !(class.is_empty() || class.starts_with("IPM.NOTE") || class.starts_with("IPM.SCHEDULE.MEETING.")) {
            skipped.push(json!({ "subject": summary.subject, "said": format!("a {} is not sent by this version", item.message_class()) }));
            continue;
        }
        let Some(at) = account_for(s, &item.sender_email()).and_then(|a| s.accounts.iter().position(|b| b.id == a.id)) else { continue };
        let Some(account) = s.accounts.get(at) else { continue };
        let (to, data, kept) = prepare(&item, account, now, n);
        let going = Going {
            account: account.id.clone(),
            folder,
            id: summary.id,
            subject: summary.subject.clone(),
            from: account.email.clone(),
            to,
            data,
            kept,
        };
        match batches.iter_mut().find(|b| b.reach.account.id == account.id) {
            Some(b) => b.going.push(going),
            None => batches.push(Batch {
                reach: Reach::of(s, at)?,
                going: vec![going],
                to_server: account.kind == crate::accounts::Kind::Imap && account.sent_on_server,
            }),
        }
    }
    let count: usize = batches.iter().map(|b| b.going.len()).sum();
    if count == 0 {
        s.status = match (skipped.is_empty(), waiting.as_slice()) {
            (true, []) => "There is nothing in the Outbox to send".to_string(),
            (true, [one]) => format!("It waits in the Outbox until {} and goes then", one["until"].as_str().unwrap_or_default()),
            (true, many) => format!("{} messages wait in the Outbox for their time and go then", many.len()),
            (false, _) => format!("Nothing in the Outbox could be sent: {}", skipped.first().and_then(|f| f["said"].as_str()).unwrap_or_default()),
        };
        return Ok(json!({ "sent": 0, "failed": skipped, "refused": [], "waiting": waiting }));
    }
    // Known before the copies are on the server, so a receive running meanwhile already
    // passes over them.
    for batch in batches.iter().filter(|b| b.to_server) {
        if let Some(a) = s.accounts.iter_mut().find(|a| a.id == batch.reach.account.id) {
            for going in &batch.going {
                a.received.put_in_sent(going.kept.props.str(pid::INTERNET_MESSAGE_ID).unwrap_or_default());
            }
        }
    }
    let what = format!("{SENDING} {count} message{}", if count == 1 { "" } else { "s" });
    s.begin(what, wait, move |progress| {
        progress.begin(u64::try_from(count).unwrap_or(u64::MAX));
        let mut done: Vec<(Going, Outcome)> = Vec::new();
        let mut copies: Vec<Copies> = Vec::new();
        for batch in batches {
            let first = done.len();
            let mut smtp = batch
                .reach
                .account
                .sign_in_to_send
                .then(|| batch.reach.login())
                .transpose()
                .and_then(|login| Smtp::open(&*batch.reach.connector, &batch.reach.account.outgoing_endpoint(), login.as_ref()));
            for going in batch.going {
                let outcome = if progress.stopped() {
                    Err(NetError::Unsupported("the send was stopped".into()))
                } else {
                    match &mut smtp {
                        Ok(c) => c.send(&going.from, &going.to, &going.data),
                        Err(e) => Err(NetError::Unsupported(e.to_string())),
                    }
                };
                done.push((going, outcome));
                progress.step();
            }
            if let Ok(c) = smtp {
                let _ = c.quit();
            }
            // What went, as Sent Items keeps it: Bcc and all, as only the sender sees it.
            let went: Vec<Vec<u8>> =
                done.get(first..).unwrap_or_default().iter().filter(|(_, o)| o.is_ok()).map(|(g, _)| openmailclient_mime::write(&g.kept)).collect();
            if batch.to_server && !went.is_empty() {
                copies.push(Copies { account: batch.reach.account.name.clone(), put: put_in_sent(&batch.reach, &went) });
            }
        }
        let step: Ended = Box::new(move |s: &mut Session| ended(s, store, done, skipped, copies, waiting));
        step
    })
}

/// The words a send job begins with; one is under way while a job so named is.
pub(crate) const SENDING: &str = "Sending";

/// Take back what a send came to: what went is kept in Sent Items and leaves the Outbox; what
/// did not stays, with the reason said.
fn ended(
    s: &mut Session,
    store: usize,
    done: Vec<(Going, Outcome)>,
    mut failed: Vec<Value>,
    copies: Vec<Copies>,
    waiting: Vec<Value>,
) -> (String, Value) {
    let sent_folder = s.find_folder(store, FolderKind::Sent);
    // Each account's Sent Items: in its own data file where it has one, as Outlook keeps it.
    let mut kept_in: Vec<(String, Option<(usize, FolderId)>)> = Vec::new();
    let mut touched: Vec<usize> = Vec::new();
    let mut sent = 0usize;
    let mut refused: Vec<Value> = Vec::new();
    let mut unkept = 0usize;
    for (going, outcome) in done {
        match outcome {
            Ok(said) => {
                sent += 1;
                for (to, why) in said.refused {
                    refused.push(json!({ "subject": going.subject, "to": to, "said": why }));
                }
                let own = match kept_in.iter().find(|(id, _)| *id == going.account) {
                    Some((_, own)) => *own,
                    None => {
                        let own = s
                            .accounts
                            .iter()
                            .position(|a| a.id == going.account)
                            .and_then(|at| super::account::account_store(s, at).ok().flatten())
                            .filter(|&at| at != store)
                            .and_then(|at| Some((at, s.find_folder(at, FolderKind::Sent)?)));
                        kept_in.push((going.account.clone(), own));
                        own
                    }
                };
                // Kept first, then taken out of the Outbox: a message that went is never lost.
                // Where there is no Sent Items it stays in the Outbox, marked as gone, and is
                // not sent again.
                let kept = match own {
                    Some((at, f)) => {
                        let mut copy = going.kept.clone();
                        // An IMAP account's copy is found on the server's Sent folder by its
                        // Message-ID when one was put there; otherwise it stays here only and is
                        // not put there later as if it were new.
                        if let Some(a) = s.accounts.iter().find(|a| a.id == going.account && a.kind == crate::accounts::Kind::Imap) {
                            let place = if a.sent_on_server { Place::Pending { raw: String::new() } } else { Place::Here };
                            place.put(&mut copy);
                        }
                        let kept = s.stores.get_mut(at).is_some_and(|st| st.add(f, copy).is_ok());
                        if kept && !touched.contains(&at) {
                            touched.push(at);
                        }
                        kept
                    }
                    None => sent_folder.is_some_and(|f| s.stores.get_mut(store).is_some_and(|st| st.add(f, going.kept.clone()).is_ok())),
                };
                let Some(st) = s.stores.get_mut(store) else {
                    unkept += 1;
                    continue;
                };
                if kept {
                    // Already taken out (by its writer, while it went) is as good.
                    let _ = st.remove(going.folder, ItemId(going.id));
                } else {
                    unkept += 1;
                    let _ = st.update(going.folder, ItemId(going.id), going.kept);
                }
            }
            Err(e) => failed.push(json!({ "subject": going.subject, "said": e.to_string() })),
        }
    }
    s.refresh_folders(store);
    for &at in &touched {
        s.refresh_folders(at);
    }
    let _ = s.reload_rows();
    // An account's own data file is saved once what went is in it, on a thread of its own.
    for &at in &touched {
        let status = std::mem::take(&mut s.status);
        let _ = super::file::save_in_background(s, at);
        s.status = status;
    }
    let mut status = match (sent, failed.len()) {
        (0, _) => format!("Nothing was sent: {}", failed.first().and_then(|f| f["said"].as_str()).unwrap_or("the server did not take it")),
        (n, 0) => format!("Sent {n} message{}", if n == 1 { "" } else { "s" }),
        (n, f) => format!("Sent {n}; {f} stayed in the Outbox: {}", failed.first().and_then(|f| f["said"].as_str()).unwrap_or_default()),
    };
    if !refused.is_empty() {
        status.push_str(&format!("; {} recipient{} not taken by the server", refused.len(), if refused.len() == 1 { " was" } else { "s were" }));
    }
    if unkept > 0 {
        status.push_str("; what went could not be kept in Sent Items and stays in the Outbox, marked as sent");
    }
    match waiting.as_slice() {
        [] => {}
        [one] => status.push_str(&format!("; one waits in the Outbox until {}", one["until"].as_str().unwrap_or_default())),
        many => status.push_str(&format!("; {} wait in the Outbox for their time", many.len())),
    }
    if std::mem::take(&mut s.send_again) && send_outbox(s, false).is_ok_and(|started| started["started"] == true) {
        status.push_str("; sending what came into the Outbox meanwhile");
    }
    let mut on_server = 0usize;
    for c in &copies {
        match &c.put {
            Ok(n) => on_server += n,
            // What went has gone: only the server's copy is missing, and that is said.
            Err(e) => status.push_str(&format!("; {}: a copy could not be put in the server's Sent folder ({e})", c.account)),
        }
    }
    (status, json!({ "sent": sent, "failed": failed, "refused": refused, "onServer": on_server, "waiting": waiting }))
}
