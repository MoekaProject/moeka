use crate::style::TextInputStyles;
use gpui::{Div, Stateful, div, prelude::*, px};

/// Outer presentation only. TextInput keeps all selection, IME, history and
/// multiline geometry in its existing engine.
pub(crate) fn input_shell(
    styles: &TextInputStyles,
    focused: bool,
    total_height: f32,
) -> Stateful<Div> {
    let background = if focused {
        styles.text_input_focus_bg
    } else {
        styles.text_input_bg
    };
    let hover = if focused {
        Some(styles.text_input_focus_bg)
    } else {
        styles.text_input_hover_bg
    };
    div()
        .id("text-input")
        .cursor(gpui::CursorStyle::IBeam)
        .w_full()
        .h(px(total_height))
        .pl(px(styles.text_input_padding_left_px))
        .pr(px(styles.text_input_padding_right_px))
        .bg(background)
        .when_some(hover, |this, color| this.hover(move |s| s.bg(color)))
        .rounded(px(styles.text_input_radius_px))
        .when(focused, |this| {
            this.shadow(styles.text_input_focus_ring.shadows(1.0, false))
        })
        .flex()
        .items_center()
        .overflow_hidden()
        .text_size(px(styles.text_input_font_size_px))
        .line_height(px(styles.text_input_height_px))
}
