#![allow(dead_code)]

use std::time::Duration;

use gpui::{FontWeight, Hsla};

use crate::theme::{Theme, with_alpha};

pub(crate) const TOP_BAR_HEIGHT_PX: f32 = 33.0;

/// Fluent caption buttons are 46dp wide regardless of the surrounding UI.
pub(crate) const WINDOW_CAPTION_BUTTON_WIDTH_PX: f32 = 46.0;
pub(crate) const LINUX_CAPTION_BUTTONS_WIDTH_PX: f32 = 108.0;

/// Leading margin for the tab bar's left controls once fullscreen removes the
/// macOS traffic lights.
pub(crate) const FULLSCREEN_LEADING_MARGIN_PX: f32 = 8.0;

/// Per-component style configuration, derived from the active theme.
///
/// Naming convention: `{功能}_{位置}_{状态}_{样式}_{单位}`
///   功能 = editor, sidebar, tab, toolbar, cursor, menu, gutter, …
///   位置 = item, header, bar, container (省略 = 组件自身)
///   状态 = hover, active, selected (默认状态省略)
///   样式 = bg, text_color, font_size, width, height, padding, gap, radius, opacity, border_width, border_color, …
///   单位 = px, ms, factor (无单位 = 无量纲比例)
pub struct Styles {
    pub ui_font_family: &'static str,
    pub ui_font_size_px: f32,
    pub anchored_popup_gap_px: f32,
    pub anchored_popup_margin_px: f32,
    pub status_bar_padding_left_px: f32,
    pub window: WindowStyles,
    pub editor: EditorStyles,
    pub sidebar: SidebarStyles,
    pub outline: OutlineStyles,
    pub utility_panel: UtilityPanelStyles,
    pub tab_bar: TabBarStyles,
    pub toolbar: ToolbarStyles,
    pub format_toolbar: FormatToolbarStyles,
    pub whiteboard: WhiteboardStyles,
    pub tooltip: TooltipStyles,
    pub cursor: CursorStyles,
    pub code_block: CodeBlockStyles,
    pub quote_block: QuoteBlockStyles,
    pub wiki_link: WikiLinkStyles,
    pub checkbox: CheckboxStyles,
    pub table: TableStyles,
    pub gutter: GutterStyles,
    pub context_menu: ContextMenuStyles,
    pub selection: SelectionStyles,
    pub hr: HrStyles,
    pub ime: ImeStyles,
    pub image_preview: ImagePreviewStyles,
    pub dialog: DialogStyles,
    pub notification: NotificationStyles,
    pub workspace_switcher: WorkspaceSwitcherStyles,
    pub text_input: TextInputStyles,
    pub search: SearchStyles,
    pub workspace_search: WorkspaceSearchStyles,
    pub quick_open: QuickOpenStyles,
    pub settings_panel: SettingsPanelStyles,
}

const RIGHT_PANEL_MIN_WIDTH_PX: f32 = 220.0;
const RIGHT_PANEL_MAX_WIDTH_PX: f32 = 560.0;

// ── Window ──────────────────────────────────────────────────────────

pub struct WindowStyles {
    pub window_bg: Hsla,
    pub window_border_width_px: f32,
    pub window_border_color: Hsla,
    pub window_radius_px: f32,
}

// ── Editor ──────────────────────────────────────────────────────────

pub struct EditorStyles {
    pub editor_font_size_px: f32,
    pub editor_h1_font_size_px: f32,
    pub editor_h2_font_size_px: f32,
    pub editor_h3_font_size_px: f32,
    pub editor_h4_font_size_px: f32,
    pub editor_h5_font_size_px: f32,
    pub editor_h6_font_size_px: f32,
    pub editor_paragraph_line_height_factor: f32,
    pub editor_heading_line_height_factor: f32,
    pub editor_letter_spacing_px: f32,
    pub editor_body_latin_font_family: &'static str,
    pub editor_body_cjk_font_family: &'static str,
    pub editor_heading_latin_font_family: &'static str,
    pub editor_heading_cjk_font_family: &'static str,
    pub editor_italic_latin_font_family: &'static str,
    pub editor_italic_cjk_font_family: &'static str,
    pub editor_monospace_font_family: &'static str,
    pub editor_paragraph_padding_top_px: f32,
    pub editor_paragraph_padding_bottom_px: f32,
    pub editor_heading_padding_top_px: f32,
    pub editor_heading_padding_bottom_px: f32,
    pub editor_padding_left_px: f32,
    pub editor_padding_top_px: f32,
    pub editor_non_markdown_padding_top_px: f32,
    pub editor_padding_right_px: f32,
    pub editor_padding_bottom_px: f32,
    pub editor_wrap_right_padding_px: f32,
    pub editor_max_width_px: f32,
    pub editor_non_markdown_max_width_px: f32,
    pub editor_file_drop_bg: Hsla,
    pub editor_path_bar_height_px: f32,
    pub editor_path_bar_padding_x_px: f32,
    pub editor_path_bar_gap_px: f32,
    pub editor_path_bar_font_size_px: f32,
    pub editor_path_bar_bg: Hsla,
    pub editor_path_bar_text_color: Hsla,
    pub editor_path_bar_separator_color: Hsla,
    pub editor_path_bar_separator_size_px: f32,
    pub editor_path_bar_suffix_opacity: f32,
    pub editor_path_bar_item_padding_x_px: f32,
    pub editor_path_bar_item_padding_y_px: f32,
    pub editor_path_bar_item_radius_px: f32,
    pub editor_path_bar_item_hover_text_color: Hsla,
    pub editor_path_bar_item_opacity: f32,
    pub editor_path_bar_diff_group_gap_px: f32,
    pub editor_path_bar_diff_button_label_gap_px: f32,
    pub editor_path_bar_diff_search_gap_px: f32,
    pub editor_path_bar_diff_label_arrow_gap_px: f32,
    pub editor_path_bar_action_gap_px: f32,
    pub editor_path_bar_action_btn_size_px: f32,
    pub editor_path_bar_action_icon_size_px: f32,
    pub editor_path_bar_action_opacity: f32,
    pub editor_path_bar_action_hover_opacity: f32,
    pub editor_path_bar_action_padding_right_px: f32,
    pub editor_path_bar_border_width_px: f32,
    pub editor_path_bar_border_color: Hsla,
    pub editor_list_content_indent_px: f32,
    pub editor_unordered_list_marker_cycle: [&'static str; 3],
    pub editor_unordered_list_marker_font_size_offset_px: f32,
    pub editor_unordered_list_marker_line_height_offset_px: f32,
    pub editor_superscript_scale_factor: f32,
    pub editor_subscript_scale_factor: f32,
    pub editor_superscript_offset_px: f32,
    pub editor_subscript_offset_px: f32,
    pub editor_bg: Hsla,
    pub editor_text_color: Hsla,
    pub editor_empty_placeholder_opacity: f32,
    pub editor_diff_collapsed_line_label_text_color: Hsla,
    pub editor_indent_guide_color: Hsla,
    pub editor_indent_guide_width_px: f32,
    pub editor_indent_guide_dash_length_px: f32,
    pub editor_indent_guide_gap_length_px: f32,
    pub editor_whitespace_dot_size_px: f32,
    pub editor_whitespace_dot_color: Hsla,
    pub editor_inline_code_bg: Hsla,
    pub editor_inline_code_bg_x_padding_px: f32,
    pub editor_inline_code_bg_radius_px: f32,
    pub editor_inline_code_bg_y_inset_px: f32,
    pub editor_inline_math_height_factor: f32,
    pub editor_inline_highlight_bg: Hsla,
    pub editor_list_marker_text_color: Hsla,
    pub editor_list_marker_font_weight: FontWeight,
    pub editor_heading_marker_text_color: Hsla,
    pub editor_selection_match_bg: Hsla,
    pub editor_selection_match_border: Hsla,
    pub editor_bracket_match_border: Hsla,
    pub editor_match_border_width_px: f32,
    pub editor_scrollbar_top_margin_px: f32,
    pub editor_scrollbar_bottom_margin_px: f32,
    pub editor_scrollbar_right_margin_px: f32,
    pub editor_scrollbar_width_px: f32,
    pub editor_scrollbar_hover_width_px: f32,
    pub editor_scrollbar_hit_width_px: f32,
    pub editor_scrollbar_min_thumb_height_px: f32,
    pub editor_scrollbar_radius_px: f32,
    pub editor_scrollbar_thumb_color: Hsla,
    pub editor_scrollbar_idle_opacity: f32,
    pub editor_scrollbar_opacity: f32,
    pub editor_scrollbar_hover_opacity: f32,
    pub editor_scrollbar_animation_frame_ms: u64,
    pub editor_scrollbar_fade_animation_ms: u64,
    pub editor_scrollbar_width_animation_ms: u64,
    pub editor_scrollbar_idle_delay_ms: u64,
    pub editor_scale_step: f32,
    pub editor_scale_min: f32,
    pub editor_scale_max: f32,
}

// ── Sidebar ─────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SidebarStyles {
    // Container
    pub sidebar_width_px: f32,
    pub sidebar_bg: Hsla,
    pub sidebar_bottom_spacer_height_px: f32,
    pub sidebar_top_buttons_visible: bool,
    pub sidebar_top_button_padding_right_px: f32,
    pub sidebar_top_button_gap_px: f32,
    pub sidebar_top_button_width_px: f32,
    pub sidebar_top_button_height_px: f32,
    pub sidebar_top_button_size_px: f32,
    pub sidebar_top_button_icon_size_px: f32,
    pub sidebar_top_button_inactive_color: Hsla,
    pub sidebar_top_button_active_color: Hsla,
    // Logo
    pub sidebar_logo_size_px: f32,
    pub sidebar_logo_padding_y_px: f32,
    // Header (root directory row)
    pub sidebar_header_height_px: f32,
    pub sidebar_header_bg: Hsla,
    pub sidebar_header_padding_left_px: f32,
    pub sidebar_header_padding_right_px: f32,
    pub sidebar_header_gap_px: f32,
    pub sidebar_header_icon_size_px: f32,
    pub sidebar_header_root_icon_size_px: f32,
    pub sidebar_header_star_icon_size_px: f32,
    pub sidebar_header_icon_color: Hsla,
    pub sidebar_header_font_size_px: f32,
    pub sidebar_header_text_color: Hsla,
    pub sidebar_header_hover_bg: Hsla,
    pub sidebar_header_action_gap_px: f32,
    pub sidebar_header_action_btn_size_px: f32,
    pub sidebar_header_action_icon_size_px: f32,
    pub sidebar_starred_item_close_icon_size_px: f32,
    pub sidebar_header_action_opacity: f32,
    pub sidebar_header_action_hover_opacity: f32,
    // File tree item
    pub sidebar_item_height_px: f32,
    pub sidebar_item_indent_base_px: f32,
    pub sidebar_item_indent_step_px: f32,
    pub sidebar_item_left_shift_px: f32,
    pub sidebar_item_padding_right_px: f32,
    pub sidebar_item_gap_px: f32,
    pub sidebar_item_hover_bg: Hsla,
    pub sidebar_item_selected_bg: Hsla,
    // File tree item — fold arrow
    pub sidebar_item_fold_container_width_px: f32,
    pub sidebar_item_fold_container_height_px: f32,
    pub sidebar_item_fold_icon_size_px: f32,
    pub sidebar_item_fold_icon_color: Hsla,
    // File tree item — file/folder icon
    pub sidebar_dir_icon_size_px: f32,
    pub sidebar_item_icon_size_px: f32,
    pub sidebar_item_icon_color: Hsla,
    pub sidebar_item_selected_icon_color: Hsla,
    pub sidebar_item_badge_min_width_px: f32,
    pub sidebar_item_badge_height_px: f32,
    pub sidebar_item_badge_padding_x_px: f32,
    pub sidebar_item_badge_radius_px: f32,
    pub sidebar_item_badge_font_size_px: f32,
    pub sidebar_item_badge_bg: Hsla,
    pub sidebar_item_badge_text_color: Hsla,
    // Directory file-count badge (file tree rows, workspace/starred/file list headers)
    pub sidebar_count_badge_min_width_px: f32,
    pub sidebar_count_badge_height_px: f32,
    pub sidebar_count_badge_padding_x_px: f32,
    pub sidebar_count_badge_radius_px: f32,
    pub sidebar_count_badge_font_size_px: f32,
    pub sidebar_count_badge_margin_left_px: f32,
    pub sidebar_count_badge_bg: Hsla,
    pub sidebar_count_badge_text_color: Hsla,
    // File tree item — label
    pub sidebar_item_font_size_px: f32,
    pub sidebar_item_text_color: Hsla,
    pub sidebar_item_selected_text_color: Hsla,
    pub sidebar_item_git_added_text_color: Hsla,
    pub sidebar_item_git_modified_text_color: Hsla,
    pub sidebar_item_suffix_opacity: f32,
    // File tree item — active file dot
    pub sidebar_item_active_dot_size_px: f32,
    pub sidebar_item_active_dot_color: Hsla,
    // Inline edit row
    pub sidebar_edit_input_height_px: f32,
    pub sidebar_edit_input_padding_x_px: f32,
    pub sidebar_edit_input_bg: Hsla,
    pub sidebar_edit_input_border_width_px: f32,
    pub sidebar_edit_input_border_color: Hsla,
    pub sidebar_edit_input_radius_px: f32,
    pub sidebar_edit_input_font_size_px: f32,
    pub sidebar_edit_input_cursor_height_ratio: f32,
    pub sidebar_edit_input_text_color: Hsla,
    pub sidebar_edit_input_placeholder_color: Hsla,
    // Error message
    pub sidebar_error_padding_x_px: f32,
    pub sidebar_error_padding_y_px: f32,
    pub sidebar_error_font_size_px: f32,
    pub sidebar_error_text_color: Hsla,
    // Empty state
    pub sidebar_empty_gap_px: f32,
    pub sidebar_empty_padding_x_px: f32,
    pub sidebar_empty_font_size_px: f32,
    pub sidebar_empty_text_color: Hsla,
    pub sidebar_empty_btn_padding_x_px: f32,
    pub sidebar_empty_btn_padding_y_px: f32,
    pub sidebar_empty_btn_border_width_px: f32,
    pub sidebar_empty_btn_border_color: Hsla,
    pub sidebar_empty_btn_font_size_px: f32,
    pub sidebar_empty_btn_text_color: Hsla,
    pub sidebar_empty_btn_hover_bg: Hsla,
    // Resize handle
    pub sidebar_resize_handle_width_px: f32,
    pub sidebar_resize_indicator_width_px: f32,
    pub sidebar_resize_active_indicator_width_px: f32,
    pub sidebar_resize_indicator_color: Hsla,
    pub sidebar_resize_active_indicator_color: Hsla,
    pub sidebar_resize_min_width_px: f32,
    pub sidebar_resize_max_width_px: f32,
    pub sidebar_visibility_animation_ms: u64,
    // Indent alignment guides
    pub sidebar_indent_guide_color: Hsla,
    pub sidebar_indent_guide_width_px: f32,
    pub sidebar_indent_guide_dash_length_px: f32,
    pub sidebar_indent_guide_gap_length_px: f32,
    // Focus ring
    pub sidebar_item_focus_border_color: Hsla,
    // Active border (selected item)
    pub sidebar_item_active_border_color: Hsla,
    // File list pane (to the right of the file tree)
    pub file_list_width_px: f32,
    pub file_list_bg: Hsla,
    pub file_list_header_height_px: f32,
    pub file_list_header_padding_left_px: f32,
    pub file_list_header_padding_right_px: f32,
    pub file_list_header_gap_px: f32,
    pub file_list_header_font_size_px: f32,
    pub file_list_header_text_color: Hsla,
    pub file_list_header_border_width_px: f32,
    pub file_list_header_border_color: Hsla,
    pub file_list_item_padding_x_px: f32,
    pub file_list_item_padding_y_px: f32,
    pub file_list_item_gap_px: f32,
    pub file_list_item_title_gap_px: f32,
    pub file_list_item_title_font_size_px: f32,
    pub file_list_item_title_line_height_px: f32,
    pub file_list_item_title_color: Hsla,
    pub file_list_item_selected_title_color: Hsla,
    pub file_list_item_preview_font_size_px: f32,
    pub file_list_item_preview_line_height_px: f32,
    pub file_list_item_preview_max_lines: usize,
    pub file_list_item_preview_color: Hsla,
    pub file_list_item_time_font_size_px: f32,
    pub file_list_item_time_line_height_px: f32,
    pub file_list_item_time_color: Hsla,
    pub file_list_item_thumbnail_gap_px: f32,
    pub file_list_item_thumbnail_radius_px: f32,
    pub file_list_item_thumbnail_border_width_px: f32,
    pub file_list_item_thumbnail_border_color: Hsla,
    pub file_list_item_bg: Hsla,
    pub file_list_item_hover_bg: Hsla,
    pub file_list_item_selected_bg: Hsla,
    pub file_list_item_separator_width_px: f32,
    pub file_list_item_separator_color: Hsla,
    pub file_list_empty_font_size_px: f32,
    pub file_list_empty_text_color: Hsla,
    pub file_list_resize_min_width_px: f32,
    pub file_list_resize_max_width_px: f32,
    // Sidebar / file list overlay scrollbar (chrome palette so it stays
    // visible on the dark sidebar even in light mode)
    pub sidebar_scrollbar_thumb_color: Hsla,
}

// ── Outline ─────────────────────────────────────────────────────────

