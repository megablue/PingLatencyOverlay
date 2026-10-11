//! Which network adapters the machine has, and which one a host's probe
//! leaves through.
//!
//! The same split as `monitors`: [`NetworkAdapter`] describes one adapter with
//! no Win32 in it, [`enumerate`] is the only function that asks Windows
//! anything, and the rule that turns a raw row into a listed adapter is a pure
//! function — so the filter can be tested against hand-written rows rather
//! than against whichever adapters the test machine happens to have.
//!
//! ## Why the GUID, and why the address
//!
//! A profile stores the adapter's GUID string (`AdapterName`, `{...}`), not
//! its address: the address is a DHCP lease that changes, and an index into
//! the enumeration order is whatever order Windows brought adapters up in.
//! The address is read back from the GUID at probe time, which is also what
//! lets a lease change heal on the next sample. The cost of the choice is
//! that a profile moved to another machine names GUIDs that do not exist
//! there; a host with a missing adapter is one that cannot probe, which the
//! editor shows as "Not connected".
//!
//! ## What is listed
//!
//! Only adapters that are up and have a usable IPv4 unicast address: a probe
//! binds to that address (`IcmpSendEcho2Ex`'s `SourceAddress`, or a TCP
//! socket bound before connect), so an adapter with no address is one nothing
//! can be bound to. IPv4 only, like the probes themselves; an adapter with
//! several IPv4 addresses is bound through the first.

use std::net::Ipv4Addr;

/// One network adapter with an IPv4 address, as far as anything here cares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkAdapter {
    /// The adapter's GUID string, from `AdapterName`. This is what a host
    /// stores; it survives reboots, DHCP renewals and a cable moving ports.
    pub id: String,
    /// The Windows name — "Wi-Fi", "Ethernet", a VPN tunnel's name.
    pub name: String,
    /// The first usable IPv4 unicast address, the one a probe binds to.
    pub ipv4: Ipv4Addr,
}

impl NetworkAdapter {
    /// The label the editor shows for this adapter.
    ///
    /// The address is in the label because two adapters can carry the same
    /// name ("Ethernet" twice, or two VPNs), and because it is the part that
    /// changes when a lease does.
    pub fn label(&self) -> String {
        if self.name.is_empty() {
            format!("{} ({})", self.id, self.ipv4)
        } else {
            format!("{} ({})", self.name, self.ipv4)
        }
    }
}

