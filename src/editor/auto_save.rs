use std::io;
use std::path::PathBuf;
use std::time::Duration;

use gpui::Context;

use super::*;

const AUTO_SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

fn should_auto_save(
    enabled: bool,
    is_editor: bool,
    supports_text_buffer: bool,
    is_dirty: bool,
    has_path: bool,
) -> bool {
    enabled && is_editor && supports_text_buffer && is_dirty && has_path
}

fn auto_save_epoch_is_current(current_epoch: Option<usize>, task_epoch: usize) -> bool {
    current_epoch == Some(task_epoch)
}

impl EditorView {
    pub(crate) fn active_tab_uses_auto_save(&self) -> bool {
        let Some(index) = self.active_tab_index() else {
            return false;
        };
        let tab = &self.tabs[index];
        should_auto_save(
            self.app_settings.auto_save,
            tab.kind == TabKind::Editor,
            tab.file_kind
                .as_ref()
                .is_some_and(FileKind::supports_text_buffer),
            tab.is_dirty,
            tab.path.is_some(),
        )
    }

    pub(crate) fn schedule_auto_save_for_active_tab(&mut self, cx: &mut Context<Self>) {
        if !self.active_tab_uses_auto_save() {
            return;
        }
        let Some(index) = self.active_tab_index() else {
            return;
        };
        let tab_id = self.tabs[index].id.clone();
        self.schedule_temp_save_for_active_tab(cx);
        if self.tabs[index]
            .path
            .as_deref()
            .is_some_and(|path| !file_ops::DocumentSave::auto_save_allowed(path))
        {
            return;
        }
        let task_tab_id = tab_id.clone();
        let epoch = self
            .auto_save_epochs
            .entry(tab_id.clone())
            .and_modify(|value| *value = value.wrapping_add(1))
            .or_insert(1usize)
            .to_owned();

        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTO_SAVE_DEBOUNCE).await;
            let payload = this
                .update(cx, |editor, _cx| {
                    editor.auto_save_payload_for_tab(&task_tab_id, epoch)
                })
                .ok()
                .flatten();
            let Some((path, snapshot)) = payload else {
                return;
            };

            let save = this.update(cx, |_, _| file_ops::DocumentSave::new(path.clone()));
            let Ok(save) = save else { return };
            let token = save.clone();
            let text = snapshot.text.clone();
            let result = match save.reserve_auto_save() {
                Ok(()) => {
                    crate::platform::fs_task::run("auto-save", path.clone(), move || {
                        save.write(text.as_bytes())
                    })
                    .await
                }
                Err(error) => Err(error),
            };
            token.observe_result(&result);
            let confirmed = matches!(result, Ok(true)) && token.is_latest();

