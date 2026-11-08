use std::path::PathBuf;

use gpui::{Pixels, Point};

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
    pub line_idx: Option<usize>,
    pub view: ImageViewState,
}

#[derive(Clone)]
pub(crate) struct OpenTab {
    pub id: String,
    pub title: String,
    pub heading_title_cache: std::cell::RefCell<Option<Option<String>>>,
    pub kind: TabKind,
    pub path: Option<PathBuf>,
    pub file_kind: Option<FileKind>,
    pub editor_left_aligned: bool,
    pub reading_mode: bool,
    pub source_mode: bool,
    pub source_word_wrap: bool,
    pub snapshot: EditorSnapshot,
    pub image_view: ImageViewState,
    pub is_preview: bool,
    pub is_dirty: bool,
    pub is_deleted: bool,
    pub is_pinned: bool,
    pub recovery: config_store::TabRecoveryState,
    pub history: history_store::TabHistory,
}

impl OpenTab {
    pub(super) fn set_snapshot(&mut self, snapshot: EditorSnapshot) {
        self.heading_title_cache.get_mut().take();
        self.snapshot = snapshot;
    }

    pub(super) fn set_snapshot_text(&mut self, text: String) {
        self.heading_title_cache.get_mut().take();
        self.snapshot.text = text;
    }
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
