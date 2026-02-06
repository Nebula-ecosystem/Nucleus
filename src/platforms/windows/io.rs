//! Raw file-descriptor I/O (Windows).
//!
//! Thin wrappers around Win32 `ReadFile` / `WriteFile` (for file
//! HANDLEs) and WinSock `recv` / `send` (for SOCKETs).  The correct
//! backend is selected **at runtime** by calling
//! [`is_socket`](super::utils::is_socket) on the descriptor.
//!
//! # Dual-dispatch model
//!
//! Windows uses two distinct handle spaces: kernel HANDLEs (files,
//! pipes, console) and WinSock SOCKETs.  Because both are stored as
//! the same [`RawFd`] (`u64`) in our abstraction, every read/write
//! call must first determine which kind of handle it is dealing with
//! and call the matching Win32 function.
//!
//! # Non-blocking mode
//!
//! For **sockets**, the descriptor must already be in non-blocking
//! mode (set via `ioctlsocket(FIONBIO)`).  For **files**, Win32 does
//! not support `O_NONBLOCK`-style I/O; the calls are synchronous.
//!
//! # Error reporting
//!
//! `sys_read` and `sys_write` return `-1` on error.  The caller
//! should use [`std::io::Error::last_os_error()`] to retrieve the
//! detailed error code.

use std::ptr;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Networking::WinSock::{SOCKET, SOCKET_ERROR, recv, send};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};

use super::utils::is_socket;

/// Platform file-descriptor type (Windows).
///
/// On Windows this is an alias for [`std::os::windows::io::RawSocket`]
/// (`u64`).  It can hold either a WinSock `SOCKET` or a Win32
/// `HANDLE` (both fit in 64 bits).  Every function in the `platform`
/// modules accepts and returns this type so that the crate can swap
/// between Unix and Windows descriptors transparently.
pub type RawFd = std::os::windows::io::RawSocket;

/// Read from a file descriptor into `buffer`.
///
/// * **Socket** — calls `recv(fd, buffer, 0)`.  Non-blocking if the
///   socket has been configured with `FIONBIO`.
/// * **File HANDLE** — calls `ReadFile(fd, buffer, &mut read, NULL)`.
///   This is a synchronous, blocking call.
///
/// # Arguments
///
/// * `fd` — a valid file descriptor (socket or HANDLE).
/// * `buffer` — destination slice.  At most `buffer.len()` bytes are
///   read.
///
/// # Returns
///
/// * `>= 0` — number of bytes successfully read.
/// * `-1` — error; inspect via [`std::io::Error::last_os_error()`].
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

/// Write `buffer` to a file descriptor.
///
/// * **Socket** — calls `send(fd, buffer, 0)`.  Non-blocking if the
///   socket has been configured with `FIONBIO`.
/// * **File HANDLE** — calls `WriteFile(fd, buffer, &mut written, NULL)`.
///   This is a synchronous, blocking call.
///
/// # Arguments
///
/// * `fd` — a valid file descriptor (socket or HANDLE).
/// * `buffer` — source slice.  At most `buffer.len()` bytes are
///   written.
///
/// # Returns
///
/// * `>= 0` — number of bytes successfully written (may be less than
///   `buffer.len()` for a short write).
/// * `-1` — error; inspect via [`std::io::Error::last_os_error()`].
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

/// Close a file descriptor.
///
/// * **Socket** — calls `closesocket`.
/// * **File HANDLE** — calls `CloseHandle`.
///
/// Errors are **silently ignored** because there is no meaningful
/// recovery: the handle is invalid after this call regardless of
/// the return value.
pub fn sys_close(fd: RawFd) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Networking::WinSock::closesocket;

    unsafe {
        if is_socket(fd) {
            let _ = closesocket(fd as SOCKET);
        } else {
            let _ = CloseHandle(fd as HANDLE);
        }
    }
}
