use std::path::{Path, PathBuf};

use crate::i18n::{UiLanguage, default_language_code};
use crate::theme::{EditorThemeMode, ThemeKind};

use super::{SidebarSortMode, TerminalCursorShape, config_store};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct AppSettings {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_dark_theme")]
    pub dark_theme: String,
    #[serde(default = "default_light_theme")]
    pub light_theme: String,
    #[serde(default = "default_dark_editor_theme_mode")]
    pub dark_editor_theme_mode: String,
    #[serde(default = "default_light_editor_theme_mode")]
    pub light_editor_theme_mode: String,
    #[serde(default = "default_editor_theme_mode")]
    pub editor_theme_mode: String,
    #[serde(default = "default_ui_font")]
    pub ui_font: String,
    #[serde(default = "default_language")]
    pub language: String,
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
    #[serde(default = "default_editor_content_width")]
    pub editor_content_width: f32,
    #[serde(default = "default_sidebar_sort_mode")]
    pub sidebar_sort_mode: String,
    #[serde(default)]
    pub file_list_width: Option<f32>,
    #[serde(default = "default_file_list_visible")]
    pub file_list_visible: bool,
    #[serde(default = "default_date_time_format")]
    pub date_time_format: String,
    #[serde(default = "default_starred_files")]
    pub starred_files: Vec<PathBuf>,
    #[serde(default = "default_outline_rail_visible")]
    pub outline_rail_visible: bool,
    #[serde(default = "default_show_whitespace")]
    pub show_whitespace: bool,
    #[serde(default = "default_highlight_line_background")]
    pub highlight_line_background: bool,
    #[serde(default)]
    pub simple_mode: bool,
    #[serde(default = "default_attachment_location")]
    pub attachment_location: String,
    #[serde(default = "default_attachment_folder")]
    pub attachment_folder: String,
    #[serde(default = "default_terminal_cursor_shape")]
    pub terminal_cursor_shape: String,
    #[serde(default = "default_terminal_copy_on_select")]
    pub terminal_copy_on_select: bool,
    /// User keybinding overrides: command id (see `CommandId::config_key`) to
    /// gpui keystroke string, e.g. `save: cmd-shift-s`.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub keybindings: std::collections::BTreeMap<String, String>,
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
    "Menlo".to_string()
}

fn default_language() -> String {
    default_language_code()
}

fn default_editor_theme_mode() -> String {
    EditorThemeMode::Colorful.as_str().to_string()
}

fn default_dark_editor_theme_mode() -> String {
    EditorThemeMode::Colorful.as_str().to_string()
}

fn default_light_editor_theme_mode() -> String {
    EditorThemeMode::Colorful.as_str().to_string()
}

fn default_ui_scale() -> f32 {
    1.0
}

fn default_body_latin_font() -> String {
    "Menlo".to_string()
}

fn default_body_cjk_font() -> String {
    "Menlo".to_string()
}

fn default_code_font() -> String {
    "Menlo".to_string()
}

fn default_font_size() -> f32 {
    15.0
}

fn default_line_height() -> f32 {
    1.7
}

fn default_letter_spacing() -> f32 {
    1.0
}

fn default_editor_content_width() -> f32 {
    700.0
}

fn default_sidebar_sort_mode() -> String {
    "created_desc".to_string()
}

fn default_date_time_format() -> String {
    "%Y-%m-%d %H:%M".to_string()
}

fn default_file_list_visible() -> bool {
    true
}

fn default_starred_files() -> Vec<PathBuf> {
    Vec::new()
}

fn default_outline_rail_visible() -> bool {
    true
}

fn default_show_whitespace() -> bool {
    false
}

fn default_highlight_line_background() -> bool {
    true
}

fn default_attachment_location() -> String {
    "current_file".to_string()
}

fn default_attachment_folder() -> String {
    "attachments".to_string()
}

fn default_terminal_cursor_shape() -> String {
    TerminalCursorShape::default().as_str().to_string()
}

