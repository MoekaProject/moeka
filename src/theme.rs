use gpui::Hsla;

#[derive(Clone, Copy)]
pub struct MinimapPalette {
    pub separator: Hsla,
    pub placeholder: Hsla,
    pub viewport: Hsla,
    pub viewport_hover: Hsla,
    pub viewport_pressed: Hsla,
}

impl Theme {
    pub(crate) fn dialog_secondary_hover_background(background: Hsla, preferred: Hsla) -> Hsla {
        if (preferred.l - background.l).abs() >= 0.06 {
            return preferred;
        }
        Hsla {
            l: (background.l + if background.l >= 0.5 { -0.08 } else { 0.08 }).clamp(0.0, 1.0),
            ..preferred
        }
    }

    pub(crate) fn dialog_hover_background(background: Hsla, foreground: Hsla) -> Hsla {
        Hsla {
            l: (background.l + if foreground.l >= 0.5 { -0.08 } else { 0.08 }).clamp(0.0, 1.0),
            ..background
        }
    }

    pub fn minimap_palette(&self) -> MinimapPalette {
        MinimapPalette {
            separator: self.border_primary,
            placeholder: with_alpha(self.bg_primary, 0.2),
            viewport: with_alpha(self.text_bright, 0.22),
            viewport_hover: with_alpha(self.text_accent, 0.38),
            viewport_pressed: with_alpha(self.text_accent, 0.58),
        }
    }
}

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
    StatusSuccess,
    StatusWarning,
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
        Self::InlineCodeBg,
        Self::HighlightBg,
        Self::Link,
        Self::SyntaxMarker,
        Self::ListMarker,
        Self::CheckboxChecked,
        Self::GutterHeading,
        Self::StatusSuccess,
        Self::StatusWarning,
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
            Self::StatusSuccess => "status_success",
            Self::StatusWarning => "status_warning",
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
    /// One Dark accents, with comments lifted for both code and Source surfaces.
    pub const ONE_DARK: Self = Self {
        keyword: 0xC678DD,
        operator: 0x56B6C2,
        string: 0x98C379,
        function: 0x61AFEF,
        type_name: 0xE5C07B,
        number: 0xD19A66,
        variable: 0xE06C75,
        comment: 0x838993,
        foreground: 0xABB2BF,
    };

    /// Pearl Morning: mulberry keywords, indigo strings and emerald calls.
    pub const ONE_LIGHT: Self = Self {
        keyword: 0xA02276,
        operator: 0x4F48A0,
        string: 0x4E3AB0,
        function: 0x0B7258,
        type_name: 0x2357B0,
        number: 0xA24B14,
        variable: 0x69508E,
        comment: 0x77578C,
        foreground: 0x443553,
    };

    pub const DRACULA: Self = Self {
        keyword: 0xFF79C6,
        operator: 0xFF79C6,
        string: 0xF1FA8C,
        function: 0x50FA7B,
        type_name: 0x8BE9FD,
        number: 0xBD93F9,
        variable: 0xFFB86C,
        comment: 0x7C8DC0,
        foreground: 0xF8F8F2,
    };

    pub const NORD: Self = Self {
        keyword: 0x81A1C1,
        operator: 0x81A1C1,
        string: 0xA3BE8C,
        function: 0x88C0D0,
        type_name: 0x8FBCBB,
        number: 0xB892B1,
        variable: 0xD8DEE9,
        comment: 0x909EB9,
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
        comment: 0x7781AE,
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
        comment: 0x858AA0,
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
        keyword: 0xB0BC66,
        operator: 0xA5B97C,
        string: 0x79B5A1,
        function: 0x68ADD3,
        type_name: 0xD2B66F,
        number: 0xD58EAB,
        variable: 0x8EB8BB,
        comment: 0x8AA09B,
        foreground: 0xB1C0BC,
    };

    pub const GRUVBOX_DARK: Self = Self {
        keyword: 0xFF4D37,
        operator: 0xFE8019,
        string: 0xB8BB26,
        function: 0xFABD2F,
        type_name: 0x8EC07C,
        number: 0xD3869B,
        variable: 0x83A598,
        comment: 0x9A8B7B,
        foreground: 0xEBDBB2,
    };

    pub const ROSE_PINE: Self = Self {
        keyword: 0x5193AF,
        operator: 0x908CAA,
        string: 0xF6C177,
        function: 0xEBBCBA,
        type_name: 0x9CCFD8,
        number: 0xC4A7E7,
        variable: 0xE0DEF4,
        comment: 0x8B87A4,
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
        comment: 0x909E94,
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
        comment: 0x908E90,
        foreground: 0xFCFCFA,
    };

    /// Paper White: graphite keywords, mahogany strings and navy calls.
    pub const TOKYO_NIGHT_LIGHT: Self = Self {
        keyword: 0x303631,
        operator: 0x5A5E53,
        string: 0x8C4930,
        function: 0x284D82,
        type_name: 0x526749,
        number: 0x7B3A52,
        variable: 0x444A3D,
        comment: 0x6A6B5D,
        foreground: 0x343630,
    };

    /// Nordic Snow: navy keywords, cyan strings and heather calls.
    pub const NORD_LIGHT: Self = Self {
        keyword: 0x254B81,
        operator: 0x2F6D85,
        string: 0x006B76,
        function: 0x79469C,
        type_name: 0xA04440,
        number: 0x4D59A1,
        variable: 0x365777,
        comment: 0x4B688C,
        foreground: 0x293A57,
    };

    /// Sunlit Canvas: saffron keywords, petrol strings and vermilion calls.
    pub const AYU_LIGHT: Self = Self {
        keyword: 0x876002,
        operator: 0x81518E,
        string: 0x005D74,
        function: 0xA32E26,
        type_name: 0x174FB0,
        number: 0x7832A5,
        variable: 0x615526,
        comment: 0x786529,
        foreground: 0x51421D,
    };

    /// Studio Daylight: cobalt keywords, brick strings and magenta calls.
    pub const XCODE_LIGHT: Self = Self {
        keyword: 0x164BB3,
        operator: 0x006C69,
        string: 0xB42E36,
        function: 0xA12687,
        type_name: 0x1B6751,
        number: 0x7740AE,
        variable: 0x334A72,
        comment: 0x5D6890,
        foreground: 0x26344B,
    };

    /// Porcelain Mist: crimson keywords, navy strings and orchid calls.
    pub const GITHUB_LIGHT: Self = Self {
        keyword: 0xBE203B,
        operator: 0x5B567D,
        string: 0x173E78,
        function: 0x8034AC,
        type_name: 0x995022,
        number: 0x175E9E,
        variable: 0x4E4054,
        comment: 0x527077,
        foreground: 0x253B42,
    };

    /// Solarized Light: olive keywords, jade strings and azure calls.
    pub const SOLARIZED_LIGHT: Self = Self {
        keyword: 0x5B6509,
        operator: 0x705F03,
        string: 0x0A6C64,
        function: 0x0D649E,
        type_name: 0x7E5909,
        number: 0xB02570,
        variable: 0x365F67,
        comment: 0x5C6341,
        foreground: 0x3A4C4B,
    };

    /// Vanilla Latte: caramel keywords, amethyst strings and raspberry calls.
    pub const CATPPUCCIN_LATTE: Self = Self {
        keyword: 0x96520C,
        operator: 0x72502E,
        string: 0x7231A2,
        function: 0xB32644,
        type_name: 0x206252,
        number: 0x2958A0,
        variable: 0x66462A,
        comment: 0x836029,
        foreground: 0x513A2A,
    };

    /// Rose Dawn: crimson keywords, lapis strings and aubergine calls.
    pub const ROSE_PINE_DAWN: Self = Self {
        keyword: 0xB32439,
        operator: 0x8F4564,
        string: 0x264BA8,
        function: 0x742C67,
        type_name: 0x935611,
        number: 0x08706A,
        variable: 0x6A425D,
        comment: 0x805572,
        foreground: 0x523247,
    };

    /// Linen Paper: coffee keywords, wine strings and olive calls.
    pub const GRUVBOX_LIGHT: Self = Self {
        keyword: 0x694023,
        operator: 0x865A23,
        string: 0x922F4E,
        function: 0x4F672A,
        type_name: 0x28655C,
        number: 0xA1492E,
        variable: 0x49382A,
        comment: 0x726045,
        foreground: 0x49382A,
    };

    /// Sage Garden: forest keywords, rust strings and plum calls.
    pub const EVERFOREST_LIGHT: Self = Self {
        keyword: 0x165F38,
        operator: 0x386841,
        string: 0xA34124,
        function: 0x70408E,
        type_name: 0x596E22,
        number: 0x9C5216,
        variable: 0x36543D,
        comment: 0x546D4F,
        foreground: 0x2B4433,
    };

    pub const MILKY_NIGHT: Self = Self {
        keyword: 0xCDA0C9,
        operator: 0xBDB3A2,
        string: 0xBEC789,
        function: 0xE7C89E,
        type_name: 0x8ABDB7,
        number: 0xE3A287,
        variable: 0xD8C7B0,
        comment: 0xA09B90,
        foreground: 0xDDD8CE,
    };

    pub const AYU_MIRAGE: Self = Self {
        keyword: 0xFF8F40,
        operator: 0xF29E74,
        string: 0xAAD94C,
        function: 0xFFB454,
        type_name: 0x59C2FF,
        number: 0xD2A6FF,
        variable: 0x95E6CB,
        comment: 0x7F8E9D,
        foreground: 0xCCCAC2,
    };

    pub const MOONLIGHT_HARBOR: Self = Self {
        keyword: 0xE3B590,
        operator: 0x8BC5BC,
        string: 0xCAD695,
        function: 0x80CCD0,
        type_name: 0xD7BD85,
        number: 0xD8ACC1,
        variable: 0xA8D6C6,
        comment: 0x84A4A5,
        foreground: 0xD0E3DD,
    };

    pub const COBALT_VELVET: Self = Self {
        keyword: 0xF0C779,
        operator: 0x93BFF5,
        string: 0x8ED5CC,
        function: 0xDBA3E0,
        type_name: 0xBDCAFF,
        number: 0xEF9CA4,
        variable: 0xDCE4FF,
        comment: 0x92A0C9,
        foreground: 0xDCE4FF,
    };

    pub const OBSIDIAN_EMBER: Self = Self {
        keyword: 0xE6A077,
        operator: 0xD6AE7B,
        string: 0xA8BD83,
        function: 0xE7C19A,
        type_name: 0x8BBC9E,
        number: 0xD4A0AB,
        variable: 0xE5B18B,
        comment: 0xA28C7C,
        foreground: 0xE3D2C2,
    };

    /// Willow Mist: moss keywords, amethyst strings and claret calls.
    pub const WILLOW_LIGHT: Self = Self {
        keyword: 0x4D6920,
        operator: 0x37643A,
        string: 0x703594,
        function: 0x952A49,
        type_name: 0x8D5B16,
        number: 0x2F54A2,
        variable: 0x525D31,
        comment: 0x656A3C,
        foreground: 0x434B29,
    };

    /// Coastal Fog: teal keywords, cobalt strings and copper calls.
    pub const COASTAL_LIGHT: Self = Self {
        keyword: 0x006970,
        operator: 0x9A4440,
        string: 0x234DB3,
        function: 0xA1511D,
        type_name: 0x773690,
        number: 0x9A2F66,
        variable: 0x2D6471,
        comment: 0x3A706D,
        foreground: 0x23474A,
    };

    /// Lavender Haze: violet keywords, emerald strings and berry calls.
    pub const LAVENDER_LIGHT: Self = Self {
        keyword: 0x6D26B1,
        operator: 0x3652A1,
        string: 0x0E7042,
        function: 0xAB296A,
        type_name: 0x0B6990,
        number: 0x835A0F,
        variable: 0x564277,
        comment: 0x6A5A87,
        foreground: 0x403159,
    };

    /// Apricot Bloom: burnt-orange keywords, olive strings and cobalt calls.
    pub const APRICOT_LIGHT: Self = Self {
        keyword: 0xA14A08,
        operator: 0x775399,
        string: 0x526A18,
        function: 0x1C56B1,
        type_name: 0x8D2F78,
        number: 0x8A5906,
        variable: 0x65432E,
        comment: 0x845A3A,
        foreground: 0x583727,
    };

    /// Silver Morning: steel keywords, ruby strings and graphite calls.
    pub const SILVER_LIGHT: Self = Self {
        keyword: 0x3D6188,
        operator: 0x4D6577,
        string: 0xA22E52,
        function: 0x303E48,
        type_name: 0x644285,
        number: 0x146C67,
        variable: 0x3B535D,
        comment: 0x4B697C,
        foreground: 0x303D45,
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

// Markdown has its own editorial hierarchy, not the code grammar's role order.
// Palette values are authored in perceptual space, then baked to sRGB; no color
// correction or palette generation runs in the rendering/highlighting hot path.
fn markdown_syntax_for(code: CodeSyntaxPalette) -> MarkdownSyntaxPalette {
    if code == CodeSyntaxPalette::GITHUB_LIGHT {
        markdown_syntax(
            0x234F65, 0x9B2C47, 0x74428C, 0x526870, 0x8034AC, 0x126780, 0x386B70,
        )
    } else if code == CodeSyntaxPalette::GRUVBOX_LIGHT {
        markdown_syntax(
            0x674628, 0x8D314C, 0x586332, 0x6C5B45, 0x922F4E, 0x3B632E, 0x886126,
        )
    } else if code == CodeSyntaxPalette::XCODE_LIGHT {
        markdown_syntax(
            0x214CAE, 0x9D267E, 0x226553, 0x52637E, 0xB42E36, 0x195C9E, 0x4D4596,
        )
    } else if code == CodeSyntaxPalette::ROSE_PINE_DAWN {
        markdown_syntax(
            0xAE2C39, 0x794629, 0x57408E, 0x795367, 0x264BA8, 0x086879, 0x922E5A,
        )
    } else if code == CodeSyntaxPalette::CATPPUCCIN_LATTE {
        markdown_syntax(
            0x94520F, 0x9D2A45, 0x6F3295, 0x755B45, 0x7231A2, 0x24614E, 0x8B5B1C,
        )
    } else if code == CodeSyntaxPalette::EVERFOREST_LIGHT {
        markdown_syntax(
            0x205D3C, 0x9C4229, 0x70408E, 0x506A56, 0x70408E, 0x175B67, 0x516A26,
        )
    } else if code == CodeSyntaxPalette::NORD_LIGHT {
        markdown_syntax(
            0x254B81, 0x91423C, 0x79469C, 0x4C6282, 0x086873, 0x225F8C, 0x49649A,
        )
    } else if code == CodeSyntaxPalette::SOLARIZED_LIGHT {
        markdown_syntax(
            0x8B5909, 0x666D06, 0xA22A69, 0x5E6B58, 0x03625B, 0x17669A, 0x626E13,
        )
    } else if code == CodeSyntaxPalette::AYU_LIGHT {
        markdown_syntax(
            0x92610A, 0x9B3029, 0x175D74, 0x746033, 0x005D74, 0x174FB0, 0x75438E,
        )
    } else if code == CodeSyntaxPalette::TOKYO_NIGHT_LIGHT {
        markdown_syntax(
            0x343E36, 0x7D412C, 0x52614A, 0x606153, 0x284D82, 0x284D82, 0x59614E,
        )
    } else if code == CodeSyntaxPalette::ONE_LIGHT {
        markdown_syntax(
            0x9B2D79, 0x775013, 0x0C7359, 0x6D597D, 0x4E3AB0, 0x166957, 0x7E388C,
        )
    } else if code == CodeSyntaxPalette::WILLOW_LIGHT {
        markdown_syntax(
            0x566C22, 0x8B2947, 0x752E76, 0x626D43, 0x703594, 0x30547E, 0x7C5B24,
        )
    } else if code == CodeSyntaxPalette::COASTAL_LIGHT {
        markdown_syntax(
            0x006C70, 0xA25127, 0x773690, 0x4A706F, 0x234DB3, 0x235A9A, 0x9A4440,
        )
    } else if code == CodeSyntaxPalette::LAVENDER_LIGHT {
        markdown_syntax(
            0x6931AB, 0x9D2B65, 0x267047, 0x655183, 0x0A6339, 0x00668E, 0x73518F,
        )
    } else if code == CodeSyntaxPalette::APRICOT_LIGHT {
        markdown_syntax(
            0xA34D12, 0x576A1D, 0x8D2F78, 0x7D5740, 0x1651AB, 0x235EA6, 0x8E5907,
        )
    } else if code == CodeSyntaxPalette::SILVER_LIGHT {
        markdown_syntax(
            0x3E5D79, 0x8C2B4C, 0x4E6258, 0x576B77, 0xA22E52, 0x1C6174, 0x5A5378,
        )
    } else if code == CodeSyntaxPalette::ONE_DARK {
        markdown_syntax(
            0xE06C75, 0xD19A66, 0xC678DD, 0x7E899F, 0x98C379, 0x61AFEF, 0xE06C75,
        )
    } else if code == CodeSyntaxPalette::MILKY_NIGHT {
        markdown_syntax(
            0xE7C89E, 0xE3A287, 0xCDA0C9, 0xA09B90, 0xBEC789, 0x8ABDB7, 0xBDB3A2,
        )
    } else if code == CodeSyntaxPalette::DRACULA {
        markdown_syntax(
            0xFF79C6, 0xFFB86C, 0xBD93F9, 0x7C8DC0, 0x50FA7B, 0x8BE9FD, 0xFF79C6,
        )
    } else if code == CodeSyntaxPalette::NORD {
        markdown_syntax(
            0x88C0D0, 0xEBCB8B, 0xB892B1, 0x909EB9, 0xA3BE8C, 0x81A1C1, 0x8FBCBB,
        )
    } else if code == CodeSyntaxPalette::TOKYO_NIGHT {
        markdown_syntax(
            0x7AA2F7, 0xFF9E64, 0xBB9AF7, 0x7781AE, 0x73DACA, 0x7DCFFF, 0x2AC3DE,
        )
    } else if code == CodeSyntaxPalette::CATPPUCCIN_MOCHA {
        markdown_syntax(
            0xCBA6F7, 0xFAB387, 0xF5C2E7, 0x858AA0, 0xA6E3A1, 0x89B4FA, 0xF9E2AF,
        )
    } else if code == CodeSyntaxPalette::GITHUB_DARK {
        markdown_syntax(
            0x79C0FF, 0xFFA657, 0xD2A8FF, 0x8B949E, 0xA5D6FF, 0x58A6FF, 0xFF7B72,
        )
    } else if code == CodeSyntaxPalette::SOLARIZED_DARK {
        markdown_syntax(
            0xE2AB81, 0xD2B66F, 0xC2A4CF, 0x8AA09B, 0x79B5A1, 0x68ADD3, 0xB0BC66,
        )
    } else if code == CodeSyntaxPalette::GRUVBOX_DARK {
        markdown_syntax(
            0xFE8019, 0xFABD2F, 0xD3869B, 0x9A8B7B, 0xB8BB26, 0x83A598, 0xFF4D37,
        )
    } else if code == CodeSyntaxPalette::ROSE_PINE {
        markdown_syntax(
            0xEB6F92, 0xF6C177, 0xC4A7E7, 0x8B87A4, 0x9CCFD8, 0xC4A7E7, 0xEBBCBA,
        )
    } else if code == CodeSyntaxPalette::EVERFOREST_DARK {
        markdown_syntax(
            0xE67E80, 0xDBBC7F, 0xD699B6, 0x909E94, 0xA7C080, 0x7FBBB3, 0xE69875,
        )
    } else if code == CodeSyntaxPalette::MONOKAI_PRO {
        markdown_syntax(
            0xFF6188, 0xFC9867, 0xAB9DF2, 0x908E90, 0xA9DC76, 0x78DCE8, 0xFFD866,
        )
    } else if code == CodeSyntaxPalette::AYU_MIRAGE {
        markdown_syntax(
            0xFF8F40, 0xFFB454, 0xD2A6FF, 0x7F8E9D, 0xAAD94C, 0x59C2FF, 0xF29E74,
        )
    } else if code == CodeSyntaxPalette::MOONLIGHT_HARBOR {
        markdown_syntax(
            0x80CCD0, 0xE3B590, 0xD8ACC1, 0x84A4A5, 0xCAD695, 0xA8D6C6, 0x8BC5BC,
        )
    } else if code == CodeSyntaxPalette::COBALT_VELVET {
        markdown_syntax(
            0xB9C9FF, 0xF0C779, 0xDBA3E0, 0x92A0C9, 0x8ED5CC, 0x9FBEFF, 0xEF9CA4,
        )
    } else if code == CodeSyntaxPalette::OBSIDIAN_EMBER {
        markdown_syntax(
            0xE6A077, 0xE7C19A, 0xD4A0AB, 0xA28C7C, 0xA8BD83, 0x8BBC9E, 0xD6AE7B,
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
    background: "#F5FAFB",
    surface: "#E8F1F2",
    elevated: "#FBFEFE",
    hover: "#E0EBEE",
    selected: "#D8E5EB",
    border: "#C6D8DB",
    text: "#253B42",
    muted: "#4C6169",
    bright: "#122B34",
    accent: "#205E78",
    secondary: "#9C3048",
    danger: "#BE3746",
    warning: "#935C01",
    success: "#27764E",
    code_background: "#EDF4F6",
    inline_background: "#E5EFF2",
    code_palette: CodeSyntaxPalette::GITHUB_LIGHT,
};

const LINEN_PAPER: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#F7EBD9",
    surface: "#F1E1CA",
    elevated: "#FFF7E9",
    hover: "#ECDBC1",
    selected: "#DDD8BA",
    border: "#D8C7AC",
    text: "#49382A",
    muted: "#63523D",
    bright: "#2D2016",
    accent: "#715334",
    secondary: "#65703D",
    danger: "#A9332C",
    warning: "#7E5506",
    success: "#43662C",
    code_background: "#F2E5D0",
    inline_background: "#EFDFC7",
    code_palette: CodeSyntaxPalette::GRUVBOX_LIGHT,
};

const STUDIO_DAYLIGHT: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#FFFFFF",
    surface: "#F3F5FA",
    elevated: "#FFFFFF",
    hover: "#EBEFF7",
    selected: "#E1E9F7",
    border: "#D2DAEA",
    text: "#26344B",
    muted: "#4D5D78",
    bright: "#12233D",
    accent: "#2457A6",
    secondary: "#9D267E",
    danger: "#C02D3D",
    warning: "#8C5D08",
    success: "#247446",
    code_background: "#F7F8FC",
    inline_background: "#F0F3FA",
    code_palette: CodeSyntaxPalette::XCODE_LIGHT,
};

const ROSE_DAWN: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#FFF0F0",
    surface: "#FAE5E5",
    elevated: "#FFF9F9",
    hover: "#F6DEDE",
    selected: "#F1D7DA",
    border: "#E5C9C9",
    text: "#522F35",
    muted: "#794C53",
    bright: "#35191F",
    accent: "#AE2C39",
    secondary: "#3056A0",
    danger: "#A53A4E",
    warning: "#855501",
    success: "#306957",
    code_background: "#FCE9E9",
    inline_background: "#F8E3E3",
    code_palette: CodeSyntaxPalette::ROSE_PINE_DAWN,
};

