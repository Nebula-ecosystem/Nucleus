//! File-system operations (Windows).
//!
//! Wrappers around `CreateFileA` and `CreateDirectoryA` with
//! POSIX-to-Win32 flag translation.  These are thin `unsafe`
//! functions because they accept raw C string pointers — the caller
//! (typically higher-level Cadentis I/O code) is responsible for
//! constructing valid, null-terminated paths.
//!
//! # POSIX flag translation
//!
//! Nucleus uses POSIX-style open flags everywhere for consistency.
//! This module maps them to Win32 parameters:
//!
//! | POSIX flag(s) | Win32 access | Win32 disposition |
//! |---|---|---|
//! | `O_RDONLY` | `FILE_GENERIC_READ` | `OPEN_EXISTING` |
//! | `O_WRONLY` | `FILE_GENERIC_WRITE` | `OPEN_EXISTING` |
//! | `O_RDWR` | `FILE_GENERIC_READ \| FILE_GENERIC_WRITE` | `OPEN_EXISTING` |
//! | `O_CREAT` | (as above) | `CREATE_ALWAYS` |
//! | `O_CREAT \| O_EXCL` | (as above) | `CREATE_NEW` |
//!
//! # Constants
//!
//! | Constant | Value | Typical use |
//! |---|---|---|
//! | [`OPENFLAGS`] | `O_RDONLY` | Read an existing file |
//! | [`CREATEFLAGS`] | `O_CREAT \| O_RDWR` | Create-or-overwrite a file |

use std::ffi::{CStr, c_char};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_ALWAYS, CREATE_NEW, CreateDirectoryA, CreateFileA, FILE_ATTRIBUTE_NORMAL,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

use super::io::RawFd;

/// POSIX-style open flags (subset).
///
/// These constants mirror the POSIX values so that the rest of the
/// crate can use the same flag interface on both Unix and Windows.
const O_RDONLY: i32 = 0x0000;
const O_WRONLY: i32 = 0x0001;
const O_RDWR: i32 = 0x0002;
const O_CREAT: i32 = 0x0100;
const O_EXCL: i32 = 0x0080;

/// Default flags for opening a file in read-only mode.
///
/// Equivalent to POSIX `O_RDONLY`.
pub const OPENFLAGS: i32 = O_RDONLY;

/// Default flags for creating (or overwriting) a file in read-write
/// mode.
///
/// Equivalent to POSIX `O_CREAT | O_RDWR`.
pub const CREATEFLAGS: i32 = O_CREAT | O_RDWR;

/// Open a file.
///
/// Translates POSIX-style `flags` into `CreateFileA` parameters
/// (access mask and creation disposition) and opens the file.  If
/// `O_CREAT` is set and `CREATE_ALWAYS` fails with
/// `ERROR_ALREADY_EXISTS`, the function falls back to
/// `OPEN_EXISTING` so the existing file is opened rather than
/// rejected.
///
/// # Arguments
///
/// * `path` — pointer to a null-terminated C string (ANSI path).
/// * `flags` — POSIX open flags (e.g. [`OPENFLAGS`], [`CREATEFLAGS`],
///   or a custom combination).  Only `O_RDONLY`, `O_WRONLY`,
///   `O_RDWR`, `O_CREAT`, and `O_EXCL` are recognised.
/// * `_mode` — ignored on Windows (permissions are controlled by
///   ACLs).
///
/// # Returns
///
/// A `RawFd` representing the Win32 HANDLE, or `u64::MAX` on error.
///
/// # Safety
///
/// `path` must point to a valid, null-terminated C string that
/// remains valid for the duration of this call.
pub unsafe fn sys_open(path: *const c_char, flags: i32, _mode: u32) -> RawFd {
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

/// Create a directory.
///
/// Calls `CreateDirectoryA` after normalising forward slashes to
/// backslashes and resolving the path to an absolute form via
/// [`to_lexical_absolute`].
///
/// # Arguments
///
/// * `path` — pointer to a null-terminated C string (ANSI path).
/// * `_mode` — ignored on Windows.
///
/// # Returns
///
/// `0` on success, or `u64::MAX` on error.
///
/// # Safety
///
/// `path` must point to a valid, null-terminated C string that
/// remains valid for the duration of this call.
pub unsafe fn sys_mkdir(path: *const c_char, _mode: u32) -> RawFd {
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

/// Resolve a path to a lexical absolute path without touching the
/// filesystem.
///
/// Processes `.` and `..` components purely lexically, prepending
/// the current working directory if the path is relative.  This
/// avoids the need for `GetFullPathName` which would follow
/// symlinks and require the path to exist.
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
