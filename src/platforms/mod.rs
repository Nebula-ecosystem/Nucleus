//! Platform abstraction layer.
//!
//! This module contains one sub-module per supported platform family.
//! Each sub-module exposes the **same set of child modules** so that
//! the rest of the crate can import from `crate::platform::*` without
//! knowing which OS is active:
//!
//! | Child module | Provides |
//! |---|---|
//! | [`io`](unix::io) | `RawFd` type alias and thin I/O wrappers (`sys_read`, `sys_write`, `sys_close`) |
//! | [`fs`](unix::fs) | File-system operations (`sys_open`, `sys_mkdir`, `storage_left`) and default open-flag constants |
//! | [`socket`](unix::socket) | Complete TCP socket lifecycle — create, bind, listen, accept, connect, shutdown — plus common socket options |
//! | [`address`](unix::address) | Bidirectional conversion between [`std::net::SocketAddr`] and the OS-level `sockaddr_storage` |
//! | [`utils`](unix::utils) | Platform-specific helpers (non-blocking mode, Winsock init, …) |
//!
//! # Compile-time selection
//!
//! The crate root imports the active backend as `crate::platform`:
//!
//! ```rust,ignore
//! #[cfg(unix)]
//! pub(crate) use platforms::unix as platform;
//!
//! #[cfg(windows)]
//! pub(crate) use platforms::windows as platform;
//! ```
//!
//! This means a consumer only ever writes
//! `crate::platform::io::RawFd` and the correct definition is
//! selected automatically.

#[cfg(unix)]
pub mod unix;

#[cfg(windows)]
pub mod windows;
