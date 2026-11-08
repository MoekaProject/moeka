use gpui::Context;
#[cfg(target_os = "macos")]
use gpui::{Window, point, px};

use crate::markdown::BlockType;

use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DocumentMode {
    #[default]
    Edit,
    Reading,
    Source,
}

impl DocumentMode {
    pub(crate) fn from_flags(reading: bool, source: bool) -> Self {
        if source {
            Self::Source
        } else if reading {
            Self::Reading
        } else {
            Self::Edit
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Edit => Self::Reading,
            Self::Reading => Self::Source,
            Self::Source => Self::Edit,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Edit => Self::Source,
            Self::Reading => Self::Edit,
            Self::Source => Self::Reading,
        }
    }

    pub(crate) fn icon_path(self) -> &'static str {
        match self {
            Self::Edit => EDIT_MODE_SVG_PATH,
            Self::Reading => DOC_SVG_PATH,
            Self::Source => SOURCE_MODE_SVG_PATH,
        }
    }

    fn left_aligned(self, preference: bool) -> bool {
        self == Self::Source || preference
    }

    fn word_wrap(self, source_word_wrap: bool, non_markdown: bool, left_aligned: bool) -> bool {
        if self == Self::Source {
            source_word_wrap
        } else {
            !non_markdown || !left_aligned
        }
    }

    pub(crate) fn tooltip(self, language: UiLanguage) -> &'static str {
        crate::i18n::document_mode_tooltip(
            language,
            match self {
                Self::Edit => "edit",
                Self::Reading => "reading",
                Self::Source => "source",
            },
        )
    }
}

pub(crate) struct DocumentModeScrollAnchor {
    tab_id: String,
    cursor: Cursor,
    offset: usize,
    screen_y: f32,
    epoch: usize,
    remaining_frames: u8,
    deadline: std::time::Instant,
    confirmation: CursorCenterConfirmation,
}

impl DocumentModeScrollAnchor {
    fn is_current(&self, tab_id: Option<&str>, cursor: Cursor, epoch: usize) -> bool {
        tab_id == Some(self.tab_id.as_str())
            && self.cursor == cursor
            && self.epoch == epoch
            && std::time::Instant::now() < self.deadline
    }

    fn confirm_painted_layout(
        &mut self,
        target: f32,
        painted_scroll: f32,
        measured: bool,
        previews_need_rendering: bool,
        previews_in_flight: bool,
    ) -> bool {
        let measured_target =
            (measured && !previews_need_rendering && (target - painted_scroll).abs() <= 0.5)
                .then_some(painted_scroll);
        self.needs_another_layout(
            measured_target,
            previews_in_flight && !previews_need_rendering,
        )
    }

    fn needs_another_layout(
        &mut self,
        measured_target: Option<f32>,
        previews_pending: bool,
    ) -> bool {
        if !previews_pending {
            self.remaining_frames = self.remaining_frames.saturating_sub(1);
        }
        let settled = self.confirmation.observe(measured_target, previews_pending);
        !settled && self.remaining_frames > 0 && std::time::Instant::now() < self.deadline
    }
}

fn screen_anchored_scroll_target(content_y: f32, screen_y: f32) -> f32 {
    (content_y - screen_y).max(0.0)
}

