//! Windows platform backend.
//!
//! Provides low-level system primitives implemented on top of
//! `windows-sys` (Win32 API).  This backend is active when
//! `#[cfg(windows)]`.
//!
//! # Sub-modules
//!
//! | Module | Contents |
//! |---|---|
//! | [`io`] | `RawFd` type alias, `sys_read` / `sys_write` / `sys_close` (auto-dispatches between file HANDLEs and WinSock SOCKETs) |
//! | [`fs`] | `sys_open` / `sys_mkdir` / `storage_left` with POSIX-to-Win32 flag translation |
//! | [`socket`] | Full TCP socket lifecycle and option helpers (WinSock) |
//! | [`address`] | `SocketAddr` ↔ `SOCKADDR_STORAGE` conversions |
//! | [`utils`] | `is_socket`, `makeword`, `ensure_winsock` |

pub mod address;
pub mod fs;
pub mod io;
pub mod socket;
pub mod utils;
