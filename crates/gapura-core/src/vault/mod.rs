//! Vault lifecycle: create, unlock, save.
//!
//! One vault per user, stored in the app data directory
//! (`~/.local/share/gapura/vault.json.enc` on Linux,
//! `%APPDATA%\gapura\vault.json.enc` on Windows).
//!
//! Two unlock modes:
//!
//! - `Keychain` (default): the master key is a random 32-byte value stored in
//!   the OS keychain. Opening the app needs no passphrase.
//! - `Passphrase` (paranoid): the master key is derived from a user
//!   passphrase with argon2id. The derivation parameters live in the header.

pub mod crypto;
pub mod keychain;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::error::{GapuraError, Result};
use crate::model::VaultPayload;
use crypto::{
    KdfMode, KdfParams, VaultHeader, DEFAULT_ITERATIONS, DEFAULT_MEM_KIB, DEFAULT_PARALLELISM,
    KEY_LEN,
};
use keychain::KeyStore;

pub const VAULT_FILE_NAME: &str = "vault.json.enc";
/// Number of previous versions kept next to the live file (design doc 2.4).
const BACKUP_DEPTH: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultMode {
    /// Master key lives in the OS keychain.
    Keychain,
    /// Master key derived from a passphrase (paranoid mode).
    Passphrase,
}

impl VaultMode {
    pub fn from_kdf(kdf: KdfMode) -> Self {
        match kdf {
            KdfMode::Keychain => VaultMode::Keychain,
            KdfMode::Argon2id => VaultMode::Passphrase,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            VaultMode::Keychain => "keychain",
            VaultMode::Passphrase => "passphrase",
        }
    }
}

pub fn vault_path(data_dir: &Path) -> PathBuf {
    data_dir.join(VAULT_FILE_NAME)
}

pub fn vault_exists(data_dir: &Path) -> bool {
    vault_path(data_dir).is_file()
}

/// Read only the header to learn which mode the vault uses. The UI calls this
/// before showing an unlock prompt, so it never needs the passphrase or the
/// keychain.
pub fn peek_mode(data_dir: &Path) -> Result<Option<VaultMode>> {
    let path = vault_path(data_dir);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|e| GapuraError::PathIo {
        path: path.clone(),
        message: e.to_string(),
    })?;
    let (header, _) = VaultHeader::decode(&bytes)?;
    Ok(Some(VaultMode::from_kdf(header.kdf_mode)))
}

/// A decrypted, editable vault. Call [`UnlockedVault::save`] to persist.
pub struct UnlockedVault {
    path: PathBuf,
    mode: VaultMode,
    key: Zeroizing<[u8; KEY_LEN]>,
    /// Salt and argon2 params captured at setup/unlock; re-written on every
    /// save so the file stays self-describing.
    header_params: Option<KdfParams>,
    pub payload: VaultPayload,
}

impl UnlockedVault {
    pub fn mode(&self) -> VaultMode {
        self.mode
    }

    /// Encrypt and write the payload. The write is atomic (temp file, fsync,
    /// rename) and rotates backups of previous versions.
    pub fn save(&self) -> Result<()> {
        let plaintext = serde_json::to_vec(&self.payload)?;
        let nonce = crypto::random_nonce()?;
        let header = match self.mode {
            VaultMode::Keychain => VaultHeader::keychain(nonce),
            VaultMode::Passphrase => VaultHeader::argon2id(
                self.header_params.clone().ok_or_else(|| {
                    GapuraError::Crypto("passphrase vault lost its kdf params".into())
                })?,
                nonce,
            ),
        };
        let ciphertext = crypto::encrypt(&self.key, &nonce, &plaintext)?;
        let mut bytes = header.encode();
        bytes.extend_from_slice(&ciphertext);

        rotate_backups(&self.path);
        let tmp = self.path.with_extension("enc.tmp");
        {
            let mut f = fs::File::create(&tmp).map_err(|e| GapuraError::PathIo {
                path: tmp.clone(),
                message: e.to_string(),
            })?;
            f.write_all(&bytes).map_err(|e| GapuraError::PathIo {
                path: tmp.clone(),
                message: e.to_string(),
            })?;
            f.sync_all().map_err(|e| GapuraError::PathIo {
                path: tmp.clone(),
                message: e.to_string(),
            })?;
        }
        fs::rename(&tmp, &self.path).map_err(|e| GapuraError::PathIo {
            path: self.path.clone(),
            message: e.to_string(),
        })?;
        tracing::debug!(path = %self.path.display(), "vault saved");
        Ok(())
    }
}