fn caret_vertical_overflow(
    cursor_top: f32,
    cursor_bottom: f32,
    viewport_top: f32,
    viewport_bottom: f32,
) -> std::cmp::Ordering {
    if cursor_bottom - cursor_top > viewport_bottom - viewport_top {
        let anchor = (cursor_top + cursor_bottom) * 0.5;
        return if anchor < viewport_top {
            std::cmp::Ordering::Less
        } else if anchor > viewport_bottom {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        };
    }
    if cursor_top < viewport_top {
        std::cmp::Ordering::Less
    } else if cursor_bottom > viewport_bottom {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

fn cursor_scroll_viewport(scroll_offset: f32, viewport_height: f32) -> (f32, f32) {
    (scroll_offset, scroll_offset + viewport_height.max(0.0))
}

fn quote_vertical_padding(
    style: &crate::style::QuoteBlockStyles,
    scale: f32,
    first: bool,
    last: bool,
    callout: bool,
    folded: bool,
) -> (f32, f32) {
    let top = if first {
        style.quote_block_padding_top_px
            + if callout {
                style.quote_block_callout_padding_top_px
            } else {
                0.0
            }
    } else {
        0.0
    };
    let bottom = if last || folded {
        style.quote_block_padding_bottom_px
            + if callout {
                style.quote_block_callout_padding_bottom_px
            } else {
                0.0
            }
    } else {
        0.0
    };
    (top * scale, bottom * scale)
}

#[test]
fn collapsed_callouts_retain_bottom_padding_with_custom_metrics() {
    let theme = crate::theme::ThemeKind::OneDark.build();
    let mut styles = crate::style::Styles::from_theme(&theme, &theme);
    let style = &mut styles.quote_block;
    style.quote_block_padding_top_px = 3.0;
    style.quote_block_padding_bottom_px = 5.0;
    style.quote_block_callout_padding_top_px = 7.0;
    style.quote_block_callout_padding_bottom_px = 11.0;
    assert_eq!(
        quote_vertical_padding(style, 1.5, true, false, true, true),
        (15.0, 24.0)
    );
    assert_eq!(
        quote_vertical_padding(style, 1.5, true, false, true, false),
        (15.0, 0.0)
    );
    assert_eq!(
        quote_vertical_padding(style, 1.5, false, true, true, false),
        (0.0, 24.0)
    );
    assert_eq!(
        quote_vertical_padding(style, 1.5, true, false, false, true),
        (4.5, 7.5)
    );
}

fn cursor_visibility_scroll_delta(
    cursor_top: f32,
    cursor_bottom: f32,
    scroll_offset: f32,
    viewport_height: f32,
) -> f32 {
    let viewport_bottom = scroll_offset + viewport_height;
    if cursor_bottom - cursor_top > viewport_height {
        return if caret_vertical_overflow(cursor_top, cursor_bottom, scroll_offset, viewport_bottom)
            == std::cmp::Ordering::Equal
        {
            0.0
        } else {
            (cursor_top + cursor_bottom - scroll_offset - viewport_bottom) * 0.5
        };
    }
    match caret_vertical_overflow(cursor_top, cursor_bottom, scroll_offset, viewport_bottom) {
        std::cmp::Ordering::Less => cursor_top - scroll_offset,
        std::cmp::Ordering::Greater => cursor_bottom - viewport_bottom,
        std::cmp::Ordering::Equal => 0.0,
    }
}

fn preview_source_cursor_is_visible(
    bounds: Option<(f32, f32)>,
    scroll_offset: f32,
    viewport_height: f32,
) -> bool {
    viewport_height > 0.0
        && bounds.is_some_and(|(top, bottom)| {
            cursor_visibility_scroll_delta(top, bottom, scroll_offset, viewport_height) == 0.0
        })
}

fn edit_scroll_target(
    scroll_before: f32,
    viewport_height: f32,
    cursor_top: f32,
    cursor_bottom: f32,
) -> f32 {
    if viewport_height <= 0.0 {
        return scroll_before.max(0.0);
    }
    (scroll_before
        + cursor_visibility_scroll_delta(cursor_top, cursor_bottom, scroll_before, viewport_height))
    .max(0.0)
}

fn centered_row_scroll_target(top: f32, bottom: f32, viewport_height: f32) -> f32 {
    ((top + bottom - viewport_height) * 0.5).max(0.0)
}

fn last_soft_row_scroll_limit(content_bottom: f32, last_row_top: f32, viewport_height: f32) -> f32 {
    last_row_top.max(content_bottom - viewport_height).max(0.0)
}

fn cursor_center_layout_target(
    measured_bounds: Option<(f32, f32)>,
    estimated_top: f32,
    estimated_height: f32,
    viewport_height: f32,
) -> f32 {
    let (top, bottom) =
        measured_bounds.unwrap_or((estimated_top, estimated_top + estimated_height));
    centered_row_scroll_target(top, bottom, viewport_height)
}

fn begin_cursor_center_epoch(scroll_epoch: &mut usize, navigation_epoch: &mut Option<usize>) {
    *scroll_epoch = scroll_epoch.wrapping_add(1);
    *navigation_epoch = Some(*scroll_epoch);
}

#[derive(Default)]
struct CursorCenterConfirmation {
    previous_target: Option<f32>,
    settled_frames: u8,
}

impl CursorCenterConfirmation {
    fn observe(&mut self, measured_target: Option<f32>, previews_pending: bool) -> bool {
        let unchanged = self
            .previous_target
            .zip(measured_target)
            .is_some_and(|(previous, current)| (previous - current).abs() <= 0.5);
        self.settled_frames = if unchanged && !previews_pending {
            self.settled_frames + 1
        } else {
            0
        };
        self.previous_target = measured_target;
        self.settled_frames >= 2
    }
}

fn settled_cursor_center_target(
    confirmation: &mut CursorCenterConfirmation,
    measured_target: Option<f32>,
    previews_pending: bool,
) -> Option<f32> {
    if confirmation.observe(measured_target, previews_pending) {
        measured_target
    } else {
        None
    }
}

#[cfg(test)]
fn typing_scroll_should_restore(
    _same_logical_line: bool,
    _pending_fence_on_same_visual_row: bool,
    _visual_geometry_ready: bool,
    scroll_before: f32,
    viewport_height: f32,
    cursor_top: f32,
    cursor_bottom: f32,
) -> bool {
    viewport_height <= 0.0
        || caret_vertical_overflow(
            cursor_top,
            cursor_bottom,
            scroll_before,
            scroll_before + viewport_height,
        ) == std::cmp::Ordering::Equal
}

#[cfg(test)]
fn typing_scroll_restore_offset(
    scroll_before: f32,
    _applied_anchor_delta: f32,
    _previous_cursor_top: Option<f32>,
    _cursor_top: f32,
) -> f32 {
    scroll_before.max(0.0)
}

#[derive(Debug, PartialEq)]
enum FrontMatterVerticalTarget {
    Line(usize),
    Source(usize),
}

fn front_matter_visible_navigation_line(
    plans: &[helpers::FrontMatterGroupPlan],
    line: usize,
    editable: bool,
) -> usize {
    if !editable && let Some(plan) = plans.iter().find(|plan| plan.hides_line(line)) {
        plan.label_line()
    } else {
        line
    }
}

fn front_matter_value_endpoint(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    line: usize,
    downward: bool,
) -> usize {
    let col = decorations
        .get(line)
        .and_then(|d| d.front_matter.as_ref())
        .map(|info| {
            if downward {
                info.value_range.start
            } else {
                info.value_range.end
            }
        })
        .unwrap_or_else(|| {
            if downward {
                0
            } else {
                buffer.line_content(line).len()
            }
        });
    buffer.clamp_offset(buffer.line_start(line) + col)
}

fn front_matter_vertical_neighbor(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    plans: &[helpers::FrontMatterGroupPlan],
    line: usize,
    downward: bool,
    editable: bool,
    delimiters_visible: bool,
    cache_rows: impl Fn(usize) -> Option<usize>,
) -> Option<FrontMatterVerticalTarget> {
    let mut target = if downward {
        line.checked_add(1)?
    } else {
        line.checked_sub(1)?
    };
    while target < buffer.line_count() {
        if !delimiters_visible && decorations.get(target).and_then(|d| d.table_info.as_ref())
            .is_some_and(|table| table.is_front_matter && table.is_separator)
            // A metadata-only empty block displays a summary at row zero.
            && !(target == 0 && super::fold::front_matter_fold_range(decorations, 0)
                .is_some_and(|range| range.end == 2))
        {
            target = if downward {
                target.checked_add(1)?
            } else {
                target.checked_sub(1)?
            };
            continue;
        }
        let plan = plans
            .iter()
            .find(|plan| plan.group.source_lines.contains(&target));
        if editable {
            if let Some(plan) = plan
                && !plan.source_visible
                && (plan.hides_line(target)
                    || cache_rows(target).is_none_or(|rows| rows == 0)
                    || (!plan.active
                        && target == plan.group.header_line
                        && matches!(plan.group.kind, super::front_matter_groups::GroupKind::List)))
            {
                if matches!(plan.group.kind, super::front_matter_groups::GroupKind::List) {
                    let item = plan
                        .group
                        .items
                        .iter()
                        .find(|item| item.source_line == target)
                        .or_else(|| {
                            if downward {
                                plan.group.items.first()
                            } else {
                                plan.group.items.last()
                            }
                        });
                    if let Some(item) = item {
                        return Some(FrontMatterVerticalTarget::Source(if downward {
                            item.source_range.start
                        } else {
                            item.source_range.end
                        }));
                    }
                }
                let value_line = if downward && plan.hides_line(target) {
                    plan.label_line()
                } else {
                    target
                };
                return Some(FrontMatterVerticalTarget::Source(
                    front_matter_value_endpoint(buffer, decorations, value_line, downward),
                ));
            }
        } else {
            let front_matter = decorations
                .get(target)
                .and_then(|d| d.table_info.as_ref())
                .is_some_and(|t| t.is_front_matter);
            if plan.is_some_and(|plan| plan.hides_line(target))
                || (front_matter && cache_rows(target) == Some(0))
            {
                target = if downward {
                    target.checked_add(1)?
                } else {
                    target.checked_sub(1)?
                };
                continue;
            }
        }
        if decorations
            .get(target)
            .and_then(|d| d.table_info.as_ref())
            .is_some_and(|table| table.is_front_matter)
            && cache_rows(target).is_none()
        {
            return Some(FrontMatterVerticalTarget::Source(
                front_matter_value_endpoint(buffer, decorations, target, downward),
            ));
        }
        return Some(FrontMatterVerticalTarget::Line(target));
    }
    None
}

impl EditorView {
    pub(crate) fn cursor_vertical_bounds(&self) -> Option<(f32, f32)> {
        if !self.active_tab_supports_text_buffer() || self.last_viewport_height <= 0.0 {
            return None;
        }
        if self.table_cursor_cell().is_some() {
            let geometry = self.table_cursor_paint_bounds?;
            if geometry.offset != self.cursor.offset {
                return None;
            }
            return Some(geometry.vertical_bounds());
        }
        let line = self.cursor_line();
        let (row_top, row_bottom) =
            self.visual_row_vertical_bounds_for_offset(self.cursor.offset)?;
        let text_top = self.text_y_offset_for_line(line);
        Some((text_top + row_top, text_top + row_bottom))
    }

    pub(crate) fn cursor_visible_in_viewport(&self) -> bool {
        let Some((top, bottom)) = self.cursor_vertical_bounds() else {
            return false;
        };
        caret_vertical_overflow(
            top,
            bottom,
            self.scroll_offset,
            self.scroll_offset + self.editor_visible_height(),
        ) == std::cmp::Ordering::Equal
    }

    pub(crate) fn keep_previously_visible_cursor_in_view(&mut self) -> bool {
        let Some((top, bottom)) = self.cursor_vertical_bounds() else {
            return false;
        };
        let correction = cursor_visibility_scroll_delta(
            top,
            bottom,
            self.scroll_offset,
            self.editor_visible_height(),
        );
        let before = self.scroll_offset;
        self.scroll_offset = (self.scroll_offset + correction).max(0.0);
        self.clamp_scroll();
        let changed = (self.scroll_offset - before).abs() > 0.5;
        if changed {
            self.table_cursor_paint_bounds = None;
        }
        changed
    }

    fn actual_visual_line_count(info: &LineDisplayInfo) -> usize {
        if info.visual_line_count == 0 {
            0
        } else {
            info.wrapped_line.wrap_boundaries().len() + 1
        }
    }

    pub(crate) fn set_visible_utility_panel(
        &mut self,
        next_panel: Option<RightUtilityPanel>,
        cx: &mut Context<Self>,
    ) {
        let previous_panel = self.utility_panel;
        self.utility_panel = next_panel;

        if next_panel == Some(RightUtilityPanel::Info) {
            self.invalidate_info_panel_cache();
        }
        // Switching directly between two visible utility panels is a panel-type
        // switch: apply the width change instantly. Only animate when a panel is
        // appearing (none -> some) or disappearing (some -> none).
        let is_type_switch =
            previous_panel.is_some() && next_panel.is_some() && previous_panel != next_panel;
        if is_type_switch {
            self.displayed_utility_panel = next_panel;
            self.set_utility_panel_width_immediately(self.utility_panel_target_width());
        } else {
            self.sync_utility_panel_width_animation(cx);
        }
        cx.notify();
    }

    pub(crate) fn scaled(&self, value: f32) -> f32 {
        value * self.ui_scale
    }

    /// Space the platform's own window controls take at the leading edge of the
    /// tab bar. Only macOS puts them there; elsewhere they sit on the right.
    pub(crate) fn window_controls_leading_inset(&self) -> f32 {
        if cfg!(target_os = "macos") {
            if self.window_is_fullscreen {
                // Fullscreen hides the traffic lights, so only keep a margin.
                self.scaled(crate::style::FULLSCREEN_LEADING_MARGIN_PX)
            } else {
                self.styles.tab_bar.tab_history_hidden_sidebar_margin_px
            }
        } else {
            0.0
        }
    }

    /// Width reserved at the trailing edge for the caption buttons the app
    /// draws itself.
    pub(crate) fn window_caption_trailing_inset(&self) -> f32 {
        #[cfg(target_os = "linux")]
        {
            if self.client_side_window_decorations {
                self.scaled(crate::style::LINUX_CAPTION_BUTTONS_WIDTH_PX)
            } else {
                0.0
            }
        }

        #[cfg(target_os = "macos")]
        {
            0.0
        }

        #[cfg(windows)]
        {
            self.scaled(crate::style::WINDOW_CAPTION_BUTTON_WIDTH_PX * 3.0)
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            0.0
        }
    }

    pub(crate) fn ui_border_width(&self, default_width: f32) -> f32 {
        default_width
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn traffic_light_position_for_ui_scale(ui_scale: f32) -> gpui::Point<gpui::Pixels> {
        const STANDARD_BUTTON_HEIGHT_PX: f32 = 14.0;
        traffic_light_offsets_for_layout(
            ui_scale,
            crate::style::TOP_BAR_HEIGHT_PX * ui_scale,
            STANDARD_BUTTON_HEIGHT_PX,
        )
    }

    /// Called every frame; fullscreen has no window-state event to observe.
    pub(crate) fn sync_window_fullscreen_state(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let fullscreen = window.is_fullscreen();
        if self.window_is_fullscreen == fullscreen {
            return;
        }
        self.window_is_fullscreen = fullscreen;
        #[cfg(target_os = "macos")]
        {
            let radius = if fullscreen {
                0.0
            } else {
                crate::WINDOW_CONTENT_CORNER_RADIUS
            };
            apply_macos_content_corner_radius(window, radius);
            // Leaving fullscreen restores AppKit's own button frames.
            self.traffic_light_ui_scale = None;
        }
        cx.notify();
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn sync_macos_traffic_light_position(&mut self, window: &mut Window) {
        let already_synced = self
            .traffic_light_ui_scale
            .is_some_and(|scale| (scale - self.ui_scale).abs() < f32::EPSILON);
        if already_synced
            && macos_traffic_light_position_matches(window, self.ui_scale, self.tab_bar_height())
        {
            return;
        }
        if apply_macos_traffic_light_position(window, self.ui_scale, self.tab_bar_height()) {
            // AppKit may perform one more titlebar layout after the first content frame.
            // Keep the cache empty once so the following frame confirms the final position.
            if self.traffic_light_ui_scale.is_none() {
                self.traffic_light_ui_scale = Some(f32::NAN);
            } else {
                self.traffic_light_ui_scale = Some(self.ui_scale);
            }
        }
    }

    pub(crate) fn base_font_size(&self) -> f32 {
        self.scaled(self.styles.editor.editor_font_size_px)
    }

    pub(crate) fn gutter_left_inset(&self) -> f32 {
        if self.editor_uses_code_gutter() {
            0.0
        } else {
            self.scaled(self.styles.editor.editor_padding_left_px)
        }
    }

    pub(crate) fn gutter_line_number_width(&self) -> f32 {
        if !self.editor_uses_code_gutter() {
            return 0.0;
        }
        let gutter_styles = &self.styles.gutter;
        let line_number_digits = self.buffer.line_count().max(1).to_string().len() as f32;
        let estimated_text_width =
            line_number_digits * self.scaled(gutter_styles.gutter_line_number_font_size_px) * 0.6;
        estimated_text_width.clamp(
            self.scaled(gutter_styles.gutter_line_number_min_width_px),
            self.scaled(gutter_styles.gutter_line_number_max_width_px),
        )
    }

    pub(crate) fn gutter_column_gap(&self) -> f32 {
        if self.editor_uses_code_gutter() {
            self.scaled(self.styles.gutter.gutter_column_gap_px)
        } else {
            0.0
        }
    }

    pub(crate) fn gutter_icon_column_width(&self) -> f32 {
        let gutter_styles = &self.styles.gutter;
        let max_heading_icon_size = gutter_styles
            .gutter_heading_h1_icon_size_px
            .max(gutter_styles.gutter_heading_h2_icon_size_px)
            .max(gutter_styles.gutter_heading_h3_icon_size_px)
            .max(gutter_styles.gutter_heading_h4_icon_size_px);
        self.scaled(
            (self.styles.self_drawn.gutter_fold_icon_size_px
                + self.styles.self_drawn.gutter_fold_icon_offset_px)
                .max(max_heading_icon_size + gutter_styles.gutter_heading_icon_offset_px),
        )
    }

    pub(crate) fn gutter_line_number_right_x(&self, editor_left: f32) -> f32 {
        editor_left + self.gutter_left_inset() + self.gutter_line_number_width()
    }

    pub(crate) fn gutter_icon_column_left(&self, editor_left: f32) -> f32 {
        self.gutter_line_number_right_x(editor_left) + self.gutter_column_gap()
    }

    pub(crate) fn gutter_icon_x(&self, editor_left: f32, icon_size: f32) -> f32 {
        self.gutter_icon_column_left(editor_left)
            + ((self.gutter_icon_column_width() - icon_size) / 2.0).max(0.0)
    }

    pub(crate) fn padding_left(&self) -> f32 {
        if self.editor_uses_code_gutter() {
            self.gutter_left_inset()
                + self.gutter_line_number_width()
                + self.gutter_column_gap()
                + self.gutter_icon_column_width()
                + self.gutter_column_gap()
        } else {
            self.scaled(self.styles.editor.editor_padding_left_px)
        }
    }

    pub(crate) fn padding_top(&self) -> f32 {
        let padding = if self.app_settings.simple_mode {
            self.styles.editor.editor_non_markdown_padding_top_px
        } else {
            self.styles.editor.editor_padding_top_px
        };
        self.scaled(padding)
    }

    pub(crate) fn padding_right(&self) -> f32 {
        self.scaled(self.styles.editor.editor_padding_right_px)
    }

    pub(crate) fn padding_bottom(&self) -> f32 {
        self.scaled(self.styles.editor.editor_padding_bottom_px)
    }

    pub(crate) fn max_editor_width(&self) -> f32 {
        if self.active_tab_is_non_markdown_text() {
            self.scaled(self.styles.editor.editor_non_markdown_max_width_px)
        } else {
            self.scaled(self.styles.editor.editor_max_width_px)
        }
    }

    pub(crate) fn editor_uses_expanded_layout(&self) -> bool {
        self.active_tab_is_non_markdown_text()
            || self.document_mode().left_aligned(self.editor_left_aligned)
    }

    pub(crate) fn editor_wraps_to_available_width(&self) -> bool {
        self.document_mode().word_wrap(
            self.source_word_wrap,
            self.active_tab_is_non_markdown_text(),
            self.editor_left_aligned,
        )
    }

    pub(crate) fn tab_bar_height(&self) -> f32 {
        self.scaled(self.styles.tab_bar.tab_bar_height_px)
    }

    pub(crate) fn font_size_for_block(&self, block_type: &BlockType) -> f32 {
        match block_type {
            BlockType::Heading(1) => self.scaled(self.styles.editor.editor_h1_font_size_px),
            BlockType::Heading(2) => self.scaled(self.styles.editor.editor_h2_font_size_px),
            BlockType::Heading(3) => self.scaled(self.styles.editor.editor_h3_font_size_px),
            BlockType::Heading(4) => self.scaled(self.styles.editor.editor_h4_font_size_px),
            BlockType::Heading(5) => self.scaled(self.styles.editor.editor_h5_font_size_px),
            BlockType::Heading(6) => self.scaled(self.styles.editor.editor_h6_font_size_px),
            _ => self.base_font_size(),
        }
    }

    pub(crate) fn line_height_for_block(&self, block_type: &BlockType) -> f32 {
        let factor = match block_type {
            BlockType::Heading(_) => self.styles.editor.editor_heading_line_height_factor,
            _ => self.styles.editor.editor_paragraph_line_height_factor,
        };
        self.font_size_for_block(block_type) * factor
    }

    fn is_non_empty_line(&self, line: usize) -> bool {
        if line >= self.buffer.line_count() {
            return false;
        }

        !self.buffer.line_content(line).trim().is_empty()
    }

    fn is_heading_line(&self, line: usize) -> bool {
        if !self.is_non_empty_line(line) {
            return false;
        }

        matches!(
            self.decorations.get(line).map(|d| &d.block_type),
            Some(BlockType::Heading(_))
        )
    }

    fn is_paragraph_style_line(&self, line: usize) -> bool {
        if !self.is_non_empty_line(line) {
            return false;
        }

        matches!(
            self.decorations.get(line).map(|d| &d.block_type),
            Some(block_type) if !matches!(block_type, BlockType::Heading(_))
        )
    }

    pub(crate) fn block_padding_for_line(&self, line: usize) -> (f32, f32) {
        let (base_top, base_bottom) = self.base_block_padding_for_line(line);
        let table_handle_padding = super::table_handles::top_padding(
            self.decorations.get(line),
            self.scaled(self.styles.self_drawn.table_border_track_width_px),
        );
        (base_top + table_handle_padding, base_bottom)
    }

    pub(crate) fn line_diff_preview_height_for_line(&self, line: usize, trailing: bool) -> f32 {
        let row_count = self.line_diff_preview_row_count(line, trailing);
        if row_count == 0 {
            return 0.0;
        }
        let line = line.min(self.decorations.len().saturating_sub(1));
        let line_height = self.decorations.get(line).map_or_else(
            || self.line_height_for_block(&BlockType::Normal),
            |decoration| self.line_height_for_block(&decoration.block_type),
        );
        row_count as f32 * line_height * self.line_diff_reveal_progress
    }

    fn base_block_padding_for_line(&self, line: usize) -> (f32, f32) {
        if line >= self.buffer.line_count() {
            return (0.0, 0.0);
        }

        let line_content = self.buffer.line_content(line);
        if line_content.trim().is_empty() {
            return (0.0, 0.0);
        }

        let Some(block_type) = self.decorations.get(line).map(|d| &d.block_type) else {
            return (0.0, 0.0);
        };

        if self.is_quote_line(line) {
            // Callout blocks (`> [!KIND]`) get extra inner vertical padding on
            // top of the plain quote padding; the callout kind is only tagged on
            // the group's first line.
            let group_start = self.quote_group_start(line);
            let is_callout = self
                .decorations
                .get(group_start)
                .is_some_and(|decoration| decoration.callout_kind.is_some());
            return quote_vertical_padding(
                &self.styles.quote_block,
                self.ui_scale,
                self.is_quote_group_start(line),
                self.is_quote_group_end(line),
                is_callout,
                self.folded_sections.contains(&line)
                    && self
                        .fold_range_for_line(line)
                        .is_some_and(|hidden| !self.is_quote_line(hidden.end)),
            );
        }

        match block_type {
            BlockType::Heading(_) if self.is_heading_line(line) => (
                self.scaled(self.styles.editor.editor_heading_padding_top_px),
                self.scaled(self.styles.editor.editor_heading_padding_bottom_px),
            ),
            _ if self.is_paragraph_style_line(line) => {
                let top = if line == 0 || self.is_heading_line(line - 1) {
                    self.scaled(self.styles.editor.editor_paragraph_padding_top_px)
                } else {
                    0.0
                };
                let bottom =
                    if line + 1 >= self.buffer.line_count() || self.is_heading_line(line + 1) {
                        self.scaled(self.styles.editor.editor_paragraph_padding_bottom_px)
                    } else {
                        0.0
                    };
                (top, bottom)
            }
            _ => (0.0, 0.0),
        }
    }

    pub(crate) fn is_quote_line(&self, line: usize) -> bool {
        self.decorations.get(line).is_some_and(|decoration| {
            decoration.block_type == BlockType::Blockquote
                || decoration
                    .block_contexts
                    .contains(&crate::markdown::BlockContext::Blockquote)
        })
    }

    pub(crate) fn is_quote_group_start(&self, line: usize) -> bool {
        self.is_quote_line(line) && (line == 0 || !self.is_quote_line(line - 1))
    }

    pub(crate) fn is_quote_group_end(&self, line: usize) -> bool {
        self.is_quote_line(line)
            && (line + 1 >= self.buffer.line_count() || !self.is_quote_line(line + 1))
    }

    pub(crate) fn quote_group_start(&self, line: usize) -> usize {
        let mut start = line;
        while start > 0 && self.is_quote_line(start - 1) {
            start -= 1;
        }
        start
    }

    fn current_visual_height_for_line(&self, line: usize) -> f32 {
        self.visual_height_with_front_matter_visibility(
            line,
            self.front_matter_presentation_line_is_hidden(line),
        )
    }

    fn visual_height_with_front_matter_visibility(&self, line: usize, hidden: bool) -> f32 {
        if hidden {
            return 0.0;
        }
        if let Some(Some(info)) = self.line_display_info.get(line)
            && (info.visual_line_count > 0
                || !self
                    .decorations
                    .get(line)
                    .and_then(|d| d.table_info.as_ref())
                    .is_some_and(|table| table.is_front_matter))
        {
            info.total_height()
                + self.line_diff_preview_height_for_line(line, false)
                + self.line_diff_preview_height_for_line(line, true)
        } else {
            if !self.folded_sections.is_empty()
                && self.folded_sections.iter().any(|&fold_line| {
                    self.fold_range_for_line(fold_line)
                        .is_some_and(|range| range.contains(&line))
                })
            {
                return 0.0;
            }

            let block_type = if line < self.decorations.len() {
                &self.decorations[line].block_type
            } else {
                &BlockType::Normal
            };
            let (padding_top, padding_bottom) = self.block_padding_for_line(line);
            // A reparse can drop a line's shaping info while its image preview
            // is still rendered; excluding the preview height here made every
            // scroll computation between reparse and the next prepaint short
            // by the image height, which jittered the view while typing near
            // image lines.
            self.line_height_for_block(block_type)
                + padding_top
                + padding_bottom
                + self.image_preview_extra_height_for_line(line)
                + self.line_diff_preview_height_for_line(line, false)
                + self.line_diff_preview_height_for_line(line, true)
        }
    }

    fn ensure_line_geometry_cache(&self) {
        let line_count = self.buffer.line_count();
        if self.uses_sparse_line_geometry() {
            let mut cache = self.line_geometry_cache.borrow_mut();
            cache.prefix_y_offsets.clear();
            let default_height = self.line_height_for_block(&BlockType::Normal) as f64;
            let dimensions_changed = cache.sparse.line_count != line_count
                || (cache.sparse.default_height - default_height).abs() > 0.01;
            cache.sparse.ensure_dimensions(line_count, default_height);
            if (dimensions_changed || self.line_geometry_cache_dirty.get())
                && let Some(table) = self
                    .decorations
                    .first()
                    .and_then(|d| d.table_info.as_ref())
                    .filter(|table| table.is_front_matter)
            {
                for line in table.table_start_line..table.table_end_line.min(line_count) {
                    let hidden = self.front_matter_presentation_line_is_hidden(line);
                    cache.sparse.set_measured_height(
                        line,
                        self.visual_height_with_front_matter_visibility(line, hidden) as f64,
                    );
                }
            }
            self.line_geometry_cache_dirty.set(false);
            return;
        }
        if !self.line_geometry_cache_dirty.get()
            && self.line_geometry_cache.borrow().prefix_y_offsets.len() == line_count + 1
        {
            return;
        }

        let mut cache = self.line_geometry_cache.borrow_mut();
        cache.prefix_y_offsets.clear();
        cache.prefix_y_offsets.reserve(line_count + 1);

        let mut y = self.padding_top();
        cache.prefix_y_offsets.push(y);
        for line in 0..line_count {
            let hidden = self.front_matter_presentation_line_is_hidden(line);
            y += self.visual_height_with_front_matter_visibility(line, hidden);
            cache.prefix_y_offsets.push(y);
        }

        self.line_geometry_cache_dirty.set(false);
    }

    pub(crate) fn visual_height_for_line(&self, line: usize) -> f32 {
        self.current_visual_height_for_line(line)
    }

    pub(crate) fn line_y_offset(&self, target_line: usize) -> f32 {
        self.line_y_offset_precise(target_line) as f32
    }

    pub(crate) fn line_y_offset_precise(&self, target_line: usize) -> f64 {
        if self.uses_sparse_line_geometry() {
            self.ensure_line_geometry_cache();
            return self
                .line_geometry_cache
                .borrow()
                .sparse
                .y_offset(target_line, self.padding_top() as f64);
        }
        self.ensure_line_geometry_cache();
        let target_line = target_line.min(self.buffer.line_count());
        self.line_geometry_cache.borrow().prefix_y_offsets[target_line] as f64
    }

    /// Extra height reserved above an image line's syntax text by its inline
    /// preview (image plus top/bottom gaps). The preview is painted above the
    /// syntax text, so this offset shifts the text row down within the line
    /// box. Keep in sync with the per-line layout in `element.rs`.
    pub(crate) fn image_preview_extra_height_for_line(&self, line: usize) -> f32 {
        if self.folded_image_previews.contains(&line) {
            return 0.0;
        }
        let Some(path) = self
            .decorations
            .get(line)
            .and_then(|decoration| decoration.image_path.as_ref())
        else {
            return 0.0;
        };
        let Some(Some(img_data)) = self.image_cache.get(path) else {
            return 0.0;
        };
        let img_size = img_data.size(0);
        let img_w = img_size.width.0 as f32;
        let img_h = img_size.height.0 as f32;
        if img_w <= 0.0 || img_h <= 0.0 {
            return 0.0;
        }
        let (_, display_h) = helpers::compute_image_preview_size(
            img_w,
            img_h,
            self.last_text_width,
            self.last_window_scale_factor,
        );
        display_h
            + self.scaled(self.styles.image_preview.image_gap_top_px)
            + self.scaled(self.styles.image_preview.image_gap_bottom_px)
    }

    pub(crate) fn text_y_offset_for_line(&self, line: usize) -> f32 {
        self.line_y_offset(line)
            + self.block_padding_for_line(line).0
            + self.image_preview_extra_height_for_line(line)
            + self.line_diff_preview_height_for_line(line, false)
    }

    pub(crate) fn total_content_height(&self) -> f32 {
        if self.uses_sparse_line_geometry() {
            self.ensure_line_geometry_cache();
            return (self
                .line_geometry_cache
                .borrow()
                .sparse
                .y_offset(self.buffer.line_count(), self.padding_top() as f64)
                + self.padding_bottom() as f64) as f32;
        }
        self.ensure_line_geometry_cache();
        self.line_geometry_cache.borrow().prefix_y_offsets[self.buffer.line_count()]
            + self.padding_bottom()
    }

    pub(crate) fn line_at_y(&self, y: f32) -> usize {
        let line_count = self.buffer.line_count();
        if line_count == 0 {
            return 0;
        }

        if self.uses_sparse_line_geometry() {
            self.ensure_line_geometry_cache();
            let line = self
                .line_geometry_cache
                .borrow()
                .sparse
                .line_at_y(y as f64, self.padding_top() as f64);
            return if self.front_matter_presentation_line_is_hidden(line) {
                (0..line)
                    .rev()
                    .find(|&line| self.visual_height_for_line(line) > 0.0)
                    .unwrap_or(0)
            } else {
                line
            };
        }

        self.ensure_line_geometry_cache();
        let cache = self.line_geometry_cache.borrow();
        let prefix = &cache.prefix_y_offsets;
        let target_y = y.max(prefix[0]);

        let mut low = 0usize;
        let mut high = line_count;
        while low < high {
            let mid = (low + high) / 2;
            if prefix[mid + 1] <= target_y {
                low = mid + 1;
            } else {
                high = mid;
            }
        }

        let line = low.min(line_count.saturating_sub(1));
        if self.front_matter_presentation_line_is_hidden(line) {
            (0..line)
                .rev()
                .find(|&line| self.visual_height_for_line(line) > 0.0)
                .unwrap_or(0)
        } else {
            line
        }
    }

    pub(crate) fn scroll_to_cursor(&mut self) {
        if self
            .pending_typing_scroll_guard
            .is_some_and(|guard| guard.cursor_offset == self.cursor.offset)
        {
            self.pending_scroll_to_cursor = false;
            return;
        }
        if self.layout_dirty {
            self.pending_scroll_to_cursor = true;
            return;
        }

        self.pending_scroll_to_cursor = false;
        // Keep visible preview source stable, but never strand a caret below the viewport.
        if self.focused_code_mode_preview_key().is_some()
            && preview_source_cursor_is_visible(
                self.cursor_vertical_bounds(),
                self.scroll_offset,
                self.editor_visible_height(),
            )
        {
            return;
        }
        if self.table_cursor_cell().is_some() {
            let Some((top, bottom)) = self.cursor_vertical_bounds() else {
                self.pending_scroll_to_cursor = true;
                return;
            };
            let correction = cursor_visibility_scroll_delta(
                top,
                bottom,
                self.scroll_offset,
                self.editor_visible_height(),
            );
            if correction != 0.0 {
                self.scroll_offset = (self.scroll_offset + correction).max(0.0);
                self.clamp_scroll();
                self.table_cursor_paint_bounds = None;
            }
            return;
        }
        let cursor_line = self.cursor_line();
        let (_, cursor_visual_x) = self
            .cursor_visual_position(self.cursor.offset)
            .unwrap_or((0, 0.0));
        if let Some(start_line) = self
            .code_block_group_start(cursor_line)
            .filter(|start_line| !self.code_block_word_wrap_enabled(*start_line))
        {
            let state = self.code_block_display_override(start_line);
            let base_content_x = self
                .line_display_info
                .get(cursor_line)
                .and_then(|info| info.as_ref())
                .map(|info| info.content_x + state.horizontal_offset)
                .unwrap_or(self.last_content_left);
            let viewport_right = self
                .line_display_info
                .get(cursor_line)
                .and_then(|info| info.as_ref())
                .and_then(|info| info.horizontal_clip_right_x)
                .unwrap_or(self.last_content_left + self.last_text_width);
            let margin = self.scaled(12.0);
            let cursor_width = self.styles.cursor.cursor_width_px.max(1.0);
            let cursor_screen_x = base_content_x + cursor_visual_x - state.horizontal_offset;
            let desired_offset = if cursor_screen_x < base_content_x {
                (cursor_visual_x - margin).max(0.0)
            } else if cursor_screen_x + cursor_width > viewport_right {
                (cursor_visual_x + cursor_width - (viewport_right - base_content_x) + margin)
                    .clamp(0.0, state.max_horizontal_offset)
            } else {
                state.horizontal_offset
            };
            if (desired_offset - state.horizontal_offset).abs() > 0.5 {
                if let Some(tab_id) = self.active_tab_id.as_ref()
                    && let Some(state) = self
                        .code_block_display_overrides
                        .get_mut(tab_id)
                        .and_then(|overrides| overrides.get_mut(&start_line))
                {
                    state.horizontal_offset = desired_offset;
                }
                self.layout_dirty = true;
            }
        } else if self.editor_uses_horizontal_text_scroll() {
            let state = self.non_markdown_horizontal_scroll_state();
            let base_content_x = self
                .line_display_info
                .get(cursor_line)
                .and_then(|info| info.as_ref())
                .map(|info| info.content_x + state.offset)
                .unwrap_or(self.last_content_left);
            let viewport_right = self
                .line_display_info
                .get(cursor_line)
                .and_then(|info| info.as_ref())
                .and_then(|info| info.horizontal_clip_right_x)
                .unwrap_or(self.last_content_left + self.last_text_width);
            let margin = self.scaled(12.0);
            let cursor_width = self.styles.cursor.cursor_width_px.max(1.0);
            let cursor_screen_x = base_content_x + cursor_visual_x - state.offset;
            let desired_offset = if cursor_screen_x < base_content_x {
                (cursor_visual_x - margin).max(0.0)
            } else if cursor_screen_x + cursor_width > viewport_right {
                (cursor_visual_x + cursor_width - (viewport_right - base_content_x) + margin)
                    .clamp(0.0, state.max_offset)
            } else {
                state.offset
            };
            if (desired_offset - state.offset).abs() > 0.5
                && self.set_code_block_horizontal_offset(
                    super::NON_MARKDOWN_HORIZONTAL_SCROLL_TARGET,
                    desired_offset,
                )
            {
                self.layout_dirty = true;
            }
        }
        let (row_top, row_bottom) = self
            .visual_row_vertical_bounds_for_offset(self.cursor.offset)
            .unwrap_or_else(|| {
                let block_type = self
                    .decorations
                    .get(cursor_line)
                    .map(|decoration| &decoration.block_type)
                    .unwrap_or(&BlockType::Normal);
                (0.0, self.line_height_for_block(block_type))
            });
        let cursor_y = self.text_y_offset_for_line(cursor_line) + row_top;
        let cursor_line_height = row_bottom - row_top;
        let viewport_height = self.editor_visible_height();
        let (viewport_top, viewport_bottom) =
            cursor_scroll_viewport(self.scroll_offset, viewport_height);
        let cursor_top = cursor_y;
        let cursor_bottom = cursor_y + cursor_line_height;
        if cursor_line_height > viewport_height {
            self.scroll_offset = (self.scroll_offset
                + cursor_visibility_scroll_delta(
                    cursor_top,
                    cursor_bottom,
                    self.scroll_offset,
                    viewport_height,
                ))
            .max(0.0);
            self.clamp_scroll();
            return;
        }

        match caret_vertical_overflow(cursor_top, cursor_bottom, viewport_top, viewport_bottom) {
            std::cmp::Ordering::Less => {
                let margin = self
                    .padding_top()
                    .min((viewport_height - cursor_line_height).max(0.0));
                self.scroll_offset = (cursor_y - margin).max(0.0);
                self.clamp_scroll();
                self.last_scroll_cursor_visual_x = Some(cursor_visual_x);
            }
            std::cmp::Ordering::Greater => {
                let margin = self
                    .padding_bottom()
                    .min((viewport_height - cursor_line_height).max(0.0));
                self.scroll_offset = cursor_y + cursor_line_height - viewport_height + margin;
                self.clamp_scroll();
                self.last_scroll_cursor_visual_x = Some(cursor_visual_x);
            }
            std::cmp::Ordering::Equal => {}
        }
    }

    pub(crate) fn capture_typing_scroll_guard(&mut self) {
        self.scroll_anim_epoch = self.scroll_anim_epoch.wrapping_add(1);
        let line = self.cursor_line();
        self.pending_click_anchor = None;
        self.pending_preview_reveal = None;
        self.document_mode_scroll_anchor = None;
        self.pending_center_cursor_in_view = false;
        self.pending_center_cursor_in_view_immediately = false;
        self.center_cursor_confirm_frames = 0;
        if self
            .pending_typing_scroll_guard
            .as_ref()
            .is_some_and(|guard| guard.line == self.cursor_line())
        {
            return;
        }
        self.pending_typing_scroll_guard = Some(PendingTypingScrollGuard {
            line,
            cursor_offset: self.cursor.offset,
            viewport_height: if self.last_viewport_height > 0.0 {
                self.editor_visible_height()
            } else {
                0.0
            },
            scroll_offset: self.scroll_offset,
        });
    }

    pub(crate) fn restore_scroll_if_typing_stayed_on_visual_line(
        &mut self,
        _applied_anchor_delta: f32,
    ) -> bool {
        let Some(guard) = self.pending_typing_scroll_guard else {
            return false;
        };
        if guard.cursor_offset == self.cursor.offset {
            self.pending_scroll_to_cursor = false;
            self.pending_center_cursor_in_view = false;
            self.scroll_offset = guard.scroll_offset;
            return true;
        }
        let line = self.cursor_line();
        let (row_top, row_bottom) = self
            .visual_row_vertical_bounds_for_offset(self.cursor.offset)
            .unwrap_or_else(|| (0.0, self.line_height_for_block(&BlockType::Normal)));
        let text_top = self.text_y_offset_for_line(line);
        let (cursor_top, cursor_bottom) = if self.table_cursor_cell().is_some() {
            let Some(bounds) = self.cursor_vertical_bounds() else {
                return false;
            };
            bounds
        } else {
            (text_top + row_top, text_top + row_bottom)
        };
        let restored_offset = edit_scroll_target(
            guard.scroll_offset,
            guard.viewport_height,
            cursor_top,
            cursor_bottom,
        );

        self.pending_scroll_to_cursor = false;
        self.pending_center_cursor_in_view = false;
        if (self.scroll_offset - restored_offset).abs() > 0.5 {
            self.table_cursor_paint_bounds = None;
        }
        self.scroll_offset = restored_offset;
        self.scroll_to_cursor();
        true
    }

    fn start_cursor_center_navigation(
        &mut self,
        target_scroll: f32,
        animate: bool,
        cx: &mut Context<Self>,
    ) {
        let start_scroll = self.scroll_offset;
        let animate =
            animate && self.animations_enabled() && (target_scroll - start_scroll).abs() >= 1.0;
        let animation_target = target_scroll;
        if !animate {
            self.scroll_offset = target_scroll;
            self.clamp_scroll();
            self.table_cursor_paint_bounds = None;
        }
        let epoch = self.scroll_anim_epoch;
        let pane_id = self.current_pane_id();
        let tab_id = self.active_tab_id.clone();
        let cursor_offset = self.cursor.offset;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let animation_steps = if animate { 14u32 } else { 0 };
            let mut confirmation = CursorCenterConfirmation::default();
            // Visible wrapping and asynchronous previews can settle after the
            // animation. Bound the confirmation just like heading navigation.
            for step in 1..=animation_steps + 60 {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
                let keep = this
                    .update(cx, |this, cx| {
                        if this.scroll_anim_epoch != epoch {
                            return false;
                        }
                        let mut scroll_changed = false;
                        let keep = this.with_pane_if_alive(pane_id, |editor| {
                            if editor.active_tab_id != tab_id
                                || editor.cursor.offset != cursor_offset
                            {
                                return false;
                            }
                            if step <= animation_steps && editor.animations_enabled() {
                                let t = step as f32 / animation_steps as f32;
                                let progress = 1.0 - (1.0 - t).powi(3);
                                let next_scroll =
                                    start_scroll + (animation_target - start_scroll) * progress;
                                if (editor.scroll_offset - next_scroll).abs() > 0.01 {
                                    editor.scroll_offset = next_scroll;
                                    editor.clamp_scroll();
                                    editor.table_cursor_paint_bounds = None;
                                    scroll_changed = true;
                                }
                                true
                            } else {
                                // Keep the bootstrapped viewport steady while
                                // windowed layout and visible previews settle.
                                let measured = (!editor.layout_dirty)
                                    .then(|| editor.cursor_center_scroll_target())
                                    .flatten();
                                if let Some(settled_target) = settled_cursor_center_target(
                                    &mut confirmation,
                                    measured,
                                    editor.cursor_center_previews_pending(),
                                ) {
                                    if (editor.scroll_offset - settled_target).abs() > 0.5 {
                                        editor.scroll_offset = settled_target;
                                        editor.clamp_scroll();
                                        editor.table_cursor_paint_bounds = None;
                                        scroll_changed = true;
                                    }
                                    false
                                } else {
                                    true
                                }
                            }
                        });
                        let keep = keep.unwrap_or(false);
                        if keep || scroll_changed {
                            cx.notify();
                        }
                        keep
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
            let _ = this.update(cx, |this, _| {
                if this.outline_navigation_epoch == Some(epoch) {
                    this.outline_navigation_epoch = None;
                }
            });
        })
        .detach();
    }

    fn prepare_cursor_center_navigation(&mut self) {
        // Even an unmeasured or subpixel navigation supersedes an older animation
        // and any scroll restoration captured before this explicit request.
        // This existing epoch is only a scroll-navigation guard: sharing it
        // with heading jumps also keeps preview refreshes from cancelling center.
        begin_cursor_center_epoch(
            &mut self.scroll_anim_epoch,
            &mut self.outline_navigation_epoch,
        );
        self.fold_scroll_guard = None;
        self.pending_preview_reveal = None;
        self.pending_click_anchor = None;
        self.pending_typing_scroll_guard = None;
        self.pending_scroll_to_cursor = false;
        self.pending_center_cursor_in_view = false;
        self.pending_center_cursor_in_view_immediately = false;
        self.center_cursor_confirm_frames = 0;
    }

    fn cursor_center_layout_scroll_target(&self) -> f32 {
        let cursor_line = self.cursor_line();
        // Rendered tables are shaped at their header, not at a hidden data row.
        let line = self
            .decorations
            .get(cursor_line)
            .and_then(|decoration| decoration.table_info.as_ref())
            .filter(|table| !table.is_front_matter)
            .map_or(cursor_line, |table| table.table_start_line);
        let block = self
            .decorations
            .get(line)
            .map_or(&BlockType::Normal, |decoration| &decoration.block_type);
        let measured = (!self.layout_dirty)
            .then(|| self.cursor_vertical_bounds())
            .flatten();
        cursor_center_layout_target(
            measured,
            self.text_y_offset_for_line(line),
            self.line_height_for_block(block),
            self.editor_visible_height(),
        )
    }

    fn cursor_center_previews_pending(&self) -> bool {
        if self.generated_preview_rerender_pending
            || self.generated_preview_rerender_task.is_some()
            || self.mermaid_previews_in_flight()
        {
            return true;
        }
        let (start, end) = self.generated_preview_priority_line_range();
        self.decorations
            .get(start..=end)
            .is_some_and(|decorations| {
                decorations.iter().enumerate().any(|(index, decoration)| {
                    decoration.block_preview.as_ref().is_some_and(|preview| {
                        !self.image_cache.contains_key(&preview.cache_key)
                            || !self
                                .generated_preview_svg_cache
                                .contains_key(&preview.cache_key)
                            || self.mermaid_preview_needs_refresh(&preview.cache_key)
                    }) || decoration.inline_math_spans.iter().any(|range| {
                        self.buffer
                            .line_content(start + index)
                            .get(range.clone())
                            .is_some_and(|source| {
                                let key = helpers::inline_math_cache_key(source);
                                !self.image_cache.contains_key(&key)
                                    || !self.generated_preview_svg_cache.contains_key(&key)
                            })
                    })
                })
            })
    }

    pub(crate) fn center_cursor_in_view(&mut self, cx: &mut Context<Self>) {
        if self.embedded_editor && self.embedded_read_only {
            self.embedded_stick_to_bottom = false;
        }
        self.prepare_cursor_center_navigation();
        let measured = (!self.layout_dirty)
            .then(|| self.cursor_center_scroll_target())
            .flatten();
        // Windowed prepaint cannot measure an offscreen target until its
        // estimated source position enters the viewport. Bootstrap immediately
        // instead of waiting on a cache that the old viewport can never fill.
        let target = self.cursor_center_layout_scroll_target();
        self.start_cursor_center_navigation(target, measured.is_some(), cx);
    }

    pub(crate) fn center_cursor_in_view_immediately(&mut self, cx: &mut Context<Self>) {
        if self.embedded_editor && self.embedded_read_only {
            self.embedded_stick_to_bottom = false;
        }
        self.prepare_cursor_center_navigation();
        let target = self.cursor_center_layout_scroll_target();
        self.start_cursor_center_navigation(target, false, cx);
    }

    pub(crate) fn center_cursor_in_view_once(&mut self, cx: &mut Context<Self>) {
        if self.embedded_editor && self.embedded_read_only {
            self.embedded_stick_to_bottom = false;
        }
        self.prepare_cursor_center_navigation();
        self.outline_navigation_epoch = None;
        let target = self.cursor_center_layout_scroll_target();
        self.scroll_offset = target;
        self.clamp_scroll();
        // Reconcile the measured target within the next prepaint transaction,
        // so its estimated position is never published as an intermediate frame.
        self.center_cursor_confirm_frames = 1;
        self.table_cursor_paint_bounds = None;
        cx.notify();
    }

    pub(crate) fn cursor_center_scroll_target(&self) -> Option<f32> {
        let (top, bottom) = self.cursor_vertical_bounds()?;
        Some(centered_row_scroll_target(
            top,
            bottom,
            self.editor_visible_height(),
        ))
    }

    pub(crate) fn max_scroll_offset(&self) -> f32 {
        if self.embedded_editor {
            // Embedded read-only editors pin the scroll
            // range to the exact content height: no scroll-beyond-last-line,
            // so no blank gap can appear below the content. Prefer the
            // layout-accurate painted height over mid-frame estimates.
            (self
                .total_content_height()
                .max(self.embedded_content_height.get())
                - self.last_viewport_height)
                .max(0.0)
        } else {
            // No scrolling (and no scrollbar) when the whole document
            // already fits inside the viewport.
            let viewport_height = self.editor_visible_height();
            if self.total_content_height() <= viewport_height {
                return 0.0;
            }
            // Scroll beyond the final soft row, not just the final source line.
            let last_line = self.buffer.line_count().saturating_sub(1);
            let last_row_top = self
                .line_display_info
                .get(last_line)
                .and_then(|info| info.as_ref())
                .filter(|info| info.visual_line_count > 0)
                .map(|info| {
                    self.text_y_offset_for_line(last_line)
                        + f32::from(info.wrapped_line.row_top(
                            info.wrapped_line.wrap_boundaries().len(),
                            gpui::px(info.line_height),
                        ))
                })
                .unwrap_or_else(|| self.line_y_offset(last_line));
            last_soft_row_scroll_limit(
                self.total_content_height() - self.padding_bottom(),
                last_row_top,
                viewport_height,
            )
        }
    }

    pub(crate) fn clamp_scroll(&mut self) {
        self.scroll_offset = self
            .fold_scroll_guard
            .unwrap_or_else(|| self.scroll_offset.clamp(0.0, self.max_scroll_offset()));
    }

    // ── Visual wrap-line cursor movement helpers ─────────────────────

    pub(crate) fn disp_to_buffer_offset(
        &self,
        info: &LineDisplayInfo,
        line: usize,
        disp_idx: usize,
    ) -> usize {
        let mut dc = disp_idx.min(info.display_text.len());
        while dc > 0 && !info.display_text.is_char_boundary(dc) {
            dc -= 1;
        }
        let buffer_col = if dc < info.display_to_buffer.len() {
            info.display_to_buffer[dc]
        } else if let Some(&last) = info.display_to_buffer.last() {
            last + 1
        } else {
            0
        };
        self.buffer.line_start(line) + buffer_col
    }

    fn front_matter_navigation_origin(
        &self,
        offset: usize,
    ) -> (usize, usize, Vec<helpers::FrontMatterGroupPlan>) {
        let (line, _) = self.buffer.offset_to_line_col(offset);
        let plans = self.front_matter_group_plan();
        let visible = if self.front_matter_navigation_skips_line(line) {
            let end = self.decorations[line]
                .table_info
                .as_ref()
                .unwrap()
                .table_end_line;
            if line == 0 {
                (1..end - 1).find(|&row| !self.front_matter_presentation_line_is_hidden(row))
            } else {
                (1..end - 1)
                    .rev()
                    .find(|&row| !self.front_matter_presentation_line_is_hidden(row))
            }
            .unwrap_or(line)
        } else {
            front_matter_visible_navigation_line(
                &plans,
                line,
                self.markdown_syntax_expansion_allowed(),
            )
        };
        if visible != line {
            return (
                visible,
                front_matter_value_endpoint(&self.buffer, &self.decorations, visible, true),
                plans,
            );
        }
        (line, offset, plans)
    }

    fn front_matter_vertical_neighbor(
        &self,
        plans: &[helpers::FrontMatterGroupPlan],
        line: usize,
        downward: bool,
    ) -> Option<FrontMatterVerticalTarget> {
        if let Some(range) = self.collapsed_front_matter_range() {
            if downward && line < range.end && line + 1 >= range.start {
                return (range.end < self.buffer.line_count()).then_some(
                    FrontMatterVerticalTarget::Source(self.buffer.line_start(range.end)),
                );
            }
            if !downward && line > 0 && range.contains(&(line - 1)) {
                return Some(FrontMatterVerticalTarget::Source(
                    self.buffer.line_end(range.start - 1),
                ));
            }
        }
        front_matter_vertical_neighbor(
            &self.buffer,
            &self.decorations,
            plans,
            line,
            downward,
            self.markdown_syntax_expansion_allowed(),
            self.front_matter_delimiters_are_visible(),
            |line| {
                self.line_display_info
                    .get(line)
                    .and_then(|info| info.as_ref())
                    .map(|info| info.visual_line_count)
            },
        )
    }

    fn front_matter_unmeasured_vertical_offset(
        &self,
        plans: &[helpers::FrontMatterGroupPlan],
        line: usize,
        downward: bool,
    ) -> Option<usize> {
        self.front_matter_vertical_neighbor(plans, line, downward)
            .map(|target| match target {
                FrontMatterVerticalTarget::Source(offset) => offset,
                FrontMatterVerticalTarget::Line(line) => {
                    front_matter_value_endpoint(&self.buffer, &self.decorations, line, downward)
                }
            })
    }

    pub(crate) fn offset_for_visual_up(&self, current_offset: usize) -> Option<usize> {
        if let Some(range) = self.collapsed_front_matter_range()
            && self.buffer.offset_to_line_col(current_offset).0 < range.end
        {
            return Some(self.buffer.line_start(range.start - 1));
        }
        let (line, current_offset, plans) = self.front_matter_navigation_origin(current_offset);
        let info = self
            .line_display_info
            .get(line)
            .and_then(|info| info.as_ref())
            .filter(|info| info.visual_line_count > 0);
        if info.is_none()
            && self
                .decorations
                .get(line)
                .and_then(|d| d.table_info.as_ref())
                .is_some_and(|table| table.is_front_matter)
        {
            return Some(
                self.front_matter_unmeasured_vertical_offset(&plans, line, false)
                    .unwrap_or(current_offset),
            );
        }
        let info = info?;
        let (cur_visual, current_x) = self.cursor_visual_position(current_offset)?;
        let goal_x = self.vertical_move_goal_x.unwrap_or(current_x);

        if cur_visual > 0 {
            let target_visual = cur_visual - 1;
            let target_y = info
                .wrapped_line
                .row_top(target_visual, gpui::px(info.line_height))
                + info
                    .wrapped_line
                    .row_height(target_visual, gpui::px(info.line_height))
                    * 0.5;
            let target_indent = if target_visual > 0 {
                info.indent_px
            } else {
                0.0
            };
            let target_x = (goal_x - target_indent).max(0.0);
            let new_disp = if target_x <= 0.5 {
                Self::wrap_boundary_display_index_for_visual_line(info, target_visual)
                    .unwrap_or_default()
            } else {
                match info.wrapped_line.closest_index_for_position(
                    gpui::point(gpui::px(target_x), target_y),
                    gpui::px(info.line_height),
                ) {
                    Ok(idx) | Err(idx) => idx,
                }
            };
            Some(self.disp_to_buffer_offset(info, line, new_disp))
        } else if line > 0 {
            let Some(previous) = self.front_matter_vertical_neighbor(&plans, line, false) else {
                return Some(current_offset);
            };
            let previous_line = match previous {
                FrontMatterVerticalTarget::Source(offset) => return Some(offset),
                FrontMatterVerticalTarget::Line(line) => line,
            };
            let prev_info = self
                .line_display_info
                .get(previous_line)?
                .as_ref()?
                .as_ref();
            let last_visual = Self::actual_visual_line_count(prev_info).saturating_sub(1);
            let target_y = prev_info
                .wrapped_line
                .row_top(last_visual, gpui::px(prev_info.line_height))
                + prev_info
                    .wrapped_line
                    .row_height(last_visual, gpui::px(prev_info.line_height))
                    * 0.5;
            let target_indent = if last_visual > 0 {
                prev_info.indent_px
            } else {
                0.0
            };
            let target_x = (goal_x - target_indent).max(0.0);
            let new_disp = if target_x <= 0.5 {
                Self::wrap_boundary_display_index_for_visual_line(prev_info, last_visual)
                    .unwrap_or_default()
            } else {
                match prev_info.wrapped_line.closest_index_for_position(
                    gpui::point(gpui::px(target_x), target_y),
                    gpui::px(prev_info.line_height),
                ) {
                    Ok(idx) | Err(idx) => idx,
                }
            };
            Some(self.disp_to_buffer_offset(prev_info, previous_line, new_disp))
        } else {
            Some(0)
        }
    }

    pub(crate) fn offset_for_visual_down(&self, current_offset: usize) -> Option<usize> {
        if let Some(range) = self.collapsed_front_matter_range()
            && self.buffer.offset_to_line_col(current_offset).0 < range.end
        {
            return Some(if range.end < self.buffer.line_count() {
                self.buffer.line_start(range.end)
            } else {
                self.buffer.line_end(range.start - 1)
            });
        }
        let (line, current_offset, plans) = self.front_matter_navigation_origin(current_offset);
        let info = self
            .line_display_info
            .get(line)
            .and_then(|info| info.as_ref())
            .filter(|info| info.visual_line_count > 0);
        if info.is_none()
            && self
                .decorations
                .get(line)
                .and_then(|d| d.table_info.as_ref())
                .is_some_and(|table| table.is_front_matter)
        {
            return Some(
                self.front_matter_unmeasured_vertical_offset(&plans, line, true)
                    .unwrap_or(current_offset),
            );
        }
        let info = info?;
        let (cur_visual, current_x) = self.cursor_visual_position(current_offset)?;
        let goal_x = self.vertical_move_goal_x.unwrap_or(current_x);

        if cur_visual + 1 < Self::actual_visual_line_count(info) {
            let target_visual = cur_visual + 1;
            let target_y = info
                .wrapped_line
                .row_top(target_visual, gpui::px(info.line_height))
                + info
                    .wrapped_line
                    .row_height(target_visual, gpui::px(info.line_height))
                    * 0.5;
            let target_indent = if target_visual > 0 {
                info.indent_px
            } else {
                0.0
            };
            let target_x = (goal_x - target_indent).max(0.0);
            let new_disp = if target_x <= 0.5 {
                Self::wrap_boundary_display_index_for_visual_line(info, target_visual)
                    .unwrap_or_default()
            } else {
                match info.wrapped_line.closest_index_for_position(
                    gpui::point(gpui::px(target_x), target_y),
                    gpui::px(info.line_height),
                ) {
                    Ok(idx) | Err(idx) => idx,
                }
            };
            Some(self.disp_to_buffer_offset(info, line, new_disp))
        } else if line + 1 < self.buffer.line_count() {
            let next = self.front_matter_vertical_neighbor(&plans, line, true)?;
            let next_line = match next {
                FrontMatterVerticalTarget::Source(offset) => return Some(offset),
                FrontMatterVerticalTarget::Line(line) => line,
            };
            let next_info = self.line_display_info.get(next_line)?.as_ref()?.as_ref();
            let target_y = next_info
                .wrapped_line
                .row_top(0, gpui::px(next_info.line_height))
                + next_info
                    .wrapped_line
                    .row_height(0, gpui::px(next_info.line_height))
                    * 0.5;
            let target_x = goal_x.max(0.0);
            let new_disp = if target_x <= 0.5 {
                0
            } else {
                match next_info.wrapped_line.closest_index_for_position(
                    gpui::point(gpui::px(target_x), target_y),
                    gpui::px(next_info.line_height),
                ) {
                    Ok(idx) | Err(idx) => idx,
                }
            };
            Some(self.disp_to_buffer_offset(next_info, next_line, new_disp))
        } else {
            Some(self.buffer.len())
        }
    }

    pub(crate) fn toggle_sidebar_visibility(&mut self, cx: &mut Context<Self>) {
        if self.workspaces.is_empty() {
            self.sidebar_visible = false;
            self.sidebar_resizing = false;
            self.sync_sidebar_width_animation(cx);
            cx.notify();
            return;
        }
        self.sidebar_visible = !self.sidebar_visible;
        if !self.sidebar_visible {
            self.sidebar_resizing = false;
        }
        self.sync_sidebar_width_animation(cx);
        cx.notify();
    }

    pub(crate) fn toggle_editor_alignment(&mut self, cx: &mut Context<Self>) {
        if self.source_mode {
            let mut anchor = self.capture_document_mode_scroll_anchor();
            self.cancel_outline_navigation();
            self.pending_click_anchor = None;
            self.pending_typing_scroll_guard = None;
            self.pending_scroll_to_cursor = false;
            self.pending_center_cursor_in_view = false;
            self.pending_center_cursor_in_view_immediately = false;
            self.center_cursor_confirm_frames = 0;
            self.source_word_wrap = !self.source_word_wrap;
            if let Some(tab_id) = self.active_tab_id.as_ref() {
                self.non_markdown_horizontal_scroll_states.remove(tab_id);
            }
            self.clear_active_tab_layout_cache();
            if let Some(anchor) = anchor.as_mut() {
                anchor.epoch = self.scroll_anim_epoch;
            }
            self.document_mode_scroll_anchor = anchor;
            self.restore_document_mode_scroll_anchor(false);
            self.persist_active_tab_state();
            session_store::persist_session(self);
            self.sync_code_block_horizontal_scrollbar(cx);
            cx.notify();
            return;
        }
        self.editor_left_aligned = !self.editor_left_aligned;
        if !self.editor_left_aligned
            && let Some(tab_id) = self.active_tab_id.as_ref()
        {
            self.non_markdown_horizontal_scroll_states.remove(tab_id);
        }
        self.layout_dirty = true;
        if let Some(index) = self.active_tab_index() {
            self.tabs[index].editor_left_aligned = self.editor_left_aligned;
            session_store::persist_session(self);
        }
        cx.notify();
    }

    pub(crate) fn document_mode(&self) -> DocumentMode {
        DocumentMode::from_flags(self.reading_mode, self.source_mode)
    }

    pub(crate) fn cycle_document_mode(&mut self, cx: &mut Context<Self>) {
        self.set_document_mode(self.document_mode().next(), cx);
    }

    pub(crate) fn cycle_document_mode_backwards(&mut self, cx: &mut Context<Self>) {
        self.set_document_mode(self.document_mode().previous(), cx);
    }

    pub(crate) fn set_document_mode(&mut self, mode: DocumentMode, cx: &mut Context<Self>) {
        self.minimap_state = Default::default();
        self.fold_scroll_guard = None;
        if self.document_mode() == mode || !self.active_tab_supports_text_buffer() {
            return;
        }
        self.finish_ime_composition();
        self.commit_history_transaction();
        let mut scroll_anchor = self.capture_document_mode_scroll_anchor();
        self.cancel_outline_navigation();
        self.pending_click_anchor = None;
        self.pending_typing_scroll_guard = None;
        self.pending_scroll_to_cursor = false;
        self.pending_center_cursor_in_view = false;
        self.pending_center_cursor_in_view_immediately = false;
        self.center_cursor_confirm_frames = 0;
        self.reading_mode = mode == DocumentMode::Reading;
        self.source_mode = mode == DocumentMode::Source;
        // Keep the saved alignment preference untouched. Source presentation
        // forces left alignment independently and releases it on exit.
        if let Some(index) = self.active_tab_index() {
            self.tabs[index].reading_mode = self.reading_mode;
            self.tabs[index].source_mode = self.source_mode;
        }
        self.reset_table_handles();
        self.hovering_table_start = None;
        self.table_border_segment_areas.clear();
        self.table_boundary_caret = None;
        self.table_boundary_drag_origin = None;
        self.table_source = None;
        self.pending_code_fence = None;
        self.code_mode_preview_key = None;
        self.pending_preview_reveal = None;
        self.close_preview_overlay();
        self.cancel_line_drag(cx);
        self.dismiss_line_format_toolbar(cx);
        self.dismiss_slash_menu(cx);
        self.wiki_link_start = None;
        self.clear_marked_ranges();
        self.pending_line_display_transfer = None;
        self.clear_active_tab_layout_cache();
        self.reparse();
        self.table_cursor_paint_bounds = None;
        if let Some(anchor) = scroll_anchor.as_mut() {
            anchor.epoch = self.scroll_anim_epoch;
        }
        self.document_mode_scroll_anchor = scroll_anchor;
        // Bootstrap at the new estimated source position before prepaint; exact
        // soft-row/table geometry is reconciled after shaping, without centering.
        self.restore_document_mode_scroll_anchor(false);
        self.close_context_menu_immediately(cx);
        self.persist_active_tab_state();
        session_store::persist_session(self);
        cx.notify();
    }

    pub(crate) fn document_mode_scroll_anchor_is_current(&self) -> bool {
        self.document_mode_scroll_anchor
            .as_ref()
            .is_some_and(|anchor| {
                anchor.is_current(
                    self.active_tab_id.as_deref(),
                    self.cursor,
                    self.scroll_anim_epoch,
                )
            })
    }

    fn document_mode_anchor_content_y(&self, offset: usize) -> (f32, bool) {
        let line = self
            .buffer
            .offset_to_line_col(offset.min(self.buffer.len()))
            .0;
        let decoration = self.decorations.get(line);
        if let Some(preview) = decoration.and_then(|row| row.block_preview.as_ref())
            && self.focused_code_mode_preview_key().as_deref() != Some(preview.cache_key.as_str())
        {
            // A hidden source row is represented by the rendered block. Its top
            // is the closest stable visual counterpart when entering Reading.
            let start = preview.start_line;
            let measured = !self.layout_dirty
                && self
                    .line_display_info
                    .get(start)
                    .is_some_and(Option::is_some);
            return (
                self.line_y_offset(start) + self.block_padding_for_line(start).0,
                measured,
            );
        }
        if offset == self.cursor.offset
            && !self.layout_dirty
            && let Some((top, _)) = self.cursor_vertical_bounds()
        {
            return (top, true);
        }
        if let Some(table) = decoration.and_then(|row| row.table_info.as_ref()) {
            if !self.layout_dirty
                && let Some((table_start, source_line, column)) = self.table_cell_for_offset(offset)
                && let Some(row) = self.table_visual_row_for_source_line(table_start, source_line)
                && let Some((bounds, ..)) = self
                    .table_click_areas
                    .iter()
                    .find(|(_, t, r, c)| *t == table_start && *r == row && *c == column)
            {
                let padding_y = self.scaled(self.styles.table.table_cell_padding_px) * 0.6;
                return (
                    f32::from(bounds.origin.y - self.content_origin.y)
                        + self.scroll_offset
                        + padding_y,
                    true,
                );
            }
            return (self.text_y_offset_for_line(table.table_start_line), false);
        }
        let row = (!self.layout_dirty)
            .then(|| self.visual_row_vertical_bounds_for_offset(offset))
            .flatten();
        (
            self.text_y_offset_for_line(line) + row.map_or(0.0, |(top, _)| top),
            row.is_some(),
        )
    }

    fn capture_document_mode_scroll_anchor(&self) -> Option<DocumentModeScrollAnchor> {
        if self.last_viewport_height <= 0.0 {
            return None;
        }
        let tab_id = self.active_tab_id.clone()?;
        let existing = self
            .document_mode_scroll_anchor
            .as_ref()
            .filter(|_| self.document_mode_scroll_anchor_is_current());
        let (offset, screen_y) = if let Some(anchor) = existing {
            // Rapid repeated clicks must retain the original screen position,
            // not a transient estimate from the previous mode's first frame.
            (anchor.offset, anchor.screen_y)
        } else {
            let (content_y, _) = self.document_mode_anchor_content_y(self.cursor.offset);
            let screen_y = content_y - self.scroll_offset;
            if (0.0..self.editor_visible_height()).contains(&screen_y) {
                (self.cursor.offset, screen_y)
            } else {
                // A caret scrolled out of view must not yank the document back.
                self.document_mode_viewport_anchor()
            }
        };
        Some(DocumentModeScrollAnchor {
            tab_id,
            cursor: self.cursor,
            offset,
            screen_y,
            epoch: self.scroll_anim_epoch,
            remaining_frames: 8,
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(3),
            confirmation: CursorCenterConfirmation::default(),
        })
    }

    fn document_mode_viewport_anchor(&self) -> (usize, f32) {
        let viewport_y = self.content_origin.y;
        if !self.layout_dirty
            && let Some((bounds, table, row, column)) = self
                .table_click_areas
                .iter()
                .find(|(bounds, ..)| bounds.origin.y <= viewport_y && bounds.bottom() > viewport_y)
            && let Some(line) = self.table_source_line_for_visual_row(*table, *row)
            && let Some(cell) = self
                .decorations
                .get(line)
                .and_then(|row| row.table_info.as_ref())
                .and_then(|table| table.cell_ranges.get(*column))
        {
            let padding_y = self.scaled(self.styles.table.table_cell_padding_px) * 0.6;
            return (
                self.buffer.line_start(line) + cell.start,
                f32::from(bounds.origin.y - viewport_y) + padding_y,
            );
        }

        let line = self.line_at_y(self.scroll_offset);
        let text_y = self.text_y_offset_for_line(line);
        if !self.layout_dirty
            && let Some(Some(info)) = self.line_display_info.get(line)
            && info.visual_line_count > 0
        {
            let local_y = (self.scroll_offset - text_y).max(0.0);
            let row = info
                .wrapped_line
                .row_for_y(gpui::px(local_y), gpui::px(info.line_height));
            let top = info.wrapped_line.row_top(row, gpui::px(info.line_height));
            let height = info
                .wrapped_line
                .row_height(row, gpui::px(info.line_height));
            let index = info
                .wrapped_line
                .closest_index_for_position(
                    gpui::point(gpui::px(0.0), top + height * 0.5),
                    gpui::px(info.line_height),
                )
                .unwrap_or_else(|index| index);
            return (
                self.disp_to_buffer_offset(info, line, index),
                text_y + f32::from(top) - self.scroll_offset,
            );
        }
        let offset = self.buffer.line_start(line);
        let (content_y, _) = self.document_mode_anchor_content_y(offset);
        (offset, content_y - self.scroll_offset)
    }

    pub(super) fn capture_width_reflow_scroll_anchor(&mut self, viewport_width: f32) {
        let editor_width = if self.editor_uses_expanded_layout() {
            viewport_width
        } else {
            viewport_width.min(self.max_editor_width())
        };
        let text_width =
            (editor_width - self.padding_left() - self.padding_right()).max(self.scaled(120.0));
        if self.last_text_width <= 0.0
            || (self.last_text_width - text_width).abs() < 0.5
            || self.last_viewport_height <= 0.0
            || self.document_mode_scroll_anchor_is_current()
            || self.pending_scroll_to_cursor
            || self.pending_center_cursor_in_view
            || self.center_cursor_confirm_frames > 0
            || self.pending_click_anchor.is_some()
            || self.pending_typing_scroll_guard.is_some()
            || self.fold_scroll_guard.is_some()
            || self.editor_scrollbar_drag.is_some()
            || self.minimap_state.is_dragging()
            || super::outline::outline_navigation_in_progress(
                self.outline_navigation_epoch,
                self.scroll_anim_epoch,
            )
        {
            return;
        }
        let Some(tab_id) = self.active_tab_id.clone() else {
            return;
        };
        let (offset, screen_y) = self.document_mode_viewport_anchor();
        self.document_mode_scroll_anchor = Some(DocumentModeScrollAnchor {
            tab_id,
            cursor: self.cursor,
            offset,
            screen_y,
            epoch: self.scroll_anim_epoch,
            remaining_frames: 8,
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(3),
            confirmation: CursorCenterConfirmation::default(),
        });
    }

    fn document_mode_images_pending(&self) -> bool {
        let (start, end) = self.generated_preview_priority_line_range();
        self.decorations.get(start..=end).is_some_and(|rows| {
            rows.iter().any(|row| {
                row.image_path
                    .as_ref()
                    .is_some_and(|path| self.image_cache.get(path).is_none_or(Option::is_none))
            })
        })
    }

    /// Measure without advancing the frame confirmation: prepaint may discard
    /// this attempt and reconcile its geometry again before anything is painted.
    pub(crate) fn restore_document_mode_scroll_anchor(&mut self, after_layout: bool) -> bool {
        let Some(anchor) = self.document_mode_scroll_anchor.take() else {
            return false;
        };
        if !anchor.is_current(
            self.active_tab_id.as_deref(),
            self.cursor,
            self.scroll_anim_epoch,
        ) || self.pending_click_anchor.is_some()
            || self.pending_typing_scroll_guard.is_some()
            || self.pending_center_cursor_in_view
            || self.pending_scroll_to_cursor
            || self.editor_scrollbar_drag.is_some()
            || self.minimap_state.is_dragging()
            || self.fold_scroll_guard.is_some()
            || self.center_cursor_confirm_frames > 0
            || super::outline::outline_navigation_in_progress(
                self.outline_navigation_epoch,
                self.scroll_anim_epoch,
            )
        {
            return false;
        }
        let (content_y, _) = self.document_mode_anchor_content_y(anchor.offset);
        let target = screen_anchored_scroll_target(content_y, anchor.screen_y);
        let previous_scroll = self.scroll_offset;
        self.scroll_offset = target;
        if after_layout {
            self.clamp_scroll();
        }
        let changed = (self.scroll_offset - previous_scroll).abs() > 0.5;
        if changed {
            self.table_cursor_paint_bounds = None;
        }
        self.pending_preview_reveal = None;
        self.document_mode_scroll_anchor = Some(anchor);
        changed
    }

    /// Observe only accepted frames. Async image/preview completions wake the
    /// anchor themselves; waiting for them must not poll or consume frame budget.
    pub(crate) fn confirm_document_mode_scroll_anchor(&mut self) -> bool {
        let Some(mut anchor) = self.document_mode_scroll_anchor.take() else {
            return false;
        };
        if !anchor.is_current(
            self.active_tab_id.as_deref(),
            self.cursor,
            self.scroll_anim_epoch,
        ) {
            return false;
        }
        let (content_y, measured) = self.document_mode_anchor_content_y(anchor.offset);
        let target =
            screen_anchored_scroll_target(content_y, anchor.screen_y).min(self.max_scroll_offset());
        let previews_need_rendering = self.visible_generated_previews_need_rendering();
        let previews_in_flight = self.generated_preview_rerender_pending
            || self.generated_preview_rerender_task.is_some()
            || self.document_mode_images_pending()
            || self.mermaid_previews_in_flight();
        // Missing, unqueued previews need a bounded follow-up frame. Queued
        // Mermaid work instead wakes layout on completion, without polling or
        // spending the anchor's frame budget while the CPU job runs.
        if !anchor.confirm_painted_layout(
            target,
            self.scroll_offset,
            measured,
            previews_need_rendering,
            previews_in_flight,
        ) {
            return false;
        }
        self.document_mode_scroll_anchor = Some(anchor);
        previews_need_rendering || !previews_in_flight
    }

    pub(crate) fn toggle_outline_visibility(&mut self, cx: &mut Context<Self>) {
        self.toggle_outline_panel(cx);
    }

    pub(crate) fn toggle_info_panel(&mut self, cx: &mut Context<Self>) {
        self.toggle_utility_panel(RightUtilityPanel::Info, cx);
    }

    pub(crate) fn toggle_outline_panel(&mut self, cx: &mut Context<Self>) {
        self.toggle_utility_panel(RightUtilityPanel::Outline, cx);
    }

    pub(crate) fn toggle_utility_panel(
        &mut self,
        panel: RightUtilityPanel,
        cx: &mut Context<Self>,
    ) {
        if self.utility_panel == Some(panel) {
            self.utility_panel_resizing = false;
            self.set_visible_utility_panel(None, cx);
            return;
        }

        self.set_visible_utility_panel(Some(panel), cx);
    }
}

#[cfg(target_os = "macos")]
fn macos_traffic_light_position_matches(
    window: &mut Window,
    ui_scale: f32,
    tab_bar_height: f32,
) -> bool {
    use objc::{msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::AppKit(appkit_handle) = handle.as_raw() else {
        return false;
    };

    unsafe {
        let ns_view = appkit_handle.ns_view.as_ptr() as *mut objc::runtime::Object;
        let ns_window: *mut objc::runtime::Object = msg_send![ns_view, window];
        if ns_window.is_null() {
            return false;
        }
        let close_button: *mut objc::runtime::Object =
            msg_send![ns_window, standardWindowButton: 0u64];
        if close_button.is_null() {
            return false;
        }

        let frame: core_graphics::geometry::CGRect = msg_send![close_button, frame];
        let window_frame: core_graphics::geometry::CGRect = msg_send![ns_window, frame];
        let content_layout_rect: core_graphics::geometry::CGRect =
            msg_send![ns_window, contentLayoutRect];
        let titlebar_height = window_frame.size.height - content_layout_rect.size.height;
        let expected =
            traffic_light_offsets_for_layout(ui_scale, tab_bar_height, frame.size.height as f32);
        let actual_left = frame.origin.x;
        let actual_top = titlebar_height - frame.origin.y - frame.size.height;
        (actual_left - f32::from(expected.x) as f64).abs() < 0.25
            && (actual_top - f32::from(expected.y) as f64).abs() < 0.25
    }
}

#[cfg(target_os = "macos")]
fn traffic_light_offsets_for_layout(
    ui_scale: f32,
    tab_bar_height: f32,
    button_height: f32,
) -> gpui::Point<gpui::Pixels> {
    const BASE_LEFT_PX: f32 = 10.0;
    point(
        px(BASE_LEFT_PX * ui_scale),
        px(((tab_bar_height - button_height) * 0.5).max(0.0)),
    )
}

#[cfg(target_os = "macos")]
fn apply_macos_traffic_light_position(
    window: &mut Window,
    ui_scale: f32,
    tab_bar_height: f32,
) -> bool {
    use core_graphics::geometry::{CGPoint, CGRect};
    use objc::{msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::AppKit(appkit_handle) = handle.as_raw() else {
        return false;
    };

    unsafe {
        let ns_view = appkit_handle.ns_view.as_ptr() as *mut objc::runtime::Object;
        let ns_window: *mut objc::runtime::Object = msg_send![ns_view, window];
        if ns_window.is_null() {
            return false;
        }
        let style_mask: u64 = msg_send![ns_window, styleMask];
        const NS_WINDOW_STYLE_MASK_FULL_SCREEN: u64 = 1 << 14;
        if style_mask & NS_WINDOW_STYLE_MASK_FULL_SCREEN != 0 {
            return false;
        }

        let close_button: *mut objc::runtime::Object =
            msg_send![ns_window, standardWindowButton: 0u64];
        let minimize_button: *mut objc::runtime::Object =
            msg_send![ns_window, standardWindowButton: 1u64];
        let zoom_button: *mut objc::runtime::Object =
            msg_send![ns_window, standardWindowButton: 2u64];
        if close_button.is_null() || minimize_button.is_null() || zoom_button.is_null() {
            return false;
        }

        let window_frame: CGRect = msg_send![ns_window, frame];
        let content_layout_rect: CGRect = msg_send![ns_window, contentLayoutRect];
        let close_frame: CGRect = msg_send![close_button, frame];
        let minimize_frame: CGRect = msg_send![minimize_button, frame];
        let titlebar_height = window_frame.size.height - content_layout_rect.size.height;
        let offsets = traffic_light_offsets_for_layout(
            ui_scale,
            tab_bar_height,
            close_frame.size.height as f32,
        );
        let left = f32::from(offsets.x) as f64;
        let top = f32::from(offsets.y) as f64;
        let y = titlebar_height - top - close_frame.size.height;
        let spacing = minimize_frame.origin.x - close_frame.origin.x;

        let _: () = msg_send![close_button, setFrameOrigin: CGPoint::new(left, y)];
        let _: () = msg_send![minimize_button, setFrameOrigin: CGPoint::new(left + spacing, y)];
        let _: () = msg_send![zoom_button, setFrameOrigin: CGPoint::new(left + spacing * 2.0, y)];
    }
    true
}

/// Fullscreen windows fill the display, so the rounded content corners that
/// match the macOS window chrome have to be squared off.
#[cfg(target_os = "macos")]
pub(crate) fn apply_macos_content_corner_radius(window: &mut Window, radius: f64) {
    use objc::{msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(appkit_handle) = handle.as_raw() else {
        return;
    };

    unsafe {
        let ns_view = appkit_handle.ns_view.as_ptr() as *mut objc::runtime::Object;
        let ns_window: *mut objc::runtime::Object = msg_send![ns_view, window];
        if ns_window.is_null() {
            return;
        }
        let content_view: *mut objc::runtime::Object = msg_send![ns_window, contentView];
        if content_view.is_null() {
            return;
        }
        let () = msg_send![content_view, setWantsLayer: true];
        let layer: *mut objc::runtime::Object = msg_send![content_view, layer];
        if layer.is_null() {
            return;
        }
        let () = msg_send![layer, setCornerRadius: radius];
        let () = msg_send![layer, setMasksToBounds: true];
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn document_modes_cycle_without_overwriting_alignment_preference() {
        use super::DocumentMode;
        assert_eq!(DocumentMode::from_flags(false, false), DocumentMode::Edit);
        assert_eq!(DocumentMode::from_flags(true, false), DocumentMode::Reading);
        assert_eq!(DocumentMode::from_flags(false, true), DocumentMode::Source);
        assert_eq!(DocumentMode::from_flags(true, true), DocumentMode::Source);
        for preference in [false, true] {
            let mut mode = DocumentMode::Edit;
            for expected in [
                DocumentMode::Reading,
                DocumentMode::Source,
                DocumentMode::Edit,
            ] {
                mode = mode.next();
                assert_eq!(mode, expected);
                assert_eq!(
                    mode.left_aligned(preference),
                    mode == DocumentMode::Source || preference
                );
            }
            assert_eq!(mode.left_aligned(preference), preference);
        }
    }

    #[test]
    fn document_modes_cycle_backwards_and_reverse_the_forward_cycle() {
        use super::DocumentMode;
        for (mode, previous) in [
            (DocumentMode::Edit, DocumentMode::Source),
            (DocumentMode::Source, DocumentMode::Reading),
            (DocumentMode::Reading, DocumentMode::Edit),
        ] {
            assert_eq!(mode.previous(), previous);
            assert_eq!(mode.previous().next(), mode);
            assert_eq!(mode.next().previous(), mode);
            assert_eq!(mode.previous().previous().previous(), mode);
        }
    }

    #[test]
    fn document_modes_use_the_requested_icons_and_keep_them_embedded() {
        use super::DocumentMode;
        for (mode, path) in [
            (DocumentMode::Edit, "assets/icons/editor/textbox_fill.svg"),
            (DocumentMode::Reading, "assets/icons/workspace/doc.svg"),
            (
                DocumentMode::Source,
                "assets/icons/editor/pencil_ruler_fill.svg",
            ),
        ] {
            assert_eq!(mode.icon_path(), path);
            assert!(
                crate::embedded_asset(path).is_some(),
                "missing icon: {path}"
            );
        }
    }

    #[test]
    fn source_word_wrap_is_independent_of_markdown_alignment_and_other_modes() {
        use super::DocumentMode;
        for left_aligned in [false, true] {
            for non_markdown in [false, true] {
                for source_wrap in [false, true] {
                    assert_eq!(
                        DocumentMode::Source.word_wrap(source_wrap, non_markdown, left_aligned),
                        source_wrap
                    );
                    assert!(DocumentMode::Source.left_aligned(left_aligned));
                    for mode in [DocumentMode::Edit, DocumentMode::Reading] {
                        assert_eq!(
                            mode.word_wrap(source_wrap, non_markdown, left_aligned),
                            !non_markdown || !left_aligned
                        );
                        assert_eq!(mode.left_aligned(left_aligned), left_aligned);
                    }
                }
            }
        }
    }

    #[test]
    fn document_mode_scroll_keeps_the_visual_row_at_its_previous_screen_height() {
        use super::screen_anchored_scroll_target;
        for (old_content_y, old_scroll, new_content_y, expected) in [
            (1400.0, 1100.0, 2200.0, 1900.0),
            (2200.0, 1900.0, 1400.0, 1100.0),
            (1400.0, 1100.0, 1432.0, 1132.0),
            (1400.0, 1100.0, 1400.0, 1100.0),
            (1400.0, 1450.0, 900.0, 950.0),
        ] {
            let screen_y = old_content_y - old_scroll;
            let target = screen_anchored_scroll_target(new_content_y, screen_y);
            assert_eq!(target, expected);
            assert_eq!(new_content_y - target, screen_y);
        }
        // A shorter document cannot scroll past its beginning to preserve a row.
        assert_eq!(screen_anchored_scroll_target(100.0, 300.0), 0.0);
    }

    fn document_mode_anchor() -> super::DocumentModeScrollAnchor {
        super::DocumentModeScrollAnchor {
            tab_id: "document".into(),
            cursor: crate::cursor::Cursor {
                offset: 12,
                anchor: 4,
            },
            offset: 12,
            screen_y: 200.0,
            epoch: 7,
            remaining_frames: 8,
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(3),
            confirmation: super::CursorCenterConfirmation::default(),
        }
    }

    #[test]
    fn document_mode_scroll_anchor_yields_to_newer_user_actions() {
        let anchor = document_mode_anchor();
        assert!(anchor.is_current(Some("document"), anchor.cursor, 7));
        assert!(!anchor.is_current(Some("other-tab"), anchor.cursor, 7));
        assert!(!anchor.is_current(None, anchor.cursor, 7));
        assert!(!anchor.is_current(Some("document"), anchor.cursor, 8));
        assert!(!anchor.is_current(
            Some("document"),
            crate::cursor::Cursor {
                offset: 13,
                ..anchor.cursor
            },
            7
        ));
        assert!(!anchor.is_current(
            Some("document"),
            crate::cursor::Cursor {
                anchor: 5,
                ..anchor.cursor
            },
            7
        ));
    }

    #[test]
    fn document_mode_confirmation_observes_only_geometry_painted_at_the_target() {
        let mut anchor = document_mode_anchor();
        for _ in 0..3 {
            assert!(anchor.confirm_painted_layout(1100.0, 900.0, true, false, false));
        }
        assert_eq!(anchor.remaining_frames, 5);
        // Rejected same-frame attempts never reach confirmation. Once an
        // accepted frame reaches the target it still needs stable observations.
        assert!(anchor.confirm_painted_layout(1100.0, 1100.0, true, false, false));
        assert!(anchor.confirm_painted_layout(1100.0, 1100.0, true, false, false));
        assert!(!anchor.confirm_painted_layout(1100.0, 1100.0, true, false, false));
    }

    #[test]
    fn document_mode_missing_previews_request_bounded_followup_layouts() {
        // A resized/reflowed viewport can expose previews after this frame's
        // synchronous preview budget was spent. They need another frame rather
        // than an async completion notification that will never arrive. A slow
        // image alongside this work must not bypass the active-frame budget.
        for previews_in_flight in [false, true] {
            let mut anchor = document_mode_anchor();
            for _ in 0..7 {
                assert!(anchor.confirm_painted_layout(
                    900.0,
                    900.0,
                    true,
                    true,
                    previews_in_flight,
                ));
            }
            assert!(!anchor.confirm_painted_layout(900.0, 900.0, true, true, previews_in_flight,));
            assert_eq!(anchor.remaining_frames, 0);
        }
        let mut anchor = document_mode_anchor();
        for _ in 0..12 {
            assert!(anchor.confirm_painted_layout(900.0, 900.0, true, false, true));
        }
        assert_eq!(anchor.remaining_frames, 8);
    }

    #[test]
    fn document_mode_scroll_confirmation_settles_and_bounds_preview_work() {
        let mut anchor = document_mode_anchor();
        assert!(anchor.needs_another_layout(Some(900.0), false));
        assert!(anchor.needs_another_layout(Some(900.0), false));
        assert!(!anchor.needs_another_layout(Some(900.0), false));
        let mut anchor = document_mode_anchor();
        for _ in 0..7 {
            assert!(anchor.needs_another_layout(None, false));
        }
        assert!(!anchor.needs_another_layout(None, false));
        // Image completion wakes reconciliation, without burning frame budget
        // or polling while the decoder is still in flight.
        let mut anchor = document_mode_anchor();
        for _ in 0..12 {
            assert!(anchor.needs_another_layout(Some(900.0), true));
        }
        assert_eq!(anchor.remaining_frames, 8);
        assert!(anchor.needs_another_layout(Some(1100.0), false));
        assert!(anchor.needs_another_layout(Some(1100.0), false));
        assert!(!anchor.needs_another_layout(Some(1100.0), false));
        let mut anchor = document_mode_anchor();
        anchor.deadline = std::time::Instant::now() - std::time::Duration::from_millis(1);
        assert!(!anchor.needs_another_layout(Some(900.0), true));
        assert!(!anchor.is_current(Some("document"), anchor.cursor, 7));
    }

    #[cfg(target_os = "macos")]
    use super::{EditorView, traffic_light_offsets_for_layout};
    use super::{
        caret_vertical_overflow, centered_row_scroll_target, cursor_scroll_viewport,
        cursor_visibility_scroll_delta, last_soft_row_scroll_limit, typing_scroll_restore_offset,
        typing_scroll_should_restore,
    };

    #[test]
    fn front_matter_vertical_navigation_skips_unfocused_delimiters_without_a_prepaint_cache() {
        use super::{FrontMatterVerticalTarget as Target, front_matter_vertical_neighbor};
        let buffer = crate::buffer::Buffer::from_text("---\ntitle: value\n---\nBody");
        let decorations = crate::markdown::parse_decorations(buffer.text());
        // A reading-mode caret or an editable body caret must bypass hidden
        // separators, even if the last frame still measured them as visible.
        for editable in [false, true] {
            for stale_cache in [None, Some(0), Some(1)] {
                let neighbor = |line, downward, visible| {
                    front_matter_vertical_neighbor(
                        &buffer,
                        &decorations,
                        &[],
                        line,
                        downward,
                        editable,
                        visible,
                        |_| stale_cache,
                    )
                };
                let expected = if stale_cache.is_none() || (!editable && stale_cache == Some(0)) {
                    if !editable && stale_cache == Some(0) {
                        None
                    } else {
                        Some(Target::Source(buffer.line_end(1)))
                    }
                } else {
                    Some(Target::Line(1))
                };
                assert_eq!(neighbor(3, false, false), expected);
                assert_eq!(neighbor(1, true, false), Some(Target::Line(3)));
                assert_eq!(neighbor(1, false, false), None);
                if editable && stale_cache == Some(1) {
                    assert_eq!(neighbor(1, false, true), Some(Target::Line(0)));
                    assert_eq!(neighbor(1, true, true), Some(Target::Line(2)));
                }
            }
        }
    }

    #[test]
    fn front_matter_vertical_navigation_enters_values_instead_of_hidden_column_zero() {
        use super::super::helpers::front_matter_group_plan;
        use super::{FrontMatterVerticalTarget as Target, front_matter_vertical_neighbor};
        let source = "---\ntags:\n  - 'first'\n  - 二🌸\nnext: scalar\nbody: |-\n  first line\n  second line\n---";
        let buffer = crate::buffer::Buffer::from_text(source);
        let decorations = crate::markdown::parse_decorations(source);
        let plans = front_matter_group_plan(&buffer, &decorations, &[], true);
        let hidden = |line| Some(if [2, 3, 5].contains(&line) { 0 } else { 1 });
        let neighbor = |line, downward| {
            front_matter_vertical_neighbor(
                &buffer,
                &decorations,
                &plans,
                line,
                downward,
                true,
                true,
                hidden,
            )
        };
        assert_eq!(
            neighbor(0, true),
            Some(Target::Source(plans[0].group.items[0].source_range.start))
        );
        assert_eq!(
            neighbor(1, true),
            Some(Target::Source(plans[0].group.items[0].source_range.start))
        );
        assert_eq!(
            neighbor(4, false),
            Some(Target::Source(plans[0].group.items[1].source_range.end))
        );
        let body_start = buffer.line_start(6)
            + decorations[6]
                .front_matter
                .as_ref()
                .unwrap()
                .value_range
                .start;
        assert_eq!(neighbor(4, true), Some(Target::Source(body_start)));
        // Focus has already changed, but the previous frame still has zero-height item/header caches.
        let caret = plans[0].group.items[0].source_range.start;
        let active = front_matter_group_plan(&buffer, &decorations, &[caret..caret], true);
        assert_eq!(
            front_matter_vertical_neighbor(
                &buffer,
                &decorations,
                &active,
                1,
                true,
                true,
                true,
                hidden
            ),
            Some(Target::Source(caret))
        );
        let caret = body_start;
        let active = front_matter_group_plan(&buffer, &decorations, &[caret..caret], true);
        assert_eq!(
            front_matter_vertical_neighbor(
                &buffer,
                &decorations,
                &active,
                6,
                false,
                true,
                true,
                hidden
            ),
            Some(Target::Source(
                buffer.line_start(5)
                    + decorations[5]
                        .front_matter
                        .as_ref()
                        .unwrap()
                        .value_range
                        .end
            ))
        );
    }

    #[test]
    fn front_matter_reading_navigation_skips_hidden_rows_and_uses_visible_click_anchor() {
        use super::super::helpers::front_matter_group_plan;
        use super::{
            FrontMatterVerticalTarget as Target, front_matter_vertical_neighbor,
            front_matter_visible_navigation_line,
        };
        let source = "---\ntags:\n  - first\n  - second\nnext: scalar\nbody: |-\n  first line\n  second line\n---";
        let buffer = crate::buffer::Buffer::from_text(source);
        let decorations = crate::markdown::parse_decorations(source);
        let plans = front_matter_group_plan(&buffer, &decorations, &[], false);
        let hidden = |line| Some(if [2, 3, 5].contains(&line) { 0 } else { 1 });
        let neighbor = |line, downward| {
            front_matter_vertical_neighbor(
                &buffer,
                &decorations,
                &plans,
                line,
                downward,
                false,
                false,
                hidden,
            )
        };
        assert_eq!(neighbor(1, true), Some(Target::Line(4)));
        assert_eq!(neighbor(4, false), Some(Target::Line(1)));
        assert_eq!(neighbor(4, true), Some(Target::Line(6)));
        assert_eq!(neighbor(6, false), Some(Target::Line(4)));
        assert_eq!(front_matter_visible_navigation_line(&plans, 3, false), 1);
        assert_eq!(front_matter_visible_navigation_line(&plans, 5, false), 6);
        assert_eq!(front_matter_visible_navigation_line(&plans, 3, true), 3);
        assert_eq!(buffer.text(), source);
    }

    #[test]
    fn cursor_center_bootstraps_unmeasured_target_then_uses_measured_match_row() {
        use super::cursor_center_layout_target;
        let viewport = 600.0;
        let estimate = cursor_center_layout_target(None, 10_000.0, 24.0, viewport);
        assert_eq!(estimate, 9_712.0);
        assert!(estimate <= 10_000.0 && 10_024.0 <= estimate + viewport);

        // The match is on a later soft row after syntax expansion / tall math,
        // not at the source line's initial estimated top.
        let measured =
            cursor_center_layout_target(Some((10_084.0, 10_144.0)), 10_000.0, 24.0, viewport);
        assert_eq!(measured, 9_814.0);
        assert_eq!((10_084.0 + 10_144.0) * 0.5 - measured, viewport * 0.5);
    }

    #[test]
    fn table_match_center_does_not_follow_the_mutated_scroll_offset() {
        use super::super::TableCursorGeometry;
        use gpui::{Bounds, point, px, size};

        for initial_scroll in [0.0, 1_000.0, 5_000.0] {
            let viewport_top = 85.0;
            let row_top = viewport_top + 3_000.0 - initial_scroll;
            let mut geometry = TableCursorGeometry::from_paint(
                100,
                Bounds::new(
                    point(px(10.0), px(3_000.0 - initial_scroll)),
                    size(px(2.0), px(20.0)),
                ),
                3,
                row_top..row_top + 24.0,
                viewport_top,
                initial_scroll,
            );
            let mut scroll = initial_scroll;
            for _ in 0..3 {
                let (top, bottom) = geometry.vertical_bounds();
                scroll = centered_row_scroll_target(top, bottom, 600.0);
                assert_eq!(scroll, 2_712.0);
                assert_eq!((top + bottom) * 0.5 - scroll, 300.0);
                // A later attempt paints at a different screen coordinate;
                // content-space centering must remain unchanged.
                geometry.paint_bounds.origin.y = px(top - scroll);
            }
            // Preview growth above the table shifts typing's saved caret top,
            // but preserving its screen position needs only that content delta.
            geometry.content_top += 200.0;
            let (top, _) = geometry.vertical_bounds();
            assert_eq!(
                typing_scroll_restore_offset(scroll, 0.0, Some(3_000.0), top),
                scroll,
            );
        }
    }

    #[test]
    fn cursor_center_wrap_match_uses_cumulative_row_top_and_its_own_height() {
        use super::cursor_center_layout_target;
        let wrapped = super::super::input_wrap::test_wrapped_line(&[24.0, 60.0, 24.0]);
        for (offset, expected_top, expected_bottom) in
            [(1, 100.0, 124.0), (3, 124.0, 184.0), (5, 184.0, 208.0)]
        {
            let pos = super::EditorView::visual_position_for_wrapped_display(
                &wrapped,
                &wrapped.text,
                offset,
                24.0,
                0.0,
                &std::collections::HashMap::new(),
                None,
            )
            .unwrap();
            let row = wrapped.row_for_y(pos.y, gpui::px(24.0));
            let top = 100.0 + f32::from(wrapped.row_top(row, gpui::px(24.0)));
            let bottom = top + f32::from(wrapped.row_height(row, gpui::px(24.0)));
            assert_eq!((top, bottom), (expected_top, expected_bottom));
            let target = cursor_center_layout_target(Some((top, bottom)), 100.0, 24.0, 80.0);
            assert_eq!((top + bottom) * 0.5 - target, 40.0);
        }
    }

    #[test]
    fn cursor_center_confirmation_waits_for_measured_geometry_and_preview_refresh() {
        let mut confirmation = super::CursorCenterConfirmation::default();
        for _ in 0..5 {
            assert!(!confirmation.observe(None, false));
        }
        assert!(!confirmation.observe(Some(100.0), true));
        for _ in 0..5 {
            assert!(!confirmation.observe(Some(100.0), true));
        }
        assert!(!confirmation.observe(Some(100.0), false));
        assert!(confirmation.observe(Some(100.0), false));
    }

    #[test]
    fn cursor_center_confirmation_restarts_when_layout_or_target_changes() {
        let mut confirmation = super::CursorCenterConfirmation::default();
        assert!(!confirmation.observe(Some(100.0), false));
        assert!(!confirmation.observe(Some(100.0), false));
        assert!(!confirmation.observe(Some(160.0), false));
        assert!(!confirmation.observe(Some(160.0), false));
        assert!(!confirmation.observe(None, false));
        assert!(!confirmation.observe(Some(160.0), false));
        assert!(!confirmation.observe(Some(160.0), false));
        assert!(confirmation.observe(Some(160.0), false));
    }

    #[test]
    fn cursor_center_scrolls_to_the_final_target_only_after_layout_settles() {
        let mut confirmation = super::CursorCenterConfirmation::default();
        assert_eq!(
            super::settled_cursor_center_target(&mut confirmation, Some(100.0), false),
            None
        );
        assert_eq!(
            super::settled_cursor_center_target(&mut confirmation, Some(160.0), false),
            None
        );
        assert_eq!(
            super::settled_cursor_center_target(&mut confirmation, Some(160.0), true),
            None
        );
        assert_eq!(
            super::settled_cursor_center_target(&mut confirmation, Some(160.0), false),
            None
        );
        assert_eq!(
            super::settled_cursor_center_target(&mut confirmation, Some(160.0), false),
            Some(160.0)
        );
    }

    #[test]
    fn cursor_center_epoch_protects_preview_refresh_but_not_newer_navigation() {
        use super::super::outline::outline_navigation_in_progress;
        let mut epoch = 4;
        let mut navigation = Some(epoch);
        super::begin_cursor_center_epoch(&mut epoch, &mut navigation);
        assert_eq!((epoch, navigation), (5, Some(5)));
        assert!(outline_navigation_in_progress(navigation, epoch));
        assert!(!outline_navigation_in_progress(Some(4), epoch));
        // A second request supersedes the first even if its scroll delta is
        // subpixel or its layout has not been measured yet.
        super::begin_cursor_center_epoch(&mut epoch, &mut navigation);
        assert!(!outline_navigation_in_progress(Some(5), epoch));
        assert!(outline_navigation_in_progress(navigation, epoch));
        // Wheel scrolling increments the same epoch without renewing the guard.
        epoch = epoch.wrapping_add(1);
        assert!(!outline_navigation_in_progress(navigation, epoch));
    }

    #[test]
    fn cursor_center_epoch_wraps_without_reusing_the_previous_navigation() {
        use super::super::outline::outline_navigation_in_progress;
        let mut epoch = usize::MAX;
        let mut navigation = Some(epoch);
        super::begin_cursor_center_epoch(&mut epoch, &mut navigation);
        assert_eq!((epoch, navigation), (0, Some(0)));
        assert!(outline_navigation_in_progress(navigation, epoch));
        assert!(!outline_navigation_in_progress(Some(usize::MAX), epoch));
    }

    #[test]
    fn cursor_center_confirmation_accepts_subpixel_layout_noise() {
        let mut confirmation = super::CursorCenterConfirmation::default();
        assert!(!confirmation.observe(Some(100.0), false));
        assert!(!confirmation.observe(Some(100.25), false));
        assert!(confirmation.observe(Some(100.0), false));
    }

    #[test]
    fn oversized_row_reveal_converges_without_top_bottom_oscillation() {
        let (top, bottom, viewport) = (100.0, 400.0, 80.0);
        for initial_scroll in [0.0, 100.0, 210.0, 350.0, 500.0] {
            let scroll = initial_scroll
                + cursor_visibility_scroll_delta(top, bottom, initial_scroll, viewport);
            assert_eq!(
                cursor_visibility_scroll_delta(top, bottom, scroll, viewport),
                0.0
            );
            assert_eq!(
                caret_vertical_overflow(top, bottom, scroll, scroll + viewport),
                std::cmp::Ordering::Equal
            );
        }
    }

    #[test]
    fn preview_source_navigation_continues_down_across_multiple_viewports() {
        let mut scroll = 0.0;
        let viewport = 120.0;
        for row in 0..500 {
            let top = row as f32 * 24.0;
            let bottom = top + 24.0;
            if !super::preview_source_cursor_is_visible(Some((top, bottom)), scroll, viewport) {
                scroll += cursor_visibility_scroll_delta(top, bottom, scroll, viewport);
            }
            assert!(super::preview_source_cursor_is_visible(
                Some((top, bottom)),
                scroll,
                viewport
            ));
        }
        assert!(scroll > viewport * 10.0);
        assert!(!super::preview_source_cursor_is_visible(
            Some((0.0, 24.0)),
            scroll,
            viewport
        ));
        assert!(super::preview_source_cursor_is_visible(
            Some((40.0, 64.0)),
            0.0,
            viewport
        ));
        assert!(!super::preview_source_cursor_is_visible(
            None, 0.0, viewport
        ));
    }

    #[test]
    fn last_soft_row_scroll_limit_and_center_use_measured_rows() {
        let wrapped = super::super::input_wrap::test_wrapped_line(&[24.0, 60.0, 24.0]);
        let text_top = 100.0;
        let content_bottom = text_top + f32::from(wrapped.size(gpui::px(24.0)).height);
        let last_top = text_top + f32::from(wrapped.row_top(2, gpui::px(24.0)));
        assert_eq!(
            last_soft_row_scroll_limit(content_bottom, last_top, 80.0),
            184.0
        );
        assert_eq!(
            centered_row_scroll_target(last_top, content_bottom, 60.0),
            166.0
        );
        // A single oversized final row must still allow its lower content to be revealed.
        assert_eq!(last_soft_row_scroll_limit(500.0, 100.0, 80.0), 420.0);
    }

    #[test]
    fn lazy_layout_only_corrects_caret_that_leaves_the_viewport() {
        assert_eq!(
            cursor_visibility_scroll_delta(130.0, 150.0, 100.0, 200.0),
            0.0
        );
        assert_eq!(
            cursor_visibility_scroll_delta(80.0, 100.0, 100.0, 200.0),
            -20.0
        );
        assert_eq!(
            cursor_visibility_scroll_delta(310.0, 330.0, 100.0, 200.0),
            30.0
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn traffic_light_stays_centered_in_scaled_tab_bar() {
        let button_height = 14.0;
        for ui_scale in [0.8, 1.0, 1.4] {
            let tab_bar_height = crate::style::TOP_BAR_HEIGHT_PX * ui_scale;
            let offsets = traffic_light_offsets_for_layout(ui_scale, tab_bar_height, button_height);
            assert_eq!(f32::from(offsets.x), 10.0 * ui_scale);
            let actual_center = f32::from(offsets.y) + button_height * 0.5;
            let expected_center = tab_bar_height * 0.5;
            assert!((actual_center - expected_center).abs() < 0.0001);

            let initial = EditorView::traffic_light_position_for_ui_scale(ui_scale);
            assert_eq!(initial, offsets);
        }
    }

    #[test]
    fn tall_row_scroll_and_typing_guard_use_its_actual_bottom() {
        let wrapped = super::super::input_wrap::test_wrapped_line(&[24.0, 60.0, 24.0]);
        let top = 100.0 + f32::from(wrapped.row_top(1, gpui::px(24.0)));
        let bottom = top + f32::from(wrapped.row_height(1, gpui::px(24.0)));
        assert_eq!((top, bottom), (124.0, 184.0));
        assert_eq!(
            cursor_visibility_scroll_delta(top, bottom, 100.0, 60.0),
            24.0
        );
        assert!(!typing_scroll_should_restore(
            true, false, true, 100.0, 60.0, top, bottom
        ));
        assert!(typing_scroll_should_restore(
            true, false, true, 100.0, 84.0, top, bottom
        ));
        let following_top = 100.0 + f32::from(wrapped.row_top(2, gpui::px(24.0)));
        assert_eq!(following_top, bottom);
    }

    #[test]
    fn up_down_targets_stay_inside_each_actual_row() {
        let wrapped = super::super::input_wrap::test_wrapped_line(&[24.0, 60.0, 24.0]);
        for (row, center) in [(0, 12.0), (1, 54.0), (2, 96.0)] {
            let target_y = wrapped.row_top(row, gpui::px(24.0))
                + wrapped.row_height(row, gpui::px(24.0)) * 0.5;
            assert_eq!(f32::from(target_y), center);
            assert_eq!(wrapped.row_for_y(target_y, gpui::px(24.0)), row);
            assert_eq!(
                wrapped.closest_index_for_position(
                    gpui::point(gpui::px(10.0), target_y),
                    gpui::px(24.0)
                ),
                Ok(row * 2 + 1)
            );
        }
    }

    #[test]
    fn caret_scrolls_only_after_leaving_the_viewport() {
        use std::cmp::Ordering::{Equal, Greater, Less};

        assert_eq!(caret_vertical_overflow(100.0, 120.0, 100.0, 200.0), Equal);
        assert_eq!(caret_vertical_overflow(180.0, 200.0, 100.0, 200.0), Equal);
        assert_eq!(caret_vertical_overflow(99.9, 119.9, 100.0, 200.0), Less);
        assert_eq!(caret_vertical_overflow(180.1, 200.1, 100.0, 200.0), Greater);
    }

    #[test]
    fn cursor_scroll_viewport_does_not_exclude_editor_padding() {
        let viewport = cursor_scroll_viewport(100.0, 100.0);

        assert_eq!(viewport, (100.0, 200.0));
        assert_eq!(
            caret_vertical_overflow(180.0, 200.0, viewport.0, viewport.1),
            std::cmp::Ordering::Equal
        );
    }

    #[test]
    fn multiline_history_reveal_has_one_stable_minimal_target() {
        for (top, bottom, expected) in [
            (180.0, 210.0, 120.0),
            (90.0, 110.0, 90.0),
            (410.0, 450.0, 150.0),
        ] {
            let before = 120.0;
            let mut current = before;
            for _ in 0..3 {
                current = super::edit_scroll_target(before, 300.0, top, bottom);
                assert_eq!(current, expected);
            }
            assert_eq!(
                super::edit_scroll_target(before, 300.0, top, bottom),
                current
            );
        }
    }

    #[test]
    fn editing_keeps_viewport_when_caret_changes_lines_or_heading_height() {
        assert_eq!(
            typing_scroll_restore_offset(120.0, 30.0, Some(180.0), 210.0),
            120.0
        );
        assert!(typing_scroll_should_restore(
            false, false, true, 120.0, 300.0, 210.0, 250.0
        ));
        assert!(!typing_scroll_should_restore(
            false, false, true, 120.0, 300.0, 410.0, 450.0
        ));
    }

    #[test]
    fn typing_scroll_guard_preserves_first_layout_and_visible_short_edits() {
        assert!(typing_scroll_should_restore(
            true, false, false, 120.0, 0.0, 900.0, 920.0
        ));
        assert!(typing_scroll_should_restore(
            true, false, true, 120.0, 300.0, 180.0, 200.0
        ));
        assert!(!typing_scroll_should_restore(
            true, false, true, 120.0, 300.0, 430.0, 450.0
        ));
        assert!(typing_scroll_should_restore(
            false, false, false, 120.0, 300.0, 180.0, 200.0
        ));
    }

    #[test]
    fn typing_guard_respects_preview_height_compensation_above_the_viewport() {
        // The chart above the viewport gains 276px while a visible caret is
        // edited below it. Comparing to the old absolute offset would reject
        // the guard and schedule another caret scroll in the same frame.
        let scroll_before = 120.0;
        let anchor_delta = 276.0;
        assert!(!typing_scroll_should_restore(
            true,
            false,
            true,
            scroll_before,
            300.0,
            430.0,
            450.0
        ));
        assert!(typing_scroll_should_restore(
            true,
            false,
            true,
            scroll_before + anchor_delta,
            300.0,
            430.0,
            450.0
        ));
    }

    #[test]
    fn first_edit_keeps_its_screen_anchor_across_table_and_preview_reflow() {
        for delta in [-240.0, 320.0] {
            let scroll_before = 500.0;
            let previous_top = 600.0;
            let cursor_top = previous_top + delta;
            // A partial anchor correction must be replaced by, not added to,
            // the measured change above the same edited row.
            let target = typing_scroll_restore_offset(
                scroll_before,
                delta / 2.0,
                Some(previous_top),
                cursor_top,
            );
            assert_eq!(target, scroll_before);
        }
        // No reliable old row, or a newly wrapped row: preserve only the
        // independently measured viewport compensation.
        assert_eq!(
            typing_scroll_restore_offset(500.0, 80.0, None, 950.0),
            500.0
        );
    }

    #[test]
    fn pending_fence_typing_keeps_scroll_despite_reparse_height_changes() {
        assert!(!typing_scroll_should_restore(
            true, true, true, 120.0, 300.0, 110.0, 130.0
        ));
        assert!(!typing_scroll_should_restore(
            true, true, true, 120.0, 300.0, 430.0, 450.0
        ));
        assert!(!typing_scroll_should_restore(
            true, false, false, 120.0, 300.0, 430.0, 450.0
        ));
        assert!(!typing_scroll_should_restore(
            true, false, true, 120.0, 300.0, 430.0, 450.0
        ));
        assert!(!typing_scroll_should_restore(
            false, true, true, 120.0, 300.0, 430.0, 450.0
        ));
    }
}
