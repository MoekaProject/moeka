use std::path::Path;

use gpui::{
    AnyElement, Context, FontWeight, MouseButton, MouseDownEvent, SharedString, div, prelude::*,
    px, svg,
};

use super::*;

impl EditorView {
    pub(crate) fn sidebar_root_name(&self) -> String {
        self.sidebar_root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.sidebar_root.display().to_string())
    }

    pub(crate) fn active_tab_index(&self) -> Option<usize> {
        self.active_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().position(|tab| &tab.id == id))
    }

    pub(crate) fn persist_active_tab_state(&mut self) {
        if let Some(index) = self.active_tab_index() {
            self.tabs[index].snapshot = self.current_snapshot();
            self.current_file_path = self.tabs[index].path.clone();
        }
    }

    pub(crate) fn activate_tab_by_index(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.persist_active_tab_state();
        let tab = self.tabs[index].clone();
        self.active_tab_id = Some(tab.id.clone());
        self.current_file_path = tab.path.clone();
        self.buffer = Buffer::from_text(&tab.snapshot.text);
        self.cursor.offset = tab.snapshot.cursor_offset.min(self.buffer.len());
        self.cursor.anchor = tab.snapshot.cursor_anchor.min(self.buffer.len());
        self.marked_range = None;
        self.scroll_offset = tab.snapshot.scroll_offset;
        self.reparse();
        self.clamp_scroll();
        self.pause_blinking(cx);
    }

    pub(crate) fn switch_to_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if self.active_tab_id.as_deref() == Some(tab_id) {
            return;
        }
        if let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) {
            self.activate_tab_by_index(index, cx);
        }
    }

    pub(crate) fn close_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        let was_active = self.active_tab_id.as_deref() == Some(tab_id);
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.active_tab_id = None;
            cx.notify();
            return;
        }
        if was_active {
            let next_index = index.min(self.tabs.len().saturating_sub(1));
            self.activate_tab_by_index(next_index, cx);
        } else {
            cx.notify();
        }
    }

    pub(crate) fn close_other_tabs(&mut self, current_tab_id: &str, cx: &mut Context<Self>) {
        self.tabs.retain(|tab| tab.id == current_tab_id);
        if let Some(index) = self.tabs.iter().position(|tab| tab.id == current_tab_id) {
            self.activate_tab_by_index(index, cx);
        }
        cx.notify();
    }

    pub(crate) fn close_right_tabs(&mut self, from_tab_id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == from_tab_id) else {
            return;
        };
        self.tabs.truncate(index + 1);
        if self.tabs.is_empty() {
            self.create_scratch_tab(cx);
            return;
        }
        if self
            .active_tab_id
            .as_deref()
            .is_none_or(|id| !self.tabs.iter().any(|t| t.id == id))
        {
            self.activate_tab_by_index(index.min(self.tabs.len().saturating_sub(1)), cx);
        }
        cx.notify();
    }

    pub(crate) fn close_saved_tabs(&mut self, cx: &mut Context<Self>) {
        let active_id = self.active_tab_id.clone();
        self.tabs
            .retain(|tab| tab.is_dirty || active_id.as_deref() == Some(tab.id.as_str()));
        if self.tabs.is_empty() {
            self.create_scratch_tab(cx);
            return;
        }
        if self
            .active_tab_id
            .as_deref()
            .is_none_or(|id| !self.tabs.iter().any(|t| t.id == id))
        {
            self.activate_tab_by_index(0, cx);
        }
        cx.notify();
    }

    pub(crate) fn reorder_tab_before(
        &mut self,
        from_id: &str,
        to_id: &str,
        cx: &mut Context<Self>,
    ) {
        if from_id == to_id {
            return;
        }
        let Some(from_idx) = self.tabs.iter().position(|tab| tab.id == from_id) else {
            return;
        };
        let Some(mut to_idx) = self.tabs.iter().position(|tab| tab.id == to_id) else {
            return;
        };
        let moved = self.tabs.remove(from_idx);
        if from_idx < to_idx {
            to_idx = to_idx.saturating_sub(1);
        }
        self.tabs.insert(to_idx, moved);
        cx.notify();
    }

    pub(crate) fn mark_active_tab_dirty(&mut self) {
        if let Some(index) = self.active_tab_index() {
            self.tabs[index].is_dirty = true;
        }
    }

    pub(crate) fn title_for_path(path: &Path) -> String {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    }

    pub(crate) fn create_scratch_tab(&mut self, cx: &mut Context<Self>) {
        self.persist_active_tab_state();
        let title = format!("Untitled {}", self.next_tab_id);
        let tab = OpenTab {
            id: self.next_tab_identity(),
            title,
            path: None,
            snapshot: EditorSnapshot {
                text: String::new(),
                cursor_offset: 0,
                cursor_anchor: 0,
                scroll_offset: 0.0,
            },
            is_dirty: false,
            is_deleted: false,
        };
        self.tabs.push(tab);
        if let Some(index) = self.tabs.len().checked_sub(1) {
            self.activate_tab_by_index(index, cx);
        }
    }

    pub(crate) fn next_tab_identity(&mut self) -> String {
        let id = format!("tab-{}", self.next_tab_id);
        self.next_tab_id += 1;
        id
    }

    // ── Tab rendering ───────────────────────────────────────────────

    pub(crate) fn render_tab(&self, tab: &OpenTab, cx: &mut Context<Self>) -> AnyElement {
        let is_active = self.active_tab_id.as_deref() == Some(tab.id.as_str());
        let is_dirty = tab.is_dirty;
        let is_deleted = tab.is_deleted;
        let tab_id = tab.id.clone();
        let close_id = tab.id.clone();
        let middle_close_id = tab.id.clone();
        let context_tab_id = tab.id.clone();
        let drop_target_id = tab.id.clone();
        let title = tab.title.clone();
        let ts = &self.styles.tab_bar;
        let tab_hover_bg = ts.tab_item_hover_bg;
        let close_hover_bg = ts.tab_item_close_hover_bg;

        div()
            .id(SharedString::from(format!("tab-{}", tab.id)))
            .group("tab-item")
            .h(px(self.tab_bar_height()))
            .min_w(px(self.scaled(ts.tab_item_min_width_px)))
            .max_w(px(self.scaled(ts.tab_item_max_width_px)))
            .pl(px(self.scaled(ts.tab_item_padding_left_px)))
            .pr(px(self.scaled(ts.tab_item_padding_right_px)))
            .flex()
            .items_center()
            .gap(px(self.scaled(ts.tab_item_gap_px)))
            .flex_none()
            .bg(if is_active {
                ts.tab_item_active_bg
            } else {
                ts.tab_item_bg
            })
            .border_r(px(ts.tab_item_border_width_px))
            .border_color(ts.tab_item_border_color)
            .hover(move |style| style.bg(tab_hover_bg))
            .child(
                svg()
                    .path(MARKDOWN_SVG_PATH)
                    .size(px(self.scaled(ts.tab_item_icon_size_px)))
                    .text_color(ts.tab_item_icon_color)
                    .opacity(if is_active {
                        ts.tab_item_icon_active_opacity
                    } else {
                        ts.tab_item_icon_inactive_opacity
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(px(self.scaled(ts.tab_item_font_size_px)))
                    .font_weight(if is_active {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if is_deleted {
                        ts.tab_item_deleted_text_color
                    } else if is_active {
                        ts.tab_item_active_text_color
                    } else {
                        ts.tab_item_text_color
                    })
                    .when(is_deleted, |this| this.line_through())
                    .child(title),
            )
            .child(
                div()
                    .id(SharedString::from(format!("close-{}", tab.id)))
                    .w(px(self.scaled(ts.tab_item_close_size_px)))
                    .h(px(self.scaled(ts.tab_item_close_size_px)))
                    .rounded_full()
                    .flex()
                    .flex_none()
                    .justify_center()
                    .items_center()
                    .when(is_dirty, |this| {
                        this.child(
                            div()
                                .w(px(self.scaled(ts.tab_item_dirty_dot_size_px)))
                                .h(px(self.scaled(ts.tab_item_dirty_dot_size_px)))
                                .rounded_full()
                                .bg(ts.tab_item_dirty_dot_color),
                        )
                    })
                    .when(!is_dirty, |this| {
                        this.text_size(px(self.scaled(ts.tab_item_close_font_size_px)))
                            .text_color(ts.tab_item_close_text_color)
                            .opacity(if is_active {
                                ts.tab_item_close_active_opacity
                            } else {
                                ts.tab_item_close_inactive_opacity
                            })
                            .group_hover("tab-item", |style| {
                                style.opacity(ts.tab_item_close_active_opacity)
                            })
                            .hover(move |style| style.bg(close_hover_bg))
                            .child("✕")
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if let Some(tab) = this.tabs.iter().find(|t| t.id == close_id) {
                                if tab.is_dirty {
                                    this.show_dialog(
                                        super::Dialog {
                                            title: "未保存的更改".to_string(),
                                            message: format!(
                                                "\"{}\" 中有未保存的更改。关闭后将丢失这些更改。",
                                                tab.title
                                            ),
                                            save_label: if tab.path.is_some() {
                                                Some("保存".to_string())
                                            } else {
                                                None
                                            },
                                            confirm_label: "不保存".to_string(),
                                            cancel_label: "取消".to_string(),
                                            is_danger: true,
                                            action: super::DialogAction::CloseTabUnsaved {
                                                tab_id: close_id.clone(),
                                            },
                                        },
                                        cx,
                                    );
                                    return;
                                }
                            }
                            this.close_tab(&close_id, cx);
                        }),
                    ),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.tab_dragging_id = Some(tab_id.clone());
                    this.switch_to_tab(&tab_id, cx);
                    cx.stop_propagation();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    if let Some(from_id) = this.tab_dragging_id.clone() {
                        this.reorder_tab_before(&from_id, &drop_target_id, cx);
                    }
                    this.tab_dragging_id = None;
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.open_tab_context_menu(context_tab_id.clone(), event.position, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.close_tab(&middle_close_id, cx);
                }),
            )
            .into_any_element()
    }

    pub(crate) fn render_tab_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let ts = &self.styles.tab_bar;
        let mut tab_items: Vec<AnyElement> = self
            .tabs
            .iter()
            .map(|tab| self.render_tab(tab, cx))
            .collect();

        if !self.sidebar_visible {
            tab_items.insert(
                0,
                div()
                    .h(px(self.tab_bar_height()))
                    .w(px(self.scaled(ts.tab_spacer_width_px)))
                    .flex_none()
                    .into_any_element(),
            );
        }

        let new_hover_color = ts.tab_new_hover_text_color;
        let new_hover_bg = ts.tab_item_hover_bg;
        tab_items.push(
            div()
                .h(px(self.tab_bar_height()))
                .w(px(self.scaled(ts.tab_new_width_px)))
                .flex_none()
                .flex()
                .justify_center()
                .items_center()
                .text_size(px(self.scaled(ts.tab_new_font_size_px)))
                .text_color(ts.tab_new_text_color)
                .hover(move |style| style.text_color(new_hover_color).bg(new_hover_bg))
                .child("+")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.create_scratch_tab(cx);
                    }),
                )
                .into_any_element(),
        );

        // Trailing spacer that fills remaining tab-bar space and handles window drag
        tab_items.push(
            div()
                .id("tab-bar-drag-spacer")
                .flex_1()
                .h_full()
                .min_w(px(0.0))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, window, cx| {
                        this.focused_sidebar_path = None;
                        if event.click_count >= 2 {
                            window.zoom_window();
                        } else {
                            super::perform_window_drag(window);
                        }
                        cx.notify();
                    }),
                )
                .into_any_element(),
        );

        div()
            .id("tab-bar")
            .w_full()
            .h(px(self.tab_bar_height()))
            .flex()
            .items_center()
            .bg(ts.tab_bar_bg)
            .border_b(px(ts.tab_bar_border_width_px))
            .border_color(ts.tab_bar_border_color)
            .overflow_x_scroll()
            .children(tab_items)
            .into_any_element()
    }
}
