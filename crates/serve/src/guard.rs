//! Sign-in limits and bans (M7.4), decided before a password is checked.
//!
//! Each address may fail [`TRIES`] times in [`WINDOW_MS`]; the failure after that bans it for
//! [`FIRST_BAN_MS`]. A ban that starts within [`REPEAT_WITHIN_MS`] of the last one ending lasts
//! twice as long, to [`LONGEST_BAN_MS`], and the one after a ban of that length lasts until the
//! owner lifts it. An IPv6 address is counted with the rest of its /64, since one home or phone
//! is given a whole /64; an IPv4 address written as IPv6 is counted as itself. A banned address
//! is refused without its password being checked, so a ban costs the host nothing and tells the
//! guesser nothing new.
//!
//! Failures before a ban make each next try wait longer, and failures from many addresses at once
//! make every try wait ([`Verdict::Try`]), so guessing cannot be shared out among addresses.
//! Addresses the owner names ([`Guard::never_ban`]) are slowed but never banned.
//!
//! Time is given, not read, so the rules can be tested; what is tracked is bounded however many
//! addresses come.

use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::net::{IpAddr, Ipv6Addr};
use std::path::Path;

/// Failures an address may have in [`WINDOW_MS`] before the next one bans it.
pub const TRIES: usize = 5;
/// The window failures are counted in.
pub const WINDOW_MS: u64 = 15 * 60 * 1000;
/// How long a first ban lasts.
pub const FIRST_BAN_MS: u64 = 60 * 60 * 1000;
/// The longest ban that ends on its own.
pub const LONGEST_BAN_MS: u64 = 24 * 60 * 60 * 1000;
/// A ban starting this soon after the last one ended is a repeat, and lasts twice as long.
pub const REPEAT_WITHIN_MS: u64 = 24 * 60 * 60 * 1000;
/// Failures over all addresses in [`ALARM_WINDOW_MS`] at which every try is slowed.
pub const ALARM_FAILURES: usize = 30;
/// The window failures over all addresses are counted in.
pub const ALARM_WINDOW_MS: u64 = 10 * 60 * 1000;
/// How long every try waits while failures come from many places at once.
pub const ALARM_WAIT_MS: u64 = 3000;
/// The most addresses whose failures are tracked; the oldest are let go first.
pub const MAX_TRACKED: usize = 50_000;
/// The most bans kept; past it, the ones ending soonest are let go first.
pub const MAX_BANS: usize = 50_000;
/// How often everything tracked is looked over for what to forget.
const SWEEP_EVERY_MS: u64 = 1000;
/// The most addresses kept from being banned.
pub const MAX_NEVER: usize = 256;

/// What to do with a sign-in from an address, before its password is looked at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Check the password, after waiting this long (0 for not at all).
    Try { wait_ms: u64 },
    /// Refuse without checking: banned until then, or for good (`None`).
    Banned { until_ms: Option<u64> },
}

/// A ban as kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ban {
    /// When it began.
    pub since_ms: u64,
    /// When it ends, or `None` for when the owner lifts it.
    pub until_ms: Option<u64>,
    /// Which ban this is in a row, each within a day of the last: 1 for the first.
    pub level: u32,
}

impl Ban {
    /// Whether it holds at `now_ms`.
    pub fn holds(&self, now_ms: u64) -> bool {
        self.until_ms.is_none_or(|until| now_ms < until)
    }
}

/// What a failure led to, to be logged and told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failed {
    /// Counted; this many tries are left before a ban.
    Counted { left: usize },
    /// The address is now banned.
    Banned(Ban),
    /// The address is one never banned; counted only to slow it.
    Spared,
}

/// A network of addresses, as the owner names them to be kept from bans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Net {
    pub addr: IpAddr,
    pub prefix: u8,
}

impl Net {
    /// `192.168.1.0/24`, `fd00::/8` or a single address.
    pub fn parse(text: &str) -> Option<Net> {
        let text = text.trim();
        let (addr, prefix) = match text.split_once('/') {
            Some((a, p)) => (a.parse::<IpAddr>().ok()?, Some(p.parse::<u8>().ok()?)),
            None => (text.parse::<IpAddr>().ok()?, None),
        };
        let addr = plain(addr);
        let most = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = prefix.unwrap_or(most);
        (prefix <= most).then_some(Net { addr, prefix })
    }

