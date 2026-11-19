//! Session lifecycle commands: open, write, resize, close, reconnect.

use std::sync::Arc;
use std::time::Instant;

use base64::Engine;
use chrono::Utc;
use gapura_core::model::{AuthMethod, Connection};
use gapura_core::ssh::backend::RemoteForwardRegistry;
use gapura_core::ssh::backend::{connect, test_connect, ConnectParams, ResolvedAuth};
use gapura_core::ssh::session::{
    start_session, EventSink, SessionCommand, SessionEvent, SessionState,
};
use gapura_core::GapuraError;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Runtime, State};
use uuid::Uuid;

use crate::commands::connections::AuthInput;
use crate::commands::forwards;
use crate::error::CommandResult;
use crate::state::{AppState, SessionEntry};

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: Uuid,
    pub connection_id: Uuid,
    /// Fingerprint pinned on this connect (what the UI may toast about).
    pub fingerprint: String,
    pub new_host_key: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionTab {
    pub id: Uuid,
    pub connection_id: Uuid,
    pub name: String,
    pub color: Option<String>,
}

/// Read-only snapshot of live sessions, used to rebuild the tab bar.
#[tauri::command]
pub fn list_sessions(state: tauri::State<'_, AppState>) -> CommandResult<Vec<SessionTab>> {
    let sessions = state
        .sessions
        .read()
        .map_err(|_| GapuraError::Other("session lock poisoned".into()))?;
    let mut tabs = Vec::new();
    for (id, entry) in sessions.iter() {
        let name = state
            .with_vault(|vault| {
                Ok(vault
                    .payload
                    .connection(entry.connection_id)
                    .map(|c| c.name.clone()))
            })
            .ok()
            .flatten()
            .unwrap_or_else(|| "session".to_string());
        let color = state
            .with_vault(|vault| {
                Ok(vault
                    .payload
                    .connection(entry.connection_id)
                    .and_then(|c| c.color.clone()))
            })
            .ok()
            .flatten();
        tabs.push(SessionTab {
            id: *id,
            connection_id: entry.connection_id,
            name,
            color,
        });
    }
    Ok(tabs)
}

/// Input for the pre-save connection test: same shape as the form.
#[derive(Debug, Deserialize)]
pub struct TestConnectionInput {
    /// Set when editing: lets a blank password/passphrase fall back to the
    /// secret already stored for that connection.
    pub id: Option<Uuid>,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthInput,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestConnectionResult {
    pub fingerprint: String,
    pub algorithm: String,
    pub new_host_key: bool,
    pub latency_ms: u32,
}

/// Handshake + authenticate without opening a shell. Proves credentials and
/// shows the host fingerprint before the connection is saved. First-use host
/// keys are reported but NOT pinned (see `test_connect` in core).
#[tauri::command]
pub async fn test_connection(
    state: State<'_, AppState>,
    input: TestConnectionInput,
) -> CommandResult<TestConnectionResult> {
    let host = input.host.trim().to_string();
    if host.is_empty() {
        return Err(GapuraError::Other("host cannot be empty".into()));
    }
    if input.port == 0 {
        return Err(GapuraError::Other("port must be 1-65535".into()));
    }
    let auth = resolve_test_auth(&state, &input)?;
    let params = ConnectParams {
        host: host.clone(),
        port: input.port,
        username: input.username.trim().to_string(),
        auth,
    };
    let started = Instant::now();
    let outcome = test_connect(params, Arc::clone(&state.known_hosts)).await?;
    let latency_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;
    let result = TestConnectionResult {
        fingerprint: outcome.fingerprint.clone(),
        algorithm: outcome.algorithm.clone(),
        new_host_key: outcome.new_host_key,
        latency_ms,
    };
    let _ = outcome.close().await;
    tracing::info!(%host, port = input.port, latency_ms, "connection test succeeded");
    Ok(result)
}

/// Open a session for a connection. The heavy lifting runs on the async
/// runtime; the actor emits `session:data` / `session:status` events.
#[tauri::command]
pub async fn session_open<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    connection_id: Uuid,
    cols: u32,
    rows: u32,
) -> CommandResult<SessionInfo> {
    open_and_register(&app, &state, connection_id, None, cols, rows).await
}

/// Reconnect an existing session (reconnect banner): same session id, fresh
/// connection. The terminal keeps its scrollback on the UI side.
#[tauri::command]
pub async fn session_reconnect<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    session_id: Uuid,
    cols: u32,
    rows: u32,
) -> CommandResult<SessionInfo> {
    let connection_id = {
        let sessions = state
            .sessions
            .read()
            .map_err(|_| GapuraError::Other("session lock poisoned".into()))?;
        sessions
            .get(&session_id)
            .map(|e| e.connection_id)
            .ok_or_else(|| GapuraError::Other("session not found".into()))?
    };
    open_and_register(&app, &state, connection_id, Some(session_id), cols, rows).await
}

