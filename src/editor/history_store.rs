use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::EditorSnapshot;
use super::config_paths;
use super::file_ops;

const HISTORY_SCHEMA_VERSION: u32 = 1;
pub(crate) const SNAPSHOT_INTERVAL: usize = 50;
pub(crate) const MAX_HISTORY_ENTRIES: usize = 10_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TabHistory {
    pub undo_stack: Vec<HistoryEntry>,
    pub redo_stack: Vec<HistoryEntry>,
    pub journal_path: Option<PathBuf>,
    pub source_identity: Option<String>,
}

/// In-memory history entry. Only the top of each stack keeps a full snapshot;
/// entries below it store a delta relative to the state of the entry directly
/// above them (which is the live editor state by the time they are popped).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HistoryEntry {
    Full(EditorSnapshot),
    Delta {
        start: usize,
        /// Text present in this entry's state (restored on pop).
        deleted_text: String,
        /// Text present in the successor state (removed on pop).
        inserted_text: String,
        view: SnapshotViewState,
    },
}

fn snapshot_from_view(text: String, view: &SnapshotViewState) -> EditorSnapshot {
    EditorSnapshot {
        text,
        cursor_offset: view.cursor_offset,
        cursor_anchor: view.cursor_anchor,
        scroll_offset: view.scroll_offset,
        extra_cursors: view.extra_cursors.clone(),
    }
}

/// Push a full snapshot, converting the previous full top into a delta
/// relative to the new snapshot's text.
fn push_entry(stack: &mut Vec<HistoryEntry>, snapshot: EditorSnapshot) {
    if let Some(HistoryEntry::Full(previous)) = stack.last() {
        let (start, deleted_text, inserted_text) = compute_text_diff(previous, &snapshot);
        let view = SnapshotViewState::from(previous);
        let entry = HistoryEntry::Delta {
            start,
            deleted_text,
            inserted_text,
            view,
        };
        let last_index = stack.len() - 1;
        stack[last_index] = entry;
    }
    stack.push(HistoryEntry::Full(snapshot));
}

/// Pop the top entry, reconstructing its snapshot. Delta entries are applied
/// to `live`, which must be the state the delta was encoded against.
fn pop_entry(stack: &mut Vec<HistoryEntry>, live: &EditorSnapshot) -> Option<EditorSnapshot> {
    match stack.last()? {
        HistoryEntry::Full(_) => {
            let Some(HistoryEntry::Full(snapshot)) = stack.pop() else {
                unreachable!()
            };
            Some(snapshot)
        }
        HistoryEntry::Delta {
            start,
            deleted_text,
            inserted_text,
            view,
        } => {
            let end = start + inserted_text.len();
            if end > live.text.len()
                || !live.text.is_char_boundary(*start)
                || live.text.get(*start..end) != Some(inserted_text.as_str())
            {
                debug_assert!(false, "history delta out of sync with live buffer");
                return None;
            }
            let mut text = live.text.clone();
            text.replace_range(*start..end, deleted_text);
            let snapshot = snapshot_from_view(text, view);
            stack.pop();
            Some(snapshot)
        }
    }
}

/// Decode a delta stack back into full snapshots (bottom..top order).
fn decode_stack(entries: &[HistoryEntry]) -> io::Result<Vec<EditorSnapshot>> {
    let mut snapshots: Vec<EditorSnapshot> = Vec::with_capacity(entries.len());
    let mut successor: Option<EditorSnapshot> = None;
    for entry in entries.iter().rev() {
        let snapshot = match entry {
            HistoryEntry::Full(snapshot) => snapshot.clone(),
            HistoryEntry::Delta {
                start,
                deleted_text,
                inserted_text,
                view,
            } => {
                let base = successor.as_ref().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "delta entry without successor")
                })?;
                let end = start + inserted_text.len();
                if end > base.text.len()
                    || !base.text.is_char_boundary(*start)
                    || base.text.get(*start..end) != Some(inserted_text.as_str())
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid history delta",
                    ));
                }
                let mut text = base.text.clone();
                text.replace_range(*start..end, deleted_text);
                snapshot_from_view(text, view)
            }
        };
        successor = Some(snapshot.clone());
        snapshots.push(snapshot);
    }
    snapshots.reverse();
    Ok(snapshots)
}

