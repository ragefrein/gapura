//! Parser for OpenSSH client configuration files (PRD F7).
//!
//! Scope is deliberate: we import what a connection manager needs —
//! `Host`, `HostName`, `User`, `Port`, `IdentityFile`, `ProxyJump` — and
//! anything we do not understand is recorded as a warning instead of being
//! guessed at. The import is one-way and the UI shows a review list before
//! anything is saved.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use uuid::Uuid;

use crate::error::{GapuraError, Result};
use crate::model::{AuthMethod, Connection};

/// Fields we understand. Everything else in a matched block becomes a note.
#[derive(Debug, Clone, PartialEq)]
struct ParsedBlock {
    alias: String,
    host_name: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<String>,
    proxy_jump: Option<String>,
    unknown_keys: Vec<String>,
    is_pattern: bool,
}

/// One proposed connection produced by [`parse_ssh_config`]. Serialized so
/// the review UI can send accepted candidates back verbatim.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportCandidate {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
    pub notes: String,
    /// Set when an entry with the same host/port/user already exists in the
    /// vault; the review UI pre-deselects these.
    pub duplicate_of: Option<Uuid>,
    /// Non-fatal things the user should see before importing.
    pub warnings: Vec<String>,
}

/// Expand `~` and `$HOME` at the front of a path.
fn expand_home(path: &str, home: &Path) -> PathBuf {
    if path == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home.join(rest);
    }
    if let Some(rest) = path.strip_prefix("$HOME/") {
        return home.join(rest);
    }
    PathBuf::from(path)
}

/// Tokenize one line: strip comments and quotes, split on whitespace.
/// Returns `None` for blank lines and comments.
fn tokenize(line: &str) -> Option<Vec<String>> {
    let no_comment = line.split('#').next().unwrap_or("").trim();
    if no_comment.is_empty() {
        return None;
    }
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in no_comment.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    if tokens.is_empty() {
        None
    } else {
        Some(tokens)
    }
}

fn parse_port(value: &str) -> Option<u16> {
    value.parse::<u16>().ok().filter(|p| *p > 0)
}

fn is_pattern(alias: &str) -> bool {
    alias.contains('*') || alias.contains('?') || alias == "!"
}

/// Parse the content of an ssh config file into candidate connections.
///
/// Only literal `Host` blocks become candidates; wildcard patterns and
/// `Match` blocks are skipped (their options are conditional and copying
/// them onto a single connection would be wrong).
pub fn parse_ssh_config(content: &str, home: &Path) -> Vec<ImportCandidate> {
    let mut blocks: Vec<ParsedBlock> = Vec::new();
    let mut current: Option<ParsedBlock> = None;
    let mut in_match = false;

    for line in content.lines() {
        let Some(tokens) = tokenize(line) else {
            continue;
        };
        let keyword = tokens[0].to_ascii_lowercase();
        match keyword.as_str() {
            "host" => {
                if let Some(block) = current.take() {
                    blocks.push(block);
                }
                in_match = false;
                // ssh config allows several patterns after one Host keyword;
                // we only import when the block has exactly one literal name.
                let patterns = &tokens[1..];
                if patterns.len() == 1 && !is_pattern(&patterns[0]) {
                    current = Some(ParsedBlock {
                        alias: patterns[0].clone(),
                        host_name: None,
                        user: None,
                        port: None,
                        identity_file: None,
                        proxy_jump: None,
                        unknown_keys: Vec::new(),
                        is_pattern: false,
                    });
                } else {
                    current = Some(ParsedBlock {
                        alias: patterns.first().cloned().unwrap_or_default(),
                        host_name: None,
                        user: None,
                        port: None,
                        identity_file: None,
                        proxy_jump: None,
                        unknown_keys: Vec::new(),
                        is_pattern: true,
                    });
                }
            }
            "match" => {
                if let Some(block) = current.take() {
                    blocks.push(block);
                }
                in_match = true;
                current = None;
            }
            _ if in_match || current.is_none() => {}
            _ => {
                let block = current.as_mut().unwrap();
                let value = tokens.get(1).cloned();
                match keyword.as_str() {
                    "hostname" => block.host_name = value,
                    "user" => block.user = value,
                    "port" => block.port = value.and_then(|v| parse_port(&v)),
                    "identityfile" => {
                        if block.identity_file.is_none() {
                            block.identity_file = value;
                        }
                    }
                    "proxyjump" => block.proxy_jump = value,
                    _ => {
                        if !block.unknown_keys.contains(&keyword) {
                            block.unknown_keys.push(keyword);
                        }
                    }
                }
            }
        }
    }
    if let Some(block) = current.take() {
        blocks.push(block);
    }

    blocks
        .into_iter()
        .filter(|b| !b.is_pattern)
        .map(|b| {
            let mut warnings = Vec::new();
            for key in &b.unknown_keys {
                warnings.push(format!("ignored ssh config option: {key}"));
            }
            if let Some(jump) = &b.proxy_jump {
                warnings.push(format!("ProxyJump {jump} is not applied until v0.2"));
            }
            let had_host_name = b.host_name.is_some();
            let host = b.host_name.clone().unwrap_or_else(|| b.alias.clone());
            if !had_host_name {
                warnings.push("no HostName, using the alias as host".into());
            }
            let identity = b
                .identity_file
                .map(|p| expand_home(&p, home))
                .unwrap_or_else(|| home.join(".ssh").join("id_ed25519"));
            let notes = if warnings.is_empty() {
                String::new()
            } else {
                warnings.join("; ")
            };
            ImportCandidate {
                name: b.alias,
                host,
                port: b.port.unwrap_or(22),
                username: b.user.unwrap_or_else(whoami_fallback),
                auth: AuthMethod::KeyFile {
                    path: identity,
                    key_id: None,
                    passphrase_secret_id: None,
                },
                notes,
                duplicate_of: None,
                warnings,
            }
        })
        .collect()
}

