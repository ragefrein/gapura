//! Snippets (PRD F14): saved commands, global or per-connection.

use chrono::Utc;
use gapura_core::model::Snippet;
use gapura_core::GapuraError;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::CommandResult;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SnippetInput {
    pub id: Option<Uuid>,
    pub name: String,
    pub command: String,
    pub connection_id: Option<Uuid>,
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
            .filter(|s| s.connection_id.is_none() || s.connection_id == connection_id)
            .cloned()
            .collect();
        snippets.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
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
            return Ok(snippet.clone());
        }
        let snippet = Snippet {
            id: Uuid::new_v4(),
            name,
            command: input.command.trim().to_string(),
            connection_id: input.connection_id,
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
