//! Session actor (design doc section 5): one task pair per SSH session.
//!
//! The writer half of the russh channel is owned by the command loop; the
//! reader half runs in its own task and forwards messages through a queue.
//! Output is coalesced (~16 ms / 64 KiB) before it crosses the IPC boundary,
//! so full-screen ncurses apps do not flood the webview with tiny events.

use std::sync::Arc;
use std::time::{Duration, Instant};

use russh::client::{Handle, Msg};
use russh::{ChannelMsg, ChannelReadHalf};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::backend::ClientHandler;
use crate::error::{GapuraError, Result};

/// Not `Clone`: the forward variants own live TCP streams.
#[derive(Debug)]
pub enum SessionCommand {
    /// Bytes typed by the user (or pasted) — forwarded to the remote shell.
    Write(Vec<u8>),
    Resize(u32, u32),
    Close,
    /// A locally accepted TCP connection for a Local forward rule: the actor
    /// opens a direct-tcpip channel to the rule target and pipes the two
    /// streams into each other.
    ForwardChannel {
        rule_id: Uuid,
        stream: tokio::net::TcpStream,
        target_host: String,
        target_port: u16,
        originator: String,
    },
    /// Ask the server to listen for a Remote forward rule.
    RemoteListen {
        rule_id: Uuid,
        bind_host: String,
        bind_port: u16,
    },
    /// Ask the server to stop listening for a Remote forward rule.
    RemoteCancel {
        rule_id: Uuid,
        bind_host: String,
        bind_port: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Connected,
    Closed { reason: String },
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// Raw terminal output bytes (coalesced).
    Data(Vec<u8>),
    Status(SessionState),
    /// Outcome of a forwarding request (rule id, success, human message).
    ForwardStatus {
        rule_id: Uuid,
        ok: bool,
        message: String,
    },
}

/// UI/IPC callback invoked from the actor task.
pub type EventSink = Arc<dyn Fn(SessionEvent) + Send + Sync>;

const COALESCE_WINDOW: Duration = Duration::from_millis(16);
const COALESCE_LIMIT: usize = 64 * 1024;
const PTY_TERM: &str = "xterm-256color";

/// Request a PTY + shell on the connection and start the actor.
/// Returns the command sender used to write/resize/close later.
pub async fn start_session(
    handle: Handle<ClientHandler>,
    cols: u32,
    rows: u32,
    on_event: EventSink,
) -> Result<mpsc::Sender<SessionCommand>> {
    let channel = handle
        .channel_open_session()
        .await
        .map_err(GapuraError::from)?;
    channel
        .request_pty(false, PTY_TERM, cols, rows, 0, 0, &[])
        .await
        .map_err(GapuraError::from)?;
    channel
        .request_shell(true)
        .await
        .map_err(GapuraError::from)?;

    tracing::info!(%cols, %rows, "session actor starting (pty requested)");
    let (commands, command_rx) = mpsc::channel::<SessionCommand>(64);
    tokio::spawn(run_actor(handle, channel, command_rx, on_event));
    Ok(commands)
}

async fn run_actor(
    handle: Handle<ClientHandler>,
    channel: russh::Channel<Msg>,
    mut command_rx: mpsc::Receiver<SessionCommand>,
    on_event: EventSink,
) {
    let (read_half, write_half) = channel.split();
    let (msg_tx, mut msg_rx) = mpsc::unbounded_channel::<ChannelMsg>();

    // Reader task: forwards raw channel messages to the actor loop below.
    tokio::spawn(async move {
        let mut read_half: ChannelReadHalf = read_half;
        while let Some(msg) = read_half.wait().await {
            if msg_tx.send(msg).is_err() {
                break;
            }
        }
        // Reader ended; the unbounded queue dropping tells the loop.
    });

    let mut out: Vec<u8> = Vec::with_capacity(8 * 1024);
    let mut last_flush = Instant::now();
    let mut exit_code: Option<u32> = None;
    let mut reason: Option<String> = None;

    loop {
        tokio::select! {
            cmd = command_rx.recv() => {
                match cmd {
                    None => {
                        reason = Some("session dropped".into());
                        break;
                    }
                    Some(SessionCommand::Close) => {
                        reason = Some("closed by user".into());
                        break;
                    }
                    Some(SessionCommand::Write(data)) => {
                        if let Err(e) = write_half.data_bytes(data).await {
                            reason = Some(format!("write failed: {e}"));
                            break;
                        }
                    }
                    Some(SessionCommand::Resize(cols, rows)) => {
                        tracing::info!(cols, rows, "pty resize requested");
                        if let Err(e) = write_half.window_change(cols, rows, 0, 0).await {
                            tracing::warn!("window_change failed: {e}");
                        }
                    }
                    Some(SessionCommand::ForwardChannel { rule_id, stream, target_host, target_port, originator }) => {
                        let handle = &handle;
                        match handle
                            .channel_open_direct_tcpip(&target_host, target_port as u32, &originator, 0)
                            .await
                        {
                            Ok(channel) => {
                                tracing::info!(%rule_id, %target_host, target_port, "forward channel opened");
                                tokio::spawn(async move {
                                    let mut stream = stream;
                                    let mut remote = channel.into_stream();
                                    let _ = tokio::io::copy_bidirectional(&mut stream, &mut remote).await;
                                });
                            }
                            Err(e) => {
                                let message = format!("server refused the tunnel to {target_host}:{target_port}: {e}");
                                tracing::warn!(%rule_id, "{message}");
                                on_event(SessionEvent::ForwardStatus {
                                    rule_id,
                                    ok: false,
                                    message,
                                });
                            }
                        }
                    }
                    Some(SessionCommand::RemoteListen { rule_id, bind_host, bind_port }) => {
                        let handle = &handle;
                        match handle.tcpip_forward(&bind_host, bind_port as u32).await {
                            Ok(_assigned) => {
                                tracing::info!(%rule_id, %bind_host, bind_port, "remote forward listening");
                                on_event(SessionEvent::ForwardStatus {
                                    rule_id,
                                    ok: true,
                                    message: format!("server listening on {bind_host}:{bind_port}"),
                                });
                            }
                            Err(e) => {
                                let message = format!("server refused to listen on {bind_host}:{bind_port}: {e}");
                                tracing::warn!(%rule_id, "{message}");
                                on_event(SessionEvent::ForwardStatus {
                                    rule_id,
                                    ok: false,
                                    message,
                                });
                            }
                        }
                    }
                    Some(SessionCommand::RemoteCancel { rule_id, bind_host, bind_port }) => {
                        let handle = &handle;
                        if let Err(e) = handle.cancel_tcpip_forward(&bind_host, bind_port as u32).await {
                            tracing::warn!(%rule_id, "cancel_tcpip_forward failed: {e}");
                        }
                        on_event(SessionEvent::ForwardStatus {
                            rule_id,
                            ok: true,
                            message: format!("server stopped listening on {bind_host}:{bind_port}"),
                        });
                    }
                }
            }
            msg = msg_rx.recv() => {
                match msg {
                    None => {
                        tracing::info!("channel reader ended");
                        if reason.is_none() {
                            reason = Some("connection lost".into());
                        }
                        break;
                    }
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                        out.extend_from_slice(&data);
                        if out.len() >= COALESCE_LIMIT || last_flush.elapsed() >= COALESCE_WINDOW {
                            on_event(SessionEvent::Data(std::mem::take(&mut out)));
                            last_flush = Instant::now();
                        }
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        tracing::info!(exit_status, "remote shell reported exit status");
                        exit_code = Some(exit_status);
                    }
                    Some(ChannelMsg::Close) => {
                        reason = Some(match exit_code {
                            Some(0) => "session ended".into(),
                            Some(code) => format!("session ended with exit code {code}"),
                            None => "connection closed by server".into(),
                        });
                        break;
                    }
                    _ => {}
                }
            }
            // Coalescing timer: small output that never gets followed by
            // more data must still reach the terminal within one window.
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(
                last_flush + COALESCE_WINDOW,
            )) => {
                if !out.is_empty() {
                    on_event(SessionEvent::Data(std::mem::take(&mut out)));
                }
                last_flush = Instant::now();
            }
        }
    }

    // Drain whatever is left before reporting the close.
    if !out.is_empty() {
        on_event(SessionEvent::Data(std::mem::take(&mut out)));
    }
    // Best-effort cleanup; the connection may already be gone.
    let _ = write_half.eof().await;
    let _ = write_half.close().await;
    let _ = handle
        .disconnect(russh::Disconnect::ByApplication, "", "english")
        .await;
    on_event(SessionEvent::Status(SessionState::Closed {
        reason: reason.unwrap_or_else(|| "connection lost".into()),
    }));
}
