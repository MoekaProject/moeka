use std::path::{Path, PathBuf};

use crate::i18n::{UiLanguage, default_language_code};
use crate::theme::{EditorThemeMode, Theme, ThemeKind};

use super::{
    ChatAgentCli, FileListDisplayMode, SidebarSortMode, TerminalCursorShape, config_store,
};

/// How the light/dark appearance is chosen. `Manual` keeps whatever theme the
/// user last picked; `Auto` mirrors the OS appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThemeMode {
    Auto,
    Manual,
}

impl ThemeMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "manual" | "light" | "dark" => Some(Self::Manual),
            _ => None,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct AppSettings {
    #[serde(default = "default_theme")]
    pub theme: String,
    /// `auto` follows the system appearance; `light`/`dark` pin it.
    #[serde(default = "default_stored_theme_mode")]
    pub theme_mode: String,
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
    #[serde(default = "default_file_list_display_mode")]
    pub file_list_display_mode: String,
    #[serde(default)]
    pub file_list_width: Option<f32>,
    #[serde(default)]
    pub workspace_search_width: Option<f32>,
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
    pub default_left_align: bool,
    #[serde(default = "default_simple_mode")]
    pub simple_mode: bool,
    #[serde(default)]
    pub auto_save: bool,
    #[serde(default = "default_attachment_location")]
    pub attachment_location: String,
    #[serde(default = "default_attachment_folder")]
    pub attachment_folder: String,
    #[serde(default = "default_link_syntax")]
    pub link_syntax: String,
    #[serde(default = "default_link_path_style")]
    pub link_path_style: String,
    #[serde(default)]
    pub attachment_external_folder: String,
    #[serde(default)]
    pub attachment_external_folder_bookmark: String,
    #[serde(default = "default_terminal_cursor_shape")]
    pub terminal_cursor_shape: String,
    #[serde(default = "default_terminal_copy_on_select")]
    pub terminal_copy_on_select: bool,
    #[serde(default = "default_terminal_launch_command")]
    pub terminal_launch_command: String,
    #[serde(default = "default_terminal_command_args")]
    pub terminal_command_args: String,
    /// Terminal font family; empty means "follow the editor code font".
    #[serde(default)]
    pub terminal_font: String,
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: f32,
    #[serde(default = "default_sidebar_always_dark")]
    pub sidebar_always_dark: bool,
    /// User keybinding overrides: command id (see `CommandId::config_key`) to
    /// gpui keystroke string, e.g. `save: cmd-shift-s`.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub keybindings: std::collections::BTreeMap<String, String>,
}

fn default_theme() -> String {
    "One Dark".to_string()
}

/// Settings files written before the auto mode existed keep their explicit
/// light/dark choice; only fresh installs (see `Default`) start on auto.
fn default_stored_theme_mode() -> String {
    ThemeMode::Manual.as_str().to_string()
}

fn default_dark_theme() -> String {
    "One Dark".to_string()
}

fn default_light_theme() -> String {
    "Porcelain Mist".to_string()
}

fn default_ui_font() -> String {
    crate::platform::fonts::MONOSPACE.to_string()
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
    1.2
}

fn default_body_latin_font() -> String {
    crate::platform::fonts::MONOSPACE.to_string()
}

fn default_body_cjk_font() -> String {
    crate::platform::fonts::MONOSPACE.to_string()
}

fn default_code_font() -> String {
    crate::platform::fonts::MONOSPACE.to_string()
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

fn default_file_list_display_mode() -> String {
    "title".to_string()
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

fn default_simple_mode() -> bool {
    true
}

fn default_attachment_location() -> String {
    "workspace_folder".to_string()
}

fn default_attachment_folder() -> String {
    "attachments".to_string()
}

fn default_link_syntax() -> String {
    "markdown".to_string()
}

fn default_link_path_style() -> String {
    "relative".to_string()
}

fn default_terminal_cursor_shape() -> String {
    TerminalCursorShape::default().as_str().to_string()
}

fn default_terminal_copy_on_select() -> bool {
    true
}

fn default_terminal_launch_command() -> String {
    ChatAgentCli::Terminal.settings_value().to_string()
}

fn default_terminal_command_args() -> String {
    ChatAgentCli::Terminal.command_name().to_string()
}

fn default_terminal_font_size() -> f32 {
    12.0
}

fn default_sidebar_always_dark() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            theme_mode: ThemeMode::Auto.as_str().to_string(),
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
            file_list_display_mode: default_file_list_display_mode(),
            file_list_width: None,
            workspace_search_width: None,
            file_list_visible: default_file_list_visible(),
            date_time_format: default_date_time_format(),
            starred_files: default_starred_files(),
            outline_rail_visible: default_outline_rail_visible(),
            show_whitespace: default_show_whitespace(),
            highlight_line_background: default_highlight_line_background(),
            default_left_align: false,
            simple_mode: default_simple_mode(),
            auto_save: false,
            attachment_location: default_attachment_location(),
            attachment_folder: default_attachment_folder(),
            link_syntax: default_link_syntax(),
            link_path_style: default_link_path_style(),
            attachment_external_folder: String::new(),
            attachment_external_folder_bookmark: String::new(),
            terminal_cursor_shape: default_terminal_cursor_shape(),
            terminal_copy_on_select: default_terminal_copy_on_select(),
            terminal_launch_command: default_terminal_launch_command(),
            terminal_command_args: default_terminal_command_args(),
            terminal_font: String::new(),
            terminal_font_size: default_terminal_font_size(),
            sidebar_always_dark: default_sidebar_always_dark(),
            keybindings: std::collections::BTreeMap::new(),
        }
    }
}