            this.update(cx, |editor, cx| {
                if auto_save_epoch_is_current(
                    editor.auto_save_epochs.get(&task_tab_id).copied(),
                    epoch,
                ) {
                    editor.auto_save_tasks.remove(&task_tab_id);
                    if token.is_latest() {
                        editor.finish_auto_save(&task_tab_id, epoch, path, snapshot, result, cx);
                    }
                    if confirmed && editor.active_tab_id.as_deref() == Some(&task_tab_id) {
                        // Edits made while a known-successful write was running
                        // can be saved next. Uncertain outcomes never reschedule.
                        editor.schedule_auto_save_for_active_tab(cx);
                    }
                }
            })
            .ok();
        });
        self.auto_save_tasks.insert(tab_id, task);
    }

    fn auto_save_payload_for_tab(
        &mut self,
        tab_id: &str,
        epoch: usize,
    ) -> Option<(PathBuf, EditorSnapshot)> {
        if !auto_save_epoch_is_current(self.auto_save_epochs.get(tab_id).copied(), epoch)
            || !self.app_settings.auto_save
        {
            return None;
        }
        let owner = self.pane_id_for_tab(tab_id)?;
        self.with_pane(owner, |editor| {
            let index = editor.tabs.iter().position(|tab| tab.id == tab_id)?;
            let tab = &editor.tabs[index];
            if !should_auto_save(
                editor.app_settings.auto_save,
                tab.kind == TabKind::Editor,
                tab.file_kind
                    .as_ref()
                    .is_some_and(FileKind::supports_text_buffer),
                tab.is_dirty,
                tab.path.is_some(),
            ) {
                return None;
            }
            let path = tab.path.clone()?;
            if !file_ops::DocumentSave::auto_save_allowed(&path) {
                return None;
            }
            let snapshot = if editor.active_tab_id.as_deref() == Some(tab_id) {
                editor.current_snapshot()
            } else {
                tab.snapshot.clone()
            };
            Some((path, snapshot))
        })
    }

    fn finish_auto_save(
        &mut self,
        tab_id: &str,
        epoch: usize,
        path: PathBuf,
        snapshot: EditorSnapshot,
        result: io::Result<bool>,
        cx: &mut Context<Self>,
    ) {
        if !auto_save_epoch_is_current(self.auto_save_epochs.get(tab_id).copied(), epoch) {
            return;
        }
        if let Err(error) = result {
            self.persist_recovery_for_tab(tab_id);
            let message = format!(
                "{} ({}): {error}",
                self.t(crate::i18n::TextKey::SaveFailed),
                path.display()
            );
            self.notify_error(message, cx);
            return;
        }
        if matches!(result, Ok(false)) {
            return;
        }

        let Some(owner) = self.pane_id_for_tab(tab_id) else {
            return;
        };
        let saved = self.with_pane(owner, |editor| {
            let Some(index) = editor.tabs.iter().position(|tab| tab.id == tab_id) else {
                return false;
            };
            if !editor.app_settings.auto_save
                || !editor.tabs[index].is_dirty
                || editor.tabs[index].path.as_ref() != Some(&path)
            {
                return false;
            }
            editor.tabs[index].history.saved_text = Some(snapshot.text.clone());
            let current_text = if editor.active_tab_id.as_deref() == Some(tab_id) {
                editor.buffer.text()
            } else {
                &editor.tabs[index].snapshot.text
            };
            if current_text != snapshot.text {
                editor.tabs[index].recovery.source_content_hash =
                    Some(file_ops::content_hash(snapshot.text.as_bytes()));
                return false;
            }
            editor.tabs[index].is_dirty = false;
            editor.tabs[index].snapshot = snapshot;
            editor.clear_temp_recovery_for_tab(tab_id);
            true
        });
        if saved {
            self.auto_save_epochs.remove(tab_id);
            session_store::persist_session(self);
            if path == config_paths::settings_file_path() {
                self.apply_saved_config_file(cx);
            }
            cx.notify();
        }
    }

    pub(crate) fn cancel_auto_save_for_tab(&mut self, tab_id: &str) {
        self.auto_save_tasks.remove(tab_id);
        self.auto_save_epochs.remove(tab_id);
    }

    pub(crate) fn cancel_all_auto_saves(&mut self) {
        self.auto_save_tasks.clear();
        self.auto_save_epochs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{auto_save_epoch_is_current, should_auto_save};

    #[test]
    fn auto_save_requires_an_enabled_dirty_saved_text_editor() {
        assert!(should_auto_save(true, true, true, true, true));
        assert!(!should_auto_save(false, true, true, true, true));
        assert!(!should_auto_save(true, false, true, true, true));
        assert!(!should_auto_save(true, true, false, true, true));
        assert!(!should_auto_save(true, true, true, false, true));
        assert!(!should_auto_save(true, true, true, true, false));
    }

    #[test]
    fn newer_edits_invalidate_older_auto_save_results() {
        assert!(auto_save_epoch_is_current(Some(4), 4));
        assert!(!auto_save_epoch_is_current(Some(5), 4));
        assert!(!auto_save_epoch_is_current(None, 4));
    }
}