    /// Whether `addr` is in it.
    pub fn holds(&self, addr: IpAddr) -> bool {
        match (self.addr, plain(addr)) {
            (IpAddr::V4(net), IpAddr::V4(a)) => mask(u128::from(u32::from(net)), 32, self.prefix) == mask(u128::from(u32::from(a)), 32, self.prefix),
            (IpAddr::V6(net), IpAddr::V6(a)) => mask(u128::from(net), 128, self.prefix) == mask(u128::from(a), 128, self.prefix),
            _ => false,
        }
    }
}

impl std::fmt::Display for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix)
    }
}

/// The top `prefix` bits of a `width`-bit number.
fn mask(value: u128, width: u32, prefix: u8) -> u128 {
    let keep = u32::from(prefix).min(width);
    if keep == 0 {
        return 0;
    }
    let drop = width - keep;
    value.checked_shr(drop).unwrap_or(0)
}

/// An IPv4 address written as IPv6 read as itself.
fn plain(addr: IpAddr) -> IpAddr {
    match addr {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    }
}

/// The address failures and bans are counted against: an IPv4 address itself, an IPv6 one's /64.
pub fn counted_as(addr: IpAddr) -> IpAddr {
    match plain(addr) {
        IpAddr::V6(v6) => IpAddr::V6(Ipv6Addr::from(u128::from(v6) & !((1u128 << 64) - 1))),
        v4 => v4,
    }
}

/// Shown for an address counted as [`counted_as`] gives: `203.0.113.7` or `2001:db8:1:2::/64`.
pub fn shown(counted: IpAddr) -> String {
    match counted {
        IpAddr::V6(v6) => format!("{v6}/64"),
        v4 => v4.to_string(),
    }
}

/// An address as the owner writes it to lift a ban: an address, or an IPv6 one with `/64`.
pub fn parse_address(text: &str) -> Option<IpAddr> {
    let text = text.trim();
    let text = text.strip_suffix("/64").unwrap_or(text);
    text.parse::<IpAddr>().ok().map(counted_as)
}

/// The limits and bans of one host.
#[derive(Clone, Debug, Default)]
pub struct Guard {
    /// Recent failures per counted address, oldest first, and when the address last failed.
    failures: HashMap<IpAddr, VecDeque<u64>>,
    /// Bans, held or ended within [`REPEAT_WITHIN_MS`] (so a repeat is known).
    bans: BTreeMap<IpAddr, Ban>,
    /// Recent failures over all addresses.
    all: VecDeque<u64>,
    /// Kept from being banned.
    never: Vec<Net>,
    /// When everything was last looked over for what is old enough to forget.
    swept_ms: Option<u64>,
}

impl Guard {
    pub fn new() -> Guard {
        Guard::default()
    }

    /// Keep these networks from being banned (the home network, say). At most [`MAX_NEVER`].
    pub fn never_ban(&mut self, nets: impl IntoIterator<Item = Net>) {
        self.never = nets.into_iter().take(MAX_NEVER).collect();
    }

    /// The networks kept from being banned.
    pub fn spared(&self) -> &[Net] {
        &self.never
    }

    fn is_spared(&self, addr: IpAddr) -> bool {
        self.never.iter().any(|n| n.holds(addr))
    }

    /// Whether a sign-in from `addr` may be tried at `now_ms`, and after how long a wait.
    pub fn check(&mut self, addr: IpAddr, now_ms: u64) -> Verdict {
        let key = counted_as(addr);
        self.forget_old(now_ms);
        if let Some(ban) = self.bans.get(&key).filter(|b| b.holds(now_ms))
            && !self.is_spared(addr)
        {
            return Verdict::Banned { until_ms: ban.until_ms };
        }
        let since = now_ms.saturating_sub(WINDOW_MS);
        let mine = self.failures.get(&key).map_or(0, |list| list.iter().filter(|t| **t >= since).count());
        // Each failure after the first two waits twice as long as the one before: 1 s, 2 s, 4 s.
        let own = match mine {
            0..=1 => 0,
            n => 1000u64.saturating_mul(1u64 << (n - 2).min(6)),
        };
        let alarm = if self.all.len() >= ALARM_FAILURES { ALARM_WAIT_MS } else { 0 };
        Verdict::Try { wait_ms: own.max(alarm) }
    }

