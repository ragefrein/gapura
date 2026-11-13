//! SSH integration tests (design doc section 8).
//!
//! Two layers are covered:
//!
//! 1. An in-process russh **server** fixture — no external dependencies, so
//!    these run anywhere, including CI. It exercises auth (password /
//!    key file), the PTY/shell path, the session actor, and the TOFU logic.
//! 2. A real OpenSSH server when `GAPURA_TEST_HOST` is set (CI starts one
//!    locally). Skipped silently otherwise.

use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gapura_core::error::GapuraError;
use gapura_core::model::AuthMethod;
use gapura_core::ssh::backend::{connect, ConnectParams, ResolvedAuth};
use gapura_core::ssh::keys::generate_ed25519;
use gapura_core::ssh::known_hosts::{KnownHostEntry, KnownHosts};
use gapura_core::ssh::session::{start_session, SessionCommand, SessionEvent, SessionState};
use rand_core::UnwrapErr;
use russh::server::{Auth, Handler, Msg};
use russh::{ChannelId, CryptoVec};
use ssh_key::{HashAlg, PrivateKey};

const TEST_USER: &str = "gapura-tester";
const TEST_PASSWORD: &str = "a-test-password-9";

// ---------------------------------------------------------------------------
// In-process sshd fixture
// ---------------------------------------------------------------------------

struct EchoServer {
    addr: SocketAddr,
    host_key: PrivateKey,
    /// Kept alive on purpose: dropping the task ends the server.
    acceptor: tokio::task::JoinHandle<()>,
}

impl EchoServer {
    fn stop(self) {
        self.acceptor.abort();
    }
}

fn server_config(host_key: PrivateKey) -> Arc<russh::server::Config> {
    Arc::new(russh::server::Config {
        keys: vec![host_key],
        auth_rejection_time: Duration::from_millis(50),
        ..russh::server::Config::default()
    })
}

#[derive(Clone)]
struct EchoHandler {
    authorized_fp: Option<Arc<String>>,
}

impl Handler for EchoHandler {
    type Error = russh::Error;

    async fn channel_open_session(
        &mut self,
        _channel: russh::Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> impl Future<Output = Result<Auth, Self::Error>> + Send {
        let ok = user == TEST_USER && password == TEST_PASSWORD;
        async move {
            Ok(if ok {
                Auth::Accept
            } else {
                Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                }
            })
        }
    }

    fn auth_publickey(
        &mut self,
        _user: &str,
        key: russh::server::VerifiedPublicKey<'_>,
    ) -> impl Future<Output = Result<Auth, Self::Error>> + Send {
        let expected = self.authorized_fp.clone();
        let presented = key.public_key.fingerprint(HashAlg::Sha256).to_string();
        async move {
            let ok = expected.as_deref() == Some(&presented);
            Ok(if ok {
                Auth::Accept
            } else {
                Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                }
            })
        }
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        let _ = session.channel_success(channel);
        session.data(channel, "Gapura echo shell ready\r\n")?;
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        let text = String::from_utf8_lossy(data).to_string();
        for line in text.lines() {
            if line.trim() == "exit" {
                let _ = session.close(channel);
                return Ok(());
            }
            let reply = format!("ECHO: {line}\r\n");
            session.data(channel, reply)?;
        }
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        _channel: ChannelId,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

async fn spawn_echo_server(authorized_fp: Option<String>) -> EchoServer {
    let host_key = PrivateKey::random(
        &mut UnwrapErr(getrandom::SysRng),
        ssh_key::Algorithm::Ed25519,
    )
    .expect("host key generation cannot fail");
    let handler = EchoHandler {
        authorized_fp: authorized_fp.clone().map(Arc::new),
    };
    let config = server_config(host_key.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let acceptor = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => return,
            };
            let handler = handler.clone();
            let config = Arc::clone(&config);
            tokio::spawn(async move {
                let _ = russh::server::run_stream(config, stream, handler).await;
            });
        }
    });

    EchoServer {
        addr,
        host_key,
        acceptor,
    }
}

fn temp_known_hosts_dir() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    (tmp, dir)
}

async fn connect_to(
    addr: SocketAddr,
    auth: ResolvedAuth,
    known_hosts: Arc<Mutex<KnownHosts>>,
) -> gapura_core::error::Result<gapura_core::ssh::backend::ConnectOutcome> {
    connect(
        ConnectParams {
            host: addr.ip().to_string(),
            port: addr.port(),
            username: TEST_USER.into(),
            auth,
        },
        known_hosts,
    )
    .await
}

