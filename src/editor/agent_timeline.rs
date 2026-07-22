//! Virtualized per-entry rendering for the agent chat timeline.
//!
//! Each transcript entry renders as its own block: text-bearing entries
//! (user prompts, agent replies, thoughts, errors) reuse the read-only
//! embedded Markdown editor, while tool calls, plans, and notices use
//! dedicated icon-based rows (Mingcute icons, no emoji). Only the entries
//! near the viewport are rendered; the rest is represented by top/bottom
//! spacers, and streaming updates only touch the affected entry's editor
//! instead of re-projecting the whole transcript.

use agent_client_protocol::schema::v1::{PlanEntryStatus, ToolCallStatus, ToolKind};
use gpui::{
    Animation, AnimationExt, AnyElement, Context, CursorStyle, FontWeight, MouseButton,
    SharedString, Transformation, div, point, prelude::*, px, radians, svg,
};

use super::acp_client::AcpConnectionStatus;
use super::acp_transcript::{TranscriptEntry, TranscriptLabels};
use super::agent_panel::{
    AGENT_ATTACHMENT_SVG_PATH, AGENT_CHEVRON_UP_SVG_PATH, AGENT_STATUS_DONE_SVG_PATH,
    AGENT_STATUS_FAILED_SVG_PATH, AGENT_STATUS_PENDING_SVG_PATH, AGENT_STATUS_RUNNING_SVG_PATH,
};
use super::markdown_editor::EmbeddedMarkdownEditor;
use super::{EditorView, PanelScrollbarKind};

// Mingcute icons copied into the project (assets/icons/agent/).
const AGENT_THINKING_SVG_PATH: &str = "assets/icons/agent/thinking.svg";
const AGENT_PLAN_SVG_PATH: &str = "assets/icons/agent/plan.svg";
const AGENT_ERROR_SVG_PATH: &str = "assets/icons/agent/error.svg";
const AGENT_ALERT_SVG_PATH: &str = "assets/icons/agent/alert.svg";
const AGENT_TOOL_READ_SVG_PATH: &str = "assets/icons/agent/tool-read.svg";
const AGENT_TOOL_EDIT_SVG_PATH: &str = "assets/icons/agent/tool-edit.svg";
const AGENT_TOOL_DELETE_SVG_PATH: &str = "assets/icons/agent/tool-delete.svg";
const AGENT_TOOL_MOVE_SVG_PATH: &str = "assets/icons/agent/tool-move.svg";
const AGENT_TOOL_SEARCH_SVG_PATH: &str = "assets/icons/agent/tool-search.svg";
const AGENT_TOOL_EXECUTE_SVG_PATH: &str = "assets/icons/agent/tool-execute.svg";
const AGENT_TOOL_FETCH_SVG_PATH: &str = "assets/icons/agent/tool-fetch.svg";
const AGENT_TOOL_SWITCH_SVG_PATH: &str = "assets/icons/agent/tool-switch.svg";
const AGENT_TOOL_GENERIC_SVG_PATH: &str = "assets/icons/agent/tool-generic.svg";

/// Header/notice row height (before UI scaling).
const TIMELINE_ROW_HEIGHT: f32 = 22.0;
/// Tool-call detail line height (before UI scaling).
const TIMELINE_DETAIL_ROW_HEIGHT: f32 = 17.0;
/// Plan checklist row height (before UI scaling).
const TIMELINE_PLAN_ROW_HEIGHT: f32 = 19.0;
/// User-card inner padding (before UI scaling).
const TIMELINE_CARD_PADDING: f32 = 8.0;
/// Attachment row height inside a user card (before UI scaling).
const TIMELINE_ATTACHMENT_ROW_HEIGHT: f32 = 18.0;
/// Vertical gap between timeline blocks (before UI scaling).
const TIMELINE_BLOCK_GAP: f32 = 12.0;
/// Padding above the first block (before UI scaling).
const TIMELINE_TOP_PADDING: f32 = 8.0;

