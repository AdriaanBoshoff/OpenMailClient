//! Web access (M7): the mailbox from a browser, served by OpenMailClient itself from the machine
//! it runs on (`ROADMAP.md`, M7 plan).
//!
//! What is here so far is the way in. [`password`] is the one web password: kept on the host
//! only as an Argon2id hash in a file of its own beside the settings, never in them; set, and
//! reset when forgotten, at the host (`openmailclient-cli web password`), and weak ones refused
//! by a list and patterns of our own, nothing sent anywhere (M7.2). [`guard`] decides before a
//! password is checked whether an address may try at all: five failures in 15 minutes ban it for
//! an hour, each ban after that within a day doubles to a day and then lasts until lifted, an
//! IPv6 address counted with the rest of its /64, and failures from many addresses at once slow
//! every sign-in (M7.4). Neither touches the network; the server that uses them comes in M7.1.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod guard;
pub mod password;

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
