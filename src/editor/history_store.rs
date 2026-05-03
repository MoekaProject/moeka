use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::EditorSnapshot;
use super::config_paths;

const HISTORY_SCHEMA_VERSION: u32 = 1;
pub(crate) const SNAPSHOT_INTERVAL: usize = 50;
pub(crate) const MAX_HISTORY_ENTRIES: usize = 10_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TabHistory {
    pub undo_stack: Vec<EditorSnapshot>,
    pub redo_stack: Vec<EditorSnapshot>,
    pub journal_path: Option<PathBuf>,
    pub source_identity: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub(crate) struct UndoJournal {
    pub schema_version: u32,
    pub source_identity: String,
    pub current_index: usize,
    pub entries: Vec<UndoJournalEntry>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(tag = "kind")]
pub(crate) enum UndoJournalEntry {
    Snapshot {
        snapshot: EditorSnapshot,
    },
    Edit {
        start: usize,
        deleted_text: String,
        inserted_text: String,
        before_view: SnapshotViewState,
        after_view: SnapshotViewState,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub(crate) struct SnapshotViewState {
    pub cursor_offset: usize,
    pub cursor_anchor: usize,
    pub scroll_offset: f32,
    #[serde(default)]
    pub extra_cursors: Vec<(usize, usize)>,
}

impl TabHistory {
    pub(crate) fn new(source_identity: Option<String>, journal_path: Option<PathBuf>) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            journal_path,
            source_identity,
        }
    }

    pub(crate) fn push_undo_snapshot(&mut self, snapshot: EditorSnapshot) {
        self.undo_stack.push(snapshot);
        self.redo_stack.clear();
        if self.undo_stack.len() > MAX_HISTORY_ENTRIES {
            let overflow = self.undo_stack.len() - MAX_HISTORY_ENTRIES;
            self.undo_stack.drain(0..overflow);
        }
    }

    pub(crate) fn take_undo(&mut self, current: &EditorSnapshot) -> Option<EditorSnapshot> {
        let snapshot = self.undo_stack.pop()?;
        self.redo_stack.push(current.clone());
        Some(snapshot)
    }

    pub(crate) fn take_redo(&mut self, current: &EditorSnapshot) -> Option<EditorSnapshot> {
        let snapshot = self.redo_stack.pop()?;
        self.undo_stack.push(current.clone());
        Some(snapshot)
    }

    pub(crate) fn current_index(&self) -> usize {
        self.undo_stack.len()
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.undo_stack.len() + self.redo_stack.len() + 1
    }
}

pub(crate) fn default_source_identity(tab_id: &str, source_path: Option<&Path>) -> String {
    source_path
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("untitled:{tab_id}"))
}

pub(crate) fn default_journal_path(tab_id: &str, source_path: Option<&Path>) -> PathBuf {
    let file_name = match source_path {
        Some(path) => config_paths::history_file_name_for_source(path),
        None => config_paths::history_file_name_for_untitled(tab_id),
    };
    config_paths::history_dir().join(file_name)
}

pub(crate) fn load_tab_history(
    tab_id: &str,
    source_path: Option<&Path>,
    expected_current: &EditorSnapshot,
) -> TabHistory {
    load_tab_history_from_path(tab_id, source_path, None, expected_current)
}

pub(crate) fn load_tab_history_from_path(
    tab_id: &str,
    source_path: Option<&Path>,
    journal_path: Option<&Path>,
    expected_current: &EditorSnapshot,
) -> TabHistory {
    let journal_path = journal_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_journal_path(tab_id, source_path));
    let source_identity = default_source_identity(tab_id, source_path);

    let Ok(content) = fs::read_to_string(&journal_path) else {
        return TabHistory::new(Some(source_identity), Some(journal_path));
    };
    let Ok(journal) = serde_json::from_str::<UndoJournal>(&content) else {
        return TabHistory::new(Some(source_identity), Some(journal_path));
    };
    let Ok((mut history, current_snapshot)) = history_from_journal(&journal) else {
        return TabHistory::new(Some(source_identity), Some(journal_path));
    };
    if &current_snapshot != expected_current {
        return TabHistory::new(Some(source_identity), Some(journal_path));
    }
    history.journal_path = Some(journal_path);
    history.source_identity = Some(journal.source_identity);
    history
}

