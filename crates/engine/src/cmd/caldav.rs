//! Calendars kept in step with a CalDAV server ([RFC 4791]): each calendar of the server an
//! account names is a Calendar folder of the account's own data file (the first its Calendar,
//! the others beside it under it), and what changes on either side goes to the other on each
//! `calendar.sync` and each Send/Receive. The network is talked to on a thread of its own
//! (`openmailclient_net::dav`); the data file is read before and changed after, on the
//! session's thread.
//!
//! An appointment kept in step carries where it is on the server ("DavPlace": its path, its
//! ETag, and a hash of it as it was then): one whose hash differs was changed here; one whose
//! ETag differs from the server's was changed there. Changed on both sides, the server's is
//! taken and ours kept beside it as a copy, so nothing written is lost. Only what changed is
//! asked for where the server keeps sync tokens ([RFC 6578]); the token kept is the one from
//! before what we put, so an answer lost on the way never loses a change.

use super::account::{Reach, account_store};
use super::imap_sync::PS_OPENMAILCLIENT;
use super::{CommandSpec, p_bool};
use crate::accounts::{DavCal, DavSetup};
use crate::jobs::Ended;
use crate::{CmdResult, EngineError, Session};
use openmailclient_formats::ics;
use openmailclient_model::calendar;
use openmailclient_model::zone::Zone;
use openmailclient_model::{FolderId, FolderKind, Item, ItemId, NameId, NamedKey, Progress, PropValue};
use openmailclient_net::NetError;
use openmailclient_net::dav::{self, Dav, Deleted, Put, Sync};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

const PLACE: &str = "DavPlace";
/// Calendars of one server kept in step.
const MAX_CALENDARS: usize = 20;
/// Items of one Calendar folder looked at.
const MAX_ITEMS: usize = 20_000;
/// Events taken down in one sync, across its calendars; the rest come with the next.
const MAX_FETCH: usize = 2000;
/// Events put on the server in one sync; the rest go with the next.
const MAX_PUT: usize = 500;

/// The words a sync job begins with; one is under way while a job so named is.
pub(crate) const SYNCING: &str = "Keeping calendars in step";

fn has_caldav(s: &Session) -> bool {
    s.accounts.iter().any(|a| a.caldav.is_some())
}

pub(super) fn commands() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "calendar.sync",
        label: "Update Calendars",
        tab: "Send / Receive",
        group: "Send & Receive",
        shortcut: "",
        params: r#"{"id": one account (default: every account with a CalDAV server), "wait": false to do it on a thread of its own}; the server's calendars are taken down into Calendar folders of the account's data file and what changed here is put there; returns {"accounts": [{"id", "calendars": [{"name", "folder", "readOnly", "fetched", "put", "deleted", "removed", "copies": appointments changed on both sides, ours kept beside the server's, "notPut": changes a read-only calendar does not take}], "said": why it stopped, if it did}]}"#,
        enabled: has_caldav,
        run: sync,
    }]
}

fn sync(s: &mut Session, p: &Value) -> CmdResult {
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
    if one.is_some_and(|at| s.accounts.get(at).is_some_and(|a| a.caldav.is_none())) {
        return Err(EngineError::BadParams("that account has no CalDAV server; give it one with account.edit {\"caldav\"}".into()));
    }
    sync_from(s, one, p_bool(p, "wait") != Some(false))
}

/// Where an appointment is on the server, as it was when last kept in step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DavPlace {
    pub href: String,
    pub etag: String,
    /// The appointment's hash as it was then (`hash_of`).
    pub hash: u64,
}

fn key() -> NamedKey {
    NamedKey { guid: PS_OPENMAILCLIENT, name: NameId::Name(PLACE.to_string()) }
}

impl DavPlace {
    pub(crate) fn of(item: &Item) -> Option<Self> {
        let k = key();
        item.named.iter().find(|(n, _)| *n == k).and_then(|(_, v)| match v {
            PropValue::String(t) => Self::parse(t),
            _ => None,
        })
    }