/// ssh config defaults the user to the local login name; `USER` is the least
/// magical way to get it when not on an interactive session.
fn whoami_fallback() -> String {
    std::env::var("USER").unwrap_or_else(|_| "root".to_string())
}

/// Mark candidates that collide with connections already in the vault
/// (same host, port, and username). Returns the candidates with
/// `duplicate_of` filled in, preserving order.
pub fn mark_duplicates(
    mut candidates: Vec<ImportCandidate>,
    existing: &[Connection],
) -> Vec<ImportCandidate> {
    let mut index: HashMap<(String, u16, String), Uuid> = HashMap::new();
    for conn in existing {
        index.insert(
            (conn.host.clone(), conn.port, conn.username.clone()),
            conn.id,
        );
    }
    for c in &mut candidates {
        c.duplicate_of = index
            .get(&(c.host.clone(), c.port, c.username.clone()))
            .copied();
        if c.duplicate_of.is_some() {
            c.warnings
                .push("duplicate of an existing connection".into());
            c.notes = if c.notes.is_empty() {
                "duplicate of an existing connection".into()
            } else {
                format!("{}; duplicate of an existing connection", c.notes)
            };
        }
    }
    candidates
}

/// Parse a file from disk.
pub fn parse_file(path: &Path, home: &Path) -> Result<Vec<ImportCandidate>> {
    let content = std::fs::read_to_string(path).map_err(|e| GapuraError::PathIo {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    Ok(parse_ssh_config(&content, home))
}

/// Turn accepted candidates into vault connections (one-way commit).
pub fn candidates_to_connections(candidates: &[ImportCandidate]) -> Vec<Connection> {
    let now = Utc::now();
    candidates
        .iter()
        .map(|c| Connection {
            id: Uuid::new_v4(),
            name: c.name.clone(),
            group_id: None,
            host: c.host.clone(),
            port: c.port,
            username: c.username.clone(),
            auth: c.auth.clone(),
            tags: vec![],
            color: None,
            jump_host_id: None,
            notes: c.notes.clone(),
            created_at: now,
            updated_at: now,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/home/testuser")
    }

    const SAMPLE: &str = r#"
# global comment
Host web1
    HostName web1.example.com
    User deploy
    Port 2200
    IdentityFile ~/.ssh/deploy_key

Host *
    ServerAliveInterval 60

Host db1
    HostName 10.0.0.5
    User ops
    ProxyJump bastion

Host bastion
    HostName 203.0.113.9

Host alias-only
    User someone

Host "two names" other
    HostName multi.example.com

Match host *.internal
    User auto

Host tricky
    HostName quoted.example.com # inline comment
    User "quoted user"
"#;

    #[test]
    fn parses_literal_blocks_and_skips_patterns() {
        let candidates = parse_ssh_config(SAMPLE, &home());
        let names: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["web1", "db1", "bastion", "alias-only", "tricky"]
        );

        let web1 = &candidates[0];
        assert_eq!(web1.host, "web1.example.com");
        assert_eq!(web1.port, 2200);
        assert_eq!(web1.username, "deploy");
        match &web1.auth {
            AuthMethod::KeyFile { path, .. } => {
                assert_eq!(path, &PathBuf::from("/home/testuser/.ssh/deploy_key"))
            }
            other => panic!("expected key file auth, got {other:?}"),
        }

        // db1 has no port and no identity file: defaults apply, ProxyJump warns
        let db1 = &candidates[1];
        assert_eq!(db1.host, "10.0.0.5");
        assert_eq!(db1.port, 22);
        assert!(db1.warnings.iter().any(|w| w.contains("ProxyJump bastion")));

        let bastion = &candidates[2];
        assert_eq!(bastion.host, "203.0.113.9");

        // alias-only has no HostName: falls back to the alias with a warning
        let alias_only = &candidates[3];
        assert_eq!(alias_only.host, "alias-only");
        assert!(alias_only
            .warnings
            .iter()
            .any(|w| w.contains("alias as host")));

        // tricky: quotes and inline comments are handled
        let tricky = &candidates[4];
        assert_eq!(tricky.host, "quoted.example.com");
        assert_eq!(tricky.username, "quoted user");
    }

    #[test]
    fn empty_and_comment_only_files_yield_nothing() {
        assert!(parse_ssh_config("# nothing here\n\n   \n", &home()).is_empty());
        assert!(parse_ssh_config("", &home()).is_empty());
    }

    #[test]
    fn duplicates_are_marked_against_existing_connections() {
        let mut existing = Vec::new();
        let mut cand = candidates_to_connections(&parse_ssh_config(SAMPLE, &home()));
        existing.push(cand.remove(0)); // web1 already exists

        let candidates = mark_duplicates(parse_ssh_config(SAMPLE, &home()), &existing);
        assert!(candidates[0].duplicate_of.is_some());
        assert!(candidates[1].duplicate_of.is_none());
        assert!(candidates[0]
            .warnings
            .iter()
            .any(|w| w.contains("duplicate")));
    }

    #[test]
    fn candidates_convert_to_connections() {
        let candidates = parse_ssh_config(SAMPLE, &home());
        let conns = candidates_to_connections(&candidates);
        assert_eq!(conns.len(), candidates.len());
        assert_eq!(conns[0].name, "web1");
        assert_eq!(conns[0].display_target(), "deploy@web1.example.com:2200");
    }

    #[test]
    fn malformed_ports_fall_back_to_22() {
        let content = "Host bad\n  Port 99999\n  HostName h\n";
        let c = parse_ssh_config(content, &home());
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].port, 22);
    }
}
