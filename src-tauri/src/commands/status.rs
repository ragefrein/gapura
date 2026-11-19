//! Reachability probes and the ssh-command quick-connect parser (F15).

use std::time::{Duration, Instant};

use futures_util::stream::{self, StreamExt};
use gapura_core::status::{self, HostProbe};
use gapura_core::GapuraError;
use serde::Serialize;
use uuid::Uuid;

use crate::error::CommandResult;
use crate::state::{AppState, CachedStatus};

/// Probe results older than this are refreshed by a non-forced request.
const CACHE_TTL: Duration = Duration::from_secs(60);
/// Never open more than this many TCP connections at once.
const MAX_PARALLEL_PROBES: usize = 8;
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionStatus {
    pub connection_id: Uuid,
    pub up: bool,
    pub latency_ms: Option<u32>,
}

/// Probe several connections at once.
///
/// `force = false` reuses results younger than [`CACHE_TTL`] so the UI can
/// poll politely; `force = true` always probes. Probes run with a bounded
/// concurrency so a large list cannot open hundreds of sockets at once.
#[tauri::command]
pub async fn check_connection_status(
    state: tauri::State<'_, AppState>,
    ids: Vec<Uuid>,
    force: Option<bool>,
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

    let force = force.unwrap_or(false);
    let now = Instant::now();
    let mut results: std::collections::HashMap<Uuid, ConnectionStatus> =
        std::collections::HashMap::new();

    // Serve fresh cache entries first.
    if let Ok(cache) = state.status_cache.lock() {
        for (id, _, _) in &targets {
            if force {
                continue;
            }
            if let Some(cached) = cache.get(id) {
                if now.duration_since(cached.checked_at) < CACHE_TTL {
                    results.insert(
                        *id,
                        ConnectionStatus {
                            connection_id: *id,
                            up: cached.up,
                            latency_ms: cached.latency_ms,
                        },
                    );
                }
            }
        }
    }

    let pending: Vec<(Uuid, String, u16)> = targets
        .into_iter()
        .filter(|(id, _, _)| !results.contains_key(id))
        .collect();

    let probed: Vec<ConnectionStatus> = stream::iter(pending)
        .map(|(id, host, port)| async move {
            let result = status::probe(&host, port, PROBE_TIMEOUT).await;
            let (up, latency_ms) = match result {
                HostProbe::Up { latency_ms } => (true, Some(latency_ms)),
                HostProbe::Down => (false, None),
            };
            ConnectionStatus {
                connection_id: id,
                up,
                latency_ms,
            }
        })
        .buffer_unordered(MAX_PARALLEL_PROBES)
        .collect()
        .await;

    if let Ok(mut cache) = state.status_cache.lock() {
        let checked_at = Instant::now();
        for result in &probed {
            cache.insert(
                result.connection_id,
                CachedStatus {
                    up: result.up,
                    latency_ms: result.latency_ms,
                    checked_at,
                },
            );
            results.insert(result.connection_id, result.clone());
        }
    }

    // Preserve the order the caller asked for.
    Ok(ids
        .into_iter()
        .filter_map(|id| results.remove(&id))
        .collect())
}

/// Pure passthrough so the UI can prefill the connection form from an ssh
/// command line without reimplementing the parser.
#[tauri::command]
pub fn parse_ssh_command_text(
    text: String,
) -> CommandResult<gapura_core::quick_connect::SshCommandDraft> {
    gapura_core::quick_connect::parse_ssh_command(&text).map_err(GapuraError::Other)
}
