//! Socket address conversions (Unix).
//!
//! Utilities for converting between Rust’s [`SocketAddr`] and the C
//! `sockaddr_storage` structure used by the BSD socket API.
//!
//! Three functions are provided:
//!
//! | Function | Direction |
//! |---|---|
//! | [`sys_parse_sockaddr`] | `&str` (“host:port”) → `sockaddr_storage` |
//! | [`sockaddr_storage_to_socketaddr`] | `sockaddr_storage` → [`SocketAddr`] |
//! | [`socketaddr_to_storage`] | [`SocketAddr`] → `sockaddr_storage` |
//!
//! Both IPv4 (`AF_INET`) and IPv6 (`AF_INET6`) families are
//! supported.  Any other family produces an error.

use libc::{AF_INET, AF_INET6, sockaddr_in, sockaddr_in6, sockaddr_storage, socklen_t};
use std::io;
use std::mem;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::str::FromStr;

/// Parse a `"host:port"` string into a `sockaddr_storage`.
///
/// The string is first parsed into a [`SocketAddr`] via the standard
/// library, then converted to the C representation with
/// [`socketaddr_to_storage`].
///
/// # Arguments
///
/// * `address` — a string of the form `"127.0.0.1:8080"` or
///   `"[::1]:443"`.
///
/// # Returns
///
/// A `(sockaddr_storage, socklen_t)` tuple ready to pass to
/// `bind(2)` or `connect(2)`.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidInput`] if the string cannot be
/// parsed as a valid socket address.
pub fn sys_parse_sockaddr(address: &str) -> io::Result<(sockaddr_storage, socklen_t)> {
    let addr = SocketAddr::from_str(address)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid socket addr"))?;

    Ok(socketaddr_to_storage(&addr))
}

/// Convert a `sockaddr_storage` into a Rust [`SocketAddr`].
///
/// Inspects `ss_family` to determine whether the storage contains a
/// `sockaddr_in` (IPv4) or `sockaddr_in6` (IPv6), then reinterprets
/// the bytes accordingly.
///
/// # Arguments
///
/// * `storage` — a populated `sockaddr_storage`, typically filled by
///   `accept(2)` or `getsockname(2)`.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidData`] if `ss_family` is neither
/// `AF_INET` nor `AF_INET6`.
pub fn sockaddr_storage_to_socketaddr(storage: &sockaddr_storage) -> io::Result<SocketAddr> {
    use libc::c_int;

    match storage.ss_family as c_int {
        AF_INET => {
            let addr = unsafe { &*(storage as *const _ as *const sockaddr_in) };
            let ip = Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr));
            let port = u16::from_be(addr.sin_port);

            Ok(SocketAddr::V4(SocketAddrV4::new(ip, port)))
        }

        AF_INET6 => {
            let addr = unsafe { &*(storage as *const _ as *const sockaddr_in6) };
            let ip = Ipv6Addr::from(addr.sin6_addr.s6_addr);
            let port = u16::from_be(addr.sin6_port);

            Ok(SocketAddr::V6(SocketAddrV6::new(
                ip,
                port,
                addr.sin6_flowinfo,
                addr.sin6_scope_id,
            )))
        }

        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported address family",
        )),
    }
}

/// Convert a Rust [`SocketAddr`] into a `sockaddr_storage`.
///
/// Produces a zeroed `sockaddr_storage` and fills the appropriate
/// `sockaddr_in` or `sockaddr_in6` overlay depending on the address
/// family.  The returned `socklen_t` indicates the actual byte length
/// of the populated structure (required by `bind` / `connect`).
///
/// # Arguments
///
/// * `addr` — a Rust socket address (v4 or v6).
///
/// # Returns
///
/// A `(sockaddr_storage, socklen_t)` tuple.
pub fn socketaddr_to_storage(addr: &SocketAddr) -> (sockaddr_storage, socklen_t) {
    let mut storage: sockaddr_storage = unsafe { mem::zeroed() };

    match addr {
        SocketAddr::V4(v4) => {
            let sa = unsafe { &mut *(&mut storage as *mut _ as *mut sockaddr_in) };
            sa.sin_family = AF_INET as _;
            sa.sin_port = v4.port().to_be();
            sa.sin_addr.s_addr = u32::from(*v4.ip()).to_be();

            (storage, mem::size_of::<sockaddr_in>() as socklen_t)
        }

        SocketAddr::V6(v6) => {
            let sa = unsafe { &mut *(&mut storage as *mut _ as *mut sockaddr_in6) };
            sa.sin6_family = AF_INET6 as _;
            sa.sin6_port = v6.port().to_be();
            sa.sin6_addr.s6_addr = v6.ip().octets();
            sa.sin6_flowinfo = v6.flowinfo();
            sa.sin6_scope_id = v6.scope_id();

            (storage, mem::size_of::<sockaddr_in6>() as socklen_t)
        }
    }
}
