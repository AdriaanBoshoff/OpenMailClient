//! Receiving: new mail is taken down from each account's incoming server into the Inbox of the
//! account's own data file (`account::account_store`), or, where the host keeps none, of the
//! data file new mail is kept in, on a thread of its own. An IMAP account with a data file of
//! its own has every folder of the server there: Sent, Drafts, Deleted Items, Junk Email and
//! Archive are the ones the server marks so ([RFC 6154]) or names so, and any other is made
//! with the same name and place. An account's own data file is saved once mail has come into
//! it. Nothing is taken down twice: an
//! account remembers what it took (`accounts::Received`), and that is moved on only once the
//! message is in the Inbox.
//!
//! The server is changed as little as can be: IMAP messages are read without marking them read
//! (`BODY.PEEK`), and a POP3 message is taken off the server, when the account says so, only on
//! a later receive, once the data file it went into has been saved; a message is never removed
//! from a server before it is kept here.
//!
//! While a receive runs, what has come is handed back part by part (`Parts`) and kept as it
//! comes, so the mail is seen as it arrives and only a part is ever held; an account's own
//! data file is saved every `SAVE_EVERY` bytes on the way. Into an account's own data file,
//! where each message knows its place on the server, each folder comes newest first: what was
//! not come to is known by its place and taken next time.

use super::account::Reach;
use super::imap_sync::{self, Local, Map, Place, Synced};
use super::{CommandSpec, p_bool};
use crate::accounts::{Kind, MAX_FOLDERS, MAX_SEEN, Mark, Received, Step};
use crate::jobs::Ended;
use crate::{CmdResult, EngineError, Session};
use openmailclient_model::{Attachment, Flag, FolderId, FolderKind, Item, MSGFLAG_READ, PropValue, pid};
use openmailclient_net::NetError;
use openmailclient_net::imap::{Folder, Imap, Selected, Summary};
use openmailclient_net::pop3::Pop3;
use serde_json::{Value, json};
use std::sync::Arc;

/// Messages taken down from one account in one receive, and how many bytes of them; the rest
/// come with the next.
const MAX_PER_RUN: usize = 300;
const MAX_BYTES: u64 = 256 * 1024 * 1024;
/// The same where what comes is kept part by part, so that little is held at once: a
/// mailbox of any size comes down in one receive.
const MAX_PER_RUN_IN_PARTS: usize = 2_000_000;
const MAX_BYTES_IN_PARTS: u64 = 1 << 40;
/// Messages asked for in one command, and the bytes they may come to together.
const FETCH_AT_ONCE: usize = 25;
const FETCH_BYTES: u64 = 8 * 1024 * 1024;
/// A part is handed back once it holds this many messages or bytes, or this long after the
/// last one, in milliseconds.
const PART_MESSAGES: usize = 200;
const PART_BYTES: u64 = 16 * 1024 * 1024;
#[cfg(not(target_arch = "wasm32"))]
const PART_MS: u128 = 1_000;
/// An account's own data file is saved after every so many bytes taken in: what is not saved
/// yet is held beside the file, so this is what a receive holds at most.
const SAVE_EVERY: u64 = 64 * 1024 * 1024;

/// The words a receive job begins with; one is under way while a job so named is.
pub(crate) const RECEIVING: &str = "Receiving";

fn has_accounts(s: &Session) -> bool {
    !s.accounts.is_empty()
}

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "mail.sendReceive",
            label: "Send/Receive All Folders",
            tab: "Send / Receive",
            group: "Send & Receive",
            shortcut: "F9",
            params: r#"{"wait": false to do it on threads of their own}; sends what is in the Outbox, then takes new mail down from every account and keeps the calendars of those with a CalDAV server in step; returns {"send", "receive", "calendars"} as mail.sendAll, account.receive and calendar.sync give them"#,
            enabled: has_accounts,
            run: send_receive,
        },
        CommandSpec {
            id: "account.receive",
            label: "Receive",
            tab: "",
            group: "",
            shortcut: "",
            params: r#"{"id": one account (every account when not given), "wait": false to receive on a thread of its own}; takes new mail down into the Inbox; returns {"received", "accounts": [{"id", "received", "waiting": what is left on the server for next time, "said": why it stopped, if it did, "inStep": IMAP folders kept in step, with "unchanged" and "changesOnly": folders where the server's mod-sequences (CONDSTORE, QRESYNC) let only what changed be asked for}]}"#,
            enabled: has_accounts,
            run: receive,
        },
    ]
}

fn send_receive(s: &mut Session, p: &Value) -> CmdResult {
    let wait = p_bool(p, "wait") != Some(false);
    // An Outbox with nothing in it, or none at all, is not a reason to take no mail down.
    let send = super::send::send_outbox(s, wait).unwrap_or_else(|e| json!({ "said": e.to_string() }));
    let status = s.status.clone();
    let receive = receive_from(s, None, wait)?;
    if wait && !status.is_empty() {
        s.status = format!("{status}; {}", s.status);
    }
    // Calendars go with mail; a calendar server that cannot be reached stops no mail.
    let calendars = if s.accounts.iter().any(|a| a.caldav.is_some()) {
        let status = s.status.clone();
        let done = super::caldav::sync_from(s, None, wait).unwrap_or_else(|e| json!({ "said": e.to_string() }));
        s.status = if wait { format!("{status}; {}", s.status) } else { status };
        done
    } else {
        Value::Null
    };
    Ok(json!({ "send": send, "receive": receive, "calendars": calendars }))
}

fn receive(s: &mut Session, p: &Value) -> CmdResult {
    let one = match p.get("id") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) => Some(
            s.accounts
                .iter()
                .position(|a| &a.id == id)
                .ok_or_else(|| EngineError::BadParams(format!("there is no account {}", id.chars().take(64).collect::<String>())))?,
        ),
        Some(_) => return Err(EngineError::BadParams("\"id\" is an account's id".into())),
    };
    receive_from(s, one, p_bool(p, "wait") != Some(false))
}

/// One account's part of a receive.
struct Taking {
    reach: Reach,
    received: Received,
    /// POP3: messages taken down before are taken off the server this time.
    take_off: bool,
    /// IMAP: every folder of the server is taken down, not only the Inbox; so when the mail
    /// goes into a data file of the account's own, where the folders can be as the server's.
    folders: bool,
    /// IMAP with a data file of the account's own: what it holds, kept in step with the
    /// server both ways; read from `gather` on the receive's own thread.
    local: Option<Local>,
    gather: Option<imap_sync::Gather>,
}

/// A message taken down.
struct Got {
    /// The IMAP id, or the POP3 unique id.
    uid: Uid,
    /// IMAP: the server has it as read.
    read: bool,
    /// IMAP: the server has it flagged.
    flagged: bool,
    /// IMAP: the server has it as a draft, not sent yet ([RFC 9051] 2.3.2).
    draft: bool,
    data: Vec<u8>,
}

enum Uid {
    Imap(u32),
    Pop(String),
}

