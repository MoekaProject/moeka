use std::time::Duration;

use gpui::Context;

use super::*;

const TEMP_SAVE_DEBOUNCE: Duration = Duration::from_secs(1);

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

        // Recovery must not depend on a remote auto-save completing successfully.
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
                editor.persist_recovery_for_tab(&task_tab_id);
            })
            .ok();
        });
        self.temp_save_tasks.insert(tab_id, task);
    }

    pub(crate) fn persist_recovery_for_tab(&mut self, tab_id: &str) {
        let Some(owner) = self.pane_id_for_tab(tab_id) else {
            return;
        };
        if owner != self.current_pane_id() {
            let tab_id = tab_id.to_string();
            self.with_pane(owner, |this| this.persist_recovery_for_tab(&tab_id));
            return;
        }
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        if !self.tabs[index].is_dirty {
            return;
        }
        let snapshot = if self.active_tab_id.as_deref() == Some(tab_id) {
            self.current_snapshot()
        } else {
            self.tabs[index].snapshot.clone()
        };
        update_tab_recovery(&mut self.tabs[index], snapshot);
        session_store::persist_session(self);
    }
}

fn update_tab_recovery(tab: &mut OpenTab, snapshot: EditorSnapshot) {
    tab.recovery.text = Some(snapshot.text.clone());
    tab.snapshot = snapshot;
    // A detached recovery draft still needs its original source identity.
    if tab.path.is_some() {
        let same_source = tab.recovery.source_path == tab.path;
        tab.recovery.source_path = tab.path.clone();
        tab.recovery.source_content_hash = tab
            .history
            .saved_text
            .as_ref()
            .map(|text| file_ops::content_hash(text.as_bytes()))
            .or_else(|| {
                same_source
                    .then(|| tab.recovery.source_content_hash.clone())
                    .flatten()
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detached_recovery_edits_keep_source_identity_and_unverified_baseline() {
        let mut tab = builtin_welcome_tab(crate::i18n::UiLanguage::English);
        tab.path = None;
        tab.recovery.source_path = Some(PathBuf::from("original.moeka"));
        tab.recovery.source_content_hash = Some("old source hash".into());
        let mut snapshot = tab.snapshot.clone();
        snapshot.text = "edited recovery".into();
        update_tab_recovery(&mut tab, snapshot);
        assert_eq!(tab.recovery.text.as_deref(), Some("edited recovery"));
        assert_eq!(
            tab.recovery.source_path,
            Some(PathBuf::from("original.moeka"))
        );
        assert_eq!(
            tab.recovery.source_content_hash.as_deref(),
            Some("old source hash")
        );
        assert!(tab.path.is_none());
    }

    #[test]
    fn bound_recovery_keeps_verified_hash_until_a_new_saved_baseline_exists() {
        let mut tab = builtin_welcome_tab(crate::i18n::UiLanguage::English);
        tab.path = Some(PathBuf::from("original.moeka"));
        tab.recovery.source_path = tab.path.clone();
        tab.recovery.source_content_hash = Some("verified source hash".into());
        tab.history.saved_text = None;
        let snapshot = tab.snapshot.clone();
        update_tab_recovery(&mut tab, snapshot);
        assert_eq!(
            tab.recovery.source_content_hash.as_deref(),
            Some("verified source hash")
        );
        tab.history.saved_text = Some("new saved text".into());
        let snapshot = tab.snapshot.clone();
        update_tab_recovery(&mut tab, snapshot);
        assert_eq!(
            tab.recovery.source_content_hash,
            Some(file_ops::content_hash(b"new saved text"))
        );
        tab.path = Some(PathBuf::from("another.moeka"));
        tab.history.saved_text = None;
        let snapshot = tab.snapshot.clone();
        update_tab_recovery(&mut tab, snapshot);
        assert!(tab.recovery.source_content_hash.is_none());
    }
}

impl EditorView {
    pub(crate) fn clear_temp_file_for_tab(&mut self, tab_id: &str) {
        self.cancel_auto_save_for_tab(tab_id);
        self.clear_temp_recovery_for_tab(tab_id);
    }

    pub(crate) fn cancel_pending_temp_save_for_tab(&mut self, tab_id: &str) {
        self.temp_save_tasks.remove(tab_id);
        self.temp_save_epochs.remove(tab_id);
    }

    pub(crate) fn clear_temp_recovery_for_tab(&mut self, tab_id: &str) {
        self.cancel_pending_temp_save_for_tab(tab_id);
        if let Some(tab) = self.all_tabs_mut().find(|tab| tab.id == tab_id) {
            tab.recovery = Default::default();
        }
    }

    pub(crate) fn flush_recovery_files(&mut self) {
        self.persist_active_tab_state();
        let dirty_tab_ids = self
            .all_tabs()
            .filter(|tab| tab.kind == TabKind::Editor && tab.is_dirty)
            .map(|tab| tab.id.clone())
            .collect::<Vec<_>>();
        for tab_id in dirty_tab_ids {
            self.persist_recovery_for_tab(&tab_id);
        }
        session_store::persist_session(self);
    }
}
