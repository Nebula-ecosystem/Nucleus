//! Socket lifecycle and options (Windows).
//!
//! Complete wrappers around the WinSock API for TCP stream sockets.
//! The module covers the full connection lifecycle:
//!
//! ```text
//! sys_socket → sys_bind → sys_listen → sys_accept   (server)
//! sys_socket → sys_connect                           (client)
//! sys_shutdown                                        (teardown)
//! ```
//!
//! Plus option helpers:
//!
//! * [`sys_set_reuseaddr`] — enables `SO_REUSEADDR`.
//! * [`sys_get_socket_error`] — reads `SO_ERROR`.
//! * [`sys_ipv6_is_necessary`] / [`sys_set_v6only`] — IPv6
//!   dual-stack configuration.
//!
//! Every socket created by this module is automatically put into
//! **non-blocking mode** via `ioctlsocket(FIONBIO)`.
//!
//! # WinSock initialisation
//!
//! All public functions call
//! [`ensure_winsock()`](super::utils::ensure_winsock) before issuing
//! any WinSock call, so the caller never needs to initialise WinSock
//! manually.

use std::ffi::c_int;
use std::io;
use std::mem;
use std::net::{Shutdown, SocketAddr};

use windows_sys::Win32::Networking::WinSock::{
    AF_INET as WS_AF_INET, AF_INET6 as WS_AF_INET6, FIONBIO, INVALID_SOCKET, IPPROTO_IPV6,
    IPV6_V6ONLY, SD_BOTH, SD_RECEIVE, SD_SEND, SO_ERROR, SO_REUSEADDR, SOCK_STREAM, SOCKADDR,
    SOCKADDR_STORAGE, SOCKET, SOL_SOCKET, WSAEWOULDBLOCK, accept, bind, connect, getsockname,
    getsockopt, ioctlsocket, listen, setsockopt, shutdown, socket,
};

use super::address::{sockaddr_storage_to_socketaddr, socketaddr_to_storage};
use super::io::RawFd;
use super::utils::ensure_winsock;

/// IPv4 address family constant.
pub const AF_INET: c_int = WS_AF_INET as c_int;

/// IPv6 address family constant.
pub const AF_INET6: c_int = WS_AF_INET6 as c_int;

