use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::config::ProbeConfig;

/// Measure the round-trip latency for one probe. Returns `None` on timeout or
/// any error (a non-response).
///
/// This is the one-shot form, and both arms resolve on every call. A probe task
/// caches the address instead (the address cache in `probes`), so a slow
/// lookup cannot sit between two samples. `source` is the local IPv4 address
/// the probe leaves through — the adapter a host picked; `None` is the
/// system's own choice of interface, which is what every probe did before
/// hosts could pick an adapter.
///
/// The source address is a parameter, not a lookup in here: deciding what an
/// adapter's current address is belongs to `adapters`, and a caller that
/// already resolved it should not have it resolved again.
pub async fn measure(
    probe: &ProbeConfig,
    timeout_ms: u32,
    source: Option<Ipv4Addr>,
) -> Option<u32> {
    match probe {
        ProbeConfig::Icmp { host } => {
            let addr = lookup_ipv4(host).await?;
            ping_ipv4(addr, timeout_ms, source).await
        }
        ProbeConfig::Tcp { host, port } => {
            let addr = lookup_ipv4(host).await?;
            connect_ipv4(addr, *port, timeout_ms, source).await
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

/// ICMP echo to an already-resolved address, using the Win32
/// `IcmpSendEcho2Ex` API, which does not require Administrator privileges and
/// can take a source address. `source` of `Some(address)` makes the request
/// leave through the adapter that owns it; `None` passes the "any" address,
/// which makes Windows route it exactly as the older `IcmpSendEcho2` call did.
///
/// `IcmpSendEcho2Ex` lives here as a module-local `extern "system"` block
/// rather than a dependency: the `ping-rs` crate wraps the API that cannot
/// bind a source address, and this crate is deliberately free of Windows SDK
/// binding crates (see `lib.rs`).
pub async fn ping_ipv4(addr: IpAddr, timeout_ms: u32, source: Option<Ipv4Addr>) -> Option<u32> {
    let destination = match addr {
        IpAddr::V4(addr) => addr,
        // The resolver is IPv4-only for the same reason: one code path.
        IpAddr::V6(_) => return None,
    };
    tokio::task::spawn_blocking(move || icmp::echo(destination, source, timeout_ms))
        .await
        .ok()
        .flatten()
}

/// TCP connect latency to an already-resolved address: time to complete the
/// handshake.
///
/// A `SocketAddr` is not resolved by the connect, so no name lookup runs
/// inside the timeout. `source` of `Some(address)` binds the socket to that
/// local address first, so the handshake leaves through the adapter that owns
/// it; the local port is left to the system. `None` is the plain connect this
/// has always made.
pub async fn connect_ipv4(
    addr: IpAddr,
    port: u16,
    timeout_ms: u32,
    source: Option<Ipv4Addr>,
) -> Option<u32> {
    let destination = SocketAddr::new(addr, port);
    let start = Instant::now();
    let connect = async {
        let stream = match source {
            Some(local) => {
                let socket = tokio::net::TcpSocket::new_v4()?;
                socket.bind(SocketAddr::new(IpAddr::V4(local), 0))?;
                socket.connect(destination).await?
            }
            None => tokio::net::TcpStream::connect(destination).await?,
        };
        Ok::<_, std::io::Error>(stream)
    };
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

/// The Win32 `IcmpSendEcho2Ex` call and the reply header it fills in.
#[cfg(windows)]
mod icmp {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::net::Ipv4Addr;
    use std::ptr::null_mut;

    /// The request payload, the same 32 bytes of zeroes the previous
    /// implementation sent.
    const REQUEST_DATA: [u8; 32] = [0u8; 32];

    /// `IP_SUCCESS`: the echo came back.
    const IP_SUCCESS: u32 = 0;

    // Present since Windows 2000, so this links unconditionally.
    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn IcmpCreateFile() -> *mut c_void;
        fn IcmpCloseHandle(handle: *mut c_void) -> i32;
        fn IcmpSendEcho2Ex(
            handle: *mut c_void,
            event: *mut c_void,
            apc_routine: *mut c_void,
            apc_context: *mut c_void,
            source_address: u32,
            destination_address: u32,
            request_data: *const c_void,
            request_size: u16,
            request_options: *const IpOptionInformation,
            reply_buffer: *mut c_void,
            reply_size: u32,
            timeout: u32,
        ) -> u32;
    }

    /// `IP_OPTION_INFORMATION`, with the TTL and flags the previous
    /// implementation sent: TTL 128, no fragmentation flag.
    #[repr(C)]
    struct IpOptionInformation {
        ttl: u8,
        tos: u8,
        flags: u8,
        options_size: u8,
        options_data: *mut u8,
    }

    /// The prefix of `ICMP_ECHO_REPLY` this module reads. The fields after
    /// `RoundTripTime` are never touched, but they are declared: the API
    /// rejects a reply buffer smaller than this struct, the request echoed
    /// back and eight bytes of ICMP slack, so `size_of` has to be the real
    /// header's. The real header also carries a `Data` pointer and an
    /// `IP_OPTION_INFORMATION`, which is what makes it 40 bytes on this
    /// architecture rather than the 16 the first three fields occupy.
    #[repr(C)]
    struct IcmpEchoReply {
        _address: u32,
        status: u32,
        round_trip_time: u32,
        _data_size: u16,
        _reserved: u16,
        _data: *mut c_void,
        _options: IpOptionInformation,
    }

    /// One echo request. `Some(rtt)` when a reply came back inside the
    /// timeout; `None` for a timeout, an unreachable destination, or any
    /// other failure — to the sample that gets written, those are the same
    /// non-response.
    pub fn echo(destination: Ipv4Addr, source: Option<Ipv4Addr>, timeout_ms: u32) -> Option<u32> {
        let handle = unsafe { IcmpCreateFile() };
        if handle.is_null() {
            return None;
        }
        let result = unsafe { send(handle, destination, source, timeout_ms) };
        unsafe {
            IcmpCloseHandle(handle);
        }
        result
    }

    /// The `IcmpSendEcho2Ex` call itself, split out so the handle is closed on
    /// every path.
    unsafe fn send(
        handle: *mut c_void,
        destination: Ipv4Addr,
        source: Option<Ipv4Addr>,
        timeout_ms: u32,
    ) -> Option<u32> {
        let options = IpOptionInformation {
            ttl: 128,
            tos: 0,
            flags: 0,
            options_size: 0,
            options_data: null_mut(),
        };
        // The documentation's minimum for the reply buffer: the reply header,
        // the request echoed back, plus the ICMP and IP overhead's slack.
        let reply_size = size_of::<IcmpEchoReply>() + REQUEST_DATA.len() + 8;
        let mut reply = vec![0u8; reply_size];
        let sent = unsafe {
            IcmpSendEcho2Ex(
                handle,
                null_mut(),
                null_mut(),
                null_mut(),
                source.map_or(0, address_bytes),
                address_bytes(destination),
                REQUEST_DATA.as_ptr() as *const c_void,
                REQUEST_DATA.len() as u16,
                &options,
                reply.as_mut_ptr() as *mut c_void,
                reply_size as u32,
                timeout_ms,
            )
        };
        if sent == 0 {
            return None;
        }
        let reply = unsafe { &*(reply.as_ptr() as *const IcmpEchoReply) };
        if reply.status != IP_SUCCESS {
            return None;
        }
        Some(reply.round_trip_time)
    }

    /// An IPv4 address as the `IPAddr` the API wants.
    ///
    /// `IPAddr` is the address's bytes in network order, read as a native
    /// integer — which is what `inet_addr` hands the same call in C: the
    /// bytes `[1, 2, 3, 4]` become whatever `u32` those bytes are on this
    /// machine. `u32::from_be_bytes` would send the address reversed on
    /// every architecture this ships on.
    fn address_bytes(addr: Ipv4Addr) -> u32 {
        u32::from_ne_bytes(addr.octets())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn an_address_keeps_its_bytes_in_network_order() {
            // 1.2.3.4 must be the four bytes 1, 2, 3, 4 in memory.
            let encoded = address_bytes(Ipv4Addr::new(1, 2, 3, 4));
            assert_eq!(encoded.to_ne_bytes(), [1, 2, 3, 4]);
        }
    }
}

/// The non-Windows form, for the shape of the module rather than for use: this
/// application is Windows-only (see `AGENTS.md`).
#[cfg(not(windows))]
mod icmp {
    use std::net::Ipv4Addr;

    pub fn echo(
        _destination: Ipv4Addr,
        _source: Option<Ipv4Addr>,
        _timeout_ms: u32,
    ) -> Option<u32> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// The source-bind path end to end: a listener and a connect that both
    /// live on loopback, with the socket bound to loopback first. No external
    /// network and no second adapter, so this pins the bind-and-connect
    /// sequence rather than the machine it runs on.
    #[tokio::test]
    async fn a_tcp_connect_bound_to_loopback_reaches_a_loopback_listener() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("loopback listener");
        let port = listener.local_addr().expect("listener address").port();

        let latency = connect_ipv4(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            port,
            1_000,
            Some(Ipv4Addr::LOCALHOST),
        )
        .await;

        assert!(
            latency.is_some(),
            "a loopback connect bound to loopback must complete"
        );
    }

    /// The unbound arm still connects.
    #[tokio::test]
    async fn a_tcp_connect_without_a_source_still_reaches_loopback() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("loopback listener");
        let port = listener.local_addr().expect("listener address").port();

        let latency = connect_ipv4(IpAddr::V4(Ipv4Addr::LOCALHOST), port, 1_000, None).await;

        assert!(latency.is_some());
    }

    /// A port with nothing behind it is a failure, not a hang.
    #[tokio::test]
    async fn a_tcp_connect_to_a_closed_port_fails() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("loopback listener");
        let port = listener.local_addr().expect("listener address").port();
        drop(listener);

        let latency = connect_ipv4(IpAddr::V4(Ipv4Addr::LOCALHOST), port, 250, None).await;

        assert!(latency.is_none());
    }

    /// The echo path end to end against the one destination every machine
    /// answers: loopback. It pins that the FFI call is made correctly and
    /// that its reply buffer parses, which is exactly what replaced `ping-rs`.
    #[cfg(windows)]
    #[tokio::test]
    async fn an_icmp_echo_to_loopback_answers() {
        let latency = ping_ipv4(IpAddr::V4(Ipv4Addr::LOCALHOST), 1_000, None).await;

        assert!(latency.is_some(), "a loopback echo must answer");
    }

    /// The same echo with the source address bound — the feature's ICMP half.
    #[cfg(windows)]
    #[tokio::test]
    async fn an_icmp_echo_bound_to_loopback_answers() {
        let latency = ping_ipv4(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            1_000,
            Some(Ipv4Addr::LOCALHOST),
        )
        .await;

        assert!(
            latency.is_some(),
            "a loopback echo bound to loopback must answer"
        );
    }
}
