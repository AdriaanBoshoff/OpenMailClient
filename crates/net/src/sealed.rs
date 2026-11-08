//! Passwords kept where there is no credential store of the system's (M7.5): a server with no
//! desktop, such as Ubuntu Server, has no Secret Service to ask. They are kept in one file
//! beside the settings, sealed with ChaCha20-Poly1305 (RFC 8439) under a key that is kept
//! somewhere else: given by systemd as a credential (`LoadCredentialEncrypted=`, so that it is
//! itself sealed by the machine's TPM or its own key) or a key file in another folder, so that a
//! copy of the settings' folder carries no password that can be read. Neither file is ever
//! written in the clear; both are kept from other users' eyes where the system allows it.
//!
//! [`HostVault`] chooses: the system's store when it answers, otherwise this file; and reads
//! from either, so that passwords kept in one before are still found.

use crate::vault::{SystemVault, Vault};
use crate::{NetError, Secret};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The sealed passwords, beside the settings.
pub const SEALED_FILE: &str = "passwords.sealed";
/// The key, in a folder of its own when no credential gives it.
pub const KEY_FILE: &str = "passwords.key";
/// The name of the systemd credential that gives the key (`$CREDENTIALS_DIRECTORY/…`).
pub const KEY_CREDENTIAL: &str = "openmailclient-passwords-key";

const FIRST_LINE: &str = "OpenMailClient sealed passwords 1";
const AAD: &[u8] = b"OpenMailClient sealed passwords 1";
/// A sealed file larger than this is not ours.
const MAX_FILE: u64 = 1 << 20;
/// The most passwords kept.
const MAX_KEPT: usize = 10_000;

/// Where the key is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyPlace {
    /// A file of our own, made when the first password is kept.
    File(PathBuf),
    /// Given by systemd (or another host): read, never made or changed.
    Given(PathBuf),
}

impl KeyPlace {
    /// The credential systemd gives this run when there is one, otherwise `dir`'s key file.
    pub fn here(dir: &Path) -> KeyPlace {
        if let Some(creds) = std::env::var_os("CREDENTIALS_DIRECTORY").filter(|v| !v.is_empty()) {
            let given = PathBuf::from(creds).join(KEY_CREDENTIAL);
            if given.is_file() {
                return KeyPlace::Given(given);
            }
        }
        KeyPlace::File(dir.join(KEY_FILE))
    }

    pub fn path(&self) -> &Path {
        match self {
            KeyPlace::File(p) | KeyPlace::Given(p) => p,
        }
    }
}

fn trouble(what: impl Into<String>) -> NetError {
    NetError::Vault(what.into())
}

/// The key as a file holds it: 32 bytes as they are, or written in base64.
fn key_of(bytes: &[u8]) -> Option<[u8; 32]> {
    if let Ok(raw) = <[u8; 32]>::try_from(bytes) {
        return Some(raw);
    }
    let text = std::str::from_utf8(bytes).ok()?.trim();
    STANDARD.decode(text).ok().and_then(|k| <[u8; 32]>::try_from(k.as_slice()).ok())
}

/// Write `bytes` to `path` through a file beside it swapped in at once, readable by this user only.
fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), NetError> {
    use std::io::Write;
    let dir = path.parent().ok_or_else(|| trouble(format!("{} is in no folder", path.display())))?;
    std::fs::create_dir_all(dir).map_err(|e| trouble(format!("{}: {e}", dir.display())))?;
    let mut name = path.file_name().map(std::ffi::OsStr::to_os_string).unwrap_or_default();
    name.push(".new");
    let temp = dir.join(name);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = (|| {
        let mut f = options.open(&temp)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(trouble(format!("{}: {e}", temp.display())));
    }
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        trouble(format!("{}: {e}", path.display()))
    })
}

/// Passwords sealed in a file under a key kept elsewhere.
pub struct SealedVault {
    file: PathBuf,
    key: KeyPlace,
    busy: Mutex<()>,
}