pub struct OutlineStyles {
    // Rail container (overlay on the editor's right edge)
    pub outline_rail_right_margin_px: f32,
    pub outline_rail_vertical_padding_px: f32,
    // Heading label
    pub outline_rail_label_font_size_px: f32,
    pub outline_rail_label_max_width_px: f32,
    pub outline_rail_label_indent_step_px: f32,
    pub outline_rail_label_indent_guide_width_px: f32,
    pub outline_rail_label_indent_guide_dash_length_px: f32,
    pub outline_rail_label_indent_guide_gap_length_px: f32,
    pub outline_rail_label_indent_guide_color: Hsla,
    pub outline_rail_label_fold_icon_size_px: f32,
    pub outline_rail_label_fold_icon_gap_px: f32,
    pub outline_rail_label_gap_px: f32,
    pub outline_rail_label_item_height_px: f32,
    pub outline_rail_label_item_gap_px: f32,
    pub outline_rail_label_item_padding_y_px: f32,
    pub outline_rail_label_line_height_px: f32,
    pub outline_rail_label_wrap_chars_per_line: usize,
    pub outline_rail_label_max_lines: usize,
    pub outline_rail_label_color: Hsla,
    pub outline_rail_label_hover_color: Hsla,
    pub outline_rail_label_active_color: Hsla,
    pub outline_rail_tick_level_opacities: [f32; 6],
    pub outline_rail_label_level_opacities: [f32; 6],
    pub outline_rail_label_level_brightness_mix: [f32; 6],
    // Row metrics
    pub outline_rail_row_height_px: f32,
    // Tick indicator
    pub outline_rail_tick_width_px: f32,
    pub outline_rail_tick_thickness_px: f32,
    pub outline_rail_tick_color: Hsla,
    pub outline_rail_tick_active_color: Hsla,
    pub outline_rail_tick_active_extra_width_px: f32,
    pub outline_rail_tick_active_neighbor_count: usize,
    pub outline_rail_tick_active_falloff_power: f32,
    pub outline_rail_tick_active_animation_seconds: f32,
    /// Distance (px) over which ticks fade out near the top and bottom edges of
    /// the visible rail viewport, and the hover highlight fade-in duration.
    pub outline_rail_edge_fade_px: f32,
    pub outline_rail_hover_animation_seconds: f32,
    // Flash highlight after clicking a heading
    pub outline_flash_initial_delay_ms: u64,
    pub outline_flash_show_duration_ms: u64,
    pub outline_flash_gap_duration_ms: u64,
}

// ── Utility Panel ───────────────────────────────────────────────────

pub struct UtilityPanelStyles {
    pub utility_panel_width_px: f32,
    pub utility_panel_bg: Hsla,
    pub utility_panel_terminal_padding_px: f32,
    pub utility_panel_border_width_px: f32,
    pub utility_panel_border_color: Hsla,
    pub utility_panel_top_spacer_height_px: f32,
    pub utility_panel_bottom_spacer_height_px: f32,
    pub utility_panel_header_height_px: f32,
    pub utility_panel_header_padding_x_px: f32,
    pub utility_panel_header_font_size_px: f32,
    pub utility_panel_header_text_color: Hsla,
    pub utility_panel_section_gap_px: f32,
    pub utility_panel_section_padding_x_px: f32,
    pub utility_panel_section_title_font_size_px: f32,
    pub utility_panel_section_title_text_color: Hsla,
    pub utility_panel_item_height_px: f32,
    pub utility_panel_item_gap_px: f32,
    pub utility_panel_item_font_size_px: f32,
    pub utility_panel_item_label_text_color: Hsla,
    pub utility_panel_item_value_text_color: Hsla,
    pub utility_panel_terminal_inactive_opacity: f32,
    pub utility_panel_button_height_px: f32,
    pub utility_panel_button_gap_px: f32,
    pub utility_panel_button_padding_x_px: f32,
    pub utility_panel_button_radius_px: f32,
    pub utility_panel_button_border_width_px: f32,
    pub utility_panel_button_border_color: Hsla,
    pub utility_panel_button_text_color: Hsla,
    pub utility_panel_button_hover_bg: Hsla,
    pub utility_panel_empty_font_size_px: f32,
    pub utility_panel_empty_text_color: Hsla,
    pub utility_panel_resize_handle_width_px: f32,
    pub utility_panel_resize_indicator_width_px: f32,
    pub utility_panel_resize_active_indicator_width_px: f32,
    pub utility_panel_resize_indicator_color: Hsla,
    pub utility_panel_resize_active_indicator_color: Hsla,
    pub utility_panel_resize_min_width_px: f32,
    pub utility_panel_resize_max_width_px: f32,
    pub utility_panel_visibility_animation_ms: u64,
}

// ── Tab Bar ─────────────────────────────────────────────────────────

pub struct TabBarStyles {
    // Container
    pub tab_bar_height_px: f32,
    pub tab_bar_bg: Hsla,
    pub tab_bar_border_width_px: f32,
    pub tab_bar_border_color: Hsla,
    // Tab item
    pub tab_item_min_width_px: f32,
    pub tab_item_max_width_px: f32,
    pub tab_item_padding_left_px: f32,
    pub tab_item_padding_right_px: f32,
    pub tab_item_gap_px: f32,
    pub tab_item_bg: Hsla,
    pub tab_item_hover_bg: Hsla,
    pub tab_item_active_bg: Hsla,
    pub tab_item_border_width_px: f32,
    pub tab_item_border_color: Hsla,
    // Tab item — icon
    pub tab_item_icon_size_px: f32,
    pub tab_action_emphasis_icon_size_px: f32,
    pub tab_item_icon_color: Hsla,
    pub tab_item_icon_active_opacity: f32,
    pub tab_item_icon_inactive_opacity: f32,
    pub tab_item_badge_min_width_px: f32,
    pub tab_item_badge_height_px: f32,
    pub tab_item_badge_padding_x_px: f32,
    pub tab_item_badge_radius_px: f32,
    pub tab_item_badge_font_size_px: f32,
    pub tab_item_badge_bg: Hsla,
    pub tab_item_badge_text_color: Hsla,
    // Tab item — label
    pub tab_item_font_size_px: f32,
    pub tab_item_text_color: Hsla,
    pub tab_item_active_text_color: Hsla,
    pub tab_item_suffix_opacity: f32,
    // Tab item — close button
    pub tab_item_close_size_px: f32,
    pub tab_item_close_font_size_px: f32,
    pub tab_item_close_text_color: Hsla,
    pub tab_item_close_hover_bg: Hsla,
    pub tab_item_close_active_opacity: f32,
    pub tab_item_close_inactive_opacity: f32,
    // Tab dirty indicator
    pub tab_item_dirty_dot_size_px: f32,
    pub tab_item_dirty_dot_color: Hsla,
    // Tab deleted indicator
    pub tab_item_deleted_text_color: Hsla,
    // New-tab button
    pub tab_new_width_px: f32,
    pub tab_new_font_size_px: f32,
    pub tab_new_text_color: Hsla,
    pub tab_new_hover_text_color: Hsla,
    // Fixed trailing drop target after the new-tab button
    pub tab_end_drop_target_width_px: f32,
    // Update button
    pub tab_update_button_height_inset_px: f32,
    pub tab_update_button_min_height_px: f32,
    pub tab_update_button_padding_x_px: f32,
    pub tab_update_button_gap_px: f32,
    pub tab_update_button_radius_px: f32,
    pub tab_update_button_opacity: f32,
    pub tab_update_button_hover_opacity: f32,
    pub tab_update_button_icon_size_px: f32,
    pub tab_update_button_font_size_px: f32,
    pub tab_update_button_text_color: Hsla,
    pub tab_update_button_hover_text_color: Hsla,
    pub tab_update_button_icon_color: Hsla,
    // Tab drag ghost
    pub tab_drag_ghost_opacity: f32,
    pub tab_drag_source_opacity: f32,
    // Tab drop indicator
    pub tab_drop_indicator_width_px: f32,
    pub tab_drop_indicator_color: Hsla,
    // Tab pin
    pub tab_pin_icon_size_px: f32,
    pub tab_pin_icon_color: Hsla,
    pub tab_pinned_width_px: f32,
    // Tab overflow indicator
    pub tab_overflow_font_size_px: f32,
    pub tab_overflow_text_color: Hsla,
    pub tab_overflow_bg: Hsla,
    pub tab_overflow_padding_x_px: f32,
    pub tab_history_hidden_sidebar_margin_px: f32,
    pub tab_history_padding_x_px: f32,
    pub tab_history_gap_px: f32,
    pub tab_history_group_border_width_px: f32,
    pub tab_history_group_border_color: Hsla,
    pub tab_history_btn_size_px: f32,
    pub tab_history_btn_radius_px: f32,
    pub tab_history_btn_bg: Hsla,
    pub tab_history_btn_icon_size_px: f32,
    pub tab_history_btn_icon_color: Hsla,
    pub tab_history_btn_hover_icon_color: Hsla,
    pub tab_history_btn_icon_disabled_color: Hsla,
    pub tab_history_btn_enabled_opacity: f32,
    pub tab_history_btn_disabled_opacity: f32,
    pub tab_action_button_padding_x_px: f32,
}

// ── Toolbar ─────────────────────────────────────────────────────────

pub struct ToolbarStyles {
    pub toolbar_offset_left_px: f32,
    pub toolbar_offset_top_px: f32,
    pub toolbar_gap_px: f32,
    pub toolbar_hidden_opacity: f32,
    pub toolbar_opacity_animation_ms: u64,
    pub toolbar_reveal_hover_height_px: f32,
    pub toolbar_corner_reveal_width_px: f32,
    pub toolbar_corner_reveal_height_px: f32,
    pub toolbar_primary_btn_size_px: f32,
    pub toolbar_primary_icon_size_px: f32,
    pub toolbar_primary_hover_opacity: f32,
    pub toolbar_secondary_btn_size_px: f32,
    pub toolbar_secondary_icon_size_px: f32,
    pub toolbar_secondary_btn_padding_x_px: f32,
    pub toolbar_secondary_hover_opacity: f32,
    pub toolbar_btn_shadow: bool,
    pub toolbar_btn_bg: Hsla,
    pub toolbar_btn_radius_px: f32,
    pub toolbar_whiteboard_bottom_px: f32,
    pub toolbar_whiteboard_gap_px: f32,
    pub toolbar_whiteboard_padding_px: f32,
    pub toolbar_whiteboard_btn_size_px: f32,
    pub toolbar_whiteboard_icon_size_px: f32,
    pub toolbar_whiteboard_border_width_px: f32,
    pub toolbar_whiteboard_shadow: bool,
    pub toolbar_whiteboard_bg: Hsla,
    pub toolbar_whiteboard_border_color: Hsla,
    pub toolbar_whiteboard_btn_bg: Hsla,
    pub toolbar_whiteboard_btn_active_bg: Hsla,
    pub toolbar_whiteboard_btn_hover_bg: Hsla,
    pub toolbar_whiteboard_icon_color: Hsla,
    pub toolbar_whiteboard_icon_active_color: Hsla,
}

/// Hover and active states shift icon contrast only; the bar draws no button
/// backgrounds.
pub struct FormatToolbarStyles {
    pub format_toolbar_height_px: f32,
    pub format_toolbar_padding_x_px: f32,
    /// Pulls the row back by the transparent margin baked into the icons, so
    /// the first glyph lines up optically with the body text.
    pub format_toolbar_optical_left_inset_px: f32,
    pub format_toolbar_gap_px: f32,
    pub format_toolbar_group_gap_px: f32,
    pub format_toolbar_bg: Hsla,
    pub format_toolbar_border_width_px: f32,
    pub format_toolbar_border_color: Hsla,
    pub format_toolbar_btn_size_px: f32,
    pub format_toolbar_icon_size_px: f32,
    pub format_toolbar_icon_color: Hsla,
    pub format_toolbar_icon_hover_color: Hsla,
    pub format_toolbar_icon_active_color: Hsla,
    pub format_toolbar_separator_width_px: f32,
    pub format_toolbar_separator_height_px: f32,
    pub format_toolbar_separator_margin_x_px: f32,
    pub format_toolbar_separator_color: Hsla,
}

// ── Whiteboard ─────────────────────────────────────────────────────

pub struct WhiteboardStyles {
    pub whiteboard_swatch_size_px: f32,
    pub whiteboard_swatch_ring_width_px: f32,
    pub whiteboard_swatch_ring_gap_px: f32,
    pub whiteboard_swatch_checker_cell_px: f32,
    pub whiteboard_swatch_grid_gap_y_px: f32,
    pub whiteboard_swatch_ring_color: Hsla,
    pub whiteboard_swatch_ring_idle_color: Hsla,
    pub whiteboard_swatch_checker_light_color: Hsla,
    pub whiteboard_swatch_checker_dark_color: Hsla,
    pub whiteboard_render_cull_margin_px: f32,
    pub whiteboard_fill_opacity_levels: [f32; 5],
}

// ── Tooltip ─────────────────────────────────────────────────────────

pub struct TooltipStyles {
    pub tooltip_delay_ms: u64,
    pub tooltip_viewport_padding_px: f32,
    pub tooltip_offset_px: f32,
    pub tooltip_max_width_px: f32,
    pub tooltip_padding_x_px: f32,
    pub tooltip_padding_y_px: f32,
    pub tooltip_gap_px: f32,
    pub tooltip_radius_px: f32,
    pub tooltip_border_width_px: f32,
    pub tooltip_bg: Hsla,
    pub tooltip_border_color: Hsla,
    pub tooltip_title_font_size_px: f32,
    pub tooltip_title_text_color: Hsla,
    pub tooltip_subtitle_font_size_px: f32,
    pub tooltip_subtitle_text_color: Hsla,
    pub tooltip_subtitle_opacity: f32,
    pub tooltip_shortcut_gap_px: f32,
    pub tooltip_shortcut_key_min_width_px: f32,
    pub tooltip_shortcut_key_height_px: f32,
    pub tooltip_shortcut_key_padding_x_px: f32,
    pub tooltip_shortcut_key_radius_px: f32,
    pub tooltip_shortcut_key_bg: Hsla,
}

// ── Cursor ──────────────────────────────────────────────────────────

pub struct CursorStyles {
    pub cursor_color: Hsla,
    pub cursor_width_px: f32,
    pub cursor_height_factor: f32,
    pub cursor_blink_pause_ms: u64,
    pub cursor_blink_visible_ms: u64,
    pub cursor_blink_transparent_ms: u64,
    pub cursor_opacity_min: f32,
    pub cursor_opacity_max: f32,
}

impl CursorStyles {
    pub fn blink_pause_delay(&self) -> Duration {
        Duration::from_millis(self.cursor_blink_pause_ms)
    }
    pub fn blink_visible_duration(&self) -> Duration {
        Duration::from_millis(self.cursor_blink_visible_ms)
    }
    pub fn blink_transparent_duration(&self) -> Duration {
        Duration::from_millis(self.cursor_blink_transparent_ms)
    }
}

// ── Code Block ──────────────────────────────────────────────────────

pub struct CodeBlockStyles {
    pub code_block_bg: Hsla,
    pub code_block_border_width_px: f32,
    pub code_block_border_color: Hsla,
    /// Brighter divider drawn between the source (left) and rendered (right)
    /// panes in a generated preview's edit view.
    pub code_block_split_border_color: Hsla,
    /// Width of the edit/render split divider and its dash segment length and
    /// gap (the divider is rendered as a dashed line).
    pub code_block_split_border_width_px: f32,
    pub code_block_split_border_dash_length_px: f32,
    pub code_block_split_border_dash_gap_px: f32,
    pub code_block_radius_px: f32,
    pub code_block_fill_inset_px: f32,
    pub code_block_text_padding_left_px: f32,
    pub code_block_line_number_padding_left_px: f32,
    pub code_block_wrap_right_padding_px: f32,
    pub code_block_horizontal_scrollbar_margin_x_px: f32,
    pub code_block_fence_clip_px: f32,
    pub code_block_copy_button_top_px: f32,
    pub code_block_copy_button_right_px: f32,
    pub code_block_copy_button_padding_x_px: f32,
    pub code_block_copy_button_padding_y_px: f32,
    pub code_block_copy_button_radius_px: f32,
    pub code_block_copy_button_border_width_px: f32,
    pub code_block_copy_button_font_size_px: f32,
    pub code_block_copy_button_bg: Hsla,
    pub code_block_copy_button_text_color: Hsla,
    pub code_block_copy_button_border_color: Hsla,
    pub code_block_copy_button_hover_bg: Hsla,
    pub code_block_copy_button_hover_text_color: Hsla,
    pub code_block_copy_button_hover_border_color: Hsla,
    pub code_block_copy_button_copied_bg: Hsla,
    pub code_block_copy_button_copied_text_color: Hsla,
    pub code_block_copy_button_copied_border_color: Hsla,
    pub code_block_display_action_button_size_px: f32,
    pub code_block_display_action_icon_size_px: f32,
    pub code_block_display_action_gap_px: f32,
    pub generated_preview_action_padding_px: f32,
    pub generated_preview_action_inner_padding_x_px: f32,
    pub generated_preview_action_gap_px: f32,
    pub generated_preview_action_button_size_px: f32,
    pub generated_preview_action_icon_size_px: f32,
    pub generated_preview_action_button_radius_px: f32,
    pub generated_preview_action_border_width_px: f32,
    pub generated_preview_action_border_color: Hsla,
    pub generated_preview_min_height_px: f32,
    pub generated_preview_max_height_px: f32,
    pub generated_preview_padding_x_px: f32,
    pub generated_preview_padding_y_px: f32,
}

// ── Quote Block ─────────────────────────────────────────────────────

pub struct QuoteBlockStyles {
    pub quote_block_border_width_px: f32,
    pub quote_block_radius_px: f32,
    pub quote_block_padding_top_px: f32,
    pub quote_block_padding_bottom_px: f32,
    /// Extra inner vertical padding applied only to callout blocks
    /// (`> [!KIND]`), on top of the plain quote padding.
    pub quote_block_callout_padding_top_px: f32,
    pub quote_block_callout_padding_bottom_px: f32,
    pub quote_block_padding_left_px: f32,
    /// Extra indent for quoted lines that carry their own line-level marker
    /// (list, ordered list, task), so the nesting reads as one step deeper.
    pub quote_block_line_syntax_indent_px: f32,
    pub quote_block_padding_right_px: f32,
    pub quote_block_callout_icon_color: Hsla,
    pub quote_block_callout_icon_size_factor: f32,
    pub quote_block_callout_icon_min_px: f32,
    pub quote_block_callout_icon_max_line_height_factor: f32,
    pub quote_block_callout_icon_label_gap_px: f32,
    pub quote_block_bg: Hsla,
    pub quote_block_border_color: Hsla,
    pub quote_block_note_bg: Hsla,
    pub quote_block_note_border_color: Hsla,
    pub quote_block_tip_bg: Hsla,
    pub quote_block_tip_border_color: Hsla,
    pub quote_block_important_bg: Hsla,
    pub quote_block_important_border_color: Hsla,
    pub quote_block_warning_bg: Hsla,
    pub quote_block_warning_border_color: Hsla,
    pub quote_block_caution_bg: Hsla,
    pub quote_block_caution_border_color: Hsla,
}

