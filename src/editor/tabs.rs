use std::path::Path;

use gpui::{
    AnyElement, Context, FontWeight, MouseButton, MouseDownEvent, SharedString, Window, div,
    prelude::*, px, svg,
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
        self.scroll_offset = 0.0;
        self.reparse();
        self.scroll_to_cursor();
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
            let id = self.next_tab_identity();
            let text = default_document_text().to_string();
            self.tabs.push(OpenTab {
                id: id.clone(),
                title: "Welcome".to_string(),
                path: None,
                snapshot: EditorSnapshot {
                    text,
                    cursor_offset: 0,
                    cursor_anchor: 0,
                },
                is_dirty: false,
            });
            self.active_tab_id = Some(id);
            self.activate_tab_by_index(0, cx);
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
            },
            is_dirty: false,
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
        let tab_id = tab.id.clone();
        let close_id = tab.id.clone();
        let context_tab_id = tab.id.clone();
        let drop_target_id = tab.id.clone();
        let title = if tab.is_dirty {
            format!("{} •", tab.title)
        } else {
            tab.title.clone()
        };

        div()
            .id(SharedString::from(format!("tab-{}", tab.id)))
            .h(px(self.tab_bar_height()))
            .min_w(px(self.scaled(60.0)))
            .max_w(px(self.scaled(180.0)))
            .pl(px(self.scaled(12.0)))
            .pr(px(self.scaled(8.0)))
            .flex()
            .items_center()
            .gap(px(self.scaled(6.0)))
            .flex_none()
            .bg(if is_active {
                self.theme.tab_active_bg
            } else {
                self.theme.tab_bg
            })
            .border_r_1()
            .border_color(self.theme.tab_border)
            .hover(|style| style.bg(self.theme.tab_hover))
            .child(
                svg()
                    .path(MARKDOWN_SVG_PATH)
                    .size(px(self.scaled(14.0)))
                    .text_color(if is_active {
                        self.theme.panel_muted
                    } else {
                        self.theme.panel_muted
                    })
                    .opacity(if is_active { 1.0 } else { 0.4 }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .text_size(px(self.scaled(12.0)))
                    .font_weight(if is_active {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if is_active {
                        self.theme.tab_active_text
                    } else {
                        self.theme.foreground
                    })
                    .child(title),
            )
            .child(
                div()
                    .w(px(self.scaled(16.0)))
                    .h(px(self.scaled(16.0)))
                    .rounded_full()
                    .flex()
                    .flex_none()
                    .justify_center()
                    .items_center()
                    .text_size(px(self.scaled(10.0)))
                    .text_color(self.theme.foreground)
                    .opacity(if is_active || tab.is_dirty { 1.0 } else { 0.0 })
                    .hover(|style| style.bg(self.theme.panel_hover))
                    .child(if tab.is_dirty { "●" } else { "✕" })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.close_tab(&close_id, cx);
                        }),
                    ),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.tab_dragging_id = Some(tab_id.clone());
                    this.switch_to_tab(&tab_id, cx);
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
                    this.open_tab_context_menu(context_tab_id.clone(), event.position, cx);
                }),
            )
            .into_any_element()
    }

    pub(crate) fn render_tab_bar(&self, cx: &mut Context<Self>) -> AnyElement {
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
                    .w(px(self.scaled(84.0)))
                    .flex_none()
                    .into_any_element(),
            );
        }

        tab_items.push(
            div()
                .h(px(self.tab_bar_height()))
                .w(px(self.scaled(36.0)))
                .flex_none()
                .flex()
                .justify_center()
                .items_center()
                .text_size(px(self.scaled(16.0)))
                .text_color(self.theme.panel_muted)
                .hover(|style| style.text_color(self.theme.tab_active_text))
                .child("+")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.create_scratch_tab(cx);
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
            .bg(self.theme.tab_bar_bg)
            .border_b_1()
            .border_color(self.theme.tab_border)
            .overflow_x_scroll()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, event: &MouseDownEvent, window, _cx| {
                    if event.click_count >= 2 {
                        window.zoom_window();
                    }
                }),
            )
            .children(tab_items)
            .into_any_element()
    }
}