/// Every adapter that is up and has a usable IPv4 address.
///
/// Sorted by name, so the editor's list is stable across calls — Windows
/// enumerates in no particular order, and a list that reshuffles per frame is
/// one a pointer can hover over the wrong row in.
pub fn enumerate() -> Vec<NetworkAdapter> {
    #[cfg(windows)]
    {
        win::enumerate()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// The adapter's current IPv4 address, by the GUID a host stores.
///
/// `None` means the adapter is not attached, not up, or has no IPv4 address
/// right now — in every case there is nothing for a probe to bind to, and a
/// host pointed at it is a host that cannot probe.
pub fn ipv4_for(id: &str) -> Option<Ipv4Addr> {
    enumerate()
        .into_iter()
        .find(|adapter| adapter.id.eq_ignore_ascii_case(id))
        .map(|adapter| adapter.ipv4)
}

/// The rule that decides what is listed, as a pure function over one raw row.
///
/// `up` is `OperStatus == IfOperStatusUp`; `ipv4` is the adapter's first
/// unicast address. `0.0.0.0` (the API's way of saying "no address") and the
/// loopback address are treated as no address, because neither can carry a
/// probe.
fn adapter_from_parts(
    id: &str,
    name: &str,
    up: bool,
    ipv4: Option<Ipv4Addr>,
) -> Option<NetworkAdapter> {
    let id = id.trim();
    if !up || id.is_empty() {
        return None;
    }
    let ipv4 = ipv4.filter(|addr| !addr.is_unspecified() && !addr.is_loopback())?;
    Some(NetworkAdapter {
        id: id.to_owned(),
        name: name.trim().to_owned(),
        ipv4,
    })
}

#[cfg(windows)]
mod win {
    use super::{adapter_from_parts, NetworkAdapter};
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::net::Ipv4Addr;

    /// `AF_INET`.
    const AF_INET: u32 = 2;
    /// `GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER`:
    /// the walk only reads unicast addresses, so asking Windows to build the
    /// other lists is work thrown away on every call.
    const GAA_FLAGS: u32 = 0x2 | 0x4 | 0x8;
    const ERROR_SUCCESS: u32 = 0;
    const ERROR_BUFFER_OVERFLOW: u32 = 111;
    /// `IfOperStatusUp`, the one `IF_OPER_STATUS` a probe can bind against.
    const IF_OPER_STATUS_UP: u32 = 1;
    /// The starting size the API's own documentation suggests.
    const INITIAL_BUFFER_SIZE: u32 = 15 * 1024;
    /// Enough attempts for a machine whose adapter count grows between calls;
    /// a pathological loop would otherwise grow the buffer forever.
    const MAX_ATTEMPTS: u32 = 4;

    // `GetAdaptersAddresses` lives in `iphlpapi`, the same unbound-to-a-crate
    // corner of the Windows SDK the other modules declare for themselves.
    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn GetAdaptersAddresses(
            family: u32,
            flags: u32,
            reserved: *mut c_void,
            addresses: *mut IpAdapterAddresses,
            size: *mut u32,
        ) -> u32;
    }

    /// The prefix of `IP_ADAPTER_ADDRESSES_LH` this module reads.
    ///
    /// Declared only through `OperStatus`: the fields after it (`Ipv6IfIndex`,
    /// `ZoneIndices`, `FirstPrefix`, …) are never touched, and the API is
    /// given the byte buffer's size rather than this struct's, so the tail is
    /// free to be any layout. The fields between the ones read are kept as
    /// underscorized members so the offsets stay the SDK's.
    #[repr(C)]
    struct IpAdapterAddresses {
        /// `union { ULONGLONG Alignment; struct { ULONG Length; IF_INDEX IfIndex } }`.
        _length_and_index: u64,
        next: *mut IpAdapterAddresses,
        /// The GUID string, ANSI and NUL-terminated.
        adapter_name: *const u8,
        first_unicast: *const IpAdapterUnicastAddress,
        _first_anycast: *const c_void,
        _first_multicast: *const c_void,
        _first_dns_server: *const c_void,
        _dns_suffix: *const u16,
        _description: *const u16,
        /// The Windows name, UTF-16 and NUL-terminated.
        friendly_name: *const u16,
        _physical_address: [u8; 8],
        _physical_address_length: u32,
        _flags: u32,
        _mtu: u32,
        _if_type: u32,
        /// `IF_OPER_STATUS`.
        oper_status: u32,
    }

    /// The prefix of `IP_ADAPTER_UNICAST_ADDRESS_LH` this module reads.
    #[repr(C)]
    struct IpAdapterUnicastAddress {
        /// `union { ULONGLONG Alignment; struct { ULONG Length; DWORD Flags } }`.
        _length_and_flags: u64,
        next: *const IpAdapterUnicastAddress,
        address: SocketAddress,
    }

    /// `SOCKET_ADDRESS`.
    #[repr(C)]
    struct SocketAddress {
        sockaddr: *const SockaddrIn,
        sockaddr_length: i32,
    }

    /// `sockaddr_in`, as far as a unicast entry needs it.
    #[repr(C)]
    struct SockaddrIn {
        family: u16,
        _port: u16,
        /// The address bytes in network order, at offset 4.
        addr: [u8; 4],
        _zero: [u8; 8],
    }

    pub fn enumerate() -> Vec<NetworkAdapter> {
        let mut buffer: Vec<u8> = Vec::new();
        let mut size = INITIAL_BUFFER_SIZE;
        let mut ready = false;
        for _ in 0..MAX_ATTEMPTS {
            buffer.resize(size as usize, 0);
            let result = unsafe {
                GetAdaptersAddresses(
                    AF_INET,
                    GAA_FLAGS,
                    std::ptr::null_mut(),
                    buffer.as_mut_ptr() as *mut IpAdapterAddresses,
                    &mut size,
                )
            };
            match result {
                ERROR_SUCCESS => {
                    ready = true;
                    break;
                }
                // `size` now holds what the call needs; go round again.
                ERROR_BUFFER_OVERFLOW => continue,
                // `ERROR_NO_DATA` (no adapter of this family) and every other
                // error end the same way, with nothing listed.
                _ => return Vec::new(),
            }
        }
        if !ready {
            return Vec::new();
        }

        let mut listed = Vec::new();
        unsafe {
            let mut current = buffer.as_ptr() as *const IpAdapterAddresses;
            while !current.is_null() {
                let entry = &*current;
                let adapter = adapter_from_parts(
                    &c_string(entry.adapter_name),
                    &wide_string(entry.friendly_name),
                    entry.oper_status == IF_OPER_STATUS_UP,
                    first_ipv4(entry.first_unicast),
                );
                if let Some(adapter) = adapter {
                    listed.push(adapter);
                }
                current = entry.next;
            }
        }
        listed.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });
        listed.dedup_by(|a, b| a.id == b.id);
        listed
    }

    /// The first `AF_INET` unicast address in the chain, if there is one.
    ///
    /// The family check is belt and braces: with `AF_INET` requested the API
    /// only fills the list with IPv4 addresses, but reading a `sockaddr` is
    /// reading memory through a length the caller has not verified, so both
    /// the length and the family are checked before the bytes are taken.
    unsafe fn first_ipv4(mut entry: *const IpAdapterUnicastAddress) -> Option<Ipv4Addr> {
        while !entry.is_null() {
            let unicast = unsafe { &*entry };
            let sockaddr = unicast.address.sockaddr;
            if !sockaddr.is_null()
                && unicast.address.sockaddr_length >= size_of::<SockaddrIn>() as i32
            {
                let sockaddr = unsafe { &*sockaddr };
                if sockaddr.family == AF_INET as u16 {
                    let [a, b, c, d] = sockaddr.addr;
                    return Some(Ipv4Addr::new(a, b, c, d));
                }
            }
            entry = unicast.next;
        }
        None
    }

    /// A NUL-terminated ANSI string as a `String`.
    unsafe fn c_string(ptr: *const u8) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut len = 0usize;
        while unsafe { *ptr.add(len) } != 0 {
            len += 1;
        }
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) }).into_owned()
    }

    /// A NUL-terminated UTF-16 string as a `String`.
    unsafe fn wide_string(ptr: *const u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut len = 0usize;
        while unsafe { *ptr.add(len) } != 0 {
            len += 1;
        }
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_sockaddr_becomes_the_address_it_holds() {
            let sockaddr = SockaddrIn {
                family: AF_INET as u16,
                _port: 0,
                addr: [192, 168, 1, 42],
                _zero: [0; 8],
            };
            let unicast = IpAdapterUnicastAddress {
                _length_and_flags: 0,
                next: std::ptr::null(),
                address: SocketAddress {
                    sockaddr: &sockaddr,
                    sockaddr_length: size_of::<SockaddrIn>() as i32,
                },
            };
            assert_eq!(
                unsafe { first_ipv4(&unicast) },
                Some(Ipv4Addr::new(192, 168, 1, 42))
            );
        }

        #[test]
        fn a_sockaddr_of_another_family_is_skipped() {
            // `AF_INET6`, which the AF_INET call should never produce.
            let sockaddr = SockaddrIn {
                family: 23,
                _port: 0,
                addr: [0x20, 0x01, 0x0d, 0xb8],
                _zero: [0; 8],
            };
            let unicast = IpAdapterUnicastAddress {
                _length_and_flags: 0,
                next: std::ptr::null(),
                address: SocketAddress {
                    sockaddr: &sockaddr,
                    sockaddr_length: size_of::<SockaddrIn>() as i32,
                },
            };
            assert_eq!(unsafe { first_ipv4(&unicast) }, None);
        }

        #[test]
        fn an_empty_chain_has_no_address() {
            assert_eq!(unsafe { first_ipv4(std::ptr::null()) }, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(id: &str, name: &str, ipv4: &str) -> NetworkAdapter {
        adapter_from_parts(id, name, true, Some(ipv4.parse().unwrap())).unwrap()
    }

    #[test]
    fn only_up_adapters_with_a_usable_address_are_listed() {
        let address = Some("10.0.0.2".parse().unwrap());
        assert!(adapter_from_parts("{a}", "Ethernet", false, address).is_none());
        assert!(adapter_from_parts("  ", "Ethernet", true, address).is_none());
        assert!(adapter_from_parts("{a}", "Ethernet", true, None).is_none());
        assert!(adapter_from_parts("{a}", "Ethernet", true, Some(Ipv4Addr::UNSPECIFIED)).is_none());
        assert!(adapter_from_parts("{a}", "Ethernet", true, Some(Ipv4Addr::LOCALHOST)).is_none());
        assert!(adapter_from_parts("{a}", "Ethernet", true, address).is_some());
    }

    #[test]
    fn the_label_names_the_adapter_and_its_address() {
        assert_eq!(
            listed("{a}", "Wi-Fi", "192.168.1.42").label(),
            "Wi-Fi (192.168.1.42)"
        );
        assert_eq!(listed("{b}", "  ", "10.1.2.3").label(), "{b} (10.1.2.3)");
    }

    #[test]
    fn an_unknown_guid_has_no_address() {
        assert_eq!(ipv4_for("{00000000-0000-0000-0000-000000000000}"), None);
    }

    #[test]
    fn enumeration_is_sorted_and_well_formed() {
        let adapters = enumerate();
        for adapter in &adapters {
            assert!(!adapter.id.trim().is_empty());
            assert!(!adapter.ipv4.is_unspecified());
            assert!(!adapter.ipv4.is_loopback());
        }
        let names: Vec<String> = adapters.iter().map(|a| a.name.to_lowercase()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        let mut ids: Vec<&str> = adapters.iter().map(|a| a.id.as_str()).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count);
    }
}
