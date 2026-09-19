use gpui::Hsla;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeKind {
    PorcelainMist,
    LinenPaper,
    StudioDaylight,
    RoseDawn,
    VanillaLatte,
    SageGarden,
    NordicSnow,
    SolarizedLight,
    SunlitCanvas,
    PaperWhite,
    PearlMorning,
    WillowMist,
    CoastalFog,
    LavenderHaze,
    ApricotBloom,
    SilverMorning,
    OneDark,
    MilkyDark,
    DraculaNight,
    NordicNight,
    TokyoNight,
    CatppuccinMocha,
    GithubDark,
    SolarizedDark,
    GruvboxDark,
    RosePine,
    EverforestDark,
    MonokaiPro,
    AyuMirage,
    MoonlightHarbor,
    CobaltVelvet,
    ObsidianEmber,
}

impl ThemeKind {
    pub fn dark_themes() -> &'static [ThemeKind] {
        &[
            Self::OneDark,
            Self::MilkyDark,
            Self::DraculaNight,
            Self::NordicNight,
            Self::TokyoNight,
            Self::CatppuccinMocha,
            Self::GithubDark,
            Self::SolarizedDark,
            Self::GruvboxDark,
            Self::RosePine,
            Self::EverforestDark,
            Self::MonokaiPro,
            Self::AyuMirage,
            Self::MoonlightHarbor,
            Self::CobaltVelvet,
            Self::ObsidianEmber,
        ]
    }

    pub fn light_themes() -> &'static [ThemeKind] {
        &[
            Self::PorcelainMist,
            Self::LinenPaper,
            Self::StudioDaylight,
            Self::RoseDawn,
            Self::VanillaLatte,
            Self::SageGarden,
            Self::NordicSnow,
            Self::SolarizedLight,
            Self::SunlitCanvas,
            Self::PaperWhite,
            Self::PearlMorning,
            Self::WillowMist,
            Self::CoastalFog,
            Self::LavenderHaze,
            Self::ApricotBloom,
            Self::SilverMorning,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::PorcelainMist => "Porcelain Mist",
            Self::LinenPaper => "Linen Paper",
            Self::StudioDaylight => "Studio Daylight",
            Self::RoseDawn => "Rose Dawn",
            Self::VanillaLatte => "Vanilla Latte",
            Self::SageGarden => "Sage Garden",
            Self::NordicSnow => "Nordic Snow",
            Self::SolarizedLight => "Solarized Light",
            Self::SunlitCanvas => "Sunlit Canvas",
            Self::PaperWhite => "Paper White",
            Self::PearlMorning => "Pearl Morning",
            Self::WillowMist => "Willow Mist",
            Self::CoastalFog => "Coastal Fog",
            Self::LavenderHaze => "Lavender Haze",
            Self::ApricotBloom => "Apricot Bloom",
            Self::SilverMorning => "Silver Morning",
            Self::OneDark => "One Dark",
            Self::MilkyDark => "Milky Dark",
            Self::DraculaNight => "Dracula Night",
            Self::NordicNight => "Nordic Night",
            Self::TokyoNight => "Tokyo Night",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::GithubDark => "GitHub Dark",
            Self::SolarizedDark => "Solarized Dark",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::RosePine => "Rose Pine",
            Self::EverforestDark => "Everforest Dark",
            Self::MonokaiPro => "Monokai Pro",
            Self::AyuMirage => "Ayu Mirage",
            Self::MoonlightHarbor => "Moonlight Harbor",
            Self::CobaltVelvet => "Cobalt Velvet",
            Self::ObsidianEmber => "Obsidian Ember",
        }
    }

    pub fn is_dark(self) -> bool {
        Self::dark_themes().contains(&self)
    }

    pub fn build(self) -> Theme {
        let mut theme = self.palette();
        // One caret colour across every theme so it never blends into a
        // palette's own foreground colours.
        theme.cursor = EDITOR_CARET;
        theme
    }

    fn palette(self) -> Theme {
        match self {
            Self::PorcelainMist => Theme::curated(PORCELAIN_MIST),
            Self::LinenPaper => Theme::curated(LINEN_PAPER),
            Self::StudioDaylight => Theme::curated(STUDIO_DAYLIGHT),
            Self::RoseDawn => Theme::curated(ROSE_DAWN),
            Self::VanillaLatte => Theme::curated(VANILLA_LATTE),
            Self::SageGarden => Theme::curated(SAGE_GARDEN),
            Self::NordicSnow => Theme::curated(NORDIC_SNOW),
            Self::SolarizedLight => Theme::curated(SOLARIZED_LIGHT_THEME),
            Self::SunlitCanvas => Theme::curated(SUNLIT_CANVAS),
            Self::PaperWhite => Theme::curated(PAPER_WHITE),
            Self::PearlMorning => Theme::curated(PEARL_MORNING),
            Self::WillowMist => Theme::curated(WILLOW_MIST),
            Self::CoastalFog => Theme::curated(COASTAL_FOG),
            Self::LavenderHaze => Theme::curated(LAVENDER_HAZE),
            Self::ApricotBloom => Theme::curated(APRICOT_BLOOM),
            Self::SilverMorning => Theme::curated(SILVER_MORNING),
            Self::OneDark => Theme::curated(ONE_DARK_THEME),
            Self::MilkyDark => Theme::curated(MILKY_NIGHT),
            Self::DraculaNight => Theme::curated(DRACULA_NIGHT),
            Self::NordicNight => Theme::curated(NORDIC_NIGHT),
            Self::TokyoNight => Theme::curated(TOKYO_NIGHT_THEME),
            Self::CatppuccinMocha => Theme::curated(CATPPUCCIN_MOCHA_THEME),
            Self::GithubDark => Theme::curated(GITHUB_DARK_THEME),
            Self::SolarizedDark => Theme::curated(SOLARIZED_DARK_THEME),
            Self::GruvboxDark => Theme::curated(GRUVBOX_DARK_THEME),
            Self::RosePine => Theme::curated(ROSE_PINE_THEME),
            Self::EverforestDark => Theme::curated(EVERFOREST_DARK_THEME),
            Self::MonokaiPro => Theme::curated(MONOKAI_PRO_THEME),
            Self::AyuMirage => Theme::curated(AYU_MIRAGE),
            Self::MoonlightHarbor => Theme::curated(MOONLIGHT_HARBOR),
            Self::CobaltVelvet => Theme::curated(COBALT_VELVET),
            Self::ObsidianEmber => Theme::curated(OBSIDIAN_EMBER),
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
/// Theme-independent caret colour.
pub const EDITOR_CARET: Hsla = hex_to_hsla("#2F7CF6");

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

#[allow(dead_code)]
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

    pub const MILKY_NIGHT: Self = Self {
        keyword: 0xC792D4,
        operator: 0x89DDFF,
        string: 0xA7C080,
        function: 0x82AAFF,
        type_name: 0xEBCB8B,
        number: 0xF0A47A,
        variable: 0xE58D91,
        comment: 0x7E838D,
        foreground: 0xD8D6D1,
    };

    pub const AYU_MIRAGE: Self = Self {
        keyword: 0xFF8F40,
        operator: 0xF29E74,
        string: 0xAAD94C,
        function: 0xFFB454,
        type_name: 0x59C2FF,
        number: 0xD2A6FF,
        variable: 0x95E6CB,
        comment: 0x6C7A89,
        foreground: 0xCCCAC2,
    };

    pub const MOONLIGHT_HARBOR: Self = Self {
        keyword: 0xC099FF,
        operator: 0x7FDBCA,
        string: 0xA9DC76,
        function: 0x82B1FF,
        type_name: 0x66D9EF,
        number: 0xF5A97F,
        variable: 0xE6B0D8,
        comment: 0x65708A,
        foreground: 0xD7DEEA,
    };

    pub const COBALT_VELVET: Self = Self {
        keyword: 0xFF78B4,
        operator: 0x67D4E8,
        string: 0xB5E88C,
        function: 0x7FA7FF,
        type_name: 0xE9C46A,
        number: 0xD6A2E8,
        variable: 0xF29B8F,
        comment: 0x7180A6,
        foreground: 0xDCE4FF,
    };

    pub const OBSIDIAN_EMBER: Self = Self {
        keyword: 0xF0785A,
        operator: 0xE6A15C,
        string: 0xB4C96B,
        function: 0xE8C170,
        type_name: 0x74B6A0,
        number: 0xD98AA7,
        variable: 0xE89A72,
        comment: 0x81756D,
        foreground: 0xDDD3C8,
    };

    pub const WILLOW_LIGHT: Self = Self {
        keyword: 0x556B2F,
        operator: 0x52766C,
        string: 0x6B7F38,
        function: 0x376E72,
        type_name: 0x8A632A,
        number: 0x985F45,
        variable: 0x744F75,
        comment: 0x879284,
        foreground: 0x34413A,
    };

    pub const COASTAL_LIGHT: Self = Self {
        keyword: 0x2D5F8B,
        operator: 0x287D83,
        string: 0x4E7A64,
        function: 0x356FA8,
        type_name: 0x74639A,
        number: 0x9A6543,
        variable: 0x8C5267,
        comment: 0x7C8993,
        foreground: 0x334550,
    };

    pub const LAVENDER_LIGHT: Self = Self {
        keyword: 0x7252A3,
        operator: 0x526AA3,
        string: 0x56805B,
        function: 0x4169A1,
        type_name: 0x9A623B,
        number: 0x9A5278,
        variable: 0xA04B58,
        comment: 0x898494,
        foreground: 0x433E50,
    };

    pub const APRICOT_LIGHT: Self = Self {
        keyword: 0xB04C38,
        operator: 0x9B642C,
        string: 0x627A36,
        function: 0x9A5B33,
        type_name: 0x397A78,
        number: 0x8E5590,
        variable: 0xA64D5C,
        comment: 0x8C8178,
        foreground: 0x4A3B34,
    };

    pub const SILVER_LIGHT: Self = Self {
        keyword: 0x4C5E88,
        operator: 0x39747B,
        string: 0x587447,
        function: 0x3C6F9C,
        type_name: 0x7D5F87,
        number: 0x8B6046,
        variable: 0x865263,
        comment: 0x7F8791,
        foreground: 0x343A43,
    };
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MarkdownSyntaxPalette {
    heading: u32,
    bold: u32,
    italic: u32,
    strikethrough: u32,
    inline_code: u32,
    link: u32,
    list_marker: u32,
}

const fn markdown_syntax(
    heading: u32,
    bold: u32,
    italic: u32,
    strikethrough: u32,
    inline_code: u32,
    link: u32,
    list_marker: u32,
) -> MarkdownSyntaxPalette {
    MarkdownSyntaxPalette {
        heading,
        bold,
        italic,
        strikethrough,
        inline_code,
        link,
        list_marker,
    }
}

fn markdown_syntax_for(code: CodeSyntaxPalette) -> MarkdownSyntaxPalette {
    if code == CodeSyntaxPalette::GITHUB_LIGHT {
        markdown_syntax(
            0xCF222E, 0x953800, 0x8250DF, 0x6E7781, 0x0A3069, 0x0969DA, 0xCF222E,
        )
    } else if code == CodeSyntaxPalette::GRUVBOX_LIGHT {
        markdown_syntax(
            0xAF3A03, 0xB57614, 0x8F3F71, 0x928374, 0x79740E, 0x076678, 0x9D0006,
        )
    } else if code == CodeSyntaxPalette::XCODE_LIGHT {
        markdown_syntax(
            0x9B2393, 0x643820, 0x3900A0, 0x5D6C79, 0xC41A16, 0x326D74, 0x947100,
        )
    } else if code == CodeSyntaxPalette::ROSE_PINE_DAWN {
        markdown_syntax(
            0xB4637A, 0x9D640E, 0x907AA9, 0x9893A5, 0x286983, 0x56949F, 0xB4637A,
        )
    } else if code == CodeSyntaxPalette::CATPPUCCIN_LATTE {
        markdown_syntax(
            0x8839EF, 0xB94700, 0xA83A91, 0x8C8FA1, 0x367F28, 0x1E66F5, 0xA86A00,
        )
    } else if code == CodeSyntaxPalette::EVERFOREST_LIGHT {
        markdown_syntax(
            0xC84C49, 0xA66B00, 0xB44B91, 0x939F91, 0x708600, 0x287FA9, 0xD06422,
        )
    } else if code == CodeSyntaxPalette::NORD_LIGHT {
        markdown_syntax(
            0x5E81AC, 0x965C44, 0x9A6590, 0x8892A5, 0x6C8A55, 0x3E7B91, 0x497D7B,
        )
    } else if code == CodeSyntaxPalette::SOLARIZED_LIGHT {
        markdown_syntax(
            0xCB4B16, 0xB58900, 0x6C71C4, 0x7B8D8D, 0x1B756F, 0x268BD2, 0x697A00,
        )
    } else if code == CodeSyntaxPalette::AYU_LIGHT {
        markdown_syntax(
            0xC35F00, 0x8B5FB3, 0x287E9A, 0x787B80, 0x668A00, 0x247BB5, 0xC66C48,
        )
    } else if code == CodeSyntaxPalette::TOKYO_NIGHT_LIGHT {
        markdown_syntax(
            0x34548A, 0x965027, 0x5A4A78, 0x9699A3, 0x485E30, 0x0F4B6E, 0x33635C,
        )
    } else if code == CodeSyntaxPalette::ONE_LIGHT {
        markdown_syntax(
            0xA626A4, 0x986801, 0x4078F2, 0x7C7D83, 0x50A14F, 0x2F6EDB, 0x0184BC,
        )
    } else if code == CodeSyntaxPalette::WILLOW_LIGHT {
        markdown_syntax(
            0x556B2F, 0x8A632A, 0x744F75, 0x879284, 0x6B7F38, 0x376E72, 0x52766C,
        )
    } else if code == CodeSyntaxPalette::COASTAL_LIGHT {
        markdown_syntax(
            0x2D5F8B, 0x9A6543, 0x74639A, 0x7C8993, 0x4E7A64, 0x356FA8, 0x287D83,
        )
    } else if code == CodeSyntaxPalette::LAVENDER_LIGHT {
        markdown_syntax(
            0x7252A3, 0x9A623B, 0xA04B58, 0x898494, 0x56805B, 0x4169A1, 0x526AA3,
        )
    } else if code == CodeSyntaxPalette::APRICOT_LIGHT {
        markdown_syntax(
            0xB04C38, 0x9B642C, 0x8E5590, 0x8C8178, 0x627A36, 0x397A78, 0xA64D5C,
        )
    } else if code == CodeSyntaxPalette::SILVER_LIGHT {
        markdown_syntax(
            0x4C5E88, 0x8B6046, 0x7D5F87, 0x7F8791, 0x587447, 0x3C6F9C, 0x39747B,
        )
    } else if code == CodeSyntaxPalette::ONE_DARK {
        markdown_syntax(
            0xE06C75, 0xD19A66, 0xC678DD, 0x636D83, 0x98C379, 0x61AFEF, 0xE06C75,
        )
    } else if code == CodeSyntaxPalette::MILKY_NIGHT {
        markdown_syntax(
            0xE58D91, 0xEBCB8B, 0xC792D4, 0x7E838D, 0xA7C080, 0x82AAFF, 0x89DDFF,
        )
    } else if code == CodeSyntaxPalette::DRACULA {
        markdown_syntax(
            0xFF79C6, 0xFFB86C, 0xBD93F9, 0x6272A4, 0x50FA7B, 0x8BE9FD, 0xFF79C6,
        )
    } else if code == CodeSyntaxPalette::NORD {
        markdown_syntax(
            0x88C0D0, 0xEBCB8B, 0xB48EAD, 0x616E88, 0xA3BE8C, 0x81A1C1, 0x8FBCBB,
        )
    } else if code == CodeSyntaxPalette::TOKYO_NIGHT {
        markdown_syntax(
            0x7AA2F7, 0xFF9E64, 0xBB9AF7, 0x565F89, 0x73DACA, 0x7DCFFF, 0x2AC3DE,
        )
    } else if code == CodeSyntaxPalette::CATPPUCCIN_MOCHA {
        markdown_syntax(
            0xCBA6F7, 0xFAB387, 0xF5C2E7, 0x6C7086, 0xA6E3A1, 0x89B4FA, 0xF9E2AF,
        )
    } else if code == CodeSyntaxPalette::GITHUB_DARK {
        markdown_syntax(
            0x79C0FF, 0xFFA657, 0xD2A8FF, 0x8B949E, 0xA5D6FF, 0x58A6FF, 0xFF7B72,
        )
    } else if code == CodeSyntaxPalette::SOLARIZED_DARK {
        markdown_syntax(
            0xCB4B16, 0xB58900, 0x6C71C4, 0x586E75, 0x2AA198, 0x268BD2, 0x859900,
        )
    } else if code == CodeSyntaxPalette::GRUVBOX_DARK {
        markdown_syntax(
            0xFE8019, 0xFABD2F, 0xD3869B, 0x928374, 0xB8BB26, 0x83A598, 0xFB4934,
        )
    } else if code == CodeSyntaxPalette::ROSE_PINE {
        markdown_syntax(
            0xEB6F92, 0xF6C177, 0xC4A7E7, 0x6E6A86, 0x9CCFD8, 0xC4A7E7, 0xEBBCBA,
        )
    } else if code == CodeSyntaxPalette::EVERFOREST_DARK {
        markdown_syntax(
            0xE67E80, 0xDBBC7F, 0xD699B6, 0x859289, 0xA7C080, 0x7FBBB3, 0xE69875,
        )
    } else if code == CodeSyntaxPalette::MONOKAI_PRO {
        markdown_syntax(
            0xFF6188, 0xFC9867, 0xAB9DF2, 0x727072, 0xA9DC76, 0x78DCE8, 0xFFD866,
        )
    } else if code == CodeSyntaxPalette::AYU_MIRAGE {
        markdown_syntax(
            0xFF8F40, 0xFFB454, 0xD2A6FF, 0x6C7A89, 0xAAD94C, 0x59C2FF, 0xF29E74,
        )
    } else if code == CodeSyntaxPalette::MOONLIGHT_HARBOR {
        markdown_syntax(
            0xC099FF, 0xF5A97F, 0xE6B0D8, 0x65708A, 0xA9DC76, 0x82B1FF, 0x7FDBCA,
        )
    } else if code == CodeSyntaxPalette::COBALT_VELVET {
        markdown_syntax(
            0xFF78B4, 0xE9C46A, 0xD6A2E8, 0x7180A6, 0xB5E88C, 0x7FA7FF, 0x67D4E8,
        )
    } else if code == CodeSyntaxPalette::OBSIDIAN_EMBER {
        markdown_syntax(
            0xF0785A, 0xE8C170, 0xD98AA7, 0x81756D, 0xB4C96B, 0x74B6A0, 0xE6A15C,
        )
    } else {
        markdown_syntax(
            code.variable,
            code.number,
            code.keyword,
            code.comment,
            code.string,
            code.function,
            code.operator,
        )
    }
}

fn packed_rgb_to_hsla(rgb: u32) -> Hsla {
    rgb_to_hsla(
        ((rgb >> 16) & 0xFF) as f32 / 255.0,
        ((rgb >> 8) & 0xFF) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
        1.0,
    )
}

#[derive(Clone, Copy)]
struct CuratedThemeSpec {
    dark: bool,
    background: &'static str,
    surface: &'static str,
    elevated: &'static str,
    hover: &'static str,
    selected: &'static str,
    border: &'static str,
    text: &'static str,
    muted: &'static str,
    bright: &'static str,
    accent: &'static str,
    secondary: &'static str,
    danger: &'static str,
    warning: &'static str,
    success: &'static str,
    code_background: &'static str,
    inline_background: &'static str,
    code_palette: CodeSyntaxPalette,
}

const PORCELAIN_MIST: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#FCFCFD",
    surface: "#F5F6F8",
    elevated: "#FFFFFF",
    hover: "#ECEFF3",
    selected: "#E4ECF7",
    border: "#D9DEE7",
    text: "#24272D",
    muted: "#68707C",
    bright: "#111318",
    accent: "#2468B2",
    secondary: "#7A4DA3",
    danger: "#C43D4B",
    warning: "#A56800",
    success: "#2B7A52",
    code_background: "#F4F6F8",
    inline_background: "#EDEFF2",
    code_palette: CodeSyntaxPalette::GITHUB_LIGHT,
};

const LINEN_PAPER: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#FFFCF4",
    surface: "#F4F0E5",
    elevated: "#FFFDF8",
    hover: "#EAE4D5",
    selected: "#E7E1D3",
    border: "#D8D1C1",
    text: "#2D2B27",
    muted: "#756F65",
    bright: "#171613",
    accent: "#1F6F67",
    secondary: "#9A5A38",
    danger: "#B43D35",
    warning: "#9B6B12",
    success: "#557A3E",
    code_background: "#F5F1E7",
    inline_background: "#ECE7DB",
    code_palette: CodeSyntaxPalette::GRUVBOX_LIGHT,
};

