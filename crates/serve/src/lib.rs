//! Web access (M7): the mailbox from a browser, served by OpenMailClient itself from the machine
//! it runs on (`ROADMAP.md`, M7 plan).
//!
//! [`server`] serves it over HTTPS only (M7.1): rustls with TLS 1.2 and 1.3 and a certificate of
//! its own made on first run ([`tls`]), whose fingerprint `openmailclient-cli web status` prints;
//! a plain HTTP request is sent to HTTPS; the Host and Origin are checked so another site cannot
//! reach it through the browser; a signed-in browser holds a random token in a `__Host-` cookie
//! and the server keeps only its hash ([`sessions`]); pages are drawn on the host with every
//! message's text escaped and a strict content security policy ([`http`]); and the window's
//! commands are offered over a WebSocket ([`ws`]) to signed-in browsers only.
//!
//! The way in: [`password`] is the one web password: kept on the host
//! only as an Argon2id hash in a file of its own beside the settings, never in them; set, and
//! reset when forgotten, at the host (`openmailclient-cli web password`), and weak ones refused
//! by a list and patterns of our own, nothing sent anywhere (M7.2). [`guard`] decides before a
//! password is checked whether an address may try at all: five failures in 15 minutes ban it for
//! an hour, each ban after that within a day doubles to a day and then lasts until lifted, an
//! IPv6 address counted with the rest of its /64, and failures from many addresses at once slow
//! every sign-in (M7.4). Neither touches the network; the server asks them.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod guard;
pub mod host;
pub mod http;
pub mod password;
pub mod server;
pub mod sessions;
pub mod tls;
pub mod ws;

use std::io::Write;
use std::path::{Path, PathBuf};

/// The file the web password's hash is kept in, in the settings' folder.
pub fn password_file(dir: &Path) -> PathBuf {
    dir.join("web-password.json")
}

/// The file bans are kept in, in the settings' folder, so they outlast a restart.
pub fn bans_file(dir: &Path) -> PathBuf {
    dir.join("web-bans.json")
}

/// The longest file of ours read back; anything longer is not ours.
pub const MAX_FILE: u64 = 4 << 20;

/// Read one of our files, or `None` when there is none.
pub(crate) fn read_small(path: &Path) -> Result<Option<String>, String> {
    let shown = || path.display().to_string();
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", shown())),
        Ok(m) if m.len() > MAX_FILE => return Err(format!("{}: too large to be ours", shown())),
        Ok(_) => {}
    }
    std::fs::read_to_string(path).map(Some).map_err(|e| format!("{}: {e}", shown()))
}

/// Write one of our files where only this user can read it, whole or not at all: to a file
/// beside it first, then put in its place.
pub(crate) fn write_private(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options
        .open(&tmp)
        .and_then(|mut file| file.write_all(text.as_bytes()).and_then(|()| file.sync_all()))
        .and_then(|()| std::fs::rename(&tmp, path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", path.display()));
    }
    Ok(())
}

/// Bytes from the system's source of randomness.
pub(crate) fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|e| format!("no randomness from the system: {e}"))?;
    Ok(bytes)
}

/// The calendar date `days` after 1 January 1970 (the civil-from-days rule of the proleptic
/// Gregorian calendar).
pub(crate) fn civil(days: u64) -> (i32, u8, u8) {
    let z = i64::try_from(days).unwrap_or(0).min(2_000_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (i32::try_from(year).unwrap_or(2100), u8::try_from(month).unwrap_or(1), u8::try_from(day).unwrap_or(1))
}

/// A moment as `2026-11-08 14:03:09 UTC`, for the sign-in log.
pub(crate) fn stamp(ms: u64) -> String {
    let (y, m, d) = civil(ms / 86_400_000);
    let secs = (ms / 1000) % 86_400;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", secs / 3600, secs / 60 % 60, secs % 60)
}

/// Milliseconds since 1970 by the system's clock.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
