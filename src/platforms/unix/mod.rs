//! Unix platform backend.
//!
//! Provides low-level system primitives implemented on top of `libc`
//! (POSIX).  This backend is active on Linux, macOS, and any other
//! `#[cfg(unix)]` target.
//!
//! # Sub-modules
//!
//! | Module | Contents |
//! |---|---|
//! | [`io`] | `RawFd` type alias, `sys_read`, `sys_write`, `sys_close` |
//! | [`fs`] | `sys_open`, `sys_mkdir`, `OPENFLAGS`, `CREATEFLAGS` |
//! | [`socket`] | Full TCP socket lifecycle and option helpers |
//! | [`address`] | `SocketAddr` ↔ `sockaddr_storage` conversions |
//! | [`utils`] | `sys_set_nonblocking`, `safe_close` |

pub mod address;
pub mod fs;
pub mod io;
pub mod socket;
pub mod utils;