const STUDIO_DAYLIGHT: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#F8FAFC",
    surface: "#EEF2F6",
    elevated: "#FFFFFF",
    hover: "#E4EAF1",
    selected: "#DCE8F8",
    border: "#D2D9E3",
    text: "#20242B",
    muted: "#626B78",
    bright: "#0D1117",
    accent: "#1769AA",
    secondary: "#6D4BA3",
    danger: "#C2303F",
    warning: "#986500",
    success: "#287849",
    code_background: "#F1F4F7",
    inline_background: "#E8EDF3",
    code_palette: CodeSyntaxPalette::XCODE_LIGHT,
};

const ROSE_DAWN: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#FFF9F2",
    surface: "#F6EFE8",
    elevated: "#FFFCF8",
    hover: "#EDE4DF",
    selected: "#E9DFDA",
    border: "#D9CEC9",
    text: "#575279",
    muted: "#797593",
    bright: "#3C3858",
    accent: "#B4637A",
    secondary: "#286983",
    danger: "#B84B5D",
    warning: "#A86F16",
    success: "#39725E",
    code_background: "#FAF4ED",
    inline_background: "#F0E8E3",
    code_palette: CodeSyntaxPalette::ROSE_PINE_DAWN,
};

const VANILLA_LATTE: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#F7F7FB",
    surface: "#ECEEF5",
    elevated: "#FCFCFF",
    hover: "#E1E4EE",
    selected: "#DCE3F3",
    border: "#CDD1DE",
    text: "#4C4F69",
    muted: "#6C6F85",
    bright: "#303247",
    accent: "#1E66F5",
    secondary: "#8839EF",
    danger: "#D20F39",
    warning: "#9A5C00",
    success: "#2F7D26",
    code_background: "#EFF1F5",
    inline_background: "#E6E9F0",
    code_palette: CodeSyntaxPalette::CATPPUCCIN_LATTE,
};

