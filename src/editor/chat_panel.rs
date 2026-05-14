use std::io::{Read, Write};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use alacritty_terminal::{
    event::{Event as AlacTermEvent, EventListener},
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point as AlacPoint},
    sync::FairMutex,
    term::{
        Config, RenderableCursor, Term,
        cell::{Cell, Flags},
    },
    vte::ansi::{Color as AlacColor, CursorShape as AlacCursorShape, NamedColor, Processor, Rgb},
};
use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, Font,
    FontFallbacks, FontStyle, FontWeight, GlobalElementId, Hsla, InputHandler, KeyDownEvent,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollWheelEvent, SharedString, Style, TextRun as GpuiTextRun, TouchPhase, UTF16Selection,
    Window, div, fill, point, prelude::*, px, relative, size,
};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::*;

const CHAT_PANEL_DEFAULT_ROWS: u16 = 28;
const CHAT_PANEL_DEFAULT_COLS: u16 = 96;
const CHAT_PANEL_POLL_MS: u64 = 33;

pub(crate) struct ChatTerminalSession {
    pub(crate) status: ChatTerminalStatus,
    pub(crate) snapshot: ChatTerminalSnapshot,
    runtime: Option<ChatTerminalRuntime>,
    receiver: Option<mpsc::Receiver<ChatTerminalEvent>>,
    pub(crate) poll_task: Option<Task<()>>,
    rows: u16,
    cols: u16,
}

