//! Tauri state: the unlocked vault, active sessions, known hosts, settings.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use std::collections::HashMap as StdHashMap;

use gapura_core::model::ForwardKind;
use gapura_core::settings::Settings;
pub use gapura_core::ssh::backend::RemoteForwardRegistry;
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
    /// Running port forwards, keyed by rule id.
    pub forwards: RwLock<StdHashMap<Uuid, ForwardRuntime>>,
    /// Last reachability result per connection, so polling does not re-probe
    /// every host on each UI refresh.
    pub status_cache: Mutex<StdHashMap<Uuid, CachedStatus>>,
}

/// One cached TCP probe result.
pub struct CachedStatus {
    pub up: bool,
    pub latency_ms: Option<u32>,
    pub checked_at: Instant,
}

pub struct SessionEntry {
    pub connection_id: Uuid,
    pub commands: mpsc::Sender<SessionCommand>,
    /// Registry the connection's handler reads when the server hands us a
    /// forwarded-tcpip channel (Remote forward rules).
    pub remote_forwards: RemoteForwardRegistry,
}

/// A running port-forward (PRD F12), one per enabled rule per session.
pub struct ForwardRuntime {
    pub session_id: Uuid,
    pub kind: ForwardKind,
    pub bind_host: String,
    pub bind_port: u16,
    pub target_host: String,
    pub target_port: u16,
    /// Accept-loop task for Local rules; drives nothing for Remote rules.
    pub task: tokio::task::JoinHandle<()>,
    /// For Remote rules: sends a RemoteCancel command to the actor.
    pub commands: Option<mpsc::Sender<SessionCommand>>,
    /// For Remote rules: the registry key to unregister.
    pub registry_key: Option<(String, u16)>,
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
            forwards: RwLock::new(StdHashMap::new()),
            status_cache: Mutex::new(StdHashMap::new()),
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

    /// Stop every forward bound to a session (tab close, quit, reconnect).
    pub fn stop_session_forwards(&self, session_id: &Uuid) {
        let Ok(mut forwards) = self.forwards.write() else {
            return;
        };
        let doomed: Vec<Uuid> = forwards
            .iter()
            .filter(|(_, f)| &f.session_id == session_id)
            .map(|(id, _)| *id)
            .collect();
        for id in doomed {
            if let Some(mut runtime) = forwards.remove(&id) {
                stop_runtime(&mut runtime);
            }
        }
    }

    pub fn put_forward(&self, rule_id: Uuid, runtime: ForwardRuntime) {
        if let Ok(mut forwards) = self.forwards.write() {
            if let Some(old) = forwards.insert(rule_id, runtime) {
                stop_runtime(&mut { old });
            }
        }
    }

    pub fn take_forward(&self, rule_id: &Uuid) -> Option<ForwardRuntime> {
        self.forwards.write().ok()?.remove(rule_id)
    }
}

/// Stop one runtime: abort the listener (Local) and/or ask the server to
/// stop listening (Remote).
pub fn stop_runtime(runtime: &mut ForwardRuntime) {
    runtime.task.abort();
    if let (Some(commands), Some((bind_host, bind_port))) =
        (runtime.commands.take(), runtime.registry_key.take())
    {
        let _ = commands.try_send(SessionCommand::RemoteCancel {
            rule_id: Uuid::nil(),
            bind_host,
            bind_port,
        });
    }
}
