use std::env;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

const APP_NAME: &str = "moeka";

/// `~/.config/moeka` on every Unix (macOS included) and `%APPDATA%\moeka` on
/// Windows, so the file stays where users expect to hand-edit it.
pub(crate) fn config_dir() -> PathBuf {
    #[cfg(windows)]
    if let Some(appdata) = env::var_os("APPDATA") {
        return PathBuf::from(appdata).join(APP_NAME);
    }

    #[cfg(not(windows))]
    {
        if let Some(xdg) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
            return PathBuf::from(xdg).join(APP_NAME);
        }
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home).join(".config").join(APP_NAME);
        }
    }

    fallback_config_dir()
}

pub(crate) fn cache_dir() -> PathBuf {
    project_dirs()
        .map(|dirs| dirs.cache_dir().to_path_buf())
        .unwrap_or_else(fallback_cache_dir)
}

/// User-editable preferences. Only this file drives settings reloads.
pub(crate) fn settings_file_path() -> PathBuf {
    config_dir().join("settings.yaml")
}

/// Restored workspaces, panes and tabs; written by the app, not hand-edited.
pub(crate) fn sessions_file_path() -> PathBuf {
    config_dir().join("sessions.yaml")
}

pub(crate) fn history_dir() -> PathBuf {
    cache_dir().join("history")
}

pub(crate) fn history_file_name_for_source(path: &Path) -> String {
    let basename = file_name_or_fallback(path);
    format!("{basename}.{}.undo.json", short_hash(path))
}

pub(crate) fn history_file_name_for_untitled(tab_id: &str) -> String {
    format!("untitled-{tab_id}.undo.json")
}

pub(crate) fn short_hash(path: &Path) -> String {
    let canonical = path.to_string_lossy();
    let hash = fnv1a64(canonical.as_bytes());
    format!("{:08x}", hash as u32)
}

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", APP_NAME)
}

fn fallback_config_dir() -> PathBuf {
    platform_base_dir("APPDATA", ".config")
}

fn fallback_cache_dir() -> PathBuf {
    platform_base_dir("LOCALAPPDATA", ".cache")
}

fn platform_base_dir(primary_env: &str, unix_fallback: &str) -> PathBuf {
    if let Some(path) = env::var_os(primary_env) {
        return PathBuf::from(path).join(APP_NAME);
    }

    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(unix_fallback).join(APP_NAME);
    }

    PathBuf::from(".").join(APP_NAME)
}

fn file_name_or_fallback(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "untitled".to_string())
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_hash_is_stable_for_same_path() {
        let path = Path::new("/tmp/example/note.md");
        assert_eq!(short_hash(path), short_hash(path));
        assert_eq!(short_hash(path), "44e0a532");
    }

    #[test]
    fn short_hash_differs_for_different_paths() {
        let left = Path::new("/tmp/example/note.md");
        let right = Path::new("/tmp/other/note.md");
        assert_ne!(short_hash(left), short_hash(right));
    }

    #[test]
    fn builds_history_file_name_for_saved_file() {
        let path = Path::new("/tmp/example/note.md");
        assert_eq!(
            history_file_name_for_source(path),
            "note.md.44e0a532.undo.json"
        );
    }

    #[test]
    fn builds_history_file_name_for_untitled_tab() {
        assert_eq!(
            history_file_name_for_untitled("tab-7"),
            "untitled-tab-7.undo.json"
        );
    }
}
