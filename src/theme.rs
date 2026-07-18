use gpui::Hsla;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    OneDark,
    OneLight,
    MilkyDark,
    MilkyLight,
    Dracula,
    Nord,
    TokyoNight,
    CatppuccinMocha,
    GithubDark,
    SolarizedDark,
    GruvboxDark,
    RosePine,
    EverforestDark,
    MonokaiPro,
    GithubLight,
    SolarizedLight,
    CatppuccinLatte,
    RosePineDawn,
    GruvboxLight,
    EverforestLight,
    TokyoNightLight,
    NordLight,
    AyuLight,
    XcodeLight,
}

impl ThemeKind {
    pub fn dark_themes() -> &'static [ThemeKind] {
        &[
            Self::OneDark,
            Self::MilkyDark,
            Self::Dracula,
            Self::Nord,
            Self::TokyoNight,
            Self::CatppuccinMocha,
            Self::GithubDark,
            Self::SolarizedDark,
            Self::GruvboxDark,
            Self::RosePine,
            Self::EverforestDark,
            Self::MonokaiPro,
        ]
    }

    pub fn light_themes() -> &'static [ThemeKind] {
        &[
            Self::OneLight,
            Self::MilkyLight,
            Self::NordLight,
            Self::TokyoNightLight,
            Self::CatppuccinLatte,
            Self::GithubLight,
            Self::SolarizedLight,
            Self::GruvboxLight,
            Self::RosePineDawn,
            Self::EverforestLight,
            Self::AyuLight,
            Self::XcodeLight,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OneDark => "One Dark",
            Self::OneLight => "One Light",
            Self::MilkyDark => "Milky Dark",
            Self::MilkyLight => "Milky Light",
            Self::Dracula => "Dracula",
            Self::Nord => "Nord",
            Self::TokyoNight => "Tokyo Night",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::GithubDark => "GitHub Dark",
            Self::SolarizedDark => "Solarized Dark",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::RosePine => "Rosé Pine",
            Self::EverforestDark => "Everforest Dark",
            Self::MonokaiPro => "Monokai Pro",
            Self::GithubLight => "GitHub Light",
            Self::SolarizedLight => "Solarized Light",
            Self::CatppuccinLatte => "Catppuccin Latte",
            Self::RosePineDawn => "Rosé Pine Dawn",
            Self::GruvboxLight => "Gruvbox Light",
            Self::EverforestLight => "Everforest Light",
            Self::TokyoNightLight => "Tokyo Night Light",
            Self::NordLight => "Nord Light",
            Self::AyuLight => "Ayu Light",
            Self::XcodeLight => "Xcode Light",
        }
    }

    pub fn is_dark(self) -> bool {
        Self::dark_themes().contains(&self)
    }

    pub fn build(self) -> Theme {
        match self {
            Self::OneDark => Theme::one_dark(),
            Self::OneLight => Theme::one_light(),
            Self::MilkyDark => Theme::ayu_dark(),
            Self::MilkyLight => Theme::milky(),
            Self::Dracula => Theme::dracula(),
            Self::Nord => Theme::nord(),
            Self::TokyoNight => Theme::tokyo_night(),
            Self::CatppuccinMocha => Theme::catppuccin_mocha(),
            Self::GithubDark => Theme::github_dark(),
            Self::SolarizedDark => Theme::solarized_dark(),
            Self::GruvboxDark => Theme::gruvbox_dark(),
            Self::RosePine => Theme::rose_pine(),
            Self::EverforestDark => Theme::everforest_dark(),
            Self::MonokaiPro => Theme::monokai_pro(),
            Self::GithubLight => Theme::github_light(),
            Self::SolarizedLight => Theme::solarized_light(),
            Self::CatppuccinLatte => Theme::catppuccin_latte(),
            Self::RosePineDawn => Theme::rose_pine_dawn(),
            Self::GruvboxLight => Theme::gruvbox_light(),
            Self::EverforestLight => Theme::everforest_light(),
            Self::TokyoNightLight => Theme::tokyo_night_light(),
            Self::NordLight => Theme::nord_light(),
            Self::AyuLight => Theme::ayu_light(),
            Self::XcodeLight => Theme::xcode_light(),
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
    TextOnAccent,
    TextWarning,
    TextWarningHover,
    BgPrimary,
    BgSecondary,
    BgTertiary,
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
    InlineCodeBg,
    HighlightBg,
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
    pub const UI_FIELDS: [Self; 23] = [
        Self::TextPrimary,
        Self::TextSecondary,
        Self::TextBright,
        Self::TextAccent,
        Self::TextDanger,
        Self::TextOnAccent,
        Self::TextWarning,
        Self::TextWarningHover,
        Self::BgPrimary,
        Self::BgSecondary,
        Self::BgTertiary,
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

    pub const EDITOR_FIELDS: [Self; 22] = [
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
        Self::InlineCodeBg,
        Self::HighlightBg,
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
            Self::TextOnAccent => "text_on_accent",
            Self::TextWarning => "text_warning",
            Self::TextWarningHover => "text_warning_hover",
            Self::BgPrimary => "bg_primary",
            Self::BgSecondary => "bg_secondary",
            Self::BgTertiary => "bg_tertiary",
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
            Self::InlineCodeBg => "inline_code_bg",
            Self::HighlightBg => "highlight_bg",
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
pub const DARK_EDITOR_LINK: Hsla = hex_to_hsla("#5AA0FF");

/// Per-theme token colors for code block syntax highlighting.
///
/// Colors are packed as `0xRRGGBB` so the palette stays `Copy + Eq + Hash`
/// and can key the syntect theme / highlight caches directly. The palette is
/// carried on [`Theme`] but intentionally kept out of [`ThemeColorField`] /
/// the theme debugger.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CodeSyntaxPalette {
    pub keyword: u32,
    pub operator: u32,
    pub string: u32,
    pub function: u32,
    pub type_name: u32,
    pub number: u32,
    pub variable: u32,
    pub comment: u32,
    pub foreground: u32,
}

impl CodeSyntaxPalette {
    /// One Dark Pro — also used by Milky Dark (legacy behavior).
    pub const ONE_DARK: Self = Self {
        keyword: 0xC678DD,
        operator: 0x56B6C2,
        string: 0x98C379,
        function: 0x61AFEF,
        type_name: 0xE5C07B,
        number: 0xD19A66,
        variable: 0xE06C75,
        comment: 0x7F848E,
        foreground: 0xABB2BF,
    };

    /// One Light — also used by Milky Light.
    pub const ONE_LIGHT: Self = Self {
        keyword: 0xA626A4,
        operator: 0x0184BC,
        string: 0x50A14F,
        function: 0x4078F2,
        type_name: 0xC18401,
        number: 0x986801,
        variable: 0xE45649,
        comment: 0xA0A1A7,
        foreground: 0x383A42,
    };

    pub const DRACULA: Self = Self {
        keyword: 0xFF79C6,
        operator: 0xFF79C6,
        string: 0xF1FA8C,
        function: 0x50FA7B,
        type_name: 0x8BE9FD,
        number: 0xBD93F9,
        variable: 0xFFB86C,
        comment: 0x6272A4,
        foreground: 0xF8F8F2,
    };

    pub const NORD: Self = Self {
        keyword: 0x81A1C1,
        operator: 0x81A1C1,
        string: 0xA3BE8C,
        function: 0x88C0D0,
        type_name: 0x8FBCBB,
        number: 0xB48EAD,
        variable: 0xD8DEE9,
        comment: 0x616E88,
        foreground: 0xD8DEE9,
    };

    pub const TOKYO_NIGHT: Self = Self {
        keyword: 0xBB9AF7,
        operator: 0x89DDFF,
        string: 0x9ECE6A,
        function: 0x7AA2F7,
        type_name: 0x2AC3DE,
        number: 0xFF9E64,
        variable: 0x73DACA,
        comment: 0x565F89,
        foreground: 0xA9B1D6,
    };

    pub const CATPPUCCIN_MOCHA: Self = Self {
        keyword: 0xCBA6F7,
        operator: 0x89DCEB,
        string: 0xA6E3A1,
        function: 0x89B4FA,
        type_name: 0xF9E2AF,
        number: 0xFAB387,
        variable: 0xB4BEFE,
        comment: 0x6C7086,
        foreground: 0xCDD6F4,
    };

    pub const GITHUB_DARK: Self = Self {
        keyword: 0xFF7B72,
        operator: 0xFF7B72,
        string: 0xA5D6FF,
        function: 0xD2A8FF,
        type_name: 0xFFA657,
        number: 0x79C0FF,
        variable: 0xFFA657,
        comment: 0x8B949E,
        foreground: 0xE6EDF3,
    };

    pub const SOLARIZED_DARK: Self = Self {
        keyword: 0x859900,
        operator: 0x859900,
        string: 0x2AA198,
        function: 0x268BD2,
        type_name: 0xB58900,
        number: 0xD33682,
        variable: 0x268BD2,
        comment: 0x586E75,
        foreground: 0x839496,
    };

    pub const GRUVBOX_DARK: Self = Self {
        keyword: 0xFB4934,
        operator: 0xFE8019,
        string: 0xB8BB26,
        function: 0xFABD2F,
        type_name: 0x8EC07C,
        number: 0xD3869B,
        variable: 0x83A598,
        comment: 0x928374,
        foreground: 0xEBDBB2,
    };

    pub const ROSE_PINE: Self = Self {
        keyword: 0x31748F,
        operator: 0x908CAA,
        string: 0xF6C177,
        function: 0xEBBCBA,
        type_name: 0x9CCFD8,
        number: 0xC4A7E7,
        variable: 0xE0DEF4,
        comment: 0x6E6A86,
        foreground: 0xE0DEF4,
    };

    pub const EVERFOREST_DARK: Self = Self {
        keyword: 0xE67E80,
        operator: 0xE69875,
        string: 0xA7C080,
        function: 0x7FBBB3,
        type_name: 0xDBBC7F,
        number: 0xD699B6,
        variable: 0x83C092,
        comment: 0x859289,
        foreground: 0xD3C6AA,
    };

    pub const MONOKAI_PRO: Self = Self {
        keyword: 0xFF6188,
        operator: 0xFF6188,
        string: 0xFFD866,
        function: 0xA9DC76,
        type_name: 0x78DCE8,
        number: 0xAB9DF2,
        variable: 0xFCFCFA,
        comment: 0x727072,
        foreground: 0xFCFCFA,
    };

    /// Tokyo Night Light (enkia light variant).
    pub const TOKYO_NIGHT_LIGHT: Self = Self {
        keyword: 0x5A4A78,
        operator: 0x0F4B6E,
        string: 0x485E30,
        function: 0x34548A,
        type_name: 0x8F5E15,
        number: 0x965027,
        variable: 0x33635C,
        comment: 0x9699A3,
        foreground: 0x343B58,
    };

    /// Nord palette accents darkened for readable contrast on Snow Storm.
    pub const NORD_LIGHT: Self = Self {
        keyword: 0x5E81AC,
        operator: 0x5E81AC,
        string: 0x6C8A55,
        function: 0x3E7B91,
        type_name: 0x497D7B,
        number: 0x9A6590,
        variable: 0x2E3440,
        comment: 0x8892A5,
        foreground: 0x2E3440,
    };

    pub const AYU_LIGHT: Self = Self {
        keyword: 0xFA8D3E,
        operator: 0xED9366,
        string: 0x86B300,
        function: 0xF2AE49,
        type_name: 0x399EE6,
        number: 0xA37ACC,
        variable: 0x55B4D4,
        comment: 0x787B80,
        foreground: 0x5C6166,
    };

    /// Xcode "Default (Light)" source colors.
    pub const XCODE_LIGHT: Self = Self {
        keyword: 0x9B2393,
        operator: 0x643820,
        string: 0xC41A16,
        function: 0x326D74,
        type_name: 0x3900A0,
        number: 0x1C00CF,
        variable: 0x947100,
        comment: 0x5D6C79,
        foreground: 0x262626,
    };

    pub const GITHUB_LIGHT: Self = Self {
        keyword: 0xCF222E,
        operator: 0xCF222E,
        string: 0x0A3069,
        function: 0x8250DF,
        type_name: 0x953800,
        number: 0x0550AE,
        variable: 0x953800,
        comment: 0x6E7781,
        foreground: 0x1F2328,
    };

    pub const SOLARIZED_LIGHT: Self = Self {
        keyword: 0x859900,
        operator: 0x859900,
        string: 0x2AA198,
        function: 0x268BD2,
        type_name: 0xB58900,
        number: 0xD33682,
        variable: 0x268BD2,
        comment: 0x93A1A1,
        foreground: 0x657B83,
    };

    pub const CATPPUCCIN_LATTE: Self = Self {
        keyword: 0x8839EF,
        operator: 0x04A5E5,
        string: 0x40A02B,
        function: 0x1E66F5,
        type_name: 0xDF8E1D,
        number: 0xFE640B,
        variable: 0x7287FD,
        comment: 0x8C8FA1,
        foreground: 0x4C4F69,
    };

    pub const ROSE_PINE_DAWN: Self = Self {
        keyword: 0x286983,
        operator: 0x797593,
        string: 0xEA9D34,
        function: 0xD7827E,
        type_name: 0x56949F,
        number: 0x907AA9,
        variable: 0x575279,
        comment: 0x9893A5,
        foreground: 0x575279,
    };

    pub const GRUVBOX_LIGHT: Self = Self {
        keyword: 0x9D0006,
        operator: 0xAF3A03,
        string: 0x79740E,
        function: 0xB57614,
        type_name: 0x427B58,
        number: 0x8F3F71,
        variable: 0x076678,
        comment: 0x7C6F64,
        foreground: 0x3C3836,
    };

    pub const EVERFOREST_LIGHT: Self = Self {
        keyword: 0xF85552,
        operator: 0xF57D26,
        string: 0x8DA101,
        function: 0x3A94C5,
        type_name: 0xDFA000,
        number: 0xDF69BA,
        variable: 0x35A77C,
        comment: 0x939F91,
        foreground: 0x5C6A72,
    };
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
    pub text_on_accent: Hsla,
    pub text_warning: Hsla,
    pub text_warning_hover: Hsla,

    // ── Background colors ───────────────────────────────────────
    pub bg_primary: Hsla,
    pub bg_secondary: Hsla,
    pub bg_tertiary: Hsla,
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

    // ── Code block syntax highlighting ──────────────────────
    /// Token palette for code block / mermaid syntax highlighting.
    /// Not part of [`ThemeColorField`]; not exposed in the theme debugger.
    pub code_palette: CodeSyntaxPalette,
}

impl Theme {
    pub fn color(&self, field: ThemeColorField) -> Hsla {
        match field {
            ThemeColorField::TextPrimary => self.text_primary,
            ThemeColorField::TextSecondary => self.text_secondary,
            ThemeColorField::TextBright => self.text_bright,
            ThemeColorField::TextAccent => self.text_accent,
            ThemeColorField::TextDanger => self.text_danger,
            ThemeColorField::TextOnAccent => self.text_on_accent,
            ThemeColorField::TextWarning => self.text_warning,
            ThemeColorField::TextWarningHover => self.text_warning_hover,
            ThemeColorField::BgPrimary => self.bg_primary,
            ThemeColorField::BgSecondary => self.bg_secondary,
            ThemeColorField::BgTertiary => self.bg_tertiary,
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
            ThemeColorField::InlineCodeBg => self.inline_code_bg,
            ThemeColorField::HighlightBg => self.highlight_bg,
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
            ThemeColorField::TextOnAccent => self.text_on_accent = color,
            ThemeColorField::TextWarning => self.text_warning = color,
            ThemeColorField::TextWarningHover => self.text_warning_hover = color,
            ThemeColorField::BgPrimary => self.bg_primary = color,
            ThemeColorField::BgSecondary => self.bg_secondary = color,
            ThemeColorField::BgTertiary => self.bg_tertiary = color,
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
            ThemeColorField::InlineCodeBg => self.inline_code_bg = color,
            ThemeColorField::HighlightBg => self.highlight_bg = color,
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
    if is_dark {
        themed.link = DARK_EDITOR_LINK;
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
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#D6A100"),
            text_warning_hover: crate::hex_hsla!("#FFD24D"),
            // Background
            bg_primary: crate::hex_hsla!("#21252B"),
            bg_secondary: crate::hex_hsla!("#282C34"),
            bg_tertiary: crate::hex_hsla!("#2E323B"),
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
            git_status_added: crate::hex_hsla!("#88C796"),
            git_status_modified: crate::hex_hsla!("#DDC194"),
            diff_added_bg: crate::hex_hsla!("#1b5e42"),
            diff_removed_bg: crate::hex_hsla!("#7a2738"),
            code_palette: CodeSyntaxPalette::ONE_DARK,
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
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#8A6400"),
            text_warning_hover: crate::hex_hsla!("#A87900"),
            // Background
            bg_primary: crate::hex_hsla!("#FFF9F0"),
            bg_secondary: crate::hex_hsla!("#F6EEE2"),
            bg_tertiary: crate::hex_hsla!("#F9F4EC"),
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
            git_status_added: crate::hex_hsla!("#88C796"),
            git_status_modified: crate::hex_hsla!("#DDC194"),
            diff_added_bg: crate::hex_hsla!("#217a4b"),
            diff_removed_bg: crate::hex_hsla!("#a44040"),
            code_palette: CodeSyntaxPalette::ONE_LIGHT,
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
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#8A6400"),
            text_warning_hover: crate::hex_hsla!("#A87900"),
            // Background
            bg_primary: crate::hex_hsla!("#FFFFFF"),
            bg_secondary: crate::hex_hsla!("#F6F6F6"),
            bg_tertiary: crate::hex_hsla!("#FCFCFC"),
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
            git_status_added: crate::hex_hsla!("#88C796"),
            git_status_modified: crate::hex_hsla!("#DDC194"),
            diff_added_bg: crate::hex_hsla!("#1d6f46"),
            diff_removed_bg: crate::hex_hsla!("#a33b3b"),
            code_palette: CodeSyntaxPalette::ONE_LIGHT,
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
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#D6A100"),
            text_warning_hover: crate::hex_hsla!("#FFD24D"),
            // Background
            bg_primary: crate::hex_hsla!("#1e212b"),
            bg_secondary: crate::hex_hsla!("#242835"),
            bg_tertiary: crate::hex_hsla!("#292E3D"),
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
            git_status_added: crate::hex_hsla!("#88C796"),
            git_status_modified: crate::hex_hsla!("#DDC194"),
            diff_added_bg: crate::hex_hsla!("#1a5e40"),
            diff_removed_bg: crate::hex_hsla!("#7a2a37"),
            code_palette: CodeSyntaxPalette::ONE_DARK,
        }
    }

    pub fn dracula() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#F8F8F2"),
            text_secondary: crate::hex_hsla!("#A9ABBE"),
            text_bright: crate::hex_hsla!("#FFFFFF"),
            text_accent: crate::hex_hsla!("#BD93F9"),
            text_danger: crate::hex_hsla!("#FF5555"),
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#D6A100"),
            text_warning_hover: crate::hex_hsla!("#FFD24D"),
            // Background
            bg_primary: crate::hex_hsla!("#21222C"),
            bg_secondary: crate::hex_hsla!("#282A36"),
            bg_tertiary: crate::hex_hsla!("#2E3040"),
            bg_elevated: crate::hex_hsla!("#252734"),
            bg_hover: crate::hex_hsla!("#343747"),
            bg_selected: crate::hex_hsla!("#3B3E51"),
            bg_bar: crate::hex_hsla!("#282A36"),
            bg_tab: crate::hex_hsla!("#282A36"),
            bg_tab_hover: crate::hex_hsla!("#21222C"),
            bg_tab_active: crate::hex_hsla!("#21222C"),
            bg_code: crate::hex_hsla!("#1E1F29"),
            bg_code_inline: crate::hex_hsla!("#2F3240"),
            // Border
            border_primary: crate::hex_hsla!("#6272A433"),
            border_secondary: crate::hex_hsla!("#6272A433"),
            border_tertiary: crate::hex_hsla!("#44475A"),
            border_decorative: with_alpha(crate::hex_hsla!("#6272A4"), 0.5),
            border_code_block: with_alpha(crate::hex_hsla!("#2F3240"), 1.0),
            window_border: crate::hex_hsla!("#6272A433"),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#BD93F9"),
            selection: with_alpha(crate::hex_hsla!("#44475A"), 0.8),
            line_highlight: with_alpha(crate::hex_hsla!("#44475A"), 0.4),
            heading: crate::hex_hsla!("#FF79C6"),
            bold: crate::hex_hsla!("#FFB86C"),
            italic: crate::hex_hsla!("#BD93F9"),
            strikethrough: crate::hex_hsla!("#6272A4"),
            inline_code: crate::hex_hsla!("#50FA7B"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#D8DCE8"), 0.14),
            highlight_bg: with_alpha(crate::hex_hsla!("#F1FA8C"), 0.22),
            link: crate::hex_hsla!("#8BE9FD"),
            syntax_marker: with_alpha(crate::hex_hsla!("#6272A4"), 1.0),
            list_marker: crate::hex_hsla!("#FF79C6"),
            checkbox_checked: crate::hex_hsla!("#BD93F9"),
            gutter_heading: with_alpha(crate::hex_hsla!("#F8F8F2"), 0.2),
            git_status_added: crate::hex_hsla!("#50FA7B"),
            git_status_modified: crate::hex_hsla!("#F1FA8C"),
            diff_added_bg: crate::hex_hsla!("#1b5e42"),
            diff_removed_bg: crate::hex_hsla!("#7a2738"),
            code_palette: CodeSyntaxPalette::DRACULA,
        }
    }

    pub fn nord() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#D8DEE9"),
            text_secondary: crate::hex_hsla!("#7B88A1"),
            text_bright: crate::hex_hsla!("#ECEFF4"),
            text_accent: crate::hex_hsla!("#88C0D0"),
            text_danger: crate::hex_hsla!("#BF616A"),
            text_on_accent: crate::hex_hsla!("#2E3440"),
            text_warning: crate::hex_hsla!("#D6A100"),
            text_warning_hover: crate::hex_hsla!("#FFD24D"),
            // Background
            bg_primary: crate::hex_hsla!("#292E39"),
            bg_secondary: crate::hex_hsla!("#2E3440"),
            bg_tertiary: crate::hex_hsla!("#333947"),
            bg_elevated: crate::hex_hsla!("#2B303C"),
            bg_hover: crate::hex_hsla!("#3B4252"),
            bg_selected: crate::hex_hsla!("#3F4859"),
            bg_bar: crate::hex_hsla!("#2E3440"),
            bg_tab: crate::hex_hsla!("#2E3440"),
            bg_tab_hover: crate::hex_hsla!("#292E39"),
            bg_tab_active: crate::hex_hsla!("#292E39"),
            bg_code: crate::hex_hsla!("#252A33"),
            bg_code_inline: crate::hex_hsla!("#353C4A"),
            // Border
            border_primary: crate::hex_hsla!("#4C566A33"),
            border_secondary: crate::hex_hsla!("#4C566A33"),
            border_tertiary: crate::hex_hsla!("#4C566A"),
            border_decorative: with_alpha(crate::hex_hsla!("#616E88"), 0.5),
            border_code_block: with_alpha(crate::hex_hsla!("#343B49"), 1.0),
            window_border: crate::hex_hsla!("#4C566A33"),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#88C0D0"),
            selection: with_alpha(crate::hex_hsla!("#4C566A"), 0.6),
            line_highlight: with_alpha(crate::hex_hsla!("#3B4252"), 0.6),
            heading: crate::hex_hsla!("#88C0D0"),
            bold: crate::hex_hsla!("#EBCB8B"),
            italic: crate::hex_hsla!("#B48EAD"),
            strikethrough: crate::hex_hsla!("#616E88"),
            inline_code: crate::hex_hsla!("#A3BE8C"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#D8DEE9"), 0.14),
            highlight_bg: with_alpha(crate::hex_hsla!("#EBCB8B"), 0.22),
            link: crate::hex_hsla!("#81A1C1"),
            syntax_marker: with_alpha(crate::hex_hsla!("#616E88"), 1.0),
            list_marker: crate::hex_hsla!("#81A1C1"),
            checkbox_checked: crate::hex_hsla!("#A3BE8C"),
            gutter_heading: with_alpha(crate::hex_hsla!("#D8DEE9"), 0.2),
            git_status_added: crate::hex_hsla!("#A3BE8C"),
            git_status_modified: crate::hex_hsla!("#EBCB8B"),
            diff_added_bg: crate::hex_hsla!("#1b5e42"),
            diff_removed_bg: crate::hex_hsla!("#7a2738"),
            code_palette: CodeSyntaxPalette::NORD,
        }
    }

    pub fn tokyo_night() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#C0CAF5"),
            text_secondary: crate::hex_hsla!("#7E86AD"),
            text_bright: crate::hex_hsla!("#DDE3FE"),
            text_accent: crate::hex_hsla!("#7AA2F7"),
            text_danger: crate::hex_hsla!("#F7768E"),
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#D6A100"),
            text_warning_hover: crate::hex_hsla!("#FFD24D"),
            // Background
            bg_primary: crate::hex_hsla!("#16161E"),
            bg_secondary: crate::hex_hsla!("#1A1B26"),
            bg_tertiary: crate::hex_hsla!("#20212E"),
            bg_elevated: crate::hex_hsla!("#191A24"),
            bg_hover: crate::hex_hsla!("#252738"),
            bg_selected: crate::hex_hsla!("#2A2E44"),
            bg_bar: crate::hex_hsla!("#1A1B26"),
            bg_tab: crate::hex_hsla!("#1A1B26"),
            bg_tab_hover: crate::hex_hsla!("#16161E"),
            bg_tab_active: crate::hex_hsla!("#16161E"),
            bg_code: crate::hex_hsla!("#111117"),
            bg_code_inline: crate::hex_hsla!("#24263B"),
            // Border
            border_primary: crate::hex_hsla!("#565F8933"),
            border_secondary: crate::hex_hsla!("#565F8933"),
            border_tertiary: crate::hex_hsla!("#3B4261"),
            border_decorative: with_alpha(crate::hex_hsla!("#565F89"), 0.5),
            border_code_block: with_alpha(crate::hex_hsla!("#1F2130"), 1.0),
            window_border: crate::hex_hsla!("#565F8933"),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#7AA2F7"),
            selection: with_alpha(crate::hex_hsla!("#3D59A1"), 0.4),
            line_highlight: with_alpha(crate::hex_hsla!("#292E42"), 0.8),
            heading: crate::hex_hsla!("#7AA2F7"),
            bold: crate::hex_hsla!("#FF9E64"),
            italic: crate::hex_hsla!("#BB9AF7"),
            strikethrough: crate::hex_hsla!("#565F89"),
            inline_code: crate::hex_hsla!("#73DACA"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#D8DCE8"), 0.12),
            highlight_bg: with_alpha(crate::hex_hsla!("#E0AF68"), 0.22),
            link: crate::hex_hsla!("#7DCFFF"),
            syntax_marker: with_alpha(crate::hex_hsla!("#565F89"), 1.0),
            list_marker: crate::hex_hsla!("#7AA2F7"),
            checkbox_checked: crate::hex_hsla!("#9ECE6A"),
            gutter_heading: with_alpha(crate::hex_hsla!("#C0CAF5"), 0.2),
            git_status_added: crate::hex_hsla!("#9ECE6A"),
            git_status_modified: crate::hex_hsla!("#E0AF68"),
            diff_added_bg: crate::hex_hsla!("#1b5e42"),
            diff_removed_bg: crate::hex_hsla!("#7a2738"),
            code_palette: CodeSyntaxPalette::TOKYO_NIGHT,
        }
    }

    pub fn catppuccin_mocha() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#CDD6F4"),
            text_secondary: crate::hex_hsla!("#A6ADC8"),
            text_bright: crate::hex_hsla!("#F5E0DC"),
            text_accent: crate::hex_hsla!("#89B4FA"),
            text_danger: crate::hex_hsla!("#F38BA8"),
            text_on_accent: crate::hex_hsla!("#1E1E2E"),
            text_warning: crate::hex_hsla!("#D6A100"),
            text_warning_hover: crate::hex_hsla!("#FFD24D"),
            // Background
            bg_primary: crate::hex_hsla!("#181825"),
            bg_secondary: crate::hex_hsla!("#1E1E2E"),
            bg_tertiary: crate::hex_hsla!("#252539"),
            bg_elevated: crate::hex_hsla!("#1B1B2A"),
            bg_hover: crate::hex_hsla!("#2A2A40"),
            bg_selected: crate::hex_hsla!("#313244"),
            bg_bar: crate::hex_hsla!("#1E1E2E"),
            bg_tab: crate::hex_hsla!("#1E1E2E"),
            bg_tab_hover: crate::hex_hsla!("#181825"),
            bg_tab_active: crate::hex_hsla!("#181825"),
            bg_code: crate::hex_hsla!("#141420"),
            bg_code_inline: crate::hex_hsla!("#2A2A3E"),
            // Border
            border_primary: crate::hex_hsla!("#6C708633"),
            border_secondary: crate::hex_hsla!("#6C708633"),
            border_tertiary: crate::hex_hsla!("#45475A"),
            border_decorative: with_alpha(crate::hex_hsla!("#6C7086"), 0.5),
            border_code_block: with_alpha(crate::hex_hsla!("#26263A"), 1.0),
            window_border: crate::hex_hsla!("#6C708633"),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#F5E0DC"),
            selection: with_alpha(crate::hex_hsla!("#585B70"), 0.6),
            line_highlight: with_alpha(crate::hex_hsla!("#313244"), 0.6),
            heading: crate::hex_hsla!("#CBA6F7"),
            bold: crate::hex_hsla!("#FAB387"),
            italic: crate::hex_hsla!("#F5C2E7"),
            strikethrough: crate::hex_hsla!("#6C7086"),
            inline_code: crate::hex_hsla!("#A6E3A1"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#D8DCE8"), 0.12),
            highlight_bg: with_alpha(crate::hex_hsla!("#F9E2AF"), 0.22),
            link: crate::hex_hsla!("#89B4FA"),
            syntax_marker: with_alpha(crate::hex_hsla!("#6C7086"), 1.0),
            list_marker: crate::hex_hsla!("#CBA6F7"),
            checkbox_checked: crate::hex_hsla!("#89B4FA"),
            gutter_heading: with_alpha(crate::hex_hsla!("#CDD6F4"), 0.2),
            git_status_added: crate::hex_hsla!("#A6E3A1"),
            git_status_modified: crate::hex_hsla!("#F9E2AF"),
            diff_added_bg: crate::hex_hsla!("#1b5e42"),
            diff_removed_bg: crate::hex_hsla!("#7a2738"),
            code_palette: CodeSyntaxPalette::CATPPUCCIN_MOCHA,
        }
    }

    pub fn github_dark() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#E6EDF3"),
            text_secondary: crate::hex_hsla!("#8B949E"),
            text_bright: crate::hex_hsla!("#FFFFFF"),
            text_accent: crate::hex_hsla!("#58A6FF"),
            text_danger: crate::hex_hsla!("#F85149"),
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#D6A100"),
            text_warning_hover: crate::hex_hsla!("#FFD24D"),
            // Background
            bg_primary: crate::hex_hsla!("#0D1117"),
            bg_secondary: crate::hex_hsla!("#161B22"),
            bg_tertiary: crate::hex_hsla!("#1C2129"),
            bg_elevated: crate::hex_hsla!("#11161D"),
            bg_hover: crate::hex_hsla!("#21262D"),
            bg_selected: crate::hex_hsla!("#262C36"),
            bg_bar: crate::hex_hsla!("#161B22"),
            bg_tab: crate::hex_hsla!("#161B22"),
            bg_tab_hover: crate::hex_hsla!("#0D1117"),
            bg_tab_active: crate::hex_hsla!("#0D1117"),
            bg_code: crate::hex_hsla!("#0A0D12"),
            bg_code_inline: crate::hex_hsla!("#1F242C"),
            // Border
            border_primary: crate::hex_hsla!("#30363D"),
            border_secondary: crate::hex_hsla!("#21262D"),
            border_tertiary: crate::hex_hsla!("#3D444D"),
            border_decorative: with_alpha(crate::hex_hsla!("#8B949E"), 0.4),
            border_code_block: with_alpha(crate::hex_hsla!("#21262D"), 1.0),
            window_border: crate::hex_hsla!("#30363D"),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#58A6FF"),
            selection: with_alpha(crate::hex_hsla!("#58A6FF"), 0.35),
            line_highlight: with_alpha(crate::hex_hsla!("#6E7681"), 0.25),
            heading: crate::hex_hsla!("#79C0FF"),
            bold: crate::hex_hsla!("#FFA657"),
            italic: crate::hex_hsla!("#BC8CFF"),
            strikethrough: crate::hex_hsla!("#8B949E"),
            inline_code: crate::hex_hsla!("#A5D6FF"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#D8DCE8"), 0.12),
            highlight_bg: with_alpha(crate::hex_hsla!("#D29922"), 0.22),
            link: crate::hex_hsla!("#58A6FF"),
            syntax_marker: with_alpha(crate::hex_hsla!("#6E7681"), 1.0),
            list_marker: crate::hex_hsla!("#79C0FF"),
            checkbox_checked: crate::hex_hsla!("#3FB950"),
            gutter_heading: with_alpha(crate::hex_hsla!("#E6EDF3"), 0.2),
            git_status_added: crate::hex_hsla!("#3FB950"),
            git_status_modified: crate::hex_hsla!("#D29922"),
            diff_added_bg: crate::hex_hsla!("#1b5e42"),
            diff_removed_bg: crate::hex_hsla!("#7a2738"),
            code_palette: CodeSyntaxPalette::GITHUB_DARK,
        }
    }

    pub fn solarized_dark() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#839496"),
            text_secondary: crate::hex_hsla!("#657B83"),
            text_bright: crate::hex_hsla!("#93A1A1"),
            text_accent: crate::hex_hsla!("#268BD2"),
            text_danger: crate::hex_hsla!("#DC322F"),
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#D6A100"),
            text_warning_hover: crate::hex_hsla!("#FFD24D"),
            // Background
            bg_primary: crate::hex_hsla!("#002B36"),
            bg_secondary: crate::hex_hsla!("#073642"),
            bg_tertiary: crate::hex_hsla!("#0A3C4A"),
            bg_elevated: crate::hex_hsla!("#04303C"),
            bg_hover: crate::hex_hsla!("#0E4250"),
            bg_selected: crate::hex_hsla!("#12495A"),
            bg_bar: crate::hex_hsla!("#073642"),
            bg_tab: crate::hex_hsla!("#073642"),
            bg_tab_hover: crate::hex_hsla!("#002B36"),
            bg_tab_active: crate::hex_hsla!("#002B36"),
            bg_code: crate::hex_hsla!("#00242D"),
            bg_code_inline: crate::hex_hsla!("#0A3B48"),
            // Border
            border_primary: crate::hex_hsla!("#586E7533"),
            border_secondary: crate::hex_hsla!("#586E7533"),
            border_tertiary: crate::hex_hsla!("#1A5766"),
            border_decorative: with_alpha(crate::hex_hsla!("#586E75"), 0.5),
            border_code_block: with_alpha(crate::hex_hsla!("#0A3945"), 1.0),
            window_border: crate::hex_hsla!("#586E7533"),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#268BD2"),
            selection: with_alpha(crate::hex_hsla!("#586E75"), 0.5),
            line_highlight: with_alpha(crate::hex_hsla!("#073642"), 0.7),
            heading: crate::hex_hsla!("#CB4B16"),
            bold: crate::hex_hsla!("#B58900"),
            italic: crate::hex_hsla!("#6C71C4"),
            strikethrough: crate::hex_hsla!("#586E75"),
            inline_code: crate::hex_hsla!("#2AA198"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#93A1A1"), 0.14),
            highlight_bg: with_alpha(crate::hex_hsla!("#B58900"), 0.25),
            link: crate::hex_hsla!("#268BD2"),
            syntax_marker: with_alpha(crate::hex_hsla!("#586E75"), 1.0),
            list_marker: crate::hex_hsla!("#CB4B16"),
            checkbox_checked: crate::hex_hsla!("#859900"),
            gutter_heading: with_alpha(crate::hex_hsla!("#839496"), 0.25),
            git_status_added: crate::hex_hsla!("#859900"),
            git_status_modified: crate::hex_hsla!("#B58900"),
            diff_added_bg: crate::hex_hsla!("#1b5e42"),
            diff_removed_bg: crate::hex_hsla!("#7a2738"),
            code_palette: CodeSyntaxPalette::SOLARIZED_DARK,
        }
    }

    pub fn gruvbox_dark() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#EBDBB2"),
            text_secondary: crate::hex_hsla!("#A89984"),
            text_bright: crate::hex_hsla!("#FBF1C7"),
            text_accent: crate::hex_hsla!("#83A598"),
            text_danger: crate::hex_hsla!("#FB4934"),
            text_on_accent: crate::hex_hsla!("#1D2021"),
            text_warning: crate::hex_hsla!("#D6A100"),
            text_warning_hover: crate::hex_hsla!("#FFD24D"),
            // Background
            bg_primary: crate::hex_hsla!("#1D2021"),
            bg_secondary: crate::hex_hsla!("#282828"),
            bg_tertiary: crate::hex_hsla!("#32302F"),
            bg_elevated: crate::hex_hsla!("#242322"),
            bg_hover: crate::hex_hsla!("#3C3836"),
            bg_selected: crate::hex_hsla!("#45403D"),
            bg_bar: crate::hex_hsla!("#282828"),
            bg_tab: crate::hex_hsla!("#282828"),
            bg_tab_hover: crate::hex_hsla!("#1D2021"),
            bg_tab_active: crate::hex_hsla!("#1D2021"),
            bg_code: crate::hex_hsla!("#191C1D"),
            bg_code_inline: crate::hex_hsla!("#363432"),
            // Border
            border_primary: crate::hex_hsla!("#92837433"),
            border_secondary: crate::hex_hsla!("#92837433"),
            border_tertiary: crate::hex_hsla!("#504945"),
            border_decorative: with_alpha(crate::hex_hsla!("#928374"), 0.5),
            border_code_block: with_alpha(crate::hex_hsla!("#2E2C2B"), 1.0),
            window_border: crate::hex_hsla!("#92837433"),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#FE8019"),
            selection: with_alpha(crate::hex_hsla!("#665C54"), 0.6),
            line_highlight: with_alpha(crate::hex_hsla!("#3C3836"), 0.6),
            heading: crate::hex_hsla!("#FE8019"),
            bold: crate::hex_hsla!("#FABD2F"),
            italic: crate::hex_hsla!("#D3869B"),
            strikethrough: crate::hex_hsla!("#928374"),
            inline_code: crate::hex_hsla!("#B8BB26"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#EBDBB2"), 0.12),
            highlight_bg: with_alpha(crate::hex_hsla!("#FABD2F"), 0.22),
            link: crate::hex_hsla!("#83A598"),
            syntax_marker: with_alpha(crate::hex_hsla!("#928374"), 1.0),
            list_marker: crate::hex_hsla!("#FE8019"),
            checkbox_checked: crate::hex_hsla!("#B8BB26"),
            gutter_heading: with_alpha(crate::hex_hsla!("#EBDBB2"), 0.2),
            git_status_added: crate::hex_hsla!("#B8BB26"),
            git_status_modified: crate::hex_hsla!("#FABD2F"),
            diff_added_bg: crate::hex_hsla!("#1b5e42"),
            diff_removed_bg: crate::hex_hsla!("#7a2738"),
            code_palette: CodeSyntaxPalette::GRUVBOX_DARK,
        }
    }

    pub fn rose_pine() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#E0DEF4"),
            text_secondary: crate::hex_hsla!("#908CAA"),
            text_bright: crate::hex_hsla!("#F4F2FF"),
            text_accent: crate::hex_hsla!("#C4A7E7"),
            text_danger: crate::hex_hsla!("#EB6F92"),
            text_on_accent: crate::hex_hsla!("#191724"),
            text_warning: crate::hex_hsla!("#D6A100"),
            text_warning_hover: crate::hex_hsla!("#FFD24D"),
            // Background
            bg_primary: crate::hex_hsla!("#191724"),
            bg_secondary: crate::hex_hsla!("#1F1D2E"),
            bg_tertiary: crate::hex_hsla!("#26233A"),
            bg_elevated: crate::hex_hsla!("#1C1A29"),
            bg_hover: crate::hex_hsla!("#2A273F"),
            bg_selected: crate::hex_hsla!("#343048"),
            bg_bar: crate::hex_hsla!("#1F1D2E"),
            bg_tab: crate::hex_hsla!("#1F1D2E"),
            bg_tab_hover: crate::hex_hsla!("#191724"),
            bg_tab_active: crate::hex_hsla!("#191724"),
            bg_code: crate::hex_hsla!("#15131F"),
            bg_code_inline: crate::hex_hsla!("#26233A"),
            // Border
            border_primary: crate::hex_hsla!("#6E6A8633"),
            border_secondary: crate::hex_hsla!("#6E6A8633"),
            border_tertiary: crate::hex_hsla!("#403D52"),
            border_decorative: with_alpha(crate::hex_hsla!("#6E6A86"), 0.5),
            border_code_block: with_alpha(crate::hex_hsla!("#232033"), 1.0),
            window_border: crate::hex_hsla!("#6E6A8633"),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#9CCFD8"),
            selection: with_alpha(crate::hex_hsla!("#524F67"), 0.6),
            line_highlight: with_alpha(crate::hex_hsla!("#26233A"), 0.7),
            heading: crate::hex_hsla!("#EB6F92"),
            bold: crate::hex_hsla!("#F6C177"),
            italic: crate::hex_hsla!("#C4A7E7"),
            strikethrough: crate::hex_hsla!("#6E6A86"),
            inline_code: crate::hex_hsla!("#9CCFD8"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#E0DEF4"), 0.12),
            highlight_bg: with_alpha(crate::hex_hsla!("#F6C177"), 0.22),
            link: crate::hex_hsla!("#C4A7E7"),
            syntax_marker: with_alpha(crate::hex_hsla!("#6E6A86"), 1.0),
            list_marker: crate::hex_hsla!("#EB6F92"),
            checkbox_checked: crate::hex_hsla!("#31748F"),
            gutter_heading: with_alpha(crate::hex_hsla!("#E0DEF4"), 0.2),
            git_status_added: crate::hex_hsla!("#9CCFD8"),
            git_status_modified: crate::hex_hsla!("#F6C177"),
            diff_added_bg: crate::hex_hsla!("#1b5e42"),
            diff_removed_bg: crate::hex_hsla!("#7a2738"),
            code_palette: CodeSyntaxPalette::ROSE_PINE,
        }
    }

    pub fn github_light() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#1F2328"),
            text_secondary: crate::hex_hsla!("#656D76"),
            text_bright: crate::hex_hsla!("#0A0C10"),
            text_accent: crate::hex_hsla!("#0969DA"),
            text_danger: crate::hex_hsla!("#CF222E"),
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#8A6400"),
            text_warning_hover: crate::hex_hsla!("#A87900"),
            // Background
            bg_primary: crate::hex_hsla!("#FFFFFF"),
            bg_secondary: crate::hex_hsla!("#F6F8FA"),
            bg_tertiary: crate::hex_hsla!("#FAFBFC"),
            bg_elevated: crate::hex_hsla!("#F3F5F7"),
            bg_hover: crate::hex_hsla!("#EFF2F5"),
            bg_selected: crate::hex_hsla!("#EAEEF2"),
            bg_bar: crate::hex_hsla!("#F6F8FA"),
            bg_tab: crate::hex_hsla!("#F6F8FA"),
            bg_tab_hover: crate::hex_hsla!("#FFFFFF"),
            bg_tab_active: crate::hex_hsla!("#FFFFFF"),
            bg_code: crate::hex_hsla!("#FAFBFC"),
            bg_code_inline: crate::hex_hsla!("#EAEEF2"),
            // Border
            border_primary: crate::hex_hsla!("#D8DEE4"),
            border_secondary: crate::hex_hsla!("#D0D7DE66"),
            border_tertiary: crate::hex_hsla!("#AFB8C1"),
            border_decorative: with_alpha(crate::hex_hsla!("#8C959F"), 0.4),
            border_code_block: with_alpha(crate::hex_hsla!("#D0D7DE"), 0.8),
            window_border: LIGHT_WINDOW_BORDER,
            // Editor/Markdown
            cursor: crate::hex_hsla!("#0969DA"),
            selection: with_alpha(crate::hex_hsla!("#0969DA"), 0.15),
            line_highlight: with_alpha(crate::hex_hsla!("#AFB8C1"), 0.2),
            heading: crate::hex_hsla!("#0550AE"),
            bold: crate::hex_hsla!("#953800"),
            italic: crate::hex_hsla!("#8250DF"),
            strikethrough: crate::hex_hsla!("#6E7781"),
            inline_code: crate::hex_hsla!("#0A3069"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#AFB8C1"), 0.3),
            highlight_bg: with_alpha(crate::hex_hsla!("#D4A72C"), 0.3),
            link: crate::hex_hsla!("#0969DA"),
            syntax_marker: with_alpha(crate::hex_hsla!("#6E7781"), 0.9),
            list_marker: crate::hex_hsla!("#0550AE"),
            checkbox_checked: crate::hex_hsla!("#1A7F37"),
            gutter_heading: with_alpha(crate::hex_hsla!("#1F2328"), 0.25),
            git_status_added: crate::hex_hsla!("#1A7F37"),
            git_status_modified: crate::hex_hsla!("#9A6700"),
            diff_added_bg: crate::hex_hsla!("#1d6f46"),
            diff_removed_bg: crate::hex_hsla!("#a33b3b"),
            code_palette: CodeSyntaxPalette::GITHUB_LIGHT,
        }
    }

    pub fn solarized_light() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#657B83"),
            text_secondary: crate::hex_hsla!("#839496"),
            text_bright: crate::hex_hsla!("#073642"),
            text_accent: crate::hex_hsla!("#268BD2"),
            text_danger: crate::hex_hsla!("#DC322F"),
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#8A6400"),
            text_warning_hover: crate::hex_hsla!("#A87900"),
            // Background
            bg_primary: crate::hex_hsla!("#FDF6E3"),
            bg_secondary: crate::hex_hsla!("#EEE8D5"),
            bg_tertiary: crate::hex_hsla!("#F7F0DD"),
            bg_elevated: crate::hex_hsla!("#F5EEDC"),
            bg_hover: crate::hex_hsla!("#E8E1CC"),
            bg_selected: crate::hex_hsla!("#E1DAC4"),
            bg_bar: crate::hex_hsla!("#EEE8D5"),
            bg_tab: crate::hex_hsla!("#EEE8D5"),
            bg_tab_hover: crate::hex_hsla!("#FDF6E3"),
            bg_tab_active: crate::hex_hsla!("#FDF6E3"),
            bg_code: crate::hex_hsla!("#F9F2DF"),
            bg_code_inline: crate::hex_hsla!("#ECE5D1"),
            // Border
            border_primary: crate::hex_hsla!("#D3CCB899"),
            border_secondary: crate::hex_hsla!("#D3CCB855"),
            border_tertiary: crate::hex_hsla!("#B8B09A"),
            border_decorative: with_alpha(crate::hex_hsla!("#93A1A1"), 0.4),
            border_code_block: with_alpha(crate::hex_hsla!("#E0D9C5"), 0.9),
            window_border: LIGHT_WINDOW_BORDER,
            // Editor/Markdown
            cursor: crate::hex_hsla!("#268BD2"),
            selection: with_alpha(crate::hex_hsla!("#268BD2"), 0.15),
            line_highlight: with_alpha(crate::hex_hsla!("#D9D2C0"), 0.35),
            heading: crate::hex_hsla!("#CB4B16"),
            bold: crate::hex_hsla!("#B58900"),
            italic: crate::hex_hsla!("#6C71C4"),
            strikethrough: crate::hex_hsla!("#93A1A1"),
            inline_code: crate::hex_hsla!("#2AA198"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#D9D2C0"), 0.5),
            highlight_bg: with_alpha(crate::hex_hsla!("#B58900"), 0.25),
            link: crate::hex_hsla!("#268BD2"),
            syntax_marker: with_alpha(crate::hex_hsla!("#93A1A1"), 1.0),
            list_marker: crate::hex_hsla!("#CB4B16"),
            checkbox_checked: crate::hex_hsla!("#859900"),
            gutter_heading: with_alpha(crate::hex_hsla!("#657B83"), 0.25),
            git_status_added: crate::hex_hsla!("#859900"),
            git_status_modified: crate::hex_hsla!("#B58900"),
            diff_added_bg: crate::hex_hsla!("#1d6f46"),
            diff_removed_bg: crate::hex_hsla!("#a33b3b"),
            code_palette: CodeSyntaxPalette::SOLARIZED_LIGHT,
        }
    }

    pub fn catppuccin_latte() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#4C4F69"),
            text_secondary: crate::hex_hsla!("#6C6F85"),
            text_bright: crate::hex_hsla!("#232638"),
            text_accent: crate::hex_hsla!("#1E66F5"),
            text_danger: crate::hex_hsla!("#D20F39"),
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#8A6400"),
            text_warning_hover: crate::hex_hsla!("#A87900"),
            // Background
            bg_primary: crate::hex_hsla!("#EFF1F5"),
            bg_secondary: crate::hex_hsla!("#E6E9EF"),
            bg_tertiary: crate::hex_hsla!("#EBEDF2"),
            bg_elevated: crate::hex_hsla!("#E9EBF1"),
            bg_hover: crate::hex_hsla!("#DFE3EA"),
            bg_selected: crate::hex_hsla!("#D9DDE6"),
            bg_bar: crate::hex_hsla!("#E6E9EF"),
            bg_tab: crate::hex_hsla!("#E6E9EF"),
            bg_tab_hover: crate::hex_hsla!("#EFF1F5"),
            bg_tab_active: crate::hex_hsla!("#EFF1F5"),
            bg_code: crate::hex_hsla!("#F5F6F9"),
            bg_code_inline: crate::hex_hsla!("#DCE0E8"),
            // Border
            border_primary: crate::hex_hsla!("#BCC0CC99"),
            border_secondary: crate::hex_hsla!("#BCC0CC55"),
            border_tertiary: crate::hex_hsla!("#9CA0B0"),
            border_decorative: with_alpha(crate::hex_hsla!("#8C8FA1"), 0.4),
            border_code_block: with_alpha(crate::hex_hsla!("#CCD0DA"), 0.8),
            window_border: LIGHT_WINDOW_BORDER,
            // Editor/Markdown
            cursor: crate::hex_hsla!("#DC8A78"),
            selection: with_alpha(crate::hex_hsla!("#1E66F5"), 0.15),
            line_highlight: with_alpha(crate::hex_hsla!("#CCD0DA"), 0.35),
            heading: crate::hex_hsla!("#8839EF"),
            bold: crate::hex_hsla!("#FE640B"),
            italic: crate::hex_hsla!("#EA76CB"),
            strikethrough: crate::hex_hsla!("#8C8FA1"),
            inline_code: crate::hex_hsla!("#40A02B"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#BCC0CC"), 0.35),
            highlight_bg: with_alpha(crate::hex_hsla!("#DF8E1D"), 0.25),
            link: crate::hex_hsla!("#1E66F5"),
            syntax_marker: with_alpha(crate::hex_hsla!("#8C8FA1"), 1.0),
            list_marker: crate::hex_hsla!("#8839EF"),
            checkbox_checked: crate::hex_hsla!("#40A02B"),
            gutter_heading: with_alpha(crate::hex_hsla!("#4C4F69"), 0.25),
            git_status_added: crate::hex_hsla!("#40A02B"),
            git_status_modified: crate::hex_hsla!("#DF8E1D"),
            diff_added_bg: crate::hex_hsla!("#1d6f46"),
            diff_removed_bg: crate::hex_hsla!("#a33b3b"),
            code_palette: CodeSyntaxPalette::CATPPUCCIN_LATTE,
        }
    }

    pub fn rose_pine_dawn() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#575279"),
            text_secondary: crate::hex_hsla!("#797593"),
            text_bright: crate::hex_hsla!("#444066"),
            text_accent: crate::hex_hsla!("#907AA9"),
            text_danger: crate::hex_hsla!("#B4637A"),
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#8A6400"),
            text_warning_hover: crate::hex_hsla!("#A87900"),
            // Background
            bg_primary: crate::hex_hsla!("#FAF4ED"),
            bg_secondary: crate::hex_hsla!("#F2E9E1"),
            bg_tertiary: crate::hex_hsla!("#F6EFE7"),
            bg_elevated: crate::hex_hsla!("#F4EDE8"),
            bg_hover: crate::hex_hsla!("#EBE2DA"),
            bg_selected: crate::hex_hsla!("#E5DDD7"),
            bg_bar: crate::hex_hsla!("#F2E9E1"),
            bg_tab: crate::hex_hsla!("#F2E9E1"),
            bg_tab_hover: crate::hex_hsla!("#FAF4ED"),
            bg_tab_active: crate::hex_hsla!("#FAF4ED"),
            bg_code: crate::hex_hsla!("#FFFAF3"),
            bg_code_inline: crate::hex_hsla!("#EFE6DE"),
            // Border
            border_primary: crate::hex_hsla!("#CECACD99"),
            border_secondary: crate::hex_hsla!("#CECACD66"),
            border_tertiary: crate::hex_hsla!("#B5AFB8"),
            border_decorative: with_alpha(crate::hex_hsla!("#9893A5"), 0.45),
            border_code_block: with_alpha(crate::hex_hsla!("#DFDAD9"), 0.9),
            window_border: LIGHT_WINDOW_BORDER,
            // Editor/Markdown
            cursor: crate::hex_hsla!("#56949F"),
            selection: with_alpha(crate::hex_hsla!("#907AA9"), 0.18),
            line_highlight: with_alpha(crate::hex_hsla!("#DFDAD9"), 0.4),
            heading: crate::hex_hsla!("#B4637A"),
            bold: crate::hex_hsla!("#EA9D34"),
            italic: crate::hex_hsla!("#907AA9"),
            strikethrough: crate::hex_hsla!("#9893A5"),
            inline_code: crate::hex_hsla!("#286983"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#DFDAD9"), 0.6),
            highlight_bg: with_alpha(crate::hex_hsla!("#EA9D34"), 0.25),
            link: crate::hex_hsla!("#907AA9"),
            syntax_marker: with_alpha(crate::hex_hsla!("#9893A5"), 1.0),
            list_marker: crate::hex_hsla!("#B4637A"),
            checkbox_checked: crate::hex_hsla!("#286983"),
            gutter_heading: with_alpha(crate::hex_hsla!("#575279"), 0.25),
            git_status_added: crate::hex_hsla!("#56949F"),
            git_status_modified: crate::hex_hsla!("#EA9D34"),
            diff_added_bg: crate::hex_hsla!("#1d6f46"),
            diff_removed_bg: crate::hex_hsla!("#a33b3b"),
            code_palette: CodeSyntaxPalette::ROSE_PINE_DAWN,
        }
    }

    pub fn gruvbox_light() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#3C3836"),
            text_secondary: crate::hex_hsla!("#7C6F64"),
            text_bright: crate::hex_hsla!("#282828"),
            text_accent: crate::hex_hsla!("#076678"),
            text_danger: crate::hex_hsla!("#9D0006"),
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#8A6400"),
            text_warning_hover: crate::hex_hsla!("#A87900"),
            // Background
            bg_primary: crate::hex_hsla!("#F9F5D7"),
            bg_secondary: crate::hex_hsla!("#F2E5BC"),
            bg_tertiary: crate::hex_hsla!("#F6EDCC"),
            bg_elevated: crate::hex_hsla!("#F5ECC9"),
            bg_hover: crate::hex_hsla!("#EBDBB2"),
            bg_selected: crate::hex_hsla!("#E0D0A8"),
            bg_bar: crate::hex_hsla!("#F2E5BC"),
            bg_tab: crate::hex_hsla!("#F2E5BC"),
            bg_tab_hover: crate::hex_hsla!("#F9F5D7"),
            bg_tab_active: crate::hex_hsla!("#F9F5D7"),
            bg_code: crate::hex_hsla!("#FAF2CE"),
            bg_code_inline: crate::hex_hsla!("#EDE0B8"),
            // Border
            border_primary: crate::hex_hsla!("#D5C4A199"),
            border_secondary: crate::hex_hsla!("#D5C4A155"),
            border_tertiary: crate::hex_hsla!("#BDAE93"),
            border_decorative: with_alpha(crate::hex_hsla!("#928374"), 0.45),
            border_code_block: with_alpha(crate::hex_hsla!("#E5D5AC"), 0.9),
            window_border: LIGHT_WINDOW_BORDER,
            // Editor/Markdown
            cursor: crate::hex_hsla!("#AF3A03"),
            selection: with_alpha(crate::hex_hsla!("#076678"), 0.15),
            line_highlight: with_alpha(crate::hex_hsla!("#EBDBB2"), 0.5),
            heading: crate::hex_hsla!("#AF3A03"),
            bold: crate::hex_hsla!("#B57614"),
            italic: crate::hex_hsla!("#8F3F71"),
            strikethrough: crate::hex_hsla!("#928374"),
            inline_code: crate::hex_hsla!("#79740E"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#D5C4A1"), 0.4),
            highlight_bg: with_alpha(crate::hex_hsla!("#B57614"), 0.25),
            link: crate::hex_hsla!("#076678"),
            syntax_marker: with_alpha(crate::hex_hsla!("#7C6F64"), 1.0),
            list_marker: crate::hex_hsla!("#AF3A03"),
            checkbox_checked: crate::hex_hsla!("#79740E"),
            gutter_heading: with_alpha(crate::hex_hsla!("#3C3836"), 0.25),
            git_status_added: crate::hex_hsla!("#79740E"),
            git_status_modified: crate::hex_hsla!("#B57614"),
            diff_added_bg: crate::hex_hsla!("#1d6f46"),
            diff_removed_bg: crate::hex_hsla!("#a33b3b"),
            code_palette: CodeSyntaxPalette::GRUVBOX_LIGHT,
        }
    }

    pub fn everforest_light() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#5C6A72"),
            text_secondary: crate::hex_hsla!("#829181"),
            text_bright: crate::hex_hsla!("#3A464C"),
            text_accent: crate::hex_hsla!("#3A94C5"),
            text_danger: crate::hex_hsla!("#F85552"),
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#8A6400"),
            text_warning_hover: crate::hex_hsla!("#A87900"),
            // Background
            bg_primary: crate::hex_hsla!("#FDF6E3"),
            bg_secondary: crate::hex_hsla!("#F4F0D9"),
            bg_tertiary: crate::hex_hsla!("#F9F3DE"),
            bg_elevated: crate::hex_hsla!("#F8F2DD"),
            bg_hover: crate::hex_hsla!("#EFEBD4"),
            bg_selected: crate::hex_hsla!("#E6E2CC"),
            bg_bar: crate::hex_hsla!("#F4F0D9"),
            bg_tab: crate::hex_hsla!("#F4F0D9"),
            bg_tab_hover: crate::hex_hsla!("#FDF6E3"),
            bg_tab_active: crate::hex_hsla!("#FDF6E3"),
            bg_code: crate::hex_hsla!("#FAF3DF"),
            bg_code_inline: crate::hex_hsla!("#ECE8D1"),
            // Border
            border_primary: crate::hex_hsla!("#E0DCC799"),
            border_secondary: crate::hex_hsla!("#E0DCC766"),
            border_tertiary: crate::hex_hsla!("#BDC3AF"),
            border_decorative: with_alpha(crate::hex_hsla!("#939F91"), 0.5),
            border_code_block: with_alpha(crate::hex_hsla!("#E6E2CC"), 0.9),
            window_border: LIGHT_WINDOW_BORDER,
            // Editor/Markdown
            cursor: crate::hex_hsla!("#35A77C"),
            selection: with_alpha(crate::hex_hsla!("#35A77C"), 0.15),
            line_highlight: with_alpha(crate::hex_hsla!("#EFEBD4"), 0.6),
            heading: crate::hex_hsla!("#F85552"),
            bold: crate::hex_hsla!("#DFA000"),
            italic: crate::hex_hsla!("#DF69BA"),
            strikethrough: crate::hex_hsla!("#939F91"),
            inline_code: crate::hex_hsla!("#8DA101"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#E0DCC7"), 0.5),
            highlight_bg: with_alpha(crate::hex_hsla!("#DFA000"), 0.25),
            link: crate::hex_hsla!("#3A94C5"),
            syntax_marker: with_alpha(crate::hex_hsla!("#939F91"), 1.0),
            list_marker: crate::hex_hsla!("#F85552"),
            checkbox_checked: crate::hex_hsla!("#8DA101"),
            gutter_heading: with_alpha(crate::hex_hsla!("#5C6A72"), 0.25),
            git_status_added: crate::hex_hsla!("#8DA101"),
            git_status_modified: crate::hex_hsla!("#DFA000"),
            diff_added_bg: crate::hex_hsla!("#1d6f46"),
            diff_removed_bg: crate::hex_hsla!("#a33b3b"),
            code_palette: CodeSyntaxPalette::EVERFOREST_LIGHT,
        }
    }

    pub fn tokyo_night_light() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#343B58"),
            text_secondary: crate::hex_hsla!("#71769B"),
            text_bright: crate::hex_hsla!("#1F2335"),
            text_accent: crate::hex_hsla!("#34548A"),
            text_danger: crate::hex_hsla!("#8C4351"),
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#8A6400"),
            text_warning_hover: crate::hex_hsla!("#A87900"),
            // Background
            bg_primary: crate::hex_hsla!("#D5D6DB"),
            bg_secondary: crate::hex_hsla!("#CBCCD3"),
            bg_tertiary: crate::hex_hsla!("#D0D1D7"),
            bg_elevated: crate::hex_hsla!("#D2D3D9"),
            bg_hover: crate::hex_hsla!("#C2C4CD"),
            bg_selected: crate::hex_hsla!("#B9BCC7"),
            bg_bar: crate::hex_hsla!("#CBCCD3"),
            bg_tab: crate::hex_hsla!("#CBCCD3"),
            bg_tab_hover: crate::hex_hsla!("#D5D6DB"),
            bg_tab_active: crate::hex_hsla!("#D5D6DB"),
            bg_code: crate::hex_hsla!("#DCDDE2"),
            bg_code_inline: crate::hex_hsla!("#C2C4CD"),
            // Border
            border_primary: crate::hex_hsla!("#9699A399"),
            border_secondary: crate::hex_hsla!("#9699A355"),
            border_tertiary: crate::hex_hsla!("#888B94"),
            border_decorative: with_alpha(crate::hex_hsla!("#9699A3"), 0.5),
            border_code_block: with_alpha(crate::hex_hsla!("#B8BAC4"), 0.9),
            window_border: LIGHT_WINDOW_BORDER,
            // Editor/Markdown
            cursor: crate::hex_hsla!("#34548A"),
            selection: with_alpha(crate::hex_hsla!("#34548A"), 0.16),
            line_highlight: with_alpha(crate::hex_hsla!("#B6B8C2"), 0.4),
            heading: crate::hex_hsla!("#34548A"),
            bold: crate::hex_hsla!("#965027"),
            italic: crate::hex_hsla!("#5A4A78"),
            strikethrough: crate::hex_hsla!("#9699A3"),
            inline_code: crate::hex_hsla!("#33635C"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#9699A3"), 0.25),
            highlight_bg: with_alpha(crate::hex_hsla!("#8F5E15"), 0.25),
            link: crate::hex_hsla!("#0F4B6E"),
            syntax_marker: with_alpha(crate::hex_hsla!("#888B94"), 1.0),
            list_marker: crate::hex_hsla!("#34548A"),
            checkbox_checked: crate::hex_hsla!("#485E30"),
            gutter_heading: with_alpha(crate::hex_hsla!("#343B58"), 0.25),
            git_status_added: crate::hex_hsla!("#485E30"),
            git_status_modified: crate::hex_hsla!("#8F5E15"),
            diff_added_bg: crate::hex_hsla!("#1d6f46"),
            diff_removed_bg: crate::hex_hsla!("#a33b3b"),
            code_palette: CodeSyntaxPalette::TOKYO_NIGHT_LIGHT,
        }
    }

    pub fn nord_light() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#2E3440"),
            text_secondary: crate::hex_hsla!("#6A7488"),
            text_bright: crate::hex_hsla!("#242933"),
            text_accent: crate::hex_hsla!("#5E81AC"),
            text_danger: crate::hex_hsla!("#BF616A"),
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#8A6400"),
            text_warning_hover: crate::hex_hsla!("#A87900"),
            // Background
            bg_primary: crate::hex_hsla!("#ECEFF4"),
            bg_secondary: crate::hex_hsla!("#E5E9F0"),
            bg_tertiary: crate::hex_hsla!("#E9ECF2"),
            bg_elevated: crate::hex_hsla!("#EAEDF3"),
            bg_hover: crate::hex_hsla!("#DDE3EC"),
            bg_selected: crate::hex_hsla!("#D4DBE7"),
            bg_bar: crate::hex_hsla!("#E5E9F0"),
            bg_tab: crate::hex_hsla!("#E5E9F0"),
            bg_tab_hover: crate::hex_hsla!("#ECEFF4"),
            bg_tab_active: crate::hex_hsla!("#ECEFF4"),
            bg_code: crate::hex_hsla!("#F1F3F8"),
            bg_code_inline: crate::hex_hsla!("#DBE1EA"),
            // Border
            border_primary: crate::hex_hsla!("#C2CBD999"),
            border_secondary: crate::hex_hsla!("#C2CBD966"),
            border_tertiary: crate::hex_hsla!("#AEB7C6"),
            border_decorative: with_alpha(crate::hex_hsla!("#8892A5"), 0.45),
            border_code_block: with_alpha(crate::hex_hsla!("#D8DEE9"), 0.9),
            window_border: LIGHT_WINDOW_BORDER,
            // Editor/Markdown
            cursor: crate::hex_hsla!("#5E81AC"),
            selection: with_alpha(crate::hex_hsla!("#5E81AC"), 0.16),
            line_highlight: with_alpha(crate::hex_hsla!("#D8DEE9"), 0.5),
            heading: crate::hex_hsla!("#5E81AC"),
            bold: crate::hex_hsla!("#B25B36"),
            italic: crate::hex_hsla!("#9A6590"),
            strikethrough: crate::hex_hsla!("#8892A5"),
            inline_code: crate::hex_hsla!("#6C8A55"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#AEB7C6"), 0.3),
            highlight_bg: with_alpha(crate::hex_hsla!("#C89138"), 0.25),
            link: crate::hex_hsla!("#3E7B91"),
            syntax_marker: with_alpha(crate::hex_hsla!("#8892A5"), 1.0),
            list_marker: crate::hex_hsla!("#5E81AC"),
            checkbox_checked: crate::hex_hsla!("#6C8A55"),
            gutter_heading: with_alpha(crate::hex_hsla!("#2E3440"), 0.25),
            git_status_added: crate::hex_hsla!("#6C8A55"),
            git_status_modified: crate::hex_hsla!("#B08800"),
            diff_added_bg: crate::hex_hsla!("#1d6f46"),
            diff_removed_bg: crate::hex_hsla!("#a33b3b"),
            code_palette: CodeSyntaxPalette::NORD_LIGHT,
        }
    }

    pub fn everforest_dark() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#D3C6AA"),
            text_secondary: crate::hex_hsla!("#9DA9A0"),
            text_bright: crate::hex_hsla!("#F2EFDF"),
            text_accent: crate::hex_hsla!("#7FBBB3"),
            text_danger: crate::hex_hsla!("#E67E80"),
            text_on_accent: crate::hex_hsla!("#2D353B"),
            text_warning: crate::hex_hsla!("#D6A100"),
            text_warning_hover: crate::hex_hsla!("#FFD24D"),
            // Background
            bg_primary: crate::hex_hsla!("#232A2E"),
            bg_secondary: crate::hex_hsla!("#2D353B"),
            bg_tertiary: crate::hex_hsla!("#343F44"),
            bg_elevated: crate::hex_hsla!("#272E33"),
            bg_hover: crate::hex_hsla!("#3D484D"),
            bg_selected: crate::hex_hsla!("#475258"),
            bg_bar: crate::hex_hsla!("#2D353B"),
            bg_tab: crate::hex_hsla!("#2D353B"),
            bg_tab_hover: crate::hex_hsla!("#232A2E"),
            bg_tab_active: crate::hex_hsla!("#232A2E"),
            bg_code: crate::hex_hsla!("#1E2529"),
            bg_code_inline: crate::hex_hsla!("#3D484D"),
            // Border
            border_primary: crate::hex_hsla!("#85928933"),
            border_secondary: crate::hex_hsla!("#85928933"),
            border_tertiary: crate::hex_hsla!("#475258"),
            border_decorative: with_alpha(crate::hex_hsla!("#859289"), 0.5),
            border_code_block: with_alpha(crate::hex_hsla!("#323C41"), 1.0),
            window_border: crate::hex_hsla!("#85928933"),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#A7C080"),
            selection: with_alpha(crate::hex_hsla!("#543A48"), 0.7),
            line_highlight: with_alpha(crate::hex_hsla!("#3D484D"), 0.55),
            heading: crate::hex_hsla!("#E67E80"),
            bold: crate::hex_hsla!("#DBBC7F"),
            italic: crate::hex_hsla!("#D699B6"),
            strikethrough: crate::hex_hsla!("#859289"),
            inline_code: crate::hex_hsla!("#A7C080"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#D3C6AA"), 0.12),
            highlight_bg: with_alpha(crate::hex_hsla!("#DBBC7F"), 0.22),
            link: crate::hex_hsla!("#7FBBB3"),
            syntax_marker: with_alpha(crate::hex_hsla!("#859289"), 1.0),
            list_marker: crate::hex_hsla!("#E67E80"),
            checkbox_checked: crate::hex_hsla!("#A7C080"),
            gutter_heading: with_alpha(crate::hex_hsla!("#D3C6AA"), 0.2),
            git_status_added: crate::hex_hsla!("#A7C080"),
            git_status_modified: crate::hex_hsla!("#DBBC7F"),
            diff_added_bg: crate::hex_hsla!("#1b5e42"),
            diff_removed_bg: crate::hex_hsla!("#7a2738"),
            code_palette: CodeSyntaxPalette::EVERFOREST_DARK,
        }
    }

    pub fn monokai_pro() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#FCFCFA"),
            text_secondary: crate::hex_hsla!("#939293"),
            text_bright: crate::hex_hsla!("#FFFFFF"),
            text_accent: crate::hex_hsla!("#78DCE8"),
            text_danger: crate::hex_hsla!("#FF6188"),
            text_on_accent: crate::hex_hsla!("#2D2A2E"),
            text_warning: crate::hex_hsla!("#D6A100"),
            text_warning_hover: crate::hex_hsla!("#FFD24D"),
            // Background
            bg_primary: crate::hex_hsla!("#221F22"),
            bg_secondary: crate::hex_hsla!("#2D2A2E"),
            bg_tertiary: crate::hex_hsla!("#353236"),
            bg_elevated: crate::hex_hsla!("#262326"),
            bg_hover: crate::hex_hsla!("#3B383C"),
            bg_selected: crate::hex_hsla!("#423F43"),
            bg_bar: crate::hex_hsla!("#2D2A2E"),
            bg_tab: crate::hex_hsla!("#2D2A2E"),
            bg_tab_hover: crate::hex_hsla!("#221F22"),
            bg_tab_active: crate::hex_hsla!("#221F22"),
            bg_code: crate::hex_hsla!("#1D1A1D"),
            bg_code_inline: crate::hex_hsla!("#383539"),
            // Border
            border_primary: crate::hex_hsla!("#72707233"),
            border_secondary: crate::hex_hsla!("#72707233"),
            border_tertiary: crate::hex_hsla!("#403E41"),
            border_decorative: with_alpha(crate::hex_hsla!("#727072"), 0.5),
            border_code_block: with_alpha(crate::hex_hsla!("#322F33"), 1.0),
            window_border: crate::hex_hsla!("#72707233"),
            // Editor/Markdown
            cursor: crate::hex_hsla!("#FC9867"),
            selection: with_alpha(crate::hex_hsla!("#5B595C"), 0.6),
            line_highlight: with_alpha(crate::hex_hsla!("#403E41"), 0.55),
            heading: crate::hex_hsla!("#FF6188"),
            bold: crate::hex_hsla!("#FC9867"),
            italic: crate::hex_hsla!("#AB9DF2"),
            strikethrough: crate::hex_hsla!("#727072"),
            inline_code: crate::hex_hsla!("#A9DC76"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#D8DCE8"), 0.12),
            highlight_bg: with_alpha(crate::hex_hsla!("#FFD866"), 0.22),
            link: crate::hex_hsla!("#78DCE8"),
            syntax_marker: with_alpha(crate::hex_hsla!("#727072"), 1.0),
            list_marker: crate::hex_hsla!("#FF6188"),
            checkbox_checked: crate::hex_hsla!("#A9DC76"),
            gutter_heading: with_alpha(crate::hex_hsla!("#FCFCFA"), 0.2),
            git_status_added: crate::hex_hsla!("#A9DC76"),
            git_status_modified: crate::hex_hsla!("#FFD866"),
            diff_added_bg: crate::hex_hsla!("#1b5e42"),
            diff_removed_bg: crate::hex_hsla!("#7a2738"),
            code_palette: CodeSyntaxPalette::MONOKAI_PRO,
        }
    }

    pub fn ayu_light() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#5C6166"),
            text_secondary: crate::hex_hsla!("#8A9199"),
            text_bright: crate::hex_hsla!("#2F3439"),
            text_accent: crate::hex_hsla!("#399EE6"),
            text_danger: crate::hex_hsla!("#F07171"),
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#8A6400"),
            text_warning_hover: crate::hex_hsla!("#A87900"),
            // Background
            bg_primary: crate::hex_hsla!("#FCFCFC"),
            bg_secondary: crate::hex_hsla!("#F3F4F5"),
            bg_tertiary: crate::hex_hsla!("#F8F9FA"),
            bg_elevated: crate::hex_hsla!("#F6F7F8"),
            bg_hover: crate::hex_hsla!("#ECEDEE"),
            bg_selected: crate::hex_hsla!("#E4E6E8"),
            bg_bar: crate::hex_hsla!("#F3F4F5"),
            bg_tab: crate::hex_hsla!("#F3F4F5"),
            bg_tab_hover: crate::hex_hsla!("#FCFCFC"),
            bg_tab_active: crate::hex_hsla!("#FCFCFC"),
            bg_code: crate::hex_hsla!("#F8F9FA"),
            bg_code_inline: crate::hex_hsla!("#EAECEE"),
            // Border
            border_primary: crate::hex_hsla!("#D8DBDF99"),
            border_secondary: crate::hex_hsla!("#D8DBDF66"),
            border_tertiary: crate::hex_hsla!("#B7BCC2"),
            border_decorative: with_alpha(crate::hex_hsla!("#8A9199"), 0.4),
            border_code_block: with_alpha(crate::hex_hsla!("#E0E3E6"), 0.9),
            window_border: LIGHT_WINDOW_BORDER,
            // Editor/Markdown
            cursor: crate::hex_hsla!("#FFAA33"),
            selection: with_alpha(crate::hex_hsla!("#399EE6"), 0.15),
            line_highlight: with_alpha(crate::hex_hsla!("#E4E6E8"), 0.6),
            heading: crate::hex_hsla!("#FA8D3E"),
            bold: crate::hex_hsla!("#F2AE49"),
            italic: crate::hex_hsla!("#A37ACC"),
            strikethrough: crate::hex_hsla!("#8A9199"),
            inline_code: crate::hex_hsla!("#86B300"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#B7BCC2"), 0.3),
            highlight_bg: with_alpha(crate::hex_hsla!("#FFAA33"), 0.25),
            link: crate::hex_hsla!("#399EE6"),
            syntax_marker: with_alpha(crate::hex_hsla!("#8A9199"), 1.0),
            list_marker: crate::hex_hsla!("#FA8D3E"),
            checkbox_checked: crate::hex_hsla!("#86B300"),
            gutter_heading: with_alpha(crate::hex_hsla!("#5C6166"), 0.25),
            git_status_added: crate::hex_hsla!("#6CBF43"),
            git_status_modified: crate::hex_hsla!("#F2AE49"),
            diff_added_bg: crate::hex_hsla!("#1d6f46"),
            diff_removed_bg: crate::hex_hsla!("#a33b3b"),
            code_palette: CodeSyntaxPalette::AYU_LIGHT,
        }
    }

    pub fn xcode_light() -> Self {
        Self {
            // Text
            text_primary: crate::hex_hsla!("#262626"),
            text_secondary: crate::hex_hsla!("#6C7986"),
            text_bright: crate::hex_hsla!("#000000"),
            text_accent: crate::hex_hsla!("#0F68A0"),
            text_danger: crate::hex_hsla!("#C41A16"),
            text_on_accent: crate::hex_hsla!("#FFFFFF"),
            text_warning: crate::hex_hsla!("#8A6400"),
            text_warning_hover: crate::hex_hsla!("#A87900"),
            // Background
            bg_primary: crate::hex_hsla!("#FFFFFF"),
            bg_secondary: crate::hex_hsla!("#F5F5F5"),
            bg_tertiary: crate::hex_hsla!("#FAFAFA"),
            bg_elevated: crate::hex_hsla!("#F7F7F7"),
            bg_hover: crate::hex_hsla!("#EEEEEE"),
            bg_selected: crate::hex_hsla!("#E4E4E4"),
            bg_bar: crate::hex_hsla!("#F5F5F5"),
            bg_tab: crate::hex_hsla!("#F5F5F5"),
            bg_tab_hover: crate::hex_hsla!("#FFFFFF"),
            bg_tab_active: crate::hex_hsla!("#FFFFFF"),
            bg_code: crate::hex_hsla!("#FAFAFA"),
            bg_code_inline: crate::hex_hsla!("#ECECEC"),
            // Border
            border_primary: crate::hex_hsla!("#DDDDDD99"),
            border_secondary: crate::hex_hsla!("#DDDDDD66"),
            border_tertiary: crate::hex_hsla!("#BBBBBB"),
            border_decorative: with_alpha(crate::hex_hsla!("#8E8E93"), 0.4),
            border_code_block: with_alpha(crate::hex_hsla!("#E5E5E5"), 0.9),
            window_border: LIGHT_WINDOW_BORDER,
            // Editor/Markdown
            cursor: crate::hex_hsla!("#007AFF"),
            selection: with_alpha(crate::hex_hsla!("#007AFF"), 0.15),
            line_highlight: with_alpha(crate::hex_hsla!("#E8F0FE"), 0.6),
            heading: crate::hex_hsla!("#9B2393"),
            bold: crate::hex_hsla!("#643820"),
            italic: crate::hex_hsla!("#3900A0"),
            strikethrough: crate::hex_hsla!("#8E8E93"),
            inline_code: crate::hex_hsla!("#C41A16"),
            inline_code_bg: with_alpha(crate::hex_hsla!("#BBBBBB"), 0.25),
            highlight_bg: with_alpha(crate::hex_hsla!("#FFCC00"), 0.3),
            link: crate::hex_hsla!("#0F68A0"),
            syntax_marker: with_alpha(crate::hex_hsla!("#8E8E93"), 1.0),
            list_marker: crate::hex_hsla!("#9B2393"),
            checkbox_checked: crate::hex_hsla!("#28A745"),
            gutter_heading: with_alpha(crate::hex_hsla!("#262626"), 0.25),
            git_status_added: crate::hex_hsla!("#28A745"),
            git_status_modified: crate::hex_hsla!("#B58900"),
            diff_added_bg: crate::hex_hsla!("#1d6f46"),
            diff_removed_bg: crate::hex_hsla!("#a33b3b"),
            code_palette: CodeSyntaxPalette::XCODE_LIGHT,
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
    use super::{
        DARK_EDITOR_LINK, EditorThemeMode, Theme, ThemeColorField, ThemeKind,
        apply_editor_theme_mode, with_alpha,
    };

    #[test]
    fn interface_semantic_colors_round_trip_for_all_builtin_themes() {
        let fields = [
            ThemeColorField::TextOnAccent,
            ThemeColorField::TextWarning,
            ThemeColorField::TextWarningHover,
            ThemeColorField::BgTertiary,
        ];
        let themes = [
            Theme::one_dark(),
            Theme::one_light(),
            Theme::milky(),
            Theme::ayu_dark(),
        ];

        for mut theme in themes {
            for field in fields {
                let original = theme.color(field);
                theme.set_color(field, theme.text_accent);
                assert_eq!(theme.color(field), theme.text_accent);
                theme.set_color(field, original);
                assert_eq!(theme.color(field), original);
            }
        }
    }

    #[test]
    fn monochrome_mode_preserves_interface_semantic_colors() {
        let base = Theme::one_dark();
        let themed = apply_editor_theme_mode(&base, EditorThemeMode::Monochrome, true);

        assert_eq!(themed.text_on_accent, base.text_on_accent);
        assert_eq!(themed.text_warning, base.text_warning);
        assert_eq!(themed.text_warning_hover, base.text_warning_hover);
        assert_eq!(themed.bg_tertiary, base.bg_tertiary);
    }

    #[test]
    fn monochrome_mode_keeps_code_block_colors() {
        let base = Theme::one_dark();
        let themed = apply_editor_theme_mode(&base, EditorThemeMode::Monochrome, true);

        assert_eq!(themed.bg_code, base.bg_code);
        assert_eq!(themed.border_code_block, base.border_code_block);
        assert_eq!(themed.heading, with_alpha(base.text_bright, 0.88));
        assert_eq!(themed.inline_code, with_alpha(base.text_bright, 0.88));
        assert_eq!(themed.link, DARK_EDITOR_LINK);
    }

    #[test]
    fn dark_editor_themes_share_link_color_in_all_modes() {
        for kind in ThemeKind::dark_themes() {
            let base = kind.build();
            for mode in EditorThemeMode::all() {
                let themed = apply_editor_theme_mode(&base, *mode, true);
                assert_eq!(themed.link, DARK_EDITOR_LINK, "{}", kind.label());
            }
        }
    }

    #[test]
    fn monochrome_mode_uses_grayscale_markers_in_light_theme() {
        let base = Theme::one_light();
        let themed = apply_editor_theme_mode(&base, EditorThemeMode::Monochrome, false);

        assert_eq!(themed.heading, base.text_primary);
        assert_eq!(themed.inline_code, base.text_primary);
        assert_eq!(themed.link, crate::hex_hsla!("#0057CDFF"));
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
