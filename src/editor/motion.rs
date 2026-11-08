use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, Context, Entity, Render, Subscription, WeakEntity, Window, div, prelude::*,
};

use super::EditorView;
use crate::ui::motion::Transition;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum OverlayKind {
    FontPicker,
    QuickOpen,
    CommandPalette,
    LanguagePicker,
    Dialog,
    Notifications,
}

impl OverlayKind {
    fn visible(self, editor: &EditorView) -> bool {
        match self {
            Self::FontPicker => editor.settings_font_picker.is_some(),
            Self::QuickOpen => editor.quick_open_visible,
            Self::CommandPalette => editor.command_palette_visible,
            Self::LanguagePicker => editor.language_picker_visible,
            Self::Dialog => editor.dialog.is_some(),
            Self::Notifications => !editor.notifications.is_empty(),
        }
    }
}

pub(super) type OverlayViews = HashMap<OverlayKind, Entity<MotionOverlay>>;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum PanelKind {
    FileList,
    WorkspaceSearch,
    Outline,
}

impl PanelKind {
    fn visible(self, editor: &EditorView) -> bool {
        match self {
            Self::FileList => editor.file_list_width() > 0.0,
            Self::WorkspaceSearch => editor.workspace_search_panel_width() > 0.0,
            Self::Outline => {
                editor.displayed_utility_panel == Some(super::RightUtilityPanel::Outline)
                    && editor.utility_panel_width() > 0.0
            }
        }
    }
}

pub(super) type PanelViews = HashMap<PanelKind, Entity<PanelContentView>>;

pub(super) struct PanelContentView {
    editor: WeakEntity<EditorView>,
    kind: PanelKind,
    _subscription: Subscription,
}

impl Render for PanelContentView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(editor) = self.editor.upgrade() else {
            return div().into_any_element();
        };
        let content = editor.update(cx, |editor, cx| match self.kind {
            PanelKind::FileList => editor.render_file_list(cx),
            PanelKind::WorkspaceSearch => {
                if !window.is_window_active() {
                    editor.workspace_search_disclosure.borrow_mut().take();
                }
                editor.prepare_search_disclosure();
                editor.render_workspace_search_panel(cx)
            }
            PanelKind::Outline => {
                if !window.is_window_active() {
                    editor.outline_fold_motion.borrow_mut().take();
                }
                editor.render_outline_panel(cx)
            }
        });
        if self.kind == PanelKind::Outline && editor.read(cx).outline_fold_motion_running() {
            window.request_presentation_frame();
        }
        if self.kind == PanelKind::WorkspaceSearch && editor.read(cx).search_disclosure_running() {
            window.request_presentation_frame();
        }
        content
    }
}

pub(super) struct MotionOverlay {
    editor: WeakEntity<EditorView>,
    kind: OverlayKind,
    visible: bool,
    opacity: Transition,
    _subscription: Subscription,
}

impl MotionOverlay {
    fn new(editor: Entity<EditorView>, kind: OverlayKind, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&editor, |this, editor, cx| {
            if !this.kind.visible(editor.read(cx)) {
                this.visible = false;
            } else {
                cx.notify();
            }
        });
        Self {
            editor: editor.downgrade(),
            kind,
            visible: false,
            opacity: Transition::settled(0.0, Instant::now()),
            _subscription: subscription,
        }
    }
}

impl Render for MotionOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(editor) = self.editor.upgrade() else {
            return div().into_any_element();
        };
        if !self.kind.visible(editor.read(cx)) {
            self.visible = false;
            return div().into_any_element();
        }
        let now = Instant::now();
        let spec = &editor.read(cx).styles.ui.spec;
        let duration = if window.is_window_active() {
            spec.motion_duration(spec.overlay_motion_duration)
        } else {
            Duration::ZERO
        };
        if !self.visible {
            self.opacity = Transition::settled(0.0, now);
            self.visible = true;
        }
        self.opacity.set_easing(spec.easing, now);
        self.opacity.retarget(1.0, duration, now);
        let opacity = if self.kind == OverlayKind::Notifications {
            1.0
        } else {
            self.opacity.value(now)
        };
        if self.kind != OverlayKind::Notifications && self.opacity.running(now) {
            window.request_presentation_frame();
        }
        let content = editor.update(cx, |editor, cx| match self.kind {
            OverlayKind::FontPicker => editor.render_font_picker_overlay(
                editor
                    .settings_font_picker
                    .as_ref()
                    .expect("visible picker"),
                cx,
            ),
            OverlayKind::QuickOpen => editor.render_quick_open_overlay(cx),
            OverlayKind::CommandPalette => editor.render_command_palette_overlay(cx),
            OverlayKind::LanguagePicker => editor.render_language_picker_overlay(cx),
            OverlayKind::Dialog => editor.render_dialog(cx),
            OverlayKind::Notifications => editor.render_notifications(cx),
        });
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .opacity(opacity)
            .child(content)
            .into_any_element()
    }
}