    fn parse(t: &str) -> Option<Self> {
        if t.len() > 4000 || t.chars().any(char::is_control) {
            return None;
        }
        let mut parts = t.splitn(4, ' ');
        let (Some("1"), Some(hash), Some(href), Some(etag)) = (parts.next(), parts.next(), parts.next(), parts.next()) else { return None };
        let hash = u64::from_str_radix(hash, 16).ok()?;
        (href.starts_with('/') && !etag.is_empty()).then(|| Self { href: href.to_string(), etag: etag.to_string(), hash })
    }

    fn put(&self, item: &mut Item) {
        let (k, v) = (key(), PropValue::String(format!("1 {:016x} {} {}", self.hash, self.href, self.etag)));
        match item.named.iter_mut().find(|(n, _)| *n == k) {
            Some(slot) => slot.1 = v,
            None => item.named.push((k, v)),
        }
    }

    fn clear(item: &mut Item) {
        let k = key();
        item.named.retain(|(n, _)| *n != k);
    }
}

/// An appointment's hash: of what of it a calendar file says, so that what this program keeps
/// for itself, and when it was last written, is no change.
fn hash_of(item: &Item) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in ics::write(std::slice::from_ref(item), &Zone::utc(), 0, None).bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// The path a new event is put at in a calendar: its UID when that is plain, else a hash of it.
fn new_href(calendar: &str, uid: &str) -> String {
    let plain = !uid.is_empty() && uid.len() <= 200 && uid.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'@'));
    if plain {
        format!("{calendar}{uid}.ics")
    } else {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in uid.bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
        format!("{calendar}omc-{h:016x}.ics")
    }
}

/// What one calendar's folder holds, read before the job.
#[derive(Default)]
struct Here {
    href: String,
    token: String,
    /// Kept in step before: by path, the item, its ETag and whether it changed here since.
    known: HashMap<String, Known>,
    /// Never put there: the item, the path it is to go at and the event.
    new: Vec<(u64, String, Vec<u8>, u64)>,
}

#[derive(Clone)]
struct Known {
    id: u64,
    etag: String,
    /// The hash it has now; differing from its place's, it changed here.
    hash: u64,
    /// The event as it is now, when it changed here.
    data: Option<Vec<u8>>,
}

/// Deleted here: an item of Deleted Items and where it was on the server.
struct Gone {
    id: u64,
    place: DavPlace,
}

/// One account's part, gathered on the session's thread.
struct Taking {
    reach: Reach,
    setup: DavSetup,
    here: Vec<Here>,
    gone: Vec<Gone>,
}

/// What one server calendar came to.
#[derive(Default)]
struct Came {
    href: String,
    name: String,
    color: String,
    read_only: bool,
    /// The token to keep; `None` keeps the one there was.
    token: Option<String>,
    /// Taken down: the path, its ETag and the event.
    fetched: Vec<(String, String, Vec<u8>)>,
    /// Put: the item, its path, its new ETag and its hash as put.
    put: Vec<(u64, String, String, u64)>,
    /// Gone from the server.
    removed: Vec<String>,
    /// Changed on both sides: ours kept as a copy before the server's is written on it.
    copies: Vec<u64>,
    /// Changed here but not taken, the calendar being read-only.
    not_put: usize,
    deleted: usize,
}

struct Took {
    id: String,
    name: String,
    store: usize,
    came: Vec<Came>,
    /// Items of Deleted Items whose place is done with.
    cleared: Vec<u64>,
    /// Each item's hash as gathered: one that differs at the end was changed meanwhile, and
    /// what the server has of it waits for the next sync.
    seen: HashMap<u64, u64>,
    error: Option<NetError>,
}

