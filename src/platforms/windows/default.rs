//! Windows platform abstraction layer.
//!
//! This module provides the Windows implementation of low-level
//! system primitives required by the Cadentis runtime.
//!
//! It mirrors the Unix platform layer and exposes identical
//! function names and semantics where possible.
//!
//! Both SOCKETs and file HANDLEs are supported. The implementation
//! dynamically distinguishes between them when performing I/O.

use std::ffi::{CStr, c_char, c_int};
use std::io;
use std::mem;
use std::net::{Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::path::{Component, Path, PathBuf};
use std::ptr;
use std::str::FromStr;
use std::sync::Once;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, FIONBIO, INVALID_SOCKET, IPPROTO_IPV6, IPV6_V6ONLY, SD_BOTH, SD_RECEIVE,
    SD_SEND, SO_ERROR, SO_REUSEADDR, SO_TYPE, SOCK_STREAM, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6,
    SOCKADDR_STORAGE, SOCKET, SOCKET_ERROR, SOL_SOCKET, WSADATA, WSAEWOULDBLOCK, WSAStartup,
    accept, bind, closesocket, connect, getsockname, getsockopt, ioctlsocket, listen, recv, send,
    setsockopt, shutdown, socket,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_ALWAYS, CREATE_NEW, CreateDirectoryA, CreateFileA, FILE_ATTRIBUTE_NORMAL,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile, WriteFile,
};

/// Raw file descriptor type on Windows.
///
/// Internally this is either:
/// - a WinSock `SOCKET`, or
/// - a Win32 file `HANDLE`.
///
/// The runtime dynamically detects which kind it is.
pub type RawFd = std::os::windows::io::RawSocket;

/// POSIX-style open flags (partial).
const O_RDONLY: i32 = 0x0000;
const O_WRONLY: i32 = 0x0001;
const O_RDWR: i32 = 0x0002;
const O_CREAT: i32 = 0x0100;
const O_EXCL: i32 = 0x0080;

/// Default flags used when opening a file for reading.
pub const OPENFLAGS: u64 = O_RDONLY as u64;

/// Default flags used when creating a file for writing.
pub const CREATEFLAGS: u64 = O_CREAT as u64 | O_RDWR as u64;

/// Returns `true` if the given descriptor refers to a socket.
fn is_socket(fd: RawFd) -> bool {
    if fd == u64::MAX {
        return false;
    }

    unsafe {
        let mut ty: i32 = 0;
        let mut len = mem::size_of::<i32>() as i32;

        getsockopt(
            fd as SOCKET,
            SOL_SOCKET,
            SO_TYPE,
            &mut ty as *mut _ as *mut u8,
            &mut len,
        ) == 0
    }
}

/// Creates a MAKEWORD value for Winsock version.
#[inline]
const fn makeword(low: u8, high: u8) -> u16 {
    ((high as u16) << 8) | (low as u16)
}

/// Winsock initialization guard.
static WINSOCK_INIT: Once = Once::new();

/// Initialize Winsock if not already initialized.
pub fn ensure_winsock() {
    WINSOCK_INIT.call_once(|| unsafe {
        let mut data: WSADATA = mem::zeroed();
        let rc = WSAStartup(makeword(2, 2), &mut data as *mut _);
        assert_eq!(rc, 0, "WSAStartup failed: {}", rc);
    });
}

/// Reads from a file descriptor into the given buffer.
///
/// Returns the number of bytes read, or `-1` on error.
pub fn sys_read(fd: RawFd, buffer: &mut [u8]) -> isize {
    unsafe {
        if is_socket(fd) {
            let rc = recv(fd as SOCKET, buffer.as_mut_ptr(), buffer.len() as i32, 0);
            if rc == SOCKET_ERROR { -1 } else { rc as isize }
        } else {
            let mut read = 0u32;
            let ok = ReadFile(
                fd as HANDLE,
                buffer.as_mut_ptr() as *mut _,
                buffer.len() as u32,
                &mut read,
                ptr::null_mut(),
            );
            if ok == 0 { -1 } else { read as isize }
        }
    }
}

/// Writes the buffer to a file descriptor.
///
/// Returns the number of bytes written, or `-1` on error.
pub fn sys_write(fd: RawFd, buffer: &[u8]) -> isize {
    unsafe {
        if is_socket(fd) {
            let rc = send(fd as SOCKET, buffer.as_ptr(), buffer.len() as i32, 0);
            if rc == SOCKET_ERROR { -1 } else { rc as isize }
        } else {
            let mut written = 0u32;
            let ok = WriteFile(
                fd as HANDLE,
                buffer.as_ptr() as *const _,
                buffer.len() as u32,
                &mut written,
                ptr::null_mut(),
            );
            if ok == 0 { -1 } else { written as isize }
        }
    }
}

