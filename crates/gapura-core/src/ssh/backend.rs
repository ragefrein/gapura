//! russh-backed SSH client (design doc section 3).
//!
//! This is the one place that knows about russh; the rest of the codebase
//! sees [`ConnectOutcome`] and the session actor in [`super::session`]. If
//! the Plan B backend (OpenSSH subprocess) is ever needed, it slots in here
//! behind the same interface.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use russh::client::{self, Handle};
use russh::keys::{agent::client::AgentClient, load_secret_key, PrivateKey, PrivateKeyWithHashAlg};
use ssh_key::{HashAlg, PublicKey};

use super::known_hosts::{HostCheck, KnownHosts};
use crate::error::{GapuraError, Result};

/// Authentication already resolved to concrete values (password decrypted
/// from the vault, key path known). Built by the caller, consumed by
/// [`connect`].
#[derive(Debug, Clone)]
pub enum ResolvedAuth {
    Agent,
    Password {
        /// Short-lived: handed to russh and dropped after auth.
        password: String,
    },
    KeyFile {
        path: PathBuf,
        passphrase: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct ConnectParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: ResolvedAuth,
}

/// What a successful connect hands to the session layer.
pub struct ConnectOutcome {
    pub handle: Handle<ClientHandler>,
    /// Fingerprint the server presented (already pinned if this was first use).
    pub fingerprint: String,
    pub algorithm: String,
    /// True on the very first connection to this host: the UI can tell the
    /// user the fingerprint was just pinned.
    pub new_host_key: bool,
}

impl std::fmt::Debug for ConnectOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // russh's Handle does not implement Debug; skip it.
        f.debug_struct("ConnectOutcome")
            .field("fingerprint", &self.fingerprint)
            .field("algorithm", &self.algorithm)
            .field("new_host_key", &self.new_host_key)
            .finish_non_exhaustive()
    }
}

/// Remote-forward registry: which server-side listener maps to which local
/// target. Keyed by `bind_host:bind_port` (as the server reports it in
/// forwarded-tcpip channel opens), valued by the local dial target.
pub type RemoteForwardRegistry = Arc<Mutex<HashMap<(String, u16), (String, u16)>>>;

/// Shared out-of-band state the handshake handler writes to. The connection
/// itself returns russh errors, so the fingerprint details travel beside it.
#[derive(Default)]
struct HandshakeInfo {
    new_host_key: AtomicBool,
    key_info: Mutex<Option<(String, String)>>, // (fingerprint, algorithm)
}

pub struct ClientHandler {
    host: String,
    port: u16,
    known_hosts: Arc<Mutex<KnownHosts>>,
    info: Arc<HandshakeInfo>,
    /// Targets for Remote forward rules (PRD F12).
    remote_forwards: RemoteForwardRegistry,
}

impl client::Handler for ClientHandler {
    type Error = GapuraError;

    /// A Remote forward rule fired: the server accepted a connection on the
    /// listening port and hands us a channel. Dial the rule's local target
    /// and pipe the two streams into each other.
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::client::ChannelOpenHandle,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        let key = (connected_address.to_string(), connected_port as u16);
        let target = self
            .remote_forwards
            .lock()
            .ok()
            .and_then(|map| map.get(&key).cloned());
        let Some((target_host, target_port)) = target else {
            tracing::warn!(
                connected = %connected_address,
                port = connected_port,
                "forwarded connection for an unknown remote forward; closing"
            );
            return Ok(());
        };
        tracing::info!(
            bind = %connected_address,
            bind_port = connected_port,
            %target_host,
            target_port,
            "remote forward connection accepted"
        );
        tokio::spawn(async move {
            let mut channel = channel.into_stream();
            let mut local =
                match tokio::net::TcpStream::connect((target_host.as_str(), target_port)).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(
                            "cannot reach local target {target_host}:{target_port}: {e}"
                        );
                        return;
                    }
                };
            let _ = tokio::io::copy_bidirectional(&mut channel, &mut local).await;
        });
        Ok(())
    }

    /// TOFU decision point (PRD F5). New hosts are pinned and remembered; a
    /// changed fingerprint aborts the handshake with
    /// [`GapuraError::HostKeyMismatch`]. There is deliberately no
    /// auto-accept path (design doc section 7).
    async fn check_server_key(
        &mut self,
        server_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let public_key: PublicKey = match server_key {
            russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            russh::keys::PublicKeyOrCertificate::Certificate(cert) => {
                ssh_key::PublicKey::new(cert.public_key().clone(), "")
            }
        };
        let fingerprint = public_key.fingerprint(HashAlg::Sha256).to_string();
        let algorithm = public_key.algorithm().as_str().to_string();

        let mut known = self
            .known_hosts
            .lock()
            .map_err(|_| GapuraError::Other("known-hosts lock poisoned".into()))?;
        match known.check(&self.host, self.port, &fingerprint) {
            HostCheck::FirstUse => {
                known
                    .record(
                        &self.host,
                        self.port,
                        fingerprint.clone(),
                        algorithm.clone(),
                    )
                    .map_err(|e| GapuraError::Other(format!("cannot record host key: {e}")))?;
                self.info.new_host_key.store(true, Ordering::SeqCst);
                *self.info.key_info.lock().unwrap() = Some((fingerprint, algorithm));
                tracing::info!(host = %self.host, port = self.port, "host key pinned (first use)");
                Ok(true)
            }
            HostCheck::Match => {
                *self.info.key_info.lock().unwrap() = Some((fingerprint, algorithm));
                Ok(true)
            }
            HostCheck::Mismatch { stored } => Err(GapuraError::HostKeyMismatch {
                host: self.host.clone(),
                port: self.port,
                stored,
                actual: fingerprint,
            }),
        }
    }
}

