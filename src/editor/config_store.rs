use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::AppSettings;
use super::config_paths;

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
    #[serde(default)]
    pub tabs: Vec<TabSessionRecord>,
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
    write_atomic(path, yaml.as_bytes())
}

fn write_atomic(path: &Path, content: &[u8]) -> io::Result<()> {
    let temp_path = atomic_temp_path(path);
    fs::write(&temp_path, content)?;

    if path.exists() {
        fs::remove_file(path)?;
    }

    fs::rename(temp_path, path)
}

fn atomic_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.yaml".to_string());
    path.with_file_name(format!(".{file_name}.tmp"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    fn sample_settings() -> AppSettings {
        AppSettings {
            theme: "AyuDark".to_string(),
            ui_font: "SF Pro".to_string(),
            ui_scale: 1.2,
            body_latin_font: "IBM Plex Sans".to_string(),
            body_cjk_font: "PingFang SC".to_string(),
            code_font: "JetBrains Mono".to_string(),
            font_size: 17.0,
            line_height: 1.8,
            letter_spacing: 0.5,
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
        assert_eq!(loaded.settings.ui_font, AppSettings::default().ui_font);
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
        second.settings.theme = "AyuLight".to_string();
        save_config_to_path(&path, &second).unwrap();

        let loaded = load_config_from_path(&path).unwrap();
        assert_eq!(loaded.settings.theme, "AyuLight");
        assert!(!dir.path().join(".config.yaml.tmp").exists());
    }
}
