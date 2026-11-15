//! Host reachability (PRD F15): a quick TCP probe per connection, with the
//! result shown as a small dot in the connection list. This is a
//! reachability check only — it does not authenticate, so a server that is
//! up but rejecting keys still shows as reachable.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Result of one TCP probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostProbe {
    /// TCP connect succeeded; latency in milliseconds.
    Up { latency_ms: u32 },
    /// Connect failed or timed out.
    Down,
}

/// Probe a host:port. Any failure (refused, DNS, timeout) is simply `Down` —
/// the UI wants an honest dot, not an error dialog.
pub async fn probe(host: &str, port: u16, timeout: Duration) -> HostProbe {
    let started = std::time::Instant::now();
    let attempt = tokio::time::timeout(timeout, tokio::net::TcpStream::connect((host, port))).await;
    match attempt {
        Ok(Ok(_stream)) => HostProbe::Up {
            latency_ms: started.elapsed().as_millis().min(u32::MAX as u128) as u32,
        },
        _ => HostProbe::Down,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn probe_reports_up_for_a_listening_port() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        // Accept one connection so the handshake completes; dropping the
        // future without accept still completes TCP on loopback.
        let task = tokio::spawn(async move {
            let _ = listener.accept().await;
        });
        let result = probe("127.0.0.1", port, Duration::from_secs(2)).await;
        task.abort();
        match result {
            HostProbe::Up { latency_ms } => assert!(latency_ms < 2000),
            other => panic!("expected Up, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn probe_reports_down_for_a_closed_port() {
        // Port 1 on loopback is reliably closed in test environments.
        let result = probe("127.0.0.1", 1, Duration::from_millis(300)).await;
        assert_eq!(result, HostProbe::Down);
    }

    #[tokio::test]
    async fn probe_reports_down_on_timeout() {
        // A non-routable address makes connect hang until the timeout.
        let result = probe("10.255.255.1", 1, Duration::from_millis(200)).await;
        assert_eq!(result, HostProbe::Down);
    }
}
