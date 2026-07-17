//! Git panel UI: the right utility panel's Git mode with a Changes tab
//! (status sections, stage/unstage/discard, commit box) and a History tab
//! (repository or current-file scope, expandable commit file lists), plus the
//! "set remote" input overlay used by the sync flow.

use std::path::PathBuf;

use gpui::{
    AnyElement, Context, CursorStyle, FontWeight, MouseButton, MouseDownEvent, SharedString, div,
    prelude::*, px, svg,
};

use super::*;

#[derive(Clone)]
enum GitChangesRow {
    SectionHeader {
        title: String,
        count: usize,
        action: Option<GitSectionHeaderAction>,
    },
    Entry(GitChangeEntry),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GitSectionHeaderAction {
    StageAll,
    UnstageAll,
}

#[derive(Clone)]
enum GitHistoryRow {
    Commit(GitHistoryItem),
    CommitFile(GitCommitFileEntry),
    CommitFilesLoading,
}

impl EditorView {
    fn git_change_kind_color(&self, kind: GitChangeKind) -> gpui::Hsla {
        match kind {
            GitChangeKind::Added => self.theme.git_status_added,
            GitChangeKind::Modified | GitChangeKind::Renamed => self.theme.git_status_modified,
            GitChangeKind::Deleted | GitChangeKind::Conflicted => self.theme.text_danger,
        }
    }

    // ── Panel scaffold ─────────────────────────────────────────────

    pub(crate) fn render_git_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.refresh_git_panel_status_if_needed(cx);

        let ups = &self.styles.utility_panel;
        let top_spacer = self.scaled(ups.utility_panel_top_spacer_height_px);
        let header_height = self.scaled(ups.utility_panel_header_height_px);
        let header_padding_x = self.scaled(ups.utility_panel_header_padding_x_px);
        let header_font_size = self.scaled(ups.utility_panel_header_font_size_px);
        let header_text_color = ups.utility_panel_header_text_color;
        let panel_bg = ups.utility_panel_bg;
        let panel_border_width = ups.utility_panel_border_width_px;
        let panel_border_color = ups.utility_panel_border_color;

        let header = div()
            .h(px(header_height))
            .flex_shrink_0()
            .flex()
            .items_center()
            .px(px(header_padding_x))
            .gap(px(self.scaled(6.0)))
            .text_size(px(header_font_size))
            .text_color(header_text_color)
            .child(self.t(crate::i18n::TextKey::GitPanelTitle))
            .child(div().flex_1())
            .child(self.render_git_branch_button(cx))
            .child(self.render_git_sync_button(cx))
            .into_any_element();

        let body = match self.git_panel_tab {
            GitPanelTab::Changes => self.render_git_changes_body(cx),
            GitPanelTab::History => self.render_git_history_body(cx),
        };

        let commit_area = if self.git_panel_tab == GitPanelTab::Changes
            && self.git_panel_repo_status().is_some()
        {
            Some(self.render_git_commit_area(cx))
        } else {
            None
        };

        div()
            .id("git-panel")
            .h_full()
            .w_full()
            .bg(panel_bg)
            .relative()
            .overflow_hidden()
            .border_l(px(panel_border_width))
            .border_color(panel_border_color)
            .flex()
            .flex_col()
            .child(div().h(px(top_spacer)).flex_shrink_0())
            .child(header)
            .child(self.render_git_panel_tabs(cx))
            .child(body)
            .when_some(commit_area, |this, commit_area| this.child(commit_area))
            .into_any_element()
    }

