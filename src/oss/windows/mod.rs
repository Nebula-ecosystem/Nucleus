//! Windows poller backend (`WSAPoll`).
//!
//! This module provides the [`Poller`](poll::Poller) implementation
//! used on Windows targets.  It delegates to the `WSAPoll` function
//! from WinSock for readiness-based I/O notification and uses a
//! loopback UDP socket pair for cross-thread wake-ups.
//!
//! See [`poll`] for the full API.

pub mod poll;