const SAGE_GARDEN: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#F7F8F2",
    surface: "#ECEFE5",
    elevated: "#FBFCF8",
    hover: "#E1E6D9",
    selected: "#DCE6D3",
    border: "#CDD4C4",
    text: "#46514A",
    muted: "#6C796F",
    bright: "#26312A",
    accent: "#3B7662",
    secondary: "#8A5D3B",
    danger: "#B24743",
    warning: "#916915",
    success: "#557A3C",
    code_background: "#EEF1E8",
    inline_background: "#E3E8DE",
    code_palette: CodeSyntaxPalette::EVERFOREST_LIGHT,
};

const NORDIC_SNOW: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#F8FAFD",
    surface: "#EDF1F7",
    elevated: "#FFFFFF",
    hover: "#E1E7EF",
    selected: "#D7E3F1",
    border: "#CCD5E1",
    text: "#2E3440",
    muted: "#667286",
    bright: "#18202B",
    accent: "#4A6F9B",
    secondary: "#8A5F8D",
    danger: "#B34855",
    warning: "#8D6818",
    success: "#55754B",
    code_background: "#EEF2F7",
    inline_background: "#E4E9F0",
    code_palette: CodeSyntaxPalette::NORD_LIGHT,
};

const SOLARIZED_LIGHT_THEME: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#FDF8E7",
    surface: "#F0EAD7",
    elevated: "#FFFBEE",
    hover: "#E5DECA",
    selected: "#E2DCCB",
    border: "#D2CBB7",
    text: "#3D5358",
    muted: "#687C7F",
    bright: "#20383D",
    accent: "#16739A",
    secondary: "#9A5D23",
    danger: "#C0443E",
    warning: "#956B00",
    success: "#647A12",
    code_background: "#F3EDDA",
    inline_background: "#E9E2CE",
    code_palette: CodeSyntaxPalette::SOLARIZED_LIGHT,
};