impl AppSettings {
    pub(crate) fn effective_editor_font_size(&self) -> f32 {
        (self.font_size - 1.0).clamp(9.0, 29.0)
    }

    pub(crate) fn effective_terminal_font_size(&self) -> f32 {
        self.terminal_font_size.clamp(8.0, 32.0)
    }

    pub(crate) fn effective_editor_content_width(&self) -> f32 {
        self.editor_content_width.round().clamp(320.0, 1600.0)
    }

    pub(crate) fn terminal_cursor_shape(&self) -> TerminalCursorShape {
        TerminalCursorShape::from_str(&self.terminal_cursor_shape).unwrap_or_default()
    }

    pub(crate) fn terminal_launch_cli(&self) -> ChatAgentCli {
        ChatAgentCli::from_settings_value(&self.terminal_launch_command)
            .unwrap_or(ChatAgentCli::Terminal)
    }

    pub(crate) fn attachment_policy(&self) -> super::AttachmentPolicy {
        super::AttachmentPolicy::from_settings(
            &self.attachment_location,
            &self.attachment_folder,
            &self.attachment_external_folder,
            &self.link_syntax,
            &self.link_path_style,
            false,
        )
    }

    pub(crate) fn workspace_link_policy(&self) -> super::AttachmentPolicy {
        super::AttachmentPolicy::from_settings(
            "workspace_folder",
            &self.attachment_folder,
            "",
            &self.link_syntax,
            &self.link_path_style,
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

    pub(crate) fn theme_mode(&self) -> ThemeMode {
        ThemeMode::from_str(&self.theme_mode).unwrap_or(ThemeMode::Manual)
    }

    pub(crate) fn ui_language(&self) -> UiLanguage {
        UiLanguage::from_code(&self.language).unwrap_or_else(crate::i18n::detect_system_language)
    }

    pub(crate) fn ui_theme_for(&self, editor_theme_kind: ThemeKind) -> Theme {
        editor_theme_kind.build()
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
                _ => ThemeKind::PorcelainMist,
            },
        }
    }