/// What one account's part came to.
struct Took {
    id: String,
    name: String,
    got: Vec<Got>,
    /// IMAP: the Inbox's UIDVALIDITY as read now.
    uid_validity: Option<u32>,
    /// POP3: every unique id the server has now, when it said them.
    on_server: Option<Vec<String>>,
    /// Messages left on the server for the next receive.
    waiting: usize,
    /// IMAP: what came from the server's other folders.
    others: Vec<FromFolder>,
    /// IMAP: every folder the server has, by the name it knows it by, when it said them.
    listed: Option<Vec<String>>,
    /// IMAP: the Inbox's highest id passed over, as a message here has it already.
    passed: u32,
    /// IMAP: what keeping in step with the server came to.
    synced: Synced,
    local: Option<Arc<Local>>,
    /// IMAP: a folder could not be opened, so not every one was looked through.
    partial: bool,
    error: Option<NetError>,
    /// IMAP: whether what is in the Inbox now is not told of as new: the first receive, one
    /// after the server numbered it anew, or one before it was ever taken whole. Read on the
    /// receive's own thread from the account as it was, so that each part says the same.
    first: Option<bool>,
    /// The Inbox comes newest first: its mark moves on only once it has come whole (`passed`).
    newest_first: bool,
    /// A part handed back while the receive runs, not what it came to at the end; how many
    /// messages have come so far, and whether the data file is to be saved now.
    part: Option<(usize, bool)>,
}

/// What was taken down from one IMAP folder other than the Inbox; cloned without its mail.
struct FromFolder {
    raw: String,
    /// The folder's place as it reads, its parents first.
    path: Vec<String>,
    /// The kind of folder it is, as the server marks it or names it.
    kind: Option<FolderKind>,
    uid_validity: Option<u32>,
    got: Vec<Got>,
    /// The highest id passed over, as a message here has it already.
    passed: u32,
    /// The folder here it is paired with, when it is.
    here: Option<FolderId>,
    /// The server's own id for it ([RFC 8474]), when it gives one.
    mailbox_id: Option<String>,
    /// It comes newest first: its mark moves on only once it has come whole (`passed`).
    newest_first: bool,
}

/// What one receive may still take down from an account, shared by its folders.
struct Budget {
    /// Messages that may still come.
    messages: usize,
    /// Bytes taken down so far, and how many may be.
    bytes: u64,
    max_bytes: u64,
}

/// Handing back what has come so far, part by part, while a receive runs.
struct Handing<'a> {
    /// Hands a part back; false when there is no one to take it any more.
    give: &'a dyn Fn(Took) -> bool,
    /// Whether parts are taken as they come; where they are not, everything comes at the end.
    streams: bool,
    /// Messages handed back so far, and bytes since the data file was last saved.
    so_far: usize,
    unsaved: u64,
    #[cfg(not(target_arch = "wasm32"))]
    last: std::time::Instant,
}

impl Handing<'_> {
    /// Whether a part holding `n` messages of `bytes` is to be handed back now.
    fn due(&self, n: usize, bytes: u64) -> bool {
        if !self.streams || n == 0 {
            return false;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let late = self.last.elapsed().as_millis() >= PART_MS;
        #[cfg(target_arch = "wasm32")]
        let late = false;
        n >= PART_MESSAGES || bytes >= PART_BYTES || late
    }

    /// Hand back `part`, which holds `got` messages of `bytes`. False when no one takes it.
    fn hand(&mut self, mut part: Took, got: usize, bytes: u64) -> bool {
        self.so_far = self.so_far.saturating_add(got);
        self.unsaved = self.unsaved.saturating_add(bytes);
        let save = self.unsaved >= SAVE_EVERY;
        if save {
            self.unsaved = 0;
        }
        part.part = Some((self.so_far, save));
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.last = std::time::Instant::now();
        }
        (self.give)(part)
    }
}

impl Clone for FromFolder {
    fn clone(&self) -> Self {
        FromFolder {
            raw: self.raw.clone(),
            path: self.path.clone(),
            kind: self.kind,
            uid_validity: self.uid_validity,
            got: Vec::new(),
            passed: self.passed,
            here: self.here,
            mailbox_id: self.mailbox_id.clone(),
            newest_first: self.newest_first,
        }
    }
}

fn bytes_of(got: &[Got]) -> u64 {
    got.iter().map(|g| u64::try_from(g.data.len()).unwrap_or(u64::MAX)).fold(0, u64::saturating_add)
}

/// The kind of folder a server's folder is: by the special use it is marked with
/// ([RFC 6154] 2), and otherwise by the names other programs give such folders.
pub(super) fn kind_of(f: &Folder, path: &[String]) -> Option<FolderKind> {
    for (mark, kind) in [
        ("\\Sent", FolderKind::Sent),
        ("\\Drafts", FolderKind::Drafts),
        ("\\Trash", FolderKind::Deleted),
        ("\\Junk", FolderKind::Junk),
        ("\\Archive", FolderKind::Archive),
    ] {
        if f.has(mark) {
            return Some(kind);
        }
    }
    // A name at the top, or under the Inbox as some servers keep every folder.
    let name = match path {
        [name] => name,
        [inbox, name] if inbox.eq_ignore_ascii_case("Inbox") => name,
        _ => return None,
    };
    match name.trim().to_lowercase().as_str() {
        "sent" | "sent items" | "sent messages" | "sent mail" => Some(FolderKind::Sent),
        "drafts" | "draft" => Some(FolderKind::Drafts),
        "trash" | "deleted items" | "deleted messages" | "deleted" | "bin" => Some(FolderKind::Deleted),
        "junk" | "spam" | "junk e-mail" | "junk email" | "bulk mail" => Some(FolderKind::Junk),
        "archive" | "archives" => Some(FolderKind::Archive),
        _ => None,
    }
}

/// A folder's place as it reads: its name split where the server's delimiter is, each part
/// bounded as a folder name is.
pub(super) fn path_of(f: &Folder) -> Vec<String> {
    let parts: Vec<String> = match f.delimiter {
        Some(d) => f.name.split(d).map(str::to_string).collect(),
        None => vec![f.name.clone()],
    };
    parts
        .into_iter()
        .map(|p| p.chars().filter(|c| !c.is_control()).take(255).collect::<String>().trim().to_string())
        .filter(|p| !p.is_empty())
        .take(32)
        .collect()
}

/// Folders whose mail is in other folders too, as a view: taken down, the mail would be there
/// twice ([RFC 6154] 2).
pub(super) fn is_view(f: &Folder) -> bool {
    ["\\All", "\\Flagged", "\\Important"].iter().any(|a| f.has(a))
}

/// Where an account's new mail goes: the Inbox of a data file, and whether the file is the
/// account's own.
#[derive(Clone, Copy)]
struct Target {
    store: usize,
    folder: FolderId,
    own: bool,
}

/// The data file mail is taken down into, as `Received::into` names it.
fn into_of(s: &Session, target: Target) -> String {
    if !target.own {
        return String::new();
    }
    s.stores.get(target.store).and_then(|st| st.path()).map(|p| p.display().to_string()).unwrap_or_default()
}

