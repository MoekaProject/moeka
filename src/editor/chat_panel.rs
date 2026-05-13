use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use alacritty_terminal::{
    event::{Event as AlacTermEvent, EventListener},
    grid::Dimensions,
    index::{Column, Line, Point as AlacPoint},
    sync::FairMutex,
    term::{
        cell::{Cell, Flags},
        Config, RenderableCursor, Term,
    },
    vte::ansi::{Color as AlacColor, CursorShape as AlacCursorShape, NamedColor, Processor, Rgb},
};
use gpui::{
    AnyElement, Context, CursorStyle, FontWeight, Hsla, KeyDownEvent, SharedString, Window, div,
    prelude::*, px,
};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::*;

const CHAT_PANEL_DEFAULT_ROWS: u16 = 28;
const CHAT_PANEL_DEFAULT_COLS: u16 = 96;
const CHAT_PANEL_POLL_MS: u64 = 16;

pub(crate) struct ChatTerminalSession {
    pub(crate) cwd: Option<PathBuf>,
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
            cwd: None,
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

        let mut command = CommandBuilder::new("copilot");
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
            cwd: Some(cwd),
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

    fn resize(&mut self, rows: u16, cols: u16) {
        self.rows = rows.max(2);
        self.cols = cols.max(20);
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

fn terminal_indexed_color(index: u8) -> Hsla {
    match index {
        0 => gpui::rgb(0x000000).into(),
        1 => gpui::rgb(0xcd3131).into(),
        2 => gpui::rgb(0x0dbc79).into(),
        3 => gpui::rgb(0xe5e510).into(),
        4 => gpui::rgb(0x2472c8).into(),
        5 => gpui::rgb(0xbc3fbc).into(),
        6 => gpui::rgb(0x11a8cd).into(),
        7 => gpui::rgb(0xe5e5e5).into(),
        8 => gpui::rgb(0x666666).into(),
        9 => gpui::rgb(0xf14c4c).into(),
        10 => gpui::rgb(0x23d18b).into(),
        11 => gpui::rgb(0xf5f543).into(),
        12 => gpui::rgb(0x3b8eea).into(),
        13 => gpui::rgb(0xd670d6).into(),
        14 => gpui::rgb(0x29b8db).into(),
        15 => gpui::rgb(0xffffff).into(),
        16..=231 => {
            let idx = index - 16;
            let r = idx / 36;
            let g = (idx % 36) / 6;
            let b = idx % 6;
            let level = |value: u8| if value == 0 { 0 } else { 55 + value * 40 };
            gpui::rgb(
                ((level(r) as u32) << 16) | ((level(g) as u32) << 8) | (level(b) as u32),
            )
            .into()
        }
        232..=255 => {
            let gray = 8 + (index - 232) * 10;
            gpui::rgb(((gray as u32) << 16) | ((gray as u32) << 8) | (gray as u32)).into()
        }
    }
}

fn terminal_named_color(color: NamedColor, default_fg: Hsla, default_bg: Hsla) -> Hsla {
    match color {
        NamedColor::Background => default_bg,
        NamedColor::Foreground => default_fg,
        NamedColor::Black => terminal_indexed_color(0),
        NamedColor::Red => terminal_indexed_color(1),
        NamedColor::Green => terminal_indexed_color(2),
        NamedColor::Yellow => terminal_indexed_color(3),
        NamedColor::Blue => terminal_indexed_color(4),
        NamedColor::Magenta => terminal_indexed_color(5),
        NamedColor::Cyan => terminal_indexed_color(6),
        NamedColor::White => terminal_indexed_color(7),
        NamedColor::BrightBlack => terminal_indexed_color(8),
        NamedColor::BrightRed => terminal_indexed_color(9),
        NamedColor::BrightGreen => terminal_indexed_color(10),
        NamedColor::BrightYellow => terminal_indexed_color(11),
        NamedColor::BrightBlue => terminal_indexed_color(12),
        NamedColor::BrightMagenta => terminal_indexed_color(13),
        NamedColor::BrightCyan => terminal_indexed_color(14),
        NamedColor::BrightWhite => terminal_indexed_color(15),
        NamedColor::Cursor => default_fg,
        _ => default_fg,
    }
}

fn terminal_color_to_hsla(color: AlacColor, default_fg: Hsla, default_bg: Hsla) -> Hsla {
    match color {
        AlacColor::Named(named) => terminal_named_color(named, default_fg, default_bg),
        AlacColor::Spec(Rgb { r, g, b }) => {
            gpui::rgb(((r as u32) << 16) | ((g as u32) << 8) | (b as u32)).into()
        }
        AlacColor::Indexed(index) => terminal_indexed_color(index),
    }
}

fn keystroke_to_terminal_bytes(keystroke: &gpui::Keystroke) -> Option<Vec<u8>> {
    let modifiers = keystroke.modifiers;
    let key = keystroke.key.as_str();

    if modifiers.platform || modifiers.function {
        return None;
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
                                    session.refresh_snapshot();
                                }
                                ChatTerminalEvent::Exited => {
                                    session.status = ChatTerminalStatus::Exited;
                                    session.refresh_snapshot();
                                    session.poll_task = None;
                                    exited = true;
                                }
                                ChatTerminalEvent::Error(message) => {
                                    session.status = ChatTerminalStatus::Unavailable(message);
                                    session.poll_task = None;
                                    exited = true;
                                }
                            }
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
        let font_size = self
            .scaled(self.styles.utility_panel.utility_panel_item_font_size_px)
            .max(11.0);
        let panel_width = self.utility_panel_width().max(220.0);
        let panel_height = self.last_window_height.max(240.0);
        let cols = ((panel_width - self.scaled(24.0)) / (font_size * 0.62)).floor() as u16;
        let rows = ((panel_height - self.scaled(96.0)) / (font_size * 1.6)).floor() as u16;
        if let Some(session) = self.chat_terminal.as_mut() {
            session.resize(rows, cols);
        }
    }

