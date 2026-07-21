//! Branch picker: an anchored, searchable overlay (modeled on the language
//! picker) listing local branches with checkout on confirm, plus a synthetic
//! "create branch" row when the query does not match an existing branch.

use gpui::{AnyElement, Bounds, Context, Pixels, Point, Window, point, prelude::*, px, size};

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

        let branches = git_repo_root(&base_dir)
            .map(|repo_root| collect_local_branches(&repo_root))
            .unwrap_or_default();
        self.branch_picker_visible = true;
        self.branch_picker_anchor = [
            self.status_bar_branch_button_bounds.get(),
            self.git_panel_branch_button_bounds.get(),
        ]
        .into_iter()
        .flatten()
        .find(|bounds| bounds.contains(&position))
        .or_else(|| {
            Some(Bounds::new(
                point(
                    position.x,
                    px(self.last_window_height - self.status_bar_height()),
                ),
                size(px(0.0), px(self.status_bar_height())),
            ))
        });
        self.branch_picker_query.clear();
        self.branch_picker_selected_index = 0;
        self.branch_picker_all = branches;
        self.branch_picker_current = self
            .git_panel_repo_status()
            .map(|status| status.branch_label());
        self.update_branch_picker_results(cx);

        if self.branch_picker_editor.is_none() {
            let styles = self.search_menu_text_input_styles();
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
        let anchor = self.branch_picker_anchor.unwrap_or_else(|| {
            Bounds::new(
                point(px(8.0), px(self.last_window_height)),
                size(px(0.0), px(0.0)),
            )
        });
        let config = SearchMenuConfig {
            id: "branch-picker-overlay",
            anchor,
            viewport_width: self.last_window_width,
            viewport_height: self.last_window_height,
            desired_width: self.scaled(220.0),
            match_anchor_width: false,
            row_count: self.branch_picker_items.len(),
        };

        let rows = self
            .branch_picker_items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let (label, detail) = match item {
                    BranchPickerItem::Branch { name, is_current } => (
                        name.clone(),
                        is_current.then(|| self.t(crate::i18n::TextKey::Current).to_string()),
                    ),
                    BranchPickerItem::Create { name } => (
                        crate::i18n::git_create_branch_label(self.ui_language, name),
                        None,
                    ),
                };
                let row = SearchMenuRow::new(label)
                    .detail(detail)
                    .selected(index == self.branch_picker_selected_index);
                self.render_search_menu_row(
                    SharedString::from(format!("branch-picker-{index}")),
                    row,
                    move |this, _, cx| {
                        this.branch_picker_selected_index = index;
                        this.confirm_branch_picker(cx);
                    },
                    cx,
                )
            })
            .collect();

        self.render_search_menu_overlay(
            &config,
            self.branch_picker_editor.clone(),
            Some(&self.branch_picker_scroll_handle),
            None,
            rows,
            |this, window, cx| this.close_branch_picker(window, cx),
            cx,
        )
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
