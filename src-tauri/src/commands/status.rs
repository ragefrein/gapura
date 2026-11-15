//! Reachability probes and the ssh-command quick-connect parser (F15).

use gapura_core::status::{self, HostProbe};
use gapura_core::GapuraError;
use serde::Serialize;
use uuid::Uuid;

use crate::error::CommandResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionStatus {
    pub connection_id: Uuid,
    pub up: bool,
    pub latency_ms: Option<u32>,
}

/// Probe several connections at once. Each probe gets its own short timeout;
/// failures are honest "down" results, not errors.
#[tauri::command]
pub async fn check_connection_status(
    state: tauri::State<'_, AppState>,
    ids: Vec<Uuid>,
) -> CommandResult<Vec<ConnectionStatus>> {
    let targets: Vec<(Uuid, String, u16)> = state.with_vault(|vault| {
        Ok(vault
            .payload
            .connections
            .iter()
            .filter(|c| ids.contains(&c.id))
            .map(|c| (c.id, c.host.clone(), c.port))
            .collect())
    })?;

    let probes = targets.into_iter().map(|(id, host, port)| async move {
        let result = status::probe(&host, port, std::time::Duration::from_secs(3)).await;
        ConnectionStatus {
            connection_id: id,
            up: matches!(result, HostProbe::Up { .. }),
            latency_ms: match result {
                HostProbe::Up { latency_ms } => Some(latency_ms),
                HostProbe::Down => None,
            },
        }
    });

    Ok(futures_util::future::join_all(probes).await)
}

/// Pure passthrough so the UI can prefill the connection form from an ssh
/// command line without reimplementing the parser.
#[tauri::command]
pub fn parse_ssh_command_text(text: String) -> CommandResult<gapura_core::quick_connect::SshCommandDraft> {
    gapura_core::quick_connect::parse_ssh_command(&text)
        .map_err(|e| GapuraError::Other(e))
}
