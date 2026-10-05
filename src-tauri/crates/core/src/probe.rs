use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::config::ProbeConfig;

/// Measure the round-trip latency for one probe. Returns `None` on timeout or
/// any error (a non-response).
///
/// This is the one-shot form, and both arms resolve on every call. A probe task
/// caches the address instead (the address cache in `probes`), so a slow
/// lookup cannot sit between two samples.
pub async fn measure(probe: &ProbeConfig, timeout_ms: u32) -> Option<u32> {
    match probe {
        ProbeConfig::Icmp { host } => {
            let addr = lookup_ipv4(host).await?;
            ping_ipv4(addr, timeout_ms).await
        }
        ProbeConfig::Tcp { host, port } => {
            let addr = lookup_ipv4(host).await?;
            connect_ipv4(addr, *port, timeout_ms).await
        }
    }
}

/// Resolve a host to the IPv4 address a probe will send to. Literal IPs are
/// accepted. The blocking lookup runs on a blocking thread, but it can still
/// take seconds on a cold DNS cache, so a probe task caches the result rather
/// than calling this per tick. TCP is IPv4-only for the same reason ICMP is:
/// one cached address per task.
pub async fn lookup_ipv4(host: &str) -> Option<IpAddr> {
    let host = host.to_owned();
    tokio::task::spawn_blocking(move || resolve_ipv4(&host))
        .await
        .ok()
        .flatten()
}

/// ICMP echo to an already-resolved address, using the Win32 `IcmpSendEcho2`
/// API (via `ping-rs`), which does not require Administrator privileges.
pub async fn ping_ipv4(addr: IpAddr, timeout_ms: u32) -> Option<u32> {
    let timeout = Duration::from_millis(timeout_ms as u64);
    tokio::task::spawn_blocking(move || {
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

/// TCP connect latency to an already-resolved address: time to complete the
/// handshake.
///
/// A `SocketAddr` is not resolved by `TcpStream::connect`, so no name lookup
/// runs inside the connect the timeout covers.
pub async fn connect_ipv4(addr: IpAddr, port: u16, timeout_ms: u32) -> Option<u32> {
    let start = Instant::now();
    let connect = tokio::net::TcpStream::connect(SocketAddr::new(addr, port));
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
