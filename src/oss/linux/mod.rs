//! Linux poller backend (`epoll`).
//!
//! This module provides the [`Poller`](poll::Poller) implementation
//! used on Linux targets.  It delegates to the kernel's `epoll`
//! facility for O(1) readiness notification and uses an `eventfd`
//! for cross-thread wake-ups.
//!
//! See [`poll`] for the full API.

pub mod poll;