/// Bridge russh errors into the single error type.
impl From<russh::Error> for GapuraError {
    fn from(error: russh::Error) -> Self {
        GapuraError::Ssh(error.to_string())
    }
}

/// SSH-2 client id; identifies the app on the wire.
const CLIENT_ID: &str = "SSH-2.0-Gapura_0.1";

fn russh_config() -> Arc<client::Config> {
    // design doc 3.2: keepalive every 15 s, give up after 3 misses.
    Arc::new(client::Config {
        keepalive_interval: Some(std::time::Duration::from_secs(15)),
        keepalive_max: 3,
        nodelay: true,
        client_id: russh::SshId::Standard(CLIENT_ID.into()),
        ..client::Config::default()
    })
}

/// Connect, verify the host key (TOFU), and authenticate.
pub async fn connect(
    params: ConnectParams,
    known_hosts: Arc<Mutex<KnownHosts>>,
    remote_forwards: RemoteForwardRegistry,
) -> Result<ConnectOutcome> {
    let address = (params.host.as_str(), params.port);
    let info = Arc::new(HandshakeInfo::default());
    let handler = ClientHandler {
        host: params.host.clone(),
        port: params.port,
        known_hosts: Arc::clone(&known_hosts),
        info: Arc::clone(&info),
        remote_forwards,
    };

    let mut handle = match client::connect(russh_config(), address, handler).await {
        Ok(handle) => handle,
        // The fingerprint-mismatch error travels to the UI unchanged — the
        // modal needs the stored and actual fingerprints to decide.
        Err(e @ (GapuraError::HostKeyMismatch { .. } | GapuraError::Other(_))) => return Err(e),
        Err(e) => {
            return Err(GapuraError::ConnectFailed {
                host: params.host.clone(),
                port: params.port,
                detail: e.to_string(),
            })
        }
    };

    authenticate(&mut handle, &params).await?;

    let (fingerprint, algorithm) = info
        .key_info
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| (String::new(), String::from("unknown")));
    let new_host_key = info.new_host_key.load(Ordering::SeqCst);

    Ok(ConnectOutcome {
        handle,
        fingerprint,
        algorithm,
        new_host_key,
    })
}

async fn authenticate(handle: &mut Handle<ClientHandler>, params: &ConnectParams) -> Result<()> {
    let username = params.username.as_str();
    match &params.auth {
        ResolvedAuth::Agent => agent_auth(handle, username).await,
        ResolvedAuth::Password { password } => {
            let result = handle
                .authenticate_password(username, password.as_str())
                .await?;
            finish_auth(result, params)
        }
        ResolvedAuth::KeyFile { path, passphrase } => {
            let path = super::keys::expand_home(path);
            let key = load_secret_key(&path, passphrase.as_deref()).map_err(|e| {
                GapuraError::Other(format!("cannot read key {}: {e}", path.display()))
            })?;
            let hash_alg = preferred_hash_alg(&key);
            let result = handle
                .authenticate_publickey(
                    username,
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg),
                )
                .await?;
            finish_auth(result, params)
        }
    }
}

async fn agent_auth(handle: &mut Handle<ClientHandler>, username: &str) -> Result<()> {
    let mut agent = AgentClient::connect_env()
        .await
        .map_err(|e| GapuraError::Ssh(format!("no SSH agent available (SSH_AUTH_SOCK): {e}")))?;
    let identities = agent
        .request_identities()
        .await
        .map_err(GapuraError::from)?;
    if identities.is_empty() {
        return Err(GapuraError::AuthFailed {
            username: username.to_string(),
            host: String::new(),
            detail: "the agent holds no keys".into(),
        });
    }
    let mut last_detail = String::from("the agent refused every key");
    for identity in identities {
        let public_key = identity.public_key().into_owned();
        let hash_alg = preferred_hash_alg_for_algorithm(public_key.algorithm());
        let result = handle
            .authenticate_publickey_with(username, public_key, hash_alg, &mut agent)
            .await
            .map_err(|e| GapuraError::Ssh(format!("agent error: {e}")))?;
        if result.success() {
            return Ok(());
        }
        last_detail = format!(
            "agent key {} was rejected",
            identity.public_key().fingerprint(HashAlg::Sha256)
        );
    }
    Err(GapuraError::AuthFailed {
        username: username.to_string(),
        host: String::new(),
        detail: last_detail,
    })
}

/// RSA keys must sign with `rsa-sha2-256` these days: plain `ssh-rsa`
/// (SHA-1) is rejected by OpenSSH 8.8+ — and by AWS EC2 hosts. Ed25519 has
/// its own signature scheme, where the hash parameter stays unused.
fn preferred_hash_alg(key: &PrivateKey) -> Option<HashAlg> {
    preferred_hash_alg_for_algorithm(key.algorithm())
}

fn preferred_hash_alg_for_algorithm(algorithm: ssh_key::Algorithm) -> Option<HashAlg> {
    match algorithm {
        ssh_key::Algorithm::Rsa { .. } => Some(HashAlg::Sha256),
        _ => None,
    }
}

fn finish_auth(result: russh::client::AuthResult, params: &ConnectParams) -> Result<()> {
    if result.success() {
        Ok(())
    } else {
        Err(GapuraError::AuthFailed {
            username: params.username.clone(),
            host: params.host.clone(),
            detail: "the server rejected the credentials".into(),
        })
    }
}
