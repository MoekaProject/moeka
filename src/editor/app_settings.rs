use crate::theme::ThemeKind;

use super::config_store;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct AppSettings {
    #[serde(default = "default_theme")]
    pub theme: String,
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
}

fn default_theme() -> String {
    "OneDark".to_string()
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

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            ui_font: default_ui_font(),
            ui_scale: default_ui_scale(),
            body_latin_font: default_body_latin_font(),
            body_cjk_font: default_body_cjk_font(),
            code_font: default_code_font(),
            font_size: default_font_size(),
            line_height: default_line_height(),
            letter_spacing: default_letter_spacing(),
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
        match self.theme.as_str() {
            "OneLight" => ThemeKind::OneLight,
            "AyuDark" => ThemeKind::AyuDark,
            "AyuLight" => ThemeKind::AyuLight,
            "Milky" => ThemeKind::Milky,
            "MilkyDark" => ThemeKind::MilkyDark,
            _ => ThemeKind::OneDark,
        }
    }
}

pub(crate) fn theme_kind_to_string(kind: ThemeKind) -> String {
    match kind {
        ThemeKind::OneDark => "OneDark",
        ThemeKind::OneLight => "OneLight",
        ThemeKind::AyuDark => "AyuDark",
        ThemeKind::AyuLight => "AyuLight",
        ThemeKind::Milky => "Milky",
        ThemeKind::MilkyDark => "MilkyDark",
    }
    .to_string()
}
