//! Reminders that come up for appointments, tasks and messages flagged for follow-up, from
//! every data file open, as Outlook's Reminders window shows them ([MS-OXORMDR]). The model is
//! `openmailclient_model::reminder`; the commands are `reminders.*` in `cmd/reminders.rs`.
//!
//! `Session::reminders_due` is asked by the host every so often. It looks through the
//! Calendar and Tasks folders and the mail flagged for follow-up once a minute, and at once
//! after something was done: an item is read again only when its row says it changed, so a
//! look over a large data file reads little. A reminder whose time came while the window was
//! closed is shown when it opens.
//!
//! Dismissing or snoozing writes what [MS-OXORMDR] says into the item, so Outlook reads the
//! same. In a data file that cannot be written it is kept for this run instead, so a reminder
//! put away does not come back until the program starts again.

use crate::{EngineError, Session};
use openmailclient_model::reminder::{self, Due, Kind, Reminder};
use openmailclient_model::{FolderId, FolderKind, Item, ItemId, StoreError};
use serde_json::{Value, json};
use std::collections::HashMap;

/// How often the folders are looked through, in milliseconds.
const LOOK_MS: u64 = 60_000;
/// After something was done, the folders are looked through again this soon at the least.
const AFTER_MS: u64 = 2_000;
/// Everything is read again this often, for changes a row does not show.
const AFRESH_MS: u64 = 10 * 60_000;
/// Items of one folder looked at at most.
const MAX_ITEMS: usize = 20_000;
/// Reminders shown at most, the earliest first.
pub const MAX_DUE: usize = 500;
/// Characters kept of a subject or a location.
const MAX_TEXT: usize = 255;

/// Where an item is: its data file, folder and id.
pub type Key = (usize, FolderId, u64);

/// A reminder that is due, and the item it is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub store: usize,
    pub folder: FolderId,
    pub id: u64,
    pub reminder: Reminder,
}

impl Pending {
    pub fn key(&self) -> Key {
        (self.store, self.folder, self.id)
    }

    /// What commands and the window show of it. Subject and location are the item's own,
    /// which may be a stranger's (a meeting asked for, a message flagged).
    pub fn to_json(&self, now: i64, today: i64) -> Value {
        let r = &self.reminder;
        json!({
            "store": self.store,
            "folder": self.folder.0,
            "id": self.id,
            "kind": r.kind.name(),
            "due": match r.due {
                Due::At(t) => json!({ "at": openmailclient_model::time::to_unix(t) }),
                Due::Day(d) => json!({ "day": d }),
                Due::Unknown => Value::Null,
            },
            "dueIn": reminder::due_in(r.due, now, today),
            "signal": openmailclient_model::time::to_unix(r.signal),
            "occurrence": r.occurrence,
            "untrusted_content": { "subject": r.subject, "location": r.location },
        })
    }
}

/// What was done with a reminder that could not be written to its item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Held {
    /// Put away, while its signal time is this.
    Dismissed(i64),
    /// Given again at the first, while its signal time is the second.
    Snoozed(i64, i64),
}

/// An item's row as it was last read, and the reminder it had then.
#[derive(Clone, Debug)]
struct Seen {
    time: i64,
    size: u32,
    reminder: Option<Reminder>,
    /// It is a series, whose reminder moves on with time.
    series: bool,
}

/// How reminders are shown, and those that are due.
#[derive(Debug)]
pub struct Reminders {
    /// Whether the Reminders window comes up when one is due (`reminders.options`).
    pub show: bool,
    /// Whether a sound is played when one comes up.
    pub sound: bool,
    /// Whether the Reminders window is open (`view.reminders` opens it).
    pub window: bool,
    /// Those due now, the earliest first.
    pub due: Vec<Pending>,
    /// Counted up each time one more comes due, so the window knows to come up and sound.
    pub fresh: u64,
    /// The earliest time one not yet due comes due, store time.
    pub next: Option<i64>,
    /// Those already counted in `fresh`, by item and signal time.
    told: HashMap<Key, i64>,
    seen: HashMap<Key, Seen>,
    held: HashMap<Key, Held>,
    /// On the host's clock: the last look, and the last time everything was read again.
    looked: Option<u64>,
    afresh: Option<u64>,
    /// Commands run as of the last look.
    runs: u64,
    /// Data files open as of the last look; when this changes, everything is read again.
    stores: usize,
}

impl Default for Reminders {
    fn default() -> Self {
        Self {
            show: true,
            sound: true,
            window: false,
            due: Vec::new(),
            fresh: 0,
            next: None,
            told: HashMap::new(),
            seen: HashMap::new(),
            held: HashMap::new(),
            looked: None,
            afresh: None,
            runs: 0,
            stores: 0,
        }
    }
}

