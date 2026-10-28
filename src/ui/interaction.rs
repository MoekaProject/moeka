use gpui::{InteractiveElement, ParentElement, Styled, div, px};

fn is_activation_key(key: &str, modified: bool) -> bool {
    !modified && matches!(key, "enter" | "space")
}

/// Preserve the owner's IDs/focus and navigation keys while making an action
/// inert. The child consumes mouse-up in bubble phase, after GPUI has cleared
/// any pending press in capture phase but before the parent's click callback.
/// Capturing mouse-up here would leave a stale press when disabled mid-gesture.
pub(crate) fn inert_activation<E: InteractiveElement + ParentElement + Styled>(element: E) -> E {
    let mut release_barrier = div().absolute().left(px(0.0)).top(px(0.0)).size_full();
    release_barrier
        .interactivity()
        .on_any_mouse_up(|_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        });
    element
        .capture_any_mouse_down(|_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
        .capture_key_down(|event, window, cx| {
            if is_activation_key(&event.keystroke.key, event.keystroke.modifiers.modified()) {
                window.prevent_default();
                cx.stop_propagation();
            }
        })
        .capture_key_up(|event, window, cx| {
            if is_activation_key(&event.keystroke.key, event.keystroke.modifiers.modified()) {
                window.prevent_default();
                cx.stop_propagation();
            }
        })
        .relative()
        .child(release_barrier)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inert_actions_keep_owner_navigation_and_modified_shortcuts() {
        for key in ["enter", "space"] {
            assert!(is_activation_key(key, false));
            assert!(!is_activation_key(key, true));
        }
        for key in ["tab", "escape", "up", "down", "left", "right", "a"] {
            assert!(!is_activation_key(key, false));
            assert!(!is_activation_key(key, true));
        }
    }
}
