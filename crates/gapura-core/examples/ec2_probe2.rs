//! Replicates the app's session actor exactly: start_session + immediate
//! resizes + typing, then idle observation.
//! Run: cargo run -p gapura-core --example ec2_probe2 -- <host> <port> <user> <keypath>

use gapura_core::ssh::backend::{connect, ConnectParams, ResolvedAuth};
use gapura_core::ssh::known_hosts::KnownHosts;
use gapura_core::ssh::session::{start_session, SessionCommand, SessionEvent};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (host, port, user, key) = (
        args[1].clone(),
        args[2].parse().unwrap(),
        args[3].clone(),
        args[4].clone(),
    );
    let tmp = tempfile::tempdir().unwrap();
    let known = Arc::new(Mutex::new(KnownHosts::load(tmp.path()).unwrap()));

    let outcome = connect(
        ConnectParams {
            host,
            port,
            username: user,
            auth: ResolvedAuth::KeyFile {
                path: key.into(),
                passphrase: None,
            },
        },
        known,
        Arc::new(Mutex::new(HashMap::new())),
    )
    .await
    .expect("connect");

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SessionEvent>();
    let sink: gapura_core::ssh::session::EventSink = Arc::new(move |event| {
        let _ = tx.send(event);
    });

    let commands = start_session(outcome.handle, 80, 24, sink)
        .await
        .expect("session");
    println!("actor started");

    // what TerminalView does right after mount: a fit-driven resize
    tokio::time::sleep(Duration::from_millis(600)).await;
    commands
        .send(SessionCommand::Resize(190, 45))
        .await
        .unwrap();
    println!("resize sent");

    // typing
    tokio::time::sleep(Duration::from_millis(600)).await;
    commands
        .send(SessionCommand::Write(b"echo probe2-alive\n".to_vec()))
        .await
        .unwrap();

    let start = std::time::Instant::now();
    let deadline = start + Duration::from_secs(25);
    let mut got_echo = false;
    while std::time::Instant::now() < deadline {
        match tokio::time::timeout(deadline - std::time::Instant::now(), rx.recv()).await {
            Ok(Some(SessionEvent::Data(bytes))) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                if text.contains("probe2-alive") {
                    got_echo = true;
                    println!(
                        "[{:>5}ms] echo confirmed",
                        (std::time::Instant::now() - start).as_millis()
                    );
                }
                if text.contains("exit") || text.contains("Alarm") {
                    println!(
                        "[{:>5}ms] suspicious output: {text:?}",
                        (std::time::Instant::now() - start).as_millis()
                    );
                }
            }
            Ok(Some(SessionEvent::Status(status))) => {
                println!(
                    "[{:>5}ms] STATUS: {status:?}",
                    (std::time::Instant::now() - start).as_millis()
                );
                break;
            }
            Ok(Some(SessionEvent::ForwardStatus { ok, message, .. })) => {
                println!(
                    "[{:>5}ms] FORWARD ({ok}): {message}",
                    (std::time::Instant::now() - start).as_millis()
                );
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    println!("done, echo={got_echo}");
}