const SUNLIT_CANVAS: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#FBFCFD",
    surface: "#F1F3F5",
    elevated: "#FFFFFF",
    hover: "#E7EAED",
    selected: "#DEEAF4",
    border: "#D5DADF",
    text: "#44494F",
    muted: "#717981",
    bright: "#262A2E",
    accent: "#2778AF",
    secondary: "#A45B24",
    danger: "#C34249",
    warning: "#996700",
    success: "#4E7A32",
    code_background: "#F2F4F6",
    inline_background: "#E8EBEE",
    code_palette: CodeSyntaxPalette::AYU_LIGHT,
};

const PAPER_WHITE: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#FAFAF7",
    surface: "#F1F0EA",
    elevated: "#FEFEFC",
    hover: "#E7E6DF",
    selected: "#DCE9E2",
    border: "#D4D3CB",
    text: "#393A34",
    muted: "#6D7068",
    bright: "#20211D",
    accent: "#1C6B48",
    secondary: "#8B5E31",
    danger: "#AB4C4C",
    warning: "#8B6C14",
    success: "#397353",
    code_background: "#F1F0E9",
    inline_background: "#E7E5DB",
    code_palette: CodeSyntaxPalette::TOKYO_NIGHT_LIGHT,
};

const PEARL_MORNING: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#FBFCFF",
    surface: "#F0F3F9",
    elevated: "#FFFFFF",
    hover: "#E5EAF3",
    selected: "#DCE5F4",
    border: "#CFD7E5",
    text: "#383A42",
    muted: "#6F7582",
    bright: "#1E2027",
    accent: "#4B6FB5",
    secondary: "#8A5A96",
    danger: "#C44755",
    warning: "#946813",
    success: "#39754F",
    code_background: "#F2F5FA",
    inline_background: "#E7EBF3",
    code_palette: CodeSyntaxPalette::ONE_LIGHT,
};

const WILLOW_MIST: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#F8FAF5",
    surface: "#EDF1E8",
    elevated: "#FCFDFB",
    hover: "#E2E8DC",
    selected: "#D9E6D8",
    border: "#CAD4C7",
    text: "#34413A",
    muted: "#6B786F",
    bright: "#1E2922",
    accent: "#52766C",
    secondary: "#88684A",
    danger: "#B44B49",
    warning: "#8D6A1B",
    success: "#5C7B43",
    code_background: "#EEF2EA",
    inline_background: "#E3E9DF",
    code_palette: CodeSyntaxPalette::WILLOW_LIGHT,
};

const COASTAL_FOG: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#F6FAFB",
    surface: "#EAF1F3",
    elevated: "#FCFEFE",
    hover: "#DFE9EC",
    selected: "#D5E5EA",
    border: "#C7D5D9",
    text: "#334550",
    muted: "#687982",
    bright: "#1B303A",
    accent: "#287D83",
    secondary: "#76639A",
    danger: "#B84A55",
    warning: "#916716",
    success: "#4E7A64",
    code_background: "#EAF1F3",
    inline_background: "#DDE7EA",
    code_palette: CodeSyntaxPalette::COASTAL_LIGHT,
};

