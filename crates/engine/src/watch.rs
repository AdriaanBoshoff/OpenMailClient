//! New mail told of as it comes: the Inbox of each IMAP account is watched on a connection of
//! its own with IDLE ([RFC 2177]), and when the server says something in it changed, the
//! account is received from at once, as a Send/Receive of that account alone would.
//!
//! A watch runs on a thread of its own and never changes the mailbox: it says what it heard,
//! and `Session::watch_due`, on the session's thread, begins the receive. The Inbox is opened
//! with EXAMINE, so watching it changes nothing on the server, not even which messages are new.
//! A connection that is lost is made again, soon when it had been watching and later each time
//! it could not be made; a server that does not offer IDLE is received from on the timer only.
//! Watching is turned off with `mail.newMailAsItComes`.

use crate::Session;
use crate::accounts::{Account, Kind};
use crate::cmd::Reach;
use openmailclient_net::NetError;
use openmailclient_net::imap::Imap;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Duration;
use web_time::Instant;

/// Something the host gives so that it is woken when a watch heard something: the window
/// repaints, and asks `Session::send_receive_due`.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// How long the server is listened to at a time, so that a watch stops within about that long.
const LISTEN: Duration = Duration::from_secs(1);
/// IDLE is begun again this often: a server may end one that lasts more than 29 minutes
/// ([RFC 2177] 3).
const RENEW: Duration = Duration::from_secs(25 * 60);
/// A connection that had been watching a while and was lost is made again after this long; one
/// lost sooner is taken for one that could not be made, so a server that ends every wait at
/// once is not asked again and again.
const AGAIN_SOON_S: u64 = 10;
const WATCHED_A_WHILE: Duration = Duration::from_secs(60);
/// One that could not be made is tried again after this long, twice as long each time after,
/// up to the longest.
const AGAIN_FIRST_S: u64 = 60;
const AGAIN_MOST_S: u64 = 30 * 60;

/// What a watch heard, said to the session.
#[derive(Debug)]
enum Heard {
    /// The Inbox is watched.
    Watching,
    /// Something in it changed: mail came or went, or a message's flags changed.
    Changed,
    /// The connection was lost or could not be made, and is tried again in so many seconds.
    Lost { why: String, again_in_s: u64 },
    /// The server does not offer IDLE: the account is received from on the timer.
    NotOffered,
}

/// An account's Inbox being watched.
pub struct Watch {
    /// The account's id.
    pub account: String,
    /// What the watch was begun for: its server and name. Changed, it is begun again.
    key: String,
    stop: Arc<AtomicBool>,
    heard: Receiver<Heard>,
    /// What it is doing, in words for Show Progress.
    pub said: String,
    /// The Inbox is watched now.
    pub on: bool,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// What a watch is begun for: a change to any of it begins it again.
fn key_of(a: &Account) -> String {
    format!("{:?} {}", a.incoming_endpoint(), a.user)
}

impl Session {
    /// The Inboxes being watched for new mail, by account name, each with what it is doing.
    pub fn watching(&self) -> Vec<(String, String)> {
        self.watches
            .iter()
            .map(|w| (self.accounts.iter().find(|a| a.id == w.account).map(|a| a.name.clone()).unwrap_or_default(), w.said.clone()))
            .collect()
    }

    /// Begin and end watches as the accounts are now, take what they heard, and receive from
    /// an account whose Inbox changed. Asked by `send_receive_due`; returns in how many
    /// milliseconds to ask again, `None` when only a watch hearing something need wake it.
    pub(crate) fn watch_due(&mut self) -> Option<u64> {
        let want = self.new_mail_as_it_comes && self.connector.is_some() && !cfg!(target_arch = "wasm32");
        let wanted: Vec<(usize, String, String)> = if want {
            self.accounts.iter().enumerate().filter(|(_, a)| a.kind == Kind::Imap).map(|(at, a)| (at, a.id.clone(), key_of(a))).collect()
        } else {
            Vec::new()
        };
        self.watches.retain(|w| wanted.iter().any(|(_, id, key)| *id == w.account && *key == w.key));
        self.watch_pending.retain(|id| wanted.iter().any(|(_, w, _)| w == id));
        for (at, id, key) in &wanted {
            if self.watches.iter().any(|w| w.account == *id) {
                continue;
            }
            let Ok(reach) = Reach::of(self, *at) else { continue };
            if let Some(w) = begin(reach, id.clone(), key.clone(), self.waker.clone()) {
                self.watches.push(w);
            }
        }
        let receiving = self.jobs.iter().any(|j| j.what.starts_with(crate::cmd::RECEIVING));
        for w in &mut self.watches {
            loop {
                let heard = match w.heard.try_recv() {
                    Ok(h) => h,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        w.on = false;
                        break;
                    }
                };
                match heard {
                    Heard::Watching => {
                        w.on = true;
                        w.said = "Inbox watched: new mail comes as it arrives".into();
                    }
                    // A change heard while the account is being received from may be that
                    // receive's own doing, or may have come after the receive looked: it is
                    // received again once that receive ends. One more receive that finds
                    // nothing costs little; a change from elsewhere left unseen does not.
                    Heard::Changed => {
                        if !self.watch_pending.contains(&w.account) {
                            self.watch_pending.push(w.account.clone());
                        }
                    }
                    Heard::Lost { why, again_in_s } => {
                        w.on = false;
                        let when = if again_in_s < 60 { format!("{again_in_s} seconds") } else { format!("{} minutes", again_in_s.div_ceil(60)) };
                        w.said = format!("Inbox not watched ({why}); tried again in {when}");
                    }
                    Heard::NotOffered => {
                        w.on = false;
                        w.said = "the server does not tell of new mail as it comes; it is looked for on the timer".into();
                    }
                }
            }
        }
        if self.watch_pending.is_empty() {
            return None;
        }
        if receiving || !self.saves.is_empty() || !self.exports.is_empty() {
            return Some(1_000);
        }
        let id = self.watch_pending.remove(0);
        // Receiving on its own is not something the owner did: saving on its own goes on as
        // it was, and what the status bar said stays unless the receive has something to say.
        let quiet = self.auto.runs == self.auto.seen;
        let status = self.status.clone();
        let begun = self.run("account.receive", &json!({ "id": id, "wait": false }));
        if quiet {
            self.auto.seen = self.auto.runs;
        }
        if begun.is_err() {
            self.status = status;
        }
        Some(1_000)
    }
}

