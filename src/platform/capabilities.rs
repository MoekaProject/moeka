//! What the host lets this build do.
//!
//! The App Store channel (`--features mas`) runs inside the sandbox, which
//! denies child processes outright. Rather than compile those subsystems out
//! — which would fork the code base along a seam that touches tabs, panels and
//! menus — each one asks here and the UI hides the affordances it cannot back.

/// Whether the app may spawn helper processes.
///
/// Gates the terminal, the agent CLIs and every `git` invocation.
pub(crate) const CHILD_PROCESSES: bool = !cfg!(feature = "mas");

/// Whether the app ships its own updater.
///
/// The App Store delivers updates itself and forbids self-updating.
pub(crate) const SELF_UPDATE: bool = !cfg!(feature = "mas");
