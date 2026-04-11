use gpui::Hsla;

/// One Dark Pro inspired color palette
pub struct Theme {
    pub background: Hsla,
    pub foreground: Hsla,
    pub panel_bg: Hsla,
    pub panel_surface: Hsla,
    pub panel_header_bg: Hsla,
    pub panel_hover: Hsla,
    pub panel_selected: Hsla,
    pub panel_active: Hsla,
    pub panel_border: Hsla,
    pub panel_muted: Hsla,
    pub tab_bar_bg: Hsla,
    pub tab_bg: Hsla,
    pub tab_hover: Hsla,
    pub tab_active_bg: Hsla,
    pub tab_active_text: Hsla,
    pub tab_border: Hsla,
    pub accent: Hsla,
    pub danger: Hsla,
    pub line_highlight: Hsla,
    pub cursor: Hsla,
    pub selection: Hsla,
    pub heading: Hsla,
    pub bold: Hsla,
    pub italic: Hsla,
    pub strikethrough: Hsla,
    pub inline_code: Hsla,
    pub inline_code_bg: Hsla,
    pub code_block_bg: Hsla,
    pub link: Hsla,
    pub syntax_marker: Hsla,
    pub blockquote_border: Hsla,
    pub list_marker: Hsla,
    pub checkbox_border: Hsla,
    pub checkbox_checked: Hsla,
    pub gutter_heading: Hsla,
}

impl Theme {
    pub fn one_dark() -> Self {
        Self {
            background: hsla_hex(0x282c34, 1.0),
            foreground: hsla_hex(0xabb2bf, 1.0),
            panel_bg: hsla_hex(0x21252b, 1.0),
            panel_surface: hsla_hex(0x252a31, 1.0),
            panel_header_bg: hsla_hex(0x1d2127, 1.0),
            panel_hover: hsla_hex(0x313842, 1.0),
            panel_selected: hsla_hex(0x2f3742, 1.0),
            panel_active: hsla_hex(0x528bff, 0.95),
            panel_border: hsla_hex(0x181b20, 1.0),
            panel_muted: hsla_hex(0x7f8793, 1.0),
            tab_bar_bg: hsla_hex(0x23272e, 1.0),
            tab_bg: hsla_hex(0x2b313a, 1.0),
            tab_hover: hsla_hex(0x353c47, 1.0),
            tab_active_bg: hsla_hex(0x1f2329, 1.0),
            tab_active_text: hsla_hex(0xd7dae0, 1.0),
            tab_border: hsla_hex(0x191d23, 1.0),
            accent: hsla_hex(0x61afef, 1.0),
            danger: hsla_hex(0xe06c75, 1.0),
            line_highlight: hsla_hex(0x2c313a, 1.0),
            cursor: hsla_hex(0x528bff, 1.0),
            selection: hsla_hex(0x3e4451, 0.6),
            heading: hsla_hex(0xe06c75, 1.0),
            bold: hsla_hex(0xd19a66, 1.0),
            italic: hsla_hex(0xc678dd, 1.0),
            strikethrough: hsla_hex(0x636d83, 1.0),
            inline_code: hsla_hex(0x98c379, 1.0),
            inline_code_bg: hsla_hex(0x2c313a, 1.0),
            code_block_bg: hsla_hex(0x21252b, 1.0),
            link: hsla_hex(0x61afef, 1.0),
            syntax_marker: hsla_hex(0x636d83, 0.7),
            blockquote_border: hsla_hex(0x636d83, 0.5),
            list_marker: hsla_hex(0xe06c75, 1.0),
            checkbox_border: hsla_hex(0x3e4452, 1.0),
            checkbox_checked: hsla_hex(0x3778ae, 1.0),
            gutter_heading: hsla_hex(0xabb2bf, 0.18),
        }
    }
}

fn hsla_hex(hex: u32, alpha: f32) -> Hsla {
    let r = ((hex >> 16) & 0xFF) as f32 / 255.0;
    let g = ((hex >> 8) & 0xFF) as f32 / 255.0;
    let b = (hex & 0xFF) as f32 / 255.0;
    rgb_to_hsla(r, g, b, alpha)
}

fn rgb_to_hsla(r: f32, g: f32, b: f32, a: f32) -> Hsla {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;

    if (max - min).abs() < f32::EPSILON {
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

    let h = if (max - r).abs() < f32::EPSILON {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < f32::EPSILON {
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
