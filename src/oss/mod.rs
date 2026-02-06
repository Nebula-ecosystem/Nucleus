//! OS-specific poller backends.
//!
//! This module contains one sub-module per supported operating system,
//! each providing a [`Poller`] that wraps the native event notification
//! facility:
//!
//! | Sub-module | Kernel API | Wake mechanism |
//! |---|---|---|
//! | `linux` | `epoll` — `epoll_create1` / `epoll_ctl` / `epoll_wait` | `eventfd` registered as a persistent read source |
//! | `macos` | `kqueue` — `kqueue()` / `kevent()` | `EVFILT_USER` with `NOTE_TRIGGER` |
//! | `windows` | `WSAPoll` over non-blocking sockets | Loopback UDP socket pair (`send` to wake, `recv` to drain) |
//!
//! Only the sub-module that matches the compilation target is included
//! in the build.  The crate root re-exports the active backend as `os`.
//!
//! # Shared types
//!
//! The [`common`] module defines three types shared by every backend:
//!
//! * [`Interest`](common::poll::Interest) — read / write flags passed
//!   during registration.
//! * [`Event`](common::poll::Event) — token + readiness flags returned
//!   after polling.
//! * [`Waker`](common::poll::Waker) — a thin wrapper around the wake
//!   file descriptor / socket.  It is `Send + Sync` so the reactor can
//!   hand out clones to other threads.

pub(crate) mod common;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;