const LAVENDER_HAZE: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#FAF8FC",
    surface: "#F0EDF5",
    elevated: "#FEFCFF",
    hover: "#E7E2ED",
    selected: "#E1DDF0",
    border: "#D4CDDC",
    text: "#433E50",
    muted: "#756F80",
    bright: "#292432",
    accent: "#7252A3",
    secondary: "#A05F72",
    danger: "#BD4657",
    warning: "#956713",
    success: "#56805B",
    code_background: "#F1EDF5",
    inline_background: "#E7E1EC",
    code_palette: CodeSyntaxPalette::LAVENDER_LIGHT,
};

const APRICOT_BLOOM: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#FFF9F5",
    surface: "#F7EEE8",
    elevated: "#FFFCFA",
    hover: "#EDE2DB",
    selected: "#F0DDD4",
    border: "#DECEC4",
    text: "#4A3B34",
    muted: "#7C6D65",
    bright: "#2C211C",
    accent: "#B75C42",
    secondary: "#397A78",
    danger: "#C8444B",
    warning: "#986512",
    success: "#627A36",
    code_background: "#F8EFE9",
    inline_background: "#EEDFD7",
    code_palette: CodeSyntaxPalette::APRICOT_LIGHT,
};

const SILVER_MORNING: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#F7F9FB",
    surface: "#ECEFF3",
    elevated: "#FDFEFF",
    hover: "#E1E5EA",
    selected: "#D9E2EC",
    border: "#CBD2DA",
    text: "#343A43",
    muted: "#69727D",
    bright: "#1C222A",
    accent: "#4C6F96",
    secondary: "#7D5F87",
    danger: "#B94753",
    warning: "#8C6717",
    success: "#587447",
    code_background: "#EDF1F5",
    inline_background: "#E2E7EC",
    code_palette: CodeSyntaxPalette::SILVER_LIGHT,
};

const ONE_DARK_THEME: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#1E2025",
    surface: "#25282F",
    elevated: "#2B2F37",
    hover: "#333842",
    selected: "#39414D",
    border: "#3D434D",
    text: "#D7DAE0",
    muted: "#8D949F",
    bright: "#F2F4F7",
    accent: "#6EA8E6",
    secondary: "#C18AD6",
    danger: "#E06C75",
    warning: "#D6A85F",
    success: "#86B97B",
    code_background: "#191B20",
    inline_background: "#30343C",
    code_palette: CodeSyntaxPalette::ONE_DARK,
};

const TOKYO_NIGHT_THEME: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#171925",
    surface: "#1E2130",
    elevated: "#25293A",
    hover: "#2D3348",
    selected: "#333B54",
    border: "#343B50",
    text: "#C4C9E0",
    muted: "#7F879F",
    bright: "#EEF0FA",
    accent: "#7AA2F7",
    secondary: "#BB9AF7",
    danger: "#F7768E",
    warning: "#E0AF68",
    success: "#9ECE6A",
    code_background: "#131520",
    inline_background: "#292D40",
    code_palette: CodeSyntaxPalette::TOKYO_NIGHT,
};

const CATPPUCCIN_MOCHA_THEME: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#1E1E2E",
    surface: "#252536",
    elevated: "#2D2D40",
    hover: "#36364A",
    selected: "#3D3D55",
    border: "#45475A",
    text: "#CDD6F4",
    muted: "#9399B2",
    bright: "#F0F2FA",
    accent: "#89B4FA",
    secondary: "#CBA6F7",
    danger: "#F38BA8",
    warning: "#E5C890",
    success: "#A6E3A1",
    code_background: "#181825",
    inline_background: "#313244",
    code_palette: CodeSyntaxPalette::CATPPUCCIN_MOCHA,
};

const EVERFOREST_DARK_THEME: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#202622",
    surface: "#272E29",
    elevated: "#2E3731",
    hover: "#374139",
    selected: "#3D493F",
    border: "#465148",
    text: "#D3CDBB",
    muted: "#8F978B",
    bright: "#F0E9D8",
    accent: "#83C092",
    secondary: "#7FBBB3",
    danger: "#E67E80",
    warning: "#DBBC7F",
    success: "#A7C080",
    code_background: "#1A1F1C",
    inline_background: "#303831",
    code_palette: CodeSyntaxPalette::EVERFOREST_DARK,
};

const ROSE_PINE_THEME: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#1F1D2E",
    surface: "#262338",
    elevated: "#2E2A42",
    hover: "#38334D",
    selected: "#403A58",
    border: "#49445E",
    text: "#E0DEF4",
    muted: "#908CAA",
    bright: "#F4F1FF",
    accent: "#EBBCBA",
    secondary: "#9CCFD8",
    danger: "#EB6F92",
    warning: "#F6C177",
    success: "#75B8A3",
    code_background: "#191724",
    inline_background: "#312E45",
    code_palette: CodeSyntaxPalette::ROSE_PINE,
};

const GRUVBOX_DARK_THEME: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#272320",
    surface: "#302A26",
    elevated: "#39312C",
    hover: "#443A33",
    selected: "#4B4037",
    border: "#55483E",
    text: "#E8DDC9",
    muted: "#A09586",
    bright: "#FFF6E8",
    accent: "#E88B52",
    secondary: "#D3869B",
    danger: "#E66B62",
    warning: "#D8AE55",
    success: "#A4B86A",
    code_background: "#211E1B",
    inline_background: "#3A322C",
    code_palette: CodeSyntaxPalette::GRUVBOX_DARK,
};

const MILKY_NIGHT: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#202124",
    surface: "#282A2E",
    elevated: "#303238",
    hover: "#383B42",
    selected: "#404751",
    border: "#484C55",
    text: "#D8D6D1",
    muted: "#92918D",
    bright: "#F6F4EF",
    accent: "#82AAFF",
    secondary: "#C792D4",
    danger: "#E58D91",
    warning: "#EBCB8B",
    success: "#A7C080",
    code_background: "#1A1B1E",
    inline_background: "#303238",
    code_palette: CodeSyntaxPalette::MILKY_NIGHT,
};

const DRACULA_NIGHT: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#20212B",
    surface: "#282A36",
    elevated: "#303241",
    hover: "#393C4D",
    selected: "#414559",
    border: "#484C60",
    text: "#E6E6E1",
    muted: "#9093A6",
    bright: "#F8F8F2",
    accent: "#BD93F9",
    secondary: "#8BE9FD",
    danger: "#FF6E87",
    warning: "#F1CA76",
    success: "#50C878",
    code_background: "#191A22",
    inline_background: "#303241",
    code_palette: CodeSyntaxPalette::DRACULA,
};

const NORDIC_NIGHT: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#252A34",
    surface: "#2E3440",
    elevated: "#353C4A",
    hover: "#3B4453",
    selected: "#434E60",
    border: "#4C566A",
    text: "#D8DEE9",
    muted: "#8D98AA",
    bright: "#ECEFF4",
    accent: "#88C0D0",
    secondary: "#B48EAD",
    danger: "#BF616A",
    warning: "#D5B36A",
    success: "#A3BE8C",
    code_background: "#20242D",
    inline_background: "#343B48",
    code_palette: CodeSyntaxPalette::NORD,
};

