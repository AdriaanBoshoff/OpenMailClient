//! New mail told of, and Send/Receive on a timer.
//!
//! Each receive notes the unread messages it put in an Inbox; the window tells the owner of
//! them (an alert at the corner of the screen, a sound of our own, the taskbar button) as
//! `mail.newMailAlerts` says, and opens one with `mail.openArrived`. What a message says of
//! itself (its sender and subject) is a stranger's text, shown and never acted on.
//!
//! `Session::send_receive_due` is asked by the host every so often, and sends and receives
//! every `send_receive_min` minutes (`mail.sendReceiveEvery`), counted from the end of the last
//! Send/Receive, whoever asked for it.

use crate::Session;
use openmailclient_model::{FolderId, Item, ItemId};
use serde_json::{Value, json};

/// Messages noted for the window at most; older ones make way.
pub const MAX_ARRIVED: usize = 50;
/// What of a message's text is kept for the alert, in characters.
const PREVIEW_CHARS: usize = 160;
/// How often the Outbox is looked through for messages held by Delay Delivery, in milliseconds.
const HELD_LOOK_MS: u64 = 60_000;
/// How long after the host starts its first Send/Receive is made, so the window comes first.
const FIRST_AFTER_MS: u64 = 5_000;
/// Send/Receive is made no more often than every minute, and no less than once a day.
pub const MAX_EVERY_MIN: u64 = 1_440;

/// A message that came, for the window to tell of.
#[derive(Clone, Debug, PartialEq)]
pub struct Arrived {
    /// Counted up from 1 over the run, so the window knows which it has told of.
    pub seq: u64,
    pub store: usize,
    pub folder: FolderId,
    pub item: ItemId,
    /// The account it came to, as the owner named it.
    pub account: String,
    pub from: String,
    pub subject: String,
    /// The start of its text, on one line.
    pub preview: String,
}

impl Arrived {
    /// What the window and commands show of it. Sender, subject and text are the message's own.
    pub fn to_json(&self) -> Value {
        json!({
            "seq": self.seq,
            "store": self.store,
            "item": self.item.0,
            "account": self.account,
            "untrusted_content": { "from": self.from, "subject": self.subject, "preview": self.preview },
        })
    }
}

/// How the owner is told of new mail; each part can be turned off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Alerts {
    /// An alert at the corner of the screen with the sender, subject and start of the text.
    pub alert: bool,
    /// A short sound of our own.
    pub sound: bool,
    /// The window's taskbar button asks for attention while the window is not in front.
    pub flash: bool,
}

impl Default for Alerts {
    fn default() -> Self {
        Self { alert: true, sound: true, flash: true }
    }
}

impl Alerts {
    pub fn to_json(self) -> Value {
        json!({ "alert": self.alert, "sound": self.sound, "flash": self.flash })
    }

    /// The parts `v` gives, over these; `None` when a part given is not true or false.
    pub fn with(self, v: &Value) -> Option<Self> {
        let part = |key: &str, now: bool| match v.get(key) {
            None | Some(Value::Null) => Some(now),
            Some(Value::Bool(b)) => Some(*b),
            Some(_) => None,
        };
        Some(Self { alert: part("alert", self.alert)?, sound: part("sound", self.sound)?, flash: part("flash", self.flash)? })
    }
}

/// What an alert says of a message, taken before it is put in its folder.
#[derive(Debug)]
pub(crate) struct Told {
    from: String,
    subject: String,
    preview: String,
}

impl Told {
    pub(crate) fn of(message: &Item) -> Self {
        let text = message.body_text();
        let mut preview = String::new();
        for word in text.split_whitespace() {
            if preview.chars().count() >= PREVIEW_CHARS {
                break;
            }
            if !preview.is_empty() {
                preview.push(' ');
            }
            preview.push_str(word);
        }
        Self { from: line(&message.sender_name()), subject: line(&message.subject()), preview: preview.chars().take(PREVIEW_CHARS).collect() }
    }
}

/// A stranger's words on one line, and not too many of them.
fn line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect()
}

/// Where the timer stands, on the host's clock.
#[derive(Debug, Default)]
pub(crate) struct Timer {
    /// When the next Send/Receive is due; `None` until the timer is first asked.
    next: Option<u64>,
    /// The minutes it was counting for, so a change starts it again.
    every: u32,
    /// The host's clock when it last asked.
    asked: u64,
    /// When the first message held in the Outbox (Delay Delivery) may go, store time.
    held: Option<i64>,
    /// The host's clock when the Outbox was last looked through for held messages.
    held_seen: Option<u64>,
}

/// The messages that came in this run and were not told of or opened yet, the newest last.
#[derive(Debug, Default)]
pub struct Arrivals {
    pub list: Vec<Arrived>,
    /// The last `seq` given.
    pub last: u64,
    /// Accounts received from in this run: from the second time on, a POP3 server that had
    /// nothing the first time is not taken to be received from for the first time.
    pub(crate) heard_from: Vec<String>,
    /// How many times a rule has asked for the new-mail sound; the window plays it when this
    /// goes up.
    pub sounds: u64,
    /// What rules have answered and forwarded in this run.
    pub(crate) answered: crate::rules::Answered,
}

impl Arrivals {
    /// Note a message put in an Inbox by a receive.
    pub(crate) fn note(&mut self, store: usize, folder: FolderId, item: ItemId, account: &str, told: Told) {
        self.last = self.last.saturating_add(1);
        self.list.push(Arrived {
            seq: self.last,
            store,
            folder,
            item,
            account: line(account),
            from: told.from,
            subject: told.subject,
            preview: told.preview,
        });
        if self.list.len() > MAX_ARRIVED {
            let over = self.list.len() - MAX_ARRIVED;
            self.list.drain(..over);
        }
    }

