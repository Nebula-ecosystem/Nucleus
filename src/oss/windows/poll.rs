//! Windows `WSAPoll`-based poller implementation.
//!
//! This module provides a readiness-based Windows backend for
//! Cadentis’ reactor.  It mirrors the semantics of the Linux
//! `epoll` and macOS
//! `kqueue` pollers using non-blocking
//! sockets and `WSAPoll`.
//!
//! # Kernel primitives
//!
//! | Primitive | Role |
//! |---|---|
//! | `WSASocketW` | Create UDP sockets for the wake channel |
//! | `ioctlsocket(FIONBIO)` | Set sockets to non-blocking mode |
//! | `WSAPoll` | Block until readiness or timeout |
//! | `send` / `recv` | Signal and drain the wake channel |
//!
//! # Responsibilities
//!
//! * Register and deregister sockets with read/write interests via
//!   an in-memory `HashMap`.
//! * Block the reactor thread until at least one socket is ready or
//!   a timeout expires.
//! * Wake the reactor from any thread when new commands are submitted
//!   to the command queue.
//! * Translate raw `WSAPOLLFD` results into the common `Event`
//!   type.
//!
//! # Wake-up protocol
//!
//! During construction the poller creates two non-blocking UDP
//! sockets bound to `127.0.0.1` and connected to each other.  The
//! *send* side is cloned into a `Waker`; calling
//! `Waker::wake()` sends a single byte.  On the next `WSAPoll`
//! return, the poller detects `POLLIN` on the *receive* side, drains
//! all pending bytes, and **does not** emit an `Event`.
//!
//! Unlike Linux’s `eventfd` or macOS’s `EVFILT_USER`, Windows has no
//! kernel-level "poke" primitive for `WSAPoll`, so this loopback
//! socket pair is the cheapest portable alternative.
//!
//! # Readiness vs. completion
//!
//! This poller is **readiness-based** and does not use overlapped or
//! IOCP-style completion I/O.  It is intended for semantic parity
//! with the Unix backends; a future IOCP backend would replace it
//! for production workloads on Windows.
//!
//! # Target selection
//!
//! This backend is compiled only when `target_os = "windows"` and
//! is re-exported by the crate root as `os`.

// Re-export
pub use crate::oss::common::poll::{Event, Interest, Waker};
use crate::platform::{io::RawFd, utils::ensure_winsock};

use std::collections::HashMap;
use std::io;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use windows_sys::Win32::Networking::WinSock::{
    AF_INET, FIONBIO, IPPROTO_UDP, POLLERR, POLLHUP, POLLIN, POLLNVAL, POLLOUT, SOCK_DGRAM,
    SOCKADDR_IN, SOCKET, SOCKET_ERROR, WSAPOLLFD, WSAPoll, WSASocketW, bind, closesocket, connect,
    getsockname, ioctlsocket, recv, send,
};

/// Windows poller based on `WSAPoll`.
///
/// `Poller` is the Windows implementation of the reactor’s I/O
/// multiplexing backend.  It owns:
///
/// * **A socket registry** — a `HashMap<RawFd, (token, Interest)>`
///   that records every monitored socket and its current interest
///   flags.  On each poll round this map is flattened into a
///   `Vec<WSAPOLLFD>` passed to `WSAPoll`.
/// * **A loopback UDP socket pair** — `wake_recv` (receive side) and
///   `wake_send` (send side), both bound to `127.0.0.1` on an
///   ephemeral port.  The send side is shared via `Waker`; writing
///   a byte to it makes `WSAPoll` return immediately.
/// * **An `Arc<Waker>`** — the thread-safe handle distributed to
///   executor and timer threads.
///
/// # Lifetime
///
/// The two wake-up sockets are closed in the [`Drop`] implementation
/// via `closesocket`.  Registered application sockets are **not**
/// closed by the poller — they belong to user code.
///
/// # Thread safety
///
/// `Poller` is [`Send`] **and** [`Sync`].  In practice it is still
/// owned by a single reactor thread, but the additional `Sync` bound
/// simplifies generic constraints on Windows.
pub struct Poller {
    /// Registered sockets: `fd → (token, interest)`.
    reg: HashMap<RawFd, (usize, Interest)>,

    /// Wake-up socket (receive side).
    wake_recv: SOCKET,

    /// Wake-up socket (send side).
    wake_send: SOCKET,

    /// Waker used by the reactor to interrupt polling.
    waker: Arc<Waker>,
}

unsafe impl Send for Poller {}
unsafe impl Sync for Poller {}