#[tauri::command]
pub async fn session_write(
    state: State<'_, AppState>,
    session_id: Uuid,
    data: String,
) -> CommandResult<()> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|e| GapuraError::Other(format!("terminal payload is not valid base64: {e}")))?;
    send_command(&state, &session_id, SessionCommand::Write(bytes)).await
}

#[tauri::command]
pub async fn session_resize(
    state: State<'_, AppState>,
    session_id: Uuid,
    cols: u32,
    rows: u32,
) -> CommandResult<()> {
    send_command(&state, &session_id, SessionCommand::Resize(cols, rows)).await
}

#[tauri::command]
pub async fn session_close(state: State<'_, AppState>, session_id: Uuid) -> CommandResult<()> {
    tracing::info!(session = %session_id, "session close requested by UI");
    send_command(&state, &session_id, SessionCommand::Close).await?;
    let _ = state.take_session(&session_id);
    Ok(())
}

pub(crate) async fn send_command(
    state: &AppState,
    session_id: &Uuid,
    command: SessionCommand,
) -> CommandResult<()> {
    let sender = {
        let sessions = state
            .sessions
            .read()
            .map_err(|_| GapuraError::Other("session lock poisoned".into()))?;
        sessions
            .get(session_id)
            .map(|e| e.commands.clone())
            .ok_or_else(|| GapuraError::Other("session not found".into()))?
    };
    sender
        .send(command)
        .await
        .map_err(|_| GapuraError::Other("session is gone".into()))
}

/// Shared flow for open and reconnect.
async fn open_and_register<R: Runtime>(
    app: &AppHandle<R>,
    state: &State<'_, AppState>,
    connection_id: Uuid,
    reuse_session_id: Option<Uuid>,
    cols: u32,
    rows: u32,
) -> CommandResult<SessionInfo> {
    let connection = state.with_vault(|vault| {
        vault
            .payload
            .connection(connection_id)
            .cloned()
            .ok_or_else(|| GapuraError::Other("connection not found".into()))
    })?;
    let auth = resolve_auth(state, &connection)?;

    let params = ConnectParams {
        host: connection.host.clone(),
        port: connection.port,
        username: connection.username.clone(),
        auth,
    };

    let known_hosts = Arc::clone(&state.known_hosts);
    let remote_forwards: RemoteForwardRegistry = Default::default();
    // TODO(F13): jump chains are implemented in core (connect_with_jump) but
    // disabled at the app layer pending investigation of a corrupted
    // handshake over the direct-tcpip tunnel (see feat/jump-snippets notes).
    let _ = &resolve_jump_chain;
    let outcome = connect(params, known_hosts, Arc::clone(&remote_forwards)).await?;

    if outcome.new_host_key {
        let _ = app.emit(
            "hostkey:added",
            serde_json::json!({
                "host": connection.host,
                "fingerprint": outcome.fingerprint,
            }),
        );
    }

    let session_id = reuse_session_id.unwrap_or_else(Uuid::new_v4);
    tracing::info!(
        session = %session_id,
        host = %connection.host,
        reconnect = reuse_session_id.is_some(),
        "session opened"
    );
    let sink = event_sink(app.clone(), session_id);
    let commands = start_session(outcome.handle, cols, rows, sink).await?;

    state.put_session(
        session_id,
        SessionEntry {
            connection_id,
            commands,
            remote_forwards,
        },
    );

    // Activity bookkeeping for the "recent" ordering and diagnostics.
    record_connection_opened(state, connection_id);

    // Start enabled auto-start rules (PRD F12) once the session is live.
    forwards::autostart_forwards(state, session_id, connection_id).await;

    Ok(SessionInfo {
        id: session_id,
        connection_id,
        fingerprint: outcome.fingerprint,
        new_host_key: outcome.new_host_key,
    })
}

/// Resolve the jump chain for a connection (PRD F13): follow jump_host_id
/// up to two levels, rejecting cycles and missing connections.
fn resolve_jump_chain(
    state: &tauri::State<'_, AppState>,
    connection_id: Uuid,
) -> CommandResult<Vec<ConnectParams>> {
    let mut chain: Vec<ConnectParams> = Vec::new();
    let mut seen: Vec<Uuid> = vec![connection_id];
    let mut next_id = state.with_vault(|vault| {
        Ok(vault
            .payload
            .connection(connection_id)
            .and_then(|c| c.jump_host_id))
    })?;
    while let Some(jump_id) = next_id {
        if chain.len() >= 2 {
            return Err(GapuraError::Other(
                "jump chains are limited to two levels".into(),
            ));
        }
        if seen.contains(&jump_id) {
            return Err(GapuraError::Other(
                "jump host chain loops back on itself".into(),
            ));
        }
        let jump = state.with_vault(|vault| {
            vault
                .payload
                .connection(jump_id)
                .cloned()
                .ok_or_else(|| GapuraError::Other("the jump host connection is missing".into()))
        })?;
        next_id = jump.jump_host_id;
        seen.push(jump_id);
        chain.push(ConnectParams {
            host: jump.host.clone(),
            port: jump.port,
            username: jump.username.clone(),
            auth: resolve_auth(state, &jump)?,
        });
    }
    Ok(chain)
}

