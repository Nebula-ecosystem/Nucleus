//! Linux `epoll`-based poller implementation.
//!
//! This module provides the Linux backend for Cadentis’ reactor.  It
//! is functionally equivalent to the macOS `kqueue`
//! and Windows `WSAPoll` pollers and exposes
//! the same interface.
//!
//! # Kernel primitives
//!
//! | Primitive | Role |
//! |---|---|
//! | `epoll_create1(EPOLL_CLOEXEC)` | Create the event set |
//! | `epoll_ctl(ADD / MOD / DEL)` | Manage per-fd interest masks |
//! | `epoll_wait` | Block until readiness or timeout |
//! | `eventfd(EFD_NONBLOCK \| EFD_CLOEXEC)` | Cross-thread wake-up channel |
//!
//! # Responsibilities
//!
//! * Register and deregister file descriptors with read/write
//!   interests.
//! * Block the reactor thread until at least one descriptor is ready
//!   or a timeout expires.
//! * Wake the reactor from any thread when new commands are submitted
//!   to the command queue.
//! * Translate raw `epoll_event` structures into the common
//!   `Event` type.
//!
//! # Wake-up protocol
//!
//! During construction the poller creates a non-blocking `eventfd`
//! and permanently registers it in the epoll set with the reserved
//! token `WAKE_TOKEN` (`u64::MAX`).  A thread wanting to wake the
//! reactor writes `1u64` to the eventfd via `Waker::wake()`.  On
//! the next `epoll_wait` return, the poller detects `WAKE_TOKEN`,
//! reads from the eventfd to reset its counter, and **does not**
//! forward the event to the reactor.
//!
//! # Target selection
//!
//! This backend is compiled only when `target_os = "linux"` and is
//! re-exported by the crate root as `os`.

// Re-export
pub use crate::oss::common::poll::{Event, Interest, Waker};
use crate::platform::io::RawFd;

use libc::{
    EPOLL_CLOEXEC, EPOLL_CTL_ADD, EPOLL_CTL_DEL, EPOLL_CTL_MOD, EPOLLERR, EPOLLHUP, EPOLLIN,
    EPOLLOUT, epoll_create1, epoll_ctl, epoll_event, epoll_wait,
};
use std::io;
use std::sync::Arc;
use std::time::Duration;

/// Reserved token used internally for the wake-up `eventfd`.
///
/// The token is stored inside every `epoll_event` registered for the
/// eventfd.  When `epoll_wait` returns an event with this token, the
/// poller knows it is a wake-up signal rather than application I/O.
///
/// `u64::MAX` is chosen because valid reactor tokens are slab indices
/// that start at zero and grow upward — a collision is therefore
/// impossible in practice.
const WAKE_TOKEN: u64 = u64::MAX;

/// Linux `epoll` poller.
///
/// `Poller` is the Linux implementation of the reactor’s I/O
/// multiplexing backend.  It owns:
///
/// * **An `epoll` instance** — created with `EPOLL_CLOEXEC` to
///   prevent the descriptor from leaking across `exec` boundaries.
/// * **An `eventfd`** — configured as non-blocking with
///   `EFD_NONBLOCK | EFD_CLOEXEC`.  This descriptor is permanently
///   registered in the epoll set with interest `EPOLLIN` and the
///   token `WAKE_TOKEN`, so that any thread can interrupt
///   `epoll_wait` by writing to it.
/// * **A reusable event buffer** — pre-allocated with capacity 64.
///   On each poll round the buffer’s length is set to the kernel’s
///   reported event count.
///
/// # Lifetime
///
/// The `epoll` and `eventfd` file descriptors are **not** closed on
/// drop.  This is acceptable because the reactor is expected to live
/// for the entire duration of the process.
///
/// # Thread safety
///
/// `Poller` is [`Send`] but not [`Sync`].  It must be driven from a
/// single thread (the reactor thread).  The `Waker` returned by
/// [`waker()`](Self::waker) is `Send + Sync` and safe to share.
pub struct Poller {
    /// Epoll file descriptor.
    epoll: RawFd,

    /// Reusable buffer for epoll events.
    events: Vec<epoll_event>,

    /// Waker wrapping the internal eventfd.
    waker: Arc<Waker>,
}

