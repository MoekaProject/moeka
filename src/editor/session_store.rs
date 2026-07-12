use std::fs;
use std::path::{Path, PathBuf};

use super::config_store::{
    self, SessionConfig, TabHistoryIndex, TabRecoveryState, TabSessionRecord, TabViewState,
};
use super::history_store;
use super::*;

pub(crate) fn persist_session(editor: &EditorView) {
    let mut config = config_store::load_config();
    config.workspaces.roots = editor.workspaces.clone();
    config.workspaces.active_root = if editor.sidebar_root.as_os_str().is_empty() {
        None
    } else {
        Some(editor.sidebar_root.clone())
    };
    config.session = SessionConfig {
        active_tab_id: editor.active_tab_id.clone(),
        next_tab_id: Some(editor.next_tab_id),
        tabs: editor
            .tabs
            .iter()
            .filter(|tab| tab.kind != TabKind::Diff)
            .map(tab_to_record)
            .collect(),
    };
    config_store::save_config(&config);
}

pub(crate) fn restore_session(editor: &mut EditorView) {
    let config = config_store::load_config();
    editor.workspaces = config.workspaces.roots.clone();
    if let Some(active_root) = config.workspaces.active_root {
        editor.sidebar_root = active_root.clone();
        editor.sidebar_visible = true;
        editor.expanded_dirs.clear();
        editor.expanded_dirs.insert(active_root);
    }

    if config.session.tabs.is_empty() {
        return;
    }

    let mut restored_tabs = Vec::new();
    for record in &config.session.tabs {
        if let Some(tab) = restore_tab_from_record(record, editor.ui_language) {
            restored_tabs.push(tab);
        }
    }

    if restored_tabs.is_empty() {
        return;
    }

    editor.tabs = restored_tabs;
    editor.active_tab_id = config
        .session
        .active_tab_id
        .filter(|active_id| editor.tabs.iter().any(|tab| tab.id == *active_id))
        .or_else(|| editor.tabs.first().map(|tab| tab.id.clone()));
    editor.next_tab_id = config
        .session
        .next_tab_id
        .unwrap_or_else(|| infer_next_tab_id(&editor.tabs, editor.next_tab_id));

    let Some(index) = editor.active_tab_index() else {
        return;
    };
    let active_tab = editor.tabs[index].clone();
    editor.current_file_path = active_tab.path.clone();
    editor.editor_left_aligned = active_tab.editor_left_aligned;
    if active_tab.kind == TabKind::Editor {
        if active_tab
            .file_kind
            .as_ref()
            .is_some_and(FileKind::supports_text_buffer)
        {
            editor.buffer = Buffer::from_text(&active_tab.snapshot.text);
            editor.cursor.offset = active_tab.snapshot.cursor_offset.min(editor.buffer.len());
            editor.cursor.anchor = active_tab.snapshot.cursor_anchor.min(editor.buffer.len());
            editor.extra_cursors = active_tab
                .snapshot
                .extra_cursors
                .iter()
                .map(|(offset, anchor)| Cursor {
                    offset: (*offset).min(editor.buffer.len()),
                    anchor: (*anchor).min(editor.buffer.len()),
                })
                .collect();
        } else {
            editor.buffer = Buffer::from_text("");
            editor.cursor.offset = 0;
            editor.cursor.anchor = 0;
            editor.extra_cursors.clear();
        }
        editor.scroll_offset = active_tab.snapshot.scroll_offset;
        editor.extra_marked_ranges.clear();
        editor.reparse();
    }
}

fn tab_to_record(tab: &OpenTab) -> TabSessionRecord {
    TabSessionRecord {
        id: tab.id.clone(),
        kind: serialize_tab_kind(tab.kind),
        title: tab.title.clone(),
        path: tab.path.clone(),
        file_kind: tab.file_kind.as_ref().map(serialize_file_kind),
        is_preview: tab.is_preview,
        is_pinned: tab.is_pinned,
        view: TabViewState {
            cursor_offset: tab.snapshot.cursor_offset,
            cursor_anchor: tab.snapshot.cursor_anchor,
            scroll_offset: tab.snapshot.scroll_offset,
            editor_left_aligned: tab.editor_left_aligned,
            extra_cursors: tab.snapshot.extra_cursors.clone(),
        },
        recovery: tab.recovery.clone(),
        history: TabHistoryIndex {
            undo_log_path: tab.history.journal_path.clone(),
            source_identity: tab.history.source_identity.clone(),
            current_index: Some(tab.history.current_index()),
            entry_count: tab.history.entry_count(),
        },
    }
}

