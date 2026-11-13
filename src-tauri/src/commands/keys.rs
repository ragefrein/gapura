//! Key management commands (PRD F8).

use gapura_core::model::{SecretKind, SecretSlot, SecretValue, SshKey};
use gapura_core::ssh::keys::{generate_ed25519, inspect_private_key};
use gapura_core::GapuraError;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::CommandResult;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct GenerateKeyInput {
    /// File name inside ~/.ssh, e.g. `id_ed25519_gapura`.
    pub name: String,
    pub passphrase: Option<String>,
}

#[tauri::command]
pub fn list_keys(state: tauri::State<'_, AppState>) -> CommandResult<Vec<SshKey>> {
    state.with_vault(|vault| Ok(vault.payload.keys.clone()))
}

#[tauri::command]
pub fn generate_key(
    state: tauri::State<'_, AppState>,
    input: GenerateKeyInput,
) -> CommandResult<SshKey> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(GapuraError::Other("give the key a name".into()));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(GapuraError::Other(
            "key names may only contain letters, digits, '-', '_' and '.'".into(),
        ));
    }

    let ssh_dir = dirs_home().join(".ssh");
    let path = ssh_dir.join(&name);
    let info = generate_ed25519(&path, "gapura", input.passphrase.as_deref())?;

    state.with_vault_mut(|vault| {
        let key = SshKey {
            id: Uuid::new_v4(),
            name: name.clone(),
            path: path.clone(),
            public_key: info.public_key,
            fingerprint: info.fingerprint,
            has_passphrase: info.has_passphrase,
            created_at: chrono::Utc::now(),
        };
        vault.payload.keys.push(key.clone());

        // A passphrase-protected key keeps its passphrase as a vault secret.
        if let Some(pass) = input.passphrase.filter(|p| !p.is_empty()) {
            vault.payload.secrets.push(SecretSlot {
                id: Uuid::new_v4(),
                kind: SecretKind::KeyPassphrase,
                value: SecretValue::from(pass),
                created_at: chrono::Utc::now(),
            });
        }
        Ok(key)
    })
}

/// Register an existing key file (registers path + fingerprint only).
#[tauri::command]
pub fn register_key(
    state: tauri::State<'_, AppState>,
    path: String,
    passphrase: Option<String>,
) -> CommandResult<SshKey> {
    let info = inspect_private_key(std::path::Path::new(&path), passphrase.as_deref())?;
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "key".into());

    state.with_vault_mut(|vault| {
        if vault
            .payload
            .keys
            .iter()
            .any(|k| k.path.as_os_str() == path.as_str())
        {
            return Err(GapuraError::Other("this key is already registered".into()));
        }
        let key = SshKey {
            id: Uuid::new_v4(),
            name,
            path: std::path::PathBuf::from(&path),
            public_key: info.public_key,
            fingerprint: info.fingerprint,
            has_passphrase: info.has_passphrase,
            created_at: chrono::Utc::now(),
        };
        vault.payload.keys.push(key.clone());
        Ok(key)
    })
}

/// Remove a key from the registry only; the file itself is kept.
#[tauri::command]
pub fn delete_key(state: tauri::State<'_, AppState>, id: Uuid) -> CommandResult<()> {
    state.with_vault_mut(|vault| {
        let before = vault.payload.keys.len();
        vault.payload.keys.retain(|k| k.id != id);
        if vault.payload.keys.len() == before {
            return Err(GapuraError::Other("key not found".into()));
        }
        Ok(())
    })
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}
