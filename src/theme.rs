use gpui::Hsla;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    OneDark,
    OneLight,
    AyuDark,
    AyuLight,
    Milky,
}

impl ThemeKind {
    pub fn all() -> &'static [ThemeKind] {
        &[
            Self::OneDark,
            Self::OneLight,
            Self::AyuDark,
            Self::AyuLight,
            Self::Milky,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OneDark => "One Dark",
            Self::OneLight => "One Light",
            Self::AyuDark => "Ayu Dark",
            Self::AyuLight => "Ayu Light",
            Self::Milky => "Milky",
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, Self::OneDark | Self::AyuDark)
    }

    pub fn build(self) -> Theme {
        match self {
            Self::OneDark => Theme::one_dark(),
            Self::OneLight => Theme::one_light(),
            Self::AyuDark => Theme::ayu_dark(),
            Self::AyuLight => Theme::ayu_light(),
            Self::Milky => Theme::milky(),
        }
    }
}

#[macro_export]
macro_rules! hex_hsla {
    ($hex:literal) => {{
        const VALUE: ::gpui::Hsla = $crate::theme::hex_to_hsla($hex);
        VALUE
    }};
}

/// Color palette organized by visual category: text, background, border, editor.
///
/// Naming convention: `{category}_{depth}` — no functional descriptions.
///   category = text | bg | border  (editor/markdown semantics keep their own names)
///   depth    = primary, secondary, tertiary, bright, accent, danger,
///              elevated, hover, selected, bar, tab,
///              tab_hover, tab_active, code, code_inline, decorative …
pub struct Theme {
    // ── Text colors ─────────────────────────────────────────────
    pub text_primary: Hsla,
    pub text_secondary: Hsla,
    pub text_bright: Hsla,
    pub text_accent: Hsla,
    pub text_danger: Hsla,

    // ── Background colors ───────────────────────────────────────
    pub bg_primary: Hsla,
    pub bg_secondary: Hsla,
    pub bg_elevated: Hsla,
    pub bg_hover: Hsla,
    pub bg_selected: Hsla,
    pub bg_bar: Hsla,
    pub bg_tab: Hsla,
    pub bg_tab_hover: Hsla,
    pub bg_tab_active: Hsla,
    pub bg_code: Hsla,
    pub bg_code_inline: Hsla,

    // ── Border colors ───────────────────────────────────────────
    pub border_primary: Hsla,
    pub border_secondary: Hsla,
    pub border_tertiary: Hsla,
    pub border_decorative: Hsla,

    // ── Editor / Markdown semantic colors ────────────────────────
    pub cursor: Hsla,
    pub selection: Hsla,
    pub line_highlight: Hsla,
    pub heading: Hsla,
    pub bold: Hsla,
    pub italic: Hsla,
    pub strikethrough: Hsla,
    pub inline_code: Hsla,
    pub link: Hsla,
    pub syntax_marker: Hsla,
    pub list_marker: Hsla,
    pub checkbox_checked: Hsla,
    pub gutter_heading: Hsla,
}