fn restore_tab_from_record(record: &TabSessionRecord, ui_language: UiLanguage) -> Option<OpenTab> {
    let kind = deserialize_tab_kind(&record.kind)?;
    if kind == TabKind::Settings {
        return Some(settings_tab(record.id.clone()));
    }

    let file_kind = record.file_kind.as_deref().and_then(deserialize_file_kind);
    let (path, snapshot, is_dirty) = resolve_snapshot(record, file_kind.as_ref(), ui_language)?;
    let whiteboard = if matches!(file_kind, Some(FileKind::Whiteboard)) {
        path.as_deref().map(WhiteboardTabState::load_from_path)
    } else {
        None
    };
    let mut history = history_store::load_tab_history_from_path(
        &record.id,
        path.as_deref(),
        record.history.undo_log_path.as_deref(),
        &snapshot,
    );
    if let Some(source_identity) = record.history.source_identity.clone() {
        history.source_identity = Some(source_identity);
    }

    let recovery = if is_dirty {
        let mut recovery = record.recovery.clone();
        recovery.recovered_from_temp = recovery.temp_path.is_some();
        recovery
    } else {
        if let Some(temp_path) = record.recovery.temp_path.as_deref() {
            let _ = super::temp_store::delete_temp_file(temp_path);
        }
        TabRecoveryState::default()
    };

    Some(OpenTab {
        id: record.id.clone(),
        title: record.title.clone(),
        kind,
        path,
        file_kind,
        editor_left_aligned: record.view.editor_left_aligned,
        snapshot,
        image_view: ImageViewState::default(),
        whiteboard,
        git_diff: None,
        inline_diff: None,
        is_preview: record.is_preview,
        is_dirty,
        is_deleted: false,
        is_pinned: record.is_pinned,
        recovery,
        history,
    })
}

fn resolve_snapshot(
    record: &TabSessionRecord,
    file_kind: Option<&FileKind>,
    ui_language: UiLanguage,
) -> Option<(Option<PathBuf>, EditorSnapshot, bool)> {
    let temp_text = record
        .recovery
        .temp_path
        .as_ref()
        .and_then(|path| fs::read_to_string(path).ok());
    let has_temp_text = temp_text.is_some();

    match record.path.as_ref() {
        Some(path) if path.exists() => {
            let (mut snapshot, resolved_kind) = load_file_snapshot(path)?;
            if resolved_kind.supports_text_buffer() {
                if let Some(text) = temp_text {
                    if snapshot.text != text {
                        let source_changed = record
                            .recovery
                            .source_content_hash
                            .as_ref()
                            .is_some_and(|expected| {
                                file_ops::content_hash(snapshot.text.as_bytes()) != *expected
                            });
                        snapshot.text = text;
                        snapshot.cursor_offset = record.view.cursor_offset.min(snapshot.text.len());
                        snapshot.cursor_anchor = record.view.cursor_anchor.min(snapshot.text.len());
                        snapshot.scroll_offset = record.view.scroll_offset;
                        snapshot.extra_cursors = record
                            .view
                            .extra_cursors
                            .iter()
                            .map(|(offset, anchor)| {
                                (
                                    (*offset).min(snapshot.text.len()),
                                    (*anchor).min(snapshot.text.len()),
                                )
                            })
                            .collect();
                        return Some(((!source_changed).then(|| path.clone()), snapshot, true));
                    }
                }
                snapshot.cursor_offset = record.view.cursor_offset.min(snapshot.text.len());
                snapshot.cursor_anchor = record.view.cursor_anchor.min(snapshot.text.len());
                snapshot.scroll_offset = record.view.scroll_offset;
                snapshot.extra_cursors = record
                    .view
                    .extra_cursors
                    .iter()
                    .map(|(offset, anchor)| {
                        (
                            (*offset).min(snapshot.text.len()),
                            (*anchor).min(snapshot.text.len()),
                        )
                    })
                    .collect();
                Some((Some(path.clone()), snapshot, false))
            } else {
                snapshot.scroll_offset = record.view.scroll_offset;
                Some((Some(path.clone()), snapshot, false))
            }
        }
        Some(_) => temp_text.map(|text| {
            let snapshot = EditorSnapshot {
                cursor_offset: record.view.cursor_offset.min(text.len()),
                cursor_anchor: record.view.cursor_anchor.min(text.len()),
                scroll_offset: record.view.scroll_offset,
                extra_cursors: record
                    .view
                    .extra_cursors
                    .iter()
                    .map(|(offset, anchor)| ((*offset).min(text.len()), (*anchor).min(text.len())))
                    .collect(),
                text,
            };
            (None, snapshot, true)
        }),
        None if record.id == super::WELCOME_TAB_ID
            && file_kind.is_some_and(FileKind::supports_text_buffer) =>
        {
            let text = temp_text.unwrap_or_else(|| super::default_document_text(ui_language));
            let snapshot = EditorSnapshot {
                cursor_offset: record.view.cursor_offset.min(text.len()),
                cursor_anchor: record.view.cursor_anchor.min(text.len()),
                scroll_offset: record.view.scroll_offset,
                extra_cursors: record
                    .view
                    .extra_cursors
                    .iter()
                    .map(|(offset, anchor)| ((*offset).min(text.len()), (*anchor).min(text.len())))
                    .collect(),
                text,
            };
            Some((None, snapshot, has_temp_text))
        }
        None if file_kind.is_some_and(FileKind::supports_text_buffer) => temp_text.map(|text| {
            let snapshot = EditorSnapshot {
                cursor_offset: record.view.cursor_offset.min(text.len()),
                cursor_anchor: record.view.cursor_anchor.min(text.len()),
                scroll_offset: record.view.scroll_offset,
                extra_cursors: record
                    .view
                    .extra_cursors
                    .iter()
                    .map(|(offset, anchor)| ((*offset).min(text.len()), (*anchor).min(text.len())))
                    .collect(),
                text,
            };
            (None, snapshot, true)
        }),
        None => None,
    }
}