const GITHUB_DARK_THEME: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#0D1117",
    surface: "#161B22",
    elevated: "#1C2128",
    hover: "#252C35",
    selected: "#2D3744",
    border: "#30363D",
    text: "#C9D1D9",
    muted: "#8B949E",
    bright: "#F0F6FC",
    accent: "#58A6FF",
    secondary: "#D2A8FF",
    danger: "#FF7B72",
    warning: "#D29922",
    success: "#56D364",
    code_background: "#090C10",
    inline_background: "#21262D",
    code_palette: CodeSyntaxPalette::GITHUB_DARK,
};

const SOLARIZED_DARK_THEME: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#002B36",
    surface: "#073642",
    elevated: "#0B414D",
    hover: "#124B57",
    selected: "#175563",
    border: "#23616C",
    text: "#93A1A1",
    muted: "#718486",
    bright: "#EEE8D5",
    accent: "#268BD2",
    secondary: "#B58900",
    danger: "#DC5A5F",
    warning: "#D3A52E",
    success: "#859900",
    code_background: "#00232C",
    inline_background: "#0B3B46",
    code_palette: CodeSyntaxPalette::SOLARIZED_DARK,
};

const MONOKAI_PRO_THEME: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#221F22",
    surface: "#2D2A2E",
    elevated: "#353237",
    hover: "#403C42",
    selected: "#49434C",
    border: "#514B54",
    text: "#E8E6E3",
    muted: "#939094",
    bright: "#FCFCFA",
    accent: "#78DCE8",
    secondary: "#AB9DF2",
    danger: "#FF6188",
    warning: "#FFD866",
    success: "#A9DC76",
    code_background: "#191719",
    inline_background: "#363237",
    code_palette: CodeSyntaxPalette::MONOKAI_PRO,
};

const AYU_MIRAGE: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#1F2430",
    surface: "#252B38",
    elevated: "#2C3341",
    hover: "#343C4C",
    selected: "#3B4658",
    border: "#424D5E",
    text: "#CCCAC2",
    muted: "#7E8796",
    bright: "#F2F0E8",
    accent: "#FFB454",
    secondary: "#59C2FF",
    danger: "#F07178",
    warning: "#E6B450",
    success: "#AAD94C",
    code_background: "#191E28",
    inline_background: "#2D3442",
    code_palette: CodeSyntaxPalette::AYU_MIRAGE,
};

const MOONLIGHT_HARBOR: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#18202C",
    surface: "#202A38",
    elevated: "#283342",
    hover: "#313E50",
    selected: "#39495E",
    border: "#425267",
    text: "#D7DEEA",
    muted: "#8490A3",
    bright: "#F3F7FC",
    accent: "#82B1FF",
    secondary: "#7FDBCA",
    danger: "#EF7C8E",
    warning: "#E8B66D",
    success: "#A9DC76",
    code_background: "#121923",
    inline_background: "#293645",
    code_palette: CodeSyntaxPalette::MOONLIGHT_HARBOR,
};

const COBALT_VELVET: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#171C32",
    surface: "#1E2540",
    elevated: "#262E4D",
    hover: "#303A5B",
    selected: "#394569",
    border: "#445071",
    text: "#DCE4FF",
    muted: "#8792B3",
    bright: "#F5F7FF",
    accent: "#7FA7FF",
    secondary: "#D29AE8",
    danger: "#F07891",
    warning: "#E9C46A",
    success: "#9BCB7B",
    code_background: "#111629",
    inline_background: "#28304F",
    code_palette: CodeSyntaxPalette::COBALT_VELVET,
};

const OBSIDIAN_EMBER: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#1D1A19",
    surface: "#272321",
    elevated: "#302A27",
    hover: "#3A322E",
    selected: "#463B35",
    border: "#51443D",
    text: "#DDD3C8",
    muted: "#948980",
    bright: "#FFF8F0",
    accent: "#E88961",
    secondary: "#74B6A0",
    danger: "#E06F61",
    warning: "#E8C170",
    success: "#A9BD6A",
    code_background: "#161312",
    inline_background: "#312A26",
    code_palette: CodeSyntaxPalette::OBSIDIAN_EMBER,
};

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
    fn curated(spec: CuratedThemeSpec) -> Self {
        let color = hex_to_hsla;
        let markdown = markdown_syntax_for(spec.code_palette);
        let background = color(spec.background);
        let surface = color(spec.surface);
        let elevated = color(spec.elevated);
        let hover = color(spec.hover);
        let selected = color(spec.selected);
        let border = color(spec.border);
        let text = color(spec.text);
        let muted = color(spec.muted);
        let bright = color(spec.bright);
        let accent = color(spec.accent);
        let _secondary = color(spec.secondary);
        let danger = color(spec.danger);
        let warning = color(spec.warning);
        let success = color(spec.success);
        let semantic_alpha = if spec.dark { 0.34 } else { 0.2 };

        Self {
            text_primary: text,
            text_secondary: muted,
            text_bright: bright,
            text_accent: accent,
            text_danger: danger,
            text_on_accent: if spec.dark {
                color("#14161B")
            } else {
                color("#FFFFFF")
            },
            text_warning: warning,
            text_warning_hover: with_alpha(warning, 0.88),
            bg_primary: background,
            bg_secondary: surface,
            bg_tertiary: hover,
            bg_elevated: elevated,
            bg_hover: hover,
            bg_selected: selected,
            bg_bar: surface,
            bg_tab: surface,
            bg_tab_hover: hover,
            bg_tab_active: background,
            bg_code: color(spec.code_background),
            bg_code_inline: color(spec.inline_background),
            border_primary: border,
            border_secondary: border,
            border_tertiary: with_alpha(muted, if spec.dark { 0.62 } else { 0.52 }),
            border_decorative: with_alpha(muted, 0.42),
            border_code_block: with_alpha(border, 0.9),
            window_border: if spec.dark {
                border
            } else {
                LIGHT_WINDOW_BORDER
            },
            cursor: accent,
            selection: with_alpha(accent, if spec.dark { 0.3 } else { 0.2 }),
            line_highlight: with_alpha(accent, if spec.dark { 0.09 } else { 0.06 }),
            heading: packed_rgb_to_hsla(markdown.heading),
            bold: packed_rgb_to_hsla(markdown.bold),
            italic: packed_rgb_to_hsla(markdown.italic),
            strikethrough: packed_rgb_to_hsla(markdown.strikethrough),
            inline_code: packed_rgb_to_hsla(markdown.inline_code),
            inline_code_bg: color(spec.inline_background),
            highlight_bg: with_alpha(warning, if spec.dark { 0.28 } else { 0.24 }),
            link: packed_rgb_to_hsla(markdown.link),
            syntax_marker: with_alpha(muted, 0.9),
            list_marker: packed_rgb_to_hsla(markdown.list_marker),
            checkbox_checked: success,
            gutter_heading: with_alpha(text, if spec.dark { 0.28 } else { 0.22 }),
            git_status_added: success,
            git_status_modified: warning,
            diff_added_bg: with_alpha(success, semantic_alpha),
            diff_removed_bg: with_alpha(danger, semantic_alpha),
            code_palette: spec.code_palette,
        }
    }

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
        themed.link = theme.link;
        themed.syntax_marker = subtle;
        themed.list_marker = subtle;
        themed.checkbox_checked = subtle;
        themed.gutter_heading = with_alpha(themed.text_primary, if is_dark { 1.0 } else { 1.0 });
    }
    themed
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
struct MoekaRetroPalette {
    dark: Hsla,
    dark_elevated: Hsla,
    dark_hover: Hsla,
    light: Hsla,
    light_elevated: Hsla,
    light_hover: Hsla,
    accent: Hsla,
    secondary_accent: Hsla,
}