unsafe impl Send for Poller {}

impl Waker {
    /// Wake the poller.
    ///
    /// Writes a `1u64` to the internal `eventfd`, which increments
    /// its counter and makes the descriptor readable.  If the poller
    /// is blocked inside `epoll_wait`, it returns immediately; if the
    /// poller is not currently waiting, the write is accumulated and
    /// the next `epoll_wait` call will return without blocking.
    ///
    /// This method is safe to call from **any thread**, any number of
    /// times — redundant wakes are harmless (the poller simply drains
    /// the eventfd counter on the next poll round).
    pub fn wake(&self) {
        let buf: u64 = 1;
        unsafe {
            libc::write(self.0, &buf as *const _ as *const _, 8);
        }
    }
}

impl Poller {
    /// Create a new `Poller` backed by Linux `epoll`.
    ///
    /// This constructor performs three steps:
    ///
    /// 1. **`epoll_create1(EPOLL_CLOEXEC)`** — obtains the epoll file
    ///    descriptor.
    /// 2. **`eventfd(0, EFD_NONBLOCK | EFD_CLOEXEC)`** — creates the
    ///    wake-up descriptor that other threads will write to.
    /// 3. **`epoll_ctl(EPOLL_CTL_ADD)`** — registers the eventfd in
    ///    the epoll set as a persistent `EPOLLIN` source with the
    ///    reserved token `WAKE_TOKEN`.
    ///
    /// # Panics
    ///
    /// Panics if any of the three syscalls fails.  This is intentional:
    /// if the OS cannot provide the primitives the reactor needs, the
    /// runtime cannot function and an early, loud failure is preferable
    /// to a silent one.
    pub fn new() -> Self {
        let epoll = unsafe { epoll_create1(EPOLL_CLOEXEC) };
        assert!(epoll >= 0, "epoll_create1 failed");

        let eventfd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        assert!(eventfd >= 0, "eventfd failed");

        let mut event = epoll_event {
            events: EPOLLIN as u32,
            u64: WAKE_TOKEN,
        };

        let rc = unsafe { epoll_ctl(epoll, EPOLL_CTL_ADD, eventfd, &mut event) };
        assert!(rc == 0, "failed to register wake eventfd");

        Self {
            epoll,
            events: Vec::with_capacity(64),
            waker: Arc::new(Waker(eventfd)),
        }
    }

    /// Return an [`Arc`]-wrapped `Waker` for this poller.
    ///
    /// The reactor calls this once during setup and distributes clones
    /// of the `Arc` to every component that may need to interrupt
    /// polling (e.g. the executor when a future becomes runnable, or
    /// a timer thread when a deadline expires).
    ///
    /// Calling `Waker::wake()` writes to the internal `eventfd`,
    /// causing the next (or current) `epoll_wait` to return.
    pub fn waker(&self) -> Arc<Waker> {
        self.waker.clone()
    }

    /// Register a file descriptor with the poller.
    ///
    /// Calls `epoll_ctl(EPOLL_CTL_ADD)` with an event mask derived
    /// from `interest`:
    ///
    /// * `interest.read  == true`  ⇒  `EPOLLIN`
    /// * `interest.write == true`  ⇒  `EPOLLOUT`
    ///
    /// The `token` is stored inside the `epoll_event` and will be
    /// returned verbatim in any future `Event` produced for this
    /// descriptor.  It is typically a slab index that the reactor
    /// uses to look up the I/O entry.
    ///
    /// # Panics (debug)
    ///
    /// In debug builds, panics if `epoll_ctl` returns an error (e.g.
    /// the descriptor is already registered or is invalid).
    pub fn register(&self, fd: RawFd, token: usize, interest: Interest) {
        let mut flags = 0;

        if interest.read {
            flags |= EPOLLIN;
        }
        if interest.write {
            flags |= EPOLLOUT;
        }

        let mut event = epoll_event {
            events: flags as u32,
            u64: token as u64,
        };

        let rc = unsafe { epoll_ctl(self.epoll, EPOLL_CTL_ADD, fd, &mut event) };
        debug_assert_eq!(rc, 0);
    }

