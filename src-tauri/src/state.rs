//! Tauri state: the unlocked vault, active sessions, known hosts, settings.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use gapura_core::settings::Settings;
use gapura_core::ssh::known_hosts::KnownHosts;
use gapura_core::vault::{UnlockedVault, VaultMode};
use tokio::sync::mpsc;
use uuid::Uuid;

use gapura_core::ssh::session::SessionCommand;

/// Everything the commands need, managed by Tauri.
pub struct AppState {
    pub vault: RwLock<Option<UnlockedVault>>,
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub known_hosts: Arc<Mutex<KnownHosts>>,
    pub settings: RwLock<Settings>,
    pub sessions: RwLock<HashMap<Uuid, SessionEntry>>,
    /// Consumed by `vault_status` to explain a failed keychain unlock once.
    pub keychain_error: Mutex<Option<String>>,
}

pub struct SessionEntry {
    pub connection_id: Uuid,
    pub commands: mpsc::Sender<SessionCommand>,
}

impl AppState {
    pub fn new(data_dir: PathBuf, config_dir: PathBuf) -> Result<Self, String> {
        let known_hosts = KnownHosts::load(&data_dir).map_err(|e| e.to_string())?;
        let settings = gapura_core::settings::load(&config_dir).map_err(|e| e.to_string())?;
        Ok(Self {
            vault: RwLock::new(None),
            data_dir,
            config_dir,
            known_hosts: Arc::new(Mutex::new(known_hosts)),
            settings: RwLock::new(settings),
            sessions: RwLock::new(HashMap::new()),
            keychain_error: Mutex::new(None),
        })
    }

    /// Panic-free read access to the unlocked vault.
    pub fn with_vault<T>(
        &self,
        f: impl FnOnce(&UnlockedVault) -> Result<T, gapura_core::GapuraError>,
    ) -> Result<T, gapura_core::GapuraError> {
        let guard = self
            .vault
            .read()
            .map_err(|_| gapura_core::GapuraError::Other("vault lock poisoned".into()))?;
        match guard.as_ref() {
            Some(vault) => f(vault),
            None => Err(gapura_core::GapuraError::VaultLocked),
        }
    }

    /// Mutable access with the vault held under the write lock for the whole
    /// call, so mutations and saves are atomic from the command's viewpoint.
    pub fn with_vault_mut<T>(
        &self,
        f: impl FnOnce(&mut UnlockedVault) -> Result<T, gapura_core::GapuraError>,
    ) -> Result<T, gapura_core::GapuraError> {
        let mut guard = self
            .vault
            .write()
            .map_err(|_| gapura_core::GapuraError::Other("vault lock poisoned".into()))?;
        match guard.as_mut() {
            Some(vault) => {
                let out = f(vault)?;
                vault.save()?;
                Ok(out)
            }
            None => Err(gapura_core::GapuraError::VaultLocked),
        }
    }

    pub fn vault_mode(&self) -> Option<VaultMode> {
        self.vault
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(|v| v.mode()))
    }

    pub fn put_session(&self, id: Uuid, entry: SessionEntry) {
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.insert(id, entry);
        }
    }

    pub fn take_session(&self, id: &Uuid) -> Option<SessionEntry> {
        self.sessions.write().ok()?.remove(id)
    }
}
