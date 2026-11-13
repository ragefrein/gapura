//! The single error type shared by everything in Gapura.
//!
//! Two rules that the whole codebase relies on:
//!
//! 1. Error variants must never carry secret material (passwords, passphrases,
//!    key bytes). When an error needs context, pass identifiers or hostnames,
//!    never secret values. This keeps [`std::fmt::Debug`] safe to print.
//! 2. Messages are written for humans; they end up in the UI as-is.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum GapuraError {
    // ---- vault ----
    #[error("no vault exists yet")]
    VaultNotFound,

    #[error("the vault is locked")]
    VaultLocked,

    #[error("that passphrase does not match the vault")]
    WrongPassphrase,

    #[error("the vault file is damaged or unreadable: {0}")]
    VaultCorrupt(String),

    #[error("this vault was written by a newer or unknown format version ({0})")]
    VaultVersion(u8),

    #[error("the OS keychain rejected the operation: {0}")]
    Keychain(String),

    #[error("crypto failure: {0}")]
    Crypto(String),

    // ---- ssh ----
    #[error("ssh error: {0}")]
    Ssh(String),

    #[error("authentication failed for {username}@{host}: {detail}")]
    AuthFailed {
        username: String,
        host: String,
        detail: String,
    },

    #[error(
        "the server fingerprint for {host}:{port} changed since the last visit \
         (expected {stored}, got {actual})"
    )]
    HostKeyMismatch {
        host: String,
        port: u16,
        stored: String,
        actual: String,
    },

    #[error("cannot reach {host}:{port}: {detail}")]
    ConnectFailed {
        host: String,
        port: u16,
        detail: String,
    },

    // ---- generic ----
    #[error("{0}")]
    Other(String),

    #[error("{path}: {message}")]
    PathIo { path: PathBuf, message: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

impl From<russh::keys::Error> for GapuraError {
    fn from(error: russh::keys::Error) -> Self {
        GapuraError::Ssh(error.to_string())
    }
}

impl GapuraError {
    /// Stable machine-readable code, used by the IPC layer to let the UI
    /// branch on error kinds without parsing messages.
    pub fn code(&self) -> &'static str {
        match self {
            GapuraError::VaultNotFound => "vault.not_found",
            GapuraError::VaultLocked => "vault.locked",
            GapuraError::WrongPassphrase => "vault.wrong_passphrase",
            GapuraError::VaultCorrupt(_) => "vault.corrupt",
            GapuraError::VaultVersion(_) => "vault.version",
            GapuraError::Keychain(_) => "vault.keychain",
            GapuraError::Crypto(_) => "vault.crypto",
            GapuraError::Ssh(_) => "ssh.error",
            GapuraError::AuthFailed { .. } => "ssh.auth_failed",
            GapuraError::HostKeyMismatch { .. } => "ssh.host_key_mismatch",
            GapuraError::ConnectFailed { .. } => "ssh.connect_failed",
            GapuraError::Other(_) => "general.error",
            GapuraError::PathIo { .. } => "general.path_io",
            GapuraError::Io(_) => "general.io",
            GapuraError::Serde(_) => "general.serde",
        }
    }
}

pub type Result<T, E = GapuraError> = std::result::Result<T, E>;

// ---- IPC serialization ----------------------------------------------------
//
// Errors cross the webview boundary as `{code, message, ...context}`. The
// impl lives here because the orphan rule requires it in the owning crate.
// Host-key mismatches carry their fingerprints so the UI can render the
// TOFU dialog without a second round-trip.

impl GapuraError {
    /// Host context attached to ssh dial errors, if any.
    fn host_context(&self) -> Option<(&String, u16)> {
        match self {
            GapuraError::HostKeyMismatch { host, port, .. } => Some((host, *port)),
            GapuraError::ConnectFailed { host, port, .. } => Some((host, *port)),
            _ => None,
        }
    }
}

impl serde::Serialize for GapuraError {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("GapuraError", 2)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", &self.to_string())?;
        if let Some((host, port)) = self.host_context() {
            state.serialize_field("host", host)?;
            state.serialize_field("port", &port)?;
        }
        if let GapuraError::HostKeyMismatch { stored, actual, .. } = self {
            state.serialize_field("stored_fingerprint", stored)?;
            state.serialize_field("actual_fingerprint", actual)?;
        }
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for GapuraError {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Shape {
            code: String,
            message: String,
            #[serde(default)]
            host: Option<String>,
            #[serde(default)]
            port: Option<u16>,
            #[serde(default)]
            stored_fingerprint: Option<String>,
            #[serde(default)]
            actual_fingerprint: Option<String>,
        }
        let shape = Shape::deserialize(deserializer)?;
        Ok(match shape.code.as_str() {
            "vault.not_found" => GapuraError::VaultNotFound,
            "vault.locked" => GapuraError::VaultLocked,
            "vault.wrong_passphrase" => GapuraError::WrongPassphrase,
            "vault.keychain" => GapuraError::Keychain(shape.message),
            "ssh.host_key_mismatch" => GapuraError::HostKeyMismatch {
                host: shape.host.unwrap_or_default(),
                port: shape.port.unwrap_or(22),
                stored: shape.stored_fingerprint.unwrap_or_default(),
                actual: shape.actual_fingerprint.unwrap_or_default(),
            },
            _ => GapuraError::Other(shape.message),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Debug must never leak anything that looks like a secret. The variants
    /// that take context strings are constructed with identifiers only, and
    /// this test pins that contract for the obvious cases.
    #[test]
    fn debug_output_contains_no_secret_markers() {
        let password_marker = "s3cret-password";
        let errors = vec![
            GapuraError::VaultNotFound,
            GapuraError::WrongPassphrase,
            GapuraError::Keychain("denied".into()),
            GapuraError::AuthFailed {
                username: "ops".into(),
                host: "db1.example.com".into(),
                detail: "publickey".into(),
            },
            GapuraError::HostKeyMismatch {
                host: "db1.example.com".into(),
                port: 22,
                stored: "SHA256:abc".into(),
                actual: "SHA256:def".into(),
            },
        ];
        for e in errors {
            let rendered = format!("{:?}", e);
            assert!(!rendered.contains(password_marker));
        }
    }

    #[test]
    fn codes_are_stable() {
        assert_eq!(GapuraError::VaultLocked.code(), "vault.locked");
        assert_eq!(
            GapuraError::HostKeyMismatch {
                host: "h".into(),
                port: 22,
                stored: "s".into(),
                actual: "a".into(),
            }
            .code(),
            "ssh.host_key_mismatch"
        );
    }
}
