use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, Context, CursorStyle, DragMoveEvent, FontWeight, MouseButton, MouseDownEvent,
    SharedString, Transformation, div, img, prelude::*, px, radians, svg,
};

use super::*;

impl EditorView {
    fn sidebar_git_status_marker(
        status: Option<SidebarGitStatus>,
        is_dir: bool,
    ) -> Option<&'static str> {
        if is_dir {
            return None;
        }

        match status {
            Some(SidebarGitStatus::Added) => Some("A"),
            Some(SidebarGitStatus::Modified) => Some("M"),
            None => None,
        }
    }

    fn render_sidebar_name_label(&self, name: &str, color: gpui::Hsla) -> AnyElement {
        let ss = &self.styles.sidebar;
        let (base, suffix) = split_file_name_for_display(name);
        div()
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .flex()
            .gap(px(0.0))
            .items_center()
            .text_size(px(self.scaled(ss.sidebar_item_font_size_px)))
            .text_color(color)
            .child(
                div()
                    .whitespace_nowrap()
                    .text_color(color)
                    .child(base.to_string()),
            )
            .when_some(suffix, |this, suffix| {
                this.child(
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .text_color(color)
                        .opacity(ss.sidebar_item_suffix_opacity)
                        .child(suffix.to_string()),
                )
            })
            .into_any_element()
    }

    fn render_sidebar_edit_indicator(
        &self,
        name: &str,
        is_dir: bool,
        color: gpui::Hsla,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        if is_dir {
            return svg()
                .path(FOLDER_CLOSED_SVG_PATH)
                .size(px(self.scaled(ss.sidebar_item_icon_size_px)))
                .text_color(color)
                .into_any_element();
        }

        if let Some(label) = file_kind_badge_text_for_name(name) {
            return div()
                .min_w(px(self.scaled(ss.sidebar_item_badge_min_width_px)))
                .h(px(self.scaled(ss.sidebar_item_badge_height_px)))
                .px(px(self.scaled(ss.sidebar_item_badge_padding_x_px)))
                .rounded(px(self.scaled(ss.sidebar_item_badge_radius_px)))
                .flex()
                .items_center()
                .justify_center()
                .bg(ss.sidebar_item_badge_bg)
                .text_size(px(self.scaled(ss.sidebar_item_badge_font_size_px)))
                .text_color(ss.sidebar_item_badge_text_color)
                .font_weight(FontWeight::EXTRA_BOLD)
                .child(label)
                .into_any_element();
        }

        if !file_kind_has_indicator_for_name(name) {
            return svg()
                .path(MARKDOWN_SVG_PATH)
                .size(px(self.scaled(ss.sidebar_item_icon_size_px + 4.0)))
                .text_color(color)
                .into_any_element();
        }

        svg()
            .path(MARKDOWN_SVG_PATH)
            .size(px(self.scaled(ss.sidebar_item_icon_size_px + 4.0)))
            .text_color(color)
            .into_any_element()
    }

    fn render_sidebar_header_action_button(
        &self,
        id: &'static str,
        icon_path: &'static str,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &MouseDownEvent, &mut gpui::Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        let hover_opacity = ss.sidebar_header_action_hover_opacity;
        div()
            .id(id)
            .w(px(self.scaled(ss.sidebar_header_action_btn_size_px)))
            .h(px(self.scaled(ss.sidebar_header_action_btn_size_px)))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .cursor(CursorStyle::Arrow)
            .opacity(ss.sidebar_header_action_opacity)
            .hover(move |style| style.opacity(hover_opacity))
            .child(
                svg()
                    .path(icon_path)
                    .size(px(self.scaled(ss.sidebar_header_action_icon_size_px)))
                    .text_color(ss.sidebar_header_icon_color),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    handler(this, event, window, cx);
                }),
            )
            .into_any_element()
    }

    fn render_sidebar_top_button(
        &self,
        id: &'static str,
        icon_path: &'static str,
        is_active: bool,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &MouseDownEvent, &mut gpui::Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        let icon_color = if is_active {
            ss.sidebar_top_button_active_color
        } else {
            ss.sidebar_top_button_inactive_color
        };
        div()
            .id(id)
            .w(px(self.scaled(ss.sidebar_top_button_size_px)))
            .h(px(self.scaled(ss.sidebar_top_button_size_px)))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .cursor(CursorStyle::Arrow)
            .child(
                svg()
                    .path(icon_path)
                    .size(px(self.scaled(ss.sidebar_top_button_icon_size_px)))
                    .text_color(icon_color),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    handler(this, event, window, cx);
                }),
            )
            .into_any_element()
    }

    fn render_sidebar_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let ss = &self.styles.sidebar;
        let root_name = self.sidebar_root_name().to_uppercase();
        let header_hover_bg = ss.sidebar_header_hover_bg;
        let header_drag_over_bg = ss.sidebar_item_selected_bg;
        let root_drag_over = self
            .sidebar_drag_hover_target
            .as_ref()
            .is_some_and(|target| target == &self.sidebar_root);
        div()
            .w_full()
            .h(px(self.scaled(ss.sidebar_header_height_px)))
            .bg(if root_drag_over {
                header_drag_over_bg
            } else {
                ss.sidebar_header_bg
            })
            .px(px(self.scaled(ss.sidebar_header_padding_right_px)))
            .pl(px(self.scaled(ss.sidebar_header_padding_left_px)))
            .flex()
            .items_center()
            .gap(px(self.scaled(ss.sidebar_header_gap_px)))
            .flex_none()
            .hover(move |s| s.bg(header_hover_bg))
            .child(
                svg()
                    .path(ROOT_SVG_PATH)
                    .size(px(self.scaled(ss.sidebar_header_root_icon_size_px)))
                    .text_color(ss.sidebar_header_icon_color),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(self.scaled(ss.sidebar_header_font_size_px)))
                    .text_color(ss.sidebar_header_text_color)
                    .overflow_hidden()
                    .truncate()
                    .child(root_name),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(ss.sidebar_header_action_gap_px)))
                    .child(self.render_sidebar_header_action_button(
                        "sidebar-header-search",
                        FILETREE_SEARCH_SVG_PATH,
                        cx,
                        |this, _event, window, cx| {
                            this.toggle_workspace_search(window, cx);
                        },
                    ))
                    .child(self.render_sidebar_header_action_button(
                        "sidebar-header-sort",
                        FILETREE_SORT_SVG_PATH,
                        cx,
                        |this, event, _window, cx| {
                            this.open_sidebar_sort_menu(event.position, cx);
                        },
                    ))
                    .child(self.render_sidebar_header_action_button(
                        "sidebar-header-create",
                        FILETREE_ADD_SVG_PATH,
                        cx,
                        |this, event, _window, cx| {
                            this.open_sidebar_create_menu(event.position, cx);
                        },
                    )),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.toggle_sidebar_directory(this.sidebar_root.clone(), cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.open_sidebar_context_menu(
                        this.sidebar_root.clone(),
                        true,
                        event.position,
                        cx,
                    );
                }),
            )
            .drag_over::<DraggedSidebarItems>({
                let root = self.sidebar_root.clone();
                move |style, dragged, _, _| {
                    if Self::sidebar_drop_would_move(&dragged.paths, &root) {
                        style.bg(header_drag_over_bg)
                    } else {
                        style
                    }
                }
            })
            .on_drop({
                let root = self.sidebar_root.clone();
                cx.listener(move |this, dragged: &DraggedSidebarItems, _, cx| {
                    let paths = dragged.paths.clone();
                    this.clear_sidebar_drag_state(cx);
                    this.move_sidebar_items(paths, root.clone(), cx);
                })
            })
            .on_drag_move({
                let root = self.sidebar_root.clone();
                cx.listener(
                    move |this, event: &DragMoveEvent<DraggedSidebarItems>, _, cx| {
                        if event.bounds.contains(&event.event.position) {
                            this.update_sidebar_drag_autoscroll(event.event.position, cx);
                            let dragged = event.drag(cx);
                            let valid_move = Self::sidebar_drop_would_move(&dragged.paths, &root);
                            if valid_move {
                                this.update_sidebar_drag_expand_hover_dir(None, cx);
                                this.update_sidebar_drag_target(Some(root.clone()), cx);
                            } else {
                                this.clear_sidebar_drag_hover(cx);
                            }
                        }
                    },
                )
            })
            .into_any_element()
    }

    fn render_starred_sidebar_section(
        &self,
        starred_files: &[PathBuf],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        let header_hover_bg = ss.sidebar_header_hover_bg;
        let icon_color = ss.sidebar_header_icon_color;
        let title_color = ss.sidebar_header_text_color;
        let is_collapsed = self.sidebar_starred_collapsed;

        let header = div()
            .w_full()
            .h(px(self.scaled(ss.sidebar_header_height_px)))
            .px(px(self.scaled(ss.sidebar_header_padding_right_px)))
            .pl(px(self.scaled(ss.sidebar_header_padding_left_px)))
            .flex()
            .items_center()
            .gap(px(self.scaled(ss.sidebar_header_gap_px)))
            .flex_none()
            .hover(move |style| style.bg(header_hover_bg))
            .cursor(CursorStyle::Arrow)
            .child(
                svg()
                    .path(FILETREE_STAR_SVG_PATH)
                    .size(px(self.scaled(ss.sidebar_header_star_icon_size_px)))
                    .text_color(icon_color),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(self.scaled(ss.sidebar_header_font_size_px)))
                    .text_color(title_color)
                    .overflow_hidden()
                    .truncate()
                    .child(self.t(crate::i18n::TextKey::StarredFiles)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.sidebar_starred_collapsed = !this.sidebar_starred_collapsed;
                    cx.notify();
                }),
            );

        div()
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .child(header)
            .when(!is_collapsed, |this| {
                this.children(
                    starred_files
                        .iter()
                        .cloned()
                        .map(|path| self.render_starred_sidebar_item(path, cx))
                        .collect::<Vec<_>>(),
                )
            })
            .into_any_element()
    }

    fn render_starred_sidebar_item(&self, path: PathBuf, cx: &mut Context<Self>) -> AnyElement {
        if let Some(SidebarEditMode::RenameStarred {
            path: ref rename_path,
            ref editor,
        }) = self.sidebar_edit
            && *rename_path == path
        {
            return self.render_sidebar_edit_row(editor, 1, false, cx);
        }

        let ss = &self.styles.sidebar;
        let is_selected = self.is_sidebar_item_selected(&path);
        let is_focused = self.focused_sidebar_path.as_deref() == Some(path.as_path());
        let normalized_path = Self::normalize_sidebar_path(&path);
        let git_status = self.sidebar_git_statuses.get(&normalized_path).copied();
        let git_status_color = self.sidebar_git_status_color(git_status);
        let row_bg = if is_selected {
            ss.sidebar_item_selected_bg
        } else {
            gpui::transparent_black()
        };
        let item_hover_bg = ss.sidebar_item_hover_bg;
        let icon_color = git_status_color.unwrap_or(if is_selected {
            ss.sidebar_item_selected_icon_color
        } else {
            ss.sidebar_item_icon_color
        });
        let label_color = git_status_color.unwrap_or(if is_selected {
            ss.sidebar_item_selected_text_color
        } else {
            ss.sidebar_item_text_color
        });
        let focus_border_color = ss.sidebar_item_focus_border_color;
        let active_border_color = ss.sidebar_item_active_border_color;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let left_padding = self.scaled(
            (ss.sidebar_item_indent_base_px + ss.sidebar_item_indent_step_px
                - ss.sidebar_item_left_shift_px)
                .max(0.0),
        );
        let action_hover_opacity = ss.sidebar_header_action_hover_opacity;
        let remove_star_path = path.clone();
        let click_path = path.clone();
        let context_menu_path = path.clone();
        let close_hover_bg = ss.sidebar_item_hover_bg;

        div()
            .id(SharedString::from(format!(
                "sidebar-starred-{}",
                path.display()
            )))
            .group("sidebar-starred-item")
            .w_full()
            .h(px(self.scaled(ss.sidebar_item_height_px)))
            .flex_none()
            .flex()
            .items_center()
            .bg(row_bg)
            .border_1()
            .border_color(if is_selected {
                active_border_color
            } else if is_focused {
                focus_border_color
            } else {
                gpui::transparent_black()
            })
            .when(!self.sidebar_resizing && !self.outline_resizing, |this| {
                this.hover(move |style| style.bg(item_hover_bg))
            })
            .child(
                div()
                    .w_full()
                    .h_full()
                    .pl(px(left_padding))
                    .pr(px(self.scaled(ss.sidebar_item_padding_right_px)))
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(ss.sidebar_item_gap_px)))
                    .child(
                        div()
                            .w(px(0.0))
                            .h(px(self.scaled(ss.sidebar_item_fold_container_height_px)))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .child(div().w(px(self.scaled(ss.sidebar_item_fold_icon_size_px)))),
                    )
                    .child(self.render_sidebar_edit_indicator(&name, false, icon_color))
                    .child(self.render_sidebar_name_label(&name, label_color))
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "sidebar-starred-remove-{}",
                                path.display()
                            )))
                            .w(px(self.scaled(ss.sidebar_header_action_btn_size_px)))
                            .h(px(self.scaled(ss.sidebar_header_action_btn_size_px)))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .cursor(CursorStyle::Arrow)
                            .opacity(0.0)
                            .group_hover("sidebar-starred-item", move |style| {
                                style.opacity(ss.sidebar_header_action_opacity)
                            })
                            .hover(move |style| {
                                style.opacity(action_hover_opacity).bg(close_hover_bg)
                            })
                            .child(
                                svg()
                                    .path(FILETREE_CLOSE_SVG_PATH)
                                    .size(px(
                                        self.scaled(ss.sidebar_starred_item_close_icon_size_px)
                                    ))
                                    .text_color(ss.sidebar_header_icon_color),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    let normalized =
                                        Self::normalize_sidebar_path(&remove_star_path);
                                    if this.app_settings.remove_starred_file(&normalized) {
                                        this.app_settings.save();
                                    }
                                    cx.notify();
                                }),
                            ),
                    ),
            )
            .on_click({
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    let mods = event.modifiers();
                    let has_modifier = mods.platform || mods.shift;
                    this.sidebar_select_item(&click_path, mods.shift, mods.platform);
                    if !has_modifier {
                        this.suppress_next_sidebar_sync_path = Some(click_path.clone());
                        if event.click_count() >= 2 {
                            this.focused_sidebar_path = None;
                            this.open_path_in_permanent_tab(click_path.clone(), true, cx);
                            this.move_cursor_to_first_line_end();
                            this.request_editor_focus(cx);
                        } else if !this.switch_to_existing_editor_tab_for_path(
                            &click_path,
                            false,
                            cx,
                        ) {
                            this.open_path_in_preview_tab(click_path.clone(), false, cx);
                        }
                    }
                    window.prevent_default();
                    cx.notify();
                })
            })
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    if !this.selected_sidebar_paths.contains(&context_menu_path) {
                        this.sidebar_select_item(&context_menu_path, false, false);
                    }
                    this.focused_sidebar_path = Some(context_menu_path.clone());
                    this.open_starred_sidebar_context_menu(
                        context_menu_path.clone(),
                        event.position,
                        cx,
                    );
                }),
            )
            .into_any_element()
    }

    pub(crate) fn render_sidebar_edit_row(
        &self,
        editor: &gpui::Entity<crate::text_input::TextInput>,
        depth: usize,
        is_dir: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        let shift = ss.sidebar_item_left_shift_px;
        let is_root_level = depth == 0;
        let left_padding = if is_root_level {
            self.scaled(self.styles.workspace_switcher.ws_bar_padding_x_px)
        } else {
            self.scaled(
                (ss.sidebar_item_indent_base_px + depth as f32 * ss.sidebar_item_indent_step_px
                    - shift)
                    .max(0.0),
            )
        };
        let show_fold_icon = is_dir && !is_root_level;
        let fold_slot_width = if show_fold_icon {
            self.scaled(ss.sidebar_item_fold_container_width_px)
        } else {
            0.0
        };
        let current_name = editor.read(cx).text().to_string();

        div()
            .w_full()
            .h(px(self.scaled(ss.sidebar_item_height_px)))
            .flex_none()
            .flex()
            .items_center()
            .bg(ss.sidebar_item_selected_bg)
            .child(
                div()
                    .w_full()
                    .h_full()
                    .pl(px(left_padding))
                    .pr(px(self.scaled(ss.sidebar_item_padding_right_px)))
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(ss.sidebar_item_gap_px)))
                    .child(
                        div()
                            .w(px(fold_slot_width))
                            .h(px(self.scaled(ss.sidebar_item_fold_container_height_px)))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .child(if show_fold_icon {
                                svg()
                                    .path(FILETREE_FOLD_SVG_PATH)
                                    .size(px(self.scaled(ss.sidebar_item_fold_icon_size_px)))
                                    .text_color(ss.sidebar_item_fold_icon_color)
                                    .into_any_element()
                            } else {
                                div()
                                    .w(px(self.scaled(ss.sidebar_item_fold_icon_size_px)))
                                    .into_any_element()
                            }),
                    )
                    .child(self.render_sidebar_edit_indicator(
                        &current_name,
                        is_dir,
                        ss.sidebar_item_selected_icon_color,
                    ))
                    .child(div().flex_1().min_w(px(0.0)).child(editor.clone())),
            )
            .on_mouse_down(MouseButton::Left, |_event, _window, _cx| {})
            .into_any_element()
    }

    pub(crate) fn render_sidebar_item(
        &self,
        item: SidebarItem,
        active_guide_levels: &HashSet<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(SidebarEditMode::Rename {
            path: ref rp,
            ref editor,
        }) = self.sidebar_edit
            && *rp == item.path
        {
            return self.render_sidebar_edit_row(editor, item.depth, item.is_dir, cx);
        }

        let is_selected = self.is_sidebar_item_selected(&item.path);
        let path = item.path.clone();
        let is_dir = item.is_dir;
        let ss = &self.styles.sidebar;
        let normalized_path = Self::normalize_sidebar_path(&path);
        let git_status = self.sidebar_git_statuses.get(&normalized_path).copied();
        let git_status_color = self.sidebar_git_status_color(git_status);
        let git_status_marker = Self::sidebar_git_status_marker(git_status, is_dir);
        let shift = ss.sidebar_item_left_shift_px;
        let is_root_level = item.depth == 0;
        let left_padding = if is_root_level {
            self.scaled(self.styles.workspace_switcher.ws_bar_padding_x_px)
        } else {
            self.scaled(
                (ss.sidebar_item_indent_base_px
                    + item.depth as f32 * ss.sidebar_item_indent_step_px
                    - shift)
                    .max(0.0),
            )
        };
        let show_fold_icon = item.is_dir && !is_root_level;
        let fold_slot_width = if show_fold_icon {
            self.scaled(ss.sidebar_item_fold_container_width_px)
        } else {
            0.0
        };
        let row_bg = if is_selected {
            ss.sidebar_item_selected_bg
        } else {
            gpui::transparent_black()
        };
        let is_cut_item = self.file_clipboard.as_ref().is_some_and(|clipboard| {
            clipboard.is_cut
                && clipboard.paths.first().is_some_and(|cut_path| {
                    let cut_path = Self::normalize_sidebar_path(cut_path);
                    let current_path = Self::normalize_sidebar_path(&path);
                    current_path == cut_path || current_path.starts_with(&cut_path)
                })
        });
        let icon_color = git_status_color.unwrap_or(if is_selected {
            ss.sidebar_item_selected_icon_color
        } else {
            ss.sidebar_item_icon_color
        });
        let label_color = git_status_color.unwrap_or(if is_selected {
            ss.sidebar_item_selected_text_color
        } else {
            ss.sidebar_item_text_color
        });
        let is_focused = self.focused_sidebar_path.as_deref() == Some(path.as_path());
        let focus_border_color = ss.sidebar_item_focus_border_color;
        let active_border_color = ss.sidebar_item_active_border_color;
        let item_hover_bg = ss.sidebar_item_hover_bg;

        let guide_color = ss.sidebar_indent_guide_color;
        let guide_width = ss.sidebar_indent_guide_width_px;
        let dash_len = self.scaled(ss.sidebar_indent_guide_dash_length_px);
        let gap_len = self.scaled(ss.sidebar_indent_guide_gap_length_px);
        let indent_base = ss.sidebar_item_indent_base_px;
        let indent_step = ss.sidebar_item_indent_step_px;
        let item_height = self.scaled(ss.sidebar_item_height_px);
        let fold_w = self.scaled(ss.sidebar_item_fold_container_width_px);
        let guides: Vec<gpui::AnyElement> = (1..=item.depth)
            .filter(|level| active_guide_levels.contains(level))
            .flat_map(|level| {
                let guide_x = self
                    .scaled((indent_base + level as f32 * indent_step - shift).max(0.0))
                    + fold_w / 2.0
                    - guide_width / 2.0;
                let segment_step = dash_len + gap_len;
                let count = (item_height / segment_step).ceil() as usize;
                (0..count).map(move |i| {
                    let y = i as f32 * segment_step;
                    let h = dash_len.min(item_height - y);
                    div()
                        .absolute()
                        .left(px(guide_x))
                        .top(px(y))
                        .w(px(guide_width))
                        .h(px(h))
                        .bg(guide_color)
                        .into_any_element()
                })
            })
            .collect();

        let is_drag_over = self
            .sidebar_drag_hover_target
            .as_ref()
            .is_some_and(|target| target == &path || path.starts_with(target));
        let drag_over_bg = ss.sidebar_item_selected_bg;

        let drag_paths: Vec<PathBuf> = if self.selected_sidebar_paths.contains(&path) {
            Self::filter_nested_paths(&self.selected_sidebar_paths)
        } else {
            vec![path.clone()]
        };
        let drag_label = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let drag_count = drag_paths.len();
        let item_drop_target = if item.is_dir {
            path.clone()
        } else {
            path.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.sidebar_root.clone())
        };
        let indicator = if item.is_dir {
            svg()
                .path(if item.is_expanded {
                    FOLDER_OPEN_SVG_PATH
                } else {
                    FOLDER_CLOSED_SVG_PATH
                })
                .size(px(self.scaled(ss.sidebar_dir_icon_size_px)))
                .text_color(icon_color)
                .into_any_element()
        } else if let Some(label) = file_kind_badge_text(&item.path) {
            div()
                .min_w(px(self.scaled(ss.sidebar_item_badge_min_width_px)))
                .h(px(self.scaled(ss.sidebar_item_badge_height_px)))
                .px(px(self.scaled(ss.sidebar_item_badge_padding_x_px)))
                .rounded(px(self.scaled(ss.sidebar_item_badge_radius_px)))
                .flex()
                .items_center()
                .justify_center()
                .bg(ss.sidebar_item_badge_bg)
                .text_size(px(self.scaled(ss.sidebar_item_badge_font_size_px)))
                .text_color(ss.sidebar_item_badge_text_color)
                .font_weight(FontWeight::EXTRA_BOLD)
                .child(label)
                .into_any_element()
        } else if !file_kind_has_indicator(&item.path) {
            div()
                .w(px(self.scaled(ss.sidebar_item_icon_size_px)))
                .h(px(self.scaled(ss.sidebar_item_icon_size_px)))
                .flex_none()
                .into_any_element()
        } else {
            svg()
                .path(MARKDOWN_SVG_PATH)
                .size(px(self.scaled(ss.sidebar_item_icon_size_px + 4.0)))
                .text_color(icon_color)
                .into_any_element()
        };

        div()
            .id(SharedString::from(format!("sidebar-{}", path.display())))
            .w_full()
            .h(px(item_height))
            .flex_none()
            .relative()
            .flex()
            .items_center()
            .opacity(if is_cut_item { 0.48 } else { 1.0 })
            .bg(if is_drag_over { drag_over_bg } else { row_bg })
            .border_1()
            .border_color(if is_selected {
                active_border_color
            } else if is_focused {
                focus_border_color
            } else {
                gpui::transparent_black()
            })
            .when(!self.sidebar_resizing && !self.outline_resizing, |this| {
                this.hover(move |style| style.bg(item_hover_bg))
            })
            .children(guides)
            .child(
                div()
                    .w_full()
                    .h_full()
                    .pl(px(left_padding))
                    .pr(px(self.scaled(ss.sidebar_item_padding_right_px)))
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(ss.sidebar_item_gap_px)))
                    .child(
                        div()
                            .w(px(fold_slot_width))
                            .h(px(self.scaled(ss.sidebar_item_fold_container_height_px)))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .child(if show_fold_icon {
                                svg()
                                    .path(FILETREE_FOLD_SVG_PATH)
                                    .size(px(self.scaled(ss.sidebar_item_fold_icon_size_px)))
                                    .text_color(ss.sidebar_item_fold_icon_color)
                                    .when(item.is_expanded, |s| {
                                        s.with_transformation(Transformation::rotate(radians(
                                            std::f32::consts::FRAC_PI_2,
                                        )))
                                    })
                                    .into_any_element()
                            } else {
                                div()
                                    .w(px(self.scaled(ss.sidebar_item_fold_icon_size_px)))
                                    .into_any_element()
                            }),
                    )
                    .child(indicator)
                    .child(self.render_sidebar_name_label(&item.name, label_color))
                    .when_some(git_status_marker, |this, marker| {
                        this.child(
                            div()
                                .flex_none()
                                .text_size(px(self.scaled(ss.sidebar_item_font_size_px)))
                                .text_color(label_color)
                                .font_weight(FontWeight::EXTRA_BOLD)
                                .child(marker),
                        )
                    }),
            )
            .on_click({
                let path = path.clone();
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    let mods = event.modifiers();
                    let has_modifier = mods.platform || mods.shift;
                    this.sidebar_select_item(&path, mods.shift, mods.platform);
                    if !has_modifier {
                        if path.is_dir() {
                            this.toggle_sidebar_directory(path.clone(), cx);
                        } else {
                            this.suppress_next_sidebar_sync_path = Some(path.clone());
                            if event.click_count() >= 2 {
                                this.focused_sidebar_path = None;
                                this.open_path_in_permanent_tab(path.clone(), true, cx);
                                this.move_cursor_to_first_line_end();
                                this.request_editor_focus(cx);
                            } else if !this.switch_to_existing_editor_tab_for_path(&path, false, cx)
                            {
                                this.open_path_in_preview_tab(path.clone(), false, cx);
                            }
                        }
                    }
                    window.prevent_default();
                    cx.notify();
                })
            })
            .when(!self.sidebar_resizing && !self.outline_resizing, |el| {
                el.on_drag(DraggedSidebarItems { paths: drag_paths }, {
                    let label = drag_label;
                    let count = drag_count;
                    let ghost_font_size_px = self.scaled(self.styles.ui_font_size_px);
                    move |_dragged, offset, _window, cx| {
                        cx.new(|_cx| DraggedSidebarItemsView {
                            label: label.clone(),
                            count,
                            font_size_px: ghost_font_size_px,
                            cursor_offset: offset,
                        })
                    }
                })
            })
            .drag_over::<DraggedSidebarItems>({
                let drop_path = item_drop_target.clone();
                move |style, dragged, _, _| {
                    if Self::sidebar_drop_would_move(&dragged.paths, &drop_path) {
                        style.bg(drag_over_bg)
                    } else {
                        style
                    }
                }
            })
            .on_drop({
                let drop_path = item_drop_target.clone();
                cx.listener(move |this, dragged: &DraggedSidebarItems, _, cx| {
                    let paths = dragged.paths.clone();
                    this.clear_sidebar_drag_state(cx);
                    this.move_sidebar_items(paths, drop_path.clone(), cx);
                })
            })
            .on_drag_move(cx.listener({
                let path = path.clone();
                let drop_path = item_drop_target.clone();
                move |this, event: &DragMoveEvent<DraggedSidebarItems>, _, cx| {
                    if !event.bounds.contains(&event.event.position) {
                        return;
                    }

                    cx.stop_propagation();
                    this.sidebar_drag_child_hover_position =
                        Some(Self::sidebar_drag_position_key(event.event.position));
                    this.update_sidebar_drag_autoscroll(event.event.position, cx);

                    let dragged = event.drag(cx);
                    let valid_move = Self::sidebar_drop_would_move(&dragged.paths, &drop_path);
                    let desired_expand_hover = is_dir.then(|| path.clone());

                    this.update_sidebar_drag_expand_hover_dir(desired_expand_hover, cx);

                    if !valid_move {
                        this.update_sidebar_drag_target(None, cx);
                        return;
                    }

                    let new_target = Some(drop_path.clone());
                    if this.sidebar_drag_hover_target != new_target {
                        this.update_sidebar_drag_target(new_target, cx);
                        cx.notify();
                    }
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    if !this.selected_sidebar_paths.contains(&path) {
                        this.sidebar_select_item(&path, false, false);
                    }
                    this.focused_sidebar_path = Some(path.clone());
                    this.open_sidebar_context_menu(path.clone(), is_dir, event.position, cx);
                }),
            )
            .into_any_element()
    }

    pub(crate) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let has_root = self.sidebar_root.exists() && self.sidebar_error.is_none();
        if has_root {
            self.refresh_sidebar_git_statuses_if_needed(cx);
        }
        let starred_files = if has_root {
            self.visible_starred_sidebar_files()
        } else {
            Vec::new()
        };
        let show_workspace_bar = true;
        let items = if has_root {
            Some(self.sidebar_items())
        } else {
            None
        };
        let ss = self.styles.sidebar.clone();

        let top_spacer = div()
            .id("sidebar-top-spacer")
            .w_full()
            .h(px(self.scaled(ss.sidebar_top_spacer_height_px)))
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .when(ss.sidebar_top_buttons_visible, |this| {
                this.pr(px(self.scaled(ss.sidebar_top_button_padding_right_px)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(self.scaled(ss.sidebar_top_button_gap_px)))
                            .child(self.render_sidebar_top_button(
                                "sidebar-top-filetree",
                                "assets/icons/filetree.svg",
                                !self.workspace_search_visible,
                                cx,
                                |this, _event, window, cx| {
                                    this.sidebar_visible = true;
                                    if this.workspace_search_visible {
                                        this.close_workspace_search(window, cx);
                                    } else {
                                        cx.notify();
                                    }
                                },
                            ))
                            .child(self.render_sidebar_top_button(
                                "sidebar-top-search",
                                "assets/icons/search.svg",
                                self.workspace_search_visible,
                                cx,
                                |this, _event, window, cx| {
                                    this.toggle_workspace_search(window, cx);
                                },
                            )),
                    )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, event: &MouseDownEvent, window, _cx| {
                    if event.click_count >= 2 {
                        window.zoom_window();
                    } else {
                        super::perform_window_drag(window);
                    }
                }),
            );

        let header = self.render_sidebar_header(cx);
        let mut tree_items: Vec<AnyElement> = Vec::new();

        if let Some(error) = &self.sidebar_error {
            tree_items.push(
                div()
                    .w_full()
                    .px(px(self.scaled(ss.sidebar_error_padding_x_px)))
                    .py(px(self.scaled(ss.sidebar_error_padding_y_px)))
                    .text_size(px(self.scaled(ss.sidebar_error_font_size_px)))
                    .text_color(ss.sidebar_error_text_color)
                    .child(error.clone())
                    .into_any_element(),
            );
        } else if !has_root {
            let empty_hover_bg = ss.sidebar_empty_btn_hover_bg;
            tree_items.push(
                div()
                    .w_full()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .px(px(self.scaled(ss.sidebar_empty_padding_x_px)))
                    .child(
                        div()
                            .px(px(self.scaled(ss.sidebar_empty_btn_padding_x_px)))
                            .py(px(self.scaled(ss.sidebar_empty_btn_padding_y_px)))
                            .rounded_full()
                            .border(px(ss.sidebar_empty_btn_border_width_px))
                            .border_dashed()
                            .border_color(ss.sidebar_empty_btn_border_color)
                            .text_size(px(self.scaled(ss.sidebar_empty_btn_font_size_px)))
                            .text_color(ss.sidebar_empty_btn_text_color)
                            .hover(move |s| s.bg(empty_hover_bg))
                            .cursor(CursorStyle::Arrow)
                            .child(self.t(crate::i18n::TextKey::OpenFolder))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.pick_workspace_folder(cx);
                                }),
                            ),
                    )
                    .into_any_element(),
            );
        } else {
            let create_parent = match &self.sidebar_edit {
                Some(SidebarEditMode::CreateFile { parent, .. })
                | Some(SidebarEditMode::CreateDir { parent, .. }) => Some(parent.clone()),
                _ => None,
            };
            let create_depth = create_parent.as_ref().map(|p| {
                p.strip_prefix(&self.sidebar_root)
                    .map(|rel| rel.components().count() + 1)
                    .unwrap_or(1)
            });
            let create_editor = match &self.sidebar_edit {
                Some(SidebarEditMode::CreateFile { editor, .. })
                | Some(SidebarEditMode::CreateDir { editor, .. }) => Some(editor.clone()),
                _ => None,
            };
            let create_is_dir =
                matches!(&self.sidebar_edit, Some(SidebarEditMode::CreateDir { .. }));

            let items = items.unwrap_or_default();
            let render_all_items = self.sidebar_edit.is_some();

            let mut next_at_depth: HashMap<usize, usize> = HashMap::new();
            let mut item_guide_levels: Vec<HashSet<usize>> = vec![HashSet::new(); items.len()];
            for i in (0..items.len()).rev() {
                let depth = items[i].depth;
                next_at_depth.insert(depth, i);
                let mut levels = HashSet::new();
                for level in 1..=depth {
                    if let Some(&next_idx) = next_at_depth.get(&level)
                        && next_idx > i
                    {
                        levels.insert(level);
                    }
                }
                item_guide_levels[i] = levels;
            }

            if render_all_items {
                let mut injected = false;
                if create_parent.as_ref() == Some(&self.sidebar_root)
                    && let Some(ref editor) = create_editor
                {
                    tree_items.push(self.render_sidebar_edit_row(editor, 1, create_is_dir, cx));
                    injected = true;
                }
                for (idx, item) in items.iter().enumerate() {
                    tree_items.push(self.render_sidebar_item(
                        item.clone(),
                        &item_guide_levels[idx],
                        cx,
                    ));
                    if let Some(ref parent) = create_parent
                        && !injected
                        && item.is_dir
                        && &item.path == parent
                        && let Some(ref editor) = create_editor
                    {
                        let depth = create_depth.unwrap_or(1);
                        tree_items.push(self.render_sidebar_edit_row(
                            editor,
                            depth,
                            create_is_dir,
                            cx,
                        ));
                        injected = true;
                    }
                }
                if !injected && let (Some(depth), Some(editor)) = (create_depth, &create_editor) {
                    tree_items.push(self.render_sidebar_edit_row(editor, depth, create_is_dir, cx));
                }
            } else {
                let item_height = self.sidebar_item_height_px();
                let (visible_start, visible_end, top_spacer_height, bottom_spacer_height) = self
                    .visible_uniform_list_range(
                        &self.sidebar_scroll_handle,
                        self.sidebar_scroll_prefix_height(true),
                        item_height,
                        items.len(),
                        12,
                    );
                if top_spacer_height > 0.0 {
                    tree_items.push(
                        div()
                            .w_full()
                            .h(px(top_spacer_height))
                            .flex_none()
                            .into_any_element(),
                    );
                }
                for idx in visible_start..visible_end {
                    tree_items.push(self.render_sidebar_item(
                        items[idx].clone(),
                        &item_guide_levels[idx],
                        cx,
                    ));
                }
                if bottom_spacer_height > 0.0 {
                    tree_items.push(
                        div()
                            .w_full()
                            .h(px(bottom_spacer_height))
                            .flex_none()
                            .into_any_element(),
                    );
                }
            }
        }

        tree_items.insert(
            0,
            div()
                .w_full()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .py(px(self.scaled(ss.sidebar_logo_padding_y_px)))
                .child(
                    img(helpers::resolve_asset_image_path(LOGO_SVG_PATH))
                        .w(px(self.scaled(ss.sidebar_logo_size_px)))
                        .h(px(self.scaled(ss.sidebar_logo_size_px))),
                )
                .into_any_element(),
        );

        if has_root {
            tree_items.insert(1, self.render_starred_sidebar_section(&starred_files, cx));
            tree_items.insert(2, header);
        }

        tree_items.push(
            div()
                .w_full()
                .h(px(self.scaled(ss.sidebar_bottom_spacer_height_px)))
                .flex_none()
                .into_any_element(),
        );

        let logo_area_height =
            self.scaled(ss.sidebar_logo_size_px + 2.0 * ss.sidebar_logo_padding_y_px);
        let scroll_y = -f32::from(self.sidebar_scroll_handle.offset().y);
        let sticky_headers_visible = has_root && scroll_y >= logo_area_height;

        div()
            .id("sidebar")
            .w(px(self.sidebar_layout_width()))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .relative()
            .bg(ss.sidebar_bg)
            .when(self.sidebar_resizing, |this| {
                this.border_r(px(self.scaled(ss.sidebar_resize_active_indicator_width_px)))
                    .border_color(ss.sidebar_resize_active_indicator_color)
            })
            .when(!self.sidebar_resizing, |this| {
                this.border_r(px(ss.sidebar_resize_indicator_width_px))
                    .border_color(ss.sidebar_resize_indicator_color)
            })
            .child(top_spacer)
            .when(
                self.workspace_search_visible,
                |this: gpui::Stateful<gpui::Div>| this.child(self.render_workspace_search(cx)),
            )
            .when(
                !self.workspace_search_visible,
                |this: gpui::Stateful<gpui::Div>| {
                    this.child(
                        div()
                            .id("sidebar-tree")
                            .flex_1()
                            .overflow_y_scroll()
                            .track_scroll(&self.sidebar_scroll_handle)
                            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                                let total_items = this.sidebar_items().len();
                                this.maybe_extend_sidebar_items(total_items, cx);
                            }))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.focused_sidebar_path = None;
                                    this.clear_sidebar_drag_state(cx);
                                    cx.notify();
                                }),
                            )
                            .on_mouse_down(MouseButton::Right, {
                                let root = self.sidebar_root.clone();
                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    this.open_sidebar_context_menu(
                                        root.clone(),
                                        true,
                                        event.position,
                                        cx,
                                    );
                                })
                            })
                            .drag_over::<DraggedSidebarItems>(move |style, _, _, _| style)
                            .on_drag_move({
                                let root = self.sidebar_root.clone();
                                cx.listener(
                                    move |this,
                                          event: &DragMoveEvent<DraggedSidebarItems>,
                                          _,
                                          cx| {
                                        if event.bounds.contains(&event.event.position) {
                                            this.update_sidebar_drag_autoscroll(
                                                event.event.position,
                                                cx,
                                            );

                                            if this.sidebar_drag_child_handled_same_event(
                                                event.event.position,
                                                &root,
                                            ) {
                                                return;
                                            }

                                            let dragged = event.drag(cx);
                                            let valid_move = Self::sidebar_drop_would_move(
                                                &dragged.paths,
                                                &root,
                                            );
                                            if !valid_move {
                                                this.clear_sidebar_drag_hover(cx);
                                                return;
                                            }
                                            if this.sidebar_drag_has_child_target(&root) {
                                                return;
                                            }
                                            this.update_sidebar_drag_expand_hover_dir(None, cx);
                                            this.update_sidebar_drag_target(Some(root.clone()), cx);
                                        }
                                    },
                                )
                            })
                            .on_drop({
                                let root = self.sidebar_root.clone();
                                cx.listener(move |this, dragged: &DraggedSidebarItems, _, cx| {
                                    let paths = dragged.paths.clone();
                                    this.clear_sidebar_drag_state(cx);
                                    this.move_sidebar_items(paths, root.clone(), cx);
                                })
                            })
                            .children(tree_items),
                    )
                },
            )
            .when(
                sticky_headers_visible && !self.workspace_search_visible,
                |this: gpui::Stateful<gpui::Div>| {
                    this.child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(self.scaled(ss.sidebar_top_spacer_height_px)))
                            .w_full()
                            .bg(ss.sidebar_bg)
                            .child(self.render_starred_sidebar_section(&starred_files, cx))
                            .child(self.render_sidebar_header(cx)),
                    )
                },
            )
            .when(show_workspace_bar, |this: gpui::Stateful<gpui::Div>| {
                this.child(self.render_workspace_bar(cx))
            })
            .into_any_element()
    }

    fn render_workspace_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let wss = &self.styles.workspace_switcher;
        let ws_name = self.sidebar_root_name().to_uppercase();
        let hover_bg = wss.ws_bar_hover_bg;
        let root_drag_over = self
            .sidebar_drag_hover_target
            .as_ref()
            .is_some_and(|target| target == &self.sidebar_root);

        div()
            .id("workspace-bar")
            .w_full()
            .h(px(self.scaled(wss.ws_bar_height_px)))
            .flex_none()
            .flex()
            .items_center()
            .px(px(self.scaled(wss.ws_bar_padding_x_px)))
            .gap(px(self.scaled(wss.ws_bar_gap_px)))
            .bg(if root_drag_over {
                hover_bg
            } else {
                wss.ws_bar_bg
            })
            .border_t(px(1.0))
            .border_color(wss.ws_bar_border_color)
            .cursor(CursorStyle::Arrow)
            .hover(move |s| s.bg(hover_bg))
            .child(
                svg()
                    .path(WORKSPACE_EXPAND_SVG_PATH)
                    .size(px(self.scaled(wss.ws_bar_icon_size_px)))
                    .text_color(wss.ws_bar_icon_color),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .text_size(px(self.scaled(wss.ws_bar_font_size_px)))
                    .text_color(wss.ws_bar_text_color)
                    .font_weight(FontWeight::BOLD)
                    .truncate()
                    .child(ws_name),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    let pos = event.position;
                    let menu_pos = gpui::point(pos.x, pos.y - gpui::px(20.0));
                    this.open_workspace_menu(menu_pos, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    let pos = event.position;
                    let menu_pos = gpui::point(pos.x, pos.y - gpui::px(20.0));
                    this.open_workspace_menu(menu_pos, cx);
                }),
            )
            .drag_over::<DraggedSidebarItems>({
                let root = self.sidebar_root.clone();
                move |style, dragged, _, _| {
                    if Self::sidebar_drop_would_move(&dragged.paths, &root) {
                        style.bg(hover_bg)
                    } else {
                        style
                    }
                }
            })
            .on_drag_move({
                let root = self.sidebar_root.clone();
                cx.listener(
                    move |this, event: &DragMoveEvent<DraggedSidebarItems>, _, cx| {
                        if event.bounds.contains(&event.event.position) {
                            this.update_sidebar_drag_autoscroll(event.event.position, cx);
                            let dragged = event.drag(cx);
                            let valid_move = Self::sidebar_drop_would_move(&dragged.paths, &root);
                            if valid_move {
                                this.update_sidebar_drag_expand_hover_dir(None, cx);
                                this.update_sidebar_drag_target(Some(root.clone()), cx);
                            } else {
                                this.clear_sidebar_drag_hover(cx);
                            }
                        }
                    },
                )
            })
            .on_drop({
                let root = self.sidebar_root.clone();
                cx.listener(move |this, dragged: &DraggedSidebarItems, _, cx| {
                    let paths = dragged.paths.clone();
                    this.clear_sidebar_drag_state(cx);
                    this.move_sidebar_items(paths, root.clone(), cx);
                })
            })
            .into_any_element()
    }
}
