//! Platform helpers (Windows).
//!
//! Utility functions that don’t fit neatly into the I/O, filesystem,
//! or socket modules.  On Windows the main helpers are:
//!
//! * [`is_socket`] — runtime check to distinguish a WinSock
//!   `SOCKET` from a Win32 `HANDLE` (both are stored as [`RawFd`]).
//! * [`makeword`] — constructs a `MAKEWORD` value for WinSock
//!   version negotiation.
//! * [`ensure_winsock`] — one-time, process-wide WinSock 2.2
//!   initialisation.
//! * [`sys_set_nonblocking`] — puts a socket into non-blocking mode.
//! * [`safe_close`] — closes a handle/socket and returns any error.

use std::io;
use std::mem;
use std::sync::Once;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Networking::WinSock::{
    FIONBIO, SO_TYPE, SOCKET, SOL_SOCKET, WSADATA, WSAStartup, closesocket, getsockopt, ioctlsocket,
};

use super::io::RawFd;

/// Return `true` if `fd` refers to a WinSock socket.
///
/// Calls `getsockopt(fd, SOL_SOCKET, SO_TYPE)`.  If the call
/// succeeds the descriptor is a valid socket; if it fails (or if
/// `fd == u64::MAX`, which Nucleus uses as a sentinel for invalid
/// handles) the descriptor is treated as a file HANDLE.
///
/// This heuristic is used by [`sys_read`](super::io::sys_read) and
/// [`sys_write`](super::io::sys_write) to dispatch to the correct
/// Win32 function.
pub fn is_socket(fd: RawFd) -> bool {
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

/// Pack two bytes into a `MAKEWORD` value.
///
/// Used exclusively for the `WSAStartup` version parameter.  The
/// result encodes `major.minor` as `(high << 8) | low`.
///
/// # Example
///
/// ```rust,ignore
/// assert_eq!(makeword(2, 2), 0x0202); // WinSock 2.2
/// ```
#[inline]
pub const fn makeword(low: u8, high: u8) -> u16 {
    ((high as u16) << 8) | (low as u16)
}

/// One-time WinSock initialisation guard.
///
/// [`Once`] ensures that `WSAStartup` is called exactly once, even
/// when multiple threads race to create sockets.
static WINSOCK_INIT: Once = Once::new();

/// Initialise WinSock 2.2 (process-wide, idempotent).
///
/// Calls `WSAStartup(MAKEWORD(2, 2))` the first time it is invoked.
/// Subsequent calls are no-ops.  Every socket-related function in
/// this crate calls `ensure_winsock()` before issuing any WinSock
/// call, so the caller never needs to initialise WinSock manually.
///
/// # Panics
///
/// Panics if `WSAStartup` returns a non-zero error code.  This
/// typically means WinSock 2.2 is not available on the system, which
/// is fatal for the runtime.
pub fn ensure_winsock() {
    WINSOCK_INIT.call_once(|| unsafe {
        let mut data: WSADATA = mem::zeroed();
        let rc = WSAStartup(makeword(2, 2), &mut data as *mut _);
        assert_eq!(rc, 0, "WSAStartup failed: {}", rc);
    });
}

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

/// Close a handle or socket, returning any OS error.
///
/// Unlike [`sys_close`](super::io::sys_close) — which silently
/// ignores errors — this function propagates the error so the
/// caller can log or handle it.
///
/// # Arguments
///
/// * `fd` — the handle or socket to close.
///
/// # Errors
///
/// Returns the OS error if the close operation fails.
pub fn safe_close(fd: RawFd) -> io::Result<()> {
    if is_socket(fd) {
        let rc = unsafe { closesocket(fd as SOCKET) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
    } else {
        let rc = unsafe { CloseHandle(fd as isize) };
        if rc == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
