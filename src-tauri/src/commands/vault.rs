//! Vault commands: setup, unlock, lock, status.

use gapura_core::ssh::session::SessionCommand;
use gapura_core::vault::{self, VaultMode};
use gapura_core::GapuraError;
use serde::Serialize;

use crate::error::CommandResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct VaultStatus {
    pub exists: bool,
    pub unlocked: bool,
    pub mode: Option<String>,
    /// Set when the last keychain unlock failed; the UI shows a recovery
    /// hint instead of a bare passphrase prompt.
    pub keychain_error: Option<String>,
}

#[tauri::command]
pub fn vault_status(state: tauri::State<'_, AppState>) -> VaultStatus {
    let exists = vault::vault_exists(&state.data_dir);
    let unlocked = state.vault.read().map(|g| g.is_some()).unwrap_or(false);
    // When locked, the in-memory vault is None — the mode must come from the
    // file header instead, or the UI can never decide how to unlock.
    let mode = if unlocked {
        state.vault_mode().map(|m| m.as_str().to_string())
    } else {
        vault::peek_mode(&state.data_dir)
            .ok()
            .flatten()
            .map(|m| m.as_str().to_string())
    };
    let keychain_error = state
        .keychain_error
        .lock()
        .ok()
        .and_then(|slot| slot.clone());
    VaultStatus {
        exists,
        unlocked,
        mode,
        keychain_error,
    }
}

#[tauri::command]
pub async fn vault_setup(
    state: tauri::State<'_, AppState>,
    mode: String,
    passphrase: Option<String>,
) -> CommandResult<()> {
    let vault_mode = match mode.as_str() {
        "keychain" => VaultMode::Keychain,
        "passphrase" => VaultMode::Passphrase,
        other => return Err(GapuraError::Other(format!("unknown vault mode {other:?}"))),
    };
    let store = keychain_store();
    let data_dir = state.data_dir.clone();
    let vault = tauri::async_runtime::spawn_blocking(move || {
        vault::setup(&data_dir, vault_mode, passphrase.as_deref(), &store)
    })
    .await
    .map_err(|e| GapuraError::Other(format!("setup task failed: {e}")))??;

    let mut guard = state
        .vault
        .write()
        .map_err(|_| GapuraError::Other("vault lock poisoned".into()))?;
    *guard = Some(vault);
    Ok(())
}

#[tauri::command]
pub async fn vault_unlock(
    state: tauri::State<'_, AppState>,
    passphrase: Option<String>,
) -> CommandResult<()> {
    let store = keychain_store();
    let data_dir = state.data_dir.clone();
    tracing::info!("vault unlock requested");
    // The Secret Service can stall when the login keyring is locked; give it
    // a bounded window instead of leaving the UI hanging forever.
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tauri::async_runtime::spawn_blocking(move || {
            vault::unlock(&data_dir, passphrase.as_deref(), &store)
        }),
    )
    .await
    .map_err(|_| GapuraError::Keychain(
        "the system keychain did not answer within 30 seconds — is the login keyring unlocked?".into(),
    ))
    .and_then(|inner| inner.map_err(|e| GapuraError::Other(format!("unlock task failed: {e}"))));

    match result {
        Ok(Ok(vault)) => {
            tracing::info!("vault unlocked");
            let mut guard = state
                .vault
                .write()
                .map_err(|_| GapuraError::Other("vault lock poisoned".into()))?;
            *guard = Some(vault);
            Ok(())
        }
        Ok(Err(e)) => {
            // Every unlock failure is surfaced to the UI; the locked screen
            // must never show a bare spinner with no way forward.
            tracing::warn!(code = e.code(), "vault unlock failed: {e}");
            if let Ok(mut slot) = state.keychain_error.lock() {
                *slot = Some(e.to_string());
            }
            Err(e)
        }
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub fn vault_lock(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    // Dropping the vault locks it. Live sessions keep running on purpose:
    // closing an SSH channel mid-work would be more harmful than keeping it.
    let mut guard = state
        .vault
        .write()
        .map_err(|_| GapuraError::Other("vault lock poisoned".into()))?;
    *guard = None;
    Ok(())
}

/// Close every live session (used on quit).
#[tauri::command]
pub fn close_all_sessions(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    if let Ok(mut sessions) = state.sessions.write() {
        for (_id, entry) in sessions.drain() {
            let _ = entry.commands.try_send(SessionCommand::Close);
        }
    }
    Ok(())
}

fn keychain_store() -> gapura_core::vault::keychain::OsKeychain {
    gapura_core::vault::keychain::OsKeychain::new(
        gapura_core::vault::keychain::KeychainEntryRef::vault_master(),
    )
}
