use std::io::{Read, Write};
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
        cell::{Cell, Flags},
        Config, RenderableCursor, Term,
    },
    vte::ansi::{Color as AlacColor, CursorShape as AlacCursorShape, NamedColor, Processor, Rgb},
};
use gpui::{
    AnyElement, Context, CursorStyle, FontWeight, Hsla, KeyDownEvent, ScrollWheelEvent,
    SharedString, Window, div, prelude::*, px,
};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::*;

const CHAT_PANEL_DEFAULT_ROWS: u16 = 28;
const CHAT_PANEL_DEFAULT_COLS: u16 = 96;
const CHAT_PANEL_POLL_MS: u64 = 16;

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
            let _ = session.write_bytes(format!("@{}", relative_path).as_bytes());
        }
    }

    fn swallow_chat_terminal_editor_action(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let terminal_padding = self.scaled(self.styles.utility_panel.utility_panel_terminal_padding_px);
        let panel_width = (self.utility_panel_width() - terminal_padding * 2.0).max(220.0);
        let panel_height = (self.last_window_height - terminal_padding * 2.0).max(240.0);
        let cols = (panel_width / (font_size * 0.62)).floor() as u16;
        let rows = (panel_height / (font_size * 1.6)).floor() as u16;
        if let Some(session) = self.chat_terminal.as_mut() {
            session.resize(rows, cols);
        }
    }

    fn on_chat_terminal_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_chat_terminal_panel(window);
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

        let line_height = (self
            .scaled(self.styles.utility_panel.utility_panel_item_font_size_px)
            .max(11.0)
            * 1.5)
            .max(1.0);
        let delta_y: f32 = event.delta.pixel_delta(px(line_height)).y.into();
        if delta_y.abs() < 1.0 {
            return;
        }

        let lines = ((delta_y.abs() / line_height).round() as i32).max(1);
        let delta = if delta_y < 0.0 { lines } else { -lines };
        if let Some(session) = self.chat_terminal.as_mut() {
            session.scroll_display(Scroll::Delta(delta));
            cx.notify();
        }
    }

    fn on_chat_terminal_paste_action(
        &mut self,
        _: &Paste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.swallow_chat_terminal_editor_action(window, cx);
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

    pub(crate) fn render_chat_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.resize_chat_terminal();

        let ups = &self.styles.utility_panel;
        let body_font_size = self.scaled(ups.utility_panel_item_font_size_px).max(11.0);
        let cell_width = (body_font_size * 0.62).max(7.0);
        let cell_height = (body_font_size * 1.5).max(body_font_size + self.scaled(4.0));
        let terminal_padding = self.scaled(ups.utility_panel_terminal_padding_px);
        let monospace = SharedString::from(self.styles.editor.editor_monospace_font_family);
        let default_fg = ups.utility_panel_item_value_text_color;
        let default_bg = ups.utility_panel_bg;
        let cursor_color = self.theme.cursor;
        let palette = terminal_theme_palette(&self.theme, default_bg);

        let snapshot = self
            .chat_terminal
            .as_ref()
            .map(|session| session.snapshot.clone())
            .unwrap_or_default();

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
        ) && snapshot.cursor.shape != AlacCursorShape::Hidden;
        let cursor_shape = snapshot.cursor.shape;

        let row_children = grid.into_iter().enumerate().map(|(row_index, row)| {
            let cell_children = row.into_iter().enumerate().map(|(col_index, maybe_cell)| {
                let cell = maybe_cell.unwrap_or_default();
                let mut fg = terminal_color_to_hsla(cell.fg, &palette, default_fg, default_bg);
                let mut bg = terminal_color_to_hsla(cell.bg, &palette, default_fg, default_bg);
                if cell.flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                }

                let is_cursor = cursor_visible && row_index == cursor_row && col_index == cursor_col;
                if is_cursor {
                    if cursor_shape == AlacCursorShape::Block {
                        std::mem::swap(&mut fg, &mut bg);
                        bg = cursor_color;
                    }
                }

                let is_wide = cell.flags.contains(Flags::WIDE_CHAR);
                let weight = if cell.flags.contains(Flags::BOLD) {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                };

                div()
                    .relative()
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
                    .when(is_cursor && cursor_shape == AlacCursorShape::Beam, |this| {
                        this.child(
                            div()
                                .absolute()
                                .left(px(0.0))
                                .top(px(0.0))
                                .bottom(px(0.0))
                                .w(px(2.0))
                                .bg(cursor_color),
                        )
                    })
                    .when(is_cursor && cursor_shape == AlacCursorShape::Underline, |this| {
                        this.child(
                            div()
                                .absolute()
                                .left(px(0.0))
                                .right(px(0.0))
                                .bottom(px(0.0))
                                .h(px(2.0))
                                .bg(cursor_color),
                        )
                    })
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
                this.swallow_chat_terminal_editor_action(window, cx);
            }))
            .on_action(cx.listener(|this, _: &Cut, window, cx| {
                this.swallow_chat_terminal_editor_action(window, cx);
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
                gpui::MouseButton::Left,
                cx.listener(|this, _event, window, cx| {
                    this.focus_chat_terminal_panel(window);
                    cx.stop_propagation();
                }),
            )
            .cursor(CursorStyle::Arrow)
            .h_full()
            .w_full()
            .bg(ups.utility_panel_bg)
            .border_l(px(self.scaled(ups.utility_panel_border_width_px)))
            .border_color(ups.utility_panel_border_color)
            .child(
                div()
                    .flex_1()
                    .p(px(terminal_padding))
                    .bg(default_bg)
                    .overflow_hidden()
                    .on_scroll_wheel(cx.listener(Self::on_chat_terminal_scroll))
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
