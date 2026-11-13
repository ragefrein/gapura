//! Domain model: the JSON shape stored inside the encrypted vault payload.
//!
//! Everything here is `serde`-serializable because the payload is persisted
//! as JSON inside `vault.json.enc`. Secret values live in [`SecretSlot`] as a
//! [`SecretString`]; the whole payload is encrypted at the file level, so the
//! inner representation only needs to stay out of logs.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroize;

/// Bumped on every breaking change to [`VaultPayload`]. See `vault::migrate`.
pub const SCHEMA_VERSION: u32 = 1;

/// A string secret (password, key passphrase) that lives inside the vault
/// payload. Serializing is allowed because the payload only ever exists
/// decrypted in memory or inside the encrypted file — but `Debug` stays
/// redacted and the string is zeroized when the value is dropped.
///
/// This type exists because the `secrecy` crate deliberately refuses to
/// serialize; the vault is the one sanctioned place where secrets must be
/// persisted.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SecretValue {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for SecretValue {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl Serialize for SecretValue {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SecretValue {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        String::deserialize(deserializer).map(SecretValue)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

/// How a session authenticates. The user picks one explicitly; there is no
/// silent fallback order (design doc, section 3.2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum AuthMethod {
    /// Defer to the running SSH agent (`SSH_AUTH_SOCK`).
    Agent,
    /// Password stored as a secret slot inside the vault.
    Password { secret_id: Uuid },
    /// Private key referenced by file path. v0.1 never copies private key
    /// material into the vault (design doc, section 2.2).
    KeyFile {
        path: PathBuf,
        key_id: Option<Uuid>,
        passphrase_secret_id: Option<Uuid>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: Uuid,
    pub name: String,
    pub group_id: Option<Uuid>,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub color: Option<String>,
    /// Reserved for v0.2 jump host chaining (PRD F13). Persisted now so the
    /// schema does not need a migration the moment v0.2 lands.
    #[serde(default)]
    pub jump_host_id: Option<Uuid>,
    #[serde(default)]
    pub notes: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Connection {
    /// Convenience label used by tabs and the connection list.
    pub fn display_target(&self) -> String {
        format!("{}@{}:{}", self.username, self.host, self.port)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SecretKind {
    Password,
    KeyPassphrase,
}

/// One secret value. `value` is a [`SecretString`]: it zeroizes on drop and
/// its `Debug` impl prints `[REDACTED]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretSlot {
    pub id: Uuid,
    pub kind: SecretKind,
    pub value: SecretValue,
    pub created_at: DateTime<Utc>,
}

/// A registered private key. v0.1 stores the key at its original path; the
/// vault only remembers where it is and what its fingerprint is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshKey {
    pub id: Uuid,
    pub name: String,
    pub path: PathBuf,
    /// Full OpenSSH public key line, e.g. `ssh-ed25519 AAAA... gapura@laptop`.
    pub public_key: String,
    /// `SHA256:...` fingerprint, OpenSSH style.
    pub fingerprint: String,
    /// True when a passphrase slot for this key exists in the vault.
    pub has_passphrase: bool,
    pub created_at: DateTime<Utc>,
}

/// The decrypted content of the vault file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VaultPayload {
    pub schema_version: u32,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub secrets: Vec<SecretSlot>,
    #[serde(default)]
    pub keys: Vec<SshKey>,
}

impl VaultPayload {
    pub fn empty() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            groups: Vec::new(),
            connections: Vec::new(),
            secrets: Vec::new(),
            keys: Vec::new(),
        }
    }

    pub fn connection(&self, id: Uuid) -> Option<&Connection> {
        self.connections.iter().find(|c| c.id == id)
    }

    pub fn secret(&self, id: Uuid) -> Option<&SecretSlot> {
        self.secrets.iter().find(|s| s.id == id)
    }

    /// Drop an orphaned secret slot that no connection references anymore.
    /// Returns true when something was removed.
    pub fn remove_secret_if_unused(&mut self, id: Uuid) -> bool {
        let referenced = self.connections.iter().any(|c| match &c.auth {
            AuthMethod::Password { secret_id } => *secret_id == id,
            AuthMethod::KeyFile {
                passphrase_secret_id,
                ..
            } => *passphrase_secret_id == Some(id),
            AuthMethod::Agent => false,
        });
        if referenced {
            return false;
        }
        let before = self.secrets.len();
        self.secrets.retain(|s| s.id != id);
        self.secrets.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn payload_json_roundtrip_keeps_secrets() {
        let secret_id = Uuid::new_v4();
        let payload = VaultPayload {
            schema_version: SCHEMA_VERSION,
            groups: vec![Group {
                id: Uuid::new_v4(),
                name: "production".into(),
                created_at: now(),
            }],
            connections: vec![Connection {
                id: Uuid::new_v4(),
                name: "db1".into(),
                group_id: None,
                host: "db1.example.com".into(),
                port: 2222,
                username: "ops".into(),
                auth: AuthMethod::Password { secret_id },
                tags: vec!["prod".into()],
                color: Some("#14b8a6".into()),
                jump_host_id: None,
                notes: "primary replica".into(),
                created_at: now(),
                updated_at: now(),
            }],
            secrets: vec![SecretSlot {
                id: secret_id,
                kind: SecretKind::Password,
                value: SecretValue::from("hunter2"),
                created_at: now(),
            }],
            keys: vec![],
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(
            json.contains("hunter2"),
            "secret must be persisted inside the encrypted payload"
        );
        let back: VaultPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(back.connections.len(), 1);
        assert_eq!(back.secret(secret_id).unwrap().value.expose(), "hunter2");
    }

    #[test]
    fn secret_debug_is_redacted() {
        let slot = SecretSlot {
            id: Uuid::new_v4(),
            kind: SecretKind::Password,
            value: SecretValue::from("topsecret"),
            created_at: now(),
        };
        let rendered = format!("{:?}", slot);
        assert!(!rendered.contains("topsecret"));
        assert!(rendered.contains("REDACTED"));
    }

    #[test]
    fn auth_method_serializes_with_tags() {
        let auth = AuthMethod::KeyFile {
            path: PathBuf::from("/home/user/.ssh/id_ed25519"),
            key_id: None,
            passphrase_secret_id: None,
        };
        let json = serde_json::to_string(&auth).unwrap();
        assert!(json.contains(r#""kind":"key_file""#));
        let back: AuthMethod = serde_json::from_str(&json).unwrap();
        assert_eq!(back, auth);
    }

    #[test]
    fn remove_secret_if_unused_behaves() {
        let mut payload = VaultPayload::empty();
        let secret_id = Uuid::new_v4();
        payload.secrets.push(SecretSlot {
            id: secret_id,
            kind: SecretKind::Password,
            value: SecretValue::from("x"),
            created_at: now(),
        });
        payload.connections.push(Connection {
            id: Uuid::new_v4(),
            name: "a".into(),
            group_id: None,
            host: "h".into(),
            port: 22,
            username: "u".into(),
            auth: AuthMethod::Password { secret_id },
            tags: vec![],
            color: None,
            jump_host_id: None,
            notes: String::new(),
            created_at: now(),
            updated_at: now(),
        });
        assert!(!payload.remove_secret_if_unused(secret_id));
        payload.connections.clear();
        assert!(payload.remove_secret_if_unused(secret_id));
        assert!(payload.secret(secret_id).is_none());
    }
}
