use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};

use super::AppSettings;
use super::config_paths;
use super::file_ops;

pub(crate) const CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Default)]
struct SettingsRevision {
    generation: u64,
    latest: Option<AppSettings>,
    status: SettingsWriteStatus,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum SettingsWriteStatus {
    #[default]
    Ready,
    Pending,
    Failed,
}

#[derive(Default)]
struct SettingsRevisionTracker {
    state: Mutex<SettingsRevision>,
    changed: Condvar,
}

impl SettingsRevisionTracker {
    fn register(&self, settings: &AppSettings, status: SettingsWriteStatus) -> u64 {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.generation = state.generation.wrapping_add(1);
        state.latest = Some(settings.clone());
        state.status = status;
        self.changed.notify_all();
        state.generation
    }

    fn finish(&self, generation: u64, succeeded: bool) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.generation == generation {
            state.status = if succeeded {
                SettingsWriteStatus::Ready
            } else {
                SettingsWriteStatus::Failed
            };
        }
        self.changed.notify_all();
    }

    fn newer_than(&self, generation: u64) -> Option<(u64, AppSettings)> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while state.generation != generation && state.status == SettingsWriteStatus::Pending {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        if state.generation == generation || state.status == SettingsWriteStatus::Failed {
            None
        } else {
            state
                .latest
                .clone()
                .map(|settings| (state.generation, settings))
        }
    }
}

fn settings_revision() -> &'static SettingsRevisionTracker {
    static REVISION: OnceLock<SettingsRevisionTracker> = OnceLock::new();
    REVISION.get_or_init(SettingsRevisionTracker::default)
}

pub(crate) fn register_settings_write(settings: &AppSettings) -> u64 {
    settings_revision().register(settings, SettingsWriteStatus::Ready)
}

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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub workspace_sessions: BTreeMap<PathBuf, SessionConfig>,
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

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
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
    pub reading_mode: bool,
    #[serde(default)]
    pub source_mode: bool,
    #[serde(default = "default_source_word_wrap")]
    pub source_word_wrap: bool,
    #[serde(default)]
    pub extra_cursors: Vec<(usize, usize)>,
}

impl Default for TabViewState {
    fn default() -> Self {
        Self {
            cursor_offset: 0,
            cursor_anchor: 0,
            scroll_offset: 0.0,
            editor_left_aligned: false,
            reading_mode: false,
            source_mode: false,
            source_word_wrap: default_source_word_wrap(),
            extra_cursors: Vec::new(),
        }
    }
}