/// The data file and Inbox new mail goes in.
fn inbox(s: &Session) -> Option<(usize, FolderId)> {
    let store = s.writable_store()?;
    Some((store, s.find_folder(store, FolderKind::Inbox)?))
}

pub(crate) fn receive_from(s: &mut Session, one: Option<usize>, wait: bool) -> CmdResult {
    if s.accounts.is_empty() {
        return Err(EngineError::Format("there is no account to receive from; add one with account.add".into()));
    }
    if s.jobs.iter().any(|j| j.what.starts_with(RECEIVING)) {
        return Err(EngineError::Format("mail is being received already".into()));
    }
    let mut taking = Vec::new();
    let mut targets: Vec<(String, Target)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for at in 0..s.accounts.len() {
        if one.is_some_and(|o| o != at) {
            continue;
        }
        let target = match super::account::account_store(s, at) {
            Ok(Some(store)) => s.find_folder(store, FolderKind::Inbox).map(|folder| Target { store, folder, own: true }),
            Ok(None) => inbox(s).map(|(store, folder)| Target { store, folder, own: false }),
            Err(e) => {
                let name = s.accounts.get(at).map(|a| a.name.clone()).unwrap_or_default();
                skipped.push(format!("{name}: its data file could not be opened ({e})"));
                continue;
            }
        };
        let Some(account) = s.accounts.get(at) else { continue };
        let Some(target) = target else {
            skipped.push(format!("{}: there is no Inbox in a data file that can be written", account.name));
            continue;
        };
        // Taken off a POP3 server only what is kept for good: the data file is a file, and
        // holds no change that is not saved.
        let saved = s.stores.get(target.store).is_some_and(|st| st.path().is_some() && !st.unsaved());
        let (received, take_off) = (account.received.kept_in(&into_of(s, target)), !account.leave_on_server && saved);
        targets.push((account.id.clone(), target));
        let gather = (target.own && account.kind == Kind::Imap).then(|| imap_sync::local(s, target.store, &received));
        taking.push(Taking { reach: Reach::of(s, at)?, received, take_off, folders: target.own, local: None, gather });
    }
    if taking.is_empty() {
        return Err(EngineError::Format(skipped.first().cloned().unwrap_or_else(|| "there is no account to receive from".into())));
    }
    let what = match taking.as_slice() {
        [one] => format!("{RECEIVING} from {}", one.reach.account.name),
        all => format!("{RECEIVING} from {} accounts", all.len()),
    };
    s.parts.clear();
    s.begin_in_parts(what, wait, move |progress, parts| {
        progress.begin(u64::try_from(taking.len()).unwrap_or(u64::MAX));
        let mut took = Vec::new();
        for mut t in taking {
            if progress.stopped() {
                break;
            }
            let give = |part: Took| {
                let targets = targets.clone();
                parts.hand(Box::new(move |s: &mut Session| ended(s, &targets, vec![part], Vec::new())))
            };
            let mut handing = Handing {
                give: &give,
                streams: parts.streams(),
                so_far: 0,
                unsaved: 0,
                #[cfg(not(target_arch = "wasm32"))]
                last: std::time::Instant::now(),
            };
            let one = match t.reach.account.kind {
                Kind::Imap => match t.gather.take().map(|g| g.finish(progress)).transpose() {
                    Ok(local) => {
                        t.local = local;
                        from_imap(&mut t, progress, &mut handing)
                    }
                    Err(e) => {
                        let mut out = self::took(&t);
                        out.error = Some(e);
                        out
                    }
                },
                Kind::Pop3 => from_pop3(&t, progress),
            };
            took.push(one);
            progress.step();
        }
        let step: Ended = Box::new(move |s: &mut Session| ended(s, &targets, took, skipped));
        step
    })
}

fn took(t: &Taking) -> Took {
    Took {
        id: t.reach.account.id.clone(),
        name: t.reach.account.name.clone(),
        got: Vec::new(),
        uid_validity: None,
        on_server: None,
        waiting: 0,
        others: Vec::new(),
        listed: None,
        passed: 0,
        synced: Synced::default(),
        local: None,
        partial: false,
        error: None,
        first: None,
        newest_first: false,
        part: None,
    }
}

fn from_imap(t: &mut Taking, progress: &openmailclient_model::Progress, handing: &mut Handing) -> Took {
    let mut out = took(t);
    if let Err(e) = imap_into(t, progress, &mut out, handing) {
        out.error = Some(e);
    }
    out
}

fn imap_into(t: &mut Taking, progress: &openmailclient_model::Progress, out: &mut Took, handing: &mut Handing) -> Result<(), NetError> {
    let login = t.reach.login()?;
    let mut imap = Imap::open(&*t.reach.connector, &t.reach.account.incoming_endpoint(), &login)?;
    // What was done here goes to the server first, so that what is taken down after is the
    // server as it is with it: folders, and then messages.
    let mut map = None;
    let mut folders = None;
    if let Some(local) = t.local.as_mut() {
        let listed = imap.folders()?;
        let mut m = Map::of(&listed, local);
        let moved = imap_sync::follow(&mut imap, &listed, &mut m, local, &mut t.received, &mut out.synced, progress)?;
        imap_sync::push(&mut imap, &mut m, local, &mut out.synced, progress)?;
        // Folders made, renamed or deleted on the server are listed as they are now.
        folders = Some(if out.synced.created.is_empty() && !moved { listed } else { imap.folders()? });
        map = Some(m);
    }
    // What was to go on the server has gone; what is left is only read from now on, and
    // shared with each part handed back.
    let shared: Option<Arc<Local>> = t.local.take().map(|mut l| {
        l.items.iter_mut().for_each(|i| i.upload = None);
        Arc::new(l)
    });
    out.local = shared.clone();
    let t: &Taking = t;
    let sync = shared.as_deref().zip(map.as_ref());
    let step = sync.and(t.received.step("INBOX"));
    let inbox = imap.select_since("INBOX", step.map(|s| (s.uid_validity, s.modseq)))?;
    out.uid_validity = inbox.uid_validity;
    // Newest first where what was not come to is known by its place next time.
    out.newest_first = sync.is_some();
    out.first =
        inbox.uid_validity.map(|v| t.received.uid_validity == 0 || v != t.received.uid_validity || (out.newest_first && t.received.last_uid == 0));
    let mut budget = match handing.streams {
        true => Budget { messages: MAX_PER_RUN_IN_PARTS, bytes: 0, max_bytes: MAX_BYTES_IN_PARTS },
        false => Budget { messages: MAX_PER_RUN, bytes: 0, max_bytes: MAX_BYTES },
    };
    let whole = in_step(&mut imap, sync, "INBOX", &inbox, step, &mut out.synced)?;
    let (uid_validity, first, newest_first) = (out.uid_validity, out.first, out.newest_first);
    let hand = |got: &mut Vec<Got>, h: &mut Handing| {
        let bytes = bytes_of(got);
        if !h.due(got.len(), bytes) {
            return true;
        }
        let got = std::mem::take(got);
        let n = got.len();
        let part = Took { got, uid_validity, first, newest_first, local: shared.clone(), ..took(t) };
        h.hand(part, n, bytes)
    };
    let (got, waiting, passed) = take(
        &mut imap,
        &inbox,
        (t.received.uid_validity, t.received.last_uid),
        &mut budget,
        progress,
        whole.as_deref(),
        sync.map(|(l, _)| l),
        "INBOX",
        &out.synced,
        newest_first,
        &mut |got| hand(got, handing),
    )?;
    (out.got, out.waiting, out.passed) = (got, waiting, passed);
    // What the Inbox gave is kept even when the other folders cannot be read.
    if t.folders
        && let Err(e) = others_into(&mut imap, t, folders, sync, &mut budget, progress, out, handing)
    {
        out.error = Some(e);
        return Ok(());
    }
    if let Some(local) = &shared {
        out.synced.matched = !local.matched && !out.partial && !progress.stopped();
    }
    imap.logout()
}

