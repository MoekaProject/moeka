use std::collections::HashMap;
use std::io::{Read, Write};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use std::{env, ffi::OsString};

use alacritty_terminal::{
    event::{Event as AlacTermEvent, EventListener},
    grid::{Dimensions, Scroll},
    index::{Column, Direction as AlacDirection, Line, Point as AlacPoint},
    sync::FairMutex,
    term::{
        Config, RenderableCursor, Term, TermMode,
        cell::{Cell, Flags},
        search::{Match as TermSearchMatch, RegexIter, RegexSearch},
    },
    vte::ansi::{
        ClearMode, Color as AlacColor, CursorShape as AlacCursorShape,
        CursorStyle as AlacCursorStyle, Handler as AnsiHandler, NamedColor, Processor, Rgb,
    },
};
use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, Font,
    FontFallbacks, FontStyle, FontWeight, GlobalElementId, Hsla, InputHandler, KeyDownEvent,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollWheelEvent, SharedString, Style, TextRun as GpuiTextRun, TouchPhase, UTF16Selection,
    Window, div, fill, point, prelude::*, px, relative, size,
};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use super::*;

const CHAT_PANEL_DEFAULT_ROWS: u16 = 28;
const CHAT_PANEL_DEFAULT_COLS: u16 = 96;
const CHAT_PANEL_POLL_MS: u64 = 33;
const TERMINAL_PROCESS_INFO_POLL_MS: u64 = 500;
const TERMINAL_SEARCH_MAX_MATCHES: usize = 1000;

fn alacritty_cursor_style(shape: TerminalCursorShape) -> AlacCursorStyle {
    let shape = match shape {
        TerminalCursorShape::Block => AlacCursorShape::Block,
        TerminalCursorShape::Underline => AlacCursorShape::Underline,
        TerminalCursorShape::Bar => AlacCursorShape::Beam,
        TerminalCursorShape::Hollow => AlacCursorShape::HollowBlock,
    };
    AlacCursorStyle {
        shape,
        blinking: false,
    }
}

fn terminal_config(cursor_shape: TerminalCursorShape) -> Config {
    let mut config = Config::default();
    config.default_cursor_style = alacritty_cursor_style(cursor_shape);
    config
}

fn terminal_process_title(cwd: &std::path::Path, process_name: &str, argv: &[String]) -> String {
    let directory = cwd
        .file_name()
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| cwd.to_string_lossy().into_owned());
    let command_name = std::path::Path::new(process_name)
        .file_name()
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| process_name.to_string());
    let arguments = argv
        .iter()
        .skip(1)
        .filter(|argument| !argument.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let command = if arguments.is_empty() {
        command_name
    } else {
        format!("{command_name} {arguments}")
    };
    format!("{directory} - {command}")
}

pub(crate) struct ChatTerminalSession {
    pub(crate) status: ChatTerminalStatus,
    pub(crate) snapshot: ChatTerminalSnapshot,
    dynamic_title: Option<String>,
    runtime: Option<ChatTerminalRuntime>,
    receiver: Option<mpsc::Receiver<ChatTerminalEvent>>,
    pub(crate) poll_task: Option<Task<()>>,
    rows: u16,
    cols: u16,
    /// Dedicated agent/custom commands are meaningful long-running work even
    /// when their process is the PTY's direct child.
    warn_on_close: bool,
}

/// Per-terminal-tab runtime state, keyed by [`OpenTab::id`] in
/// `EditorView::terminal_tab_states`. Lives outside `OpenTab` because the PTY
/// session is not cloneable, and outside the per-pane swap state so terminal
/// tabs can move freely between panes.
pub(crate) struct TerminalTabState {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) session: Option<ChatTerminalSession>,
    pub(crate) input_bounds: Option<Bounds<Pixels>>,
    marked_text: Option<String>,
    selection: Option<ChatTerminalSelection>,
    selecting: bool,
    scroll_px: Pixels,
    mouse_scroll_px: Point<Pixels>,
    mouse_left_pressed: bool,
    mouse_middle_pressed: bool,
    mouse_right_pressed: bool,
    last_mouse_cell: Option<(usize, usize)>,
    /// Active in-terminal search matches, in absolute grid coordinates.
    search_matches: Vec<TermSearchMatch>,
    /// Compiled regex pattern of the active search, kept so matches can be
    /// recomputed when new terminal output arrives.
    search_pattern: Option<String>,
}

impl TerminalTabState {
    fn new(cx: &mut Context<EditorView>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            session: None,
            input_bounds: None,
            marked_text: None,
            selection: None,
            selecting: false,
            scroll_px: Pixels::ZERO,
            mouse_scroll_px: point(Pixels::ZERO, Pixels::ZERO),
            mouse_left_pressed: false,
            mouse_middle_pressed: false,
            mouse_right_pressed: false,
            last_mouse_cell: None,
            search_matches: Vec::new(),
            search_pattern: None,
        }
    }

    fn clear_interaction_state(&mut self) {
        self.marked_text = None;
        self.selection = None;
        self.selecting = false;
        self.mouse_scroll_px = point(Pixels::ZERO, Pixels::ZERO);
        self.mouse_left_pressed = false;
        self.mouse_middle_pressed = false;
        self.mouse_right_pressed = false;
        self.last_mouse_cell = None;
    }
}

struct ChatTerminalRuntime {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    term: Arc<FairMutex<Term<ChatTerminalListener>>>,
    config: Config,
    process_system: System,
    process_refresh_kind: ProcessRefreshKind,
    last_process_info_refresh: Instant,
    last_foreground_pid: Option<Pid>,
    reader_thread: Option<JoinHandle<()>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ChatTerminalStatus {
    Launching,
    Running,
    Exited,
    Unavailable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalStatusSummary {
    pub(crate) agent: ChatAgentCli,
    pub(crate) title: String,
    pub(crate) history_size: usize,
    pub(crate) display_offset: usize,
}

impl TerminalStatusSummary {
    pub(crate) fn scrollback(&self) -> Option<(usize, usize)> {
        (self.display_offset > 0).then_some((self.display_offset, self.history_size))
    }
}

#[derive(Clone)]
pub(crate) struct ChatTerminalSnapshot {
    rows: u16,
    cols: u16,
    cells: Vec<ChatIndexedCell>,
    cursor: RenderableCursor,
    display_offset: usize,
    history_size: usize,
}

#[derive(Clone)]
struct ChatIndexedCell {
    point: AlacPoint,
    cell: Cell,
}

enum ChatTerminalEvent {
    Updated,
    Exited,
    Error(String),
}

#[derive(Clone)]
struct ChatTerminalListener {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

#[derive(Clone, Copy)]
struct ChatTerminalSize {
    rows: usize,
    cols: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ChatTerminalPalette {
    foreground: Hsla,
    bright_foreground: Hsla,
    dim_foreground: Hsla,
    black: Hsla,
    red: Hsla,
    green: Hsla,
    yellow: Hsla,
    blue: Hsla,
    magenta: Hsla,
    cyan: Hsla,
    white: Hsla,
    bright_black: Hsla,
    bright_red: Hsla,
    bright_green: Hsla,
    bright_yellow: Hsla,
    bright_blue: Hsla,
    bright_magenta: Hsla,
    bright_cyan: Hsla,
    bright_white: Hsla,
    dim_black: Hsla,
    dim_red: Hsla,
    dim_green: Hsla,
    dim_yellow: Hsla,
    dim_blue: Hsla,
    dim_magenta: Hsla,
    dim_cyan: Hsla,
    dim_white: Hsla,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChatTerminalSelection {
    start_row: usize,
    start_col: usize,
    end_row: usize,
    end_col: usize,
}

#[derive(Clone, Debug)]
struct ChatTerminalRow {
    source_line: i32,
    cells: Vec<Cell>,
}

#[derive(Clone, Debug)]
struct ChatTerminalPaintBatch {
    row: usize,
    start_col: usize,
    text: String,
    cell_count: usize,
    style: GpuiTextRun,
    font_size: f32,
}

impl ChatTerminalSelection {
    fn single(row: usize, col: usize) -> Self {
        Self {
            start_row: row,
            start_col: col,
            end_row: row,
            end_col: col,
        }
    }

    fn normalized(self) -> Self {
        if (self.start_row, self.start_col) <= (self.end_row, self.end_col) {
            self
        } else {
            Self {
                start_row: self.end_row,
                start_col: self.end_col,
                end_row: self.start_row,
                end_col: self.start_col,
            }
        }
    }

    fn is_empty(self) -> bool {
        self.start_row == self.end_row && self.start_col == self.end_col
    }

    fn contains(self, row: usize, col: usize) -> bool {
        let selection = self.normalized();
        if row < selection.start_row || row > selection.end_row {
            return false;
        }

        if selection.start_row == selection.end_row {
            return row == selection.start_row
                && col >= selection.start_col
                && col <= selection.end_col;
        }

        if row == selection.start_row {
            return col >= selection.start_col;
        }

        if row == selection.end_row {
            return col <= selection.end_col;
        }

        true
    }
}

impl ChatTerminalPaintBatch {
    fn new(row: usize, start_col: usize, text: String, style: GpuiTextRun, font_size: f32) -> Self {
        Self {
            row,
            start_col,
            cell_count: 1,
            text,
            style,
            font_size,
        }
    }

    fn can_append(&self, style: &GpuiTextRun, next_col: usize) -> bool {
        self.style.font == style.font
            && self.style.color == style.color
            && self.style.background_color == style.background_color
            && self.style.underline == style.underline
            && self.style.strikethrough == style.strikethrough
            && self.start_col + self.cell_count == next_col
    }

    fn append_char(&mut self, text: &str, counts_as_cell: bool) {
        self.text.push_str(text);
        self.style.len += text.len();
        if counts_as_cell {
            self.cell_count += 1;
        }
    }

    fn paint(
        &self,
        origin: Point<Pixels>,
        cell_width: f32,
        cell_height: f32,
        window: &mut Window,
        cx: &mut App,
    ) {
        let position = point(
            origin.x + px(self.start_col as f32 * cell_width),
            origin.y + px(self.row as f32 * cell_height),
        );

        let _ = window
            .text_system()
            .shape_line(
                self.text.clone().into(),
                px(self.font_size),
                std::slice::from_ref(&self.style),
                Some(px(cell_width)),
            )
            .paint(position, px(cell_height), window, cx);
    }
}

struct ChatTerminalInputElement {
    editor: gpui::Entity<EditorView>,
    tab_id: String,
}

struct ChatTerminalSurfaceElement {
    editor: gpui::Entity<EditorView>,
    snapshot: ChatTerminalSnapshot,
    selection: Option<ChatTerminalSelection>,
    selecting: bool,
    cursor_row: Option<usize>,
    cursor_col: usize,
    cursor_visible: bool,
    cursor_shape: AlacCursorShape,
    focused: bool,
    search_matches: Vec<TermSearchMatch>,
    current_search_match: Option<usize>,
    default_fg: Hsla,
    default_bg: Hsla,
    cursor_color: Hsla,
    palette: ChatTerminalPalette,
}

struct ChatTerminalInputHandler {
    editor: gpui::Entity<EditorView>,
    tab_id: String,
    bounds: Bounds<Pixels>,
}

impl gpui::IntoElement for ChatTerminalInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui::IntoElement for ChatTerminalSurfaceElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ChatTerminalInputElement {
    type RequestLayoutState = ();
    type PrepaintState = Bounds<Pixels>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let tab_id = self.tab_id.clone();
        self.editor.update(cx, |editor, cx| {
            let Some(state) = editor.terminal_tab_states.get_mut(&tab_id) else {
                return;
            };
            let bounds_changed = state.input_bounds.is_none_or(|current| current != bounds);
            state.input_bounds = Some(bounds);
            if bounds_changed {
                editor.resize_chat_terminal(&tab_id);
                cx.notify();
            }
        });
        bounds
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(focus_handle) = self
            .editor
            .read(cx)
            .terminal_tab_states
            .get(&self.tab_id)
            .map(|state| state.focus_handle.clone())
        else {
            return;
        };
        window.handle_input(
            &focus_handle,
            ChatTerminalInputHandler {
                editor: self.editor.clone(),
                tab_id: self.tab_id.clone(),
                bounds: *prepaint,
            },
            cx,
        );
    }
}

impl Element for ChatTerminalSurfaceElement {
    type RequestLayoutState = ();
    type PrepaintState = Bounds<Pixels>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        bounds
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        bounds: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let editor = self.editor.read(cx);
        let (body_font_size, cell_width, cell_height) = editor.chat_terminal_metrics();
        let beam_width = editor.styles.cursor.cursor_width_px.max(1.0);
        let cursor_opacity = if !self.focused {
            1.0
        } else if self.selecting {
            editor.styles.cursor.cursor_opacity_max
        } else {
            editor.cursor_opacity
        };
        let mut cursor_color = self.cursor_color;
        cursor_color.a *= cursor_opacity;
        let selection_bg = editor.theme.selection;
        let search_match_bg = editor.styles.search.search_match_bg;
        let search_match_active_bg = editor.styles.search.search_match_active_bg;
        let rows = EditorView::chat_terminal_rows(&self.snapshot);
        let base_font = editor.chat_terminal_font(window);
        let origin = bounds.origin;

        // Per-visible-line search highlight intervals:
        // source_line -> [(start_col, end_col_inclusive, is_current)].
        let mut search_rows: HashMap<i32, Vec<(usize, usize, bool)>> = HashMap::new();
        if !self.search_matches.is_empty() {
            let min_line = rows.first().map(|row| row.source_line).unwrap_or(0);
            let max_line = rows.last().map(|row| row.source_line).unwrap_or(0);
            let cols = (self.snapshot.cols as usize).max(1);
            for (match_index, search_match) in self.search_matches.iter().enumerate() {
                let is_current = self.current_search_match == Some(match_index);
                let (match_start, match_end) = (*search_match.start(), *search_match.end());
                if match_end.line.0 < min_line || match_start.line.0 > max_line {
                    continue;
                }
                for line in match_start.line.0.max(min_line)..=match_end.line.0.min(max_line) {
                    let start_col = if line == match_start.line.0 {
                        match_start.column.0
                    } else {
                        0
                    };
                    let end_col = if line == match_end.line.0 {
                        match_end.column.0
                    } else {
                        cols - 1
                    };
                    search_rows
                        .entry(line)
                        .or_default()
                        .push((start_col, end_col, is_current));
                }
            }
        }

        let cell_bounds = |row_index: usize, col_index: usize, width_cells: f32| {
            Bounds::new(
                point(
                    origin.x + px(col_index as f32 * cell_width),
                    origin.y + px(row_index as f32 * cell_height),
                ),
                size(px(cell_width * width_cells), px(cell_height)),
            )
        };

        let mut batches: Vec<ChatTerminalPaintBatch> = Vec::new();
        let mut cursor_paint: Option<(usize, usize, bool)> = None;

        for (row_index, row) in rows.iter().enumerate() {
            let row_search = search_rows.get(&row.source_line);
            let mut previous_cell_had_extras = false;
            // In-progress run of same-colored background cells:
            // (start_col as f32, width in cells, color).
            let mut bg_run: Option<(f32, f32, Hsla)> = None;

            let flush_bg =
                |run: &mut Option<(f32, f32, Hsla)>, window: &mut Window, row_index: usize| {
                    if let Some((start, width, color)) = run.take() {
                        window.paint_quad(fill(
                            Bounds::new(
                                point(
                                    origin.x + px(start * cell_width),
                                    origin.y + px(row_index as f32 * cell_height),
                                ),
                                size(px(width * cell_width), px(cell_height)),
                            ),
                            color,
                        ));
                    }
                };

            for (col_index, cell) in row.cells.iter().enumerate() {
                let mut fg = terminal_color_to_hsla(
                    cell.fg,
                    &self.palette,
                    self.default_fg,
                    self.default_bg,
                );
                let mut bg = terminal_color_to_hsla(
                    cell.bg,
                    &self.palette,
                    self.default_fg,
                    self.default_bg,
                );
                if cell.flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if cell.flags.contains(Flags::DIM) {
                    fg = terminal_dimmed_color(cell.fg, &self.palette, fg);
                }

                if is_chat_terminal_spacer_cell(cell) {
                    continue;
                }

                let is_selected = self
                    .selection
                    .is_some_and(|selection| selection.contains(row_index, col_index));
                if is_selected {
                    bg = selection_bg;
                }
                if let Some(intervals) = row_search {
                    for (start_col, end_col, is_current) in intervals {
                        if col_index >= *start_col && col_index <= *end_col {
                            bg = if *is_current {
                                search_match_active_bg
                            } else {
                                search_match_bg
                            };
                            break;
                        }
                    }
                }

                let is_cursor = self.cursor_visible
                    && self.cursor_row == Some(row_index)
                    && col_index == self.cursor_col;
                let is_wide = cell.flags.contains(Flags::WIDE_CHAR);
                let width_cells = if is_wide { 2.0 } else { 1.0 };
                if is_cursor {
                    cursor_paint = Some((row_index, col_index, is_wide));
                }

                // Merge horizontally adjacent same-colored backgrounds into a
                // single quad.
                if bg != self.default_bg {
                    match bg_run.as_mut() {
                        Some((start, width, color))
                            if *color == bg && *start + *width == col_index as f32 =>
                        {
                            *width += width_cells;
                        }
                        _ => {
                            flush_bg(&mut bg_run, window, row_index);
                            bg_run = Some((col_index as f32, width_cells, bg));
                        }
                    }
                } else {
                    flush_bg(&mut bg_run, window, row_index);
                }

                if cell.c == ' ' && previous_cell_had_extras {
                    previous_cell_had_extras = false;
                    continue;
                }
                previous_cell_had_extras =
                    matches!(cell.zerowidth(), Some(chars) if !chars.is_empty());

                let text = normalize_cell_char(cell);
                let has_visible_text = cell.c != ' ' || cell.zerowidth().is_some();
                if !has_visible_text {
                    continue;
                }

                // A focused block cursor inverts the glyph under it while the
                // blink phase keeps the block visible.
                if is_cursor
                    && self.focused
                    && self.cursor_shape == AlacCursorShape::Block
                    && cursor_opacity > 0.5
                {
                    fg = self.default_bg;
                }

                let font = Font {
                    weight: if cell.flags.contains(Flags::BOLD) {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    },
                    style: if cell.flags.contains(Flags::ITALIC) {
                        FontStyle::Italic
                    } else {
                        FontStyle::Normal
                    },
                    ..base_font.clone()
                };
                let has_underline = cell.flags.intersects(
                    Flags::UNDERLINE
                        | Flags::DOUBLE_UNDERLINE
                        | Flags::UNDERCURL
                        | Flags::DOTTED_UNDERLINE
                        | Flags::DASHED_UNDERLINE,
                );
                let run = GpuiTextRun {
                    len: text.len(),
                    font,
                    color: fg,
                    background_color: None,
                    underline: has_underline.then(|| gpui::UnderlineStyle {
                        color: Some(fg),
                        thickness: px(1.0),
                        wavy: cell.flags.contains(Flags::UNDERCURL),
                    }),
                    strikethrough: cell.flags.contains(Flags::STRIKEOUT).then(|| {
                        gpui::StrikethroughStyle {
                            color: Some(fg),
                            thickness: px(1.0),
                        }
                    }),
                };

                if let Some(last) = batches.last_mut()
                    && last.row == row_index
                    && last.can_append(&run, col_index)
                {
                    last.append_char(&text, true);
                } else {
                    batches.push(ChatTerminalPaintBatch::new(
                        row_index,
                        col_index,
                        text,
                        run,
                        body_font_size,
                    ));
                }
            }

            flush_bg(&mut bg_run, window, row_index);
        }

        // Cursor: painted before text so a filled block sits underneath the
        // (inverted) glyph. Unfocused terminals render a hollow block.
        if let Some((row_index, col_index, is_wide)) = cursor_paint {
            let width_cells = if is_wide { 2.0 } else { 1.0 };
            let full = cell_bounds(row_index, col_index, width_cells);
            if !self.focused {
                window.paint_quad(gpui::quad(
                    full,
                    0.0,
                    gpui::transparent_black(),
                    1.0,
                    cursor_color,
                    gpui::BorderStyle::Solid,
                ));
            } else {
                match self.cursor_shape {
                    AlacCursorShape::Block => {
                        window.paint_quad(fill(full, cursor_color));
                    }
                    AlacCursorShape::Underline => {
                        let thickness = (cell_height * 0.12).clamp(1.0, 3.0);
                        window.paint_quad(fill(
                            Bounds::new(
                                point(
                                    full.origin.x,
                                    full.origin.y + full.size.height - px(thickness),
                                ),
                                size(full.size.width, px(thickness)),
                            ),
                            cursor_color,
                        ));
                    }
                    AlacCursorShape::HollowBlock => {
                        window.paint_quad(gpui::quad(
                            full,
                            0.0,
                            gpui::transparent_black(),
                            1.0,
                            cursor_color,
                            gpui::BorderStyle::Solid,
                        ));
                    }
                    AlacCursorShape::Beam => {
                        window.paint_quad(fill(
                            Bounds::new(full.origin, size(px(beam_width), full.size.height)),
                            cursor_color,
                        ));
                    }
                    AlacCursorShape::Hidden => {}
                }
            }
        }

        for batch in &batches {
            batch.paint(origin, cell_width, cell_height, window, cx);
        }
    }
}

impl InputHandler for ChatTerminalInputHandler {
    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(&mut self, _window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        self.editor
            .read(cx)
            .terminal_tab_states
            .get(&self.tab_id)
            .and_then(|state| state.marked_text.as_ref())
            .map(|text| 0..text.encode_utf16().count())
    }

    fn text_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<String> {
        None
    }

    fn replace_text_in_range(
        &mut self,
        _replacement_range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        let tab_id = self.tab_id.clone();
        self.editor.update(cx, |editor, editor_cx| {
            editor.pause_cursor_blinking(editor_cx);
            if let Some(state) = editor.terminal_tab_states.get_mut(&tab_id) {
                state.marked_text = None;
            }
            editor.terminal_write_user_input(&tab_id, text.as_bytes());
            editor_cx.notify();
        });
        window.invalidate_character_coordinates();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range_utf16: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let tab_id = self.tab_id.clone();
        self.editor.update(cx, |editor, editor_cx| {
            if let Some(state) = editor.terminal_tab_states.get_mut(&tab_id) {
                state.marked_text = (!new_text.is_empty()).then(|| new_text.to_string());
            }
            editor.pause_cursor_blinking(editor_cx);
            editor_cx.notify();
        });
        window.invalidate_character_coordinates();
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut App) {
        let tab_id = self.tab_id.clone();
        self.editor.update(cx, |editor, editor_cx| {
            if let Some(state) = editor.terminal_tab_states.get_mut(&tab_id) {
                state.marked_text = None;
            }
            editor_cx.notify();
        });
        window.invalidate_character_coordinates();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        let editor = self.editor.read(cx);
        let snapshot = editor
            .terminal_session(&self.tab_id)
            .map(|session| session.snapshot.clone())
            .unwrap_or_default();
        let (_, cell_width, cell_height) = editor.chat_terminal_metrics();
        let cursor_row = EditorView::chat_terminal_cursor_display_row(&snapshot)? as f32;
        let cursor_col = snapshot.cursor.point.column.0 as f32 + range_utf16.start as f32;

        Some(Bounds::new(
            point(
                self.bounds.origin.x + px(cursor_col * cell_width),
                self.bounds.origin.y + px(cursor_row * cell_height),
            ),
            size(
                px((range_utf16.len().max(1) as f32) * cell_width),
                px(cell_height),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<usize> {
        None
    }

    fn apple_press_and_hold_enabled(&mut self) -> bool {
        false
    }
}

impl EventListener for ChatTerminalListener {
    fn send_event(&self, event: AlacTermEvent) {
        let AlacTermEvent::PtyWrite(text) = event else {
            return;
        };
        let Ok(mut writer) = self.writer.lock() else {
            return;
        };
        let _ = writer.write_all(text.as_bytes());
        let _ = writer.flush();
    }
}

impl Dimensions for ChatTerminalSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

impl Default for ChatTerminalSnapshot {
    fn default() -> Self {
        Self {
            rows: CHAT_PANEL_DEFAULT_ROWS,
            cols: CHAT_PANEL_DEFAULT_COLS,
            cells: Vec::new(),
            cursor: RenderableCursor {
                shape: AlacCursorShape::Block,
                point: AlacPoint::new(Line(0), Column(0)),
            },
            display_offset: 0,
            history_size: 0,
        }
    }
}

impl ChatAgentCli {
    pub(crate) fn all() -> [Self; 5] {
        [
            Self::Terminal,
            Self::OpenAICodex,
            Self::GithubCopilot,
            Self::ClaudeCode,
            Self::GeminiCli,
        ]
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::GithubCopilot => "Github Copilot",
            Self::OpenAICodex => "OpenAI Codex",
            Self::ClaudeCode => "Claude Code",
            Self::GeminiCli => "Gemini CLI",
        }
    }

    pub(crate) fn command_name(self) -> &'static str {
        match self {
            Self::GithubCopilot => "copilot",
            Self::Terminal => crate::platform::terminal::DEFAULT_SHELL_PROGRAM,
            Self::OpenAICodex => "codex",
            Self::ClaudeCode => "claude",
            Self::GeminiCli => "gemini",
        }
    }

    pub(crate) fn settings_value(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::GithubCopilot => "github_copilot",
            Self::OpenAICodex => "openai_codex",
            Self::ClaudeCode => "claude_code",
            Self::GeminiCli => "gemini_cli",
        }
    }

    pub(crate) fn from_settings_value(value: &str) -> Option<Self> {
        Self::all()
            .into_iter()
            .find(|agent_cli| agent_cli.settings_value() == value)
    }

    fn install_url(self) -> Option<&'static str> {
        match self {
            Self::GithubCopilot => {
                Some("https://docs.github.com/en/copilot/github-copilot-in-the-cli")
            }
            Self::OpenAICodex => Some("https://github.com/openai/codex"),
            Self::ClaudeCode => Some("https://docs.anthropic.com/en/docs/claude-code"),
            Self::GeminiCli => Some("https://github.com/google-gemini/gemini-cli"),
            Self::Terminal => None,
        }
    }

    pub(crate) fn build_command(self, custom_command_line: Option<&str>) -> CommandBuilder {
        if let Some(command_line) = custom_command_line.filter(|line| !line.trim().is_empty()) {
            let mut command = CommandBuilder::new(chat_terminal_default_shell_program());
            for argument in crate::platform::terminal::shell_command_args(command_line.trim()) {
                command.arg(argument);
            }
            return command;
        }
        match self {
            Self::Terminal => CommandBuilder::new(chat_terminal_default_shell_program()),
            _ => CommandBuilder::new(self.command_name()),
        }
    }

    pub(crate) fn icon_path(self) -> &'static str {
        match self {
            Self::GithubCopilot => MENU_COPILOT_SVG_PATH,
            Self::Terminal => MENU_TERMINAL_SVG_PATH,
            Self::OpenAICodex => MENU_CODEX_SVG_PATH,
            Self::ClaudeCode => MENU_CLAUDE_SVG_PATH,
            Self::GeminiCli => MENU_GEMINI_SVG_PATH,
        }
    }
}

impl ChatTerminalSession {
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: ChatTerminalStatus::Unavailable(message.into()),
            snapshot: ChatTerminalSnapshot::default(),
            dynamic_title: None,
            runtime: None,
            receiver: None,
            poll_task: None,
            rows: CHAT_PANEL_DEFAULT_ROWS,
            cols: CHAT_PANEL_DEFAULT_COLS,
            warn_on_close: false,
        }
    }