fn load_file_snapshot(path: &Path) -> Option<(EditorSnapshot, FileKind)> {
    let bytes = fs::read(path).ok()?;
    let file_kind = classify_file_kind_for_bytes(path, &bytes);
    let text = if file_kind.supports_text_buffer() {
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        String::new()
    };
    let first_line_end = text.find('\n').unwrap_or(text.len());
    Some((
        EditorSnapshot {
            text,
            cursor_offset: first_line_end,
            cursor_anchor: first_line_end,
            scroll_offset: 0.0,
            extra_cursors: Vec::new(),
        },
        file_kind,
    ))
}

fn serialize_tab_kind(kind: TabKind) -> String {
    match kind {
        TabKind::Editor => "editor",
        TabKind::Diff => "diff",
        TabKind::Settings => "settings",
    }
    .to_string()
}

fn deserialize_tab_kind(value: &str) -> Option<TabKind> {
    match value {
        "editor" => Some(TabKind::Editor),
        "diff" => Some(TabKind::Diff),
        "settings" => Some(TabKind::Settings),
        _ => None,
    }
}

fn serialize_file_kind(kind: &FileKind) -> String {
    match kind {
        FileKind::Markdown => "markdown".to_string(),
        FileKind::Text => "text".to_string(),
        FileKind::Image => "image".to_string(),
        FileKind::Binary => "binary".to_string(),
        FileKind::Whiteboard => "whiteboard".to_string(),
        FileKind::Code { language } => format!("code:{language}"),
    }
}

fn deserialize_file_kind(value: &str) -> Option<FileKind> {
    match value {
        "markdown" => Some(FileKind::Markdown),
        "text" => Some(FileKind::Text),
        "image" => Some(FileKind::Image),
        "binary" => Some(FileKind::Binary),
        "whiteboard" => Some(FileKind::Whiteboard),
        _ => value.strip_prefix("code:").map(|language| FileKind::Code {
            language: language.to_string(),
        }),
    }
}

fn settings_tab(id: String) -> OpenTab {
    OpenTab {
        id,
        title: crate::i18n::translate(
            crate::i18n::detect_system_language(),
            crate::i18n::TextKey::SettingsTitle,
        )
        .to_string(),
        kind: TabKind::Settings,
        path: None,
        file_kind: None,
        editor_left_aligned: false,
        snapshot: EditorSnapshot {
            text: String::new(),
            cursor_offset: 0,
            cursor_anchor: 0,
            scroll_offset: 0.0,
            extra_cursors: Vec::new(),
        },
        image_view: ImageViewState::default(),
        whiteboard: None,
        git_diff: None,
        inline_diff: None,
        is_preview: false,
        is_dirty: false,
        is_deleted: false,
        is_pinned: false,
        recovery: TabRecoveryState::default(),
        history: history_store::TabHistory::default(),
    }
}

