//! macOS `kqueue`-based poller implementation.
//!
//! This module provides the macOS backend for Cadentis’ reactor.  It
//! is functionally equivalent to the Linux `epoll`
//! and Windows `WSAPoll` pollers and exposes
//! the same interface.
//!
//! # Kernel primitives
//!
//! | Primitive | Role |
//! |---|---|
//! | `kqueue()` | Create the event queue |
//! | `kevent()` (submit) | Add, modify, or delete monitored filters |
//! | `kevent()` (poll) | Block until readiness or timeout |
//! | `EVFILT_USER` + `NOTE_TRIGGER` | Cross-thread wake-up signal |
//!
//! # Responsibilities
//!
//! * Register and deregister file descriptors with `EVFILT_READ` /
//!   `EVFILT_WRITE` interests.
//! * Block the reactor thread until at least one descriptor is ready
//!   or a timeout expires.
//! * Wake the reactor from any thread when new commands are submitted
//!   to the command queue.
//! * Aggregate per-filter kernel events into the common `Event` type
//!   (kqueue reports read and write as separate events).
//!
//! # Wake-up protocol
//!
//! During construction the poller registers a persistent `EVFILT_USER`
//! event with identifier `WAKE_IDENT` (`1`).  A thread wanting to
//! wake the reactor triggers this event via `Waker::wake()` using
//! `NOTE_TRIGGER`.  On the next `kevent` return, the poller sees
//! `EVFILT_USER` and silently skips it — no `Event` is emitted.
//!
//! Unlike Linux’s `eventfd`-based scheme, the `EVFILT_USER` trigger
//! is auto-reset because the event is registered with `EV_CLEAR`.
//!
//! # Target selection
//!
//! This backend is compiled only when `target_os = "macos"` and is
//! re-exported by the crate root as `os`.

pub use crate::oss::common::poll::{Event, Interest, Waker};
use crate::platform::io::RawFd;

use libc::{
    EV_ADD, EV_CLEAR, EV_DELETE, EV_ENABLE, EVFILT_READ, EVFILT_USER, EVFILT_WRITE, NOTE_TRIGGER,
    c_long, kevent, kqueue, time_t, timespec,
};
use std::sync::Arc;
use std::time::Duration;
use std::{io, ptr};

/// macOS `kqueue` poller.
///
/// `Poller` is the macOS implementation of the reactor’s I/O
/// multiplexing backend.  It owns:
///
/// * **A `kqueue` instance** — the central event queue file
///   descriptor.
/// * **A persistent `EVFILT_USER` event** — registered once during
///   construction with `EV_ADD | EV_ENABLE | EV_CLEAR` and
///   identifier `WAKE_IDENT`.  This is the cross-thread wake-up
///   channel; triggering it costs a single `kevent` syscall.
/// * **A reusable event buffer** — pre-allocated with capacity 64.
///   On each poll round the buffer’s length is set to the kernel’s
///   reported event count.
///
/// # Aggregation
///
/// `kqueue` reports `EVFILT_READ` and `EVFILT_WRITE` as **separate**
/// events even when they fire in the same poll round.  The poller
/// merges them by matching on the `udata` field (which carries the
/// token) so the reactor always receives at most one `Event` per
/// token.
///
/// # Lifetime
///
/// The `kqueue` file descriptor is **not** closed on drop.  This is
/// acceptable because the reactor is expected to live for the entire
/// duration of the process.
///
/// # Thread safety
///
/// `Poller` is [`Send`] but not [`Sync`].  It must be driven from a
/// single thread (the reactor thread).  The `Waker` returned by
/// [`waker()`](Self::waker) is `Send + Sync` and safe to share.
pub struct Poller {
    /// The kqueue file descriptor.
    kqueue: RawFd,

    /// Internal buffer used to receive kevents.
    events: Vec<kevent>,

    /// Waker used to interrupt `kevent()` from another thread.
    waker: Arc<Waker>,
}

unsafe impl Send for Poller {}

impl Waker {
    /// Wake the poller.
    ///
    /// Triggers the persistent `EVFILT_USER` event registered as
    /// `WAKE_IDENT` by submitting a `kevent` with `NOTE_TRIGGER`.
    /// If the poller is blocked inside `kevent()`, it returns
    /// immediately; if the poller is not currently waiting, the
    /// trigger is latched (the event is `EV_CLEAR`, so the poller
    /// will see it once on the next round and then auto-reset).
    ///
    /// This method is safe to call from **any thread**, any number of
    /// times — redundant wakes are harmless (the poller simply skips
    /// `EVFILT_USER` events during event processing).
    pub fn wake(&self) {
        let event = kevent {
            ident: WAKE_IDENT,
            filter: EVFILT_USER,
            flags: 0,
            fflags: NOTE_TRIGGER,
            data: 0,
            udata: ptr::null_mut(),
        };

        unsafe {
            kevent(self.0, &event, 1, ptr::null_mut(), 0, ptr::null());
        }
    }
}