/// Collect output events until `needle` appears (or timeout).
async fn wait_for_output(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<SessionEvent>,
    needle: &str,
    timeout: Duration,
) -> String {
    use std::time::Instant;
    let deadline = Instant::now() + timeout;
    let mut collected = String::new();
    loop {
        let now = Instant::now();
        if now >= deadline {
            return collected;
        }
        match tokio::time::timeout(deadline - now, rx.recv()).await {
            Ok(Some(SessionEvent::Data(bytes))) => {
                collected.push_str(&String::from_utf8_lossy(&bytes));
                if collected.contains(needle) {
                    return collected;
                }
            }
            Ok(Some(SessionEvent::Status(_))) | Ok(None) | Err(_) => return collected,
        }
    }
}

fn events_channel() -> (
    tokio::sync::mpsc::UnboundedSender<SessionEvent>,
    tokio::sync::mpsc::UnboundedReceiver<SessionEvent>,
) {
    tokio::sync::mpsc::unbounded_channel()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn password_auth_shell_echo_roundtrip() {
    let server = spawn_echo_server(None).await;
    let (_tmp, dir) = temp_known_hosts_dir();
    let known = Arc::new(Mutex::new(KnownHosts::load(&dir).unwrap()));

    let outcome = connect_to(
        server.addr,
        ResolvedAuth::Password {
            password: TEST_PASSWORD.into(),
        },
        known.clone(),
    )
    .await
    .expect("password connect must succeed");

    assert!(outcome.fingerprint.starts_with("SHA256:"));
    assert_eq!(
        outcome.fingerprint,
        *server
            .host_key
            .public_key()
            .fingerprint(HashAlg::Sha256)
            .to_string()
    );
    assert!(outcome.new_host_key, "first connect pins the key");

    let (tx, mut rx) = events_channel();
    let sink = Arc::new(move |event| {
        let _ = tx.send(event);
    });
    let commands = start_session(outcome.handle, 80, 24, sink).await.unwrap();

    // The banner proves the PTY/shell path works.
    let banner = wait_for_output(&mut rx, "echo shell ready", Duration::from_secs(10)).await;
    assert!(
        banner.contains("Gapura echo shell ready"),
        "got: {banner:?}"
    );

    // Round-trip typed input through the actor.
    commands
        .send(SessionCommand::Write(b"hello gapura\n".to_vec()))
        .await
        .unwrap();
    let echoed = wait_for_output(&mut rx, "ECHO: hello gapura", Duration::from_secs(10)).await;
    assert!(echoed.contains("ECHO: hello gapura"), "got: {echoed:?}");

    // Resize must not break anything.
    commands
        .send(SessionCommand::Resize(100, 40))
        .await
        .unwrap();
    commands
        .send(SessionCommand::Write(b"still alive\n".to_vec()))
        .await
        .unwrap();
    let after = wait_for_output(&mut rx, "ECHO: still alive", Duration::from_secs(10)).await;
    assert!(after.contains("ECHO: still alive"));

    // Close and expect the closed status event (actor needs a moment).
    commands.send(SessionCommand::Close).await.unwrap();
    let mut closed = false;
    for _ in 0..50 {
        match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
            Ok(Some(SessionEvent::Status(SessionState::Closed { .. }))) => {
                closed = true;
                break;
            }
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => break,
        }
    }
    assert!(closed, "must receive a Closed status after Close");

    server.stop();
}

#[tokio::test]
async fn wrong_password_is_auth_failure() {
    let server = spawn_echo_server(None).await;
    let (_tmp, dir) = temp_known_hosts_dir();
    let known = Arc::new(Mutex::new(KnownHosts::load(&dir).unwrap()));

    let result = connect_to(
        server.addr,
        ResolvedAuth::Password {
            password: "wrong".into(),
        },
        known,
    )
    .await;
    assert!(
        matches!(result, Err(GapuraError::AuthFailed { .. })),
        "got: {result:?}"
    );
    server.stop();
}

#[tokio::test]
async fn keyfile_auth_roundtrip() {
    // Generate a real key pair with our own key code, authorize it on the
    // server by fingerprint, and connect with KeyFile auth.
    let key_dir = tempfile::tempdir().unwrap();
    let key_path = key_dir.path().join("test_key");
    let info = generate_ed25519(&key_path, "gapura-test", None).unwrap();

    let server = spawn_echo_server(Some(info.fingerprint.clone())).await;
    let (_tmp, dir) = temp_known_hosts_dir();
    let known = Arc::new(Mutex::new(KnownHosts::load(&dir).unwrap()));
    let outcome = connect_to(
        server.addr,
        ResolvedAuth::KeyFile {
            path: key_path,
            passphrase: None,
        },
        known,
    )
    .await
    .expect("key file connect must succeed");

    let (tx, mut rx) = events_channel();
    let sink = Arc::new(move |event| {
        let _ = tx.send(event);
    });
    let _commands = start_session(outcome.handle, 80, 24, sink).await.unwrap();
    let banner = wait_for_output(&mut rx, "echo shell ready", Duration::from_secs(10)).await;
    assert!(
        banner.contains("Gapura echo shell ready"),
        "got: {banner:?}"
    );
    server.stop();
}

