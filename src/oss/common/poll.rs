//! Platform-agnostic types for the I/O poller.
//!
//! This module is the **single source of truth** for the data types
//! that flow between the reactor and the OS-specific poller backends.
//! Every poller — regardless of whether it is backed by `epoll`,
//! `kqueue`, or `WSAPoll` — receives [`Interest`] values when
//! descriptors are registered and produces [`Event`] values when
//! readiness is detected.
//!
//! # Type overview
//!
//! | Type | Role | Direction |
//! |---|---|---|
//! | [`Interest`] | Desired readiness conditions (read and/or write) | reactor → poller |
//! | [`Event`] | Reported readiness for a given token | poller → reactor |
//! | [`Waker`] | Interrupt a blocking poll from another thread | any thread → poller |
//!
//! # Waker contract
//!
//! [`Waker`] wraps the file descriptor (or socket) that the
//! OS-specific backend uses as its internal wake-up channel.  The
//! only operation exposed is `wake()`, which is implemented per-OS in
//! each backend's `impl Waker` block.  The type deliberately does
//! **not** implement [`Drop`] — the `Poller` that created the
//! underlying descriptor is responsible for closing it.
//!
//! `Waker` is `Send + Sync`, so an [`Arc<Waker>`](std::sync::Arc) can
//! be cheaply shared with any number of executor or runtime threads
//! that need to signal the reactor.

use crate::platform::io::RawFd;

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
    pub(crate) token: usize,

    /// `true` when the descriptor has data available for reading, or
    /// when an error / hang-up condition makes a `read` call return
    /// immediately.
    #[allow(dead_code)]
    pub(crate) readable: bool,

    /// `true` when the descriptor can accept a write without blocking,
    /// or when an error / hang-up condition makes a `write` call
    /// return immediately.
    #[allow(dead_code)]
    pub(crate) writable: bool,
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