const VANILLA_LATTE: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#FFF4E8",
    surface: "#F8E9D9",
    elevated: "#FFFAF2",
    hover: "#F3E4D1",
    selected: "#EFDCCF",
    border: "#E1CEBB",
    text: "#513A2A",
    muted: "#6F5540",
    bright: "#332010",
    accent: "#94520F",
    secondary: "#6F3295",
    danger: "#C50A34",
    warning: "#8E560A",
    success: "#24721A",
    code_background: "#FBEDDF",
    inline_background: "#F8E7D3",
    code_palette: CodeSyntaxPalette::CATPPUCCIN_LATTE,
};

const SAGE_GARDEN: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#E7F2EA",
    surface: "#DBE9DE",
    elevated: "#F3FBF5",
    hover: "#D4E4D9",
    selected: "#CADFD1",
    border: "#BCD1C3",
    text: "#2B4433",
    muted: "#445D4A",
    bright: "#15311F",
    accent: "#205D3C",
    secondary: "#9C4229",
    danger: "#A83E3C",
    warning: "#805B03",
    success: "#486B2E",
    code_background: "#DEEDE2",
    inline_background: "#D7E8DC",
    code_palette: CodeSyntaxPalette::EVERFOREST_LIGHT,
};

const NORDIC_SNOW: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#EBF1FC",
    surface: "#DEE7F7",
    elevated: "#F7FAFF",
    hover: "#D8E2F3",
    selected: "#CEDCF1",
    border: "#C0CEE5",
    text: "#293A57",
    muted: "#425877",
    bright: "#16294A",
    accent: "#254B81",
    secondary: "#78508E",
    danger: "#A93E4C",
    warning: "#815C03",
    success: "#4B6A42",
    code_background: "#E3EBF9",
    inline_background: "#DBE6F6",
    code_palette: CodeSyntaxPalette::NORD_LIGHT,
};

