//! Raw file-descriptor I/O.
//!
//! Thin wrappers around `read(2)`, `write(2)`, and `close(2)`.  Every
//! function operates on a [`RawFd`] and performs **no buffering** —
//! the caller (typically the reactor) is responsible for managing
//! non-blocking semantics and retry-on-`EAGAIN` logic.
//!
//! # Non-blocking mode
//!
//! These wrappers do *not* set `O_NONBLOCK` themselves.  The
//! descriptor **must** already be in non-blocking mode before it is
//! passed to [`sys_read`] or [`sys_write`].  Use
//! [`sys_set_nonblocking`](super::utils::sys_set_nonblocking) to
//! configure the descriptor after creation.
//!
//! # Error reporting
//!
//! `sys_read` and `sys_write` return the raw `isize` from the
//! underlying syscall.  A negative return value indicates an error;
//! call [`std::io::Error::last_os_error()`] to retrieve the
//! `errno`-based error.

use libc::{close, read, write};

/// Platform file-descriptor type (Unix).
///
/// On Unix this is an alias for [`std::os::unix::io::RawFd`] (i.e.
/// `i32`).  Every function in the `platform` modules accepts and
/// returns this type so that the crate can swap between Unix and
/// Windows descriptors transparently.
pub type RawFd = std::os::unix::io::RawFd;

/// Read from a file descriptor into `buffer`.
///
/// Wraps `read(2)`.  The call is non-blocking if `fd` has been
/// configured with `O_NONBLOCK`; in that case a return value of `-1`
/// with `errno == EAGAIN` means no data is available yet.
///
/// # Arguments
///
/// * `fd` — an open, non-blocking file descriptor.
/// * `buffer` — destination slice.  At most `buffer.len()` bytes are
///   read.
///
/// # Returns
///
/// * `> 0` — number of bytes successfully read.
/// * `0` — end-of-file (peer closed the connection for a socket).
/// * `< 0` — error; inspect `errno` via
///   [`std::io::Error::last_os_error()`].
pub fn sys_read(fd: RawFd, buffer: &mut [u8]) -> isize {
    unsafe { read(fd, buffer.as_mut_ptr() as *mut _, buffer.len()) }
}

/// Write `buffer` to a file descriptor.
///
/// Wraps `write(2)`.  The call is non-blocking if `fd` has been
/// configured with `O_NONBLOCK`; in that case a return value of `-1`
/// with `errno == EAGAIN` means the write buffer is full.
///
/// # Arguments
///
/// * `fd` — an open, non-blocking file descriptor.
/// * `buffer` — source slice.  At most `buffer.len()` bytes are
///   written.
///
/// # Returns
///
/// * `> 0` — number of bytes successfully written (may be less than
///   `buffer.len()` for a short write).
/// * `< 0` — error; inspect `errno` via
///   [`std::io::Error::last_os_error()`].
pub fn sys_write(fd: RawFd, buffer: &[u8]) -> isize {
    unsafe { write(fd, buffer.as_ptr() as *const _, buffer.len()) }
}

/// Close a file descriptor.
///
/// Wraps `close(2)`.  Errors are **silently ignored** because there
/// is no meaningful recovery: the descriptor is invalid after this
/// call regardless of the return value.  Use
/// [`safe_close`](super::utils::safe_close) when the caller needs to
/// observe the error.
pub fn sys_close(fd: RawFd) {
    unsafe { close(fd) };
}