struct ChatTerminalRuntime {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    term: Arc<FairMutex<Term<ChatTerminalListener>>>,
    reader_thread: Option<JoinHandle<()>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ChatTerminalStatus {
    Launching,
    Running,
    Exited,
    Unavailable(String),
}

#[derive(Clone)]
pub(crate) struct ChatTerminalSnapshot {
    rows: u16,
    cols: u16,
    cells: Vec<ChatIndexedCell>,
    cursor: RenderableCursor,
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
struct ChatTerminalListener;

#[derive(Clone, Copy)]
struct ChatTerminalSize {
    rows: usize,
    cols: usize,
}

#[derive(Clone, Copy)]
struct ChatTerminalPalette {
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
}

struct ChatTerminalSurfaceElement {
    editor: gpui::Entity<EditorView>,
    snapshot: ChatTerminalSnapshot,
    selection: Option<ChatTerminalSelection>,
    cursor_row: Option<usize>,
    cursor_col: usize,
    cursor_visible: bool,
    cursor_shape: AlacCursorShape,
    default_fg: Hsla,
    default_bg: Hsla,
    cursor_color: Hsla,
    palette: ChatTerminalPalette,
}

struct ChatTerminalInputHandler {
    editor: gpui::Entity<EditorView>,
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
        let _ = self.editor.update(cx, |editor, _| {
            editor.chat_terminal_input_bounds = Some(bounds);
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
        let focus_handle = self.editor.read(cx).chat_panel_focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ChatTerminalInputHandler {
                editor: self.editor.clone(),
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
        let rows = EditorView::chat_terminal_rows(&self.snapshot);
        let base_font = editor.chat_terminal_font(window);
        let origin = bounds.origin;

        let mut batches: Vec<ChatTerminalPaintBatch> = Vec::new();

        for (row_index, row) in rows.iter().enumerate() {
            let mut previous_cell_had_extras = false;

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

                if is_chat_terminal_spacer_cell(cell) {
                    continue;
                }

                let is_selected = self
                    .selection
                    .is_some_and(|selection| selection.contains(row_index, col_index));
                if is_selected {
                    bg = editor.theme.selection;
                }

                let is_cursor = self.cursor_visible
                    && self.cursor_row == Some(row_index)
                    && col_index == self.cursor_col;
                if is_cursor && self.cursor_shape == AlacCursorShape::Block {
                    std::mem::swap(&mut fg, &mut bg);
                    bg = self.cursor_color;
                }

                if bg != self.default_bg {
                    let width = if cell.flags.contains(Flags::WIDE_CHAR) {
                        cell_width * 2.0
                    } else {
                        cell_width
                    };
                    window.paint_quad(fill(
                        Bounds::new(
                            point(
                                origin.x + px(col_index as f32 * cell_width),
                                origin.y + px(row_index as f32 * cell_height),
                            ),
                            size(px(width), px(cell_height)),
                        ),
                        bg,
                    ));
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
                    if is_cursor && self.cursor_shape == AlacCursorShape::Beam {
                        window.paint_quad(fill(
                            Bounds::new(
                                point(
                                    origin.x + px(col_index as f32 * cell_width),
                                    origin.y + px(row_index as f32 * cell_height),
                                ),
                                size(px(2.0), px(cell_height)),
                            ),
                            self.cursor_color,
                        ));
                    } else if is_cursor && self.cursor_shape == AlacCursorShape::Underline {
                        window.paint_quad(fill(
                            Bounds::new(
                                point(
                                    origin.x + px(col_index as f32 * cell_width),
                                    origin.y + px((row_index as f32 + 1.0) * cell_height - 2.0),
                                ),
                                size(px(cell_width), px(2.0)),
                            ),
                            self.cursor_color,
                        ));
                    }
                    continue;
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
                let run = GpuiTextRun {
                    len: text.len(),
                    font,
                    color: fg,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
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

                if is_cursor && self.cursor_shape == AlacCursorShape::Beam {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(
                                origin.x + px(col_index as f32 * cell_width),
                                origin.y + px(row_index as f32 * cell_height),
                            ),
                            size(px(2.0), px(cell_height)),
                        ),
                        self.cursor_color,
                    ));
                } else if is_cursor && self.cursor_shape == AlacCursorShape::Underline {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(
                                origin.x + px(col_index as f32 * cell_width),
                                origin.y + px((row_index as f32 + 1.0) * cell_height - 2.0),
                            ),
                            size(px(cell_width), px(2.0)),
                        ),
                        self.cursor_color,
                    ));
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
            .chat_terminal_marked_text
            .as_ref()
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
        self.editor.update(cx, |editor, editor_cx| {
            editor.chat_terminal_marked_text = None;
            if let Some(session) = editor.chat_terminal.as_mut() {
                let _ = session.write_bytes(text.as_bytes());
            }
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
        self.editor.update(cx, |editor, editor_cx| {
            editor.chat_terminal_marked_text = (!new_text.is_empty()).then(|| new_text.to_string());
            editor_cx.notify();
        });
        window.invalidate_character_coordinates();
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut App) {
        self.editor.update(cx, |editor, editor_cx| {
            editor.chat_terminal_marked_text = None;
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
            .chat_terminal
            .as_ref()
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
    fn send_event(&self, _event: AlacTermEvent) {}
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
        }
    }
}

impl ChatTerminalSession {
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: ChatTerminalStatus::Unavailable(message.into()),
            snapshot: ChatTerminalSnapshot::default(),
            runtime: None,
            receiver: None,
            poll_task: None,
            rows: CHAT_PANEL_DEFAULT_ROWS,
            cols: CHAT_PANEL_DEFAULT_COLS,
        }
    }

    fn launch(cwd: PathBuf) -> anyhow::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: CHAT_PANEL_DEFAULT_ROWS,
            cols: CHAT_PANEL_DEFAULT_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut command = CommandBuilder::new("codex");
        command.cwd(cwd.clone());

        let child = pair.slave.spawn_command(command)?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));
        let mut reader = pair.master.try_clone_reader()?;

        let term = Arc::new(FairMutex::new(Term::new(
            Config::default(),
            &ChatTerminalSize {
                rows: CHAT_PANEL_DEFAULT_ROWS as usize,
                cols: CHAT_PANEL_DEFAULT_COLS as usize,
            },
            ChatTerminalListener,
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
            runtime: Some(ChatTerminalRuntime {
                master: pair.master,
                child,
                writer,
                term,
                reader_thread: Some(reader_thread),
            }),
            receiver: Some(receiver),
            poll_task: None,
            rows: CHAT_PANEL_DEFAULT_ROWS,
            cols: CHAT_PANEL_DEFAULT_COLS,
        };
        session.refresh_snapshot();
        Ok(session)
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
        };
    }