#[allow(dead_code)]
impl Theme {
    fn moeka_retro(palette: MoekaRetroPalette, dark: bool) -> Self {
        let mut theme = if dark {
            Self::ayu_dark()
        } else {
            Self::milky()
        };
        let (background, elevated, hover, foreground) = if dark {
            (
                palette.dark,
                palette.dark_elevated,
                palette.dark_hover,
                palette.light,
            )
        } else {
            (
                palette.light,
                palette.light_elevated,
                palette.light_hover,
                palette.dark,
            )
        };

        theme.text_primary = foreground;
        theme.text_secondary = with_alpha(foreground, 0.76);
        theme.text_bright = foreground;
        theme.text_accent = palette.accent;
        theme.text_danger = palette.accent;
        theme.text_on_accent = palette.light;
        theme.text_warning = palette.secondary_accent;
        theme.text_warning_hover = palette.secondary_accent;
        theme.bg_primary = background;
        theme.bg_secondary = elevated;
        theme.bg_tertiary = elevated;
        theme.bg_elevated = elevated;
        theme.bg_hover = hover;
        theme.bg_selected = hover;
        theme.bg_bar = background;
        theme.bg_tab = elevated;
        theme.bg_tab_hover = hover;
        theme.bg_tab_active = background;
        theme.bg_code = elevated;
        theme.bg_code_inline = hover;
        theme.border_primary = foreground;
        theme.border_secondary = foreground;
        theme.border_tertiary = with_alpha(foreground, 0.72);
        theme.border_decorative = foreground;
        theme.border_code_block = foreground;
        theme.window_border = foreground;
        theme.cursor = palette.secondary_accent;
        theme.selection = with_alpha(palette.secondary_accent, 0.32);
        theme.line_highlight = with_alpha(foreground, 0.08);
        theme.heading = palette.accent;
        theme.bold = foreground;
        theme.italic = palette.secondary_accent;
        theme.strikethrough = with_alpha(foreground, 0.65);
        theme.inline_code = foreground;
        theme.inline_code_bg = if dark {
            with_alpha(palette.light, 0.16)
        } else {
            palette.light_elevated
        };
        theme.highlight_bg = with_alpha(palette.secondary_accent, 0.28);
        theme.link = palette.secondary_accent;
        theme.syntax_marker = with_alpha(foreground, 0.58);
        theme.list_marker = palette.accent;
        theme.checkbox_checked = palette.secondary_accent;
        theme.gutter_heading = with_alpha(foreground, 0.3);
        theme.git_status_added = palette.secondary_accent;
        theme.git_status_modified = palette.accent;
        theme.diff_added_bg = with_alpha(palette.secondary_accent, if dark { 0.42 } else { 0.22 });
        theme.diff_removed_bg = with_alpha(palette.accent, if dark { 0.42 } else { 0.22 });
        theme
    }

    fn moeka_print_palette() -> MoekaRetroPalette {
        MoekaRetroPalette {
            dark: crate::hex_hsla!("#1E212B"),
            dark_elevated: crate::hex_hsla!("#252936"),
            dark_hover: crate::hex_hsla!("#323849"),
            light: crate::hex_hsla!("#F3E6BF"),
            light_elevated: crate::hex_hsla!("#E9D7A8"),
            light_hover: crate::hex_hsla!("#DCC58A"),
            accent: crate::hex_hsla!("#D84A2F"),
            secondary_accent: crate::hex_hsla!("#D49A21"),
        }
    }

    pub fn moeka_print_dark() -> Self {
        Self::moeka_retro(Self::moeka_print_palette(), true)
    }

    pub fn moeka_print_light() -> Self {
        Self::moeka_retro(Self::moeka_print_palette(), false)
    }

    fn moeka_mono_palette() -> MoekaRetroPalette {
        MoekaRetroPalette {
            dark: crate::hex_hsla!("#171717"),
            dark_elevated: crate::hex_hsla!("#242424"),
            dark_hover: crate::hex_hsla!("#363636"),
            light: crate::hex_hsla!("#F1EFE8"),
            light_elevated: crate::hex_hsla!("#E3E0D8"),
            light_hover: crate::hex_hsla!("#D1CDC3"),
            accent: crate::hex_hsla!("#55524D"),
            secondary_accent: crate::hex_hsla!("#8C877D"),
        }
    }

    pub fn moeka_mono_dark() -> Self {
        Self::moeka_retro(Self::moeka_mono_palette(), true)
    }

    pub fn moeka_mono_light() -> Self {
        Self::moeka_retro(Self::moeka_mono_palette(), false)
    }

    fn moeka_newspaper_palette() -> MoekaRetroPalette {
        MoekaRetroPalette {
            dark: crate::hex_hsla!("#25231F"),
            dark_elevated: crate::hex_hsla!("#332F29"),
            dark_hover: crate::hex_hsla!("#484139"),
            light: crate::hex_hsla!("#E7DFCC"),
            light_elevated: crate::hex_hsla!("#D8CEB8"),
            light_hover: crate::hex_hsla!("#C8BCA3"),
            accent: crate::hex_hsla!("#A33A2D"),
            secondary_accent: crate::hex_hsla!("#435A55"),
        }
    }

    pub fn moeka_newspaper_dark() -> Self {
        Self::moeka_retro(Self::moeka_newspaper_palette(), true)
    }

    pub fn moeka_newspaper_light() -> Self {
        Self::moeka_retro(Self::moeka_newspaper_palette(), false)
    }

    fn moeka_kraft_palette() -> MoekaRetroPalette {
        MoekaRetroPalette {
            dark: crate::hex_hsla!("#35281C"),
            dark_elevated: crate::hex_hsla!("#493727"),
            dark_hover: crate::hex_hsla!("#5D4733"),
            light: crate::hex_hsla!("#C9A66B"),
            light_elevated: crate::hex_hsla!("#B88E50"),
            light_hover: crate::hex_hsla!("#A77C42"),
            accent: crate::hex_hsla!("#7E2F24"),
            secondary_accent: crate::hex_hsla!("#315A48"),
        }
    }

    pub fn moeka_kraft_dark() -> Self {
        Self::moeka_retro(Self::moeka_kraft_palette(), true)
    }

