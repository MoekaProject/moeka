use gpui::Hsla;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    OneDark,
    Milky,
}

impl ThemeKind {
    pub fn toggle(self) -> Self {
        match self {
            Self::OneDark => Self::Milky,
            Self::Milky => Self::OneDark,
        }
    }

    pub fn build(self) -> Theme {
        match self {
            Self::OneDark => Theme::one_dark(),
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

/// One Dark Pro inspired color palette, organized by category.
pub struct Theme {
    // ── Text colors ─────────────────────────────────────────────
    pub foreground: Hsla,      // primary text
    pub panel_muted: Hsla,     // secondary/muted text
    pub tab_active_text: Hsla, // active/bright text
    pub accent: Hsla,          // accent text/links
    pub danger: Hsla,          // danger/error text

    // ── Background colors ───────────────────────────────────────
    pub background: Hsla,      // primary editor bg
    pub panel_bg: Hsla,        // secondary panel bg
    pub panel_surface: Hsla,   // elevated surface
    pub panel_header_bg: Hsla, // panel header
    pub panel_hover: Hsla,     // hover state
    pub panel_selected: Hsla,  // selected state
    pub panel_active: Hsla,    // active state
    pub tab_bar_bg: Hsla,      // tab bar bg
    pub tab_bg: Hsla,          // inactive tab bg
    pub tab_hover: Hsla,       // tab hover bg
    pub tab_active_bg: Hsla,   // active tab bg
    pub code_block_bg: Hsla,   // code block bg
    pub inline_code_bg: Hsla,  // inline code bg

    // ── Border colors ───────────────────────────────────────────
    pub panel_border: Hsla,      // primary border
    pub tab_border: Hsla,        // tab border
    pub checkbox_border: Hsla,   // checkbox border
    pub blockquote_border: Hsla, // decorative border

    // ── Editor/Markdown colors ──────────────────────────────────
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
            foreground: crate::hex_hsla!("#abb2bf"),
            panel_muted: crate::hex_hsla!("#7f8793"),
            tab_active_text: crate::hex_hsla!("#ffffff"),
            accent: crate::hex_hsla!("#61afef"),
            danger: crate::hex_hsla!("#e06c75"),
            // Background
            background: crate::hex_hsla!("#282c34"),
            panel_bg: crate::hex_hsla!("#21252b"),
            panel_surface: crate::hex_hsla!("#252a31"),
            panel_header_bg: crate::hex_hsla!("#1d2127"),
            panel_hover: crate::hex_hsla!("#313842"),
            panel_selected: crate::hex_hsla!("#2f3742"),
            panel_active: with_alpha(crate::hex_hsla!("#528bff"), 0.95),
            tab_bar_bg: crate::hex_hsla!("#22262d"),
            tab_bg: crate::hex_hsla!("#22262d"),
            tab_hover: crate::hex_hsla!("#282c34"),
            tab_active_bg: crate::hex_hsla!("#282c34"),
            code_block_bg: crate::hex_hsla!("#21252b"),
            inline_code_bg: crate::hex_hsla!("#2c313a"),
            // Border
            panel_border: crate::hex_hsla!("#67718633"),
            tab_border: crate::hex_hsla!("#67718633"),
            checkbox_border: crate::hex_hsla!("#3e4452"),
            blockquote_border: with_alpha(crate::hex_hsla!("#636d83"), 0.5),
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
            foreground: crate::hex_hsla!("#37352f"),
            panel_muted: crate::hex_hsla!("#9b9a97"),
            tab_active_text: crate::hex_hsla!("#1a1a1a"),
            accent: crate::hex_hsla!("#2383e2"),
            danger: crate::hex_hsla!("#eb5757"),
            // Background
            background: crate::hex_hsla!("#faf8f5"),
            panel_bg: crate::hex_hsla!("#f3f0eb"),
            panel_surface: crate::hex_hsla!("#f5f2ed"),
            panel_header_bg: crate::hex_hsla!("#edeae5"),
            panel_hover: crate::hex_hsla!("#e8e5e0"),
            panel_selected: crate::hex_hsla!("#e3e0db"),
            panel_active: with_alpha(crate::hex_hsla!("#2383e2"), 0.9),
            tab_bar_bg: crate::hex_hsla!("#f3f0eb"),
            tab_bg: crate::hex_hsla!("#f3f0eb"),
            tab_hover: crate::hex_hsla!("#faf8f5"),
            tab_active_bg: crate::hex_hsla!("#faf8f5"),
            code_block_bg: crate::hex_hsla!("#efecea"),
            inline_code_bg: crate::hex_hsla!("#f0ece8"),
            // Border
            panel_border: crate::hex_hsla!("#d6d3ce55"),
            tab_border: crate::hex_hsla!("#d6d3ce55"),
            checkbox_border: crate::hex_hsla!("#c8c5c0"),
            blockquote_border: with_alpha(crate::hex_hsla!("#9b9a97"), 0.4),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#37352f"),
            selection: with_alpha(crate::hex_hsla!("#2383e2"), 0.15),
            line_highlight: with_alpha(crate::hex_hsla!("#d6d3ce"), 0.3),
            heading: crate::hex_hsla!("#c74440"),
            bold: crate::hex_hsla!("#9d6b3a"),
            italic: crate::hex_hsla!("#8e44ad"),
            strikethrough: crate::hex_hsla!("#9b9a97"),
            inline_code: crate::hex_hsla!("#518a3c"),
            link: crate::hex_hsla!("#2383e2"),
            syntax_marker: with_alpha(crate::hex_hsla!("#9b9a97"), 0.6),
            list_marker: crate::hex_hsla!("#c74440"),
            checkbox_checked: crate::hex_hsla!("#2383e2"),
            gutter_heading: with_alpha(crate::hex_hsla!("#37352f"), 0.12),
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
