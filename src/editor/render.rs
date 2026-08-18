use gpui::{
    AnyElement, App, Bounds, Corners, CursorStyle, ExternalPaths, HitboxBehavior, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Render, SharedString, Window, canvas, deferred,
    div, img, prelude::*, px, quad,
};
#[cfg(target_os = "linux")]
use gpui::{BoxShadow, Decorations, Hsla, Point, ResizeEdge, Size, Tiling, transparent_black};

use super::*;

pub(super) fn clamp_resize_dimension(value: f32, minimum: f32, maximum: f32) -> f32 {
    let lower_bound = minimum.min(maximum);
    let upper_bound = minimum.max(maximum);
    value.clamp(lower_bound, upper_bound)
}

#[cfg(target_os = "linux")]
fn linux_client_resize_edge(
    position: Point<gpui::Pixels>,
    inset: gpui::Pixels,
    size: Size<gpui::Pixels>,
    tiling: Tiling,
) -> Option<ResizeEdge> {
    let top = !tiling.top && position.y < inset;
    let bottom = !tiling.bottom && position.y > size.height - inset;
    let left = !tiling.left && position.x < inset;
    let right = !tiling.right && position.x > size.width - inset;

    match (top, bottom, left, right) {
        (true, _, true, _) => Some(ResizeEdge::TopLeft),
        (true, _, _, true) => Some(ResizeEdge::TopRight),
        (_, true, true, _) => Some(ResizeEdge::BottomLeft),
        (_, true, _, true) => Some(ResizeEdge::BottomRight),
        (true, _, _, _) => Some(ResizeEdge::Top),
        (_, true, _, _) => Some(ResizeEdge::Bottom),
        (_, _, true, _) => Some(ResizeEdge::Left),
        (_, _, _, true) => Some(ResizeEdge::Right),
        _ => None,
    }
}