pub(crate) fn sync_from(s: &mut Session, one: Option<usize>, wait: bool) -> CmdResult {
    if s.jobs.iter().any(|j| j.what.starts_with(SYNCING)) {
        return Err(EngineError::Format("calendars are being kept in step already".into()));
    }
    let mut taking = Vec::new();
    let mut stores = Vec::new();
    let mut skipped = Vec::new();
    for at in 0..s.accounts.len() {
        if one.is_some_and(|o| o != at) {
            continue;
        }
        let Some(setup) = s.accounts.get(at).and_then(|a| a.caldav.clone()) else { continue };
        let name = s.accounts.get(at).map(|a| a.name.clone()).unwrap_or_default();
        let store = match account_store(s, at) {
            Ok(Some(store)) if s.stores.get(store).is_some_and(|st| st.writable()) => store,
            Ok(_) => {
                skipped.push(format!("{name}: there is no data file of its own to keep its calendars in"));
                continue;
            }
            Err(e) => {
                skipped.push(format!("{name}: its data file could not be opened ({e})"));
                continue;
            }
        };
        let (here, gone) = gather(s, store, &setup);
        stores.push(store);
        taking.push(Taking { reach: Reach::of(s, at)?, setup, here, gone });
    }
    if taking.is_empty() {
        return Err(EngineError::Format(skipped.first().cloned().unwrap_or_else(|| "no account has a CalDAV server".into())));
    }
    let what = match taking.as_slice() {
        [one] => format!("{SYNCING} for {}", one.reach.account.name),
        all => format!("{SYNCING} for {} accounts", all.len()),
    };
    s.begin(what, wait, move |progress| {
        progress.begin(u64::try_from(taking.len()).unwrap_or(u64::MAX));
        let mut took = Vec::new();
        for (t, store) in taking.into_iter().zip(stores) {
            if progress.stopped() {
                break;
            }
            took.push(work(t, store, progress));
            progress.step();
        }
        Box::new(move |s: &mut Session| ended(s, took, skipped)) as Ended
    })
}

/// What the account's calendar folders and Deleted Items hold, as the job needs it.
fn gather(s: &mut Session, store: usize, setup: &DavSetup) -> (Vec<Here>, Vec<Gone>) {
    let deleted = s.find_folder(store, FolderKind::Deleted);
    let folders: HashSet<FolderId> = s.folders.get(store).map(|f| f.iter().map(|x| x.id).collect()).unwrap_or_default();
    let (here_tz, now) = (s.zone.clone(), s.time_now());
    let Some(st) = s.stores.get_mut(store) else { return (Vec::new(), Vec::new()) };
    let mut out = Vec::new();
    let mut gone = Vec::new();
    for cal in &setup.calendars {
        let folder = FolderId(cal.here);
        if !folders.contains(&folder) {
            continue;
        }
        let mut here = Here { href: cal.href.clone(), token: cal.token.clone(), ..Here::default() };
        for row in st.list(folder).unwrap_or_default().into_iter().take(MAX_ITEMS) {
            let Ok(mut item) = st.load(folder, ItemId(row.id)) else { continue };
            if calendar::global_id(&item).is_none() {
                // Known on the server for life by its UID, which comes of this.
                calendar::set_global_id(&mut item, &calendar::new_global_id(now, row.id ^ u64::try_from(here_tz.bias).unwrap_or(0)));
                if st.update(folder, ItemId(row.id), item.clone()).is_err() {
                    continue;
                }
            }
            let hash = hash_of(&item);
            let data = || ics::write(std::slice::from_ref(&item), &here_tz, now, None).into_bytes();
            match DavPlace::of(&item) {
                Some(place) if place.href.starts_with(&cal.href) && !here.known.contains_key(&place.href) => {
                    let changed = place.hash != hash;
                    here.known.insert(place.href.clone(), Known { id: row.id, etag: place.etag, hash, data: changed.then(data) });
                }
                // Moved here from another calendar: deleted there, and new here.
                Some(place) => {
                    if setup.calendars.iter().any(|c| place.href.starts_with(&c.href) && c.href != cal.href) {
                        gone.push(Gone { id: u64::MAX, place });
                    }
                    here.new.push((row.id, new_href(&cal.href, &ics::uid(&item)), data(), hash));
                }
                None => here.new.push((row.id, new_href(&cal.href, &ics::uid(&item)), data(), hash)),
            }
        }
        out.push(here);
    }
    if let Some(deleted) = deleted {
        for row in st.list(deleted).unwrap_or_default().into_iter().take(MAX_ITEMS) {
            if let Ok(item) = st.load(deleted, ItemId(row.id))
                && let Some(place) = DavPlace::of(&item)
                && setup.calendars.iter().any(|c| place.href.starts_with(&c.href))
            {
                gone.push(Gone { id: row.id, place });
            }
        }
    }
    (out, gone)
}

