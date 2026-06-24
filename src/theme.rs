use gpui::Hsla;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    OneDark,
    OneLight,
    MilkyDark,
    MilkyLight,
}

impl ThemeKind {
    pub fn dark_themes() -> &'static [ThemeKind] {
        &[Self::OneDark, Self::MilkyDark]
    }

    pub fn light_themes() -> &'static [ThemeKind] {
        &[Self::OneLight, Self::MilkyLight]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OneDark => "One Dark",
            Self::OneLight => "One Light",
            Self::MilkyDark => "Milky Dark",
            Self::MilkyLight => "Milky Light",
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, Self::OneDark | Self::MilkyDark)
    }

    pub fn build(self) -> Theme {
        match self {
            Self::OneDark => Theme::one_dark(),
            Self::OneLight => Theme::one_light(),
            Self::MilkyDark => Theme::ayu_dark(),
            Self::MilkyLight => Theme::milky(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorThemeMode {
    Colorful,
    Monochrome,
}

impl EditorThemeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Colorful => "colorful",
            Self::Monochrome => "monochrome",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::Colorful, Self::Monochrome]
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "colorful" | "Colorful" => Some(Self::Colorful),
            "monochrome" | "Monochrome" => Some(Self::Monochrome),
            _ => None,
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
    BorderCodeBlock,
    WindowBorder,
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
    GitStatusAdded,
    GitStatusModified,
    DiffAddedBg,
    DiffRemovedBg,
}

impl ThemeColorField {
    pub const UI_FIELDS: [Self; 19] = [
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
        Self::BorderPrimary,
        Self::BorderSecondary,
        Self::BorderTertiary,
        Self::BorderDecorative,
        Self::WindowBorder,
    ];

    pub const EDITOR_FIELDS: [Self; 20] = [
        Self::BgCode,
        Self::BgCodeInline,
        Self::BorderCodeBlock,
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
        Self::GitStatusAdded,
        Self::GitStatusModified,
        Self::DiffAddedBg,
        Self::DiffRemovedBg,
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
            Self::BorderCodeBlock => "border_code_block",
            Self::WindowBorder => "window_border",
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
            Self::GitStatusAdded => "git_status_added",
            Self::GitStatusModified => "git_status_modified",
            Self::DiffAddedBg => "diff_added_bg",
            Self::DiffRemovedBg => "diff_removed_bg",
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

/// Default outer window chrome border color for light themes.
///
/// Kept intentionally dark so the rounded window edge stays clearly visible
/// against bright surroundings. Used as the `window_border` field value for
/// light theme builders; also tunable per-theme via the theme debugger.
pub const LIGHT_WINDOW_BORDER: Hsla = hex_to_hsla("#00000033");

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
    pub border_code_block: Hsla,
    /// Outer window chrome border. Kept dark in light themes so the rounded
    /// window edge stays visible; tunable via the theme debugger.
    pub window_border: Hsla,

    // ── Editor / Markdown semantic colors ────────────────────────
    pub cursor: Hsla,
    pub selection: Hsla,
    pub line_highlight: Hsla,
    pub heading: Hsla,
    pub bold: Hsla,
    pub italic: Hsla,
    pub strikethrough: Hsla,
    pub inline_code: Hsla,
    pub inline_code_bg: Hsla,
    pub highlight_bg: Hsla,
    pub link: Hsla,
    pub syntax_marker: Hsla,
    pub list_marker: Hsla,
    pub checkbox_checked: Hsla,
    pub gutter_heading: Hsla,
    pub git_status_added: Hsla,
    pub git_status_modified: Hsla,
    pub diff_added_bg: Hsla,
    pub diff_removed_bg: Hsla,
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
            ThemeColorField::BorderCodeBlock => self.border_code_block,
            ThemeColorField::WindowBorder => self.window_border,
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
            ThemeColorField::GitStatusAdded => self.git_status_added,
            ThemeColorField::GitStatusModified => self.git_status_modified,
            ThemeColorField::DiffAddedBg => self.diff_added_bg,
            ThemeColorField::DiffRemovedBg => self.diff_removed_bg,
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
            ThemeColorField::BorderCodeBlock => self.border_code_block = color,
            ThemeColorField::WindowBorder => self.window_border = color,
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
            ThemeColorField::GitStatusAdded => self.git_status_added = color,
            ThemeColorField::GitStatusModified => self.git_status_modified = color,
            ThemeColorField::DiffAddedBg => self.diff_added_bg = color,
            ThemeColorField::DiffRemovedBg => self.diff_removed_bg = color,
        }
    }
}