// ── Wiki Link ───────────────────────────────────────────────────────

pub struct WikiLinkStyles {
    pub wiki_link_text_color: Hsla,
    pub wiki_link_text_weight: FontWeight,
}

// ── Selection & Line Highlight ──────────────────────────────────────

pub struct SelectionStyles {
    pub selection_color: Hsla,
    pub selection_line_highlight_color: Hsla,
    pub selection_diff_added_line_bg: Hsla,
    pub selection_diff_removed_line_bg: Hsla,
    pub selection_diff_added_inline_bg: Hsla,
    pub selection_diff_removed_inline_bg: Hsla,
    pub selection_diff_added_inline_border_color: Hsla,
    pub selection_diff_removed_inline_border_color: Hsla,
    pub selection_diff_inline_border_width_px: f32,
    pub selection_diff_revert_button_padding_x_px: f32,
    pub selection_diff_revert_button_padding_y_px: f32,
    pub selection_diff_revert_button_radius_px: f32,
    pub selection_diff_revert_button_border_width_px: f32,
    pub selection_diff_revert_button_gap_y_px: f32,
    pub selection_diff_revert_button_icon_size_px: f32,
    pub selection_diff_revert_button_icon_gap_px: f32,
    pub selection_diff_revert_button_font_size_px: f32,
    pub selection_diff_revert_button_bg: Hsla,
    pub selection_diff_revert_button_text_color: Hsla,
    pub selection_diff_revert_button_border_color: Hsla,
    pub selection_diff_revert_button_hover_bg: Hsla,
    pub selection_diff_revert_button_hover_text_color: Hsla,
    pub selection_diff_revert_button_hover_border_color: Hsla,
    /// Left-edge indicator bars for in-place (working-tree) diffs, drawn as a
    /// spaced diagonal-hatch pattern flush against the editor's left edge.
    pub selection_diff_added_bar_color: Hsla,
    pub selection_diff_removed_bar_color: Hsla,
    pub selection_diff_bar_width_px: f32,
    /// Thickness of each diagonal stripe and the gap between consecutive stripes.
    pub selection_diff_bar_stripe_thickness_px: f32,
    pub selection_diff_bar_stripe_spacing_px: f32,
    /// Bar brightness (opacity) when the hunk is collapsed vs expanded.
    pub selection_diff_bar_collapsed_opacity: f32,
    pub selection_diff_bar_expanded_opacity: f32,
    pub selection_trailing_width_factor: f32,
}

// ── Checkbox ────────────────────────────────────────────────────────

pub struct CheckboxStyles {
    pub checkbox_size_factor: f32,
    pub checkbox_text_gap_px: f32,
    pub checkbox_parent_prefix_chars: f32,
    pub checkbox_radius_px: f32,
    pub checkbox_border_width_px: f32,
    pub checkbox_border_color: Hsla,
    pub checkbox_checked_bg: Hsla,
    pub checkbox_check_icon_scale: f32,
    pub checkbox_check_icon_color: Hsla,
    pub checkbox_default_opacity: f32,
    pub checkbox_hover_opacity: f32,
    pub checkbox_hit_padding_px: f32,
}

// ── Table ───────────────────────────────────────────────────────────

pub struct TableStyles {
    pub table_cell_padding_px: f32,
    pub table_border_width_px: f32,
    pub table_border_color: Hsla,
    pub table_outer_radius_px: f32,
    pub table_header_bg: Hsla,
    pub front_matter_key_text_color: Hsla,
    pub front_matter_key_padding_right_px: f32,
    pub table_handle_size_px: f32,
    pub table_handle_color: Hsla,
    pub table_handle_dot_size_px: f32,
    pub table_handle_gap_px: f32,
    pub table_handle_highlight_width_px: f32,
    pub table_drag_indicator_width_px: f32,
    pub table_selection_border_width_px: f32,
}

// ── Gutter ──────────────────────────────────────────────────────────

pub struct GutterStyles {
    // Line numbers
    pub gutter_line_number_min_width_px: f32,
    pub gutter_line_number_max_width_px: f32,
    pub gutter_line_number_font_size_px: f32,
    pub gutter_line_number_vertical_offset_px: f32,
    pub gutter_line_number_right_gap_px: f32,
    pub gutter_column_gap_px: f32,
    pub gutter_line_number_text_color: Hsla,
    pub gutter_line_number_active_text_color: Hsla,
    // Fold icon
    pub gutter_fold_icon_size_px: f32,
    pub gutter_fold_icon_offset_px: f32,
    pub gutter_fold_hit_padding_px: f32,
    pub gutter_fold_default_opacity: f32,
    pub gutter_fold_folded_opacity: f32,
    pub gutter_fold_hover_opacity: f32,
    pub gutter_fold_animation_ms: u64,
    pub gutter_fold_folded_color: Hsla,
    pub gutter_fold_hover_color: Hsla,
    // Heading hash icons
    pub gutter_heading_color: Hsla,
    pub gutter_heading_opacity: f32,
    pub gutter_heading_h1_icon_size_px: f32,
    pub gutter_heading_h2_icon_size_px: f32,
    pub gutter_heading_h3_icon_size_px: f32,
    pub gutter_heading_h4_icon_size_px: f32,
    pub gutter_heading_icon_gap_px: f32,
    pub gutter_heading_icon_offset_px: f32,
    pub gutter_heading_max_total_width_px: f32,
}

// ── Image Preview ───────────────────────────────────────────────────

pub struct ImagePreviewStyles {
    pub image_border_radius_px: f32,
    pub image_active_border_width_px: f32,
    pub image_active_border_color: Hsla,
    pub image_gap_top_px: f32,
    pub image_gap_bottom_px: f32,
    pub image_edit_icon_size_px: f32,
    pub image_edit_icon_gap_px: f32,
    pub image_edit_icon_color: Hsla,
    pub image_edit_icon_normal_opacity: f32,
    pub image_edit_icon_hover_opacity: f32,
}

// ── HR (horizontal rule) ────────────────────────────────────────────

pub struct HrStyles {
    pub hr_dash_length_px: f32,
    pub hr_gap_length_px: f32,
    pub hr_height_px: f32,
    pub hr_color: Hsla,
}

// ── IME ─────────────────────────────────────────────────────────────

pub struct ImeStyles {
    pub ime_underline_height_px: f32,
    pub ime_underline_color: Hsla,
}

// ── Dialog ──────────────────────────────────────────────────────────

pub struct DialogStyles {
    pub dialog_backdrop_color: Hsla,
    pub dialog_bg: Hsla,
    pub dialog_border_width_px: f32,
    pub dialog_border_color: Hsla,
    pub dialog_radius_px: f32,
    pub dialog_width_px: f32,
    pub dialog_padding_x_px: f32,
    pub dialog_padding_y_px: f32,
    pub dialog_gap_px: f32,
    pub dialog_title_font_size_px: f32,
    pub dialog_title_text_color: Hsla,
    pub dialog_message_font_size_px: f32,
    pub dialog_message_text_color: Hsla,
    pub dialog_btn_height_px: f32,
    pub dialog_btn_padding_x_px: f32,
    pub dialog_btn_font_size_px: f32,
    pub dialog_btn_radius_px: f32,
    pub dialog_btn_gap_px: f32,
    pub dialog_btn_hover_opacity: f32,
    pub dialog_btn_primary_bg: Hsla,
    pub dialog_btn_primary_text_color: Hsla,
    pub dialog_btn_primary_hover_bg: Hsla,
    pub dialog_btn_secondary_bg: Hsla,
    pub dialog_btn_secondary_text_color: Hsla,
    pub dialog_btn_secondary_hover_bg: Hsla,
    pub dialog_btn_danger_bg: Hsla,
    pub dialog_btn_danger_text_color: Hsla,
    pub dialog_btn_danger_hover_bg: Hsla,
}

// ── Notifications (toast) ───────────────────────────────────────────

pub struct NotificationStyles {
    pub notification_width_px: f32,
    pub notification_margin_right_px: f32,
    pub notification_margin_bottom_px: f32,
    pub notification_gap_px: f32,
    pub notification_padding_x_px: f32,
    pub notification_padding_y_px: f32,
    pub notification_radius_px: f32,
    pub notification_border_width_px: f32,
    pub notification_bg: Hsla,
    pub notification_border_color: Hsla,
    pub notification_text_color: Hsla,
    pub notification_font_size_px: f32,
    pub notification_hover_opacity: f32,
    pub notification_dismiss_after_ms: u64,
    pub notification_max_visible: usize,
}

// ── Context Menu ────────────────────────────────────────────────────

pub struct ContextMenuStyles {
    // Container
    pub menu_bg: Hsla,
    pub menu_border_width_px: f32,
    pub menu_border_color: Hsla,
    pub menu_radius_px: f32,
    pub menu_shadow: bool,
    pub menu_padding_y_px: f32,
    pub menu_submenu_gap_px: f32,
    pub menu_max_height_margin_px: f32,
    pub menu_opacity_animation_ms: u64,
    // Item
    pub menu_item_height_px: f32,
    pub menu_item_padding_x_px: f32,
    pub menu_item_font_size_px: f32,
    pub menu_item_text_color: Hsla,
    pub menu_item_primary_text_color: Hsla,
    pub menu_item_danger_text_color: Hsla,
    pub menu_item_hover_bg: Hsla,
    pub menu_item_selected_bg: Hsla,
    // Separator
    pub menu_separator_height_px: f32,
    pub menu_separator_margin_y_px: f32,
    pub menu_separator_color: Hsla,
    // Shortcut label
    pub menu_shortcut_font_size_px: f32,
    pub menu_shortcut_font_family: &'static str,
    pub menu_shortcut_text_color: Hsla,
    pub menu_shortcut_gap_px: f32,
    pub menu_shortcut_key_min_width_px: f32,
    pub menu_shortcut_key_height_px: f32,
    pub menu_shortcut_key_padding_x_px: f32,
    pub menu_shortcut_key_radius_px: f32,
    pub menu_shortcut_key_bg: Hsla,
    // Format menu icon
    pub menu_format_icon_size_px: f32,
    pub menu_format_icon_gap_px: f32,
    pub menu_format_icon_color: Hsla,
    pub menu_format_active_text_color: Hsla,
    // Widths per menu kind
    pub menu_sidebar_width_px: f32,
    pub menu_format_width_px: f32,
    pub menu_export_width_px: f32,
    pub menu_tab_width_px: f32,
}

// ── Workspace Switcher ──────────────────────────────────────────────

pub struct WorkspaceSwitcherStyles {
    pub ws_bar_bg: Hsla,
    pub ws_bar_border_color: Hsla,
    pub ws_bar_padding_x_px: f32,
    pub ws_bar_gap_px: f32,
    pub ws_bar_hover_bg: Hsla,
    // Workspace menu items
    pub ws_menu_width_px: f32,
    pub ws_menu_item_check_size_px: f32,
    pub ws_menu_item_check_color: Hsla,
    pub ws_menu_item_action_icon_size_px: f32,
    pub ws_menu_item_action_icon_color: Hsla,
    pub ws_menu_item_action_icon_gap_px: f32,
    pub ws_menu_item_close_size_px: f32,
    pub ws_menu_item_close_color: Hsla,
    pub ws_menu_item_close_hover_color: Hsla,
    pub ws_menu_item_close_hover_bg: Hsla,
}

// ── TextInput ───────────────────────────────────────────────────────

#[derive(Clone)]
pub struct TextInputStyles {
    pub text_input_height_px: f32,
    pub text_input_padding_left_px: f32,
    pub text_input_padding_right_px: f32,
    pub text_input_font_size_px: f32,
    pub text_input_bg: Hsla,
    pub text_input_hover_bg: Option<Hsla>,
    pub text_input_focus_bg: Hsla,
    pub text_input_text_color: Hsla,
    pub text_input_placeholder_color: Hsla,
    pub text_input_border_width_px: f32,
    pub text_input_border_color: Hsla,
    pub text_input_focus_border_color: Hsla,
    pub text_input_radius_px: f32,
    pub text_input_cursor_color: Hsla,
    pub text_input_cursor_width_px: f32,
    pub text_input_cursor_height_ratio: f32,
    pub text_input_cursor_height_adjust_px: f32,
    pub text_input_selection_color: Hsla,
    pub text_input_center: bool,
    /// Right-align text within the input (overridden by `text_input_center`).
    pub text_input_right: bool,
    /// Context-menu chrome (independent of the input's own bg/font so styled
    /// inputs like the whiteboard's transparent overlay keep a normal menu).
    /// Values mirror the editor's context-menu styles for visual consistency.
    pub text_input_menu_bg: Hsla,
    pub text_input_menu_text_color: Hsla,
    pub text_input_menu_font_size_px: f32,
    pub text_input_menu_border_width_px: f32,
    pub text_input_menu_border_color: Hsla,
    pub text_input_menu_radius_px: f32,
    pub text_input_menu_padding_y_px: f32,
    pub text_input_menu_item_height_px: f32,
    pub text_input_menu_item_padding_x_px: f32,
    pub text_input_menu_hover_bg: Hsla,
    pub text_input_menu_separator_color: Hsla,
    pub text_input_menu_width_px: f32,
    pub text_input_menu_shadow: bool,
}

// ── Search (in-editor buffer search) ────────────────────────────────

pub struct SearchStyles {
    // Search bar container
    pub search_bar_width_px: f32,
    pub search_bar_padding_px: f32,
    pub search_bar_bg: Hsla,
    pub search_bar_border_width_px: f32,
    pub search_bar_border_color: Hsla,
    pub search_bar_radius_px: f32,
    pub search_bar_shadow: bool,
    pub search_bar_gap_px: f32,
    pub search_bar_offset_top_px: f32,
    pub search_bar_offset_right_px: f32,
    pub search_input_gap_px: f32,
    pub search_input_height_px: f32,
    pub search_input_font_size_px: f32,
    pub search_input_icon_size_px: f32,
    pub search_input_icon_color: Hsla,
    pub search_input_icon_gap_px: f32,
    pub search_input_left_inset_px: f32,
    pub search_input_right_inset_px: f32,
    pub search_input_counter_min_width_px: f32,
    pub search_input_focus_bg: Hsla,
    pub search_input_cursor_height_ratio: f32,
    // Search option buttons
    pub search_option_btn_size_px: f32,
    pub search_option_btn_icon_size_px: f32,
    pub search_option_btn_radius_px: f32,
    pub search_option_btn_active_bg: Hsla,
    pub search_option_btn_hover_bg: Hsla,
    pub search_option_btn_icon_color: Hsla,
    pub search_option_btn_active_icon_color: Hsla,
    // Match counter
    pub search_counter_font_size_px: f32,
    pub search_counter_text_color: Hsla,
    // Navigation buttons
    pub search_nav_btn_size_px: f32,
    pub search_nav_btn_icon_size_px: f32,
    pub search_nav_btn_icon_color: Hsla,
    pub search_nav_btn_hover_bg: Hsla,
    // Match highlighting in editor
    pub search_match_bg: Hsla,
    pub search_match_active_bg: Hsla,
    pub search_match_text_color: Hsla,
    pub search_match_active_text_color: Hsla,
    pub search_match_border_width_px: f32,
    pub search_match_border_color: Hsla,
    // Scrollbar match markers
    pub search_scrollbar_marker_width_px: f32,
    pub search_scrollbar_marker_height_px: f32,
    pub search_scrollbar_marker_color: Hsla,
    // Replace
    pub search_replace_btn_height_px: f32,
    pub search_replace_btn_padding_x_px: f32,
    pub search_replace_btn_font_size_px: f32,
    pub search_replace_btn_bg: Hsla,
    pub search_replace_btn_text_color: Hsla,
    pub search_replace_btn_hover_bg: Hsla,
    // Close button
    pub search_close_btn_size_px: f32,
    pub search_close_btn_icon_color: Hsla,
    pub search_close_btn_hover_bg: Hsla,
}

// ── Workspace Search (sidebar global search) ────────────────────────

pub struct WorkspaceSearchStyles {
    // Search area
    pub ws_search_padding_px: f32,
    pub ws_search_gap_px: f32,
    pub ws_search_input_gap_px: f32,
    pub ws_search_input_left_inset_px: f32,
    pub ws_search_input_right_inset_px: f32,
    pub ws_search_input_icon_size_px: f32,
    pub ws_search_input_icon_color: Hsla,
    pub ws_search_input_icon_gap_px: f32,
    pub ws_search_input_height_px: f32,
    pub ws_search_input_radius_px: f32,
    pub ws_search_input_bg: Hsla,
    pub ws_search_input_hover_bg: Hsla,
    pub ws_search_input_focus_bg: Hsla,
    pub ws_search_input_border_width_px: f32,
    pub ws_search_input_border_color: Hsla,
    pub ws_search_input_cursor_height_ratio: f32,
    pub ws_search_input_cursor_height_adjust_px: f32,
    // File group header (sticky)
    pub ws_search_file_header_height_px: f32,
    pub ws_search_file_header_bg: Hsla,
    pub ws_search_file_header_border_width_px: f32,
    pub ws_search_file_header_border_color: Hsla,
    pub ws_search_file_header_font_size_px: f32,
    pub ws_search_file_header_text_color: Hsla,
    pub ws_search_file_header_dir_gap_px: f32,
    pub ws_search_file_header_dir_text_color: Hsla,
    pub ws_search_file_header_icon_size_px: f32,
    pub ws_search_file_header_count_font_size_px: f32,
    pub ws_search_file_header_count_text_color: Hsla,
    // Match line item
    pub ws_search_match_height_px: f32,
    pub ws_search_match_padding_x_px: f32,
    pub ws_search_match_font_size_px: f32,
    pub ws_search_match_text_color: Hsla,
    pub ws_search_match_highlight_bg: Hsla,
    pub ws_search_match_highlight_text_color: Hsla,
    pub ws_search_match_line_number_color: Hsla,
    pub ws_search_match_hover_bg: Hsla,
    pub ws_search_match_active_bg: Hsla,
    // Progress
    pub ws_search_progress_height_px: f32,
    pub ws_search_progress_color: Hsla,
    // Summary
    pub ws_search_summary_height_px: f32,
    pub ws_search_summary_margin_bottom_px: f32,
    pub ws_search_summary_font_size_px: f32,
    pub ws_search_summary_text_color: Hsla,
    // Sidebar search icon button
    pub ws_search_btn_size_px: f32,
    pub ws_search_btn_icon_size_px: f32,
    pub ws_search_btn_icon_color: Hsla,
    pub ws_search_btn_active_icon_color: Hsla,
    pub ws_search_btn_hover_bg: Hsla,
    pub ws_search_btn_active_bg: Hsla,
}