    fn launch(
        cwd: PathBuf,
        agent_cli: ChatAgentCli,
        custom_command_line: Option<&str>,
        cursor_shape: TerminalCursorShape,
    ) -> anyhow::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: CHAT_PANEL_DEFAULT_ROWS,
            cols: CHAT_PANEL_DEFAULT_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let initial_process_name = custom_command_line
            .and_then(|line| line.split_whitespace().next())
            .map(str::to_string)
            .unwrap_or_else(|| match agent_cli {
                ChatAgentCli::Terminal => chat_terminal_default_shell_program(),
                _ => agent_cli.command_name().to_string(),
            });
        let initial_title = terminal_process_title(&cwd, &initial_process_name, &[]);
        let shell_program = chat_terminal_default_shell_program();
        let warn_on_close = agent_cli != ChatAgentCli::Terminal
            || custom_command_line.is_some_and(|line| line.trim() != shell_program);
        let mut command = agent_cli.build_command(custom_command_line);
        command.cwd(cwd.clone());
        configure_chat_terminal_environment(&mut command);

        let child = pair.slave.spawn_command(command)?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));
        let mut reader = pair.master.try_clone_reader()?;

        let config = terminal_config(cursor_shape);
        let term = Arc::new(FairMutex::new(Term::new(
            config.clone(),
            &ChatTerminalSize {
                rows: CHAT_PANEL_DEFAULT_ROWS as usize,
                cols: CHAT_PANEL_DEFAULT_COLS as usize,
            },
            ChatTerminalListener {
                writer: writer.clone(),
            },
        )));

        let (sender, receiver) = mpsc::channel();
        let term_for_reader = term.clone();
        let reader_thread = thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            let mut processor = Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::new();
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read_len) => {
                        let converted = convert_terminal_bytes(&buffer[..read_len]);
                        {
                            let mut term = term_for_reader.lock();
                            processor.advance(&mut *term, &converted);
                        }
                        if sender.send(ChatTerminalEvent::Updated).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(ChatTerminalEvent::Error(error.to_string()));
                        break;
                    }
                }
            }

            let _ = sender.send(ChatTerminalEvent::Exited);
        });

        let mut session = Self {
            status: ChatTerminalStatus::Launching,
            snapshot: ChatTerminalSnapshot::default(),
            dynamic_title: Some(initial_title),
            runtime: Some(ChatTerminalRuntime {
                master: pair.master,
                child,
                writer,
                term,
                config,
                process_system: System::new(),
                process_refresh_kind: ProcessRefreshKind::new()
                    .with_cmd(UpdateKind::Always)
                    .with_cwd(UpdateKind::Always)
                    .with_exe(UpdateKind::Always),
                last_process_info_refresh: Instant::now()
                    - Duration::from_millis(TERMINAL_PROCESS_INFO_POLL_MS),
                last_foreground_pid: None,
                reader_thread: Some(reader_thread),
            }),
            receiver: Some(receiver),
            poll_task: None,
            rows: CHAT_PANEL_DEFAULT_ROWS,
            cols: CHAT_PANEL_DEFAULT_COLS,
            warn_on_close,
        };
        session.refresh_snapshot();
        Ok(session)
    }

    fn has_running_command(&self) -> bool {
        if !matches!(
            self.status,
            ChatTerminalStatus::Launching | ChatTerminalStatus::Running
        ) {
            return false;
        }
        if self.warn_on_close {
            return true;
        }

        // ConPTY exposes no foreground process group, so Windows cannot tell an
        // idle shell apart from one running a command.
        #[cfg(unix)]
        {
            let Some(runtime) = self.runtime.as_ref() else {
                return false;
            };
            let foreground = runtime
                .master
                .process_group_leader()
                .and_then(|pid| (pid > 0).then_some(pid as u32));
            foreground
                .zip(runtime.child.process_id())
                .is_some_and(|(foreground, shell)| foreground != shell)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    fn refresh_dynamic_title(&mut self) -> Option<String> {
        let runtime = self.runtime.as_mut()?;
        if runtime.last_process_info_refresh.elapsed()
            < Duration::from_millis(TERMINAL_PROCESS_INFO_POLL_MS)
        {
            return None;
        }
        runtime.last_process_info_refresh = Instant::now();

        #[cfg(unix)]
        let foreground_pid = runtime
            .master
            .process_group_leader()
            .and_then(|pid| (pid > 0).then_some(pid as u32));
        #[cfg(not(unix))]
        let foreground_pid: Option<u32> = None;
        let pid = foreground_pid.or_else(|| runtime.child.process_id())?;
        let pid = Pid::from_u32(pid);
        if runtime.last_foreground_pid != Some(pid) {
            runtime.process_system = System::new();
            runtime.last_foreground_pid = Some(pid);
        }
        runtime.process_system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            runtime.process_refresh_kind,
        );
        let process = runtime.process_system.process(pid)?;
        let cwd = process.cwd()?;
        let process_name = process.name().to_string_lossy();
        let argv = process
            .cmd()
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let title = terminal_process_title(cwd, &process_name, &argv);
        if self.dynamic_title.as_deref() == Some(title.as_str()) {
            return None;
        }
        self.dynamic_title = Some(title.clone());
        Some(title)
    }

    fn set_cursor_shape(&mut self, cursor_shape: TerminalCursorShape) {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.config.default_cursor_style = alacritty_cursor_style(cursor_shape);
            runtime.term.lock().set_options(runtime.config.clone());
            self.refresh_snapshot();
        }
    }

    fn write_bytes(&self, bytes: &[u8]) -> anyhow::Result<()> {
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("chat terminal is not running"))?;
        let mut writer = runtime
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("chat terminal writer lock poisoned"))?;
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    fn term_mode(&self) -> TermMode {
        self.runtime
            .as_ref()
            .map(|runtime| *runtime.term.lock().mode())
            .unwrap_or(TermMode::NONE)
    }

    fn mouse_mode(&self, shift: bool) -> bool {
        !shift && self.term_mode().intersects(TermMode::MOUSE_MODE)
    }

    fn alternate_scroll_mode(&self, shift: bool) -> bool {
        let mode = self.term_mode();
        !shift
            && !mode.intersects(TermMode::MOUSE_MODE)
            && mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL)
    }

    fn write_mouse_report(
        &self,
        row: usize,
        col: usize,
        button: u8,
        released: bool,
        shift: bool,
        alt: bool,
        control: bool,
    ) -> anyhow::Result<()> {
        let mode = self.term_mode();
        let mut modifier_bits = 0;
        if shift {
            modifier_bits += 4;
        }
        if alt {
            modifier_bits += 8;
        }
        if control {
            modifier_bits += 16;
        }

        if mode.contains(TermMode::SGR_MOUSE) {
            let state = if released { 'm' } else { 'M' };
            let message = format!(
                "\x1b[<{};{};{}{}",
                button + modifier_bits,
                col + 1,
                row + 1,
                state
            );
            return self.write_bytes(message.as_bytes());
        }

        let utf8 = mode.contains(TermMode::UTF8_MOUSE);
        let max_point = if utf8 { 2015 } else { 223 };
        if row >= max_point || col >= max_point {
            return Ok(());
        }

        let reported_button = if released {
            3 + modifier_bits
        } else {
            button + modifier_bits
        };
        let mut message = vec![b'\x1b', b'[', b'M', 32 + reported_button];

        let mouse_pos_encode = |position: usize| {
            let position = 32 + 1 + position;
            let first = 0xC0 + position / 64;
            let second = 0x80 + (position & 63);
            [first as u8, second as u8]
        };

        if utf8 && col >= 95 {
            message.extend_from_slice(&mouse_pos_encode(col));
        } else {
            message.push(32 + 1 + col as u8);
        }

        if utf8 && row >= 95 {
            message.extend_from_slice(&mouse_pos_encode(row));
        } else {
            message.push(32 + 1 + row as u8);
        }

        self.write_bytes(&message)
    }

    fn scroll_display(&mut self, scroll: Scroll) {
        let Some(runtime) = self.runtime.as_ref() else {
            return;
        };

        {
            let mut term = runtime.term.lock();
            term.scroll_display(scroll);
        }

        self.refresh_snapshot();
    }

    /// Clears the screen and scrollback like Terminal.app's Cmd+K, keeping the
    /// current prompt line (mirrors Zed's `clear_saved_screen`).
    fn clear(&mut self) {
        let Some(runtime) = self.runtime.as_ref() else {
            return;
        };

        {
            let mut term = runtime.term.lock();
            term.clear_screen(ClearMode::Saved);

            let cursor = term.grid().cursor.point;
            term.grid_mut().reset_region(..cursor.line);

            let columns = term.grid().columns();
            let line = term.grid()[cursor.line][..Column(columns)]
                .iter()
                .cloned()
                .enumerate()
                .collect::<Vec<(usize, Cell)>>();
            for (index, cell) in line {
                term.grid_mut()[Line(0)][Column(index)] = cell;
            }

            term.grid_mut().cursor.point = AlacPoint::new(Line(0), term.grid().cursor.point.column);
            let new_cursor = term.grid().cursor.point;
            if (new_cursor.line.0 as usize) < term.screen_lines() - 1 {
                term.grid_mut().reset_region((new_cursor.line + 1)..);
            }
        }

        self.refresh_snapshot();
    }

    /// Expands a point using Alacritty's selection boundaries, matching Zed's
    /// semantic (double-click) and line (triple-click) terminal selection.
    fn expanded_selection_range(
        &self,
        point: AlacPoint,
        line_selection: bool,
    ) -> Option<(AlacPoint, AlacPoint)> {
        let runtime = self.runtime.as_ref()?;
        let term = runtime.term.lock();
        let point = AlacPoint::new(
            point
                .line
                .max(term.grid().topmost_line())
                .min(term.grid().bottommost_line()),
            point.column.min(term.grid().last_column()),
        );

        if line_selection {
            return Some((term.line_search_left(point), term.line_search_right(point)));
        }

        if let Some(matching) = term.bracket_search(point) {
            return Some(if matching < point {
                (matching, point)
            } else {
                (point, matching)
            });
        }

        Some((
            term.semantic_search_left(point),
            term.semantic_search_right(point),
        ))
    }

    /// Runs a regex search over the whole grid (scrollback included).
    fn search(&self, pattern: &str, max_matches: usize) -> Vec<TermSearchMatch> {
        let Some(runtime) = self.runtime.as_ref() else {
            return Vec::new();
        };
        let Ok(mut regex) = RegexSearch::new(pattern) else {
            return Vec::new();
        };

        let term = runtime.term.lock();
        let start = AlacPoint::new(term.grid().topmost_line(), Column(0));
        let end = AlacPoint::new(term.grid().bottommost_line(), term.grid().last_column());
        RegexIter::new(start, end, AlacDirection::Right, &term, &mut regex)
            .take(max_matches)
            .collect()
    }

    /// Scrolls the viewport the minimal amount needed to make `line` visible.
    fn scroll_line_into_view(&mut self, line: i32) {
        let Some(runtime) = self.runtime.as_ref() else {
            return;
        };

        let delta = {
            let term = runtime.term.lock();
            let display_offset = term.grid().display_offset() as i32;
            let screen_lines = term.screen_lines() as i32;
            // Visible grid lines are [-display_offset, screen_lines - display_offset).
            let target_offset = if line < -display_offset {
                -line
            } else if line >= screen_lines - display_offset {
                (screen_lines - 1 - line).max(0)
            } else {
                display_offset
            };
            target_offset - display_offset
        };

        if delta != 0 {
            self.scroll_display(Scroll::Delta(delta));
        }
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        let next_rows = rows.max(2);
        let next_cols = cols.max(20);
        if self.rows == next_rows && self.cols == next_cols {
            return;
        }

        self.rows = next_rows;
        self.cols = next_cols;
        if let Some(runtime) = self.runtime.as_mut() {
            let _ = runtime.master.resize(PtySize {
                rows: self.rows,
                cols: self.cols,
                pixel_width: 0,
                pixel_height: 0,
            });
            {
                let mut term = runtime.term.lock();
                term.resize(ChatTerminalSize {
                    rows: self.rows as usize,
                    cols: self.cols as usize,
                });
            }
            self.refresh_snapshot();
        }
    }

    fn refresh_snapshot(&mut self) {
        let Some(runtime) = self.runtime.as_ref() else {
            return;
        };

        let term = runtime.term.lock();
        let content = term.renderable_content();
        let display_offset = content.display_offset;
        let history_size = term.grid().history_size();
        let mut cells = Vec::with_capacity(content.display_iter.size_hint().0);
        cells.extend(content.display_iter.map(|indexed| ChatIndexedCell {
            point: indexed.point,
            cell: indexed.cell.clone(),
        }));
        self.snapshot = ChatTerminalSnapshot {
            rows: self.rows,
            cols: self.cols,
            cells,
            cursor: content.cursor,
            display_offset,
            history_size,
        };
    }

    fn stop(&mut self) {
        if let Some(mut runtime) = self.runtime.take() {
            let _ = runtime.child.kill();
            drop(runtime.writer);
            // Closing the pty is what lets the reader see EOF. Windows keeps the
            // ConPTY pipe open until then, so joining first would deadlock.
            drop(runtime.master);
            if let Some(handle) = runtime.reader_thread.take() {
                let _ = handle.join();
            }
            let _ = runtime.child.wait();
        }
        self.receiver = None;
        self.poll_task = None;
    }
}

