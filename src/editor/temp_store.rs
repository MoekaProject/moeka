use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::Context;

use super::config_paths;
use super::*;

const UNTITLED_TEMP_DIR: &str = ".markdown-editor-cache/untitled";
const TEMP_SAVE_DEBOUNCE: Duration = Duration::from_secs(5);

pub(crate) fn choose_temp_path_for_tab(
    tab_id: &str,
    source_path: Option<&Path>,
    current_temp_path: Option<&Path>,
    sidebar_root: &Path,
) -> PathBuf {
    if let Some(existing) = current_temp_path {
        return existing.to_path_buf();
    }

    if let Some(source_path) = source_path {
        let default_path = config_paths::hidden_temp_file_path(source_path);
        if !default_path.exists() {
            return default_path;
        }
        return config_paths::hidden_temp_file_path_with_hash(source_path);
    }

    untitled_temp_root(sidebar_root).join(format!("{tab_id}.temp"))
}

pub(crate) fn write_temp_file(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "temp".to_string());
    let temp_path = path.with_file_name(format!(".{file_name}.tmp"));
    fs::write(&temp_path, text.as_bytes())?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temp_path, path)
}

pub(crate) fn delete_temp_file(path: &Path) -> io::Result<()> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn untitled_temp_root(sidebar_root: &Path) -> PathBuf {
    if !sidebar_root.as_os_str().is_empty() {
        return sidebar_root.join(UNTITLED_TEMP_DIR);
    }
    config_paths::cache_dir().join("untitled")
}

impl EditorView {
    pub(crate) fn schedule_temp_save_for_active_tab(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.active_tab_index() else {
            return;
        };
        if self.tabs[index].kind != TabKind::Editor {
            return;
        }
        if !self.tabs[index]
            .file_kind
            .as_ref()
            .is_some_and(FileKind::supports_text_buffer)
        {
            return;
        }
        if !self.tabs[index].is_dirty {
            return;
        }

        let tab_id = self.tabs[index].id.clone();
        let task_tab_id = tab_id.clone();
        let epoch = self
            .temp_save_epochs
            .entry(tab_id.clone())
            .and_modify(|value| *value += 1)
            .or_insert(1usize)
            .to_owned();
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TEMP_SAVE_DEBOUNCE).await;
            this.update(cx, |editor, _cx| {
                let Some(current_epoch) = editor.temp_save_epochs.get(&task_tab_id).copied() else {
                    return;
                };
                if current_epoch != epoch {
                    return;
                }
                let _ = editor.write_temp_file_for_tab(&task_tab_id);
            })
            .ok();
        });
        self.temp_save_tasks.insert(tab_id, task);
    }

    pub(crate) fn write_temp_file_for_tab(&mut self, tab_id: &str) -> io::Result<Option<PathBuf>> {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return Ok(None);
        };
        if !self.tabs[index].is_dirty {
            return Ok(None);
        }
        let snapshot = if self.active_tab_id.as_deref() == Some(tab_id) {
            self.current_snapshot()
        } else {
            self.tabs[index].snapshot.clone()
        };
        let temp_path = choose_temp_path_for_tab(
            tab_id,
            self.tabs[index].path.as_deref(),
            self.tabs[index].recovery.temp_path.as_deref(),
            &self.sidebar_root,
        );
        write_temp_file(&temp_path, &snapshot.text)?;
        self.tabs[index].recovery.temp_path = Some(temp_path.clone());
        self.tabs[index].recovery.source_path = self.tabs[index].path.clone();
        self.tabs[index].recovery.recovered_from_temp = true;
        Ok(Some(temp_path))
    }

    pub(crate) fn clear_temp_file_for_tab(&mut self, tab_id: &str) {
        self.temp_save_tasks.remove(tab_id);
        self.temp_save_epochs.remove(tab_id);
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            if let Some(path) = tab.recovery.temp_path.take() {
                let _ = delete_temp_file(&path);
            }
            tab.recovery.recovered_from_temp = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    fn chooses_hidden_temp_path_for_saved_file() {
        let path = Path::new("/tmp/demo/note.md");
        let temp_path = choose_temp_path_for_tab("tab-1", Some(path), None, Path::new(""));
        assert_eq!(temp_path, PathBuf::from("/tmp/demo/.note.md.temp"));
    }

    #[test]
    fn falls_back_to_hashed_temp_path_when_default_name_is_occupied() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        let default_temp = dir.path().join(".note.md.temp");
        fs::write(&default_temp, "occupied").unwrap();

        let temp_path = choose_temp_path_for_tab("tab-1", Some(&path), None, Path::new(""));
        assert_eq!(
            temp_path,
            dir.path()
                .join(format!(".note.md.{}.temp", config_paths::short_hash(&path)))
        );
    }

    #[test]
    fn chooses_workspace_scoped_temp_path_for_untitled_tab() {
        let temp_path = choose_temp_path_for_tab("tab-7", None, None, Path::new("/tmp/workspace"));
        assert_eq!(
            temp_path,
            PathBuf::from("/tmp/workspace/.markdown-editor-cache/untitled/tab-7.temp")
        );
    }

    #[test]
    fn writes_and_deletes_temp_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".note.md.temp");

        write_temp_file(&path, "hello").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");

        delete_temp_file(&path).unwrap();
        assert!(!path.exists());
    }
}