#[tokio::test]
async fn tofu_mismatch_blocks_and_carries_both_fingerprints() {
    let server = spawn_echo_server(None).await;
    let (_tmp, dir) = temp_known_hosts_dir();

    // Pre-seed known-hosts with a fingerprint the server will not present.
    let mut known = KnownHosts::load(&dir).unwrap();
    known
        .record(
            server.addr.ip().to_string().as_str(),
            server.addr.port(),
            "SHA256:stale-fingerprint-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            "ssh-ed25519".into(),
        )
        .unwrap();
    let known = Arc::new(Mutex::new(known));

    let result = connect_to(
        server.addr,
        ResolvedAuth::Password {
            password: TEST_PASSWORD.into(),
        },
        known,
    )
    .await;
    match result {
        Err(GapuraError::HostKeyMismatch { stored, actual, .. }) => {
            assert!(stored.contains("stale-fingerprint"));
            assert_eq!(
                actual,
                *server
                    .host_key
                    .public_key()
                    .fingerprint(HashAlg::Sha256)
                    .to_string()
            );
        }
        other => panic!("expected HostKeyMismatch, got {other:?}"),
    }
    server.stop();
}

#[tokio::test]
async fn known_hosts_persist_between_connections() {
    let server = spawn_echo_server(None).await;
    let (_tmp, dir) = temp_known_hosts_dir();
    let known = Arc::new(Mutex::new(KnownHosts::load(&dir).unwrap()));

    connect_to(
        server.addr,
        ResolvedAuth::Password {
            password: TEST_PASSWORD.into(),
        },
        known.clone(),
    )
    .await
    .unwrap();

    let known_hosts_path = dir.join("known_hosts.json");
    assert!(known_hosts_path.is_file());
    let reloaded = KnownHosts::load(&dir).unwrap();
    assert_eq!(reloaded.entries().len(), 1);
    let host_port = format!("{}:{}", server.addr.ip(), server.addr.port());
    assert_eq!(reloaded.entries()[0].host_port, host_port);
    server.stop();
}

// ---------------------------------------------------------------------------
// Real OpenSSH server (CI only)
// ---------------------------------------------------------------------------

fn real_sshd_env() -> Option<(String, u16, String, PathBuf)> {
    let host = std::env::var("GAPURA_TEST_HOST").ok()?;
    let port = std::env::var("GAPURA_TEST_PORT").ok()?.parse().ok()?;
    let user = std::env::var("GAPURA_TEST_USER").ok()?;
    let key = PathBuf::from(std::env::var("GAPURA_TEST_KEY").ok()?);
    Some((host, port, user, key))
}

#[tokio::test]
async fn real_sshd_session_if_available() {
    let Some((host, port, user, key_path)) = real_sshd_env() else {
        eprintln!("skipping: GAPURA_TEST_HOST not set");
        return;
    };

    let known = Arc::new(Mutex::new(
        KnownHosts::load(&std::env::temp_dir().join("gapura-test-known-hosts")).unwrap(),
    ));
    let outcome = connect(
        ConnectParams {
            host,
            port,
            username: user,
            auth: ResolvedAuth::KeyFile {
                path: key_path,
                passphrase: None,
            },
        },
        known,
    )
    .await
    .expect("connect to the real sshd");

    let (tx, mut rx) = events_channel();
    let sink = Arc::new(move |event| {
        let _ = tx.send(event);
    });
    let commands = start_session(outcome.handle, 80, 24, sink).await.unwrap();
    commands
        .send(SessionCommand::Write(
            b"echo gapura-sshd-ok $((21*2))\n".to_vec(),
        ))
        .await
        .unwrap();
    let out = wait_for_output(&mut rx, "gapura-sshd-ok 42", Duration::from_secs(20)).await;
    assert!(out.contains("gapura-sshd-ok 42"), "got: {out:?}");
    commands.send(SessionCommand::Close).await.unwrap();
}

// Silence unused warnings for types used only in some cfg paths.
#[allow(unused)]
fn _type_witness() {
    let _: Option<KnownHostEntry> = None;
    let _: Option<AuthMethod> = None;
    let _: Option<CryptoVec> = None;
    let _: Option<Msg> = None;
}