    /// The password from `addr` was wrong.
    pub fn failed(&mut self, addr: IpAddr, now_ms: u64) -> Failed {
        let key = counted_as(addr);
        self.forget_old(now_ms);
        self.all.push_back(now_ms);
        while self.all.len() > MAX_TRACKED {
            self.all.pop_front();
        }
        if !self.failures.contains_key(&key) && self.failures.len() >= MAX_TRACKED {
            self.let_go_oldest();
        }
        let since = now_ms.saturating_sub(WINDOW_MS);
        let list = self.failures.entry(key).or_default();
        while list.front().is_some_and(|t| *t < since) {
            list.pop_front();
        }
        list.push_back(now_ms);
        while list.len() > TRIES + 1 {
            list.pop_front();
        }
        let count = list.len();
        if self.is_spared(addr) {
            return Failed::Spared;
        }
        if count <= TRIES {
            return Failed::Counted { left: TRIES - count };
        }
        self.failures.remove(&key);
        let ban = self.ban(key, now_ms);
        Failed::Banned(ban)
    }

    /// The password from `addr` was right: its failures are forgotten. Its bans' history stays.
    pub fn succeeded(&mut self, addr: IpAddr) {
        self.failures.remove(&counted_as(addr));
    }

    fn ban(&mut self, key: IpAddr, now_ms: u64) -> Ban {
        let level = match self.bans.get(&key) {
            Some(last) => match last.until_ms {
                Some(ended) if now_ms.saturating_sub(ended) < REPEAT_WITHIN_MS => last.level.saturating_add(1),
                Some(_) => 1,
                None => last.level,
            },
            None => 1,
        };
        let ban = Ban { since_ms: now_ms, until_ms: length(level).map(|ms| now_ms.saturating_add(ms)), level };
        if !self.bans.contains_key(&key) && self.bans.len() >= MAX_BANS {
            self.let_go_soonest_ban();
        }
        self.bans.insert(key, ban);
        ban
    }

    /// Ban `addr` as the owner asks, until `until_ms` or for good.
    pub fn ban_by_hand(&mut self, addr: IpAddr, now_ms: u64, until_ms: Option<u64>) {
        let key = counted_as(addr);
        if !self.bans.contains_key(&key) && self.bans.len() >= MAX_BANS {
            self.let_go_soonest_ban();
        }
        self.bans.insert(key, Ban { since_ms: now_ms, until_ms, level: 0 });
    }

    /// Lift the ban on `addr` (and forget its failures and history); whether there was one.
    pub fn unban(&mut self, addr: IpAddr) -> bool {
        let key = counted_as(addr);
        self.failures.remove(&key);
        self.bans.remove(&key).is_some()
    }

    /// The bans holding at `now_ms`, by address as counted.
    pub fn bans(&self, now_ms: u64) -> Vec<(IpAddr, Ban)> {
        self.bans.iter().filter(|(_, b)| b.holds(now_ms)).map(|(a, b)| (*a, *b)).collect()
    }

    /// Failures over the window just gone, over all addresses.
    pub fn recent_failures(&self) -> usize {
        self.all.len()
    }

    /// Forget what is old: failures over all addresses each time, and, at most once a second,
    /// each address's failures and the bans long ended (so many addresses cost no more than a
    /// look over them a second).
    fn forget_old(&mut self, now_ms: u64) {
        let alarm_since = now_ms.saturating_sub(ALARM_WINDOW_MS);
        while self.all.front().is_some_and(|t| *t < alarm_since) {
            self.all.pop_front();
        }
        if self.swept_ms.is_some_and(|at| now_ms >= at && now_ms - at < SWEEP_EVERY_MS) {
            return;
        }
        self.swept_ms = Some(now_ms);
        let since = now_ms.saturating_sub(WINDOW_MS);
        self.failures.retain(|_, list| {
            while list.front().is_some_and(|t| *t < since) {
                list.pop_front();
            }
            !list.is_empty()
        });
        self.bans.retain(|_, b| b.until_ms.is_none_or(|until| now_ms.saturating_sub(until) < REPEAT_WITHIN_MS));
    }