impl Theme {
    pub fn one_dark() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#abb2bf"),
            text_secondary: crate::hex_hsla!("#7f8793"),
            text_bright: crate::hex_hsla!("#ffffff"),
            text_accent: crate::hex_hsla!("#61afef"),
            text_danger: crate::hex_hsla!("#e06c75"),
            // Background
            bg_primary: crate::hex_hsla!("#282c34"),
            bg_secondary: crate::hex_hsla!("#21252b"),
            bg_elevated: crate::hex_hsla!("#252a31"),
            bg_hover: crate::hex_hsla!("#313842"),
            bg_selected: crate::hex_hsla!("#2f3742"),
            bg_bar: crate::hex_hsla!("#22262d"),
            bg_tab: crate::hex_hsla!("#22262d"),
            bg_tab_hover: crate::hex_hsla!("#282c34"),
            bg_tab_active: crate::hex_hsla!("#282c34"),
            bg_code: crate::hex_hsla!("#21252b"),
            bg_code_inline: crate::hex_hsla!("#2c313a"),
            // Border
            border_primary: crate::hex_hsla!("#67718633"),
            border_secondary: crate::hex_hsla!("#67718633"),
            border_tertiary: crate::hex_hsla!("#3e4452"),
            border_decorative: with_alpha(crate::hex_hsla!("#636d83"), 0.5),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#528bff"),
            selection: with_alpha(crate::hex_hsla!("#3e4451"), 0.6),
            line_highlight: with_alpha(crate::hex_hsla!("#3e4451"), 0.5),
            heading: crate::hex_hsla!("#e06c75"),
            bold: crate::hex_hsla!("#d19a66"),
            italic: crate::hex_hsla!("#c678dd"),
            strikethrough: crate::hex_hsla!("#636d83"),
            inline_code: crate::hex_hsla!("#98c379"),
            link: crate::hex_hsla!("#61afef"),
            syntax_marker: with_alpha(crate::hex_hsla!("#636d83"), 0.7),
            list_marker: crate::hex_hsla!("#e06c75"),
            checkbox_checked: crate::hex_hsla!("#3778ae"),
            gutter_heading: with_alpha(crate::hex_hsla!("#abb2bf"), 0.18),
        }
    }

    pub fn milky() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#37352f"),
            text_secondary: crate::hex_hsla!("#9b9a97"),
            text_bright: crate::hex_hsla!("#1a1a1a"),
            text_accent: crate::hex_hsla!("#2383e2"),
            text_danger: crate::hex_hsla!("#eb5757"),
            // Background
            bg_primary: crate::hex_hsla!("#faf8f5"),
            bg_secondary: crate::hex_hsla!("#f7f2ea"),
            bg_elevated: crate::hex_hsla!("#f5f2ed"),
            bg_hover: crate::hex_hsla!("#e8e5e0"),
            bg_selected: crate::hex_hsla!("#e3e0db"),
            bg_bar: crate::hex_hsla!("#f3f0eb"),
            bg_tab: crate::hex_hsla!("#f3f0eb"),
            bg_tab_hover: crate::hex_hsla!("#faf8f5"),
            bg_tab_active: crate::hex_hsla!("#faf8f5"),
            bg_code: crate::hex_hsla!("#efecea"),
            bg_code_inline: crate::hex_hsla!("#f0ece8"),
            // Border
            border_primary: crate::hex_hsla!("#d6d3ce9e"),
            border_secondary: crate::hex_hsla!("#d6d3ce55"),
            border_tertiary: crate::hex_hsla!("#c8c5c0"),
            border_decorative: with_alpha(crate::hex_hsla!("#9b9a97"), 0.4),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#528bff"),
            selection: with_alpha(crate::hex_hsla!("#2383e2"), 0.15),
            line_highlight: with_alpha(crate::hex_hsla!("#d6d3ce"), 0.3),
            heading: crate::hex_hsla!("#c74440"),
            bold: crate::hex_hsla!("#9d6b3a"),
            italic: crate::hex_hsla!("#8e44ad"),
            strikethrough: crate::hex_hsla!("#9b9a97"),
            inline_code: crate::hex_hsla!("#518a3c"),
            link: crate::hex_hsla!("#2383e2"),
            syntax_marker: with_alpha(crate::hex_hsla!("#808080"), 1.0),
            list_marker: crate::hex_hsla!("#c74440"),
            checkbox_checked: crate::hex_hsla!("#2383e2"),
            gutter_heading: with_alpha(crate::hex_hsla!("#37352f"), 0.12),
        }
    }

    pub fn one_light() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#383a42"),
            text_secondary: crate::hex_hsla!("#a0a1a7"),
            text_bright: crate::hex_hsla!("#242529"),
            text_accent: crate::hex_hsla!("#5c78e2"),
            text_danger: crate::hex_hsla!("#d3604f"),
            // Background
            bg_primary: crate::hex_hsla!("#fafafa"),
            bg_secondary: crate::hex_hsla!("#f0f0f1"),
            bg_elevated: crate::hex_hsla!("#eaeaeb"),
            bg_hover: crate::hex_hsla!("#e5e5e6"),
            bg_selected: crate::hex_hsla!("#dcdcdd"),
            bg_bar: crate::hex_hsla!("#ebebec"),
            bg_tab: crate::hex_hsla!("#ebebec"),
            bg_tab_hover: crate::hex_hsla!("#f5f5f5"),
            bg_tab_active: crate::hex_hsla!("#fafafa"),
            bg_code: crate::hex_hsla!("#f0f0f1"),
            bg_code_inline: crate::hex_hsla!("#ecedef"),
            // Border
            border_primary: crate::hex_hsla!("#c9c9ca55"),
            border_secondary: crate::hex_hsla!("#c9c9ca33"),
            border_tertiary: crate::hex_hsla!("#c9c9ca"),
            border_decorative: with_alpha(crate::hex_hsla!("#a0a1a7"), 0.4),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#5c78e2"),
            selection: with_alpha(crate::hex_hsla!("#5c78e2"), 0.15),
            line_highlight: with_alpha(crate::hex_hsla!("#c9c9ca"), 0.3),
            heading: crate::hex_hsla!("#d3604f"),
            bold: crate::hex_hsla!("#ad6e25"),
            italic: crate::hex_hsla!("#a449ab"),
            strikethrough: crate::hex_hsla!("#a0a1a7"),
            inline_code: crate::hex_hsla!("#649f57"),
            link: crate::hex_hsla!("#5b79e3"),
            syntax_marker: with_alpha(crate::hex_hsla!("#a0a1a7"), 0.7),
            list_marker: crate::hex_hsla!("#d3604f"),
            checkbox_checked: crate::hex_hsla!("#5c78e2"),
            gutter_heading: with_alpha(crate::hex_hsla!("#383a42"), 0.12),
        }
    }

    pub fn ayu_dark() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#bfbdb6"),
            text_secondary: crate::hex_hsla!("#636a76"),
            text_bright: crate::hex_hsla!("#e6e1cf"),
            text_accent: crate::hex_hsla!("#5ac1fe"),
            text_danger: crate::hex_hsla!("#f07178"),
            // Background
            bg_primary: crate::hex_hsla!("#0d1017"),
            bg_secondary: crate::hex_hsla!("#131721"),
            bg_elevated: crate::hex_hsla!("#161b25"),
            bg_hover: crate::hex_hsla!("#1f2430"),
            bg_selected: crate::hex_hsla!("#232834"),
            bg_bar: crate::hex_hsla!("#0f1319"),
            bg_tab: crate::hex_hsla!("#0f1319"),
            bg_tab_hover: crate::hex_hsla!("#131721"),
            bg_tab_active: crate::hex_hsla!("#0d1017"),
            bg_code: crate::hex_hsla!("#131721"),
            bg_code_inline: crate::hex_hsla!("#1a1f2b"),
            // Border
            border_primary: crate::hex_hsla!("#3f404333"),
            border_secondary: crate::hex_hsla!("#3f404322"),
            border_tertiary: crate::hex_hsla!("#3f4043"),
            border_decorative: with_alpha(crate::hex_hsla!("#5c6773"), 0.5),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#5ac1fe"),
            selection: with_alpha(crate::hex_hsla!("#5ac1fe"), 0.15),
            line_highlight: with_alpha(crate::hex_hsla!("#1f2430"), 0.7),
            heading: crate::hex_hsla!("#bfbdb6"),
            bold: crate::hex_hsla!("#5ac1fe"),
            italic: crate::hex_hsla!("#fe8f40"),
            strikethrough: crate::hex_hsla!("#5c6773"),
            inline_code: crate::hex_hsla!("#a9d94b"),
            link: crate::hex_hsla!("#fe8f40"),
            syntax_marker: with_alpha(crate::hex_hsla!("#5c6773"), 0.7),
            list_marker: crate::hex_hsla!("#ff8f3f"),
            checkbox_checked: crate::hex_hsla!("#5ac1fe"),
            gutter_heading: with_alpha(crate::hex_hsla!("#bfbdb6"), 0.15),
        }
    }

    pub fn ayu_light() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#5c6166"),
            text_secondary: crate::hex_hsla!("#8b8e92"),
            text_bright: crate::hex_hsla!("#3a3d40"),
            text_accent: crate::hex_hsla!("#3b9ee5"),
            text_danger: crate::hex_hsla!("#f07178"),
            // Background
            bg_primary: crate::hex_hsla!("#fcfcfc"),
            bg_secondary: crate::hex_hsla!("#f0f0f0"),
            bg_elevated: crate::hex_hsla!("#e8e8e8"),
            bg_hover: crate::hex_hsla!("#e0e0e0"),
            bg_selected: crate::hex_hsla!("#d8d8d8"),
            bg_bar: crate::hex_hsla!("#ececed"),
            bg_tab: crate::hex_hsla!("#ececed"),
            bg_tab_hover: crate::hex_hsla!("#f5f5f5"),
            bg_tab_active: crate::hex_hsla!("#fcfcfc"),
            bg_code: crate::hex_hsla!("#f0f0f0"),
            bg_code_inline: crate::hex_hsla!("#ebeced"),
            // Border
            border_primary: crate::hex_hsla!("#cfd1d255"),
            border_secondary: crate::hex_hsla!("#cfd1d233"),
            border_tertiary: crate::hex_hsla!("#cfd1d2"),
            border_decorative: with_alpha(crate::hex_hsla!("#8b8e92"), 0.4),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#3b9ee5"),
            selection: with_alpha(crate::hex_hsla!("#3b9ee5"), 0.15),
            line_highlight: with_alpha(crate::hex_hsla!("#cfd1d2"), 0.3),
            heading: crate::hex_hsla!("#5c6166"),
            bold: crate::hex_hsla!("#3b9ee5"),
            italic: crate::hex_hsla!("#f98d3f"),
            strikethrough: crate::hex_hsla!("#8b8e92"),
            inline_code: crate::hex_hsla!("#86b300"),
            link: crate::hex_hsla!("#f98d3f"),
            syntax_marker: with_alpha(crate::hex_hsla!("#abb0b6"), 0.7),
            list_marker: crate::hex_hsla!("#fa8d3e"),
            checkbox_checked: crate::hex_hsla!("#3b9ee5"),
            gutter_heading: with_alpha(crate::hex_hsla!("#5c6166"), 0.12),
        }
    }
}

