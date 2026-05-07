use gpui::Hsla;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    OneDark,
    OneLight,
    AyuDark,
    AyuLight,
    Milky,
    MilkyDark,
}

impl ThemeKind {
    pub fn dark_themes() -> &'static [ThemeKind] {
        &[Self::OneDark, Self::AyuDark, Self::MilkyDark]
    }

    pub fn light_themes() -> &'static [ThemeKind] {
        &[Self::OneLight, Self::AyuLight, Self::Milky]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OneDark => "One Dark",
            Self::OneLight => "One Light",
            Self::AyuDark => "Ayu Dark",
            Self::AyuLight => "Ayu Light",
            Self::Milky => "Milky",
            Self::MilkyDark => "Milky Dark",
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, Self::OneDark | Self::AyuDark | Self::MilkyDark)
    }

    pub fn paired_appearance(self) -> ThemeKind {
        match self {
            Self::OneDark => Self::OneLight,
            Self::OneLight => Self::OneDark,
            Self::AyuDark => Self::AyuLight,
            Self::AyuLight => Self::AyuDark,
            Self::Milky => Self::MilkyDark,
            Self::MilkyDark => Self::Milky,
        }
    }

    pub fn build(self) -> Theme {
        match self {
            Self::OneDark => Theme::one_dark(),
            Self::OneLight => Theme::one_light(),
            Self::AyuDark => Theme::ayu_dark(),
            Self::AyuLight => Theme::ayu_light(),
            Self::Milky => Theme::milky(),
            Self::MilkyDark => Theme::milky_dark(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeColorField {
    TextPrimary,
    TextSecondary,
    TextBright,
    TextAccent,
    TextDanger,
    BgPrimary,
    BgSecondary,
    BgElevated,
    BgHover,
    BgSelected,
    BgBar,
    BgTab,
    BgTabHover,
    BgTabActive,
    BgCode,
    BgCodeInline,
    BorderPrimary,
    BorderSecondary,
    BorderTertiary,
    BorderDecorative,
    Cursor,
    Selection,
    LineHighlight,
    Heading,
    Bold,
    Italic,
    Strikethrough,
    InlineCode,
    Link,
    SyntaxMarker,
    ListMarker,
    CheckboxChecked,
    GutterHeading,
}

impl ThemeColorField {
    pub const ALL: [Self; 33] = [
        Self::TextPrimary,
        Self::TextSecondary,
        Self::TextBright,
        Self::TextAccent,
        Self::TextDanger,
        Self::BgPrimary,
        Self::BgSecondary,
        Self::BgElevated,
        Self::BgHover,
        Self::BgSelected,
        Self::BgBar,
        Self::BgTab,
        Self::BgTabHover,
        Self::BgTabActive,
        Self::BgCode,
        Self::BgCodeInline,
        Self::BorderPrimary,
        Self::BorderSecondary,
        Self::BorderTertiary,
        Self::BorderDecorative,
        Self::Cursor,
        Self::Selection,
        Self::LineHighlight,
        Self::Heading,
        Self::Bold,
        Self::Italic,
        Self::Strikethrough,
        Self::InlineCode,
        Self::Link,
        Self::SyntaxMarker,
        Self::ListMarker,
        Self::CheckboxChecked,
        Self::GutterHeading,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::TextPrimary => "text_primary",
            Self::TextSecondary => "text_secondary",
            Self::TextBright => "text_bright",
            Self::TextAccent => "text_accent",
            Self::TextDanger => "text_danger",
            Self::BgPrimary => "bg_primary",
            Self::BgSecondary => "bg_secondary",
            Self::BgElevated => "bg_elevated",
            Self::BgHover => "bg_hover",
            Self::BgSelected => "bg_selected",
            Self::BgBar => "bg_bar",
            Self::BgTab => "bg_tab",
            Self::BgTabHover => "bg_tab_hover",
            Self::BgTabActive => "bg_tab_active",
            Self::BgCode => "bg_code",
            Self::BgCodeInline => "bg_code_inline",
            Self::BorderPrimary => "border_primary",
            Self::BorderSecondary => "border_secondary",
            Self::BorderTertiary => "border_tertiary",
            Self::BorderDecorative => "border_decorative",
            Self::Cursor => "cursor",
            Self::Selection => "selection",
            Self::LineHighlight => "line_highlight",
            Self::Heading => "heading",
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Strikethrough => "strikethrough",
            Self::InlineCode => "inline_code",
            Self::Link => "link",
            Self::SyntaxMarker => "syntax_marker",
            Self::ListMarker => "list_marker",
            Self::CheckboxChecked => "checkbox_checked",
            Self::GutterHeading => "gutter_heading",
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
#[derive(Clone)]
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
    pub fn color(&self, field: ThemeColorField) -> Hsla {
        match field {
            ThemeColorField::TextPrimary => self.text_primary,
            ThemeColorField::TextSecondary => self.text_secondary,
            ThemeColorField::TextBright => self.text_bright,
            ThemeColorField::TextAccent => self.text_accent,
            ThemeColorField::TextDanger => self.text_danger,
            ThemeColorField::BgPrimary => self.bg_primary,
            ThemeColorField::BgSecondary => self.bg_secondary,
            ThemeColorField::BgElevated => self.bg_elevated,
            ThemeColorField::BgHover => self.bg_hover,
            ThemeColorField::BgSelected => self.bg_selected,
            ThemeColorField::BgBar => self.bg_bar,
            ThemeColorField::BgTab => self.bg_tab,
            ThemeColorField::BgTabHover => self.bg_tab_hover,
            ThemeColorField::BgTabActive => self.bg_tab_active,
            ThemeColorField::BgCode => self.bg_code,
            ThemeColorField::BgCodeInline => self.bg_code_inline,
            ThemeColorField::BorderPrimary => self.border_primary,
            ThemeColorField::BorderSecondary => self.border_secondary,
            ThemeColorField::BorderTertiary => self.border_tertiary,
            ThemeColorField::BorderDecorative => self.border_decorative,
            ThemeColorField::Cursor => self.cursor,
            ThemeColorField::Selection => self.selection,
            ThemeColorField::LineHighlight => self.line_highlight,
            ThemeColorField::Heading => self.heading,
            ThemeColorField::Bold => self.bold,
            ThemeColorField::Italic => self.italic,
            ThemeColorField::Strikethrough => self.strikethrough,
            ThemeColorField::InlineCode => self.inline_code,
            ThemeColorField::Link => self.link,
            ThemeColorField::SyntaxMarker => self.syntax_marker,
            ThemeColorField::ListMarker => self.list_marker,
            ThemeColorField::CheckboxChecked => self.checkbox_checked,
            ThemeColorField::GutterHeading => self.gutter_heading,
        }
    }

    pub fn set_color(&mut self, field: ThemeColorField, color: Hsla) {
        match field {
            ThemeColorField::TextPrimary => self.text_primary = color,
            ThemeColorField::TextSecondary => self.text_secondary = color,
            ThemeColorField::TextBright => self.text_bright = color,
            ThemeColorField::TextAccent => self.text_accent = color,
            ThemeColorField::TextDanger => self.text_danger = color,
            ThemeColorField::BgPrimary => self.bg_primary = color,
            ThemeColorField::BgSecondary => self.bg_secondary = color,
            ThemeColorField::BgElevated => self.bg_elevated = color,
            ThemeColorField::BgHover => self.bg_hover = color,
            ThemeColorField::BgSelected => self.bg_selected = color,
            ThemeColorField::BgBar => self.bg_bar = color,
            ThemeColorField::BgTab => self.bg_tab = color,
            ThemeColorField::BgTabHover => self.bg_tab_hover = color,
            ThemeColorField::BgTabActive => self.bg_tab_active = color,
            ThemeColorField::BgCode => self.bg_code = color,
            ThemeColorField::BgCodeInline => self.bg_code_inline = color,
            ThemeColorField::BorderPrimary => self.border_primary = color,
            ThemeColorField::BorderSecondary => self.border_secondary = color,
            ThemeColorField::BorderTertiary => self.border_tertiary = color,
            ThemeColorField::BorderDecorative => self.border_decorative = color,
            ThemeColorField::Cursor => self.cursor = color,
            ThemeColorField::Selection => self.selection = color,
            ThemeColorField::LineHighlight => self.line_highlight = color,
            ThemeColorField::Heading => self.heading = color,
            ThemeColorField::Bold => self.bold = color,
            ThemeColorField::Italic => self.italic = color,
            ThemeColorField::Strikethrough => self.strikethrough = color,
            ThemeColorField::InlineCode => self.inline_code = color,
            ThemeColorField::Link => self.link = color,
            ThemeColorField::SyntaxMarker => self.syntax_marker = color,
            ThemeColorField::ListMarker => self.list_marker = color,
            ThemeColorField::CheckboxChecked => self.checkbox_checked = color,
            ThemeColorField::GutterHeading => self.gutter_heading = color,
        }
    }
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
            bg_primary: crate::hex_hsla!("#21252b"),
            bg_secondary: crate::hex_hsla!("#282c34"),
            bg_elevated: crate::hex_hsla!("#252a31"),
            bg_hover: crate::hex_hsla!("#313842"),
            bg_selected: crate::hex_hsla!("#2f3742"),
            bg_bar: crate::hex_hsla!("#22262d"),
            bg_tab: crate::hex_hsla!("#22262d"),
            bg_tab_hover: crate::hex_hsla!("#1f232a"),
            bg_tab_active: crate::hex_hsla!("#181b20"),
            bg_code: crate::hex_hsla!("#1c1e24"),
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
            syntax_marker: with_alpha(crate::hex_hsla!("#636d83"), 1.0),
            list_marker: crate::hex_hsla!("#e06c75"),
            checkbox_checked: crate::hex_hsla!("#3778ae"),
            gutter_heading: with_alpha(crate::hex_hsla!("#abb2bf"), 0.2),
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
            gutter_heading: with_alpha(crate::hex_hsla!("#37352f"), 0.2),
        }
    }

    pub fn milky_dark() -> Self {
        Self {
            // Text — warm tones on dark
            text_primary: crate::hex_hsla!("#c8c3b8"),
            text_secondary: crate::hex_hsla!("#7a766c"),
            text_bright: crate::hex_hsla!("#e8e4da"),
            text_accent: crate::hex_hsla!("#6daaed"),
            text_danger: crate::hex_hsla!("#eb5757"),
            // Background — warm dark tones
            bg_primary: crate::hex_hsla!("#1e1d1b"),
            bg_secondary: crate::hex_hsla!("#17161a"),
            bg_elevated: crate::hex_hsla!("#252420"),
            bg_hover: crate::hex_hsla!("#2e2d29"),
            bg_selected: crate::hex_hsla!("#353430"),
            bg_bar: crate::hex_hsla!("#222120"),
            bg_tab: crate::hex_hsla!("#222120"),
            bg_tab_hover: crate::hex_hsla!("#2a2926"),
            bg_tab_active: crate::hex_hsla!("#1e1d1b"),
            bg_code: crate::hex_hsla!("#252420"),
            bg_code_inline: crate::hex_hsla!("#2a2825"),
            // Border
            border_primary: crate::hex_hsla!("#3a38349e"),
            border_secondary: crate::hex_hsla!("#3a383455"),
            border_tertiary: crate::hex_hsla!("#4a4843"),
            border_decorative: with_alpha(crate::hex_hsla!("#7a766c"), 0.4),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#528bff"),
            selection: with_alpha(crate::hex_hsla!("#6daaed"), 0.2),
            line_highlight: with_alpha(crate::hex_hsla!("#3a3834"), 0.4),
            heading: crate::hex_hsla!("#e27570"),
            bold: crate::hex_hsla!("#d4a06a"),
            italic: crate::hex_hsla!("#c98de0"),
            strikethrough: crate::hex_hsla!("#7a766c"),
            inline_code: crate::hex_hsla!("#7db86a"),
            link: crate::hex_hsla!("#6daaed"),
            syntax_marker: with_alpha(crate::hex_hsla!("#808080"), 1.0),
            list_marker: crate::hex_hsla!("#e27570"),
            checkbox_checked: crate::hex_hsla!("#6daaed"),
            gutter_heading: with_alpha(crate::hex_hsla!("#c8c3b8"), 0.2),
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
            gutter_heading: with_alpha(crate::hex_hsla!("#383a42"), 0.2),
        }
    }

    pub fn ayu_dark() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#bfbdb6"),
            text_secondary: crate::hex_hsla!("#aeaca5"),
            text_bright: crate::hex_hsla!("#e6e1cf"),
            text_accent: crate::hex_hsla!("#5ac1fe"),
            text_danger: crate::hex_hsla!("#f07178"),
            // Background
            bg_primary: crate::hex_hsla!("#1e212b"),
            bg_secondary: crate::hex_hsla!("#242835"),
            bg_elevated: crate::hex_hsla!("#252936"),
            bg_hover: crate::hex_hsla!("#323849"),
            bg_selected: crate::hex_hsla!("#181D28"),
            bg_bar: crate::hex_hsla!("#242835"),
            bg_tab: crate::hex_hsla!("#242835"),
            bg_tab_hover: crate::hex_hsla!("#212530"),
            bg_tab_active: crate::hex_hsla!("#16181f"),
            bg_code: crate::hex_hsla!("#141823"),
            bg_code_inline: crate::hex_hsla!("#141823"),
            // Border
            border_primary: crate::hex_hsla!("#363a4f"),
            border_secondary: crate::hex_hsla!("#363a4f"),
            border_tertiary: crate::hex_hsla!("#363a4f"),
            border_decorative: with_alpha(crate::hex_hsla!("#5c6773"), 0.5),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#5496ff"),
            selection: with_alpha(crate::hex_hsla!("#5496ff"), 0.15),
            line_highlight: with_alpha(crate::hex_hsla!("#1f2430"), 0.7),
            heading: crate::hex_hsla!("#bfbdb6"),
            bold: crate::hex_hsla!("#8bb7ff"),
            italic: crate::hex_hsla!("#fe8f40"),
            strikethrough: crate::hex_hsla!("#5c6773"),
            inline_code: crate::hex_hsla!("#a9d94b"),
            link: crate::hex_hsla!("#fe8f40"),
            syntax_marker: with_alpha(crate::hex_hsla!("#65727f"), 1.0),
            list_marker: crate::hex_hsla!("#ff8f3f"),
            checkbox_checked: crate::hex_hsla!("#3778ae"),
            gutter_heading: with_alpha(crate::hex_hsla!("#bfbdb6"), 0.4),
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
            gutter_heading: with_alpha(crate::hex_hsla!("#5c6166"), 0.2),
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

pub fn parse_hex_color(input: &str) -> Option<Hsla> {
    let hex = input.trim().trim_start_matches('#');
    let (r, g, b, a) = match hex.len() {
        3 => {
            let r = parse_runtime_hex_nibble(*hex.as_bytes().first()?)?;
            let g = parse_runtime_hex_nibble(*hex.as_bytes().get(1)?)?;
            let b = parse_runtime_hex_nibble(*hex.as_bytes().get(2)?)?;
            (r * 17, g * 17, b * 17, 255)
        }
        4 => {
            let r = parse_runtime_hex_nibble(*hex.as_bytes().first()?)?;
            let g = parse_runtime_hex_nibble(*hex.as_bytes().get(1)?)?;
            let b = parse_runtime_hex_nibble(*hex.as_bytes().get(2)?)?;
            let a = parse_runtime_hex_nibble(*hex.as_bytes().get(3)?)?;
            (r * 17, g * 17, b * 17, a * 17)
        }
        6 => {
            let bytes = hex.as_bytes();
            (
                parse_runtime_hex_byte(bytes[0], bytes[1])?,
                parse_runtime_hex_byte(bytes[2], bytes[3])?,
                parse_runtime_hex_byte(bytes[4], bytes[5])?,
                255,
            )
        }
        8 => {
            let bytes = hex.as_bytes();
            (
                parse_runtime_hex_byte(bytes[0], bytes[1])?,
                parse_runtime_hex_byte(bytes[2], bytes[3])?,
                parse_runtime_hex_byte(bytes[4], bytes[5])?,
                parse_runtime_hex_byte(bytes[6], bytes[7])?,
            )
        }
        _ => return None,
    };

    Some(rgb_to_hsla(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ))
}

pub fn hex_color_has_alpha(input: &str) -> bool {
    matches!(input.trim().trim_start_matches('#').len(), 4 | 8)
}

pub fn hsla_to_hex_rgba(color: Hsla) -> String {
    let (r, g, b) = hsla_to_rgb(color.h, color.s, color.l);
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        channel_to_byte(r),
        channel_to_byte(g),
        channel_to_byte(b),
        channel_to_byte(color.a)
    )
}

fn parse_runtime_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn parse_runtime_hex_byte(high: u8, low: u8) -> Option<u8> {
    Some((parse_runtime_hex_nibble(high)? << 4) | parse_runtime_hex_nibble(low)?)
}

fn channel_to_byte(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn hsla_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s <= f32::EPSILON {
        return (l, l, l);
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;

    (
        hue_to_rgb(p, q, h + 1.0 / 3.0),
        hue_to_rgb(p, q, h),
        hue_to_rgb(p, q, h - 1.0 / 3.0),
    )
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 1.0 / 2.0 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
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