/// What the server did to the messages here taken from the selected folder. Where the server
/// keeps mod-sequences and the folder was kept in step before (`was`), only what changed since
/// is asked for; otherwise the folder's whole list is read, and given back for what is new.
fn in_step(
    imap: &mut Imap,
    sync: Option<(&Local, &Map)>,
    raw: &str,
    selected: &Selected,
    was: Option<&Step>,
    done: &mut Synced,
) -> Result<Option<Vec<Summary>>, NetError> {
    let Some((local, map)) = sync else { return Ok(None) };
    // Where the folder is left now, where the server keeps mod-sequences.
    let now = selected.highest_modseq.zip(selected.uid_validity).map(|(modseq, uid_validity)| Step {
        raw: raw.to_string(),
        uid_validity,
        modseq,
        exists: selected.exists,
        uid_next: selected.uid_next.unwrap_or(0),
    });
    if !imap_sync::wants_list(local, raw) {
        // Nothing here is kept in step with it: what it is now is all there is to remember.
        done.steps.extend(now);
        return Ok(None);
    }
    let here = map.here(raw);
    if let (Some(was), Some(now), Some(here)) = (was, &now, here)
        && local.matched
        && was.uid_validity == now.uid_validity
        && was.modseq <= now.modseq
        && let Some(delta) = imap_sync::since(imap, selected, was, now, local)?
    {
        if delta.changed.is_empty() && delta.gone.is_empty() && was.modseq == now.modseq {
            done.unchanged += 1;
        } else {
            done.changes_only += 1;
        }
        imap_sync::pull_since(raw, here, now.uid_validity, &delta, local, done);
        done.steps.push(now.clone());
        return Ok(None);
    }
    let listed = if selected.exists == 0 { Vec::new() } else { imap.list(1)? };
    let whole = match here {
        Some(here) => imap_sync::pull(imap, raw, here, selected, &listed, local, done)?,
        None => false,
    };
    // Only a whole list leaves nothing that went unseen before the mod-sequence kept.
    if whole {
        done.steps.extend(now);
    }
    Ok(Some(listed))
}

