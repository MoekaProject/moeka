use std::ops::Range;
use std::time::Duration;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, GlobalElementId, Hsla, KeyDownEvent, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, Render,
    ShapedLine, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window, anchored,
    deferred, div, fill, point, prelude::*, px, relative, size,
};

use crate::style::TextInputStyles;

// ── Events ──────────────────────────────────────────────────────────

pub enum TextInputEvent {
    Changed,
    Submit,
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
    // Layout cache for mouse hit-testing
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    last_center_offset: Pixels,
    is_selecting: bool,
    // Cursor blink state
    cursor_opacity: f32,
    cursor_opacity_direction: f32,
    blink_epoch: usize,
    // Context menu
    context_menu_position: Option<Point<Pixels>>,
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
            last_layout: None,
            last_bounds: None,
            last_center_offset: Pixels::ZERO,
            is_selecting: false,
            cursor_opacity: 1.0,
            cursor_opacity_direction: -1.0,
            blink_epoch: 0,
            context_menu_position: None,
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
        self.cursor_offset = offset.min(self.text.len());
        if !extend_selection {
            self.selection_anchor = self.cursor_offset;
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.text.is_empty() {
            return 0;
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
        if offset == 0 {
            return 0;
        }
        let mut pos = offset - 1;
        while pos > 0 && !self.text.is_char_boundary(pos) {
            pos -= 1;
        }
        pos
    }

    fn next_char_boundary(&self, offset: usize) -> usize {
        if offset >= self.text.len() {
            return self.text.len();
        }
        let mut pos = offset + 1;
        while pos < self.text.len() && !self.text.is_char_boundary(pos) {
            pos += 1;
        }
        pos
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
        let menu_bg = s.text_input_focus_bg;
        let text_color = s.text_input_text_color;
        let has_sel = self.has_selection();

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
                let hover_bg = Hsla {
                    a: 0.1,
                    ..text_color
                };
                div()
                    .id(SharedString::from(format!("ctx-{id}")))
                    .w_full()
                    .h(px(26.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .text_size(px(12.0))
                    .text_color(item_text_color)
                    .rounded(px(4.0))
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
            .w(px(140.0))
            .bg(menu_bg)
            .border_1()
            .border_color(Hsla {
                a: 0.2,
                ..text_color
            })
            .rounded(px(6.0))
            .shadow_lg()
            .p(px(4.0))
            .flex()
            .flex_col()
            .gap(px(1.0))
            .text_size(px(12.0))
            .line_height(px(26.0))
            .child(item("cut", "剪切", has_sel, cx, |this, w, cx| {
                this.on_cut(&TextInputCut, w, cx);
            }))
            .child(item("copy", "复制", has_sel, cx, |this, w, cx| {
                this.on_copy(&TextInputCopy, w, cx);
            }))
            .child(item("paste", "粘贴", true, cx, |this, w, cx| {
                this.on_paste(&TextInputPaste, w, cx);
            }))
            .child(div().w_full().h(px(1.0)).my(px(2.0)).bg(Hsla {
                a: 0.1,
                ..text_color
            }))
            .child(item(
                "select-all",
                "全选",
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
        let height = s.text_input_height_px;
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
            .on_action(cx.listener(|_this, _: &TextInputPickerUp, _window, cx| {
                cx.emit(TextInputEvent::MoveUp);
            }))
            .on_action(cx.listener(|_this, _: &TextInputPickerDown, _window, cx| {
                cx.emit(TextInputEvent::MoveDown);
            }))
            .on_action(cx.listener(|_this, _: &TextInputSubmit, _window, cx| {
                cx.emit(TextInputEvent::Submit);
            }))
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
            .h(px(height))
            .pl(px(padding_left))
            .pr(px(padding_right))
            .bg(bg)
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
        let layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + layout.x_for_index(range.end),
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
        let bounds = self.last_bounds.as_ref()?;
        let layout = self.last_layout.as_ref()?;
        let local = bounds.localize(&pt)?;
        let utf8_idx = layout.index_for_x(pt.x - local.x)?;
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
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
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

        // Compute center offset for centered text inputs
        let center_offset = if styles.text_input_center {
            let text_width = line.x_for_index(display_text_len);
            ((bounds.size.width - text_width) / 2.0).max(Pixels::ZERO)
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
        let cursor_h = bounds.size.height * cursor_h_ratio;
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

        // Paint selection background
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }

        // Paint text
        let center_offset = prepaint.center_offset;
        let line = prepaint.line.take().unwrap();
        let text_origin = bounds.origin + point(center_offset, Pixels::ZERO);
        line.paint(text_origin, window.line_height(), window, cx)
            .unwrap();

        // Paint cursor (only when focused)
        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        // Cache layout for mouse hit-testing
        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
            input.last_center_offset = center_offset;
        });
    }
}
