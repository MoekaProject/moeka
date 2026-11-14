use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, Context, CursorStyle, DragMoveEvent, ExternalPaths, MouseButton, MouseDownEvent,
    SharedString, Transformation, div, prelude::*, px, radians,
};

use super::*;

const FILETREE_FILE_SVG_PATH: &str = "assets/icons/filetree/file.svg";

impl EditorView {
    fn render_filtered_sidebar_name_label(&self, name: &str, color: gpui::Hsla) -> AnyElement {
        let ranges = self.sidebar_filter_match_ranges(name);
        if ranges.is_empty() {
            return self.render_sidebar_name_label(name, color);
        }
        let ss = &self.styles.sidebar;
        let (base, suffix) = split_file_name_for_display(name);
        let styled = |text: &str, offset: usize| {
            gpui::StyledText::new(text.to_owned()).with_highlights(ranges.iter().filter_map(
                |range| {
                    let start = range.start.max(offset);
                    let end = range.end.min(offset + text.len());
                    (start < end).then(|| {
                        (
                            start - offset..end - offset,
                            gpui::HighlightStyle {
                                color: Some(ss.sidebar_filter_match_color),
                                ..Default::default()
                            },
                        )
                    })
                },
            ))
        };
        crate::ui::decoration::file_name(self.styles.sidebar_name(color), self.ui_scale)
            .child(crate::ui::decoration::file_name_base_content(styled(
                base, 0,
            )))
            .when_some(suffix, |this, suffix| {
                let mut suffix_color = color;
                suffix_color.a *= ss.sidebar_item_suffix_opacity;
                this.child(
                    crate::ui::decoration::suffix_content(1.0, styled(suffix, base.len()))
                        .text_color(suffix_color),
                )
            })
            .into_any_element()
    }

    fn render_sidebar_name_label(&self, name: &str, color: gpui::Hsla) -> AnyElement {
        let ss = &self.styles.sidebar;
        let (base, suffix) = split_file_name_for_display(name);
        crate::ui::decoration::file_name(self.styles.sidebar_name(color), self.ui_scale)
            .child(crate::ui::decoration::file_name_base(base.to_string()))
            .when_some(suffix, |this, suffix| {
                this.child(crate::ui::decoration::suffix(
                    ss.sidebar_item_suffix_opacity,
                    suffix.to_string(),
                ))
            })
            .into_any_element()
    }

    /// Extra right margin that lines a header badge up with the file tree rows,
    /// whose own right edge sits inside a 1px row border plus the item padding.
    fn sidebar_header_badge_right_margin(&self) -> f32 {
        let ss = &self.styles.sidebar;
        (self.scaled(ss.sidebar_item_padding_right_px) + self.ui_border_width(1.0)
            - self.scaled(ss.sidebar_header_padding_right_px))
        .max(0.0)
    }

    /// Rounded pill showing how many documents live under a directory.
    /// Shared by file tree rows, the workspace header, the starred header and
    /// the FileList header.
    pub(super) fn render_sidebar_count_badge(
        &self,
        count: usize,
        left_margin: bool,
        right_margin_px: f32,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        crate::ui::typography::badge(self.styles.sidebar_count_badge(), self.ui_scale)
            .when(left_margin, |this| {
                this.ml(px(self.scaled(ss.sidebar_count_badge_margin_left_px)))
            })
            .when(right_margin_px > 0.0, |this| this.mr(px(right_margin_px)))
            .min_w(px(self.scaled(ss.sidebar_count_badge_min_width_px)))
            .h(px(self.scaled(ss.sidebar_count_badge_height_px)))
            .child(count.to_string())
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
            return crate::ui::decoration::icon(
                crate::style::IconStyle::new(
                    f32::from(px(self.scaled(ss.sidebar_dir_icon_size_px))),
                    color,
                ),
                FOLDER_CLOSED_SVG_PATH,
            )
            .into_any_element();
        }

        if let Some(label) = file_kind_badge_text_for_name(name) {
            return crate::ui::typography::badge(
                self.styles.sidebar_kind_badge(false),
                self.ui_scale,
            )
            .min_w(px(self.scaled(ss.sidebar_item_badge_min_width_px)))
            .h(px(self.scaled(ss.sidebar_item_badge_height_px)))
            .child(label)
            .into_any_element();
        }