const SOLARIZED_LIGHT_THEME: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#F4EACD",
    surface: "#EDE0BE",
    elevated: "#FDF6DE",
    hover: "#E7DBB6",
    selected: "#D5D8AF",
    border: "#D4C7A3",
    text: "#3A4C4B",
    muted: "#4C5846",
    bright: "#223B3B",
    accent: "#25687C",
    secondary: "#7A6B17",
    danger: "#AC302D",
    warning: "#7A5706",
    success: "#52640A",
    code_background: "#F0E3C3",
    inline_background: "#ECDEBB",
    code_palette: CodeSyntaxPalette::SOLARIZED_LIGHT,
};

const SUNLIT_CANVAS: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#FFF6CB",
    surface: "#F9EBB8",
    elevated: "#FFFBE5",
    hover: "#F5E5AF",
    selected: "#EEDEB1",
    border: "#E2D09C",
    text: "#51421D",
    muted: "#6B572A",
    bright: "#35280D",
    accent: "#92610A",
    secondary: "#176477",
    danger: "#B3333D",
    warning: "#845A07",
    success: "#436D25",
    code_background: "#FBEFC3",
    inline_background: "#F7E9B7",
    code_palette: CodeSyntaxPalette::AYU_LIGHT,
};

const PAPER_WHITE: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#FFFFFF",
    surface: "#F4F5F6",
    elevated: "#FFFFFF",
    hover: "#EDEFF2",
    selected: "#E4E9EF",
    border: "#D7DADE",
    text: "#303236",
    muted: "#5C6168",
    bright: "#1D2025",
    accent: "#3D4D59",
    secondary: "#854538",
    danger: "#A84A4B",
    warning: "#826303",
    success: "#397353",
    code_background: "#F8F8F9",
    inline_background: "#F1F3F6",
    code_palette: CodeSyntaxPalette::TOKYO_NIGHT_LIGHT,
};

