use std::ops::Range;
use std::time::Duration;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, GlobalElementId, Hsla, KeyDownEvent, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, Render,
    ShapedLine, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window, anchored,
    deferred, div, fill, point, prelude::*, px, relative, size,
};

use crate::{
    buffer::{clamp_grapheme_boundary, next_grapheme_boundary, prev_grapheme_boundary},
    style::TextInputStyles,
};

// ── Events ──────────────────────────────────────────────────────────

pub enum TextInputEvent {
    Changed,
    Submit,
    SubmitBackward,
    Escape,
    MoveUp,
    MoveDown,
}

impl gpui::EventEmitter<TextInputEvent> for TextInput {}

// ── Actions (scoped to TextInput context) ───────────────────────────

gpui::actions!(
    text_input,
    [
        TextInputLeft,
        TextInputRight,
        TextInputHome,
        TextInputEnd,
        TextInputSelectLeft,
        TextInputSelectRight,
        TextInputSelectHome,
        TextInputSelectEnd,
        TextInputSelectAll,
        TextInputSelectWordLeft,
        TextInputSelectWordRight,
        TextInputBackspace,
        TextInputDelete,
        TextInputDeleteWordLeft,
        TextInputDeleteWordRight,
        TextInputDeleteToStart,
        TextInputDeleteToEnd,
        TextInputCopy,
        TextInputCut,
        TextInputPaste,
        TextInputMoveWordLeft,
        TextInputMoveWordRight,
        TextInputUndo,
        TextInputPickerUp,
        TextInputPickerDown,
        TextInputSubmit,
        TextInputSubmitBackward,
    ]
);

// ── TextInput component ─────────────────────────────────────────────

pub struct TextInput {
    pub(crate) text: String,
    pub(crate) cursor_offset: usize,
    pub(crate) selection_anchor: usize,
    pub(crate) marked_range: Option<Range<usize>>,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) placeholder: SharedString,
    pub(crate) styles: TextInputStyles,
    /// Multi-line mode: Enter inserts a newline instead of submitting, and the
    /// text lays out as stacked lines.
    pub(crate) multiline: bool,
    // Layout cache for mouse hit-testing
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    last_center_offset: Pixels,
    /// Multi-line layout cache: shaped line, segment start byte offset, and
    /// horizontal centering offset for each visual line.
    last_lines: Vec<(ShapedLine, usize, Pixels)>,
    is_selecting: bool,
    // Cursor blink state
    cursor_opacity: f32,
    cursor_opacity_direction: f32,
    blink_epoch: usize,
    // Context menu
    context_menu_position: Option<Point<Pixels>>,
    /// Whether right-click opens the copy/paste context menu. Must be disabled
    /// for inputs hosted inside `deferred(...)` overlays (quick open, pickers,
    /// modals): the menu itself is a deferred element, and gpui panics with
    /// "cannot call defer_draw during deferred drawing" when deferred draws
    /// nest.
    pub(crate) context_menu_enabled: bool,
    // Undo
    undo_stack: Vec<(String, usize)>, // (text, cursor_offset)
}

impl TextInput {
    pub fn new(
        styles: TextInputStyles,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let mut this = Self {
            text: String::new(),
            cursor_offset: 0,
            selection_anchor: 0,
            marked_range: None,
            focus_handle,
            placeholder: placeholder.into(),
            styles,
            multiline: false,
            last_layout: None,
            last_bounds: None,
            last_center_offset: Pixels::ZERO,
            last_lines: Vec::new(),
            is_selecting: false,
            cursor_opacity: 1.0,
            cursor_opacity_direction: -1.0,
            blink_epoch: 0,
            context_menu_position: None,
            context_menu_enabled: true,
            undo_stack: Vec::new(),
        };
        this.start_blinking(cx);
        this
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.text = text.to_string();
        self.cursor_offset = self.text.len();
        self.selection_anchor = self.cursor_offset;
        cx.notify();
    }

    pub fn set_styles(&mut self, styles: TextInputStyles, cx: &mut Context<Self>) {
        self.styles = styles;
        cx.notify();
    }