/// The new messages of the selected folder, as far as the budget goes: those after the mark's
/// `last_uid`, or every one when the folder was numbered anew (the mark's UIDVALIDITY) or never
/// read. `whole` is the folder's list when it was read already. A message a message here has
/// the place of already (this receive put it there, or a move here did) is passed over. With
/// `newest_first` the newest come first, and what is passed over moves the mark on only once
/// the folder has come whole. What has come is given to `hand` part by part as it comes;
/// returns the rest, how many are left for next time, and the highest id passed over.
#[allow(clippy::too_many_arguments)]
fn take(
    imap: &mut Imap,
    selected: &Selected,
    (uid_validity, last_uid): (u32, u32),
    budget: &mut Budget,
    progress: &openmailclient_model::Progress,
    whole: Option<&[Summary]>,
    local: Option<&Local>,
    raw: &str,
    synced: &Synced,
    newest_first: bool,
    hand: &mut dyn FnMut(&mut Vec<Got>) -> bool,
) -> Result<(Vec<Got>, usize, u32), NetError> {
    let same = selected.uid_validity.is_some_and(|v| v == uid_validity);
    let from = if same { last_uid.saturating_add(1) } else { 1 };
    // Nothing came since, as the folder's next id says: there is nothing to ask for. A folder
    // that holds nothing has come whole.
    if selected.exists == 0 || (same && selected.uid_next.is_some_and(|next| next <= from)) {
        let passed = if newest_first && selected.exists == 0 { selected.uid_next.unwrap_or(1).saturating_sub(1) } else { 0 };
        return Ok((Vec::new(), 0, passed));
    }
    let listed = match whole {
        Some(all) => all.iter().filter(|m| m.uid >= from).cloned().collect(),
        None => imap.list(from)?,
    };
    let v = selected.uid_validity.unwrap_or(0);
    let here: std::collections::HashSet<u32> = local
        .map(|l| {
            l.items
                .iter()
                .filter_map(|i| match &i.place {
                    Some(Place::On { raw: r, uid_validity, uid, .. }) if r == raw && *uid_validity == v => Some(*uid),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let mut todo: Vec<&Summary> = listed.iter().collect();
    if newest_first {
        todo.reverse();
    }
    let mut got = Vec::new();
    let mut passed = 0;
    let mut at = 0usize;
    loop {
        // The next few to ask for together, as far as the budget goes.
        let (mut ask, mut ask_bytes, mut full) = (Vec::<&Summary>::new(), 0u64, false);
        while let Some(m) = todo.get(at).copied() {
            if here.contains(&m.uid) || synced.ours.contains(&(raw.to_string(), m.uid)) {
                if !newest_first {
                    passed = passed.max(m.uid);
                }
                at += 1;
                continue;
            }
            // The first message of a receive comes whatever its size; after it, what fits.
            let taken = budget.bytes.saturating_add(ask_bytes);
            let fits = taken == 0 || taken.saturating_add(m.size) <= budget.max_bytes;
            if budget.messages <= ask.len() || !fits || progress.stopped() {
                full = true;
                break;
            }
            if !ask.is_empty() && (ask.len() >= FETCH_AT_ONCE || ask_bytes.saturating_add(m.size) > FETCH_BYTES) {
                break;
            }
            ask.push(m);
            ask_bytes = ask_bytes.saturating_add(m.size);
            at += 1;
        }
        if ask.is_empty() && !full {
            break;
        }
        let uids: Vec<u32> = ask.iter().map(|m| m.uid).collect();
        // One that is gone by now is passed over.
        let fetched = match uids.as_slice() {
            [] => Vec::new(),
            [one] => imap.fetch(*one)?.map(|data| vec![(*one, data)]).unwrap_or_default(),
            many => imap.fetch_many(many)?,
        };
        for (uid, data) in fetched {
            let Some(m) = ask.iter().find(|m| m.uid == uid) else { continue };
            let len = u64::try_from(data.len()).unwrap_or(u64::MAX).max(1);
            budget.messages = budget.messages.saturating_sub(1);
            budget.bytes = budget.bytes.saturating_add(len);
            let has = |flag: &str| m.flags.iter().any(|f| f.eq_ignore_ascii_case(flag));
            got.push(Got { uid: Uid::Imap(uid), read: has("\\Seen"), flagged: has("\\Flagged"), draft: has("\\Draft"), data });
        }
        // Handed back once there is enough of it, or it has waited long enough.
        if !hand(&mut got) {
            full = true;
        }
        if full {
            let passed = if newest_first { 0 } else { passed };
            return Ok((got, todo.len().saturating_sub(at), passed));
        }
    }
    // Come whole: newest first, the mark goes past every message the folder has.
    if newest_first {
        passed = listed.last().map_or(0, |m| m.uid);
    }
    Ok((got, 0, passed))
}

/// Every folder of the server but the Inbox, each from where it was left.
#[allow(clippy::too_many_arguments)]
fn others_into(
    imap: &mut Imap,
    t: &Taking,
    folders: Option<Vec<Folder>>,
    sync: Option<(&Local, &Map)>,
    budget: &mut Budget,
    progress: &openmailclient_model::Progress,
    out: &mut Took,
    handing: &mut Handing,
) -> Result<(), NetError> {
    let folders = match folders {
        Some(f) => f,
        None => imap.folders()?,
    };
    out.listed = Some(folders.iter().map(|f| f.raw.clone()).collect());
    for f in folders.iter().filter(|f| f.selectable() && !f.raw.eq_ignore_ascii_case("INBOX") && !is_view(f)).take(MAX_FOLDERS) {
        if progress.stopped() {
            break;
        }
        let path = path_of(f);
        if path.is_empty() {
            continue;
        }
        // A folder the server will not open now is passed over, and tried again next time.
        let step = sync.and(t.received.step(&f.raw));
        let selected = match imap.select_since(&f.raw, step.map(|s| (s.uid_validity, s.modseq))) {
            Ok(s) => s,
            Err(NetError::Refused(_)) => {
                out.partial = true;
                continue;
            }
            Err(e) => return Err(e),
        };
        let whole = in_step(imap, sync, &f.raw, &selected, step, &mut out.synced)?;
        let mark = t.received.folders.iter().find(|m| m.raw == f.raw).cloned().unwrap_or_default();
        let from = FromFolder {
            raw: f.raw.clone(),
            path,
            kind: kind_of(f, &path_of(f)),
            uid_validity: selected.uid_validity,
            got: Vec::new(),
            passed: 0,
            here: sync.and_then(|(_, m)| m.here(&f.raw)),
            mailbox_id: selected.mailbox_id.clone(),
            newest_first: out.newest_first,
        };
        let (uid_validity, first, newest_first, local) = (out.uid_validity, out.first, out.newest_first, out.local.clone());
        let hand = |got: &mut Vec<Got>, h: &mut Handing| {
            let bytes = bytes_of(got);
            if !h.due(got.len(), bytes) {
                return true;
            }
            let n = got.len();
            let part = FromFolder { got: std::mem::take(got), ..from.clone() };
            let part = Took { others: vec![part], uid_validity, first, newest_first, local: local.clone(), ..took(t) };
            h.hand(part, n, bytes)
        };
        let (got, waiting, passed) = take(
            imap,
            &selected,
            (mark.uid_validity, mark.last_uid),
            budget,
            progress,
            whole.as_deref(),
            sync.map(|(l, _)| l),
            &f.raw,
            &out.synced,
            newest_first,
            &mut |got| hand(got, handing),
        )?;
        out.waiting = out.waiting.saturating_add(waiting);
        out.others.push(FromFolder { got, passed, ..from });
    }
    Ok(())
}

fn from_pop3(t: &Taking, progress: &openmailclient_model::Progress) -> Took {
    let mut out = took(t);
    if let Err(e) = pop3_into(t, progress, &mut out) {
        out.error = Some(e);
    }
    out
}

fn pop3_into(t: &Taking, progress: &openmailclient_model::Progress, out: &mut Took) -> Result<(), NetError> {
    let login = t.reach.login()?;
    let mut pop = Pop3::open(&*t.reach.connector, &t.reach.account.incoming_endpoint(), &login)?;
    let listed = pop.list()?;
    let seen: std::collections::HashSet<&str> = t.received.seen.iter().map(String::as_str).collect();
    let mut bytes = 0u64;
    let mut new = 0usize;
    let mut staying = Vec::with_capacity(listed.len());
    for m in &listed {
        if seen.contains(m.uid.as_str()) {
            // Kept here, and saved, before: it may go from the server now.
            if t.take_off {
                pop.delete(m.number)?;
            } else {
                staying.push(m.uid.clone());
            }
            continue;
        }
        staying.push(m.uid.clone());
        if new >= MAX_PER_RUN || (bytes > 0 && bytes.saturating_add(m.size) > MAX_BYTES) || progress.stopped() {
            out.waiting += 1;
            continue;
        }
        let data = pop.retrieve(m.number)?;
        bytes = bytes.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
        new += 1;
        out.got.push(Got { uid: Uid::Pop(m.uid.clone()), read: false, flagged: false, draft: false, data });
    }
    // Messages marked to go are taken off only when the session ends well ([RFC 1939] 6);
    // until then the server has them all.
    pop.quit()?;
    out.on_server = Some(staying);
    Ok(())
}

/// A message as it is kept: read as mail, or, when it cannot be, kept whole as an attachment so
/// nothing of it is lost. `on` is the server's folder and its UIDVALIDITY, when the message is
/// to know its place there.
fn item_of(got: &Got, now: i64, on: Option<(&str, u32)>) -> Item {
    let mut item = openmailclient_mime::parse(&got.data).unwrap_or_else(|e| {
        let mut item = Item::new_mail();
        item.set_subject(&format!("A message that could not be read ({e})"));
        item.set_body_text("The message the server gave is attached as it came.");
        item.attachments.push(Attachment::file("message.eml", "message/rfc822", got.data.clone()));
        item
    });
    if item.props.time(pid::MESSAGE_DELIVERY_TIME).is_none_or(|t| t <= 0 || t > now) {
        item.props.set(pid::MESSAGE_DELIVERY_TIME, PropValue::Time(now));
    }
    let mut flags = item.props.i32(pid::MESSAGE_FLAGS).unwrap_or(0) & !MSGFLAG_READ & !openmailclient_model::MSGFLAG_UNSENT;
    // A draft written elsewhere is a draft here too, to be written on and sent.
    if got.draft {
        flags |= openmailclient_model::MSGFLAG_UNSENT;
    }
    item.props.set(pid::MESSAGE_FLAGS, PropValue::Int32(if got.read { flags | MSGFLAG_READ } else { flags }));
    if got.flagged {
        item.set_flag(Flag::Flagged);
    }
    if let (Some((raw, uid_validity)), Uid::Imap(uid)) = (on, &got.uid) {
        Place::On { raw: raw.to_string(), uid_validity, uid: *uid, seen: got.read, flagged: got.flagged }.put(&mut item);
    }
    item
}

/// The folder of the account's data file a server's folder is kept in: the one of its kind, or
/// the one of the same name and place, made when it is not there yet. None for a folder that
/// mail must not go into here: the Outbox, whose mail would be sent, and folders of
/// appointments, contacts and the like.
fn local_folder(s: &mut Session, store: usize, from: &FromFolder) -> Result<Option<FolderId>, String> {
    // The folder it is paired with, wherever it stands and whatever it is called now.
    if let Some(here) = from.here
        && s.folder_info(store, here).is_some()
    {
        return Ok(Some(here));
    }
    if let Some(kind) = from.kind
        && let Some(id) = s.find_folder(store, kind)
    {
        return Ok(Some(id));
    }
    // The server's top is where the Inbox stands.
    let list = s.folders.get(store).ok_or("the data file is not open")?;
    let mut parent = list.iter().find(|f| f.kind == FolderKind::Inbox).and_then(|f| f.parent);
    for part in &from.path {
        let wanted = part.to_lowercase();
        let here =
            s.folders.get(store).and_then(|l| l.iter().find(|f| f.parent == parent && f.name.to_lowercase() == wanted).map(|f| (f.id, f.kind)));
        parent = match here {
            Some((
                _,
                FolderKind::Outbox
                | FolderKind::Calendar
                | FolderKind::Contacts
                | FolderKind::Tasks
                | FolderKind::Notes
                | FolderKind::Journal
                | FolderKind::Search
                | FolderKind::Root,
            )) => return Ok(None),
            Some((id, _)) => Some(id),
            None => {
                let st = s.stores.get_mut(store).ok_or("the data file is not open")?;
                let made = match parent {
                    Some(p) => st.create_folder(p, part),
                    None => st.create_top_folder(part),
                };
                let id = made.map_err(|e| format!("the folder {part} could not be made: {e}"))?;
                s.refresh_folders(store);
                Some(id)
            }
        };
    }
    Ok(parent)
}

/// Keep what came from one of the server's folders other than the Inbox, and move its mark on
/// past each message once it is kept. A message this program put in the server's Sent folder
/// is not kept again: Sent Items has it already. Returns how many were kept, how many of them
/// unread, and what went wrong.
fn keep_folder(
    s: &mut Session,
    store: usize,
    from: &FromFolder,
    received: &mut Received,
    now: i64,
    sync: &mut Adopting,
) -> (usize, usize, Option<String>) {
    let at = received.folders.iter().position(|m| m.raw == from.raw);
    let mut mark = at.and_then(|i| received.folders.get(i).cloned()).unwrap_or_else(|| Mark { raw: from.raw.clone(), ..Mark::default() });
    if let Some(v) = from.uid_validity
        && v != mark.uid_validity
    {
        mark = Mark { raw: from.raw.clone(), uid_validity: v, last_uid: 0, ..mark };
    }
    if let Some(id) = &from.mailbox_id {
        mark.mailbox_id = id.clone();
    }
    let (mut kept, mut unread, mut why) = (0, 0, None);
    // A folder the server has is made here even while it holds nothing, as the server shows it.
    {
        match local_folder(s, store, from) {
            Ok(Some(folder)) => {
                // Paired from now on: a rename or move on either side is followed by this.
                if mark.here != folder.0 || mark.agreed.is_none() {
                    mark.here = folder.0;
                    mark.agreed = s.folder_info(store, folder).map(|f| (f.parent.map(|p| p.0), f.name.clone()));
                }
                let sent = s.folders.get(store).and_then(|l| l.iter().find(|f| f.id == folder)).is_some_and(|f| f.kind == FolderKind::Sent);
                for got in &from.got {
                    let Uid::Imap(uid) = got.uid else { continue };
                    let item = item_of(got, now, sync.local.is_some().then_some(from.raw.as_str()).zip(from.uid_validity));
                    let id = item.props.str(pid::INTERNET_MESSAGE_ID).map(str::trim).unwrap_or_default().to_string();
                    // One put on the server from here: the message here is it.
                    // Newest first, the mark moves on only once the folder has come whole.
                    let past = if from.newest_first { mark.last_uid } else { mark.last_uid.max(uid) };
                    if sync.adopt(s, store, folder, &from.raw, &id, &item) {
                        mark.last_uid = past;
                        received.sent_here.retain(|m| *m != id);
                        continue;
                    }
                    if sent && !id.is_empty() && received.sent_here.contains(&id) {
                        received.sent_here.retain(|m| *m != id);
                        mark.last_uid = past;
                        continue;
                    }
                    match s.stores.get_mut(store).map(|st| st.add(folder, item)) {
                        Some(Ok(_)) => {
                            kept += 1;
                            unread += usize::from(!got.read);
                            mark.last_uid = past;
                        }
                        Some(Err(e)) => {
                            why = Some(format!("a message could not be kept: {e}"));
                            break;
                        }
                        None => break,
                    }
                }
            }
            // Mail that has no place here is passed over for good.
            Ok(None) => {
                mark.last_uid = from.got.iter().filter_map(|g| if let Uid::Imap(u) = g.uid { Some(u) } else { None }).fold(mark.last_uid, u32::max);
            }
            Err(e) => why = Some(e),
        }
    }
    if why.is_none() {
        mark.last_uid = mark.last_uid.max(from.passed);
    }
    if let Some(m) = at.and_then(|i| received.folders.get_mut(i)) {
        *m = mark;
    } else if received.folders.len() < MAX_FOLDERS {
        received.folders.push(mark);
    }
    (kept, unread, why)
}

/// Take back what a receive came to: each message into the Inbox, and the account moved on
/// past it only once it is there.
/// What the parts of a receive that runs came to for one account, counted into what its end
/// says.
#[derive(Clone, Debug, Default)]
pub(crate) struct PartsOf {
    id: String,
    kept: usize,
    unread: usize,
    /// A part could not be kept: nothing more of the account's is kept this time, and its
    /// marks stay where they were.
    failed: bool,
}

fn parts_of<'a>(s: &'a mut Session, id: &str) -> &'a mut PartsOf {
    if let Some(at) = s.parts.iter().position(|p| p.id == id) {
        &mut s.parts[at]
    } else {
        s.parts.push(PartsOf { id: id.to_string(), ..PartsOf::default() });
        let last = s.parts.len() - 1;
        &mut s.parts[last]
    }
}

fn ended(s: &mut Session, targets: &[(String, Target)], took: Vec<Took>, said: Vec<String>) -> (String, Value) {
    // A part handed back while the receive runs: what came is kept, and the rest waits.
    if let [one] = took.as_slice()
        && let Some((so_far, save)) = one.part
    {
        let (id, name) = (one.id.clone(), one.name.clone());
        // After a part that could not be kept, nothing more is: it all comes next time.
        if !s.parts.iter().any(|p| p.id == id && p.failed) {
            let (_, value) = kept(s, targets, took, said, false);
            if value["failed"].as_bool() == Some(true) {
                parts_of(s, &id).failed = true;
            } else if save {
                for (_, t) in targets.iter().filter(|(a, t)| *a == id && t.own) {
                    let status = std::mem::take(&mut s.status);
                    let _ = super::file::save_in_background(s, t.store);
                    s.status = status;
                }
            }
        }
        let status = match so_far {
            1 => format!("{RECEIVING} from {name}: 1 message so far…"),
            n => format!("{RECEIVING} from {name}: {n} messages so far…"),
        };
        return (status, Value::Null);
    }
    kept(s, targets, took, said, true)
}

/// Keep what a receive came to, or a part of it (`last` false): only what came, without what
/// keeping in step with the server did, which comes with the end.
fn kept(s: &mut Session, targets: &[(String, Target)], took: Vec<Took>, mut said: Vec<String>, last: bool) -> (String, Value) {
    let mut failed = false;
    let now = s.now.max(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_secs()).ok())
            .map_or(0, openmailclient_model::time::from_unix),
    );
    let mut all = 0usize;
    let mut unread = 0usize;
    let mut changes = 0usize;
    let mut accounts = Vec::new();
    let mut touched: Vec<Target> = Vec::new();
    let mut meetings: Vec<String> = Vec::new();
    for t in took {
        let Some(&(_, Target { store, folder, own })) = targets.iter().find(|(id, _)| *id == t.id) else { continue };
        let unread_at = unread;
        let into = into_of(s, Target { store, folder, own });
        let mut received = s.accounts.iter().find(|a| a.id == t.id).map(|a| a.received.kept_in(&into)).unwrap_or_default();
        // Mail already on a server the first time it is received from, or all of an Inbox the
        // server numbered anew, is not told of as new: only what came since.
        let first = t.first.unwrap_or_else(|| match t.uid_validity {
            Some(v) => received.uid_validity == 0 || v != received.uid_validity,
            None => received.seen.is_empty() && !s.arrived.heard_from.contains(&t.id),
        });
        // A part of this receive could not be kept: the marks stay where they are, so that
        // what came after it comes again.
        let broken = last && s.parts.iter().any(|p| p.id == t.id && p.failed);
        if s.arrived.heard_from.len() < crate::accounts::MAX_ACCOUNTS && !s.arrived.heard_from.contains(&t.id) {
            s.arrived.heard_from.push(t.id.clone());
        }
        if let Some(v) = t.uid_validity
            && v != received.uid_validity
        {
            // The Inbox was numbered anew; the other folders keep their own marks.
            received = Received {
                uid_validity: v,
                into,
                folders: received.folders,
                sent_here: received.sent_here,
                gone: received.gone,
                matched: received.matched,
                steps: received.steps,
                folders_gone: received.folders_gone,
                ..Received::default()
            };
        }
        // Folders renamed on the server, or by it as asked here, keep what is remembered of them.
        for (from, to, d) in &t.synced.renamed {
            received.renamed(from, to, *d);
        }
        let mut kept = 0usize;
        let mut trouble = t.error.as_ref().map(ToString::to_string);
        // What was done on the server, and what went there from here, is taken in first.
        let applied = match (&t.local, last) {
            (Some(l), true) => imap_sync::apply(s, store, l, &t.synced, &mut received),
            _ => Default::default(),
        };
        if let Some(why) = &applied.why {
            trouble.get_or_insert(why.clone());
        }
        let (folders_moved, why) = if t.local.is_some() && last { folders_here(s, store, &t.synced) } else { (0, None) };
        if let Some(why) = why {
            trouble.get_or_insert(why);
        }
        let in_step = applied.changed + applied.removed + t.synced.pushed + folders_moved + t.synced.replaced.len().min(1);
        let mut sync = Adopting { local: t.local.as_deref(), adopted: std::collections::HashSet::new() };
        for from in &t.others {
            let was = received.folders.iter().find(|m| m.raw == from.raw).map(|m| m.last_uid);
            let (n, unread_here, why) = keep_folder(s, store, from, &mut received, now, &mut sync);
            if broken && let Some(m) = received.folders.iter_mut().find(|m| m.raw == from.raw) {
                m.last_uid = was.unwrap_or(0).min(m.last_uid);
            }
            failed |= why.is_some();
            kept += n;
            unread += unread_here;
            if let Some(why) = why {
                trouble.get_or_insert(why);
            }
        }
        // Marks of folders the server no longer has are forgotten.
        if let Some(listed) = t.listed.as_ref().filter(|_| last) {
            received.folders.retain(|m| listed.contains(&m.raw));
        }
        // Each pair as it stands now is what the two agree on, so neither side follows it back.
        for (raw, here, parent, name) in &t.synced.agreed {
            if let Some(m) = received.folders.iter_mut().find(|m| m.raw == *raw) {
                m.here = here.0;
                m.agreed = Some((parent.map(|p| p.0), name.clone()));
            }
        }
        received.folders_gone.retain(|g| !t.synced.folders_deleted.contains(&g.raw));
        let mut new_seen = Vec::new();
        let inbox_on = sync.local.is_some().then_some("INBOX").zip(t.uid_validity);
        let mut inbox_kept = true;
        for got in &t.got {
            let mut item = item_of(got, now, inbox_on);
            // A meeting's message is kept as Outlook keeps one, so a data file of ours shows it.
            super::meeting::dress(&mut item, &s.zone, true);
            let is_unread = !got.read;
            let id = item.props.str(pid::INTERNET_MESSAGE_ID).map(str::trim).unwrap_or_default().to_string();
            // Newest first, the mark moves on only once the Inbox has come whole.
            let past = |was: u32, uid: u32| if t.newest_first { was } else { was.max(uid) };
            if let Uid::Imap(uid) = &got.uid
                && sync.adopt(s, store, folder, "INBOX", &id, &item)
            {
                received.last_uid = past(received.last_uid, *uid);
                continue;
            }
            let told = (is_unread && !first).then(|| crate::arrival::Told::of(&item));
            // A meeting's answer or cancellation is taken in once the message is kept.
            let meeting = openmailclient_model::meeting::is_meeting_message(&item).then(|| item.clone());
            match s.stores.get_mut(store).map(|st| st.add(folder, item)) {
                Some(Ok(new)) => {
                    if let Some(said) = meeting.and_then(|m| super::meeting::arrived(s, &m)) {
                        meetings.push(said);
                    }
                    if let Some(told) = told {
                        s.arrived.note(store, folder, new, &t.name, told);
                    }
                    kept += 1;
                    unread += usize::from(is_unread);
                    match &got.uid {
                        Uid::Imap(uid) => received.last_uid = past(received.last_uid, *uid),
                        Uid::Pop(uid) => new_seen.push(uid.clone()),
                    }
                }
                // What could not be kept is not counted as taken down, and comes again.
                Some(Err(e)) => {
                    trouble = Some(format!("a message could not be kept: {e}"));
                    inbox_kept = false;
                    break;
                }
                None => {
                    inbox_kept = false;
                    break;
                }
            }
        }
        failed |= !inbox_kept;
        if inbox_kept && !broken {
            received.last_uid = received.last_uid.max(t.passed);
        }
        // The POP3 ids remembered are those the server still has; ids it no longer has are
        // forgotten, which keeps the list as short as the server's.
        if let Some(on_server) = &t.on_server {
            let there: std::collections::HashSet<&str> = on_server.iter().map(String::as_str).collect();
            received.seen.retain(|u| there.contains(u.as_str()));
        }
        received.seen.extend(new_seen);
        if received.seen.len() > MAX_SEEN {
            let over = received.seen.len() - MAX_SEEN;
            received.seen.drain(..over);
        }
        if let Some(a) = s.accounts.iter_mut().find(|a| a.id == t.id) {
            a.received = received;
        }
        // A part counts what it kept toward the end; the end says all that came.
        if last {
            if let Some(p) = s.parts.iter().find(|p| p.id == t.id) {
                kept = kept.saturating_add(p.kept);
                unread = unread.saturating_add(p.unread);
            }
        } else {
            let p = parts_of(s, &t.id);
            p.kept = p.kept.saturating_add(kept);
            p.unread = p.unread.saturating_add(unread.saturating_sub(unread_at));
        }
        all += kept;
        changes += in_step;
        let save = own && (kept > 0 || in_step > 0 || !sync.adopted.is_empty());
        if !touched.iter().any(|x| x.store == store) {
            touched.push(Target { store, folder, own: save });
        } else if let Some(x) = touched.iter_mut().find(|x| x.store == store) {
            x.own |= save;
        }
        if let Some(why) = &trouble {
            said.push(format!("{}: {why}", t.name));
        }
        accounts.push(json!({
            "id": t.id,
            "received": kept,
            "waiting": t.waiting,
            "inStep": t.local.is_some().then(|| json!({
                "pushed": t.synced.pushed,
                "changed": applied.changed,
                "removed": applied.removed,
                "created": t.synced.created,
                // Folders renamed or moved, on either side; deleted on the server as asked
                // here; and kept here as the server no longer has them.
                "renamed": t.synced.renamed.iter().map(|(from, to, _)| json!({ "from": from, "to": to })).collect::<Vec<_>>(),
                "foldersDeleted": t.synced.folders_deleted,
                "keptHere": t.synced.kept_here.len(),
                // Folders that cost one SELECT, unchanged, and those of which only what changed
                // was read, where the server keeps mod-sequences.
                "unchanged": t.synced.unchanged,
                "changesOnly": t.synced.changes_only,
            })),
            "said": trouble
        }));
    }
    for x in &touched {
        s.refresh_folders(x.store);
    }
    let _ = s.reload_rows();
    if !last {
        return (String::new(), json!({ "received": all, "accounts": accounts, "failed": failed }));
    }
    s.parts.clear();
    // An account's own data file is saved once mail is in it, on a thread of its own: the
    // owner is never asked to save mail that only came, and POP3 mail can leave the server.
    for x in touched.iter().filter(|x| x.own) {
        let status = std::mem::take(&mut s.status);
        let _ = super::file::save_in_background(s, x.store);
        s.status = status;
    }
    let mut status = match all {
        0 => "No new mail".to_string(),
        1 => "1 new message".to_string(),
        n => format!("{n} new messages"),
    };
    if unread > 0 && unread < all {
        status.push_str(&format!(", {unread} unread"));
    }
    let waiting: usize = accounts.iter().filter_map(|a| a["waiting"].as_u64()).map(|n| usize::try_from(n).unwrap_or(0)).sum();
    match changes {
        0 => {}
        1 => status.push_str("; 1 change kept in step with the server"),
        n => status.push_str(&format!("; {n} changes kept in step with the server")),
    }
    if waiting > 0 {
        status.push_str(&format!("; {waiting} more wait on the server for the next Send/Receive"));
    }
    if !said.is_empty() {
        status.push_str(&format!("; {}", said.join("; ")));
    }
    // What a meeting's answers and cancellations changed in the calendar.
    if !meetings.is_empty() {
        status.push_str(&format!("; {}", meetings.join("; ")));
    }
    (status, json!({ "received": all, "accounts": accounts, "meetings": meetings }))
}