impl EditorView {
    fn render_right_panels(&mut self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("right-panels")
            .h_full()
            .flex()
            .flex_row()
            .when(self.should_render_theme_debugger_panel(), |this| {
                this.child(
                    div()
                        .id("theme-debugger-wrap")
                        .h_full()
                        .w(px(self.theme_debugger_width()))
                        .overflow_hidden()
                        .flex_shrink_0()
                        .child(self.render_theme_debugger(cx)),
                )
            })
            .when(self.should_render_utility_panel_on_right(), |this| {
                let ups = &self.styles.utility_panel;
                let panel_w = self.utility_panel_width();
                let panel_layout_w = self.utility_panel_layout_width();
                let panel_top_offset = if self.tabs.is_empty() {
                    0.0
                } else {
                    self.tab_bar_height()
                };
                let hover_indicator_color = ups.utility_panel_resize_active_indicator_color;
                let hover_indicator_width =
                    self.scaled(ups.utility_panel_resize_active_indicator_width_px);
                let handle_width = self.scaled(ups.utility_panel_resize_handle_width_px);
                this.child(
                    div()
                        .id("utility-panel-column")
                        .h_full()
                        .w(px(panel_w))
                        .flex()
                        .flex_col()
                        .flex_shrink_0()
                        .child(
                            div()
                                .h(px(panel_top_offset))
                                .w_full()
                                .flex_none()
                                .bg(self.styles.tab_bar.tab_bar_bg)
                                .when(panel_top_offset > 0.0, |this| {
                                    this.border_b(px(self.styles.tab_bar.tab_bar_border_width_px))
                                        .border_color(self.styles.tab_bar.tab_bar_border_color)
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(Self::on_tab_bar_drag_space_mouse_down),
                                ),
                        )
                        .child(
                            div()
                                .id("utility-panel-wrap")
                                .relative()
                                .flex_1()
                                .min_h(px(0.0))
                                .w_full()
                                .overflow_hidden()
                                .child(
                                    div()
                                        .h_full()
                                        .w(px(panel_layout_w))
                                        .child(self.render_utility_panel(cx)),
                                )
                                .child(
                                    div()
                                        .id("utility-panel-resize-handle")
                                        .absolute()
                                        // Keep the whole hit area inside the
                                        // clipped panel wrap. Previously half
                                        // of it was clipped, making border
                                        // hover difficult to trigger.
                                        .left(px(0.0))
                                        .top_0()
                                        .w(px(handle_width))
                                        .h_full()
                                        .group("utility-panel-resize")
                                        .cursor(CursorStyle::ResizeLeftRight)
                                        .child(
                                            // Hover highlight on the panel's
                                            // left border.
                                            div()
                                                .absolute()
                                                .left(px(0.0))
                                                .top_0()
                                                .w(px(hover_indicator_width))
                                                .h_full()
                                                .group_hover(
                                                    "utility-panel-resize",
                                                    move |style| {
                                                        style.bg(hover_indicator_color)
                                                    },
                                                ),
                                        )
                                        .on_mouse_move(cx.listener(
                                            |this, event: &MouseMoveEvent, window, cx| {
                                                if this.utility_panel_resizing {
                                                    let window_width =
                                                        f32::from(window.bounds().size.width);
                                                    let active_panel =
                                                        this.active_resizable_utility_panel();
                                                    let (min_w, max_w) = active_panel
                                                        .map(|panel| {
                                                            this.utility_panel_resize_bounds_for(panel)
                                                        })
                                                        .unwrap_or_else(|| {
                                                            (
                                                                this.scaled(
                                                                    this.styles
                                                                        .utility_panel
                                                                        .utility_panel_resize_min_width_px,
                                                                ),
                                                                this.scaled(
                                                                    this.styles
                                                                        .utility_panel
                                                                        .utility_panel_resize_max_width_px,
                                                                ),
                                                            )
                                                        });
                                                    let new_width =
                                                        window_width - f32::from(event.position.x);
                                                    let new_width = clamp_resize_dimension(
                                                        new_width, min_w, max_w,
                                                    );
                                                    if let Some(panel) =
                                                        this.active_resizable_utility_panel()
                                                    {
                                                        this.set_utility_panel_custom_width_for(
                                                            panel,
                                                            Some(new_width),
                                                        );
                                                    }
                                                    this.set_utility_panel_width_immediately(
                                                        new_width,
                                                    );
                                                    cx.notify();
                                                }
                                            },
                                        ))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _: &MouseDownEvent, window, cx| {
                                                window.prevent_default();
                                                cx.stop_propagation();
                                                this.tab_drag_candidate_id = None;
                                                this.tab_dragging_id = None;
                                                this.utility_panel_resizing = true;
                                                cx.notify();
                                            }),
                                        )
                                        .on_mouse_up(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, cx| {
                                                this.utility_panel_resizing = false;
                                                cx.notify();
                                            }),
                                        )
                                        .on_mouse_up_out(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, cx| {
                                                this.utility_panel_resizing = false;
                                                cx.notify();
                                            }),
                                        ),
                                ),
                        ),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_toolbar_button(
        &self,
        id: &'static str,
        button_width: f32,
        button_height: f32,
        icon_size: f32,
        base_opacity: f32,
        hover_opacity: f32,
        icon_path: &'static str,
        _anchor_bounds: Bounds<gpui::Pixels>,
        tooltip_title: &'static str,
        tooltip_subtitle: Option<SharedString>,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let editor = cx.entity().clone();
        let title = SharedString::from(tooltip_title);
        let subtitle = tooltip_subtitle.map(SharedString::from);
        let is_hovered = self.hovered_toolbar_button == Some(id);

        let button = div()
            .id(id)
            .w(px(button_width))
            .h(px(button_height))
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::transparent_black())
            .cursor(CursorStyle::Arrow)
            .on_hover({
                let editor = editor.clone();
                move |hovering: &bool, _: &mut Window, cx: &mut App| {
                    let _ = editor.update(cx, |this, cx| {
                        let next_hover = if *hovering { Some(id) } else { None };
                        if this.hovered_toolbar_button == Some(id) || next_hover.is_some() {
                            if this.hovered_toolbar_button != next_hover {
                                this.hovered_toolbar_button = next_hover;
                                cx.notify();
                            }
                        }
                    });
                }
            })
            .child(
                img(helpers::asset_image_source(icon_path))
                    .w(px(icon_size))
                    .h(px(icon_size))
                    .opacity(if is_hovered {
                        hover_opacity
                    } else {
                        base_opacity
                    }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    on_click(this, cx);
                }),
            );
        tooltip::with_tooltip(button, cx.entity().clone(), title, subtitle)
    }

    pub(super) fn render_preview_overlay_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let editor_handle = cx.entity().clone();
        let action_size = self.scaled(
            self.styles
                .code_block
                .generated_preview_action_button_size_px,
        );
        let icon_size = self.scaled(self.styles.code_block.generated_preview_action_icon_size_px);
        let gap = self.scaled(self.styles.code_block.generated_preview_action_gap_px);
        let top = self.scaled(self.styles.code_block.generated_preview_action_padding_px);
        let right = self.scaled(self.styles.code_block.generated_preview_action_padding_px);
        let button_bg = self.styles.code_block.code_block_copy_button_bg;
        let button_hover_bg = self.styles.code_block.code_block_copy_button_hover_bg;
        let button_border = self.styles.code_block.generated_preview_action_border_color;
        let button_radius = px(self.scaled(
            self.styles
                .code_block
                .generated_preview_action_button_radius_px,
        ));
        let button_border_width = px(self.scaled(
            self.styles
                .code_block
                .generated_preview_action_border_width_px,
        ));
        let icon_color = self.styles.code_block.code_block_copy_button_text_color;
        let icon_opacity = 0.78;
        let actions = [
            (
                PREVIEW_FULLSCREEN_EXIT_SVG_PATH,
                GeneratedPreviewAction::Fullscreen,
            ),
            (PREVIEW_RESTORE_SVG_PATH, GeneratedPreviewAction::Restore),
            (PREVIEW_ZOOM_IN_SVG_PATH, GeneratedPreviewAction::ZoomIn),
            (PREVIEW_ZOOM_OUT_SVG_PATH, GeneratedPreviewAction::ZoomOut),
        ];
        deferred(
            canvas(
                move |bounds, window, cx| {
                    let paints = editor_handle.update(cx, |editor, _| {
                        if editor.preview_overlay_state().is_none() {
                            editor.preview_overlay_action_areas.clear();
                            return Vec::new();
                        }
                        let hovered_action = editor.hovering_preview_overlay_action;
                        let total_width = actions.len() as f32 * action_size
                            + actions.len().saturating_sub(1) as f32 * gap;
                        let start_x = f32::from(bounds.origin.x) + f32::from(bounds.size.width)
                            - right
                            - total_width;
                        let y = f32::from(bounds.origin.y) + top;
                        let mut paints = Vec::with_capacity(actions.len());
                        let mut areas = Vec::with_capacity(actions.len());
                        for (index, (icon_path, action)) in actions.iter().copied().enumerate() {
                            let x = start_x + index as f32 * (action_size + gap);
                            let button_bounds = Bounds::new(
                                gpui::point(px(x), px(y)),
                                gpui::size(px(action_size), px(action_size)),
                            );
                            areas.push((button_bounds, action));
                            let hovered = hovered_action == Some(action);
                            paints.push((button_bounds, icon_path, hovered, action));
                        }
                        editor.preview_overlay_action_areas = areas;
                        paints
                    });
                    paints
                        .into_iter()
                        .map(|(button_bounds, icon_path, hovered, action)| {
                            let hitbox =
                                window.insert_hitbox(button_bounds, HitboxBehavior::Normal);
                            let tooltip_title = SharedString::from({
                                let editor = editor_handle.read(cx);
                                match action {
                                    GeneratedPreviewAction::Fullscreen => {
                                        crate::i18n::preview_action_label(
                                            editor.ui_language,
                                            "fullscreen",
                                        )
                                    }
                                    GeneratedPreviewAction::Restore => {
                                        editor.t(crate::i18n::TextKey::Reset)
                                    }
                                    GeneratedPreviewAction::ZoomIn => {
                                        crate::i18n::preview_action_label(
                                            editor.ui_language,
                                            "zoom_in",
                                        )
                                    }
                                    GeneratedPreviewAction::ZoomOut => {
                                        crate::i18n::preview_action_label(
                                            editor.ui_language,
                                            "zoom_out",
                                        )
                                    }
                                    _ => "",
                                }
                            });
                            // Tooltips must be requested during prepaint.
                            tooltip::request_tooltip_for_hitbox(
                                window,
                                cx,
                                editor_handle.clone(),
                                button_bounds,
                                tooltip_title,
                                None::<&'static str>,
                            );
                            (button_bounds, icon_path, hovered, hitbox)
                        })
                        .collect::<Vec<_>>()
                },
                move |_bounds, paints, window, cx| {
                    for (button_bounds, icon_path, hovered, hitbox) in paints {
                        window.set_cursor_style(CursorStyle::Arrow, &hitbox);
                        tooltip::register_tooltip_hover_refresh(window, &hitbox, button_bounds);
                        let bg = if hovered { button_hover_bg } else { button_bg };
                        window.paint_quad(quad(
                            button_bounds,
                            Corners::all(button_radius),
                            bg,
                            button_border_width,
                            button_border,
                            gpui::BorderStyle::default(),
                        ));
                        let icon_left = f32::from(button_bounds.origin.x)
                            + (f32::from(button_bounds.size.width) - icon_size) * 0.5;
                        let icon_top = f32::from(button_bounds.origin.y)
                            + (f32::from(button_bounds.size.height) - icon_size) * 0.5;
                        let mut color = icon_color;
                        color.a *= if hovered { 1.0 } else { icon_opacity };
                        window
                            .paint_svg(
                                Bounds::new(
                                    gpui::point(px(icon_left), px(icon_top)),
                                    gpui::size(px(icon_size), px(icon_size)),
                                ),
                                icon_path.into(),
                                gpui::TransformationMatrix::default(),
                                color,
                                cx,
                            )
                            .ok();
                    }
                },
            )
            .absolute()
            .left(px(0.0))
            .top(px(0.0))
            .right(px(0.0))
            .bottom(px(0.0)),
        )
        .into_any_element()
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(target_os = "macos")]
        self.sync_macos_traffic_light_position(window);
        #[cfg(not(target_os = "macos"))]
        {
            self.window_is_maximized = window.is_maximized();
            crate::platform::window_state::track(window.window_bounds());
        }
        #[cfg(target_os = "linux")]
        let linux_decorations = window.window_decorations();
        #[cfg(target_os = "linux")]
        {
            self.client_side_window_decorations =
                matches!(linux_decorations, Decorations::Client { .. });
            match linux_decorations {
                Decorations::Client { .. } => window.set_client_inset(px(12.0)),
                Decorations::Server => window.set_client_inset(px(0.0)),
            }
        }
        #[cfg(target_os = "macos")]
        let show_window_caption_buttons = false;
        #[cfg(windows)]
        let show_window_caption_buttons = true;
        #[cfg(target_os = "linux")]
        let show_window_caption_buttons = self.client_side_window_decorations;
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        let show_window_caption_buttons = true;
        self.execute_pending_command(window, cx);
        self.refresh_chat_terminal_metrics(window, cx);
        self.process_pending_inline_diff(cx);
        // Keeps the status-bar branch label and panel badges fresh; only
        // schedules background work when the snapshot is dirty.
        self.refresh_git_panel_status_if_needed(cx);
        if self.pending_editor_focus {
            window.focus(&self.focus_handle(cx));
            self.pending_editor_focus = false;
        }
        if let Some(tab_id) = self.pending_terminal_focus.clone() {
            if self.active_terminal_tab_id().as_deref() != Some(tab_id.as_str()) {
                self.pending_terminal_focus = None;
            } else if let Some(state) = self.terminal_tab_states.get(&tab_id) {
                window.focus(&state.focus_handle);
                window.invalidate_character_coordinates();
                self.pending_terminal_focus = None;
            }
        }
        let focused_terminal_tab_id = self
            .terminal_tab_states
            .iter()
            .find(|(_, state)| state.focus_handle.is_focused(window))
            .map(|(tab_id, _)| tab_id.clone());
        // The terminal caret shares the editor's blink opacity, so it has to
        // keep the animation alive while it owns focus. Tabs without a text
        // buffer (settings, images) paint no caret at all.
        self.sync_cursor_blink_allowed(
            window.is_window_active()
                && self.focused_sidebar_path.is_none()
                && ((self.focus_handle.is_focused(window)
                    && self.active_tab_supports_text_buffer())
                    || focused_terminal_tab_id.is_some()),
            cx,
        );
        if let Some(tab_id) = focused_terminal_tab_id {
            self.record_terminal_focus(&tab_id);
            window.invalidate_character_coordinates();
        }
        if self.focus_handle.is_focused(window) && self.active_tab_supports_text_buffer() {
            window.invalidate_character_coordinates();
        }
        let resizing = self.sidebar_resizing
            || self.file_list_resizing
            || self.utility_panel_resizing
            || self.pane_resize_drag.is_some();
        let show_empty_workspace = self.panes.len() == 1 && self.tabs.is_empty();
        let ws = &self.styles.window;
        let editor = cx.entity().clone();
        let global_mouse_listeners = (!self.embedded_editor).then(|| {
            canvas(
                |_, _, _| (),
                move |_, _, window, _cx| {
                    window.on_mouse_event({
                        let editor = editor.clone();
                        move |event: &MouseMoveEvent, phase, _window, cx| {
                            if !phase.capture() {
                                return;
                            }

                            let handled = editor.update(cx, |this, cx| {
                                if this.editor_scrollbar_drag.is_none() {
                                    return false;
                                }

                                if event.dragging() {
                                    this.hovered_editor_position = Some(event.position);
                                    this.set_editor_scrollbar_hovered(true, cx);
                                    this.update_editor_scrollbar_drag(event.position, cx);
                                } else {
                                    this.stop_editor_scrollbar_drag(event.position, cx);
                                }
                                true
                            });
                            if handled {
                                cx.stop_propagation();
                            }
                        }
                    });
                    window.on_mouse_event({
                        let editor = editor.clone();
                        move |event: &MouseMoveEvent, phase, _window, cx| {
                            if !phase.capture() {
                                return;
                            }

                            let handled = editor.update(cx, |this, cx| {
                                if event.dragging() {
                                    this.update_panel_scrollbar_drag(event.position, cx)
                                } else {
                                    this.stop_panel_scrollbar_drag(cx)
                                }
                            });
                            if handled {
                                cx.stop_propagation();
                            }
                        }
                    });
                    window.on_mouse_event({
                        let editor = editor.clone();
                        move |event: &MouseUpEvent, phase, _window, cx| {
                            if !phase.capture() || event.button != MouseButton::Left {
                                return;
                            }

                            let handled = editor.update(cx, |this, cx| {
                                this.stop_editor_scrollbar_drag(event.position, cx)
                            });
                            if handled {
                                cx.stop_propagation();
                            }
                        }
                    });
                    window.on_mouse_event({
                        let editor = editor.clone();
                        move |event: &MouseUpEvent, phase, _window, cx| {
                            if !phase.capture() || event.button != MouseButton::Left {
                                return;
                            }

                            let handled =
                                editor.update(cx, |this, cx| this.stop_panel_scrollbar_drag(cx));
                            if handled {
                                cx.stop_propagation();
                            }
                        }
                    });
                    // Focus the pane under the cursor before any bubble-phase handler
                    // runs, so subsequent listeners operate on the right pane's state.
                    window.on_mouse_event({
                        let editor = editor.clone();
                        move |event: &MouseDownEvent, phase, _window, cx| {
                            if !phase.capture() {
                                return;
                            }
                            editor.update(cx, |this, cx| {
                                if this.modal_overlay_is_open() || this.panes.len() < 2 {
                                    return;
                                }
                                let target =
                                    this.panes.iter().map(|pane| pane.id).find(|pane_id| {
                                        this.pane_bounds
                                            .get(pane_id)
                                            .is_some_and(|bounds| bounds.contains(&event.position))
                                    });
                                if let Some(pane_id) = target {
                                    this.focus_pane(pane_id, cx);
                                }
                            });
                        }
                    });
                    // Completes a tab drag over a pane's content area (split or merge).
                    // Handled in the capture phase from our own tracked drag state instead
                    // of gpui's drop dispatch, which is raced by the drag source's
                    // capture-phase stop_propagation and other bubble listeners.
                    window.on_mouse_event({
                        let editor = editor.clone();
                        move |event: &MouseUpEvent, phase, _window, cx| {
                            if !phase.capture() || event.button != MouseButton::Left {
                                return;
                            }
                            editor.update(cx, |this, cx| {
                                let Some((pane_id, direction)) = this.tab_drag_split_target.take()
                                else {
                                    return;
                                };
                                let Some(tab_id) = this.tab_dragging_id.take() else {
                                    return;
                                };
                                this.tab_drag_candidate_id = None;
                                match direction {
                                    Some(direction) => {
                                        this.split_pane_with_tab(pane_id, direction, &tab_id, cx);
                                    }
                                    None => {
                                        if this.pane_id_for_tab(&tab_id) != Some(pane_id) {
                                            this.move_tab_to_pane(&tab_id, pane_id, None, cx);
                                        } else {
                                            this.focus_pane(pane_id, cx);
                                            this.switch_to_tab(&tab_id, cx);
                                        }
                                    }
                                }
                                cx.notify();
                            });
                        }
                    });
                },
            )
            .absolute()
            .size_full()
        });
        // Store window bounds for menu positioning
        let wb = window.bounds();
        self.last_window_width = f32::from(wb.size.width);
        self.last_window_height = f32::from(wb.size.height);
        let content = div()
            .id("editor-root")
            .key_context(if self.focused_sidebar_path.is_some() {
                "MarkdownEditor Sidebar"
            } else if self.active_tab_is_whiteboard() {
                "MarkdownEditor Whiteboard"
            } else {
                "MarkdownEditor"
            })
            .track_focus(&self.focus_handle(cx))
            .when_some(global_mouse_listeners, |this, listeners| {
                this.child(listeners)
            })
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if this.active_tab_is_whiteboard()
                    && event.keystroke.key == "space"
                    && this.whiteboard_text_editor.is_none()
                    && !this.whiteboard_space_down
                {
                    this.whiteboard_space_down = true;
                    cx.notify();
                }
            }))
            .on_key_up(cx.listener(|this, event: &gpui::KeyUpEvent, _, cx| {
                if event.keystroke.key == "space" && this.whiteboard_space_down {
                    this.whiteboard_space_down = false;
                    cx.notify();
                }
            }))
            .relative()
            .size_full()
            .flex()
            .flex_row()
            .overflow_hidden()
            .font_family(SharedString::from(self.styles.ui_font_family))
            .text_size(px(self.scaled(self.styles.ui_font_size_px)))
            .bg(ws.window_bg)
            .border(px(ws.window_border_width_px))
            .border_color(ws.window_border_color)
            .rounded(px(ws.window_radius_px))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.context_menu.is_some() {
                        this.close_context_menu(cx);
                    }
                }),
            )
            .on_action(cx.listener(Self::on_undo))
            .on_action(cx.listener(Self::on_redo))
            .on_action(cx.listener(Self::on_backspace))
            .on_action(cx.listener(Self::on_delete))
            .on_action(cx.listener(Self::on_left))
            .on_action(cx.listener(Self::on_right))
            .on_action(cx.listener(Self::on_up))
            .on_action(cx.listener(Self::on_down))
            .on_action(cx.listener(Self::on_select_left))
            .on_action(cx.listener(Self::on_select_right))
            .on_action(cx.listener(Self::on_select_up))
            .on_action(cx.listener(Self::on_select_down))
            .on_action(cx.listener(Self::on_select_all))
            .on_action(cx.listener(Self::on_home))
            .on_action(cx.listener(Self::on_end))
            .on_action(cx.listener(Self::on_select_home))
            .on_action(cx.listener(Self::on_select_end))
            .on_action(cx.listener(Self::on_enter))
            .on_action(cx.listener(Self::on_tab))
            .on_action(cx.listener(Self::on_back_tab))
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_cut))
            .on_action(cx.listener(Self::on_paste))
            .on_action(cx.listener(Self::on_paste_plain_text))
            .on_action(cx.listener(Self::on_insert_soft_break))
            .on_action(cx.listener(Self::on_show_character_palette))
            .on_action(cx.listener(Self::on_move_word_left))
            .on_action(cx.listener(Self::on_move_word_right))
            .on_action(cx.listener(Self::on_select_word_left))
            .on_action(cx.listener(Self::on_select_word_right))
            .on_action(cx.listener(Self::on_delete_word_left))
            .on_action(cx.listener(Self::on_delete_word_right))
            .on_action(cx.listener(Self::on_move_to_document_start))
            .on_action(cx.listener(Self::on_move_to_document_end))
            .on_action(cx.listener(Self::on_select_to_document_start))
            .on_action(cx.listener(Self::on_select_to_document_end))
            .on_action(cx.listener(Self::on_delete_to_line_start))
            .on_action(cx.listener(Self::on_delete_to_line_end))
            .on_action(cx.listener(Self::on_kill_to_line_end))
            .on_action(cx.listener(Self::on_yank))
            .on_action(cx.listener(Self::on_open_line))
            .on_action(cx.listener(Self::on_transpose_chars))
            .on_action(cx.listener(Self::on_move_page_down))
            .on_action(cx.listener(Self::on_toggle_line_comment))
            .on_action(cx.listener(Self::on_fold))
            .on_action(cx.listener(Self::on_unfold))
            .on_action(cx.listener(Self::on_toggle_fold))
            .on_action(cx.listener(Self::on_fold_all))
            .on_action(cx.listener(Self::on_unfold_all))
            .on_action(cx.listener(Self::on_join_lines))
            .on_action(cx.listener(Self::on_toggle_bold))
            .on_action(cx.listener(Self::on_toggle_italic))
            .on_action(cx.listener(Self::on_toggle_underline))
            .on_action(cx.listener(Self::on_toggle_strikethrough))
            .on_action(cx.listener(Self::on_toggle_inline_code))
            .on_action(cx.listener(Self::on_toggle_link))
            .on_action(cx.listener(Self::on_toggle_image))
            .on_action(cx.listener(Self::on_toggle_quote))
            .on_action(cx.listener(Self::on_toggle_todo))
            .on_action(cx.listener(Self::on_toggle_unordered_list))
            .on_action(cx.listener(Self::on_toggle_ordered_list))
            .on_action(cx.listener(Self::on_insert_table))
            .on_action(cx.listener(Self::on_insert_code_block))
            .on_action(cx.listener(Self::on_insert_horizontal_rule))
            .on_action(cx.listener(Self::on_insert_heading1))
            .on_action(cx.listener(Self::on_insert_heading2))
            .on_action(cx.listener(Self::on_insert_heading3))
            .on_action(cx.listener(Self::on_insert_heading4))
            .on_action(cx.listener(Self::on_insert_heading5))
            .on_action(cx.listener(Self::on_insert_heading6))
            .on_action(cx.listener(Self::on_set_paragraph))
            .on_action(cx.listener(Self::on_zoom_in))
            .on_action(cx.listener(Self::on_zoom_out))
            .on_action(cx.listener(Self::on_zoom_reset))
            .on_action(cx.listener(Self::on_toggle_sidebar))
            .on_action(cx.listener(Self::on_toggle_theme))
            .on_action(cx.listener(Self::on_toggle_editor_left_align))
            .on_action(cx.listener(Self::on_toggle_word_wrap))
            .on_action(cx.listener(Self::on_select_line))
            .on_action(cx.listener(Self::on_insert_line_below))
            .on_action(cx.listener(Self::on_insert_line_above))
            .on_action(cx.listener(Self::on_duplicate_line))
            .on_action(cx.listener(Self::on_copy_line_up))
            .on_action(cx.listener(Self::on_copy_line_down))
            .on_action(cx.listener(Self::on_delete_line))
            .on_action(cx.listener(Self::on_move_line_up))
            .on_action(cx.listener(Self::on_move_line_down))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_save_as))
            .on_action(cx.listener(Self::on_new_document))
            .on_action(cx.listener(Self::on_new_tab))
            .on_action(cx.listener(Self::on_new_whiteboard))
            .on_action(cx.listener(Self::on_close_active_tab))
            .on_action(cx.listener(Self::on_reopen_closed_tab))
            .on_action(cx.listener(Self::on_activate_previous_tab))
            .on_action(cx.listener(Self::on_activate_next_tab))
            .on_action(cx.listener(Self::on_split_pane_right))
            .on_action(cx.listener(Self::on_split_pane_left))
            .on_action(cx.listener(Self::on_split_pane_up))
            .on_action(cx.listener(Self::on_split_pane_down))
            .on_action(cx.listener(Self::on_focus_next_pane))
            .on_action(cx.listener(Self::on_find))
            .on_action(cx.listener(Self::on_find_next))
            .on_action(cx.listener(Self::on_find_previous))
            .on_action(cx.listener(Self::on_dismiss_search))
            .on_action(cx.listener(Self::on_toggle_replace))
            .on_action(cx.listener(Self::on_toggle_search_case_sensitive))
            .on_action(cx.listener(Self::on_toggle_search_whole_word))
            .on_action(cx.listener(Self::on_toggle_search_regex))
            .on_action(cx.listener(Self::on_replace_next))
            .on_action(cx.listener(Self::on_replace_all))
            .on_action(cx.listener(Self::on_open_file_dialog))
            .on_action(cx.listener(Self::on_find_in_files))
            .on_action(cx.listener(Self::on_quick_open))
            .on_action(cx.listener(Self::on_open_settings))
            .on_action(cx.listener(Self::on_toggle_command_palette))
            .on_action(cx.listener(Self::on_escape))
            .on_key_down(cx.listener(Self::on_preview_overlay_key_down))
            .on_action(cx.listener(Self::on_toggle_outline))
            .on_action(cx.listener(Self::on_toggle_info_panel))
            .on_action(cx.listener(Self::on_toggle_terminal_panel))
            .on_action(cx.listener(Self::on_toggle_git_history))
            .on_action(cx.listener(Self::on_toggle_theme_debugger))
            .on_action(cx.listener(Self::on_add_cursor_above))
            .on_action(cx.listener(Self::on_add_cursor_below))
            .on_action(cx.listener(Self::on_select_next_occurrence))
            .on_action(cx.listener(Self::on_select_all_occurrences))
            .on_action(cx.listener(Self::on_toggle_runtime_profile))
            .on_action(cx.listener(Self::on_write_runtime_profile_snapshot))
            .on_action(cx.listener(Self::on_whiteboard_select_tool))
            .on_action(cx.listener(Self::on_whiteboard_rectangle_tool))
            .on_action(cx.listener(Self::on_whiteboard_ellipse_tool))
            .on_action(cx.listener(Self::on_whiteboard_arrow_tool))
            .on_action(cx.listener(Self::on_whiteboard_line_tool))
            .on_action(cx.listener(Self::on_whiteboard_text_tool))
            .on_action(cx.listener(Self::on_sidebar_create_file))
            .on_action(cx.listener(Self::on_sidebar_create_folder))
            .on_action(cx.listener(Self::on_sidebar_rename))
            .on_action(cx.listener(Self::on_sidebar_delete))
            .when(self.should_render_sidebar_panel(), |this| {
                let sidebar_w = self.sidebar_width();
                let sidebar_layout_w = self.sidebar_layout_width();
                let file_list_w = self.file_list_width();
                let file_list_layout_w = self.file_list_layout_width();
                let ss = &self.styles.sidebar;
                let hover_indicator_color = ss.sidebar_resize_active_indicator_color;
                let hover_indicator_width =
                    self.scaled(ss.sidebar_resize_active_indicator_width_px);
                let handle_width = self.scaled(ss.sidebar_resize_handle_width_px);
                this.child(
                    div()
                        .id("sidebar-wrap")
                        .w(px(sidebar_w + file_list_w))
                        .h_full()
                        .overflow_hidden()
                        .flex_none()
                        .flex()
                        .child(
                            div()
                                .h_full()
                                .w(px(sidebar_w))
                                .flex_none()
                                .overflow_hidden()
                                .child(
                                    div()
                                        .h_full()
                                        .w(px(sidebar_layout_w))
                                        .child(self.render_sidebar(cx)),
                                ),
                        )
                        .when(file_list_w > 0.5, |this| {
                            this.child(
                                div()
                                    .h_full()
                                    .w(px(file_list_w))
                                    .flex_none()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .h_full()
                                            .w(px(file_list_layout_w))
                                            .child(self.render_file_list(cx)),
                                    ),
                            )
                        }),
                )
                // Transparent sensing area for resize drag
                .child(
                    div()
                        .id("sidebar-resize-handle")
                        .absolute()
                        .left(px(sidebar_w - handle_width / 2.0))
                        .top_0()
                        .w(px(handle_width))
                        .h_full()
                        .cursor(CursorStyle::ResizeLeftRight)
                        .hover(move |style| {
                            style
                                .border_l(px(hover_indicator_width))
                                .border_color(hover_indicator_color)
                        })
                        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                            if this.sidebar_resizing {
                                let new_width = f32::from(event.position.x);
                                let min_w =
                                    this.scaled(this.styles.sidebar.sidebar_resize_min_width_px);
                                let max_w =
                                    this.scaled(this.styles.sidebar.sidebar_resize_max_width_px);
                                let new_width = clamp_resize_dimension(new_width, min_w, max_w);
                                this.set_active_sidebar_custom_width(new_width);
                                this.set_sidebar_width_immediately(new_width);
                                cx.notify();
                            }
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.sidebar_resizing = true;
                                cx.notify();
                            }),
                        )
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.sidebar_resizing = false;
                                cx.notify();
                            }),
                        )
                        .on_mouse_up_out(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.sidebar_resizing = false;
                                cx.notify();
                            }),
                        ),
                )
                .when(file_list_w > 0.5, |this| {
                    this.child(
                        div()
                            .id("file-list-resize-handle")
                            .absolute()
                            .left(px(sidebar_w + file_list_w - handle_width / 2.0))
                            .top_0()
                            .w(px(handle_width))
                            .h_full()
                            .cursor(CursorStyle::ResizeLeftRight)
                            .hover(move |style| {
                                style
                                    .border_l(px(hover_indicator_width))
                                    .border_color(hover_indicator_color)
                            })
                            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                                if this.file_list_resizing {
                                    let new_width = f32::from(event.position.x)
                                        - this.sidebar_width()
                                        - this.file_list_resize_grab_offset_x;
                                    let min_w = this
                                        .scaled(this.styles.sidebar.file_list_resize_min_width_px);
                                    let max_w = this
                                        .scaled(this.styles.sidebar.file_list_resize_max_width_px);
                                    let new_width = clamp_resize_dimension(new_width, min_w, max_w);
                                    this.file_list_custom_width = Some(new_width);
                                    this.file_list_display_width = new_width;
                                    this.file_list_layout_width = new_width;
                                    this.file_list_width_target = new_width;
                                    this.file_list_width_task = None;
                                    cx.notify();
                                }
                            }))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                    let border_x = this.sidebar_width() + this.file_list_width();
                                    this.file_list_resize_grab_offset_x =
                                        f32::from(event.position.x) - border_x;
                                    this.file_list_resizing = true;
                                    cx.notify();
                                }),
                            )
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.finish_file_list_resize(cx);
                                }),
                            )
                            .on_mouse_up_out(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.finish_file_list_resize(cx);
                                }),
                            ),
                    )
                })
            })
            .child(
                div()
                    .id("workspace-pane")
                    .flex_1()
                    .size_full()
                    .overflow_hidden()
                    .relative()
                    .flex()
                    .flex_row()
                    .bg(self.styles.editor.editor_bg)
                    .child(
                        div()
                            .id("workspace-main-column")
                            .flex_1()
                            .h_full()
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .when(show_empty_workspace, |this| {
                                // Empty state: no tabs open
                                let bg_hover = self.styles.sidebar.sidebar_item_hover_bg;
                                let btn_color = self.styles.sidebar.sidebar_empty_btn_text_color;
                                let border_color =
                                    self.styles.sidebar.sidebar_empty_btn_border_color;
                                let file_drop_bg = self.styles.editor.editor_file_drop_bg;
                                this.child(
                                    div()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .justify_center()
                                        .drag_over::<ExternalPaths>(move |style, _, _, _| {
                                            style.bg(file_drop_bg)
                                        })
                                        .on_drop(cx.listener(
                                            |this, paths: &ExternalPaths, _, cx| {
                                                this.open_external_paths_in_tabs(paths.paths(), cx);
                                            },
                                        ))
                                        .cursor(CursorStyle::Arrow)
                                        .child(
                                            div()
                                                .id("empty-workspace-btn")
                                                .px(px(20.0))
                                                .py(px(8.0))
                                                .border(px(self.ui_border_width(1.5)))
                                                .border_dashed()
                                                .border_color(border_color)
                                                .rounded_full()
                                                .text_size(px(self.scaled(
                                                    self.styles
                                                        .sidebar
                                                        .sidebar_empty_btn_font_size_px,
                                                )))
                                                .text_color(btn_color)
                                                .cursor(CursorStyle::Arrow)
                                                .hover(move |s| s.bg(bg_hover))
                                                .child(self.t(crate::i18n::TextKey::NewFile))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(
                                                        |this, _: &MouseDownEvent, _, cx| {
                                                            this.create_scratch_tab(cx);
                                                        },
                                                    ),
                                                ),
                                        ),
                                )
                            })
                            .when(!show_empty_workspace, |this| {
                                this.child(self.render_workspace_content_column(window, cx))
                            }),
                    )
                    .when(self.should_render_right_panels(), |this| {
                        this.child(self.render_right_panels(cx))
                    })
                    .when(
                        !show_empty_workspace && self.should_render_utility_panel_on_right(),
                        |this| {
                            this.child(
                                div()
                                    .id("workspace-tab-bar-actions")
                                    .absolute()
                                    .top_0()
                                    // Sits beside the caption buttons the window
                                    // draws in the same corner.
                                    .right(px(self.window_caption_trailing_inset()))
                                    .h(px(self.tab_bar_height()))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .occlude()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(Self::on_tab_bar_drag_space_mouse_down),
                                    )
                                    .child(self.render_tab_bar_toggle_actions(cx)),
                            )
                        },
                    )
                    .when(show_empty_workspace, |this| {
                        this.child(
                            div()
                                .id("empty-workspace-titlebar-drag-region")
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .h(px(self.tab_bar_height()))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(Self::on_tab_bar_drag_space_mouse_down),
                                ),
                        )
                    }),
            )
            .when(resizing, |this| {
                let indicator_line = if self.sidebar_resizing {
                    let iw =
                        self.scaled(self.styles.sidebar.sidebar_resize_active_indicator_width_px);
                    let ic = self.styles.sidebar.sidebar_resize_active_indicator_color;
                    let x = self.sidebar_width() - iw;
                    Some(
                        div()
                            .absolute()
                            .left(px(x))
                            .top_0()
                            .w(px(iw))
                            .h_full()
                            .bg(ic)
                            .into_any_element(),
                    )
                } else if self.file_list_resizing {
                    let iw =
                        self.scaled(self.styles.sidebar.sidebar_resize_active_indicator_width_px);
                    let ic = self.styles.sidebar.sidebar_resize_active_indicator_color;
                    let x = self.sidebar_width() + self.file_list_width() - iw;
                    Some(
                        div()
                            .absolute()
                            .left(px(x))
                            .top_0()
                            .w(px(iw))
                            .h_full()
                            .bg(ic)
                            .into_any_element(),
                    )
                } else if self.utility_panel_resizing {
                    let iw = self.scaled(
                        self.styles
                            .utility_panel
                            .utility_panel_resize_active_indicator_width_px,
                    );
                    let ic = self
                        .styles
                        .utility_panel
                        .utility_panel_resize_active_indicator_color;
                    let top = if self.tabs.is_empty() {
                        0.0
                    } else {
                        self.tab_bar_height()
                    };
                    let height = (self.last_window_height - top).max(0.0);
                    let x = (self.last_window_width - self.utility_panel_width()).max(0.0);
                    Some(
                        div()
                            .absolute()
                            .left(px(x))
                            .top(px(top))
                            .w(px(iw))
                            .h(px(height))
                            .bg(ic)
                            .into_any_element(),
                    )
                } else {
                    None
                };
                let resize_cursor = if self.pane_resize_drag.is_some() {
                    match self.pane_resize_drag.as_ref().map(|drag| drag.axis) {
                        Some(SplitAxis::Vertical) => CursorStyle::ResizeUpDown,
                        _ => CursorStyle::ResizeLeftRight,
                    }
                } else {
                    CursorStyle::ResizeLeftRight
                };
                this.child(deferred(
                    div()
                        .occlude()
                        .absolute()
                        .left(px(0.0))
                        .top(px(0.0))
                        .w_full()
                        .h_full()
                        .cursor(resize_cursor)
                        .children(indicator_line)
                        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                            if this.pane_resize_drag.is_some() {
                                this.update_pane_resize_drag(event.position);
                                cx.notify();
                            } else if this.sidebar_resizing {
                                let new_width = f32::from(event.position.x);
                                let min_w =
                                    this.scaled(this.styles.sidebar.sidebar_resize_min_width_px);
                                let max_w =
                                    this.scaled(this.styles.sidebar.sidebar_resize_max_width_px);
                                let new_width = clamp_resize_dimension(new_width, min_w, max_w);
                                this.set_active_sidebar_custom_width(new_width);
                                this.set_sidebar_width_immediately(new_width);
                                cx.notify();
                            } else if this.file_list_resizing {
                                let new_width = f32::from(event.position.x)
                                    - this.sidebar_width()
                                    - this.file_list_resize_grab_offset_x;
                                let min_w =
                                    this.scaled(this.styles.sidebar.file_list_resize_min_width_px);
                                let max_w =
                                    this.scaled(this.styles.sidebar.file_list_resize_max_width_px);
                                let new_width = clamp_resize_dimension(new_width, min_w, max_w);
                                this.file_list_custom_width = Some(new_width);
                                this.file_list_display_width = new_width;
                                this.file_list_layout_width = new_width;
                                this.file_list_width_target = new_width;
                                this.file_list_width_task = None;
                                cx.notify();
                            } else if this.utility_panel_resizing {
                                let window_width = f32::from(window.bounds().size.width);
                                let active_panel = this.active_resizable_utility_panel();
                                let (min_w, max_w) = active_panel
                                    .map(|panel| this.utility_panel_resize_bounds_for(panel))
                                    .unwrap_or_else(|| {
                                        (
                                            this.scaled(
                                                this.styles
                                                    .utility_panel
                                                    .utility_panel_resize_min_width_px,
                                            ),
                                            this.scaled(
                                                this.styles
                                                    .utility_panel
                                                    .utility_panel_resize_max_width_px,
                                            ),
                                        )
                                    });
                                let new_size = clamp_resize_dimension(
                                    window_width - f32::from(event.position.x),
                                    min_w,
                                    max_w,
                                );
                                if let Some(panel) = active_panel {
                                    this.set_utility_panel_custom_width_for(panel, Some(new_size));
                                }
                                this.set_utility_panel_width_immediately(new_size);
                                cx.notify();
                            }
                        }))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.sidebar_resizing = false;
                                this.utility_panel_resizing = false;
                                this.pane_resize_drag = None;
                                this.finish_file_list_resize(cx);
                                cx.notify();
                            }),
                        )
                        .on_mouse_down(MouseButton::Left, cx.listener(|_this, _, _, _cx| {})),
                ))
            })
            .when(self.context_menu.is_some(), |this| {
                this.child(deferred(self.render_context_menu(cx)))
            })
            .when(self.settings_font_picker.is_some(), |this| {
                if let Some(picker) = &self.settings_font_picker {
                    this.child(deferred(self.render_font_picker_overlay(picker, cx)))
                } else {
                    this
                }
            })
            .when(self.quick_open_visible, |this| {
                this.child(deferred(self.render_quick_open_overlay(cx)))
            })
            .when(self.command_palette_visible, |this| {
                this.child(deferred(self.render_command_palette_overlay(cx)))
            })
            .when(self.language_picker_visible, |this| {
                this.child(deferred(self.render_language_picker_overlay(cx)))
            })
            .when(self.branch_picker_visible, |this| {
                this.child(deferred(self.render_branch_picker_overlay(cx)))
            })
            .when(self.git_remote_input_visible, |this| {
                this.child(deferred(self.render_git_remote_input_overlay(window, cx)))
            })
            .when(self.dialog.is_some(), |this| {
                this.child(deferred(self.render_dialog(cx)))
            })
            .when(!self.notifications.is_empty(), |this| {
                this.child(deferred(self.render_notifications(cx)))
            })
            // Anchored to the window rather than the tab bar, which stops short
            // of the frame whenever a right-hand panel is open.
            .when(show_window_caption_buttons, |this| {
                this.child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .child(self.render_window_caption_buttons(cx)),
                )
            });

        #[cfg(target_os = "linux")]
        {
            const SHADOW_INSET: f32 = 18.0;
            const CORNER_RADIUS: f32 = 12.0;

            match linux_decorations {
                Decorations::Server => div().size_full().child(content).into_any_element(),
                Decorations::Client { tiling } => div()
                    .id("linux-window-backdrop")
                    .size_full()
                    .bg(transparent_black())
                    .when(!tiling.top, |this| this.pt(px(SHADOW_INSET)))
                    .when(!tiling.bottom, |this| this.pb(px(SHADOW_INSET)))
                    .when(!tiling.left, |this| this.pl(px(SHADOW_INSET)))
                    .when(!tiling.right, |this| this.pr(px(SHADOW_INSET)))
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        let size = window.window_bounds().get_bounds().size;
                        if let Some(edge) =
                            linux_client_resize_edge(event.position, px(SHADOW_INSET), size, tiling)
                        {
                            cx.stop_propagation();
                            window.start_window_resize(edge);
                        }
                    })
                    .child(
                        div()
                            .size_full()
                            .when(!(tiling.top || tiling.left), |this| {
                                this.rounded_tl(px(CORNER_RADIUS))
                            })
                            .when(!(tiling.top || tiling.right), |this| {
                                this.rounded_tr(px(CORNER_RADIUS))
                            })
                            .when(!(tiling.bottom || tiling.left), |this| {
                                this.rounded_bl(px(CORNER_RADIUS))
                            })
                            .when(!(tiling.bottom || tiling.right), |this| {
                                this.rounded_br(px(CORNER_RADIUS))
                            })
                            .when(!tiling.is_tiled(), |this| {
                                this.shadow(vec![BoxShadow {
                                    color: Hsla {
                                        h: 0.0,
                                        s: 0.0,
                                        l: 0.0,
                                        a: 0.22,
                                    },
                                    offset: gpui::point(px(0.0), px(3.0)),
                                    blur_radius: px(18.0),
                                    spread_radius: px(0.0),
                                }])
                            })
                            .overflow_hidden()
                            .child(content),
                    )
                    .into_any_element(),
            }
        }

        #[cfg(not(target_os = "linux"))]
        content
    }
}

