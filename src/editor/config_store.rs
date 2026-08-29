use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::AppSettings;
use super::config_paths;
use super::file_ops;

pub(crate) const CONFIG_SCHEMA_VERSION: u32 = 1;

/// `settings.yaml` — the only file the settings UI reads back and reloads on.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct SettingsFile {
    #[serde(default = "default_schema_version")]
    pub version: u32,
    #[serde(default)]
    pub settings: AppSettings,
}

/// `sessions.yaml` — workspaces plus the restored pane/tab layout.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct SessionsFile {
    #[serde(default = "default_schema_version")]
    pub version: u32,
    #[serde(default)]
    pub workspaces: WorkspaceConfig,
    #[serde(default)]
    pub sessions: SessionConfig,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct WorkspaceConfig {
    #[serde(default)]
    pub roots: Vec<PathBuf>,
    #[serde(default)]
    pub active_root: Option<PathBuf>,
    /// Base64 security-scoped bookmarks keyed by root, so the sandboxed build
    /// can reopen folders the user granted access to in an earlier launch.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bookmarks: BTreeMap<PathBuf, String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct SessionConfig {
    #[serde(default)]
    pub active_tab_id: Option<String>,
    #[serde(default)]
    pub next_tab_id: Option<usize>,
    /// Legacy flat tab list (the focused pane's tabs); kept for forward/
    /// backward compatibility with configs from before split panes.
    #[serde(default)]
    pub tabs: Vec<TabSessionRecord>,
    /// Recursive split-pane layout. When present it supersedes `tabs`.
    #[serde(default)]
    pub pane_layout: Option<PaneLayoutRecord>,
    /// Index of the focused pane within the layout's leaf order.
    #[serde(default)]
    pub focused_pane_leaf: Option<usize>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub(crate) enum PaneLayoutRecord {
    Leaf {
        #[serde(default)]
        tabs: Vec<TabSessionRecord>,
        #[serde(default)]
        active_tab_id: Option<String>,
    },
    Split {
        /// "horizontal" (row) or "vertical" (column).
        axis: String,
        #[serde(default)]
        flexes: Vec<f32>,
        children: Vec<PaneLayoutRecord>,
    },
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct TabSessionRecord {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub file_kind: Option<String>,
    /// Agent CLI identifier for terminal tabs (kind == "terminal").
    #[serde(default)]
    pub agent_cli: Option<String>,
    #[serde(default)]
    pub is_preview: bool,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub view: TabViewState,
    #[serde(default)]
    pub recovery: TabRecoveryState,
    #[serde(default)]
    pub history: TabHistoryIndex,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct TabViewState {
    #[serde(default)]
    pub cursor_offset: usize,
    #[serde(default)]
    pub cursor_anchor: usize,
    #[serde(default)]
    pub scroll_offset: f32,
    #[serde(default)]
    pub editor_left_aligned: bool,
    #[serde(default)]
    pub extra_cursors: Vec<(usize, usize)>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct TabRecoveryState {
    #[serde(default)]
    pub temp_path: Option<PathBuf>,
    #[serde(default)]
    pub source_path: Option<PathBuf>,
    #[serde(default)]
    pub source_mtime: Option<u64>,
    #[serde(default)]
    pub source_content_hash: Option<String>,
    #[serde(default)]
    pub recovered_from_temp: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct TabHistoryIndex {
    #[serde(default)]
    pub undo_log_path: Option<PathBuf>,
    #[serde(default)]
    pub source_identity: Option<String>,
    #[serde(default)]
    pub current_index: Option<usize>,
    #[serde(default)]
    pub entry_count: usize,
}

impl Default for SettingsFile {
    fn default() -> Self {
        Self {
            version: default_schema_version(),
            settings: AppSettings::default(),
        }
    }
}

impl Default for SessionsFile {
    fn default() -> Self {
        Self {
            version: default_schema_version(),
            workspaces: WorkspaceConfig::default(),
            sessions: SessionConfig::default(),
        }
    }
}

pub(crate) fn load_sessions() -> SessionsFile {
    load_yaml_file::<SessionsFile>(&config_paths::sessions_file_path()).unwrap_or_default()
}

pub(crate) fn save_sessions(sessions: &SessionsFile) {
    if let Err(error) = save_yaml_file(&config_paths::sessions_file_path(), sessions) {
        log_error!("failed to save sessions: {error}");
    }
}

pub(crate) fn load_settings() -> AppSettings {
    load_yaml_file::<SettingsFile>(&config_paths::settings_file_path())
        .unwrap_or_default()
        .settings
}

/// Reports the parse error instead of silently falling back, so a hand-edited
/// settings file can be rejected with a message.
pub(crate) fn load_settings_strict() -> Result<AppSettings, String> {
    let path = config_paths::settings_file_path();
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    serde_yaml::from_str::<SettingsFile>(&content)
        .map(|file| file.settings)
        .map_err(|error| error.to_string())
}

pub(crate) fn save_settings(settings: &AppSettings) {
    let file = SettingsFile {
        version: CONFIG_SCHEMA_VERSION,
        settings: settings.clone(),
    };
    if let Err(error) = save_yaml_file(&config_paths::settings_file_path(), &file) {
        log_error!("failed to save settings: {error}");
    }
}

fn default_schema_version() -> u32 {
    CONFIG_SCHEMA_VERSION
}

fn load_yaml_file<T: serde::de::DeserializeOwned + Default>(path: &Path) -> io::Result<T> {
    if !path.exists() {
        return Ok(T::default());
    }

    let content = fs::read_to_string(path)?;
    match serde_yaml::from_str::<T>(&content) {
        Ok(value) => Ok(value),
        Err(error) => {
            log_error!("failed to parse {}: {error}", path.display());
            Ok(T::default())
        }
    }
}

fn save_yaml_file<T: serde::Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let yaml = serde_yaml::to_string(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    file_ops::atomic_write(path, yaml.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    fn sample_settings() -> AppSettings {
        AppSettings {
            theme: "MilkyDark".to_string(),
            theme_mode: "auto".to_string(),
            dark_theme: "MilkyDark".to_string(),
            light_theme: "MilkyLight".to_string(),
            dark_editor_theme_mode: "monochrome".to_string(),
            light_editor_theme_mode: "colorful".to_string(),
            editor_theme_mode: "colorful".to_string(),
            ui_font: "SF Pro".to_string(),
            language: "fr".to_string(),
            ui_scale: 1.2,
            body_latin_font: "IBM Plex Sans".to_string(),
            body_cjk_font: crate::platform::fonts::CJK.to_string(),
            code_font: "JetBrains Mono".to_string(),
            mermaid_theme: "auto".to_string(),
            font_size: 17.0,
            line_height: 1.7,
            letter_spacing: 0.5,
            editor_content_width: 800.0,
            sidebar_sort_mode: "created_desc".to_string(),
            file_list_width: None,
            workspace_search_width: None,
            file_list_visible: true,
            outline_rail_visible: true,
            show_whitespace: false,
            highlight_line_background: true,
            simple_mode: false,
            format_toolbar_visible: true,
            auto_save: false,
            attachment_location: "current_file".to_string(),
            attachment_folder: "attachments".to_string(),
            attachment_external_folder: String::new(),
            attachment_external_folder_bookmark: String::new(),
            terminal_cursor_shape: "bar".to_string(),
            terminal_copy_on_select: false,
            terminal_launch_command: "github_copilot".to_string(),
            terminal_command_args: "copilot --model gpt-5".to_string(),
            terminal_font: String::new(),
            terminal_font_size: 12.0,
            sidebar_always_dark: true,
            file_list_display_mode: "title".to_string(),
            date_time_format: "%Y-%m-%d %H:%M".to_string(),
            starred_files: Vec::new(),
            keybindings: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn load_missing_config_returns_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.yaml");

        let config = load_yaml_file::<SettingsFile>(&path).unwrap();

        assert_eq!(config, SettingsFile::default());
    }

    #[test]
    fn save_and_load_roundtrip_preserves_settings() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.yaml");
        let config = SettingsFile {
            settings: sample_settings(),
            ..SettingsFile::default()
        };

        save_yaml_file(&path, &config).unwrap();
        let loaded = load_yaml_file::<SettingsFile>(&path).unwrap();

        assert_eq!(loaded, config);
    }

    #[test]
    fn sessions_roundtrip_preserves_workspaces() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sessions.yaml");
        let sessions = SessionsFile {
            workspaces: WorkspaceConfig {
                roots: vec![PathBuf::from("/tmp/project")],
                active_root: Some(PathBuf::from("/tmp/project")),
                bookmarks: BTreeMap::from([(PathBuf::from("/tmp/project"), "Ym9va".to_string())]),
            },
            ..SessionsFile::default()
        };

        save_yaml_file(&path, &sessions).unwrap();
        let loaded = load_yaml_file::<SessionsFile>(&path).unwrap();

        assert_eq!(loaded, sessions);
    }

    #[test]
    fn load_partial_yaml_fills_missing_fields_with_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.yaml");
        fs::write(&path, "settings:\n  theme: MilkyDark\n").unwrap();

        let loaded = load_yaml_file::<SettingsFile>(&path).unwrap();

        assert_eq!(loaded.settings.theme, "MilkyDark");
        assert_eq!(
            loaded.settings.dark_theme,
            AppSettings::default().dark_theme
        );
        assert_eq!(
            loaded.settings.light_theme,
            AppSettings::default().light_theme
        );
        assert_eq!(loaded.settings.language, AppSettings::default().language);
        assert_eq!(
            loaded.settings.dark_editor_theme_mode,
            AppSettings::default().dark_editor_theme_mode
        );
        assert_eq!(
            loaded.settings.light_editor_theme_mode,
            AppSettings::default().light_editor_theme_mode
        );
        assert_eq!(loaded.settings.ui_font, AppSettings::default().ui_font);
        assert_eq!(
            loaded.settings.terminal_cursor_shape,
            AppSettings::default().terminal_cursor_shape
        );
        assert!(loaded.settings.terminal_copy_on_select);
        assert_eq!(loaded.version, CONFIG_SCHEMA_VERSION);
    }

    #[test]
    fn invalid_yaml_falls_back_to_default_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.yaml");
        fs::write(&path, "settings: [unterminated").unwrap();

        let loaded = load_yaml_file::<SettingsFile>(&path).unwrap();

        assert_eq!(loaded, SettingsFile::default());
    }

    #[test]
    fn atomic_write_overwrites_existing_file_contents() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.yaml");

        let mut first = SettingsFile::default();
        first.settings.theme = "OneLight".to_string();
        save_yaml_file(&path, &first).unwrap();

        let mut second = SettingsFile::default();
        second.settings.theme = "MilkyLight".to_string();
        second.settings.light_theme = "MilkyLight".to_string();
        save_yaml_file(&path, &second).unwrap();

        let loaded = load_yaml_file::<SettingsFile>(&path).unwrap();
        assert_eq!(loaded.settings.theme, "MilkyLight");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn session_roundtrip_preserves_recovery_and_view_state() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sessions.yaml");
        let source_path = dir.path().join("note.md");
        let temp_path = dir.path().join(".note.md.temp");
        let mut config = SessionsFile::default();
        config.sessions = SessionConfig {
            active_tab_id: Some("tab-7".to_string()),
            next_tab_id: Some(8),
            pane_layout: None,
            focused_pane_leaf: None,
            tabs: vec![TabSessionRecord {
                id: "tab-7".to_string(),
                kind: "editor".to_string(),
                title: "note.md".to_string(),
                path: Some(source_path.clone()),
                file_kind: Some("markdown".to_string()),
                agent_cli: None,
                is_preview: false,
                is_pinned: true,
                view: TabViewState {
                    cursor_offset: 12,
                    cursor_anchor: 4,
                    scroll_offset: 25.5,
                    editor_left_aligned: true,
                    extra_cursors: vec![(9, 3)],
                },
                recovery: TabRecoveryState {
                    temp_path: Some(temp_path.clone()),
                    source_path: Some(source_path),
                    source_mtime: Some(123_456),
                    source_content_hash: Some("abc123".to_string()),
                    recovered_from_temp: true,
                },
                history: TabHistoryIndex {
                    undo_log_path: Some(dir.path().join("history.json")),
                    source_identity: Some("source".to_string()),
                    current_index: Some(2),
                    entry_count: 4,
                },
            }],
        };

        save_yaml_file(&path, &config).unwrap();
        let loaded = load_yaml_file::<SessionsFile>(&path).unwrap();

        assert_eq!(loaded.sessions, config.sessions);
    }
}
