use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::AppSettings;
use super::config_paths;
use super::file_ops;

pub(crate) const CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct AppConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub settings: AppSettings,
    #[serde(default)]
    pub workspaces: WorkspaceConfig,
    #[serde(default)]
    pub session: SessionConfig,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct WorkspaceConfig {
    #[serde(default)]
    pub roots: Vec<PathBuf>,
    #[serde(default)]
    pub active_root: Option<PathBuf>,
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

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: default_schema_version(),
            settings: AppSettings::default(),
            workspaces: WorkspaceConfig::default(),
            session: SessionConfig::default(),
        }
    }
}

pub(crate) fn load_config() -> AppConfig {
    load_config_from_path(&config_paths::config_file_path()).unwrap_or_default()
}

pub(crate) fn save_config(config: &AppConfig) {
    if let Err(error) = save_config_to_path(&config_paths::config_file_path(), config) {
        eprintln!("failed to save config: {error}");
    }
}

pub(crate) fn load_settings() -> AppSettings {
    load_config().settings
}

pub(crate) fn save_settings(settings: &AppSettings) {
    let mut config = load_config();
    config.settings = settings.clone();
    save_config(&config);
}

fn default_schema_version() -> u32 {
    CONFIG_SCHEMA_VERSION
}

fn load_config_from_path(path: &Path) -> io::Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let content = fs::read_to_string(path)?;
    match serde_yaml::from_str::<AppConfig>(&content) {
        Ok(config) => Ok(config),
        Err(error) => {
            eprintln!("failed to parse config yaml: {error}");
            Ok(AppConfig::default())
        }
    }
}

fn save_config_to_path(path: &Path, config: &AppConfig) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let yaml = serde_yaml::to_string(config)
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
            dark_theme: "MilkyDark".to_string(),
            light_theme: "MilkyLight".to_string(),
            dark_editor_theme_mode: "monochrome".to_string(),
            light_editor_theme_mode: "colorful".to_string(),
            editor_theme_mode: "colorful".to_string(),
            ui_font: "SF Pro".to_string(),
            language: "fr".to_string(),
            ui_scale: 1.2,
            body_latin_font: "IBM Plex Sans".to_string(),
            body_cjk_font: "PingFang SC".to_string(),
            code_font: "JetBrains Mono".to_string(),
            mermaid_theme: "auto".to_string(),
            font_size: 17.0,
            line_height: 1.7,
            letter_spacing: 0.5,
            editor_content_width: 800.0,
            sidebar_sort_mode: "created_desc".to_string(),
            file_list_width: None,
            file_list_visible: true,
            outline_rail_visible: true,
            show_whitespace: false,
            highlight_line_background: true,
            simple_mode: false,
            attachment_location: "current_file".to_string(),
            attachment_folder: "attachments".to_string(),
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
        let path = dir.path().join("config.yaml");

        let config = load_config_from_path(&path).unwrap();

        assert_eq!(config, AppConfig::default());
    }

    #[test]
    fn save_and_load_roundtrip_preserves_settings() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let config = AppConfig {
            settings: sample_settings(),
            workspaces: WorkspaceConfig {
                roots: vec![PathBuf::from("/tmp/project")],
                active_root: Some(PathBuf::from("/tmp/project")),
            },
            ..AppConfig::default()
        };

        save_config_to_path(&path, &config).unwrap();
        let loaded = load_config_from_path(&path).unwrap();

        assert_eq!(loaded, config);
    }

    #[test]
    fn load_partial_yaml_fills_missing_fields_with_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            "settings:\n  theme: MilkyDark\nworkspaces:\n  roots:\n    - /tmp/demo\n",
        )
        .unwrap();

        let loaded = load_config_from_path(&path).unwrap();

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
        assert_eq!(loaded.workspaces.roots, vec![PathBuf::from("/tmp/demo")]);
        assert_eq!(loaded.schema_version, CONFIG_SCHEMA_VERSION);
    }

    #[test]
    fn invalid_yaml_falls_back_to_default_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(&path, "settings: [unterminated").unwrap();

        let loaded = load_config_from_path(&path).unwrap();

        assert_eq!(loaded, AppConfig::default());
    }

    #[test]
    fn atomic_write_overwrites_existing_file_contents() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");

        let mut first = AppConfig::default();
        first.settings.theme = "OneLight".to_string();
        save_config_to_path(&path, &first).unwrap();

        let mut second = AppConfig::default();
        second.settings.theme = "MilkyLight".to_string();
        second.settings.light_theme = "MilkyLight".to_string();
        save_config_to_path(&path, &second).unwrap();

        let loaded = load_config_from_path(&path).unwrap();
        assert_eq!(loaded.settings.theme, "MilkyLight");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn session_roundtrip_preserves_recovery_and_view_state() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let source_path = dir.path().join("note.md");
        let temp_path = dir.path().join(".note.md.temp");
        let mut config = AppConfig::default();
        config.session = SessionConfig {
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

        save_config_to_path(&path, &config).unwrap();
        let loaded = load_config_from_path(&path).unwrap();

        assert_eq!(loaded.session, config.session);
    }
}
