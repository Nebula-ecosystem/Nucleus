//! Platform-agnostic types for event-driven I/O polling.
//!
//! This module defines the core type system used by all OS-specific poller
//! implementations across the Nebula runtime. It provides uniform abstractions
//! over epoll (Linux), kqueue (macOS), and WSAPoll (Windows), ensuring that
//! the reactor can operate identically regardless of the underlying kernel API.
//!
//! # Purpose
//!
//! The types defined here represent I/O readiness metadata that flows between
//! the reactor and the kernel. They are designed to be:
//! - **lightweight**: no heap allocation, copyable
//! - **explicit**: clear semantics for registration and notification
//! - **portable**: identical behavior on all supported platforms
//! - **opaque**: tokens are reactor-managed identifiers, not raw descriptors
//!
//! Every poller backend — whether backed by epoll's edge-triggered events,
//! kqueue's filter abstraction, or WSAPoll's readiness-based model — consumes
//! and produces these types, allowing the reactor to remain platform-agnostic.
//!
//! # Type overview
//!
//! ## [`Interest`]
//!
//! Specifies the I/O directions a file descriptor (or socket) should be
//! monitored for. It carries two boolean flags:
//! - `read`: monitor for incoming data or connection-accepted
//! - `write`: monitor for buffer space available or non-blocking connect
//!
//! An `Interest` is provided when a descriptor is **registered** and may be
//! updated later via **reregister**. The OS translates these flags into
//! kernel-specific event masks (e.g. `EPOLLIN | EPOLLOUT`).
//!
//! ## [`Event`]
//!
//! Represents a single readiness notification returned by the poller. Each
//! event contains:
//! - `token`: an opaque identifier (typically a slab index) that maps the
//!   event back to the corresponding I/O resource in the reactor
//! - `readable` / `writable`: booleans indicating which directions became ready
//!
//! Multiple kernel events for the same descriptor may be merged into one
//! `Event` with the union of readiness flags before being delivered to the
//! reactor.
//!
//! ## [`Waker`]
//!
//! A thread-safe handle to the poller's internal wake-up channel. Calling
//! `Waker::wake()` from any thread causes a blocked `Poller::poll()` to
//! return immediately, even if no I/O events are pending.
//!
//! # Ownership and lifecycle
//!
//! - **`Interest`** and **`Event`** are value types (Copy). They are passed
//!   by value and do not own any resources.
//! - **`Waker`** wraps a file descriptor (Unix) or socket handle (Windows)
//!   and is `Send + Sync`. It deliberately does **not** implement `Drop`;
//!   the `Poller` that created the wake descriptor is responsible for
//!   cleanup.
//!
//! # Relationship with `std::task::Waker`
//!
//! This `Waker` is a **reactor-internal** primitive unrelated to the
//! `std::task::Waker` used by async executors. The bridge between the two
//! is the executor itself: when an `std::task::Waker` is invoked, the
//! executor updates its internal task queue and then calls
//! `nucleus::Waker::wake()` to notify the reactor that new work is available.
//!
//! # Design constraints
//!
//! All types are intentionally minimal and do not:
//! - perform any OS calls directly
//! - own or manage file descriptors
//! - expose platform-specific details
//!
//! Platform-specific behavior is isolated to the backend implementations
//! (e.g. `oss::linux::poll::Poller`), not these shared types.

pub use crate::platform::io::RawFd;

/// I/O interest flags.
///
/// An `Interest` tells the poller which readiness conditions to
/// monitor for a given file descriptor.  It is provided when a
/// descriptor is first *registered* and may be changed later via
/// *reregister*.
///
/// Both fields are crate-private.  The reactor constructs `Interest`
/// values on behalf of user-facing I/O types.
///
/// # Possible combinations
///
/// | `read` | `write` | Meaning |
/// |---|---|---|
/// | `true` | `false` | Monitor for incoming data only |
/// | `false` | `true` | Monitor for write-readiness only |
/// | `true` | `true` | Monitor both directions |
#[derive(Clone, Copy)]
pub struct Interest {
    /// `true` to monitor for read readiness (incoming data or
    /// connection-accepted).
    pub(crate) read: bool,