pub(crate) fn default_source_word_wrap() -> bool {
    true
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct TabRecoveryState {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub source_path: Option<PathBuf>,
    #[serde(default)]
    pub source_content_hash: Option<String>,
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
            workspace_sessions: BTreeMap::new(),
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
    let revision = settings_revision();
    let generation = revision.register(settings, SettingsWriteStatus::Pending);
    let result = try_save_settings(settings);
    revision.finish(generation, result.is_ok());
    if let Err(error) = result {
        log_error!("failed to save settings: {error}");
    }
}

pub(crate) fn save_registered_settings(generation: u64, settings: AppSettings) -> io::Result<()> {
    let path = config_paths::settings_file_path();
    save_latest_settings(generation, settings, settings_revision(), |settings| {
        save_settings_to_path(&path, settings)
    })
}

fn save_latest_settings(
    mut generation: u64,
    mut settings: AppSettings,
    revision: &SettingsRevisionTracker,
    mut save: impl FnMut(&AppSettings) -> io::Result<()>,
) -> io::Result<()> {
    loop {
        save(&settings)?;
        let newer = revision.newer_than(generation);
        match newer {
            Some((next_generation, next_settings)) => {
                generation = next_generation;
                settings = next_settings;
            }
            None => return Ok(()),
        }
    }
}

pub(crate) fn try_save_settings(settings: &AppSettings) -> io::Result<()> {
    save_settings_to_path(&config_paths::settings_file_path(), settings)
}

fn save_settings_to_path(path: &Path, settings: &AppSettings) -> io::Result<()> {
    let file = SettingsFile {
        version: CONFIG_SCHEMA_VERSION,
        settings: settings.clone(),
    };
    save_yaml_file(path, &file)
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

    #[test]
    fn settings_write_reports_filesystem_errors() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("not-a-directory");
        fs::write(&parent, b"file").unwrap();
        assert!(
            save_settings_to_path(&parent.join("settings.yaml"), &AppSettings::default()).is_err()
        );
    }

    #[test]
    fn background_settings_write_repairs_a_newer_concurrent_save() {
        let revision = SettingsRevisionTracker::default();
        let old = AppSettings::default();
        let mut newer = old.clone();
        newer.minimap_position = super::super::app_settings::MinimapPosition::Off;
        newer.theme = "One Dark".to_string();
        let generation = revision.register(&old, SettingsWriteStatus::Ready);
        let mut saved = Vec::new();
        save_latest_settings(generation, old.clone(), &revision, |settings| {
            saved.push(settings.clone());
            if saved.len() == 1 {
                revision.register(&newer, SettingsWriteStatus::Ready);
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(saved, [old.clone(), newer.clone()]);

        let mut attempts = 0;
        let error = save_latest_settings(generation, AppSettings::default(), &revision, |_| {
            attempts += 1;
            Err(io::Error::other("write failed"))
        })
        .unwrap_err();
        assert_eq!(attempts, 1);
        assert_eq!(error.to_string(), "write failed");

        let revision = SettingsRevisionTracker::default();
        let generation = revision.register(&old, SettingsWriteStatus::Ready);
        let mut saved = Vec::new();
        save_latest_settings(generation, old.clone(), &revision, |settings| {
            saved.push(settings.clone());
            if saved.len() == 1 {
                let failed = revision.register(&newer, SettingsWriteStatus::Pending);
                revision.finish(failed, false);
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(saved, [old]);
    }

    #[test]
    fn background_settings_write_waits_for_a_pending_newer_save() {
        let revision = SettingsRevisionTracker::default();
        let old = AppSettings::default();
        let mut newer = old.clone();
        newer.theme = "One Dark".to_string();
        let generation = revision.register(&old, SettingsWriteStatus::Ready);
        let mut saved = Vec::new();
        std::thread::scope(|scope| {
            save_latest_settings(generation, old, &revision, |settings| {
                saved.push(settings.clone());
                if saved.len() == 1 {
                    let pending = revision.register(&newer, SettingsWriteStatus::Pending);
                    let revision = &revision;
                    scope.spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        revision.finish(pending, true);
                    });
                }
                Ok(())
            })
            .unwrap();
        });
        assert_eq!(saved.last(), Some(&newer));
        assert_eq!(saved.len(), 2);
    }

    fn sample_settings() -> AppSettings {
        AppSettings {
            ui_corners: crate::style::UiCorners::Full,
            ui_elevation: true,
            theme: "One Dark".to_string(),
            theme_mode: "auto".to_string(),
            dark_theme: "One Dark".to_string(),
            light_theme: "Porcelain Mist".to_string(),
            dark_editor_theme_mode: "monochrome".to_string(),
            light_editor_theme_mode: "colorful".to_string(),
            editor_theme_mode: "colorful".to_string(),
            ui_font: "SF Pro".to_string(),
            language: "fr".to_string(),
            ui_scale: 1.2,
            body_latin_font: "IBM Plex Sans".to_string(),
            body_cjk_font: crate::platform::fonts::CJK.to_string(),
            code_font: "JetBrains Mono".to_string(),
            font_size: 17.0,
            line_height: 1.7,
            letter_spacing: 0.5,
            editor_content_width: 800.0,
            sidebar_sort_mode: "created_desc".to_string(),
            file_list_width: None,
            workspace_search_width: None,
            file_list_visible: true,
            outline_rail_visible: true,
            minimap_position: super::super::app_settings::MinimapPosition::Right,
            minimap_last_side: None,
            show_whitespace: false,
            highlight_line_background: true,
            default_left_align: false,
            simple_mode: false,
            auto_save: false,
            attachment_location: "current_file".to_string(),
            attachment_folder: "attachments".to_string(),
            attachment_folder_template: Some("${folderName}/${fileName}".to_string()),
            link_syntax: "wikilink".to_string(),
            link_path_style: "shortest".to_string(),
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
        assert_eq!(loaded.settings.link_syntax, "wikilink");
        assert_eq!(loaded.settings.link_path_style, "shortest");
    }

    #[test]
    fn removed_mermaid_settings_are_ignored_and_not_saved() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.yaml");
        fs::write(
            &path,
            "settings:\n  mermaid_style: editorial\n  mermaid_theme: mermaid-dark\n  code_font: Menlo\n",
        )
        .unwrap();
        let config = load_yaml_file::<SettingsFile>(&path).unwrap();
        assert_eq!(config.settings.code_font, "Menlo");
        save_yaml_file(&path, &config).unwrap();
        let saved = fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("mermaid_style"));
        assert!(!saved.contains("mermaid_theme"));
        assert_eq!(load_yaml_file::<SettingsFile>(&path).unwrap(), config);
    }

    #[test]
    fn legacy_format_toolbar_setting_is_ignored_and_not_saved() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.yaml");
        fs::write(&path, "settings: {}\n").unwrap();
        let expected = load_yaml_file::<SettingsFile>(&path).unwrap();
        fs::write(&path, "settings:\n  format_toolbar_visible: false\n").unwrap();

        let config = load_yaml_file::<SettingsFile>(&path).unwrap();
        assert_eq!(config, expected);

        save_yaml_file(&path, &config).unwrap();
        assert!(
            !fs::read_to_string(&path)
                .unwrap()
                .contains("format_toolbar_visible")
        );
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
    fn session_roundtrip_preserves_source_mode_and_source_word_wrap_in_flat_and_split_tabs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sessions.yaml");
        let source_tab = TabSessionRecord {
            id: "tab-source".into(),
            kind: "editor".into(),
            view: TabViewState {
                source_mode: true,
                source_word_wrap: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let reading_tab = TabSessionRecord {
            id: "tab-reading".into(),
            kind: "editor".into(),
            view: TabViewState {
                editor_left_aligned: true,
                reading_mode: true,
                source_word_wrap: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let config = SessionsFile {
            sessions: SessionConfig {
                tabs: vec![source_tab.clone(), reading_tab.clone()],
                pane_layout: Some(PaneLayoutRecord::Split {
                    axis: "horizontal".into(),
                    flexes: vec![1.0, 1.0],
                    children: vec![
                        PaneLayoutRecord::Leaf {
                            active_tab_id: Some(source_tab.id.clone()),
                            tabs: vec![source_tab],
                        },
                        PaneLayoutRecord::Leaf {
                            active_tab_id: Some(reading_tab.id.clone()),
                            tabs: vec![reading_tab],
                        },
                    ],
                }),
                focused_pane_leaf: Some(1),
                ..Default::default()
            },
            ..Default::default()
        };

        save_yaml_file(&path, &config).unwrap();
        let loaded = load_yaml_file::<SessionsFile>(&path).unwrap();

        assert_eq!(loaded, config);
        assert!(loaded.sessions.tabs[0].view.source_mode);
        assert!(!loaded.sessions.tabs[0].view.reading_mode);
        assert!(!loaded.sessions.tabs[0].view.source_word_wrap);
        assert!(!loaded.sessions.tabs[0].view.editor_left_aligned);
        assert!(!loaded.sessions.tabs[1].view.source_mode);
        assert!(loaded.sessions.tabs[1].view.reading_mode);
        assert!(loaded.sessions.tabs[1].view.source_word_wrap);
        assert!(loaded.sessions.tabs[1].view.editor_left_aligned);
    }

    #[test]
    fn legacy_session_view_defaults_source_mode_to_false() {
        let config: SessionsFile = serde_yaml::from_str(
            "sessions:\n  tabs:\n    - id: tab-reading\n      kind: editor\n      view:\n        reading_mode: true\n    - id: tab-edit\n      kind: editor\n",
        )
        .unwrap();

        assert!(config.sessions.tabs[0].view.reading_mode);
        assert!(!config.sessions.tabs[0].view.source_mode);
        assert!(!config.sessions.tabs[1].view.reading_mode);
        assert!(!config.sessions.tabs[1].view.source_mode);
        assert!(!TabViewState::default().source_mode);
    }

    #[test]
    fn source_word_wrap_defaults_to_true_for_new_and_legacy_tab_views() {
        assert!(TabViewState::default().source_word_wrap);
        assert!(TabSessionRecord::default().view.source_word_wrap);
        for yaml in ["{}", "source_mode: true", "editor_left_aligned: true"] {
            let view: TabViewState = serde_yaml::from_str(yaml).unwrap();
            assert!(view.source_word_wrap);
        }

        let config: SessionsFile = serde_yaml::from_str(
            "sessions:\n  tabs:\n    - id: tab-source\n      kind: editor\n      view:\n        source_mode: true\n    - id: tab-edit\n      kind: editor\n  pane_layout: !Leaf\n    tabs:\n      - id: tab-pane\n        kind: editor\n        view: {}\n",
        )
        .unwrap();
        assert!(config.sessions.tabs[0].view.source_word_wrap);
        assert!(config.sessions.tabs[1].view.source_word_wrap);
        let Some(PaneLayoutRecord::Leaf { tabs, .. }) = config.sessions.pane_layout else {
            panic!("expected a pane leaf");
        };
        assert!(tabs[0].view.source_word_wrap);
    }

    #[test]
    fn load_partial_yaml_fills_missing_fields_with_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.yaml");
        fs::write(&path, "settings:\n  theme: One Dark\n").unwrap();

        let loaded = load_yaml_file::<SettingsFile>(&path).unwrap();

        assert_eq!(loaded.settings.theme, "One Dark");
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
        first.settings.theme = "Linen Paper".to_string();
        save_yaml_file(&path, &first).unwrap();

        let mut second = SettingsFile::default();
        second.settings.theme = "Porcelain Mist".to_string();
        second.settings.light_theme = "Porcelain Mist".to_string();
        save_yaml_file(&path, &second).unwrap();

        let loaded = load_yaml_file::<SettingsFile>(&path).unwrap();
        assert_eq!(loaded.settings.theme, "Porcelain Mist");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn session_roundtrip_preserves_recovery_and_view_state() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sessions.yaml");
        let source_path = dir.path().join("note.md");
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
                    reading_mode: true,
                    source_mode: false,
                    source_word_wrap: true,
                    extra_cursors: vec![(9, 3)],
                },
                recovery: TabRecoveryState {
                    text: Some("unsaved draft".to_string()),
                    source_path: Some(source_path),
                    source_content_hash: Some("abc123".to_string()),
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