impl Reminders {
    /// What a data file that was closed had is forgotten, and everything is read again.
    pub(crate) fn closed(&mut self) {
        self.seen.clear();
        self.held.clear();
        self.told.clear();
        self.due.clear();
        self.next = None;
        self.looked = None;
    }

    pub fn options_json(&self) -> Value {
        json!({ "show": self.show, "sound": self.sound })
    }
}

fn line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_TEXT).collect()
}

/// The folders of a data file reminders are looked for in, each with whether only items its
/// rows say are flagged are read: Calendar and Tasks folders whole, and mail folders' flagged
/// messages. Not Deleted Items, Junk Email or a search's.
fn folders_of(s: &Session, store: usize) -> Vec<(FolderId, bool)> {
    s.folders
        .get(store)
        .map(|list| {
            list.iter()
                .filter_map(|f| match f.kind {
                    FolderKind::Calendar | FolderKind::Tasks => Some((f.id, false)),
                    FolderKind::Inbox
                    | FolderKind::Drafts
                    | FolderKind::Sent
                    | FolderKind::Outbox
                    | FolderKind::Archive
                    | FolderKind::Mail
                    | FolderKind::Other => Some((f.id, true)),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

impl Session {
    /// Look for reminders when it is time to, and say in how many milliseconds to ask again.
    /// `now_ms` is the host's clock, which only goes forward.
    pub fn reminders_due(&mut self, now_ms: u64) -> Option<u64> {
        let r = &self.reminders;
        let since = r.looked.map(|at| now_ms.saturating_sub(at));
        let now = self.time_now();
        let came = r.next.is_some_and(|at| at <= now);
        let done = r.runs != self.auto_runs() && since.is_none_or(|ms| ms >= AFTER_MS);
        if since.is_none_or(|ms| ms >= LOOK_MS) || came || done || r.stores != self.stores.len() {
            self.look_for_reminders(now_ms);
        }
        let now = self.time_now();
        // Ticks of 100 ns to milliseconds; asked again at least once a minute.
        let until_next = self.reminders.next.map_or(LOOK_MS, |at| u64::try_from(at.saturating_sub(now) / 10_000).unwrap_or(0));
        Some(until_next.clamp(250, LOOK_MS))
    }

    /// Look through the folders now and make what is due as it is.
    pub fn look_for_reminders(&mut self, now_ms: u64) {
        let now = self.time_now();
        let afresh = self.reminders.afresh.is_none_or(|at| now_ms.saturating_sub(at) >= AFRESH_MS) || self.reminders.stores != self.stores.len();
        if afresh {
            self.reminders.afresh = Some(now_ms);
        }
        // A row's time and size need not change when an item is changed (a task ticked off, a
        // flag cleared), so all is read again after a command; the looks between are cheap.
        if afresh || self.reminders.runs != self.auto_runs() {
            self.reminders.seen.clear();
        }
        self.reminders.looked = Some(now_ms);
        self.reminders.runs = self.auto_runs();
        self.reminders.stores = self.stores.len();
        let zone = self.zone.clone();
        let mut found: Vec<Pending> = Vec::new();
        let mut kept: HashMap<Key, Seen> = HashMap::new();
        for store in 0..self.stores.len() {
            for (folder, flagged_only) in folders_of(self, store) {
                let Some(st) = self.stores.get_mut(store) else { continue };
                let Ok(rows) = st.list(folder) else { continue };
                for row in rows.into_iter().take(MAX_ITEMS) {
                    if flagged_only && row.flag == openmailclient_model::Flag::None {
                        continue;
                    }
                    let key = (store, folder, row.id);
                    let known = self.reminders.seen.remove(&key).filter(|k| k.time == row.time && k.size == row.size);
                    // A series is read again once its reminder has come, as it moves on with time.
                    let seen = match known {
                        Some(k) if !(k.series && k.reminder.as_ref().is_some_and(|r| r.signal <= now)) => k,
                        _ => {
                            let item = st.load(folder, ItemId(row.id)).ok();
                            let reminder = item.as_ref().and_then(|i| reminder::of(i, now, &zone)).map(|mut r| {
                                r.subject = line(&r.subject);
                                r.location = line(&r.location);
                                r
                            });
                            let series = item.as_ref().is_some_and(is_series);
                            Seen { time: row.time, size: row.size, reminder, series }
                        }
                    };
                    if let Some(r) = &seen.reminder {
                        found.push(Pending { store, folder, id: row.id, reminder: r.clone() });
                    }
                    kept.insert(key, seen);
                }
            }
        }
        self.reminders.seen = kept;
        self.settle_reminders(found, now);
    }

    /// Makes `found` what is due and what comes next, with what was done with those that
    /// could not be written.
    fn settle_reminders(&mut self, found: Vec<Pending>, now: i64) {
        let r = &mut self.reminders;
        // What was held of one whose item changed since is let go.
        r.held.retain(|key, held| {
            found.iter().any(|p| {
                p.key() == *key
                    && match *held {
                        Held::Dismissed(signal) | Held::Snoozed(_, signal) => p.reminder.signal == signal,
                    }
            })
        });
        let mut due = Vec::new();
        let mut next: Option<i64> = None;
        for mut p in found {
            match r.held.get(&p.key()) {
                Some(Held::Dismissed(_)) => continue,
                Some(Held::Snoozed(until, _)) => p.reminder.signal = *until,
                None => {}
            }
            if p.reminder.is_due(now) {
                due.push(p);
            } else {
                next = Some(next.map_or(p.reminder.signal, |n| n.min(p.reminder.signal)));
            }
        }
        due.sort_by_key(|p| (p.reminder.signal, p.key()));
        due.truncate(MAX_DUE);
        for p in &due {
            if r.told.get(&p.key()) != Some(&p.reminder.signal) {
                r.told.insert(p.key(), p.reminder.signal);
                r.fresh = r.fresh.wrapping_add(1);
            }
        }
        r.told.retain(|key, _| due.iter().any(|p| p.key() == *key));
        r.due = due;
        r.next = next;
    }

    fn auto_runs(&self) -> u64 {
        self.auto.runs
    }

    /// The due reminder on item `key`.
    pub(crate) fn pending(&self, key: Key) -> Option<&Pending> {
        self.reminders.due.iter().find(|p| p.key() == key)
    }

    /// Dismisses or snoozes the reminder on `key`: written into the item where its data file
    /// can be written, else held for this run. `until` snoozes; `None` dismisses.
    pub(crate) fn put_off(&mut self, key: Key, until: Option<i64>) -> Result<(), EngineError> {
        let (store, folder, id) = key;
        let now = self.time_now();
        let zone = self.zone.clone();
        let st = self.stores.get_mut(store).ok_or(StoreError::NoFolder)?;
        let mut item: Item = st.load(folder, ItemId(id))?;
        let Some(before) = reminder::of(&item, now, &zone) else {
            return Err(EngineError::BadParams("that item has no reminder".into()));
        };
        let changed = match until {
            Some(at) => reminder::snooze(&mut item, at, now, &zone),
            None => reminder::dismiss(&mut item, now, &zone),
        };
        let written = changed && st.writable() && st.update(folder, ItemId(id), item.clone()).is_ok();
        if !written {
            let most = now.saturating_add(reminder::MAX_SNOOZE_MIN.saturating_mul(openmailclient_model::calendar::MINUTE));
            let held = match until {
                Some(at) => Held::Snoozed(at.clamp(now, most), before.signal),
                None => Held::Dismissed(before.signal),
            };
            self.reminders.held.insert(key, held);
        } else {
            self.refresh_folders(store);
        }
        // Read again at the next look, whatever its row says.
        self.reminders.seen.remove(&key);
        self.reminders.due.retain(|p| p.key() != key);
        if written {
            // What the item says now: the next occurrence of a series, a snoozed one later.
            if let Some(r) = reminder::of(&item, now, &zone) {
                let p = Pending { store, folder, id, reminder: r };
                if p.reminder.is_due(now) {
                    self.reminders.due.push(p);
                } else {
                    self.reminders.next = Some(self.reminders.next.map_or(p.reminder.signal, |n| n.min(p.reminder.signal)));
                }
            }
        } else if let Some(Held::Snoozed(at, _)) = self.reminders.held.get(&key) {
            self.reminders.next = Some(self.reminders.next.map_or(*at, |n| n.min(*at)));
        }
        self.reminders.told.remove(&key);
        Ok(())
    }
}

/// Whether an item is an appointment that repeats.
fn is_series(item: &Item) -> bool {
    openmailclient_model::calendar::is_appointment(item)
        && openmailclient_model::calendar::Appointment::of(item).is_some_and(|a| a.recurrence.is_some())
}

/// What a reminder is of, as the window names it.
pub fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Appointment => "Appointment",
        Kind::Task => "Task",
        Kind::Message => "Flagged message",
    }
}
