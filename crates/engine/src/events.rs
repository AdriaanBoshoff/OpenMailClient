//! What agents are told of (M6.5): mail that came, work that ended and reminders that came due,
//! each numbered over the run, so that an agent asking every so often with `mail.events` hears
//! of each once, whatever the window has shown or put away meanwhile. What a message, an
//! appointment or a task says of itself is under `untrusted_content`, as everywhere.

use serde_json::{Value, json};
use std::collections::VecDeque;

/// Events kept; an agent that asks less often than this many come is told it missed some.
pub const MAX_EVENTS: usize = 200;

/// The events of the run, the newest last.
#[derive(Debug, Default)]
pub struct Events {
    list: VecDeque<Value>,
    /// The number of the last one.
    last: u64,
}

impl Events {
    /// Note one more: `kind` names it, and `what` (an object) says what happened.
    pub fn note(&mut self, kind: &str, what: Value) {
        self.last = self.last.saturating_add(1);
        let mut event = json!({ "seq": self.last, "kind": kind });
        if let (Some(event), Value::Object(what)) = (event.as_object_mut(), what) {
            for (k, v) in what {
                event.entry(k).or_insert(v);
            }
        }
        self.list.push_back(event);
        while self.list.len() > MAX_EVENTS {
            self.list.pop_front();
        }
    }

    /// The number of the last event; 0 before the first.
    pub fn last(&self) -> u64 {
        self.last
    }

    /// Those after `seq`, the oldest first, and whether some after it are no longer kept.
    pub fn since(&self, seq: u64) -> (Vec<Value>, bool) {
        let first = self.list.front().and_then(|e| e.get("seq")).and_then(Value::as_u64).unwrap_or(self.last.saturating_add(1));
        let missed = seq < self.last && seq.saturating_add(1) < first;
        let after: Vec<Value> = self.list.iter().filter(|e| e.get("seq").and_then(Value::as_u64).is_some_and(|n| n > seq)).cloned().collect();
        (after, missed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_is_told_once_and_what_was_let_go_is_said() {
        let mut e = Events::default();
        assert_eq!(e.since(0), (Vec::new(), false));
        e.note("jobEnded", json!({ "job": "Sending", "ok": true }));
        e.note("newMail", json!({ "seq": 99, "store": 0 }));
        let (got, missed) = e.since(0);
        assert!(!missed);
        assert_eq!(got.len(), 2);
        // The event's own number is not written over by what it carries.
        assert_eq!(got[1]["seq"], 2);
        assert_eq!(got[0]["job"], "Sending");
        assert_eq!(e.since(1).0.len(), 1);
        assert!(e.since(2).0.is_empty());
        // Far past the end is nothing; a number never given is not "missed".
        assert_eq!(e.since(u64::MAX), (Vec::new(), false));
        for i in 0..(MAX_EVENTS + 5) {
            e.note("jobEnded", json!({ "i": i }));
        }
        let (got, missed) = e.since(0);
        assert!(missed);
        assert_eq!(got.len(), MAX_EVENTS);
        assert!(!e.since(e.last() - 3).1);
    }
}