    pub fn set_selection(&mut self, anchor: usize, offset: usize, cx: &mut Context<Self>) {
        self.selection_anchor = anchor.min(self.text.len());
        self.cursor_offset = offset.min(self.text.len());
        cx.notify();
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.selection_anchor = 0;
        self.cursor_offset = self.text.len();
        cx.notify();
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Insert a newline at the cursor (multiline mode).
    fn insert_newline(&mut self, cx: &mut Context<Self>) {
        self.undo_stack
            .push((self.text.clone(), self.cursor_offset));
        self.delete_selection();
        self.text.insert(self.cursor_offset, '\n');
        self.cursor_offset += 1;
        self.selection_anchor = self.cursor_offset;
        self.pause_blinking(cx);
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }

    /// Move the cursor one visual line up/down (multiline mode), preserving the
    /// byte column where possible.
    fn move_line_vertical(&mut self, down: bool, cx: &mut Context<Self>) {
        let segments: Vec<(usize, usize)> = {
            let mut out = Vec::new();
            let mut start = 0usize;
            for part in self.text.split('\n') {
                out.push((start, start + part.len()));
                start += part.len() + 1;
            }
            out
        };
        let row = segments
            .iter()
            .position(|(s, e)| self.cursor_offset >= *s && self.cursor_offset <= *e)
            .unwrap_or(0);
        let target = if down {
            if row + 1 >= segments.len() {
                return;
            }
            row + 1
        } else {
            if row == 0 {
                return;
            }
            row - 1
        };
        let col = self.cursor_offset - segments[row].0;
        let (ts, te) = segments[target];
        let mut offset = (ts + col).min(te);
        // Keep the offset on a char boundary.
        while offset > ts && !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        self.cursor_offset = offset;
        self.selection_anchor = offset;
        self.pause_blinking(cx);
        cx.notify();
    }

    pub fn has_selection(&self) -> bool {
        self.cursor_offset != self.selection_anchor
    }

    pub fn selection_range(&self) -> Range<usize> {
        let start = self.cursor_offset.min(self.selection_anchor);
        let end = self.cursor_offset.max(self.selection_anchor);
        start..end
    }

    fn delete_selection(&mut self) {
        if !self.has_selection() {
            return;
        }
        let range = self.selection_range();
        self.text.replace_range(range.clone(), "");
        self.cursor_offset = range.start;
        self.selection_anchor = range.start;
    }

    fn move_cursor(&mut self, offset: usize, extend_selection: bool) {
        self.cursor_offset = clamp_grapheme_boundary(&self.text, offset);
        if !extend_selection {
            self.selection_anchor = self.cursor_offset;
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.text.is_empty() {
            return 0;
        }
        if self.multiline && !self.last_lines.is_empty() {
            let Some(bounds) = self.last_bounds.as_ref() else {
                return 0;
            };
            let line_height = px(self.styles.text_input_height_px);
            let row = (f32::from(position.y - bounds.top()) / f32::from(line_height))
                .floor()
                .clamp(0.0, (self.last_lines.len() - 1) as f32) as usize;
            let (line, seg_start, center_offset) = &self.last_lines[row];
            return seg_start
                + line.closest_index_for_x(position.x - bounds.left() - *center_offset);
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.x <= bounds.left() {
            return 0;
        }
        if position.x >= bounds.right() {
            return self.text.len();
        }
        line.closest_index_for_x(position.x - bounds.left() - self.last_center_offset)
    }

    fn prev_char_boundary(&self, offset: usize) -> usize {
        prev_grapheme_boundary(&self.text, offset)
    }

    fn next_char_boundary(&self, offset: usize) -> usize {
        next_grapheme_boundary(&self.text, offset)
    }

    fn prev_word_boundary(&self, offset: usize) -> usize {
        if offset == 0 {
            return 0;
        }
        let bytes = self.text.as_bytes();
        let mut pos = self.prev_char_boundary(offset);
        while pos > 0 && bytes[pos].is_ascii_whitespace() {
            pos = self.prev_char_boundary(pos);
        }
        while pos > 0 {
            let prev = self.prev_char_boundary(pos);
            if bytes[prev].is_ascii_whitespace() || bytes[prev] == b'/' || bytes[prev] == b'.' {
                break;
            }
            pos = prev;
        }
        pos
    }

    fn next_word_boundary(&self, offset: usize) -> usize {
        if offset >= self.text.len() {
            return self.text.len();
        }
        let bytes = self.text.as_bytes();
        let mut pos = self.next_char_boundary(offset);
        while pos < self.text.len()
            && !bytes[pos].is_ascii_whitespace()
            && bytes[pos] != b'/'
            && bytes[pos] != b'.'
        {
            pos = self.next_char_boundary(pos);
        }
        while pos < self.text.len() && bytes[pos].is_ascii_whitespace() {
            pos = self.next_char_boundary(pos);
        }
        pos
    }

    fn fire_change(&self, _window: &mut Window, cx: &mut Context<Self>) {
        self.pause_blinking(cx);
        cx.emit(TextInputEvent::Changed);
    }

    // ── Cursor blink ────────────────────────────────────────────────

    fn next_blink_epoch(&mut self) -> usize {
        self.blink_epoch += 1;
        self.blink_epoch
    }

    fn start_blinking(&mut self, cx: &mut Context<Self>) {
        self.cursor_opacity = 1.0;
        self.cursor_opacity_direction = -1.0;
        let epoch = self.next_blink_epoch();
        self.schedule_blink(epoch, cx);
    }

    fn pause_blinking(&self, cx: &mut Context<Self>) {
        // Immediately show cursor at full opacity, then restart blink after a delay
        let epoch = self.blink_epoch + 1;
        // We can't mutate self here since &self, so schedule a task
        cx.spawn(async move |this, cx| {
            this.update(cx, |this, cx| {
                this.cursor_opacity = 1.0;
                this.cursor_opacity_direction = -1.0;
                this.blink_epoch = epoch;
                cx.notify();
            })
            .ok();
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            this.update(cx, |this, cx| {
                if this.blink_epoch == epoch {
                    this.schedule_blink(epoch, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn schedule_blink(&self, epoch: usize, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(5))
                .await;
            this.update(cx, |this, cx| {
                if this.blink_epoch == epoch {
                    this.cursor_opacity += 0.03 * this.cursor_opacity_direction;
                    if this.cursor_opacity <= 0.0 {
                        this.cursor_opacity = 0.0;
                        this.cursor_opacity_direction = 1.0;
                        cx.notify();
                        // Pause at transparent
                        this.schedule_blink_pause(epoch, Duration::from_millis(300), cx);
                        return;
                    } else if this.cursor_opacity >= 1.0 {
                        this.cursor_opacity = 1.0;
                        this.cursor_opacity_direction = -1.0;
                        cx.notify();
                        // Pause at visible
                        this.schedule_blink_pause(epoch, Duration::from_millis(200), cx);
                        return;
                    }
                    cx.notify();
                    this.schedule_blink(epoch, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn schedule_blink_pause(&self, epoch: usize, duration: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            this.update(cx, |this, cx| {
                if this.blink_epoch == epoch {
                    this.schedule_blink(epoch, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    // ── Mouse handlers ──────────────────────────────────────────────

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        let offset = self.index_for_mouse_position(event.position);

        if event.click_count == 3 {
            // Triple click: select all
            self.selection_anchor = 0;
            self.cursor_offset = self.text.len();
        } else if event.click_count == 2 {
            // Double click: select word
            let start = self.prev_word_boundary(offset);
            let end = self.next_word_boundary(offset);
            self.selection_anchor = start;
            self.cursor_offset = end;
        } else if event.modifiers.shift {
            self.move_cursor(offset, true);
        } else {
            self.move_cursor(offset, false);
        }
        self.pause_blinking(cx);
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _cx: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            let offset = self.index_for_mouse_position(event.position);
            self.move_cursor(offset, true);
            cx.notify();
        }
    }

    fn on_right_click(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.context_menu_enabled {
            return;
        }
        self.context_menu_position = Some(event.position);
        cx.notify();
    }

    fn dismiss_context_menu(&mut self, cx: &mut Context<Self>) {
        self.context_menu_position = None;
        cx.notify();
    }

    fn render_context_menu(
        &self,
        _position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let s = &self.styles;
        let menu_bg = s.text_input_menu_bg;
        let text_color = s.text_input_menu_text_color;
        let menu_font_size = s.text_input_menu_font_size_px;
        let border_color = s.text_input_menu_border_color;
        let radius = s.text_input_menu_radius_px;
        let padding_y = s.text_input_menu_padding_y_px;
        let item_height = s.text_input_menu_item_height_px;
        let item_padding_x = s.text_input_menu_item_padding_x_px;
        let hover_bg = s.text_input_menu_hover_bg;
        let separator_color = s.text_input_menu_separator_color;
        let menu_width = s.text_input_menu_width_px;
        let menu_shadow = s.text_input_menu_shadow;
        let has_sel = self.has_selection();
        let language = crate::i18n::detect_system_language();

        let item =
            |id: &str,
             label: &str,
             enabled: bool,
             cx: &mut Context<Self>,
             action: fn(&mut TextInput, &mut Window, &mut Context<TextInput>)| {
                let item_text_color = if enabled {
                    text_color
                } else {
                    Hsla {
                        a: text_color.a * 0.4,
                        ..text_color
                    }
                };
                div()
                    .id(SharedString::from(format!("ctx-{id}")))
                    .w_full()
                    .h(px(item_height))
                    .px(px(item_padding_x))
                    .flex()
                    .items_center()
                    .text_size(px(menu_font_size))
                    .text_color(item_text_color)
                    .cursor(CursorStyle::Arrow)
                    .when(enabled, |el| {
                        el.hover(move |s| s.bg(hover_bg)).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.context_menu_position = None;
                                action(this, window, cx);
                            }),
                        )
                    })
                    .child(label.to_string())
            };

        // The menu is positioned by anchored(), no need for offset calculations
        div()
            .w(px(menu_width))
            .bg(menu_bg)
            .border_1()
            .border_color(border_color)
            .rounded(px(radius))
            .when(menu_shadow, |this| this.shadow_lg())
            .py(px(padding_y))
            .overflow_hidden()
            .flex()
            .flex_col()
            .text_size(px(menu_font_size))
            .child(item(
                "cut",
                crate::i18n::text_input_menu_label(language, "cut"),
                has_sel,
                cx,
                |this, w, cx| {
                    this.on_cut(&TextInputCut, w, cx);
                },
            ))
            .child(item(
                "copy",
                crate::i18n::text_input_menu_label(language, "copy"),
                has_sel,
                cx,
                |this, w, cx| {
                    this.on_copy(&TextInputCopy, w, cx);
                },
            ))
            .child(item(
                "paste",
                crate::i18n::text_input_menu_label(language, "paste"),
                true,
                cx,
                |this, w, cx| {
                    this.on_paste(&TextInputPaste, w, cx);
                },
            ))
            .child(div().w_full().h(px(1.0)).my(px(2.0)).bg(separator_color))
            .child(item(
                "select-all",
                crate::i18n::text_input_menu_label(language, "select_all"),
                !self.text.is_empty(),
                cx,
                |this, _w, cx| {
                    this.selection_anchor = 0;
                    this.cursor_offset = this.text.len();
                    cx.notify();
                },
            ))
    }

    // ── Action handlers ─────────────────────────────────────────────

    fn on_left(&mut self, _: &TextInputLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.has_selection() {
            let start = self.selection_range().start;
            self.move_cursor(start, false);
        } else {
            let pos = self.prev_char_boundary(self.cursor_offset);
            self.move_cursor(pos, false);
        }
        cx.notify();
    }

    fn on_right(&mut self, _: &TextInputRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.has_selection() {
            let end = self.selection_range().end;
            self.move_cursor(end, false);
        } else {
            let pos = self.next_char_boundary(self.cursor_offset);
            self.move_cursor(pos, false);
        }
        cx.notify();
    }

    fn on_home(&mut self, _: &TextInputHome, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(0, false);
        cx.notify();
    }

    fn on_end(&mut self, _: &TextInputEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(self.text.len(), false);
        cx.notify();
    }

    fn on_select_left(&mut self, _: &TextInputSelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let pos = self.prev_char_boundary(self.cursor_offset);
        self.move_cursor(pos, true);
        cx.notify();
    }

    fn on_select_right(
        &mut self,
        _: &TextInputSelectRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pos = self.next_char_boundary(self.cursor_offset);
        self.move_cursor(pos, true);
        cx.notify();
    }

    fn on_select_home(&mut self, _: &TextInputSelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(0, true);
        cx.notify();
    }

    fn on_select_end(&mut self, _: &TextInputSelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(self.text.len(), true);
        cx.notify();
    }

    fn on_select_all(&mut self, _: &TextInputSelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selection_anchor = 0;
        self.cursor_offset = self.text.len();
        cx.notify();
    }

    fn on_backspace(
        &mut self,
        _: &TextInputBackspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.push_undo();
        if self.has_selection() {
            self.delete_selection();
        } else if self.cursor_offset > 0 {
            let prev = self.prev_char_boundary(self.cursor_offset);
            self.text.replace_range(prev..self.cursor_offset, "");
            self.cursor_offset = prev;
            self.selection_anchor = prev;
        }
        self.fire_change(window, cx);
        cx.notify();
    }

    fn on_delete(&mut self, _: &TextInputDelete, window: &mut Window, cx: &mut Context<Self>) {
        self.push_undo();
        if self.has_selection() {
            self.delete_selection();
        } else if self.cursor_offset < self.text.len() {
            let next = self.next_char_boundary(self.cursor_offset);
            self.text.replace_range(self.cursor_offset..next, "");
        }
        self.fire_change(window, cx);
        cx.notify();
    }

    fn on_delete_word_left(
        &mut self,
        _: &TextInputDeleteWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.push_undo();
        if self.has_selection() {
            self.delete_selection();
        } else {
            let prev = self.prev_word_boundary(self.cursor_offset);
            self.text.replace_range(prev..self.cursor_offset, "");
            self.cursor_offset = prev;
            self.selection_anchor = prev;
        }
        self.fire_change(window, cx);
        cx.notify();
    }

    fn on_copy(&mut self, _: &TextInputCopy, _: &mut Window, cx: &mut Context<Self>) {
        if self.has_selection() {
            let range = self.selection_range();
            cx.write_to_clipboard(ClipboardItem::new_string(self.text[range].to_string()));
        }
    }

    fn on_cut(&mut self, _: &TextInputCut, window: &mut Window, cx: &mut Context<Self>) {
        self.push_undo();
        if self.has_selection() {
            let range = self.selection_range();
            cx.write_to_clipboard(ClipboardItem::new_string(self.text[range].to_string()));
            self.delete_selection();
            self.fire_change(window, cx);
            cx.notify();
        }
    }

    fn on_paste(&mut self, _: &TextInputPaste, window: &mut Window, cx: &mut Context<Self>) {
        self.push_undo();
        if let Some(item) = cx.read_from_clipboard() {
            let paste_text = item.text().unwrap_or_default().replace('\n', " ");
            if self.has_selection() {
                self.delete_selection();
            }
            self.text.insert_str(self.cursor_offset, &paste_text);
            self.cursor_offset += paste_text.len();
            self.selection_anchor = self.cursor_offset;
            self.fire_change(window, cx);
            cx.notify();
        }
    }

    fn on_word_left(&mut self, _: &TextInputMoveWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let pos = self.prev_word_boundary(self.cursor_offset);
        self.move_cursor(pos, false);
        cx.notify();
    }

    fn on_word_right(
        &mut self,
        _: &TextInputMoveWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pos = self.next_word_boundary(self.cursor_offset);
        self.move_cursor(pos, false);
        cx.notify();
    }

    fn on_select_word_left(
        &mut self,
        _: &TextInputSelectWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pos = self.prev_word_boundary(self.cursor_offset);
        self.move_cursor(pos, true);
        cx.notify();
    }

    fn on_select_word_right(
        &mut self,
        _: &TextInputSelectWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pos = self.next_word_boundary(self.cursor_offset);
        self.move_cursor(pos, true);
        cx.notify();
    }

    fn on_delete_word_right(
        &mut self,
        _: &TextInputDeleteWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.push_undo();
        if self.has_selection() {
            self.delete_selection();
        } else {
            let next = self.next_word_boundary(self.cursor_offset);
            self.text.replace_range(self.cursor_offset..next, "");
        }
        self.fire_change(window, cx);
        cx.notify();
    }

    fn on_delete_to_start(
        &mut self,
        _: &TextInputDeleteToStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.push_undo();
        if self.cursor_offset > 0 {
            self.text.replace_range(0..self.cursor_offset, "");
            self.cursor_offset = 0;
            self.selection_anchor = 0;
        }
        self.fire_change(window, cx);
        cx.notify();
    }

    fn on_delete_to_end(
        &mut self,
        _: &TextInputDeleteToEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.push_undo();
        if self.cursor_offset < self.text.len() {
            self.text
                .replace_range(self.cursor_offset..self.text.len(), "");
        }
        self.fire_change(window, cx);
        cx.notify();
    }

    fn on_undo(&mut self, _: &TextInputUndo, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some((text, offset)) = self.undo_stack.pop() {
            self.text = text;
            self.cursor_offset = offset.min(self.text.len());
            self.selection_anchor = self.cursor_offset;
            cx.emit(TextInputEvent::Changed);
            self.pause_blinking(cx);
            cx.notify();
        }
    }

    fn push_undo(&mut self) {
        self.undo_stack
            .push((self.text.clone(), self.cursor_offset));
        if self.undo_stack.len() > 50 {
            self.undo_stack.remove(0);
        }
    }

    // ── UTF-16 helpers ──────────────────────────────────────────────

    fn offset_from_utf16(&self, utf16_offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for ch in self.text.chars() {
            if utf16 >= utf16_offset {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    fn offset_to_utf16(&self, utf8_offset: usize) -> usize {
        let mut utf16 = 0;
        let mut utf8 = 0;
        for ch in self.text.chars() {
            if utf8 >= utf8_offset {
                break;
            }
            utf8 += ch.len_utf8();
            utf16 += ch.len_utf16();
        }
        utf16
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
}

// ── Render ──────────────────────────────────────────────────────────

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = &self.styles;
        let is_focused = self.focus_handle.is_focused(window);
        let bg = if is_focused {
            s.text_input_focus_bg
        } else {
            s.text_input_bg
        };
        let hover_bg = if is_focused {
            Some(s.text_input_focus_bg)
        } else {
            s.text_input_hover_bg
        };
        let height = s.text_input_height_px;
        let line_count = if self.multiline {
            self.text.split('\n').count().max(1)
        } else {
            1
        };
        let total_height = height * line_count as f32;
        let padding_left = s.text_input_padding_left_px;
        let padding_right = s.text_input_padding_right_px;
        let font_size = s.text_input_font_size_px;

        div()
            .id("text-input")
            .key_context("TextInput")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_left))
            .on_action(cx.listener(Self::on_right))
            .on_action(cx.listener(Self::on_home))
            .on_action(cx.listener(Self::on_end))
            .on_action(cx.listener(Self::on_select_left))
            .on_action(cx.listener(Self::on_select_right))
            .on_action(cx.listener(Self::on_select_home))
            .on_action(cx.listener(Self::on_select_end))
            .on_action(cx.listener(Self::on_select_all))
            .on_action(cx.listener(Self::on_backspace))
            .on_action(cx.listener(Self::on_delete))
            .on_action(cx.listener(Self::on_delete_word_left))
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_cut))
            .on_action(cx.listener(Self::on_paste))
            .on_action(cx.listener(Self::on_word_left))
            .on_action(cx.listener(Self::on_word_right))
            .on_action(cx.listener(Self::on_select_word_left))
            .on_action(cx.listener(Self::on_select_word_right))
            .on_action(cx.listener(Self::on_delete_word_right))
            .on_action(cx.listener(Self::on_delete_to_start))
            .on_action(cx.listener(Self::on_delete_to_end))
            .on_action(cx.listener(Self::on_undo))
            .on_action(cx.listener(|this, _: &TextInputPickerUp, _window, cx| {
                if this.multiline {
                    this.move_line_vertical(false, cx);
                } else {
                    cx.emit(TextInputEvent::MoveUp);
                }
            }))
            .on_action(cx.listener(|this, _: &TextInputPickerDown, _window, cx| {
                if this.multiline {
                    this.move_line_vertical(true, cx);
                } else {
                    cx.emit(TextInputEvent::MoveDown);
                }
            }))
            .on_action(cx.listener(|this, _: &TextInputSubmit, _window, cx| {
                if this.multiline {
                    this.insert_newline(cx);
                } else {
                    cx.emit(TextInputEvent::Submit);
                }
            }))
            .on_action(
                cx.listener(|this, _: &TextInputSubmitBackward, _window, cx| {
                    if this.multiline {
                        this.insert_newline(cx);
                    } else {
                        cx.emit(TextInputEvent::SubmitBackward);
                    }
                }),
            )
            .on_key_down(cx.listener(|_this, event: &KeyDownEvent, _window, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(TextInputEvent::Escape);
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    this.dismiss_context_menu(cx);
                    this.on_mouse_down(event, _window, cx);
                }),
            )
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_click))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .cursor(CursorStyle::IBeam)
            .w_full()
            .h(px(total_height))
            .pl(px(padding_left))
            .pr(px(padding_right))
            .bg(bg)
            .when_some(hover_bg, |this, hover_bg| {
                this.hover(move |style| style.bg(hover_bg))
            })
            .rounded(px(s.text_input_radius_px))
            .flex()
            .items_center()
            .overflow_hidden()
            .text_size(px(font_size))
            .line_height(px(height))
            .child(TextElement { input: cx.entity() })
            .when_some(self.context_menu_position, |el, pos| {
                el.child(
                    deferred(
                        anchored().position(pos).child(
                            div()
                                .id("text-input-context-menu-backdrop")
                                .occlude()
                                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                    this.dismiss_context_menu(cx);
                                }))
                                .child(self.render_context_menu(pos, cx)),
                        ),
                    )
                    .with_priority(1),
                )
            })
    }
}

// ── EntityInputHandler ──────────────────────────────────────────────

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        let end = range.end.min(self.text.len());
        let start = range.start.min(end);
        actual_range.replace(self.range_to_utf16(&(start..end)));
        Some(self.text[start..end].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selection_range()),
            reversed: self.cursor_offset < self.selection_anchor,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.marked_range.is_none() {
            self.push_undo();
        }
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or_else(|| self.selection_range());

        let start = range.start.min(self.text.len());
        let end = range.end.min(self.text.len());
        self.text.replace_range(start..end, new_text);
        let new_offset = start + new_text.len();
        self.cursor_offset = new_offset.min(self.text.len());
        self.selection_anchor = self.cursor_offset;
        self.marked_range = None;
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or_else(|| self.selection_range());

        let start = range.start.min(self.text.len());
        let end = range.end.min(self.text.len());
        self.text.replace_range(start..end, new_text);

        if !new_text.is_empty() {
            self.marked_range = Some(start..start + new_text.len());
        } else {
            self.marked_range = None;
        }

        self.cursor_offset = new_selected_range_utf16
            .as_ref()
            .map(|r| {
                let r8 = self.range_from_utf16(r);
                start + r8.end.min(new_text.len())
            })
            .unwrap_or(start + new_text.len());
        self.cursor_offset = self.cursor_offset.min(self.text.len());
        self.selection_anchor = self.cursor_offset;
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        // Multi-line: anchor the IME candidate window to the row containing
        // the range start, honoring per-line centering.
        if self.multiline && !self.last_lines.is_empty() {
            let line_height = px(self.styles.text_input_height_px);
            let mut row = 0;
            for (i, (_, seg_start, _)) in self.last_lines.iter().enumerate() {
                if range.start >= *seg_start {
                    row = i;
                } else {
                    break;
                }
            }
            let (line, seg_start, center_offset) = &self.last_lines[row];
            let local_start = range.start.saturating_sub(*seg_start);
            let local_end = range.end.saturating_sub(*seg_start);
            let top = bounds.top() + line_height * row as f32;
            return Some(Bounds::from_corners(
                point(
                    bounds.left() + *center_offset + line.x_for_index(local_start),
                    top,
                ),
                point(
                    bounds.left() + *center_offset + line.x_for_index(local_end),
                    top + line_height,
                ),
            ));
        }
        let layout = self.last_layout.as_ref()?;
        Some(Bounds::from_corners(
            point(
                bounds.left() + self.last_center_offset + layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + self.last_center_offset + layout.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        pt: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        if self.multiline && !self.last_lines.is_empty() {
            let utf8_idx = self.index_for_mouse_position(pt);
            return Some(self.offset_to_utf16(utf8_idx));
        }
        let bounds = self.last_bounds.as_ref()?;
        let layout = self.last_layout.as_ref()?;
        let local = bounds.localize(&pt)?;
        let utf8_idx = layout.index_for_x(pt.x - local.x - self.last_center_offset)?;
        Some(self.offset_to_utf16(utf8_idx))
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

// ── TextElement: custom Element that paints text + registers input handler ──

struct TextElement {
    input: Entity<TextInput>,
}

struct TextElementPrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    center_offset: Pixels,
    /// Multiline mode: shaped line, segment start byte offset, and centering
    /// offset for each visual line, plus per-line selection quads.
    lines: Vec<(ShapedLine, usize, Pixels)>,
    selections: Vec<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = TextElementPrepaintState;

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
        _cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let input = self.input.read(_cx);
        let mut height = px(input.styles.text_input_height_px);
        if input.multiline {
            height *= input.text.split('\n').count().max(1) as f32;
        }
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = height.into();
        (window.request_layout(style, [], _cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let styles = &input.styles;
        let content = input.text.clone();
        let cursor_offset = input.cursor_offset;
        let selection_anchor = input.selection_anchor;
        let placeholder = input.placeholder.clone();
        let marked_range = input.marked_range.clone();
        let cursor_opacity = if input.is_selecting {
            1.0
        } else {
            input.cursor_opacity
        };

        let text_style = window.text_style();
        let font_size = px(styles.text_input_font_size_px);

        // Multi-line layout: shape each '\n'-separated segment as its own line.
        if input.multiline && !content.is_empty() {
            let line_height = px(styles.text_input_height_px);
            let text_color = styles.text_input_text_color;
            let sel_start = cursor_offset.min(selection_anchor);
            let sel_end = cursor_offset.max(selection_anchor);
            let cursor_h_ratio = styles.text_input_cursor_height_ratio.clamp(0.3, 1.0);
            let cursor_height_adjust = px(styles.text_input_cursor_height_adjust_px);
            let cursor_h = (line_height * cursor_h_ratio + cursor_height_adjust)
                .max(px(1.0))
                .min(line_height);
            let cursor_y_inset = (line_height - cursor_h) / 2.0;
            let mut cursor_color = styles.text_input_cursor_color;
            cursor_color.a *= cursor_opacity;
            let selection_color = styles.text_input_selection_color;
            let centered = styles.text_input_center;
            let right_aligned = styles.text_input_right;

            let mut lines: Vec<(ShapedLine, usize, Pixels)> = Vec::new();
            let mut selections: Vec<PaintQuad> = Vec::new();
            let mut cursor_quad: Option<PaintQuad> = None;

            let mut seg_start = 0usize;
            for (row, part) in content.split('\n').enumerate() {
                let seg_end = seg_start + part.len();
                let base_run = TextRun {
                    len: part.len(),
                    font: text_style.font(),
                    color: text_color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let runs: Vec<TextRun> = if let Some(ref marked) = marked_range {
                    let ms = marked.start.clamp(seg_start, seg_end) - seg_start;
                    let me = marked.end.clamp(seg_start, seg_end) - seg_start;
                    if ms < me {
                        vec![
                            TextRun {
                                len: ms,
                                ..base_run.clone()
                            },
                            TextRun {
                                len: me - ms,
                                underline: Some(UnderlineStyle {
                                    color: Some(text_color),
                                    thickness: px(1.0),
                                    wavy: false,
                                }),
                                ..base_run.clone()
                            },
                            TextRun {
                                len: part.len() - me,
                                ..base_run.clone()
                            },
                        ]
                        .into_iter()
                        .filter(|run| run.len > 0)
                        .collect()
                    } else {
                        vec![base_run]
                    }
                } else {
                    vec![base_run]
                };
                let shaped = window.text_system().shape_line(
                    SharedString::from(part.to_string()),
                    font_size,
                    &runs,
                    None,
                );
                let center_offset = if centered {
                    ((bounds.size.width - shaped.x_for_index(part.len())) / 2.0).max(Pixels::ZERO)
                } else if right_aligned {
                    (bounds.size.width - shaped.x_for_index(part.len())).max(Pixels::ZERO)
                } else {
                    Pixels::ZERO
                };
                let top = bounds.top() + line_height * row as f32;

                if sel_start != sel_end && sel_start <= seg_end && sel_end >= seg_start {
                    let s = sel_start.max(seg_start) - seg_start;
                    let e = sel_end.min(seg_end) - seg_start;
                    if s < e {
                        selections.push(fill(
                            Bounds::from_corners(
                                point(
                                    bounds.left() + center_offset + shaped.x_for_index(s),
                                    top + cursor_y_inset,
                                ),
                                point(
                                    bounds.left() + center_offset + shaped.x_for_index(e),
                                    top + line_height - cursor_y_inset,
                                ),
                            ),
                            selection_color,
                        ));
                    }
                }
                if cursor_quad.is_none() && cursor_offset >= seg_start && cursor_offset <= seg_end {
                    let x = shaped.x_for_index(cursor_offset - seg_start);
                    cursor_quad = Some(fill(
                        Bounds::new(
                            point(bounds.left() + center_offset + x, top + cursor_y_inset),
                            size(px(styles.text_input_cursor_width_px), cursor_h),
                        ),
                        cursor_color,
                    ));
                }
                lines.push((shaped, seg_start, center_offset));
                seg_start = seg_end + 1;
            }

            return TextElementPrepaintState {
                line: None,
                cursor: cursor_quad,
                selection: None,
                center_offset: Pixels::ZERO,
                lines,
                selections,
            };
        }

        let (display_text, text_color) = if content.is_empty() {
            (placeholder, styles.text_input_placeholder_color)
        } else {
            (
                SharedString::from(content.clone()),
                styles.text_input_text_color,
            )
        };

        let run = TextRun {
            len: display_text.len(),
            font: text_style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let runs = if let Some(ref marked) = marked_range {
            if !content.is_empty() && marked.end <= display_text.len() {
                vec![
                    TextRun {
                        len: marked.start,
                        ..run.clone()
                    },
                    TextRun {
                        len: marked.end - marked.start,
                        underline: Some(UnderlineStyle {
                            color: Some(text_color),
                            thickness: px(1.0),
                            wavy: false,
                        }),
                        ..run.clone()
                    },
                    TextRun {
                        len: display_text.len() - marked.end,
                        ..run
                    },
                ]
                .into_iter()
                .filter(|r| r.len > 0)
                .collect()
            } else {
                vec![run]
            }
        } else {
            vec![run]
        };

        let display_text_len = display_text.len();
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        // Compute center offset for centered/right-aligned text inputs
        let center_offset = if styles.text_input_center {
            let text_width = line.x_for_index(display_text_len);
            ((bounds.size.width - text_width) / 2.0).max(Pixels::ZERO)
        } else if styles.text_input_right {
            let text_width = line.x_for_index(display_text_len);
            (bounds.size.width - text_width).max(Pixels::ZERO)
        } else {
            Pixels::ZERO
        };

        let sel_start = cursor_offset.min(selection_anchor);
        let sel_end = cursor_offset.max(selection_anchor);
        let has_selection = sel_start != sel_end && !content.is_empty();

        let mut cursor_color = styles.text_input_cursor_color;
        cursor_color.a *= cursor_opacity;
        let selection_color = styles.text_input_selection_color;

        let cursor_h_ratio = styles.text_input_cursor_height_ratio.clamp(0.3, 1.0);
        let cursor_height_adjust = px(styles.text_input_cursor_height_adjust_px);
        let cursor_h = (bounds.size.height * cursor_h_ratio + cursor_height_adjust)
            .max(px(1.0))
            .min(bounds.size.height);
        let cursor_y_inset = (bounds.size.height - cursor_h) / 2.0;

        let (selection, cursor) = if has_selection {
            let cursor_x = line.x_for_index(cursor_offset);
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + center_offset + line.x_for_index(sel_start),
                            bounds.top() + cursor_y_inset,
                        ),
                        point(
                            bounds.left() + center_offset + line.x_for_index(sel_end),
                            bounds.bottom() - cursor_y_inset,
                        ),
                    ),
                    selection_color,
                )),
                Some(fill(
                    Bounds::new(
                        point(
                            bounds.left() + center_offset + cursor_x,
                            bounds.top() + cursor_y_inset,
                        ),
                        size(px(styles.text_input_cursor_width_px), cursor_h),
                    ),
                    cursor_color,
                )),
            )
        } else if !content.is_empty() {
            let x = line.x_for_index(cursor_offset);
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(
                            bounds.left() + center_offset + x,
                            bounds.top() + cursor_y_inset,
                        ),
                        size(px(styles.text_input_cursor_width_px), cursor_h),
                    ),
                    cursor_color,
                )),
            )
        } else {
            // Empty text: cursor at start
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + center_offset, bounds.top() + cursor_y_inset),
                        size(px(styles.text_input_cursor_width_px), cursor_h),
                    ),
                    cursor_color,
                )),
            )
        };

        TextElementPrepaintState {
            line: Some(line),
            cursor,
            selection,
            center_offset,
            lines: Vec::new(),
            selections: Vec::new(),
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Register input handler - this is critical for text input to work
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );

        // Multi-line painting path.
        if !prepaint.lines.is_empty() {
            for quad in prepaint.selections.drain(..) {
                window.paint_quad(quad);
            }
            let line_height = px(self.input.read(cx).styles.text_input_height_px);
            for (row, (line, _, center_offset)) in prepaint.lines.iter().enumerate() {
                let origin = bounds.origin + point(*center_offset, line_height * row as f32);
                line.paint(origin, line_height, window, cx).ok();
            }
            if focus_handle.is_focused(window)
                && let Some(cursor) = prepaint.cursor.take()
            {
                window.paint_quad(cursor);
            }
            let lines = std::mem::take(&mut prepaint.lines);
            self.input.update(cx, |input, _cx| {
                input.last_layout = None;
                input.last_bounds = Some(bounds);
                input.last_lines = lines;
            });
            return;
        }

        // Paint selection background
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }

        // Paint text
        let center_offset = prepaint.center_offset;
        let Some(line) = prepaint.line.take() else {
            return;
        };
        let text_origin = bounds.origin + point(center_offset, Pixels::ZERO);
        line.paint(text_origin, bounds.size.height, window, cx).ok();

        // Paint cursor (only when focused)
        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        // Cache layout for mouse hit-testing
        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
            input.last_center_offset = center_offset;
        });
    }
}
