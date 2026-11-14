use std::path::{Path, PathBuf};

use gpui::{AnyElement, Context, Focusable, MouseButton, Window, div, prelude::*, px};

use super::{
    EditorView,
    shortcut::{COMMAND_SPECS, CommandSpec, ShortcutRenderStyle, render_shortcut},
};
use crate::actions::ToggleCommandPalette;
use crate::i18n::TextKey;

impl EditorView {
    pub(super) fn open_command_workspace_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.command_palette_visible {
            self.close_command_palette(window, cx);
        }
        self.toggle_command_palette(window, cx);
        self.command_palette_workspace_mode = true;
        cx.notify();
    }

    pub(super) fn open_command_pane_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_palette_visible {
            self.close_command_palette(window, cx);
        }
        self.toggle_command_palette(window, cx);
        self.command_palette_pane_mode = true;
        cx.notify();
    }

    fn command_panes(&self) -> Vec<(usize, super::PaneId)> {
        if !self.command_palette_pane_mode {
            return Vec::new();
        }
        self.visible_pane_ids()
            .into_iter()
            .enumerate()
            .filter(|(_, pane)| *pane != self.current_pane_id())
            .collect()
    }

    pub(crate) fn on_toggle_command_palette(
        &mut self,
        _: &ToggleCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_command_palette(window, cx);
    }

    pub(crate) fn toggle_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_palette_visible {
            self.close_command_palette(window, cx);
            return;
        }

        self.capture_command_origin(window, cx);
        self.command_palette_pane_mode = false;
        self.command_palette_workspace_mode = false;
        if self.quick_open_visible {
            self.close_quick_open(window, cx);
        }

        if self.command_palette_editor.is_none() {
            let styles = self.quick_open_text_input_styles();
            let placeholder = self.t(TextKey::CommandPalettePlaceholder);
            let editor =
                cx.new(move |cx| crate::text_input::TextInput::new(styles, placeholder, cx));
            cx.subscribe(
                &editor,
                |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                    crate::text_input::TextInputEvent::Changed => {
                        this.command_palette_query = this
                            .command_palette_editor
                            .as_ref()
                            .map(|editor| editor.read(cx).text().to_string())
                            .unwrap_or_default();
                        this.command_palette_selected_index = 0;
                        this.command_palette_scroll_handle.scroll_to_item(0);
                        cx.notify();
                    }
                    crate::text_input::TextInputEvent::Submit
                    | crate::text_input::TextInputEvent::SubmitBackward => {
                        this.queue_selected_command(cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {
                        this.command_palette_visible = false;
                        this.reset_command_palette_state(cx);
                        this.pending_command_focus_restore = true;
                        cx.notify();
                    }
                    crate::text_input::TextInputEvent::MoveUp => {
                        this.move_command_palette_selection(-1, cx);
                    }
                    crate::text_input::TextInputEvent::MoveDown => {
                        this.move_command_palette_selection(1, cx);
                    }
                },
            )
            .detach();
            self.command_palette_editor = Some(editor);
        }

        self.command_palette_visible = true;
        self.command_palette_selected_index = 0;
        self.command_palette_scroll_handle.scroll_to_item(0);
        if let Some(editor) = &self.command_palette_editor {
            editor.update(cx, |input, cx| {
                input.styles = self.quick_open_text_input_styles();
                input.set_text("", cx);
            });
            editor.focus_handle(cx).focus(window);
        }
        cx.notify();
    }

    /// Clears query and selection so the next open starts fresh.
    pub(crate) fn reset_command_palette_state(&mut self, cx: &mut Context<Self>) {
        self.command_palette_pane_mode = false;
        self.command_palette_workspace_mode = false;
        self.command_palette_query.clear();
        self.command_palette_selected_index = 0;
        self.command_palette_scroll_handle.scroll_to_item(0);
        if let Some(editor) = self.command_palette_editor.clone() {
            editor.update(cx, |input, cx| input.set_text("", cx));
        }
        self.command_palette_query.clear();
    }

    pub(crate) fn close_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.command_palette_visible = false;
        self.reset_command_palette_state(cx);
        self.restore_command_focus(window, cx);
        cx.notify();
    }

    pub(crate) fn execute_pending_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "linux")]
        if std::mem::take(&mut self.clear_menu_origin_after_dispatch) {
            self.command_origin = None;
        }
        if self.pending_command_focus_restore {
            self.restore_command_focus(window, cx);
        }
        if let Some(pane) = self.command_palette_pending_pane.take() {
            self.command_palette_visible = false;
            self.on_move_tab_command(
                &crate::actions::MoveActiveTabToPane { pane: pane as u64 },
                window,
                cx,
            );
        }
        let Some(command) = self.command_palette_pending.take() else {
            return;
        };
        self.command_palette_visible = false;
        self.reset_command_palette_state(cx);
        self.on_run_command(
            &crate::actions::RunCommand {
                id: command.config_key().into(),
            },
            window,
            cx,
        );
    }

    fn visible_command_specs(&self) -> Vec<&'static CommandSpec> {
        if self.command_palette_pane_mode || self.command_palette_workspace_mode {
            return Vec::new();
        }
        let query = self.command_palette_query.trim().to_lowercase();
        COMMAND_SPECS
            .iter()
            .filter(|spec| spec.id.is_available())
            .filter(|spec| {
                if query.is_empty() {
                    return true;
                }
                query.split_whitespace().all(|term| {
                    self.command_title(spec).to_lowercase().contains(term)
                        || spec
                            .id
                            .english_description()
                            .to_lowercase()
                            .contains(term)
                        || spec.category.to_lowercase().contains(term)
                        || spec.localized_category(self.ui_language).to_lowercase().contains(term)
                        // The config key carries snake_case English aliases
                        // ("split_right", "find_in_files") worth searching.
                        || spec.id.config_key().replace('_', " ").contains(term)
                        || spec.id.config_key().contains(term)
                })
            })
            .collect()
    }

    /// Workspace roots offered as "switch workspace" rows below the commands.
    fn visible_command_palette_workspaces(&self) -> Vec<PathBuf> {
        if self.command_palette_pane_mode {
            return Vec::new();
        }
        let query = self.command_palette_query.trim().to_lowercase();
        let label = crate::i18n::command_label(self.ui_language, "switch_workspace").to_lowercase();
        self.workspaces
            .iter()
            .filter(|root| root.as_path() != self.sidebar_root.as_path())
            .filter(|root| {
                if query.is_empty() {
                    return true;
                }
                let name = Self::workspace_display_name(root).to_lowercase();
                query.split_whitespace().all(|term| {
                    name.contains(term) || label.contains(term) || "switch workspace".contains(term)
                })
            })
            .cloned()
            .collect()
    }

    fn workspace_display_name(root: &Path) -> String {
        root.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| root.display().to_string())
    }

    fn command_palette_item_count(&self) -> usize {
        self.visible_command_specs().len()
            + self.visible_command_palette_workspaces().len()
            + self.command_panes().len()
    }

    fn command_title(&self, spec: &CommandSpec) -> String {
        if spec.id == super::shortcut::CommandId::PinTab
            && self
                .active_tab_index()
                .is_some_and(|index| self.tabs[index].is_pinned)
        {
            return self.t(TextKey::UnpinTab).into();
        }
        spec.localized_title(self.ui_language)
    }

    fn move_command_palette_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let item_count = self.command_palette_item_count();
        if item_count == 0 {
            self.command_palette_selected_index = 0;
            cx.notify();
            return;
        }
        let index = (self.command_palette_selected_index as isize + delta)
            .clamp(0, item_count as isize - 1) as usize;
        self.command_palette_selected_index = index;
        self.command_palette_scroll_handle.scroll_to_item(index);
        cx.notify();
    }

    fn queue_selected_command(&mut self, cx: &mut Context<Self>) {
        if self.command_palette_pane_mode {
            if let Some((_, pane)) = self
                .command_panes()
                .get(self.command_palette_selected_index)
            {
                self.command_palette_pending_pane = Some(*pane);
                self.command_palette_visible = false;
                cx.notify();
            }
            return;
        }
        let specs = self.visible_command_specs();
        if let Some(spec) = specs.get(self.command_palette_selected_index) {
            if !self.command_state(spec.id).enabled {
                return;
            }
            self.command_palette_pending = Some(spec.id);
            self.command_palette_visible = false;
            cx.notify();
            return;
        }
        let workspace_index = self.command_palette_selected_index - specs.len();
        if let Some(root) = self
            .visible_command_palette_workspaces()
            .get(workspace_index)
            .cloned()
        {
            self.command_palette_visible = false;
            self.reset_command_palette_state(cx);
            self.switch_workspace(root, cx);
            self.pending_command_focus_restore = true;
            cx.notify();
        }
    }

    pub(crate) fn render_command_palette_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let styles = &self.styles.quick_open;
        let panel_width = self.scaled(styles.quick_open_panel_width_px);
        let sidebar_width = self.sidebar_width();
        let right_panel_width = self.right_panel_width();
        let editor_width =
            (self.last_window_width - sidebar_width - right_panel_width).max(panel_width);
        let panel_left = sidebar_width + (editor_width - panel_width) / 2.0;
        let panel_top =
            (self.last_window_height / 5.0).max(self.scaled(styles.quick_open_panel_top_offset_px));
        let title_size = self.scaled(styles.quick_open_result_name_font_size_px);
        let description_size = self.scaled(styles.quick_open_result_path_font_size_px);
        let shortcut_style = ShortcutRenderStyle {
            font_size_px: description_size,
            font_family: self.styles.ui_font_family,
            text_color: styles.quick_open_result_secondary_text_color,
            gap_px: self.scaled(2.0),
        };
        let specs = self.visible_command_specs();
        let shortcut_bg = styles.quick_open_result_shortcut_bg;
        let command_count = specs.len();
        let workspace_label = crate::i18n::command_label(self.ui_language, "switch_workspace");
        let mut workspace_rows = self
            .visible_command_palette_workspaces()
            .into_iter()
            .enumerate()
            .map(|(offset, root)| {
                let index = command_count + offset;
                let selected = index == self.command_palette_selected_index;
                let name = Self::workspace_display_name(&root);
                let detail = root.display().to_string();
                crate::ui::list::list_item(
                    self.styles.command_item(),
                    crate::style::ControlState {
                        selected,
                        ..Default::default()
                    },
                    self.ui_scale,
                )
                .id(("command-palette-workspace", offset))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .items_center()
                        .gap(px(self.scaled(
                            self.styles.ui.space(crate::style::SpaceStep::OneAndHalf),
                        )))
                        .child(
                            div()
                                .flex_none()
                                .truncate()
                                .text_size(px(title_size))
                                .text_color(styles.quick_open_result_text_color)
                                .child(format!("{workspace_label}: {name}")),
                        )
                        .child(
                            div()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(description_size))
                                .text_color(styles.quick_open_result_secondary_text_color)
                                .child(detail),
                        ),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        this.command_palette_visible = false;
                        this.reset_command_palette_state(cx);
                        this.restore_command_focus(window, cx);
                        this.switch_workspace(root.clone(), cx);
                        cx.notify();
                    }),
                )
            })
            .collect::<Vec<_>>();
        for (row, (index, pane)) in self.command_panes().into_iter().enumerate() {
            workspace_rows.push(
                crate::ui::list::list_item(
                    self.styles.command_item(),
                    crate::style::ControlState {
                        selected: row == self.command_palette_selected_index,
                        ..Default::default()
                    },
                    self.ui_scale,
                )
                .id(("command-pane", pane))
                .child(crate::i18n::move_tab_to_pane_label(
                    self.ui_language,
                    index + 1,
                ))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.command_palette_pending_pane = Some(pane);
                        this.command_palette_visible = false;
                        cx.notify();
                    }),
                ),
            );
        }

        let rows = specs.into_iter().enumerate().map(|(index, spec)| {
            let selected = index == self.command_palette_selected_index;
            let command = spec.id;
            let state = self.command_state(command);
            let shortcuts: Vec<String> = match self.command_shortcut_hint(spec.id) {
                Some(hint) => vec![hint],
                None => spec
                    .shortcut
                    .into_iter()
                    .chain(spec.alternate_shortcuts.iter().copied())
                    .map(super::shortcut::display_shortcut_from_keystroke)
                    .collect(),
            };
            crate::ui::list::list_item(
                self.styles.command_item(),
                crate::style::ControlState {
                    selected,
                    disabled: !state.enabled,
                    ..Default::default()
                },
                self.ui_scale,
            )
            .id(("command-palette", index))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(
                        self.styles.ui.space(crate::style::SpaceStep::OneAndHalf),
                    )))
                    .child(
                        div()
                            .flex_none()
                            .truncate()
                            .text_size(px(title_size))
                            .text_color(styles.quick_open_result_text_color)
                            .child(if state.checked == Some(true) {
                                format!("✓ {}", self.command_title(spec))
                            } else {
                                self.command_title(spec)
                            }),
                    )
                    .child(
                        div()
                            .min_w(px(0.0))
                            .truncate()
                            .text_size(px(description_size))
                            .text_color(styles.quick_open_result_secondary_text_color)
                            .child(
                                state
                                    .reason
                                    .unwrap_or(spec.localized_category(self.ui_language)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(self.scaled(
                        self.styles.ui.space(crate::style::SpaceStep::ThreeQuarters),
                    )))
                    .children(shortcuts.into_iter().map(|shortcut| {
                        div()
                            .h(px(self.scaled(18.0)))
                            .px(px(
                                self.scaled(self.styles.ui.space(crate::style::SpaceStep::One))
                            ))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(
                                self.scaled(self.styles.ui.spec.appearance.corners.factor() * 4.0)
                            ))
                            .bg(shortcut_bg)
                            .child(render_shortcut(&shortcut, shortcut_style))
                    })),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    if !this.command_state(command).enabled {
                        return;
                    }
                    this.command_palette_pending = Some(command);
                    this.command_palette_visible = false;
                    cx.notify();
                }),
            )
        });

        div()
            .occlude()
            .absolute()
            .left_0()
            .top_0()
            .size_full()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .size_full()
                    .bg(styles.quick_open_backdrop_bg)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_command_palette(window, cx);
                        }),
                    ),
            )
            .child(
                crate::ui::surface::surface(&self.styles.command_surface(), self.ui_scale)
                    .id("command-palette-overlay")
                    .absolute()
                    .top(px(panel_top))
                    .left(px(panel_left.max(0.0)))
                    .w(px(panel_width))
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
                            .px(px(self.scaled(styles.quick_open_input_padding_x_px)))
                            .py(px(self.scaled(styles.quick_open_input_padding_y_px)))
                            .border_b(px(self.ui_border_width(1.0)))
                            .border_color(styles.quick_open_panel_border_color)
                            .when_some(self.command_palette_editor.clone(), |this, editor| {
                                this.child(editor)
                            }),
                    )
                    .child(
                        div()
                            .id("command-palette-results")
                            .w_full()
                            .max_h(px(self.scaled(styles.quick_open_results_max_height_px)))
                            .overflow_y_scroll()
                            .track_scroll(&self.command_palette_scroll_handle)
                            .children(rows)
                            .children(workspace_rows),
                    )
                    .child(self.render_search_shortcut_footer(
                        crate::i18n::search_menu_footer_label(self.ui_language, "execute"),
                    )),
            )
            .into_any_element()
    }
}
