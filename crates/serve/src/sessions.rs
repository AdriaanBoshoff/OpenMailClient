//! Browsers signed in (M7.1, first part of M7.3): each holds a random token in a cookie of its
//! own; the server keeps only the token's SHA-256, so a look at the server's memory or a log
//! gives no way in. A session ends when it is left alone too long, when it has lasted its
//! longest, when it signs out, or when the password is set again (each session remembers the
//! password's setting it was opened under).

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use std::collections::HashMap;

/// A session left alone this long ends.
pub const IDLE_MS: u64 = 30 * 60 * 1000;
/// A session lasts at most this long.
pub const LONGEST_MS: u64 = 12 * 60 * 60 * 1000;
/// … or this long when "keep me signed in" was ticked, and then it is not ended for being idle
/// until a week has gone by.
pub const KEPT_MS: u64 = 30 * 24 * 60 * 60 * 1000;
pub const KEPT_IDLE_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// The most sessions at once; the one used longest ago makes room for a new one.
pub const MAX_SESSIONS: usize = 256;

/// The name of the cookie: `__Host-` makes browsers keep it to this host, over HTTPS, for every path.
pub const COOKIE: &str = "__Host-omc";

#[derive(Clone, Debug)]
struct Open {
    began_ms: u64,
    used_ms: u64,
    kept: bool,
    generation: u64,
}

#[derive(Default)]
pub struct Sessions {
    open: HashMap<[u8; 32], Open>,
}

fn digest(token: &str) -> Option<[u8; 32]> {
    // A token of ours is 43 characters of URL-safe base64; anything else is not looked up.
    if token.len() != 43 || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return None;
    }
    let d = ring::digest::digest(&ring::digest::SHA256, token.as_bytes());
    d.as_ref().try_into().ok()
}

impl Sessions {
    pub fn new() -> Sessions {
        Sessions::default()
    }

    /// Open a session; the token goes to the browser and is never kept here.
    pub fn open(&mut self, now_ms: u64, kept: bool, generation: u64) -> Result<String, String> {
        self.forget_ended(now_ms, generation);
        while self.open.len() >= MAX_SESSIONS {
            let Some(oldest) = self.open.iter().min_by_key(|(_, o)| o.used_ms).map(|(k, _)| *k) else { break };
            self.open.remove(&oldest);
        }
        let token = URL_SAFE_NO_PAD.encode(crate::random::<32>()?);
        let key = digest(&token).ok_or("a token that is not one")?;
        self.open.insert(key, Open { began_ms: now_ms, used_ms: now_ms, kept, generation });
        Ok(token)
    }

    /// Whether `token` is of a session still open under password setting `generation`; using it
    /// counts as activity.
    pub fn check(&mut self, token: &str, now_ms: u64, generation: u64) -> bool {
        let Some(key) = digest(token) else { return false };
        let Some(open) = self.open.get_mut(&key) else { return false };
        if ended(open, now_ms, generation) {
            self.open.remove(&key);
            return false;
        }
        open.used_ms = now_ms;
        true
    }

    /// Whether the session was opened with "keep me signed in", for how long its cookie lasts.
    pub fn kept(&self, token: &str) -> bool {
        digest(token).and_then(|k| self.open.get(&k)).is_some_and(|o| o.kept)
    }

    pub fn close(&mut self, token: &str) {
        if let Some(key) = digest(token) {
            self.open.remove(&key);
        }
    }

    /// End every session: when the password is set again they end of themselves, and this is for
    /// the owner who wants everyone out now.
    pub fn close_all(&mut self) {
        self.open.clear();
    }

    pub fn len(&self) -> usize {
        self.open.len()
    }

    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    pub fn forget_ended(&mut self, now_ms: u64, generation: u64) {
        self.open.retain(|_, o| !ended(o, now_ms, generation));
    }
}

fn ended(o: &Open, now_ms: u64, generation: u64) -> bool {
    let (longest, idle) = if o.kept { (KEPT_MS, KEPT_IDLE_MS) } else { (LONGEST_MS, IDLE_MS) };
    o.generation != generation || now_ms.saturating_sub(o.began_ms) >= longest || now_ms.saturating_sub(o.used_ms) >= idle || now_ms < o.began_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_end_when_idle_old_signed_out_or_the_password_changes() {
        let mut s = Sessions::new();
        let t = s.open(1000, false, 1).unwrap();
        assert_eq!(t.len(), 43);
        assert!(s.check(&t, 1000 + IDLE_MS - 1, 1));
        assert!(s.check(&t, 1000 + 2 * IDLE_MS - 2, 1), "use keeps it open");
        assert!(!s.check(&t, 1000 + 3 * IDLE_MS, 1), "idle too long");
        assert!(!s.check(&t, 1000, 1), "and it stays ended");

        let t = s.open(0, false, 1).unwrap();
        let mut now = 0;
        while now + IDLE_MS / 2 < LONGEST_MS {
            now += IDLE_MS / 2;
            assert!(s.check(&t, now, 1));
        }
        assert!(!s.check(&t, LONGEST_MS, 1), "lasted its longest");

        let t = s.open(0, true, 1).unwrap();
        assert!(s.kept(&t));
        assert!(s.check(&t, 6 * 24 * 60 * 60 * 1000, 1), "kept outlasts a day idle");
        assert!(!s.check(&t, 6 * 24 * 60 * 60 * 1000, 2), "the password was set again");

        let t = s.open(0, false, 1).unwrap();
        s.close(&t);
        assert!(!s.check(&t, 1, 1));
        for junk in ["", "x", &"a".repeat(43), &"é".repeat(22), &"a".repeat(10_000)] {
            assert!(!s.check(junk, 1, 1));
        }
    }

    #[test]
    fn sessions_are_bounded() {
        let mut s = Sessions::new();
        let first = s.open(0, false, 1).unwrap();
        for i in 0..MAX_SESSIONS as u64 * 2 {
            s.open(1 + i, false, 1).unwrap();
        }
        assert_eq!(s.len(), MAX_SESSIONS);
        assert!(!s.check(&first, 2, 1), "the one used longest ago made room");
    }
}