pub(crate) fn persist_tab_history(
    tab_id: &str,
    source_path: Option<&Path>,
    history: &mut TabHistory,
    current_snapshot: &EditorSnapshot,
) -> io::Result<PathBuf> {
    let path = history
        .journal_path
        .clone()
        .unwrap_or_else(|| default_journal_path(tab_id, source_path));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let source_identity = history
        .source_identity
        .clone()
        .unwrap_or_else(|| default_source_identity(tab_id, source_path));
    let journal = build_journal(history, current_snapshot, source_identity.clone());
    let content = serde_json::to_vec_pretty(&journal)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    write_atomic(&path, &content)?;
    history.journal_path = Some(path.clone());
    history.source_identity = Some(source_identity);
    Ok(path)
}

fn build_journal(
    history: &TabHistory,
    current_snapshot: &EditorSnapshot,
    source_identity: String,
) -> UndoJournal {
    let mut timeline = history.undo_stack.clone();
    timeline.push(current_snapshot.clone());
    timeline.extend(history.redo_stack.iter().rev().cloned());

    let entries = timeline
        .iter()
        .enumerate()
        .map(|(index, snapshot)| {
            if index == 0 || index % SNAPSHOT_INTERVAL == 0 {
                UndoJournalEntry::Snapshot {
                    snapshot: snapshot.clone(),
                }
            } else {
                let previous = &timeline[index - 1];
                let (start, deleted_text, inserted_text) = compute_text_diff(previous, snapshot);
                UndoJournalEntry::Edit {
                    start,
                    deleted_text,
                    inserted_text,
                    before_view: SnapshotViewState::from(previous),
                    after_view: SnapshotViewState::from(snapshot),
                }
            }
        })
        .collect();

    UndoJournal {
        schema_version: HISTORY_SCHEMA_VERSION,
        source_identity,
        current_index: history.current_index(),
        entries,
    }
}

fn history_from_journal(journal: &UndoJournal) -> io::Result<(TabHistory, EditorSnapshot)> {
    let timeline = rebuild_timeline(&journal.entries)?;
    if timeline.is_empty() || journal.current_index >= timeline.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid history journal index",
        ));
    }
    let current = timeline[journal.current_index].clone();
    let undo_stack = timeline[..journal.current_index].to_vec();
    let redo_stack = timeline[journal.current_index + 1..]
        .iter()
        .rev()
        .cloned()
        .collect();
    Ok((
        TabHistory {
            undo_stack,
            redo_stack,
            journal_path: None,
            source_identity: Some(journal.source_identity.clone()),
        },
        current,
    ))
}

fn rebuild_timeline(entries: &[UndoJournalEntry]) -> io::Result<Vec<EditorSnapshot>> {
    let mut timeline = Vec::with_capacity(entries.len());
    for entry in entries {
        match entry {
            UndoJournalEntry::Snapshot { snapshot } => timeline.push(snapshot.clone()),
            UndoJournalEntry::Edit {
                start,
                deleted_text,
                inserted_text,
                after_view,
                ..
            } => {
                let base = timeline.last().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "edit entry without base snapshot",
                    )
                })?;
                timeline.push(apply_edit(
                    base,
                    *start,
                    deleted_text,
                    inserted_text,
                    after_view,
                )?);
            }
        }
    }
    Ok(timeline)
}

fn apply_edit(
    base: &EditorSnapshot,
    start: usize,
    deleted_text: &str,
    inserted_text: &str,
    after_view: &SnapshotViewState,
) -> io::Result<EditorSnapshot> {
    let end = start + deleted_text.len();
    if end > base.text.len()
        || !base.text.is_char_boundary(start)
        || !base.text.is_char_boundary(end)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid edit range",
        ));
    }
    if &base.text[start..end] != deleted_text {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "edit base text mismatch",
        ));
    }
    let mut text = base.text.clone();
    text.replace_range(start..end, inserted_text);
    Ok(EditorSnapshot {
        text,
        cursor_offset: after_view.cursor_offset,
        cursor_anchor: after_view.cursor_anchor,
        scroll_offset: after_view.scroll_offset,
        extra_cursors: after_view.extra_cursors.clone(),
    })
}

