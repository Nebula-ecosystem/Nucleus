//! Socket address conversions (Windows).
//!
//! Utilities for converting between Rust’s [`SocketAddr`] and the
//! Win32 `SOCKADDR_STORAGE` structure used by WinSock.
//!
//! Three functions are provided:
//!
//! | Function | Direction |
//! |---|---|
//! | [`sys_parse_sockaddr`] | `&str` (“host:port”) → `SOCKADDR_STORAGE` |
//! | [`sockaddr_storage_to_socketaddr`] | `SOCKADDR_STORAGE` → [`SocketAddr`] |
//! | [`socketaddr_to_storage`] | [`SocketAddr`] → `SOCKADDR_STORAGE` |
//!
//! Both IPv4 (`AF_INET`) and IPv6 (`AF_INET6`) families are
//! supported.  Any other family produces an error.
//!
//! # Windows-specific details
//!
//! * `SOCKADDR_IN` / `SOCKADDR_IN6` use the same byte layout as
//!   their POSIX counterparts, but the struct field names differ
//!   (e.g. `S_un.S_addr` instead of `s_addr`).
//! * FlowInfo and ScopeId are always set to `0` when converting from
//!   a `SOCKADDR_IN6` because Cadentis does not use scoped IPv6
//!   addresses.

use std::io;
use std::mem;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::str::FromStr;

use windows_sys::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, SOCKADDR_IN, SOCKADDR_IN6, SOCKADDR_STORAGE,
};

/// Parse a `"host:port"` string into a `SOCKADDR_STORAGE`.
///
/// The string is first parsed into a [`SocketAddr`] via the standard
/// library, then converted to the Win32 representation with
/// [`socketaddr_to_storage`].
///
/// # Arguments
///
/// * `address` — a string of the form `"127.0.0.1:8080"` or
///   `"[::1]:443"`.
///
/// # Returns
///
/// A `(SOCKADDR_STORAGE, i32)` tuple ready to pass to `bind` or
/// `connect`.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidInput`] if the string cannot be
/// parsed as a valid socket address.
pub fn sys_parse_sockaddr(address: &str) -> io::Result<(SOCKADDR_STORAGE, i32)> {
    let addr = SocketAddr::from_str(address)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid socket addr"))?;
    Ok(socketaddr_to_storage(&addr))
}

/// Convert a `SOCKADDR_STORAGE` into a Rust [`SocketAddr`].
///
/// Inspects `ss_family` to determine whether the storage contains a
/// `SOCKADDR_IN` (IPv4) or `SOCKADDR_IN6` (IPv6), then reinterprets
/// the bytes accordingly.
///
/// # Arguments
///
/// * `storage` — a populated `SOCKADDR_STORAGE`, typically filled by
///   `accept` or `getsockname`.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidData`] if `ss_family` is neither
/// `AF_INET` nor `AF_INET6`.
pub fn sockaddr_storage_to_socketaddr(storage: &SOCKADDR_STORAGE) -> io::Result<SocketAddr> {
    unsafe {
        match storage.ss_family {
            AF_INET => {
                let sin = &*(storage as *const _ as *const SOCKADDR_IN);
                let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.S_un.S_addr));
                Ok(SocketAddr::V4(SocketAddrV4::new(
                    ip,
                    u16::from_be(sin.sin_port),
                )))
            }
            AF_INET6 => {
                let sin6 = &*(storage as *const _ as *const SOCKADDR_IN6);
                let ip = Ipv6Addr::from(sin6.sin6_addr.u.Byte);
                Ok(SocketAddr::V6(SocketAddrV6::new(
                    ip,
                    u16::from_be(sin6.sin6_port),
                    0,
                    0,
                )))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported family",
            )),
        }
    }
}

/// Convert a Rust [`SocketAddr`] into a `SOCKADDR_STORAGE`.
///
/// Produces a zeroed `SOCKADDR_STORAGE` and fills the appropriate
/// `SOCKADDR_IN` or `SOCKADDR_IN6` overlay depending on the address
/// family.  The returned `i32` indicates the actual byte length of
/// the populated structure (required by `bind` / `connect`).
///
/// # Arguments
///
/// * `addr` — a Rust socket address (v4 or v6).
///
/// # Returns
///
/// A `(SOCKADDR_STORAGE, i32)` tuple.
pub fn socketaddr_to_storage(addr: &SocketAddr) -> (SOCKADDR_STORAGE, i32) {
    let mut storage: SOCKADDR_STORAGE = unsafe { mem::zeroed() };
    match addr {
        SocketAddr::V4(v4) => {
            let sa = unsafe { &mut *(&mut storage as *mut _ as *mut SOCKADDR_IN) };
            sa.sin_family = AF_INET;
            sa.sin_port = v4.port().to_be();
            sa.sin_addr.S_un.S_addr = u32::from(*v4.ip()).to_be();
            (storage, mem::size_of::<SOCKADDR_IN>() as i32)
        }
        SocketAddr::V6(v6) => {
            let sa = unsafe { &mut *(&mut storage as *mut _ as *mut SOCKADDR_IN6) };
            sa.sin6_family = AF_INET6;
            sa.sin6_port = v6.port().to_be();
            sa.sin6_addr.u.Byte = v6.ip().octets();
            sa.Anonymous.sin6_scope_id = v6.scope_id();
            (storage, mem::size_of::<SOCKADDR_IN6>() as i32)
        }
    }
}