impl Waker {
    /// Wake the poller.
    ///
    /// Sends a single byte (`0x01`) on the internal UDP send socket.
    /// Because the corresponding receive socket is always included in
    /// the `WSAPOLLFD` array passed to `WSAPoll`, the call returns
    /// immediately with `POLLIN` on the receive side.
    ///
    /// This method is safe to call from **any thread**, any number of
    /// times — redundant wakes are harmless (the poller drains all
    /// pending bytes on the receive side before processing I/O
    /// events).
    pub fn wake(&self) {
        unsafe {
            let buf = [1u8; 1];
            let _ = send(self.0 as SOCKET, buf.as_ptr(), 1, 0);
        }
    }
}

impl Poller {
    /// Create a new `Poller` backed by Windows `WSAPoll`.
    ///
    /// This constructor performs four steps:
    ///
    /// 1. **`ensure_winsock()`** — one-time WinSock 2.2
    ///    initialisation (process-wide).
    /// 2. **`WSASocketW` (recv)** — creates a non-blocking UDP socket
    ///    bound to `127.0.0.1:0`; the OS assigns an ephemeral port.
    /// 3. **`WSASocketW` (send)** — creates a second non-blocking UDP
    ///    socket and `connect`s it to the bound address discovered
    ///    via `getsockname`.
    /// 4. Wraps the send socket in an `Arc<Waker>` so other threads
    ///    can wake the poller.
    ///
    /// # Panics
    ///
    /// Panics if any of the socket or bind/connect calls fail.  This
    /// is intentional: if the OS cannot provide the primitives the
    /// reactor needs, the runtime cannot function and an early, loud
    /// failure is preferable to a silent one.
    pub fn new() -> Self {
        unsafe {
            ensure_winsock();

            // --- Wake receiver socket ---
            let recv_sock = WSASocketW(
                AF_INET as i32,
                SOCK_DGRAM,
                IPPROTO_UDP,
                std::ptr::null(),
                0,
                0,
            );
            assert!(recv_sock != SOCKET_ERROR as usize);

            let mut nonblocking: u32 = 1;
            let _ = ioctlsocket(recv_sock, FIONBIO, &mut nonblocking);

            let mut addr: SOCKADDR_IN = std::mem::zeroed();
            addr.sin_family = AF_INET;
            addr.sin_port = 0;
            addr.sin_addr.S_un.S_addr = u32::from_ne_bytes(Ipv4Addr::LOCALHOST.octets());

            let rc = bind(
                recv_sock,
                &addr as *const _ as *const _,
                std::mem::size_of::<SOCKADDR_IN>() as i32,
            );
            assert!(rc != SOCKET_ERROR);

            // Discover the bound port
            let mut bound: SOCKADDR_IN = std::mem::zeroed();
            let mut len = std::mem::size_of::<SOCKADDR_IN>() as i32;

            let rc = getsockname(recv_sock, &mut bound as *mut _ as *mut _, &mut len);
            assert!(rc != SOCKET_ERROR);

            // --- Wake sender socket ---
            let send_sock = WSASocketW(
                AF_INET as i32,
                SOCK_DGRAM,
                IPPROTO_UDP,
                std::ptr::null(),
                0,
                0,
            );
            assert!(send_sock != SOCKET_ERROR as usize);

            let _ = ioctlsocket(send_sock, FIONBIO, &mut nonblocking);

            let rc = connect(
                send_sock,
                &bound as *const _ as *const _,
                std::mem::size_of::<SOCKADDR_IN>() as i32,
            );
            assert!(rc != SOCKET_ERROR);

            Self {
                reg: HashMap::new(),
                wake_recv: recv_sock,
                wake_send: send_sock,
                waker: Arc::new(Waker(send_sock as RawFd)),
            }
        }
    }

    /// Return an [`Arc`]-wrapped `Waker` for this poller.
    ///
    /// The reactor calls this once during setup and distributes clones
    /// of the `Arc` to every component that may need to interrupt
    /// polling (e.g. the executor when a future becomes runnable, or
    /// a timer thread when a deadline expires).
    ///
    /// Calling `Waker::wake()` sends a byte on the internal UDP
    /// socket, causing the next (or current) `WSAPoll` to return.
    pub fn waker(&self) -> Arc<Waker> {
        self.waker.clone()
    }

    /// Register a socket with the poller.
    ///
    /// Inserts `(token, interest)` into the internal `HashMap`.  The
    /// entry will be translated into a `WSAPOLLFD` on the next
    /// [`poll()`](Self::poll) call.
    ///
    /// Unlike the Linux and macOS backends, registration does **not**
    /// issue a syscall — `WSAPoll` is stateless and recomputes the
    /// poll set from scratch on every invocation.
    pub fn register(&mut self, fd: RawFd, token: usize, interest: Interest) {
        self.reg.insert(fd, (token, interest));
    }