/// Closes a file descriptor.
pub fn sys_close(fd: RawFd) {
    unsafe {
        if is_socket(fd) {
            let _ = closesocket(fd as SOCKET);
        } else {
            let _ = CloseHandle(fd as HANDLE);
        }
    }
}

/// Opens or creates a file.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn sys_open(path: *const c_char, flags: i32, _mode: u32) -> RawFd {
    unsafe {
        let access = if flags & O_RDWR != 0 {
            FILE_GENERIC_READ | FILE_GENERIC_WRITE
        } else if flags & O_WRONLY != 0 {
            FILE_GENERIC_WRITE
        } else {
            FILE_GENERIC_READ
        };

        let mut disposition = if flags & O_CREAT != 0 {
            if flags & O_EXCL != 0 {
                CREATE_NEW
            } else {
                CREATE_ALWAYS
            }
        } else {
            OPEN_EXISTING
        };

        let mut handle = CreateFileA(
            path as *const u8,
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            disposition,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_BACKUP_SEMANTICS,
            ptr::null_mut(),
        );

        if handle == INVALID_HANDLE_VALUE
            && flags & O_CREAT != 0
            && flags & O_EXCL == 0
            && GetLastError() == ERROR_ALREADY_EXISTS
        {
            disposition = OPEN_EXISTING;
            handle = CreateFileA(
                path as *const u8,
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null(),
                disposition,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_BACKUP_SEMANTICS,
                ptr::null_mut(),
            );
        }

        if handle == INVALID_HANDLE_VALUE {
            u64::MAX
        } else {
            handle as RawFd
        }
    }
}

/// Converts a path to a lexical absolute path.
fn to_lexical_absolute(path: &Path) -> io::Result<PathBuf> {
    let mut out = if path.is_absolute() {
        PathBuf::new()
    } else {
        std::env::current_dir()?
    };

    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(p) => out.push(p),
            Component::RootDir => out.push("\\"),
            Component::Prefix(p) => out.push(p.as_os_str()),
        }
    }

    Ok(out)
}

/// Creates a directory.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn sys_mkdir(path: *const c_char, _mode: u32) -> RawFd {
    unsafe {
        let s = match CStr::from_ptr(path).to_str() {
            Ok(s) => s.replace('/', "\\"),
            Err(_) => return u64::MAX,
        };

        let abs = match to_lexical_absolute(Path::new(&s)) {
            Ok(p) => p,
            Err(_) => return u64::MAX,
        };

        let mut normalized = abs.to_string_lossy().to_string();
        while normalized.ends_with('\\') && normalized.len() > 3 && !normalized.ends_with(":\\") {
            normalized.pop();
        }

        let c = match std::ffi::CString::new(normalized) {
            Ok(c) => c,
            Err(_) => return u64::MAX,
        };

        if CreateDirectoryA(c.as_ptr() as *const u8, ptr::null()) == 0 {
            u64::MAX
        } else {
            0
        }
    }
}

/// Sets a socket to non-blocking mode.
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

/// Creates a new socket.
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

/// Binds a socket to an address.
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

/// Puts a socket into listening mode.
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

/// Accepts a new connection on a listening socket.
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

/// Gets the local address of a socket.
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

/// Connects a socket to a remote address.
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

/// Shuts down part or all of a socket connection.
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

/// Retrieves the pending socket error via `SO_ERROR`.
///
/// Returns `Ok(())` if no error is pending, or the error otherwise.
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

/// Sets the SO_REUSEADDR option on a socket.
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

/// Parses a socket address string into a SOCKADDR_STORAGE.
pub fn sys_parse_sockaddr(address: &str) -> io::Result<(SOCKADDR_STORAGE, i32)> {
    let addr = SocketAddr::from_str(address)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid socket addr"))?;
    Ok(socketaddr_to_storage(&addr))
}

/// Converts a SOCKADDR_STORAGE to a SocketAddr.
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

/// Converts a SocketAddr to a SOCKADDR_STORAGE.
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

/// Configures IPv6 socket options based on domain.
pub fn sys_ipv6_is_necessary(fd: RawFd, domain: c_int) -> io::Result<()> {
    if domain == AF_INET6 as i32 {
        sys_set_v6only(fd, false)?;
    }
    Ok(())
}

/// Sets the IPV6_V6ONLY option on a socket.
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
