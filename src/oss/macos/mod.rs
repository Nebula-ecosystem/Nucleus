//! macOS poller backend (`kqueue`).
//!
//! This module provides the [`Poller`](poll::Poller) implementation
//! used on macOS (and BSD) targets.  It delegates to the kernel’s
//! `kqueue` facility for edge-triggered readiness notification and
//! uses an `EVFILT_USER` event for cross-thread wake-ups.
//!
//! See [`poll`] for the full API.

pub mod poll;
