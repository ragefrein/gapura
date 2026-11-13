//! End-to-end test through the Tauri command layer (no webview).
//!
//! Uses tauri's mock runtime to invoke the real commands exactly as the UI
//! does: vault setup -> connection CRUD -> session_open -> assert terminal
//! data arrives via the session:data event. Talks to a real OpenSSH server
//! when GAPURA_TEST_* is set; skipped otherwise (the core integration tests
//! cover the in-process fixture path).

#![cfg(not(target_os = "windows"))]

use std::time::Duration;

use gapura_core::paths;
use gapura_lib::commands;
use gapura_lib::state::AppState;
use tauri::{Listener, Manager};

fn real_sshd_env() -> Option<(String, u16, String, String)> {
    let host = std::env::var("GAPURA_TEST_HOST").ok()?;
    let port = std::env::var("GAPURA_TEST_PORT").ok()?.parse().ok()?;
    let user = std::env::var("GAPURA_TEST_USER").ok()?;
    let key = std::env::var("GAPURA_TEST_KEY").ok()?;
    Some((host, port, user, key))
}

#[tokio::test]
async fn full_flow_through_commands() {
    let Some((host, port, user, key_path)) = real_sshd_env() else {
        eprintln!("skipping: GAPURA_TEST_HOST not set");
        return;
    };

    // Use a throwaway data dir so the developer's real vault is untouched.
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().to_path_buf();
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();

    let app = tauri::test::mock_builder()
        .build(tauri::generate_context!())
        .expect("mock app builds");
    app.manage(AppState::new(data_dir.clone(), config_dir).unwrap());

    let state = app.state::<AppState>();

    // 1. Vault setup (passphrase mode; keychain needs a desktop session).
    commands::vault::vault_setup(
        state.clone(),
        "passphrase".into(),
        Some("test-pass-9".into()),
    )
    .await
    .expect("vault setup");

    // 2. Create a connection pointing at the real sshd.
    let connection = commands::connections::save_connection(
        state.clone(),
        gapura_lib::commands::connections::ConnectionInput {
            id: None,
            name: "ci-sshd".into(),
            group_id: None,
            host: host.clone(),
            port,
            username: user.clone(),
            auth: gapura_lib::commands::connections::AuthInput::KeyFile {
                path: key_path.clone(),
                passphrase: None,
            },
            tags: vec![],
            color: None,
            notes: String::new(),
        },
    )
    .expect("connection saved");

    // 3. Collect session:data events while the session runs. tokio's channel
    // keeps the current-thread runtime alive while awaiting below.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    app.listen("session:data", move |event| {
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
            if let Some(data) = payload["data"].as_str() {
                let _ = tx.send(data.to_string());
            }
        }
    });

    // 4. Open the session through the actual command.
    let info = commands::sessions::session_open(
        app.handle().clone(),
        state.clone(),
        connection.id,
        80,
        24,
    )
    .await
    .expect("session opens against the real sshd");
    assert!(info.fingerprint.starts_with("SHA256:"));

    // 5. Type a command and wait for it to come back.
    use base64::Engine;
    let typed = base64::engine::general_purpose::STANDARD.encode(b"echo gapura-cmd-ok\r\n");
    commands::sessions::session_write(state.clone(), info.id, typed)
        .await
        .expect("write works");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut collected = String::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!(
                "terminal output never contained the echo; got {} chars: {collected}",
                collected.len()
            );
        }
        match tokio::time::timeout(remaining.min(Duration::from_millis(500)), rx.recv()).await {
            Ok(Some(chunk)) => {
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&chunk) {
                    collected.push_str(&String::from_utf8_lossy(&bytes));
                }
                if collected.contains("gapura-cmd-ok") {
                    break;
                }
            }
            Ok(None) => panic!("event channel closed; got: {collected}"),
            Err(_) => continue,
        }
    }

    // 6. Resize must be accepted, close must clean up.
    commands::sessions::session_resize(state.clone(), info.id, 120, 40)
        .await
        .expect("resize works");
    commands::sessions::session_close(state.clone(), info.id)
        .await
        .expect("close works");
    assert!(state.sessions.read().unwrap().is_empty());

    // 7. Known-hosts persisted in the throwaway data dir.
    let hosts =
        gapura_core::ssh::known_hosts::KnownHosts::load(&paths::ensure_dir(&data_dir).unwrap())
            .unwrap();
    assert_eq!(hosts.entries().len(), 1);
}

#[tokio::test]
async fn tofu_mismatch_surfaces_through_commands() {
    let Some((host, port, user, key_path)) = real_sshd_env() else {
        eprintln!("skipping: GAPURA_TEST_HOST not set");
        return;
    };

    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().to_path_buf();
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();

    // Seed the stale fingerprint BEFORE AppState loads known-hosts.
    let mut hosts =
        gapura_core::ssh::known_hosts::KnownHosts::load(&paths::ensure_dir(&data_dir).unwrap())
            .unwrap();
    hosts
        .record(
            &host,
            port,
            "SHA256:stale-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            "ssh-ed25519".into(),
        )
        .unwrap();

    let app = tauri::test::mock_builder()
        .build(tauri::generate_context!())
        .expect("mock app builds");
    app.manage(AppState::new(data_dir.clone(), config_dir).unwrap());
    let state = app.state::<AppState>();

    commands::vault::vault_setup(
        state.clone(),
        "passphrase".into(),
        Some("test-pass-9".into()),
    )
    .await
    .expect("vault setup");

    let connection = commands::connections::save_connection(
        state.clone(),
        gapura_lib::commands::connections::ConnectionInput {
            id: None,
            name: "stale-host".into(),
            group_id: None,
            host: host.clone(),
            port,
            username: user.clone(),
            auth: gapura_lib::commands::connections::AuthInput::KeyFile {
                path: key_path,
                passphrase: None,
            },
            tags: vec![],
            color: None,
            notes: String::new(),
        },
    )
    .expect("connection saved");

    let result = commands::sessions::session_open(
        app.handle().clone(),
        state.clone(),
        connection.id,
        80,
        24,
    )
    .await;
    match result {
        Err(e) => {
            let payload = serde_json::to_value(&e).unwrap();
            assert_eq!(payload["code"], "ssh.host_key_mismatch");
            assert!(payload["stored_fingerprint"]
                .as_str()
                .unwrap()
                .contains("stale-"));
            assert!(payload["actual_fingerprint"]
                .as_str()
                .unwrap()
                .starts_with("SHA256:"));
        }
        Ok(_) => panic!("expected the host key mismatch to surface as an error"),
    }
}