impl EditorView {
    pub(super) fn render_binary_placeholder(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let path = self
            .active_tab_index()
            .and_then(|index| self.tabs.get(index))
            .and_then(|tab| tab.path.clone());

        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(self.scaled(10.0)))
            .text_color(self.theme.text_secondary)
            .child(
                div()
                    .text_size(px(self.scaled(self.styles.ui_font_size_px + 1.0)))
                    .text_color(self.theme.text_primary)
                    .child(self.t(crate::i18n::TextKey::BinaryFileMessage)),
            )
            .child(
                div()
                    .text_size(px(self.scaled(self.styles.ui_font_size_px - 1.0)))
                    .child(
                        path.as_ref()
                            .map(|value| value.display().to_string())
                            .unwrap_or_else(|| {
                                self.t(crate::i18n::TextKey::CurrentFile).to_string()
                            }),
                    ),
            )
            .child(
                div()
                    .px(px(self.scaled(12.0)))
                    .py(px(self.scaled(4.0)))
                    .rounded(px(self.scaled(8.0)))
                    .bg(self.theme.bg_hover)
                    .text_color(self.theme.text_primary)
                    .cursor(CursorStyle::PointingHand)
                    .child(self.t(crate::i18n::TextKey::OpenFile))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            if let Some(path) = this
                                .active_tab_index()
                                .and_then(|index| this.tabs.get(index))
                                .and_then(|tab| tab.path.clone())
                            {
                                if let Err(error) = crate::platform::shell::open_path(&path) {
                                    let message = format!(
                                        "{}: {error}",
                                        this.t(crate::i18n::TextKey::OpenFailed)
                                    );
                                    this.notify_error(message, cx);
                                }
                            }
                            cx.notify();
                        }),
                    ),
            )
            .into_any_element()
    }
}
