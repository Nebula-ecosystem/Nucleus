use libc::{
    AF_INET, AF_INET6, F_GETFL, F_SETFL, IPPROTO_IPV6, IPV6_V6ONLY, O_CREAT, O_NONBLOCK, O_RDONLY,
    O_TRUNC, O_WRONLY, SHUT_RD, SHUT_RDWR, SHUT_WR, SO_ERROR, SO_REUSEADDR, SOCK_STREAM,
    SOL_SOCKET, accept, bind, c_char, c_int, close, connect, fcntl, getsockname, getsockopt,
    listen, mkdir, mode_t, open, read, setsockopt, shutdown, sockaddr, sockaddr_in, sockaddr_in6,
    sockaddr_storage, socket, socklen_t, write,
};
use std::ffi::c_uint;
use std::net::{Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::str::FromStr;
use std::{io, mem};

pub type RawFd = std::os::unix::io::RawFd;

/// Default flags used when opening a file for reading.
pub const OPENFLAGS: i32 = O_RDONLY | O_NONBLOCK;

/// Default flags used when creating a file for writing.
pub const CREATEFLAGS: i32 = O_WRONLY | O_CREAT | O_TRUNC | O_NONBLOCK;

/// Reads from a file descriptor into the given buffer.
///
/// Returns the number of bytes read, or a negative value on error.
/// The file descriptor **must** be non-blocking.
pub fn sys_read(fd: RawFd, buffer: &mut [u8]) -> isize {
    unsafe { read(fd, buffer.as_mut_ptr() as *mut _, buffer.len()) }
}

/// Writes the buffer to a file descriptor.
///
/// Returns the number of bytes written, or a negative value on error.
/// The file descriptor **must** be non-blocking.
pub fn sys_write(fd: RawFd, buffer: &[u8]) -> isize {
    unsafe { write(fd, buffer.as_ptr() as *const _, buffer.len()) }
}

/// Closes a file descriptor.
pub fn sys_close(fd: RawFd) {
    unsafe { close(fd) };
}

/// Opens a file using `open(2)`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn sys_open(path: *const c_char, flags: i32, mode: mode_t) -> RawFd {
    unsafe { open(path, flags, mode as c_uint) }
}

/// Creates a directory using `mkdir(2)`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn sys_mkdir(path: *const c_char, mode: mode_t) -> RawFd {
    unsafe { mkdir(path, mode) }
}

/// Sets a file descriptor to non-blocking mode.
pub fn sys_set_nonblocking(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { fcntl(fd, F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }

    let rc = unsafe { fcntl(fd, F_SETFL, flags | O_NONBLOCK) };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

/// Creates a non-blocking stream socket.
pub fn sys_socket(domain: c_int) -> io::Result<RawFd> {
    let fd = unsafe { socket(domain, SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }

    if let Err(e) = sys_set_nonblocking(fd) {
        unsafe { close(fd) };
        return Err(e);
    }

    Ok(fd)
}

/// Binds a socket to an address.
pub fn sys_bind(fd: RawFd, addr: &sockaddr_storage, len: socklen_t) -> io::Result<()> {
    let rc = unsafe { bind(fd, addr as *const _ as *const sockaddr, len) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Marks a socket as a listening socket.
pub fn sys_listen(fd: RawFd) -> io::Result<()> {
    let rc = unsafe { listen(fd, 128) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Accepts a new incoming connection.
///
/// The returned client socket is automatically set to non-blocking mode.
pub fn sys_accept(fd: RawFd) -> io::Result<(RawFd, SocketAddr)> {
    let mut storage: sockaddr_storage = unsafe { mem::zeroed() };
    let mut len = mem::size_of::<sockaddr_storage>() as socklen_t;

    let client_fd = unsafe { accept(fd, &mut storage as *mut _ as *mut sockaddr, &mut len) };

    if client_fd < 0 {
        return Err(io::Error::last_os_error());
    }

    if let Err(e) = sys_set_nonblocking(client_fd) {
        unsafe { close(client_fd) };
        return Err(e);
    }

    let addr = sockaddr_storage_to_socketaddr(&storage)?;

    Ok((client_fd, addr))
}

/// Returns the local address of a socket.
pub fn sys_sockname(fd: RawFd) -> io::Result<SocketAddr> {
    let mut storage: sockaddr_storage = unsafe { mem::zeroed() };
    let mut len = mem::size_of::<sockaddr_storage>() as socklen_t;

    let rc = unsafe { getsockname(fd, &mut storage as *mut _ as *mut sockaddr, &mut len) };

    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        sockaddr_storage_to_socketaddr(&storage)
    }
}

/// Initiates a non-blocking connection.
pub fn sys_connect(fd: RawFd, addr: &SocketAddr) -> io::Result<()> {
    let (storage, len) = socketaddr_to_storage(addr);

    let rc = unsafe { connect(fd, &storage as *const _ as *const sockaddr, len) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Shuts down a socket.
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

/// Retrieves the pending socket error via `SO_ERROR`.
///
/// Returns `Ok(())` if no error is pending, or the error otherwise.
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

/// Enables `SO_REUSEADDR` on a socket.
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

/// Parses a socket address string into a `sockaddr_storage`.
pub fn sys_parse_sockaddr(address: &str) -> io::Result<(sockaddr_storage, socklen_t)> {
    let addr = SocketAddr::from_str(address)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid socket addr"))?;

    Ok(socketaddr_to_storage(&addr))
}

/// Converts a `sockaddr_storage` to a Rust `SocketAddr`.
pub fn sockaddr_storage_to_socketaddr(storage: &sockaddr_storage) -> io::Result<SocketAddr> {
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

/// Converts a `SocketAddr` to a `sockaddr_storage`.
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

/// Enables IPv6 dual-stack support when required.
pub fn sys_ipv6_is_necessary(fd: RawFd, domain: c_int) -> io::Result<()> {
    if domain == AF_INET6 {
        sys_set_v6only(fd, false)?;
    }
    Ok(())
}

/// Sets the `IPV6_V6ONLY` socket option.
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

/// An I/O event reported by the poller.
///
/// An `Event` represents readiness information for a registered
/// file descriptor. It is produced by the poller and consumed
/// by the reactor to wake the appropriate tasks.
///
/// The event indicates whether the file descriptor is readable,
/// writable, or both.
pub struct Event {
    /// Token associated with the registered file descriptor.
    ///
    /// This token is used to identify the I/O entry inside the reactor.
    pub(crate) token: usize,

    /// Indicates that the file descriptor is readable.
    pub(crate) readable: bool,

    /// Indicates that the file descriptor is writable.
    pub(crate) writable: bool,
}
