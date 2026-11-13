//! Connection and group CRUD (PRD F1).

use chrono::Utc;
use gapura_core::model::{AuthMethod, Connection, Group, SecretKind, SecretSlot, SecretValue};
use gapura_core::GapuraError;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::CommandResult;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum AuthInput {
    Agent,
    Password {
        /// None keeps the stored password (used when editing a connection).
        password: Option<String>,
    },
    KeyFile {
        path: String,
        passphrase: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
pub struct ConnectionInput {
    pub id: Option<Uuid>,
    pub name: String,
    pub group_id: Option<Uuid>,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthInput,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub notes: String,
}

#[tauri::command]
pub fn list_connections(state: tauri::State<'_, AppState>) -> CommandResult<Vec<Connection>> {
    state.with_vault(|vault| {
        let mut connections = vault.payload.connections.clone();
        connections.sort_by_key(|a| a.name.to_lowercase());
        Ok(connections)
    })
}

#[tauri::command]
pub fn save_connection(
    state: tauri::State<'_, AppState>,
    input: ConnectionInput,
) -> CommandResult<Connection> {
    if input.host.trim().is_empty() {
        return Err(GapuraError::Other("host cannot be empty".into()));
    }
    if input.name.trim().is_empty() {
        return Err(GapuraError::Other("name cannot be empty".into()));
    }

    state.with_vault_mut(|vault| {
        let now = Utc::now();
        let existing_id = input.id;

        // Resolve the auth method against the secret store.
        let auth = match input.auth {
            AuthInput::Agent => AuthMethod::Agent,
            AuthInput::Password { password } => match password {
                Some(value) if !value.is_empty() => {
                    let secret_id = upsert_password_secret(
                        vault,
                        existing_id,
                        SecretValue::from(value),
                        SecretKind::Password,
                    );
                    AuthMethod::Password { secret_id }
                }
                Some(_) | None => {
                    // Empty/absent: keep the existing secret binding.
                    let prior = existing_id
                        .and_then(|id| find_connection(&vault.payload, id))
                        .map(|c| c.auth.clone());
                    match prior {
                        Some(a @ AuthMethod::Password { .. }) => a,
                        _ => {
                            return Err(GapuraError::Other(
                                "a password is required for password authentication".into(),
                            ))
                        }
                    }
                }
            },
            AuthInput::KeyFile { path, passphrase } => {
                let passphrase_secret_id = match passphrase {
                    Some(value) if !value.is_empty() => Some(upsert_password_secret(
                        vault,
                        existing_id,
                        SecretValue::from(value),
                        SecretKind::KeyPassphrase,
                    )),
                    _ => None,
                };
                AuthMethod::KeyFile {
                    path: path.into(),
                    key_id: None,
                    passphrase_secret_id,
                }
            }
        };

        let mut connection = Connection {
            id: existing_id.unwrap_or_else(Uuid::new_v4),
            name: input.name.trim().to_string(),
            group_id: input.group_id,
            host: input.host.trim().to_string(),
            port: input.port,
            username: input.username.trim().to_string(),
            auth,
            tags: input.tags,
            color: input.color,
            jump_host_id: None,
            notes: input.notes,
            created_at: now,
            updated_at: now,
        };

        if let Some(slot) = vault
            .payload
            .connections
            .iter_mut()
            .find(|c| c.id == connection.id)
        {
            connection.created_at = slot.created_at;
            *slot = connection.clone();
        } else {
            vault.payload.connections.push(connection.clone());
        }
        Ok(connection)
    })
}

#[tauri::command]
pub fn delete_connection(state: tauri::State<'_, AppState>, id: Uuid) -> CommandResult<()> {
    state.with_vault_mut(|vault| {
        let before = vault.payload.connections.len();
        vault.payload.connections.retain(|c| c.id != id);
        if vault.payload.connections.len() == before {
            return Err(GapuraError::Other("connection not found".into()));
        }
        // Secrets that nothing references anymore are dropped.
        let secret_ids: Vec<Uuid> = vault.payload.secrets.iter().map(|s| s.id).collect();
        for secret_id in secret_ids {
            vault.payload.remove_secret_if_unused(secret_id);
        }
        Ok(())
    })
}

#[tauri::command]
pub fn list_groups(state: tauri::State<'_, AppState>) -> CommandResult<Vec<Group>> {
    state.with_vault(|vault| {
        let mut groups = vault.payload.groups.clone();
        groups.sort_by_key(|a| a.name.to_lowercase());
        Ok(groups)
    })
}

#[tauri::command]
pub fn save_group(
    state: tauri::State<'_, AppState>,
    id: Option<Uuid>,
    name: String,
) -> CommandResult<Group> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(GapuraError::Other("group name cannot be empty".into()));
    }
    state.with_vault_mut(|vault| {
        if let Some(id) = id {
            let group = vault
                .payload
                .groups
                .iter_mut()
                .find(|g| g.id == id)
                .ok_or_else(|| GapuraError::Other("group not found".into()))?;
            group.name = name.clone();
            return Ok(group.clone());
        }
        let group = Group {
            id: Uuid::new_v4(),
            name,
            created_at: Utc::now(),
        };
        vault.payload.groups.push(group.clone());
        Ok(group)
    })
}

#[tauri::command]
pub fn delete_group(state: tauri::State<'_, AppState>, id: Uuid) -> CommandResult<()> {
    state.with_vault_mut(|vault| {
        vault.payload.groups.retain(|g| g.id != id);
        for connection in &mut vault.payload.connections {
            if connection.group_id == Some(id) {
                connection.group_id = None;
            }
        }
        Ok(())
    })
}

fn find_connection(vault: &gapura_core::model::VaultPayload, id: Uuid) -> Option<&Connection> {
    vault.connection(id)
}

/// Create a fresh secret slot for a connection, replacing any previous slot
/// it owned (the old value becomes unreachable and is garbage-collected on
/// the next delete/save).
fn upsert_password_secret(
    vault: &mut gapura_core::vault::UnlockedVault,
    connection_id: Option<Uuid>,
    value: SecretValue,
    kind: SecretKind,
) -> Uuid {
    // Drop slots the connection currently owns so secrets do not accumulate.
    if let Some(conn_id) = connection_id {
        let stale: Vec<Uuid> = vault
            .payload
            .connection(conn_id)
            .map(|c| match &c.auth {
                AuthMethod::Password { secret_id } => vec![*secret_id],
                AuthMethod::KeyFile {
                    passphrase_secret_id,
                    ..
                } => passphrase_secret_id.iter().copied().collect(),
                AuthMethod::Agent => vec![],
            })
            .unwrap_or_default();
        vault.payload.secrets.retain(|s| !stale.contains(&s.id));
    }

    let id = Uuid::new_v4();
    vault.payload.secrets.push(SecretSlot {
        id,
        kind,
        value,
        created_at: Utc::now(),
    });
    id
}