    fn let_go_oldest(&mut self) {
        let oldest = self.failures.iter().min_by_key(|(_, list)| list.back().copied().unwrap_or(0)).map(|(a, _)| *a);
        if let Some(a) = oldest {
            self.failures.remove(&a);
        }
    }

    fn let_go_soonest_ban(&mut self) {
        let soonest = self.bans.iter().filter(|(_, b)| b.until_ms.is_some()).min_by_key(|(_, b)| b.until_ms).map(|(a, _)| *a);
        let soonest = soonest.or_else(|| self.bans.keys().next().copied());
        if let Some(a) = soonest {
            self.bans.remove(&a);
        }
    }

    /// The bans and spared networks as kept in their file; failures are not kept.
    pub fn to_json(&self) -> Value {
        let bans: Vec<Value> =
            self.bans.iter().map(|(a, b)| json!({"address": shown(*a), "since": b.since_ms, "until": b.until_ms, "level": b.level})).collect();
        let never: Vec<String> = self.never.iter().map(Net::to_string).collect();
        json!({"bans": bans, "neverBan": never})
    }

    /// Read back what [`Guard::to_json`] wrote; anything not understood is left out, never fatal.
    pub fn from_json(value: &Value) -> Guard {
        let mut guard = Guard::new();
        for entry in value.get("bans").and_then(Value::as_array).into_iter().flatten().take(MAX_BANS) {
            let Some(addr) = entry.get("address").and_then(Value::as_str).and_then(parse_address) else { continue };
            let Some(since_ms) = entry.get("since").and_then(Value::as_u64) else { continue };
            let until_ms = entry.get("until").and_then(Value::as_u64);
            if entry.get("until").is_some_and(|u| !u.is_null() && !u.is_u64()) {
                continue;
            }
            let level = entry.get("level").and_then(Value::as_u64).map_or(1, |l| u32::try_from(l.min(64)).unwrap_or(1));
            guard.bans.insert(addr, Ban { since_ms, until_ms, level });
        }
        let never = value.get("neverBan").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).filter_map(Net::parse);
        guard.never_ban(never);
        guard
    }

    /// The guard kept in `dir`, or a new one when there is none.
    pub fn load(dir: &Path) -> Result<Guard, String> {
        let path = crate::bans_file(dir);
        let Some(text) = crate::read_small(&path)? else { return Ok(Guard::new()) };
        let value: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Guard::from_json(&value))
    }

    /// Take the bans and spared networks of `kept`, read again after the console changed them
    /// (`openmailclient-cli web unban`), keeping the failures counted here.
    pub fn take_kept(&mut self, kept: Guard) {
        self.bans = kept.bans;
        self.never = kept.never;
    }

    /// Keep the bans and spared networks in `dir`.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        crate::write_private(&crate::bans_file(dir), &format!("{:#}\n", self.to_json()))
    }
}