const PEARL_MORNING: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#F4F0FC",
    surface: "#ECE5F7",
    elevated: "#FCF9FF",
    hover: "#E7DFF5",
    selected: "#E1D8F2",
    border: "#D5C9E6",
    text: "#443553",
    muted: "#614E71",
    bright: "#2D1C3D",
    accent: "#9B2D79",
    secondary: "#187459",
    danger: "#B03445",
    warning: "#835A01",
    success: "#316D49",
    code_background: "#F0E9F9",
    inline_background: "#EBE3F6",
    code_palette: CodeSyntaxPalette::ONE_LIGHT,
};

const WILLOW_MIST: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#EFF2D6",
    surface: "#E5E9C6",
    elevated: "#FAFCE8",
    hover: "#DEE4BB",
    selected: "#D7DEB3",
    border: "#C8D0A4",
    text: "#434B29",
    muted: "#525C33",
    bright: "#2C3415",
    accent: "#566C22",
    secondary: "#752E76",
    danger: "#A83F3F",
    warning: "#7E5D03",
    success: "#4D6A33",
    code_background: "#E8ECCC",
    inline_background: "#E2E7C0",
    code_palette: CodeSyntaxPalette::WILLOW_LIGHT,
};

const COASTAL_FOG: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#E5F5F3",
    surface: "#D5EDEA",
    elevated: "#F3FCFA",
    hover: "#D0E7E5",
    selected: "#C4E2E0",
    border: "#BAD3D0",
    text: "#23474A",
    muted: "#385D5D",
    bright: "#103438",
    accent: "#006C70",
    secondary: "#A25127",
    danger: "#AA3D4B",
    warning: "#835B0A",
    success: "#406B56",
    code_background: "#DBF0EE",
    inline_background: "#D3EBE8",
    code_palette: CodeSyntaxPalette::COASTAL_LIGHT,
};

const LAVENDER_HAZE: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#EEECFB",
    surface: "#E5E1F7",
    elevated: "#F8F7FF",
    hover: "#E0DBF3",
    selected: "#D9D5F3",
    border: "#CDC6E5",
    text: "#403159",
    muted: "#5C487A",
    bright: "#271741",
    accent: "#6931AB",
    secondary: "#267047",
    danger: "#A93347",
    warning: "#805708",
    success: "#406846",
    code_background: "#E9E5F8",
    inline_background: "#E4DFF6",
    code_palette: CodeSyntaxPalette::LAVENDER_LIGHT,
};

const APRICOT_BLOOM: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#FFEEE2",
    surface: "#F9E3D3",
    elevated: "#FFF7F0",
    hover: "#F6DDCB",
    selected: "#EFD7C5",
    border: "#E4C8B3",
    text: "#583727",
    muted: "#734D37",
    bright: "#381D10",
    accent: "#A34D12",
    secondary: "#235EA6",
    danger: "#B12D39",
    warning: "#835607",
    success: "#516723",
    code_background: "#FCE7D8",
    inline_background: "#F9E1D0",
    code_palette: CodeSyntaxPalette::APRICOT_LIGHT,
};

const SILVER_MORNING: CuratedThemeSpec = CuratedThemeSpec {
    dark: false,
    background: "#ECEFF1",
    surface: "#E1E5E9",
    elevated: "#F7F9FA",
    hover: "#DAE0E5",
    selected: "#D0DAE2",
    border: "#C3CCD3",
    text: "#303D45",
    muted: "#475A66",
    bright: "#192C37",
    accent: "#3E5D79",
    secondary: "#A32C51",
    danger: "#A83947",
    warning: "#7D5A09",
    success: "#4D673C",
    code_background: "#E4E9ED",
    inline_background: "#DDE4E9",
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
    muted: "#A6AEB9",
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
    muted: "#9EA7C0",
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
    background: "#24212D",
    surface: "#2D2838",
    elevated: "#373042",
    hover: "#42394E",
    selected: "#4D425B",
    border: "#594D68",
    text: "#CDD6F4",
    muted: "#AFB5CF",
    bright: "#F0F2FA",
    accent: "#CBA6F7",
    secondary: "#F5C2E7",
    danger: "#F38BA8",
    warning: "#E5C890",
    success: "#A6E3A1",
    code_background: "#1C1925",
    inline_background: "#393145",
    code_palette: CodeSyntaxPalette::CATPPUCCIN_MOCHA,
};

const EVERFOREST_DARK_THEME: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#28342D",
    surface: "#313F35",
    elevated: "#3B4A3F",
    hover: "#455649",
    selected: "#506254",
    border: "#5E7260",
    text: "#DFD9C6",
    muted: "#D3DBCF",
    bright: "#F0E9D8",
    accent: "#83C092",
    secondary: "#7FBBB3",
    danger: "#F2898A",
    warning: "#DBBC7F",
    success: "#A7C080",
    code_background: "#222C26",
    inline_background: "#37473B",
    code_palette: CodeSyntaxPalette::EVERFOREST_DARK,
};

const ROSE_PINE_THEME: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#261F32",
    surface: "#30273E",
    elevated: "#3A2F4A",
    hover: "#453857",
    selected: "#514166",
    border: "#5E4D75",
    text: "#E0DEF4",
    muted: "#B9B5D4",
    bright: "#F4F1FF",
    accent: "#EBBCBA",
    secondary: "#9CCFD8",
    danger: "#EB6F92",
    warning: "#F6C177",
    success: "#75B8A3",
    code_background: "#1E1829",
    inline_background: "#3A2D49",
    code_palette: CodeSyntaxPalette::ROSE_PINE,
};

const GRUVBOX_DARK_THEME: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#29251D",
    surface: "#343025",
    elevated: "#3E392C",
    hover: "#4B4434",
    selected: "#554D3B",
    border: "#655B44",
    text: "#E8DDC9",
    muted: "#CABFAF",
    bright: "#FFF6E8",
    accent: "#D9AF56",
    secondary: "#D3869B",
    danger: "#F0746A",
    warning: "#D8AE55",
    success: "#A4B86A",
    code_background: "#221F18",
    inline_background: "#3C3528",
    code_palette: CodeSyntaxPalette::GRUVBOX_DARK,
};

const MILKY_NIGHT: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#282725",
    surface: "#31302C",
    elevated: "#3A3832",
    hover: "#444139",
    selected: "#504C41",
    border: "#605A4D",
    text: "#DDD8CE",
    muted: "#C3BEB2",
    bright: "#F6F0E3",
    accent: "#D6BA8C",
    secondary: "#9BBDB4",
    danger: "#E58D91",
    warning: "#EBCB8B",
    success: "#A7C080",
    code_background: "#211F1D",
    inline_background: "#3B3831",
    code_palette: CodeSyntaxPalette::MILKY_NIGHT,
};

const DRACULA_NIGHT: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#282436",
    surface: "#312C42",
    elevated: "#3A334D",
    hover: "#443C59",
    selected: "#504564",
    border: "#5F5273",
    text: "#E6E6E1",
    muted: "#B7BBCE",
    bright: "#F8F8F2",
    accent: "#BD93F9",
    secondary: "#8BE9FD",
    danger: "#FF6E87",
    warning: "#F1CA76",
    success: "#50C878",
    code_background: "#211D2D",
    inline_background: "#3B334E",
    code_palette: CodeSyntaxPalette::DRACULA,
};

const NORDIC_NIGHT: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#2E3440",
    surface: "#363E4C",
    elevated: "#3F495A",
    hover: "#485467",
    selected: "#526077",
    border: "#606E85",
    text: "#D8DEE9",
    muted: "#D0DCF0",
    bright: "#ECEFF4",
    accent: "#88C0D0",
    secondary: "#B48EAD",
    danger: "#F18D95",
    warning: "#D5B36A",
    success: "#A3BE8C",
    code_background: "#272D38",
    inline_background: "#3C4657",
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
    muted: "#98A1AB",
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
    text: "#B4C4C0",
    muted: "#AFC6C0",
    bright: "#F3ECD8",
    accent: "#268BD2",
    secondary: "#B58900",
    danger: "#F57073",
    warning: "#D3A52E",
    success: "#8FA41C",
    code_background: "#00232C",
    inline_background: "#0B3B46",
    code_palette: CodeSyntaxPalette::SOLARIZED_DARK,
};

