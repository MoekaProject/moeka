//! Branch picker: an anchored, searchable overlay (modeled on the language
//! picker) listing local branches with checkout on confirm, plus a synthetic
//! "create branch" row when the query does not match an existing branch.

use gpui::{
    AnyElement, Context, CursorStyle, MouseButton, Pixels, Point, Window, div, prelude::*, px,
};

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BranchPickerItem {
    Branch { name: String, is_current: bool },
    Create { name: String },
}

fn branch_matches(name: &str, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }
    name.to_ascii_lowercase().contains(&query)
}

fn is_valid_branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('.')
        && !name.ends_with('/')
        && !name.contains("..")
        && !name
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '~' | '^' | ':' | '?' | '*' | '[' | '\\'))
}

impl EditorView {
    pub(crate) fn open_branch_picker(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(base_dir) = self.git_panel_base_dir() else {
            return;
        };
        if self.git_operation_running() {
            return;
        }

        self.branch_picker_visible = true;
        self.branch_picker_anchor = Some(position);
        self.branch_picker_query.clear();
        self.branch_picker_selected_index = 0;
        self.branch_picker_all.clear();
        self.branch_picker_current = self
            .git_panel_repo_status()
            .map(|status| status.branch_label());
        self.update_branch_picker_results(cx);

        // Load the branch list off the main thread.
        self.branch_picker_task = Some(cx.spawn(async move |this, cx| {
            let branches = cx
                .background_executor()
                .spawn(async move {
                    git_repo_root(&base_dir)
                        .map(|repo_root| collect_local_branches(&repo_root))
                        .unwrap_or_default()
                })
                .await;
            this.update(cx, |this, cx| {
                this.branch_picker_task = None;
                if this.branch_picker_visible {
                    this.branch_picker_all = branches;
                    this.update_branch_picker_results(cx);
                }
            })
            .ok();
        }));

        if self.branch_picker_editor.is_none() {
            let styles = self.language_picker_text_input_styles();
            let placeholder = self.t(crate::i18n::TextKey::GitBranchPlaceholder);
            let editor = cx.new(|cx| {
                let mut input = crate::text_input::TextInput::new(styles, placeholder, cx);
                // Hosted inside a deferred overlay; a deferred context menu
                // would nest defer_draw and panic.
                input.context_menu_enabled = false;
                input
            });
            cx.subscribe(
                &editor,
                |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                    crate::text_input::TextInputEvent::Changed => {
                        this.branch_picker_query = this
                            .branch_picker_editor
                            .as_ref()
                            .map(|editor| editor.read(cx).text().to_string())
                            .unwrap_or_default();
                        this.update_branch_picker_results(cx);
                    }
                    crate::text_input::TextInputEvent::Submit
                    | crate::text_input::TextInputEvent::SubmitBackward => {
                        this.confirm_branch_picker(cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {}
                    crate::text_input::TextInputEvent::MoveUp => {
                        this.move_branch_picker_selection(-1, cx);
                    }
                    crate::text_input::TextInputEvent::MoveDown => {
                        this.move_branch_picker_selection(1, cx);
                    }
                },
            )
            .detach();
            self.branch_picker_editor = Some(editor);
        }

        if let Some(editor) = &self.branch_picker_editor {
            editor.update(cx, |input, cx| {
                input.set_text("", cx);
            });
            editor.focus_handle(cx).focus(window);
        }
        cx.notify();
    }

    pub(crate) fn close_branch_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.branch_picker_visible = false;
        self.branch_picker_anchor = None;
        self.branch_picker_task = None;
        window.focus(&self.focus_handle(cx));
        cx.notify();
    }