    fn stop(&mut self) {
        if let Some(mut runtime) = self.runtime.take() {
            let _ = runtime.child.kill();
            let _ = runtime.child.wait();
            drop(runtime.writer);
            if let Some(handle) = runtime.reader_thread.take() {
                let _ = handle.join();
            }
        }
        self.receiver = None;
        self.poll_task = None;
    }
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

fn terminal_emphasis_color(color: Hsla, is_dark: bool) -> Hsla {
    Hsla {
        h: color.h,
        s: (color.s * if is_dark { 1.08 } else { 1.12 }).clamp(0.0, 1.0),
        l: (color.l + if is_dark { 0.1 } else { -0.08 }).clamp(0.0, 1.0),
        a: color.a,
    }
}

fn terminal_theme_palette(theme: &crate::theme::Theme, panel_bg: Hsla) -> ChatTerminalPalette {
    let is_dark = panel_bg.l < 0.5;
    let neutral_black = if is_dark {
        theme.bg_primary
    } else {
        crate::theme::with_alpha(theme.text_primary, 0.92)
    };
    let neutral_white = if is_dark {
        theme.text_primary
    } else {
        crate::theme::with_alpha(theme.text_primary, 0.78)
    };

    let base = ChatTerminalPalette {
        black: neutral_black,
        red: theme.text_danger,
        green: theme.inline_code,
        yellow: theme.bold,
        blue: theme.link,
        magenta: theme.italic,
        cyan: theme.checkbox_checked,
        white: neutral_white,
        bright_black: theme.text_secondary,
        bright_red: theme.text_danger,
        bright_green: theme.inline_code,
        bright_yellow: theme.bold,
        bright_blue: theme.link,
        bright_magenta: theme.italic,
        bright_cyan: theme.checkbox_checked,
        bright_white: theme.text_bright,
    };

    ChatTerminalPalette {
        bright_black: terminal_emphasis_color(base.bright_black, is_dark),
        bright_red: terminal_emphasis_color(base.bright_red, is_dark),
        bright_green: terminal_emphasis_color(base.bright_green, is_dark),
        bright_yellow: terminal_emphasis_color(base.bright_yellow, is_dark),
        bright_blue: terminal_emphasis_color(base.bright_blue, is_dark),
        bright_magenta: terminal_emphasis_color(base.bright_magenta, is_dark),
        bright_cyan: terminal_emphasis_color(base.bright_cyan, is_dark),
        ..base
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
        _ => default_fg,
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

fn should_route_chat_terminal_text_input(keystroke: &gpui::Keystroke) -> bool {
    keystroke.key_char.is_some()
        && !keystroke.modifiers.control
        && !keystroke.modifiers.alt
        && !keystroke.modifiers.platform
        && !keystroke.modifiers.function
}

fn keystroke_to_terminal_bytes(keystroke: &gpui::Keystroke) -> Option<Vec<u8>> {
    let modifiers = keystroke.modifiers;
    let key = keystroke.key.as_str();

    if modifiers.platform || modifiers.function {
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
            "pageup" => Some(format!("\x1b[5;{}~", modifier_code).into_bytes()),
            "pagedown" => Some(format!("\x1b[6;{}~", modifier_code).into_bytes()),
            "delete" => Some(format!("\x1b[3;{}~", modifier_code).into_bytes()),
            _ => None,
        };

        if let Some(bytes) = modified {
            return Some(bytes);
        }
    }

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
        "left" => b"\x1b[D".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        _ => {
            if modifiers.control && key.len() == 1 {
                let ascii = key.as_bytes()[0].to_ascii_lowercase();
                if ascii.is_ascii_lowercase() {
                    vec![ascii - b'a' + 1]
                } else {
                    return None;
                }
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
        let configured_font_family = |configured: &'static str| {
            if configured == ".SystemUIFont" {
                text_style.font().family.clone()
            } else {
                configured.into()
            }
        };
        let body_cjk_fallbacks = |configured: &'static str| {
            (configured != editor_styles.editor_body_cjk_font_family).then(|| {
                FontFallbacks::from_fonts(vec![
                    editor_styles.editor_body_cjk_font_family.to_string(),
                ])
            })
        };

        let mut monospace_font = text_style.font();
        monospace_font.family = configured_font_family(editor_styles.editor_monospace_font_family);
        monospace_font.fallbacks = body_cjk_fallbacks(editor_styles.editor_monospace_font_family);
        monospace_font
    }

    pub(crate) fn refresh_chat_terminal_metrics(&mut self, window: &Window, cx: &App) {
        let body_font_size = self
            .scaled(self.styles.utility_panel.utility_panel_item_font_size_px)
            .max(11.0);
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
        let body_font_size = self
            .scaled(self.styles.utility_panel.utility_panel_item_font_size_px)
            .max(11.0);
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

    fn clear_chat_terminal_interaction_state(&mut self) {
        self.chat_terminal_marked_text = None;
        self.chat_terminal_selection = None;
        self.chat_terminal_selecting = false;
    }

    fn focus_chat_terminal_panel(&self, window: &mut Window) {
        window.focus(&self.chat_panel_focus_handle);
    }

    fn send_chat_terminal_bytes(
        &mut self,
        bytes: &[u8],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_chat_terminal_panel(window);
        cx.stop_propagation();
        self.chat_terminal_marked_text = None;
        if let Some(session) = self.chat_terminal.as_mut() {
            let _ = session.write_bytes(bytes);
        }
    }

    pub(crate) fn ensure_chat_terminal_visible(&mut self, cx: &mut Context<Self>) {
        self.pending_chat_input_focus = true;
        if self.utility_panel != Some(RightUtilityPanel::Chat) {
            self.toggle_utility_panel(RightUtilityPanel::Chat, cx);
            return;
        }

        if self.chat_terminal.is_none() {
            self.open_chat_terminal(cx);
            return;
        }

        cx.notify();
    }

    pub(crate) fn add_path_to_chat(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let Some(relative_path) = self.relative_path_for_path(path) else {
            return;
        };

        self.ensure_chat_terminal_visible(cx);
        if let Some(session) = self.chat_terminal.as_mut() {
            let _ = session.write_bytes(format!("@{} ", relative_path).as_bytes());
        }
    }

    fn chat_reference_position(&self, offset: usize) -> (usize, usize) {
        Self::chat_reference_position_in_buffer(&self.buffer, offset)
    }

    fn chat_reference_position_in_buffer(
        buffer: &crate::buffer::Buffer,
        offset: usize,
    ) -> (usize, usize) {
        let clamped = buffer.clamp_offset(offset.min(buffer.len()));
        let (line, byte_col) = buffer.offset_to_line_col(clamped);
        let line_content = buffer.line_content(line);
        let column = line_content[..byte_col.min(line_content.len())]
            .chars()
            .count()
            + 1;
        (line + 1, column)
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
        if range.is_empty() {
            self.add_path_to_chat(path, cx);
            return;
        }

        let end_anchor = range.end.saturating_sub(1);
        let (start_line, start_col) = self.chat_reference_position(range.start);
        let (end_line, end_col) = self.chat_reference_position(end_anchor);

        self.ensure_chat_terminal_visible(cx);
        if let Some(session) = self.chat_terminal.as_mut() {
            let text = format!(
                "@{} L{}:C{}-L{}:C{} ",
                relative_path, start_line, start_col, end_line, end_col
            );
            let _ = session.write_bytes(text.as_bytes());
        }
    }

    fn swallow_chat_terminal_editor_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_chat_terminal_panel(window);
        cx.stop_propagation();
    }

    pub(crate) fn chat_panel_working_directory(&self) -> Option<PathBuf> {
        self.current_file_path
            .as_ref()
            .and_then(|path| self.workspace_root_for_path(path))
            .or_else(|| {
                (!self.sidebar_root.as_os_str().is_empty()).then(|| self.sidebar_root.clone())
            })
    }

    pub(crate) fn open_chat_terminal(&mut self, cx: &mut Context<Self>) {
        self.stop_chat_terminal();
        self.pending_chat_input_focus = true;
        self.clear_chat_terminal_interaction_state();

        let Some(cwd) = self.chat_panel_working_directory() else {
            self.chat_terminal = Some(ChatTerminalSession::unavailable(
                "No workspace root available for chat terminal.",
            ));
            cx.notify();
            return;
        };

        match ChatTerminalSession::launch(cwd) {
            Ok(mut session) => {
                session.status = ChatTerminalStatus::Running;
                self.chat_terminal = Some(session);
                self.start_chat_terminal_poll(cx);
                self.resize_chat_terminal();
            }
            Err(error) => {
                self.chat_terminal = Some(ChatTerminalSession::unavailable(error.to_string()));
            }
        }

        cx.notify();
    }

    pub(crate) fn stop_chat_terminal(&mut self) {
        self.pending_chat_input_focus = false;
        self.chat_terminal_scroll_px = Pixels::ZERO;
        self.chat_terminal_input_bounds = None;
        self.clear_chat_terminal_interaction_state();
        if let Some(mut session) = self.chat_terminal.take() {
            session.stop();
        }
    }

    fn start_chat_terminal_poll(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.chat_terminal.as_mut() else {
            return;
        };

        session.poll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let done = this
                    .update(cx, |this, cx| {
                        let Some(session) = this.chat_terminal.as_mut() else {
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
                                    this.chat_terminal_selection = None;
                                    this.chat_terminal_selecting = false;
                                    session.refresh_snapshot();
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

                        if clear_interaction_state {
                            this.clear_chat_terminal_interaction_state();
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

    fn resize_chat_terminal(&mut self) {
        let (_, cell_width, cell_height) = self.chat_terminal_metrics();
        let terminal_padding =
            self.scaled(self.styles.utility_panel.utility_panel_terminal_padding_px);
        let panel_width = if self.chat_panel_placement == ChatPanelPlacement::Bottom {
            (self.last_window_width
                - self.sidebar_width()
                - self.right_panel_width()
                - terminal_padding * 2.0)
                .max(220.0)
        } else {
            (self.utility_panel_width() - terminal_padding * 2.0).max(220.0)
        };
        let panel_height = if self.chat_panel_placement == ChatPanelPlacement::Bottom {
            (self.utility_panel_width() - terminal_padding * 2.0).max(120.0)
        } else {
            (self.last_window_height - terminal_padding * 2.0).max(240.0)
        };
        let cols = (panel_width / cell_width.max(1.0)).floor() as u16;
        let rows = (panel_height / cell_height.max(1.0)).floor() as u16;
        if let Some(session) = self.chat_terminal.as_mut() {
            if session.rows != rows.max(2) || session.cols != cols.max(20) {
                session.resize(rows, cols);
            }
        }
    }

    fn on_chat_terminal_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_chat_terminal_panel(window);
        if should_route_chat_terminal_text_input(&event.keystroke) {
            return;
        }
        cx.stop_propagation();
        let Some(bytes) = keystroke_to_terminal_bytes(&event.keystroke) else {
            return;
        };
        if let Some(session) = self.chat_terminal.as_mut() {
            let _ = session.write_bytes(&bytes);
        }
    }

    fn on_chat_terminal_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_chat_terminal_panel(window);
        cx.stop_propagation();
        self.chat_terminal_selection = None;
        self.chat_terminal_selecting = false;

        let (_, _, line_height) = self.chat_terminal_metrics();
        let Some(delta) = self.chat_terminal_scroll_lines(event, line_height) else {
            return;
        };

        if let Some(session) = self.chat_terminal.as_mut() {
            session.scroll_display(Scroll::Delta(delta));
            cx.notify();
        }
    }

    fn chat_terminal_scroll_lines(
        &mut self,
        event: &ScrollWheelEvent,
        line_height: f32,
    ) -> Option<i32> {
        let line_height = px(line_height.max(1.0));
        let scroll_height = self
            .chat_terminal
            .as_ref()
            .map(|session| line_height * session.snapshot.rows.max(1) as f32)
            .unwrap_or(line_height);
        match event.touch_phase {
            TouchPhase::Started => {
                self.chat_terminal_scroll_px = Pixels::ZERO;
                None
            }
            TouchPhase::Moved => {
                let old_offset = (self.chat_terminal_scroll_px / line_height) as i32;
                self.chat_terminal_scroll_px += event.delta.pixel_delta(line_height).y;
                let new_offset = (self.chat_terminal_scroll_px / line_height) as i32;
                self.chat_terminal_scroll_px %= scroll_height;
                Some(new_offset - old_offset)
            }
            TouchPhase::Ended => None,
        }
    }

    fn on_chat_terminal_paste_action(
        &mut self,
        _: &Paste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.swallow_chat_terminal_editor_action(window, cx);
        self.paste_chat_terminal_from_clipboard(cx);
    }

    pub(crate) fn paste_chat_terminal_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let Some(text) = item.text() else {
            return;
        };
        if let Some(session) = self.chat_terminal.as_mut() {
            let _ = session.write_bytes(text.as_bytes());
        }
    }

    fn chat_terminal_cell_from_position(&self, position: Point<Pixels>) -> Option<(usize, usize)> {
        let bounds = self.chat_terminal_input_bounds?;
        let local = bounds.localize(&position)?;
        let (_, cell_width, cell_height) = self.chat_terminal_metrics();
        let snapshot = self
            .chat_terminal
            .as_ref()
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

    fn chat_terminal_normalized_col(&self, row: usize, col: usize) -> Option<usize> {
        let snapshot = self
            .chat_terminal
            .as_ref()
            .map(|session| session.snapshot.clone())
            .unwrap_or_default();
        let rows = Self::chat_terminal_rows(&snapshot);
        let row_cells = &rows.get(row)?.cells;
        previous_non_spacer_terminal_col(row_cells, col)
    }

    pub(crate) fn copy_chat_terminal_selection(&self, cx: &mut Context<Self>) -> bool {
        let Some(selection) = self.chat_terminal_selection else {
            return false;
        };
        if selection.is_empty() {
            return false;
        }

        let snapshot = self
            .chat_terminal
            .as_ref()
            .map(|session| session.snapshot.clone())
            .unwrap_or_default();
        let rows = Self::chat_terminal_rows(&snapshot);
        let selection = selection.normalized();
        let mut lines = Vec::new();

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
            lines.push(line.trim_end_matches(' ').to_string());
        }

        if lines.is_empty() {
            return false;
        }

        cx.write_to_clipboard(ClipboardItem::new_string(lines.join("\n")));
        true
    }

    fn chat_terminal_selection_for_click(
        &self,
        row: usize,
        col: usize,
    ) -> Option<ChatTerminalSelection> {
        let col = self.chat_terminal_normalized_col(row, col).unwrap_or(col);
        self.chat_terminal_word_selection(row, col)
            .or_else(|| Some(ChatTerminalSelection::single(row, col)))
    }

    pub(crate) fn select_all_chat_terminal(&mut self, cx: &mut Context<Self>) -> bool {
        let snapshot = self
            .chat_terminal
            .as_ref()
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

        self.chat_terminal_selection = Some(ChatTerminalSelection {
            start_row: 0,
            start_col: 0,
            end_row,
            end_col,
        });
        self.chat_terminal_selecting = false;
        cx.notify();
        true
    }

    fn chat_terminal_word_selection(
        &self,
        row: usize,
        col: usize,
    ) -> Option<ChatTerminalSelection> {
        let snapshot = self
            .chat_terminal
            .as_ref()
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
    }

    fn on_chat_terminal_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_chat_terminal_panel(window);
        cx.stop_propagation();
        self.chat_terminal_marked_text = None;
        if let Some((row, col)) = self.chat_terminal_cell_from_position(event.position) {
            let col = self.chat_terminal_normalized_col(row, col).unwrap_or(col);
            self.chat_terminal_selection = if event.click_count >= 2 {
                self.chat_terminal_selection_for_click(row, col)
            } else {
                Some(ChatTerminalSelection::single(row, col))
            };
            self.chat_terminal_selecting = event.click_count < 2;
            cx.notify();
        } else {
            self.chat_terminal_selection = None;
            self.chat_terminal_selecting = false;
        }
    }

    fn on_chat_terminal_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.chat_terminal_selecting || !event.dragging() {
            return;
        }

        let Some((row, col)) = self.chat_terminal_cell_from_position(event.position) else {
            return;
        };
        let col = self.chat_terminal_normalized_col(row, col).unwrap_or(col);

        if let Some(selection) = self.chat_terminal_selection.as_mut() {
            selection.end_row = row;
            selection.end_col = col;
            cx.notify();
        }
    }

    fn on_chat_terminal_context_menu(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_chat_terminal_panel(window);
        cx.stop_propagation();
        self.chat_terminal_marked_text = None;
        if let Some((row, col)) = self.chat_terminal_cell_from_position(event.position) {
            let col = self.chat_terminal_normalized_col(row, col).unwrap_or(col);
            let keep_selection = self
                .chat_terminal_selection
                .map(ChatTerminalSelection::normalized)
                .is_some_and(|selection| !selection.is_empty() && selection.contains(row, col));
            if !keep_selection {
                self.chat_terminal_selection = Some(ChatTerminalSelection::single(row, col));
            }
        }
        self.open_terminal_context_menu(event.position, cx);
    }

    fn on_chat_terminal_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.chat_terminal_selecting = false;
        if self
            .chat_terminal_selection
            .is_some_and(ChatTerminalSelection::is_empty)
        {
            self.chat_terminal_selection = None;
        }
        cx.notify();
    }