pub fn apply_editor_theme_mode(theme: &Theme, mode: EditorThemeMode, is_dark: bool) -> Theme {
    let mut themed = theme.clone();
    if mode == EditorThemeMode::Monochrome {
        let monochrome_link = crate::hex_hsla!("#0057CDFF");
        let emphasis = if is_dark {
            themed.text_bright
        } else {
            themed.text_primary
        };
        let subtle = with_alpha(themed.text_secondary, if is_dark { 0.92 } else { 0.82 });
        themed.heading = if is_dark {
            with_alpha(emphasis, 0.88)
        } else {
            emphasis
        };
        themed.bold = themed.text_primary;
        themed.italic = subtle;
        themed.strikethrough = subtle;
        themed.inline_code = if is_dark {
            with_alpha(emphasis, 0.88)
        } else {
            emphasis
        };
        themed.link = monochrome_link;
        themed.syntax_marker = subtle;
        themed.list_marker = subtle;
        themed.checkbox_checked = subtle;
        themed.gutter_heading = with_alpha(themed.text_primary, if is_dark { 1.0 } else { 1.0 });
    }
    themed
}

impl Theme {
    pub fn one_dark() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#ABB2BF"),
            text_secondary: crate::hex_hsla!("#7F8793"),
            text_bright: crate::hex_hsla!("#FFFFFF"),
            text_accent: crate::hex_hsla!("#61AFEF"),
            text_danger: crate::hex_hsla!("#E06C75"),
            // Background
            bg_primary: crate::hex_hsla!("#21252B"),
            bg_secondary: crate::hex_hsla!("#282C34"),
            bg_elevated: crate::hex_hsla!("#252A31"),
            bg_hover: crate::hex_hsla!("#313842"),
            bg_selected: crate::hex_hsla!("#2F3742"),
            bg_bar: crate::hex_hsla!("#282C34"),
            bg_tab: crate::hex_hsla!("#282C34"),
            bg_tab_hover: crate::hex_hsla!("#21252B"),
            bg_tab_active: crate::hex_hsla!("#21252B"),
            bg_code: crate::hex_hsla!("#1D1F26"),
            bg_code_inline: crate::hex_hsla!("#2C313A"),
            // Border
            border_primary: crate::hex_hsla!("#67718633"),
            border_secondary: crate::hex_hsla!("#67718633"),
            border_tertiary: crate::hex_hsla!("#3E4452"),
            border_decorative: with_alpha(crate::hex_hsla!("#636D83"), 0.5),
            border_code_block: with_alpha(crate::hex_hsla!("#303740"), 1.0),
            window_border: crate::hex_hsla!("#67718633"),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#528BFF"),
            selection: with_alpha(crate::hex_hsla!("#5A6275"), 0.6),
            line_highlight: with_alpha(crate::hex_hsla!("#3E4451"), 0.5),
            heading: crate::hex_hsla!("#E06C75"),
            bold: crate::hex_hsla!("#D19A66"),
            italic: crate::hex_hsla!("#C678DD"),
            strikethrough: crate::hex_hsla!("#636D83"),
            inline_code: crate::hex_hsla!("#98C379"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#D8DCE8"), 0.14),
            highlight_bg: with_alpha(crate::hex_hsla!("#E5C07B"), 0.22),
            link: crate::hex_hsla!("#61AFEF"),
            syntax_marker: with_alpha(crate::hex_hsla!("#636D83"), 1.0),
            list_marker: crate::hex_hsla!("#E06C75"),
            checkbox_checked: crate::hex_hsla!("#3778AE"),
            gutter_heading: with_alpha(crate::hex_hsla!("#ABB2BF"), 0.2),
            git_status_added: crate::hex_hsla!("#3aa675"),
            git_status_modified: crate::hex_hsla!("#3aa675"),
            diff_added_bg: crate::hex_hsla!("#1b5e42"),
            diff_removed_bg: crate::hex_hsla!("#7a2738"),
        }
    }

    pub fn milky() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#37352F"),
            text_secondary: crate::hex_hsla!("#9B9A97"),
            text_bright: crate::hex_hsla!("#1A1A1A"),
            text_accent: crate::hex_hsla!("#2383E2"),
            text_danger: crate::hex_hsla!("#EB5757"),
            // Background
            bg_primary: crate::hex_hsla!("#FFF9F0"),
            bg_secondary: crate::hex_hsla!("#F6EEE2"),
            bg_elevated: crate::hex_hsla!("#F4E8D3"),
            bg_hover: crate::hex_hsla!("#E9E2D7"),
            bg_selected: crate::hex_hsla!("#E2DCD0"),
            bg_bar: crate::hex_hsla!("#F6EEE2"),
            bg_tab: crate::hex_hsla!("#F6EEE2"),
            bg_tab_hover: crate::hex_hsla!("#FDF7EE"),
            bg_tab_active: crate::hex_hsla!("#FDF7EE"),
            bg_code: crate::hex_hsla!("#FAF5ED"),
            bg_code_inline: crate::hex_hsla!("#F0ECE8"),
            // Border
            border_primary: crate::hex_hsla!("#D6D3CE9E"),
            border_secondary: crate::hex_hsla!("#D6CAB555"),
            border_tertiary: crate::hex_hsla!("#C7BCA9"),
            border_decorative: with_alpha(crate::hex_hsla!("#9B9A97"), 0.4),
            border_code_block: with_alpha(crate::hex_hsla!("#E7E2DB"), 0.8),
            window_border: LIGHT_WINDOW_BORDER,
            // Editor/Markdown
            cursor: crate::hex_hsla!("#528BFF"),
            selection: with_alpha(crate::hex_hsla!("#2383E2"), 0.15),
            line_highlight: with_alpha(crate::hex_hsla!("#D6D3CE"), 0.3),
            heading: crate::hex_hsla!("#C74440"),
            bold: crate::hex_hsla!("#C74440"),
            italic: crate::hex_hsla!("#8E44AD"),
            strikethrough: crate::hex_hsla!("#9B9A97"),
            inline_code: crate::hex_hsla!("#518A3C"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#E2E2E2"), 0.65),
            highlight_bg: with_alpha(crate::hex_hsla!("#DCEB88"), 0.55),
            link: crate::hex_hsla!("#2383E2"),
            syntax_marker: with_alpha(crate::hex_hsla!("#946666"), 1.0),
            list_marker: crate::hex_hsla!("#C74440"),
            checkbox_checked: crate::hex_hsla!("#2383E2"),
            gutter_heading: with_alpha(crate::hex_hsla!("#37352F"), 0.2),
            git_status_added: crate::hex_hsla!("#3AA675"),
            git_status_modified: crate::hex_hsla!("#3AA675"),
            diff_added_bg: crate::hex_hsla!("#217a4b"),
            diff_removed_bg: crate::hex_hsla!("#a44040"),
        }
    }

    pub fn one_light() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#323232"),
            text_secondary: crate::hex_hsla!("#676767"),
            text_bright: crate::hex_hsla!("#34363E"),
            text_accent: crate::hex_hsla!("#5C78E2"),
            text_danger: crate::hex_hsla!("#D3604F"),
            // Background
            bg_primary: crate::hex_hsla!("#FFFFFF"),
            bg_secondary: crate::hex_hsla!("#FBFBFB"),
            bg_elevated: crate::hex_hsla!("#F3F3F3"),
            bg_hover: crate::hex_hsla!("#F1F1F1"),
            bg_selected: crate::hex_hsla!("#F1F0F0"),
            bg_bar: crate::hex_hsla!("#FBFBFB"),
            bg_tab: crate::hex_hsla!("#FBFBFB"),
            bg_tab_hover: crate::hex_hsla!("#FFFFFF"),
            bg_tab_active: crate::hex_hsla!("#FFFFFF"),
            bg_code: crate::hex_hsla!("#FEFEFE"),
            bg_code_inline: crate::hex_hsla!("#ECEDEF"),
            // Border
            border_primary: crate::hex_hsla!("#EEEEEF"),
            border_secondary: crate::hex_hsla!("#B3B3B333"),
            border_tertiary: crate::hex_hsla!("#9AA4D0"),
            border_decorative: with_alpha(crate::hex_hsla!("#A0A1A7"), 0.4),
            border_code_block: with_alpha(crate::hex_hsla!("#AAB0CD"), 0.2),
            window_border: crate::hex_hsla!("#00000033"),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#657BCF"),
            selection: with_alpha(crate::hex_hsla!("#3A56C1"), 0.15),
            line_highlight: with_alpha(crate::hex_hsla!("#C3CAEB"), 0.2),
            heading: crate::hex_hsla!("#323232"),
            bold: crate::hex_hsla!("#323232"),
            italic: crate::hex_hsla!("#676767D1"),
            strikethrough: crate::hex_hsla!("#676767D1"),
            inline_code: crate::hex_hsla!("#323232"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#E5E5E5"), 0.78),
            highlight_bg: with_alpha(crate::hex_hsla!("#DDEB7A"), 0.5),
            link: crate::hex_hsla!("#0057CD"),
            syntax_marker: with_alpha(crate::hex_hsla!("#676767"), 0.82),
            list_marker: crate::hex_hsla!("#676767D1"),
            checkbox_checked: crate::hex_hsla!("#676767D1"),
            gutter_heading: with_alpha(crate::hex_hsla!("#323232"), 1.0),
            git_status_added: crate::hex_hsla!("#2F8F63"),
            git_status_modified: crate::hex_hsla!("#2F8F63"),
            diff_added_bg: crate::hex_hsla!("#1d6f46"),
            diff_removed_bg: crate::hex_hsla!("#a33b3b"),
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
            bg_selected: crate::hex_hsla!("#2C3141FF"),
            bg_bar: crate::hex_hsla!("#242835"),
            bg_tab: crate::hex_hsla!("#242835"),
            bg_tab_hover: crate::hex_hsla!("#1e212b"),
            bg_tab_active: crate::hex_hsla!("#1e212b"),
            bg_code: crate::hex_hsla!("#191D27"),
            bg_code_inline: crate::hex_hsla!("#141823"),
            // Border
            border_primary: crate::hex_hsla!("#363a4f"),
            border_secondary: crate::hex_hsla!("#363a4f"),
            border_tertiary: crate::hex_hsla!("#4B5067"),
            border_decorative: with_alpha(crate::hex_hsla!("#5c6773"), 0.5),
            border_code_block: with_alpha(crate::hex_hsla!("#2C3041"), 1.0),
            window_border: crate::hex_hsla!("#363a4f"),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#528bff"),
            selection: with_alpha(crate::hex_hsla!("#4F5768"), 0.6),
            line_highlight: with_alpha(crate::hex_hsla!("#3e4451"), 0.5),
            heading: crate::hex_hsla!("#e06c75"),
            bold: crate::hex_hsla!("#d19a66"),
            italic: crate::hex_hsla!("#c678dd"),
            strikethrough: crate::hex_hsla!("#636d83"),
            inline_code: crate::hex_hsla!("#98c379"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#D8DCE8"), 0.12),
            highlight_bg: with_alpha(crate::hex_hsla!("#E5C07B"), 0.22),
            link: crate::hex_hsla!("#61afef"),
            syntax_marker: with_alpha(crate::hex_hsla!("#6A748D"), 1.0),
            list_marker: crate::hex_hsla!("#e06c75"),
            checkbox_checked: crate::hex_hsla!("#3778ae"),
            gutter_heading: with_alpha(crate::hex_hsla!("#abb2bf"), 0.2),
            git_status_added: crate::hex_hsla!("#3aa675"),
            git_status_modified: crate::hex_hsla!("#3aa675"),
            diff_added_bg: crate::hex_hsla!("#1a5e40"),
            diff_removed_bg: crate::hex_hsla!("#7a2a37"),
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

#[cfg(test)]
mod tests {
    use super::{EditorThemeMode, Theme, apply_editor_theme_mode, with_alpha};

    #[test]
    fn monochrome_mode_keeps_code_block_colors() {
        let base = Theme::one_dark();
        let themed = apply_editor_theme_mode(&base, EditorThemeMode::Monochrome, true);

        assert_eq!(themed.bg_code, base.bg_code);
        assert_eq!(themed.border_code_block, base.border_code_block);
        assert_eq!(themed.heading, with_alpha(base.text_bright, 0.88));
        assert_eq!(themed.inline_code, with_alpha(base.text_bright, 0.88));
        assert_eq!(themed.link, crate::hex_hsla!("#61afef"));
    }

    #[test]
    fn monochrome_mode_uses_grayscale_markers_in_light_theme() {
        let base = Theme::one_light();
        let themed = apply_editor_theme_mode(&base, EditorThemeMode::Monochrome, false);

        assert_eq!(themed.heading, base.text_primary);
        assert_eq!(themed.inline_code, base.text_primary);
        assert_eq!(themed.link, crate::hex_hsla!("#61afef"));
        assert_eq!(themed.list_marker, with_alpha(base.text_secondary, 0.82));
        assert_eq!(themed.syntax_marker, with_alpha(base.text_secondary, 0.82));
    }
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