// ── Quick Open ─────────────────────────────────────────────────────

pub struct QuickOpenStyles {
    pub quick_open_backdrop_bg: Hsla,
    pub quick_open_panel_width_px: f32,
    pub quick_open_panel_top_offset_px: f32,
    pub quick_open_panel_radius_px: f32,
    pub quick_open_panel_border_color: Hsla,
    pub quick_open_panel_bg: Hsla,
    pub quick_open_input_padding_x_px: f32,
    pub quick_open_input_padding_y_px: f32,
    pub quick_open_input_text_padding_left_px: f32,
    pub quick_open_input_bg: Hsla,
    pub quick_open_input_focus_bg: Hsla,
    pub quick_open_input_text_color: Hsla,
    pub quick_open_input_cursor_height_ratio: f32,
    pub quick_open_input_cursor_height_adjust_px: f32,
    pub quick_open_results_max_height_px: f32,
    pub quick_open_result_row_height_px: f32,
    pub quick_open_result_padding_x_px: f32,
    pub quick_open_result_gap_y_px: f32,
    pub quick_open_result_name_font_size_px: f32,
    pub quick_open_result_path_font_size_px: f32,
    pub quick_open_result_hover_bg: Hsla,
    pub quick_open_result_selected_bg: Hsla,
    pub quick_open_result_text_color: Hsla,
    pub quick_open_result_secondary_text_color: Hsla,
    pub quick_open_result_shortcut_bg: Hsla,
    pub quick_open_result_match_text_color: Hsla,
    pub quick_open_empty_height_px: f32,
    pub quick_open_empty_font_size_px: f32,
    pub quick_open_empty_text_color: Hsla,
}

// ── Settings Panel ─────────────────────────────────────────────────

pub struct SettingsPanelStyles {
    pub settings_title_version_gap_px: f32,
    pub settings_title_version_font_size_px: f32,
    pub settings_title_version_text_color: Hsla,
    pub settings_title_version_margin_bottom_px: f32,
    pub settings_primary_text_color: Hsla,
    pub settings_secondary_text_color: Hsla,
    pub settings_header_gap_px: f32,
    pub settings_header_actions_gap_px: f32,
    pub settings_header_button_max_width_px: f32,
    pub settings_site_button_height_px: f32,
    pub settings_site_button_padding_x_px: f32,
    pub settings_site_button_radius_px: f32,
    pub settings_site_button_border_width_px: f32,
    pub settings_site_button_gap_px: f32,
    pub settings_site_button_icon_size_px: f32,
    pub settings_site_button_font_size_px: f32,
    pub settings_site_button_text_color: Hsla,
    pub settings_site_button_icon_color: Hsla,
    pub settings_site_button_update_text_color: Hsla,
    pub settings_site_button_update_icon_color: Hsla,
    pub settings_site_button_bg: Hsla,
    pub settings_site_button_hover_bg: Hsla,
    pub settings_site_button_border_color: Hsla,
    pub settings_option_button_height_px: f32,
    pub settings_option_button_padding_x_px: f32,
    pub settings_option_button_radius_px: f32,
    pub settings_option_button_border_width_px: f32,
    pub settings_option_button_gap_px: f32,
    pub settings_option_button_font_size_px: f32,
    pub settings_option_button_text_color: Hsla,
    pub settings_option_button_active_text_color: Hsla,
    pub settings_option_button_bg: Hsla,
    pub settings_option_button_hover_bg: Hsla,
    pub settings_option_button_active_bg: Hsla,
    pub settings_option_button_border_color: Hsla,
    pub settings_option_button_active_border_color: Hsla,
    pub settings_contrast_border_color: Hsla,
    pub settings_input_height_px: f32,
    pub settings_input_radius_px: f32,
    pub settings_stepper_input_radius_px: f32,
    pub settings_theme_group_gap_px: f32,
    pub settings_theme_toggle_height_px: f32,
    pub settings_theme_toggle_padding_x_px: f32,
    pub settings_theme_toggle_gap_px: f32,
    pub settings_theme_toggle_radius_px: f32,
    pub settings_theme_toggle_border_width_px: f32,
    pub settings_theme_toggle_icon_size_px: f32,
    pub settings_theme_option_height_px: f32,
    pub settings_theme_option_padding_x_px: f32,
    pub settings_theme_option_radius_px: f32,
    pub settings_theme_option_border_width_px: f32,
    pub settings_theme_option_font_size_px: f32,
    pub settings_theme_option_hover_bg: Hsla,
    pub settings_theme_option_active_bg: Hsla,
    pub settings_theme_option_active_text_color: Hsla,
    pub settings_theme_option_text_color: Hsla,
    pub settings_theme_option_border_color: Hsla,
}

// ═════════════════════════════════════════════════════════════════════
// Construction
// ═════════════════════════════════════════════════════════════════════

impl Styles {
    /// Re-map all chrome to the same-appearance palette used by the active
    /// Moeka retro theme and apply their shared two-pixel component borders.
    pub(crate) fn apply_moeka_retro_chrome(&mut self, editor: &Theme, chrome: &Theme) {
        let border_width = 2.0;
        let print_surface_text = editor.text_primary;
        let print_surface_text_muted = editor.text_secondary;

        self.window.window_border_width_px = border_width;
        self.window.window_border_color = chrome.window_border;

        self.editor.editor_path_bar_border_width_px = border_width;
        self.editor.editor_match_border_width_px = border_width;

        self.format_toolbar.format_toolbar_border_width_px = border_width;
        self.format_toolbar.format_toolbar_border_color = chrome.border_primary;
        self.format_toolbar.format_toolbar_separator_color = chrome.border_primary;

        self.sidebar.sidebar_edit_input_border_width_px = border_width;
        self.sidebar.sidebar_empty_btn_border_width_px = border_width;
        self.sidebar.file_list_header_border_width_px = border_width;
        self.sidebar.file_list_item_separator_width_px = border_width;
        self.sidebar.file_list_item_thumbnail_border_width_px = border_width;
        self.sidebar.file_list_item_thumbnail_border_color = chrome.border_primary;
        self.sidebar.sidebar_resize_indicator_width_px = border_width;
        self.sidebar.sidebar_resize_active_indicator_width_px = border_width;

        self.tab_bar.tab_bar_bg = chrome.bg_bar;
        self.tab_bar.tab_bar_border_width_px = border_width;
        self.tab_bar.tab_bar_border_color = chrome.border_primary;
        self.tab_bar.tab_item_bg = chrome.bg_tab;
        self.tab_bar.tab_item_hover_bg = chrome.bg_tab_hover;
        self.tab_bar.tab_item_active_bg = chrome.bg_tab_active;
        self.tab_bar.tab_item_border_width_px = border_width;
        self.tab_bar.tab_item_border_color = chrome.border_primary;
        self.tab_bar.tab_item_icon_color = chrome.text_secondary;
        self.tab_bar.tab_item_badge_bg = with_alpha(chrome.text_secondary, 0.24);
        self.tab_bar.tab_item_badge_text_color = chrome.text_primary;
        self.tab_bar.tab_item_text_color = chrome.text_primary;
        self.tab_bar.tab_item_active_text_color = chrome.text_bright;
        self.tab_bar.tab_item_close_text_color = chrome.text_primary;
        self.tab_bar.tab_item_close_hover_bg = chrome.bg_hover;
        self.tab_bar.tab_item_dirty_dot_color = chrome.text_accent;
        self.tab_bar.tab_item_deleted_text_color = chrome.text_secondary;
        self.tab_bar.tab_new_text_color = chrome.text_secondary;
        self.tab_bar.tab_new_hover_text_color = chrome.text_bright;
        self.tab_bar.tab_drop_indicator_color = chrome.text_accent;
        self.tab_bar.tab_pin_icon_color = chrome.text_secondary;
        self.tab_bar.tab_overflow_text_color = chrome.text_secondary;
        self.tab_bar.tab_overflow_bg = chrome.bg_tab;
        self.tab_bar.tab_history_group_border_width_px = border_width;
        self.tab_bar.tab_history_group_border_color = chrome.border_primary;
        self.tab_bar.tab_history_btn_bg = chrome.bg_tab;
        self.tab_bar.tab_history_btn_icon_color = chrome.text_primary;
        self.tab_bar.tab_history_btn_hover_icon_color = chrome.text_bright;
        self.tab_bar.tab_history_btn_icon_disabled_color = chrome.text_secondary;

        self.utility_panel.utility_panel_bg = chrome.bg_secondary;
        self.utility_panel.utility_panel_border_width_px = border_width;
        self.utility_panel.utility_panel_border_color = chrome.border_primary;
        self.utility_panel.utility_panel_header_text_color = chrome.text_primary;
        self.utility_panel.utility_panel_section_title_text_color = chrome.text_primary;
        self.utility_panel.utility_panel_item_label_text_color = chrome.text_secondary;
        self.utility_panel.utility_panel_item_value_text_color = chrome.text_primary;
        self.utility_panel.utility_panel_button_border_width_px = border_width;
        self.utility_panel.utility_panel_button_border_color = chrome.border_primary;
        self.utility_panel.utility_panel_button_text_color = chrome.text_primary;
        self.utility_panel.utility_panel_button_hover_bg = chrome.bg_hover;
        self.utility_panel.utility_panel_empty_text_color = chrome.text_secondary;
        self.utility_panel.utility_panel_resize_indicator_width_px = border_width;
        self.utility_panel
            .utility_panel_resize_active_indicator_width_px = border_width;
        self.utility_panel.utility_panel_resize_indicator_color = chrome.border_primary;
        self.utility_panel
            .utility_panel_resize_active_indicator_color = chrome.text_accent;

        self.toolbar.toolbar_whiteboard_border_width_px = border_width;
        self.tooltip.tooltip_border_width_px = border_width;
        self.code_block.code_block_border_width_px = border_width;
        self.code_block.code_block_split_border_width_px = border_width;
        self.checkbox.checkbox_border_width_px = border_width;
        self.code_block.code_block_copy_button_border_width_px = border_width;
        self.code_block.generated_preview_action_border_width_px = border_width;
        self.quote_block.quote_block_border_width_px = border_width;
        self.selection.selection_diff_revert_button_border_width_px = border_width;
        self.table.table_border_width_px = border_width;
        self.table.table_selection_border_width_px = border_width;
        self.image_preview.image_active_border_width_px = border_width;
        self.dialog.dialog_border_width_px = border_width;
        self.notification.notification_border_width_px = border_width;
        self.context_menu.menu_border_width_px = border_width;
        self.text_input.text_input_border_width_px = border_width;
        self.search.search_bar_border_width_px = border_width;
        self.workspace_search.ws_search_input_border_width_px = border_width;
        self.workspace_search.ws_search_file_header_border_width_px = border_width;

        self.settings_panel.settings_title_version_text_color = print_surface_text_muted;
        self.settings_panel.settings_primary_text_color = print_surface_text;
        self.settings_panel.settings_secondary_text_color = print_surface_text_muted;
        self.settings_panel.settings_site_button_border_width_px = border_width;
        self.settings_panel.settings_option_button_border_width_px = border_width;
        self.settings_panel.settings_theme_toggle_border_width_px = border_width;
        self.settings_panel.settings_theme_option_border_width_px = border_width;
        self.settings_panel.settings_site_button_text_color = chrome.text_primary;
        self.settings_panel.settings_site_button_icon_color = chrome.text_secondary;
        self.settings_panel.settings_site_button_border_color = chrome.border_primary;
        self.settings_panel.settings_site_button_bg = chrome.bg_elevated;
        self.settings_panel.settings_site_button_hover_bg = chrome.bg_hover;
        self.settings_panel.settings_option_button_text_color = chrome.text_primary;
        self.settings_panel.settings_option_button_active_text_color = chrome.text_primary;
        self.settings_panel.settings_option_button_border_color = chrome.border_primary;
        self.settings_panel
            .settings_option_button_active_border_color = chrome.border_primary;
        self.settings_panel.settings_contrast_border_color = editor.text_primary;
        self.settings_panel.settings_option_button_bg = chrome.bg_elevated;
        self.settings_panel.settings_option_button_hover_bg = chrome.bg_hover;
        self.settings_panel.settings_option_button_active_bg = chrome.bg_selected;
        self.settings_panel.settings_theme_option_hover_bg = chrome.bg_hover;
        self.settings_panel.settings_theme_option_active_bg = chrome.bg_selected;
        self.settings_panel.settings_theme_option_text_color = chrome.text_primary;
        self.settings_panel.settings_theme_option_active_text_color = chrome.text_bright;
        self.settings_panel.settings_theme_option_border_color = chrome.border_primary;

        self.text_input.text_input_text_color = chrome.text_bright;
        self.text_input.text_input_placeholder_color = chrome.text_secondary;
        self.text_input.text_input_cursor_color = chrome.cursor;
        self.text_input.text_input_selection_color = chrome.selection;
        self.text_input.text_input_menu_border_width_px = border_width;
        self.text_input.text_input_menu_text_color = chrome.text_primary;
        self.search.search_match_border_width_px = border_width;
    }

    /// `theme` drives editor content colors and follows the user's light/dark
    /// selection. `ui_theme` is the dark chrome palette (the user's selected
    /// dark theme) used for the window border, menus, and sidebar surfaces so
    /// those surfaces stay dark even when the editor content uses a light theme.
    pub fn from_theme(theme: &Theme, ui_theme: &Theme) -> Self {
        Self::from_theme_with_options(theme, ui_theme, true)
    }

