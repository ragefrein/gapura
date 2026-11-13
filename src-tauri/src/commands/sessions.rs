//! Session lifecycle commands: open, write, resize, close, reconnect.

use std::sync::Arc;

use base64::Engine;
use gapura_core::model::{AuthMethod, Connection};
use gapura_core::ssh::backend::{connect, ConnectParams, ResolvedAuth};
use gapura_core::ssh::session::{
    start_session, EventSink, SessionCommand, SessionEvent, SessionState,
};
use gapura_core::GapuraError;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, State};
use uuid::Uuid;

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
    send_command(&state, &session_id, SessionCommand::Close).await?;
    let _ = state.take_session(&session_id);
    Ok(())
}

async fn send_command(
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
    let outcome = connect(params, known_hosts).await?;

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
    let sink = event_sink(app.clone(), session_id);
    let commands = start_session(outcome.handle, cols, rows, sink).await?;

    state.put_session(
        session_id,
        SessionEntry {
            connection_id,
            commands,
        },
    );

    Ok(SessionInfo {
        id: session_id,
        connection_id,
        fingerprint: outcome.fingerprint,
        new_host_key: outcome.new_host_key,
    })
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
    })
}