    fn on_chat_terminal_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(bytes) = keystroke_to_terminal_bytes(&event.keystroke) else {
            return;
        };
        if let Some(session) = self.chat_terminal.as_mut()
            && session.write_bytes(&bytes).is_ok()
        {
            cx.stop_propagation();
        }
    }

    pub(crate) fn render_chat_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.resize_chat_terminal();

        let ups = &self.styles.utility_panel;
        let top_spacer = self.scaled(ups.utility_panel_top_spacer_height_px);
        let header_height = self.scaled(ups.utility_panel_header_height_px);
        let header_padding_x = self.scaled(ups.utility_panel_header_padding_x_px);
        let header_font_size = self.scaled(ups.utility_panel_header_font_size_px);
        let section_padding_x = self.scaled(ups.utility_panel_section_padding_x_px);
        let body_font_size = self.scaled(ups.utility_panel_item_font_size_px).max(11.0);
        let cell_width = (body_font_size * 0.62).max(7.0);
        let cell_height = (body_font_size * 1.5).max(body_font_size + self.scaled(4.0));
        let monospace = SharedString::from(self.styles.editor.editor_monospace_font_family);
        let default_fg = ups.utility_panel_item_value_text_color;
        let default_bg: Hsla = gpui::rgb(0x10151d).into();
        let cursor_color: Hsla = gpui::rgb(0xd7e3ff).into();

        let (status_label, cwd_label, snapshot) = match self.chat_terminal.as_ref() {
            Some(session) => {
                let status_label = match &session.status {
                    ChatTerminalStatus::Launching => "Starting copilot".to_string(),
                    ChatTerminalStatus::Running => "Connected".to_string(),
                    ChatTerminalStatus::Exited => "Exited".to_string(),
                    ChatTerminalStatus::Unavailable(message) => message.clone(),
                };
                let cwd_label = session
                    .cwd
                    .as_ref()
                    .map(|cwd| cwd.display().to_string())
                    .unwrap_or_else(|| "No workspace".to_string());
                (status_label, cwd_label, session.snapshot.clone())
            }
            None => (
                "Chat session is not running".to_string(),
                "No workspace".to_string(),
                ChatTerminalSnapshot::default(),
            ),
        };

        let mut grid = vec![vec![None; snapshot.cols as usize]; snapshot.rows as usize];
        for indexed in &snapshot.cells {
            let row = indexed.point.line.0 as usize;
            let col = indexed.point.column.0;
            if row >= grid.len() || col >= grid[row].len() {
                continue;
            }
            if indexed.cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || indexed.cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            grid[row][col] = Some(indexed.cell.clone());
        }

        let cursor_row = snapshot.cursor.point.line.0.max(0) as usize;
        let cursor_col = snapshot.cursor.point.column.0;
        let cursor_visible = matches!(
            self.chat_terminal.as_ref().map(|session| &session.status),
            Some(ChatTerminalStatus::Running)
        );

        let row_children = grid.into_iter().enumerate().map(|(row_index, row)| {
            let cell_children = row.into_iter().enumerate().map(|(col_index, maybe_cell)| {
                let cell = maybe_cell.unwrap_or_default();
                let mut fg = terminal_color_to_hsla(cell.fg, default_fg, default_bg);
                let mut bg = terminal_color_to_hsla(cell.bg, default_fg, default_bg);
                if cell.flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                }

                let is_cursor = cursor_visible && row_index == cursor_row && col_index == cursor_col;
                if is_cursor {
                    std::mem::swap(&mut fg, &mut bg);
                    bg = cursor_color;
                }

                let is_wide = cell.flags.contains(Flags::WIDE_CHAR);
                let weight = if cell.flags.contains(Flags::BOLD) {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                };

                div()
                    .w(px(if is_wide { cell_width * 2.0 } else { cell_width }))
                    .h(px(cell_height))
                    .flex_none()
                    .bg(bg)
                    .text_color(fg)
                    .font_family(monospace.clone())
                    .font_weight(weight)
                    .text_size(px(body_font_size))
                    .line_height(px(cell_height))
                    .overflow_hidden()
                    .child(normalize_cell_char(&cell))
                    .into_any_element()
            });

            div()
                .id(SharedString::from(format!("chat-grid-row-{}", row_index)))
                .h(px(cell_height))
                .flex_none()
                .flex()
                .flex_row()
                .children(cell_children)
                .into_any_element()
        });

        div()
            .id("chat-panel")
            .key_context("ChatTerminal")
            .track_focus(&self.chat_panel_focus_handle)
            .on_key_down(cx.listener(Self::on_chat_terminal_key_down))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _event, window, cx| {
                    this.chat_panel_focus_handle.focus(window);
                    cx.stop_propagation();
                }),
            )
            .cursor(CursorStyle::Arrow)
            .h_full()
            .w_full()
            .bg(ups.utility_panel_bg)
            .border_l(px(self.scaled(ups.utility_panel_border_width_px)))
            .border_color(ups.utility_panel_border_color)
            .flex()
            .flex_col()
            .child(div().h(px(top_spacer)).flex_shrink_0())
            .child(
                div()
                    .h(px(header_height))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(header_padding_x))
                    .text_size(px(header_font_size))
                    .text_color(ups.utility_panel_header_text_color)
                    .child("Chat")
                    .child(
                        div()
                            .text_size(px(self.scaled(ups.utility_panel_item_font_size_px)))
                            .text_color(ups.utility_panel_empty_text_color)
                            .child(status_label),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(section_padding_x))
                    .pb(px(self.scaled(8.0)))
                    .font_family(monospace.clone())
                    .text_size(px(body_font_size))
                    .text_color(ups.utility_panel_empty_text_color)
                    .child(cwd_label),
            )
            .child(
                div()
                    .flex_1()
                    .mx(px(section_padding_x))
                    .mb(px(self.scaled(12.0)))
                    .bg(default_bg)
                    .border_1()
                    .border_color(ups.utility_panel_border_color)
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .w_full()
                            .flex()
                            .flex_col()
                            .children(row_children),
                    ),
            )
            .into_any_element()
    }
}
