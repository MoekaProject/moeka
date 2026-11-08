//! Host desktop integration.
//!
//! Every capability lives in its own module with one `cfg` branch per
//! platform, so adding a target means filling in one arm per file rather than
//! hunting for platform assumptions across the codebase.

pub(crate) mod capabilities;
pub(crate) mod clipboard;
pub(crate) mod dialog;
pub(crate) mod export;
pub(crate) mod fonts;
pub(crate) mod fs_task;
pub(crate) mod image_decode;
pub(crate) mod keys;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
pub(crate) mod motion;
pub(crate) mod paths;
#[cfg(windows)]
pub(crate) mod process;
pub(crate) mod security_scope;
pub(crate) mod shell;
pub(crate) mod terminal;
pub(crate) mod trash;
pub(crate) mod window_chrome;
pub(crate) mod window_state;
#[cfg(any(windows, test))]
pub(crate) mod windows_single_instance;
