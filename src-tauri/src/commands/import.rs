//! Import from `~/.ssh/config` (PRD F7): parse, review, commit.

use std::path::PathBuf;

use gapura_core::import_ssh_config::{
    candidates_to_connections, mark_duplicates, parse_file, ImportCandidate,
};
use gapura_core::model::Connection;
use gapura_core::GapuraError;
use serde::Deserialize;

use crate::error::CommandResult;
use crate::state::AppState;

#[tauri::command]
pub fn import_ssh_config(
    state: tauri::State<'_, AppState>,
    path: Option<String>,
) -> CommandResult<Vec<ImportCandidate>> {
    let source = match path {
        Some(p) => PathBuf::from(p),
        None => home_dir().join(".ssh").join("config"),
    };
    let candidates = parse_file(&source, &home_dir())
        .map_err(|e| GapuraError::Other(format!("cannot read {}: {e}", source.display())))?;
    let existing = state.with_vault(|vault| Ok(vault.payload.connections.clone()))?;
    Ok(mark_duplicates(candidates, &existing))
}

#[derive(Debug, Deserialize)]
pub struct CommitImportInput {
    /// Full candidates the user approved in the review UI.
    pub candidates: Vec<ImportCandidate>,
}

#[tauri::command]
pub fn commit_import(
    state: tauri::State<'_, AppState>,
    input: CommitImportInput,
) -> CommandResult<usize> {
    let connections = candidates_to_connections(&input.candidates);
    let count = connections.len();
    state.with_vault_mut(|vault| {
        vault.payload.connections.extend(connections);
        Ok(())
    })?;
    tracing::info!(count, "committed imported connections");
    Ok(count)
}

/// Preview connections created from candidates (used for dedupe feedback).
#[tauri::command]
pub fn preview_import(input: CommitImportInput) -> CommandResult<Vec<Connection>> {
    Ok(candidates_to_connections(&input.candidates))
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}
