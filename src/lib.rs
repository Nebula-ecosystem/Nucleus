pub(crate) mod oss;
pub(crate) mod platforms;

#[cfg(target_os = "linux")]
pub(crate) use oss::linux as os;

#[cfg(target_os = "macos")]
pub(crate) use oss::macos as os;

#[cfg(target_os = "windows")]
pub(crate) use oss::windows as os;

#[cfg(unix)]
pub(crate) use platforms::unix as platform;

#[cfg(windows)]
pub(crate) use platforms::windows as platform;

pub(crate) use oss::common as os_common;
// pub(crate) use platforms::common as platform_common;

pub use os::*;
pub use platform::*;
