//! ACP agent chat panel: registry picker, chat surface, and history,
//! following Zed's agent panel layout language (cards, chips, status dot,
//! composer card with send/stop control).

use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, Bounds, Context, CursorStyle, Focusable, FontWeight, MouseButton, Pixels,
    SharedString, Window, deferred, div, point, prelude::*, px, size, svg,
};

use super::acp_client::{AcpConnectionStatus, PendingAcpPermission};
use super::acp_registry::{AgentInstallStatus, AgentRegistryStatus, RegistryAgent};
use super::acp_session_store::StoredAcpSession;
use super::markdown_editor::{EmbeddedMarkdownEditor, EmbeddedMarkdownEditorEvent};
use super::{EditorView, RightUtilityPanel, SearchMenuConfig, SearchMenuRow};

// Mingcute icons copied into the project (assets/icons/agent/).
const AGENT_HISTORY_SVG_PATH: &str = "assets/icons/agent/history.svg";
const AGENT_NEW_SESSION_SVG_PATH: &str = "assets/icons/agent/new-session.svg";
const AGENT_SEND_SVG_PATH: &str = "assets/icons/agent/send.svg";
const AGENT_STOP_SVG_PATH: &str = "assets/icons/agent/stop.svg";
const AGENT_CHEVRON_UP_SVG_PATH: &str = "assets/icons/agent/chevron-up.svg";
const AGENT_ATTACHMENT_SVG_PATH: &str = "assets/icons/agent/attachment.svg";
const AGENT_MODE_SVG_PATH: &str = "assets/icons/agent/mode.svg";
const AGENT_MODEL_SVG_PATH: &str = "assets/icons/agent/model.svg";
const AGENT_EFFORT_SVG_PATH: &str = "assets/icons/agent/effort.svg";
const AGENT_CONFIG_SVG_PATH: &str = "assets/icons/agent/config.svg";
const AGENT_EXPAND_SVG_PATH: &str = "assets/icons/agent/expand.svg";
const AGENT_COLLAPSE_SVG_PATH: &str = "assets/icons/agent/collapse.svg";

/// Composer editor height bounds inside the card (before UI scaling).
const COMPOSER_MIN_HEIGHT: f32 = 56.0;
const COMPOSER_MAX_HEIGHT: f32 = 180.0;
/// Header icon button rect (before UI scaling): slightly wider than tall so
/// the hover background reads as a square around the icon.
const AGENT_HEADER_BUTTON_SIZE: f32 = 22.0;
const AGENT_HEADER_BUTTON_WIDTH: f32 = 30.0;
/// A popup menu opened from the composer bottom controls.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentPopupMenu {
    /// Searchable ACP registry agent picker (opened from the header
    /// connection-status button).
    Registry,
    /// Session mode picker.
    Mode,
    /// Select-type session config picker (model, reasoning effort, …).
    /// `group` drills into a second-level menu for grouped selects.
    Config { index: usize, group: Option<usize> },
}

impl EditorView {
    pub(crate) fn toggle_agent_panel(&mut self, cx: &mut Context<Self>) {
        let opening = !self.is_utility_panel_active(RightUtilityPanel::Agent);
        self.set_visible_utility_panel(opening.then_some(RightUtilityPanel::Agent), cx);
        if opening {
            self.ensure_agent_registry_loaded(cx);
            // A restored session starts disconnected; reconnect as soon as
            // the panel becomes visible.
            self.request_acp_auto_connect(cx);
        }
    }

    fn ensure_agent_markdown_editors(&mut self, cx: &mut Context<Self>) {
        if self.agent_menu_search_editor.is_none() {
            let styles = self.search_menu_text_input_styles();
            let placeholder = self.agent_text("search");
            let editor = cx.new(move |cx| {
                let mut input = crate::text_input::TextInput::new(styles, placeholder, cx);
                input.context_menu_enabled = false;
                input
            });
            cx.subscribe(
                &editor,
                |this, _editor, event: &crate::text_input::TextInputEvent, cx| {
                    if matches!(event, crate::text_input::TextInputEvent::Changed) {
                        this.agent_menu_query = this
                            .agent_menu_search_editor
                            .as_ref()
                            .map(|editor| editor.read(cx).text().to_string())
                            .unwrap_or_default();
                        cx.notify();
                    }
                },
            )
            .detach();
            self.agent_menu_search_editor = Some(editor);
        }
        if self.agent_transcript_editor.is_none() {
            let labels = super::acp_transcript::transcript_labels(self.ui_language);
            let transcript = self.acp_panel_state.transcript.to_markdown(&labels);
            let editor =
                cx.new(move |cx| EmbeddedMarkdownEditor::new(&transcript, true, false, cx));
            editor.update(cx, |editor, cx| editor.set_ui_scale(self.ui_scale, cx));
            self.agent_transcript_editor = Some(editor);
        }
        if self.agent_composer_editor.is_none() {
            let draft = self
                .pending_acp_open_session
                .as_ref()
                .map(|session| session.composer_draft.clone())
                .unwrap_or_default();
            let composer = cx.new(move |cx| EmbeddedMarkdownEditor::new(&draft, false, true, cx));
            composer.update(cx, |editor, cx| editor.set_ui_scale(self.ui_scale, cx));
            cx.subscribe(
                &composer,
                |this, composer, event: &EmbeddedMarkdownEditorEvent, cx| match event {
                    EmbeddedMarkdownEditorEvent::Submit => {
                        // While the slash command menu is open, Enter applies
                        // the highlighted completion instead of sending.
                        if let Some((_, matches)) = this.agent_slash_state(cx) {
                            let selected = this.agent_slash_selected.min(matches.len() - 1);
                            this.apply_agent_slash_command(matches[selected], cx);
                            return;
                        }
                        if this.acp_panel_state.status != AcpConnectionStatus::Ready {
                            return;
                        }
                        let prompt = composer.read(cx).text(cx).to_string();
                        this.send_acp_prompt(prompt, cx);
                        composer.update(cx, |composer, cx| composer.clear(cx));
                    }
                    EmbeddedMarkdownEditorEvent::NavigateUp => {
                        if let Some((_, matches)) = this.agent_slash_state(cx) {
                            let len = matches.len();
                            this.agent_slash_selected =
                                (this.agent_slash_selected.min(len - 1) + len - 1) % len;
                            cx.notify();
                        }
                    }
                    EmbeddedMarkdownEditorEvent::NavigateDown => {
                        if let Some((_, matches)) = this.agent_slash_state(cx) {
                            let len = matches.len();
                            this.agent_slash_selected =
                                (this.agent_slash_selected.min(len - 1) + 1) % len;
                            cx.notify();
                        }
                    }
                },
            )
            .detach();
            // Re-render the panel whenever the composer changes so the card
            // height and slash menu track the content in the same frame.
            cx.observe(&composer, |this, composer, cx| {
                let slash = this.agent_slash_state(cx);
                let filter = slash
                    .as_ref()
                    .map(|(filter, _)| filter.clone())
                    .unwrap_or_default();
                if filter != this.agent_slash_filter {
                    this.agent_slash_filter = filter;
                    this.agent_slash_selected = 0;
                }
                let intercept = slash.is_some();
                composer.update(cx, |composer, _| {
                    composer.set_intercept_navigation(intercept)
                });
                cx.notify();
            })
            .detach();
            self.agent_composer_editor = Some(composer);
        }
        self.sync_agent_editor_fonts(cx);
    }

