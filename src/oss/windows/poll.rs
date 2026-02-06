//! Windows `WSAPoll`-based poller implementation.
//!
//! This module provides a readiness-based Windows backend for Cadentis’ reactor.
//! It mirrors the semantics of Linux `epoll` and macOS `kqueue` using
//! non-blocking sockets and `WSAPoll`.
//!
//! Responsibilities:
//! - Register sockets with read/write interests
//! - Block waiting for I/O readiness events
//! - Wake the reactor when new commands are submitted
//! - Support timer-driven wakeups via poll timeouts
//!
//! Unlike the IOCP backend, this poller is **readiness-based** and does not
//! rely on overlapped or completion-based I/O.
//!
//! This backend is primarily intended for semantic parity and simplicity.

use crate::os_common::poll::{Event, Interest, Waker};
use crate::platform::default::RawFd;
use crate::platforms::windows::ensure_winsock;

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
/// This poller owns:
/// - a registry of monitored sockets,
/// - an internal UDP socket pair used for wake-ups,
/// - a reusable buffer of `WSAPOLLFD` structures.
///
/// The wake-up mechanism allows other threads to interrupt a
/// blocking `WSAPoll` call.
pub struct WSAPollPoller {
    /// Registered sockets: `fd → (token, interest)`.
    reg: HashMap<RawFd, (usize, Interest)>,

    /// Wake-up socket (receive side).
    wake_recv: SOCKET,

    /// Wake-up socket (send side).
    wake_send: SOCKET,

    /// Waker used by the reactor to interrupt polling.
    waker: Arc<Waker>,
}

unsafe impl Send for WSAPollPoller {}
unsafe impl Sync for WSAPollPoller {}

impl Waker {
    /// Wake the poller.
    ///
    /// This sends a single byte on the internal UDP socket,
    /// causing `WSAPoll` to return immediately.
    pub fn wake(&self) {
        unsafe {
            let buf = [1u8; 1];
            let _ = send(self.0 as SOCKET, buf.as_ptr(), 1, 0);
        }
    }
}

impl WSAPollPoller {
    /// Create a new `WSAPollPoller`.
    ///
    /// This:
    /// - initializes Winsock (once per process),
    /// - creates a UDP socket pair used for wake-ups,
    /// - configures both sockets as non-blocking.
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

    /// Return the poller waker.
    ///
    /// The reactor uses this to interrupt `poll()` when new commands arrive.
    pub fn waker(&self) -> Arc<Waker> {
        self.waker.clone()
    }

    /// Register a socket with the poller.
    pub fn register(&mut self, fd: RawFd, token: usize, interest: Interest) {
        self.reg.insert(fd, (token, interest));
    }

    /// Update interest flags for a registered socket.
    pub fn reregister(&mut self, fd: RawFd, token: usize, interest: Interest) {
        self.reg.insert(fd, (token, interest));
    }

    /// Remove a socket from the poller.
    pub fn deregister(&mut self, fd: RawFd) {
        self.reg.remove(&fd);
    }

    /// Poll for I/O readiness events.
    ///
    /// Blocks until:
    /// - at least one socket becomes ready,
    /// - the wake-up socket is triggered,
    /// - or the optional timeout expires.
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

impl Drop for WSAPollPoller {
    fn drop(&mut self) {
        unsafe {
            let _ = closesocket(self.wake_recv);
            let _ = closesocket(self.wake_send);
        }
    }
}
