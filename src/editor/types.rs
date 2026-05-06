use std::path::PathBuf;

use gpui::{Pixels, Point};

use super::{config_store, history_store};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RightUtilityPanel {
    Outline,
    Info,
    GitHistory,
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
}

impl FileKind {
    pub(crate) fn supports_text_buffer(&self) -> bool {
        matches!(self, Self::Markdown | Self::Code { .. } | Self::Text)
    }

    pub(crate) fn supports_text_search(&self) -> bool {
        self.supports_text_buffer()
    }

    pub(crate) fn supports_persistent_save(&self) -> bool {
        self.supports_text_buffer()
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
    pub git_diff: Option<GitDiffState>,
    pub is_preview: bool,
    pub is_dirty: bool,
    pub is_deleted: bool,
    pub is_pinned: bool,
    pub recovery: config_store::TabRecoveryState,
    pub history: history_store::TabHistory,
}

#[derive(Clone, Debug)]
pub(crate) struct GitDiffState {
    pub source_label: String,
    pub target_label: String,
    pub rows: Vec<GitDiffRow>,
}

#[derive(Clone, Debug)]
pub(crate) struct GitDiffRow {
    pub kind: GitDiffRowKind,
    pub old_line_number: Option<usize>,
    pub new_line_number: Option<usize>,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GitDiffRowKind {
    Context,
    Added,
    Removed,
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