    pub(crate) fn render_chat_panel(
        &mut self,
        is_focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.resize_chat_terminal();

        let ups = &self.styles.utility_panel;
        let (body_font_size, cell_width, cell_height) = self.chat_terminal_metrics();
        let terminal_padding = self.scaled(ups.utility_panel_terminal_padding_px);
        let monospace = SharedString::from(self.styles.editor.editor_monospace_font_family);
        let default_fg = ups.utility_panel_item_value_text_color;
        let default_bg = ups.utility_panel_bg;
        let cursor_color = self.theme.cursor;
        let palette = terminal_theme_palette(&self.theme, default_bg);
        let content_opacity = if is_focused { 1.0 } else { 0.8 };

        let snapshot = self
            .chat_terminal
            .as_ref()
            .map(|session| session.snapshot.clone())
            .unwrap_or_default();
        let cursor_row = Self::chat_terminal_cursor_display_row(&snapshot);
        let cursor_col = snapshot.cursor.point.column.0;
        let cursor_visible = matches!(
            self.chat_terminal.as_ref().map(|session| &session.status),
            Some(ChatTerminalStatus::Running)
        ) && snapshot.cursor.shape != AlacCursorShape::Hidden
            && cursor_row.is_some();
        let cursor_shape = snapshot.cursor.shape;
        let selection = self
            .chat_terminal_selection
            .filter(|selection| !selection.is_empty())
            .map(ChatTerminalSelection::normalized);
        let marked_text = self.chat_terminal_marked_text.clone();

        div()
            .id("chat-panel")
            .key_context("ChatTerminal")
            .track_focus(&self.chat_panel_focus_handle)
            .relative()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::on_chat_terminal_key_down))
            .on_scroll_wheel(cx.listener(Self::on_chat_terminal_scroll))
            .on_action(cx.listener(|this, _: &Undo, window, cx| {
                this.swallow_chat_terminal_editor_action(window, cx);
            }))
            .on_action(cx.listener(|this, _: &Redo, window, cx| {
                this.swallow_chat_terminal_editor_action(window, cx);
            }))
            .on_action(cx.listener(|this, _: &Backspace, window, cx| {
                this.send_chat_terminal_bytes(b"\x7f", window, cx);
            }))
            .on_action(cx.listener(|this, _: &Delete, window, cx| {
                this.send_chat_terminal_bytes(b"\x1b[3~", window, cx);
            }))
            .on_action(cx.listener(|this, _: &Left, window, cx| {
                this.send_chat_terminal_bytes(b"\x1b[D", window, cx);
            }))
            .on_action(cx.listener(|this, _: &Right, window, cx| {
                this.send_chat_terminal_bytes(b"\x1b[C", window, cx);
            }))
            .on_action(cx.listener(|this, _: &Up, window, cx| {
                this.send_chat_terminal_bytes(b"\x1b[A", window, cx);
            }))
            .on_action(cx.listener(|this, _: &Down, window, cx| {
                this.send_chat_terminal_bytes(b"\x1b[B", window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectLeft, window, cx| {
                this.send_chat_terminal_bytes(b"\x1b[1;2D", window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectRight, window, cx| {
                this.send_chat_terminal_bytes(b"\x1b[1;2C", window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectUp, window, cx| {
                this.send_chat_terminal_bytes(b"\x1b[1;2A", window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectDown, window, cx| {
                this.send_chat_terminal_bytes(b"\x1b[1;2B", window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectAll, window, cx| {
                this.swallow_chat_terminal_editor_action(window, cx);
                let _ = this.select_all_chat_terminal(cx);
            }))
            .on_action(cx.listener(|this, _: &Home, window, cx| {
                this.send_chat_terminal_bytes(b"\x1b[H", window, cx);
            }))
            .on_action(cx.listener(|this, _: &End, window, cx| {
                this.send_chat_terminal_bytes(b"\x1b[F", window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectHome, window, cx| {
                this.send_chat_terminal_bytes(b"\x1b[1;2H", window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectEnd, window, cx| {
                this.send_chat_terminal_bytes(b"\x1b[1;2F", window, cx);
            }))
            .on_action(cx.listener(|this, _: &Enter, window, cx| {
                this.send_chat_terminal_bytes(b"\r", window, cx);
            }))
            .on_action(cx.listener(|this, _: &Tab, window, cx| {
                this.send_chat_terminal_bytes(b"\t", window, cx);
            }))
            .on_action(cx.listener(|this, _: &BackTab, window, cx| {
                this.send_chat_terminal_bytes(b"\x1b[Z", window, cx);
            }))
            .on_action(cx.listener(|this, _: &Copy, window, cx| {
                this.focus_chat_terminal_panel(window);
                cx.stop_propagation();
                let _ = this.copy_chat_terminal_selection(cx);
            }))
            .on_action(cx.listener(|this, _: &Cut, window, cx| {
                this.focus_chat_terminal_panel(window);
                cx.stop_propagation();
                let _ = this.copy_chat_terminal_selection(cx);
            }))
            .on_action(cx.listener(Self::on_chat_terminal_paste_action))
            .on_action(cx.listener(|this, _: &MoveWordLeft, window, cx| {
                this.send_chat_terminal_bytes(b"\x1bb", window, cx);
            }))
            .on_action(cx.listener(|this, _: &MoveWordRight, window, cx| {
                this.send_chat_terminal_bytes(b"\x1bf", window, cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteWordLeft, window, cx| {
                this.send_chat_terminal_bytes(b"\x17", window, cx);
            }))
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                this.send_chat_terminal_bytes(b"\x1b", window, cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(Self::on_chat_terminal_mouse_down),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(Self::on_chat_terminal_context_menu),
            )
            .on_mouse_move(cx.listener(Self::on_chat_terminal_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(Self::on_chat_terminal_mouse_up),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(Self::on_chat_terminal_mouse_up),
            )
            .cursor(CursorStyle::Arrow)
            .h_full()
            .w_full()
            .bg(ups.utility_panel_bg)
            .when(
                self.chat_panel_placement == ChatPanelPlacement::Bottom,
                |this| {
                    this.border_t(px(self.scaled(ups.utility_panel_border_width_px)))
                        .border_color(ups.utility_panel_border_color)
                },
            )
            .when(
                self.chat_panel_placement != ChatPanelPlacement::Bottom,
                |this| {
                    this.border_l(px(self.scaled(ups.utility_panel_border_width_px)))
                        .border_color(ups.utility_panel_border_color)
                },
            )
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .h_full()
                    .relative()
                    .opacity(content_opacity)
                    .child(
                        div()
                            .w_full()
                            .h_full()
                            .p(px(terminal_padding))
                            .relative()
                            .bg(default_bg)
                            .overflow_hidden()
                            .on_scroll_wheel(cx.listener(Self::on_chat_terminal_scroll))
                            .child(ChatTerminalSurfaceElement {
                                editor: cx.entity().clone(),
                                snapshot: snapshot.clone(),
                                selection,
                                cursor_row,
                                cursor_col,
                                cursor_visible,
                                cursor_shape,
                                default_fg,
                                default_bg,
                                cursor_color,
                                palette,
                            }),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(
                                self.scaled(ups.utility_panel_border_width_px) + terminal_padding
                            ))
                            .top(px(terminal_padding))
                            .right(px(terminal_padding))
                            .bottom(px(terminal_padding))
                            .on_scroll_wheel(cx.listener(Self::on_chat_terminal_scroll))
                            .child(ChatTerminalInputElement {
                                editor: cx.entity().clone(),
                            })
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
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn chat_reference_position_clamps_inside_cjk_char() {
        let buffer = crate::buffer::Buffer::from_text("标题\n处理");
        let inside_li = buffer.text().find('理').unwrap() + 1;

        assert_eq!(
            EditorView::chat_reference_position_in_buffer(&buffer, inside_li),
            (2, 2)
        );
    }
}