/// One account's part, on the job's thread: nothing here touches the session.
fn work(t: Taking, store: usize, progress: &Progress) -> Took {
    let seen = t.here.iter().flat_map(|h| h.known.values().map(|k| (k.id, k.hash)).chain(h.new.iter().map(|n| (n.0, n.3)))).collect();
    let mut took =
        Took { id: t.reach.account.id.clone(), name: t.reach.account.name.clone(), store, came: Vec::new(), cleared: Vec::new(), seen, error: None };
    if let Err(e) = talk(&t, &mut took, progress) {
        took.error = Some(e);
    }
    took
}

fn talk(t: &Taking, took: &mut Took, progress: &Progress) -> Result<(), NetError> {
    let login = t.reach.login()?;
    let at = dav::Url::parse(&t.setup.url)?;
    let mut server = Dav::new(&*t.reach.connector, &login, at, t.setup.trusted);
    let calendars: Vec<dav::Calendar> = server.calendars()?.into_iter().filter(|c| c.events).take(MAX_CALENDARS).collect();
    let mut budget = MAX_FETCH;
    let mut puts = MAX_PUT;
    // Deleted here: deleted there, unless it changed there since or cannot be.
    let mut back: HashMap<String, Vec<String>> = HashMap::new();
    for g in &t.gone {
        let Some(cal) = calendars.iter().find(|c| g.place.href.starts_with(&c.href)) else {
            // Its calendar is gone from the server; so is it.
            took.cleared.push(g.id);
            continue;
        };
        if !cal.writable {
            back.entry(cal.href.clone()).or_default().push(g.place.href.clone());
            took.cleared.push(g.id);
            continue;
        }
        match server.delete(&g.place.href, &g.place.etag)? {
            Deleted::Done | Deleted::Gone => took.cleared.push(g.id),
            // Changed there since: it comes back, as the server has it now.
            Deleted::Conflict => {
                back.entry(cal.href.clone()).or_default().push(g.place.href.clone());
                took.cleared.push(g.id);
            }
        }
    }
    let deleted_of = |href: &str, cleared: &[u64]| t.gone.iter().filter(|g| g.place.href.starts_with(href) && cleared.contains(&g.id)).count();
    let empty = Here::default();
    for cal in &calendars {
        if progress.stopped() {
            break;
        }
        let here = t.here.iter().find(|h| h.href == cal.href).unwrap_or(&empty);
        let mut came = Came {
            href: cal.href.clone(),
            name: cal.name.clone(),
            color: cal.color.clone(),
            read_only: !cal.writable,
            deleted: deleted_of(&cal.href, &took.cleared),
            ..Came::default()
        };
        let back = back.remove(&cal.href).unwrap_or_default();
        // What was done before a failure is kept: what was put is known to be there.
        let done = one_calendar(&server, cal, here, back, &mut came, &mut budget, &mut puts, progress);
        if done.is_err() {
            came.token = None;
        }
        took.came.push(came);
        done?;
    }
    Ok(())
}

