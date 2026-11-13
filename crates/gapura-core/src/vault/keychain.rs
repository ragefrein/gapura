//! OS keychain access for the default (non-paranoid) vault mode.
//!
//! The master key is a random 32-byte value; on Linux the Secret Service
//! (gnome-keyring / kwallet) holds it, on Windows the Credential Manager.
//! The trait exists so tests can use an in-memory store and so a future
//! backend (e.g. a hardware token) slots in without touching the vault.

use base64::Engine;
use zeroize::Zeroizing;

use crate::error::{GapuraError, Result};

#[derive(Debug, Clone)]
pub struct KeychainEntryRef {
    pub service: String,
    pub account: String,
}

impl KeychainEntryRef {
    pub fn vault_master() -> Self {
        Self {
            service: "gapura".to_string(),
            account: "vault-master-key".to_string(),
        }
    }
}

/// Abstract secret storage for the vault master key.
pub trait KeyStore: Send + Sync {
    fn store(&self, key: &[u8]) -> Result<()>;
    fn load(&self) -> Result<Zeroizing<Vec<u8>>>;
    fn delete(&self) -> Result<()>;
}

/// Real OS keychain via the `keyring` crate.
pub struct OsKeychain {
    entry: KeychainEntryRef,
}

impl OsKeychain {
    pub fn new(entry: KeychainEntryRef) -> Self {
        Self { entry }
    }
}

impl KeyStore for OsKeychain {
    fn store(&self, key: &[u8]) -> Result<()> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(key);
        entry(&self.entry)?
            .set_password(&encoded)
            .map_err(|e| GapuraError::Keychain(e.to_string()))
    }

    fn load(&self) -> Result<Zeroizing<Vec<u8>>> {
        let encoded = entry(&self.entry)?
            .get_password()
            .map_err(|e| GapuraError::Keychain(e.to_string()))?;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .map_err(|e| GapuraError::Keychain(format!("stored key is not valid base64: {e}")))?;
        Ok(Zeroizing::new(raw))
    }

    fn delete(&self) -> Result<()> {
        match entry(&self.entry)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(GapuraError::Keychain(e.to_string())),
        }
    }
}

fn entry(r: &KeychainEntryRef) -> Result<keyring::Entry> {
    keyring::Entry::new(&r.service, &r.account).map_err(|e| GapuraError::Keychain(e.to_string()))
}

/// In-memory store for tests and for plumbing that needs a deterministic
/// backend (integration tests run without a desktop keychain).
#[derive(Default)]
pub struct MemoryKeyStore {
    slot: std::sync::Mutex<Option<Vec<u8>>>,
}

impl KeyStore for MemoryKeyStore {
    fn store(&self, key: &[u8]) -> Result<()> {
        *self.slot.lock().unwrap() = Some(key.to_vec());
        Ok(())
    }

    fn load(&self) -> Result<Zeroizing<Vec<u8>>> {
        self.slot
            .lock()
            .unwrap()
            .clone()
            .map(Zeroizing::new)
            .ok_or_else(|| GapuraError::Keychain("no key stored".into()))
    }

    fn delete(&self) -> Result<()> {
        *self.slot.lock().unwrap() = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_keystore_roundtrip() {
        let store = MemoryKeyStore::default();
        assert!(store.load().is_err());
        store.store(&[1, 2, 3]).unwrap();
        assert_eq!(store.load().unwrap().as_slice(), &[1, 2, 3]);
        store.delete().unwrap();
        assert!(store.load().is_err());
    }
}