    /// Pushes the host's configured editor fonts (and UI font) into the
    /// panel-embedded transcript and composer editors.
    pub(crate) fn sync_agent_editor_fonts(&mut self, cx: &mut Context<Self>) {
        let body_latin = self.styles.editor.editor_body_latin_font_family;
        let body_cjk = self.styles.editor.editor_body_cjk_font_family;
        let monospace = self.styles.editor.editor_monospace_font_family;
        let ui_font = self.styles.ui_font_family;
        for editor in [
            self.agent_transcript_editor.clone(),
            self.agent_composer_editor.clone(),
        ]
        .into_iter()
        .flatten()
        {
            editor.update(cx, |editor, cx| {
                editor.set_editor_fonts(body_latin, body_cjk, monospace, ui_font, cx)
            });
        }
    }

    // ── Chat: slash command completion state ────────────────────────────

    /// Returns the active slash filter and the indices of matching commands
    /// while the composer content is a single `/token`.
    fn agent_slash_state(&self, cx: &gpui::App) -> Option<(String, Vec<usize>)> {
        let commands = &self.acp_panel_state.available_commands;
        if commands.is_empty() {
            return None;
        }
        let composer = self.agent_composer_editor.as_ref()?;
        let text = composer.read(cx).text(cx);
        if !text.starts_with('/') || text.chars().any(char::is_whitespace) {
            return None;
        }
        let filter = text[1..].to_lowercase();
        let mut prefix_matches = Vec::new();
        let mut contains_matches = Vec::new();
        for (index, command) in commands.iter().enumerate() {
            let name = command.name.to_lowercase();
            if name.starts_with(&filter) {
                prefix_matches.push(index);
            } else if name.contains(&filter) {
                contains_matches.push(index);
            }
        }
        prefix_matches.extend(contains_matches);
        if prefix_matches.is_empty() {
            return None;
        }
        Some((filter, prefix_matches))
    }

    fn apply_agent_slash_command(&mut self, command_index: usize, cx: &mut Context<Self>) {
        let Some(command) = self
            .acp_panel_state
            .available_commands
            .get(command_index)
            .cloned()
        else {
            return;
        };
        if let Some(composer) = self.agent_composer_editor.clone() {
            let insertion = format!("/{} ", command.name);
            composer.update(cx, |composer, cx| composer.set_text(&insertion, cx));
        }
        self.agent_slash_selected = 0;
        cx.notify();
    }

    fn set_agent_popup_menu(&mut self, popup_menu: Option<AgentPopupMenu>, cx: &mut Context<Self>) {
        self.agent_popup_anchor = None;
        self.set_agent_popup_menu_at(popup_menu, None, cx);
    }

    fn set_agent_popup_menu_at(
        &mut self,
        popup_menu: Option<AgentPopupMenu>,
        anchor: Option<Bounds<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        self.agent_popup_menu = popup_menu;
        self.agent_popup_anchor = anchor;
        self.agent_menu_query.clear();
        if let Some(editor) = self.agent_menu_search_editor.clone() {
            editor.update(cx, |editor, cx| editor.set_text("", cx));
        }
        cx.notify();
    }