const MONOKAI_PRO_THEME: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#29252A",
    surface: "#332E34",
    elevated: "#3E3740",
    hover: "#493F4B",
    selected: "#55495A",
    border: "#65556A",
    text: "#E8E6E3",
    muted: "#C2BEC3",
    bright: "#FCFCFA",
    accent: "#78DCE8",
    secondary: "#AB9DF2",
    danger: "#FF6188",
    warning: "#FFD866",
    success: "#A9DC76",
    code_background: "#211E22",
    inline_background: "#3B333E",
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
    muted: "#ABB5C4",
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
    background: "#102D32",
    surface: "#19393D",
    elevated: "#23474A",
    hover: "#2D5557",
    selected: "#356165",
    border: "#427275",
    text: "#D0E3DD",
    muted: "#B8DADB",
    bright: "#ECF6ED",
    accent: "#80CCD0",
    secondary: "#E3B590",
    danger: "#EF7C8E",
    warning: "#E8B66D",
    success: "#A9DC76",
    code_background: "#0B2429",
    inline_background: "#244448",
    code_palette: CodeSyntaxPalette::MOONLIGHT_HARBOR,
};

const COBALT_VELVET: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#151D42",
    surface: "#202A54",
    elevated: "#2A3565",
    hover: "#354173",
    selected: "#404E86",
    border: "#4F6099",
    text: "#DCE4FF",
    muted: "#B6C5EF",
    bright: "#F3F5FF",
    accent: "#B9C9FF",
    secondary: "#F0C779",
    danger: "#F07891",
    warning: "#E9C46A",
    success: "#9BCB7B",
    code_background: "#111735",
    inline_background: "#2D3965",
    code_palette: CodeSyntaxPalette::COBALT_VELVET,
};

const OBSIDIAN_EMBER: CuratedThemeSpec = CuratedThemeSpec {
    dark: true,
    background: "#2B201C",
    surface: "#342720",
    elevated: "#403027",
    hover: "#4D392E",
    selected: "#5A4336",
    border: "#6C5141",
    text: "#E3D2C2",
    muted: "#CBB4A3",
    bright: "#FBF0E1",
    accent: "#E6A077",
    secondary: "#8BBC9E",
    danger: "#E27062",
    warning: "#E8C170",
    success: "#A9BD6A",
    code_background: "#221916",
    inline_background: "#443126",
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
    pub status_success: Hsla,
    pub status_warning: Hsla,

    // ── Code block syntax highlighting ──────────────────────
    /// Token palette for code block / mermaid syntax highlighting.
    /// Not part of [`ThemeColorField`]; not exposed in the theme debugger.
    pub code_palette: CodeSyntaxPalette,
}

/// Native application semantic slots. Default values follow the existing
/// debugger fields; `UiColorOverrides` gives each slot independent ownership
/// without mutating Markdown/editor colors. Resolve sidebar defaults against
/// the sidebar theme, never the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiColor {
    Background,
    Foreground,
    Card,
    CardForeground,
    Popover,
    PopoverForeground,
    Primary,
    PrimaryForeground,
    Secondary,
    SecondaryForeground,
    Muted,
    MutedForeground,
    Accent,
    AccentForeground,
    Destructive,
    DestructiveForeground,
    Border,
    Input,
    Ring,
    Sidebar,
    SidebarForeground,
    SidebarPrimary,
    SidebarPrimaryForeground,
    SidebarAccent,
    SidebarAccentForeground,
    SidebarBorder,
    SidebarRing,
    Success,
    Warning,
    Info,
    // Reserved color slots only; Moeka has no application chart component.
    Chart1,
    Chart2,
    Chart3,
    Chart4,
    Chart5,
}

impl UiColor {
    pub const ALL: [Self; 35] = [
        Self::Background,
        Self::Foreground,
        Self::Card,
        Self::CardForeground,
        Self::Popover,
        Self::PopoverForeground,
        Self::Primary,
        Self::PrimaryForeground,
        Self::Secondary,
        Self::SecondaryForeground,
        Self::Muted,
        Self::MutedForeground,
        Self::Accent,
        Self::AccentForeground,
        Self::Destructive,
        Self::DestructiveForeground,
        Self::Border,
        Self::Input,
        Self::Ring,
        Self::Sidebar,
        Self::SidebarForeground,
        Self::SidebarPrimary,
        Self::SidebarPrimaryForeground,
        Self::SidebarAccent,
        Self::SidebarAccentForeground,
        Self::SidebarBorder,
        Self::SidebarRing,
        Self::Success,
        Self::Warning,
        Self::Info,
        Self::Chart1,
        Self::Chart2,
        Self::Chart3,
        Self::Chart4,
        Self::Chart5,
    ];