/// Begin watching an account's Inbox on a thread of its own.
fn begin(reach: Reach, account: String, key: String, waker: Option<Waker>) -> Option<Watch> {
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, heard) = channel();
    let theirs = Arc::clone(&stop);
    std::thread::Builder::new()
        .name("watch".to_string())
        .spawn(move || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| watch(&reach, &theirs, &tx, waker.as_ref())));
        })
        .ok()?;
    Some(Watch { account, key, stop, heard, said: "Inbox being watched…".into(), on: false })
}

/// How a connection's watching ended.
enum End {
    /// Asked to stop, or the session is gone.
    Stopped,
    NotOffered,
    /// Lost; `watched` when it had been watching a while.
    Lost {
        e: NetError,
        watched: bool,
    },
}

/// The watch's thread: watch, and when the connection is lost make it again.
fn watch(reach: &Reach, stop: &AtomicBool, tx: &Sender<Heard>, waker: Option<&Waker>) {
    let tell = |h: Heard| {
        let sent = tx.send(h).is_ok();
        if let Some(wake) = waker {
            wake();
        }
        sent
    };
    let mut again_s = 0u64;
    while !stop.load(Ordering::Relaxed) {
        let (e, watched) = match watch_once(reach, stop, &tell) {
            End::Stopped => return,
            End::NotOffered => {
                tell(Heard::NotOffered);
                return;
            }
            End::Lost { e, watched } => (e, watched),
        };
        again_s = match &e {
            _ if watched => AGAIN_SOON_S,
            // What the owner must put right is not asked of the server again and again.
            NetError::Auth(_) | NetError::Untrusted(_) | NetError::Certificate(_) | NetError::NotSecure | NetError::Unsupported(_) => AGAIN_MOST_S,
            _ if again_s < AGAIN_FIRST_S => AGAIN_FIRST_S,
            _ => again_s.saturating_mul(2).min(AGAIN_MOST_S),
        };
        if !tell(Heard::Lost { why: e.to_string(), again_in_s: again_s }) {
            return;
        }
        for _ in 0..again_s {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

/// One connection's watching, until it is lost or stopped.
fn watch_once(reach: &Reach, stop: &AtomicBool, tell: &dyn Fn(Heard) -> bool) -> End {
    let lost = |e| End::Lost { e, watched: false };
    let login = match reach.login() {
        Ok(l) => l,
        Err(e) => return lost(e),
    };
    let mut imap = match Imap::open(&*reach.connector, &reach.account.incoming_endpoint(), &login) {
        Ok(i) => i,
        Err(e) => return lost(e),
    };
    if !imap.has("IDLE") {
        let _ = imap.logout();
        return End::NotOffered;
    }
    if let Err(e) = imap.examine("INBOX") {
        return lost(e);
    }
    if !tell(Heard::Watching) {
        return End::Stopped;
    }
    let since = Instant::now();
    loop {
        let begun = Instant::now();
        let woke = match imap.idle(LISTEN, &mut || !stop.load(Ordering::Relaxed) && begun.elapsed() < RENEW) {
            Ok(w) => w,
            Err(e) => return End::Lost { e, watched: since.elapsed() >= WATCHED_A_WHILE },
        };
        if stop.load(Ordering::Relaxed) {
            let _ = imap.logout();
            return End::Stopped;
        }
        if woke.changed && !tell(Heard::Changed) {
            let _ = imap.logout();
            return End::Stopped;
        }
    }
}