    pub fn moeka_kraft_light() -> Self {
        Self::moeka_retro(Self::moeka_kraft_palette(), false)
    }

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

fn cap_lightness(color: Hsla, max_lightness: f32) -> Hsla {
    if color.l <= max_lightness {
        return color;
    }
    Hsla {
        l: max_lightness,
        ..color
    }
}

impl Theme {
    /// Light palettes ship very pale muted foregrounds and hairline borders that
    /// disappear against their own background. Deepen those tokens so icons,
    /// muted labels, git markers and separators stay legible. Dark palettes are
    /// returned unchanged.
    pub fn with_light_surface_contrast(&self) -> Self {
        if self.bg_primary.l < 0.5 {
            return self.clone();
        }
        let border_primary = cap_lightness(self.border_primary, 0.72);
        Self {
            text_secondary: cap_lightness(self.text_secondary, 0.42),
            git_status_added: cap_lightness(self.git_status_added, 0.38),
            git_status_modified: cap_lightness(self.git_status_modified, 0.42),
            border_primary,
            // The tab bar, tab items, path bar and workspace bar draw their
            // hairline from `border_secondary`, which light palettes often ship
            // nearly transparent. Hold it to the same weight as the borders
            // around it instead.
            border_secondary: Hsla {
                a: self.border_secondary.a.max(border_primary.a),
                ..cap_lightness(self.border_secondary, 0.72)
            },
            border_tertiary: cap_lightness(self.border_tertiary, 0.68),
            border_decorative: cap_lightness(self.border_decorative, 0.74),
            ..self.clone()
        }
    }
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
        EditorThemeMode, ThemeColorField, ThemeKind, apply_editor_theme_mode, hsla_to_rgb,
        markdown_syntax_for, with_alpha,
    };

    fn contrast_ratio(foreground: gpui::Hsla, background: gpui::Hsla) -> f32 {
        fn luminance(color: gpui::Hsla) -> f32 {
            let (red, green, blue) = hsla_to_rgb(color.h, color.s, color.l);
            let linear = |channel: f32| {
                if channel <= 0.04045 {
                    channel / 12.92
                } else {
                    ((channel + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
        }

        let foreground = luminance(foreground);
        let background = luminance(background);
        (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05)
    }

    #[test]
    fn first_twelve_dark_themes_preserve_the_original_catalog() {
        let labels = ThemeKind::dark_themes()[..12]
            .iter()
            .map(|kind| kind.label())
            .collect::<Vec<_>>();

        assert_eq!(
            labels,
            [
                "One Dark",
                "Milky Dark",
                "Dracula Night",
                "Nordic Night",
                "Tokyo Night",
                "Catppuccin Mocha",
                "GitHub Dark",
                "Solarized Dark",
                "Gruvbox Dark",
                "Rose Pine",
                "Everforest Dark",
                "Monokai Pro",
            ]
        );
    }

    #[test]
    fn light_chrome_hairlines_match_the_primary_border_weight() {
        for kind in ThemeKind::light_themes() {
            let theme = kind.build().with_light_surface_contrast();
            assert_eq!(
                theme.border_secondary.a,
                theme.border_primary.a,
                "{} tab bar / workspace bar hairline is fainter than its other borders",
                kind.label()
            );
            assert!(
                theme.border_secondary.l <= theme.border_primary.l + f32::EPSILON,
                "{} tab bar / workspace bar hairline is lighter than its other borders",
                kind.label()
            );
        }
    }

    #[test]
    fn interface_semantic_colors_round_trip_for_all_builtin_themes() {
        let fields = [
            ThemeColorField::TextOnAccent,
            ThemeColorField::TextWarning,
            ThemeColorField::TextWarningHover,
            ThemeColorField::BgTertiary,
        ];
        for kind in ThemeKind::dark_themes()
            .iter()
            .chain(ThemeKind::light_themes())
        {
            let mut theme = kind.build();
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
        let base = ThemeKind::OneDark.build();
        let themed = apply_editor_theme_mode(&base, EditorThemeMode::Monochrome, true);

        assert_eq!(themed.text_on_accent, base.text_on_accent);
        assert_eq!(themed.text_warning, base.text_warning);
        assert_eq!(themed.text_warning_hover, base.text_warning_hover);
        assert_eq!(themed.bg_tertiary, base.bg_tertiary);
    }

    #[test]
    fn monochrome_mode_keeps_code_block_colors() {
        let base = ThemeKind::OneDark.build();
        let themed = apply_editor_theme_mode(&base, EditorThemeMode::Monochrome, true);

        assert_eq!(themed.bg_code, base.bg_code);
        assert_eq!(themed.border_code_block, base.border_code_block);
        assert_eq!(themed.heading, with_alpha(base.text_bright, 0.88));
        assert_eq!(themed.inline_code, with_alpha(base.text_bright, 0.88));
        assert_eq!(themed.link, base.link);
    }

    #[test]
    fn curated_themes_have_complete_surfaces_and_syntax_palettes() {
        let mut code_palettes = std::collections::HashSet::new();
        let mut markdown_palettes = std::collections::HashSet::new();
        for kind in ThemeKind::dark_themes()
            .iter()
            .chain(ThemeKind::light_themes())
        {
            let theme = kind.build();
            assert_ne!(theme.bg_primary, theme.bg_secondary, "{}", kind.label());
            assert_ne!(theme.inline_code_bg, theme.bg_primary, "{}", kind.label());
            assert_ne!(theme.code_palette.foreground, 0, "{}", kind.label());
            assert_ne!(theme.code_palette.keyword, theme.code_palette.string);
            assert!(code_palettes.insert(theme.code_palette), "{}", kind.label());
            let markdown = markdown_syntax_for(theme.code_palette);
            assert_ne!(markdown.heading, markdown.link, "{}", kind.label());
            assert!(markdown_palettes.insert(markdown), "{}", kind.label());
        }
    }

    #[test]
    fn one_dark_uses_its_original_markdown_accents() {
        let theme = ThemeKind::OneDark.build();

        assert_eq!(theme.heading, crate::hex_hsla!("#E06C75"));
        assert_eq!(theme.bold, crate::hex_hsla!("#D19A66"));
        assert_eq!(theme.italic, crate::hex_hsla!("#C678DD"));
        assert_eq!(theme.inline_code, crate::hex_hsla!("#98C379"));
        assert_eq!(theme.link, crate::hex_hsla!("#61AFEF"));
    }

    #[test]
    fn markdown_semantic_colors_remain_readable_in_every_theme() {
        let mut failures = Vec::new();
        for kind in ThemeKind::dark_themes()
            .iter()
            .chain(ThemeKind::light_themes())
        {
            let theme = kind.build();
            for (name, color) in [
                ("heading", theme.heading),
                ("bold", theme.bold),
                ("italic", theme.italic),
                ("inline_code", theme.inline_code),
                ("link", theme.link),
                ("list_marker", theme.list_marker),
            ] {
                if contrast_ratio(color, theme.bg_primary) < 3.0 {
                    failures.push(format!("{} {name}", kind.label()));
                }
            }
            if contrast_ratio(theme.strikethrough, theme.bg_primary) < 2.5 {
                failures.push(format!("{} strikethrough", kind.label()));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join(", "));
    }

    #[test]
    fn monochrome_mode_uses_grayscale_markers_in_light_theme() {
        let base = ThemeKind::PorcelainMist.build();
        let themed = apply_editor_theme_mode(&base, EditorThemeMode::Monochrome, false);

        assert_eq!(themed.heading, base.text_primary);
        assert_eq!(themed.inline_code, base.text_primary);
        assert_eq!(themed.link, base.link);
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