/// One calendar kept in step: what changed there asked for, what changed here put, and what
/// is wanted taken down.
#[allow(clippy::too_many_arguments)]
fn one_calendar(
    server: &Dav<'_>,
    cal: &dav::Calendar,
    here: &Here,
    back: Vec<String>,
    came: &mut Came,
    budget: &mut usize,
    puts: &mut usize,
    progress: &Progress,
) -> Result<(), NetError> {
    {
        // What changed there, by token where the server keeps them; else the whole list.
        let mut whole = None;
        let mut partial = false;
        let mut changed: Vec<dav::Entry> = Vec::new();
        let mut removed: Vec<String> = Vec::new();
        let mut token = None;
        if !cal.token.is_empty() {
            match server.changes(&cal.href, &here.token)? {
                Sync::Changes(c) => {
                    changed = c.changed;
                    removed = c.removed;
                    partial = c.partial;
                    token = Some(c.token).filter(|t| !t.is_empty());
                }
                Sync::Lost => match server.changes(&cal.href, "")? {
                    Sync::Changes(c) => {
                        partial = c.partial;
                        token = Some(c.token).filter(|t| !t.is_empty());
                        whole = Some(c.changed);
                    }
                    _ => whole = Some(server.list(&cal.href)?),
                },
                Sync::NotKept => whole = Some(server.list(&cal.href)?),
            }
        } else {
            whole = Some(server.list(&cal.href)?);
        }
        if let Some(all) = whole {
            let listed: HashSet<&str> = all.iter().map(|e| e.href.as_str()).collect();
            removed = here.known.keys().filter(|h| !listed.contains(h.as_str())).cloned().collect();
            changed = all;
        }
        let removed_set: HashSet<&str> = removed.iter().map(String::as_str).collect();
        let mut want: Vec<String> = back;
        let mut changed_there: HashSet<String> = HashSet::new();
        for e in &changed {
            match here.known.get(&e.href) {
                Some(k) if k.etag == e.etag => {}
                Some(k) => {
                    changed_there.insert(e.href.clone());
                    // Changed on both sides: the server's is taken, ours kept beside it.
                    if k.data.is_some() && cal.writable {
                        came.copies.push(k.id);
                    }
                    want.push(e.href.clone());
                }
                None => want.push(e.href.clone()),
            }
        }
        // What changed here goes there.
        for (href, k) in &here.known {
            let Some(data) = &k.data else { continue };
            if removed_set.contains(href.as_str()) || changed_there.contains(href) {
                continue;
            }
            if !cal.writable {
                came.not_put += 1;
                continue;
            }
            if *puts == 0 {
                partial = true;
                continue;
            }
            *puts -= 1;
            match server.put(href, data, Some(&k.etag))? {
                Put::Done(Some(etag)) => came.put.push((k.id, href.clone(), etag, k.hash)),
                Put::Done(None) => {
                    came.put.push((k.id, href.clone(), String::new(), k.hash));
                    want.push(href.clone());
                }
                Put::Conflict => {
                    came.copies.push(k.id);
                    want.push(href.clone());
                }
            }
        }
        for (id, href, data, hash) in &here.new {
            if !cal.writable {
                came.not_put += 1;
                continue;
            }
            if *puts == 0 {
                partial = true;
                continue;
            }
            *puts -= 1;
            match server.put(href, data, None)? {
                Put::Done(Some(etag)) => came.put.push((*id, href.clone(), etag, *hash)),
                Put::Done(None) => {
                    came.put.push((*id, href.clone(), String::new(), *hash));
                    want.push(href.clone());
                }
                // One by that name is there: tried again at another next time.
                Put::Conflict => came.not_put += 1,
            }
        }
        want.sort();
        want.dedup();
        if want.len() > *budget {
            want.truncate(*budget);
            partial = true;
        }
        *budget -= want.len();
        for batch in want.chunks(dav::MULTIGET) {
            if progress.stopped() {
                partial = true;
                break;
            }
            for o in server.fetch(&cal.href, batch)? {
                came.fetched.push((o.href, o.etag, o.data.into_bytes()));
            }
        }
        came.removed = removed;
        // Moved on only once everything up to it is here.
        came.token = if partial { None } else { token.or_else(|| (!cal.token.is_empty()).then(|| cal.token.clone())) };
    }
    Ok(())
}

/// What a sync found, put in the data files on the session's thread.
fn ended(s: &mut Session, took: Vec<Took>, skipped: Vec<String>) -> (String, Value) {
    let mut out = Vec::new();
    let mut trouble: Vec<String> = skipped;
    let (mut fetched, mut put) = (0u64, 0u64);
    for t in took {
        let Some(at) = s.accounts.iter().position(|a| a.id == t.id) else { continue };
        if let Some(e) = &t.error {
            trouble.push(format!("{}: {e}", t.name));
        }
        let (calendars, kept) = apply(s, t.store, &t);
        let count = |key: &str| calendars.iter().filter_map(|c| c.get(key).and_then(Value::as_u64)).fold(0u64, u64::saturating_add);
        fetched = fetched.saturating_add(count("fetched"));
        put = put.saturating_add(count("put"));
        // Calendars the server listed are those kept; when it could not be asked, those there were.
        if let Some(dav) = s.accounts.get_mut(at).and_then(|a| a.caldav.as_mut()) {
            dav.calendars = kept;
        }
        s.refresh_folders(t.store);
        let status = std::mem::take(&mut s.status);
        if s.stores.get(t.store).is_some_and(|st| st.unsaved()) {
            let _ = super::file::save_in_background(s, t.store);
        }
        s.status = status;
        out.push(json!({ "id": t.id, "calendars": calendars, "said": t.error.as_ref().map(ToString::to_string) }));
    }
    let _ = s.reload_calendar();
    let mut status = match (fetched, put) {
        (0, 0) => "Calendars are up to date".to_string(),
        (f, 0) => format!("{f} calendar items taken down"),
        (0, p) => format!("{p} calendar items put on the server"),
        (f, p) => format!("{f} calendar items taken down and {p} put on the server"),
    };
    if let Some(first) = trouble.first() {
        status = format!("{status}; {first}");
    }
    (status, json!({ "ok": trouble.is_empty(), "accounts": out }))
}