    /// `sidebar_always_dark = false` renders sidebar surfaces (FileTree,
    /// FileList, workspace search) with the editor theme instead of the dark
    /// chrome palette.
    pub fn from_theme_with_options(
        theme: &Theme,
        ui_theme: &Theme,
        sidebar_always_dark: bool,
    ) -> Self {
        // Light palettes need their muted foregrounds and hairlines deepened
        // before any style token is derived from them.
        let theme_contrast = theme.with_light_surface_contrast();
        let ui_theme_contrast = ui_theme.with_light_surface_contrast();
        let theme = &theme_contrast;
        let ui_theme = &ui_theme_contrast;

        let ui_font_size_px = 13.0;
        let ui_font_size_sm_px = ui_font_size_px - 1.0;
        let ui_font_size_xs_px = ui_font_size_px - 2.0;
        let ui_font_size_lg_px = ui_font_size_px + 1.0;
        let editor_body_font_size_px = 14.0;
        let editor_heading_base_font_size_px = 15.0;

        let is_light = theme.bg_primary.l >= 0.5;
        let sidebar_theme = if sidebar_always_dark { ui_theme } else { theme };
        let is_light_monochrome = is_light && theme.heading == theme.text_primary;
        let light_monochrome_heading_color = if is_light_monochrome {
            theme.heading
        } else {
            theme.list_marker
        };
        let checkbox_border_color = if is_light_monochrome {
            theme.heading
        } else {
            theme.border_tertiary
        };
        let checkbox_checked_bg = if is_light_monochrome {
            theme.heading
        } else {
            theme.checkbox_checked
        };
        let text_input_theme = if is_light { theme } else { ui_theme };
        let text_input_shadow = !is_light;
        let settings_theme = if is_light { theme } else { ui_theme };
        let utility_panel_theme = theme;
        let settings_active_bg = if is_light {
            theme.bg_selected
        } else {
            ui_theme.text_primary
        };
        let settings_active_text_color = if is_light {
            theme.text_bright
        } else {
            ui_theme.bg_primary
        };
        let settings_primary_text_color = if is_light {
            theme.text_bright
        } else {
            theme.text_primary
        };
        let settings_secondary_text_color = if is_light {
            with_alpha(theme.text_primary, 0.82)
        } else {
            theme.text_secondary
        };
        let settings_update_color = settings_theme.text_warning;
        Self {
            ui_font_family: crate::platform::fonts::SYSTEM_UI,
            ui_font_size_px,
            anchored_popup_gap_px: 2.0,
            anchored_popup_margin_px: 8.0,
            status_bar_padding_left_px: 0.0,
            window: WindowStyles {
                window_bg: theme.bg_primary,
                window_border_width_px: if is_light { 0.0 } else { 1.0 },
                window_border_color: theme.window_border,
                // Only macOS rounds its own frame to match; elsewhere a rounded
                // client area just cuts corners out of a square window.
                window_radius_px: if cfg!(target_os = "macos") { 16.0 } else { 0.0 },
            },
            editor: EditorStyles {
                editor_font_size_px: editor_body_font_size_px,
                editor_h1_font_size_px: editor_heading_base_font_size_px * 1.4,
                editor_h2_font_size_px: editor_heading_base_font_size_px * 1.3,
                editor_h3_font_size_px: editor_heading_base_font_size_px * 1.2,
                editor_h4_font_size_px: editor_heading_base_font_size_px * 1.1,
                editor_h5_font_size_px: editor_heading_base_font_size_px * 1.05,
                editor_h6_font_size_px: editor_heading_base_font_size_px,
                editor_paragraph_line_height_factor: 1.5,
                editor_heading_line_height_factor: 1.5,
                editor_letter_spacing_px: 0.0,
                editor_body_latin_font_family: crate::platform::fonts::MONOSPACE,
                editor_body_cjk_font_family: crate::platform::fonts::CJK,
                editor_heading_latin_font_family: crate::platform::fonts::MONOSPACE,
                editor_heading_cjk_font_family: crate::platform::fonts::CJK,
                editor_italic_latin_font_family: crate::platform::fonts::MONOSPACE,
                editor_italic_cjk_font_family: crate::platform::fonts::ITALIC_CJK,
                editor_monospace_font_family: crate::platform::fonts::MONOSPACE,
                editor_paragraph_padding_top_px: 0.0,
                editor_paragraph_padding_bottom_px: 0.0,
                editor_heading_padding_top_px: 10.0,
                editor_heading_padding_bottom_px: 10.0,
                editor_padding_left_px: 40.0,
                editor_padding_top_px: 80.0,
                editor_non_markdown_padding_top_px: 28.0,
                editor_padding_right_px: 40.0,
                editor_padding_bottom_px: 200.0,
                editor_wrap_right_padding_px: 24.0,
                editor_max_width_px: 700.0,
                editor_non_markdown_max_width_px: 1120.0,
                editor_file_drop_bg: with_alpha(theme.text_accent, 0.2),
                editor_path_bar_height_px: 30.0,
                editor_path_bar_padding_x_px: 14.0,
                editor_path_bar_gap_px: 6.0,
                editor_path_bar_font_size_px: ui_font_size_sm_px,
                editor_path_bar_bg: theme.bg_primary,
                editor_path_bar_text_color: theme.text_secondary,
                editor_path_bar_separator_color: with_alpha(theme.text_secondary, 0.55),
                editor_path_bar_separator_size_px: 12.0,
                editor_path_bar_suffix_opacity: 0.42,
                editor_path_bar_item_padding_x_px: 6.0,
                editor_path_bar_item_padding_y_px: 3.0,
                editor_path_bar_item_radius_px: 5.0,
                editor_path_bar_item_hover_text_color: theme.text_primary,
                editor_path_bar_item_opacity: 0.68,
                editor_path_bar_diff_group_gap_px: 10.0,
                editor_path_bar_diff_button_label_gap_px: 10.0,
                editor_path_bar_diff_search_gap_px: 10.0,
                editor_path_bar_diff_label_arrow_gap_px: 3.0,
                editor_path_bar_action_gap_px: 2.0,
                editor_path_bar_action_btn_size_px: 20.0,
                editor_path_bar_action_icon_size_px: 14.0,
                editor_path_bar_action_opacity: 0.7,
                editor_path_bar_action_hover_opacity: 1.0,
                editor_path_bar_action_padding_right_px: 2.0,
                editor_path_bar_border_width_px: 1.0,
                editor_path_bar_border_color: theme.border_secondary,
                editor_list_content_indent_px: 10.0,
                editor_unordered_list_marker_cycle: ["• ", "◦ ", "▪ "],
                editor_unordered_list_marker_font_size_offset_px: 0.0,
                editor_unordered_list_marker_line_height_offset_px: 0.0,
                editor_superscript_scale_factor: 0.72,
                editor_subscript_scale_factor: 0.72,
                editor_superscript_offset_px: 2.0,
                editor_subscript_offset_px: 2.0,
                editor_bg: theme.bg_primary,
                editor_text_color: theme.text_primary,
                editor_empty_placeholder_opacity: 0.3,
                editor_diff_collapsed_line_label_text_color: with_alpha(theme.text_secondary, 0.5),
                editor_indent_guide_color: with_alpha(
                    theme.border_primary,
                    if is_light { 1.0 } else { 0.35 },
                ),
                editor_indent_guide_width_px: 1.0,
                editor_indent_guide_dash_length_px: 3.0,
                editor_indent_guide_gap_length_px: 0.0,
                editor_whitespace_dot_size_px: 2.0,
                editor_whitespace_dot_color: with_alpha(theme.text_secondary, 0.35),
                editor_inline_code_bg: if is_light {
                    with_alpha(theme.inline_code_bg, 0.42)
                } else {
                    theme.inline_code_bg
                },
                editor_inline_code_bg_x_padding_px: 4.0,
                editor_inline_code_bg_radius_px: 4.0,
                editor_inline_code_bg_y_inset_px: 2.0,
                editor_inline_math_height_factor: 0.60,
                editor_inline_highlight_bg: theme.highlight_bg,
                editor_list_marker_text_color: light_monochrome_heading_color,
                editor_list_marker_font_weight: FontWeight::BOLD,
                editor_heading_marker_text_color: with_alpha(
                    theme.text_secondary,
                    if is_light { 0.50 } else { 0.40 },
                ),
                editor_selection_match_bg: with_alpha(
                    theme.text_secondary,
                    if is_light { 0.10 } else { 0.16 },
                ),
                editor_selection_match_border: with_alpha(
                    theme.text_secondary,
                    if is_light { 0.24 } else { 0.30 },
                ),
                editor_bracket_match_border: with_alpha(theme.text_primary, 0.5),
                editor_match_border_width_px: 1.0,
                editor_scrollbar_top_margin_px: 3.0,
                editor_scrollbar_bottom_margin_px: 3.0,
                editor_scrollbar_right_margin_px: 3.0,
                editor_scrollbar_width_px: 6.0,
                editor_scrollbar_hover_width_px: 6.0,
                editor_scrollbar_hit_width_px: 8.0,
                editor_scrollbar_min_thumb_height_px: 36.0,
                editor_scrollbar_radius_px: 100.0,
                editor_scrollbar_thumb_color: with_alpha(theme.text_primary, 1.0),
                editor_scrollbar_idle_opacity: 0.0,
                editor_scrollbar_opacity: 0.2,
                editor_scrollbar_hover_opacity: 0.3,
                editor_scrollbar_animation_frame_ms: 8,
                editor_scrollbar_fade_animation_ms: 80,
                editor_scrollbar_width_animation_ms: 40,
                editor_scrollbar_idle_delay_ms: 800,
                editor_scale_step: 0.1,
                editor_scale_min: 0.8,
                editor_scale_max: 1.6,
            },
            sidebar: SidebarStyles {
                sidebar_width_px: 230.0,
                sidebar_bg: sidebar_theme.bg_secondary,
                sidebar_bottom_spacer_height_px: 30.0,
                sidebar_top_buttons_visible: false,
                sidebar_top_button_padding_right_px: 0.0,
                sidebar_top_button_gap_px: 6.0,
                sidebar_top_button_width_px: TOP_BAR_HEIGHT_PX,
                sidebar_top_button_height_px: TOP_BAR_HEIGHT_PX,
                sidebar_top_button_size_px: 18.0,
                sidebar_top_button_icon_size_px: 15.0,
                sidebar_top_button_inactive_color: sidebar_theme.text_secondary,
                sidebar_top_button_active_color: sidebar_theme.text_primary,
                sidebar_logo_size_px: 50.0,
                sidebar_logo_padding_y_px: 12.0,
                sidebar_header_height_px: 26.0,
                sidebar_header_bg: gpui::transparent_black(),
                sidebar_header_padding_left_px: 14.0,
                sidebar_header_padding_right_px: 4.0,
                sidebar_header_gap_px: 6.0,
                sidebar_header_icon_size_px: 12.0,
                sidebar_header_root_icon_size_px: 15.0,
                sidebar_header_star_icon_size_px: 15.0,
                sidebar_header_icon_color: sidebar_theme.text_secondary,
                sidebar_header_font_size_px: ui_font_size_px,
                sidebar_header_text_color: sidebar_theme.text_primary,
                sidebar_header_hover_bg: sidebar_theme.bg_hover,
                sidebar_header_action_gap_px: 2.0,
                sidebar_header_action_btn_size_px: 20.0,
                sidebar_header_action_icon_size_px: 14.0,
                sidebar_starred_item_close_icon_size_px: 12.0,
                sidebar_header_action_opacity: 0.7,
                sidebar_header_action_hover_opacity: 1.0,
                sidebar_item_height_px: 26.0,
                sidebar_item_indent_base_px: 24.0,
                sidebar_item_indent_step_px: 16.0,
                sidebar_item_left_shift_px: 10.0,
                sidebar_item_padding_right_px: 8.0,
                sidebar_item_gap_px: 4.0,
                sidebar_item_hover_bg: sidebar_theme.bg_hover,
                sidebar_item_selected_bg: sidebar_theme.bg_selected,
                sidebar_item_fold_container_width_px: 16.0,
                sidebar_item_fold_container_height_px: 18.0,
                sidebar_item_fold_icon_size_px: 12.0,
                sidebar_item_fold_icon_color: sidebar_theme.text_secondary,
                sidebar_dir_icon_size_px: 16.0,
                sidebar_item_icon_size_px: 14.0,
                sidebar_item_icon_color: sidebar_theme.text_secondary,
                sidebar_item_selected_icon_color: sidebar_theme.text_bright,
                sidebar_item_badge_min_width_px: 14.0,
                sidebar_item_badge_height_px: 14.0,
                sidebar_item_badge_padding_x_px: 3.0,
                sidebar_item_badge_radius_px: 3.0,
                sidebar_item_badge_font_size_px: 10.0,
                sidebar_item_badge_bg: with_alpha(sidebar_theme.text_secondary, 0.3),
                sidebar_item_badge_text_color: sidebar_theme.text_secondary,
                sidebar_count_badge_min_width_px: 16.0,
                sidebar_count_badge_height_px: 15.0,
                sidebar_count_badge_padding_x_px: 4.0,
                sidebar_count_badge_radius_px: 4.0,
                sidebar_count_badge_font_size_px: 10.0,
                sidebar_count_badge_margin_left_px: 6.0,
                sidebar_count_badge_bg: with_alpha(sidebar_theme.text_secondary, 0.18),
                sidebar_count_badge_text_color: with_alpha(sidebar_theme.text_secondary, 0.92),
                sidebar_item_font_size_px: ui_font_size_px,
                sidebar_item_text_color: sidebar_theme.text_primary,
                sidebar_item_selected_text_color: sidebar_theme.text_bright,
                sidebar_item_git_added_text_color: sidebar_theme.git_status_added,
                sidebar_item_git_modified_text_color: sidebar_theme.git_status_modified,
                sidebar_item_suffix_opacity: 0.48,
                sidebar_item_active_dot_size_px: 5.0,
                sidebar_item_active_dot_color: sidebar_theme.text_accent,
                sidebar_edit_input_height_px: 20.0,
                sidebar_edit_input_padding_x_px: 6.0,
                sidebar_edit_input_bg: gpui::transparent_black(),
                sidebar_edit_input_border_width_px: 1.0,
                sidebar_edit_input_border_color: sidebar_theme.text_accent,
                sidebar_edit_input_radius_px: 3.0,
                sidebar_edit_input_font_size_px: ui_font_size_px,
                sidebar_edit_input_cursor_height_ratio: 0.72,
                sidebar_edit_input_text_color: sidebar_theme.text_bright,
                sidebar_edit_input_placeholder_color: sidebar_theme.text_secondary,
                sidebar_error_padding_x_px: 14.0,
                sidebar_error_padding_y_px: 12.0,
                sidebar_error_font_size_px: ui_font_size_sm_px,
                sidebar_error_text_color: sidebar_theme.text_danger,
                sidebar_empty_gap_px: 12.0,
                sidebar_empty_padding_x_px: 16.0,
                sidebar_empty_font_size_px: ui_font_size_sm_px,
                sidebar_empty_text_color: sidebar_theme.text_secondary,
                sidebar_empty_btn_padding_x_px: 16.0,
                sidebar_empty_btn_padding_y_px: 5.0,
                sidebar_empty_btn_border_width_px: 1.5,
                sidebar_empty_btn_border_color: sidebar_theme.border_primary,
                sidebar_empty_btn_font_size_px: ui_font_size_sm_px,
                sidebar_empty_btn_text_color: sidebar_theme.text_secondary,
                sidebar_empty_btn_hover_bg: sidebar_theme.bg_hover,
                sidebar_resize_handle_width_px: 5.0,
                sidebar_resize_indicator_width_px: 1.0,
                sidebar_resize_active_indicator_width_px: 3.0,
                sidebar_resize_indicator_color: sidebar_theme.border_primary,
                sidebar_resize_active_indicator_color: sidebar_theme.text_accent,
                sidebar_resize_min_width_px: 140.0,
                sidebar_resize_max_width_px: 600.0,
                sidebar_visibility_animation_ms: 220,
                sidebar_indent_guide_color: with_alpha(sidebar_theme.border_secondary, 0.5),
                sidebar_indent_guide_width_px: 1.0,
                sidebar_indent_guide_dash_length_px: 3.0,
                sidebar_indent_guide_gap_length_px: 3.0,
                sidebar_item_focus_border_color: with_alpha(sidebar_theme.text_accent, 0.3),
                sidebar_item_active_border_color: with_alpha(sidebar_theme.text_accent, 0.2),
                file_list_width_px: 240.0,
                file_list_bg: sidebar_theme.bg_secondary,
                file_list_header_height_px: TOP_BAR_HEIGHT_PX,
                file_list_header_padding_left_px: 12.0,
                file_list_header_padding_right_px: 4.0,
                file_list_header_gap_px: 6.0,
                file_list_header_font_size_px: ui_font_size_px,
                file_list_header_text_color: sidebar_theme.text_primary,
                file_list_header_border_width_px: 1.0,
                file_list_header_border_color: sidebar_theme.border_primary,
                file_list_item_padding_x_px: 12.0,
                file_list_item_padding_y_px: 8.0,
                file_list_item_gap_px: 3.0,
                file_list_item_title_gap_px: 6.0,
                file_list_item_title_font_size_px: ui_font_size_px,
                file_list_item_title_line_height_px: 18.0,
                file_list_item_title_color: sidebar_theme.text_primary,
                file_list_item_selected_title_color: sidebar_theme.text_bright,
                file_list_item_preview_font_size_px: ui_font_size_sm_px,
                file_list_item_preview_line_height_px: 16.0,
                file_list_item_preview_max_lines: 2,
                file_list_item_preview_color: sidebar_theme.text_secondary,
                file_list_item_time_font_size_px: ui_font_size_xs_px,
                file_list_item_time_line_height_px: 15.0,
                file_list_item_time_color: with_alpha(sidebar_theme.text_secondary, 0.75),
                file_list_item_thumbnail_gap_px: 10.0,
                file_list_item_thumbnail_radius_px: 6.0,
                file_list_item_thumbnail_border_width_px: 0.0,
                file_list_item_thumbnail_border_color: sidebar_theme.border_primary,
                file_list_item_bg: sidebar_theme.bg_secondary,
                file_list_item_hover_bg: sidebar_theme.bg_primary,
                file_list_item_selected_bg: sidebar_theme.bg_primary,
                file_list_item_separator_width_px: 1.0,
                file_list_item_separator_color: with_alpha(sidebar_theme.border_secondary, 0.35),
                file_list_empty_font_size_px: ui_font_size_sm_px,
                file_list_empty_text_color: sidebar_theme.text_secondary,
                file_list_resize_min_width_px: 160.0,
                file_list_resize_max_width_px: 480.0,
                sidebar_scrollbar_thumb_color: with_alpha(sidebar_theme.text_primary, 1.0),
            },
            outline: OutlineStyles {
                outline_rail_right_margin_px: 14.0,
                outline_rail_vertical_padding_px: 20.0,
                outline_rail_label_font_size_px: ui_font_size_sm_px,
                outline_rail_label_max_width_px: 240.0,
                outline_rail_label_indent_step_px: 12.0,
                outline_rail_label_indent_guide_width_px: 1.0,
                outline_rail_label_indent_guide_dash_length_px: 3.0,
                outline_rail_label_indent_guide_gap_length_px: 3.0,
                outline_rail_label_indent_guide_color: with_alpha(theme.text_secondary, 0.6),
                outline_rail_label_fold_icon_size_px: 10.0,
                outline_rail_label_fold_icon_gap_px: 2.0,
                outline_rail_label_gap_px: 8.0,
                outline_rail_label_item_height_px: 18.0,
                outline_rail_label_item_gap_px: 0.0,
                outline_rail_label_item_padding_y_px: 4.0,
                outline_rail_label_line_height_px: 15.0,
                outline_rail_label_wrap_chars_per_line: 24,
                outline_rail_label_max_lines: 3,
                outline_rail_label_color: theme.text_secondary,
                outline_rail_label_hover_color: theme.text_bright,
                outline_rail_label_active_color: theme.text_bright,
                outline_rail_tick_level_opacities: [1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                outline_rail_label_level_opacities: [1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                outline_rail_label_level_brightness_mix: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                outline_rail_row_height_px: 6.0,
                outline_rail_tick_width_px: 8.0,
                outline_rail_tick_thickness_px: 2.0,
                outline_rail_tick_color: with_alpha(theme.text_primary, 0.3),
                outline_rail_tick_active_color: with_alpha(theme.text_primary, 0.58),
                outline_rail_tick_active_extra_width_px: 12.0,
                outline_rail_tick_active_neighbor_count: 3,
                outline_rail_tick_active_falloff_power: 2.2,
                outline_rail_tick_active_animation_seconds: 0.24,
                outline_rail_edge_fade_px: 48.0,
                outline_rail_hover_animation_seconds: 0.2,
                outline_flash_initial_delay_ms: 500,
                outline_flash_show_duration_ms: 170,
                outline_flash_gap_duration_ms: 90,
            },
            utility_panel: UtilityPanelStyles {
                utility_panel_width_px: RIGHT_PANEL_MIN_WIDTH_PX,
                utility_panel_bg: utility_panel_theme.bg_secondary,
                utility_panel_terminal_padding_px: 10.0,
                utility_panel_border_width_px: 1.0,
                utility_panel_border_color: utility_panel_theme.border_primary,
                utility_panel_top_spacer_height_px: 14.0,
                utility_panel_bottom_spacer_height_px: 14.0,
                utility_panel_header_height_px: 15.0,
                utility_panel_header_padding_x_px: 14.0,
                utility_panel_header_font_size_px: ui_font_size_xs_px,
                utility_panel_header_text_color: utility_panel_theme.text_primary,
                utility_panel_section_gap_px: 18.0,
                utility_panel_section_padding_x_px: 14.0,
                utility_panel_section_title_font_size_px: ui_font_size_xs_px,
                utility_panel_section_title_text_color: utility_panel_theme.text_primary,
                utility_panel_item_height_px: 24.0,
                utility_panel_item_gap_px: 8.0,
                utility_panel_item_font_size_px: ui_font_size_sm_px,
                utility_panel_item_label_text_color: utility_panel_theme.text_secondary,
                utility_panel_item_value_text_color: utility_panel_theme.text_primary,
                utility_panel_terminal_inactive_opacity: 1.0,
                utility_panel_button_height_px: 28.0,
                utility_panel_button_gap_px: 8.0,
                utility_panel_button_padding_x_px: 10.0,
                utility_panel_button_radius_px: 6.0,
                utility_panel_button_border_width_px: 1.0,
                utility_panel_button_border_color: utility_panel_theme.border_primary,
                utility_panel_button_text_color: utility_panel_theme.text_primary,
                utility_panel_button_hover_bg: utility_panel_theme.bg_hover,
                utility_panel_empty_font_size_px: ui_font_size_sm_px,
                utility_panel_empty_text_color: utility_panel_theme.text_secondary,
                utility_panel_resize_handle_width_px: 5.0,
                utility_panel_resize_indicator_width_px: 1.0,
                utility_panel_resize_active_indicator_width_px: 3.0,
                utility_panel_resize_indicator_color: utility_panel_theme.border_primary,
                utility_panel_resize_active_indicator_color: utility_panel_theme.text_accent,
                utility_panel_resize_min_width_px: RIGHT_PANEL_MIN_WIDTH_PX,
                utility_panel_resize_max_width_px: RIGHT_PANEL_MAX_WIDTH_PX,
                utility_panel_visibility_animation_ms: 220,
            },
            tab_bar: TabBarStyles {
                tab_bar_height_px: TOP_BAR_HEIGHT_PX,
                tab_bar_bg: theme.bg_bar,
                tab_bar_border_width_px: 1.0,
                tab_bar_border_color: theme.border_secondary,
                tab_item_min_width_px: 52.0,
                tab_item_max_width_px: 160.0,
                tab_item_padding_left_px: 9.0,
                tab_item_padding_right_px: 6.0,
                tab_item_gap_px: 5.0,
                tab_item_bg: theme.bg_tab,
                tab_item_hover_bg: theme.bg_tab_hover,
                tab_item_active_bg: theme.bg_tab_active,
                tab_item_border_width_px: 1.0,
                tab_item_border_color: theme.border_secondary,
                tab_item_icon_size_px: 14.0,
                tab_action_emphasis_icon_size_px: 16.0,
                tab_item_icon_color: theme.text_secondary,
                tab_item_icon_active_opacity: 1.0,
                tab_item_icon_inactive_opacity: 0.4,
                tab_item_badge_min_width_px: 14.0,
                tab_item_badge_height_px: 12.0,
                tab_item_badge_padding_x_px: 3.0,
                tab_item_badge_radius_px: 3.0,
                tab_item_badge_font_size_px: 8.0,
                tab_item_badge_bg: with_alpha(theme.text_secondary, 0.3),
                tab_item_badge_text_color: theme.text_secondary,
                tab_item_font_size_px: ui_font_size_sm_px,
                tab_item_text_color: theme.text_primary,
                tab_item_active_text_color: theme.text_bright,
                tab_item_suffix_opacity: 0.5,
                tab_item_close_size_px: 14.0,
                tab_item_close_font_size_px: ui_font_size_xs_px - 1.0,
                tab_item_close_text_color: theme.text_primary,
                tab_item_close_hover_bg: theme.bg_hover,
                tab_item_close_active_opacity: 1.0,
                tab_item_close_inactive_opacity: 0.0,
                tab_item_dirty_dot_size_px: 6.0,
                tab_item_dirty_dot_color: theme.text_accent,
                tab_item_deleted_text_color: theme.text_secondary,
                tab_new_width_px: 30.0,
                tab_new_font_size_px: ui_font_size_lg_px,
                tab_new_text_color: theme.text_secondary,
                tab_new_hover_text_color: theme.text_bright,
                tab_end_drop_target_width_px: 12.0,
                tab_update_button_height_inset_px: 6.0,
                tab_update_button_min_height_px: 18.0,
                tab_update_button_padding_x_px: 8.0,
                tab_update_button_gap_px: 4.0,
                tab_update_button_radius_px: 6.0,
                tab_update_button_opacity: 0.8,
                tab_update_button_hover_opacity: 1.0,
                tab_update_button_icon_size_px: 14.0,
                tab_update_button_font_size_px: ui_font_size_sm_px,
                tab_update_button_text_color: ui_theme.text_warning,
                tab_update_button_hover_text_color: ui_theme.text_warning_hover,
                tab_update_button_icon_color: ui_theme.text_warning,
                tab_drag_ghost_opacity: 0.7,
                tab_drag_source_opacity: 0.3,
                tab_drop_indicator_width_px: 2.0,
                tab_drop_indicator_color: theme.text_accent,
                tab_pin_icon_size_px: 11.0,
                tab_pin_icon_color: theme.text_secondary,
                tab_pinned_width_px: 30.0,
                tab_overflow_font_size_px: ui_font_size_xs_px,
                tab_overflow_text_color: theme.text_secondary,
                tab_overflow_bg: theme.bg_tab,
                tab_overflow_padding_x_px: 4.0,
                tab_history_hidden_sidebar_margin_px: 72.0,
                tab_history_padding_x_px: 0.0,
                tab_history_gap_px: 4.0,
                tab_history_group_border_width_px: 1.0,
                tab_history_group_border_color: theme.border_secondary,
                tab_history_btn_size_px: 24.0,
                tab_history_btn_radius_px: 6.0,
                tab_history_btn_bg: theme.bg_tab,
                tab_history_btn_icon_size_px: 11.0,
                tab_history_btn_icon_color: theme.text_primary,
                tab_history_btn_hover_icon_color: theme.text_bright,
                tab_history_btn_icon_disabled_color: theme.text_secondary,
                tab_history_btn_enabled_opacity: 1.0,
                tab_history_btn_disabled_opacity: 0.45,
                tab_action_button_padding_x_px: 5.0,
            },
            toolbar: ToolbarStyles {
                toolbar_offset_left_px: 15.0,
                toolbar_offset_top_px: 15.0,
                toolbar_gap_px: 0.0,
                toolbar_hidden_opacity: 0.0,
                toolbar_opacity_animation_ms: 120,
                toolbar_reveal_hover_height_px: 120.0,
                toolbar_corner_reveal_width_px: 100.0,
                toolbar_corner_reveal_height_px: 100.0,
                toolbar_primary_btn_size_px: 55.0,
                toolbar_primary_icon_size_px: 55.0,
                toolbar_primary_hover_opacity: 0.9,
                toolbar_secondary_btn_size_px: 50.0,
                toolbar_secondary_icon_size_px: 50.0,
                toolbar_secondary_btn_padding_x_px: 6.0,
                toolbar_secondary_hover_opacity: 0.9,
                toolbar_btn_shadow: false,
                toolbar_btn_bg: with_alpha(ui_theme.bg_elevated, 0.85),
                toolbar_btn_radius_px: 8.0,
                toolbar_whiteboard_bottom_px: 18.0,
                toolbar_whiteboard_gap_px: 3.0,
                toolbar_whiteboard_padding_px: 5.0,
                toolbar_whiteboard_btn_size_px: 30.0,
                toolbar_whiteboard_icon_size_px: 16.0,
                toolbar_whiteboard_border_width_px: 1.0,
                toolbar_whiteboard_shadow: true,
                toolbar_whiteboard_bg: theme.bg_elevated,
                toolbar_whiteboard_border_color: theme.border_primary,
                toolbar_whiteboard_btn_bg: with_alpha(theme.bg_elevated, 0.0),
                toolbar_whiteboard_btn_active_bg: theme.bg_selected,
                toolbar_whiteboard_btn_hover_bg: theme.bg_hover,
                toolbar_whiteboard_icon_color: theme.text_secondary,
                toolbar_whiteboard_icon_active_color: theme.text_bright,
            },
            format_toolbar: FormatToolbarStyles {
                format_toolbar_height_px: 30.0,
                format_toolbar_padding_x_px: 8.0,
                format_toolbar_optical_left_inset_px: 5.0,
                format_toolbar_gap_px: 1.0,
                format_toolbar_group_gap_px: 2.0,
                format_toolbar_bg: theme.bg_primary,
                format_toolbar_border_width_px: 1.0,
                format_toolbar_border_color: theme.border_secondary,
                format_toolbar_btn_size_px: 22.0,
                format_toolbar_icon_size_px: 13.0,
                format_toolbar_icon_color: with_alpha(theme.text_secondary, 0.72),
                format_toolbar_icon_hover_color: theme.text_bright,
                format_toolbar_icon_active_color: theme.text_bright,
                format_toolbar_separator_width_px: 1.0,
                format_toolbar_separator_height_px: 14.0,
                format_toolbar_separator_margin_x_px: 5.0,
                format_toolbar_separator_color: with_alpha(theme.text_secondary, 0.38),
            },
            whiteboard: WhiteboardStyles {
                whiteboard_swatch_size_px: 28.0,
                whiteboard_swatch_ring_width_px: 3.0,
                whiteboard_swatch_ring_gap_px: 1.0,
                whiteboard_swatch_checker_cell_px: 4.0,
                whiteboard_swatch_grid_gap_y_px: 6.0,
                whiteboard_swatch_ring_color: theme.text_bright,
                whiteboard_swatch_ring_idle_color: gpui::transparent_black(),
                whiteboard_swatch_checker_light_color: crate::hex_hsla!("#ffffff"),
                whiteboard_swatch_checker_dark_color: crate::hex_hsla!("#d9dee3"),
                whiteboard_render_cull_margin_px: 240.0,
                whiteboard_fill_opacity_levels: [0.2, 0.4, 0.6, 0.8, 1.0],
            },
            tooltip: TooltipStyles {
                tooltip_delay_ms: 0,
                tooltip_viewport_padding_px: 8.0,
                tooltip_offset_px: 6.0,
                tooltip_max_width_px: 320.0,
                tooltip_padding_x_px: 4.0,
                tooltip_padding_y_px: 3.0,
                tooltip_gap_px: 4.0,
                tooltip_radius_px: 6.0,
                tooltip_border_width_px: 1.0,
                tooltip_bg: with_alpha(ui_theme.bg_elevated, 1.0),
                tooltip_border_color: with_alpha(ui_theme.border_primary, 0.32),
                tooltip_title_font_size_px: ui_font_size_xs_px,
                tooltip_title_text_color: ui_theme.text_primary,
                tooltip_subtitle_font_size_px: ui_font_size_xs_px - 1.0,
                tooltip_subtitle_text_color: ui_theme.text_primary,
                tooltip_subtitle_opacity: 0.72,
                tooltip_shortcut_gap_px: 4.0,
                tooltip_shortcut_key_min_width_px: 14.0,
                tooltip_shortcut_key_height_px: 15.0,
                tooltip_shortcut_key_padding_x_px: 4.0,
                tooltip_shortcut_key_radius_px: 5.0,
                tooltip_shortcut_key_bg: with_alpha(ui_theme.bg_code, 0.98),
            },
            cursor: CursorStyles {
                // The caret keeps one blue across every theme so it never blends
                // into a palette's own foreground colours.
                cursor_color: crate::theme::EDITOR_CARET,
                cursor_width_px: 2.0,
                cursor_height_factor: 1.0,
                cursor_blink_pause_ms: 420,
                cursor_blink_visible_ms: 480,
                cursor_blink_transparent_ms: 480,
                cursor_opacity_min: 0.0,
                cursor_opacity_max: 1.0,
            },
            code_block: CodeBlockStyles {
                code_block_bg: theme.bg_code,
                code_block_border_width_px: 1.0,
                code_block_border_color: if is_light {
                    with_alpha(theme.border_primary, 0.58)
                } else {
                    with_alpha(theme.text_secondary, 0.10)
                },
                code_block_split_border_color: if is_light {
                    with_alpha(theme.text_secondary, 0.5)
                } else {
                    with_alpha(theme.text_secondary, 0.2)
                },
                code_block_split_border_width_px: 1.0,
                code_block_split_border_dash_length_px: 3.0,
                code_block_split_border_dash_gap_px: 3.0,
                code_block_radius_px: 0.0,
                code_block_fill_inset_px: 1.0,
                code_block_text_padding_left_px: 20.0,
                code_block_line_number_padding_left_px: 23.0,
                code_block_wrap_right_padding_px: 20.0,
                code_block_horizontal_scrollbar_margin_x_px: 3.0,
                code_block_fence_clip_px: 15.0,
                code_block_copy_button_top_px: 2.0,
                code_block_copy_button_right_px: -4.0,
                code_block_copy_button_padding_x_px: 10.0,
                code_block_copy_button_padding_y_px: 4.0,
                code_block_copy_button_radius_px: 999.0,
                code_block_copy_button_border_width_px: 1.0,
                code_block_copy_button_font_size_px: ui_font_size_xs_px,
                code_block_copy_button_bg: with_alpha(theme.bg_elevated, 0.92),
                code_block_copy_button_text_color: theme.text_secondary,
                code_block_copy_button_border_color: with_alpha(theme.border_primary, 0.85),
                code_block_copy_button_hover_bg: theme.bg_hover,
                code_block_copy_button_hover_text_color: theme.text_bright,
                code_block_copy_button_hover_border_color: with_alpha(theme.text_accent, 0.35),
                code_block_copy_button_copied_bg: with_alpha(theme.text_accent, 0.14),
                code_block_copy_button_copied_text_color: theme.text_bright,
                code_block_copy_button_copied_border_color: with_alpha(theme.text_accent, 0.45),
                code_block_display_action_button_size_px: 16.0,
                code_block_display_action_icon_size_px: 12.0,
                code_block_display_action_gap_px: 1.0,
                generated_preview_action_padding_px: 9.0,
                generated_preview_action_inner_padding_x_px: 3.0,
                generated_preview_action_gap_px: 3.0,
                generated_preview_action_button_size_px: 22.0,
                generated_preview_action_icon_size_px: 11.0,
                generated_preview_action_button_radius_px: 5.0,
                generated_preview_action_border_width_px: 1.0,
                generated_preview_action_border_color: with_alpha(theme.border_primary, 0.38),
                generated_preview_min_height_px: 50.0,
                generated_preview_max_height_px: 300.0,
                generated_preview_padding_x_px: 16.0,
                generated_preview_padding_y_px: 12.0,
            },
            quote_block: QuoteBlockStyles {
                quote_block_border_width_px: 4.0,
                quote_block_radius_px: 0.0,
                quote_block_padding_top_px: 0.0,
                quote_block_padding_bottom_px: 0.0,
                quote_block_callout_padding_top_px: 5.0,
                quote_block_callout_padding_bottom_px: 5.0,
                quote_block_padding_left_px: 12.0,
                quote_block_line_syntax_indent_px: 10.0,
                quote_block_padding_right_px: 10.0,
                quote_block_callout_icon_color: theme.bold,
                quote_block_callout_icon_size_factor: 1.0,
                quote_block_callout_icon_min_px: 14.0,
                quote_block_callout_icon_max_line_height_factor: 1.5,
                quote_block_callout_icon_label_gap_px: 3.0,
                quote_block_bg: with_alpha(theme.text_secondary, if is_light { 0.0 } else { 0.0 }),
                quote_block_border_color: with_alpha(
                    theme.text_secondary,
                    if is_light { 0.72 } else { 0.3 },
                ),
                quote_block_note_bg: with_alpha(theme.link, if is_light { 0.10 } else { 0.08 }),
                quote_block_note_border_color: with_alpha(
                    theme.link,
                    if is_light { 1.0 } else { 0.6 },
                ),
                quote_block_tip_bg: with_alpha(
                    theme.git_status_added,
                    if is_light { 0.12 } else { 0.08 },
                ),
                quote_block_tip_border_color: with_alpha(
                    theme.git_status_added,
                    if is_light { 1.0 } else { 0.58 },
                ),
                quote_block_important_bg: with_alpha(
                    theme.italic,
                    if is_light { 0.12 } else { 0.08 },
                ),
                quote_block_important_border_color: with_alpha(
                    theme.italic,
                    if is_light { 1.0 } else { 0.58 },
                ),
                quote_block_warning_bg: with_alpha(
                    theme.text_warning,
                    if is_light { 0.14 } else { 0.09 },
                ),
                quote_block_warning_border_color: with_alpha(
                    theme.text_warning,
                    if is_light { 1.0 } else { 0.58 },
                ),
                quote_block_caution_bg: with_alpha(
                    theme.text_danger,
                    if is_light { 0.10 } else { 0.08 },
                ),
                quote_block_caution_border_color: with_alpha(
                    theme.text_danger,
                    if is_light { 1.0 } else { 0.58 },
                ),
            },
            wiki_link: WikiLinkStyles {
                wiki_link_text_color: theme.inline_code,
                wiki_link_text_weight: FontWeight::BOLD,
            },
            selection: SelectionStyles {
                selection_color: theme.selection,
                selection_line_highlight_color: theme.line_highlight,
                selection_diff_added_line_bg: with_alpha(
                    theme.diff_added_bg,
                    if is_light { 0.16 } else { 0.3 },
                ),
                selection_diff_removed_line_bg: with_alpha(
                    theme.diff_removed_bg,
                    if is_light { 0.16 } else { 0.3 },
                ),
                // Light mode keeps the diff red/green subtle: the inline
                // highlight background and its border are lowered so changed
                // text does not read as a heavy solid block. Dark mode keeps the
                // stronger contrast it needs to stay legible.
                selection_diff_added_inline_bg: with_alpha(
                    theme.diff_added_bg,
                    if is_light { 0.18 } else { 0.75 },
                ),
                selection_diff_removed_inline_bg: with_alpha(
                    theme.diff_removed_bg,
                    if is_light { 0.18 } else { 0.75 },
                ),
                selection_diff_added_inline_border_color: gpui::transparent_black(),
                selection_diff_removed_inline_border_color: gpui::transparent_black(),
                selection_diff_inline_border_width_px: 0.0,
                selection_diff_revert_button_padding_x_px: 12.0,
                selection_diff_revert_button_padding_y_px: 7.0,
                selection_diff_revert_button_radius_px: 5.0,
                selection_diff_revert_button_border_width_px: 1.0,
                selection_diff_revert_button_gap_y_px: 4.0,
                selection_diff_revert_button_icon_size_px: 15.0,
                selection_diff_revert_button_icon_gap_px: 6.0,
                selection_diff_revert_button_font_size_px: ui_font_size_sm_px - 1.0,
                selection_diff_revert_button_bg: theme.bg_secondary,
                selection_diff_revert_button_text_color: theme.text_primary,
                selection_diff_revert_button_border_color: theme.border_primary,
                selection_diff_revert_button_hover_bg: theme.bg_hover,
                selection_diff_revert_button_hover_text_color: theme.text_primary,
                selection_diff_revert_button_hover_border_color: theme.border_primary,
                // The left-edge indicator bars stay strong in both appearances
                // so changed lines are scannable at a glance. Their brightness
                // is dimmer while collapsed and full while expanded.
                selection_diff_added_bar_color: with_alpha(theme.diff_added_bg, 1.0),
                selection_diff_removed_bar_color: with_alpha(theme.diff_removed_bg, 1.0),
                selection_diff_bar_width_px: 8.0,
                selection_diff_bar_stripe_thickness_px: 3.0,
                selection_diff_bar_stripe_spacing_px: 3.0,
                selection_diff_bar_collapsed_opacity: 0.7,
                selection_diff_bar_expanded_opacity: 1.0,
                selection_trailing_width_factor: 0.5,
            },
            checkbox: CheckboxStyles {
                checkbox_size_factor: 1.2,
                checkbox_text_gap_px: 10.0,
                checkbox_parent_prefix_chars: 2.0,
                checkbox_radius_px: 5.0,
                checkbox_border_width_px: 1.5,
                checkbox_border_color,
                checkbox_checked_bg,
                checkbox_check_icon_scale: 0.52,
                checkbox_check_icon_color: theme.text_on_accent,
                checkbox_default_opacity: 0.9,
                checkbox_hover_opacity: 1.0,
                checkbox_hit_padding_px: 2.0,
            },
            table: TableStyles {
                table_cell_padding_px: 5.0,
                table_border_width_px: 1.0,
                table_border_color: theme.border_decorative,
                table_outer_radius_px: 5.0,
                table_header_bg: theme.bg_code,
                front_matter_key_text_color: with_alpha(theme.text_bright, 1.0),
                front_matter_key_padding_right_px: 10.0,
                table_handle_size_px: 6.0,
                table_handle_color: theme.border_decorative,
                table_handle_dot_size_px: 2.0,
                table_handle_gap_px: 1.0,
                table_handle_highlight_width_px: 2.0,
                table_drag_indicator_width_px: 2.0,
                table_selection_border_width_px: 2.0,
            },
            gutter: GutterStyles {
                gutter_line_number_min_width_px: 45.0,
                gutter_line_number_max_width_px: 50.0,
                gutter_line_number_font_size_px: ui_font_size_px,
                gutter_line_number_vertical_offset_px: 0.0,
                gutter_line_number_right_gap_px: 0.0,
                gutter_column_gap_px: 5.0,
                gutter_line_number_text_color: with_alpha(theme.text_secondary, 0.48),
                gutter_line_number_active_text_color: theme.text_bright,
                gutter_fold_icon_size_px: 16.0,
                gutter_fold_icon_offset_px: 10.0,
                gutter_fold_hit_padding_px: 3.0,
                gutter_fold_default_opacity: 0.9,
                gutter_fold_folded_opacity: 0.9,
                gutter_fold_hover_opacity: 1.0,
                gutter_fold_animation_ms: 140,
                gutter_fold_folded_color: theme.text_bright,
                gutter_fold_hover_color: theme.text_secondary,
                gutter_heading_color: theme.gutter_heading,
                gutter_heading_opacity: 0.20,
                gutter_heading_h1_icon_size_px: 22.0,
                gutter_heading_h2_icon_size_px: 20.0,
                gutter_heading_h3_icon_size_px: 18.0,
                gutter_heading_h4_icon_size_px: 16.0,
                gutter_heading_icon_gap_px: 0.0,
                gutter_heading_icon_offset_px: 10.0,
                gutter_heading_max_total_width_px: 56.0,
            },
            image_preview: ImagePreviewStyles {
                image_border_radius_px: 0.0,
                image_active_border_width_px: 2.0,
                image_active_border_color: theme.border_primary,
                image_gap_top_px: 4.0,
                image_gap_bottom_px: 4.0,
                image_edit_icon_size_px: 13.0,
                image_edit_icon_gap_px: 10.0,
                image_edit_icon_color: theme.text_secondary,
                image_edit_icon_normal_opacity: 0.7,
                image_edit_icon_hover_opacity: 0.55,
            },
            hr: HrStyles {
                hr_dash_length_px: 6.0,
                hr_gap_length_px: 4.0,
                hr_height_px: 1.0,
                hr_color: theme.border_decorative,
            },
            ime: ImeStyles {
                ime_underline_height_px: 1.0,
                ime_underline_color: theme.text_primary,
            },
            context_menu: ContextMenuStyles {
                menu_bg: ui_theme.bg_elevated,
                menu_border_width_px: 1.0,
                menu_border_color: ui_theme.border_primary,
                menu_radius_px: 8.0,
                menu_shadow: ui_theme.bg_primary.l < 0.5,
                menu_padding_y_px: 6.0,
                menu_submenu_gap_px: 2.0,
                menu_max_height_margin_px: 0.0,
                menu_opacity_animation_ms: 120,
                menu_item_height_px: 24.0,
                menu_item_padding_x_px: 10.0,
                menu_item_font_size_px: ui_font_size_sm_px,
                menu_item_text_color: with_alpha(ui_theme.text_bright, 0.84),
                menu_item_primary_text_color: with_alpha(ui_theme.text_bright, 0.96),
                menu_item_danger_text_color: with_alpha(ui_theme.text_danger, 0.84),
                menu_item_hover_bg: ui_theme.bg_hover,
                menu_item_selected_bg: ui_theme.bg_hover,
                menu_shortcut_font_size_px: ui_font_size_sm_px - 0.5,
                menu_shortcut_font_family: if cfg!(target_os = "macos") {
                    "BlinkMacSystemFont"
                } else {
                    crate::platform::fonts::SYSTEM_UI
                },
                menu_shortcut_text_color: with_alpha(ui_theme.text_secondary, 0.78),
                menu_shortcut_gap_px: 3.0,
                menu_shortcut_key_min_width_px: 14.0,
                menu_shortcut_key_height_px: 16.0,
                menu_shortcut_key_padding_x_px: 4.0,
                menu_shortcut_key_radius_px: 4.0,
                menu_shortcut_key_bg: with_alpha(ui_theme.bg_hover, 0.95),
                menu_format_icon_size_px: 13.0,
                menu_format_icon_gap_px: 6.0,
                menu_format_icon_color: with_alpha(ui_theme.text_secondary, 0.8),
                menu_format_active_text_color: with_alpha(ui_theme.text_primary, 0.92),
                menu_separator_height_px: 1.0,
                menu_separator_margin_y_px: 0.0,
                menu_separator_color: ui_theme.border_primary,
                menu_sidebar_width_px: 180.0,
                menu_format_width_px: 210.0,
                menu_export_width_px: 148.0,
                menu_tab_width_px: 180.0,
            },
            dialog: DialogStyles {
                dialog_backdrop_color: with_alpha(ui_theme.bg_primary, 0.3),
                dialog_bg: ui_theme.bg_elevated,
                dialog_border_width_px: 1.0,
                dialog_border_color: ui_theme.border_primary,
                dialog_radius_px: 12.0,
                dialog_width_px: 320.0,
                dialog_padding_x_px: 20.0,
                dialog_padding_y_px: 18.0,
                dialog_gap_px: 12.0,
                dialog_title_font_size_px: ui_font_size_lg_px,
                dialog_title_text_color: ui_theme.text_bright,
                dialog_message_font_size_px: ui_font_size_px,
                dialog_message_text_color: ui_theme.text_secondary,
                dialog_btn_height_px: 28.0,
                dialog_btn_padding_x_px: 14.0,
                dialog_btn_font_size_px: ui_font_size_sm_px,
                dialog_btn_radius_px: 6.0,
                dialog_btn_gap_px: 8.0,
                dialog_btn_hover_opacity: 0.9,
                dialog_btn_primary_bg: ui_theme.text_accent,
                dialog_btn_primary_text_color: ui_theme.text_on_accent,
                dialog_btn_primary_hover_bg: ui_theme.text_accent,
                dialog_btn_secondary_bg: ui_theme.bg_hover,
                dialog_btn_secondary_text_color: ui_theme.text_primary,
                dialog_btn_secondary_hover_bg: ui_theme.bg_selected,
                dialog_btn_danger_bg: ui_theme.text_danger,
                dialog_btn_danger_text_color: ui_theme.text_on_accent,
                dialog_btn_danger_hover_bg: ui_theme.text_danger,
            },
            notification: NotificationStyles {
                notification_width_px: 340.0,
                notification_margin_right_px: 16.0,
                notification_margin_bottom_px: 44.0,
                notification_gap_px: 8.0,
                notification_padding_x_px: 14.0,
                notification_padding_y_px: 10.0,
                notification_radius_px: 0.0,
                notification_border_width_px: 1.0,
                notification_bg: ui_theme.bg_elevated,
                notification_border_color: ui_theme.border_primary,
                notification_text_color: ui_theme.text_primary,
                notification_font_size_px: ui_font_size_px,
                notification_hover_opacity: 0.9,
                notification_dismiss_after_ms: 5000,
                notification_max_visible: 4,
            },
            workspace_switcher: WorkspaceSwitcherStyles {
                ws_bar_bg: gpui::transparent_black(),
                ws_bar_border_color: sidebar_theme.border_secondary,
                ws_bar_padding_x_px: 14.0,
                ws_bar_gap_px: 6.0,
                ws_bar_hover_bg: sidebar_theme.bg_hover,
                ws_menu_width_px: 220.0,
                ws_menu_item_check_size_px: 12.0,
                ws_menu_item_check_color: ui_theme.text_secondary,
                ws_menu_item_action_icon_size_px: 14.0,
                ws_menu_item_action_icon_color: ui_theme.text_secondary,
                ws_menu_item_action_icon_gap_px: 10.0,
                ws_menu_item_close_size_px: 14.0,
                ws_menu_item_close_color: ui_theme.text_secondary,
                ws_menu_item_close_hover_color: ui_theme.text_primary,
                ws_menu_item_close_hover_bg: ui_theme.bg_hover,
            },
            text_input: TextInputStyles {
                text_input_height_px: 24.0,
                text_input_padding_left_px: 6.0,
                text_input_padding_right_px: 6.0,
                text_input_font_size_px: ui_font_size_px,
                text_input_bg: text_input_theme.bg_primary,
                text_input_hover_bg: None,
                text_input_focus_bg: text_input_theme.bg_elevated,
                text_input_text_color: text_input_theme.text_bright,
                text_input_placeholder_color: text_input_theme.text_secondary,
                text_input_border_width_px: 1.0,
                text_input_border_color: text_input_theme.border_primary,
                text_input_focus_border_color: text_input_theme.text_accent,
                text_input_radius_px: 4.0,
                text_input_cursor_color: text_input_theme.cursor,
                text_input_cursor_width_px: 2.0,
                text_input_cursor_height_ratio: 1.0,
                text_input_cursor_height_adjust_px: 0.0,
                text_input_selection_color: text_input_theme.selection,
                text_input_center: false,
                text_input_right: false,
                text_input_menu_bg: text_input_theme.bg_elevated,
                text_input_menu_text_color: text_input_theme.text_primary,
                text_input_menu_font_size_px: ui_font_size_sm_px,
                text_input_menu_border_width_px: 1.0,
                text_input_menu_border_color: text_input_theme.border_primary,
                text_input_menu_radius_px: 8.0,
                text_input_menu_padding_y_px: 6.0,
                text_input_menu_item_height_px: 24.0,
                text_input_menu_item_padding_x_px: 10.0,
                text_input_menu_hover_bg: text_input_theme.bg_hover,
                text_input_menu_separator_color: text_input_theme.border_primary,
                text_input_menu_width_px: 160.0,
                text_input_menu_shadow: text_input_shadow,
            },
            search: SearchStyles {
                search_bar_width_px: 312.0,
                search_bar_padding_px: 5.0,
                search_bar_bg: ui_theme.bg_primary,
                search_bar_border_width_px: 1.0,
                search_bar_border_color: ui_theme.border_primary,
                search_bar_radius_px: 8.0,
                search_bar_shadow: true,
                search_bar_gap_px: 4.0,
                search_bar_offset_top_px: 8.0,
                search_bar_offset_right_px: 8.0,
                search_input_gap_px: 4.0,
                search_input_height_px: 20.0,
                search_input_font_size_px: ui_font_size_sm_px,
                search_input_icon_size_px: 12.0,
                search_input_icon_color: ui_theme.text_secondary,
                search_input_icon_gap_px: 3.0,
                search_input_left_inset_px: 24.0,
                search_input_right_inset_px: 8.0,
                search_input_counter_min_width_px: 40.0,
                search_input_focus_bg: gpui::transparent_black(),
                search_input_cursor_height_ratio: 0.72,
                search_option_btn_size_px: 18.0,
                search_option_btn_icon_size_px: 12.0,
                search_option_btn_radius_px: 4.0,
                search_option_btn_active_bg: ui_theme.bg_selected,
                search_option_btn_hover_bg: ui_theme.bg_hover,
                search_option_btn_icon_color: ui_theme.text_secondary,
                search_option_btn_active_icon_color: ui_theme.text_accent,
                search_counter_font_size_px: ui_font_size_sm_px,
                search_counter_text_color: ui_theme.text_secondary,
                search_nav_btn_size_px: 18.0,
                search_nav_btn_icon_size_px: 12.0,
                search_nav_btn_icon_color: ui_theme.text_secondary,
                search_nav_btn_hover_bg: ui_theme.bg_hover,
                search_match_bg: with_alpha(theme.highlight_bg, 0.18),
                search_match_active_bg: with_alpha(theme.bold, 0.62),
                search_match_text_color: ui_theme.text_bright,
                search_match_active_text_color: ui_theme.text_bright,
                search_match_border_width_px: 1.0,
                search_match_border_color: with_alpha(theme.highlight_bg, 0.5),
                search_scrollbar_marker_width_px: 3.0,
                search_scrollbar_marker_height_px: 2.0,
                search_scrollbar_marker_color: with_alpha(theme.highlight_bg, 0.6),
                search_replace_btn_height_px: 20.0,
                search_replace_btn_padding_x_px: 7.0,
                search_replace_btn_font_size_px: ui_font_size_xs_px,
                search_replace_btn_bg: ui_theme.bg_hover,
                search_replace_btn_text_color: ui_theme.text_primary,
                search_replace_btn_hover_bg: ui_theme.bg_selected,
                search_close_btn_size_px: 18.0,
                search_close_btn_icon_color: ui_theme.text_secondary,
                search_close_btn_hover_bg: ui_theme.bg_hover,
            },
            workspace_search: WorkspaceSearchStyles {
                ws_search_padding_px: 10.0,
                ws_search_gap_px: 8.0,
                ws_search_input_gap_px: 4.0,
                ws_search_input_left_inset_px: 28.0,
                ws_search_input_right_inset_px: 108.0,
                ws_search_input_icon_size_px: 13.0,
                ws_search_input_icon_color: sidebar_theme.text_secondary,
                ws_search_input_icon_gap_px: 4.0,
                ws_search_input_height_px: 32.0,
                ws_search_input_radius_px: 6.0,
                ws_search_input_bg: sidebar_theme.bg_elevated,
                ws_search_input_hover_bg: sidebar_theme.bg_hover,
                ws_search_input_focus_bg: sidebar_theme.bg_tab_active,
                ws_search_input_border_width_px: 1.0,
                ws_search_input_border_color: sidebar_theme.border_primary,
                ws_search_input_cursor_height_ratio: 0.72,
                ws_search_input_cursor_height_adjust_px: -4.0,
                ws_search_file_header_height_px: 28.0,
                ws_search_file_header_bg: sidebar_theme.bg_primary,
                ws_search_file_header_border_width_px: 1.0,
                ws_search_file_header_border_color: sidebar_theme.border_primary,
                ws_search_file_header_font_size_px: ui_font_size_sm_px,
                ws_search_file_header_text_color: with_alpha(sidebar_theme.text_bright, 0.94),
                ws_search_file_header_dir_gap_px: 5.0,
                ws_search_file_header_dir_text_color: with_alpha(
                    sidebar_theme.text_secondary,
                    0.72,
                ),
                ws_search_file_header_icon_size_px: 14.0,
                ws_search_file_header_count_font_size_px: ui_font_size_xs_px,
                ws_search_file_header_count_text_color: sidebar_theme.text_secondary,
                ws_search_match_height_px: 24.0,
                ws_search_match_padding_x_px: 14.0,
                ws_search_match_font_size_px: ui_font_size_sm_px,
                ws_search_match_text_color: with_alpha(sidebar_theme.text_bright, 0.94),
                ws_search_match_highlight_bg: with_alpha(theme.highlight_bg, 0.3),
                ws_search_match_highlight_text_color: sidebar_theme.text_bright,
                ws_search_match_line_number_color: sidebar_theme.text_secondary,
                ws_search_match_hover_bg: sidebar_theme.bg_hover,
                ws_search_match_active_bg: sidebar_theme.bg_selected,
                ws_search_progress_height_px: 2.0,
                ws_search_progress_color: sidebar_theme.text_accent,
                ws_search_summary_height_px: 22.0,
                ws_search_summary_margin_bottom_px: 4.0,
                ws_search_summary_font_size_px: ui_font_size_xs_px,
                ws_search_summary_text_color: sidebar_theme.text_secondary,
                ws_search_btn_size_px: 22.0,
                ws_search_btn_icon_size_px: 14.0,
                ws_search_btn_icon_color: sidebar_theme.text_secondary,
                ws_search_btn_active_icon_color: sidebar_theme.text_accent,
                ws_search_btn_hover_bg: sidebar_theme.bg_hover,
                ws_search_btn_active_bg: sidebar_theme.bg_selected,
            },
            quick_open: QuickOpenStyles {
                quick_open_backdrop_bg: with_alpha(ui_theme.bg_primary, 0.5),
                quick_open_panel_width_px: 640.0,
                quick_open_panel_top_offset_px: 48.0,
                quick_open_panel_radius_px: 8.0,
                quick_open_panel_border_color: ui_theme.border_primary,
                quick_open_panel_bg: ui_theme.bg_elevated,
                quick_open_input_padding_x_px: 8.0,
                quick_open_input_padding_y_px: 6.0,
                quick_open_input_text_padding_left_px: 2.0,
                quick_open_input_bg: gpui::transparent_black(),
                quick_open_input_focus_bg: gpui::transparent_black(),
                quick_open_input_text_color: ui_theme.text_primary,
                quick_open_input_cursor_height_ratio: 0.64,
                quick_open_input_cursor_height_adjust_px: 2.0,
                quick_open_results_max_height_px: 360.0,
                quick_open_result_row_height_px: 28.0,
                quick_open_result_padding_x_px: 10.0,
                quick_open_result_gap_y_px: 2.0,
                quick_open_result_name_font_size_px: ui_font_size_sm_px,
                quick_open_result_path_font_size_px: ui_font_size_xs_px - 1.0,
                quick_open_result_hover_bg: ui_theme.bg_hover,
                quick_open_result_selected_bg: ui_theme.bg_selected,
                quick_open_result_text_color: with_alpha(ui_theme.text_bright, 0.96),
                quick_open_result_secondary_text_color: with_alpha(ui_theme.text_secondary, 0.68),
                quick_open_result_shortcut_bg: with_alpha(
                    ui_theme.bg_code,
                    if is_light { 0.12 } else { 0.28 },
                ),
                quick_open_result_match_text_color: ui_theme.text_bright,
                quick_open_empty_height_px: 48.0,
                quick_open_empty_font_size_px: ui_font_size_xs_px,
                quick_open_empty_text_color: ui_theme.text_secondary,
            },
            settings_panel: SettingsPanelStyles {
                settings_title_version_gap_px: 10.0,
                settings_title_version_font_size_px: ui_font_size_sm_px,
                settings_title_version_text_color: settings_secondary_text_color,
                settings_title_version_margin_bottom_px: 5.0,
                settings_primary_text_color,
                settings_secondary_text_color,
                settings_header_gap_px: 12.0,
                settings_header_actions_gap_px: 8.0,
                settings_header_button_max_width_px: 150.0,
                settings_site_button_height_px: 30.0,
                settings_site_button_padding_x_px: 12.0,
                settings_site_button_radius_px: 8.0,
                settings_site_button_border_width_px: 1.0,
                settings_site_button_gap_px: 6.0,
                settings_site_button_icon_size_px: 15.0,
                settings_site_button_font_size_px: ui_font_size_sm_px,
                settings_site_button_text_color: settings_theme.text_primary,
                settings_site_button_icon_color: settings_theme.text_secondary,
                settings_site_button_update_text_color: settings_update_color,
                settings_site_button_update_icon_color: settings_update_color,
                settings_site_button_bg: settings_theme.bg_secondary,
                settings_site_button_hover_bg: settings_theme.bg_hover,
                settings_site_button_border_color: settings_theme.border_primary,
                settings_option_button_height_px: 28.0,
                settings_option_button_padding_x_px: 12.0,
                settings_option_button_radius_px: 999.0,
                settings_option_button_border_width_px: 1.0,
                settings_option_button_gap_px: 8.0,
                settings_option_button_font_size_px: ui_font_size_sm_px,
                // Match the settings text-input text color so picker trigger
                // buttons don't read lighter than typed values.
                settings_option_button_text_color: settings_theme.text_bright,
                settings_option_button_active_text_color: settings_active_text_color,
                settings_option_button_bg: settings_theme.bg_secondary,
                settings_option_button_hover_bg: settings_theme.bg_hover,
                settings_option_button_active_bg: settings_active_bg,
                settings_option_button_border_color: settings_theme.border_primary,
                settings_option_button_active_border_color: settings_active_bg,
                settings_contrast_border_color: settings_theme.border_primary,
                settings_input_height_px: 30.0,
                settings_input_radius_px: 6.0,
                settings_stepper_input_radius_px: 0.0,
                settings_theme_group_gap_px: 10.0,
                settings_theme_toggle_height_px: 30.0,
                settings_theme_toggle_padding_x_px: 12.0,
                settings_theme_toggle_gap_px: 6.0,
                settings_theme_toggle_radius_px: 6.0,
                settings_theme_toggle_border_width_px: 1.0,
                settings_theme_toggle_icon_size_px: 15.0,
                settings_theme_option_height_px: 30.0,
                settings_theme_option_padding_x_px: 12.0,
                settings_theme_option_radius_px: 6.0,
                settings_theme_option_border_width_px: 1.0,
                settings_theme_option_font_size_px: ui_font_size_sm_px,
                settings_theme_option_hover_bg: settings_theme.bg_hover,
                settings_theme_option_active_bg: settings_theme.bg_selected,
                settings_theme_option_active_text_color: settings_theme.text_bright,
                settings_theme_option_text_color: settings_theme.text_secondary,
                settings_theme_option_border_color: settings_theme.border_primary,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Styles;
    use crate::theme::Theme;

    #[test]
    fn file_tree_and_file_list_share_the_same_background() {
        let theme = Theme::one_dark();
        let styles = Styles::from_theme(&theme, &theme);

        assert_eq!(styles.sidebar.sidebar_bg, styles.sidebar.file_list_bg);
        assert_eq!(styles.sidebar.sidebar_bg, styles.sidebar.file_list_item_bg);
        assert_eq!(styles.sidebar.file_list_item_thumbnail_border_width_px, 0.0);
    }

    #[test]
    fn editor_body_is_one_point_smaller_without_changing_heading_sizes() {
        let theme = Theme::one_dark();
        let styles = Styles::from_theme(&theme, &theme);

        assert_eq!(styles.editor.editor_font_size_px, 14.0);
        assert_eq!(styles.editor.editor_h1_font_size_px, 21.0);
        assert_eq!(styles.editor.editor_h2_font_size_px, 19.5);
        assert_eq!(styles.editor.editor_h6_font_size_px, 15.0);
    }

    #[test]
    fn quote_block_padding_matches_code_block_left_padding() {
        let theme = Theme::one_dark();
        let styles = Styles::from_theme(&theme, &theme);

        assert_eq!(styles.quote_block.quote_block_border_width_px, 4.0);
        assert_eq!(styles.quote_block.quote_block_padding_top_px, 0.0);
        assert_eq!(styles.quote_block.quote_block_padding_bottom_px, 0.0);
        assert_eq!(styles.quote_block.quote_block_padding_left_px, 12.0);
        assert_eq!(styles.quote_block.quote_block_padding_right_px, 10.0);
    }

    #[test]
    fn preview_borders_use_primary_theme_border_without_accent_glow() {
        let theme = Theme::one_dark();
        let styles = Styles::from_theme(&theme, &theme);

        assert_eq!(
            styles.image_preview.image_active_border_color,
            theme.border_primary
        );
        assert_eq!(
            styles.code_block.generated_preview_action_border_color,
            crate::theme::with_alpha(theme.border_primary, 0.38)
        );
        assert!(styles.code_block.generated_preview_action_border_color.a < 0.5);
    }

    #[test]
    fn utility_panel_colors_follow_the_active_light_theme() {
        let light_theme = Theme::one_light();
        let dark_ui_theme = Theme::one_dark();
        let styles = Styles::from_theme(&light_theme, &dark_ui_theme);
        let panel = &styles.utility_panel;
        // Light palettes get their muted foregrounds and hairlines deepened
        // before any token is derived from them.
        let light_theme = light_theme.with_light_surface_contrast();

        assert_eq!(panel.utility_panel_bg, light_theme.bg_secondary);
        assert_eq!(panel.utility_panel_border_color, light_theme.border_primary);
        assert_eq!(
            panel.utility_panel_header_text_color,
            light_theme.text_primary
        );
        assert_eq!(
            panel.utility_panel_item_label_text_color,
            light_theme.text_secondary
        );
        assert_eq!(
            panel.utility_panel_item_value_text_color,
            light_theme.text_primary
        );
        assert_eq!(
            panel.utility_panel_button_border_color,
            light_theme.border_primary
        );
        assert_eq!(
            panel.utility_panel_button_text_color,
            light_theme.text_primary
        );
        assert_eq!(panel.utility_panel_button_hover_bg, light_theme.bg_hover);
        assert_eq!(
            panel.utility_panel_resize_indicator_color,
            light_theme.border_primary
        );
        assert_eq!(
            panel.utility_panel_resize_active_indicator_color,
            light_theme.text_accent
        );
        assert_eq!(
            styles.outline.outline_rail_label_color,
            light_theme.text_secondary
        );
        assert_eq!(
            styles.outline.outline_rail_label_hover_color,
            light_theme.text_bright
        );
        assert_eq!(
            styles.outline.outline_rail_label_active_color,
            light_theme.text_bright
        );
        assert_eq!(
            styles.outline.outline_rail_tick_color,
            crate::theme::with_alpha(light_theme.text_primary, 0.3)
        );
    }

    #[test]
    fn moeka_settings_controls_follow_the_same_appearance() {
        for kind in crate::theme::ThemeKind::dark_themes()
            .iter()
            .chain(crate::theme::ThemeKind::light_themes())
            .copied()
            .filter(|kind| kind.is_moeka_retro())
        {
            let theme = kind.build();
            let mut styles = Styles::from_theme(&theme, &theme);
            styles.apply_moeka_retro_chrome(&theme, &theme);

            assert_eq!(
                styles.sidebar.sidebar_bg,
                theme.bg_secondary,
                "{}",
                kind.label()
            );
            assert_eq!(
                styles.utility_panel.utility_panel_bg,
                theme.bg_secondary,
                "{}",
                kind.label()
            );
            assert_eq!(styles.tab_bar.tab_bar_bg, theme.bg_bar, "{}", kind.label());
            assert_eq!(
                styles.settings_panel.settings_primary_text_color,
                theme.text_primary,
                "{}",
                kind.label()
            );
            assert_eq!(
                styles.settings_panel.settings_option_button_text_color,
                theme.text_primary,
                "{}",
                kind.label()
            );
            assert_eq!(
                styles.settings_panel.settings_option_button_bg,
                theme.bg_elevated,
                "{}",
                kind.label()
            );
            assert_eq!(
                styles.settings_panel.settings_option_button_active_bg,
                theme.bg_selected,
                "{}",
                kind.label()
            );
            assert_eq!(
                styles.settings_panel.settings_contrast_border_color,
                theme.text_primary,
                "{}",
                kind.label()
            );
            assert_ne!(
                styles.settings_panel.settings_option_button_bg,
                theme.bg_primary,
                "{}",
                kind.label()
            );
            assert_eq!(styles.text_input.text_input_text_color, theme.text_bright);
        }
    }

    #[test]
    fn moeka_print_ui_borders_are_two_pixels() {
        let editor_theme = Theme::moeka_print_dark();
        let mut styles = Styles::from_theme(&editor_theme, &editor_theme);
        styles.apply_moeka_retro_chrome(&editor_theme, &editor_theme);

        let borders = [
            ("window", styles.window.window_border_width_px),
            ("path bar", styles.editor.editor_path_bar_border_width_px),
            ("editor match", styles.editor.editor_match_border_width_px),
            (
                "sidebar input",
                styles.sidebar.sidebar_edit_input_border_width_px,
            ),
            (
                "sidebar empty button",
                styles.sidebar.sidebar_empty_btn_border_width_px,
            ),
            (
                "sidebar divider",
                styles.sidebar.sidebar_resize_indicator_width_px,
            ),
            (
                "sidebar active divider",
                styles.sidebar.sidebar_resize_active_indicator_width_px,
            ),
            (
                "file list header",
                styles.sidebar.file_list_header_border_width_px,
            ),
            (
                "file list separator",
                styles.sidebar.file_list_item_separator_width_px,
            ),
            (
                "file list thumbnail",
                styles.sidebar.file_list_item_thumbnail_border_width_px,
            ),
            (
                "utility panel",
                styles.utility_panel.utility_panel_border_width_px,
            ),
            (
                "utility button",
                styles.utility_panel.utility_panel_button_border_width_px,
            ),
            (
                "utility divider",
                styles.utility_panel.utility_panel_resize_indicator_width_px,
            ),
            (
                "utility active divider",
                styles
                    .utility_panel
                    .utility_panel_resize_active_indicator_width_px,
            ),
            ("tab bar", styles.tab_bar.tab_bar_border_width_px),
            ("tab item", styles.tab_bar.tab_item_border_width_px),
            (
                "tab history",
                styles.tab_bar.tab_history_group_border_width_px,
            ),
            (
                "whiteboard toolbar",
                styles.toolbar.toolbar_whiteboard_border_width_px,
            ),
            ("tooltip", styles.tooltip.tooltip_border_width_px),
            ("code block", styles.code_block.code_block_border_width_px),
            (
                "code block split",
                styles.code_block.code_block_split_border_width_px,
            ),
            ("checkbox", styles.checkbox.checkbox_border_width_px),
            (
                "code copy button",
                styles.code_block.code_block_copy_button_border_width_px,
            ),
            (
                "preview action",
                styles.code_block.generated_preview_action_border_width_px,
            ),
            (
                "quote block",
                styles.quote_block.quote_block_border_width_px,
            ),
            (
                "diff revert button",
                styles
                    .selection
                    .selection_diff_revert_button_border_width_px,
            ),
            ("table", styles.table.table_border_width_px),
            (
                "table selection",
                styles.table.table_selection_border_width_px,
            ),
            (
                "active image",
                styles.image_preview.image_active_border_width_px,
            ),
            ("dialog", styles.dialog.dialog_border_width_px),
            (
                "notification",
                styles.notification.notification_border_width_px,
            ),
            ("context menu", styles.context_menu.menu_border_width_px),
            ("text input", styles.text_input.text_input_border_width_px),
            (
                "text input context menu",
                styles.text_input.text_input_menu_border_width_px,
            ),
            ("search bar", styles.search.search_bar_border_width_px),
            ("search match", styles.search.search_match_border_width_px),
            (
                "workspace search",
                styles.workspace_search.ws_search_input_border_width_px,
            ),
            (
                "workspace search header",
                styles
                    .workspace_search
                    .ws_search_file_header_border_width_px,
            ),
            (
                "settings site button",
                styles.settings_panel.settings_site_button_border_width_px,
            ),
            (
                "settings option",
                styles.settings_panel.settings_option_button_border_width_px,
            ),
            (
                "settings theme toggle",
                styles.settings_panel.settings_theme_toggle_border_width_px,
            ),
            (
                "settings theme option",
                styles.settings_panel.settings_theme_option_border_width_px,
            ),
        ];

        for (name, width) in borders {
            assert_eq!(width, 2.0, "{name}");
        }
    }

    #[test]
    fn every_moeka_retro_variant_uses_two_pixel_chrome() {
        for kind in crate::theme::ThemeKind::dark_themes()
            .iter()
            .chain(crate::theme::ThemeKind::light_themes())
            .copied()
            .filter(|kind| kind.is_moeka_retro())
        {
            let editor = kind.build();
            let mut styles = Styles::from_theme(&editor, &editor);
            styles.apply_moeka_retro_chrome(&editor, &editor);

            assert_eq!(
                styles.window.window_border_width_px,
                2.0,
                "{}",
                kind.label()
            );
            assert_eq!(
                styles.tab_bar.tab_item_border_width_px,
                2.0,
                "{}",
                kind.label()
            );
            assert_eq!(
                styles.sidebar.file_list_item_thumbnail_border_width_px,
                2.0,
                "{}",
                kind.label()
            );
            assert_eq!(
                styles.settings_panel.settings_option_button_border_width_px,
                2.0,
                "{}",
                kind.label()
            );
            assert_eq!(
                styles.settings_panel.settings_contrast_border_color,
                editor.text_primary,
                "{}",
                kind.label()
            );
            assert_eq!(
                styles.utility_panel.utility_panel_button_text_color,
                editor.text_primary,
                "{}",
                kind.label()
            );
            assert_ne!(
                styles.utility_panel.utility_panel_button_text_color,
                styles.utility_panel.utility_panel_bg,
                "{}",
                kind.label()
            );
            assert_eq!(
                styles.editor.editor_inline_code_bg.a,
                if kind.is_dark() { 0.16 } else { 0.42 },
                "{}",
                kind.label()
            );
        }
    }

    #[test]
    fn visible_component_colors_come_from_theme_tokens() {
        let source = include_str!("style.rs")
            .replace(concat!("crate::hex_", "hsla!(\"#ffffff\")"), "")
            .replace(concat!("crate::hex_", "hsla!(\"#d9dee3\")"), "");
        let forbidden = [
            concat!("hex_", "hsla!"),
            concat!("hex_to_", "hsla("),
            concat!("gpui::", "hsla("),
            concat!("gpui::", "rgb("),
            concat!("Hsla::", "black("),
            concat!("Hsla::", "white("),
            concat!("gpui::", "white("),
            concat!("color.", "l ="),
        ];

        for pattern in forbidden {
            assert!(
                !source.contains(pattern),
                "visible component color must use a Theme token: {pattern}"
            );
        }
    }
}
