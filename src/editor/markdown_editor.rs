use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, Render, Window, div, prelude::*, px,
};

use crate::actions::*;

use super::{EditorView, element};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EmbeddedMarkdownEditorEvent {
    Submit,
    NavigateUp,
    NavigateDown,
}

pub(crate) struct EmbeddedMarkdownEditor {
    editor: gpui::Entity<EditorView>,
    submit_on_enter: bool,
    /// When true, Up/Down emit navigation events (for an external popup menu)
    /// instead of moving the cursor.
    intercept_navigation: bool,
}

impl EmbeddedMarkdownEditor {
    pub(crate) fn new(
        text: &str,
        read_only: bool,
        submit_on_enter: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial_text = text.to_string();
        let editor =
            cx.new(move |cx| EditorView::new_embedded_markdown(&initial_text, read_only, cx));
        // Chain inner editor notifications outward so containers observing this
        // entity re-layout in the same frame (e.g. the agent composer height).
        cx.observe(&editor, |_, _, cx| cx.notify()).detach();
        Self {
            editor,
            submit_on_enter,
            intercept_navigation: false,
        }
    }

    pub(crate) fn text<'a>(&self, cx: &'a App) -> &'a str {
        self.editor.read(cx).buffer.text()
    }

    /// Total document height in logical pixels, used by the agent composer
    /// to grow with its content. Prefers the layout-accurate height recorded
    /// during the last prepaint so the value never flickers while typing.
    pub(crate) fn content_height(&self, cx: &App) -> f32 {
        let editor = self.editor.read(cx);
        let painted = editor.embedded_content_height.get();
        if painted > 0.0 {
            painted
        } else {
            editor.total_content_height()
        }
    }

    pub(crate) fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            editor.set_embedded_markdown_text(text, cx);
        });
    }

    pub(crate) fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_text("", cx);
    }

    /// Keep panel-embedded editors in lockstep with the host editor's zoom.
    pub(crate) fn set_ui_scale(&mut self, scale: f32, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |editor, cx| editor.set_ui_scale(scale, cx));
    }

    /// Applies the host's configured editor fonts (body Latin/CJK and code)
    /// so panel-embedded content matches the settings at runtime.
    pub(crate) fn set_editor_fonts(
        &mut self,
        body_latin: &'static str,
        body_cjk: &'static str,
        monospace: &'static str,
        ui_font: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            let editor_styles = &mut editor.styles.editor;
            if editor_styles.editor_body_latin_font_family == body_latin
                && editor_styles.editor_body_cjk_font_family == body_cjk
                && editor_styles.editor_monospace_font_family == monospace
                && editor.styles.ui_font_family == ui_font
            {
                return;
            }
            editor_styles.editor_body_latin_font_family = body_latin;
            editor_styles.editor_heading_latin_font_family = body_latin;
            editor_styles.editor_body_cjk_font_family = body_cjk;
            editor_styles.editor_heading_cjk_font_family = body_cjk;
            editor_styles.editor_monospace_font_family = monospace;
            editor.styles.ui_font_family = ui_font;
            editor.line_display_info.clear();
            editor.layout_dirty = true;
            editor.invalidate_line_geometry_cache();
            cx.notify();
        });
    }

    pub(crate) fn append_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let mut updated = self.editor.read(cx).buffer.text().to_string();
        updated.push_str(text);
        self.set_text(&updated, cx);
    }

    pub(crate) fn set_intercept_navigation(&mut self, intercept: bool) {
        self.intercept_navigation = intercept;
    }
}

impl EventEmitter<EmbeddedMarkdownEditorEvent> for EmbeddedMarkdownEditor {}

impl Focusable for EmbeddedMarkdownEditor {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.focus_handle(cx)
    }
}