    /// Update interest flags for an already registered file descriptor.
    ///
    /// This deletes the existing interest for `fd` and re-adds only the
    /// ones requested by `interest`.  The `token` is updated as well.
    ///
    /// Deletion errors are silently ignored.
    pub fn reregister(&self, fd: RawFd, token: usize, interest: Interest) {
        unsafe {
            epoll_ctl(self.epoll, EPOLL_CTL_DEL, fd, std::ptr::null_mut());
        }

        let mut flags = 0;

        if interest.read {
            flags |= EPOLLIN;
        }
        if interest.write {
            flags |= EPOLLOUT;
        }

        if flags == 0 {
            return;
        }

        let mut event = epoll_event {
            events: flags as u32,
            u64: token as u64,
        };

        let rc = unsafe { epoll_ctl(self.epoll, EPOLL_CTL_ADD, fd, &mut event) };
        debug_assert_eq!(rc, 0);
    }

    /// Remove a file descriptor from the poller.
    ///
    /// Calls `epoll_ctl(EPOLL_CTL_DEL)`.  After this call the poller
    /// will no longer report readiness events for `fd`.
    ///
    /// Any error from `epoll_ctl` is silently ignored because the
    /// file descriptor may already have been closed by user code (the
    /// kernel automatically removes closed descriptors from epoll
    /// sets).
    pub fn deregister(&self, fd: RawFd) {
        unsafe {
            epoll_ctl(self.epoll, EPOLL_CTL_DEL, fd, std::ptr::null_mut());
        }
    }

    /// Poll for I/O readiness events.
    ///
    /// Blocks the calling thread until at least one of the following
    /// occurs:
    ///
    /// * A registered file descriptor becomes readable or writable.
    /// * The internal `Waker` is triggered by another thread.
    /// * The optional `timeout` duration elapses (`None` means wait
    ///   indefinitely).
    ///
    /// On return, `events` is cleared and filled with one `Event`
    /// per ready descriptor.  If multiple raw kernel events carry
    /// the same token (e.g. simultaneous read + error), they are
    /// merged into a single `Event` with both flags set.
    ///
    /// # Wake-up handling
    ///
    /// If the poller detects readiness on the wake `eventfd`
    /// (`WAKE_TOKEN`), it reads from the eventfd to reset its
    /// counter and **does not** emit an `Event`.  This allows the
    /// reactor to distinguish I/O readiness from explicit wake-ups.
    ///
    /// # Errors
    ///
    /// Returns [`Err`] on any `epoll_wait` error **except** `EINTR`,
    /// which is silently swallowed (the caller should simply retry).
    ///
    /// # Timeout precision
    ///
    /// The timeout is converted to milliseconds via
    /// [`Duration::as_millis()`] and truncated to `i32`.  Sub-
    /// millisecond precision is therefore not available.
    pub fn poll(&mut self, events: &mut Vec<Event>, timeout: Option<Duration>) -> io::Result<()> {
        let timeout_ms = timeout.map(|t| t.as_millis() as i32).unwrap_or(-1);

        unsafe {
            self.events.set_len(self.events.capacity());
        }

        let n = unsafe {
            epoll_wait(
                self.epoll,
                self.events.as_mut_ptr(),
                self.events.capacity() as i32,
                timeout_ms,
            )
        };

        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                return Ok(());
            }
            return Err(err);
        }

        unsafe {
            self.events.set_len(n as usize);
        }

        events.clear();

        for ev in &self.events {
            // Wake-up event
            if ev.u64 == WAKE_TOKEN {
                let mut buf = 0u64;
                unsafe {
                    libc::read(self.waker.0, &mut buf as *mut _ as *mut _, 8);
                }
                continue;
            }

            let token = ev.u64 as usize;

            let readable = ev.events & ((EPOLLIN | EPOLLERR | EPOLLHUP) as u32) != 0;
            let writable = ev.events & (EPOLLOUT as u32) != 0;

            if let Some(e) = events.iter_mut().find(|e| e.token == token) {
                e.readable |= readable;
                e.writable |= writable;
            } else {
                events.push(Event {
                    token,
                    readable,
                    writable,
                });
            }
        }

        Ok(())
    }
}

impl Default for Poller {
    fn default() -> Self {
        Self::new()
    }
}
