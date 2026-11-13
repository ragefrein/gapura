//! Known-hosts store (PRD F5): the TOFU ("trust on first use") registry.
//!
//! Like OpenSSH's `known_hosts`, fingerprints are stored unencrypted — a
//! server public key is public data. Unlike OpenSSH we hard-fail on change
//! and let the user decide through the UI; there is no "accept all" option.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{GapuraError, Result};

pub const KNOWN_HOSTS_FILE_NAME: &str = "known_hosts.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnownHostEntry {
    /// `host:port`, always with an explicit port so lookups stay simple.
    pub host_port: String,
    /// `SHA256:...` fingerprint of the server host key.
    pub fingerprint: String,
    /// Key algorithm name as reported by the handshake (e.g. `ssh-ed25519`).
    pub algorithm: String,
    pub added_at: DateTime<Utc>,
}

impl KnownHostEntry {
    pub fn make(host: &str, port: u16, fingerprint: String, algorithm: String) -> Self {
        Self {
            host_port: format!("{host}:{port}"),
            fingerprint,
            algorithm,
            added_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum HostCheck {
    /// First time we see this host; the fingerprint should be recorded.
    FirstUse,
    /// Fingerprint matches what we stored.
    Match,
    /// The fingerprint changed. Contains what we had stored.
    Mismatch { stored: String },
}

pub struct KnownHosts {
    path: PathBuf,
    entries: Vec<KnownHostEntry>,
}

impl KnownHosts {
    pub fn load(data_dir: &Path) -> Result<Self> {
        let path = data_dir.join(KNOWN_HOSTS_FILE_NAME);
        let entries = if path.is_file() {
            let text = fs::read_to_string(&path).map_err(|e| GapuraError::PathIo {
                path: path.clone(),
                message: e.to_string(),
            })?;
            serde_json::from_str(&text)?
        } else {
            Vec::new()
        };
        Ok(Self { path, entries })
    }

    pub fn entries(&self) -> &[KnownHostEntry] {
        &self.entries
    }

    /// TOFU decision for a host we are about to connect to.
    pub fn check(&self, host: &str, port: u16, fingerprint: &str) -> HostCheck {
        let host_port = format!("{host}:{port}");
        match self.entries.iter().find(|e| e.host_port == host_port) {
            None => HostCheck::FirstUse,
            Some(e) if e.fingerprint == fingerprint => HostCheck::Match,
            Some(e) => HostCheck::Mismatch {
                stored: e.fingerprint.clone(),
            },
        }
    }

    /// Record a fingerprint (first use, or an explicit user-approved update).
    /// Returns true when this replaced an existing entry.
    pub fn record(
        &mut self,
        host: &str,
        port: u16,
        fingerprint: String,
        algorithm: String,
    ) -> Result<bool> {
        let host_port = format!("{host}:{port}");
        let replaced = self.entries.iter().any(|e| e.host_port == host_port);
        self.entries.retain(|e| e.host_port != host_port);
        self.entries
            .push(KnownHostEntry::make(host, port, fingerprint, algorithm));
        self.save()?;
        Ok(replaced)
    }

    /// Remove an entry (Settings > manage known hosts).
    pub fn remove(&mut self, host_port: &str) -> Result<bool> {
        let before = self.entries.len();
        self.entries.retain(|e| e.host_port != host_port);
        let changed = self.entries.len() != before;
        if changed {
            self.save()?;
        }
        Ok(changed)
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| GapuraError::PathIo {
                path: parent.to_path_buf(),
                message: e.to_string(),
            })?;
        }
        let text = serde_json::to_string_pretty(&self.entries)?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, text).map_err(|e| GapuraError::PathIo {
            path: tmp.clone(),
            message: e.to_string(),
        })?;
        fs::rename(&tmp, &self.path).map_err(|e| GapuraError::PathIo {
            path: self.path.clone(),
            message: e.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, KnownHosts) {
        let tmp = tempfile::tempdir().unwrap();
        let hosts = KnownHosts::load(tmp.path()).unwrap();
        (tmp, hosts)
    }

    const FP_A: &str = "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    const FP_B: &str = "SHA256:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=";

    #[test]
    fn tofu_lifecycle() {
        let (_tmp, mut hosts) = store();
        // first use
        assert!(matches!(hosts.check("db1", 22, FP_A), HostCheck::FirstUse));
        hosts
            .record("db1", 22, FP_A.into(), "ssh-ed25519".into())
            .unwrap();
        // match
        assert!(matches!(hosts.check("db1", 22, FP_A), HostCheck::Match));
        // change
        match hosts.check("db1", 22, FP_B) {
            HostCheck::Mismatch { stored } => assert_eq!(stored, FP_A),
            other => panic!("expected mismatch, got {other:?}"),
        }
        // explicit user-approved update
        let replaced = hosts
            .record("db1", 22, FP_B.into(), "ssh-ed25519".into())
            .unwrap();
        assert!(replaced);
        assert!(matches!(hosts.check("db1", 22, FP_B), HostCheck::Match));
    }

    #[test]
    fn ports_are_part_of_the_identity() {
        let (_tmp, mut hosts) = store();
        hosts
            .record("db1", 22, FP_A.into(), "ssh-ed25519".into())
            .unwrap();
        assert!(matches!(
            hosts.check("db1", 2222, FP_A),
            HostCheck::FirstUse
        ));
        assert!(matches!(hosts.check("db1", 22, FP_A), HostCheck::Match));
    }

    #[test]
    fn entries_persist_to_disk() {
        let (tmp, mut hosts) = store();
        hosts
            .record("web1", 22, FP_A.into(), "ssh-ed25519".into())
            .unwrap();
        let reloaded = KnownHosts::load(tmp.path()).unwrap();
        assert_eq!(reloaded.entries().len(), 1);
        assert_eq!(reloaded.entries()[0].host_port, "web1:22");
    }

    #[test]
    fn remove_entries() {
        let (_tmp, mut hosts) = store();
        hosts
            .record("web1", 22, FP_A.into(), "ssh-ed25519".into())
            .unwrap();
        assert!(hosts.remove("web1:22").unwrap());
        assert!(!hosts.remove("web1:22").unwrap());
        assert!(matches!(hosts.check("web1", 22, FP_A), HostCheck::FirstUse));
    }
}
