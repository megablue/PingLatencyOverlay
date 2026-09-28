use std::net::{IpAddr, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::config::ProbeConfig;

/// Measure the round-trip latency for one probe. Returns `None` on timeout or
/// any error (a non-response).
pub async fn measure(probe: &ProbeConfig, timeout_ms: u32) -> Option<u32> {
    match probe {
        ProbeConfig::Icmp { host } => icmp(host, timeout_ms).await,
        ProbeConfig::Tcp { host, port } => tcp(host, *port, timeout_ms).await,
    }
}

/// ICMP echo using the Win32 `IcmpSendEcho2` API (via `ping-rs`), which does not
/// require Administrator privileges.
async fn icmp(host: &str, timeout_ms: u32) -> Option<u32> {
    let host = host.to_string();
    let timeout = Duration::from_millis(timeout_ms as u64);
    tokio::task::spawn_blocking(move || {
        let addr = resolve_ipv4(&host)?;
        let data = [0u8; 32];
        let options = ping_rs::PingOptions {
            ttl: 128,
            dont_fragment: false,
        };
        match ping_rs::send_ping(&addr, timeout, &data, Some(&options)) {
            Ok(reply) => Some(reply.rtt),
            Err(_) => None,
        }
    })
    .await
    .ok()
    .flatten()
}

/// TCP connect latency: time to complete the handshake.
async fn tcp(host: &str, port: u16, timeout_ms: u32) -> Option<u32> {
    let start = Instant::now();
    let connect = tokio::net::TcpStream::connect((host, port));
    match tokio::time::timeout(Duration::from_millis(timeout_ms as u64), connect).await {
        Ok(Ok(_stream)) => Some(start.elapsed().as_millis() as u32),
        _ => None,
    }
}

/// Resolve a host to an IPv4 address (ICMP here is IPv4-only). Accepts literal
/// IPs and DNS names.
fn resolve_ipv4(host: &str) -> Option<IpAddr> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return ip.is_ipv4().then_some(ip);
    }
    (host, 0)
        .to_socket_addrs()
        .ok()?
        .find(|addr| addr.is_ipv4())
        .map(|addr| addr.ip())
}