/// Mirror of one transcript entry in the timeline view: which editor entity
/// renders it (text entries only) and how much content is already synced.
pub(crate) struct AgentTimelineItem {
    pub kind: TimelineItemKind,
    pub editor: Option<gpui::Entity<EmbeddedMarkdownEditor>>,
    /// Synced byte length for text entries; row count for tool calls/plans.
    pub synced_len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TimelineItemKind {
    User,
    Agent,
    Thought,
    ToolCall,
    Plan,
    TurnEnded,
    Error,
}

fn entry_kind(entry: &TranscriptEntry) -> TimelineItemKind {
    match entry {
        TranscriptEntry::User { .. } => TimelineItemKind::User,
        TranscriptEntry::Agent { .. } => TimelineItemKind::Agent,
        TranscriptEntry::Thought { .. } => TimelineItemKind::Thought,
        TranscriptEntry::ToolCall { .. } => TimelineItemKind::ToolCall,
        TranscriptEntry::Plan { .. } => TimelineItemKind::Plan,
        TranscriptEntry::TurnEnded { .. } => TimelineItemKind::TurnEnded,
        TranscriptEntry::Error { .. } => TimelineItemKind::Error,
    }
}

fn entry_text(entry: &TranscriptEntry) -> Option<&str> {
    match entry {
        TranscriptEntry::User { text, .. }
        | TranscriptEntry::Agent { text }
        | TranscriptEntry::Thought { text }
        | TranscriptEntry::Error { message: text } => Some(text),
        _ => None,
    }
}

fn tool_kind_icon_path(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Read => AGENT_TOOL_READ_SVG_PATH,
        ToolKind::Edit => AGENT_TOOL_EDIT_SVG_PATH,
        ToolKind::Delete => AGENT_TOOL_DELETE_SVG_PATH,
        ToolKind::Move => AGENT_TOOL_MOVE_SVG_PATH,
        ToolKind::Search => AGENT_TOOL_SEARCH_SVG_PATH,
        ToolKind::Execute => AGENT_TOOL_EXECUTE_SVG_PATH,
        ToolKind::Think => AGENT_THINKING_SVG_PATH,
        ToolKind::Fetch => AGENT_TOOL_FETCH_SVG_PATH,
        ToolKind::SwitchMode => AGENT_TOOL_SWITCH_SVG_PATH,
        _ => AGENT_TOOL_GENERIC_SVG_PATH,
    }
}

/// A continuously rotating loading icon for in-progress states.
pub(crate) fn spinning_status_icon(
    id: SharedString,
    size_px: f32,
    color: gpui::Hsla,
) -> AnyElement {
    svg()
        .path(AGENT_STATUS_RUNNING_SVG_PATH)
        .size(px(size_px))
        .flex_none()
        .text_color(color)
        .with_animation(
            id,
            Animation::new(std::time::Duration::from_millis(900)).repeat(),
            |icon, progress| {
                icon.with_transformation(Transformation::rotate(radians(
                    progress * std::f32::consts::TAU,
                )))
            },
        )
        .into_any_element()
}