impl Render for EmbeddedMarkdownEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor.clone();
        // Scrollbar thumb overlay (same look as the main editor panes).
        let scrollbar = {
            let inner = self.editor.read(cx);
            inner
                .editor_scrollbar_thumb_geometry()
                .map(|(thumb_top, thumb_height)| {
                    let mut thumb_color = inner.styles.editor.editor_scrollbar_thumb_color;
                    thumb_color.a *= inner.editor_scrollbar_opacity.clamp(0.0, 1.0);
                    (
                        thumb_top,
                        thumb_height,
                        inner.scaled(inner.editor_scrollbar_width),
                        inner.scaled(inner.styles.editor.editor_scrollbar_right_margin_px),
                        inner.scaled(inner.styles.editor.editor_scrollbar_radius_px),
                        thumb_color,
                    )
                })
        };
        div()
            .id("embedded-markdown-editor")
            .key_context("MarkdownEditor EmbeddedMarkdownEditor")
            .track_focus(&self.editor.focus_handle(cx))
            .relative()
            .size_full()
            .overflow_hidden()
            .cursor(gpui::CursorStyle::IBeam)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, event, window, cx| {
                    this.editor
                        .update(cx, |editor, cx| editor.on_mouse_down(event, window, cx));
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, event, window, cx| {
                    this.editor
                        .update(cx, |editor, cx| editor.on_mouse_up(event, window, cx));
                }),
            )
            .on_mouse_up_out(
                gpui::MouseButton::Left,
                cx.listener(|this, event, window, cx| {
                    this.editor
                        .update(cx, |editor, cx| editor.on_mouse_up(event, window, cx));
                }),
            )
            .on_mouse_move(cx.listener(|this, event, window, cx| {
                this.editor
                    .update(cx, |editor, cx| editor.on_mouse_move(event, window, cx));
            }))
            .on_scroll_wheel(cx.listener(|this, event, window, cx| {
                this.editor
                    .update(cx, |editor, cx| editor.on_scroll(event, window, cx));
            }))
            .on_action(cx.listener(|this, _: &Enter, _, cx| {
                cx.stop_propagation();
                if this.submit_on_enter
                    && this.editor.read(cx).marked_range.is_none()
                    && !this.text(cx).trim().is_empty()
                {
                    cx.emit(EmbeddedMarkdownEditorEvent::Submit);
                }
            }))
            .on_action(cx.listener(|this, action: &InsertSoftBreak, window, cx| {
                cx.stop_propagation();
                this.editor.update(cx, |editor, cx| {
                    editor.on_insert_soft_break(action, window, cx)
                });
            }))
            .on_action(cx.listener(|this, action: &Undo, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_undo(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &Redo, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_redo(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &Backspace, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_backspace(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &Delete, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_delete(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &Left, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_left(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &Right, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_right(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &Up, window, cx| {
                cx.stop_propagation();
                if this.intercept_navigation {
                    cx.emit(EmbeddedMarkdownEditorEvent::NavigateUp);
                    return;
                }
                this.editor
                    .update(cx, |editor, cx| editor.on_up(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &Down, window, cx| {
                cx.stop_propagation();
                if this.intercept_navigation {
                    cx.emit(EmbeddedMarkdownEditorEvent::NavigateDown);
                    return;
                }
                this.editor
                    .update(cx, |editor, cx| editor.on_down(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &SelectLeft, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_select_left(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &SelectRight, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_select_right(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &SelectUp, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_select_up(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &SelectDown, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_select_down(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &SelectAll, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_select_all(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &Home, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_home(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &End, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_end(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &Copy, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_copy(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &Cut, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_cut(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &Paste, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_paste(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &PastePlainText, window, cx| {
                cx.stop_propagation();
                this.editor.update(cx, |editor, cx| {
                    editor.on_paste_plain_text(action, window, cx)
                });
            }))
            .on_action(cx.listener(|this, action: &ToggleBold, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_toggle_bold(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &ToggleItalic, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_toggle_italic(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &ToggleInlineCode, window, cx| {
                cx.stop_propagation();
                this.editor.update(cx, |editor, cx| {
                    editor.on_toggle_inline_code(action, window, cx)
                });
            }))
            .on_action(cx.listener(|this, action: &Tab, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_tab(action, window, cx));
            }))
            .on_action(cx.listener(|this, action: &BackTab, window, cx| {
                cx.stop_propagation();
                this.editor
                    .update(cx, |editor, cx| editor.on_back_tab(action, window, cx));
            }))
            .child(element::EditorElement { editor, pane_id: 0 }.into_any_element())
            .when_some(
                scrollbar,
                |this, (thumb_top, thumb_height, thumb_width, right_margin, radius, color)| {
                    this.child(
                        div()
                            .absolute()
                            .top(px(thumb_top))
                            .right(px(right_margin))
                            .w(px(thumb_width))
                            .h(px(thumb_height))
                            .rounded(px(radius))
                            .bg(color)
                            .cursor(gpui::CursorStyle::Arrow)
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.editor.update(cx, |editor, cx| {
                                        editor.start_editor_scrollbar_drag(event.position, cx)
                                    });
                                }),
                            ),
                    )
                },
            )
    }
}