fn compute_text_diff(
    previous: &EditorSnapshot,
    current: &EditorSnapshot,
) -> (usize, String, String) {
    let previous_bytes = previous.text.as_bytes();
    let current_bytes = current.text.as_bytes();

    let mut prefix = 0usize;
    while prefix < previous_bytes.len()
        && prefix < current_bytes.len()
        && previous_bytes[prefix] == current_bytes[prefix]
    {
        prefix += 1;
    }
    prefix =
        snap_char_boundary(&previous.text, prefix).min(snap_char_boundary(&current.text, prefix));

    let mut previous_suffix = previous_bytes.len();
    let mut current_suffix = current_bytes.len();
    while previous_suffix > prefix
        && current_suffix > prefix
        && previous_bytes[previous_suffix - 1] == current_bytes[current_suffix - 1]
    {
        previous_suffix -= 1;
        current_suffix -= 1;
    }
    previous_suffix = snap_char_boundary(&previous.text, previous_suffix);
    current_suffix = snap_char_boundary(&current.text, current_suffix);

    (
        prefix,
        previous.text[prefix..previous_suffix].to_string(),
        current.text[prefix..current_suffix].to_string(),
    )
}

fn snap_char_boundary(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn write_atomic(path: &Path, content: &[u8]) -> io::Result<()> {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "history.json".to_string());
    let temp_path = path.with_file_name(format!(".{file_name}.tmp"));
    fs::write(&temp_path, content)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temp_path, path)
}

impl From<&EditorSnapshot> for SnapshotViewState {
    fn from(snapshot: &EditorSnapshot) -> Self {
        Self {
            cursor_offset: snapshot.cursor_offset,
            cursor_anchor: snapshot.cursor_anchor,
            scroll_offset: snapshot.scroll_offset,
            extra_cursors: snapshot.extra_cursors.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn journal_roundtrip_preserves_stacks_and_current_snapshot() {
        let mut history = TabHistory::default();
        history.push_undo_snapshot(snapshot("hello", 5));
        history.push_undo_snapshot(snapshot("hello world", 11));
        history.redo_stack = vec![
            snapshot("hello world!!!", 14),
            snapshot("hello world!!", 13),
        ];
        let current = snapshot("hello world!", 12);

        let journal = build_journal(&history, &current, "/tmp/note.md".to_string());
        let (rebuilt, rebuilt_current) = history_from_journal(&journal).unwrap();

        assert_eq!(rebuilt.undo_stack, history.undo_stack);
        assert_eq!(rebuilt.redo_stack, history.redo_stack);
        assert_eq!(rebuilt_current, current);
    }

    #[test]
    fn inserts_snapshot_every_fifty_entries() {
        let mut history = TabHistory::default();
        for index in 0..120 {
            history.push_undo_snapshot(snapshot(&format!("v{index}"), index));
        }
        let current = snapshot("current", 7);
        let journal = build_journal(&history, &current, "source".to_string());

        assert!(matches!(
            journal.entries[0],
            UndoJournalEntry::Snapshot { .. }
        ));
        assert!(matches!(
            journal.entries[50],
            UndoJournalEntry::Snapshot { .. }
        ));
        assert!(matches!(
            journal.entries[100],
            UndoJournalEntry::Snapshot { .. }
        ));
    }

    #[test]
    fn trims_undo_stack_to_ten_thousand_entries() {
        let mut history = TabHistory::default();
        for index in 0..(MAX_HISTORY_ENTRIES + 4) {
            history.push_undo_snapshot(snapshot(&format!("v{index}"), index));
        }

        assert_eq!(history.undo_stack.len(), MAX_HISTORY_ENTRIES);
        assert_eq!(history.undo_stack[0].text, "v4");
    }

    #[test]
    fn computes_diff_on_char_boundaries_for_multibyte_text() {
        let previous = snapshot("你好 world", 2);
        let current = snapshot("你好 rust", 2);
        let (start, deleted, inserted) = compute_text_diff(&previous, &current);

        assert_eq!(start, "你好 ".len());
        assert_eq!(deleted, "world");
        assert_eq!(inserted, "rust");
    }
}