fn default_terminal_copy_on_select() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            dark_theme: default_dark_theme(),
            light_theme: default_light_theme(),
            dark_editor_theme_mode: default_dark_editor_theme_mode(),
            light_editor_theme_mode: default_light_editor_theme_mode(),
            editor_theme_mode: default_editor_theme_mode(),
            ui_font: default_ui_font(),
            language: default_language(),
            ui_scale: default_ui_scale(),
            body_latin_font: default_body_latin_font(),
            body_cjk_font: default_body_cjk_font(),
            code_font: default_code_font(),
            font_size: default_font_size(),
            line_height: default_line_height(),
            letter_spacing: default_letter_spacing(),
            editor_content_width: default_editor_content_width(),
            sidebar_sort_mode: default_sidebar_sort_mode(),
            file_list_width: None,
            file_list_visible: default_file_list_visible(),
            date_time_format: default_date_time_format(),
            starred_files: default_starred_files(),
            outline_rail_visible: default_outline_rail_visible(),
            show_whitespace: default_show_whitespace(),
            highlight_line_background: default_highlight_line_background(),
            simple_mode: false,
            attachment_location: default_attachment_location(),
            attachment_folder: default_attachment_folder(),
            terminal_cursor_shape: default_terminal_cursor_shape(),
            terminal_copy_on_select: default_terminal_copy_on_select(),
            keybindings: std::collections::BTreeMap::new(),
        }
    }
}

impl AppSettings {
    pub(crate) fn effective_editor_font_size(&self) -> f32 {
        (self.font_size - 1.0).clamp(9.0, 29.0)
    }

    pub(crate) fn effective_editor_content_width(&self) -> f32 {
        self.editor_content_width.round().clamp(320.0, 1600.0)
    }

    pub(crate) fn terminal_cursor_shape(&self) -> TerminalCursorShape {
        TerminalCursorShape::from_str(&self.terminal_cursor_shape).unwrap_or_default()
    }

    pub(crate) fn attachment_policy(&self) -> super::AttachmentPolicy {
        super::AttachmentPolicy::from_settings(
            &self.attachment_location,
            &self.attachment_folder,
            "markdown",
            "relative",
            false,
        )
    }

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