fn infer_next_tab_id(tabs: &[OpenTab], fallback: usize) -> usize {
    tabs.iter()
        .filter_map(|tab| tab.id.strip_prefix("tab-"))
        .filter_map(|suffix| suffix.parse::<usize>().ok())
        .max()
        .map(|suffix| suffix + 1)
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    fn snapshot(text: &str, cursor_offset: usize) -> EditorSnapshot {
        EditorSnapshot {
            text: text.to_string(),
            cursor_offset,
            cursor_anchor: cursor_offset,
            scroll_offset: cursor_offset as f32,
            extra_cursors: Vec::new(),
        }
    }

    fn text_tab_record(
        source_path: Option<PathBuf>,
        temp_path: Option<PathBuf>,
    ) -> TabSessionRecord {
        TabSessionRecord {
            id: "tab-1".to_string(),
            kind: "editor".to_string(),
            title: "note.md".to_string(),
            path: source_path.clone(),
            file_kind: Some("markdown".to_string()),
            is_preview: false,
            is_pinned: false,
            view: TabViewState {
                cursor_offset: 100,
                cursor_anchor: 50,
                scroll_offset: 42.0,
                editor_left_aligned: true,
                extra_cursors: vec![(80, 70)],
            },
            recovery: TabRecoveryState {
                temp_path,
                source_path,
                source_mtime: Some(123),
                source_content_hash: None,
                recovered_from_temp: true,
            },
            history: TabHistoryIndex::default(),
        }
    }

    #[test]
    fn file_kind_roundtrip_preserves_code_language() {
        let kind = FileKind::Code {
            language: "rust".to_string(),
        };
        let encoded = serialize_file_kind(&kind);
        let decoded = deserialize_file_kind(&encoded).unwrap();
        assert_eq!(decoded, kind);
    }

    #[test]
    fn infer_next_tab_id_uses_largest_existing_numeric_suffix() {
        let tabs = vec![
            settings_tab("settings-1".to_string()),
            OpenTab {
                id: "tab-9".to_string(),
                title: "A".to_string(),
                kind: TabKind::Editor,
                path: None,
                file_kind: Some(FileKind::Markdown),
                editor_left_aligned: false,
                snapshot: EditorSnapshot {
                    text: String::new(),
                    cursor_offset: 0,
                    cursor_anchor: 0,
                    scroll_offset: 0.0,
                    extra_cursors: Vec::new(),
                },
                image_view: ImageViewState::default(),
                whiteboard: None,
                git_diff: None,
                inline_diff: None,
                is_preview: false,
                is_dirty: false,
                is_deleted: false,
                is_pinned: false,
                recovery: TabRecoveryState::default(),
                history: history_store::TabHistory::default(),
            },
        ];

        assert_eq!(infer_next_tab_id(&tabs, 2), 10);
    }

    #[test]
    fn restore_tab_from_record_uses_explicit_history_path() {
        let dir = tempdir().unwrap();
        let source_path = dir.path().join("note.md");
        fs::write(&source_path, "current").unwrap();

        let custom_journal_path = dir.path().join("custom.undo.json");
        let mut history = history_store::TabHistory::new(
            Some(source_path.to_string_lossy().into_owned()),
            Some(custom_journal_path.clone()),
        );
        history.push_undo_snapshot(snapshot("before", 6));
        let current = snapshot("current", 7);
        history_store::persist_tab_history("tab-1", Some(&source_path), &mut history, &current)
            .unwrap();

        let record = TabSessionRecord {
            id: "tab-1".to_string(),
            kind: "editor".to_string(),
            title: "note.md".to_string(),
            path: Some(source_path.clone()),
            file_kind: Some("markdown".to_string()),
            is_preview: false,
            is_pinned: false,
            view: TabViewState {
                cursor_offset: 7,
                cursor_anchor: 7,
                scroll_offset: 7.0,
                editor_left_aligned: false,
                extra_cursors: Vec::new(),
            },
            recovery: TabRecoveryState::default(),
            history: TabHistoryIndex {
                undo_log_path: Some(custom_journal_path.clone()),
                source_identity: Some(source_path.to_string_lossy().into_owned()),
                current_index: Some(1),
                entry_count: 2,
            },
        };

        let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();

        assert_eq!(restored.history.journal_path, Some(custom_journal_path));
        assert_eq!(restored.history.undo_stack, vec![snapshot("before", 6)]);
        assert!(restored.history.redo_stack.is_empty());
    }

    #[test]
    fn restores_newer_temporary_contents_over_saved_file() {
        let dir = tempdir().unwrap();
        let source_path = dir.path().join("note.md");
        let temp_path = dir.path().join(".note.md.temp");
        fs::write(&source_path, "saved").unwrap();
        fs::write(&temp_path, "recovered draft").unwrap();
        let record = text_tab_record(Some(source_path.clone()), Some(temp_path.clone()));

        let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();

        assert_eq!(restored.path, Some(source_path));
        assert_eq!(restored.snapshot.text, "recovered draft");
        assert_eq!(restored.snapshot.cursor_offset, "recovered draft".len());
        assert_eq!(restored.snapshot.cursor_anchor, "recovered draft".len());
        assert_eq!(
            restored.snapshot.extra_cursors,
            vec![("recovered draft".len(), "recovered draft".len())]
        );
        assert!(restored.is_dirty);
        assert_eq!(restored.recovery.temp_path, Some(temp_path));
        assert!(restored.recovery.recovered_from_temp);
    }

    #[test]
    fn recovers_temporary_contents_when_source_file_was_deleted() {
        let dir = tempdir().unwrap();
        let source_path = dir.path().join("deleted.md");
        let temp_path = dir.path().join(".deleted.md.temp");
        fs::write(&temp_path, "unsaved work").unwrap();
        let record = text_tab_record(Some(source_path.clone()), Some(temp_path.clone()));

        let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();

        assert_eq!(restored.path, None);
        assert_eq!(restored.snapshot.text, "unsaved work");
        assert!(restored.is_dirty);
        assert_eq!(restored.recovery.source_path, Some(source_path));
        assert_eq!(restored.recovery.temp_path, Some(temp_path));
    }

    #[test]
    fn falls_back_to_source_when_temporary_file_is_missing() {
        let dir = tempdir().unwrap();
        let source_path = dir.path().join("note.md");
        let temp_path = dir.path().join(".missing.temp");
        fs::write(&source_path, "saved contents").unwrap();
        let record = text_tab_record(Some(source_path.clone()), Some(temp_path));

        let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();

        assert_eq!(restored.path, Some(source_path));
        assert_eq!(restored.snapshot.text, "saved contents");
        assert!(!restored.is_dirty);
        assert_eq!(restored.recovery, TabRecoveryState::default());
    }

    #[test]
    fn matching_temporary_file_is_cleaned_after_restore() {
        let dir = tempdir().unwrap();
        let source_path = dir.path().join("note.md");
        let temp_path = dir.path().join(".note.md.temp");
        fs::write(&source_path, "same contents").unwrap();
        fs::write(&temp_path, "same contents").unwrap();
        let record = text_tab_record(Some(source_path), Some(temp_path.clone()));

        let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();

        assert!(!restored.is_dirty);
        assert_eq!(restored.recovery, TabRecoveryState::default());
        assert!(!temp_path.exists());
    }

    #[test]
    fn external_source_change_restores_draft_without_overwrite_path() {
        let dir = tempdir().unwrap();
        let source_path = dir.path().join("note.md");
        let temp_path = dir.path().join(".note.md.temp");
        fs::write(&source_path, "external edit").unwrap();
        fs::write(&temp_path, "unsaved draft").unwrap();
        let mut record = text_tab_record(Some(source_path.clone()), Some(temp_path));
        record.recovery.source_content_hash = Some(file_ops::content_hash(b"original"));

        let restored = restore_tab_from_record(&record, UiLanguage::English).unwrap();

        assert_eq!(restored.path, None);
        assert_eq!(restored.recovery.source_path, Some(source_path.clone()));
        assert_eq!(restored.snapshot.text, "unsaved draft");
        assert_eq!(fs::read_to_string(source_path).unwrap(), "external edit");
    }

    #[test]
    fn missing_source_and_temporary_files_drop_unrestorable_tab() {
        let dir = tempdir().unwrap();
        let record = text_tab_record(
            Some(dir.path().join("missing.md")),
            Some(dir.path().join(".missing.md.temp")),
        );

        assert!(restore_tab_from_record(&record, UiLanguage::English).is_none());
    }

    #[test]
    fn restores_multiple_independent_recovery_records() {
        let dir = tempdir().unwrap();
        let records = (0..8)
            .map(|index| {
                let source = dir.path().join(format!("note-{index}.md"));
                let temp = dir.path().join(format!(".note-{index}.md.temp"));
                fs::write(&source, format!("saved {index}")).unwrap();
                fs::write(&temp, format!("draft {index}")).unwrap();
                let mut record = text_tab_record(Some(source), Some(temp));
                record.id = format!("tab-{index}");
                record
            })
            .collect::<Vec<_>>();

        let restored = records
            .iter()
            .map(|record| restore_tab_from_record(record, UiLanguage::English).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(restored.len(), 8);
        for (index, tab) in restored.iter().enumerate() {
            assert_eq!(tab.snapshot.text, format!("draft {index}"));
            assert!(tab.is_dirty);
        }
    }
}
