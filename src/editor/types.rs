use std::ops::Range;
use std::path::PathBuf;

use gpui::{Bounds, Pixels, Point, WrappedLine};

use crate::markdown::LineDecoration;

use super::whiteboard::WhiteboardTabState;
use super::{config_store, history_store};

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct EditorSnapshot {
    pub text: String,
    pub cursor_offset: usize,
    pub cursor_anchor: usize,
    pub scroll_offset: f32,
    #[serde(default)]
    pub extra_cursors: Vec<(usize, usize)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WrapBoundaryAffinityKind {
    Upstream,
    Downstream,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WrapBoundaryAffinity {
    pub offset: usize,
    pub kind: WrapBoundaryAffinityKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabKind {
    Editor,
    Diff,
    Settings,
    Terminal(ChatAgentCli),
}

impl TabKind {
    pub(crate) fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal(_))
    }

    pub(crate) fn terminal_agent(&self) -> Option<ChatAgentCli> {
        match self {
            Self::Terminal(agent) => Some(*agent),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RightUtilityPanel {
    Info,
    GitHistory,
    Outline,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TerminalCursorShape {
    #[default]
    Block,
    Underline,
    Bar,
    Hollow,
}

impl TerminalCursorShape {
    pub(crate) const ALL: [Self; 4] = [Self::Block, Self::Underline, Self::Bar, Self::Hollow];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Underline => "underline",
            Self::Bar => "bar",
            Self::Hollow => "hollow",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "block" => Some(Self::Block),
            "underline" => Some(Self::Underline),
            "bar" => Some(Self::Bar),
            "hollow" => Some(Self::Hollow),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChatAgentCli {
    GithubCopilot,
    Terminal,
    OpenAICodex,
    ClaudeCode,
    GeminiCli,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TabNavigationTarget {
    Settings,
    File(PathBuf),
    LiveTab(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FileKind {
    Markdown,
    Code { language: String },
    Text,
    Image,
    Binary,
    Whiteboard,
}

impl FileKind {
    pub(crate) fn supports_text_buffer(&self) -> bool {
        matches!(self, Self::Markdown | Self::Code { .. } | Self::Text)
    }

    pub(crate) fn supports_text_search(&self) -> bool {
        self.supports_text_buffer()
    }

    pub(crate) fn supports_persistent_save(&self) -> bool {
        self.supports_text_buffer() || self.is_whiteboard()
    }

    pub(crate) fn is_markdown(&self) -> bool {
        matches!(self, Self::Markdown)
    }

    pub(crate) fn is_image(&self) -> bool {
        matches!(self, Self::Image)
    }

    pub(crate) fn is_binary(&self) -> bool {
        matches!(self, Self::Binary)
    }

    pub(crate) fn is_whiteboard(&self) -> bool {
        matches!(self, Self::Whiteboard)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ImageViewState {
    pub zoom_factor: f32,
    pub pan_x: f32,
    pub pan_y: f32,
}

impl Default for ImageViewState {
    fn default() -> Self {
        Self {
            zoom_factor: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ImageDragState {
    pub start_position: Point<Pixels>,
    pub start_pan_x: f32,
    pub start_pan_y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GeneratedPreviewAction {
    Fullscreen,
    Restore,
    ZoomIn,
    ZoomOut,
    Copy,
    Code,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TableBoundarySide {
    Above,
    Below,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TableBoundaryCaret {
    pub(crate) table_start_line: usize,
    pub(crate) side: TableBoundarySide,
}

#[derive(Clone, Debug)]
pub(crate) struct GeneratedPreviewDragState {
    pub cache_key: String,
    pub start_position: Point<Pixels>,
    pub start_pan_x: f32,
    pub start_pan_y: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct PreviewOverlayState {
    pub cache_key: String,
    pub line_idx: usize,
    pub view: ImageViewState,
}

#[derive(Clone)]
pub(crate) struct OpenTab {
    pub id: String,
    pub title: String,
    pub kind: TabKind,
    pub path: Option<PathBuf>,
    pub file_kind: Option<FileKind>,
    pub editor_left_aligned: bool,
    pub snapshot: EditorSnapshot,
    pub image_view: ImageViewState,
    pub whiteboard: Option<WhiteboardTabState>,
    pub git_diff: Option<GitDiffState>,
    pub inline_diff: Option<InlineDiffState>,
    pub is_preview: bool,
    pub is_dirty: bool,
    pub is_deleted: bool,
    pub is_pinned: bool,
    pub recovery: config_store::TabRecoveryState,
    pub history: history_store::TabHistory,
}

#[derive(Clone, Debug)]
pub(crate) struct GitDiffState {
    pub source: GitDiffSource,
    pub source_label: String,
    pub target_label: String,
    pub all_rows: Vec<GitDiffRow>,
    pub rows: Vec<GitDiffRow>,
    pub show_only_changes: bool,
    /// Hunks in base/target line coordinates, for per-hunk revert.
    pub hunks: Vec<GitDiffHunk>,
    /// The base side's full text (index or parent revision).
    pub base_text: String,
    /// The target side's full text (worktree, index, or commit).
    pub target_text: String,
}

/// One contiguous changed region of a git diff, as end-exclusive 0-based line
/// ranges into the base and target texts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GitDiffHunk {
    pub base_range: Range<usize>,
    pub target_range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GitDiffSource {
    /// Index vs worktree (unstaged changes).
    IndexToWorktree,
    /// HEAD vs index (staged changes).
    HeadToIndex,
    Commit {
        commit_hash: String,
        short_hash: String,
    },
}

impl GitDiffSource {
    /// True when the diff depends on the worktree or index and must refresh
    /// after local file/staging changes (not only on new commits).
    pub(crate) fn tracks_local_state(&self) -> bool {
        matches!(self, Self::IndexToWorktree | Self::HeadToIndex)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GitDiffRow {
    pub kind: GitDiffRowKind,
    pub text: String,
    pub decoration: LineDecoration,
    pub source_line_number: Option<usize>,
    pub is_hunk_label: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GitDiffRowKind {
    Context,
    Added,
    Removed,
}

/// In-place (working-tree vs HEAD) diff overlay for an editable file tab.
///
/// Unlike [`GitDiffState`], which drives a dedicated read-only diff tab, this
/// state is layered on top of the live, editable buffer: hunks are keyed by the
/// current buffer's line indices so the editor can draw gutter indicators,
/// inline highlights, and (when a hunk is expanded) read-only phantom rows for
/// the removed lines, all while the added/modified lines stay editable.
#[derive(Clone, Debug, Default)]
pub(crate) struct InlineDiffState {
    /// The HEAD version of the file, kept so a hunk can be reverted to its
    /// original lines.
    pub base_text: String,
    /// Hunks ordered by `buffer_start`.
    pub hunks: Vec<InlineDiffHunk>,
}

#[derive(Clone, Debug)]
pub(crate) struct InlineDiffHunk {
    pub kind: InlineDiffHunkKind,
    /// Buffer (current file) line range occupied by this hunk's added/modified
    /// lines, 0-based and end-exclusive. For a pure deletion the range is empty
    /// (`buffer_start == buffer_end`); the removed phantom rows attach directly
    /// above `buffer_start`.
    pub buffer_start: usize,
    pub buffer_end: usize,
    /// Base (HEAD) line range removed or replaced, 0-based and end-exclusive.
    /// Doubles as a stable identity for preserving expansion across edits, since
    /// base line numbers do not shift while HEAD is unchanged.
    pub base_start: usize,
    pub base_end: usize,
    /// Removed base lines, rendered as read-only phantom rows when expanded.
    pub removed_lines: Vec<InlineDiffRemovedLine>,
    /// Inline added character spans per buffer line in `buffer_start..buffer_end`
    /// (index 0 == `buffer_start`). An empty span list means the whole line is new.
    pub added_inline_spans: Vec<Vec<Range<usize>>>,
    /// Whether the hunk is currently expanded to show removed rows in place.
    pub expanded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InlineDiffHunkKind {
    Added,
    Removed,
    Modified,
}

#[derive(Clone, Debug)]
pub(crate) struct InlineDiffRemovedLine {
    pub text: String,
    pub removed_spans: Vec<Range<usize>>,
    /// Markdown decoration parsed from the HEAD line, so removed rows render
    /// with the same syntax styling (and soft-wrap) as live editor lines.
    pub decoration: LineDecoration,
}

/// A read-only character-level selection over an expanded hunk's removed
/// (phantom) rows. Rows are indexed within the hunk; offsets are byte offsets
/// within each row's text. The head is where the caret sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PhantomSelection {
    pub hunk_base_start: usize,
    pub anchor_row: usize,
    pub anchor_offset: usize,
    pub head_row: usize,
    pub head_offset: usize,
}

impl PhantomSelection {
    pub fn is_collapsed(&self) -> bool {
        self.anchor_row == self.head_row && self.anchor_offset == self.head_offset
    }

    /// `(start_row, start_offset, end_row, end_offset)` in document order.
    pub fn ordered(&self) -> (usize, usize, usize, usize) {
        if (self.anchor_row, self.anchor_offset) <= (self.head_row, self.head_offset) {
            (
                self.anchor_row,
                self.anchor_offset,
                self.head_row,
                self.head_offset,
            )
        } else {
            (
                self.head_row,
                self.head_offset,
                self.anchor_row,
                self.anchor_offset,
            )
        }
    }
}

/// Per-rendered phantom row geometry, captured during prepaint so the next
/// mouse event can hit-test the soft-wrapped removed line and map a screen
/// position to a byte offset.
#[derive(Clone)]
pub(crate) struct PhantomRowHit {
    pub bounds: Bounds<Pixels>,
    pub hunk_base_start: usize,
    pub row: usize,
    pub content_x: f32,
    pub row_top: f32,
    pub line_height: f32,
    pub wrapped: WrappedLine,
    pub text_len: usize,
    /// Maps a display byte index (into the possibly-folded rendered text) back to
    /// a byte offset in the removed line's raw text, so clicks on a folded row
    /// resolve to the correct caret offset.
    pub display_to_buffer: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarGitStatus {
    Added,
    Modified,
}

#[derive(Clone, Debug)]
pub(crate) struct SidebarItem {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub is_expanded: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct PendingSidebarScrollAnchor {
    pub candidates: Vec<(PathBuf, f32)>,
    pub fallback_scroll_top: f32,
}
