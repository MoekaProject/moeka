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
        if let Some(tab) = restore_tab_from_record(record) {
            restored_tabs.push(tab);
        }
    }

    if restored_tabs.is_empty() {
        return;
    }

    if !restored_tabs
        .iter()
        .any(|tab| tab.kind == TabKind::Settings)
    {
        restored_tabs.insert(0, settings_tab("settings-1".to_string()));
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

fn restore_tab_from_record(record: &TabSessionRecord) -> Option<OpenTab> {
    let kind = deserialize_tab_kind(&record.kind)?;
    if kind == TabKind::Settings {
        return Some(settings_tab(record.id.clone()));
    }

    let file_kind = record.file_kind.as_deref().and_then(deserialize_file_kind);
    let (path, snapshot, is_dirty) = resolve_snapshot(record, file_kind.as_ref())?;
    let mut history = history_store::load_tab_history_from_path(
        &record.id,
        path.as_deref(),
        record.history.undo_log_path.as_deref(),
        &snapshot,
    );
    if let Some(source_identity) = record.history.source_identity.clone() {
        history.source_identity = Some(source_identity);
    }

    Some(OpenTab {
        id: record.id.clone(),
        title: record.title.clone(),
        kind,
        path,
        file_kind,
        snapshot,
        image_view: ImageViewState::default(),
        git_diff: None,
        is_preview: record.is_preview,
        is_dirty,
        is_deleted: false,
        is_pinned: record.is_pinned,
        recovery: record.recovery.clone(),
        history,
    })
}

fn resolve_snapshot(
    record: &TabSessionRecord,
    file_kind: Option<&FileKind>,
) -> Option<(Option<PathBuf>, EditorSnapshot, bool)> {
    let temp_text = record
        .recovery
        .temp_path
        .as_ref()
        .and_then(|path| fs::read_to_string(path).ok());

    match record.path.as_ref() {
        Some(path) if path.exists() => {
            let (mut snapshot, resolved_kind) = load_file_snapshot(path)?;
            if resolved_kind.supports_text_buffer() {
                if let Some(text) = temp_text {
                    if snapshot.text != text {
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
                        return Some((Some(path.clone()), snapshot, true));
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
        FileKind::Code { language } => format!("code:{language}"),
    }
}

fn deserialize_file_kind(value: &str) -> Option<FileKind> {
    match value {
        "markdown" => Some(FileKind::Markdown),
        "text" => Some(FileKind::Text),
        "image" => Some(FileKind::Image),
        "binary" => Some(FileKind::Binary),
        _ => value.strip_prefix("code:").map(|language| FileKind::Code {
            language: language.to_string(),
        }),
    }
}

fn settings_tab(id: String) -> OpenTab {
    OpenTab {
        id,
        title: "设置".to_string(),
        kind: TabKind::Settings,
        path: None,
        file_kind: None,
        snapshot: EditorSnapshot {
            text: String::new(),
            cursor_offset: 0,
            cursor_anchor: 0,
            scroll_offset: 0.0,
            extra_cursors: Vec::new(),
        },
        image_view: ImageViewState::default(),
        git_diff: None,
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
                snapshot: EditorSnapshot {
                    text: String::new(),
                    cursor_offset: 0,
                    cursor_anchor: 0,
                    scroll_offset: 0.0,
                    extra_cursors: Vec::new(),
                },
                image_view: ImageViewState::default(),
                git_diff: None,
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

        let restored = restore_tab_from_record(&record).unwrap();

        assert_eq!(restored.history.journal_path, Some(custom_journal_path));
        assert_eq!(restored.history.undo_stack, vec![snapshot("before", 6)]);
        assert!(restored.history.redo_stack.is_empty());
    }
}
