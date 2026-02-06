//! Common types shared across all poller backends.
//!
//! This module re-exports nothing directly; all definitions live in
//! the [`poll`] sub-module.  It exists as a namespace so that every
//! OS-specific poller can import from a single, OS-independent
//! location:
//!
//! ```rust,ignore
//! use crate::oss::common::poll::{Event, Interest, Waker};
//! ```

pub(crate) mod poll;