/// Identifier used for the reactor wake-up `EVFILT_USER` event.
///
/// This value is stored in the `ident` field of the kevent.  A small
/// constant (`1`) is used rather than `usize::MAX` because kqueue
/// event identifiers are per-filter, so there is no risk of collision
/// with file-descriptor–based `EVFILT_READ` / `EVFILT_WRITE` events.
const WAKE_IDENT: usize = 1;

impl Poller {
    /// Create a new `Poller` backed by macOS `kqueue`.
    ///
    /// This constructor performs two steps:
    ///
    /// 1. **`kqueue()`** — creates the kqueue file descriptor.
    /// 2. **`kevent(EV_ADD | EV_ENABLE | EV_CLEAR, EVFILT_USER)`** —
    ///    registers a persistent user event with identifier
    ///    `WAKE_IDENT` that other threads can trigger to wake the
    ///    poller.
    ///
    /// # Panics
    ///
    /// Panics if either syscall fails.  This is intentional: if the OS
    /// cannot provide the primitives the reactor needs, the runtime
    /// cannot function and an early, loud failure is preferable to a
    /// silent one.
    pub fn new() -> Self {
        let kqueue = unsafe { kqueue() };
        assert!(kqueue >= 0, "kqueue() failed");

        let event = kevent {
            ident: WAKE_IDENT,
            filter: EVFILT_USER,
            flags: EV_ADD | EV_ENABLE | EV_CLEAR,
            fflags: 0,
            data: 0,
            udata: ptr::null_mut(),
        };

        let ret = unsafe { kevent(kqueue, &event, 1, ptr::null_mut(), 0, ptr::null()) };
        assert!(ret == 0, "Failed to register EVFILT_USER");

        let events = Vec::with_capacity(64);
        let waker = Arc::new(Waker(kqueue));

        Poller {
            kqueue,
            events,
            waker,
        }
    }

    /// Return an [`Arc`]-wrapped `Waker` for this poller.
    ///
    /// The reactor calls this once during setup and distributes clones
    /// of the `Arc` to every component that may need to interrupt
    /// polling (e.g. the executor when a future becomes runnable, or
    /// a timer thread when a deadline expires).
    ///
    /// Calling `Waker::wake()` triggers the `EVFILT_USER` event,
    /// causing the next (or current) `kevent()` call to return.
    pub fn waker(&self) -> Arc<Waker> {
        self.waker.clone()
    }

    /// Register a file descriptor with the poller.
    ///
    /// Submits one or two `kevent` structures depending on `interest`:
    ///
    /// * `interest.read  == true`  ⇒  `EVFILT_READ`  with `EV_ADD | EV_ENABLE | EV_CLEAR`
    /// * `interest.write == true`  ⇒  `EVFILT_WRITE` with `EV_ADD | EV_ENABLE | EV_CLEAR`
    ///
    /// The `token` is stored in the `udata` field and will be returned
    /// verbatim in any future `Event` produced for this descriptor.
    /// It is typically a slab index that the reactor uses to look up
    /// the I/O entry.
    ///
    /// `EV_CLEAR` makes the filters edge-triggered: the kernel
    /// reports readiness once and then automatically resets, avoiding
    /// busy-loops on level-triggered notifications.
    pub fn register(&self, fd: RawFd, token: usize, interest: Interest) {
        let mut events = Vec::new();

        if interest.read {
            events.push(kevent {
                ident: fd as usize,
                filter: EVFILT_READ,
                flags: EV_ADD | EV_ENABLE | EV_CLEAR,
                fflags: 0,
                data: 0,
                udata: token as *mut _,
            });
        }

        if interest.write {
            events.push(kevent {
                ident: fd as usize,
                filter: EVFILT_WRITE,
                flags: EV_ADD | EV_ENABLE | EV_CLEAR,
                fflags: 0,
                data: 0,
                udata: token as *mut _,
            });
        }

        unsafe {
            kevent(
                self.kqueue,
                events.as_ptr(),
                events.len() as i32,
                ptr::null_mut(),
                0,
                ptr::null(),
            );
        }
    }