/// Create a fresh vault. Refuses to overwrite an existing one.
pub fn setup(
    data_dir: &Path,
    mode: VaultMode,
    passphrase: Option<&str>,
    key_store: &dyn KeyStore,
) -> Result<UnlockedVault> {
    let path = vault_path(data_dir);
    if path.exists() {
        return Err(GapuraError::Other(
            "a vault already exists; delete it first to start over".into(),
        ));
    }
    fs::create_dir_all(data_dir).map_err(|e| GapuraError::PathIo {
        path: data_dir.to_path_buf(),
        message: e.to_string(),
    })?;

    let (key, header_params) = match mode {
        VaultMode::Keychain => {
            let key = crypto::random_key()?;
            key_store.store(&*key)?;
            (key, None)
        }
        VaultMode::Passphrase => {
            let passphrase = passphrase.ok_or_else(|| {
                GapuraError::Other("a passphrase is required for paranoid mode".into())
            })?;
            if passphrase.len() < 8 {
                return Err(GapuraError::Other(
                    "pick a passphrase of at least 8 characters".into(),
                ));
            }
            let params = KdfParams {
                salt: crypto::random_salt()?,
                mem_kib: DEFAULT_MEM_KIB,
                iterations: DEFAULT_ITERATIONS,
                parallelism: DEFAULT_PARALLELISM,
            };
            (crypto::derive_key(passphrase, &params)?, Some(params))
        }
    };

    let vault = UnlockedVault {
        path: path.clone(),
        mode,
        key,
        header_params,
        payload: VaultPayload::empty(),
    };
    vault.save()?;
    tracing::info!(mode = mode.as_str(), "vault created");
    Ok(vault)
}

/// Open an existing vault.
pub fn unlock(
    data_dir: &Path,
    passphrase: Option<&str>,
    key_store: &dyn KeyStore,
) -> Result<UnlockedVault> {
    let path = vault_path(data_dir);
    if !path.is_file() {
        return Err(GapuraError::VaultNotFound);
    }
    let bytes = fs::read(&path).map_err(|e| GapuraError::PathIo {
        path: path.clone(),
        message: e.to_string(),
    })?;
    let (header, offset) = VaultHeader::decode(&bytes)?;
    let mode = VaultMode::from_kdf(header.kdf_mode);

    let key: Zeroizing<[u8; KEY_LEN]> = match mode {
        VaultMode::Keychain => {
            let stored = key_store.load()?;
            let arr: [u8; KEY_LEN] = stored.as_slice().try_into().map_err(|_| {
                GapuraError::Keychain("stored master key has the wrong length".into())
            })?;
            Zeroizing::new(arr)
        }
        VaultMode::Passphrase => {
            let passphrase = passphrase.ok_or(GapuraError::VaultLocked)?;
            let params = header
                .params
                .clone()
                .ok_or_else(|| GapuraError::VaultCorrupt("missing argon2 params".into()))?;
            crypto::derive_key(passphrase, &params)?
        }
    };

    let plaintext = crypto::decrypt(&key, &header.nonce, &bytes[offset..])?;
    let payload: VaultPayload = serde_json::from_slice(&plaintext)?;

    migrate_payload(payload).map(|payload| UnlockedVault {
        path,
        mode,
        key,
        header_params: header.params.clone(),
        payload,
    })
}

/// Schema migration chain (design doc 2.4). Each step upgrades one version.
/// Nothing here ever writes an older schema back to disk.
fn migrate_payload(mut payload: VaultPayload) -> Result<VaultPayload> {
    match payload.schema_version {
        SCHEMA_LATEST => Ok(payload),
        0 => {
            // Pre-release vaults had no explicit schema marker; treat as v1.
            payload.schema_version = 1;
            Ok(payload)
        }
        other => Err(GapuraError::VaultVersion(other as u8)),
    }
}

pub const SCHEMA_LATEST: u32 = crate::model::SCHEMA_VERSION;

fn rotate_backups(live: &Path) {
    // Shift vault.bak-(N-1) -> vault.bak-N, then copy live -> vault.bak-1.
    for i in (1..BACKUP_DEPTH).rev() {
        let src = backup_path(live, i);
        if src.exists() {
            let _ = fs::rename(&src, backup_path(live, i + 1));
        }
    }
    if live.exists() {
        let _ = fs::copy(live, backup_path(live, 1));
    }
}