    fn parse_theme_kind(value: &str) -> Option<ThemeKind> {
        ThemeKind::dark_themes()
            .iter()
            .chain(ThemeKind::light_themes())
            .copied()
            .find(|kind| kind.label() == value)
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

    pub(crate) fn file_list_display_mode(&self) -> FileListDisplayMode {
        match self.file_list_display_mode.as_str() {
            "file_name" => FileListDisplayMode::FileName,
            _ => FileListDisplayMode::Title,
        }
    }

    pub(crate) fn set_file_list_display_mode(&mut self, mode: FileListDisplayMode) {
        self.file_list_display_mode = match mode {
            FileListDisplayMode::Title => "title",
            FileListDisplayMode::FileName => "file_name",
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

    /// An unavailable mount is not evidence of deletion. Only remove duplicate
    /// records here; confirmed deletes/renames use the explicit prefix helpers.
    #[cfg(test)]
    pub(crate) fn prune_missing_starred_files(&mut self) -> bool {
        let original_len = self.starred_files.len();
        let mut seen = std::collections::HashSet::new();
        self.starred_files.retain(|path| seen.insert(path.clone()));
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
    kind.label().to_string()
}

#[cfg(test)]
mod tests {
    use super::{AppSettings, theme_kind_to_string};
    use crate::editor::attachments::{AttachmentLinkSyntax, AttachmentPathStyle};
    use crate::editor::{ChatAgentCli, TerminalCursorShape};
    use crate::theme::{EditorThemeMode, ThemeKind};

    #[test]
    fn starred_pruning_preserves_unknown_offline_paths_and_only_removes_duplicates() {
        let mut settings = AppSettings::default();
        let unknown = std::path::PathBuf::from("offline-workspace/unavailable.md");
        let known = std::path::PathBuf::from("workspace/note.md");
        settings.starred_files = vec![unknown.clone(), known.clone(), unknown.clone()];
        assert!(settings.prune_missing_starred_files());
        assert_eq!(settings.starred_files, vec![unknown, known]);
        assert!(!settings.prune_missing_starred_files());
    }

    #[test]
    fn default_left_align_is_off_for_existing_settings_and_roundtrips_when_enabled() {
        let mut json = serde_json::to_value(AppSettings::default()).unwrap();
        json.as_object_mut().unwrap().remove("default_left_align");
        let mut settings: AppSettings = serde_json::from_value(json).unwrap();
        assert!(!settings.default_left_align);
        settings.default_left_align = true;
        let restored: AppSettings =
            serde_json::from_value(serde_json::to_value(settings).unwrap()).unwrap();
        assert!(restored.default_left_align);
    }

    #[test]
    fn curated_theme_names_round_trip() {
        for kind in ThemeKind::dark_themes()
            .iter()
            .chain(ThemeKind::light_themes())
            .copied()
        {
            assert_eq!(
                AppSettings::parse_theme_kind(&theme_kind_to_string(kind)),
                Some(kind)
            );
        }
        assert_eq!(ThemeKind::dark_themes().len(), 16);
        assert_eq!(ThemeKind::light_themes().len(), 16);
        assert!(
            ThemeKind::dark_themes()
                .iter()
                .chain(ThemeKind::light_themes())
                .all(|kind| kind.label().split_whitespace().count() >= 2)
        );
    }

    #[test]
    fn every_curated_theme_drives_the_complete_ui() {
        let settings = AppSettings::default();
        for kind in ThemeKind::dark_themes()
            .iter()
            .chain(ThemeKind::light_themes())
            .copied()
        {
            let editor = kind.build();
            let ui = settings.ui_theme_for(kind);
            assert_eq!(ui.bg_primary, editor.bg_primary, "{}", kind.label());
            assert_eq!(ui.bg_secondary, editor.bg_secondary, "{}", kind.label());
            assert_eq!(ui.text_primary, editor.text_primary, "{}", kind.label());
        }
    }

    #[test]
    fn default_fonts_all_use_the_platform_monospace_family() {
        let settings = AppSettings::default();
        let expected = crate::platform::fonts::MONOSPACE;

        assert_eq!(settings.ui_font, expected);
        assert_eq!(settings.body_latin_font, expected);
        assert_eq!(settings.body_cjk_font, expected);
        assert_eq!(settings.code_font, expected);
    }

    #[test]
    fn defaults_enable_simple_mode_and_use_workspace_attachments_folder() {
        let settings = AppSettings::default();

        assert!(settings.simple_mode);
        assert_eq!(settings.attachment_location, "workspace_folder");
        assert_eq!(settings.attachment_folder, "attachments");

        let missing_fields: AppSettings = serde_yaml::from_str("{}\n").expect("default settings");
        assert!(missing_fields.simple_mode);
        assert_eq!(missing_fields.attachment_location, "workspace_folder");
        assert_eq!(missing_fields.attachment_folder, "attachments");
        assert_eq!(missing_fields.link_syntax, "markdown");
        assert_eq!(missing_fields.link_path_style, "relative");
    }

    #[test]
    fn removed_mermaid_settings_are_ignored_and_not_saved() {
        let settings: AppSettings =
            serde_yaml::from_str("mermaid_style: editorial\nmermaid_theme: mermaid-dark\n")
                .unwrap();
        let yaml = serde_yaml::to_string(&settings).unwrap();
        assert!(!yaml.contains("mermaid_style"));
        assert!(!yaml.contains("mermaid_theme"));
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
    fn auto_save_defaults_to_disabled() {
        assert!(!AppSettings::default().auto_save);
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
    fn terminal_launch_command_defaults_and_rejects_unknown_values() {
        let default_settings = AppSettings::default();
        assert_eq!(
            default_settings.terminal_launch_cli(),
            ChatAgentCli::Terminal
        );
        assert!(!default_settings.terminal_command_args.is_empty());

        for agent_cli in ChatAgentCli::all() {
            let settings = AppSettings {
                terminal_launch_command: agent_cli.settings_value().to_string(),
                ..AppSettings::default()
            };
            assert_eq!(settings.terminal_launch_cli(), agent_cli);
        }

        let invalid = AppSettings {
            terminal_launch_command: "unknown".to_string(),
            ..AppSettings::default()
        };
        assert_eq!(invalid.terminal_launch_cli(), ChatAgentCli::Terminal);
    }

    #[test]
    fn attachment_policy_respects_link_settings_and_preserves_defaults() {
        let policy = AppSettings::default().attachment_policy();

        assert_eq!(policy.link_syntax, AttachmentLinkSyntax::Markdown);
        assert_eq!(policy.path_style, AttachmentPathStyle::Relative);
        assert!(!policy.automatically_update_links);
        let wiki = AppSettings {
            link_syntax: "wikilink".into(),
            link_path_style: "shortest".into(),
            ..AppSettings::default()
        }
        .attachment_policy();
        assert_eq!(wiki.link_syntax, AttachmentLinkSyntax::WikiLink);
        assert_eq!(wiki.path_style, AttachmentPathStyle::Shortest);

        let external = AppSettings {
            attachment_location: "external_folder".into(),
            link_syntax: "wikilink".into(),
            link_path_style: "shortest".into(),
            ..AppSettings::default()
        };
        assert_eq!(
            external.attachment_policy().link_syntax,
            AttachmentLinkSyntax::Markdown
        );
        assert_eq!(
            external.workspace_link_policy().link_syntax,
            AttachmentLinkSyntax::WikiLink
        );
        assert_eq!(
            external.workspace_link_policy().path_style,
            AttachmentPathStyle::Shortest
        );
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
            theme: "One Dark".to_string(),
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