    /// `true` to monitor for write readiness (buffer space available
    /// or non-blocking connect completed).
    pub(crate) write: bool,
}

impl Interest {
    /// Create an interest for read readiness only.
    pub fn read() -> Self {
        Self {
            read: true,
            write: false,
        }
    }

    /// Create an interest for write readiness only.
    pub fn write() -> Self {
        Self {
            read: false,
            write: true,
        }
    }

    /// Create an interest for both read and write readiness.
    pub fn both() -> Self {
        Self {
            read: true,
            write: true,
        }
    }
}

/// Low-level poller waker.
///
/// A `Waker` holds the file descriptor (Unix) or socket handle
/// (Windows) that the OS-specific poller backend uses as its internal
/// wake channel.  Writing to this descriptor causes the blocked poll
/// syscall (`epoll_wait`, `kevent`, or `WSAPoll`) to return
/// immediately, allowing the reactor to process new commands without
/// waiting for an I/O event or a timeout to fire.
///
/// # Thread safety
///
/// `Waker` is `Send + Sync`.  The idiomatic usage pattern is:
///
/// 1. The `Poller` creates the wake descriptor during construction.
/// 2. An `Arc<Waker>` is handed out via `Poller::waker()`.
/// 3. Any thread that needs to notify the reactor calls
///    `Waker::wake()`, whose implementation is provided by the
///    active OS backend.
///
/// # Ownership
///
/// The underlying descriptor is **not** closed when the `Waker` is
/// dropped.  The `Poller` that allocated the descriptor is
/// responsible for cleanup.
///
/// # Relationship with [`std::task::Waker`]
///
/// This type is a *reactor-internal* mechanism and is unrelated to
/// [`std::task::Waker`].  The executor bridges the two: when an
/// `std::task::Waker` is invoked, the executor enqueues the task and
/// then calls `Waker::wake()` to ensure the reactor re-polls.
pub struct Waker(pub(crate) RawFd);

unsafe impl Send for Waker {}
unsafe impl Sync for Waker {}

/// An I/O readiness event reported by the poller.
///
/// After each `Poller::poll()` call the reactor receives a
/// `Vec<Event>` describing which registered descriptors are ready.
/// Each event carries:
///
/// * **`token`** — the opaque identifier that was supplied during
///   `Poller::register()`.  The reactor uses it as a key into its
///   slab to locate the corresponding I/O resource and wake the
///   correct task.
/// * **`readable`** / **`writable`** — booleans indicating the
///   direction(s) of readiness.  On error or hang-up the OS may set
///   both flags so that the next read or write surfaces the error to
///   user code.
///
/// # Merging
///
/// If the same token appears in multiple raw kernel events within a
/// single poll round, the poller merges them into one `Event` with
/// the **union** of readiness flags.
pub struct Event {
    /// Opaque token identifying the registered I/O resource.
    ///
    /// This is the same value that was passed to `Poller::register()`
    /// and is typically a slab index managed by the reactor.
    pub token: usize,

    /// `true` when the descriptor has data available for reading, or
    /// when an error / hang-up condition makes a `read` call return
    /// immediately.
    pub readable: bool,

    /// `true` when the descriptor can accept a write without blocking,
    /// or when an error / hang-up condition makes a `write` call
    /// return immediately.
    pub writable: bool,
}

impl Event {
    /// Get the token associated with this event.
    pub fn token(&self) -> usize {
        self.token
    }

    /// Returns `true` if the descriptor is readable.
    pub fn is_readable(&self) -> bool {
        self.readable
    }

    /// Returns `true` if the descriptor is writable.
    pub fn is_writable(&self) -> bool {
        self.writable
    }
}