        crate::ui::decoration::icon(
            crate::style::IconStyle::new(
                f32::from(px(self.scaled(ss.sidebar_item_icon_size_px + 4.0))),
                color,
            ),
            if file_kind_has_indicator_for_name(name) {
                MARKDOWN_SVG_PATH
            } else {
                FILETREE_FILE_SVG_PATH
            },
        )
        .flex_none()
        .into_any_element()
    }

    pub(super) fn render_sidebar_header_action_button(
        &self,
        id: &'static str,
        icon_path: &'static str,
        tooltip_title: &'static str,
        shortcut: Option<SharedString>,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &MouseDownEvent, &mut gpui::Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        let button = crate::ui::button::icon_button(
            id,
            self.styles.sidebar_header_action(),
            Default::default(),
            self.ui_scale,
        )
        .debug_selector(|| id.to_string())
        .child(crate::ui::decoration::icon(
            crate::style::IconStyle::new(
                f32::from(px(self.scaled(ss.sidebar_header_action_icon_size_px))),
                ss.sidebar_header_icon_color,
            ),
            icon_path,
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                handler(this, event, window, cx);
            }),
        );
        tooltip::with_tooltip(button, cx.entity().clone(), tooltip_title, shortcut)
    }

    fn render_sidebar_top_button(
        &self,
        id: &'static str,
        icon_path: &'static str,
        is_active: bool,
        tooltip_title: &'static str,
        shortcut: Option<SharedString>,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &MouseDownEvent, &mut gpui::Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        let icon_color = if is_active {
            ss.sidebar_top_button_active_color
        } else {
            ss.sidebar_top_button_inactive_color
        };
        let button = crate::ui::button::icon_button(
            id,
            self.styles.sidebar_top_action(is_active),
            Default::default(),
            self.ui_scale,
        )
        .w(px(self.scaled(ss.sidebar_top_button_width_px)))
        .child(crate::ui::decoration::icon(
            crate::style::IconStyle::new(
                f32::from(px(self.scaled(ss.sidebar_top_button_icon_size_px))),
                icon_color,
            ),
            icon_path,
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                handler(this, event, window, cx);
            }),
        );
        tooltip::with_tooltip(button, cx.entity().clone(), tooltip_title, shortcut)
    }

    pub(super) fn render_workspace_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let ss = &self.styles.sidebar;
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(self.scaled(ss.sidebar_header_action_gap_px)))
            .child(self.render_sidebar_header_action_button(
                "sidebar-header-sort",
                FILETREE_SORT_SVG_PATH,
                crate::i18n::sidebar_action_label(self.ui_language, "sort"),
                None,
                cx,
                |this, event, _window, cx| {
                    this.open_sidebar_sort_menu(event.position, cx);
                },
            ))
            .child(self.render_sidebar_header_action_button(
                "sidebar-header-create",
                FILETREE_ADD_SVG_PATH,
                crate::i18n::sidebar_action_label(self.ui_language, "create"),
                None,
                cx,
                |this, event, _window, cx| {
                    this.open_sidebar_create_menu(event.position, cx);
                },
            ))
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
            .child(crate::ui::decoration::icon(
                crate::style::IconStyle::new(
                    f32::from(px(self.scaled(ss.sidebar_header_star_icon_size_px))),
                    icon_color,
                ),
                FILETREE_STAR_SVG_PATH,
            ))
            .child(
                crate::ui::typography::label(
                    crate::style::TextStyle::new(
                        f32::from(px(self.scaled(ss.sidebar_header_font_size_px))),
                        title_color,
                    ),
                    1.0,
                )
                .flex_1()
                .min_w(px(0.0))
                .overflow_hidden()
                .truncate()
                .child(self.t(crate::i18n::TextKey::StarredFiles)),
            )
            .when(is_collapsed && !starred_files.is_empty(), |this| {
                this.child(self.render_sidebar_count_badge(
                    starred_files.len(),
                    true,
                    self.sidebar_header_badge_right_margin(),
                ))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle_starred_section(cx);
                }),
            );

        div()
            .id("sidebar-starred-section")
            .debug_selector(|| "sidebar-starred-section".into())
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .child(header)
            .when(
                !is_collapsed || self.sidebar_starred_motion.is_some(),
                |this| {
                    let visible = super::list_viewport::fixed_list_window(
                        &self.sidebar_starred_scroll_handle,
                        self.sidebar_starred_list_height(),
                        0.0,
                        self.sidebar_item_height_px(),
                        starred_files.len(),
                        8,
                        16,
                    );
                    this.child(
                        div()
                            .id("sidebar-starred-list")
                            .debug_selector(|| "sidebar-starred-list".into())
                            .h(px(self.sidebar_starred_list_height()))
                            .flex_none()
                            .min_h(px(0.0))
                            .overflow_y_scroll()
                            .track_scroll(&self.sidebar_starred_scroll_handle)
                            .child(div().h(px(visible.top_spacer_px)).flex_none())
                            .children(
                                starred_files
                                    .iter()
                                    .enumerate()
                                    .skip(visible.render.start)
                                    .take(visible.render.len())
                                    .map(|(index, path)| {
                                        let row =
                                            self.render_starred_sidebar_item(path.clone(), cx);
                                        if let Some(reveal) = self.starred_reveal() {
                                            let row = div()
                                                .w_full()
                                                .flex_none()
                                                .child(row)
                                                .when(
                                                    is_collapsed,
                                                    crate::ui::interaction::inert_activation,
                                                )
                                                .into_any_element();
                                            reveal.render_row(
                                                index,
                                                self.sidebar_item_height_px(),
                                                row,
                                            )
                                        } else {
                                            row
                                        }
                                    })
                                    .collect::<Vec<_>>(),
                            )
                            .child(div().h(px(visible.bottom_spacer_px)).flex_none()),
                    )
                },
            )
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
        let is_focused = self.focused_sidebar_path.as_deref() == Some(path.as_path())
            && self.focused_starred_path.as_deref() == Some(path.as_path());
        let normalized_path = Self::normalize_sidebar_path(&path);
        let icon_color = if is_selected {
            ss.sidebar_item_selected_icon_color
        } else {
            ss.sidebar_item_icon_color
        };
        let label_color = if is_selected {
            ss.sidebar_item_selected_text_color
        } else {
            ss.sidebar_item_text_color
        };
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
        let name_label = tooltip::with_tooltip(
            self.render_filtered_sidebar_name_label(&name, label_color),
            cx.entity().clone(),
            normalized_path.display().to_string(),
            None::<&'static str>,
        );

        div()
            .w_full()
            .group("sidebar-starred-item")
            .child(
                crate::ui::list::tree_row(
                    self.styles.sidebar_tree_row(),
                    crate::style::TreeRowState {
                        selected: is_selected,
                        focused: is_focused,
                        resizing: self.sidebar_resizing,
                        ..Default::default()
                    },
                    self.ui_scale,
                )
                .id(SharedString::from(format!(
                    "sidebar-starred-{}",
                    path.display()
                )))
                .debug_selector(|| format!("sidebar-starred-{}", path.display()))
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
                        .child(name_label)
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
                                .child(crate::ui::decoration::icon(
                                    crate::style::IconStyle::new(
                                        f32::from(px(
                                            self.scaled(ss.sidebar_starred_item_close_icon_size_px)
                                        )),
                                        ss.sidebar_header_icon_color,
                                    ),
                                    FILETREE_CLOSE_SVG_PATH,
                                ))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        let normalized =
                                            Self::normalize_sidebar_path(&remove_star_path);
                                        if this.app_settings.remove_starred_file(&normalized) {
                                            this.save_settings_in_background(cx);
                                        }
                                        cx.notify();
                                    }),
                                ),
                        ),
                )
                .on_click({
                    cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                        cx.stop_propagation();
                        let mods = event.modifiers();
                        let secondary = mods.secondary();
                        let has_modifier = secondary || mods.shift;
                        if !has_modifier {
                            this.activate_starred_workspace(&click_path, cx);
                        }
                        this.select_starred_sidebar_item(&click_path, mods.shift, secondary);
                        if !has_modifier {
                            this.set_file_list_dir_for_entry(&click_path, false, cx);
                            this.suppress_next_sidebar_sync_path = Some(click_path.clone());
                            if event.click_count() >= 2 {
                                this.focused_sidebar_path = None;
                                this.focused_workspace_root = None;
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
                            this.select_starred_sidebar_item(&context_menu_path, false, false);
                        }
                        this.focused_sidebar_path = Some(context_menu_path.clone());
                        this.open_starred_sidebar_context_menu(
                            context_menu_path.clone(),
                            event.position,
                            cx,
                        );
                    }),
                ),
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
            self.scaled(self.styles.sidebar.sidebar_root_padding_x_px)
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
        let indicator_name = if !is_dir
            && matches!(self.sidebar_edit, Some(SidebarEditMode::CreateFile { .. }))
            && !file_kind_has_indicator_for_name(current_name.trim())
        {
            "untitled.md"
        } else {
            current_name.trim()
        };

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
                        crate::ui::list::tree_fold_slot(
                            px(fold_slot_width),
                            px(self.scaled(ss.sidebar_item_fold_container_height_px)),
                        )
                        .child(if show_fold_icon {
                            crate::ui::decoration::icon(
                                crate::style::IconStyle::new(
                                    f32::from(px(self.scaled(ss.sidebar_item_fold_icon_size_px))),
                                    ss.sidebar_item_fold_icon_color,
                                ),
                                FILETREE_FOLD_SVG_PATH,
                            )
                            .into_any_element()
                        } else {
                            div()
                                .w(px(self.scaled(ss.sidebar_item_fold_icon_size_px)))
                                .into_any_element()
                        }),
                    )
                    .child(self.render_sidebar_edit_indicator(
                        indicator_name,
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
            && !self.sidebar_edit_in_file_list
            && *rp == item.path
        {
            return self.render_sidebar_edit_row(editor, item.depth, item.is_dir, cx);
        }

        let is_selected = self.is_sidebar_item_selected(&item.path);
        let path = item.path.clone();
        let is_dir = item.is_dir;
        let ss = &self.styles.sidebar;
        let normalized_path = Self::normalize_sidebar_path(&path);
        let shift = ss.sidebar_item_left_shift_px;
        let is_root_level = item.depth == 0;
        let left_padding = if is_root_level {
            self.scaled(self.styles.sidebar.sidebar_root_padding_x_px)
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
        let is_cut_item = self.file_clipboard.as_ref().is_some_and(|clipboard| {
            clipboard.is_cut
                && clipboard.paths.iter().any(|cut_path| {
                    let cut_path = Self::normalize_sidebar_path(cut_path);
                    normalized_path == cut_path || normalized_path.starts_with(&cut_path)
                })
        });
        let icon_color = if is_selected {
            ss.sidebar_item_selected_icon_color
        } else {
            ss.sidebar_item_icon_color
        };
        let label_color = if is_selected {
            ss.sidebar_item_selected_text_color
        } else {
            ss.sidebar_item_text_color
        };
        let is_focused = self.focused_sidebar_path.as_deref() == Some(path.as_path())
            && self.focused_starred_path.is_none();

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
                    - fold_w / 2.0
                    - guide_width / 2.0;
                let segment_step = dash_len + gap_len;
                let count = (item_height / segment_step).ceil() as usize;
                (0..count).map(move |i| {
                    let y = i as f32 * segment_step;
                    let h = dash_len.min(item_height - y);
                    crate::ui::decoration::guide_segment(guide_x, y, guide_width, h, guide_color)
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
        let dir_file_count = if item.is_dir && self.file_list_visible {
            self.sidebar_dir_file_counts
                .get(&item.path)
                .copied()
                .filter(|count| *count > 0)
        } else {
            None
        };
        let is_expanded = item.is_expanded;
        let closing = self.sidebar_row_is_closing(&item.path);
        let indicator = if item.is_dir {
            crate::ui::decoration::icon(
                crate::style::IconStyle::new(
                    f32::from(px(self.scaled(ss.sidebar_dir_icon_size_px))),
                    icon_color,
                ),
                if is_expanded {
                    FOLDER_OPEN_SVG_PATH
                } else {
                    FOLDER_CLOSED_SVG_PATH
                },
            )
            .into_any_element()
        } else if let Some(label) = file_kind_badge_text(&item.path) {
            crate::ui::typography::badge(
                self.styles.sidebar_kind_badge(is_selected || is_focused),
                self.ui_scale,
            )
            .min_w(px(self.scaled(ss.sidebar_item_badge_min_width_px)))
            .h(px(self.scaled(ss.sidebar_item_badge_height_px)))
            .child(label)
            .into_any_element()
        } else {
            // Keep generic and Markdown icons in the same non-shrinking column.
            crate::ui::decoration::icon(
                crate::style::IconStyle::new(
                    f32::from(px(self.scaled(ss.sidebar_item_icon_size_px + 4.0))),
                    icon_color,
                ),
                if file_kind_has_indicator(&item.path) {
                    MARKDOWN_SVG_PATH
                } else {
                    FILETREE_FILE_SVG_PATH
                },
            )
            .flex_none()
            .into_any_element()
        };

        crate::ui::list::tree_row(
            self.styles.sidebar_tree_row(),
            crate::style::TreeRowState {
                selected: is_selected,
                focused: is_focused,
                drag_over: is_drag_over,
                cut: is_cut_item,
                resizing: self.sidebar_resizing,
            },
            self.ui_scale,
        )
        .id(SharedString::from(format!("sidebar-{}", path.display())))
        .debug_selector(|| format!("sidebar-{}", path.display()))
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
                    crate::ui::list::tree_fold_slot(
                        px(fold_slot_width),
                        px(self.scaled(ss.sidebar_item_fold_container_height_px)),
                    )
                    .child(if show_fold_icon {
                        crate::ui::motion::transition(
                            "directory-disclosure-motion",
                            crate::ui::decoration::icon(
                                crate::style::IconStyle::new(
                                    f32::from(px(self.scaled(ss.sidebar_item_fold_icon_size_px))),
                                    ss.sidebar_item_fold_icon_color,
                                ),
                                FILETREE_FOLD_SVG_PATH,
                            ),
                            if is_expanded { 1.0 } else { 0.0 },
                            None,
                            self.styles.ui.spec.tree_transition_duration(is_expanded),
                            self.styles.ui.spec.easing,
                            |icon, progress| {
                                icon.with_transformation(Transformation::rotate(radians(
                                    progress * std::f32::consts::FRAC_PI_2,
                                )))
                            },
                        )
                        .into_any_element()
                    } else {
                        div()
                            .w(px(self.scaled(ss.sidebar_item_fold_icon_size_px)))
                            .into_any_element()
                    }),
                )
                .child(indicator)
                .child(self.render_filtered_sidebar_name_label(&item.name, label_color))
                .when_some(dir_file_count, |this, count| {
                    this.child(self.render_sidebar_count_badge(count, true, 0.0))
                }),
        )
        .when(closing, crate::ui::interaction::inert_activation)
        .when(!closing, |row| {
            row.on_click({
                let path = path.clone();
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    let mods = event.modifiers();
                    let secondary = mods.secondary();
                    let has_modifier = secondary || mods.shift;
                    this.sidebar_select_item(&path, mods.shift, secondary);
                    if !has_modifier {
                        this.set_file_list_dir_for_entry(&path, is_dir, cx);
                        if is_dir {
                            this.toggle_sidebar_directory(path.clone(), cx);
                        } else {
                            this.suppress_next_sidebar_sync_path = Some(path.clone());
                            if event.click_count() >= 2 {
                                this.focused_sidebar_path = None;
                                this.focused_workspace_root = None;
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
            .when(!self.sidebar_resizing, |el| {
                el.on_drag(DraggedSidebarItems { paths: drag_paths }, {
                    let label = drag_label;
                    let count = drag_count;
                    let ghost_style = self.styles.drag_preview(false, self.ui_scale);
                    move |_dragged, offset, _window, cx| {
                        cx.new(|_cx| DraggedSidebarItemsView {
                            label: label.clone(),
                            count,
                            style: ghost_style.clone(),
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
            .drag_over::<ExternalPaths>({
                let drop_path = item_drop_target.clone();
                move |style, dragged, _, _| {
                    if Self::sidebar_drop_would_copy(dragged.paths(), &drop_path) {
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
            .on_drop({
                let drop_path = item_drop_target.clone();
                cx.listener(move |this, dragged: &ExternalPaths, _, cx| {
                    let paths = dragged.paths().to_vec();
                    this.clear_sidebar_drag_state(cx);
                    this.copy_external_paths_into_dir(&paths, drop_path.clone(), cx);
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
            .on_drag_move(cx.listener({
                let path = path.clone();
                let drop_path = item_drop_target.clone();
                move |this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                    if !event.bounds.contains(&event.event.position) {
                        return;
                    }

                    cx.stop_propagation();
                    this.sidebar_drag_child_hover_position =
                        Some(Self::sidebar_drag_position_key(event.event.position));
                    this.update_sidebar_drag_autoscroll(event.event.position, cx);

                    let dragged = event.drag(cx);
                    let valid_copy = Self::sidebar_drop_would_copy(dragged.paths(), &drop_path);
                    let desired_expand_hover = is_dir.then(|| path.clone());

                    this.update_sidebar_drag_expand_hover_dir(desired_expand_hover, cx);

                    if !valid_copy {
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
        })
        .into_any_element()
    }

    pub(crate) fn render_sidebar(&mut self, refresh: bool, cx: &mut Context<Self>) -> AnyElement {
        if !cx.has_active_drag() {
            self.workspace_drop_target = None;
            self.workspace_drag_scroll = None;
        }
        let has_root = !self.sidebar_root.as_os_str().is_empty();
        if has_root {
            self.ensure_sidebar_filter_input(cx);
            self.ensure_sidebar_filter_current(cx);
        }
        if has_root && refresh {
            self.refresh_sidebar_directories_if_needed(cx);
            if self.expanded_dirs.contains(&self.sidebar_root)
                && !self
                    .sidebar_directory_children
                    .contains_key(&self.sidebar_root)
            {
                self.schedule_sidebar_directory_load(self.sidebar_root.clone(), cx);
            }
        }
        let starred_files = self.visible_starred_sidebar_files();
        self.prepare_starred_motion(&starred_files);
        let items = if has_root {
            Some(self.sidebar_items())
        } else {
            None
        };
        if has_root && self.file_list_visible && refresh {
            let mut dirs = vec![self.sidebar_root.clone()];
            if let Some(items) = items.as_ref() {
                dirs.extend(
                    items
                        .iter()
                        .filter(|item| item.is_dir)
                        .map(|item| item.path.clone()),
                );
            }
            self.request_sidebar_dir_file_counts(dirs, cx);
        }
        let ss = self.styles.sidebar.clone();

        let top_spacer = div()
            .id("sidebar-top-spacer")
            .w_full()
            .h(px(self.tab_bar_height()))
            .flex_none()
            .flex()
            .items_center()
            .justify_start()
            .when(ss.sidebar_top_buttons_visible, |this| {
                this.pl(px(
                    self.window_controls_leading_inset() + self.tab_bar_height()
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        // Keeps the buttons out of the drag area behind them.
                        .occlude()
                        .gap(px(self.scaled(ss.sidebar_top_button_gap_px)))
                        .child(self.render_sidebar_top_button(
                            "sidebar-top-filetree",
                            "assets/icons/workspace/filetree.svg",
                            !self.workspace_search_visible,
                            crate::i18n::sidebar_action_label(self.ui_language, "files"),
                            None,
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
                            "assets/icons/navigation/search.svg",
                            self.workspace_search_visible,
                            crate::i18n::sidebar_action_label(self.ui_language, "search"),
                            Some(crate::editor::shortcut::hint("secondary-shift-f")),
                            cx,
                            |this, _event, window, cx| {
                                this.toggle_workspace_search(window, cx);
                            },
                        )),
                )
            })
            .child({
                div().w_full().flex().items_center().justify_end().child(
                    div()
                        .occlude()
                        .flex()
                        .items_center()
                        .pr(px(self.scaled(ss.sidebar_top_button_padding_right_px)))
                        .child(self.render_sidebar_header_action_button(
                            "sidebar-settings",
                            "assets/icons/appearance/setting.svg",
                            self.t(crate::i18n::TextKey::SettingsTitle),
                            None,
                            cx,
                            |this, _, _, cx| this.open_settings_tab(cx),
                        ))
                        .child(self.render_sidebar_header_action_button(
                            "sidebar-help",
                            DOC_SVG_PATH,
                            self.t(crate::i18n::TextKey::HelpManual),
                            None,
                            cx,
                            |this, _, _, cx| this.open_welcome_tab(cx),
                        ))
                        .child(self.render_sidebar_header_action_button(
                            "sidebar-add-workspace",
                            FILETREE_ADD_SVG_PATH,
                            self.t(crate::i18n::TextKey::OpenFolder),
                            None,
                            cx,
                            |this, _, _, cx| this.pick_workspace_folder(cx),
                        ))
                        .child(self.render_sidebar_top_button(
                            "sidebar-top-file-list-toggle",
                            "assets/icons/workspace/file-list-toggle.svg",
                            self.file_list_visible,
                            crate::i18n::sidebar_action_label(self.ui_language, "file_list"),
                            None,
                            cx,
                            |this, _event, _window, cx| {
                                this.toggle_file_list_visible(cx);
                            },
                        )),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, event: &MouseDownEvent, window, _cx| {
                    if event.click_count >= 2 {
                        crate::platform::window_chrome::toggle_maximize(window);
                    } else {
                        crate::platform::window_chrome::start_drag(window);
                    }
                }),
            );

        let mut tree_items: Vec<AnyElement> = Vec::new();

        if let Some(error) = &self.sidebar_error {
            tree_items.push(
                crate::ui::typography::label(
                    crate::style::TextStyle::new(
                        f32::from(px(self.scaled(ss.sidebar_error_font_size_px))),
                        ss.sidebar_error_text_color,
                    ),
                    1.0,
                )
                .w_full()
                .px(px(self.scaled(ss.sidebar_error_padding_x_px)))
                .py(px(self.scaled(ss.sidebar_error_padding_y_px)))
                .child(format!(
                    "{}: {error}",
                    self.t(crate::i18n::TextKey::FileOperationFailed)
                ))
                .into_any_element(),
            );
        }
        if !has_root {
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
                        crate::ui::typography::label(
                            crate::style::TextStyle::new(
                                f32::from(px(self.scaled(ss.sidebar_empty_btn_font_size_px))),
                                ss.sidebar_empty_btn_text_color,
                            ),
                            1.0,
                        )
                        .px(px(self.scaled(ss.sidebar_empty_btn_padding_x_px)))
                        .py(px(self.scaled(ss.sidebar_empty_btn_padding_y_px)))
                        .rounded_full()
                        .border(px(ss.sidebar_empty_btn_border_width_px))
                        .border_color(ss.sidebar_empty_btn_border_color)
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
            let create_parent = if self.sidebar_edit_in_file_list {
                None
            } else {
                match &self.sidebar_edit {
                    Some(SidebarEditMode::CreateFile { parent, .. })
                    | Some(SidebarEditMode::CreateDir { parent, .. }) => Some(parent.clone()),
                    _ => None,
                }
            };
            let create_depth = create_parent.as_ref().map(|p| {
                p.strip_prefix(&self.sidebar_root)
                    .map(|rel| rel.components().count() + 1)
                    .unwrap_or(1)
            });
            let create_editor = if self.sidebar_edit_in_file_list {
                None
            } else {
                match &self.sidebar_edit {
                    Some(SidebarEditMode::CreateFile { editor, .. })
                    | Some(SidebarEditMode::CreateDir { editor, .. }) => Some(editor.clone()),
                    _ => None,
                }
            };
            let create_is_dir =
                matches!(&self.sidebar_edit, Some(SidebarEditMode::CreateDir { .. }));

            let items = items.unwrap_or_default();
            if let Some(reveal_path) = self.pending_sidebar_reveal_path.clone()
                && let Some(index) = items.iter().position(|item| item.path == reveal_path)
            {
                if self.ensure_sidebar_item_minimally_visible(index, items.len()) {
                    cx.notify();
                }
                self.pending_sidebar_reveal_path = None;
            }
            if self.sidebar_directory_loading.is_empty()
                && !self.sidebar_directory_refresh_pending
                && !self.sidebar_items_dirty
            {
                self.resolve_pending_sidebar_scroll_anchor(&items);
            }
            // Restore before virtualization and prepaint, not after rows have
            // already captured their paint and hit-test positions.
            if let Some(top) = self.pending_sidebar_scroll_top.get() {
                let offset = self.sidebar_scroll_handle.offset();
                self.sidebar_scroll_handle
                    .set_offset(gpui::point(offset.x, px(-top)));
                if self.sidebar_directory_loading.is_empty()
                    && !self.sidebar_directory_refresh_pending
                    && !self.sidebar_items_dirty
                {
                    self.pending_sidebar_scroll_top.set(None);
                }
            }
            let render_all_items = self.sidebar_edit.is_some() && !self.sidebar_edit_in_file_list;
            let (items, tree_motion) = self.sidebar_tree_frame(items);
            let item_guide_levels = self.sidebar_motion_guide_levels(&items);

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
                let prefix_height = 0.0;
                let bottom_spacer = self.scaled(ss.sidebar_bottom_spacer_height_px);
                let content_height =
                    prefix_height + items.len() as f32 * item_height + bottom_spacer;
                // Clamp the scroll offset so collapsing the starred section or
                // filtering the tree (which shrinks the content) does not leave the
                // list scrolled past its end, which would make items jump upward.
                let viewport_height = f32::from(self.sidebar_scroll_handle.bounds().size.height);
                let refresh_scroll_pending = self.pending_sidebar_scroll_top.get().is_some()
                    || !self.sidebar_directory_loading.is_empty()
                    || self.sidebar_directory_refresh_pending;
                if viewport_height > 0.0 && !refresh_scroll_pending {
                    let scroll_y = -f32::from(self.sidebar_scroll_handle.offset().y);
                    let max_scroll = (content_height - viewport_height).max(0.0);
                    if scroll_y > max_scroll + 0.5 {
                        let offset = self.sidebar_scroll_handle.offset();
                        self.sidebar_scroll_handle
                            .set_offset(gpui::point(offset.x, px(-max_scroll)));
                    }
                }
                let list_window = super::list_viewport::fixed_list_window(
                    &self.sidebar_scroll_handle,
                    self.last_window_height,
                    prefix_height,
                    item_height,
                    items.len(),
                    12,
                    24,
                );
                let shifted_window = tree_motion.as_ref().map(|motion| {
                    super::list_viewport::fixed_list_window(
                        &self.sidebar_scroll_handle,
                        self.last_window_height,
                        prefix_height + motion.offset(item_height),
                        item_height,
                        items.len(),
                        12,
                        24,
                    )
                });
                let preload_range = shifted_window
                    .as_ref()
                    .unwrap_or(&list_window)
                    .preload
                    .clone();
                let preload_directories = items[preload_range]
                    .iter()
                    .filter(|item| {
                        refresh
                            && item.is_dir
                            && item.is_expanded
                            && !self.sidebar_row_is_closing(&item.path)
                    })
                    .map(|item| item.path.clone())
                    .collect::<Vec<_>>();
                for path in preload_directories {
                    self.schedule_sidebar_directory_load(path, cx);
                }
                let ranges = if let (Some(motion), Some(shifted)) = (&tree_motion, shifted_window) {
                    motion.ranges(list_window.render, shifted.render)
                } else {
                    vec![list_window.render]
                };
                let mut next = 0;
                for range in ranges {
                    if range.start > next {
                        tree_items.push(
                            div()
                                .w_full()
                                .h(px((range.start - next) as f32 * item_height))
                                .flex_none()
                                .into_any_element(),
                        );
                    }
                    next = range.end;
                    for idx in range {
                        let row = self.render_sidebar_item(
                            items[idx].clone(),
                            &item_guide_levels[idx],
                            cx,
                        );
                        tree_items.push(if let Some(motion) = &tree_motion {
                            motion.render_row(idx, item_height, row)
                        } else {
                            row
                        });
                    }
                }
                if next < items.len() {
                    tree_items.push(
                        div()
                            .w_full()
                            .h(px((items.len() - next) as f32 * item_height))
                            .flex_none()
                            .into_any_element(),
                    );
                }
            }
        }

        tree_items.push(
            div()
                .w_full()
                .h(px(self.scaled(ss.sidebar_bottom_spacer_height_px)))
                .flex_none()
                .into_any_element(),
        );

        let sidebar_scrollbar = self.render_scrollbar_for_handle(
            PanelScrollbarKind::Sidebar,
            &self.sidebar_scroll_handle,
            self.sidebar_scrollbar_track_height(),
            0.0,
            self.scaled(self.styles.self_drawn.scrollbar.right_margin_px),
            cx,
        );
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
            .when(has_root, |this| this.child(self.render_sidebar_filter(cx)))
            .when(has_root || !starred_files.is_empty(), |this| {
                this.child(self.render_starred_sidebar_section(&starred_files, cx))
            })
            .child(self.render_workspace_roots(true, cx))
            .when(has_root, |this| {
                this.child(self.render_workspace_root(self.sidebar_root.clone(), cx))
            })
            .child(
                div()
                    .id("sidebar-tree-region")
                    .debug_selector(|| "sidebar-tree-region".into())
                    .relative()
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("sidebar-tree")
                            .flex_1()
                            .min_h(px(0.0))
                            .overflow_y_scroll()
                            .track_scroll(&self.sidebar_scroll_handle)
                            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                                if this.sidebar_starred_motion.take().is_some() {
                                    cx.notify();
                                }
                                this.pending_sidebar_scroll_top.set(None);
                                this.pending_sidebar_scroll_anchor = None;
                                this.pending_sidebar_reveal_path = None;
                                this.reveal_panel_scrollbar(PanelScrollbarKind::Sidebar, cx);
                            }))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.focused_sidebar_path = None;
                                    this.focused_workspace_root = None;
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
                            .drag_over::<ExternalPaths>(move |style, _, _, _| style)
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
                            .on_drag_move({
                                let root = self.sidebar_root.clone();
                                cx.listener(
                                    move |this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
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
                                            let valid_copy = Self::sidebar_drop_would_copy(
                                                dragged.paths(),
                                                &root,
                                            );
                                            if !valid_copy {
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
                            .on_drop({
                                let root = self.sidebar_root.clone();
                                cx.listener(move |this, dragged: &ExternalPaths, _, cx| {
                                    let paths = dragged.paths().to_vec();
                                    this.clear_sidebar_drag_state(cx);
                                    this.copy_external_paths_into_dir(&paths, root.clone(), cx);
                                })
                            })
                            .children(tree_items),
                    )
                    .when_some(sidebar_scrollbar, |this, scrollbar| this.child(scrollbar)),
            )
            .child(self.render_workspace_roots(false, cx))
            .into_any_element()
    }
}
