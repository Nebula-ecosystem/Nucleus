//! File-system operations (Unix).
//!
//! Wrappers around `open(2)` and `mkdir(2)` with POSIX-style flags.
//! These are thin, `unsafe` functions because they accept raw C
//! string pointers — the caller (typically higher-level Cadentis I/O
//! code) is responsible for constructing valid, null-terminated paths.
//!
//! # Constants
//!
//! Two convenience flag sets are provided:
//!
//! | Constant | Flags | Typical use |
//! |---|---|---|
//! | [`OPENFLAGS`] | `O_RDONLY \| O_NONBLOCK` | Non-blocking read of an existing file |
//! | [`CREATEFLAGS`] | `O_WRONLY \| O_CREAT \| O_TRUNC \| O_NONBLOCK` | Create-or-truncate a file for non-blocking writes |

use libc::{
    O_CREAT, O_NONBLOCK, O_RDONLY, O_TRUNC, O_WRONLY, c_char, mkdir, mode_t, open, statvfs,
};
use std::ffi::c_uint;
use std::ffi::CString;
use std::mem;

use super::io::RawFd;

/// Default flags for opening a file in read-only, non-blocking mode.
///
/// Equivalent to `O_RDONLY | O_NONBLOCK`.
pub const OPENFLAGS: i32 = O_RDONLY | O_NONBLOCK;

/// Default flags for creating (or truncating) a file in write-only,
/// non-blocking mode.
///
/// Equivalent to `O_WRONLY | O_CREAT | O_TRUNC | O_NONBLOCK`.
pub const CREATEFLAGS: i32 = O_WRONLY | O_CREAT | O_TRUNC | O_NONBLOCK;

/// Open a file.
///
/// Wraps `open(2)` with the given POSIX flags and permission mode.
///
/// # Arguments
///
/// * `path` — pointer to a null-terminated C string.
/// * `flags` — POSIX open flags (e.g. [`OPENFLAGS`], [`CREATEFLAGS`],
///   or a custom combination).
/// * `mode` — permission bits applied when `O_CREAT` is set (e.g.
///   `0o644`).
///
/// # Returns
///
/// A non-negative file descriptor on success, or `-1` on error
/// (inspect `errno` via [`std::io::Error::last_os_error()`]).
///
/// # Safety
///
/// `path` must point to a valid, null-terminated C string that
/// remains valid for the duration of this call.
pub unsafe fn sys_open(path: *const c_char, flags: i32, mode: mode_t) -> RawFd {
    unsafe { open(path, flags, mode as c_uint) }
}

/// Create a directory.
///
/// Wraps `mkdir(2)` with the given permission mode.
///
/// # Arguments
///
/// * `path` — pointer to a null-terminated C string.
/// * `mode` — permission bits for the new directory (e.g. `0o755`).
///
/// # Returns
///
/// `0` on success, or `-1` on error (inspect `errno` via
/// [`std::io::Error::last_os_error()`]).
///
/// # Safety
///
/// `path` must point to a valid, null-terminated C string that
/// remains valid for the duration of this call.
pub unsafe fn sys_mkdir(path: *const c_char, mode: mode_t) -> RawFd {
    unsafe { mkdir(path, mode) }
}

/// Return the available storage space (in bytes) for the filesystem at `path`.
///
/// Wraps `statvfs(2)` and computes `f_bavail * f_frsize`, which represents the
/// free blocks available to unprivileged users.
///
/// # Arguments
///
/// * `path` — a filesystem path to query.
///
/// # Returns
///
/// The number of available bytes for non-root users.
///
/// # Panics
///
/// Panics if `path` contains an interior NUL byte (required to build a C string
/// for `statvfs`).
pub fn storage_left(path: &str) -> u64 {
    let c_path = CString::new(path).unwrap();
    let mut stat: statvfs = unsafe { mem::zeroed() };

    unsafe {
        statvfs(c_path.as_ptr(), &mut stat);
    }

    stat.f_bavail as u64 * stat.f_frsize as u64
}
