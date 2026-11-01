//! Time zones with their whole history, from the system's own zone database: the tz files under
//! `/usr/share/zoneinfo` on Linux, macOS and the BSDs (TZif, RFC 8536), or the rules Windows
//! keeps in its registry for each year a zone changed (its Dynamic DST). The zones this program
//! knows keep their names and today's rules; the system's history gives the years before, and
//! a zone the system knows and this program does not is known by the system's name.
//!
//! Nothing is written, and what is read is capped and checked (`openmailclient_model::tzif`).
//! Hosts turn it on ([`use_system`]); until then, as in tests, zones are what this program
//! knows alone.

use openmailclient_model::tzif::{self, History};
use openmailclient_model::zone::Zone;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Where zones' histories are found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Source {
    /// Nowhere: the zones this program knows, as it knows them.
    #[default]
    None,
    /// A directory of tz files, by name ("Europe/Berlin" is `Europe/Berlin` in it).
    Files(PathBuf),
    /// Windows' registry, by the zone's key ("W. Europe Standard Time").
    Registry,
}

static SYSTEM: OnceLock<Source> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static TESTING: std::cell::RefCell<Option<Source>> = const { std::cell::RefCell::new(None) };
}

/// Zones' histories are found where the system keeps them, from now on. Called once by a host.
pub fn use_system() {
    let _ = SYSTEM.set(system());
}

/// Where this system keeps its zone database: `TZDIR` where it is set, else the usual places;
/// the registry on Windows; nowhere in a browser.
pub fn system() -> Source {
    if cfg!(windows) {
        return Source::Registry;
    }
    if cfg!(target_arch = "wasm32") {
        return Source::None;
    }
    let tzdir = std::env::var_os("TZDIR").map(PathBuf::from);
    let usual = ["/usr/share/zoneinfo", "/usr/lib/zoneinfo", "/usr/share/lib/zoneinfo"].map(PathBuf::from);
    tzdir.into_iter().chain(usual).find(|d| d.is_dir()).map_or(Source::None, Source::Files)
}

/// The tz files in `dir` are read for this thread, as a test asks.
#[cfg(test)]
pub(crate) fn testing(source: Option<Source>) {
    TESTING.with(|t| *t.borrow_mut() = source);
}

fn source() -> Source {
    #[cfg(test)]
    if let Some(s) = TESTING.with(|t| t.borrow().clone()) {
        return s;
    }
    SYSTEM.get().cloned().unwrap_or_default()
}

/// The zone known by `name` (a tz database name or Outlook's key, ignoring case), with its
/// whole history where the system has it; `None` when neither this program nor the system
/// knows it.
pub fn named(name: &str) -> Option<Zone> {
    let source = source();
    match Zone::named(name) {
        Some(zone) => Some(with_history(&source, zone)),
        // A name only the system knows, as its file or key gives it.
        None => {
            let name = name.trim();
            let bare = if matches!(source, Source::Registry) {
                Zone { key: name.to_string(), ..Zone::default() }
            } else {
                Zone { id: name.to_string(), ..Zone::default() }
            };
            history(&source, &bare).filter(|h| h.after.is_some() || !h.years.is_empty()).map(|h| bare.with_history(&h))
        }
    }
}

/// `zone` with the history the system keeps for it laid over its own rules; as it is where the
/// system keeps none.
pub fn with_history(source: &Source, zone: Zone) -> Zone {
    match history(source, &zone) {
        Some(h) => zone.with_history(&h),
        None => zone,
    }
}

/// A name that is a path within a directory of tz files: letters, digits and `_+-`, in
/// parts of at most 64 joined by `/`, none of them `.` or `..`.
fn safe_name(name: &str) -> bool {
    name.len() <= 128
        && !name.is_empty()
        && name.split('/').all(|part| {
            !part.is_empty() && part.len() <= 64 && !part.starts_with('.') && part.chars().all(|c| c.is_ascii_alphanumeric() || "_+-".contains(c))
        })
}

fn history(source: &Source, zone: &Zone) -> Option<History> {
    match source {
        Source::None => None,
        Source::Files(dir) => {
            if !safe_name(&zone.id) {
                return None;
            }
            let path = zone.id.split('/').fold(dir.clone(), |p, part| p.join(part));
            let len = std::fs::metadata(&path).ok().filter(|m| m.is_file())?.len();
            if len > tzif::MAX_FILE as u64 {
                return None;
            }
            tzif::read(&std::fs::read(path).ok()?)
        }
        Source::Registry => registry(&zone.key),
    }
}

#[cfg(windows)]
fn registry(key: &str) -> Option<History> {
    use windows_registry::LOCAL_MACHINE;
    // A key is a name of words ("W. Europe Standard Time"); nothing that could leave the key.
    if key.is_empty() || key.len() > 128 || key.contains(['\\', '/']) {
        return None;
    }
    let dynamic = LOCAL_MACHINE.open(format!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Time Zones\{key}\Dynamic DST")).ok()?;
    let first = i64::from(dynamic.get_u32("FirstEntry").ok()?);
    let last = i64::from(dynamic.get_u32("LastEntry").ok()?).min(first.saturating_add(400));
    let mut out = History::default();
    for year in first.max(1601)..=last.min(2400) {
        let Ok(value) = dynamic.get_value(year.to_string()) else { continue };
        if let Some(rules) = tzif::of_tzi(&value) {
            out.years.insert(year, rules);
        }
    }
    (!out.years.is_empty()).then_some(out)
}

#[cfg(not(windows))]
fn registry(_key: &str) -> Option<History> {
    None
}
