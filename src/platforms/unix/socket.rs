//! Socket lifecycle and options (Unix).
//!
//! Complete wrappers around the BSD socket API for TCP stream sockets.
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
//! **non-blocking mode** via
//! [`sys_set_nonblocking`](super::utils::sys_set_nonblocking).

use libc::{
    IPPROTO_IPV6, IPV6_V6ONLY, SHUT_RD, SHUT_RDWR, SHUT_WR, SO_ERROR, SO_REUSEADDR, SOCK_STREAM,
    SOL_SOCKET, accept, bind, c_int, connect, getsockname, getsockopt, listen, setsockopt,
    shutdown, sockaddr, socket, socklen_t,
};
use std::io;
use std::mem;
use std::net::{Shutdown, SocketAddr};

/// IPv4 address family constant.
pub const AF_INET: c_int = libc::AF_INET;

/// IPv6 address family constant.
pub const AF_INET6: c_int = libc::AF_INET6;

use super::address::{sockaddr_storage_to_socketaddr, socketaddr_to_storage};
use super::io::RawFd;
use super::utils::{safe_close, sys_set_nonblocking};

/// Create a non-blocking TCP stream socket.
///
/// Calls `socket(domain, SOCK_STREAM, 0)` and, on success,
/// immediately puts the resulting file descriptor into non-blocking
/// mode with [`sys_set_nonblocking`](super::utils::sys_set_nonblocking).
/// If setting non-blocking mode fails, the descriptor is closed via
/// [`safe_close`](super::utils::safe_close) to prevent a leak.
///
/// # Arguments
///
/// * `domain` — address family, typically `AF_INET` or `AF_INET6`.
///
/// # Returns
///
/// The new socket file descriptor.
///
/// # Errors
///
/// Returns the OS error if `socket()` fails (e.g. `EMFILE` when the
/// per-process descriptor limit is reached) or if `fcntl(F_SETFL)`
/// fails.
pub fn sys_socket(domain: c_int) -> io::Result<RawFd> {
    let fd = unsafe { socket(domain, SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }

    if let Err(e) = sys_set_nonblocking(fd) {
        let _ = safe_close(fd);
        return Err(e);
    }

    Ok(fd)
}

/// Bind a socket to a local address.
///
/// Calls `bind(2)` with the provided `sockaddr_storage` and its byte
/// length.
///
/// # Arguments
///
/// * `fd` — a socket file descriptor returned by [`sys_socket`].
/// * `addr` — a populated `sockaddr_storage` (see
///   [`socketaddr_to_storage`](super::address::socketaddr_to_storage)).
/// * `len` — byte length of the relevant `sockaddr_in` or
///   `sockaddr_in6` inside the storage.
///
/// # Errors
///
/// Returns the OS error if `bind()` fails (e.g. `EADDRINUSE`,
/// `EACCES`).
pub fn sys_bind(fd: RawFd, addr: &libc::sockaddr_storage, len: socklen_t) -> io::Result<()> {
    let rc = unsafe { bind(fd, addr as *const _ as *const sockaddr, len) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Mark a socket as passive (listening for connections).
///
/// Calls `listen(2)` with a backlog of **128**.  The backlog value is
/// a hint to the kernel; on Linux it is clamped to
/// `/proc/sys/net/core/somaxconn`.
///
/// # Arguments
///
/// * `fd` — a bound socket file descriptor.
///
/// # Errors
///
/// Returns the OS error if `listen()` fails.
pub fn sys_listen(fd: RawFd) -> io::Result<()> {
    let rc = unsafe { listen(fd, 128) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Accept a new connection on a listening socket.
///
/// Calls `accept(2)` and, on success, puts the returned client
/// socket into non-blocking mode.  If setting non-blocking mode
/// fails, the client socket is closed to prevent a leak.
///
/// # Arguments
///
/// * `fd` — a listening socket file descriptor.
///
/// # Returns
///
/// A `(client_fd, peer_addr)` tuple.  The `client_fd` is already in
/// non-blocking mode and ready for registration with the poller.
///
/// # Errors
///
/// Returns the OS error if `accept()` fails (e.g. `EAGAIN` when no
/// connection is pending) or if the client socket cannot be set to
/// non-blocking mode.
pub fn sys_accept(fd: RawFd) -> io::Result<(RawFd, SocketAddr)> {
    let mut storage: libc::sockaddr_storage = unsafe { mem::zeroed() };
    let mut len = mem::size_of::<libc::sockaddr_storage>() as socklen_t;

    let client_fd = unsafe { accept(fd, &mut storage as *mut _ as *mut sockaddr, &mut len) };

    if client_fd < 0 {
        return Err(io::Error::last_os_error());
    }

    if let Err(e) = sys_set_nonblocking(client_fd) {
        let _ = safe_close(client_fd);
        return Err(e);
    }

    let addr = sockaddr_storage_to_socketaddr(&storage)?;

    Ok((client_fd, addr))
}

/// Return the local address bound to a socket.
///
/// Calls `getsockname(2)` and converts the result to a Rust
/// [`SocketAddr`].
///
/// # Arguments
///
/// * `fd` — a socket file descriptor (bound or connected).
///
/// # Errors
///
/// Returns the OS error if `getsockname()` fails, or
/// [`io::ErrorKind::InvalidData`] if the address family is
/// unsupported.
pub fn sys_sockname(fd: RawFd) -> io::Result<SocketAddr> {
    let mut storage: libc::sockaddr_storage = unsafe { mem::zeroed() };
    let mut len = mem::size_of::<libc::sockaddr_storage>() as socklen_t;

    let rc = unsafe { getsockname(fd, &mut storage as *mut _ as *mut sockaddr, &mut len) };

    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        sockaddr_storage_to_socketaddr(&storage)
    }
}

/// Initiate a non-blocking connection to a remote address.
///
/// Calls `connect(2)` with the storage representation of `addr`.
/// Because the socket is non-blocking, `connect` typically returns
/// `EINPROGRESS` — the caller should register the socket for
/// *write* readiness and check for completion with
/// [`sys_get_socket_error`].
///
/// # Arguments
///
/// * `fd` — a non-blocking socket file descriptor.
/// * `addr` — the remote address to connect to.
///
/// # Errors
///
/// Returns the OS error if `connect()` fails with anything other
/// than the expected in-progress error.
pub fn sys_connect(fd: RawFd, addr: &SocketAddr) -> io::Result<()> {
    let (storage, len) = socketaddr_to_storage(addr);

    let rc = unsafe { connect(fd, &storage as *const _ as *const sockaddr, len) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Shut down part or all of a socket connection.
///
/// Calls `shutdown(2)` with the POSIX flag corresponding to `how`:
///
/// | [`Shutdown`] variant | POSIX flag |
/// |---|---|
/// | `Read` | `SHUT_RD` |
/// | `Write` | `SHUT_WR` |
/// | `Both` | `SHUT_RDWR` |
///
/// # Errors
///
/// Returns the OS error if `shutdown()` fails (e.g. the socket is
/// not connected).
pub fn sys_shutdown(fd: RawFd, how: Shutdown) -> io::Result<()> {
    let how = match how {
        Shutdown::Read => SHUT_RD,
        Shutdown::Write => SHUT_WR,
        Shutdown::Both => SHUT_RDWR,
    };

    let rc = unsafe { shutdown(fd, how) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Retrieve the pending socket error (`SO_ERROR`).
///
/// Reads the `SO_ERROR` option via `getsockopt(2)` and resets it to
/// zero.  This is the standard way to check whether a non-blocking
/// `connect` has completed successfully.
///
/// # Returns
///
/// * `Ok(())` — no error is pending; the operation succeeded.
/// * `Err(e)` — either `getsockopt` itself failed, or the socket
///   had a pending error `e` (e.g. `ECONNREFUSED`).
pub fn sys_get_socket_error(fd: RawFd) -> io::Result<()> {
    let mut err: c_int = 0;
    let mut len: socklen_t = mem::size_of::<c_int>() as socklen_t;

    let rc = unsafe {
        getsockopt(
            fd,
            SOL_SOCKET,
            SO_ERROR,
            &mut err as *mut _ as *mut _,
            &mut len,
        )
    };

    if rc < 0 {
        Err(io::Error::last_os_error())
    } else if err != 0 {
        Err(io::Error::from_raw_os_error(err))
    } else {
        Ok(())
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
/// * `fd` — a socket file descriptor (must be called **before**
///   [`sys_bind`]).
///
/// # Errors
///
/// Returns the OS error if `setsockopt()` fails.
pub fn sys_set_reuseaddr(fd: RawFd) -> io::Result<()> {
    let yes: c_int = 1;
    let rc = unsafe {
        setsockopt(
            fd,
            SOL_SOCKET,
            SO_REUSEADDR,
            &yes as *const _ as *const _,
            mem::size_of::<c_int>() as socklen_t,
        )
    };

    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
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
/// * `fd` — a socket file descriptor.
/// * `domain` — the address family passed to [`sys_socket`].
///
/// # Errors
///
/// Propagates any error from [`sys_set_v6only`].
pub fn sys_ipv6_is_necessary(fd: RawFd, domain: c_int) -> io::Result<()> {
    if domain == AF_INET6 {
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
/// * `fd` — an `AF_INET6` socket file descriptor.
/// * `v6only` — `true` to restrict to IPv6 only; `false` to accept
///   both IPv4 and IPv6 (dual-stack).
///
/// # Errors
///
/// Returns the OS error if `setsockopt()` fails.
pub fn sys_set_v6only(fd: RawFd, v6only: bool) -> io::Result<()> {
    let value: c_int = if v6only { 1 } else { 0 };

    let rc = unsafe {
        setsockopt(
            fd,
            IPPROTO_IPV6,
            IPV6_V6ONLY,
            &value as *const _ as *const _,
            mem::size_of::<c_int>() as socklen_t,
        )
    };

    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