fn agent_cli_command_exists(agent_cli: ChatAgentCli) -> bool {
    if agent_cli == ChatAgentCli::Terminal {
        return true;
    }

    let names = crate::platform::terminal::executable_names(agent_cli.command_name());
    let paths = chat_terminal_path_env();
    env::split_paths(&paths)
        .any(|directory| names.iter().any(|name| directory.join(name).is_file()))
}

/// PATH reported by the user's login shell. Finder-launched apps inherit a
/// minimal PATH that misses user-level tool directories (cargo, npm, nvm...),
/// which made installed CLIs look missing and left PTY commands unresolvable.
static LOGIN_SHELL_PATH: std::sync::OnceLock<Option<OsString>> = std::sync::OnceLock::new();
static LOGIN_SHELL_PATH_PREFETCH: std::sync::Once = std::sync::Once::new();

pub(crate) fn prefetch_login_shell_path() {
    LOGIN_SHELL_PATH_PREFETCH.call_once(|| {
        std::thread::spawn(|| {
            let value = crate::platform::terminal::login_shell_path();
            let _ = LOGIN_SHELL_PATH.set(value);
        });
    });
}

fn chat_terminal_path_env() -> OsString {
    let mut paths: Vec<PathBuf> = Vec::new();
    let push_unique = |paths: &mut Vec<PathBuf>, candidate: PathBuf| {
        if !paths.contains(&candidate) {
            paths.push(candidate);
        }
    };

    if let Some(Some(login_path)) = LOGIN_SHELL_PATH.get() {
        for path in env::split_paths(login_path) {
            push_unique(&mut paths, path);
        }
    }
    if let Some(process_path) = env::var_os("PATH") {
        for path in env::split_paths(&process_path) {
            push_unique(&mut paths, path);
        }
    }
    for candidate in crate::platform::terminal::extra_path_candidates() {
        push_unique(&mut paths, candidate);
    }

    env::join_paths(paths).unwrap_or_else(|_| env::var_os("PATH").unwrap_or_default())
}

fn configure_chat_terminal_environment(command: &mut CommandBuilder) {
    command.env("PATH", chat_terminal_path_env());
    for (key, value) in crate::platform::terminal::locale_env_vars() {
        command.env(key, value);
    }
    command.env("SHELL", chat_terminal_default_shell_program());
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "moeka");
    command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
}

fn convert_terminal_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut converted = Vec::with_capacity(bytes.len());
    let mut prev_byte = 0u8;
    for &byte in bytes {
        if byte == b'\n' && prev_byte != b'\r' {
            converted.push(b'\r');
        }
        converted.push(byte);
        prev_byte = byte;
    }
    converted
}

fn is_chat_terminal_spacer_cell(cell: &Cell) -> bool {
    cell.flags.contains(Flags::WIDE_CHAR_SPACER)
        || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
}