impl SealedVault {
    pub fn new(file: PathBuf, key: KeyPlace) -> SealedVault {
        SealedVault { file, key, busy: Mutex::new(()) }
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    pub fn key_place(&self) -> &KeyPlace {
        &self.key
    }

    /// The key; made first when `make` and it is a file of ours that is not there yet.
    fn key(&self, make: bool) -> Result<Option<LessSafeKey>, NetError> {
        let path = self.key.path();
        let bytes = match std::fs::read(path) {
            Ok(b) if b.len() <= 4096 => b,
            Ok(_) => return Err(trouble(format!("{} is not a key of ours", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let KeyPlace::File(path) = &self.key else {
                    return Err(trouble(format!("the key given at {} cannot be read", path.display())));
                };
                if !make {
                    return Ok(None);
                }
                let mut raw = [0u8; 32];
                SystemRandom::new().fill(&mut raw).map_err(|_| trouble("the system gave no random bytes for a key"))?;
                let mut text = STANDARD.encode(raw);
                text.push('\n');
                write_whole(path, text.as_bytes())?;
                text.into_bytes()
            }
            Err(e) => return Err(trouble(format!("{}: {e}", path.display()))),
        };
        let raw = key_of(&bytes).ok_or_else(|| trouble(format!("{} is not a key of ours", path.display())))?;
        let unbound = UnboundKey::new(&CHACHA20_POLY1305, &raw).map_err(|_| trouble("the key cannot be used"))?;
        Ok(Some(LessSafeKey::new(unbound)))
    }

    /// The passwords kept, or none when there is no file yet.
    fn read(&self, key: Option<&LessSafeKey>) -> Result<BTreeMap<String, String>, NetError> {
        let shown = self.file.display();
        let len = match std::fs::metadata(&self.file) {
            Ok(m) => m.len(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(trouble(format!("{shown}: {e}"))),
        };
        if len > MAX_FILE {
            return Err(trouble(format!("{shown} is too large to be ours")));
        }
        let text = std::fs::read_to_string(&self.file).map_err(|e| trouble(format!("{shown}: {e}")))?;
        let mut lines = text.lines();
        let not_ours = || trouble(format!("{shown} is not a file of sealed passwords"));
        if lines.next() != Some(FIRST_LINE) {
            return Err(not_ours());
        }
        let nonce =
            lines.next().and_then(|l| STANDARD.decode(l.trim()).ok()).and_then(|n| <[u8; 12]>::try_from(n.as_slice()).ok()).ok_or_else(not_ours)?;
        let mut sealed = lines.next().and_then(|l| STANDARD.decode(l.trim()).ok()).ok_or_else(not_ours)?;
        let key =
            key.ok_or_else(|| trouble(format!("the passwords in {shown} are sealed with a key that is not at {}", self.key.path().display())))?;
        let open = key.open_in_place(Nonce::assume_unique_for_key(nonce), Aad::from(AAD), &mut sealed).map_err(|_| {
            trouble(format!("{shown} was sealed with another key than {}, or was changed; nothing in it was changed", self.key.path().display()))
        })?;
        let plain = std::str::from_utf8(open).map_err(|_| not_ours())?;
        let mut kept = BTreeMap::new();
        for line in plain.lines().take(MAX_KEPT) {
            let (a, p) = line.split_once('\t').ok_or_else(not_ours)?;
            let decode = |s: &str| STANDARD.decode(s).ok().and_then(|b| String::from_utf8(b).ok());
            kept.insert(decode(a).ok_or_else(not_ours)?, decode(p).ok_or_else(not_ours)?);
        }
        Ok(kept)
    }

    fn write(&self, key: &LessSafeKey, kept: &BTreeMap<String, String>) -> Result<(), NetError> {
        let mut plain = String::new();
        for (a, p) in kept {
            plain.push_str(&STANDARD.encode(a));
            plain.push('\t');
            plain.push_str(&STANDARD.encode(p));
            plain.push('\n');
        }
        let mut nonce = [0u8; 12];
        SystemRandom::new().fill(&mut nonce).map_err(|_| trouble("the system gave no random bytes"))?;
        let mut sealed = plain.into_bytes();
        key.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(AAD), &mut sealed)
            .map_err(|_| trouble("the passwords could not be sealed"))?;
        let text = format!("{FIRST_LINE}\n{}\n{}\n", STANDARD.encode(nonce), STANDARD.encode(&sealed));
        write_whole(&self.file, text.as_bytes())
    }

    fn change(&self, f: impl FnOnce(&mut BTreeMap<String, String>) -> bool) -> Result<(), NetError> {
        let _busy = self.busy.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // A key is made only for a file not there yet: one sealed already wants its own key back.
        let key = self.key(!self.file.exists())?.ok_or_else(|| {
            trouble(format!("the passwords in {} are sealed with a key that is not at {}", self.file.display(), self.key.path().display()))
        })?;
        // Read with the key it will be written with: a file sealed with another is never written over.
        let mut kept = self.read(Some(&key))?;
        if f(&mut kept) {
            if kept.len() > MAX_KEPT {
                return Err(trouble(format!("at most {MAX_KEPT} passwords are kept")));
            }
            self.write(&key, &kept)?;
        }
        Ok(())
    }
}

impl Vault for SealedVault {
    fn get(&self, account: &str) -> Result<Option<Secret>, NetError> {
        let account = crate::vault::key(account)?;
        let _busy = self.busy.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.file.exists() {
            return Ok(None);
        }
        let key = self.key(false)?;
        Ok(self.read(key.as_ref())?.get(account).map(|p| Secret::new(p.clone())))
    }

    fn set(&self, account: &str, password: &Secret) -> Result<(), NetError> {
        let account = crate::vault::key(account)?.to_string();
        let password = password.expose().to_string();
        self.change(move |kept| {
            kept.insert(account, password);
            true
        })
    }

    fn forget(&self, account: &str) -> Result<(), NetError> {
        let account = crate::vault::key(account)?;
        if !self.file.exists() {
            return Ok(());
        }
        self.change(|kept| kept.remove(account).is_some())
    }
}

/// Which store passwords are kept in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Choice {
    /// The system's when it answers, otherwise the sealed file.
    #[default]
    Either,
    System,
    Sealed,
}

impl Choice {
    pub fn parse(s: &str) -> Option<Choice> {
        match s {
            "either" | "auto" => Some(Choice::Either),
            "system" => Some(Choice::System),
            "sealed" | "file" => Some(Choice::Sealed),
            _ => None,
        }
    }
}

/// The system's store or the sealed file, as this machine allows.
pub struct HostVault {
    system: Option<SystemVault>,
    sealed: SealedVault,
}

impl HostVault {
    /// Choose where passwords are kept; with [`Choice::Either`], the system's store is asked
    /// once, for a name no account has, to see whether it answers.
    pub fn choose(sealed: SealedVault, choice: Choice) -> HostVault {
        let system = match choice {
            Choice::System => Some(SystemVault),
            Choice::Sealed => None,
            Choice::Either => SystemVault.get("OpenMailClient: does this store answer").is_ok().then_some(SystemVault),
        };
        HostVault { system, sealed }
    }

    /// Where new passwords go, in words.
    pub fn says(&self) -> String {
        match self.system {
            Some(_) => "in the system's credential store".to_string(),
            None => {
                let key = match self.sealed.key_place() {
                    KeyPlace::Given(p) => format!("the key systemd gives, {}", p.display()),
                    KeyPlace::File(p) => format!("the key in {}", p.display()),
                };
                format!("in {}, sealed with {key}", self.sealed.file().display())
            }
        }
    }

    pub fn uses_system(&self) -> bool {
        self.system.is_some()
    }
}

impl Vault for HostVault {
    fn get(&self, account: &str) -> Result<Option<Secret>, NetError> {
        let from_system = match &self.system {
            Some(s) => match s.get(account) {
                Ok(Some(p)) => return Ok(Some(p)),
                other => Some(other),
            },
            None => None,
        };
        // A password kept in the sealed file before is found as well.
        match (self.sealed.get(account), from_system) {
            (Ok(Some(p)), _) => Ok(Some(p)),
            (Ok(None), Some(Err(e))) => Err(e),
            (Ok(None), _) => Ok(None),
            (Err(e), None) => Err(e),
            (Err(_), Some(system)) => system,
        }
    }

    fn set(&self, account: &str, password: &Secret) -> Result<(), NetError> {
        match &self.system {
            Some(s) => s.set(account, password),
            None => self.sealed.set(account, password),
        }
    }

    fn forget(&self, account: &str) -> Result<(), NetError> {
        match &self.system {
            Some(s) => {
                let r = s.forget(account);
                // An older copy in the sealed file goes too; that file may not be readable here.
                let _ = self.sealed.forget(account);
                r
            }
            None => self.sealed.forget(account),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("omc-sealed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn vault(d: &Path) -> SealedVault {
        SealedVault::new(d.join("settings").join(SEALED_FILE), KeyPlace::File(d.join("keys").join(KEY_FILE)))
    }

    #[test]
    fn passwords_are_sealed_kept_and_forgotten() {
        let d = dir("round");
        let v = vault(&d);
        assert!(v.get("acct-1").unwrap().is_none());
        assert!(!d.join("keys").join(KEY_FILE).exists(), "a get makes no key");
        v.set("acct-1", &Secret::new("tab\tand\nnew line é")).unwrap();
        v.set("acct-2", &Secret::new("second")).unwrap();
        assert_eq!(v.get("acct-1").unwrap().unwrap().expose(), "tab\tand\nnew line é");
        let on_disk = std::fs::read_to_string(v.file()).unwrap();
        assert!(!on_disk.contains("second") && !on_disk.contains(&STANDARD.encode("second")), "never in the clear");
        // Another vault on the same files reads the same.
        assert_eq!(vault(&d).get("acct-2").unwrap().unwrap().expose(), "second");
        v.forget("acct-1").unwrap();
        assert!(v.get("acct-1").unwrap().is_none());
        assert_eq!(v.get("acct-2").unwrap().unwrap().expose(), "second");
        v.forget("never-kept").unwrap();
        assert!(v.set("", &Secret::new("x")).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for f in [v.file().to_path_buf(), d.join("keys").join(KEY_FILE)] {
                assert_eq!(std::fs::metadata(f).unwrap().permissions().mode() & 0o077, 0);
            }
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn another_key_never_writes_over_what_it_cannot_read() {
        let d = dir("other");
        let v = vault(&d);
        v.set("acct", &Secret::new("kept")).unwrap();
        let before = std::fs::read(v.file()).unwrap();
        let other = SealedVault::new(v.file().to_path_buf(), KeyPlace::File(d.join("elsewhere").join(KEY_FILE)));
        assert!(other.get("acct").is_err());
        assert!(other.set("acct", &Secret::new("new")).is_err());
        assert!(other.forget("acct").is_err());
        assert_eq!(std::fs::read(v.file()).unwrap(), before, "untouched");
        // The key gone: said, not guessed.
        std::fs::remove_file(d.join("keys").join(KEY_FILE)).unwrap();
        let e = v.get("acct").unwrap_err().to_string();
        assert!(e.contains("key"), "{e}");
        assert!(v.set("acct", &Secret::new("new")).is_err());
        assert!(!d.join("keys").join(KEY_FILE).exists(), "no new key made in the lost one's place");
        assert_eq!(std::fs::read(v.file()).unwrap(), before);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_given_key_is_read_never_made() {
        let d = dir("given");
        let given = d.join(KEY_CREDENTIAL);
        let v = SealedVault::new(d.join(SEALED_FILE), KeyPlace::Given(given.clone()));
        assert!(v.set("acct", &Secret::new("p")).is_err(), "no credential, no key made in its place");
        assert!(!given.exists());
        std::fs::write(&given, [7u8; 32]).unwrap();
        v.set("acct", &Secret::new("p")).unwrap();
        assert_eq!(v.get("acct").unwrap().unwrap().expose(), "p");
        std::fs::write(&given, format!("{}\n", STANDARD.encode([7u8; 32]))).unwrap();
        assert_eq!(v.get("acct").unwrap().unwrap().expose(), "p", "the same key written in base64");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn hostile_files_are_refused_without_panicking() {
        let d = dir("hostile");
        let v = vault(&d);
        v.set("acct", &Secret::new("p")).unwrap();
        let good = std::fs::read_to_string(v.file()).unwrap();
        let mut cases: Vec<Vec<u8>> = vec![
            Vec::new(),
            b"garbage".to_vec(),
            format!("{FIRST_LINE}\n").into_bytes(),
            format!("{FIRST_LINE}\n!!!\n!!!\n").into_bytes(),
            format!("{FIRST_LINE}\n{}\nAAAA\n", STANDARD.encode([0u8; 12])).into_bytes(),
            vec![0xFF; 300],
        ];
        for cut in [1, good.len() / 2, good.len().saturating_sub(3)] {
            cases.push(good.as_bytes()[..cut].to_vec());
        }
        let mut flipped = good.clone().into_bytes();
        let at = flipped.len() - 5;
        flipped[at] = if flipped[at] == b'A' { b'B' } else { b'A' };
        cases.push(flipped);
        for case in cases {
            std::fs::write(v.file(), &case).unwrap();
            assert!(v.get("acct").is_err());
            assert!(v.set("acct", &Secret::new("q")).is_err(), "never written over");
            assert_eq!(std::fs::read(v.file()).unwrap(), case);
        }
        // Too large to be ours, refused before it is read.
        std::fs::write(v.file(), vec![b'a'; (MAX_FILE + 1) as usize]).unwrap();
        assert!(v.get("acct").is_err());
        std::fs::write(d.join("keys").join(KEY_FILE), b"not a key").unwrap();
        std::fs::remove_file(v.file()).unwrap();
        assert!(v.set("acct", &Secret::new("q")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_host_vault_keeps_in_the_file_when_told_to() {
        let d = dir("host");
        let h = HostVault::choose(vault(&d), Choice::Sealed);
        assert!(!h.uses_system());
        assert!(h.says().contains(SEALED_FILE));
        h.set("acct", &Secret::new("p")).unwrap();
        assert_eq!(h.get("acct").unwrap().unwrap().expose(), "p");
        h.forget("acct").unwrap();
        assert!(h.get("acct").unwrap().is_none());
        assert_eq!(Choice::parse("file"), Some(Choice::Sealed));
        assert_eq!(Choice::parse("nope"), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
