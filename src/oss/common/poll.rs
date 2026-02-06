use crate::platform::default::RawFd;

/// I/O interest flags.
///
/// An `Interest` specifies which readiness events should be
/// monitored for a file descriptor.
///
/// It is used when registering or updating I/O entries with
/// the poller.
#[derive(Clone, Copy)]
pub struct Interest {
    /// Interest in read readiness.
    pub(crate) read: bool,

    /// Interest in write readiness.
    pub(crate) write: bool,
}

/// Low-level poller waker.
///
/// A `Waker` wraps a platform-specific file descriptor used to
/// interrupt the poller while it is blocked waiting for events.
///
/// This is a **reactor-internal** mechanism and should not be
/// confused with [`std::task::Waker`].
pub struct Waker(pub(crate) RawFd);

unsafe impl Send for Waker {}
unsafe impl Sync for Waker {}

/// An I/O event reported by the poller.
///
/// An `Event` represents readiness information for a registered
/// file descriptor. It is produced by the poller and consumed
/// by the reactor to wake the appropriate tasks.
///
/// The event indicates whether the file descriptor is readable,
/// writable, or both.
pub struct Event {
    /// Token associated with the registered file descriptor.
    ///
    /// This token is used to identify the I/O entry inside the reactor.
    pub(crate) token: usize,

    /// Indicates that the file descriptor is readable.
    pub(crate) readable: bool,

    /// Indicates that the file descriptor is writable.
    pub(crate) writable: bool,
}