    fn close_agent_popup_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_agent_popup_menu(None, cx);
        if let Some(composer) = &self.agent_composer_editor {
            composer.focus_handle(cx).focus(window);
        }
    }

    fn focus_agent_menu_search(&self, window: &mut Window, cx: &gpui::App) {
        if let Some(editor) = &self.agent_menu_search_editor {
            editor.focus_handle(cx).focus(window);
        }
    }

    fn agent_text(&self, key: &str) -> &'static str {
        crate::i18n::agent_panel_text(self.ui_language, key)
    }

    fn acp_status_label(&self) -> String {
        if matches!(
            self.selected_agent_install_status(),
            Some(AgentInstallStatus::Installing)
        ) {
            return self.agent_text("installing").to_string();
        }
        match &self.acp_panel_state.status {
            AcpConnectionStatus::Disconnected => self.agent_text("not_connected").to_string(),
            AcpConnectionStatus::Connecting => self.agent_text("connecting").to_string(),
            AcpConnectionStatus::Authenticating => self.agent_text("authenticating").to_string(),
            AcpConnectionStatus::Ready => self.agent_text("ready").to_string(),
            AcpConnectionStatus::Prompting => self.agent_text("working").to_string(),
            AcpConnectionStatus::Failed(error) => error.clone(),
        }
    }

    fn acp_status_color(&self) -> gpui::Hsla {
        match &self.acp_panel_state.status {
            AcpConnectionStatus::Ready => self.theme.git_status_added,
            AcpConnectionStatus::Prompting
            | AcpConnectionStatus::Connecting
            | AcpConnectionStatus::Authenticating => self.theme.text_warning,
            AcpConnectionStatus::Failed(_) => self.theme.text_danger,
            AcpConnectionStatus::Disconnected => {
                self.styles.utility_panel.utility_panel_empty_text_color
            }
        }
    }

    /// A small icon-sized header button (history / new session).
    fn render_header_icon_button(
        &self,
        id: &'static str,
        icon_path: &'static str,
        tooltip: &'static str,
        active: bool,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let styles = &self.styles.utility_panel;
        let hover_bg = styles.utility_panel_button_hover_bg;
        let button = div()
            .id(id)
            .w(px(self.scaled(AGENT_HEADER_BUTTON_WIDTH)))
            .h(px(self.scaled(AGENT_HEADER_BUTTON_SIZE)))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .when(active, |this| this.bg(hover_bg))
            .cursor(CursorStyle::Arrow)
            .hover(move |style| style.bg(hover_bg))
            .child(
                svg()
                    .path(icon_path)
                    .size(px(self.scaled(14.0)))
                    .text_color(styles.utility_panel_button_text_color),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    on_click(this, cx);
                }),
            );
        super::tooltip::with_tooltip(button, cx.entity().clone(), tooltip, None::<&'static str>)
    }

    // ── Registry (searchable menu from the header status button) ────────

    /// Connects the selected registry agent: launches it when a recipe is
    /// available, otherwise installs the platform binary first.
    pub(crate) fn connect_selected_registry_agent(&mut self, cx: &mut Context<Self>) {
        let Some(agent) = self.selected_registry_agent().cloned() else {
            return;
        };
        if self.agent_launch_recipe(&agent).is_some() {
            self.start_selected_acp_agent(cx);
        } else if agent.platform_binary().is_some() {
            self.install_selected_agent(cx);
        } else {
            let message = self.agent_text("unavailable").to_string();
            self.notify_error(message, cx);
        }
    }

    /// Rows for the searchable ACP registry agent menu.
    fn agent_registry_menu_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let query = self.agent_menu_query.trim().to_lowercase();
        let agents: Vec<RegistryAgent> = self
            .agent_registry_state
            .agents
            .iter()
            .filter(|agent| {
                query.is_empty()
                    || agent.name.to_lowercase().contains(&query)
                    || agent.description.to_lowercase().contains(&query)
            })
            .cloned()
            .collect();
        if agents.is_empty() {
            // With a query the standard "No matches" row applies; otherwise
            // surface the registry loading / failure state inside the menu.
            if !query.is_empty() {
                return Vec::new();
            }
            let message = match &self.agent_registry_state.status {
                AgentRegistryStatus::Loading => self.agent_text("loading_registry").to_string(),
                AgentRegistryStatus::Failed(error) => error.clone(),
                _ => crate::i18n::agent_panel_empty_label(self.ui_language).to_string(),
            };
            let styles = &self.styles.quick_open;
            return vec![
                div()
                    .w_full()
                    .h(px(self.scaled(styles.quick_open_empty_height_px)))
                    .px(px(self.scaled(styles.quick_open_result_padding_x_px)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(self.scaled(styles.quick_open_empty_font_size_px)))
                    .text_color(styles.quick_open_empty_text_color)
                    .child(SharedString::from(message))
                    .into_any_element(),
            ];
        }
        agents
            .into_iter()
            .enumerate()
            .map(|(index, agent)| {
                let checked = self.selected_agent_id.as_deref() == Some(agent.id.as_str());
                let installing = matches!(
                    self.agent_install_states.get(agent.id.as_str()),
                    Some(AgentInstallStatus::Installing)
                );
                let detail = if installing {
                    self.agent_text("installing").to_string()
                } else {
                    format!("{} · {}", agent.version, agent.distribution_label())
                };
                let agent_id = agent.id.clone();
                self.render_agent_menu_row(
                    ("agent-registry-item", index),
                    agent.name.clone(),
                    Some(detail),
                    checked,
                    false,
                    move |this, window, cx| {
                        this.close_agent_popup_menu(window, cx);
                        this.selected_agent_id = Some(agent_id.clone());
                        this.connect_selected_registry_agent(cx);
                    },
                    cx,
                )
            })
            .collect()
    }

    // ── History ─────────────────────────────────────────────────────────

    fn render_stored_session_row(
        &self,
        session: &StoredAcpSession,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let styles = &self.styles.utility_panel;
        let padding = self.scaled(styles.utility_panel_section_padding_x_px);
        let title = session.display_title();
        let agent_name = session.agent_name.clone();
        let timestamp: String = session
            .updated_at
            .replace('T', " ")
            .chars()
            .take(16)
            .collect();
        let session = session.clone();
        div()
            .id(("agent-stored-session", index))
            .w_full()
            .px(px(padding))
            .py(px(self.scaled(8.0)))
            .flex()
            .flex_col()
            .gap(px(self.scaled(2.0)))
            .cursor(CursorStyle::Arrow)
            .hover(move |style| style.bg(styles.utility_panel_button_hover_bg))
            .child(
                div()
                    .w_full()
                    .text_size(px(self.scaled(styles.utility_panel_item_font_size_px)))
                    .text_color(styles.utility_panel_item_value_text_color)
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(6.0)))
                    .text_size(px(self.scaled(10.5)))
                    .text_color(styles.utility_panel_empty_text_color)
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(self.scaled(120.0)))
                            .overflow_hidden()
                            .truncate()
                            .whitespace_nowrap()
                            .child(SharedString::from(agent_name)),
                    )
                    .child(SharedString::from(timestamp)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.agent_history_visible = false;
                    this.open_stored_acp_session(session.clone(), cx);
                }),
            )
            .into_any_element()
    }

    fn render_history_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let styles = &self.styles.utility_panel;
        let stored_sessions = self.stored_acp_sessions.clone();
        div()
            .id("agent-session-history")
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .px(px(self.scaled(styles.utility_panel_section_padding_x_px)))
                    .pt(px(self.scaled(10.0)))
                    .pb(px(self.scaled(4.0)))
                    .text_size(px(
                        self.scaled(styles.utility_panel_section_title_font_size_px)
                    ))
                    .text_color(styles.utility_panel_section_title_text_color)
                    .child(self.agent_text("history")),
            )
            .children(
                stored_sessions
                    .iter()
                    .enumerate()
                    .map(|(index, session)| self.render_stored_session_row(session, index, cx)),
            )
            .when(stored_sessions.is_empty(), |this| {
                this.child(
                    div()
                        .w_full()
                        .py(px(self.scaled(24.0)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(self.scaled(styles.utility_panel_empty_font_size_px)))
                        .text_color(styles.utility_panel_empty_text_color)
                        .child(self.agent_text("no_sessions")),
                )
            })
            .into_any_element()
    }

    // ── Chat: composer bottom controls & popup menus ────────────────────

    /// A compact menu-trigger button in the composer bottom row. `menu`
    /// registers the button as a popup trigger so the menu backdrop can
    /// switch menus when another trigger is clicked while one is open.
    #[allow(clippy::too_many_arguments)]
    fn render_composer_menu_button(
        &self,
        id: (&'static str, usize),
        icon_path: &'static str,
        label: String,
        tooltip: String,
        open: bool,
        menu: Option<AgentPopupMenu>,
        on_click: impl Fn(&mut Self, Option<Bounds<Pixels>>, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let styles = &self.styles.utility_panel;
        // Status-bar-like hover: square background at full row height.
        let hover_bg = crate::theme::with_alpha(self.theme.text_primary, 0.08);
        let text_color = styles.utility_panel_button_text_color;
        let bounds = Rc::new(Cell::new(None));
        let click_bounds = bounds.clone();
        let button = div()
            .id(id)
            .h_full()
            .max_w(px(self.scaled(130.0)))
            .px(px(self.scaled(7.0)))
            .flex()
            .items_center()
            .gap(px(self.scaled(3.0)))
            .text_size(px(self.scaled(10.5)))
            .text_color(text_color)
            .when(open, |this| this.bg(hover_bg))
            .cursor(CursorStyle::Arrow)
            .hover(move |style| style.bg(hover_bg))
            .child(
                svg()
                    .path(icon_path)
                    .flex_none()
                    .size(px(self.scaled(11.0)))
                    .text_color(text_color),
            )
            .child(
                div()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(SharedString::from(label)),
            )
            .child(
                svg()
                    .path(AGENT_CHEVRON_UP_SVG_PATH)
                    .flex_none()
                    .size(px(self.scaled(13.0)))
                    .text_color(styles.utility_panel_item_label_text_color),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    on_click(this, click_bounds.get(), window, cx);
                }),
            );
        let bounds_cell = bounds.clone();
        let trigger_registry = self.agent_menu_trigger_bounds.clone();
        let button = div()
            .flex_none()
            .h_full()
            .flex()
            .items_center()
            .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, _, _| {
                let first = bounds.first().copied();
                bounds_cell.set(first);
                if let (Some(menu), Some(bounds)) = (menu, first) {
                    trigger_registry.borrow_mut().push((menu, bounds));
                }
            })
            .child(button);
        super::tooltip::with_tooltip(
            button,
            cx.entity().clone(),
            SharedString::from(tooltip),
            None::<&'static str>,
        )
    }

    /// A single row inside a composer popup menu.
    #[allow(clippy::too_many_arguments)]
    fn render_agent_menu_row(
        &self,
        id: (&'static str, usize),
        label: String,
        description: Option<String>,
        checked: bool,
        submenu: bool,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row = SearchMenuRow::new(label)
            .detail(description)
            .checked(checked)
            .submenu(submenu);
        self.render_search_menu_row(id, row, on_click, cx)
    }

    /// A searchable menu positioned with the same anchored placement logic as
    /// the status-bar and settings pickers.
    fn render_agent_popup_container(
        &self,
        id: &'static str,
        rows: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let panel_bounds = self.agent_panel_bounds.get();
        let panel_width = panel_bounds
            .map(|bounds| f32::from(bounds.size.width))
            .unwrap_or_else(|| self.right_panel_width());
        let panel_height = panel_bounds
            .map(|bounds| f32::from(bounds.size.height))
            .unwrap_or(self.last_window_height);
        let panel_origin = panel_bounds.map(|bounds| bounds.origin).unwrap_or_default();
        let anchor = self.agent_popup_anchor.map_or_else(
            || {
                Bounds::new(
                    point(
                        px((panel_width - self.scaled(180.0)).max(0.0)),
                        px(panel_height),
                    ),
                    size(px(self.scaled(180.0)), px(0.0)),
                )
            },
            |anchor| {
                Bounds::new(
                    point(
                        anchor.origin.x - panel_origin.x,
                        anchor.origin.y - panel_origin.y,
                    ),
                    anchor.size,
                )
            },
        );
        let config = SearchMenuConfig {
            id,
            anchor,
            viewport_width: panel_width,
            viewport_height: panel_height,
            desired_width: self.scaled(220.0),
            match_anchor_width: false,
            row_count: rows.len(),
        };
        deferred(self.render_search_menu_panel(
            &config,
            self.agent_menu_search_editor.clone(),
            None,
            None,
            rows,
            cx,
        ))
        .into_any_element()
    }

    /// Rows for the session mode picker.
    fn agent_mode_menu_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(modes) = self.acp_panel_state.modes.clone() else {
            return Vec::new();
        };
        let query = self.agent_menu_query.trim().to_lowercase();
        modes
            .available_modes
            .iter()
            .filter(|mode| {
                query.is_empty()
                    || mode.name.to_lowercase().contains(&query)
                    || mode
                        .description
                        .as_ref()
                        .is_some_and(|description| description.to_lowercase().contains(&query))
            })
            .enumerate()
            .map(|(index, mode)| {
                let mode_id = mode.id.clone();
                let checked = mode.id == modes.current_mode_id;
                self.render_agent_menu_row(
                    ("agent-mode-item", index),
                    mode.name.clone(),
                    mode.description.clone(),
                    checked,
                    false,
                    move |this, window, cx| {
                        this.close_agent_popup_menu(window, cx);
                        this.set_acp_mode(mode_id.clone(), cx);
                    },
                    cx,
                )
            })
            .collect()
    }

    /// Rows for a select-type config option. Grouped selects show the group
    /// list first and drill into a second-level menu per group.
    fn agent_config_menu_rows(
        &self,
        config_index: usize,
        open_group: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        use agent_client_protocol::schema::v1::{
            SessionConfigKind, SessionConfigOptionValue, SessionConfigSelectOptions,
        };

        let Some(option) = self.acp_panel_state.config_options.get(config_index) else {
            return Vec::new();
        };
        let SessionConfigKind::Select(select) = &option.kind else {
            return Vec::new();
        };
        let config_id = option.id.clone();
        let current_value = select.current_value.clone();
        let capitalize = config_option_capitalizes_values(option);
        let query = self.agent_menu_query.trim().to_lowercase();

        let value_rows =
            |values: Vec<agent_client_protocol::schema::v1::SessionConfigSelectOption>,
             cx: &mut Context<Self>|
             -> Vec<AnyElement> {
                values
                    .into_iter()
                    .filter(|value| {
                        query.is_empty()
                            || value.name.to_lowercase().contains(&query)
                            || value.description.as_ref().is_some_and(|description| {
                                description.to_lowercase().contains(&query)
                            })
                    })
                    .enumerate()
                    .map(|(index, value)| {
                        let config_id = config_id.clone();
                        let value_id = value.value.clone();
                        let checked = value.value == current_value;
                        let label = if capitalize {
                            capitalize_first(&value.name)
                        } else {
                            value.name.clone()
                        };
                        self.render_agent_menu_row(
                            ("agent-config-value", index),
                            label,
                            value.description.clone(),
                            checked,
                            false,
                            move |this, window, cx| {
                                this.close_agent_popup_menu(window, cx);
                                this.set_acp_config(
                                    config_id.clone(),
                                    SessionConfigOptionValue::value_id(value_id.clone()),
                                    cx,
                                );
                            },
                            cx,
                        )
                    })
                    .collect()
            };

        match &select.options {
            SessionConfigSelectOptions::Ungrouped(values) => value_rows(values.clone(), cx),
            SessionConfigSelectOptions::Grouped(groups) => match open_group {
                // Second level: back header plus the group's option values.
                Some(group_index) => {
                    let Some(group) = groups.get(group_index) else {
                        return Vec::new();
                    };
                    let mut rows = vec![self.render_search_menu_row(
                        ("agent-config-group-back", config_index),
                        SearchMenuRow::new(group.name.clone()).back(true),
                        move |this, _, cx| {
                            this.agent_popup_menu = Some(AgentPopupMenu::Config {
                                index: config_index,
                                group: None,
                            });
                            cx.notify();
                        },
                        cx,
                    )];
                    rows.extend(value_rows(group.options.clone(), cx));
                    rows
                }
                // First level: one row per group.
                None => groups
                    .iter()
                    .enumerate()
                    .filter(|(_, group)| {
                        query.is_empty()
                            || group.name.to_lowercase().contains(&query)
                            || group.options.iter().any(|value| {
                                value.name.to_lowercase().contains(&query)
                                    || value.description.as_ref().is_some_and(|description| {
                                        description.to_lowercase().contains(&query)
                                    })
                            })
                    })
                    .map(|(group_index, group)| {
                        let contains_current = group
                            .options
                            .iter()
                            .any(|value| value.value == current_value);
                        self.render_agent_menu_row(
                            ("agent-config-group", group_index),
                            group.name.clone(),
                            None,
                            contains_current,
                            true,
                            move |this, _, cx| {
                                this.agent_popup_menu = Some(AgentPopupMenu::Config {
                                    index: config_index,
                                    group: Some(group_index),
                                });
                                this.agent_menu_query.clear();
                                cx.notify();
                            },
                            cx,
                        )
                    })
                    .collect(),
            },
            _ => Vec::new(),
        }
    }

    /// Rows for the slash command completion menu.
    fn agent_slash_menu_rows(
        &self,
        matches: &[usize],
        selected: usize,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let styles = &self.styles.context_menu;
        matches
            .iter()
            .enumerate()
            .map(|(row_index, &command_index)| {
                let command = &self.acp_panel_state.available_commands[command_index];
                let is_selected = row_index == selected;
                div()
                    .id(("agent-slash-item", row_index))
                    .w_full()
                    .h(px(self.scaled(styles.menu_item_height_px)))
                    .px(px(self.scaled(styles.menu_item_padding_x_px)))
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(8.0)))
                    .when(is_selected, |this| this.bg(styles.menu_item_selected_bg))
                    .cursor(CursorStyle::Arrow)
                    .hover(move |style| style.bg(styles.menu_item_hover_bg))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(self.scaled(styles.menu_item_font_size_px)))
                            .text_color(styles.menu_item_primary_text_color)
                            .child(SharedString::from(format!("/{}", command.name))),
                    )
                    .when(!command.description.is_empty(), |this| {
                        this.child(
                            div()
                                .min_w(px(0.0))
                                .flex_1()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_size(px(self.scaled(styles.menu_shortcut_font_size_px)))
                                .text_color(styles.menu_shortcut_text_color)
                                .child(SharedString::from(command.description.clone())),
                        )
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.apply_agent_slash_command(command_index, cx);
                        }),
                    )
                    .into_any_element()
            })
            .collect()
    }

    fn render_agent_slash_menu(&self, rows: Vec<AnyElement>, cx: &mut Context<Self>) -> AnyElement {
        let styles = &self.styles.context_menu;
        deferred(
            div()
                .id("agent-slash-menu")
                .absolute()
                .left(px(self.scaled(6.0)))
                .bottom(px(self.scaled(30.0)))
                .w(px(self.scaled(280.0)))
                .max_h(px(self.scaled(220.0)))
                .py(px(self.scaled(styles.menu_padding_y_px)))
                .flex()
                .flex_col()
                .bg(styles.menu_bg)
                .border(px(self.scaled(styles.menu_border_width_px)))
                .border_color(styles.menu_border_color)
                .rounded(px(self.scaled(styles.menu_radius_px)))
                .shadow_md()
                .overflow_y_scroll()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, _, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                    }),
                )
                .children(rows),
        )
        .into_any_element()
    }

    /// The mode / config controls shown at the left of the composer bottom row.
    fn render_composer_controls(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use agent_client_protocol::schema::v1::{
            SessionConfigKind, SessionConfigOptionValue, SessionConfigSelectOptions,
        };

        let mut controls: Vec<AnyElement> = Vec::new();

        // Some agents expose the session mode both as `SessionModeState` and
        // as a select config option named "mode"; keep only the config button.
        let has_mode_config = self.acp_panel_state.config_options.iter().any(|option| {
            matches!(option.kind, SessionConfigKind::Select(_))
                && (option.id.to_string().to_lowercase() == "mode"
                    || option.name.to_lowercase() == "mode")
        });

        if let Some(modes) = self
            .acp_panel_state
            .modes
            .as_ref()
            .filter(|_| !has_mode_config)
        {
            let label = modes
                .available_modes
                .iter()
                .find(|mode| mode.id == modes.current_mode_id)
                .map(|mode| mode.name.clone())
                .unwrap_or_else(|| modes.current_mode_id.to_string());
            let open = self.agent_popup_menu == Some(AgentPopupMenu::Mode);
            controls.push(self.render_composer_menu_button(
                ("agent-mode-button", 0),
                AGENT_MODE_SVG_PATH,
                label,
                self.agent_text("session_mode").to_string(),
                open,
                Some(AgentPopupMenu::Mode),
                move |this, anchor, window, cx| {
                    this.set_agent_popup_menu_at(
                        if open {
                            None
                        } else {
                            Some(AgentPopupMenu::Mode)
                        },
                        anchor,
                        cx,
                    );
                    if !open {
                        this.focus_agent_menu_search(window, cx);
                    }
                },
                cx,
            ));
        }

        for (index, option) in self.acp_panel_state.config_options.iter().enumerate() {
            match &option.kind {
                SessionConfigKind::Select(select) => {
                    let values = match &select.options {
                        SessionConfigSelectOptions::Ungrouped(values) => values.clone(),
                        SessionConfigSelectOptions::Grouped(groups) => groups
                            .iter()
                            .flat_map(|group| group.options.clone())
                            .collect(),
                        _ => Vec::new(),
                    };
                    if values.is_empty() {
                        continue;
                    }
                    let label = values
                        .iter()
                        .find(|value| value.value == select.current_value)
                        .map(|value| value.name.clone())
                        .unwrap_or_else(|| select.current_value.to_string());
                    let label = if config_option_capitalizes_values(option) {
                        capitalize_first(&label)
                    } else {
                        label
                    };
                    let open = matches!(
                        self.agent_popup_menu,
                        Some(AgentPopupMenu::Config { index: open_index, .. })
                            if open_index == index
                    );
                    controls.push(self.render_composer_menu_button(
                        ("agent-config-button", index),
                        config_option_icon(option),
                        label,
                        option.name.clone(),
                        open,
                        Some(AgentPopupMenu::Config { index, group: None }),
                        move |this, anchor, window, cx| {
                            this.set_agent_popup_menu_at(
                                if open {
                                    None
                                } else {
                                    Some(AgentPopupMenu::Config { index, group: None })
                                },
                                anchor,
                                cx,
                            );
                            if !open {
                                this.focus_agent_menu_search(window, cx);
                            }
                        },
                        cx,
                    ));
                }
                SessionConfigKind::Boolean(boolean) => {
                    let config_id = option.id.clone();
                    let next_value = SessionConfigOptionValue::boolean(!boolean.current_value);
                    let label = format!(
                        "{}: {}",
                        option.name,
                        crate::i18n::boolean_option_label(self.ui_language, boolean.current_value)
                    );
                    controls.push(self.render_composer_menu_button(
                        ("agent-config-toggle", index),
                        AGENT_CONFIG_SVG_PATH,
                        label,
                        option.name.clone(),
                        false,
                        None,
                        move |this, _, _, cx| {
                            this.set_acp_config(config_id.clone(), next_value.clone(), cx);
                        },
                        cx,
                    ));
                }
                _ => {}
            }
        }

        controls
    }

    // ── Chat: permission card ───────────────────────────────────────────

    fn render_acp_permission(
        &self,
        permission: &PendingAcpPermission,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use agent_client_protocol::schema::v1::PermissionOptionKind;

        let styles = &self.styles.utility_panel;
        let section_padding_x = self.scaled(styles.utility_panel_section_padding_x_px);
        let border_color = styles.utility_panel_border_color;
        let hover_bg = styles.utility_panel_button_hover_bg;
        let options = permission.request.options.clone();
        let tool_title = permission
            .request
            .tool_call
            .fields
            .title
            .clone()
            .unwrap_or_default();
        let green = self.theme.git_status_added;
        let red = self.theme.text_danger;

        div()
            .id("agent-permission")
            .flex_none()
            .mx(px(section_padding_x))
            .mb(px(self.scaled(6.0)))
            .p(px(self.scaled(8.0)))
            .flex()
            .flex_col()
            .gap(px(self.scaled(6.0)))
            .rounded_full()
            .border(px(styles.utility_panel_border_width_px))
            .border_color(border_color)
            .bg(self.theme.bg_elevated)
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(self.scaled(2.0)))
                    .child(
                        div()
                            .text_size(px(self.scaled(12.0)))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(styles.utility_panel_item_value_text_color)
                            .child(self.agent_text("permission_required")),
                    )
                    .when(!tool_title.is_empty(), |this| {
                        this.child(
                            div()
                                .w_full()
                                .text_size(px(self.scaled(11.0)))
                                .text_color(styles.utility_panel_empty_text_color)
                                .child(SharedString::from(tool_title)),
                        )
                    }),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(self.scaled(3.0)))
                    .children(options.into_iter().enumerate().map(|(index, option)| {
                        let option_id = option.option_id.clone();
                        let (glyph, glyph_color) = match option.kind {
                            PermissionOptionKind::AllowOnce => ("✓", green),
                            PermissionOptionKind::AllowAlways => ("✓✓", green),
                            _ => ("✕", red),
                        };
                        div()
                            .id(("agent-permission-option", index))
                            .h(px(self.scaled(26.0)))
                            .px(px(self.scaled(8.0)))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(self.scaled(6.0)))
                            .rounded_full()
                            .text_size(px(self.scaled(12.0)))
                            .text_color(styles.utility_panel_button_text_color)
                            .cursor(CursorStyle::Arrow)
                            .hover(move |style| style.bg(hover_bg))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(self.scaled(18.0)))
                                    .text_size(px(self.scaled(10.0)))
                                    .text_color(glyph_color)
                                    .child(glyph),
                            )
                            .child(option.name)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.select_acp_permission(option_id.clone(), cx);
                                }),
                            )
                    })),
            )
            .into_any_element()
    }

    // ── Chat: generating strip & composer ──────────────────────────────

    fn render_generating_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let styles = &self.styles.utility_panel;
        let section_padding_x = self.scaled(styles.utility_panel_section_padding_x_px);
        div()
            .w_full()
            .flex_none()
            .px(px(section_padding_x))
            .py(px(self.scaled(4.0)))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(6.0)))
                    .text_size(px(self.scaled(11.0)))
                    .text_color(styles.utility_panel_empty_text_color)
                    .child("⋯")
                    .child(self.agent_text("working")),
            )
            .child(
                div()
                    .id("agent-generating-stop")
                    .px(px(self.scaled(6.0)))
                    .py(px(self.scaled(2.0)))
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(3.0)))
                    .rounded_full()
                    .text_size(px(self.scaled(11.0)))
                    .text_color(self.theme.text_danger)
                    .cursor(CursorStyle::Arrow)
                    .hover(move |style| style.bg(styles.utility_panel_button_hover_bg))
                    .child(
                        svg()
                            .path(AGENT_STOP_SVG_PATH)
                            .size(px(self.scaled(10.0)))
                            .text_color(self.theme.text_danger),
                    )
                    .child(self.agent_text("stop"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.cancel_acp_prompt(cx);
                        }),
                    ),
            )
            .into_any_element()
    }

    fn render_composer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let styles = &self.styles.utility_panel;
        let border_width = styles.utility_panel_border_width_px;
        // Slightly stronger than the hairline separators so the card reads
        // clearly against both the panel and editor backgrounds.
        let mut card_border_color = styles.utility_panel_border_color;
        card_border_color.a = (card_border_color.a * 1.6).min(1.0);
        let button_text_color = styles.utility_panel_button_text_color;
        let empty_text_color = styles.utility_panel_empty_text_color;
        let expanded = self.agent_composer_expanded;
        let composer_editor = self.agent_composer_editor.clone();
        let is_ready = self.acp_panel_state.status == AcpConnectionStatus::Ready;
        let is_prompting = self.acp_panel_state.status == AcpConnectionStatus::Prompting;
        let composer_empty = composer_editor
            .as_ref()
            .map(|editor| editor.read(cx).text(cx).trim().is_empty())
            .unwrap_or(true);
        let editor_height = composer_editor
            .as_ref()
            .map(|editor| editor.read(cx).content_height(cx))
            .unwrap_or(0.0)
            .clamp(
                self.scaled(COMPOSER_MIN_HEIGHT),
                self.scaled(COMPOSER_MAX_HEIGHT),
            );
        // Bottom controls row matches the status bar height so the panel's
        // chrome strips line up with the editor chrome.
        let bottom_row_height = self.status_bar_height();
        let usage_label = self.acp_panel_state.usage.as_ref().map(|usage| {
            format!(
                "{} / {}",
                humanize_tokens(usage.used),
                humanize_tokens(usage.size)
            )
        });
        let attachments = self.acp_panel_state.pending_attachments.len();
        let accent = self.theme.text_accent;
        let on_accent = self.theme.text_on_accent;
        let danger = self.theme.text_danger;
        let send_enabled = is_ready && !composer_empty;
        let controls = self.render_composer_controls(cx);

        div()
            .id("agent-composer-wrap")
            .relative()
            .w_full()
            .map(|this| {
                if expanded {
                    this.flex_1().min_h(px(0.0)).flex().flex_col()
                } else {
                    this.flex_none()
                }
            })
            .pt(px(self.scaled(0.0)))
            .pb(px(self.scaled(0.0)))
            .child(
                div()
                    .w_full()
                    .when(expanded, |this| this.flex_1().min_h(px(0.0)))
                    .relative()
                    .flex()
                    .flex_col()
                    .border_t(px(border_width))
                    .border_color(card_border_color)
                    .bg(self.theme.bg_primary)
                    .overflow_hidden()
                    .child(
                        div()
                            .w_full()
                            .map(|this| {
                                if expanded {
                                    this.flex_1().min_h(px(0.0))
                                } else {
                                    this.h(px(editor_height))
                                }
                            })
                            .overflow_hidden()
                            .when_some(composer_editor, |this, editor| this.child(editor)),
                    )
                    .child(
                        div()
                            .id("agent-composer-expand")
                            .absolute()
                            .top(px(self.scaled(4.0)))
                            .right(px(self.scaled(4.0)))
                            .size(px(self.scaled(20.0)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .group("agent-composer-expand")
                            .cursor(CursorStyle::Arrow)
                            .child(
                                svg()
                                    .path(if expanded {
                                        AGENT_COLLAPSE_SVG_PATH
                                    } else {
                                        AGENT_EXPAND_SVG_PATH
                                    })
                                    .size(px(self.scaled(13.0)))
                                    .text_color(crate::theme::with_alpha(empty_text_color, 0.7))
                                    .group_hover("agent-composer-expand", {
                                        let hover_color =
                                            styles.utility_panel_item_value_text_color;
                                        move |style| style.text_color(hover_color)
                                    }),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.agent_composer_expanded = !this.agent_composer_expanded;
                                    cx.notify();
                                }),
                            ),
                    )
                    .child(
                        div()
                            .h(px(bottom_row_height))
                            .flex_none()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(self.scaled(6.0)))
                            .child(
                                div()
                                    .min_w(px(0.0))
                                    .h_full()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .gap(px(self.scaled(2.0)))
                                    .overflow_hidden()
                                    .children(controls)
                                    .when(attachments > 0, |this| {
                                        this.child(
                                            div()
                                                .flex_none()
                                                .flex()
                                                .items_center()
                                                .gap(px(self.scaled(2.0)))
                                                .text_size(px(self.scaled(10.5)))
                                                .text_color(empty_text_color)
                                                .child(
                                                    svg()
                                                        .path(AGENT_ATTACHMENT_SVG_PATH)
                                                        .size(px(self.scaled(11.0)))
                                                        .text_color(empty_text_color),
                                                )
                                                .child(format!("{attachments}")),
                                        )
                                    })
                                    .when_some(usage_label, |this, usage| {
                                        this.child(
                                            div()
                                                .flex_none()
                                                .text_size(px(self.scaled(10.5)))
                                                .text_color(empty_text_color)
                                                .child(usage),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .id("agent-send-stop")
                                    .size(px(self.scaled(18.0)))
                                    .mr(px(self.scaled(6.0)))
                                    .mb(px(self.scaled(3.0)))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .map(|this| {
                                        if is_prompting {
                                            this.child(
                                                svg()
                                                    .path(AGENT_STOP_SVG_PATH)
                                                    .size(px(self.scaled(8.0)))
                                                    .text_color(danger),
                                            )
                                        } else if send_enabled {
                                            // Slightly dimmed at rest; hover
                                            // brightens to the full accent.
                                            this.bg(crate::theme::with_alpha(accent, 0.82))
                                                .hover(move |style| style.bg(accent))
                                                .child(
                                                    svg()
                                                        .path(AGENT_SEND_SVG_PATH)
                                                        .size(px(self.scaled(10.0)))
                                                        .text_color(on_accent),
                                                )
                                        } else {
                                            this.opacity(0.4).child(
                                                svg()
                                                    .path(AGENT_SEND_SVG_PATH)
                                                    .size(px(self.scaled(10.0)))
                                                    .text_color(button_text_color),
                                            )
                                        }
                                    })
                                    .when(send_enabled || is_prompting, |this| {
                                        this.cursor(CursorStyle::Arrow).on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                if is_prompting {
                                                    this.cancel_acp_prompt(cx);
                                                    return;
                                                }
                                                let prompt = this
                                                    .agent_composer_editor
                                                    .as_ref()
                                                    .map(|editor| {
                                                        editor.read(cx).text(cx).to_string()
                                                    })
                                                    .unwrap_or_default();
                                                this.send_acp_prompt(prompt, cx);
                                                if let Some(editor) =
                                                    this.agent_composer_editor.clone()
                                                {
                                                    editor
                                                        .update(cx, |editor, cx| editor.clear(cx));
                                                }
                                            }),
                                        )
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_chat_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let styles = &self.styles.utility_panel;
        let transcript_editor = self.agent_transcript_editor.clone();
        let transcript_empty = self.acp_panel_state.transcript.is_empty();
        let is_prompting = self.acp_panel_state.status == AcpConnectionStatus::Prompting;
        let pending_permission = self.acp_panel_state.pending_permission.clone();
        let agent_name = self.acp_panel_state.agent_name.clone();
        let composer_expanded = self.agent_composer_expanded;

        div()
            .id("agent-chat-content")
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .flex()
            .flex_col()
            .when(!composer_expanded, |this| {
                this.child(
                    div()
                        .id("agent-transcript")
                        .flex_1()
                        .min_h(px(0.0))
                        .w_full()
                        .overflow_hidden()
                        .map(|this| {
                            if transcript_empty {
                                this.flex().items_center().justify_center().child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .gap(px(self.scaled(6.0)))
                                        .max_w(px(self.scaled(240.0)))
                                        .when_some(agent_name, |this, name| {
                                            this.child(
                                                div()
                                                    .text_size(px(self.scaled(13.0)))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(
                                                        styles.utility_panel_item_value_text_color,
                                                    )
                                                    .child(SharedString::from(name)),
                                            )
                                        })
                                        .child(
                                            div()
                                                .text_size(px(self.scaled(11.0)))
                                                .text_color(styles.utility_panel_empty_text_color)
                                                .child(self.agent_text("empty_hint")),
                                        ),
                                )
                            } else {
                                this.when_some(transcript_editor, |this, editor| this.child(editor))
                            }
                        }),
                )
            })
            .when(is_prompting, |this| {
                this.child(self.render_generating_strip(cx))
            })
            .when_some(pending_permission, |this, permission| {
                this.child(self.render_acp_permission(&permission, cx))
            })
            .child(self.render_composer(cx))
            .into_any_element()
    }

    // ── Panel root ──────────────────────────────────────────────────────

    pub(crate) fn render_agent_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_agent_registry_loaded(cx);
        self.ensure_agent_markdown_editors(cx);
        // Rebuilt by every trigger button's prepaint callback this frame.
        self.agent_menu_trigger_bounds.borrow_mut().clear();
        let styles = &self.styles.utility_panel;
        let panel_bg = styles.utility_panel_bg;
        let panel_border_width = styles.utility_panel_border_width_px;
        let panel_border_color = styles.utility_panel_border_color;
        let header_text_color = styles.utility_panel_header_text_color;
        let item_label_text_color = styles.utility_panel_item_label_text_color;
        let empty_text_color = styles.utility_panel_empty_text_color;
        let header_font_size = self.scaled(styles.utility_panel_header_font_size_px);
        let selected_name = self.acp_panel_state.agent_name.clone().or_else(|| {
            self.selected_registry_agent()
                .map(|agent| agent.name.clone())
        });
        let show_history = self.agent_history_visible;
        let status_label = self.acp_status_label();
        let status_color = self.acp_status_color();
        let history_visible = self.agent_history_visible;
        let menu_open = self.agent_popup_menu.is_some();
        let registry_open = self.agent_popup_menu == Some(AgentPopupMenu::Registry);
        let status_hover_bg = crate::theme::with_alpha(self.theme.text_primary, 0.08);

        let popup_rows = match self.agent_popup_menu {
            Some(AgentPopupMenu::Registry) => self.agent_registry_menu_rows(cx),
            Some(AgentPopupMenu::Mode) => self.agent_mode_menu_rows(cx),
            Some(AgentPopupMenu::Config { index, group }) => {
                self.agent_config_menu_rows(index, group, cx)
            }
            None => Vec::new(),
        };
        let slash_rows = match (self.agent_slash_state(cx), self.agent_popup_menu) {
            (Some((_, matches)), None) => {
                let selected = self.agent_slash_selected.min(matches.len() - 1);
                self.agent_slash_menu_rows(&matches, selected, cx)
            }
            _ => Vec::new(),
        };

        let body = if show_history {
            self.render_history_body(cx)
        } else {
            self.render_chat_body(cx)
        };

        // Connection status button: opens the searchable agent registry menu.
        let connection_anchor = Rc::new(Cell::new(None));
        let connection_anchor_click = connection_anchor.clone();
        let connection_button = div()
            .id("agent-connection-button")
            .h(px(self.scaled(AGENT_HEADER_BUTTON_SIZE)))
            .min_w(px(0.0))
            .px(px(self.scaled(6.0)))
            .flex()
            .items_center()
            .gap(px(self.scaled(5.0)))
            .when(registry_open, |this| this.bg(status_hover_bg))
            .cursor(CursorStyle::Arrow)
            .hover(move |style| style.bg(status_hover_bg))
            .child(
                div()
                    .flex_none()
                    .size(px(self.scaled(7.0)))
                    .rounded_full()
                    .bg(status_color),
            )
            .when_some(selected_name, |this, name| {
                this.child(
                    div()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .truncate()
                        .whitespace_nowrap()
                        .text_size(px(self.scaled(11.5)))
                        .text_color(item_label_text_color)
                        .child(SharedString::from(name)),
                )
            })
            .child(
                div()
                    .flex_none()
                    .text_size(px(self.scaled(10.5)))
                    .text_color(empty_text_color)
                    .child(SharedString::from(status_label)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    if this.agent_popup_menu == Some(AgentPopupMenu::Registry) {
                        this.close_agent_popup_menu(window, cx);
                    } else {
                        this.ensure_agent_registry_loaded(cx);
                        this.set_agent_popup_menu_at(
                            Some(AgentPopupMenu::Registry),
                            connection_anchor_click.get(),
                            cx,
                        );
                        this.focus_agent_menu_search(window, cx);
                    }
                }),
            );
        let connection_anchor_cell = connection_anchor.clone();
        let connection_trigger_registry = self.agent_menu_trigger_bounds.clone();
        let connection_button = div()
            .min_w(px(0.0))
            .flex()
            .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, _, _| {
                let first = bounds.first().copied();
                connection_anchor_cell.set(first);
                if let Some(bounds) = first {
                    connection_trigger_registry
                        .borrow_mut()
                        .push((AgentPopupMenu::Registry, bounds));
                }
            })
            .child(connection_button);

        let panel = div()
            .id("agent-panel")
            .size_full()
            .flex()
            .flex_col()
            .bg(panel_bg)
            .border_l(px(panel_border_width))
            .border_color(panel_border_color)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(6.0)))
                    .border_b(px(panel_border_width))
                    .border_color(panel_border_color)
                    .text_size(px(header_font_size))
                    .text_color(header_text_color)
                    .child(connection_button)
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .child(self.render_header_icon_button(
                                "agent-session-history-toggle",
                                AGENT_HISTORY_SVG_PATH,
                                crate::i18n::agent_panel_text(self.ui_language, "history"),
                                history_visible,
                                |this, cx| {
                                    this.agent_history_visible = !this.agent_history_visible;
                                    if this.agent_history_visible {
                                        this.refresh_stored_acp_sessions(cx);
                                    }
                                    cx.notify();
                                },
                                cx,
                            ))
                            .child(self.render_header_icon_button(
                                "agent-new-session",
                                AGENT_NEW_SESSION_SVG_PATH,
                                crate::i18n::agent_panel_text(self.ui_language, "new_session"),
                                false,
                                |this, cx| this.new_acp_session(cx),
                                cx,
                            )),
                    ),
            )
            .child(body)
            .when(menu_open, |this| {
                // Click-away backdrop: painted above the panel body but below
                // the deferred popup menu. Clicks on another trigger button
                // switch straight to that menu; anywhere else dismisses.
                this.child(
                    div()
                        .id("agent-menu-backdrop")
                        .occlude()
                        .absolute()
                        .top(px(0.0))
                        .left(px(0.0))
                        .size_full()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                let hit = this
                                    .agent_menu_trigger_bounds
                                    .borrow()
                                    .iter()
                                    .find(|(_, bounds)| bounds.contains(&event.position))
                                    .map(|(menu, bounds)| (*menu, *bounds));
                                let Some((menu, bounds)) = hit else {
                                    this.close_agent_popup_menu(window, cx);
                                    return;
                                };
                                let same = match (this.agent_popup_menu, menu) {
                                    (
                                        Some(AgentPopupMenu::Config { index: open, .. }),
                                        AgentPopupMenu::Config { index, .. },
                                    ) => open == index,
                                    (open, menu) => open == Some(menu),
                                };
                                if same {
                                    this.close_agent_popup_menu(window, cx);
                                    return;
                                }
                                if menu == AgentPopupMenu::Registry {
                                    this.ensure_agent_registry_loaded(cx);
                                }
                                this.set_agent_popup_menu_at(Some(menu), Some(bounds), cx);
                                this.focus_agent_menu_search(window, cx);
                            }),
                        ),
                )
            })
            .when(menu_open, |this| {
                this.child(self.render_agent_popup_container("agent-popup-menu", popup_rows, cx))
            })
            .when(!slash_rows.is_empty(), |this| {
                this.child(self.render_agent_slash_menu(slash_rows, cx))
            });
        let panel_bounds = self.agent_panel_bounds.clone();
        div()
            .size_full()
            .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, _, _| {
                panel_bounds.set(bounds.first().copied());
            })
            .child(panel)
            .into_any_element()
    }
}

/// Formats a token count the way Zed's usage indicator does (`12.3k`, `1.2M`).
fn humanize_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// Picks a composer button icon matching the config option's semantics.
fn config_option_icon(
    option: &agent_client_protocol::schema::v1::SessionConfigOption,
) -> &'static str {
    let key = format!(
        "{} {}",
        option.id.to_string().to_lowercase(),
        option.name.to_lowercase()
    );
    if key.contains("model") {
        AGENT_MODEL_SVG_PATH
    } else if key.contains("effort") || key.contains("reasoning") {
        AGENT_EFFORT_SVG_PATH
    } else if key.contains("mode") {
        AGENT_MODE_SVG_PATH
    } else {
        AGENT_CONFIG_SVG_PATH
    }
}

/// Reasoning-effort values arrive lowercased (`low`/`medium`/`high`); show
/// them capitalized in the button label and menu items.
fn config_option_capitalizes_values(
    option: &agent_client_protocol::schema::v1::SessionConfigOption,
) -> bool {
    let key = format!(
        "{} {}",
        option.id.to_string().to_lowercase(),
        option.name.to_lowercase()
    );
    key.contains("effort") || key.contains("reasoning")
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