/// Convert a CSS-style hex color string (e.g. "#ffffff", "#FF0000", "#abc") to Hsla.
/// Supports 3-digit (#RGB), 6-digit (#RRGGBB), and 8-digit (#RRGGBBAA) forms.
/// The leading '#' is optional.
pub const fn hex_to_hsla(hex_str: &str) -> Hsla {
    let bytes = hex_str.as_bytes();
    let start = if !bytes.is_empty() && bytes[0] == b'#' {
        1
    } else {
        0
    };
    let len = bytes.len() - start;

    let (r, g, b, a) = match len {
        3 => {
            let r = parse_hex_nibble(bytes[start]);
            let g = parse_hex_nibble(bytes[start + 1]);
            let b = parse_hex_nibble(bytes[start + 2]);
            (r * 17, g * 17, b * 17, 255)
        }
        6 => {
            let r = parse_hex_byte(bytes[start], bytes[start + 1]);
            let g = parse_hex_byte(bytes[start + 2], bytes[start + 3]);
            let b = parse_hex_byte(bytes[start + 4], bytes[start + 5]);
            (r, g, b, 255)
        }
        8 => {
            let r = parse_hex_byte(bytes[start], bytes[start + 1]);
            let g = parse_hex_byte(bytes[start + 2], bytes[start + 3]);
            let b = parse_hex_byte(bytes[start + 4], bytes[start + 5]);
            let a = parse_hex_byte(bytes[start + 6], bytes[start + 7]);
            (r, g, b, a)
        }
        _ => panic!("invalid hex color literal"),
    };

    rgb_to_hsla(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    )
}

const fn parse_hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => panic!("invalid hex digit"),
    }
}

const fn parse_hex_byte(high: u8, low: u8) -> u8 {
    (parse_hex_nibble(high) << 4) | parse_hex_nibble(low)
}

const fn max3(a: f32, b: f32, c: f32) -> f32 {
    let ab = if a > b { a } else { b };
    if ab > c { ab } else { c }
}

const fn min3(a: f32, b: f32, c: f32) -> f32 {
    let ab = if a < b { a } else { b };
    if ab < c { ab } else { c }
}

pub const fn with_alpha(color: Hsla, alpha: f32) -> Hsla {
    Hsla { a: alpha, ..color }
}

const fn rgb_to_hsla(r: f32, g: f32, b: f32, a: f32) -> Hsla {
    let max = max3(r, g, b);
    let min = min3(r, g, b);
    let l = (max + min) / 2.0;

    if max == min {
        return Hsla {
            h: 0.0,
            s: 0.0,
            l,
            a,
        };
    }

    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };

    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };

    Hsla {
        h: h / 6.0,
        s,
        l,
        a,
    }
}
