//! **Nucleus** — portable system primitives for the Cadentis async runtime.
//!
//! Nucleus is the lowest layer of the Nebula stack.  It wraps raw
//! operating-system calls behind a uniform Rust interface so that the
//! reactor, executor, and higher-level I/O types never touch
//! platform APIs directly.
//!
//! # Architecture
//!
//! The crate is organised into two complementary layers that are
//! compiled independently for each target:
//!
//! | Layer | Responsibility | Unix | Windows |
//! |---|---|---|---|
//! | **`platforms`** | Syscall wrappers — fd I/O, sockets, filesystem | `libc` (POSIX) | Win32 (`windows-sys`) |
//! | **`oss`** | Event-driven I/O poller for the reactor | `epoll` (Linux) / `kqueue` (macOS) | `WSAPoll` |
//!
//! At compile time, `cfg` gates select the correct backend and
//! re-export it under two **crate-level aliases**:
//!
//! * **`platform`** — resolves to `platforms::unix` *or*
//!   `platforms::windows`.
//! * **`os`** — resolves to `oss::linux`, `oss::macos`, *or*
//!   `oss::windows`.
//!
//! All downstream code imports through these aliases, making the rest
//! of Cadentis entirely platform-agnostic.
//!
//! # Platform primitives (`platform::*`)
//!
//! | Module | Contents |
//! |---|---|
//! | `io` | `RawFd` type alias, `sys_read`, `sys_write`, `sys_close` |
//! | `fs` | `sys_open`, `sys_mkdir`, open-flag constants |
//! | `socket` | Full TCP socket lifecycle — create, bind, listen, accept, connect, shutdown — plus socket options (`SO_REUSEADDR`, `IPV6_V6ONLY`, …) |
//! | `address` | Bidirectional conversion between [`std::net::SocketAddr`] and the OS-level `sockaddr_storage` / `SOCKADDR_STORAGE` |
//! | `utils` | Helpers — `sys_set_nonblocking` / `safe_close` (Unix) or `ensure_winsock` / `is_socket` (Windows) |
//!
//! # Poller backends (`os::poll`)
//!
//! Every backend exposes the same contract through three shared types
//! defined in `oss::common::poll`:
//!
//! * **`Poller`** — owns the OS event source, maintains a descriptor
//!   registry, and converts raw kernel events into `Event` values.
//! * **`Waker`** — a lightweight, `Send + Sync` handle that can
//!   interrupt a blocking poll from any thread.
//! * **`Interest`** / **`Event`** — value types that describe
//!   requested and reported I/O readiness.
//!
//! # Platform support
//!
//! | OS | Platform backend | Poller | Wake mechanism |
//! |---|---|---|---|
//! | Linux | `libc` | `epoll` | `eventfd` |
//! | macOS | `libc` | `kqueue` | `EVFILT_USER` |
//! | Windows | `windows-sys` (Win32) | `WSAPoll` | loopback UDP socket pair |
//!
//! # Thread safety
//!
//! `Poller` is [`Send`] (and additionally [`Sync`] on Windows).  It is
//! designed to be owned by a **single reactor thread**.  The associated
//! `Waker` is `Send + Sync` and can be cloned freely to let any thread
//! or task signal the reactor.
//!
//! All functions in the `platform` modules are stateless and can be
//! called from any thread without synchronisation.

pub(crate) mod oss;
pub(crate) mod platforms;

#[cfg(target_os = "linux")]
pub(crate) use oss::linux as os;

#[cfg(target_os = "macos")]
pub(crate) use oss::macos as os;

#[cfg(target_os = "windows")]
pub(crate) use oss::windows as os;

#[cfg(unix)]
pub(crate) use platforms::unix as platform;

#[cfg(windows)]
pub(crate) use platforms::windows as platform;

pub use os::*;
pub use platform::*;
