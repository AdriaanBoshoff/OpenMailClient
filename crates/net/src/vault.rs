//! Where passwords are kept: the system's own credential store (Windows Credential Manager, the
//! macOS keychain, the Secret Service on Linux and the BSDs), never a file of ours, a log or a
//! setting. An account names its password by the account's id; the password itself is read
//! only at the moment it is said to the server.

use crate::{NetError, Secret, said};
use std::collections::HashMap;
use std::sync::Mutex;

/// The name passwords are kept under in the system's store.
pub const SERVICE: &str = "OpenMailClient";

/// Somewhere passwords are kept, by account.
pub trait Vault: Send + Sync {
    /// The password kept for the account, if any.
    fn get(&self, account: &str) -> Result<Option<Secret>, NetError>;
    /// Keep a password for the account, in place of any kept before.
    fn set(&self, account: &str, password: &Secret) -> Result<(), NetError>;
    /// Forget the account's password; forgetting one that was never kept is not an error.
    fn forget(&self, account: &str) -> Result<(), NetError>;
}

/// An account's id as a key: a line of no more than 200 characters.
pub(crate) fn key(account: &str) -> Result<&str, NetError> {
    if account.is_empty() || account.chars().count() > 200 || account.chars().any(char::is_control) {
        return Err(NetError::Unsupported(format!("{} does not name an account", said(account))));
    }
    Ok(account)
}

/// Passwords kept in memory for as long as this runs: for tests, and where there is no store.
#[derive(Default)]
pub struct MemoryVault(Mutex<HashMap<String, Secret>>);

impl MemoryVault {
    fn kept(&self) -> std::sync::MutexGuard<'_, HashMap<String, Secret>> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Vault for MemoryVault {
    fn get(&self, account: &str) -> Result<Option<Secret>, NetError> {
        Ok(self.kept().get(key(account)?).cloned())
    }

    fn set(&self, account: &str, password: &Secret) -> Result<(), NetError> {
        self.kept().insert(key(account)?.to_string(), password.clone());
        Ok(())
    }

    fn forget(&self, account: &str) -> Result<(), NetError> {
        self.kept().remove(key(account)?);
        Ok(())
    }
}

/// The system's credential store.
#[derive(Default)]
pub struct SystemVault;

#[cfg(all(not(target_arch = "wasm32"), any(windows, unix)))]
mod system {
    use super::{SERVICE, key};
    use crate::{NetError, Secret, said};

    fn entry(account: &str) -> Result<keyring::Entry, NetError> {
        keyring::Entry::new(SERVICE, key(account)?).map_err(failed)
    }

    /// What the store said, which names no password.
    fn failed(e: keyring::Error) -> NetError {
        NetError::Vault(said(&e.to_string()))
    }

    pub fn get(account: &str) -> Result<Option<Secret>, NetError> {
        match entry(account)?.get_password() {
            Ok(p) => Ok(Some(Secret::new(p))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(failed(e)),
        }
    }

    pub fn set(account: &str, password: &Secret) -> Result<(), NetError> {
        entry(account)?.set_password(password.expose()).map_err(failed)
    }

    pub fn forget(account: &str) -> Result<(), NetError> {
        match entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(failed(e)),
        }
    }
}

#[cfg(not(all(not(target_arch = "wasm32"), any(windows, unix))))]
mod system {
    use super::{SERVICE, key};
    use crate::{NetError, Secret};

    fn none(account: &str) -> NetError {
        let _ = key(account);
        NetError::Vault(format!("this build has no credential store to keep {SERVICE} passwords in"))
    }

    pub fn get(account: &str) -> Result<Option<Secret>, NetError> {
        Err(none(account))
    }

    pub fn set(account: &str, _password: &Secret) -> Result<(), NetError> {
        Err(none(account))
    }

    pub fn forget(account: &str) -> Result<(), NetError> {
        Err(none(account))
    }
}

impl Vault for SystemVault {
    fn get(&self, account: &str) -> Result<Option<Secret>, NetError> {
        system::get(account)
    }

    fn set(&self, account: &str, password: &Secret) -> Result<(), NetError> {
        system::set(account, password)
    }

    fn forget(&self, account: &str) -> Result<(), NetError> {
        system::forget(account)
    }
}
