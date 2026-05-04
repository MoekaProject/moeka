use gpui::{AnyElement, Hsla, div, prelude::*, px};

#[derive(Clone, Copy)]
pub(crate) struct ShortcutRenderStyle {
    pub font_size_px: f32,
    pub text_color: Hsla,
    pub gap_px: f32,
    pub key_min_width_px: f32,
    pub key_height_px: f32,
    pub key_padding_x_px: f32,
    pub key_radius_px: f32,
    pub key_bg: Hsla,
}

pub(crate) fn render_shortcut(shortcut: &str, style: ShortcutRenderStyle) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(style.gap_px))
        .children(tokenize_shortcut(shortcut).into_iter().map(|token| {
            div()
                .min_w(px(style.key_min_width_px))
                .h(px(style.key_height_px))
                .px(px(style.key_padding_x_px))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(style.key_radius_px))
                .bg(style.key_bg)
                .text_size(px(style.font_size_px))
                .text_color(style.text_color)
                .child(token)
        }))
        .into_any_element()
}

fn tokenize_shortcut(shortcut: &str) -> Vec<String> {
    if shortcut.is_empty() {
        return Vec::new();
    }

    if !shortcut.chars().any(is_special_key_symbol) {
        return vec![shortcut.to_string()];
    }

    let mut tokens = Vec::new();
    let mut buffer = String::new();

    for ch in shortcut.chars() {
        if is_special_key_symbol(ch) {
            if !buffer.is_empty() {
                tokens.push(std::mem::take(&mut buffer));
            }
            tokens.push(ch.to_string());
        } else {
            buffer.push(ch);
        }
    }

    if !buffer.is_empty() {
        tokens.push(buffer);
    }

    tokens
}

fn is_special_key_symbol(ch: char) -> bool {
    matches!(
        ch,
        '⌘' | '⌥' | '⌃' | '⇧' | '↩' | '⌫' | '⌦' | '⇥' | '⎋' | '↑' | '↓' | '←' | '→'
    )
}
