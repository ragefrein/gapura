//! Port-forward rules and their runtime (PRD F12).

use std::sync::Arc;

use chrono::Utc;
use gapura_core::model::{ForwardKind, ForwardRule};
use gapura_core::ssh::session::SessionCommand;
use gapura_core::GapuraError;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::error::CommandResult;
use crate::state::{AppState, ForwardRuntime, SessionEntry};

#[derive(Debug, Deserialize)]
pub struct ForwardRuleInput {
    pub id: Option<Uuid>,
    pub connection_id: Uuid,
    pub kind: ForwardKind,
    pub bind_host: String,
    pub bind_port: u16,
    pub target_host: String,
    pub target_port: u16,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub auto_start: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct ActiveForward {
    pub rule_id: Uuid,
    pub session_id: Uuid,
    pub kind: ForwardKind,
    pub bind_host: String,
    pub bind_port: u16,
    pub target_host: String,
    pub target_port: u16,
}

#[tauri::command]
pub fn list_forward_rules(
    state: tauri::State<'_, AppState>,
    connection_id: Option<Uuid>,
) -> CommandResult<Vec<ForwardRule>> {
    state.with_vault(|vault| {
        let mut rules: Vec<ForwardRule> = vault
            .payload
            .forward_rules
            .iter()
            .filter(|r| connection_id.is_none() || Some(r.connection_id) == connection_id)
            .cloned()
            .collect();
        rules.sort_by_key(|r| (r.bind_port, r.created_at));
        Ok(rules)
    })
}

#[tauri::command]
pub fn save_forward_rule(
    state: tauri::State<'_, AppState>,
    input: ForwardRuleInput,
) -> CommandResult<ForwardRule> {
    if input.bind_host.trim().is_empty() || input.target_host.trim().is_empty() {
        return Err(GapuraError::Other("hosts cannot be empty".into()));
    }
    if input.bind_port == 0 || input.target_port == 0 {
        return Err(GapuraError::Other("ports must be 1-65535".into()));
    }
    state.with_vault_mut(|vault| {
        if !vault.payload.connection(input.connection_id).is_some() {
            return Err(GapuraError::Other("connection not found".into()));
        }
        if let Some(id) = input.id {
            let rule = vault
                .payload
                .forward_rules
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or_else(|| GapuraError::Other("rule not found".into()))?;
            rule.kind = input.kind;
            rule.bind_host = input.bind_host.trim().to_string();
            rule.bind_port = input.bind_port;
            rule.target_host = input.target_host.trim().to_string();
            rule.target_port = input.target_port;
            rule.enabled = input.enabled;
            rule.auto_start = input.auto_start;
            return Ok(rule.clone());
        }
        let rule = ForwardRule {
            id: Uuid::new_v4(),
            connection_id: input.connection_id,
            kind: input.kind,
            bind_host: input.bind_host.trim().to_string(),
            bind_port: input.bind_port,
            target_host: input.target_host.trim().to_string(),
            target_port: input.target_port,
            enabled: input.enabled,
            auto_start: input.auto_start,
            created_at: Utc::now(),
        };
        vault.payload.forward_rules.push(rule.clone());
        Ok(rule)
    })
}

#[tauri::command]
pub fn delete_forward_rule(state: tauri::State<'_, AppState>, id: Uuid) -> CommandResult<()> {
    state.take_forward(&id); // stop it if running
    state.with_vault_mut(|vault| {
        let before = vault.payload.forward_rules.len();
        vault.payload.forward_rules.retain(|r| r.id != id);
        if vault.payload.forward_rules.len() == before {
            return Err(GapuraError::Other("rule not found".into()));
        }
        Ok(())
    })
}

#[tauri::command]
pub fn list_active_forwards(
    state: tauri::State<'_, AppState>,
) -> CommandResult<Vec<ActiveForward>> {
    let forwards = state
        .forwards
        .read()
        .map_err(|_| GapuraError::Other("forward lock poisoned".into()))?;
    Ok(forwards
        .iter()
        .map(|(rule_id, f)| ActiveForward {
            rule_id: *rule_id,
            session_id: f.session_id,
            kind: f.kind,
            bind_host: f.bind_host.clone(),
            bind_port: f.bind_port,
            target_host: f.target_host.clone(),
            target_port: f.target_port,
        })
        .collect())
}

/// Start a rule on a live session. Local rules bind a listener here; Remote
/// rules ask the server to listen. Errors (bad bind address, server refusal
/// via the status event) surface to the UI.
#[tauri::command]
pub async fn start_forward(
    state: tauri::State<'_, AppState>,
    session_id: Uuid,
    rule_id: Uuid,
) -> CommandResult<()> {
    start_forward_ref(&state, session_id, rule_id).await
}

async fn start_forward_ref(
    state: &tauri::State<'_, AppState>,
    session_id: Uuid,
    rule_id: Uuid,
) -> CommandResult<()> {
    let Some(entry) = session_entry(state, &session_id) else {
        return Err(GapuraError::Other("session not found".into()));
    };
    let (connection_id, rule) = state.with_vault(|vault| {
        let rule = vault
            .payload
            .forward_rules
            .iter()
            .find(|r| r.id == rule_id)
            .cloned()
            .ok_or_else(|| GapuraError::Other("rule not found".into()))?;
        Ok((rule.connection_id, rule))
    })?;
    if entry.connection_id != connection_id {
        return Err(GapuraError::Other(
            "this rule belongs to another connection".into(),
        ));
    }
    if !rule.enabled {
        return Err(GapuraError::Other("this rule is disabled".into()));
    }

    match rule.kind {
        ForwardKind::Local => {
            let bind = (rule.bind_host.clone(), rule.bind_port);
            let listener = tokio::net::TcpListener::bind(bind).await.map_err(|e| {
                GapuraError::Other(format!(
                    "cannot bind {}:{}: {e}",
                    rule.bind_host, rule.bind_port
                ))
            })?;
            let commands = entry.commands.clone();
            let target_host = rule.target_host.clone();
            let target_port = rule.target_port;
            let task = tokio::spawn(async move {
                while let Ok((stream, peer)) = listener.accept().await {
                    let send = commands.send(SessionCommand::ForwardChannel {
                        rule_id,
                        stream,
                        target_host: target_host.clone(),
                        target_port,
                        originator: peer.ip().to_string(),
                    });
                    if send.await.is_err() {
                        break; // session gone
                    }
                }
                tracing::info!(%rule_id, "local forward listener stopped");
            });
            state.put_forward(
                rule.id,
                ForwardRuntime {
                    session_id,
                    kind: rule.kind,
                    bind_host: rule.bind_host.clone(),
                    bind_port: rule.bind_port,
                    target_host: rule.target_host.clone(),
                    target_port: rule.target_port,
                    task,
                    commands: None,
                    registry_key: None,
                },
            );
            tracing::info!(%rule_id, bind = %rule.bind_host, port = rule.bind_port, "local forward started");
            Ok(())
        }
        ForwardKind::Remote => {
            // Register the target before asking the server to listen, so
            // forwarded channels find their destination immediately.
            entry.remote_forwards.lock().unwrap().insert(
                (rule.bind_host.clone(), rule.bind_port),
                (rule.target_host.clone(), rule.target_port),
            );
            let sent = entry
                .commands
                .clone()
                .send(SessionCommand::RemoteListen {
                    rule_id: rule.id,
                    bind_host: rule.bind_host.clone(),
                    bind_port: rule.bind_port,
                })
                .await;
            if sent.is_err() {
                entry
                    .remote_forwards
                    .lock()
                    .unwrap()
                    .remove(&(rule.bind_host.clone(), rule.bind_port));
                return Err(GapuraError::Other("session is gone".into()));
            }
            state.put_forward(
                rule.id,
                ForwardRuntime {
                    session_id,
                    kind: rule.kind,
                    bind_host: rule.bind_host.clone(),
                    bind_port: rule.bind_port,
                    target_host: rule.target_host.clone(),
                    target_port: rule.target_port,
                    task: tokio::spawn(async {}),
                    commands: Some(entry.commands.clone()),
                    registry_key: Some((rule.bind_host.clone(), rule.bind_port)),
                },
            );
            Ok(())
        }
    }
}

#[tauri::command]
pub fn stop_forward(state: tauri::State<'_, AppState>, rule_id: Uuid) -> CommandResult<()> {
    let mut runtime = state
        .take_forward(&rule_id)
        .ok_or_else(|| GapuraError::Other("this forward is not running".into()))?;
    crate::state::stop_runtime(&mut runtime);
    Ok(())
}

fn session_entry(state: &tauri::State<'_, AppState>, session_id: &Uuid) -> Option<SessionEntry> {
    state
        .sessions
        .read()
        .ok()?
        .get(session_id)
        .map(|e| SessionEntry {
            connection_id: e.connection_id,
            commands: e.commands.clone(),
            remote_forwards: Arc::clone(&e.remote_forwards),
        })
}

/// Start every enabled auto-start rule for a session (called on open and
/// reconnect). Failures are logged; the UI learns via forward:status.
pub async fn autostart_forwards(
    state: &tauri::State<'_, AppState>,
    session_id: Uuid,
    connection_id: Uuid,
) {
    let rules = state
        .with_vault(|vault| {
            Ok(vault
                .payload
                .forward_rules
                .iter()
                .filter(|r| r.connection_id == connection_id && r.enabled && r.auto_start)
                .cloned()
                .collect::<Vec<_>>())
        })
        .unwrap_or_default();
    for rule in rules {
        if let Err(e) = start_forward_ref(state, session_id, rule.id).await {
            tracing::warn!(rule = %rule.id, "autostart failed: {e}");
        }
    }
}
