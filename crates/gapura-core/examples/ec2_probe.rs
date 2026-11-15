//! Reproduces the immediate-exit-on-EC2 issue: connects like the app does,
//! logs every channel event with timestamps for 20 seconds.
//! Run: cargo run -p gapura-core --example ec2_probe -- <host> <port> <user> <keypath>

use gapura_core::ssh::backend::{connect, ConnectParams, ResolvedAuth};
use gapura_core::ssh::known_hosts::KnownHosts;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    )
    .await
    .expect("connect");
    println!("connected, fp={}", &outcome.fingerprint[..24]);

    let handle = outcome.handle;
    let channel = handle.channel_open_session().await.expect("channel");
    channel
        .request_pty(false, "xterm-256color", 80, 24, 0, 0, &[])
        .await
        .expect("pty");
    channel.request_shell(true).await.expect("shell");
    println!("pty+shell requested");

    let (read, _write) = channel.split(); // keep write half alive
    let _ = _write;
    let mut read = read;
    let start = Instant::now();
    let deadline = start + Duration::from_secs(20);
    loop {
        let now = Instant::now();
        if now >= deadline {
            println!("20s elapsed, still alive");
            break;
        }
        match tokio::time::timeout(deadline - now, read.wait()).await {
            Ok(Some(msg)) => println!(
                "[{:>5}ms] {:?}",
                (Instant::now() - start).as_millis(),
                match &msg {
                    russh::ChannelMsg::ExitStatus { exit_status } => {
                        format!("ExitStatus({exit_status})")
                    }
                    russh::ChannelMsg::Data { data } => {
                        format!(
                            "Data({} bytes: {:?})",
                            data.len(),
                            String::from_utf8_lossy(&data[..data.len().min(60)])
                        )
                    }
                    other => format!("{other:?}"),
                }
            ),
            Ok(None) => {
                println!(
                    "[{:>5}ms] channel ended (None)",
                    (Instant::now() - start).as_millis()
                );
                break;
            }
            Err(_) => {
                println!("20s elapsed, still alive");
                break;
            }
        }
    }
}