    fn render_git_panel_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let font_size = self.scaled(ups.utility_panel_item_font_size_px);
        let section_padding_x = self.scaled(ups.utility_panel_section_padding_x_px);
        let hover_bg = ups.utility_panel_button_hover_bg;
        let segment = |label: &str, tab: GitPanelTab, active: bool| {
            div()
                .id(match tab {
                    GitPanelTab::Changes => "git-panel-tab-changes",
                    GitPanelTab::History => "git-panel-tab-history",
                })
                .flex_1()
                .h(px(self.scaled(22.0)))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(self.scaled(5.0)))
                .cursor(CursorStyle::Arrow)
                .text_size(px(font_size))
                .text_color(if active {
                    ups.utility_panel_item_value_text_color
                } else {
                    ups.utility_panel_item_label_text_color
                })
                .when(active, |this| this.bg(hover_bg))
                .when(!active, |this| this.hover(move |style| style.bg(hover_bg)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if this.git_panel_tab != tab {
                            this.git_panel_tab = tab;
                            cx.notify();
                        }
                    }),
                )
                .child(SharedString::from(label.to_string()))
        };

        div()
            .flex_shrink_0()
            .mx(px(section_padding_x))
            .my(px(self.scaled(6.0)))
            .p(px(self.scaled(2.0)))
            .flex()
            .items_center()
            .gap(px(self.scaled(2.0)))
            .rounded(px(self.scaled(6.0)))
            .border(px(1.0))
            .border_color(ups.utility_panel_border_color)
            .child(segment(
                self.t(crate::i18n::TextKey::GitChangesTab),
                GitPanelTab::Changes,
                self.git_panel_tab == GitPanelTab::Changes,
            ))
            .child(segment(
                self.t(crate::i18n::TextKey::GitHistoryTab),
                GitPanelTab::History,
                self.git_panel_tab == GitPanelTab::History,
            ))
            .into_any_element()
    }

    fn render_git_branch_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let Some(status) = self.git_panel_repo_status() else {
            return div().into_any_element();
        };
        let branch_label = status.branch_label();
        if branch_label.is_empty() {
            return div().into_any_element();
        }
        let font_size = self.scaled(ups.utility_panel_item_font_size_px);
        let busy = self.git_operation_running();
        let button = div()
            .id("git-panel-branch")
            .flex_none()
            .max_w(px(self.scaled(110.0)))
            .h(px(self.scaled(20.0)))
            .px(px(self.scaled(5.0)))
            .flex()
            .items_center()
            .gap(px(self.scaled(3.0)))
            .rounded(px(self.scaled(5.0)))
            .cursor(CursorStyle::Arrow)
            .text_size(px(font_size))
            .text_color(ups.utility_panel_item_value_text_color)
            .opacity(if busy { 0.5 } else { 1.0 })
            .child(
                svg()
                    .path(GIT_BRANCH_SVG_PATH)
                    .size(px(font_size))
                    .flex_none()
                    .text_color(ups.utility_panel_item_value_text_color),
            )
            .child(
                div()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .truncate()
                    .whitespace_nowrap()
                    .child(SharedString::from(branch_label)),
            );
        let button = if busy {
            button
        } else {
            let hover_bg = ups.utility_panel_button_hover_bg;
            button.hover(move |style| style.bg(hover_bg)).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_branch_picker(event.position, window, cx);
                }),
            )
        };
        tooltip::with_tooltip(
            button,
            cx.entity().clone(),
            self.t(crate::i18n::TextKey::GitSwitchBranch).to_string(),
            None::<&'static str>,
        )
    }

    fn render_git_sync_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let Some(status) = self.git_panel_repo_status() else {
            return div().into_any_element();
        };
        if !status.has_head {
            return div().into_any_element();
        }
        let font_size = self.scaled(ups.utility_panel_item_font_size_px);
        let busy = self.git_operation_running();
        let badge = if busy {
            "…".to_string()
        } else if status.has_upstream {
            match (status.ahead, status.behind) {
                (0, 0) => String::new(),
                (ahead, 0) => format!("↑{ahead}"),
                (0, behind) => format!("↓{behind}"),
                (ahead, behind) => format!("↑{ahead} ↓{behind}"),
            }
        } else {
            String::new()
        };
        let tooltip_text = if !status.has_remote {
            self.t(crate::i18n::TextKey::GitSetRemoteTitle)
        } else if status.has_upstream {
            self.t(crate::i18n::TextKey::GitSync)
        } else {
            self.t(crate::i18n::TextKey::GitSyncPublish)
        };
        let button = div()
            .id("git-panel-sync")
            .flex_none()
            .h(px(self.scaled(20.0)))
            .px(px(self.scaled(5.0)))
            .flex()
            .items_center()
            .gap(px(self.scaled(3.0)))
            .rounded(px(self.scaled(5.0)))
            .cursor(CursorStyle::Arrow)
            .text_size(px(font_size))
            .text_color(ups.utility_panel_item_value_text_color)
            .opacity(if busy { 0.5 } else { 1.0 })
            .child(
                svg()
                    .path(GIT_SYNC_SVG_PATH)
                    .size(px(font_size))
                    .flex_none()
                    .text_color(ups.utility_panel_item_value_text_color),
            )
            .when(!badge.is_empty(), |this| {
                this.child(div().whitespace_nowrap().child(SharedString::from(badge)))
            });
        let button = if busy {
            button
        } else {
            let hover_bg = ups.utility_panel_button_hover_bg;
            button.hover(move |style| style.bg(hover_bg)).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.run_git_operation(GitOperation::Sync, cx);
                }),
            )
        };
        tooltip::with_tooltip(
            button,
            cx.entity().clone(),
            tooltip_text.to_string(),
            None::<&'static str>,
        )
    }

    // ── Changes tab ────────────────────────────────────────────────

    fn git_changes_rows(
        status: &GitPanelStatus,
        t: impl Fn(crate::i18n::TextKey) -> &'static str,
    ) -> Vec<GitChangesRow> {
        let mut rows = Vec::new();
        if !status.conflicted.is_empty() {
            rows.push(GitChangesRow::SectionHeader {
                title: t(crate::i18n::TextKey::GitConflictsSection).to_string(),
                count: status.conflicted.len(),
                action: None,
            });
            rows.extend(status.conflicted.iter().cloned().map(GitChangesRow::Entry));
        }
        if !status.staged.is_empty() {
            rows.push(GitChangesRow::SectionHeader {
                title: t(crate::i18n::TextKey::GitStagedLabel).to_string(),
                count: status.staged.len(),
                action: Some(GitSectionHeaderAction::UnstageAll),
            });
            rows.extend(status.staged.iter().cloned().map(GitChangesRow::Entry));
        }
        if !status.unstaged.is_empty() {
            rows.push(GitChangesRow::SectionHeader {
                title: t(crate::i18n::TextKey::GitChangesSection).to_string(),
                count: status.unstaged.len(),
                action: Some(GitSectionHeaderAction::StageAll),
            });
            rows.extend(status.unstaged.iter().cloned().map(GitChangesRow::Entry));
        }
        rows
    }

    fn render_git_changes_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let section_gap = self.scaled(ups.utility_panel_section_gap_px);
        let section_padding_x = self.scaled(ups.utility_panel_section_padding_x_px);
        let empty_font_size = self.scaled(ups.utility_panel_empty_font_size_px);
        let item_height = self.scaled(ups.utility_panel_item_height_px).max(1.0);
        let bottom_spacer = self.scaled(ups.utility_panel_bottom_spacer_height_px);

        let mut children: Vec<AnyElement> = Vec::new();

        match self.git_panel_snapshot.clone() {
            Some(GitPanelSnapshot::Repo(status)) if !status.is_clean() => {
                let language = self.ui_language;
                let rows =
                    Self::git_changes_rows(&status, |key| crate::i18n::translate(language, key));
                let list_window = super::list_viewport::fixed_list_window(
                    &self.git_changes_scroll_handle,
                    self.last_window_height,
                    0.0,
                    item_height,
                    rows.len(),
                    12,
                    24,
                );
                if list_window.top_spacer_px > 0.0 {
                    children.push(
                        div()
                            .h(px(list_window.top_spacer_px))
                            .flex_shrink_0()
                            .into_any_element(),
                    );
                }
                for row in &rows[list_window.render] {
                    children.push(match row {
                        GitChangesRow::SectionHeader {
                            title,
                            count,
                            action,
                        } => self.render_git_section_header(title, *count, *action, cx),
                        GitChangesRow::Entry(entry) => {
                            self.render_git_change_row(entry, &status.repo_root, cx)
                        }
                    });
                }
                if list_window.bottom_spacer_px > 0.0 {
                    children.push(
                        div()
                            .h(px(list_window.bottom_spacer_px))
                            .flex_shrink_0()
                            .into_any_element(),
                    );
                }
            }
            Some(GitPanelSnapshot::Repo(_)) => {
                children.push(
                    div()
                        .px(px(section_padding_x))
                        .pt(px(section_gap))
                        .text_size(px(empty_font_size))
                        .text_color(ups.utility_panel_empty_text_color)
                        .child(SharedString::from(
                            self.t(crate::i18n::TextKey::GitNoChanges).to_string(),
                        ))
                        .into_any_element(),
                );
            }
            Some(GitPanelSnapshot::NoRepo { base_dir }) => {
                children.push(self.render_git_init_prompt(base_dir, cx));
            }
            None => {
                children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
            }
        }

        children.push(
            div()
                .h(px(bottom_spacer))
                .flex_shrink_0()
                .into_any_element(),
        );

        let scrollbar = self.render_scrollbar_for_handle(
            PanelScrollbarKind::GitChanges,
            &self.git_changes_scroll_handle,
            f32::from(self.git_changes_scroll_handle.bounds().size.height),
            0.0,
            self.scaled(self.styles.editor.editor_scrollbar_right_margin_px),
            cx,
        );

        div()
            .id("git-changes-body")
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .relative()
            .overflow_hidden()
            .child(
                div()
                    .id("git-changes-scroll")
                    .h_full()
                    .w_full()
                    .overflow_y_scroll()
                    .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                        this.reveal_panel_scrollbar(PanelScrollbarKind::GitChanges, cx);
                    }))
                    .track_scroll(&self.git_changes_scroll_handle)
                    .flex()
                    .flex_col()
                    .children(children),
            )
            .when_some(scrollbar, |this, scrollbar| this.child(scrollbar))
            .into_any_element()
    }

    fn render_git_init_prompt(&self, base_dir: PathBuf, cx: &mut Context<Self>) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let section_gap = self.scaled(ups.utility_panel_section_gap_px);
        let section_padding_x = self.scaled(ups.utility_panel_section_padding_x_px);
        let empty_font_size = self.scaled(ups.utility_panel_empty_font_size_px);
        let busy = self.git_operation_running();
        let hover_bg = ups.utility_panel_button_hover_bg;
        let button = div()
            .id("git-init-button")
            .min_h(px(self.scaled(ups.utility_panel_button_height_px)))
            .px(px(self.scaled(ups.utility_panel_button_padding_x_px)))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(self.scaled(ups.utility_panel_button_radius_px)))
            .border(px(self.scaled(ups.utility_panel_button_border_width_px)))
            .border_color(ups.utility_panel_button_border_color)
            .text_size(px(self.scaled(ups.utility_panel_item_font_size_px)))
            .text_color(ups.utility_panel_button_text_color)
            .cursor(CursorStyle::Arrow)
            .opacity(if busy { 0.45 } else { 1.0 })
            .child(SharedString::from(
                self.t(crate::i18n::TextKey::GitInitRepo).to_string(),
            ));
        let button = if busy {
            button
        } else {
            button.hover(move |style| style.bg(hover_bg)).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.run_git_operation(
                        GitOperation::Init {
                            base_dir: base_dir.clone(),
                        },
                        cx,
                    );
                }),
            )
        };

        div()
            .px(px(section_padding_x))
            .pt(px(section_gap))
            .flex()
            .flex_col()
            .gap(px(section_gap))
            .child(
                div()
                    .text_size(px(empty_font_size))
                    .text_color(ups.utility_panel_empty_text_color)
                    .child(SharedString::from(
                        self.t(crate::i18n::TextKey::GitNotARepo).to_string(),
                    )),
            )
            .child(button)
            .into_any_element()
    }

    fn render_git_section_header(
        &self,
        title: &str,
        count: usize,
        action: Option<GitSectionHeaderAction>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let item_height = self.scaled(ups.utility_panel_item_height_px).max(1.0);
        let font_size = self.scaled(ups.utility_panel_section_title_font_size_px);
        let busy = self.git_operation_running();
        let action_label = match action {
            Some(GitSectionHeaderAction::StageAll) => {
                Some(self.t(crate::i18n::TextKey::GitStageAll))
            }
            Some(GitSectionHeaderAction::UnstageAll) => {
                Some(self.t(crate::i18n::TextKey::GitUnstageAll))
            }
            None => None,
        };
        let hover_bg = ups.utility_panel_button_hover_bg;

        div()
            .h(px(item_height))
            .px(px(self.scaled(ups.utility_panel_section_padding_x_px)))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(self.scaled(6.0)))
            .text_size(px(font_size))
            .text_color(ups.utility_panel_section_title_text_color)
            .font_weight(FontWeight::BOLD)
            .child(SharedString::from(format!("{title} ({count})")))
            .child(div().flex_1())
            .when_some(action_label, |this, label| {
                let action = action.expect("label implies action");
                let button = div()
                    .id(match action {
                        GitSectionHeaderAction::StageAll => "git-stage-all",
                        GitSectionHeaderAction::UnstageAll => "git-unstage-all",
                    })
                    .px(px(self.scaled(5.0)))
                    .py(px(self.scaled(1.0)))
                    .rounded(px(self.scaled(4.0)))
                    .cursor(CursorStyle::Arrow)
                    .font_weight(FontWeight::NORMAL)
                    .text_color(ups.utility_panel_item_label_text_color)
                    .opacity(if busy { 0.45 } else { 1.0 })
                    .child(SharedString::from(label.to_string()));
                if busy {
                    this.child(button)
                } else {
                    this.child(button.hover(move |style| style.bg(hover_bg)).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            let operation = match action {
                                GitSectionHeaderAction::StageAll => GitOperation::StageAll,
                                GitSectionHeaderAction::UnstageAll => GitOperation::UnstageAll,
                            };
                            this.run_git_operation(operation, cx);
                        }),
                    ))
                }
            })
            .into_any_element()
    }

    fn git_change_row_is_active(&self, entry: &GitChangeEntry, absolute_path: &PathBuf) -> bool {
        let Some(index) = self.active_tab_index() else {
            return false;
        };
        let Some(tab) = self.tabs.get(index) else {
            return false;
        };
        if tab.kind != TabKind::Diff || tab.path.as_ref() != Some(absolute_path) {
            return false;
        }
        tab.git_diff.as_ref().is_some_and(|diff| match diff.source {
            GitDiffSource::HeadToIndex => entry.staged,
            GitDiffSource::IndexToWorktree => !entry.staged,
            _ => false,
        })
    }

    fn render_git_change_row(
        &self,
        entry: &GitChangeEntry,
        repo_root: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let item_height = self.scaled(ups.utility_panel_item_height_px).max(1.0);
        let font_size = self.scaled(ups.utility_panel_item_font_size_px);
        let icon_size = font_size;
        let busy = self.git_operation_running();
        let hover_bg = ups.utility_panel_button_hover_bg;
        let absolute_path = repo_root.join(&entry.rel_path);
        let is_active = self.git_change_row_is_active(entry, &absolute_path);
        let letter_color = self.git_change_kind_color(entry.kind);
        let parent_dir = entry.parent_dir().to_string();

        let action_button = |id: &'static str,
                             icon_path: &'static str,
                             tooltip_key: crate::i18n::TextKey,
                             on_click: std::rc::Rc<dyn Fn(&mut Self, &mut Context<Self>)>|
         -> AnyElement {
            let button = div()
                .id(SharedString::from(format!("{id}-{}", entry.rel_path)))
                .flex_none()
                .w(px(icon_size + self.scaled(6.0)))
                .h(px(icon_size + self.scaled(6.0)))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(self.scaled(4.0)))
                .cursor(CursorStyle::Arrow)
                .text_color(ups.utility_panel_item_value_text_color)
                .opacity(if busy { 0.3 } else { 0.55 })
                .child(
                    svg()
                        .path(icon_path)
                        .size(px(icon_size))
                        .text_color(ups.utility_panel_item_value_text_color),
                );
            let button = if busy {
                button
            } else {
                button
                    .hover(move |style| style.bg(hover_bg).opacity(0.95))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            on_click(this, cx);
                        }),
                    )
            };
            tooltip::with_tooltip(
                button,
                cx.entity().clone(),
                self.t(tooltip_key).to_string(),
                None::<&'static str>,
            )
        };

        let mut actions: Vec<AnyElement> = Vec::new();
        if entry.staged {
            let paths = entry_stage_paths(entry);
            actions.push(action_button(
                "git-unstage",
                GIT_UNSTAGE_SVG_PATH,
                crate::i18n::TextKey::GitUnstageFile,
                std::rc::Rc::new(move |this, cx| {
                    this.run_git_operation(GitOperation::UnstagePaths(paths.clone()), cx);
                }),
            ));
        } else {
            let discard_entry = entry.clone();
            actions.push(action_button(
                "git-discard",
                MENU_DELETE_SVG_PATH,
                crate::i18n::TextKey::GitDiscardFile,
                std::rc::Rc::new(move |this, cx| {
                    this.request_git_discard_entry(&discard_entry, cx);
                }),
            ));
            let paths = entry_stage_paths(entry);
            actions.push(action_button(
                "git-stage",
                GIT_STAGE_SVG_PATH,
                crate::i18n::TextKey::GitStageFile,
                std::rc::Rc::new(move |this, cx| {
                    this.run_git_operation(GitOperation::StagePaths(paths.clone()), cx);
                }),
            ));
        }

        let click_entry = entry.clone();
        let click_root = repo_root.to_path_buf();
        div()
            .h(px(item_height))
            .px(px(self.scaled(ups.utility_panel_section_padding_x_px)))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(self.scaled(6.0)))
            .text_size(px(font_size))
            .cursor(CursorStyle::Arrow)
            .bg(if is_active {
                hover_bg
            } else {
                gpui::transparent_black()
            })
            .hover(move |style| style.bg(hover_bg))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_git_change_diff(&click_entry, &click_root, cx);
                }),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(font_size * 0.9))
                    .text_color(letter_color)
                    .font_weight(FontWeight::BOLD)
                    .child(SharedString::from(entry.kind.letter().to_string())),
            )
            .child(
                div()
                    .flex_none()
                    .max_w(px(self.scaled(150.0)))
                    .overflow_hidden()
                    .truncate()
                    .whitespace_nowrap()
                    .text_color(ups.utility_panel_item_value_text_color)
                    .child(SharedString::from(entry.file_name().to_string())),
            )
            .when(!parent_dir.is_empty(), |this| {
                this.child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .truncate()
                        .whitespace_nowrap()
                        .text_color(ups.utility_panel_item_label_text_color)
                        .child(SharedString::from(parent_dir.clone())),
                )
            })
            .when(parent_dir.is_empty(), |this| this.child(div().flex_1()))
            .children(actions)
            .into_any_element()
    }

    fn open_git_change_diff(
        &mut self,
        entry: &GitChangeEntry,
        repo_root: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        let absolute_path = repo_root.join(&entry.rel_path);
        if entry.kind == GitChangeKind::Conflicted {
            self.open_path_in_preview_tab(absolute_path, true, cx);
            return;
        }
        let source = if entry.staged {
            GitDiffSource::HeadToIndex
        } else {
            GitDiffSource::IndexToWorktree
        };
        let Some(diff) = self.build_git_diff_state_for_source(&absolute_path, source) else {
            return;
        };
        let title = Self::title_for_path(&absolute_path);
        self.open_git_diff_tab(title, absolute_path, diff, cx);
    }

    fn request_git_discard_entry(&mut self, entry: &GitChangeEntry, cx: &mut Context<Self>) {
        let (tracked, untracked) = if entry.untracked {
            (Vec::new(), vec![entry.rel_path.clone()])
        } else {
            (vec![entry.rel_path.clone()], Vec::new())
        };
        let message = crate::i18n::git_discard_dialog_message(
            self.ui_language,
            entry.file_name(),
            entry.untracked,
        );
        self.show_dialog(
            Dialog {
                title: self.t(crate::i18n::TextKey::GitDiscardFile).to_string(),
                message,
                save_label: None,
                confirm_label: self.t(crate::i18n::TextKey::GitDiscardFile).to_string(),
                cancel_label: self.t(crate::i18n::TextKey::Cancel).to_string(),
                is_danger: true,
                action: DialogAction::GitDiscardEntries { tracked, untracked },
            },
            cx,
        );
    }

    // ── Commit area ────────────────────────────────────────────────

    fn ensure_git_commit_editor(&mut self, cx: &mut Context<Self>) {
        if self.git_commit_message_editor.is_some() {
            return;
        }
        let mut styles = self.language_picker_text_input_styles();
        let ups = &self.styles.utility_panel;
        styles.text_input_bg = ups.utility_panel_bg;
        styles.text_input_focus_bg = ups.utility_panel_bg;
        styles.text_input_hover_bg = Some(ups.utility_panel_bg);
        let placeholder = self.t(crate::i18n::TextKey::GitCommitPlaceholder);
        let editor = cx.new(|cx| {
            let mut input = crate::text_input::TextInput::new(styles, placeholder, cx);
            input.multiline = true;
            input
        });
        cx.subscribe(
            &editor,
            |this, _editor, event: &crate::text_input::TextInputEvent, cx| {
                if matches!(event, crate::text_input::TextInputEvent::Changed) {
                    this.git_commit_message = this
                        .git_commit_message_editor
                        .as_ref()
                        .map(|editor| editor.read(cx).text().to_string())
                        .unwrap_or_default();
                    cx.notify();
                }
            },
        )
        .detach();
        self.git_commit_message_editor = Some(editor);
    }

    fn render_git_commit_area(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_git_commit_editor(cx);
        let ups = &self.styles.utility_panel;
        let section_padding_x = self.scaled(ups.utility_panel_section_padding_x_px);
        let can_commit = self
            .git_panel_repo_status()
            .is_some_and(|status| !status.staged.is_empty())
            && !self.git_commit_message.trim().is_empty()
            && !self.git_operation_running();
        let hover_bg = ups.utility_panel_button_hover_bg;

        let commit_button = div()
            .id("git-commit-button")
            .min_h(px(self.scaled(ups.utility_panel_button_height_px)))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(self.scaled(ups.utility_panel_button_radius_px)))
            .border(px(self.scaled(ups.utility_panel_button_border_width_px)))
            .border_color(ups.utility_panel_button_border_color)
            .text_size(px(self.scaled(ups.utility_panel_item_font_size_px)))
            .text_color(ups.utility_panel_button_text_color)
            .cursor(CursorStyle::Arrow)
            .opacity(if can_commit { 1.0 } else { 0.45 })
            .child(SharedString::from(
                self.t(crate::i18n::TextKey::GitCommit).to_string(),
            ));
        let commit_button = if can_commit {
            commit_button
                .hover(move |style| style.bg(hover_bg))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        let message = this.git_commit_message.trim().to_string();
                        if message.is_empty() {
                            return;
                        }
                        this.run_git_operation(GitOperation::Commit { message }, cx);
                    }),
                )
        } else {
            commit_button
        };

        div()
            .flex_shrink_0()
            .w_full()
            .px(px(section_padding_x))
            .py(px(self.scaled(8.0)))
            .border_t(px(1.0))
            .border_color(ups.utility_panel_border_color)
            .flex()
            .flex_col()
            .gap(px(self.scaled(6.0)))
            .child(
                div()
                    .w_full()
                    .rounded(px(self.scaled(6.0)))
                    .border(px(1.0))
                    .border_color(ups.utility_panel_border_color)
                    .px(px(self.scaled(4.0)))
                    .py(px(self.scaled(2.0)))
                    .when_some(self.git_commit_message_editor.clone(), |this, editor| {
                        this.child(editor)
                    }),
            )
            .child(commit_button)
            .into_any_element()
    }

    // ── History tab ────────────────────────────────────────────────

    fn render_git_history_scope_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let font_size = self.scaled(ups.utility_panel_item_font_size_px * 0.92);
        let hover_bg = ups.utility_panel_button_hover_bg;
        let has_file = self.current_file_path.is_some();
        let scope_button =
            |id: &'static str, label: &'static str, scope: GitHistoryScope, enabled: bool| {
                let active = self.git_history_scope == scope;
                let button = div()
                    .id(id)
                    .px(px(self.scaled(6.0)))
                    .py(px(self.scaled(1.0)))
                    .rounded(px(self.scaled(4.0)))
                    .cursor(CursorStyle::Arrow)
                    .text_size(px(font_size))
                    .text_color(if active {
                        ups.utility_panel_item_value_text_color
                    } else {
                        ups.utility_panel_item_label_text_color
                    })
                    .when(active, |this| this.bg(hover_bg))
                    .opacity(if enabled { 1.0 } else { 0.45 })
                    .child(SharedString::from(label.to_string()));
                if enabled && !active {
                    button
                        .hover(move |style| style.bg(hover_bg))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.set_git_history_scope(scope, cx);
                            }),
                        )
                        .into_any_element()
                } else {
                    button.into_any_element()
                }
            };

        div()
            .flex_shrink_0()
            .px(px(self.scaled(ups.utility_panel_section_padding_x_px)))
            .pb(px(self.scaled(4.0)))
            .flex()
            .items_center()
            .gap(px(self.scaled(4.0)))
            .child(scope_button(
                "git-history-scope-all",
                self.t(crate::i18n::TextKey::GitScopeAll),
                GitHistoryScope::Repository,
                true,
            ))
            .child(scope_button(
                "git-history-scope-file",
                self.t(crate::i18n::TextKey::GitScopeCurrentFile),
                GitHistoryScope::CurrentFile,
                has_file,
            ))
            .into_any_element()
    }

    fn set_git_history_scope(&mut self, scope: GitHistoryScope, cx: &mut Context<Self>) {
        if self.git_history_scope == scope {
            return;
        }
        self.git_history_scope = scope;
        self.git_history_expanded_commit = None;
        self.invalidate_git_history_cache();
        cx.notify();
    }

    fn git_history_rows(&self, items: &[GitHistoryItem]) -> Vec<GitHistoryRow> {
        let mut rows = Vec::with_capacity(items.len());
        for item in items {
            rows.push(GitHistoryRow::Commit(item.clone()));
            if self.git_history_scope == GitHistoryScope::Repository
                && let (Some(expanded), Some(hash)) = (
                    self.git_history_expanded_commit.as_deref(),
                    item.commit_hash.as_deref(),
                )
                && expanded == hash
            {
                match self.git_commit_files.as_ref() {
                    Some((cached_hash, files)) if cached_hash == hash => {
                        rows.extend(files.iter().cloned().map(GitHistoryRow::CommitFile));
                    }
                    _ => rows.push(GitHistoryRow::CommitFilesLoading),
                }
            }
        }
        rows
    }

    fn render_git_history_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let history_items = self.cached_git_history_items(cx);
        let ups = &self.styles.utility_panel;
        let section_gap = self.scaled(ups.utility_panel_section_gap_px);
        let section_padding_x = self.scaled(ups.utility_panel_section_padding_x_px);
        let empty_font_size = self.scaled(ups.utility_panel_empty_font_size_px);
        let bottom_spacer = self.scaled(ups.utility_panel_bottom_spacer_height_px);
        let mut should_load_next_page = false;
        let mut children: Vec<AnyElement> = Vec::new();

        match history_items {
            Some(items) if !items.is_empty() => {
                let rows = self.git_history_rows(&items);
                let item_height = self.scaled(ups.utility_panel_item_font_size_px * 3.4);
                let list_window = super::list_viewport::fixed_list_window(
                    &self.git_history_scroll_handle,
                    self.last_window_height,
                    0.0,
                    item_height,
                    rows.len(),
                    12,
                    24,
                );
                if list_window.preload.end >= rows.len().saturating_sub(1) {
                    should_load_next_page = true;
                }
                if list_window.top_spacer_px > 0.0 {
                    children.push(
                        div()
                            .h(px(list_window.top_spacer_px))
                            .flex_shrink_0()
                            .into_any_element(),
                    );
                }
                for row in &rows[list_window.render] {
                    children.push(match row {
                        GitHistoryRow::Commit(item) => {
                            self.render_git_history_row(item, item_height, cx)
                        }
                        GitHistoryRow::CommitFile(entry) => {
                            self.render_git_commit_file_row(entry, item_height, cx)
                        }
                        GitHistoryRow::CommitFilesLoading => {
                            div().h(px(item_height)).flex_shrink_0().into_any_element()
                        }
                    });
                }
                if list_window.bottom_spacer_px > 0.0 {
                    children.push(
                        div()
                            .h(px(list_window.bottom_spacer_px))
                            .flex_shrink_0()
                            .into_any_element(),
                    );
                }
            }
            Some(_) => {
                children.push(
                    div()
                        .px(px(section_padding_x))
                        .pt(px(section_gap))
                        .text_size(px(empty_font_size))
                        .text_color(ups.utility_panel_empty_text_color)
                        .child(SharedString::from(
                            self.t(crate::i18n::TextKey::NoGitHistory).to_string(),
                        ))
                        .into_any_element(),
                );
            }
            None if self.git_history_refresh_task.is_some() => {
                children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
            }
            None => {
                children.push(
                    div()
                        .px(px(section_padding_x))
                        .pt(px(section_gap))
                        .text_size(px(empty_font_size))
                        .text_color(ups.utility_panel_empty_text_color)
                        .child(SharedString::from(
                            self.t(crate::i18n::TextKey::NotInGitRepo).to_string(),
                        ))
                        .into_any_element(),
                );
            }
        }

        children.push(
            div()
                .h(px(bottom_spacer))
                .flex_shrink_0()
                .into_any_element(),
        );

        let scrollbar = self.render_scrollbar_for_handle(
            PanelScrollbarKind::GitHistory,
            &self.git_history_scroll_handle,
            f32::from(self.git_history_scroll_handle.bounds().size.height),
            0.0,
            self.scaled(self.styles.editor.editor_scrollbar_right_margin_px),
            cx,
        );

        let body = div()
            .id("git-history-body")
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .flex()
            .flex_col()
            .child(self.render_git_history_scope_toggle(cx))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .w_full()
                    .relative()
                    .overflow_hidden()
                    .child(
                        div()
                            .id("git-history-scroll")
                            .h_full()
                            .w_full()
                            .overflow_y_scroll()
                            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                                this.reveal_panel_scrollbar(PanelScrollbarKind::GitHistory, cx);
                            }))
                            .track_scroll(&self.git_history_scroll_handle)
                            .flex()
                            .flex_col()
                            .children(children),
                    )
                    .when_some(scrollbar, |this, scrollbar| this.child(scrollbar)),
            )
            .into_any_element();
        if should_load_next_page {
            self.schedule_next_git_history_page(cx);
        }
        body
    }

    fn render_git_history_row(
        &self,
        item: &GitHistoryItem,
        row_height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let history_item = item.clone();
        let font_size = self.scaled(ups.utility_panel_item_font_size_px);
        let row_gap = self.scaled(ups.utility_panel_item_gap_px * 0.4);
        let repository_scope = self.git_history_scope == GitHistoryScope::Repository;
        let is_expanded = repository_scope
            && item.commit_hash.is_some()
            && self.git_history_expanded_commit == item.commit_hash;
        let is_active = if repository_scope {
            is_expanded
        } else {
            self.active_git_diff_state().is_some_and(|diff| {
                match (
                    &diff.source,
                    item.is_working_tree,
                    item.commit_hash.as_deref(),
                ) {
                    (GitDiffSource::IndexToWorktree, true, _) => true,
                    (GitDiffSource::Commit { commit_hash, .. }, false, Some(item_hash)) => {
                        commit_hash == item_hash
                    }
                    _ => false,
                }
            })
        };
        let row_bg = if is_active {
            ups.utility_panel_button_hover_bg
        } else {
            gpui::transparent_black()
        };
        let hover_bg = ups.utility_panel_button_hover_bg;
        let top_label = if item.is_working_tree || item.author.is_empty() {
            item.short_hash.clone()
        } else {
            item.author.clone()
        };

        div()
            .px(px(self.scaled(ups.utility_panel_section_padding_x_px)))
            .w_full()
            .h(px(row_height))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(row_gap))
            .bg(row_bg)
            .overflow_hidden()
            .cursor(CursorStyle::Arrow)
            .hover(move |style| style.bg(hover_bg))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if this.git_history_scope == GitHistoryScope::Repository {
                        this.toggle_git_history_commit_expansion(&history_item, cx);
                    } else {
                        this.open_git_history_item_diff(history_item.clone(), cx);
                    }
                }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(self.scaled(ups.utility_panel_item_gap_px)))
                    .text_size(px(font_size))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_color(ups.utility_panel_item_label_text_color)
                            .whitespace_nowrap()
                            .truncate()
                            .child(SharedString::from(top_label)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(ups.utility_panel_item_label_text_color)
                            .whitespace_nowrap()
                            .child(SharedString::from(item.time_label.clone())),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .text_size(px(font_size))
                    .text_color(ups.utility_panel_item_value_text_color)
                    .whitespace_nowrap()
                    .truncate()
                    .child(SharedString::from(item.message.clone())),
            )
            .into_any_element()
    }

    fn render_git_commit_file_row(
        &self,
        entry: &GitCommitFileEntry,
        row_height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let font_size = self.scaled(ups.utility_panel_item_font_size_px);
        let hover_bg = ups.utility_panel_button_hover_bg;
        let letter_color = self.git_change_kind_color(entry.kind);
        let file_name = entry
            .rel_path
            .rsplit('/')
            .next()
            .unwrap_or(entry.rel_path.as_str())
            .to_string();
        let parent_dir = match entry.rel_path.rsplit_once('/') {
            Some((parent, _)) => parent.to_string(),
            None => String::new(),
        };
        let click_path = entry.rel_path.clone();

        div()
            .pl(px(
                self.scaled(ups.utility_panel_section_padding_x_px + 14.0)
            ))
            .pr(px(self.scaled(ups.utility_panel_section_padding_x_px)))
            .w_full()
            .h(px(row_height))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(self.scaled(6.0)))
            .text_size(px(font_size))
            .cursor(CursorStyle::Arrow)
            .hover(move |style| style.bg(hover_bg))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_git_commit_file_diff(&click_path, cx);
                }),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(font_size * 0.9))
                    .text_color(letter_color)
                    .font_weight(FontWeight::BOLD)
                    .child(SharedString::from(entry.kind.letter().to_string())),
            )
            .child(
                div()
                    .flex_none()
                    .max_w(px(self.scaled(150.0)))
                    .overflow_hidden()
                    .truncate()
                    .whitespace_nowrap()
                    .text_color(ups.utility_panel_item_value_text_color)
                    .child(SharedString::from(file_name)),
            )
            .when(!parent_dir.is_empty(), |this| {
                this.child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .truncate()
                        .whitespace_nowrap()
                        .text_color(ups.utility_panel_item_label_text_color)
                        .child(SharedString::from(parent_dir)),
                )
            })
            .into_any_element()
    }

    fn toggle_git_history_commit_expansion(
        &mut self,
        item: &GitHistoryItem,
        cx: &mut Context<Self>,
    ) {
        let Some(hash) = item.commit_hash.clone() else {
            return;
        };
        if self.git_history_expanded_commit.as_deref() == Some(hash.as_str()) {
            self.git_history_expanded_commit = None;
            cx.notify();
            return;
        }
        self.git_history_expanded_commit = Some(hash.clone());
        let already_cached = self
            .git_commit_files
            .as_ref()
            .is_some_and(|(cached_hash, _)| cached_hash == &hash);
        if !already_cached {
            self.schedule_git_commit_files_load(hash, cx);
        }
        cx.notify();
    }

    fn schedule_git_commit_files_load(&mut self, commit_hash: String, cx: &mut Context<Self>) {
        let Some(base_dir) = self.git_panel_base_dir() else {
            return;
        };
        let task = cx.spawn(async move |this, cx| {
            let background_hash = commit_hash.clone();
            let files = cx
                .background_executor()
                .spawn(async move {
                    git_repo_root(&base_dir)
                        .map(|repo_root| collect_git_commit_files(&repo_root, &background_hash))
                        .unwrap_or_default()
                })
                .await;
            this.update(cx, |this, cx| {
                this.git_commit_files_task = None;
                if this.git_history_expanded_commit.as_deref() == Some(commit_hash.as_str()) {
                    this.git_commit_files = Some((commit_hash, files));
                    cx.notify();
                }
            })
            .ok();
        });
        self.git_commit_files_task = Some(task);
    }

    fn open_git_commit_file_diff(&mut self, rel_path: &str, cx: &mut Context<Self>) {
        let Some(hash) = self.git_history_expanded_commit.clone() else {
            return;
        };
        let Some(status) = self.git_panel_repo_status() else {
            return;
        };
        let absolute_path = status.repo_root.join(rel_path);
        let short_hash = hash.chars().take(8).collect::<String>();
        let source = GitDiffSource::Commit {
            commit_hash: hash,
            short_hash,
        };
        let Some(diff) = self.build_git_diff_state_for_source(&absolute_path, source) else {
            return;
        };
        let title = Self::title_for_path(&absolute_path);
        self.open_git_diff_tab(title, absolute_path, diff, cx);
    }

    // ── Remote URL input overlay ───────────────────────────────────

    pub(crate) fn open_git_remote_input(&mut self, cx: &mut Context<Self>) {
        self.git_remote_input_visible = true;
        if self.git_remote_input_editor.is_none() {
            let styles = self.language_picker_text_input_styles();
            let placeholder = self.t(crate::i18n::TextKey::GitSetRemotePlaceholder);
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
                    crate::text_input::TextInputEvent::Submit => {
                        this.confirm_git_remote_input(cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {
                        this.git_remote_input_visible = false;
                        cx.notify();
                    }
                    _ => {}
                },
            )
            .detach();
            self.git_remote_input_editor = Some(editor);
        }
        cx.notify();
    }

    pub(crate) fn close_git_remote_input(&mut self, cx: &mut Context<Self>) {
        self.git_remote_input_visible = false;
        cx.notify();
    }

    fn confirm_git_remote_input(&mut self, cx: &mut Context<Self>) {
        let url = self
            .git_remote_input_editor
            .as_ref()
            .map(|editor| editor.read(cx).text().trim().to_string())
            .unwrap_or_default();
        if url.is_empty() {
            return;
        }
        self.git_remote_input_visible = false;
        self.run_git_operation(GitOperation::AddRemote { url }, cx);
    }

    pub(crate) fn render_git_remote_input_overlay(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let styles = &self.styles.quick_open;
        let bg = styles.quick_open_panel_bg;
        let border = styles.quick_open_panel_border_color;
        let text_primary = styles.quick_open_result_text_color;
        let text_secondary = styles.quick_open_result_secondary_text_color;
        let panel_w = self.scaled(420.0).min(self.last_window_width - 32.0);
        let panel_left = ((self.last_window_width - panel_w) / 2.0).max(16.0);
        let panel_top = (self.last_window_height * 0.3).max(60.0);

        if let Some(editor) = &self.git_remote_input_editor {
            editor.focus_handle(cx).focus(window);
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
                        cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.close_git_remote_input(cx);
                        }),
                    ),
            )
            .child(
                div()
                    .id("git-remote-input-panel")
                    .absolute()
                    .top(px(panel_top))
                    .left(px(panel_left))
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
                            .px(px(self.scaled(12.0)))
                            .pt(px(self.scaled(10.0)))
                            .text_size(px(self.scaled(13.0)))
                            .text_color(text_primary)
                            .child(SharedString::from(
                                self.t(crate::i18n::TextKey::GitSetRemoteTitle).to_string(),
                            )),
                    )
                    .child(
                        div()
                            .px(px(self.scaled(12.0)))
                            .pt(px(self.scaled(2.0)))
                            .text_size(px(self.scaled(11.0)))
                            .text_color(text_secondary)
                            .child(SharedString::from(
                                self.t(crate::i18n::TextKey::GitSetRemoteHint).to_string(),
                            )),
                    )
                    .child(
                        div()
                            .w_full()
                            .px(px(self.scaled(8.0)))
                            .py(px(self.scaled(8.0)))
                            .when_some(self.git_remote_input_editor.clone(), |this, editor| {
                                this.child(editor)
                            }),
                    ),
            )
            .into_any_element()
    }
}

fn entry_stage_paths(entry: &GitChangeEntry) -> Vec<String> {
    let mut paths = vec![entry.rel_path.clone()];
    if let Some(orig) = entry.orig_path.as_ref() {
        paths.push(orig.clone());
    }
    paths
}
