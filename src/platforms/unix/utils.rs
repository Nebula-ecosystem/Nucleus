//! Platform helpers (Unix).
//!
//! Utility functions that don’t fit neatly into the I/O, filesystem,
//! or socket modules.  On Unix the two main helpers are:
//!
//! * [`sys_set_nonblocking`] — puts a file descriptor into
//!   `O_NONBLOCK` mode via `fcntl(2)`.
//! * [`safe_close`] — closes a file descriptor and propagates the
//!   OS error, unlike [`sys_close`](super::io::sys_close) which
//!   swallows it.

use libc::{F_GETFL, F_SETFL, O_NONBLOCK, close, fcntl};
use std::io;

use super::io::RawFd;

/// Put a file descriptor into non-blocking mode.
///
/// Issues two `fcntl(2)` calls:
///
/// 1. `F_GETFL` — reads the current descriptor flags.
/// 2. `F_SETFL` — writes back the flags with `O_NONBLOCK` set.
///
/// # Arguments
///
/// * `fd` — an open file descriptor (file, socket, pipe, …).
///
/// # Errors
///
/// Returns the OS error if either `fcntl` call fails.  Common
/// causes include an invalid `fd` (`EBADF`) or an `fd` that does
/// not support non-blocking mode.
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

/// Close a file descriptor, returning any OS error.
///
/// Unlike [`sys_close`](super::io::sys_close) — which silently
/// ignores errors — this function wraps `close(2)` and propagates
/// the error so the caller can log or handle it.
///
/// # Arguments
///
/// * `fd` — the file descriptor to close.
///
/// # Errors
///
/// Returns the OS error if `close(2)` fails (e.g. `EIO` on NFS).
pub fn safe_close(fd: RawFd) -> io::Result<()> {
    let rc = unsafe { close(fd) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
