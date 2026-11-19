//! Snippets (PRD F14): saved commands, global or per-connection.

use chrono::Utc;
use gapura_core::model::Snippet;
use gapura_core::ssh::session::SessionCommand;
use gapura_core::GapuraError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::commands::sessions::send_command;
use crate::error::CommandResult;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SnippetInput {
    pub id: Option<Uuid>,
    pub name: String,
    pub command: String,
    pub connection_id: Option<Uuid>,
    #[serde(default)]
    pub require_confirm: bool,
    #[serde(default = "default_true")]
    pub send_enter: bool,
}

fn default_true() -> bool {
    true
}

/// The expanded command, echoed back so the UI can show what was sent.
#[derive(Debug, Clone, Serialize)]
pub struct RunSnippetResult {
    pub command: String,
}

#[tauri::command]
pub fn list_snippets(
    state: tauri::State<'_, AppState>,
    connection_id: Option<Uuid>,
) -> CommandResult<Vec<Snippet>> {
    state.with_vault(|vault| {
        let mut snippets: Vec<Snippet> = vault
            .payload
            .snippets
            .iter()
            // No connection filter means "manage view": show everything,
            // global and per-connection. With a connection, show globals
            // plus that connection's snippets.
            .filter(|s| match connection_id {
                None => true,
                Some(id) => s.connection_id.is_none() || s.connection_id == Some(id),
            })
            .cloned()
            .collect();
        snippets.sort_by_key(|a| a.name.to_lowercase());
        Ok(snippets)
    })
}

#[tauri::command]
pub fn save_snippet(
    state: tauri::State<'_, AppState>,
    input: SnippetInput,
) -> CommandResult<Snippet> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(GapuraError::Other("give the snippet a name".into()));
    }
    if input.command.trim().is_empty() {
        return Err(GapuraError::Other("the command cannot be empty".into()));
    }
    state.with_vault_mut(|vault| {
        if let Some(id) = input.id {
            let snippet = vault
                .payload
                .snippets
                .iter_mut()
                .find(|s| s.id == id)
                .ok_or_else(|| GapuraError::Other("snippet not found".into()))?;
            snippet.name = name;
            snippet.command = input.command.trim().to_string();
            snippet.connection_id = input.connection_id;
            snippet.require_confirm = input.require_confirm;
            snippet.send_enter = input.send_enter;
            return Ok(snippet.clone());
        }
        let snippet = Snippet {
            id: Uuid::new_v4(),
            name,
            command: input.command.trim().to_string(),
            connection_id: input.connection_id,
            require_confirm: input.require_confirm,
            send_enter: input.send_enter,
            created_at: Utc::now(),
        };
        vault.payload.snippets.push(snippet.clone());
        Ok(snippet)
    })
}

#[tauri::command]
pub fn delete_snippet(state: tauri::State<'_, AppState>, id: Uuid) -> CommandResult<()> {
    state.with_vault_mut(|vault| {
        let before = vault.payload.snippets.len();
        vault.payload.snippets.retain(|s| s.id != id);
        if vault.payload.snippets.len() == before {
            return Err(GapuraError::Other("snippet not found".into()));
        }
        Ok(())
    })
}

/// Expand a snippet against the session's connection and type it into the
/// terminal. Expansion and the trailing-enter decision live here so the UI
/// does not need to duplicate the templating rules.
#[tauri::command]
pub async fn run_snippet(
    state: tauri::State<'_, AppState>,
    session_id: Uuid,
    snippet_id: Uuid,
) -> CommandResult<RunSnippetResult> {
    let connection_id = {
        let sessions = state
            .sessions
            .read()
            .map_err(|_| GapuraError::Other("session lock poisoned".into()))?;
        sessions
            .get(&session_id)
            .map(|entry| entry.connection_id)
            .ok_or_else(|| GapuraError::Other("session not found".into()))?
    };

    let (snippet, connection) = state.with_vault(|vault| {
        let snippet = vault
            .payload
            .snippets
            .iter()
            .find(|s| s.id == snippet_id)
            .cloned()
            .ok_or_else(|| GapuraError::Other("snippet not found".into()))?;
        let connection = vault
            .payload
            .connection(connection_id)
            .cloned()
            .ok_or_else(|| GapuraError::Other("connection not found".into()))?;
        Ok((snippet, connection))
    })?;

    if let Some(scope) = snippet.connection_id {
        if scope != connection_id {
            return Err(GapuraError::Other(
                "this snippet belongs to another connection".into(),
            ));
        }
    }

    let command = gapura_core::snippet::expand_placeholders(&snippet.command, &connection);
    let mut payload = command.clone().into_bytes();
    if snippet.send_enter {
        payload.push(b'\n');
    }
    send_command(&state, &session_id, SessionCommand::Write(payload)).await?;
    tracing::info!(%session_id, %snippet_id, "snippet sent");
    Ok(RunSnippetResult { command })
}