    /// Update interest flags for an already registered file descriptor.
    ///
    /// This deletes the existing `EVFILT_READ` and `EVFILT_WRITE`
    /// filters for `fd` and re-adds only the ones requested by
    /// `interest`.  The `token` is updated as well — the reactor may
    /// reassign tokens after connection migration.
    ///
    /// Deletion errors are silently ignored (the filter may not have
    /// been registered in the first place).
    pub fn reregister(&self, fd: RawFd, token: usize, interest: Interest) {
        // Remove previous interests individually.
        // We do this separately because if we batch them and the first one fails
        // (e.g. because it wasn't registered), the subsequent ones might be skipped
        // depending on the kernel implementation.
        let delete_read = kevent {
            ident: fd as usize,
            filter: EVFILT_READ,
            flags: EV_DELETE,
            fflags: 0,
            data: 0,
            udata: ptr::null_mut(),
        };
        unsafe {
            kevent(
                self.kqueue,
                &delete_read,
                1,
                ptr::null_mut(),
                0,
                ptr::null(),
            );
        }

        let delete_write = kevent {
            ident: fd as usize,
            filter: EVFILT_WRITE,
            flags: EV_DELETE,
            fflags: 0,
            data: 0,
            udata: ptr::null_mut(),
        };
        unsafe {
            kevent(
                self.kqueue,
                &delete_write,
                1,
                ptr::null_mut(),
                0,
                ptr::null(),
            );
        }

        // Re-add updated interests
        let mut changes = Vec::new();
        if interest.read {
            changes.push(kevent {
                ident: fd as usize,
                filter: EVFILT_READ,
                flags: EV_ADD | EV_ENABLE | EV_CLEAR,
                fflags: 0,
                data: 0,
                udata: token as *mut _,
            });
        }

        if interest.write {
            changes.push(kevent {
                ident: fd as usize,
                filter: EVFILT_WRITE,
                flags: EV_ADD | EV_ENABLE | EV_CLEAR,
                fflags: 0,
                data: 0,
                udata: token as *mut _,
            });
        }

        if !changes.is_empty() {
            unsafe {
                kevent(
                    self.kqueue,
                    changes.as_ptr(),
                    changes.len() as i32,
                    ptr::null_mut(),
                    0,
                    ptr::null(),
                );
            }
        }
    }

    /// Remove a file descriptor from the poller.
    ///
    /// Deletes both `EVFILT_READ` and `EVFILT_WRITE` for `fd`.  After
    /// this call the poller will no longer report readiness events for
    /// the descriptor.
    ///
    /// Deletion errors are silently ignored because the descriptor may
    /// already have been closed by user code (the kernel automatically
    /// removes closed descriptors from kqueue).
    pub fn deregister(&self, fd: RawFd) {
        let events = [
            kevent {
                ident: fd as usize,
                filter: EVFILT_READ,
                flags: EV_DELETE,
                fflags: 0,
                data: 0,
                udata: ptr::null_mut(),
            },
            kevent {
                ident: fd as usize,
                filter: EVFILT_WRITE,
                flags: EV_DELETE,
                fflags: 0,
                data: 0,
                udata: ptr::null_mut(),
            },
        ];

        unsafe {
            kevent(
                self.kqueue,
                events.as_ptr(),
                events.len() as i32,
                ptr::null_mut(),
                0,
                ptr::null(),
            );
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
    /// per ready descriptor.  Because `kqueue` reports `EVFILT_READ`
    /// and `EVFILT_WRITE` as separate kernel events, the poller
    /// aggregates them by token before returning, so the reactor
    /// always receives at most one `Event` per I/O resource.
    ///
    /// # Wake-up handling
    ///
    /// If the poller encounters an `EVFILT_USER` event, it is silently
    /// skipped (no `Event` is emitted).  The filter auto-resets thanks
    /// to `EV_CLEAR`.
    ///
    /// # Errors
    ///
    /// Returns [`Err`] on any `kevent` error **except** `EINTR`,
    /// which is silently swallowed (the caller should simply retry).
    ///
    /// # Timeout precision
    ///
    /// The timeout is converted to a `timespec` with nanosecond
    /// granularity, so sub-millisecond timeouts are supported on
    /// macOS (unlike the Linux `epoll` backend).
    pub fn poll(&mut self, events: &mut Vec<Event>, timeout: Option<Duration>) -> io::Result<()> {
        let ts;
        let timespec_ptr = match timeout {
            Some(t) => {
                ts = timespec {
                    tv_sec: t.as_secs() as time_t,
                    tv_nsec: t.subsec_nanos() as c_long,
                };
                &ts as *const timespec
            }
            None => ptr::null(),
        };

        unsafe {
            self.events.set_len(self.events.capacity());
        }

        let n = unsafe {
            kevent(
                self.kqueue,
                ptr::null(),
                0,
                self.events.as_mut_ptr(),
                self.events.capacity() as i32,
                timespec_ptr,
            )
        };

        if n < 0 {
            let error = io::Error::last_os_error();

            if error.kind() == io::ErrorKind::Interrupted {
                return Ok(());
            }

            return Err(error);
        }

        let n = n as usize;

        unsafe {
            self.events.set_len(n);
        }

        events.clear();

        // Aggregate read/write readiness per token
        for event in &self.events[..n] {
            if event.filter == EVFILT_USER {
                continue;
            }

            let token = event.udata as usize;
            let entry = events.iter_mut().find(|e| e.token == token);

            match entry {
                Some(e) => {
                    if event.filter == EVFILT_READ {
                        e.readable = true;
                    }

                    if event.filter == EVFILT_WRITE {
                        e.writable = true;
                    }
                }
                None => {
                    events.push(Event {
                        token,
                        readable: event.filter == EVFILT_READ,
                        writable: event.filter == EVFILT_WRITE,
                    });
                }
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