/// Put a socket into non-blocking mode.
///
/// Calls `ioctlsocket(fd, FIONBIO, &1)`.
///
/// # Arguments
///
/// * `fd` — a WinSock `SOCKET` stored as [`RawFd`].
///
/// # Errors
///
/// Returns the OS error if `ioctlsocket` fails.
pub fn sys_set_nonblocking(fd: RawFd) -> io::Result<()> {
    unsafe {
        let mut nonblocking: u32 = 1;
        if ioctlsocket(fd as SOCKET, FIONBIO, &mut nonblocking) != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

/// Create a non-blocking TCP stream socket.
///
/// Calls `socket(domain, SOCK_STREAM, 0)` and, on success,
/// immediately puts the returned socket into non-blocking mode with
/// [`sys_set_nonblocking`].  WinSock is initialised automatically
/// via [`ensure_winsock`].
///
/// # Arguments
///
/// * `domain` — address family, typically `AF_INET` or `AF_INET6`.
///
/// # Returns
///
/// The new socket as a [`RawFd`].
///
/// # Errors
///
/// Returns the OS error if `socket()` fails (e.g. `WSAEMFILE`) or
/// if setting non-blocking mode fails.
pub fn sys_socket(domain: c_int) -> io::Result<RawFd> {
    ensure_winsock();
    unsafe {
        let fd = socket(domain, SOCK_STREAM, 0);
        if fd == INVALID_SOCKET {
            return Err(io::Error::last_os_error());
        }
        sys_set_nonblocking(fd as RawFd)?;
        Ok(fd as RawFd)
    }
}

/// Bind a socket to a local address.
///
/// Calls WinSock `bind()` with the provided `SOCKADDR_STORAGE` and
/// its byte length.
///
/// # Arguments
///
/// * `fd` — a socket returned by [`sys_socket`].
/// * `addr` — a populated `SOCKADDR_STORAGE` (see
///   [`socketaddr_to_storage`](super::address::socketaddr_to_storage)).
/// * `len` — byte length of the relevant `SOCKADDR_IN` or
///   `SOCKADDR_IN6` inside the storage.
///
/// # Errors
///
/// Returns the OS error if `bind()` fails (e.g. `WSAEADDRINUSE`).
pub fn sys_bind(fd: RawFd, addr: &SOCKADDR_STORAGE, len: i32) -> io::Result<()> {
    ensure_winsock();
    unsafe {
        if bind(fd as SOCKET, addr as *const _ as *const SOCKADDR, len) != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

/// Mark a socket as passive (listening for connections).
///
/// Calls WinSock `listen()` with a backlog of **128**.
///
/// # Arguments
///
/// * `fd` — a bound socket.
///
/// # Errors
///
/// Returns the OS error if `listen()` fails.
pub fn sys_listen(fd: RawFd) -> io::Result<()> {
    ensure_winsock();
    unsafe {
        if listen(fd as SOCKET, 128) != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

/// Accept a new connection on a listening socket.
///
/// Calls WinSock `accept()` and, on success, puts the returned
/// client socket into non-blocking mode.
///
/// # Arguments
///
/// * `fd` — a listening socket.
///
/// # Returns
///
/// A `(client_fd, peer_addr)` tuple.  The `client_fd` is already in
/// non-blocking mode and ready for registration with the poller.
///
/// # Errors
///
/// Returns the OS error if `accept()` fails (e.g. `WSAEWOULDBLOCK`
/// when no connection is pending) or if the client socket cannot be
/// set to non-blocking mode.
pub fn sys_accept(fd: RawFd) -> io::Result<(RawFd, SocketAddr)> {
    ensure_winsock();
    unsafe {
        let mut storage: SOCKADDR_STORAGE = mem::zeroed();
        let mut len = mem::size_of::<SOCKADDR_STORAGE>() as i32;
        let client = accept(
            fd as SOCKET,
            &mut storage as *mut _ as *mut SOCKADDR,
            &mut len,
        );
        if client == INVALID_SOCKET {
            return Err(io::Error::last_os_error());
        }
        sys_set_nonblocking(client as RawFd)?;
        let addr = sockaddr_storage_to_socketaddr(&storage)?;
        Ok((client as RawFd, addr))
    }
}

/// Return the local address bound to a socket.
///
/// Calls WinSock `getsockname()` and converts the result to a Rust
/// [`SocketAddr`].
///
/// # Arguments
///
/// * `fd` — a socket (bound or connected).
///
/// # Errors
///
/// Returns the OS error if `getsockname()` fails, or
/// [`io::ErrorKind::InvalidData`] if the address family is
/// unsupported.
pub fn sys_sockname(fd: RawFd) -> io::Result<SocketAddr> {
    unsafe {
        let mut storage: SOCKADDR_STORAGE = mem::zeroed();
        let mut len = mem::size_of::<SOCKADDR_STORAGE>() as i32;
        if getsockname(
            fd as SOCKET,
            &mut storage as *mut _ as *mut SOCKADDR,
            &mut len,
        ) != 0
        {
            Err(io::Error::last_os_error())
        } else {
            sockaddr_storage_to_socketaddr(&storage)
        }
    }
}

/// Initiate a non-blocking connection to a remote address.
///
/// Calls WinSock `connect()`.  Because the socket is non-blocking,
/// `connect` typically returns `WSAEWOULDBLOCK` — this is treated
/// as success (the connection is in progress).  The caller should
/// register the socket for *write* readiness and check for
/// completion with [`sys_get_socket_error`].
///
/// # Arguments
///
/// * `fd` — a non-blocking socket.
/// * `addr` — the remote address to connect to.
///
/// # Errors
///
/// Returns the OS error if `connect()` fails with anything other
/// than `WSAEWOULDBLOCK`.
pub fn sys_connect(fd: RawFd, addr: &SocketAddr) -> io::Result<()> {
    ensure_winsock();
    let (storage, len) = socketaddr_to_storage(addr);
    unsafe {
        let rc = connect(fd as SOCKET, &storage as *const _ as *const SOCKADDR, len);
        if rc == 0 {
            Ok(())
        } else {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(WSAEWOULDBLOCK) {
                Ok(())
            } else {
                Err(err)
            }
        }
    }
}

/// Shut down part or all of a socket connection.
///
/// Calls WinSock `shutdown()` with the WinSock constant
/// corresponding to `how`:
///
/// | [`Shutdown`] variant | WinSock constant |
/// |---|---|
/// | `Read` | `SD_RECEIVE` |
/// | `Write` | `SD_SEND` |
/// | `Both` | `SD_BOTH` |
///
/// # Errors
///
/// Returns the OS error if `shutdown()` fails (e.g. the socket is
/// not connected).
pub fn sys_shutdown(fd: RawFd, how: Shutdown) -> io::Result<()> {
    let how = match how {
        Shutdown::Read => SD_RECEIVE,
        Shutdown::Write => SD_SEND,
        Shutdown::Both => SD_BOTH,
    };
    unsafe {
        if shutdown(fd as SOCKET, how) != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

/// Retrieve the pending socket error (`SO_ERROR`).
///
/// Reads the `SO_ERROR` option via `getsockopt` and resets it to
/// zero.  This is the standard way to check whether a non-blocking
/// `connect` has completed successfully.
///
/// # Returns
///
/// * `Ok(())` — no error is pending; the operation succeeded.
/// * `Err(e)` — either `getsockopt` itself failed, or the socket
///   had a pending error `e` (e.g. `WSAECONNREFUSED`).
pub fn sys_get_socket_error(fd: RawFd) -> io::Result<()> {
    unsafe {
        let mut err: i32 = 0;
        let mut len: i32 = mem::size_of::<i32>() as i32;

        let rc = getsockopt(
            fd as SOCKET,
            SOL_SOCKET,
            SO_ERROR,
            &mut err as *mut _ as *mut u8,
            &mut len,
        );

        if rc != 0 {
            Err(io::Error::last_os_error())
        } else if err != 0 {
            Err(io::Error::from_raw_os_error(err))
        } else {
            Ok(())
        }
    }
}

/// Enable the `SO_REUSEADDR` socket option.
///
/// Allows a socket to bind to an address that is already in
/// `TIME_WAIT` state.  This is almost always desirable for server
/// sockets to allow quick restarts.
///
/// # Arguments
///
/// * `fd` — a socket (must be called **before** [`sys_bind`]).
///
/// # Errors
///
/// Returns the OS error if `setsockopt()` fails.
pub fn sys_set_reuseaddr(fd: RawFd) -> io::Result<()> {
    unsafe {
        let yes: i32 = 1;
        if setsockopt(
            fd as SOCKET,
            SOL_SOCKET,
            SO_REUSEADDR,
            &yes as *const _ as *const u8,
            4,
        ) != 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

/// Configure IPv6 dual-stack mode when the socket domain is
/// `AF_INET6`.
///
/// If `domain == AF_INET6`, calls [`sys_set_v6only(fd, false)`] so
/// that the socket accepts **both** IPv4 and IPv6 connections
/// (IPv4-mapped addresses).  For any other domain the function is a
/// no-op.
///
/// # Arguments
///
/// * `fd` — a socket.
/// * `domain` — the address family passed to [`sys_socket`].
///
/// # Errors
///
/// Propagates any error from [`sys_set_v6only`].
pub fn sys_ipv6_is_necessary(fd: RawFd, domain: c_int) -> io::Result<()> {
    use windows_sys::Win32::Networking::WinSock::AF_INET6;
    if domain == AF_INET6 as i32 {
        sys_set_v6only(fd, false)?;
    }
    Ok(())
}

/// Set the `IPV6_V6ONLY` socket option.
///
/// Controls whether an IPv6 socket accepts only IPv6 connections or
/// also IPv4 connections via IPv4-mapped addresses.
///
/// # Arguments
///
/// * `fd` — an `AF_INET6` socket.
/// * `v6only` — `true` to restrict to IPv6 only; `false` to accept
///   both IPv4 and IPv6 (dual-stack).
///
/// # Errors
///
/// Returns the OS error if `setsockopt()` fails.
pub fn sys_set_v6only(fd: RawFd, v6only: bool) -> io::Result<()> {
    unsafe {
        let value: u32 = if v6only { 1 } else { 0 };
        if setsockopt(
            fd as SOCKET,
            IPPROTO_IPV6,
            IPV6_V6ONLY,
            &value as *const _ as *const u8,
            4,
        ) != 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
