//! Linux `epoll`-based poller implementation.
//!
//! This module provides the Linux backend for Cadentis’ reactor.
//! It is functionally equivalent to the macOS `kqueue` poller and
//! exposes the same interface to the reactor.
//!
//! Responsibilities:
//! - Register file descriptors with read/write interests
//! - Block waiting for I/O readiness
//! - Wake the reactor when new commands are submitted
//! - Support timer-driven wakeups via poll timeouts
//!
//! This backend is selected automatically on Linux targets.

use crate::os_common::poll::{Interest, Waker};
use crate::platform::default::{Event, RawFd};

use libc::{
    EPOLL_CLOEXEC, EPOLL_CTL_ADD, EPOLL_CTL_DEL, EPOLL_CTL_MOD, EPOLLERR, EPOLLHUP, EPOLLIN,
    EPOLLOUT, epoll_create1, epoll_ctl, epoll_event, epoll_wait,
};
use std::io;
use std::sync::Arc;
use std::time::Duration;

/// Reserved token used internally for the wake-up event.
///
/// This value must never collide with tokens produced by the slab.
/// Using `u64::MAX` guarantees uniqueness.
const WAKE_TOKEN: u64 = u64::MAX;

/// Linux `epoll` poller.
///
/// This poller owns:
/// - an `epoll` instance,
/// - an internal `eventfd` used as a wake-up signal,
/// - a reusable event buffer.
///
/// The wake-up mechanism allows other threads (executor / reactor handle)
/// to interrupt a blocking `epoll_wait()` call.
pub struct EpollPoller {
    /// Epoll file descriptor.
    epoll: RawFd,

    /// Reusable buffer for epoll events.
    events: Vec<epoll_event>,

    /// Waker wrapping the internal eventfd.
    waker: Arc<Waker>,
}

unsafe impl Send for EpollPoller {}

impl Waker {
    /// Wake the poller.
    ///
    /// This writes to the internal `eventfd`, causing `epoll_wait`
    /// to return immediately.
    pub fn wake(&self) {
        let buf: u64 = 1;
        unsafe {
            libc::write(self.0, &buf as *const _ as *const _, 8);
        }
    }
}

impl EpollPoller {
    /// Create a new `EpollPoller`.
    ///
    /// This:
    /// - creates the epoll instance,
    /// - creates a non-blocking `eventfd`,
    /// - registers the eventfd into epoll as a persistent wake source.
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

    /// Return the poller waker.
    ///
    /// The reactor uses this to interrupt `epoll_wait` when commands arrive.
    pub fn waker(&self) -> Arc<Waker> {
        self.waker.clone()
    }

    /// Register a file descriptor with the poller.
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

    /// Update interest flags for an already registered descriptor.
    pub fn reregister(&self, fd: RawFd, token: usize, interest: Interest) {
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

        let rc = unsafe { epoll_ctl(self.epoll, EPOLL_CTL_MOD, fd, &mut event) };
        debug_assert_eq!(rc, 0);
    }

    /// Remove a file descriptor from the poller.
    pub fn deregister(&self, fd: RawFd) {
        unsafe {
            epoll_ctl(self.epoll, EPOLL_CTL_DEL, fd, std::ptr::null_mut());
        }
    }

    /// Poll for I/O readiness events.
    ///
    /// Blocks until:
    /// - at least one file descriptor becomes ready,
    /// - the wake event is triggered,
    /// - or the optional timeout expires.
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

impl Default for EpollPoller {
    fn default() -> Self {
        Self::new()
    }
}