fn next_non_spacer_terminal_col(row_cells: &[Cell], start: usize) -> Option<usize> {
    let mut index = start;
    while index < row_cells.len() {
        if !is_chat_terminal_spacer_cell(&row_cells[index]) {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn previous_non_spacer_terminal_col(row_cells: &[Cell], start: usize) -> Option<usize> {
    let mut index = start.min(row_cells.len().saturating_sub(1));
    loop {
        if !is_chat_terminal_spacer_cell(&row_cells[index]) {
            return Some(index);
        }
        if index == 0 {
            return None;
        }
        index -= 1;
    }
}

fn chat_terminal_word_bounds(row_cells: &[Cell], col: usize) -> Option<(usize, usize)> {
    if row_cells.is_empty() {
        return None;
    }

    let anchor = previous_non_spacer_terminal_col(row_cells, col)?;
    let anchor_char = if row_cells[anchor].c == '\u{00A0}' {
        ' '
    } else {
        row_cells[anchor].c
    };
    let anchor_class = classify_word_char(anchor_char);

    let matches_class = |index: usize| {
        let cell = row_cells.get(index).expect("terminal word index in bounds");
        if is_chat_terminal_spacer_cell(cell) {
            return false;
        }
        let ch = if cell.c == '\u{00A0}' { ' ' } else { cell.c };
        classify_word_char(ch) == anchor_class
    };

    let mut start = anchor;
    while let Some(previous) = start
        .checked_sub(1)
        .and_then(|index| previous_non_spacer_terminal_col(row_cells, index))
    {
        if !matches_class(previous) {
            break;
        }
        start = previous;
    }

    let mut end = anchor;
    while let Some(next) = next_non_spacer_terminal_col(row_cells, end + 1) {
        if !matches_class(next) {
            break;
        }
        end = next;
    }

    Some((start, end))
}

fn normalize_cell_char(cell: &Cell) -> String {
    let mut text = String::new();
    let character = if cell.c == ' ' { '\u{00A0}' } else { cell.c };
    text.push(character);
    if let Some(extra) = cell.zerowidth() {
        for ch in extra {
            text.push(*ch);
        }
    }
    text
}

/// Terminal colors are intentionally independent from the editor theme. These
/// are Zed's One Dark/Light terminal palettes; only the terminal background is
/// supplied by the active Moeka theme.
fn terminal_palette(panel_bg: Hsla) -> ChatTerminalPalette {
    if panel_bg.l < 0.5 {
        ChatTerminalPalette {
            foreground: crate::hex_hsla!("#ABB2BF"),
            bright_foreground: crate::hex_hsla!("#DCE0E5"),
            dim_foreground: crate::hex_hsla!("#636D83"),
            black: crate::hex_hsla!("#282C34"),
            red: crate::hex_hsla!("#E06C75"),
            green: crate::hex_hsla!("#98C379"),
            yellow: crate::hex_hsla!("#E5C07B"),
            blue: crate::hex_hsla!("#61AFEF"),
            magenta: crate::hex_hsla!("#C678DD"),
            cyan: crate::hex_hsla!("#56B6C2"),
            white: crate::hex_hsla!("#ABB2BF"),
            bright_black: crate::hex_hsla!("#636D83"),
            bright_red: crate::hex_hsla!("#EA858B"),
            bright_green: crate::hex_hsla!("#AAD581"),
            bright_yellow: crate::hex_hsla!("#FFD885"),
            bright_blue: crate::hex_hsla!("#85C1FF"),
            bright_magenta: crate::hex_hsla!("#D398EB"),
            bright_cyan: crate::hex_hsla!("#6ED5DE"),
            bright_white: crate::hex_hsla!("#FAFAFA"),
            dim_black: crate::hex_hsla!("#3B3F4A"),
            dim_red: crate::hex_hsla!("#A7545A"),
            dim_green: crate::hex_hsla!("#6D8F59"),
            dim_yellow: crate::hex_hsla!("#B8985B"),
            dim_blue: crate::hex_hsla!("#457CAD"),
            dim_magenta: crate::hex_hsla!("#8D54A0"),
            dim_cyan: crate::hex_hsla!("#3C818A"),
            dim_white: crate::hex_hsla!("#8F969B"),
        }
    } else {
        ChatTerminalPalette {
            foreground: crate::hex_hsla!("#2A2C33"),
            bright_foreground: crate::hex_hsla!("#2A2C33"),
            dim_foreground: crate::hex_hsla!("#BBBBBB"),
            black: crate::hex_hsla!("#000000"),
            red: crate::hex_hsla!("#DE3E35"),
            green: crate::hex_hsla!("#3F953A"),
            yellow: crate::hex_hsla!("#D2B67C"),
            blue: crate::hex_hsla!("#2F5AF3"),
            magenta: crate::hex_hsla!("#950095"),
            cyan: crate::hex_hsla!("#0997B3"),
            white: crate::hex_hsla!("#BBBBBB"),
            bright_black: crate::hex_hsla!("#000000"),
            bright_red: crate::hex_hsla!("#DE3E35"),
            bright_green: crate::hex_hsla!("#3F953A"),
            bright_yellow: crate::hex_hsla!("#D2B67C"),
            bright_blue: crate::hex_hsla!("#2F5AF3"),
            bright_magenta: crate::hex_hsla!("#A00095"),
            bright_cyan: crate::hex_hsla!("#0BBCD6"),
            bright_white: crate::hex_hsla!("#FFFFFF"),
            dim_black: crate::hex_hsla!("#555555"),
            dim_red: crate::hex_hsla!("#9C2B26"),
            dim_green: crate::hex_hsla!("#2B6927"),
            dim_yellow: crate::hex_hsla!("#A48C5A"),
            dim_blue: crate::hex_hsla!("#2140AB"),
            dim_magenta: crate::hex_hsla!("#6A006A"),
            dim_cyan: crate::hex_hsla!("#0A7B92"),
            dim_white: crate::hex_hsla!("#888888"),
        }
    }
}

fn terminal_dimmed_color(color: AlacColor, palette: &ChatTerminalPalette, resolved: Hsla) -> Hsla {
    match color {
        AlacColor::Named(NamedColor::Foreground | NamedColor::BrightForeground) => {
            palette.dim_foreground
        }
        AlacColor::Named(NamedColor::Black | NamedColor::BrightBlack) => palette.dim_black,
        AlacColor::Named(NamedColor::Red | NamedColor::BrightRed) => palette.dim_red,
        AlacColor::Named(NamedColor::Green | NamedColor::BrightGreen) => palette.dim_green,
        AlacColor::Named(NamedColor::Yellow | NamedColor::BrightYellow) => palette.dim_yellow,
        AlacColor::Named(NamedColor::Blue | NamedColor::BrightBlue) => palette.dim_blue,
        AlacColor::Named(NamedColor::Magenta | NamedColor::BrightMagenta) => palette.dim_magenta,
        AlacColor::Named(NamedColor::Cyan | NamedColor::BrightCyan) => palette.dim_cyan,
        AlacColor::Named(NamedColor::White | NamedColor::BrightWhite) => palette.dim_white,
        AlacColor::Named(NamedColor::DimForeground) => palette.dim_foreground,
        AlacColor::Named(NamedColor::DimBlack) => palette.dim_black,
        AlacColor::Named(NamedColor::DimRed) => palette.dim_red,
        AlacColor::Named(NamedColor::DimGreen) => palette.dim_green,
        AlacColor::Named(NamedColor::DimYellow) => palette.dim_yellow,
        AlacColor::Named(NamedColor::DimBlue) => palette.dim_blue,
        AlacColor::Named(NamedColor::DimMagenta) => palette.dim_magenta,
        AlacColor::Named(NamedColor::DimCyan) => palette.dim_cyan,
        AlacColor::Named(NamedColor::DimWhite) => palette.dim_white,
        _ => Hsla {
            l: resolved.l * 0.66,
            ..resolved
        },
    }
}

fn terminal_indexed_color(index: u8, palette: &ChatTerminalPalette) -> Hsla {
    match index {
        0 => palette.black,
        1 => palette.red,
        2 => palette.green,
        3 => palette.yellow,
        4 => palette.blue,
        5 => palette.magenta,
        6 => palette.cyan,
        7 => palette.white,
        8 => palette.bright_black,
        9 => palette.bright_red,
        10 => palette.bright_green,
        11 => palette.bright_yellow,
        12 => palette.bright_blue,
        13 => palette.bright_magenta,
        14 => palette.bright_cyan,
        15 => palette.bright_white,
        16..=231 => {
            let idx = index - 16;
            let r = idx / 36;
            let g = (idx % 36) / 6;
            let b = idx % 6;
            let level = |value: u8| if value == 0 { 0 } else { 55 + value * 40 };
            gpui::rgb(((level(r) as u32) << 16) | ((level(g) as u32) << 8) | (level(b) as u32))
                .into()
        }
        232..=255 => {
            let gray = 8 + (index - 232) * 10;
            gpui::rgb(((gray as u32) << 16) | ((gray as u32) << 8) | (gray as u32)).into()
        }
    }
}

fn terminal_named_color(
    color: NamedColor,
    palette: &ChatTerminalPalette,
    default_fg: Hsla,
    default_bg: Hsla,
) -> Hsla {
    match color {
        NamedColor::Background => default_bg,
        NamedColor::Foreground => default_fg,
        NamedColor::Black => palette.black,
        NamedColor::Red => palette.red,
        NamedColor::Green => palette.green,
        NamedColor::Yellow => palette.yellow,
        NamedColor::Blue => palette.blue,
        NamedColor::Magenta => palette.magenta,
        NamedColor::Cyan => palette.cyan,
        NamedColor::White => palette.white,
        NamedColor::BrightBlack => palette.bright_black,
        NamedColor::BrightRed => palette.bright_red,
        NamedColor::BrightGreen => palette.bright_green,
        NamedColor::BrightYellow => palette.bright_yellow,
        NamedColor::BrightBlue => palette.bright_blue,
        NamedColor::BrightMagenta => palette.bright_magenta,
        NamedColor::BrightCyan => palette.bright_cyan,
        NamedColor::BrightWhite => palette.bright_white,
        NamedColor::Cursor => default_fg,
        NamedColor::BrightForeground => palette.bright_foreground,
        NamedColor::DimForeground => palette.dim_foreground,
        NamedColor::DimBlack => palette.dim_black,
        NamedColor::DimRed => palette.dim_red,
        NamedColor::DimGreen => palette.dim_green,
        NamedColor::DimYellow => palette.dim_yellow,
        NamedColor::DimBlue => palette.dim_blue,
        NamedColor::DimMagenta => palette.dim_magenta,
        NamedColor::DimCyan => palette.dim_cyan,
        NamedColor::DimWhite => palette.dim_white,
    }
}

fn terminal_color_to_hsla(
    color: AlacColor,
    palette: &ChatTerminalPalette,
    default_fg: Hsla,
    default_bg: Hsla,
) -> Hsla {
    match color {
        AlacColor::Named(named) => terminal_named_color(named, palette, default_fg, default_bg),
        AlacColor::Spec(Rgb { r, g, b }) => {
            gpui::rgb(((r as u32) << 16) | ((g as u32) << 8) | (b as u32)).into()
        }
        AlacColor::Indexed(index) => terminal_indexed_color(index, palette),
    }
}

fn chat_terminal_default_shell_program() -> String {
    crate::platform::terminal::default_shell_program()
}

fn chat_terminal_mouse_button_code(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        _ => None,
    }
}

fn should_route_chat_terminal_text_input(keystroke: &gpui::Keystroke) -> bool {
    keystroke.key_char.is_some()
        && !keystroke.modifiers.control
        && !keystroke.modifiers.alt
        && !keystroke.modifiers.platform
        && !keystroke.modifiers.function
}

fn keystroke_to_terminal_bytes(keystroke: &gpui::Keystroke, mode: TermMode) -> Option<Vec<u8>> {
    let modifiers = keystroke.modifiers;
    let key = keystroke.key.as_str();

    // Note: `modifiers.function` is set for F-keys (and Fn-translated keys
    // like page-up) on macOS, so it must not disqualify a keystroke.
    if modifiers.platform {
        return None;
    }

    let modifier_code = 1
        + if modifiers.shift { 1 } else { 0 }
        + if modifiers.alt { 2 } else { 0 }
        + if modifiers.control { 4 } else { 0 };

    if modifier_code > 1 {
        let modified = match key {
            "up" => Some(format!("\x1b[1;{}A", modifier_code).into_bytes()),
            "down" => Some(format!("\x1b[1;{}B", modifier_code).into_bytes()),
            "right" => Some(format!("\x1b[1;{}C", modifier_code).into_bytes()),
            "left" => Some(format!("\x1b[1;{}D", modifier_code).into_bytes()),
            "home" => Some(format!("\x1b[1;{}H", modifier_code).into_bytes()),
            "end" => Some(format!("\x1b[1;{}F", modifier_code).into_bytes()),
            "insert" => Some(format!("\x1b[2;{}~", modifier_code).into_bytes()),
            "pageup" => Some(format!("\x1b[5;{}~", modifier_code).into_bytes()),
            "pagedown" => Some(format!("\x1b[6;{}~", modifier_code).into_bytes()),
            "delete" => Some(format!("\x1b[3;{}~", modifier_code).into_bytes()),
            "f1" => Some(format!("\x1b[1;{}P", modifier_code).into_bytes()),
            "f2" => Some(format!("\x1b[1;{}Q", modifier_code).into_bytes()),
            "f3" => Some(format!("\x1b[1;{}R", modifier_code).into_bytes()),
            "f4" => Some(format!("\x1b[1;{}S", modifier_code).into_bytes()),
            "f5" => Some(format!("\x1b[15;{}~", modifier_code).into_bytes()),
            "f6" => Some(format!("\x1b[17;{}~", modifier_code).into_bytes()),
            "f7" => Some(format!("\x1b[18;{}~", modifier_code).into_bytes()),
            "f8" => Some(format!("\x1b[19;{}~", modifier_code).into_bytes()),
            "f9" => Some(format!("\x1b[20;{}~", modifier_code).into_bytes()),
            "f10" => Some(format!("\x1b[21;{}~", modifier_code).into_bytes()),
            "f11" => Some(format!("\x1b[23;{}~", modifier_code).into_bytes()),
            "f12" => Some(format!("\x1b[24;{}~", modifier_code).into_bytes()),
            _ => None,
        };

        if let Some(bytes) = modified {
            return Some(bytes);
        }
    }

    // DECCKM: application cursor keys send SS3 (`ESC O`) sequences.
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    let mut bytes = match key {
        "enter" | "return" => vec![b'\r'],
        "backspace" => vec![0x7f],
        "tab" => {
            if modifiers.shift {
                b"\x1b[Z".to_vec()
            } else {
                vec![b'\t']
            }
        }
        "escape" => vec![0x1b],
        "left" if app_cursor => b"\x1bOD".to_vec(),
        "right" if app_cursor => b"\x1bOC".to_vec(),
        "up" if app_cursor => b"\x1bOA".to_vec(),
        "down" if app_cursor => b"\x1bOB".to_vec(),
        "home" if app_cursor => b"\x1bOH".to_vec(),
        "end" if app_cursor => b"\x1bOF".to_vec(),
        "left" => b"\x1b[D".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "insert" => b"\x1b[2~".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        "f1" => b"\x1bOP".to_vec(),
        "f2" => b"\x1bOQ".to_vec(),
        "f3" => b"\x1bOR".to_vec(),
        "f4" => b"\x1bOS".to_vec(),
        "f5" => b"\x1b[15~".to_vec(),
        "f6" => b"\x1b[17~".to_vec(),
        "f7" => b"\x1b[18~".to_vec(),
        "f8" => b"\x1b[19~".to_vec(),
        "f9" => b"\x1b[20~".to_vec(),
        "f10" => b"\x1b[21~".to_vec(),
        "f11" => b"\x1b[23~".to_vec(),
        "f12" => b"\x1b[24~".to_vec(),
        _ => {
            if modifiers.control {
                // Control characters beyond a-z.
                let byte = match key {
                    "space" | "@" | "2" => Some(0x00),
                    "[" => Some(0x1b),
                    "\\" => Some(0x1c),
                    "]" => Some(0x1d),
                    "^" | "6" => Some(0x1e),
                    "_" | "-" | "/" => Some(0x1f),
                    "?" => Some(0x7f),
                    _ => None,
                };
                if let Some(byte) = byte {
                    vec![byte]
                } else if key.len() == 1 {
                    let ascii = key.as_bytes()[0].to_ascii_lowercase();
                    if ascii.is_ascii_lowercase() {
                        vec![ascii - b'a' + 1]
                    } else {
                        return None;
                    }
                } else {
                    return None;
                }
            } else if modifiers.alt && key.len() == 1 && key.is_ascii() {
                // Alt-as-meta: send ESC + the base key, not the composed
                // Option character macOS produces (e.g. option-a = "å").
                let ch = if modifiers.shift {
                    key.as_bytes()[0].to_ascii_uppercase()
                } else {
                    key.as_bytes()[0]
                };
                vec![ch]
            } else if let Some(key_char) = &keystroke.key_char {
                key_char.as_bytes().to_vec()
            } else {
                return None;
            }
        }
    };

    if modifiers.alt {
        let mut prefixed = vec![0x1b];
        prefixed.extend(bytes);
        bytes = prefixed;
    }

    Some(bytes)
}

impl EditorView {
    fn chat_terminal_font(&self, window: &Window) -> Font {
        let text_style = window.text_style();
        let editor_styles = &self.styles.editor;
        // Empty terminal_font means "follow the editor code font".
        let configured: &str = if self.app_settings.terminal_font.is_empty() {
            editor_styles.editor_monospace_font_family
        } else {
            self.app_settings.terminal_font.as_str()
        };
        let family: SharedString = if configured == crate::platform::fonts::SYSTEM_UI {
            text_style.font().family.clone()
        } else {
            configured.to_string().into()
        };
        let fallbacks = (configured != editor_styles.editor_body_cjk_font_family).then(|| {
            FontFallbacks::from_fonts(vec![editor_styles.editor_body_cjk_font_family.to_string()])
        });

        let mut monospace_font = text_style.font();
        monospace_font.family = family;
        monospace_font.fallbacks = fallbacks;
        monospace_font
    }

    fn chat_terminal_font_size(&self) -> f32 {
        self.scaled(self.app_settings.effective_terminal_font_size())
            .max(8.0)
    }

    pub(crate) fn refresh_chat_terminal_metrics(&mut self, window: &Window, cx: &App) {
        let body_font_size = self.chat_terminal_font_size();
        let fallback_cell_width = (body_font_size * 0.62).max(7.0);
        let fallback_cell_height = (body_font_size * 1.5).max(body_font_size + self.scaled(4.0));

        let font_id = cx
            .text_system()
            .resolve_font(&self.chat_terminal_font(window));
        let measured_cell_width = cx
            .text_system()
            .advance(font_id, px(body_font_size), 'm')
            .map(|advance| f32::from(advance.width))
            .unwrap_or(fallback_cell_width);

        let scale_factor = window.scale_factor().max(1.0);
        let snap = |value: f32| ((value * scale_factor).round() / scale_factor).max(1.0);

        self.chat_terminal_cell_width_px =
            snap(measured_cell_width.max(fallback_cell_width * 0.85));
        self.chat_terminal_cell_height_px = snap(fallback_cell_height);
    }

    fn chat_terminal_metrics(&self) -> (f32, f32, f32) {
        let body_font_size = self.chat_terminal_font_size();
        let fallback_cell_width = (body_font_size * 0.62).max(7.0);
        let fallback_cell_height = (body_font_size * 1.5).max(body_font_size + self.scaled(4.0));
        let cell_width = if self.chat_terminal_cell_width_px > 0.0 {
            self.chat_terminal_cell_width_px
        } else {
            fallback_cell_width
        };
        let cell_height = if self.chat_terminal_cell_height_px > 0.0 {
            self.chat_terminal_cell_height_px
        } else {
            fallback_cell_height
        };
        (body_font_size, cell_width, cell_height)
    }

    pub(crate) fn active_chat_terminal_scrollbar_metrics(&self) -> Option<(f32, f32, f32)> {
        let tab_id = self.active_terminal_tab_id()?;
        let session = self.terminal_session(&tab_id)?;
        let (_, _, cell_height) = self.chat_terminal_metrics();
        let viewport_height = self
            .terminal_tab_state(&tab_id)
            .and_then(|state| state.input_bounds)
            .map(|bounds| f32::from(bounds.size.height))
            .unwrap_or_else(|| session.snapshot.rows.max(1) as f32 * cell_height);
        let max_scroll = session.snapshot.history_size as f32 * cell_height;
        let scroll_top = session
            .snapshot
            .history_size
            .saturating_sub(session.snapshot.display_offset) as f32
            * cell_height;

        Some((viewport_height, max_scroll, scroll_top))
    }

    pub(crate) fn set_active_chat_terminal_scrollbar_top(
        &mut self,
        scroll_top: f32,
        cx: &mut Context<Self>,
    ) {
        let (_, _, cell_height) = self.chat_terminal_metrics();
        let Some(tab_id) = self.active_terminal_tab_id() else {
            return;
        };
        let Some(session) = self.terminal_session_mut(&tab_id) else {
            return;
        };

        let history_size = session.snapshot.history_size;
        let target_scroll_lines = (scroll_top / cell_height.max(1.0))
            .round()
            .clamp(0.0, history_size as f32) as usize;
        let target_display_offset = history_size.saturating_sub(target_scroll_lines);
        let current_display_offset = session.snapshot.display_offset;
        let delta = target_display_offset as i32 - current_display_offset as i32;
        if delta == 0 {
            return;
        }

        session.scroll_display(Scroll::Delta(delta));
        cx.notify();
    }

    fn chat_terminal_rows(snapshot: &ChatTerminalSnapshot) -> Vec<ChatTerminalRow> {
        let cols = snapshot.cols as usize;
        let empty_row = || ChatTerminalRow {
            source_line: 0,
            cells: vec![Cell::default(); cols],
        };

        if snapshot.cells.is_empty() {
            return (0..snapshot.rows as usize).map(|_| empty_row()).collect();
        }

        let mut rows = Vec::with_capacity(snapshot.rows as usize);
        let mut current_line: Option<i32> = None;
        let mut current_cells = vec![Cell::default(); cols];

        for indexed in &snapshot.cells {
            let line = indexed.point.line.0;
            if current_line != Some(line) {
                if let Some(source_line) = current_line {
                    rows.push(ChatTerminalRow {
                        source_line,
                        cells: current_cells,
                    });
                    current_cells = vec![Cell::default(); cols];
                }
                current_line = Some(line);
            }

            let col = indexed.point.column.0;
            if col >= current_cells.len() {
                continue;
            }
            current_cells[col] = indexed.cell.clone();
        }

        if let Some(source_line) = current_line {
            rows.push(ChatTerminalRow {
                source_line,
                cells: current_cells,
            });
        }

        rows.truncate(snapshot.rows as usize);
        while rows.len() < snapshot.rows as usize {
            rows.push(empty_row());
        }
        rows
    }

    fn chat_terminal_cursor_display_row(snapshot: &ChatTerminalSnapshot) -> Option<usize> {
        Self::chat_terminal_rows(snapshot)
            .iter()
            .position(|row| row.source_line == snapshot.cursor.point.line.0)
    }

    pub(super) fn focus_terminal_tab(&self, tab_id: &str, window: &mut Window) {
        if let Some(state) = self.terminal_tab_states.get(tab_id) {
            window.focus(&state.focus_handle);
        }
    }

    fn set_chat_terminal_mouse_button_state(
        &mut self,
        tab_id: &str,
        button: MouseButton,
        pressed: bool,
    ) {
        let Some(state) = self.terminal_tab_states.get_mut(tab_id) else {
            return;
        };
        match button {
            MouseButton::Left => state.mouse_left_pressed = pressed,
            MouseButton::Middle => state.mouse_middle_pressed = pressed,
            MouseButton::Right => state.mouse_right_pressed = pressed,
            _ => {}
        }
    }

    pub(crate) fn terminal_tab_state(&self, tab_id: &str) -> Option<&TerminalTabState> {
        self.terminal_tab_states.get(tab_id)
    }

    pub(crate) fn terminal_session(&self, tab_id: &str) -> Option<&ChatTerminalSession> {
        self.terminal_tab_states
            .get(tab_id)
            .and_then(|state| state.session.as_ref())
    }

    pub(crate) fn terminal_tab_has_running_command(&self, tab_id: &str) -> bool {
        self.terminal_session(tab_id)
            .is_some_and(ChatTerminalSession::has_running_command)
    }

    pub(crate) fn terminal_session_mut(
        &mut self,
        tab_id: &str,
    ) -> Option<&mut ChatTerminalSession> {
        self.terminal_tab_states
            .get_mut(tab_id)
            .and_then(|state| state.session.as_mut())
    }

    /// The active tab of the current (root-swapped-in) pane when it is a
    /// terminal tab.
    pub(crate) fn active_terminal_tab_id(&self) -> Option<String> {
        self.active_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|tab| &tab.id == id))
            .filter(|tab| tab.kind.is_terminal())
            .map(|tab| tab.id.clone())
    }

    pub(crate) fn active_tab_is_terminal(&self) -> bool {
        self.active_terminal_tab_id().is_some()
    }

    pub(crate) fn active_terminal_status_summary(&self) -> Option<TerminalStatusSummary> {
        let tab_id = self.active_tab_id.as_ref()?;
        let tab = self.tabs.iter().find(|tab| &tab.id == tab_id)?;
        let agent = tab.kind.terminal_agent()?;
        let (history_size, display_offset) = self
            .terminal_session(tab_id)
            .map(|session| {
                (
                    session.snapshot.history_size,
                    session.snapshot.display_offset,
                )
            })
            .unwrap_or((0, 0));
        Some(TerminalStatusSummary {
            agent,
            title: tab.title.clone(),
            history_size,
            display_offset,
        })
    }

    pub(crate) fn has_terminal_tabs(&self) -> bool {
        self.all_tabs().any(|tab| tab.kind.is_terminal())
    }

    fn ensure_terminal_tab_state(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if !self.terminal_tab_states.contains_key(tab_id) {
            self.terminal_tab_states
                .insert(tab_id.to_string(), TerminalTabState::new(cx));
        }
    }

    /// Stops and removes the terminal runtime state for a closed tab.
    pub(crate) fn stop_terminal_tab(&mut self, tab_id: &str) {
        if self.pending_terminal_focus.as_deref() == Some(tab_id) {
            self.pending_terminal_focus = None;
        }
        self.pending_terminal_inputs.remove(tab_id);
        self.terminal_focus_history.retain(|id| id != tab_id);
        if let Some(mut state) = self.terminal_tab_states.remove(tab_id)
            && let Some(session) = state.session.as_mut()
        {
            session.stop();
        }
    }

    fn build_chat_terminal_session(&self, agent_cli: ChatAgentCli) -> ChatTerminalSession {
        let Some(cwd) = self.chat_panel_working_directory() else {
            return ChatTerminalSession::unavailable(
                "No workspace root available for chat terminal.",
            );
        };

        if !agent_cli_command_exists(agent_cli) {
            return ChatTerminalSession::unavailable(format!(
                "{} command not found: {}",
                agent_cli.label(),
                agent_cli.command_name()
            ));
        }

        let configured_command = self.app_settings.terminal_command_args.trim();
        let custom_command_line = (agent_cli == self.app_settings.terminal_launch_cli()
            && !configured_command.is_empty()
            && configured_command != agent_cli.command_name())
        .then_some(configured_command);
        match ChatTerminalSession::launch(
            cwd,
            agent_cli,
            custom_command_line,
            self.app_settings.terminal_cursor_shape(),
        ) {
            Ok(mut session) => {
                session.status = ChatTerminalStatus::Running;
                session
            }
            Err(error) => ChatTerminalSession::unavailable(error.to_string()),
        }
    }

    fn missing_agent_cli_install_target(
        &self,
        tab_id: &str,
    ) -> Option<(ChatAgentCli, &'static str)> {
        let agent_cli = self
            .all_tabs()
            .find(|tab| tab.id == tab_id)
            .and_then(|tab| tab.kind.terminal_agent())?;
        let session = self.terminal_session(tab_id)?;
        if !matches!(session.status, ChatTerminalStatus::Unavailable(_)) {
            return None;
        }
        if agent_cli_command_exists(agent_cli) {
            return None;
        }
        Some((agent_cli, agent_cli.install_url()?))
    }

    fn open_agent_cli_install_page(&mut self, agent_cli: ChatAgentCli, cx: &mut Context<Self>) {
        let Some(url) = agent_cli.install_url() else {
            return;
        };
        if let Err(error) = crate::platform::shell::open_url(url) {
            let message = format!("{}: {error}", self.t(crate::i18n::TextKey::OpenFailed));
            self.notify_error(message, cx);
        }
    }

    /// Creates a terminal tab in the current pane and activates it. Returns
    /// the new tab's id.
    pub(crate) fn create_terminal_tab(
        &mut self,
        agent_cli: ChatAgentCli,
        cx: &mut Context<Self>,
    ) -> String {
        self.persist_active_tab_state();
        let instance_index = self
            .all_tabs()
            .filter(|tab| tab.kind.terminal_agent() == Some(agent_cli))
            .count()
            + 1;
        let title = if instance_index > 1 {
            format!("{} - {}", agent_cli.label(), instance_index)
        } else {
            agent_cli.label().to_string()
        };
        let tab_id = self.next_tab_identity();
        let tab = OpenTab {
            id: tab_id.clone(),
            title,
            kind: TabKind::Terminal(agent_cli),
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
            recovery: config_store::TabRecoveryState::default(),
            history: history_store::TabHistory::default(),
        };
        self.ensure_terminal_tab_state(&tab_id, cx);
        self.tabs.push(tab);
        if let Some(index) = self.tabs.len().checked_sub(1) {
            self.activate_tab_by_index(index, cx);
        }
        self.scroll_tab_bar_to_end(cx);
        session_store::persist_session(self);
        tab_id
    }

    /// Opens a terminal tab in a new pane on the right side of the focused
    /// pane (or in place when the focused pane has no other tabs).
    pub(crate) fn open_terminal_tab_in_right_split(
        &mut self,
        agent_cli: ChatAgentCli,
        cx: &mut Context<Self>,
    ) -> String {
        let focused = self.focused_pane_id;
        let had_other_tabs = self.pane_tab_count(focused) > 0;
        let tab_id = self.create_terminal_tab(agent_cli, cx);
        if had_other_tabs {
            self.split_pane_with_tab(focused, SplitDirection::Right, &tab_id, cx);
        }
        tab_id
    }

    /// The chat/terminal toggle: opens a terminal tab in a right split when no
    /// terminal tab exists, otherwise closes every terminal tab (collapsing
    /// panes that become empty).
    pub(crate) fn toggle_terminal_panel(&mut self, cx: &mut Context<Self>) {
        let terminal_ids: Vec<String> = self
            .all_tabs()
            .filter(|tab| tab.kind.is_terminal())
            .map(|tab| tab.id.clone())
            .collect();
        if terminal_ids.is_empty() {
            self.open_terminal_tab_in_right_split(self.app_settings.terminal_launch_cli(), cx);
        } else {
            self.pending_close_tab_ids = terminal_ids;
            if let Some(tab_id) = self.pending_close_tab_ids.first().cloned() {
                self.pending_close_tab_ids.remove(0);
                self.request_close_tab(&tab_id, cx);
            }
        }
    }

    /// Writes user-initiated input to the PTY: scrolls the viewport back to
    /// the bottom and clears any selection first (matching common terminal
    /// behavior; see Zed's `Terminal::input`).
    fn terminal_write_user_input(&mut self, tab_id: &str, bytes: &[u8]) {
        let Some(state) = self.terminal_tab_states.get_mut(tab_id) else {
            return;
        };
        state.selection = None;
        state.selecting = false;
        if let Some(session) = state.session.as_mut() {
            if session.snapshot.display_offset != 0 {
                session.scroll_display(Scroll::Bottom);
            }
            let _ = session.write_bytes(bytes);
        }
    }

    fn send_chat_terminal_bytes(
        &mut self,
        tab_id: &str,
        bytes: &[u8],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_terminal_tab(tab_id, window);
        cx.stop_propagation();
        self.pause_cursor_blinking(cx);
        if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
            state.marked_text = None;
        }
        self.terminal_write_user_input(tab_id, bytes);
    }

    pub(crate) fn record_terminal_focus(&mut self, tab_id: &str) {
        if self.pane_id_for_tab(tab_id).is_none() {
            return;
        }
        if self
            .terminal_focus_history
            .last()
            .is_some_and(|id| id == tab_id)
        {
            return;
        }
        self.terminal_focus_history.retain(|id| id != tab_id);
        self.terminal_focus_history.push(tab_id.to_string());
    }

    pub(crate) fn add_path_to_chat(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let Some(relative_path) = self.relative_path_for_path(path) else {
            return;
        };
        self.add_reference_to_terminal(format!("@{relative_path} "), cx);
    }

    pub(crate) fn add_paths_to_chat(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let references = paths
            .iter()
            .filter_map(|path| self.relative_path_for_path(path))
            .map(|path| format!("@{path} "))
            .collect::<String>();
        if !references.is_empty() {
            self.add_reference_to_terminal(references, cx);
        }
    }

    pub(crate) fn add_selection_to_chat(
        &mut self,
        path: &std::path::Path,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) {
        let Some(relative_path) = self.relative_path_for_path(path) else {
            return;
        };
        let (start_line, start_col) = Self::chat_reference_position(&self.buffer, range.start);
        let reference = if range.is_empty() {
            format!("@{relative_path} L{start_line}:C{start_col} ")
        } else {
            let (end_line, end_col) =
                Self::chat_reference_position(&self.buffer, range.end.saturating_sub(1));
            format!("@{relative_path} L{start_line}:C{start_col}-L{end_line}:C{end_col} ")
        };
        self.add_reference_to_terminal(reference, cx);
    }

    fn chat_reference_position(buffer: &crate::buffer::Buffer, offset: usize) -> (usize, usize) {
        let clamped = buffer.clamp_offset(offset.min(buffer.len()));
        let (line, byte_col) = buffer.offset_to_line_col(clamped);
        let line_content = buffer.line_content(line);
        let column = line_content[..byte_col.min(line_content.len())]
            .chars()
            .count()
            + 1;
        (line + 1, column)
    }

    fn add_reference_to_terminal(&mut self, reference: String, cx: &mut Context<Self>) {
        let tab_id = self
            .active_terminal_tab_id()
            .or_else(|| {
                self.terminal_focus_history
                    .iter()
                    .rev()
                    .find(|id| self.pane_id_for_tab(id).is_some())
                    .cloned()
            })
            .or_else(|| {
                self.all_tabs()
                    .find(|tab| tab.kind.is_terminal())
                    .map(|tab| tab.id.clone())
            })
            .unwrap_or_else(|| {
                self.open_terminal_tab_in_right_split(self.app_settings.terminal_launch_cli(), cx)
            });

        if let Some(pane_id) = self.pane_id_for_tab(&tab_id) {
            self.focus_pane(pane_id, cx);
        }
        if let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) {
            self.activate_tab_by_index(index, cx);
        }

        if self.terminal_session(&tab_id).is_some() {
            self.terminal_write_user_input(&tab_id, reference.as_bytes());
        } else {
            self.pending_terminal_inputs
                .entry(tab_id.clone())
                .or_default()
                .extend_from_slice(reference.as_bytes());
        }
        self.pending_terminal_focus = Some(tab_id);
        cx.notify();
    }

    fn swallow_chat_terminal_editor_action(
        &mut self,
        tab_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_terminal_tab(tab_id, window);
        cx.stop_propagation();
    }

    pub(crate) fn chat_panel_working_directory(&self) -> Option<PathBuf> {
        self.current_file_path
            .as_ref()
            .and_then(|path| self.workspace_root_for_path(path))
            .or_else(|| {
                (!self.sidebar_root.as_os_str().is_empty()).then(|| self.sidebar_root.clone())
            })
            // Without a workspace, behave like a regular terminal app.
            .or_else(|| {
                env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|path| path.is_dir())
            })
    }

    /// Lazily spawns the PTY session for a terminal tab (no-op when already
    /// running or when the tab is not a terminal tab).
    pub(crate) fn ensure_terminal_session(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        let Some(agent_cli) = self
            .all_tabs()
            .find(|tab| tab.id == tab_id)
            .and_then(|tab| tab.kind.terminal_agent())
        else {
            return;
        };
        self.ensure_terminal_tab_state(tab_id, cx);
        let Some(state) = self.terminal_tab_states.get(tab_id) else {
            return;
        };
        if let Some(session) = &state.session {
            // Retry once the login-shell PATH prefetch makes a previously
            // "missing" CLI resolvable.
            let missing_command_retry = matches!(
                &session.status,
                ChatTerminalStatus::Unavailable(message) if message.contains("command not found")
            ) && agent_cli_command_exists(agent_cli);
            if !missing_command_retry {
                return;
            }
        }

        let session = self.build_chat_terminal_session(agent_cli);
        let should_poll = matches!(session.status, ChatTerminalStatus::Running);
        let initial_title = session.dynamic_title.clone();
        if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
            state.session = Some(session);
        }
        if let Some(title) = initial_title
            && let Some(tab) = self.all_tabs_mut().find(|tab| tab.id == tab_id)
        {
            tab.title = title;
        }
        if should_poll {
            self.start_chat_terminal_poll(tab_id.to_string(), cx);
        }
        self.resize_chat_terminal(tab_id);
        cx.notify();
    }

    pub(crate) fn set_terminal_cursor_shape(&mut self, cursor_shape: TerminalCursorShape) {
        self.app_settings.terminal_cursor_shape = cursor_shape.as_str().to_string();
        for state in self.terminal_tab_states.values_mut() {
            if let Some(session) = state.session.as_mut() {
                session.set_cursor_shape(cursor_shape);
            }
        }
    }

    /// Stops every terminal PTY session (used on shutdown).
    pub(crate) fn stop_all_terminal_sessions(&mut self) {
        self.pending_terminal_focus = None;
        for state in self.terminal_tab_states.values_mut() {
            if let Some(session) = state.session.as_mut() {
                session.stop();
            }
        }
    }

    /// Clears the terminal screen and scrollback (Cmd+K).
    pub(crate) fn clear_terminal(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        let Some(state) = self.terminal_tab_states.get_mut(tab_id) else {
            return;
        };
        state.selection = None;
        state.selecting = false;
        if let Some(session) = state.session.as_mut() {
            session.clear();
            if let Some(pattern) = state.search_pattern.clone() {
                state.search_matches = session.search(&pattern, TERMINAL_SEARCH_MAX_MATCHES);
            }
        }
        self.search_current_index = None;
        cx.notify();
    }

    /// Builds the regex pattern for terminal search from the shared search-bar
    /// options. Returns None for an empty (or invalid regex) query.
    fn terminal_search_pattern(&self) -> Option<String> {
        let query = self.search_query.trim_end_matches('\n');
        if query.is_empty() {
            return None;
        }
        let mut pattern = if self.search_use_regex {
            query.to_string()
        } else {
            regex::escape(query)
        };
        if self.search_whole_word {
            pattern = format!(r"\b(?:{pattern})\b");
        }
        if !self.search_case_sensitive {
            pattern = format!("(?i){pattern}");
        }
        Some(pattern)
    }

    /// Runs the search-bar query against the terminal grid (scrollback
    /// included) and highlights/activates the match closest to the bottom.
    pub(crate) fn run_terminal_search(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        let pattern = self.terminal_search_pattern();
        let Some(state) = self.terminal_tab_states.get_mut(tab_id) else {
            return;
        };

        let Some(pattern) = pattern else {
            state.search_pattern = None;
            state.search_matches.clear();
            self.search_current_index = None;
            cx.notify();
            return;
        };

        state.search_pattern = Some(pattern.clone());
        state.search_matches = state
            .session
            .as_ref()
            .map(|session| session.search(&pattern, TERMINAL_SEARCH_MAX_MATCHES))
            .unwrap_or_default();

        if state.search_matches.is_empty() {
            self.search_current_index = None;
        } else {
            // Terminals usually care about the most recent output: activate
            // the last match and bring it into view.
            let index = state.search_matches.len() - 1;
            self.search_current_index = Some(index);
            self.scroll_terminal_match_into_view(tab_id, index);
        }
        cx.notify();
    }

    pub(crate) fn terminal_search_match_count(&self, tab_id: &str) -> usize {
        self.terminal_tab_state(tab_id)
            .map(|state| state.search_matches.len())
            .unwrap_or(0)
    }

    pub(crate) fn terminal_search_navigate(
        &mut self,
        tab_id: &str,
        forward: bool,
        cx: &mut Context<Self>,
    ) {
        let count = self.terminal_search_match_count(tab_id);
        if count == 0 {
            if !self.search_query.is_empty() {
                self.run_terminal_search(tab_id, cx);
            }
            return;
        }

        let index = match self.search_current_index {
            Some(current) if forward => (current + 1) % count,
            Some(current) => (current + count - 1) % count,
            None => {
                if forward {
                    0
                } else {
                    count - 1
                }
            }
        };
        self.search_current_index = Some(index.min(count - 1));
        self.scroll_terminal_match_into_view(tab_id, index);
        cx.notify();
    }

    fn scroll_terminal_match_into_view(&mut self, tab_id: &str, index: usize) {
        let Some(line) = self
            .terminal_tab_state(tab_id)
            .and_then(|state| state.search_matches.get(index))
            .map(|search_match| search_match.start().line.0)
        else {
            return;
        };
        if let Some(session) = self.terminal_session_mut(tab_id) {
            session.scroll_line_into_view(line);
        }
    }

    /// Clears terminal search state (search bar dismissed).
    pub(crate) fn clear_terminal_search(&mut self, tab_id: &str) {
        if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
            state.search_pattern = None;
            state.search_matches.clear();
        }
    }

    fn start_chat_terminal_poll(&mut self, tab_id: String, cx: &mut Context<Self>) {
        let Some(session) = self.terminal_session_mut(&tab_id) else {
            return;
        };

        session.poll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let done = this
                    .update(cx, |this, cx| {
                        let (mut changed, exited, title_update) = {
                            let Some(state) = this.terminal_tab_states.get_mut(&tab_id) else {
                                return true;
                            };
                            let Some(session) = state.session.as_mut() else {
                                return true;
                            };

                            let mut changed = false;
                            let mut exited = false;
                            let mut clear_interaction_state = false;
                            let events = session
                                .receiver
                                .as_ref()
                                .map(|receiver| receiver.try_iter().collect::<Vec<_>>())
                                .unwrap_or_default();
                            for event in events {
                                changed = true;
                                match event {
                                    ChatTerminalEvent::Updated => {
                                        if matches!(session.status, ChatTerminalStatus::Launching) {
                                            session.status = ChatTerminalStatus::Running;
                                        }
                                        state.selection = None;
                                        state.selecting = false;
                                        session.refresh_snapshot();
                                        // Keep active search results in sync with
                                        // new output.
                                        if let Some(pattern) = state.search_pattern.clone() {
                                            state.search_matches = session
                                                .search(&pattern, TERMINAL_SEARCH_MAX_MATCHES);
                                        }
                                    }
                                    ChatTerminalEvent::Exited => {
                                        session.status = ChatTerminalStatus::Exited;
                                        clear_interaction_state = true;
                                        session.refresh_snapshot();
                                        session.poll_task = None;
                                        exited = true;
                                    }
                                    ChatTerminalEvent::Error(message) => {
                                        session.status = ChatTerminalStatus::Unavailable(message);
                                        clear_interaction_state = true;
                                        session.poll_task = None;
                                        exited = true;
                                    }
                                }
                            }

                            let title_update =
                                (!exited).then(|| session.refresh_dynamic_title()).flatten();
                            if clear_interaction_state {
                                state.clear_interaction_state();
                            }
                            (changed, exited, title_update)
                        };

                        if let Some(title) = title_update
                            && let Some(tab) = this.all_tabs_mut().find(|tab| tab.id == tab_id)
                            && tab.title != title
                        {
                            tab.title = title;
                            changed = true;
                        }
                        if changed {
                            cx.notify();
                        }

                        exited
                    })
                    .unwrap_or(true);
                if done {
                    return;
                }

                cx.background_executor()
                    .timer(Duration::from_millis(CHAT_PANEL_POLL_MS))
                    .await;
            }
        }));
    }

    fn resize_chat_terminal(&mut self, tab_id: &str) {
        let (_, cell_width, cell_height) = self.chat_terminal_metrics();
        let Some(state) = self.terminal_tab_states.get_mut(tab_id) else {
            return;
        };
        let Some(bounds) = state.input_bounds else {
            return;
        };
        let panel_width = f32::from(bounds.size.width).max(220.0);
        let panel_height = f32::from(bounds.size.height).max(120.0);
        let cols = (panel_width / cell_width.max(1.0)).floor() as u16;
        let row_height = cell_height.max(1.0);
        let raw_rows = panel_height / row_height;
        let whole_rows = raw_rows.floor();
        let trailing_gap = panel_height - whole_rows * row_height;
        let rows = if trailing_gap >= row_height * 0.6 {
            whole_rows as u16 + 1
        } else {
            whole_rows as u16
        };
        if let Some(session) = state.session.as_mut()
            && (session.rows != rows.max(2) || session.cols != cols.max(20))
        {
            session.resize(rows, cols);
        }
    }

    fn on_chat_terminal_key_down(
        &mut self,
        tab_id: &str,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_terminal_tab(tab_id, window);
        if should_route_chat_terminal_text_input(&event.keystroke) {
            return;
        }
        cx.stop_propagation();

        let mode = self
            .terminal_session(tab_id)
            .map(ChatTerminalSession::term_mode)
            .unwrap_or(TermMode::NONE);

        // Shift+PageUp/PageDown/Home/End scroll the local viewport instead of
        // being sent to the application (unless it runs on the alt screen).
        let modifiers = event.keystroke.modifiers;
        if modifiers.shift
            && !modifiers.control
            && !modifiers.alt
            && !modifiers.platform
            && !mode.contains(TermMode::ALT_SCREEN)
        {
            let scroll = match event.keystroke.key.as_str() {
                "pageup" => Some(Scroll::PageUp),
                "pagedown" => Some(Scroll::PageDown),
                "home" => Some(Scroll::Top),
                "end" => Some(Scroll::Bottom),
                _ => None,
            };
            if let Some(scroll) = scroll {
                if let Some(session) = self.terminal_session_mut(tab_id) {
                    session.scroll_display(scroll);
                    self.reveal_panel_scrollbar(PanelScrollbarKind::ChatTerminal, cx);
                    cx.notify();
                }
                return;
            }
        }

        let Some(bytes) = keystroke_to_terminal_bytes(&event.keystroke, mode) else {
            return;
        };
        self.pause_cursor_blinking(cx);
        self.terminal_write_user_input(tab_id, &bytes);
        cx.notify();
    }

    fn on_chat_terminal_scroll(
        &mut self,
        tab_id: &str,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_terminal_tab(tab_id, window);
        cx.stop_propagation();
        if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
            state.selection = None;
            state.selecting = false;
        }
        self.reveal_panel_scrollbar(PanelScrollbarKind::ChatTerminal, cx);

        let (_, cell_width, line_height) = self.chat_terminal_metrics();
        if self.forward_chat_terminal_protocol_scroll(tab_id, event, cell_width, line_height) {
            cx.notify();
            return;
        }
        let Some(delta) = self.chat_terminal_scroll_lines(tab_id, event, line_height) else {
            return;
        };

        if let Some(session) = self.terminal_session_mut(tab_id) {
            session.scroll_display(Scroll::Delta(delta));
            cx.notify();
        }
    }

    fn chat_terminal_scroll_lines(
        &mut self,
        tab_id: &str,
        event: &ScrollWheelEvent,
        line_height: f32,
    ) -> Option<i32> {
        let line_height = px(line_height.max(1.0));
        let scroll_height = self
            .terminal_session(tab_id)
            .map(|session| line_height * session.snapshot.rows.max(1) as f32)
            .unwrap_or(line_height);
        let state = self.terminal_tab_states.get_mut(tab_id)?;
        match event.touch_phase {
            TouchPhase::Started => {
                state.scroll_px = Pixels::ZERO;
                None
            }
            TouchPhase::Moved => {
                let old_offset = (state.scroll_px / line_height) as i32;
                state.scroll_px += event.delta.pixel_delta(line_height).y;
                let new_offset = (state.scroll_px / line_height) as i32;
                state.scroll_px %= scroll_height;
                Some(new_offset - old_offset)
            }
            TouchPhase::Ended => None,
        }
    }

    fn on_chat_terminal_paste_action(
        &mut self,
        tab_id: &str,
        _: &Paste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.swallow_chat_terminal_editor_action(tab_id, window, cx);
        self.paste_chat_terminal_from_clipboard(tab_id, cx);
    }

    pub(crate) fn paste_chat_terminal_from_clipboard(
        &mut self,
        tab_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let Some(text) = item.text() else {
            return;
        };
        self.pause_cursor_blinking(cx);

        // Bracketed paste lets applications (vim, zsh, ...) treat the payload
        // as a literal block; otherwise newlines must become carriage returns.
        let bracketed = self
            .terminal_session(tab_id)
            .is_some_and(|session| session.term_mode().contains(TermMode::BRACKETED_PASTE));
        let bytes = if bracketed {
            let mut bytes = b"\x1b[200~".to_vec();
            bytes.extend_from_slice(text.replace("\x1b[201~", "").as_bytes());
            bytes.extend_from_slice(b"\x1b[201~");
            bytes
        } else {
            text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
        };
        self.terminal_write_user_input(tab_id, &bytes);
        cx.notify();
    }

    fn chat_terminal_cell_from_position(
        &self,
        tab_id: &str,
        position: Point<Pixels>,
    ) -> Option<(usize, usize)> {
        let bounds = self.terminal_tab_state(tab_id)?.input_bounds?;
        let local = bounds.localize(&position)?;
        let (_, cell_width, cell_height) = self.chat_terminal_metrics();
        let snapshot = self
            .terminal_session(tab_id)
            .map(|session| session.snapshot.clone())
            .unwrap_or_default();
        if snapshot.rows == 0 || snapshot.cols == 0 {
            return None;
        }

        let row =
            ((f32::from(local.y) / cell_height).floor() as usize).min(snapshot.rows as usize - 1);
        let col =
            ((f32::from(local.x) / cell_width).floor() as usize).min(snapshot.cols as usize - 1);
        Some((row, col))
    }

    fn chat_terminal_normalized_col(&self, tab_id: &str, row: usize, col: usize) -> Option<usize> {
        let snapshot = self
            .terminal_session(tab_id)
            .map(|session| session.snapshot.clone())
            .unwrap_or_default();
        let rows = Self::chat_terminal_rows(&snapshot);
        let row_cells = &rows.get(row)?.cells;
        previous_non_spacer_terminal_col(row_cells, col)
    }

    fn chat_terminal_mouse_event_cell(
        &self,
        tab_id: &str,
        position: Point<Pixels>,
    ) -> Option<(usize, usize)> {
        let (row, col) = self.chat_terminal_cell_from_position(tab_id, position)?;
        Some((
            row,
            self.chat_terminal_normalized_col(tab_id, row, col)
                .unwrap_or(col),
        ))
    }

    /// The plain text of the current selection, if any.
    pub(crate) fn chat_terminal_selection_text(&self, tab_id: &str) -> Option<String> {
        let selection = self
            .terminal_tab_state(tab_id)
            .and_then(|state| state.selection)?;
        if selection.is_empty() {
            return None;
        }

        let snapshot = self
            .terminal_session(tab_id)
            .map(|session| session.snapshot.clone())
            .unwrap_or_default();
        let rows = Self::chat_terminal_rows(&snapshot);
        let selection = selection.normalized();
        let mut text = String::new();

        for row_index in selection.start_row..=selection.end_row {
            let Some(row) = rows.get(row_index) else {
                continue;
            };
            let start_col = if row_index == selection.start_row {
                selection.start_col
            } else {
                0
            };
            let end_col = if row_index == selection.end_row {
                selection.end_col
            } else {
                row.cells.len().saturating_sub(1)
            };

            let mut line = String::new();
            for cell in row
                .cells
                .iter()
                .skip(start_col)
                .take(end_col.saturating_sub(start_col) + 1)
            {
                if is_chat_terminal_spacer_cell(cell) {
                    continue;
                }
                line.push(if cell.c == '\u{00A0}' { ' ' } else { cell.c });
                if let Some(extra) = cell.zerowidth() {
                    for ch in extra {
                        line.push(*ch);
                    }
                }
            }
            text.push_str(line.trim_end_matches(' '));
            if row_index < selection.end_row {
                let wrapped = end_col + 1 >= row.cells.len()
                    && row
                        .cells
                        .last()
                        .is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE));
                if !wrapped {
                    text.push('\n');
                }
            }
        }

        (!text.is_empty()).then_some(text)
    }

    pub(crate) fn copy_chat_terminal_selection(
        &self,
        tab_id: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(text) = self.chat_terminal_selection_text(tab_id) else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    fn chat_terminal_selection_for_click(
        &self,
        tab_id: &str,
        row: usize,
        col: usize,
    ) -> Option<ChatTerminalSelection> {
        let col = self
            .chat_terminal_normalized_col(tab_id, row, col)
            .unwrap_or(col);
        self.chat_terminal_word_selection(tab_id, row, col)
            .or_else(|| Some(ChatTerminalSelection::single(row, col)))
    }

    pub(crate) fn select_all_chat_terminal(
        &mut self,
        tab_id: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let snapshot = self
            .terminal_session(tab_id)
            .map(|session| session.snapshot.clone())
            .unwrap_or_default();
        let rows = Self::chat_terminal_rows(&snapshot);
        if rows.is_empty() {
            return false;
        }

        let end_row = rows.len().saturating_sub(1);
        let end_col = rows[end_row]
            .cells
            .iter()
            .rposition(|cell| !is_chat_terminal_spacer_cell(cell))
            .unwrap_or(0);

        let Some(state) = self.terminal_tab_states.get_mut(tab_id) else {
            return false;
        };
        state.selection = Some(ChatTerminalSelection {
            start_row: 0,
            start_col: 0,
            end_row,
            end_col,
        });
        state.selecting = false;
        cx.notify();
        true
    }

    fn chat_terminal_word_selection(
        &self,
        tab_id: &str,
        row: usize,
        col: usize,
    ) -> Option<ChatTerminalSelection> {
        self.chat_terminal_expanded_selection(tab_id, row, col, false)
            .or_else(|| {
                let snapshot = self
                    .terminal_session(tab_id)
                    .map(|session| session.snapshot.clone())
                    .unwrap_or_default();
                let rows = Self::chat_terminal_rows(&snapshot);
                let row_cells = &rows.get(row)?.cells;
                let (start, end) = chat_terminal_word_bounds(row_cells, col)?;
                Some(ChatTerminalSelection {
                    start_row: row,
                    start_col: start,
                    end_row: row,
                    end_col: end,
                })
            })
    }

    fn chat_terminal_expanded_selection(
        &self,
        tab_id: &str,
        row: usize,
        col: usize,
        line_selection: bool,
    ) -> Option<ChatTerminalSelection> {
        let session = self.terminal_session(tab_id)?;
        let rows = Self::chat_terminal_rows(&session.snapshot);
        let source_line = rows.get(row)?.source_line;
        let (start, end) = session.expanded_selection_range(
            AlacPoint::new(Line(source_line), Column(col)),
            line_selection,
        )?;
        let first_source_line = rows.first()?.source_line;
        let last_source_line = rows.last()?.source_line;
        if end.line.0 < first_source_line || start.line.0 > last_source_line {
            return None;
        }

        let start_row = rows
            .iter()
            .position(|candidate| candidate.source_line == start.line.0)
            .unwrap_or(0);
        let end_row = rows
            .iter()
            .position(|candidate| candidate.source_line == end.line.0)
            .unwrap_or_else(|| rows.len().saturating_sub(1));
        let start_col = if start.line.0 < first_source_line {
            0
        } else {
            start.column.0
        };
        let end_col = if end.line.0 > last_source_line {
            rows[end_row].cells.len().saturating_sub(1)
        } else {
            end.column.0
        };

        Some(ChatTerminalSelection {
            start_row,
            start_col,
            end_row,
            end_col,
        })
    }

    /// Triple-click selects the complete logical line, including wrapped rows.
    fn chat_terminal_line_selection(
        &self,
        tab_id: &str,
        row: usize,
    ) -> Option<ChatTerminalSelection> {
        self.chat_terminal_expanded_selection(tab_id, row, 0, true)
    }

    fn send_chat_terminal_mouse_button_report(
        &mut self,
        tab_id: &str,
        button: MouseButton,
        position: Point<Pixels>,
        shift: bool,
        alt: bool,
        control: bool,
    ) -> bool {
        let is_mouse_mode = self
            .terminal_session(tab_id)
            .is_some_and(|session| session.mouse_mode(shift));
        if !is_mouse_mode {
            return false;
        }
        let Some(button_code) = chat_terminal_mouse_button_code(button) else {
            return false;
        };
        let Some((row, col)) = self.chat_terminal_mouse_event_cell(tab_id, position) else {
            return false;
        };

        if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
            state.selection = None;
            state.selecting = false;
            state.last_mouse_cell = Some((row, col));
        }
        self.set_chat_terminal_mouse_button_state(tab_id, button, true);
        if let Some(session) = self.terminal_session(tab_id) {
            let _ = session.write_mouse_report(row, col, button_code, false, shift, alt, control);
        }
        true
    }

    fn send_chat_terminal_mouse_release_report(
        &mut self,
        tab_id: &str,
        button: MouseButton,
        position: Point<Pixels>,
        shift: bool,
        alt: bool,
        control: bool,
    ) -> bool {
        let is_mouse_mode = self
            .terminal_session(tab_id)
            .is_some_and(|session| session.mouse_mode(shift));
        self.set_chat_terminal_mouse_button_state(tab_id, button, false);
        if !is_mouse_mode {
            if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
                state.last_mouse_cell = None;
            }
            return false;
        }
        let Some(button_code) = chat_terminal_mouse_button_code(button) else {
            if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
                state.last_mouse_cell = None;
            }
            return false;
        };
        let cell = self
            .chat_terminal_mouse_event_cell(tab_id, position)
            .or_else(|| {
                self.terminal_tab_state(tab_id)
                    .and_then(|state| state.last_mouse_cell)
            });
        if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
            state.last_mouse_cell = None;
        }
        let Some((row, col)) = cell else {
            return false;
        };
        if let Some(session) = self.terminal_session(tab_id) {
            let _ = session.write_mouse_report(row, col, button_code, true, shift, alt, control);
        }
        true
    }

    fn send_chat_terminal_mouse_motion_report(
        &mut self,
        tab_id: &str,
        position: Point<Pixels>,
        shift: bool,
        alt: bool,
        control: bool,
    ) -> bool {
        let Some(mode) = self
            .terminal_session(tab_id)
            .map(ChatTerminalSession::term_mode)
        else {
            return false;
        };
        if shift || !mode.intersects(TermMode::MOUSE_MOTION | TermMode::MOUSE_DRAG) {
            return false;
        }
        let Some((row, col)) = self.chat_terminal_mouse_event_cell(tab_id, position) else {
            return false;
        };
        let Some(state) = self.terminal_tab_states.get_mut(tab_id) else {
            return false;
        };
        if state.last_mouse_cell == Some((row, col)) {
            return true;
        }
        state.last_mouse_cell = Some((row, col));

        let button_code = if state.mouse_left_pressed {
            Some(32)
        } else if state.mouse_middle_pressed {
            Some(33)
        } else if state.mouse_right_pressed {
            Some(34)
        } else if mode.contains(TermMode::MOUSE_MOTION) {
            Some(35)
        } else {
            None
        };
        if let Some(button_code) = button_code
            && let Some(session) = self.terminal_session(tab_id)
        {
            let _ = session.write_mouse_report(row, col, button_code, false, shift, alt, control);
        }
        true
    }

    fn forward_chat_terminal_protocol_scroll(
        &mut self,
        tab_id: &str,
        event: &ScrollWheelEvent,
        cell_width: f32,
        line_height: f32,
    ) -> bool {
        let modifiers = event.modifiers;
        let Some(session) = self.terminal_session(tab_id) else {
            return false;
        };
        let mode = session.term_mode();
        let alternate_scroll = session.alternate_scroll_mode(modifiers.shift);

        if !modifiers.shift && mode.intersects(TermMode::MOUSE_MODE) {
            match event.touch_phase {
                TouchPhase::Started => {
                    if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
                        state.mouse_scroll_px = point(Pixels::ZERO, Pixels::ZERO);
                    }
                    return true;
                }
                TouchPhase::Ended => return true,
                TouchPhase::Moved => {}
            }

            let Some((row, col)) = self.chat_terminal_mouse_event_cell(tab_id, event.position)
            else {
                return true;
            };
            let delta = event.delta.pixel_delta(px(line_height.max(1.0)));
            let step_width = px(cell_width.max(1.0));
            let step_height = px(line_height.max(1.0));
            let Some(state) = self.terminal_tab_states.get_mut(tab_id) else {
                return true;
            };
            state.mouse_scroll_px.x += delta.x;
            state.mouse_scroll_px.y += delta.y;

            let vertical_steps = (state.mouse_scroll_px.y / step_height) as i32;
            let horizontal_steps = (state.mouse_scroll_px.x / step_width) as i32;
            state.last_mouse_cell = Some((row, col));
            state.mouse_scroll_px.x %= step_width;
            state.mouse_scroll_px.y %= step_height;
            if let Some(session) = self.terminal_session(tab_id) {
                for _ in 0..vertical_steps.abs() {
                    let button_code = if vertical_steps > 0 { 64 } else { 65 };
                    let _ = session.write_mouse_report(
                        row,
                        col,
                        button_code,
                        false,
                        modifiers.shift,
                        modifiers.alt,
                        modifiers.control,
                    );
                }
                for _ in 0..horizontal_steps.abs() {
                    let button_code = if horizontal_steps > 0 { 66 } else { 67 };
                    let _ = session.write_mouse_report(
                        row,
                        col,
                        button_code,
                        false,
                        modifiers.shift,
                        modifiers.alt,
                        modifiers.control,
                    );
                }
            }
            return true;
        }

        if alternate_scroll {
            match event.touch_phase {
                TouchPhase::Started => {
                    if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
                        state.mouse_scroll_px = point(Pixels::ZERO, Pixels::ZERO);
                    }
                    return true;
                }
                TouchPhase::Ended => return true,
                TouchPhase::Moved => {}
            }

            let delta = event.delta.pixel_delta(px(line_height.max(1.0)));
            let step_width = px(cell_width.max(1.0));
            let step_height = px(line_height.max(1.0));
            let Some(state) = self.terminal_tab_states.get_mut(tab_id) else {
                return true;
            };
            state.mouse_scroll_px.x += delta.x;
            state.mouse_scroll_px.y += delta.y;

            let vertical_steps = (state.mouse_scroll_px.y / step_height) as i32;
            let horizontal_steps = (state.mouse_scroll_px.x / step_width) as i32;
            state.mouse_scroll_px.x %= step_width;
            state.mouse_scroll_px.y %= step_height;
            let mut bytes = Vec::new();
            for _ in 0..vertical_steps.abs() {
                bytes.extend_from_slice(if vertical_steps > 0 {
                    b"\x1bOA"
                } else {
                    b"\x1bOB"
                });
            }
            for _ in 0..horizontal_steps.abs() {
                bytes.extend_from_slice(if horizontal_steps > 0 {
                    b"\x1bOD"
                } else {
                    b"\x1bOC"
                });
            }
            if !bytes.is_empty()
                && let Some(session) = self.terminal_session(tab_id)
            {
                let _ = session.write_bytes(&bytes);
            }
            return true;
        }

        false
    }

    fn on_chat_terminal_mouse_down(
        &mut self,
        tab_id: &str,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(owner) = self.pane_id_for_tab(tab_id)
            && owner != self.focused_pane_id
        {
            self.focus_pane(owner, cx);
        }
        self.focus_terminal_tab(tab_id, window);
        cx.stop_propagation();
        if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
            state.marked_text = None;
        }
        if self.send_chat_terminal_mouse_button_report(
            tab_id,
            MouseButton::Left,
            event.position,
            event.modifiers.shift,
            event.modifiers.alt,
            event.modifiers.control,
        ) {
            cx.notify();
            return;
        }
        if let Some((row, col)) = self.chat_terminal_cell_from_position(tab_id, event.position) {
            let col = self
                .chat_terminal_normalized_col(tab_id, row, col)
                .unwrap_or(col);
            let shift_extend = event.modifiers.shift
                && event.click_count == 1
                && self
                    .terminal_tab_state(tab_id)
                    .is_some_and(|state| state.selection.is_some());
            if shift_extend {
                if let Some(selection) = self
                    .terminal_tab_states
                    .get_mut(tab_id)
                    .and_then(|state| state.selection.as_mut())
                {
                    selection.end_row = row;
                    selection.end_col = col;
                }
                if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
                    state.selecting = true;
                }
                cx.notify();
                return;
            }
            let selection = match event.click_count {
                1 => Some(ChatTerminalSelection::single(row, col)),
                2 => self.chat_terminal_selection_for_click(tab_id, row, col),
                3 => self.chat_terminal_line_selection(tab_id, row),
                _ => None,
            };
            if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
                state.selection = selection;
                state.selecting = event.click_count < 2;
            }
            cx.notify();
        } else if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
            state.selection = None;
            state.selecting = false;
        }
    }

    fn on_chat_terminal_middle_mouse_down(
        &mut self,
        tab_id: &str,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_terminal_tab(tab_id, window);
        cx.stop_propagation();
        if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
            state.marked_text = None;
        }
        if self.send_chat_terminal_mouse_button_report(
            tab_id,
            MouseButton::Middle,
            event.position,
            event.modifiers.shift,
            event.modifiers.alt,
            event.modifiers.control,
        ) {
            cx.notify();
        }
    }

    fn on_chat_terminal_mouse_move(
        &mut self,
        tab_id: &str,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.send_chat_terminal_mouse_motion_report(
            tab_id,
            event.position,
            event.modifiers.shift,
            event.modifiers.alt,
            event.modifiers.control,
        ) {
            return;
        }
        if !self
            .terminal_tab_state(tab_id)
            .is_some_and(|state| state.selecting)
            || !event.dragging()
        {
            return;
        }

        // Dragging beyond the vertical bounds auto-scrolls the viewport
        // (clamped to 3 rows per event, like Zed) and shifts the
        // viewport-relative selection anchor to compensate.
        let Some(bounds) = self
            .terminal_tab_state(tab_id)
            .and_then(|state| state.input_bounds)
        else {
            return;
        };
        let (_, _, cell_height) = self.chat_terminal_metrics();
        let top = f32::from(bounds.origin.y);
        let bottom = top + f32::from(bounds.size.height);
        let left = f32::from(bounds.origin.x);
        let right = left + f32::from(bounds.size.width);
        let y = f32::from(event.position.y);
        let scroll_delta = if y < top {
            (((top - y) / cell_height.max(1.0)).ceil() as i32).clamp(1, 3)
        } else if y > bottom {
            -(((y - bottom) / cell_height.max(1.0)).ceil() as i32).clamp(1, 3)
        } else {
            0
        };
        if scroll_delta != 0 {
            let max_row = self
                .terminal_session(tab_id)
                .map(|session| session.snapshot.rows.max(1) as i32 - 1)
                .unwrap_or(0);
            if let Some(session) = self.terminal_session_mut(tab_id) {
                session.scroll_display(Scroll::Delta(scroll_delta));
            }
            if let Some(selection) = self
                .terminal_tab_states
                .get_mut(tab_id)
                .and_then(|state| state.selection.as_mut())
            {
                selection.start_row =
                    (selection.start_row as i32 + scroll_delta).clamp(0, max_row) as usize;
            }
        }

        let clamped = point(
            px(f32::from(event.position.x).clamp(left, (right - 1.0).max(left))),
            px(y.clamp(top, (bottom - 1.0).max(top))),
        );
        let Some((row, col)) = self.chat_terminal_cell_from_position(tab_id, clamped) else {
            return;
        };
        let col = self
            .chat_terminal_normalized_col(tab_id, row, col)
            .unwrap_or(col);

        if let Some(selection) = self
            .terminal_tab_states
            .get_mut(tab_id)
            .and_then(|state| state.selection.as_mut())
        {
            selection.end_row = row;
            selection.end_col = col;
            cx.notify();
        }
    }

    fn on_chat_terminal_context_menu(
        &mut self,
        tab_id: &str,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_terminal_tab(tab_id, window);
        window.prevent_default();
        cx.stop_propagation();
        if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
            state.marked_text = None;
        }
        if self.send_chat_terminal_mouse_button_report(
            tab_id,
            MouseButton::Right,
            event.position,
            event.modifiers.shift,
            event.modifiers.alt,
            event.modifiers.control,
        ) {
            cx.notify();
            return;
        }
        if let Some((row, col)) = self.chat_terminal_cell_from_position(tab_id, event.position) {
            let col = self
                .chat_terminal_normalized_col(tab_id, row, col)
                .unwrap_or(col);
            if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
                let keep_selection = state
                    .selection
                    .map(ChatTerminalSelection::normalized)
                    .is_some_and(|selection| !selection.is_empty() && selection.contains(row, col));
                if keep_selection {
                    self.open_terminal_context_menu(tab_id.to_string(), event.position, cx);
                    return;
                }
            }
            let selection = self.chat_terminal_selection_for_click(tab_id, row, col);
            if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
                state.selection = selection;
                state.selecting = false;
            }
        }
        self.open_terminal_context_menu(tab_id.to_string(), event.position, cx);
    }

    fn on_chat_terminal_mouse_up(
        &mut self,
        tab_id: &str,
        event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.send_chat_terminal_mouse_release_report(
            tab_id,
            MouseButton::Left,
            event.position,
            event.modifiers.shift,
            event.modifiers.alt,
            event.modifiers.control,
        ) {
            return;
        }
        if let Some(state) = self.terminal_tab_states.get_mut(tab_id) {
            state.selecting = false;
            if state.selection.is_some_and(ChatTerminalSelection::is_empty) {
                state.selection = None;
            }
        }
        if self.app_settings.terminal_copy_on_select {
            self.copy_chat_terminal_selection(tab_id, cx);
        }
        cx.notify();
    }

    fn on_chat_terminal_right_mouse_up(
        &mut self,
        tab_id: &str,
        event: &MouseUpEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        let _ = self.send_chat_terminal_mouse_release_report(
            tab_id,
            MouseButton::Right,
            event.position,
            event.modifiers.shift,
            event.modifiers.alt,
            event.modifiers.control,
        );
    }

    fn on_chat_terminal_middle_mouse_up(
        &mut self,
        tab_id: &str,
        event: &MouseUpEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        let _ = self.send_chat_terminal_mouse_release_report(
            tab_id,
            MouseButton::Middle,
            event.position,
            event.modifiers.shift,
            event.modifiers.alt,
            event.modifiers.control,
        );
    }

    pub(crate) fn render_terminal_tab_content(
        &mut self,
        tab_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_terminal_session(&tab_id, cx);
        if let Some(bytes) = self.pending_terminal_inputs.remove(&tab_id) {
            self.terminal_write_user_input(&tab_id, &bytes);
        }
        self.resize_chat_terminal(&tab_id);

        let ups = &self.styles.utility_panel;
        let (body_font_size, cell_width, cell_height) = self.chat_terminal_metrics();
        let terminal_padding = self.scaled(ups.utility_panel_terminal_padding_px);
        let monospace = SharedString::from(self.styles.editor.editor_monospace_font_family);
        let default_bg = self.styles.editor.editor_bg;
        let palette = terminal_palette(default_bg);
        let default_fg = palette.foreground;
        let cursor_color = self.theme.cursor;

        let Some(focus_handle) = self
            .terminal_tab_state(&tab_id)
            .map(|state| state.focus_handle.clone())
        else {
            return div().into_any_element();
        };
        let is_focused = focus_handle.is_focused(window);
        let snapshot = self
            .terminal_session(&tab_id)
            .map(|session| session.snapshot.clone())
            .unwrap_or_default();
        let cursor_row = Self::chat_terminal_cursor_display_row(&snapshot);
        let cursor_col = snapshot.cursor.point.column.0;
        let cursor_visible = matches!(
            self.terminal_session(&tab_id)
                .map(|session| &session.status),
            Some(ChatTerminalStatus::Running)
        ) && snapshot.cursor.shape != AlacCursorShape::Hidden
            && cursor_row.is_some();
        let cursor_shape = snapshot.cursor.shape;
        let state = self.terminal_tab_state(&tab_id);
        let selection = state
            .and_then(|state| state.selection)
            .filter(|selection| !selection.is_empty())
            .map(ChatTerminalSelection::normalized);
        let selecting = state.is_some_and(|state| state.selecting);
        let marked_text = state.and_then(|state| state.marked_text.clone());
        // The IME preedit overlay replaces the cursor while composing.
        let cursor_visible = cursor_visible && marked_text.as_deref().is_none_or(str::is_empty);
        let input_bounds = state.and_then(|state| state.input_bounds);
        let search_matches = state
            .map(|state| state.search_matches.clone())
            .unwrap_or_default();
        let current_search_match = if search_matches.is_empty() {
            None
        } else {
            self.search_current_index
        };
        let missing_agent_cli_install_target = self.missing_agent_cli_install_target(&tab_id);
        let unavailable_message = self
            .terminal_session(&tab_id)
            .and_then(|session| match &session.status {
                ChatTerminalStatus::Unavailable(message) => Some(message.clone()),
                _ => None,
            })
            .filter(|_| missing_agent_cli_install_target.is_none());
        let terminal_scrollbar = {
            let viewport_height = input_bounds
                .map(|bounds| f32::from(bounds.size.height))
                .unwrap_or_else(|| snapshot.rows.max(1) as f32 * cell_height);
            let max_scroll = snapshot.history_size as f32 * cell_height;
            let scroll_top = snapshot
                .history_size
                .saturating_sub(snapshot.display_offset) as f32
                * cell_height;
            self.render_scrollbar_for_metrics(
                PanelScrollbarKind::ChatTerminal,
                viewport_height,
                max_scroll,
                scroll_top,
                viewport_height + terminal_padding * 2.0,
                0.0,
                self.scaled(self.styles.editor.editor_scrollbar_right_margin_px),
                cx,
            )
        };

        /// Registers an `on_action` listener that forwards a fixed escape
        /// sequence to this tab's PTY.
        macro_rules! bytes_action {
            ($el:expr, $action:ty, $bytes:expr) => {
                $el.on_action(cx.listener({
                    let tab_id = tab_id.clone();
                    move |this: &mut Self,
                          _: &$action,
                          window: &mut Window,
                          cx: &mut Context<Self>| {
                        this.send_chat_terminal_bytes(&tab_id, $bytes, window, cx);
                    }
                }))
            };
        }

        let root = div()
            .id(SharedString::from(format!("terminal-tab-{tab_id}")))
            .key_context("ChatTerminal")
            .track_focus(&focus_handle)
            .relative()
            .flex()
            .flex_col()
            .on_key_down(cx.listener({
                let tab_id = tab_id.clone();
                move |this, event: &KeyDownEvent, window, cx| {
                    this.on_chat_terminal_key_down(&tab_id, event, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleTerminalPanel, window, cx| {
                cx.stop_propagation();
                this.toggle_terminal_panel(cx);
                window.focus(&this.focus_handle(cx));
            }))
            .on_action(cx.listener({
                let tab_id = tab_id.clone();
                move |this, _: &NewChatTab, _, cx| {
                    cx.stop_propagation();
                    let agent_cli = this
                        .all_tabs()
                        .find(|tab| tab.id == tab_id)
                        .and_then(|tab| tab.kind.terminal_agent())
                        .unwrap_or(ChatAgentCli::Terminal);
                    this.create_terminal_tab(agent_cli, cx);
                }
            }))
            .on_action(cx.listener({
                let tab_id = tab_id.clone();
                move |this, _: &CloseActiveChatTab, _, cx| {
                    cx.stop_propagation();
                    this.request_close_tab(&tab_id, cx);
                }
            }))
            .on_action(cx.listener({
                let tab_id = tab_id.clone();
                move |this, _: &ClearTerminal, _, cx| {
                    cx.stop_propagation();
                    this.clear_terminal(&tab_id, cx);
                }
            }))
            .on_scroll_wheel(cx.listener({
                let tab_id = tab_id.clone();
                move |this, event: &ScrollWheelEvent, window, cx| {
                    this.on_chat_terminal_scroll(&tab_id, event, window, cx);
                }
            }))
            .on_action(cx.listener({
                let tab_id = tab_id.clone();
                move |this, _: &Undo, window, cx| {
                    this.swallow_chat_terminal_editor_action(&tab_id, window, cx);
                }
            }))
            .on_action(cx.listener({
                let tab_id = tab_id.clone();
                move |this, _: &Redo, window, cx| {
                    this.swallow_chat_terminal_editor_action(&tab_id, window, cx);
                }
            }))
            .on_action(cx.listener({
                let tab_id = tab_id.clone();
                move |this, _: &SelectAll, window, cx| {
                    this.swallow_chat_terminal_editor_action(&tab_id, window, cx);
                    let _ = this.select_all_chat_terminal(&tab_id, cx);
                }
            }))
            .on_action(cx.listener({
                let tab_id = tab_id.clone();
                move |this, _: &Copy, window, cx| {
                    this.focus_terminal_tab(&tab_id, window);
                    cx.stop_propagation();
                    let _ = this.copy_chat_terminal_selection(&tab_id, cx);
                }
            }))
            .on_action(cx.listener({
                let tab_id = tab_id.clone();
                move |this, _: &Cut, window, cx| {
                    this.focus_terminal_tab(&tab_id, window);
                    cx.stop_propagation();
                    let _ = this.copy_chat_terminal_selection(&tab_id, cx);
                }
            }))
            .on_action(cx.listener({
                let tab_id = tab_id.clone();
                move |this, action: &Paste, window, cx| {
                    this.on_chat_terminal_paste_action(&tab_id, action, window, cx);
                }
            }));
        let root = bytes_action!(root, Backspace, b"\x7f");
        let root = bytes_action!(root, Delete, b"\x1b[3~");
        let root = bytes_action!(root, Left, b"\x1b[D");
        let root = bytes_action!(root, Right, b"\x1b[C");
        let root = bytes_action!(root, Up, b"\x1b[A");
        let root = bytes_action!(root, Down, b"\x1b[B");
        let root = bytes_action!(root, SelectLeft, b"\x1b[1;2D");
        let root = bytes_action!(root, SelectRight, b"\x1b[1;2C");
        let root = bytes_action!(root, SelectUp, b"\x1b[1;2A");
        let root = bytes_action!(root, SelectDown, b"\x1b[1;2B");
        let root = bytes_action!(root, Home, b"\x1b[H");
        let root = bytes_action!(root, End, b"\x1b[F");
        let root = bytes_action!(root, SelectHome, b"\x1b[1;2H");
        let root = bytes_action!(root, SelectEnd, b"\x1b[1;2F");
        let root = bytes_action!(root, Enter, b"\r");
        let root = bytes_action!(root, Tab, b"\t");
        let root = bytes_action!(root, BackTab, b"\x1b[Z");
        let root = bytes_action!(root, MoveWordLeft, b"\x1bb");
        let root = bytes_action!(root, MoveWordRight, b"\x1bf");
        let root = bytes_action!(root, DeleteWordLeft, b"\x17");
        let root = bytes_action!(root, Escape, b"\x1b");

        root.on_mouse_down(
            MouseButton::Left,
            cx.listener({
                let tab_id = tab_id.clone();
                move |this, event: &MouseDownEvent, window, cx| {
                    this.on_chat_terminal_mouse_down(&tab_id, event, window, cx);
                }
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener({
                let tab_id = tab_id.clone();
                move |this, event: &MouseDownEvent, window, cx| {
                    this.on_chat_terminal_context_menu(&tab_id, event, window, cx);
                }
            }),
        )
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener({
                let tab_id = tab_id.clone();
                move |this, event: &MouseDownEvent, window, cx| {
                    this.on_chat_terminal_middle_mouse_down(&tab_id, event, window, cx);
                }
            }),
        )
        .on_mouse_move(cx.listener({
            let tab_id = tab_id.clone();
            move |this, event: &MouseMoveEvent, window, cx| {
                this.on_chat_terminal_mouse_move(&tab_id, event, window, cx);
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener({
                let tab_id = tab_id.clone();
                move |this, event: &MouseUpEvent, window, cx| {
                    this.on_chat_terminal_mouse_up(&tab_id, event, window, cx);
                }
            }),
        )
        .on_mouse_up(
            MouseButton::Right,
            cx.listener({
                let tab_id = tab_id.clone();
                move |this, event: &MouseUpEvent, window, cx| {
                    this.on_chat_terminal_right_mouse_up(&tab_id, event, window, cx);
                }
            }),
        )
        .on_mouse_up(
            MouseButton::Middle,
            cx.listener({
                let tab_id = tab_id.clone();
                move |this, event: &MouseUpEvent, window, cx| {
                    this.on_chat_terminal_middle_mouse_up(&tab_id, event, window, cx);
                }
            }),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener({
                let tab_id = tab_id.clone();
                move |this, event: &MouseUpEvent, window, cx| {
                    this.on_chat_terminal_mouse_up(&tab_id, event, window, cx);
                }
            }),
        )
        .on_mouse_up_out(
            MouseButton::Right,
            cx.listener({
                let tab_id = tab_id.clone();
                move |this, event: &MouseUpEvent, window, cx| {
                    this.on_chat_terminal_right_mouse_up(&tab_id, event, window, cx);
                }
            }),
        )
        .on_mouse_up_out(
            MouseButton::Middle,
            cx.listener({
                let tab_id = tab_id.clone();
                move |this, event: &MouseUpEvent, window, cx| {
                    this.on_chat_terminal_middle_mouse_up(&tab_id, event, window, cx);
                }
            }),
        )
        .cursor(CursorStyle::IBeam)
        .h_full()
        .w_full()
        .bg(default_bg)
        .child(
            div()
                .flex_1()
                .w_full()
                .h_full()
                .relative()
                .child(
                    div()
                        .w_full()
                        .h_full()
                        .px(px(terminal_padding))
                        .pt(px(terminal_padding))
                        .pb(px(terminal_padding))
                        .relative()
                        .bg(default_bg)
                        .overflow_hidden()
                        .on_scroll_wheel(cx.listener({
                            let tab_id = tab_id.clone();
                            move |this, event: &ScrollWheelEvent, window, cx| {
                                this.on_chat_terminal_scroll(&tab_id, event, window, cx);
                            }
                        }))
                        .child(ChatTerminalSurfaceElement {
                            editor: cx.entity().clone(),
                            snapshot: snapshot.clone(),
                            selection,
                            selecting,
                            cursor_row,
                            cursor_col,
                            cursor_visible,
                            cursor_shape,
                            focused: is_focused,
                            search_matches,
                            current_search_match,
                            default_fg,
                            default_bg,
                            cursor_color,
                            palette,
                        }),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(terminal_padding))
                        .top(px(terminal_padding))
                        .right(px(terminal_padding))
                        .bottom(px(terminal_padding))
                        .on_scroll_wheel(cx.listener({
                            let tab_id = tab_id.clone();
                            move |this, event: &ScrollWheelEvent, window, cx| {
                                this.on_chat_terminal_scroll(&tab_id, event, window, cx);
                            }
                        }))
                        .child(ChatTerminalInputElement {
                            editor: cx.entity().clone(),
                            tab_id: tab_id.clone(),
                        })
                        .when_some(unavailable_message, |this, message| {
                            // A failed spawn must never leave the panel blank.
                            this.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .px(px(self.scaled(24.0)))
                                    .text_color(ups.utility_panel_button_text_color)
                                    .text_size(px(body_font_size))
                                    .child(message),
                            )
                        })
                        .when_some(
                            missing_agent_cli_install_target,
                            |this, (agent_cli, _url)| {
                                let button_height = self.scaled(ups.utility_panel_button_height_px);
                                let button_padding_x = self
                                    .scaled(ups.utility_panel_button_padding_x_px)
                                    + self.scaled(6.0);
                                let button_border_width =
                                    self.scaled(ups.utility_panel_button_border_width_px);
                                let label = crate::i18n::install_agent_label(
                                    self.ui_language,
                                    agent_cli.label(),
                                );
                                this.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div()
                                                .h(px(button_height))
                                                .px(px(button_padding_x))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded_full()
                                                .border(px(button_border_width))
                                                .border_color(ups.utility_panel_button_border_color)
                                                .bg(default_bg)
                                                .text_color(ups.utility_panel_button_text_color)
                                                .text_size(px(body_font_size))
                                                .line_height(px(button_height))
                                                .cursor(CursorStyle::Arrow)
                                                .hover(|style| {
                                                    style.bg(ups.utility_panel_button_hover_bg)
                                                })
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(move |this, _, _, cx| {
                                                        this.open_agent_cli_install_page(
                                                            agent_cli, cx,
                                                        );
                                                    }),
                                                )
                                                .child(label),
                                        ),
                                )
                            },
                        )
                        .when_some(
                            cursor_row.zip(marked_text.filter(|text| !text.is_empty())),
                            |this, (cursor_row, text)| {
                                this.child(
                                    div()
                                        .absolute()
                                        .left(px(cursor_col as f32 * cell_width))
                                        .top(px(cursor_row as f32 * cell_height))
                                        .px(px(1.0))
                                        .h(px(cell_height))
                                        .text_color(default_fg)
                                        .font_family(monospace.clone())
                                        .text_size(px(body_font_size))
                                        .line_height(px(cell_height))
                                        .border_b(px(1.0))
                                        .border_color(cursor_color)
                                        .bg(default_bg)
                                        .child(text),
                                )
                            },
                        ),
                ),
        )
        .when_some(terminal_scrollbar, |this, scrollbar| this.child(scrollbar))
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn terminal_parser_answers_primary_device_attributes_query() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let listener = ChatTerminalListener {
            writer: Arc::new(Mutex::new(Box::new(SharedWriter(output.clone())))),
        };
        let mut term = Term::new(
            terminal_config(TerminalCursorShape::default()),
            &ChatTerminalSize { rows: 24, cols: 80 },
            listener,
        );
        let mut processor = Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::new();

        processor.advance(&mut term, b"\x1b[c");

        assert_eq!(&*output.lock().unwrap(), b"\x1b[?6c");
    }

    #[test]
    fn terminal_title_uses_working_directory_and_current_command() {
        let cwd = std::path::Path::new("/Users/moeka/project");
        assert_eq!(
            terminal_process_title(cwd, "/bin/zsh", &["/bin/zsh".to_string()]),
            "project - zsh"
        );
        assert_eq!(
            terminal_process_title(
                cwd,
                "npm",
                &["npm".to_string(), "run".to_string(), "dev".to_string()],
            ),
            "project - npm run dev"
        );
        assert_eq!(
            terminal_process_title(std::path::Path::new("/"), "zsh", &[]),
            "/ - zsh"
        );
    }

    #[test]
    fn terminal_status_summary_only_reports_scrollback_away_from_bottom() {
        let mut summary = TerminalStatusSummary {
            agent: ChatAgentCli::Terminal,
            title: "project - zsh".to_string(),
            history_size: 120,
            display_offset: 0,
        };
        assert_eq!(summary.scrollback(), None);

        summary.display_offset = 24;
        assert_eq!(summary.scrollback(), Some((24, 120)));
    }

    #[test]
    fn terminal_palette_matches_zed_and_ignores_editor_theme_colors() {
        let dark_palette = terminal_palette(crate::hex_hsla!("#111111"));
        assert_eq!(dark_palette.foreground, crate::hex_hsla!("#ABB2BF"));
        assert_eq!(dark_palette.yellow, crate::hex_hsla!("#E5C07B"));
        assert_eq!(dark_palette.cyan, crate::hex_hsla!("#56B6C2"));
        assert_eq!(dark_palette, terminal_palette(crate::hex_hsla!("#303030")));

        let light_palette = terminal_palette(crate::hex_hsla!("#FAFAFA"));
        assert_eq!(light_palette.foreground, crate::hex_hsla!("#2A2C33"));
        assert_eq!(light_palette.blue, crate::hex_hsla!("#2F5AF3"));
        assert_eq!(light_palette, terminal_palette(crate::hex_hsla!("#FFFFFF")));

        let themed_background = crate::hex_hsla!("#123456");
        assert_eq!(
            terminal_named_color(
                NamedColor::Background,
                &dark_palette,
                dark_palette.foreground,
                themed_background,
            ),
            themed_background
        );
    }

    #[test]
    fn terminal_cursor_shapes_match_zed_alacritty_shapes() {
        let cases = [
            (TerminalCursorShape::Block, AlacCursorShape::Block),
            (TerminalCursorShape::Underline, AlacCursorShape::Underline),
            (TerminalCursorShape::Bar, AlacCursorShape::Beam),
            (TerminalCursorShape::Hollow, AlacCursorShape::HollowBlock),
        ];

        for (setting, expected) in cases {
            let style = alacritty_cursor_style(setting);
            assert_eq!(style.shape, expected);
            assert!(!style.blinking);
        }
    }

    fn indexed_cell(line: i32, col: usize, ch: char) -> ChatIndexedCell {
        let mut cell = Cell::default();
        cell.c = ch;
        ChatIndexedCell {
            point: AlacPoint::new(Line(line), Column(col)),
            cell,
        }
    }

    fn snapshot_with_cells(
        rows: u16,
        cols: u16,
        cells: Vec<ChatIndexedCell>,
        cursor_line: i32,
    ) -> ChatTerminalSnapshot {
        ChatTerminalSnapshot {
            rows,
            cols,
            cells,
            cursor: RenderableCursor {
                shape: AlacCursorShape::Block,
                point: AlacPoint::new(Line(cursor_line), Column(0)),
            },
            display_offset: 0,
            history_size: 0,
        }
    }

    #[test]
    fn chat_terminal_rows_preserve_negative_scrollback_lines() {
        let snapshot = snapshot_with_cells(
            3,
            4,
            vec![
                indexed_cell(-2, 0, 'a'),
                indexed_cell(-2, 1, 'b'),
                indexed_cell(-1, 0, 'c'),
                indexed_cell(0, 0, 'd'),
            ],
            0,
        );

        let rows = EditorView::chat_terminal_rows(&snapshot);

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].source_line, -2);
        assert_eq!(rows[1].source_line, -1);
        assert_eq!(rows[2].source_line, 0);
        assert_eq!(rows[0].cells[0].c, 'a');
        assert_eq!(rows[1].cells[0].c, 'c');
        assert_eq!(rows[2].cells[0].c, 'd');
    }

    #[test]
    fn chat_terminal_cursor_display_row_uses_screen_row_order() {
        let snapshot = snapshot_with_cells(
            3,
            4,
            vec![
                indexed_cell(-2, 0, 'a'),
                indexed_cell(-1, 0, 'b'),
                indexed_cell(0, 0, 'c'),
            ],
            -1,
        );

        assert_eq!(
            EditorView::chat_terminal_cursor_display_row(&snapshot),
            Some(1)
        );
    }

    #[test]
    fn chat_terminal_word_bounds_skip_cjk_spacer_cells() {
        let mut first = Cell::default();
        first.c = '你';
        first.flags.insert(Flags::WIDE_CHAR);

        let mut first_spacer = Cell::default();
        first_spacer.flags.insert(Flags::WIDE_CHAR_SPACER);

        let mut second = Cell::default();
        second.c = '好';
        second.flags.insert(Flags::WIDE_CHAR);

        let mut second_spacer = Cell::default();
        second_spacer.flags.insert(Flags::WIDE_CHAR_SPACER);

        let row = vec![first, first_spacer, second, second_spacer];

        assert_eq!(chat_terminal_word_bounds(&row, 1), Some((0, 2)));
    }

    #[test]
    fn chat_reference_position_counts_unicode_columns() {
        let buffer = crate::buffer::Buffer::from_text("标题\n处理");
        let inside_li = buffer.text().find('理').unwrap() + 1;

        assert_eq!(
            EditorView::chat_reference_position(&buffer, inside_li),
            (2, 2)
        );
    }

    #[test]
    fn terminal_create_menu_includes_all_supported_clis() {
        assert_eq!(
            ChatAgentCli::all(),
            [
                ChatAgentCli::Terminal,
                ChatAgentCli::OpenAICodex,
                ChatAgentCli::GithubCopilot,
                ChatAgentCli::ClaudeCode,
                ChatAgentCli::GeminiCli,
            ]
        );
    }

    #[test]
    fn dedicated_terminal_command_warns_before_close() {
        let mut session = ChatTerminalSession::unavailable("");
        session.status = ChatTerminalStatus::Running;
        session.warn_on_close = true;
        assert!(session.has_running_command());

        session.status = ChatTerminalStatus::Exited;
        assert!(!session.has_running_command());
    }

    #[test]
    fn terminal_environment_declares_supported_terminal_type() {
        let mut command = CommandBuilder::new("/bin/zsh");
        command.env_clear();
        configure_chat_terminal_environment(&mut command);

        assert_eq!(
            command.get_env("TERM"),
            Some(std::ffi::OsStr::new("xterm-256color"))
        );
        assert_eq!(
            command.get_env("COLORTERM"),
            Some(std::ffi::OsStr::new("truecolor"))
        );
        assert_eq!(
            command.get_env("TERM_PROGRAM"),
            Some(std::ffi::OsStr::new("moeka"))
        );
        assert_eq!(
            command.get_env("TERM_PROGRAM_VERSION"),
            Some(std::ffi::OsStr::new(env!("CARGO_PKG_VERSION")))
        );
        assert!(command.get_env("SHELL").is_some());
    }

    fn keystroke(spec: &str) -> gpui::Keystroke {
        gpui::Keystroke::parse(spec).expect("valid keystroke")
    }

    #[test]
    fn arrow_keys_respect_application_cursor_mode() {
        assert_eq!(
            keystroke_to_terminal_bytes(&keystroke("up"), TermMode::NONE),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            keystroke_to_terminal_bytes(&keystroke("up"), TermMode::APP_CURSOR),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            keystroke_to_terminal_bytes(&keystroke("end"), TermMode::APP_CURSOR),
            Some(b"\x1bOF".to_vec())
        );
        // Modified arrows use CSI 1;{code} regardless of DECCKM.
        assert_eq!(
            keystroke_to_terminal_bytes(&keystroke("shift-up"), TermMode::APP_CURSOR),
            Some(b"\x1b[1;2A".to_vec())
        );
    }

    #[test]
    fn function_keys_map_to_xterm_sequences() {
        assert_eq!(
            keystroke_to_terminal_bytes(&keystroke("f1"), TermMode::NONE),
            Some(b"\x1bOP".to_vec())
        );
        assert_eq!(
            keystroke_to_terminal_bytes(&keystroke("f5"), TermMode::NONE),
            Some(b"\x1b[15~".to_vec())
        );
        assert_eq!(
            keystroke_to_terminal_bytes(&keystroke("f12"), TermMode::NONE),
            Some(b"\x1b[24~".to_vec())
        );
        assert_eq!(
            keystroke_to_terminal_bytes(&keystroke("shift-f3"), TermMode::NONE),
            Some(b"\x1b[1;2R".to_vec())
        );
    }

    #[test]
    fn control_characters_cover_specials() {
        assert_eq!(
            keystroke_to_terminal_bytes(&keystroke("ctrl-c"), TermMode::NONE),
            Some(vec![0x03])
        );
        assert_eq!(
            keystroke_to_terminal_bytes(&keystroke("ctrl-space"), TermMode::NONE),
            Some(vec![0x00])
        );
        assert_eq!(
            keystroke_to_terminal_bytes(&keystroke("ctrl-["), TermMode::NONE),
            Some(vec![0x1b])
        );
        assert_eq!(
            keystroke_to_terminal_bytes(&keystroke("ctrl--"), TermMode::NONE),
            Some(vec![0x1f])
        );
    }

    #[test]
    fn alt_sends_escape_prefixed_base_key() {
        // macOS composes option-a into "å"; the terminal should still see
        // ESC + the plain base key (alt-as-meta).
        let mut stroke = keystroke("alt-a");
        stroke.key_char = Some("å".to_string());
        assert_eq!(
            keystroke_to_terminal_bytes(&stroke, TermMode::NONE),
            Some(vec![0x1b, b'a'])
        );
    }
}
