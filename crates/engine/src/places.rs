//! Where a run on this machine keeps what it keeps: the settings' folder, the folder of keys
//! kept apart from it, and the hold one run has on them (M7.5). The window, the console and
//! the headless server (`openmailclient serve`) all find them here, so each finds what the
//! others kept.

use std::path::{Path, PathBuf};

fn var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The per-user configuration folder of the system: `%APPDATA%\OpenMailClient`,
/// `~/Library/Application Support/OpenMailClient`, or `$XDG_CONFIG_HOME/openmailclient`
/// (`~/.config/openmailclient`).
pub fn settings_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        Some(var("APPDATA")?.join("OpenMailClient"))
    } else if cfg!(target_os = "macos") {
        Some(var("HOME")?.join("Library").join("Application Support").join("OpenMailClient"))
    } else {
        Some(var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|home| home.join(".config")))?.join("openmailclient"))
    }
}

/// The settings file in [`settings_dir`].
pub fn settings_file() -> Option<PathBuf> {
    settings_dir().map(|d| d.join("settings.json"))
}

/// Where keys are kept apart from the settings, so that a copy of the settings' folder carries
/// none: `%LOCALAPPDATA%\OpenMailClient\Keys`, `~/Library/Application Support/OpenMailClient
/// Keys`, or `$XDG_DATA_HOME/openmailclient-keys` (`~/.local/share/openmailclient-keys`): not
/// the folder `install.sh` puts the programs in, which an uninstall takes away.
pub fn keys_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        Some(var("LOCALAPPDATA")?.join("OpenMailClient").join("Keys"))
    } else if cfg!(target_os = "macos") {
        Some(var("HOME")?.join("Library").join("Application Support").join("OpenMailClient Keys"))
    } else {
        Some(var("XDG_DATA_HOME").or_else(|| var("HOME").map(|home| home.join(".local").join("share")))?.join("openmailclient-keys"))
    }
}

/// The file whose lock says a run has the settings' folder.
pub const IN_USE: &str = "in-use.lock";
/// Who has it, in words.
pub const IN_USE_BY: &str = "in-use.txt";

/// The hold one run has on a settings' folder: while it is kept, no other window or server
/// opens the same data files and writes them too.
#[derive(Debug)]
pub struct Hold {
    _file: std::fs::File,
}

/// Take the hold on `dir`; an error names who has it when another run does.
#[cfg(not(target_arch = "wasm32"))]
pub fn hold(dir: &Path) -> Result<Hold, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(IN_USE);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    // Who holds it is said in a file beside the lock: on Windows a locked file cannot be read.
    let who_file = dir.join(IN_USE_BY);
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            let who: String = std::fs::read_to_string(&who_file).unwrap_or_default().chars().take(200).collect();
            let who = who.trim();
            return Err(if who.is_empty() {
                "another OpenMailClient is using this mailbox".to_string()
            } else {
                format!("{who} is using this mailbox")
            });
        }
        // A system that keeps no locks: nothing is held, and nothing stops the run.
        Err(std::fs::TryLockError::Error(_)) => return Ok(Hold { _file: file }),
    }
    let _ = std::fs::write(&who_file, format!("{} (process {})", who_am_i(), std::process::id()));
    Ok(Hold { _file: file })
}

/// What this run is, said in the hold: the window or the server.
fn who_am_i() -> &'static str {
    match WHO.get() {
        Some(w) => w,
        None => "OpenMailClient",
    }
}

static WHO: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// Name this run for [`hold`]: "the OpenMailClient window", "openmailclient serve".
pub fn name_this_run(who: &'static str) {
    let _ = WHO.set(who);
}

/// Where mail account passwords are kept on this machine: the system's credential store when it
/// answers, otherwise sealed in a file beside the settings under a key kept in [`keys_dir`]
/// (or given by systemd), as `choice` says.
#[cfg(not(target_arch = "wasm32"))]
pub fn vault(settings_dir: &Path, choice: openmailclient_net::sealed::Choice) -> openmailclient_net::sealed::HostVault {
    use openmailclient_net::sealed::{KeyPlace, SEALED_FILE, SealedVault};
    let keys = keys_dir().unwrap_or_else(|| settings_dir.join("keys"));
    let sealed = SealedVault::new(settings_dir.join(SEALED_FILE), KeyPlace::here(&keys));
    openmailclient_net::sealed::HostVault::choose(sealed, choice)
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn one_run_holds_a_folder_at_a_time() {
        let d = std::env::temp_dir().join(format!("omc-hold-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        super::name_this_run("a test");
        let first = super::hold(&d).unwrap();
        let e = super::hold(&d).unwrap_err();
        assert!(e.contains("a test") && e.contains("using this mailbox"), "{e}");
        drop(first);
        let again = super::hold(&d);
        assert!(again.is_ok(), "let go when the first run ends");
        drop(again);
        let _ = std::fs::remove_dir_all(&d);
    }
}
