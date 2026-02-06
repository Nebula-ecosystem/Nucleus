pub(crate) mod common;

#[cfg(unix)]
pub mod unix;

#[cfg(windows)]
pub mod windows;
