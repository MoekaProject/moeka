use crate::theme::ThemeKind;

use super::{SidebarSortMode, config_store};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct AppSettings {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_dark_theme")]
    pub dark_theme: String,
    #[serde(default = "default_light_theme")]
    pub light_theme: String,
    #[serde(default = "default_ui_font")]
    pub ui_font: String,
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
    #[serde(default = "default_body_latin_font")]
    pub body_latin_font: String,
    #[serde(default = "default_body_cjk_font")]
    pub body_cjk_font: String,
    #[serde(default = "default_code_font")]
    pub code_font: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_line_height")]
    pub line_height: f32,
    #[serde(default = "default_letter_spacing")]
    pub letter_spacing: f32,
    #[serde(default = "default_sidebar_sort_mode")]
    pub sidebar_sort_mode: String,
}

fn default_theme() -> String {
    "OneDark".to_string()
}

fn default_dark_theme() -> String {
    "OneDark".to_string()
}

fn default_light_theme() -> String {
    "OneLight".to_string()
}

fn default_ui_font() -> String {
    ".SystemUIFont".to_string()
}

fn default_ui_scale() -> f32 {
    1.0
}

fn default_body_latin_font() -> String {
    "Menlo".to_string()
}

fn default_body_cjk_font() -> String {
    "PingFang SC".to_string()
}

fn default_code_font() -> String {
    "Menlo".to_string()
}

fn default_font_size() -> f32 {
    15.0
}

fn default_line_height() -> f32 {
    1.5
}

fn default_letter_spacing() -> f32 {
    1.0
}

fn default_sidebar_sort_mode() -> String {
    "created_desc".to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            dark_theme: default_dark_theme(),
            light_theme: default_light_theme(),
            ui_font: default_ui_font(),
            ui_scale: default_ui_scale(),
            body_latin_font: default_body_latin_font(),
            body_cjk_font: default_body_cjk_font(),
            code_font: default_code_font(),
            font_size: default_font_size(),
            line_height: default_line_height(),
            letter_spacing: default_letter_spacing(),
            sidebar_sort_mode: default_sidebar_sort_mode(),
        }
    }
}

impl AppSettings {
    pub(crate) fn load() -> Self {
        config_store::load_settings()
    }

    pub(crate) fn save(&self) {
        config_store::save_settings(self);
    }

    pub(crate) fn theme_kind(&self) -> ThemeKind {
        Self::parse_theme_kind(&self.theme).unwrap_or_else(|| {
            if self.theme.eq_ignore_ascii_case("light") {
                self.light_theme_kind()
            } else {
                self.dark_theme_kind()
            }
        })
    }

    pub(crate) fn dark_theme_kind(&self) -> ThemeKind {
        match Self::parse_theme_kind(&self.dark_theme) {
            Some(kind) if kind.is_dark() => kind,
            _ => match Self::parse_theme_kind(&self.theme) {
                Some(kind) if kind.is_dark() => kind,
                _ => ThemeKind::OneDark,
            },
        }
    }

    pub(crate) fn light_theme_kind(&self) -> ThemeKind {
        match Self::parse_theme_kind(&self.light_theme) {
            Some(kind) if !kind.is_dark() => kind,
            _ => match Self::parse_theme_kind(&self.theme) {
                Some(kind) if !kind.is_dark() => kind,
                _ => ThemeKind::OneLight,
            },
        }
    }

    fn parse_theme_kind(value: &str) -> Option<ThemeKind> {
        match value {
            "OneDark" => Some(ThemeKind::OneDark),
            "OneLight" => Some(ThemeKind::OneLight),
            "MilkyDark" | "AyuDark" => Some(ThemeKind::MilkyDark),
            "MilkyLight" | "Milky" | "AyuLight" => Some(ThemeKind::MilkyLight),
            _ => None,
        }
    }

    pub(crate) fn sidebar_sort_mode(&self) -> SidebarSortMode {
        match self.sidebar_sort_mode.as_str() {
            "created_asc" => SidebarSortMode::CreatedAsc,
            "name_asc" => SidebarSortMode::NameAsc,
            "name_desc" => SidebarSortMode::NameDesc,
            "type" => SidebarSortMode::Type,
            _ => SidebarSortMode::CreatedDesc,
        }
    }

    pub(crate) fn set_sidebar_sort_mode(&mut self, mode: SidebarSortMode) {
        self.sidebar_sort_mode = match mode {
            SidebarSortMode::CreatedDesc => "created_desc",
            SidebarSortMode::CreatedAsc => "created_asc",
            SidebarSortMode::NameAsc => "name_asc",
            SidebarSortMode::NameDesc => "name_desc",
            SidebarSortMode::Type => "type",
        }
        .to_string();
    }
}

pub(crate) fn theme_kind_to_string(kind: ThemeKind) -> String {
    match kind {
        ThemeKind::OneDark => "OneDark",
        ThemeKind::OneLight => "OneLight",
        ThemeKind::MilkyDark => "MilkyDark",
        ThemeKind::MilkyLight => "MilkyLight",
    }
    .to_string()
}