/// How long the ban at `level` lasts: an hour doubled for each level past the first, at most a
/// day; `None` (for good) once a ban of a day has been and another follows it.
pub fn length(level: u32) -> Option<u64> {
    let level = level.max(1);
    let mut ms = FIRST_BAN_MS;
    for _ in 1..level {
        if ms >= LONGEST_BAN_MS {
            return None;
        }
        ms = ms.saturating_mul(2).min(LONGEST_BAN_MS);
    }
    Some(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60 * 1000;
    const HOUR: u64 = 60 * MIN;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn fail_until_banned(g: &mut Guard, a: IpAddr, now: u64) -> Ban {
        for i in 0..TRIES {
            assert_eq!(g.failed(a, now), Failed::Counted { left: TRIES - 1 - i });
        }
        match g.failed(a, now) {
            Failed::Banned(b) => b,
            other => panic!("not banned: {other:?}"),
        }
    }

    #[test]
    fn the_sixth_failure_in_a_quarter_hour_bans_for_an_hour() {
        let mut g = Guard::new();
        let a = ip("203.0.113.7");
        assert_eq!(g.check(a, 0), Verdict::Try { wait_ms: 0 });
        let ban = fail_until_banned(&mut g, a, 1000);
        assert_eq!(ban, Ban { since_ms: 1000, until_ms: Some(1000 + HOUR), level: 1 });
        assert_eq!(g.check(a, 1000 + HOUR - 1), Verdict::Banned { until_ms: Some(1000 + HOUR) });
        assert_eq!(g.check(a, 1000 + HOUR), Verdict::Try { wait_ms: 0 });
        // Another address is untouched.
        assert_eq!(g.check(ip("203.0.113.8"), 2000), Verdict::Try { wait_ms: 0 });
    }

    #[test]
    fn failures_spread_out_are_forgotten() {
        let mut g = Guard::new();
        let a = ip("198.51.100.1");
        for i in 0..20u64 {
            let r = g.failed(a, i * 4 * MIN);
            assert!(!matches!(r, Failed::Banned(_)), "banned at failure {i}");
        }
    }

    #[test]
    fn tries_wait_longer_after_each_failure() {
        let mut g = Guard::new();
        let a = ip("198.51.100.2");
        let waits: Vec<u64> = (0..5)
            .map(|_| {
                let w = g.check(a, 0);
                g.failed(a, 0);
                match w {
                    Verdict::Try { wait_ms } => wait_ms,
                    Verdict::Banned { .. } => u64::MAX,
                }
            })
            .collect();
        assert_eq!(waits, [0, 0, 1000, 2000, 4000]);
        g.succeeded(a);
        assert_eq!(g.check(a, 0), Verdict::Try { wait_ms: 0 });
    }

    #[test]
    fn bans_within_a_day_double_to_a_day_then_last_until_lifted() {
        let mut g = Guard::new();
        let a = ip("192.0.2.9");
        let mut now = 0;
        let mut lengths = Vec::new();
        for _ in 0..7 {
            let ban = fail_until_banned(&mut g, a, now);
            lengths.push(ban.until_ms.map(|u| (u - now) / HOUR));
            now = ban.until_ms.unwrap_or(now) + MIN;
        }
        assert_eq!(lengths, [Some(1), Some(2), Some(4), Some(8), Some(16), Some(24), None]);
        assert_eq!(g.check(a, now + 365 * 24 * HOUR), Verdict::Banned { until_ms: None });
        assert!(g.unban(a));
        assert_eq!(g.check(a, now), Verdict::Try { wait_ms: 0 });
        assert!(!g.unban(a));
    }

    #[test]
    fn a_ban_a_day_after_the_last_starts_again_at_an_hour() {
        let mut g = Guard::new();
        let a = ip("192.0.2.10");
        let first = fail_until_banned(&mut g, a, 0);
        let later = first.until_ms.unwrap() + REPEAT_WITHIN_MS + MIN;
        assert_eq!(fail_until_banned(&mut g, a, later).level, 1);
    }

    #[test]
    fn an_ipv6_address_is_counted_with_its_64() {
        let mut g = Guard::new();
        for i in 0..TRIES {
            g.failed(ip(&format!("2001:db8:1:2::{:x}", i + 1)), 0);
        }
        assert!(matches!(g.failed(ip("2001:db8:1:2:ffff::1"), 0), Failed::Banned(_)));
        assert!(matches!(g.check(ip("2001:db8:1:2:abcd::9"), 1), Verdict::Banned { .. }));
        assert_eq!(g.check(ip("2001:db8:1:3::1"), 1), Verdict::Try { wait_ms: 0 });
        assert_eq!(g.bans(1).first().map(|(a, _)| shown(*a)), Some("2001:db8:1:2::/64".to_string()));
        assert!(g.unban(parse_address("2001:db8:1:2::/64").unwrap()));
    }

    #[test]
    fn an_ipv4_address_written_as_ipv6_is_itself() {
        let mut g = Guard::new();
        fail_until_banned(&mut g, ip("::ffff:192.0.2.1"), 0);
        assert!(matches!(g.check(ip("192.0.2.1"), 1), Verdict::Banned { .. }));
        assert_eq!(g.check(ip("192.0.2.2"), 1), Verdict::Try { wait_ms: 0 });
    }

    #[test]
    fn spared_networks_are_slowed_but_never_banned() {
        let mut g = Guard::new();
        g.never_ban([Net::parse("192.168.1.0/24").unwrap(), Net::parse("fd00::/8").unwrap()]);
        for _ in 0..20 {
            assert_eq!(g.failed(ip("192.168.1.20"), 0), Failed::Spared);
        }
        assert!(matches!(g.check(ip("192.168.1.20"), 0), Verdict::Try { wait_ms } if wait_ms > 0));
        assert_eq!(g.failed(ip("fd12::1"), 0), Failed::Spared);
        assert_eq!(g.failed(ip("192.168.2.20"), 0), Failed::Counted { left: TRIES - 1 });
    }

    #[test]
    fn failures_from_many_addresses_slow_everyone() {
        let mut g = Guard::new();
        for i in 0..ALARM_FAILURES {
            g.failed(ip(&format!("198.18.{}.{}", i / 250, i % 250 + 1)), 0);
        }
        assert_eq!(g.check(ip("203.0.113.200"), 1), Verdict::Try { wait_ms: ALARM_WAIT_MS });
        assert_eq!(g.check(ip("203.0.113.200"), ALARM_WINDOW_MS + 1), Verdict::Try { wait_ms: 0 });
    }

    #[test]
    fn what_is_tracked_stays_bounded() {
        let mut g = Guard::new();
        for i in 0..(MAX_TRACKED as u32 + 100) {
            g.failed(IpAddr::from(std::net::Ipv4Addr::from(0x0a00_0000 + i)), u64::from(i) / 1000);
        }
        assert!(g.failures.len() <= MAX_TRACKED);
        assert!(g.all.len() <= MAX_TRACKED);
    }

    #[test]
    fn bans_outlast_a_restart() {
        let dir = std::env::temp_dir().join(format!("omc-web-bans-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut g = Guard::load(&dir).unwrap();
        g.never_ban(Net::parse("10.0.0.0/8"));
        fail_until_banned(&mut g, ip("203.0.113.50"), 0);
        g.ban_by_hand(ip("2001:db8::1"), 0, None);
        g.save(&dir).unwrap();
        let back = Guard::load(&dir).unwrap();
        assert_eq!(back.bans(1), g.bans(1));
        assert_eq!(back.spared(), g.spared());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bans_file_not_ours_is_read_as_far_as_it_makes_sense() {
        for junk in [
            json!(null),
            json!([]),
            json!({"bans": "x"}),
            json!({"bans": [1, null, {"address": "not an address", "since": 1}, {"address": "1.2.3.4"}, {"address": "1.2.3.4", "since": -1}, {"address": "1.2.3.4", "since": 1, "until": "soon"}]}),
            json!({"neverBan": ["1.2.3.4/99", "::/200", 7, "x"]}),
        ] {
            let g = Guard::from_json(&junk);
            assert!(g.bans(0).is_empty(), "{junk}");
            assert!(g.spared().is_empty(), "{junk}");
        }
        let g = Guard::from_json(&json!({"bans": [{"address": "1.2.3.4", "since": 1, "until": null, "level": 99999999999u64}]}));
        assert_eq!(g.bans(5).len(), 1);
    }

    #[test]
    fn networks_are_read_and_matched() {
        let n = Net::parse("192.168.0.0/16").unwrap();
        assert!(n.holds(ip("192.168.200.1")) && !n.holds(ip("192.169.0.1")) && !n.holds(ip("fe80::1")));
        assert!(Net::parse("0.0.0.0/0").unwrap().holds(ip("8.8.8.8")));
        assert!(Net::parse("::/0").unwrap().holds(ip("2001:db8::1")));
        assert!(Net::parse("203.0.113.5").unwrap().holds(ip("::ffff:203.0.113.5")));
        for bad in ["", "/", "1.2.3.4/", "1.2.3.4/33", "::/129", "x/8", "1.2.3.4/-1"] {
            assert_eq!(Net::parse(bad), None, "{bad}");
        }
    }
}