/// Encode full snapshots (bottom..top order) into a delta stack.
fn encode_stack(snapshots: &[EditorSnapshot]) -> Vec<HistoryEntry> {
    let mut stack = Vec::with_capacity(snapshots.len());
    for snapshot in snapshots {
        push_entry(&mut stack, snapshot.clone());
    }
    stack
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
        push_entry(&mut self.undo_stack, snapshot);
        self.redo_stack.clear();
        if self.undo_stack.len() > MAX_HISTORY_ENTRIES {
            let overflow = self.undo_stack.len() - MAX_HISTORY_ENTRIES;
            self.undo_stack.drain(0..overflow);
        }
    }

    pub(crate) fn take_undo(&mut self, current: &EditorSnapshot) -> Option<EditorSnapshot> {
        let snapshot = pop_entry(&mut self.undo_stack, current)?;
        push_entry(&mut self.redo_stack, current.clone());
        Some(snapshot)
    }

    pub(crate) fn take_redo(&mut self, current: &EditorSnapshot) -> Option<EditorSnapshot> {
        let snapshot = pop_entry(&mut self.redo_stack, current)?;
        push_entry(&mut self.undo_stack, current.clone());
        Some(snapshot)
    }

    pub(crate) fn current_index(&self) -> usize {
        self.undo_stack.len()
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.undo_stack.len() + self.redo_stack.len() + 1
    }
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
    let journal = build_journal(history, current_snapshot, source_identity.clone())?;
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
) -> io::Result<UndoJournal> {
    let mut timeline = decode_stack(&history.undo_stack)?;
    timeline.push(current_snapshot.clone());
    let mut redo_snapshots = decode_stack(&history.redo_stack)?;
    redo_snapshots.reverse();
    timeline.extend(redo_snapshots);

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

    Ok(UndoJournal {
        schema_version: HISTORY_SCHEMA_VERSION,
        source_identity,
        current_index: history.current_index(),
        entries,
    })
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
    let undo_stack = encode_stack(&timeline[..journal.current_index]);
    let redo_snapshots: Vec<EditorSnapshot> = timeline[journal.current_index + 1..]
        .iter()
        .rev()
        .cloned()
        .collect();
    let redo_stack = encode_stack(&redo_snapshots);
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
    file_ops::atomic_write(path, content)
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
        history.redo_stack = encode_stack(&[
            snapshot("hello world!!!", 14),
            snapshot("hello world!!", 13),
        ]);
        let current = snapshot("hello world!", 12);

        let journal = build_journal(&history, &current, "/tmp/note.md".to_string()).unwrap();
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
        let journal = build_journal(&history, &current, "source".to_string()).unwrap();

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
        let decoded = decode_stack(&history.undo_stack).unwrap();
        assert_eq!(decoded[0].text, "v4");
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

    #[test]
    fn undo_and_redo_roundtrip_through_delta_entries() {
        let states = [
            snapshot("", 0),
            snapshot("hello", 5),
            snapshot("hello 世界", 12),
            snapshot("hello 世界!", 13),
        ];
        let mut history = TabHistory::default();
        for state in &states[..3] {
            history.push_undo_snapshot(state.clone());
        }
        // Only the top of the stack keeps a full snapshot.
        assert_eq!(
            history
                .undo_stack
                .iter()
                .filter(|entry| matches!(entry, HistoryEntry::Full(_)))
                .count(),
            1
        );

        let mut current = states[3].clone();
        // Undo all the way back.
        for expected in states[..3].iter().rev() {
            let restored = history.take_undo(&current).expect("undo entry");
            assert_eq!(&restored, expected);
            current = restored;
        }
        assert!(history.undo_stack.is_empty());

        // Redo all the way forward.
        for expected in &states[1..] {
            let restored = history.take_redo(&current).expect("redo entry");
            assert_eq!(&restored, expected);
            current = restored;
        }
        assert!(history.redo_stack.is_empty());
        assert_eq!(current, states[3]);
    }
}