/// Folders here moved and renamed as the server's are now, and those the server no longer has
/// put with the deleted mail. How many changed, and what went wrong.
fn folders_here(s: &mut Session, store: usize, synced: &Synced) -> (usize, Option<String>) {
    let (mut n, mut why) = (0, None);
    for (here, parent, name) in &synced.moved_here {
        let Some(info) = s.folder_info(store, *here).cloned() else { continue };
        let to = parent.unwrap_or(info.parent);
        if to.is_some_and(|p| super::folder::subtree(s, store, *here).contains(&p)) {
            continue;
        }
        if super::folder::clash(s, store, to, name, Some(*here)) {
            why.get_or_insert(format!("the folder {} was not named {name} as on the server: one by that name is there", info.name));
            continue;
        }
        let Some(st) = s.stores.get_mut(store) else { break };
        let moved = match to.filter(|p| Some(*p) != info.parent) {
            Some(p) => st.move_folder(*here, p),
            None => Ok(()),
        };
        let done = moved.and_then(|()| if *name == info.name { Ok(()) } else { st.rename_folder(*here, name) });
        match done {
            Ok(()) => n += 1,
            Err(e) => {
                why.get_or_insert(format!("the folder {} could not be moved as on the server: {e}", info.name));
            }
        }
        s.refresh_folders(store);
    }
    for here in &synced.kept_here {
        let (Some(info), Some(bin)) = (s.folder_info(store, *here).cloned(), s.find_folder(store, FolderKind::Deleted)) else { continue };
        if info.parent == Some(bin) || super::folder::subtree(s, store, *here).contains(&bin) {
            continue;
        }
        let name = super::folder::free_name(s, store, bin, &info.name);
        let Some(st) = s.stores.get_mut(store) else { break };
        let done = st.move_folder(*here, bin).and_then(|()| if name == info.name { Ok(()) } else { st.rename_folder(*here, &name) });
        match done {
            Ok(()) => n += 1,
            Err(e) => {
                why.get_or_insert(format!("the folder {} the server no longer has could not be put with the deleted mail: {e}", info.name));
            }
        }
        s.refresh_folders(store);
    }
    (n, why)
}

/// Messages taken down that were put on the server from here: the item here stands for each.
struct Adopting<'a> {
    local: Option<&'a Local>,
    adopted: std::collections::HashSet<openmailclient_model::ItemId>,
}

impl Adopting<'_> {
    /// Whether the message taken down as `item` is one here already, which is now given its
    /// place.
    fn adopt(&mut self, s: &mut Session, store: usize, folder: FolderId, raw: &str, message_id: &str, item: &Item) -> bool {
        let (Some(local), Some(place)) = (self.local, Place::of(item)) else { return false };
        imap_sync::adopt(s, store, local, &mut self.adopted, folder, raw, message_id, &place)
    }
}
