//! Key bindings whose natural form differs between platforms.
//!
//! gpui resolves the `secondary-` prefix itself (Command on macOS, Control
//! elsewhere), which covers every binding that only swaps the primary
//! modifier. The constants here are for shortcuts whose *shape* differs:
//! macOS expresses line and document motion with Command, while Windows and
//! Linux use dedicated Home/End keys.

macro_rules! platform_key {
    ($name:ident, $macos:expr, $other:expr) => {
        #[cfg(target_os = "macos")]
        pub(crate) const $name: &str = $macos;
        #[cfg(not(target_os = "macos"))]
        pub(crate) const $name: &str = $other;
    };
}

platform_key!(LINE_HOME, "cmd-left", "home");
platform_key!(LINE_END, "cmd-right", "end");
platform_key!(SELECT_LINE_HOME, "shift-cmd-left", "shift-home");
platform_key!(SELECT_LINE_END, "shift-cmd-right", "shift-end");

platform_key!(DOCUMENT_START, "cmd-up", "ctrl-home");
platform_key!(DOCUMENT_END, "cmd-down", "ctrl-end");
platform_key!(SELECT_TO_DOCUMENT_START, "shift-cmd-up", "ctrl-shift-home");
platform_key!(SELECT_TO_DOCUMENT_END, "shift-cmd-down", "ctrl-shift-end");

platform_key!(WORD_LEFT, "alt-left", "ctrl-left");
platform_key!(WORD_RIGHT, "alt-right", "ctrl-right");
platform_key!(SELECT_WORD_LEFT, "shift-alt-left", "ctrl-shift-left");
platform_key!(SELECT_WORD_RIGHT, "shift-alt-right", "ctrl-shift-right");
platform_key!(DELETE_WORD_LEFT, "alt-backspace", "ctrl-backspace");
platform_key!(DELETE_WORD_RIGHT, "alt-delete", "ctrl-delete");

platform_key!(
    DELETE_TO_LINE_START,
    "cmd-backspace",
    "ctrl-shift-backspace"
);
platform_key!(DELETE_TO_LINE_END, "cmd-delete", "ctrl-shift-delete");

// Command+Control pairs collapse onto plain Control elsewhere, which would
// collide with the Control-only form of the same shortcut.
platform_key!(TOGGLE_OUTLINE, "cmd-ctrl-1", "ctrl-alt-1");
platform_key!(TOGGLE_TERMINAL_PANEL, "cmd-ctrl-t", "ctrl-alt-t");
platform_key!(CHARACTER_PALETTE, "cmd-ctrl-space", "ctrl-alt-space");

// Control+Shift+G is Find Previous once Command maps to Control.

platform_key!(SIDEBAR_DELETE, "cmd-backspace", "delete");

/// Whether the emacs-style Control bindings are registered.
///
/// They only work where Command is the primary modifier; elsewhere
/// Control+A/F/V are select-all, find and paste.
pub(crate) const EMACS_BINDINGS: bool = cfg!(target_os = "macos");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_motion_follows_the_platform_convention() {
        if cfg!(target_os = "macos") {
            assert_eq!(LINE_HOME, "cmd-left");
            assert_eq!(WORD_LEFT, "alt-left");
        } else {
            assert_eq!(LINE_HOME, "home");
            assert_eq!(WORD_LEFT, "ctrl-left");
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn command_control_pairs_never_collapse_onto_a_plain_control_shortcut() {
        // `secondary-t` is New Tab and `secondary-shift-g` is Find Previous,
        // so these must not resolve to the same Control combination.
        assert_ne!(TOGGLE_TERMINAL_PANEL, "ctrl-t");
        assert_ne!(TOGGLE_OUTLINE, "ctrl-1");
        assert_ne!(TOGGLE_GIT_HISTORY, "ctrl-shift-g");
    }

    #[test]
    fn emacs_bindings_are_macos_only() {
        assert_eq!(EMACS_BINDINGS, cfg!(target_os = "macos"));
    }
}
