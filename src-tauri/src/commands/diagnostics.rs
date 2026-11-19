//! Local diagnostics report. No telemetry: the report only exists when the
//! user asks for it, and the UI copies it to the clipboard so the user
//! decides whether to share it.

use std::fmt::Write as _;

use serde::Serialize;

use crate::error::CommandResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsReport {
    pub report: String,
}

/// Build a redacted, human-readable snapshot. Secrets are never touched:
/// only counts, modes, and connection identities (which the user already
/// sees in the UI) are included.
#[tauri::command]
pub fn diagnostics(state: tauri::State<'_, AppState>) -> CommandResult<DiagnosticsReport> {
    let mut out = String::new();
    let _ = writeln!(out, "Gapura diagnostics");
    let _ = writeln!(out, "generated: {}", chrono::Utc::now().to_rfc3339());
    let _ = writeln!(out, "version: {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "os: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(out, "data_dir: {}", state.data_dir.display());
    let _ = writeln!(out, "config_dir: {}", state.config_dir.display());
    let _ = writeln!(
        out,
        "log: {} (daily rolling; gapura.log*)",
        state.data_dir.join("gapura.log").display()
    );

    let _ = writeln!(out, "\nvault:");
    let exists = gapura_core::vault::vault_exists(&state.data_dir);
    let _ = writeln!(out, "  exists: {exists}");
    let unlocked = state.vault.read().map(|g| g.is_some()).unwrap_or(false);
    let _ = writeln!(out, "  unlocked: {unlocked}");
    let mode = state
        .vault_mode()
        .or_else(|| {
            gapura_core::vault::peek_mode(&state.data_dir)
                .ok()
                .flatten()
        })
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let _ = writeln!(out, "  mode: {mode}");

    if let Ok(vault) = state.vault.read() {
        if let Some(vault) = vault.as_ref() {
            let payload = &vault.payload;
            let pinned = payload.connections.iter().filter(|c| c.pinned).count();
            let _ = writeln!(
                out,
                "  connections: {} (pinned {pinned})",
                payload.connections.len()
            );
            let _ = writeln!(out, "  groups: {}", payload.groups.len());
            let _ = writeln!(out, "  keys: {}", payload.keys.len());
            let _ = writeln!(out, "  secrets: {}", payload.secrets.len());
            let _ = writeln!(out, "  snippets: {}", payload.snippets.len());
            let _ = writeln!(out, "  forward_rules: {}", payload.forward_rules.len());
        }
    }

    let known_hosts = state
        .known_hosts
        .lock()
        .map(|known| known.entries().len())
        .unwrap_or(0);
    let _ = writeln!(out, "  known_hosts: {known_hosts}");

    if let Ok(settings) = state.settings.read() {
        let _ = writeln!(out, "\nsettings:");
        let _ = writeln!(out, "  theme: {:?}", settings.theme);
        let _ = writeln!(
            out,
            "  terminal_font: {} {}pt",
            settings.terminal_font_family, settings.terminal_font_size
        );
        let _ = writeln!(out, "  scrollback_lines: {}", settings.scrollback_lines);
        let _ = writeln!(out, "  auto_lock_minutes: {}", settings.auto_lock_minutes);
        let _ = writeln!(out, "  connection_sort: {:?}", settings.connection_sort);
    }

    let sessions: Vec<(uuid::Uuid, uuid::Uuid)> = state
        .sessions
        .read()
        .map(|sessions| {
            sessions
                .iter()
                .map(|(id, entry)| (*id, entry.connection_id))
                .collect()
        })
        .unwrap_or_default();
    let _ = writeln!(out, "\nsessions: {}", sessions.len());
    for (session_id, connection_id) in &sessions {
        let identity = state
            .with_vault(|vault| {
                Ok(vault
                    .payload
                    .connection(*connection_id)
                    .map(|c| format!("{} {}@{}:{}", c.name, c.username, c.host, c.port)))
            })
            .ok()
            .flatten()
            .unwrap_or_else(|| "unknown connection".to_string());
        let _ = writeln!(out, "  - {session_id} {identity}");
    }

    let running = state
        .forwards
        .read()
        .map(|forwards| forwards.len())
        .unwrap_or(0);
    let _ = writeln!(out, "forwards_running: {running}");

    Ok(DiagnosticsReport { report: out })
}