/// One account's calendars put in its data file; what each came to, and the calendars to keep.
fn apply(s: &mut Session, store: usize, t: &Took) -> (Vec<Value>, Vec<DavCal>) {
    let was: Vec<DavCal> = s.accounts.iter().find(|a| a.id == t.id).and_then(|a| a.caldav.as_ref()).map(|d| d.calendars.clone()).unwrap_or_default();
    let main = s.find_folder(store, FolderKind::Calendar);
    let deleted = s.find_folder(store, FolderKind::Deleted);
    let (here_tz, now) = (s.zone.clone(), s.time_now());
    // A meeting one called, or was asked to, is kept as such.
    let mine = super::meeting::mine(s);
    let me: Vec<&str> = mine.iter().map(String::as_str).collect();
    let mut folders: Vec<(FolderId, String)> = s.folders.get(store).map(|f| f.iter().map(|x| (x.id, x.name.clone())).collect()).unwrap_or_default();
    let mut kept = Vec::new();
    let mut shown = Vec::new();
    let Some(st) = s.stores.get_mut(store) else { return (shown, was) };
    // Items of Deleted Items done with: their place is cleared.
    if let Some(deleted) = deleted {
        for id in t.cleared.iter().filter(|id| **id != u64::MAX) {
            if let Ok(mut item) = st.load(deleted, ItemId(*id)) {
                DavPlace::clear(&mut item);
                let _ = st.update(deleted, ItemId(*id), item);
            }
        }
    }
    let mut main_taken = was.iter().any(|c| Some(FolderId(c.here)) == main && t.came.iter().any(|x| x.href == c.href));
    for c in &t.came {
        // The folder it is kept in: the one it had, else the Calendar (for the first), else one
        // made under the Calendar with its name.
        let had = was.iter().find(|w| w.href == c.href).map(|w| FolderId(w.here)).filter(|f| folders.iter().any(|(id, _)| id == f));
        let folder = match (had, main) {
            (Some(f), _) => Some(f),
            (None, Some(m)) if !main_taken => {
                main_taken = true;
                Some(m)
            }
            (None, Some(m)) => {
                let name = if c.name.trim().is_empty() { "Calendar".to_string() } else { c.name.clone() };
                st.create_folder(m, &name).ok().inspect(|f| folders.push((*f, name)))
            }
            (None, None) => None,
        };
        let Some(folder) = folder else {
            shown.push(json!({ "name": c.name, "folder": null, "said": "the account's data file has no Calendar folder to keep it in" }));
            kept.extend(was.iter().find(|w| w.href == c.href).cloned());
            continue;
        };
        // A calendar of its own follows the server's name; the Calendar keeps its own.
        if Some(folder) != main && !c.name.trim().is_empty() && folders.iter().any(|(id, n)| *id == folder && *n != c.name) {
            let _ = st.rename_folder(folder, &c.name);
        }
        let mut by_href: HashMap<String, u64> = HashMap::new();
        let mut by_goid: HashMap<Vec<u8>, u64> = HashMap::new();
        let mut hashes: HashMap<u64, u64> = HashMap::new();
        for row in st.list(folder).unwrap_or_default().into_iter().take(MAX_ITEMS) {
            if let Ok(item) = st.load(folder, ItemId(row.id)) {
                hashes.insert(row.id, hash_of(&item));
                match DavPlace::of(&item) {
                    Some(p) => {
                        by_href.entry(p.href).or_insert(row.id);
                    }
                    None => {
                        if let Some(g) = calendar::global_id(&item) {
                            by_goid.entry(g).or_insert(row.id);
                        }
                    }
                }
            }
        }
        // 1. Changed on both sides: ours kept as a copy, known on no server.
        let mut copies = 0usize;
        for id in &c.copies {
            if let Ok(mut item) = st.load(folder, ItemId(*id)) {
                DavPlace::clear(&mut item);
                calendar::set_global_id(&mut item, &calendar::new_global_id(now, id.rotate_left(29) ^ 0x5eed));
                if st.add(folder, item).is_ok() {
                    copies += 1;
                }
            }
        }
        // 2. Put there: placed, as it was put.
        for (id, href, etag, hash) in &c.put {
            if let Ok(mut item) = st.load(folder, ItemId(*id)) {
                let etag = if etag.is_empty() { "\"\"".to_string() } else { etag.clone() };
                DavPlace { href: href.clone(), etag, hash: *hash }.put(&mut item);
                if st.update(folder, ItemId(*id), item).is_ok() {
                    by_href.insert(href.clone(), *id);
                }
            }
        }
        // 3. Taken down: written onto the one it is here, else added.
        let mut taken = 0usize;
        for (href, etag, data) in &c.fetched {
            let Ok(file) = ics::read(data, &here_tz) else { continue };
            let Some(event) = file.events.into_iter().next() else { continue };
            let goid = calendar::global_id_of_uid(&event.uid);
            let mine = by_href.get(href).copied().or_else(|| by_goid.get(&goid).copied());
            // Changed here while the server was talked to: that change goes there next time.
            if mine.is_some_and(|id| t.seen.get(&id).is_some_and(|h| hashes.get(&id) != Some(h))) && !c.copies.iter().any(|x| Some(*x) == mine) {
                continue;
            }
            let item = match mine.map(|id| (id, st.load(folder, ItemId(id)))) {
                Some((id, Ok(mut item))) => {
                    event.write_as(&mut item, &me);
                    Some((Some(id), item))
                }
                _ => Some((None, event.item_as(&me))),
            };
            let Some((id, mut item)) = item else { continue };
            DavPlace { href: href.clone(), etag: etag.clone(), hash: hash_of(&item) }.put(&mut item);
            let done = match id {
                Some(id) => st.update(folder, ItemId(id), item).is_ok(),
                None => st.add(folder, item).map(|n| by_href.insert(href.clone(), n.0)).is_ok(),
            };
            if done {
                taken += 1;
            }
        }
        // 4. Gone from the server: unchanged here, to Deleted Items; changed here, kept to go
        // there again.
        let mut removed = 0usize;
        for href in &c.removed {
            let Some(id) = by_href.get(href).copied() else { continue };
            let Ok(mut item) = st.load(folder, ItemId(id)) else { continue };
            let changed = DavPlace::of(&item).is_some_and(|p| p.hash != hash_of(&item));
            DavPlace::clear(&mut item);
            match deleted.filter(|_| !changed) {
                Some(to) => {
                    if st.add(to, item).is_ok() && st.remove(folder, ItemId(id)).is_ok() {
                        removed += 1;
                    }
                }
                None => {
                    let _ = st.update(folder, ItemId(id), item);
                }
            }
        }
        let name = folders.iter().find(|(id, _)| *id == folder).map(|(_, n)| n.clone()).unwrap_or_default();
        shown.push(json!({
            "name": c.name,
            "folder": name,
            "readOnly": c.read_only,
            "fetched": taken,
            "put": c.put.len(),
            "deleted": c.deleted,
            "removed": removed,
            "copies": copies,
            "notPut": c.not_put,
        }));
        let token = c.token.clone().unwrap_or_else(|| was.iter().find(|w| w.href == c.href).map(|w| w.token.clone()).unwrap_or_default());
        kept.push(DavCal { href: c.href.clone(), name: c.name.clone(), color: c.color.clone(), token, here: folder.0, read_only: c.read_only });
    }
    if t.error.is_some() && kept.is_empty() {
        return (shown, was);
    }
    (shown, kept)
}
