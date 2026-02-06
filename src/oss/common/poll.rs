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