impl EditorView {
    /// Reconciles the timeline view state with the transcript: creates or
    /// updates per-entry Markdown editors incrementally (append-only text
    /// gets `append_text`, replaced text gets `set_text`), and rebuilds
    /// everything when the transcript identity (epoch) changed.
    pub(crate) fn sync_agent_timeline(&mut self, cx: &mut Context<Self>) {
        let epoch = self.acp_panel_state.transcript.epoch;
        if self.agent_timeline_epoch != epoch {
            self.agent_timeline_epoch = epoch;
            self.agent_timeline_items.clear();
            self.agent_thought_expanded.clear();
            self.agent_timeline_stick_to_bottom.set(true);
        }
        let entry_count = self.acp_panel_state.transcript.entries.len();
        self.agent_timeline_items.truncate(entry_count);

        let ui_scale = self.ui_scale;
        let body_latin = self.styles.editor.editor_body_latin_font_family;
        let body_cjk = self.styles.editor.editor_body_cjk_font_family;
        let monospace = self.styles.editor.editor_monospace_font_family;
        let ui_font = self.styles.ui_font_family;

        for index in 0..entry_count {
            let kind = entry_kind(&self.acp_panel_state.transcript.entries[index]);
            if self
                .agent_timeline_items
                .get(index)
                .is_some_and(|item| item.kind != kind)
            {
                self.agent_timeline_items.truncate(index);
            }
            if self.agent_timeline_items.len() <= index {
                self.agent_timeline_items.push(AgentTimelineItem {
                    kind,
                    editor: None,
                    synced_len: 0,
                });
            }
            match &self.acp_panel_state.transcript.entries[index] {
                TranscriptEntry::ToolCall { details, .. } => {
                    self.agent_timeline_items[index].synced_len = details.len();
                }
                TranscriptEntry::Plan { entries } => {
                    self.agent_timeline_items[index].synced_len = entries.len();
                }
                TranscriptEntry::TurnEnded { .. } => {}
                entry => {
                    let Some(text) = entry_text(entry) else {
                        continue;
                    };
                    let desired_len = text.len();
                    let item = &mut self.agent_timeline_items[index];
                    if let Some(editor) = item.editor.clone() {
                        if item.synced_len != desired_len {
                            if desired_len > item.synced_len
                                && text.is_char_boundary(item.synced_len)
                            {
                                let suffix = &text[item.synced_len..];
                                editor.update(cx, |editor, cx| editor.append_text(suffix, cx));
                            } else {
                                editor.update(cx, |editor, cx| editor.set_text(text, cx));
                            }
                            item.synced_len = desired_len;
                        }
                    } else {
                        let editor =
                            cx.new(|cx| EmbeddedMarkdownEditor::new(text, true, false, cx));
                        editor.update(cx, |editor, cx| {
                            editor.set_ui_scale(ui_scale, cx);
                            editor.set_editor_fonts(body_latin, body_cjk, monospace, ui_font, cx);
                        });
                        // Re-render the panel when the editor's measured
                        // height changes so block heights stay accurate.
                        cx.observe(&editor, |_, _, cx| cx.notify()).detach();
                        item.editor = Some(editor);
                        item.synced_len = desired_len;
                    }
                }
            }
        }
    }