    pub(crate) fn ui_language(&self) -> UiLanguage {
        UiLanguage::from_code(&self.language).unwrap_or_else(crate::i18n::detect_system_language)
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
            "MilkyLight" | "Milky" => Some(ThemeKind::MilkyLight),
            "Dracula" => Some(ThemeKind::Dracula),
            "Nord" => Some(ThemeKind::Nord),
            "TokyoNight" => Some(ThemeKind::TokyoNight),
            "CatppuccinMocha" => Some(ThemeKind::CatppuccinMocha),
            "GithubDark" | "GitHubDark" => Some(ThemeKind::GithubDark),
            "SolarizedDark" => Some(ThemeKind::SolarizedDark),
            "GruvboxDark" => Some(ThemeKind::GruvboxDark),
            "RosePine" => Some(ThemeKind::RosePine),
            "EverforestDark" => Some(ThemeKind::EverforestDark),
            "MonokaiPro" => Some(ThemeKind::MonokaiPro),
            "GithubLight" | "GitHubLight" => Some(ThemeKind::GithubLight),
            "SolarizedLight" => Some(ThemeKind::SolarizedLight),
            "CatppuccinLatte" => Some(ThemeKind::CatppuccinLatte),
            "RosePineDawn" => Some(ThemeKind::RosePineDawn),
            "GruvboxLight" => Some(ThemeKind::GruvboxLight),
            "EverforestLight" => Some(ThemeKind::EverforestLight),
            "TokyoNightLight" => Some(ThemeKind::TokyoNightLight),
            "NordLight" => Some(ThemeKind::NordLight),
            "AyuLight" => Some(ThemeKind::AyuLight),
            "XcodeLight" => Some(ThemeKind::XcodeLight),
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

    pub(crate) fn editor_theme_mode(&self) -> EditorThemeMode {
        EditorThemeMode::from_str(&self.editor_theme_mode).unwrap_or(EditorThemeMode::Colorful)
    }

    pub(crate) fn dark_editor_theme_mode(&self) -> EditorThemeMode {
        EditorThemeMode::from_str(&self.dark_editor_theme_mode)
            .or_else(|| {
                self.theme_kind()
                    .is_dark()
                    .then(|| self.editor_theme_mode())
            })
            .unwrap_or(EditorThemeMode::Colorful)
    }

    pub(crate) fn light_editor_theme_mode(&self) -> EditorThemeMode {
        EditorThemeMode::from_str(&self.light_editor_theme_mode)
            .or_else(|| (!self.theme_kind().is_dark()).then(|| self.editor_theme_mode()))
            .unwrap_or(EditorThemeMode::Colorful)
    }

    pub(crate) fn editor_theme_mode_for_appearance(&self, is_dark: bool) -> EditorThemeMode {
        if is_dark {
            self.dark_editor_theme_mode()
        } else {
            self.light_editor_theme_mode()
        }
    }

    pub(crate) fn set_editor_theme_mode_for_appearance(
        &mut self,
        is_dark: bool,
        mode: EditorThemeMode,
    ) {
        if is_dark {
            self.dark_editor_theme_mode = mode.as_str().to_string();
        } else {
            self.light_editor_theme_mode = mode.as_str().to_string();
        }
        self.editor_theme_mode = mode.as_str().to_string();
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

    pub(crate) fn date_time_format(&self) -> &str {
        if self.date_time_format.trim().is_empty() {
            "%Y-%m-%d %H:%M"
        } else {
            &self.date_time_format
        }
    }

    pub(crate) fn set_date_time_format(&mut self, format: String) {
        self.date_time_format = format;
    }

    pub(crate) fn is_starred_file(&self, path: &Path) -> bool {
        self.starred_files.iter().any(|candidate| candidate == path)
    }

    pub(crate) fn add_starred_file(&mut self, path: PathBuf) {
        self.starred_files.retain(|candidate| candidate != &path);
        self.starred_files.insert(0, path);
    }

    pub(crate) fn remove_starred_file(&mut self, path: &Path) -> bool {
        let original_len = self.starred_files.len();
        self.starred_files.retain(|candidate| candidate != path);
        self.starred_files.len() != original_len
    }

    pub(crate) fn prune_missing_starred_files(&mut self) -> bool {
        let original_len = self.starred_files.len();
        self.starred_files
            .retain(|path| path.exists() && path.is_file());
        self.starred_files.len() != original_len
    }

    pub(crate) fn rename_starred_prefix(&mut self, from: &Path, to: &Path) -> bool {
        let mut changed = false;
        for path in &mut self.starred_files {
            if path == from {
                *path = to.to_path_buf();
                changed = true;
                continue;
            }

            if let Ok(relative) = path.strip_prefix(from) {
                *path = to.join(relative);
                changed = true;
            }
        }

        if changed {
            let mut deduped = Vec::with_capacity(self.starred_files.len());
            for path in self.starred_files.drain(..) {
                if !deduped.iter().any(|candidate| candidate == &path) {
                    deduped.push(path);
                }
            }
            self.starred_files = deduped;
        }

        changed
    }

    pub(crate) fn remove_starred_prefix(&mut self, prefix: &Path) -> bool {
        let original_len = self.starred_files.len();
        self.starred_files.retain(|path| !path.starts_with(prefix));
        self.starred_files.len() != original_len
    }
}

pub(crate) fn theme_kind_to_string(kind: ThemeKind) -> String {
    match kind {
        ThemeKind::OneDark => "OneDark",
        ThemeKind::OneLight => "OneLight",
        ThemeKind::MilkyDark => "MilkyDark",
        ThemeKind::MilkyLight => "MilkyLight",
        ThemeKind::Dracula => "Dracula",
        ThemeKind::Nord => "Nord",
        ThemeKind::TokyoNight => "TokyoNight",
        ThemeKind::CatppuccinMocha => "CatppuccinMocha",
        ThemeKind::GithubDark => "GithubDark",
        ThemeKind::SolarizedDark => "SolarizedDark",
        ThemeKind::GruvboxDark => "GruvboxDark",
        ThemeKind::RosePine => "RosePine",
        ThemeKind::EverforestDark => "EverforestDark",
        ThemeKind::MonokaiPro => "MonokaiPro",
        ThemeKind::GithubLight => "GithubLight",
        ThemeKind::SolarizedLight => "SolarizedLight",
        ThemeKind::CatppuccinLatte => "CatppuccinLatte",
        ThemeKind::RosePineDawn => "RosePineDawn",
        ThemeKind::GruvboxLight => "GruvboxLight",
        ThemeKind::EverforestLight => "EverforestLight",
        ThemeKind::TokyoNightLight => "TokyoNightLight",
        ThemeKind::NordLight => "NordLight",
        ThemeKind::AyuLight => "AyuLight",
        ThemeKind::XcodeLight => "XcodeLight",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::AppSettings;
    use crate::editor::TerminalCursorShape;
    use crate::editor::attachments::{AttachmentLinkSyntax, AttachmentPathStyle};
    use crate::theme::EditorThemeMode;

    #[test]
    fn default_fonts_are_all_menlo() {
        let settings = AppSettings::default();

        assert_eq!(settings.ui_font, "Menlo");
        assert_eq!(settings.body_latin_font, "Menlo");
        assert_eq!(settings.body_cjk_font, "Menlo");
        assert_eq!(settings.code_font, "Menlo");
    }

    #[test]
    fn editor_body_font_size_is_one_point_smaller_than_the_legacy_setting() {
        let mut settings = AppSettings::default();
        assert_eq!(settings.font_size, 15.0);
        assert_eq!(settings.effective_editor_font_size(), 14.0);

        settings.font_size = 20.0;
        assert_eq!(settings.effective_editor_font_size(), 19.0);
    }

    #[test]
    fn editor_content_width_defaults_to_700() {
        let mut settings = AppSettings::default();
        assert_eq!(settings.editor_content_width, 700.0);
        assert_eq!(settings.effective_editor_content_width(), 700.0);

        settings.editor_content_width = 100.0;
        assert_eq!(settings.effective_editor_content_width(), 320.0);

        settings.editor_content_width = 2000.0;
        assert_eq!(settings.effective_editor_content_width(), 1600.0);
    }

    #[test]
    fn terminal_cursor_shape_defaults_to_block_and_rejects_unknown_values() {
        let default_settings = AppSettings::default();
        assert_eq!(
            default_settings.terminal_cursor_shape(),
            TerminalCursorShape::Block
        );
        assert!(default_settings.terminal_copy_on_select);

        for shape in TerminalCursorShape::ALL {
            let settings = AppSettings {
                terminal_cursor_shape: shape.as_str().to_string(),
                ..AppSettings::default()
            };
            assert_eq!(settings.terminal_cursor_shape(), shape);
        }

        let invalid = AppSettings {
            terminal_cursor_shape: "unexpected".to_string(),
            ..AppSettings::default()
        };
        assert_eq!(invalid.terminal_cursor_shape(), TerminalCursorShape::Block);
    }

    #[test]
    fn attachment_policy_uses_fixed_markdown_relative_behavior() {
        let policy = AppSettings::default().attachment_policy();

        assert_eq!(policy.link_syntax, AttachmentLinkSyntax::Markdown);
        assert_eq!(policy.path_style, AttachmentPathStyle::Relative);
        assert!(!policy.automatically_update_links);
    }

    #[test]
    fn editor_theme_mode_defaults_to_colorful() {
        assert_eq!(
            AppSettings::default().editor_theme_mode(),
            EditorThemeMode::Colorful
        );
        assert_eq!(
            AppSettings::default().dark_editor_theme_mode(),
            EditorThemeMode::Colorful
        );
        assert_eq!(
            AppSettings::default().light_editor_theme_mode(),
            EditorThemeMode::Colorful
        );
    }

    #[test]
    fn invalid_editor_theme_mode_falls_back_to_colorful() {
        let settings = AppSettings {
            editor_theme_mode: "unexpected".to_string(),
            ..AppSettings::default()
        };

        assert_eq!(settings.editor_theme_mode(), EditorThemeMode::Colorful);
    }

    #[test]
    fn appearance_specific_editor_theme_mode_uses_matching_slot() {
        let settings = AppSettings {
            theme: "OneDark".to_string(),
            dark_editor_theme_mode: "monochrome".to_string(),
            light_editor_theme_mode: "colorful".to_string(),
            ..AppSettings::default()
        };

        assert_eq!(
            settings.editor_theme_mode_for_appearance(true),
            EditorThemeMode::Monochrome
        );
        assert_eq!(
            settings.editor_theme_mode_for_appearance(false),
            EditorThemeMode::Colorful
        );
    }
}