fn backup_path(live: &Path, n: u32) -> PathBuf {
    live.with_file_name(format!("vault.bak-{n}.enc"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use keychain::MemoryKeyStore;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn setup_unlock_roundtrip_keychain_mode() {
        let tmp = temp_dir();
        let dir = tmp.path();
        let store = MemoryKeyStore::default();
        assert!(matches!(
            unlock(dir, None, &store),
            Err(GapuraError::VaultNotFound)
        ));

        let mut vault = setup(dir, VaultMode::Keychain, None, &store).unwrap();
        vault.payload.connections.push(crate::model::Connection {
            id: uuid::Uuid::new_v4(),
            name: "web1".into(),
            group_id: None,
            host: "web1.example.com".into(),
            port: 22,
            username: "ops".into(),
            auth: crate::model::AuthMethod::Agent,
            tags: vec![],
            color: None,
            jump_host_id: None,
            notes: String::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        });
        vault.save().unwrap();

        let reopened = unlock(dir, None, &store).unwrap();
        assert_eq!(reopened.mode(), VaultMode::Keychain);
        assert_eq!(reopened.payload.connections.len(), 1);
        assert_eq!(reopened.payload.connections[0].host, "web1.example.com");

        assert_eq!(peek_mode(dir).unwrap(), Some(VaultMode::Keychain));
    }

    #[test]
    fn setup_unlock_roundtrip_passphrase_mode() {
        let tmp = temp_dir();
        let dir = tmp.path();
        let store = MemoryKeyStore::default();
        let mut vault = setup(
            dir,
            VaultMode::Passphrase,
            Some("correct horse battery"),
            &store,
        )
        .unwrap();
        vault.payload.groups.push(crate::model::Group {
            id: uuid::Uuid::new_v4(),
            name: "lab".into(),
            created_at: chrono::Utc::now(),
        });
        vault.save().unwrap();

        let reopened = unlock(dir, Some("correct horse battery"), &store).unwrap();
        assert_eq!(reopened.mode(), VaultMode::Passphrase);
        assert_eq!(reopened.payload.groups.len(), 1);
        assert_eq!(peek_mode(dir).unwrap(), Some(VaultMode::Passphrase));
    }

    #[test]
    fn wrong_passphrase_is_reported() {
        let tmp = temp_dir();
        let dir = tmp.path();
        let store = MemoryKeyStore::default();
        setup(dir, VaultMode::Passphrase, Some("right passphrase"), &store).unwrap();
        assert!(matches!(
            unlock(dir, Some("wrong passphrase"), &store),
            Err(GapuraError::WrongPassphrase)
        ));
    }

    #[test]
    fn setup_refuses_overwrite() {
        let tmp = temp_dir();
        let dir = tmp.path();
        let store = MemoryKeyStore::default();
        setup(dir, VaultMode::Keychain, None, &store).unwrap();
        assert!(setup(dir, VaultMode::Keychain, None, &store).is_err());
    }

    #[test]
    fn short_passphrase_is_rejected() {
        let tmp = temp_dir();
        let dir = tmp.path();
        let store = MemoryKeyStore::default();
        assert!(setup(dir, VaultMode::Passphrase, Some("short"), &store).is_err());
    }

    #[test]
    fn saves_rotate_backups() {
        let tmp = temp_dir();
        let dir = tmp.path();
        let store = MemoryKeyStore::default();
        let mut vault = setup(dir, VaultMode::Keychain, None, &store).unwrap();
        for i in 0..5u32 {
            let name = format!("host-{i}");
            vault.payload.groups.push(crate::model::Group {
                id: uuid::Uuid::new_v4(),
                name,
                created_at: chrono::Utc::now(),
            });
            vault.save().unwrap();
        }
        assert!(backup_path(&vault_path(dir), 1).exists());
        assert!(backup_path(&vault_path(dir), BACKUP_DEPTH).exists());
        assert!(!backup_path(&vault_path(dir), BACKUP_DEPTH + 1).exists());
        // The live file must still decrypt fine.
        unlock(dir, None, &store).unwrap();
    }

    #[test]
    fn tampered_vault_file_is_rejected() {
        let tmp = temp_dir();
        let dir = tmp.path();
        let store = MemoryKeyStore::default();
        setup(dir, VaultMode::Keychain, None, &store).unwrap();
        let path = vault_path(dir);
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        fs::write(&path, &bytes).unwrap();
        assert!(unlock(dir, None, &store).is_err());
    }

    #[test]
    fn future_schema_version_is_refused() {
        let tmp = temp_dir();
        let dir = tmp.path();
        let store = MemoryKeyStore::default();
        setup(dir, VaultMode::Keychain, None, &store).unwrap();
        let path = vault_path(dir);
        // Encrypt a payload claiming a future schema using the real key so
        // only the version check trips.
        let stored = store.load().unwrap();
        let key: [u8; KEY_LEN] = stored.as_slice().try_into().unwrap();
        let mut payload = VaultPayload::empty();
        payload.schema_version = 99;
        let plaintext = serde_json::to_vec(&payload).unwrap();
        let nonce = crypto::random_nonce().unwrap();
        let header = VaultHeader::keychain(nonce);
        let mut bytes = header.encode();
        bytes.extend_from_slice(&crypto::encrypt(&key, &nonce, &plaintext).unwrap());
        fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            unlock(dir, None, &store),
            Err(GapuraError::VaultVersion(99))
        ));
    }
}