    fn update_branch_picker_results(&mut self, cx: &mut Context<Self>) {
        let query = self.branch_picker_query.trim().to_string();
        let current = self.branch_picker_current.clone().unwrap_or_default();
        let mut items: Vec<BranchPickerItem> = self
            .branch_picker_all
            .iter()
            .filter(|name| branch_matches(name, &query))
            .take(200)
            .map(|name| BranchPickerItem::Branch {
                name: name.clone(),
                is_current: *name == current,
            })
            .collect();

        let exact_match = self
            .branch_picker_all
            .iter()
            .any(|name| name == query.as_str());
        if !query.is_empty() && !exact_match && is_valid_branch_name(&query) {
            items.push(BranchPickerItem::Create {
                name: query.clone(),
            });
        }

        self.branch_picker_items = items;
        self.branch_picker_selected_index = 0;
        self.branch_picker_scroll_handle.scroll_to_item(0);
        cx.notify();
    }

    fn move_branch_picker_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.branch_picker_items.is_empty() {
            self.branch_picker_selected_index = 0;
            cx.notify();
            return;
        }
        let len = self.branch_picker_items.len() as isize;
        let current = self.branch_picker_selected_index as isize;
        self.branch_picker_selected_index = (current + delta).clamp(0, len - 1) as usize;
        self.branch_picker_scroll_handle
            .scroll_to_item(self.branch_picker_selected_index);
        cx.notify();
    }

    fn confirm_branch_picker(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self
            .branch_picker_items
            .get(self.branch_picker_selected_index)
            .cloned()
        else {
            self.branch_picker_visible = false;
            self.branch_picker_anchor = None;
            cx.notify();
            return;
        };
        self.branch_picker_visible = false;
        self.branch_picker_anchor = None;
        match item {
            BranchPickerItem::Branch { name, is_current } => {
                if !is_current {
                    self.run_git_operation(GitOperation::Checkout { branch: name }, cx);
                }
            }
            BranchPickerItem::Create { name } => {
                self.run_git_operation(GitOperation::CreateBranch { name }, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn render_branch_picker_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let styles = &self.styles.quick_open;
        let bg = styles.quick_open_panel_bg;
        let border = styles.quick_open_panel_border_color;
        let selected_bg = styles.quick_open_result_selected_bg;
        let hover_bg = styles.quick_open_result_hover_bg;
        let text_primary = styles.quick_open_result_text_color;
        let text_secondary = styles.quick_open_result_secondary_text_color;
        let row_h = self.scaled(24.0);
        let name_font = self.scaled(12.0);
        let input_h = self.scaled(42.0);
        let max_visible = 12usize;
        let visible_rows = self
            .branch_picker_items
            .len()
            .min(200)
            .min(max_visible)
            .max(1);
        let desired_h = input_h + visible_rows as f32 * row_h;
        let panel_w = self
            .scaled(220.0)
            .min(self.last_window_width - 16.0)
            .max(self.scaled(180.0));
        let anchor = self.branch_picker_anchor.unwrap_or_else(|| {
            gpui::point(
                px(self.last_window_width - 8.0),
                px(self.last_window_height - 8.0),
            )
        });
        let anchor_x = f32::from(anchor.x);
        let anchor_y = f32::from(anchor.y);
        let gap = self.scaled(4.0);
        let available_below = (self.last_window_height - anchor_y - gap - 8.0).max(0.0);
        let available_above = (anchor_y - gap - 8.0).max(0.0);
        let show_above = desired_h > available_below && available_above > available_below;
        let panel_h = if show_above {
            desired_h.min(available_above).max(self.scaled(100.0))
        } else {
            desired_h.min(available_below).max(self.scaled(100.0))
        };
        let panel_left =
            (anchor_x - panel_w).clamp(8.0, (self.last_window_width - panel_w - 8.0).max(8.0));
        let panel_top = if show_above {
            (anchor_y - panel_h - gap).max(8.0)
        } else {
            (anchor_y + gap).min((self.last_window_height - panel_h - 8.0).max(8.0))
        };
        let max_results_h = (panel_h - input_h).max(row_h);

        let mut rows = Vec::new();
        if self.branch_picker_items.is_empty() {
            rows.push(
                div()
                    .w_full()
                    .h(px(self.scaled(56.0)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(self.scaled(13.0)))
                    .text_color(text_secondary)
                    .child(crate::i18n::no_matches_label(self.ui_language))
                    .into_any_element(),
            );
        } else {
            for (index, item) in self.branch_picker_items.iter().enumerate() {
                let is_selected = index == self.branch_picker_selected_index;
                let (label, detail) = match item {
                    BranchPickerItem::Branch { name, is_current } => (
                        name.clone(),
                        if *is_current {
                            Some(self.t(crate::i18n::TextKey::Current).to_string())
                        } else {
                            None
                        },
                    ),
                    BranchPickerItem::Create { name } => (
                        crate::i18n::git_create_branch_label(self.ui_language, name),
                        None,
                    ),
                };
                rows.push(
                    div()
                        .id(SharedString::from(format!("branch-picker-{index}")))
                        .mx(px(1.0))
                        .h(px(row_h))
                        .px(px(self.scaled(10.0)))
                        .flex()
                        .items_center()
                        .gap(px(self.scaled(6.0)))
                        .cursor(CursorStyle::Arrow)
                        .when(is_selected, |this| this.bg(selected_bg))
                        .hover(move |style| style.bg(hover_bg))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .overflow_hidden()
                                .truncate()
                                .whitespace_nowrap()
                                .text_size(px(name_font))
                                .text_color(text_primary)
                                .child(SharedString::from(label)),
                        )
                        .when_some(detail, |this, detail| {
                            this.child(
                                div()
                                    .flex_none()
                                    .text_size(px(self.scaled(11.0)))
                                    .text_color(text_secondary)
                                    .child(SharedString::from(detail)),
                            )
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.branch_picker_selected_index = index;
                                this.confirm_branch_picker(cx);
                            }),
                        )
                        .into_any_element(),
                );
            }
        }

        div()
            .occlude()
            .absolute()
            .left(px(0.0))
            .top(px(0.0))
            .w_full()
            .h_full()
            .child(
                div()
                    .absolute()
                    .left(px(0.0))
                    .top(px(0.0))
                    .w_full()
                    .h_full()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_branch_picker(window, cx);
                        }),
                    ),
            )
            .child(
                div()
                    .id("branch-picker-overlay")
                    .absolute()
                    .top(px(panel_top))
                    .left(px(panel_left.max(0.0)))
                    .w(px(panel_w))
                    .bg(bg)
                    .border(px(1.0))
                    .border_color(border)
                    .rounded(px(self.scaled(8.0)))
                    .overflow_hidden()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                        }),
                    )
                    .child(
                        div()
                            .w_full()
                            .px(px(self.scaled(6.0)))
                            .py(px(self.scaled(6.0)))
                            .border_b(px(1.0))
                            .border_color(border)
                            .when_some(self.branch_picker_editor.clone(), |this, editor| {
                                this.child(editor)
                            }),
                    )
                    .child(
                        div()
                            .id("branch-picker-results")
                            .w_full()
                            .max_h(px(max_results_h))
                            .overflow_y_scroll()
                            .pb(px(self.scaled(8.0)))
                            .track_scroll(&self.branch_picker_scroll_handle)
                            .children(rows),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_name_validation() {
        assert!(is_valid_branch_name("feature/login"));
        assert!(is_valid_branch_name("main"));
        assert!(!is_valid_branch_name(""));
        assert!(!is_valid_branch_name("has space"));
        assert!(!is_valid_branch_name("bad..name"));
        assert!(!is_valid_branch_name("-leading-dash"));
        assert!(!is_valid_branch_name("trailing/"));
        assert!(!is_valid_branch_name("wild*card"));
    }

    #[test]
    fn branch_query_matching_is_case_insensitive() {
        assert!(branch_matches("Feature/Login", "login"));
        assert!(branch_matches("main", ""));
        assert!(!branch_matches("main", "dev"));
    }
}
