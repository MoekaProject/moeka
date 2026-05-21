use gpui::{AnyElement, Hsla, SharedString, div, prelude::*, px};

#[derive(Clone, Copy)]
pub(crate) struct ShortcutRenderStyle {
    pub font_size_px: f32,
    pub font_family: &'static str,
    pub text_color: Hsla,
    pub gap_px: f32,
}

pub(crate) fn render_shortcut(shortcut: &str, style: ShortcutRenderStyle) -> AnyElement {
    let tokens = tokenize_shortcut(shortcut);

    div()
        .flex()
        .items_center()
        .gap(px(style.gap_px))
        .font_family(SharedString::from(style.font_family))
        .children(tokens.into_iter().map(|token| {
            let width = estimate_token_width(&token, style.font_size_px);
            div()
                .w(px(width))
                .h(px(style.font_size_px))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .text_size(px(style.font_size_px))
                .line_height(px(style.font_size_px))
                .text_color(style.text_color)
                .child(token)
        }))
        .into_any_element()
}

pub(crate) fn estimate_shortcut_width(shortcut: &str, style: ShortcutRenderStyle) -> f32 {
    let tokens = tokenize_shortcut(shortcut);
    if tokens.is_empty() {
        return 0.0;
    }

    tokens
        .iter()
        .enumerate()
        .map(|(index, token)| {
            estimate_token_width(token, style.font_size_px)
                + if index + 1 < tokens.len() {
                    style.gap_px
                } else {
                    0.0
                }
        })
        .sum()
}

pub(crate) fn estimate_text_width(text: &str, font_size_px: f32) -> f32 {
    text.chars()
        .map(|ch| match ch {
            ' ' => font_size_px * 0.36,
            _ if ch.is_ascii_uppercase() => font_size_px * 0.62,
            _ if ch.is_ascii_lowercase() || ch.is_ascii_digit() => font_size_px * 0.56,
            _ if ch.is_ascii_punctuation() => font_size_px * 0.45,
            _ => font_size_px * 0.92,
        })
        .sum()
}

fn tokenize_shortcut(shortcut: &str) -> Vec<String> {
    if shortcut.is_empty() {
        return Vec::new();
    }

    if shortcut.contains('-') && !shortcut.chars().any(is_special_key_symbol) {
        return shortcut
            .split('-')
            .filter_map(normalize_token)
            .collect::<Vec<_>>();
    }

    if !shortcut.chars().any(is_special_key_symbol) {
        return normalize_token(shortcut).into_iter().collect();
    }

    let mut tokens = Vec::new();
    let mut buffer = String::new();

    for ch in shortcut.chars() {
        if is_special_key_symbol(ch) {
            if !buffer.is_empty() {
                if let Some(token) = normalize_token(&buffer) {
                    tokens.push(token);
                }
                buffer.clear();
            }
            tokens.push(ch.to_string());
        } else {
            buffer.push(ch);
        }
    }

    if !buffer.is_empty() {
        if let Some(token) = normalize_token(&buffer) {
            tokens.push(token);
        }
    }

    tokens
}

fn estimate_token_width(token: &str, font_size_px: f32) -> f32 {
    let count = token.chars().count();
    if count == 1 {
        font_size_px.max(1.0)
    } else {
        estimate_text_width(token, font_size_px)
    }
}

fn normalize_token(token: &str) -> Option<String> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    let normalized = match token.to_ascii_lowercase().as_str() {
        "cmd" | "command" | "platform" | "super" | "win" => "⌘".to_string(),
        "alt" | "option" => "⌥".to_string(),
        "ctrl" | "control" => "⌃".to_string(),
        "shift" => "⇧".to_string(),
        "enter" | "return" => "↩".to_string(),
        "backspace" => "⌫".to_string(),
        "delete" => "⌦".to_string(),
        "tab" => "⇥".to_string(),
        "escape" | "esc" => "⎋".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        _ if token.len() == 1 && token.is_ascii() => token.to_ascii_uppercase(),
        _ => token.to_string(),
    };

    Some(normalized)
}

fn is_special_key_symbol(ch: char) -> bool {
    matches!(
        ch,
        '⌘' | '⌥' | '⌃' | '⇧' | '↩' | '⌫' | '⌦' | '⇥' | '⎋' | '↑' | '↓' | '←' | '→'
    )
}