    /// The virtualized chat timeline: an outer scroll container with
    /// top/bottom spacers and per-entry blocks for the visible window.
    pub(crate) fn render_agent_timeline(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.sync_agent_timeline(cx);
        let labels = super::acp_transcript::transcript_labels(self.ui_language);
        let is_streaming = self.acp_panel_state.status == AcpConnectionStatus::Prompting;
        let entry_count = self.acp_panel_state.transcript.entries.len();

        let gap = self.scaled(TIMELINE_BLOCK_GAP);
        let row_height = self.scaled(TIMELINE_ROW_HEIGHT);
        let detail_row_height = self.scaled(TIMELINE_DETAIL_ROW_HEIGHT);
        let plan_row_height = self.scaled(TIMELINE_PLAN_ROW_HEIGHT);
        let card_padding = self.scaled(TIMELINE_CARD_PADDING);
        let attachment_row_height = self.scaled(TIMELINE_ATTACHMENT_ROW_HEIGHT);
        let top_padding = self.scaled(TIMELINE_TOP_PADDING);

        // Block heights drive the virtualization window and the spacers, so
        // they must match the rendered blocks exactly. Editor heights come
        // from the last prepaint (`content_height`) and converge within a
        // frame via the per-editor observers.
        let mut heights = Vec::with_capacity(entry_count);
        for (index, entry) in self.acp_panel_state.transcript.entries.iter().enumerate() {
            let editor_height = self
                .agent_timeline_items
                .get(index)
                .and_then(|item| item.editor.as_ref())
                .map(|editor| editor.read(cx).content_height(cx))
                .unwrap_or(row_height);
            let height = match entry {
                TranscriptEntry::User { attachments, .. } => {
                    editor_height
                        + card_padding * 2.0
                        + attachments.len() as f32 * attachment_row_height
                }
                TranscriptEntry::Agent { .. } => editor_height,
                TranscriptEntry::Thought { .. } => {
                    let expanded = self
                        .agent_thought_expanded
                        .get(&index)
                        .copied()
                        .unwrap_or(is_streaming && index + 1 == entry_count);
                    row_height
                        + if expanded {
                            editor_height + self.scaled(4.0)
                        } else {
                            0.0
                        }
                }
                TranscriptEntry::ToolCall { details, .. } => {
                    row_height + details.len().min(4) as f32 * detail_row_height
                }
                TranscriptEntry::Plan { entries } => {
                    row_height + entries.len() as f32 * plan_row_height
                }
                TranscriptEntry::TurnEnded { .. } => row_height,
                TranscriptEntry::Error { .. } => editor_height + card_padding * 2.0,
            };
            heights.push((height + gap).max(1.0));
        }

        let window = super::list_viewport::variable_list_window(
            &self.agent_timeline_scroll_handle,
            top_padding,
            &heights,
            2,
            0,
        );

        let mut rows: Vec<AnyElement> = Vec::with_capacity(window.render.len() + 2);
        rows.push(
            div()
                .h(px(window.top_spacer_px))
                .flex_shrink_0()
                .into_any_element(),
        );
        for index in window.render.clone() {
            rows.push(self.render_timeline_entry(
                index,
                heights[index] - gap,
                is_streaming,
                entry_count,
                &labels,
                cx,
            ));
        }
        rows.push(
            div()
                .h(px(window.bottom_spacer_px))
                .flex_shrink_0()
                .into_any_element(),
        );

        // Dragging the panel scrollbar detaches bottom-following, exactly
        // like scrolling up with the wheel does.
        if self.panel_scrollbar_dragging(PanelScrollbarKind::AgentChat) {
            self.agent_timeline_stick_to_bottom.set(false);
        }
        let stick = self.agent_timeline_stick_to_bottom.clone();
        let scroll_handle = self.agent_timeline_scroll_handle.clone();
        let scrollbar = self.render_scrollbar_for_handle(
            PanelScrollbarKind::AgentChat,
            &self.agent_timeline_scroll_handle,
            f32::from(self.agent_timeline_scroll_handle.bounds().size.height),
            0.0,
            self.scaled(self.styles.editor.editor_scrollbar_right_margin_px),
            cx,
        );

        div()
            .id("agent-timeline")
            .relative()
            .size_full()
            .overflow_hidden()
            .child(
                div()
                    .size_full()
                    .on_children_prepainted(move |_, _, _| {
                        // Follow the newest content while attached to the
                        // bottom; runs post-layout so `max_offset` is fresh.
                        if !stick.get() {
                            return;
                        }
                        let max_scroll = f32::from(scroll_handle.max_offset().height);
                        let offset = scroll_handle.offset();
                        if (f32::from(offset.y) + max_scroll).abs() > 0.5 {
                            scroll_handle.set_offset(point(offset.x, px(-max_scroll)));
                        }
                    })
                    .child(
                        div()
                            .id("agent-timeline-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.agent_timeline_scroll_handle)
                            .on_scroll_wheel(cx.listener(
                                |this, event: &gpui::ScrollWheelEvent, _, cx| {
                                    this.reveal_panel_scrollbar(PanelScrollbarKind::AgentChat, cx);
                                    let delta_y = f32::from(event.delta.pixel_delta(px(16.0)).y);
                                    if delta_y > 0.0 {
                                        // Scrolling up detaches bottom-following.
                                        this.agent_timeline_stick_to_bottom.set(false);
                                    } else if delta_y < 0.0 {
                                        // Re-attach when scrolled back to the end.
                                        let handle = &this.agent_timeline_scroll_handle;
                                        let max_scroll = f32::from(handle.max_offset().height);
                                        let top = (-f32::from(handle.offset().y)).max(0.0);
                                        if top >= max_scroll - 4.0 {
                                            this.agent_timeline_stick_to_bottom.set(true);
                                        }
                                    }
                                },
                            ))
                            .pt(px(top_padding))
                            .flex()
                            .flex_col()
                            .children(rows),
                    ),
            )
            .when_some(scrollbar, |this, scrollbar| this.child(scrollbar))
            .into_any_element()
    }

    /// One timeline block. `content_height` excludes the inter-block gap,
    /// which is applied as bottom padding so the total matches `heights`.
    fn render_timeline_entry(
        &self,
        index: usize,
        content_height: f32,
        is_streaming: bool,
        entry_count: usize,
        labels: &TranscriptLabels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let styles = &self.styles.utility_panel;
        let section_padding_x = self.scaled(styles.utility_panel_section_padding_x_px);
        let gap = self.scaled(TIMELINE_BLOCK_GAP);
        let Some(entry) = self.acp_panel_state.transcript.entries.get(index) else {
            return div().into_any_element();
        };
        let editor = self
            .agent_timeline_items
            .get(index)
            .and_then(|item| item.editor.clone());

        let body: AnyElement = match entry {
            TranscriptEntry::User { attachments, .. } => {
                self.render_timeline_user(attachments, editor)
            }
            TranscriptEntry::Agent { .. } => div()
                .w_full()
                .h_full()
                .overflow_hidden()
                .when_some(editor, |this, editor| this.child(editor))
                .into_any_element(),
            TranscriptEntry::Thought { .. } => {
                let expanded = self
                    .agent_thought_expanded
                    .get(&index)
                    .copied()
                    .unwrap_or(is_streaming && index + 1 == entry_count);
                let streaming = is_streaming && index + 1 == entry_count;
                self.render_timeline_thought(index, expanded, streaming, editor, labels, cx)
            }
            TranscriptEntry::ToolCall {
                title,
                kind,
                status,
                details,
                ..
            } => self.render_timeline_tool_call(index, *kind, *status, title, details, labels),
            TranscriptEntry::Plan { entries } => self.render_timeline_plan(entries, labels),
            TranscriptEntry::TurnEnded { stop_reason } => {
                self.render_timeline_turn_ended(stop_reason, labels)
            }
            TranscriptEntry::Error { .. } => self.render_timeline_error(editor),
        };

        div()
            .id(("agent-timeline-entry", index))
            .h(px(content_height + gap))
            .flex_none()
            .w_full()
            .px(px(section_padding_x))
            .pb(px(gap))
            .child(body)
            .into_any_element()
    }

    /// User prompt: an elevated card with the Markdown text and one
    /// attachment row per resource link.
    fn render_timeline_user(
        &self,
        attachments: &[(String, String)],
        editor: Option<gpui::Entity<EmbeddedMarkdownEditor>>,
    ) -> AnyElement {
        let styles = &self.styles.utility_panel;
        let card_padding = self.scaled(TIMELINE_CARD_PADDING);
        let attachment_row_height = self.scaled(TIMELINE_ATTACHMENT_ROW_HEIGHT);
        let empty_text_color = styles.utility_panel_empty_text_color;

        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .p(px(card_padding))
            .border(px(styles.utility_panel_border_width_px))
            .border_color(styles.utility_panel_border_color)
            .bg(self.theme.bg_elevated)
            .overflow_hidden()
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .when_some(editor, |this, editor| this.child(editor)),
            )
            .children(attachments.iter().map(|(name, _uri)| {
                div()
                    .h(px(attachment_row_height))
                    .flex_none()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(5.0)))
                    .child(
                        svg()
                            .path(AGENT_ATTACHMENT_SVG_PATH)
                            .size(px(self.scaled(11.0)))
                            .flex_none()
                            .text_color(empty_text_color),
                    )
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(px(self.scaled(11.0)))
                            .text_color(empty_text_color)
                            .child(SharedString::from(name.clone())),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// Collapsible thinking block: icon + label header with a rotating
    /// chevron; the expanded body renders the reasoning as Markdown.
    fn render_timeline_thought(
        &self,
        index: usize,
        expanded: bool,
        streaming: bool,
        editor: Option<gpui::Entity<EmbeddedMarkdownEditor>>,
        labels: &TranscriptLabels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let styles = &self.styles.utility_panel;
        let row_height = self.scaled(TIMELINE_ROW_HEIGHT);
        let label_color = styles.utility_panel_empty_text_color;
        let hover_bg = styles.utility_panel_button_hover_bg;

        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id(("agent-thought-toggle", index))
                    .h(px(row_height))
                    .flex_none()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(5.0)))
                    .cursor(CursorStyle::Arrow)
                    .hover(move |style| style.bg(hover_bg))
                    .child(if streaming {
                        spinning_status_icon(
                            SharedString::from(format!("agent-thought-spinner-{index}")),
                            self.scaled(13.0),
                            label_color,
                        )
                    } else {
                        svg()
                            .path(AGENT_THINKING_SVG_PATH)
                            .size(px(self.scaled(13.0)))
                            .flex_none()
                            .text_color(label_color)
                            .into_any_element()
                    })
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(px(self.scaled(11.0)))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(label_color)
                            .child(labels.thinking),
                    )
                    .child(
                        svg()
                            .path(AGENT_CHEVRON_UP_SVG_PATH)
                            .size(px(self.scaled(14.0)))
                            .flex_none()
                            .when(!expanded, |this| {
                                this.with_transformation(Transformation::rotate(radians(
                                    std::f32::consts::PI,
                                )))
                            })
                            .text_color(label_color),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.agent_thought_expanded.insert(index, !expanded);
                            cx.notify();
                        }),
                    ),
            )
            .when(expanded, |this| {
                this.child(
                    div()
                        .w_full()
                        .flex_1()
                        .min_h(px(0.0))
                        .mt(px(self.scaled(4.0)))
                        .pl(px(self.scaled(7.0)))
                        .border_l(px(self.scaled(2.0)))
                        .border_color(styles.utility_panel_border_color)
                        .overflow_hidden()
                        .when_some(editor, |this, editor| this.child(editor)),
                )
            })
            .into_any_element()
    }

    /// Tool call: kind icon + title + status icon, plus short detail lines.
    fn render_timeline_tool_call(
        &self,
        index: usize,
        kind: ToolKind,
        status: ToolCallStatus,
        title: &str,
        details: &[String],
        labels: &TranscriptLabels,
    ) -> AnyElement {
        let styles = &self.styles.utility_panel;
        let row_height = self.scaled(TIMELINE_ROW_HEIGHT);
        let detail_row_height = self.scaled(TIMELINE_DETAIL_ROW_HEIGHT);
        let icon_color = styles.utility_panel_item_label_text_color;
        let title_color = styles.utility_panel_item_value_text_color;
        let detail_color = styles.utility_panel_empty_text_color;
        let (status_icon, status_color, status_label) = match status {
            ToolCallStatus::InProgress => (
                AGENT_STATUS_RUNNING_SVG_PATH,
                self.theme.text_accent,
                labels.tool_running,
            ),
            ToolCallStatus::Completed => (
                AGENT_STATUS_DONE_SVG_PATH,
                self.theme.git_status_added,
                labels.tool_done,
            ),
            ToolCallStatus::Failed => (
                AGENT_STATUS_FAILED_SVG_PATH,
                self.theme.text_danger,
                labels.tool_failed,
            ),
            _ => (
                AGENT_STATUS_PENDING_SVG_PATH,
                styles.utility_panel_empty_text_color,
                labels.tool_pending,
            ),
        };
        let icon_slot = self.scaled(13.0);
        let indent = icon_slot + self.scaled(6.0);

        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(row_height))
                    .flex_none()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(6.0)))
                    .child(
                        svg()
                            .path(tool_kind_icon_path(kind))
                            .size(px(icon_slot))
                            .flex_none()
                            .text_color(icon_color),
                    )
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(px(self.scaled(12.0)))
                            .text_color(title_color)
                            .child(SharedString::from(title.to_string())),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(self.scaled(3.0)))
                            .child(if status == ToolCallStatus::InProgress {
                                spinning_status_icon(
                                    SharedString::from(format!("agent-tool-running-{index}")),
                                    self.scaled(12.0),
                                    status_color,
                                )
                            } else {
                                svg()
                                    .path(status_icon)
                                    .size(px(self.scaled(12.0)))
                                    .flex_none()
                                    .text_color(status_color)
                                    .into_any_element()
                            })
                            .child(
                                div()
                                    .text_size(px(self.scaled(10.5)))
                                    .text_color(status_color)
                                    .child(status_label),
                            ),
                    ),
            )
            .children(details.iter().take(4).map(|detail| {
                let line = detail.replace('\n', " ");
                div()
                    .h(px(detail_row_height))
                    .flex_none()
                    .w_full()
                    .pl(px(indent))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(px(self.scaled(11.0)))
                            .text_color(detail_color)
                            .child(SharedString::from(line)),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// Execution plan: header + one status-icon checklist row per entry.
    fn render_timeline_plan(
        &self,
        entries: &[(PlanEntryStatus, String)],
        labels: &TranscriptLabels,
    ) -> AnyElement {
        let styles = &self.styles.utility_panel;
        let row_height = self.scaled(TIMELINE_ROW_HEIGHT);
        let plan_row_height = self.scaled(TIMELINE_PLAN_ROW_HEIGHT);
        let label_color = styles.utility_panel_empty_text_color;
        let done_color = self.theme.git_status_added;
        let running_color = self.theme.text_accent;
        let icon_slot = self.scaled(13.0);
        let indent = icon_slot + self.scaled(6.0);

        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(row_height))
                    .flex_none()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(6.0)))
                    .child(
                        svg()
                            .path(AGENT_PLAN_SVG_PATH)
                            .size(px(icon_slot))
                            .flex_none()
                            .text_color(label_color),
                    )
                    .child(
                        div()
                            .text_size(px(self.scaled(11.0)))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(label_color)
                            .child(labels.plan),
                    ),
            )
            .children(entries.iter().enumerate().map(|(row, (status, content))| {
                let (icon, color, dim) = match status {
                    PlanEntryStatus::Completed => (AGENT_STATUS_DONE_SVG_PATH, done_color, true),
                    PlanEntryStatus::InProgress => {
                        (AGENT_STATUS_RUNNING_SVG_PATH, running_color, false)
                    }
                    _ => (AGENT_STATUS_PENDING_SVG_PATH, label_color, false),
                };
                let text_color = if dim {
                    styles.utility_panel_empty_text_color
                } else {
                    styles.utility_panel_item_value_text_color
                };
                let line = content.replace('\n', " ");
                div()
                    .h(px(plan_row_height))
                    .flex_none()
                    .w_full()
                    .pl(px(indent))
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(5.0)))
                    .child(if matches!(status, PlanEntryStatus::InProgress) {
                        spinning_status_icon(
                            SharedString::from(format!("agent-plan-running-{row}")),
                            self.scaled(11.0),
                            color,
                        )
                    } else {
                        svg()
                            .path(icon)
                            .size(px(self.scaled(11.0)))
                            .flex_none()
                            .text_color(color)
                            .into_any_element()
                    })
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(px(self.scaled(11.5)))
                            .text_color(text_color)
                            .child(SharedString::from(line)),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// Non-standard turn end (`max_tokens`, `refusal`, …) as a notice row.
    fn render_timeline_turn_ended(
        &self,
        stop_reason: &str,
        labels: &TranscriptLabels,
    ) -> AnyElement {
        let row_height = self.scaled(TIMELINE_ROW_HEIGHT);
        let warning = self.theme.text_warning;

        div()
            .h(px(row_height))
            .flex_none()
            .w_full()
            .flex()
            .items_center()
            .gap(px(self.scaled(6.0)))
            .child(
                svg()
                    .path(AGENT_ALERT_SVG_PATH)
                    .size(px(self.scaled(13.0)))
                    .flex_none()
                    .text_color(warning),
            )
            .child(
                div()
                    .min_w(px(0.0))
                    .flex_1()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(px(self.scaled(11.0)))
                    .text_color(warning)
                    .child(SharedString::from(format!(
                        "{} {stop_reason}",
                        labels.turn_stopped
                    ))),
            )
            .into_any_element()
    }

    /// Connection/agent error: a bordered card with the message rendered as
    /// wrapped Markdown next to an error icon.
    fn render_timeline_error(
        &self,
        editor: Option<gpui::Entity<EmbeddedMarkdownEditor>>,
    ) -> AnyElement {
        let styles = &self.styles.utility_panel;
        let card_padding = self.scaled(TIMELINE_CARD_PADDING);
        let danger = self.theme.text_danger;
        let mut border_color = danger;
        border_color.a *= 0.35;

        div()
            .w_full()
            .h_full()
            .flex()
            .items_start()
            .gap(px(self.scaled(6.0)))
            .p(px(card_padding))
            .border(px(styles.utility_panel_border_width_px))
            .border_color(border_color)
            .bg(self.theme.bg_elevated)
            .overflow_hidden()
            .child(
                svg()
                    .path(AGENT_ERROR_SVG_PATH)
                    .size(px(self.scaled(13.0)))
                    .flex_none()
                    .mt(px(self.scaled(2.0)))
                    .text_color(danger),
            )
            .child(
                div()
                    .min_w(px(0.0))
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .when_some(editor, |this, editor| this.child(editor)),
            )
            .into_any_element()
    }
}