    pub fn source(self) -> ThemeColorField {
        use ThemeColorField as F;
        match self {
            Self::Background | Self::Sidebar => F::BgPrimary,
            Self::Foreground
            | Self::CardForeground
            | Self::SecondaryForeground
            | Self::SidebarForeground => F::TextPrimary,
            Self::Card => F::BgElevated,
            Self::Popover => F::BgPrimary,
            Self::PopoverForeground | Self::AccentForeground | Self::SidebarAccentForeground => {
                F::TextBright
            }
            Self::Primary | Self::SidebarPrimary => F::TextPrimary,
            Self::PrimaryForeground | Self::SidebarPrimaryForeground => F::BgPrimary,
            Self::Secondary | Self::Muted => F::BgSecondary,
            Self::MutedForeground => F::TextSecondary,
            Self::Accent | Self::SidebarAccent => F::BgSelected,
            Self::Destructive => F::TextDanger,
            Self::DestructiveForeground => F::TextOnAccent,
            Self::Border | Self::Input | Self::SidebarBorder => F::BorderPrimary,
            Self::Ring | Self::SidebarRing | Self::Info | Self::Chart1 => F::TextAccent,
            Self::Success | Self::Chart2 => F::StatusSuccess,
            Self::Warning | Self::Chart3 => F::TextWarning,
            Self::Chart4 => F::TextDanger,
            Self::Chart5 => F::TextBright,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TerminalPalette {
    pub foreground: Hsla,
    pub bright_foreground: Hsla,
    pub dim_foreground: Hsla,
    pub black: Hsla,
    pub red: Hsla,
    pub green: Hsla,
    pub yellow: Hsla,
    pub blue: Hsla,
    pub magenta: Hsla,
    pub cyan: Hsla,
    pub white: Hsla,
    pub bright_black: Hsla,
    pub bright_red: Hsla,
    pub bright_green: Hsla,
    pub bright_yellow: Hsla,
    pub bright_blue: Hsla,
    pub bright_magenta: Hsla,
    pub bright_cyan: Hsla,
    pub bright_white: Hsla,
    pub dim_black: Hsla,
    pub dim_red: Hsla,
    pub dim_green: Hsla,
    pub dim_yellow: Hsla,
    pub dim_blue: Hsla,
    pub dim_magenta: Hsla,
    pub dim_cyan: Hsla,
    pub dim_white: Hsla,
}

/// Terminal colors are intentionally independent from the editor theme. These
/// are Zed's One Dark/Light terminal palettes; only the terminal background is
/// supplied by the active Moeka theme.
pub(crate) fn terminal_palette(panel_bg: Hsla) -> TerminalPalette {
    if panel_bg.l < 0.5 {
        TerminalPalette {
            foreground: crate::hex_hsla!("#ABB2BF"),
            bright_foreground: crate::hex_hsla!("#DCE0E5"),
            dim_foreground: crate::hex_hsla!("#636D83"),
            black: crate::hex_hsla!("#282C34"),
            red: crate::hex_hsla!("#E06C75"),
            green: crate::hex_hsla!("#98C379"),
            yellow: crate::hex_hsla!("#E5C07B"),
            blue: crate::hex_hsla!("#61AFEF"),
            magenta: crate::hex_hsla!("#C678DD"),
            cyan: crate::hex_hsla!("#56B6C2"),
            white: crate::hex_hsla!("#ABB2BF"),
            bright_black: crate::hex_hsla!("#636D83"),
            bright_red: crate::hex_hsla!("#EA858B"),
            bright_green: crate::hex_hsla!("#AAD581"),
            bright_yellow: crate::hex_hsla!("#FFD885"),
            bright_blue: crate::hex_hsla!("#85C1FF"),
            bright_magenta: crate::hex_hsla!("#D398EB"),
            bright_cyan: crate::hex_hsla!("#6ED5DE"),
            bright_white: crate::hex_hsla!("#FAFAFA"),
            dim_black: crate::hex_hsla!("#3B3F4A"),
            dim_red: crate::hex_hsla!("#A7545A"),
            dim_green: crate::hex_hsla!("#6D8F59"),
            dim_yellow: crate::hex_hsla!("#B8985B"),
            dim_blue: crate::hex_hsla!("#457CAD"),
            dim_magenta: crate::hex_hsla!("#8D54A0"),
            dim_cyan: crate::hex_hsla!("#3C818A"),
            dim_white: crate::hex_hsla!("#8F969B"),
        }
    } else {
        TerminalPalette {
            foreground: crate::hex_hsla!("#2A2C33"),
            bright_foreground: crate::hex_hsla!("#2A2C33"),
            dim_foreground: crate::hex_hsla!("#BBBBBB"),
            black: crate::hex_hsla!("#000000"),
            red: crate::hex_hsla!("#DE3E35"),
            green: crate::hex_hsla!("#3F953A"),
            yellow: crate::hex_hsla!("#D2B67C"),
            blue: crate::hex_hsla!("#2F5AF3"),
            magenta: crate::hex_hsla!("#950095"),
            cyan: crate::hex_hsla!("#0997B3"),
            white: crate::hex_hsla!("#BBBBBB"),
            bright_black: crate::hex_hsla!("#000000"),
            bright_red: crate::hex_hsla!("#DE3E35"),
            bright_green: crate::hex_hsla!("#3F953A"),
            bright_yellow: crate::hex_hsla!("#D2B67C"),
            bright_blue: crate::hex_hsla!("#2F5AF3"),
            bright_magenta: crate::hex_hsla!("#A00095"),
            bright_cyan: crate::hex_hsla!("#0BBCD6"),
            bright_white: crate::hex_hsla!("#FFFFFF"),
            dim_black: crate::hex_hsla!("#555555"),
            dim_red: crate::hex_hsla!("#9C2B26"),
            dim_green: crate::hex_hsla!("#2B6927"),
            dim_yellow: crate::hex_hsla!("#A48C5A"),
            dim_blue: crate::hex_hsla!("#2140AB"),
            dim_magenta: crate::hex_hsla!("#6A006A"),
            dim_cyan: crate::hex_hsla!("#0A7B92"),
            dim_white: crate::hex_hsla!("#888888"),
        }
    }
}

pub(crate) fn ui_shadow_color(opacity: f32) -> Hsla {
    gpui::hsla(0.0, 0.0, 0.0, opacity)
}

impl Theme {
    pub fn ui_color(&self, slot: UiColor) -> Hsla {
        self.color(slot.source())
    }
}

#[derive(Clone, Copy)]
pub struct SwitchPalette {
    pub checked_background: Hsla,
    pub checked_hover_background: Hsla,
    pub unchecked_background: Hsla,
    pub unchecked_hover_background: Hsla,
    pub unchecked_border: Hsla,
    pub thumb: Hsla,
    pub unchecked_thumb: Hsla,
    pub description: Hsla,
}

impl Theme {
    pub fn switch_palette(&self, interface: &Self) -> SwitchPalette {
        SwitchPalette {
            checked_background: self.text_primary,
            checked_hover_background: self.text_bright,
            unchecked_background: interface.bg_secondary,
            unchecked_hover_background: interface.bg_hover,
            unchecked_border: interface.border_primary,
            thumb: self.bg_primary,
            unchecked_thumb: self.bg_primary,
            description: interface.text_secondary,
        }
    }

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
            // Separate code from prose without giving the block a heavy frame.
            border_code_block: with_alpha(border, if spec.dark { 0.38 } else { 0.55 }),
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
            status_success: success,
            status_warning: warning,
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
            ThemeColorField::StatusSuccess => self.status_success,
            ThemeColorField::StatusWarning => self.status_warning,
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
            ThemeColorField::StatusSuccess => self.status_success = color,
            ThemeColorField::StatusWarning => self.status_warning = color,
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
        // Muted text is already contrast-checked. Another alpha reduction here
        // makes small italic/list text fail on the lighter paper palettes.
        let subtle = themed.text_secondary;
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
        theme.status_success = palette.secondary_accent;
        theme.status_warning = palette.accent;
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
            status_success: crate::hex_hsla!("#88C796"),
            status_warning: crate::hex_hsla!("#DDC194"),
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
            status_success: crate::hex_hsla!("#88C796"),
            status_warning: crate::hex_hsla!("#DDC194"),
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
            status_success: crate::hex_hsla!("#88C796"),
            status_warning: crate::hex_hsla!("#DDC194"),
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
            status_success: crate::hex_hsla!("#88C796"),
            status_warning: crate::hex_hsla!("#DDC194"),
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
            status_success: crate::hex_hsla!("#50FA7B"),
            status_warning: crate::hex_hsla!("#F1FA8C"),
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
            status_success: crate::hex_hsla!("#A3BE8C"),
            status_warning: crate::hex_hsla!("#EBCB8B"),
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
            status_success: crate::hex_hsla!("#9ECE6A"),
            status_warning: crate::hex_hsla!("#E0AF68"),
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
            status_success: crate::hex_hsla!("#A6E3A1"),
            status_warning: crate::hex_hsla!("#F9E2AF"),
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
            status_success: crate::hex_hsla!("#3FB950"),
            status_warning: crate::hex_hsla!("#D29922"),
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
            status_success: crate::hex_hsla!("#859900"),
            status_warning: crate::hex_hsla!("#B58900"),
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
            status_success: crate::hex_hsla!("#B8BB26"),
            status_warning: crate::hex_hsla!("#FABD2F"),
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
            status_success: crate::hex_hsla!("#9CCFD8"),
            status_warning: crate::hex_hsla!("#F6C177"),
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
            status_success: crate::hex_hsla!("#1A7F37"),
            status_warning: crate::hex_hsla!("#9A6700"),
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
            status_success: crate::hex_hsla!("#859900"),
            status_warning: crate::hex_hsla!("#B58900"),
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
            status_success: crate::hex_hsla!("#40A02B"),
            status_warning: crate::hex_hsla!("#DF8E1D"),
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
            status_success: crate::hex_hsla!("#56949F"),
            status_warning: crate::hex_hsla!("#EA9D34"),
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
            status_success: crate::hex_hsla!("#79740E"),
            status_warning: crate::hex_hsla!("#B57614"),
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
            status_success: crate::hex_hsla!("#8DA101"),
            status_warning: crate::hex_hsla!("#DFA000"),
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
            status_success: crate::hex_hsla!("#485E30"),
            status_warning: crate::hex_hsla!("#8F5E15"),
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
            status_success: crate::hex_hsla!("#6C8A55"),
            status_warning: crate::hex_hsla!("#B08800"),
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
            status_success: crate::hex_hsla!("#A7C080"),
            status_warning: crate::hex_hsla!("#DBBC7F"),
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
            status_success: crate::hex_hsla!("#A9DC76"),
            status_warning: crate::hex_hsla!("#FFD866"),
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
            status_success: crate::hex_hsla!("#6CBF43"),
            status_warning: crate::hex_hsla!("#F2AE49"),
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
            status_success: crate::hex_hsla!("#28A745"),
            status_warning: crate::hex_hsla!("#B58900"),
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
        // Curated light borders are stored as soft hairlines; only legacy
        // translucent hairlines are lifted to the primary border weight.
        let border_primary = self.border_primary;
        Self {
            text_secondary: cap_lightness(self.text_secondary, 0.42),
            status_success: cap_lightness(self.status_success, 0.38),
            status_warning: cap_lightness(self.status_warning, 0.42),
            border_primary,
            // The tab bar, tab items, path bar and workspace bar draw their
            // hairline from `border_secondary`, which light palettes often ship
            // nearly transparent. Hold it to the same weight as the borders
            // around it instead.
            border_secondary: Hsla {
                a: self.border_secondary.a.max(border_primary.a),
                ..self.border_secondary
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
        fn luminance(red: f32, green: f32, blue: f32) -> f32 {
            let linear = |channel: f32| {
                if channel <= 0.04045 {
                    channel / 12.92
                } else {
                    ((channel + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
        }

        let (r, g, b) = hsla_to_rgb(foreground.h, foreground.s, foreground.l);
        let (br, bg, bb) = hsla_to_rgb(background.h, background.s, background.l);
        let a = foreground.a;
        let foreground = luminance(
            r * a + br * (1.0 - a),
            g * a + bg * (1.0 - a),
            b * a + bb * (1.0 - a),
        );
        let background = luminance(br, bg, bb);
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
            let base = kind.build();
            for &mode in EditorThemeMode::all() {
                let theme = apply_editor_theme_mode(&base, mode, kind.is_dark());
                for (name, color) in [
                    ("heading", theme.heading),
                    ("bold", theme.bold),
                    ("italic", theme.italic),
                    ("link", theme.link),
                    ("list_marker", theme.list_marker),
                    ("strikethrough", theme.strikethrough),
                    ("syntax_marker", theme.syntax_marker),
                ] {
                    let ratio = contrast_ratio(color, theme.bg_primary);
                    if ratio < 4.5 {
                        failures.push(format!("{} {mode:?}/{name}: {ratio:.2}", kind.label()));
                    }
                }
                for background in [theme.inline_code_bg, theme.bg_primary] {
                    let ratio = contrast_ratio(theme.inline_code, background);
                    if ratio < 4.5 {
                        failures.push(format!("{} {mode:?}/inline_code: {ratio:.2}", kind.label()));
                    }
                }
                assert!(theme.code_palette == base.code_palette);
            }
        }
        assert!(failures.is_empty(), "{}", failures.join(", "));
    }

    #[test]
    fn code_tokens_are_readable_on_code_blocks_and_raw_source_canvases() {
        let mut failures = Vec::new();
        for kind in ThemeKind::dark_themes()
            .iter()
            .chain(ThemeKind::light_themes())
        {
            let theme = kind.build();
            let p = theme.code_palette;
            for (role, packed) in [
                ("keyword", p.keyword),
                ("operator", p.operator),
                ("string", p.string),
                ("function", p.function),
                ("type", p.type_name),
                ("number", p.number),
                ("variable", p.variable),
                ("comment", p.comment),
                ("foreground", p.foreground),
            ] {
                let color = super::packed_rgb_to_hsla(packed);
                for (surface, background) in [("code", theme.bg_code), ("source", theme.bg_primary)]
                {
                    let ratio = contrast_ratio(color, background);
                    if ratio < 4.5 {
                        failures.push(format!("{} {role}/{surface}: {ratio:.2}", kind.label()));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join(", "));
    }

    #[test]
    fn themed_text_retains_contrast_across_interface_surfaces() {
        let mut failures = Vec::new();
        for kind in ThemeKind::dark_themes()
            .iter()
            .chain(ThemeKind::light_themes())
        {
            let theme = kind.build().with_light_surface_contrast();
            for (role, foreground) in [
                ("primary", theme.text_primary),
                ("secondary", theme.text_secondary),
            ] {
                for (surface, background) in [
                    ("canvas", theme.bg_primary),
                    ("sidebar", theme.bg_secondary),
                    ("elevated", theme.bg_elevated),
                    ("hover", theme.bg_hover),
                    ("selected", theme.bg_selected),
                ] {
                    let ratio = contrast_ratio(foreground, background);
                    if ratio < 4.5 {
                        failures.push(format!("{} {role}/{surface}: {ratio:.2}", kind.label()));
                    }
                }
            }
            for (role, foreground) in [
                ("danger", theme.text_danger),
                ("warning", theme.text_warning),
                ("added", theme.status_success),
                ("modified", theme.status_warning),
            ] {
                for background in [theme.bg_primary, theme.bg_secondary] {
                    let ratio = contrast_ratio(foreground, background);
                    if ratio < 4.5 {
                        failures.push(format!("{} {role}: {ratio:.2}", kind.label()));
                    }
                }
            }
            let ratio = contrast_ratio(theme.text_on_accent, theme.text_accent);
            if ratio < 4.5 {
                failures.push(format!("{} on accent: {ratio:.2}", kind.label()));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join(", "));
    }

    #[test]
    fn light_themes_use_opaque_surfaces_with_clear_chrome_and_inline_code() {
        let mut borders = std::collections::HashSet::new();
        for kind in ThemeKind::light_themes() {
            let theme = kind.build().with_light_surface_contrast();
            for surface in [
                theme.bg_primary,
                theme.bg_secondary,
                theme.bg_elevated,
                theme.bg_code,
                theme.inline_code_bg,
            ] {
                assert_eq!(surface.a, 1.0, "{}", kind.label());
                if surface == crate::hex_hsla!("#FFFFFF") {
                    assert!(
                        matches!(kind, ThemeKind::PaperWhite | ThemeKind::StudioDaylight),
                        "{}",
                        kind.label()
                    );
                } else {
                    assert!(surface.s > 0.01 && surface.l < 1.0, "{}", kind.label());
                }
            }
            borders.insert(super::hsla_to_hex_rgba(theme.border_primary));
            assert!(
                contrast_ratio(theme.bg_secondary, theme.bg_primary) >= 1.08,
                "{}",
                kind.label()
            );
            assert!(
                contrast_ratio(theme.inline_code_bg, theme.bg_primary) >= 1.10,
                "{}",
                kind.label()
            );
        }
        assert_eq!(borders.len(), ThemeKind::light_themes().len());
    }

    #[test]
    fn pure_white_themes_keep_white_canvases_in_both_markdown_color_modes() {
        let white = crate::hex_hsla!("#FFFFFF");
        let white_themes = [ThemeKind::StudioDaylight, ThemeKind::PaperWhite];
        let actual: Vec<_> = ThemeKind::light_themes()
            .iter()
            .copied()
            .filter(|kind| kind.build().bg_primary == white)
            .collect();
        assert_eq!(actual, white_themes);

        for kind in white_themes {
            let base = kind.build().with_light_surface_contrast();
            for &mode in EditorThemeMode::all() {
                let theme = apply_editor_theme_mode(&base, mode, false);
                for surface in [theme.bg_primary, theme.bg_elevated, theme.bg_tab_active] {
                    assert_eq!(surface, white, "{} {mode:?}", kind.label());
                }
                assert_ne!(theme.bg_secondary, white, "{}", kind.label());
                assert_ne!(theme.bg_code, white, "{}", kind.label());
                assert!(theme.code_palette == base.code_palette);
            }
        }
    }

    #[test]
    fn light_themes_cover_seven_hue_families_in_canvas_accent_and_headings() {
        // OKLCH hue ranges describe whole theme identities, not incidental code
        // tokens. Yellow uses pale yellow paper and readable deep-gold ink.
        for (kind, min_hue, max_hue) in [
            (ThemeKind::RoseDawn, 0.0, 35.0),
            (ThemeKind::ApricotBloom, 40.0, 70.0),
            (ThemeKind::SunlitCanvas, 70.0, 110.0),
            (ThemeKind::SageGarden, 130.0, 170.0),
            (ThemeKind::CoastalFog, 175.0, 215.0),
            (ThemeKind::NordicSnow, 240.0, 280.0),
            (ThemeKind::LavenderHaze, 285.0, 320.0),
        ] {
            assert!(ThemeKind::light_themes().contains(&kind));
            let theme = kind.build().with_light_surface_contrast();
            for (role, color, min_chroma) in [
                ("canvas", theme.bg_primary, 0.012),
                ("accent", theme.text_accent, 0.075),
                ("heading", theme.heading, 0.075),
            ] {
                let [_, a, b] = oklab(color);
                let chroma = a.hypot(b);
                let hue = b.atan2(a).to_degrees().rem_euclid(360.0);
                assert!(
                    chroma >= min_chroma && (min_hue..=max_hue).contains(&hue),
                    "{} {role}: chroma {chroma:.4}, hue {hue:.1}",
                    kind.label()
                );
            }
        }
    }

    #[test]
    fn code_block_frames_remain_quiet_in_every_theme() {
        for kind in ThemeKind::dark_themes()
            .iter()
            .chain(ThemeKind::light_themes())
        {
            let theme = kind.build();
            let ratio = contrast_ratio(theme.border_code_block, theme.bg_code);
            assert!(
                (1.08..=1.8).contains(&ratio),
                "{} code frame: {ratio:.2}",
                kind.label()
            );
        }
    }

    fn oklab(color: gpui::Hsla) -> [f32; 3] {
        let (r, g, b) = hsla_to_rgb(color.h, color.s, color.l);
        let linear = |v: f32| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = (linear(r), linear(g), linear(b));
        let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
        let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
        let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
        [
            0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
            1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
            0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
        ]
    }

    #[test]
    fn curated_themes_differ_perceptually_not_just_by_a_few_rgb_bits() {
        let signature = |kind: ThemeKind| {
            let t = kind.build();
            let p = t.code_palette;
            [
                t.bg_primary,
                t.bg_secondary,
                t.text_accent,
                t.heading,
                t.bold,
                t.italic,
                t.inline_code,
                t.link,
                super::packed_rgb_to_hsla(p.keyword),
                super::packed_rgb_to_hsla(p.string),
                super::packed_rgb_to_hsla(p.function),
                super::packed_rgb_to_hsla(p.type_name),
                super::packed_rgb_to_hsla(p.number),
            ]
            .map(oklab)
        };
        for group in [ThemeKind::light_themes(), ThemeKind::dark_themes()] {
            let signatures: Vec<_> = group.iter().copied().map(signature).collect();
            for (i, left) in signatures.iter().enumerate() {
                for (j, right) in signatures.iter().enumerate().skip(i + 1) {
                    let squared: f32 = left
                        .iter()
                        .zip(right)
                        .flat_map(|(a, b)| a.iter().zip(b))
                        .map(|(a, b)| (a - b).powi(2))
                        .sum();
                    let rms = (squared / left.len() as f32).sqrt();
                    assert!(
                        rms >= 0.04,
                        "{} / {} palettes too similar: {rms:.4}",
                        group[i].label(),
                        group[j].label()
                    );
                }
            }
        }
    }

    #[test]
    fn light_code_palettes_differ_in_common_roles_not_just_surface_or_rare_tokens() {
        // Keep the code-only gate separate from the catalog/Markdown signature.
        // These four roles recur in ordinary Rust, JavaScript and Python code;
        // surfaces, comments, types and numeric literals cannot mask similarity.
        let signatures: Vec<_> = ThemeKind::light_themes()
            .iter()
            .map(|kind| {
                let p = kind.build().code_palette;
                [p.keyword, p.operator, p.string, p.function]
                    .map(|color| oklab(super::packed_rgb_to_hsla(color)))
            })
            .collect();
        for (i, left) in signatures.iter().enumerate() {
            for (j, right) in signatures.iter().enumerate().skip(i + 1) {
                let mut squared = 0.0;
                let mut chromatic_squared = 0.0;
                let mut distinct_roles = 0;
                for (a, b) in left.iter().zip(right) {
                    let chromatic = (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
                    let distance = (a[0] - b[0]).powi(2) + chromatic;
                    squared += distance;
                    chromatic_squared += chromatic;
                    if distance >= 0.05_f32.powi(2) {
                        distinct_roles += 1;
                    }
                }
                let rms = (squared / left.len() as f32).sqrt();
                let chromatic_rms = (chromatic_squared / left.len() as f32).sqrt();
                let left_name = ThemeKind::light_themes()[i].label();
                let right_name = ThemeKind::light_themes()[j].label();
                assert!(
                    rms >= 0.08,
                    "{left_name} / {right_name} common code roles: {rms:.4}"
                );
                assert!(
                    chromatic_rms >= 0.065,
                    "{left_name} / {right_name} only differ in lightness: {chromatic_rms:.4}"
                );
                assert!(
                    distinct_roles >= 3,
                    "{left_name} / {right_name}: only {distinct_roles} distinct common roles"
                );
            }
        }
    }

    #[test]
    fn monochrome_mode_uses_grayscale_markers_in_light_theme() {
        let base = ThemeKind::PorcelainMist.build();
        let themed = apply_editor_theme_mode(&base, EditorThemeMode::Monochrome, false);

        assert_eq!(themed.heading, base.text_primary);
        assert_eq!(themed.inline_code, base.text_primary);
        assert_eq!(themed.link, base.link);
        assert_eq!(themed.list_marker, base.text_secondary);
        assert_eq!(themed.syntax_marker, base.text_secondary);
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

/// App-only colors projected from the active editor palette, before contrast
/// adjustments. This preserves the legacy chrome/preview/debugger boundary.
#[derive(Clone, Copy)]
pub struct ApplicationUiPalette {
    pub caption_close_hover_background: Hsla,
    pub caption_close_hover_foreground: Hsla,
    pub status_background: Hsla,
    pub status_title: Hsla,
    pub status_value: Hsla,
    pub status_hover: Hsla,
    pub danger: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub accent: Hsla,
}
impl Theme {
    pub fn application_ui_palette(&self) -> ApplicationUiPalette {
        let light = self.bg_primary.l >= 0.5;
        ApplicationUiPalette {
            caption_close_hover_background: gpui::rgb(0x00c42b1c).into(),
            caption_close_hover_foreground: gpui::white(),
            status_background: if light { self.bg_bar } else { self.bg_elevated },
            status_title: if light {
                with_alpha(self.text_primary, 0.86)
            } else {
                self.text_secondary
            },
            status_value: if light {
                with_alpha(self.text_primary, 0.78)
            } else {
                with_alpha(self.text_secondary, 0.62)
            },
            status_hover: with_alpha(self.text_primary, 0.08),
            danger: self.text_danger,
            success: self.status_success,
            warning: self.status_warning,
            accent: self.text_accent,
        }
    }
}

/// Application self-drawn controls, independent of document text styling.
#[derive(Clone, Copy)]
pub struct SelfDrawnPalette {
    pub editor_scrollbar_thumb_color: Hsla,
    pub code_block_copy_button_bg: Hsla,
    pub code_block_copy_button_text_color: Hsla,
    pub code_block_copy_button_border_color: Hsla,
    pub code_block_copy_button_hover_bg: Hsla,
    pub code_block_copy_button_copied_bg: Hsla,
    pub code_block_copy_button_copied_text_color: Hsla,
    pub code_block_copy_button_copied_border_color: Hsla,
    pub generated_preview_action_border_color: Hsla,
    pub gutter_fold_folded_color: Hsla,
    pub folded_heading_indicator_color: Hsla,
    pub gutter_fold_hover_color: Hsla,
    pub image_edit_icon_color: Hsla,
    pub sidebar_scrollbar_thumb_color: Hsla,
    pub table_handle_neutral: Hsla,
    pub table_handle_indicator: Hsla,
    pub front_matter_type: Hsla,
    pub front_matter_type_hover: Hsla,
    pub front_matter_delete: Hsla,
    pub table_handle_active: Hsla,
    pub pane_divider: Hsla,
    pub pane_divider_active: Hsla,
    pub drop_overlay: Hsla,
}
impl SelfDrawnPalette {
    pub fn new(theme: &Theme, sidebar_theme: &Theme) -> Self {
        Self {
            editor_scrollbar_thumb_color: with_alpha(theme.text_primary, 1.0),
            code_block_copy_button_bg: with_alpha(theme.bg_elevated, 0.92),
            code_block_copy_button_text_color: theme.text_secondary,
            code_block_copy_button_border_color: with_alpha(theme.border_primary, 0.85),
            code_block_copy_button_hover_bg: theme.bg_hover,
            code_block_copy_button_copied_bg: with_alpha(theme.text_accent, 0.14),
            code_block_copy_button_copied_text_color: theme.text_bright,
            code_block_copy_button_copied_border_color: with_alpha(theme.text_accent, 0.45),
            generated_preview_action_border_color: with_alpha(theme.border_primary, 0.38),
            gutter_fold_folded_color: theme.text_bright,
            folded_heading_indicator_color: with_alpha(theme.text_bright, 0.55),
            gutter_fold_hover_color: theme.text_secondary,
            image_edit_icon_color: theme.text_secondary,
            sidebar_scrollbar_thumb_color: with_alpha(sidebar_theme.text_primary, 1.0),
            table_handle_neutral: with_alpha(theme.border_decorative, 0.25),
            table_handle_indicator: theme.cursor,
            front_matter_type: theme.text_secondary,
            front_matter_type_hover: theme.text_accent,
            front_matter_delete: theme.text_secondary,
            table_handle_active: with_alpha(theme.cursor, 0.3),
            pane_divider: theme.border_secondary,
            pane_divider_active: sidebar_theme.text_accent,
            drop_overlay: with_alpha(theme.text_accent, 0.2),
        }
    }
}

/// Optional application-only overrides. A missing value follows the existing
/// debugger source; setting one slot never mutates another slot or the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiColorOverrides {
    values: [Option<Hsla>; 35],
}
impl Default for UiColorOverrides {
    fn default() -> Self {
        Self { values: [None; 35] }
    }
}
impl UiColorOverrides {
    pub fn get(&self, slot: UiColor) -> Option<Hsla> {
        self.values[slot as usize]
    }
    #[cfg(test)]
    pub fn set(&mut self, slot: UiColor, color: Hsla) {
        self.values[slot as usize] = Some(color);
    }
    #[cfg(test)]
    pub fn clear(&mut self, slot: UiColor) {
        self.values[slot as usize] = None;
    }
    pub fn resolve(&self, theme: &Theme, slot: UiColor) -> Hsla {
        self.get(slot).unwrap_or_else(|| theme.ui_color(slot))
    }
    pub fn iter(&self) -> impl Iterator<Item = (UiColor, Hsla)> + '_ {
        UiColor::ALL
            .into_iter()
            .filter_map(|slot| self.get(slot).map(|color| (slot, color)))
    }
}

/// Picker handles use an invariant light edge against arbitrary sampled colors.
pub const COLOR_PICKER_HANDLE: Hsla = hex_to_hsla("#FFFFFFFF");