impl EditorView {
    pub(super) fn notify_panel_presentation(&self, cx: &mut Context<Self>) {
        if let Some(window) = self.presentation_window {
            let view = cx.entity_id();
            let _ = window.update(cx, |_, window, _| window.refresh_view(view));
        } else {
            cx.notify();
        }
    }

    pub(super) fn render_cached_panel(
        &mut self,
        kind: PanelKind,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let width = match kind {
            PanelKind::FileList => self.file_list_layout_width(),
            PanelKind::WorkspaceSearch => self.workspace_search_layout_width,
            PanelKind::Outline => self.utility_panel_layout_width(),
        };
        let view = self.panel_views.entry(kind).or_insert_with(|| {
            let editor = cx.entity();
            cx.new(|cx: &mut Context<PanelContentView>| PanelContentView {
                editor: editor.downgrade(),
                kind,
                _subscription: cx.observe(&editor, |view, editor, cx| {
                    if view.kind.visible(editor.read(cx)) {
                        cx.notify();
                    }
                }),
            })
        });
        gpui::AnyView::from(view.clone())
            .cached(
                gpui::StyleRefinement::default()
                    .w(gpui::px(width))
                    .h_full()
                    .flex_none(),
            )
            .into_any_element()
    }

    pub(super) fn animations_enabled(&self) -> bool {
        self.styles.ui.spec.animations_enabled
    }

    pub(super) fn settle_disabled_motion(&mut self, cx: &mut Context<Self>) {
        if self.animations_enabled() {
            return;
        }
        self.sidebar_tree_motion = None;
        self.sidebar_starred_motion = None;
        self.workspace_search_disclosure.borrow_mut().take();
        self.outline_fold_motion.borrow_mut().take();
        self.sync_panel_display_state();
        self.sync_minimap_motion(cx);
        self.sync_control_style_metrics(cx);
        self.sync_toolbar_opacity(cx);
        self.notifications
            .retain(|notification| !notification.closing);
        self.context_menu_opacity_epoch = self.context_menu_opacity_epoch.wrapping_add(1);
        self.context_menu_opacity_task = None;
        if self.context_menu.is_some() && self.context_menu_opacity_target == 0.0 {
            self.close_context_menu_immediately(cx);
        } else {
            self.context_menu_opacity = self.context_menu_opacity_target;
        }
        self.outline_rail_hover_since.set(None);
        self.outline_rail_hover_from
            .set(self.outline_rail_hover_target.get());
        self.outline_rail_active_since.set(None);
        self.outline_rail_previous_active_line
            .set(self.outline_rail_last_active_line.get());
        self.outline_flash_epoch = self.outline_flash_epoch.wrapping_add(1);
        self.outline_flash_highlight_path = None;
        self.outline_flash_highlight_ranges.clear();
        self.outline_flash_highlight_current_index = None;
    }

    pub(super) fn render_motion_overlay(
        &mut self,
        kind: OverlayKind,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = self.motion_overlays.entry(kind).or_insert_with(|| {
            let editor = cx.entity().clone();
            cx.new(|cx| MotionOverlay::new(editor, kind, cx))
        });
        gpui::AnyView::from(view.clone())
            .cached(
                gpui::StyleRefinement::default()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
            .into_any_element()
    }

    pub(super) fn sync_system_motion(&mut self, window: &Window, cx: &mut Context<Self>) {
        let active = window.is_window_active();
        let became_active = active && !self.motion_window_active;
        self.motion_window_active = active;
        if !became_active || self.system_motion_task.is_some() {
            return;
        }
        self.system_motion_task = Some(cx.spawn(async move |this, cx| {
            let result = crate::platform::motion::reduced_motion().await;
            this.update(cx, |editor, cx| {
                editor.system_motion_task = None;
                match result {
                    Ok(reduced) if editor.system_reduced_motion != Some(reduced) => {
                        editor.system_reduced_motion = Some(reduced);
                        editor.set_ui_style_spec(editor.app_settings.ui_style_spec(), cx);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        log_error!("Could not read system animation preference; keeping the last known preference or enabled default: {error}");
                        if editor.system_reduced_motion.is_none() {
                            editor.system_reduced_motion = Some(false);
                            editor.set_ui_style_spec(editor.app_settings.ui_style_spec(), cx);
                        }
                    }
                }
            })
            .ok();
        }));
    }
}