/// Pull concrete credentials out of the vault at connect time (design doc:
/// decrypt on use, drop immediately after).
fn resolve_auth(
    state: &tauri::State<'_, AppState>,
    connection: &Connection,
) -> CommandResult<ResolvedAuth> {
    state.with_vault(|vault| match &connection.auth {
        AuthMethod::Agent => Ok(ResolvedAuth::Agent),
        AuthMethod::Password { secret_id } => {
            let slot = vault.payload.secret(*secret_id).ok_or_else(|| {
                GapuraError::Other("the stored password is missing from the vault".into())
            })?;
            Ok(ResolvedAuth::Password {
                password: slot.value.expose().to_string(),
            })
        }
        AuthMethod::KeyFile {
            path,
            passphrase_secret_id,
            ..
        } => {
            let passphrase = passphrase_secret_id
                .and_then(|id| vault.payload.secret(id))
                .map(|slot| slot.value.expose().to_string());
            Ok(ResolvedAuth::KeyFile {
                path: path.clone(),
                passphrase,
            })
        }
    })
}

/// Resolve credentials for the pre-save test. A blank password/passphrase
/// falls back to the secret already attached to the connection being edited.
fn resolve_test_auth(
    state: &State<'_, AppState>,
    input: &TestConnectionInput,
) -> CommandResult<ResolvedAuth> {
    match &input.auth {
        AuthInput::Agent => Ok(ResolvedAuth::Agent),
        AuthInput::Password { password } => {
            if let Some(value) = password.as_deref().filter(|p| !p.is_empty()) {
                return Ok(ResolvedAuth::Password {
                    password: value.to_string(),
                });
            }
            let stored = stored_secret(state, input.id, |connection| match &connection.auth {
                AuthMethod::Password { secret_id } => Some(*secret_id),
                _ => None,
            })?;
            stored
                .map(|password| ResolvedAuth::Password { password })
                .ok_or_else(|| {
                    GapuraError::Other("a password is required for password authentication".into())
                })
        }
        AuthInput::KeyFile { path, passphrase } => {
            if path.trim().is_empty() {
                return Err(GapuraError::Other("choose a private key file".into()));
            }
            let passphrase = match passphrase.as_deref().filter(|p| !p.is_empty()) {
                Some(value) => Some(value.to_string()),
                None => stored_secret(state, input.id, |connection| match &connection.auth {
                    AuthMethod::KeyFile {
                        passphrase_secret_id,
                        ..
                    } => *passphrase_secret_id,
                    _ => None,
                })?,
            };
            Ok(ResolvedAuth::KeyFile {
                path: path.into(),
                passphrase,
            })
        }
    }
}

/// Fetch a secret referenced by an existing connection, if any.
fn stored_secret(
    state: &State<'_, AppState>,
    connection_id: Option<Uuid>,
    pick: impl FnOnce(&Connection) -> Option<Uuid>,
) -> CommandResult<Option<String>> {
    state.with_vault(|vault| {
        let value = connection_id
            .and_then(|id| vault.payload.connection(id))
            .and_then(pick)
            .and_then(|secret_id| vault.payload.secret(secret_id))
            .map(|slot| slot.value.expose().to_string());
        Ok(value)
    })
}

/// Update last-connected activity; failures are non-fatal (the session is
/// already live and must not be torn down for bookkeeping).
fn record_connection_opened(state: &State<'_, AppState>, connection_id: Uuid) {
    let result = state.with_vault_mut(|vault| {
        if let Some(connection) = vault
            .payload
            .connections
            .iter_mut()
            .find(|c| c.id == connection_id)
        {
            connection.last_connected_at = Some(Utc::now());
            connection.connect_count = connection.connect_count.saturating_add(1);
            connection.updated_at = Utc::now();
        }
        Ok(())
    });
    if let Err(error) = result {
        tracing::warn!(%connection_id, "cannot record connection activity: {error}");
    }
}

fn event_sink<R: Runtime>(app: AppHandle<R>, session_id: Uuid) -> EventSink {
    Arc::new(move |event: SessionEvent| match event {
        SessionEvent::Data(bytes) => {
            let payload = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let _ = app.emit(
                "session:data",
                serde_json::json!({
                    "id": session_id,
                    "data": payload,
                }),
            );
        }
        SessionEvent::Status(state) => {
            let state_json = match state {
                SessionState::Connected => serde_json::json!("connected"),
                SessionState::Closed { reason } => serde_json::json!({ "closed": reason }),
            };
            let _ = app.emit(
                "session:status",
                serde_json::json!({
                    "id": session_id,
                    "state": state_json,
                }),
            );
        }
        SessionEvent::ForwardStatus {
            rule_id,
            ok,
            message,
        } => {
            let _ = app.emit(
                "forward:status",
                serde_json::json!({
                    "rule_id": rule_id,
                    "ok": ok,
                    "message": message,
                }),
            );
        }
    })
}