    /// Update interest flags for an already registered socket.
    ///
    /// Overwrites the existing entry in the internal `HashMap`.  The
    /// same `fd` key is reused; both `token` and `interest` may
    /// change.  The update takes effect on the next
    /// [`poll()`](Self::poll) call.
    pub fn reregister(&mut self, fd: RawFd, token: usize, interest: Interest) {
        self.reg.remove(&fd);

        if interest.read || interest.write {
            self.reg.insert(fd, (token, interest));
        }
    }

    /// Remove a socket from the poller.
    ///
    /// Removes the entry from the internal `HashMap`.  After this
    /// call, the poller will no longer include `fd` in the poll set.
    pub fn deregister(&mut self, fd: RawFd) {
        self.reg.remove(&fd);
    }

    /// Poll for I/O readiness events.
    ///
    /// Blocks the calling thread until at least one of the following
    /// occurs:
    ///
    /// * A registered socket becomes readable or writable.
    /// * The internal `Waker` is triggered by another thread.
    /// * The optional `timeout` duration elapses (`None` means wait
    ///   indefinitely).
    ///
    /// On each call, the method:
    ///
    /// 1. Builds a fresh `Vec<WSAPOLLFD>` from the registry, with the
    ///    wake-up receive socket at index 0.
    /// 2. Calls `WSAPoll` with the computed timeout.
    /// 3. If the wake socket has `POLLIN`, drains all pending bytes
    ///    (without emitting an `Event`) but continues processing —
    ///    other sockets may also be ready.
    /// 4. Maps every remaining ready socket to an `Event` using the
    ///    token stored in the registry.
    ///
    /// # Errors
    ///
    /// Returns [`Err`] if `WSAPoll` returns `SOCKET_ERROR`.
    ///
    /// # Timeout precision
    ///
    /// The timeout is converted to milliseconds via
    /// [`Duration::as_millis()`] and clamped to `i32::MAX`.  Sub-
    /// millisecond precision is therefore not available.
    pub fn poll(&mut self, events: &mut Vec<Event>, timeout: Option<Duration>) -> io::Result<()> {
        events.clear();

        let mut fds: Vec<WSAPOLLFD> = Vec::with_capacity(self.reg.len() + 1);

        // Wake-up socket
        fds.push(WSAPOLLFD {
            fd: self.wake_recv,
            events: POLLIN,
            revents: 0,
        });

        // Registered sockets
        for (&fd, &(_, interest)) in self.reg.iter() {
            let mut ev = 0;
            if interest.read {
                ev |= POLLIN;
            }
            if interest.write {
                ev |= POLLOUT;
            }

            fds.push(WSAPOLLFD {
                fd: fd as SOCKET,
                events: ev,
                revents: 0,
            });
        }

        let timeout_ms = timeout
            .map(|t| t.as_millis().min(i32::MAX as u128) as i32)
            .unwrap_or(-1);

        let rc = unsafe { WSAPoll(fds.as_mut_ptr(), fds.len() as u32, timeout_ms) };
        if rc == SOCKET_ERROR {
            return Err(io::Error::last_os_error());
        }

        // Drain wake socket if signaled (but don't return early - timers may have expired)
        let wake_mask = (POLLIN | POLLERR | POLLHUP | POLLNVAL) as i32;
        if (fds[0].revents as i32 & wake_mask) != 0 {
            unsafe {
                let mut buf = [0u8; 64];
                while recv(
                    self.wake_recv,
                    buf.as_mut_ptr() as *mut _,
                    buf.len() as i32,
                    0,
                ) > 0
                {}
            }
            // Don't return here - continue to process any ready sockets
            // and let the reactor check for expired timers
        }

        // Translate readiness into reactor events
        for pfd in fds.iter().skip(1) {
            let re = pfd.revents as i32;
            if re == 0 {
                continue;
            }

            let fd = pfd.fd as RawFd;
            if let Some(&(token, _)) = self.reg.get(&fd) {
                events.push(Event {
                    token,
                    readable: (re & (POLLIN | POLLERR | POLLHUP) as i32) != 0,
                    writable: (re & (POLLOUT | POLLERR | POLLHUP) as i32) != 0,
                });
            }
        }

        Ok(())
    }
}

impl Drop for Poller {
    fn drop(&mut self) {
        unsafe {
            let _ = closesocket(self.wake_recv);
            let _ = closesocket(self.wake_send);
        }
    }
}

impl Default for Poller {
    fn default() -> Self {
        Self::new()
    }
}
