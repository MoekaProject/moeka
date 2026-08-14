//! Host desktop integration.
//!
//! Every capability lives in its own module with one `cfg` branch per
//! platform, so adding a target means filling in one arm per file rather than
//! hunting for platform assumptions across the codebase.

pub(crate) mod clipboard;
pub(crate) mod paths;
pub(crate) mod shell;
pub(crate) mod trash;