    /// Forget those of a data file that was closed, and move later ones down.
    pub(crate) fn closed(&mut self, store: usize) {
        self.list.retain(|a| a.store != store);
        for a in &mut self.list {
            if a.store > store {
                a.store -= 1;
            }
        }
    }
}

impl Session {
    /// How long until the next Send/Receive on its own, in milliseconds, as of the host's last
    /// asking; `None` when none is waited for.
    pub fn send_receive_next_in(&self) -> Option<u64> {
        if self.send_receive_min == 0 || self.accounts.is_empty() || self.connector.is_none() {
            return None;
        }
        self.timer.next.map(|next| next.saturating_sub(self.timer.asked))
    }

    /// Make a Send/Receive when `send_receive_min` minutes have gone by since the end of the
    /// last one; the first comes a few seconds after the host first asks. Nothing is begun while
    /// one is under way, while a data file is being saved, or where there is no account or
    /// nothing to reach servers with. `now_ms` is the host's clock, which only goes forward.
    /// Returns in how many milliseconds to ask again, `None` while there is nothing to wait for.
    pub fn send_receive_due(&mut self, now_ms: u64) -> Option<u64> {
        self.timer.asked = now_ms;
        let watch = self.watch_due();
        let held = self.held_due(now_ms);
        let timer = self.timer_due(now_ms);
        [watch, held, timer].into_iter().flatten().min()
    }

    /// What a send found still waiting for its time.
    pub(crate) fn held_changed(&mut self, until: Option<i64>) {
        self.timer.held = until;
    }

    /// When a message held in the Outbox (Delay Delivery) may go, the Outbox is sent then, to
    /// the minute, whether or not Send/Receive comes on its own. The Outbox is looked through
    /// again once a minute, for what was put there or taken out meanwhile; the first look takes
    /// in what came due while the window was closed.
    fn held_due(&mut self, now_ms: u64) -> Option<u64> {
        if self.accounts.is_empty() || self.connector.is_none() {
            return None;
        }
        let now = crate::cmd::clock(self);
        if let Some(at) = self.timer.held
            && at <= now
        {
            let busy = self.jobs.iter().any(|j| j.what.starts_with(crate::cmd::SENDING));
            if busy || !self.saves.is_empty() || !self.exports.is_empty() {
                return Some(1_000);
            }
            self.timer.held = None;
            let quiet = self.auto.runs == self.auto.seen;
            let status = self.status.clone();
            let begun = self.run("mail.sendAll", &json!({ "wait": false }));
            if quiet {
                self.auto.seen = self.auto.runs;
            }
            if let Err(e) = begun {
                self.status = if status.is_empty() { format!("A message held in the Outbox did not go: {e}") } else { status };
            }
            return Some(1_000);
        }
        if self.timer.held_seen.is_none_or(|seen| now_ms.saturating_sub(seen) >= HELD_LOOK_MS) {
            let after = self.timer.held_seen.map(|_| now);
            self.timer.held_seen = Some(now_ms);
            self.timer.held = crate::cmd::held_in_outbox(self, after);
        }
        let at = self.timer.held?;
        // Ticks of 100 ns to milliseconds; asked again at least once a minute, so a clock
        // that was put forward or back is followed.
        Some(u64::try_from(at.saturating_sub(now) / 10_000).unwrap_or(0).clamp(50, HELD_LOOK_MS))
    }

    fn timer_due(&mut self, now_ms: u64) -> Option<u64> {
        let busy = self.jobs.iter().any(|j| j.what.starts_with(crate::cmd::RECEIVING) || j.what.starts_with(crate::cmd::SENDING));
        if self.send_receive_min == 0 || self.accounts.is_empty() || self.connector.is_none() {
            self.timer.next = None;
            self.timer.every = 0;
            return None;
        }
        let every = u64::from(self.send_receive_min).saturating_mul(60_000);
        if busy {
            // Counted from its end: one asked for by the owner puts the next one off.
            self.timer.next = Some(now_ms.saturating_add(every));
            return Some(1_000);
        }
        if self.timer.every != self.send_receive_min {
            self.timer.every = self.send_receive_min;
            // The first of a run comes soon; after the minutes changed, as many from now.
            let wait = if self.timer.next.is_none() { FIRST_AFTER_MS.min(every) } else { every };
            self.timer.next = Some(now_ms.saturating_add(wait));
        }
        let next = *self.timer.next.get_or_insert(now_ms.saturating_add(every));
        let left = next.saturating_sub(now_ms);
        if left > 0 {
            return Some(left);
        }
        if !self.saves.is_empty() || !self.exports.is_empty() {
            return Some(1_000);
        }
        // A Send/Receive on its own is not something the owner did: the timer of saving on
        // its own goes on as it was.
        let quiet = self.auto.runs == self.auto.seen;
        let status = self.status.clone();
        let begun = self.run("mail.sendReceive", &json!({ "wait": false }));
        if quiet {
            self.auto.seen = self.auto.runs;
        }
        self.timer.next = Some(now_ms.saturating_add(every));
        if let Err(e) = begun {
            self.status = if status.is_empty() { format!("Send/Receive on its own did not begin: {e}") } else { status };
        }
        Some(1_000)
    }
}
