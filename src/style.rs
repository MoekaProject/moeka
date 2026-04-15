#![allow(dead_code)]

use std::time::Duration;

use gpui::Hsla;

use crate::theme::{Theme, with_alpha};

/// Per-component style configuration, derived from the active theme.
///
/// Naming convention: `{功能}_{位置}_{状态}_{样式}_{单位}`
///   功能 = editor, sidebar, tab, toolbar, cursor, menu, gutter, …
///   位置 = item, header, bar, container (省略 = 组件自身)
///   状态 = hover, active, selected (默认状态省略)
///   样式 = bg, text_color, font_size, width, height, padding, gap, radius, opacity, border_width, border_color, …
///   单位 = px, ms, factor (无单位 = 无量纲比例)
pub struct Styles {
    pub window: WindowStyles,
    pub editor: EditorStyles,
    pub sidebar: SidebarStyles,
    pub outline: OutlineStyles,
    pub tab_bar: TabBarStyles,
    pub toolbar: ToolbarStyles,
    pub cursor: CursorStyles,
    pub code_block: CodeBlockStyles,
    pub checkbox: CheckboxStyles,
    pub table: TableStyles,
    pub gutter: GutterStyles,
    pub context_menu: ContextMenuStyles,
    pub selection: SelectionStyles,
    pub hr: HrStyles,
    pub ime: ImeStyles,
    pub image_preview: ImagePreviewStyles,
    pub dialog: DialogStyles,
    pub workspace_switcher: WorkspaceSwitcherStyles,
}

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
    pub editor_line_height_factor: f32,
    pub editor_padding_left_px: f32,
    pub editor_padding_top_px: f32,
    pub editor_padding_right_px: f32,
    pub editor_padding_bottom_px: f32,
    pub editor_max_width_px: f32,
    pub editor_bg: Hsla,
    pub editor_text_color: Hsla,
    pub editor_scale_step: f32,
    pub editor_scale_min: f32,
    pub editor_scale_max: f32,
}

// ── Sidebar ─────────────────────────────────────────────────────────

pub struct SidebarStyles {
    // Container
    pub sidebar_width_px: f32,
    pub sidebar_bg: Hsla,
    pub sidebar_top_spacer_height_px: f32,
    pub sidebar_bottom_spacer_height_px: f32,
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
    pub sidebar_header_icon_color: Hsla,
    pub sidebar_header_font_size_px: f32,
    pub sidebar_header_text_color: Hsla,
    pub sidebar_header_hover_bg: Hsla,
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
    pub sidebar_item_icon_size_px: f32,
    pub sidebar_item_icon_color: Hsla,
    pub sidebar_item_selected_icon_color: Hsla,
    // File tree item — label
    pub sidebar_item_font_size_px: f32,
    pub sidebar_item_text_color: Hsla,
    pub sidebar_item_selected_text_color: Hsla,
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
    // Indent alignment guides
    pub sidebar_indent_guide_color: Hsla,
    pub sidebar_indent_guide_width_px: f32,
    pub sidebar_indent_guide_dash_length_px: f32,
    pub sidebar_indent_guide_gap_length_px: f32,
    // Focus ring
    pub sidebar_item_focus_border_color: Hsla,
    // Active border (selected item)
    pub sidebar_item_active_border_color: Hsla,
}

// ── Outline ─────────────────────────────────────────────────────────

pub struct OutlineStyles {
    // Container
    pub outline_width_px: f32,
    pub outline_bg: Hsla,
    pub outline_border_width_px: f32,
    pub outline_border_color: Hsla,
    pub outline_top_spacer_height_px: f32,
    pub outline_bottom_spacer_height_px: f32,
    // Header
    pub outline_header_height_px: f32,
    pub outline_header_padding_x_px: f32,
    pub outline_header_font_size_px: f32,
    pub outline_header_text_color: Hsla,
    // Item
    pub outline_item_height_px: f32,
    pub outline_item_indent_step_px: f32,
    pub outline_item_padding_left_px: f32,
    pub outline_item_padding_right_px: f32,
    pub outline_item_font_size_px: f32,
    pub outline_item_text_color: Hsla,
    pub outline_item_active_text_color: Hsla,
    pub outline_item_active_bg: Hsla,
    pub outline_item_hover_bg: Hsla,
    // Resize handle
    pub outline_resize_handle_width_px: f32,
    pub outline_resize_indicator_width_px: f32,
    pub outline_resize_active_indicator_width_px: f32,
    pub outline_resize_indicator_color: Hsla,
    pub outline_resize_active_indicator_color: Hsla,
    pub outline_resize_min_width_px: f32,
    pub outline_resize_max_width_px: f32,
    // Indent alignment guides
    pub outline_indent_guide_color: Hsla,
    pub outline_indent_guide_width_px: f32,
    pub outline_indent_guide_dash_length_px: f32,
    pub outline_indent_guide_gap_length_px: f32,
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
    pub tab_item_icon_color: Hsla,
    pub tab_item_icon_active_opacity: f32,
    pub tab_item_icon_inactive_opacity: f32,
    // Tab item — label
    pub tab_item_font_size_px: f32,
    pub tab_item_text_color: Hsla,
    pub tab_item_active_text_color: Hsla,
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
    // Left spacer when sidebar is hidden
    pub tab_spacer_width_px: f32,
}

// ── Toolbar ─────────────────────────────────────────────────────────

pub struct ToolbarStyles {
    pub toolbar_offset_left_px: f32,
    pub toolbar_offset_top_px: f32,
    pub toolbar_gap_px: f32,
    pub toolbar_primary_btn_size_px: f32,
    pub toolbar_primary_icon_size_px: f32,
    pub toolbar_primary_hover_opacity: f32,
    pub toolbar_secondary_btn_size_px: f32,
    pub toolbar_secondary_icon_size_px: f32,
    pub toolbar_secondary_hover_opacity: f32,
    pub toolbar_btn_shadow: bool,
    pub toolbar_btn_bg: Hsla,
    pub toolbar_btn_radius_px: f32,
}

// ── Cursor ──────────────────────────────────────────────────────────

pub struct CursorStyles {
    pub cursor_color: Hsla,
    pub cursor_width_px: f32,
    pub cursor_blink_tick_ms: u64,
    pub cursor_blink_pause_ms: u64,
    pub cursor_blink_visible_ms: u64,
    pub cursor_blink_transparent_ms: u64,
    pub cursor_opacity_min: f32,
    pub cursor_opacity_max: f32,
    pub cursor_opacity_step: f32,
}

impl CursorStyles {
    pub fn blink_tick(&self) -> Duration {
        Duration::from_millis(self.cursor_blink_tick_ms)
    }
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
    pub code_block_text_padding_left_px: f32,
    pub code_block_fence_clip_px: f32,
}

// ── Selection & Line Highlight ──────────────────────────────────────

pub struct SelectionStyles {
    pub selection_color: Hsla,
    pub selection_line_highlight_color: Hsla,
    pub selection_trailing_width_factor: f32,
}

// ── Checkbox ────────────────────────────────────────────────────────

pub struct CheckboxStyles {
    pub checkbox_size_factor: f32,
    pub checkbox_text_gap_px: f32,
    pub checkbox_radius_px: f32,
    pub checkbox_border_width_px: f32,
    pub checkbox_border_color: Hsla,
    pub checkbox_checked_bg: Hsla,
    pub checkbox_check_icon_scale: f32,
    pub checkbox_check_icon_color: Hsla,
    pub checkbox_hit_padding_px: f32,
}

// ── Table ───────────────────────────────────────────────────────────

pub struct TableStyles {
    pub table_cell_padding_px: f32,
    pub table_border_width_px: f32,
    pub table_border_color: Hsla,
    pub table_header_bg: Hsla,
}

// ── Gutter ──────────────────────────────────────────────────────────

pub struct GutterStyles {
    // Fold icon
    pub gutter_fold_icon_size_px: f32,
    pub gutter_fold_icon_offset_px: f32,
    pub gutter_fold_hit_padding_px: f32,
    pub gutter_fold_folded_opacity: f32,
    pub gutter_fold_hover_opacity: f32,
    pub gutter_fold_folded_color: Hsla,
    pub gutter_fold_hover_color: Hsla,
    // Heading hash icons
    pub gutter_heading_color: Hsla,
    pub gutter_heading_h1_icon_size_px: f32,
    pub gutter_heading_h2_icon_size_px: f32,
    pub gutter_heading_h3_icon_size_px: f32,
    pub gutter_heading_h4_icon_size_px: f32,
    pub gutter_heading_icon_gap_px: f32,
    pub gutter_heading_icon_offset_px: f32,
    // Blockquote icon
    pub gutter_blockquote_icon_size_px: f32,
    pub gutter_blockquote_icon_color: Hsla,
    pub gutter_blockquote_icon_offset_px: f32,
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

// ── Context Menu ────────────────────────────────────────────────────

pub struct ContextMenuStyles {
    // Container
    pub menu_bg: Hsla,
    pub menu_border_width_px: f32,
    pub menu_border_color: Hsla,
    pub menu_radius_px: f32,
    pub menu_shadow: bool,
    pub menu_padding_y_px: f32,
    pub menu_max_height_margin_px: f32,
    // Item
    pub menu_item_height_px: f32,
    pub menu_item_padding_x_px: f32,
    pub menu_item_font_size_px: f32,
    pub menu_item_text_color: Hsla,
    pub menu_item_danger_text_color: Hsla,
    pub menu_item_hover_bg: Hsla,
    pub menu_item_selected_bg: Hsla,
    // Separator
    pub menu_separator_height_px: f32,
    pub menu_separator_margin_y_px: f32,
    pub menu_separator_color: Hsla,
    // Shortcut label
    pub menu_shortcut_font_size_px: f32,
    pub menu_shortcut_text_color: Hsla,
    // Format menu icon
    pub menu_format_icon_size_px: f32,
    pub menu_format_icon_gap_px: f32,
    pub menu_format_icon_color: Hsla,
    pub menu_format_active_text_color: Hsla,
    // Widths per menu kind
    pub menu_sidebar_width_px: f32,
    pub menu_format_width_px: f32,
    pub menu_tab_width_px: f32,
}

// ── Workspace Switcher ──────────────────────────────────────────────

pub struct WorkspaceSwitcherStyles {
    pub ws_bar_height_px: f32,
    pub ws_bar_bg: Hsla,
    pub ws_bar_border_color: Hsla,
    pub ws_bar_padding_x_px: f32,
    pub ws_bar_gap_px: f32,
    pub ws_bar_icon_size_px: f32,
    pub ws_bar_icon_color: Hsla,
    pub ws_bar_font_size_px: f32,
    pub ws_bar_text_color: Hsla,
    pub ws_bar_hover_bg: Hsla,
    // Workspace menu items
    pub ws_menu_width_px: f32,
    pub ws_menu_item_check_size_px: f32,
    pub ws_menu_item_check_color: Hsla,
    pub ws_menu_item_close_size_px: f32,
    pub ws_menu_item_close_color: Hsla,
    pub ws_menu_item_close_hover_color: Hsla,
}

// ═════════════════════════════════════════════════════════════════════
// Construction
// ═════════════════════════════════════════════════════════════════════

impl Styles {
    pub fn from_theme(theme: &Theme) -> Self {
        Self {
            window: WindowStyles {
                window_bg: theme.bg_primary,
                window_border_width_px: 1.0,
                window_border_color: theme.border_primary,
                window_radius_px: 16.0,
            },
            editor: EditorStyles {
                editor_font_size_px: 15.0,
                editor_line_height_factor: 1.6,
                editor_padding_left_px: 40.0,
                editor_padding_top_px: 80.0,
                editor_padding_right_px: 40.0,
                editor_padding_bottom_px: 200.0,
                editor_max_width_px: 750.0,
                editor_bg: theme.bg_primary,
                editor_text_color: theme.text_primary,
                editor_scale_step: 0.1,
                editor_scale_min: 0.8,
                editor_scale_max: 1.6,
            },
            sidebar: SidebarStyles {
                sidebar_width_px: 280.0,
                sidebar_bg: theme.bg_secondary,
                sidebar_top_spacer_height_px: 40.0,
                sidebar_bottom_spacer_height_px: 30.0,
                sidebar_logo_size_px: 50.0,
                sidebar_logo_padding_y_px: 12.0,
                sidebar_header_height_px: 26.0,
                sidebar_header_bg: gpui::transparent_black(),
                sidebar_header_padding_left_px: 14.0,
                sidebar_header_padding_right_px: 4.0,
                sidebar_header_gap_px: 6.0,
                sidebar_header_icon_size_px: 14.0,
                sidebar_header_icon_color: theme.text_secondary,
                sidebar_header_font_size_px: 13.0,
                sidebar_header_text_color: theme.text_primary,
                sidebar_header_hover_bg: theme.bg_hover,
                sidebar_item_height_px: 26.0,
                sidebar_item_indent_base_px: 12.0,
                sidebar_item_indent_step_px: 14.0,
                sidebar_item_left_shift_px: 10.0,
                sidebar_item_padding_right_px: 8.0,
                sidebar_item_gap_px: 4.0,
                sidebar_item_hover_bg: theme.bg_hover,
                sidebar_item_selected_bg: theme.bg_selected,
                sidebar_item_fold_container_width_px: 16.0,
                sidebar_item_fold_container_height_px: 18.0,
                sidebar_item_fold_icon_size_px: 10.0,
                sidebar_item_fold_icon_color: theme.text_secondary,
                sidebar_item_icon_size_px: 14.0,
                sidebar_item_icon_color: theme.text_secondary,
                sidebar_item_selected_icon_color: theme.text_bright,
                sidebar_item_font_size_px: 13.0,
                sidebar_item_text_color: theme.text_primary,
                sidebar_item_selected_text_color: theme.text_bright,
                sidebar_item_active_dot_size_px: 5.0,
                sidebar_item_active_dot_color: theme.text_accent,
                sidebar_edit_input_height_px: 20.0,
                sidebar_edit_input_padding_x_px: 6.0,
                sidebar_edit_input_bg: theme.bg_primary,
                sidebar_edit_input_border_width_px: 1.0,
                sidebar_edit_input_border_color: theme.text_accent,
                sidebar_edit_input_radius_px: 3.0,
                sidebar_edit_input_font_size_px: 13.0,
                sidebar_edit_input_text_color: theme.text_bright,
                sidebar_edit_input_placeholder_color: theme.text_secondary,
                sidebar_error_padding_x_px: 14.0,
                sidebar_error_padding_y_px: 12.0,
                sidebar_error_font_size_px: 12.0,
                sidebar_error_text_color: theme.text_danger,
                sidebar_empty_gap_px: 12.0,
                sidebar_empty_padding_x_px: 16.0,
                sidebar_empty_font_size_px: 12.0,
                sidebar_empty_text_color: theme.text_secondary,
                sidebar_empty_btn_padding_x_px: 16.0,
                sidebar_empty_btn_padding_y_px: 5.0,
                sidebar_empty_btn_border_width_px: 1.5,
                sidebar_empty_btn_border_color: theme.border_primary,
                sidebar_empty_btn_font_size_px: 12.0,
                sidebar_empty_btn_text_color: theme.text_secondary,
                sidebar_empty_btn_hover_bg: theme.bg_hover,
                sidebar_resize_handle_width_px: 5.0,
                sidebar_resize_indicator_width_px: 1.0,
                sidebar_resize_active_indicator_width_px: 3.0,
                sidebar_resize_indicator_color: theme.border_primary,
                sidebar_resize_active_indicator_color: theme.text_accent,
                sidebar_resize_min_width_px: 140.0,
                sidebar_resize_max_width_px: 600.0,
                sidebar_indent_guide_color: with_alpha(theme.border_secondary, 0.5),
                sidebar_indent_guide_width_px: 1.0,
                sidebar_indent_guide_dash_length_px: 3.0,
                sidebar_indent_guide_gap_length_px: 3.0,
                sidebar_item_focus_border_color: with_alpha(theme.text_accent, 0.3),
                sidebar_item_active_border_color: with_alpha(theme.text_accent, 0.2),
            },
            outline: OutlineStyles {
                outline_width_px: 220.0,
                outline_bg: theme.bg_secondary,
                outline_border_width_px: 1.0,
                outline_border_color: theme.border_secondary,
                outline_top_spacer_height_px: 40.0,
                outline_bottom_spacer_height_px: 30.0,
                outline_header_height_px: 26.0,
                outline_header_padding_x_px: 14.0,
                outline_header_font_size_px: 11.0,
                outline_header_text_color: theme.text_secondary,
                outline_item_height_px: 26.0,
                outline_item_indent_step_px: 14.0,
                outline_item_padding_left_px: 14.0,
                outline_item_padding_right_px: 8.0,
                outline_item_font_size_px: 13.0,
                outline_item_text_color: theme.text_primary,
                outline_item_active_text_color: theme.text_bright,
                outline_item_active_bg: theme.bg_selected,
                outline_item_hover_bg: theme.bg_hover,
                outline_resize_handle_width_px: 5.0,
                outline_resize_indicator_width_px: 1.0,
                outline_resize_active_indicator_width_px: 3.0,
                outline_resize_indicator_color: theme.border_primary,
                outline_resize_active_indicator_color: theme.text_accent,
                outline_resize_min_width_px: 120.0,
                outline_resize_max_width_px: 400.0,
                outline_indent_guide_color: with_alpha(theme.border_secondary, 0.5),
                outline_indent_guide_width_px: 1.0,
                outline_indent_guide_dash_length_px: 3.0,
                outline_indent_guide_gap_length_px: 3.0,
            },
            tab_bar: TabBarStyles {
                tab_bar_height_px: 38.0,
                tab_bar_bg: theme.bg_bar,
                tab_bar_border_width_px: 1.0,
                tab_bar_border_color: theme.border_secondary,
                tab_item_min_width_px: 60.0,
                tab_item_max_width_px: 180.0,
                tab_item_padding_left_px: 12.0,
                tab_item_padding_right_px: 8.0,
                tab_item_gap_px: 6.0,
                tab_item_bg: theme.bg_tab,
                tab_item_hover_bg: theme.bg_tab_hover,
                tab_item_active_bg: theme.bg_tab_active,
                tab_item_border_width_px: 1.0,
                tab_item_border_color: theme.border_secondary,
                tab_item_icon_size_px: 14.0,
                tab_item_icon_color: theme.text_secondary,
                tab_item_icon_active_opacity: 1.0,
                tab_item_icon_inactive_opacity: 0.4,
                tab_item_font_size_px: 12.0,
                tab_item_text_color: theme.text_primary,
                tab_item_active_text_color: theme.text_bright,
                tab_item_close_size_px: 16.0,
                tab_item_close_font_size_px: 10.0,
                tab_item_close_text_color: theme.text_primary,
                tab_item_close_hover_bg: theme.bg_hover,
                tab_item_close_active_opacity: 1.0,
                tab_item_close_inactive_opacity: 0.0,
                tab_item_dirty_dot_size_px: 7.0,
                tab_item_dirty_dot_color: theme.text_accent,
                tab_item_deleted_text_color: theme.text_secondary,
                tab_new_width_px: 36.0,
                tab_new_font_size_px: 16.0,
                tab_new_text_color: theme.text_secondary,
                tab_new_hover_text_color: theme.text_bright,
                tab_spacer_width_px: 84.0,
            },
            toolbar: ToolbarStyles {
                toolbar_offset_left_px: 15.0,
                toolbar_offset_top_px: 15.0,
                toolbar_gap_px: 12.0,
                toolbar_primary_btn_size_px: 40.0,
                toolbar_primary_icon_size_px: 40.0,
                toolbar_primary_hover_opacity: 0.9,
                toolbar_secondary_btn_size_px: 30.0,
                toolbar_secondary_icon_size_px: 30.0,
                toolbar_secondary_hover_opacity: 0.9,
                toolbar_btn_shadow: true,
                toolbar_btn_bg: with_alpha(theme.bg_elevated, 0.85),
                toolbar_btn_radius_px: 8.0,
            },
            cursor: CursorStyles {
                cursor_color: theme.cursor,
                cursor_width_px: 2.0,
                cursor_blink_tick_ms: 5,
                cursor_blink_pause_ms: 300,
                cursor_blink_visible_ms: 200,
                cursor_blink_transparent_ms: 300,
                cursor_opacity_min: 0.0,
                cursor_opacity_max: 1.0,
                cursor_opacity_step: 0.03,
            },
            code_block: CodeBlockStyles {
                code_block_bg: theme.bg_code,
                code_block_text_padding_left_px: 10.0,
                code_block_fence_clip_px: 15.0,
            },
            selection: SelectionStyles {
                selection_color: theme.selection,
                selection_line_highlight_color: theme.line_highlight,
                selection_trailing_width_factor: 0.4,
            },
            checkbox: CheckboxStyles {
                checkbox_size_factor: 1.2,
                checkbox_text_gap_px: 24.0,
                checkbox_radius_px: 5.0,
                checkbox_border_width_px: 1.5,
                checkbox_border_color: theme.border_tertiary,
                checkbox_checked_bg: theme.checkbox_checked,
                checkbox_check_icon_scale: 0.52,
                checkbox_check_icon_color: gpui::white(),
                checkbox_hit_padding_px: 2.0,
            },
            table: TableStyles {
                table_cell_padding_px: 5.0,
                table_border_width_px: 1.0,
                table_border_color: theme.border_decorative,
                table_header_bg: theme.bg_code,
            },
            gutter: GutterStyles {
                gutter_fold_icon_size_px: 10.0,
                gutter_fold_icon_offset_px: 5.0,
                gutter_fold_hit_padding_px: 3.0,
                gutter_fold_folded_opacity: 0.75,
                gutter_fold_hover_opacity: 0.7,
                gutter_fold_folded_color: theme.text_accent,
                gutter_fold_hover_color: theme.text_secondary,
                gutter_heading_color: theme.gutter_heading,
                gutter_heading_h1_icon_size_px: 28.0,
                gutter_heading_h2_icon_size_px: 22.0,
                gutter_heading_h3_icon_size_px: 18.0,
                gutter_heading_h4_icon_size_px: 14.0,
                gutter_heading_icon_gap_px: 3.0,
                gutter_heading_icon_offset_px: 10.0,
                gutter_blockquote_icon_size_px: 16.0,
                gutter_blockquote_icon_color: theme.border_decorative,
                gutter_blockquote_icon_offset_px: 10.0,
            },
            image_preview: ImagePreviewStyles {
                image_border_radius_px: 0.0,
                image_active_border_width_px: 2.0,
                image_active_border_color: theme.cursor,
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
                menu_bg: theme.bg_elevated,
                menu_border_width_px: 1.0,
                menu_border_color: theme.border_primary,
                menu_radius_px: 10.0,
                menu_shadow: true,
                menu_padding_y_px: 10.0,
                menu_max_height_margin_px: 0.0,
                menu_item_height_px: 28.0,
                menu_item_padding_x_px: 12.0,
                menu_item_font_size_px: 13.0,
                menu_item_text_color: theme.text_primary,
                menu_item_danger_text_color: theme.text_danger,
                menu_item_hover_bg: theme.bg_hover,
                menu_item_selected_bg: theme.bg_hover,
                menu_shortcut_font_size_px: 11.5,
                menu_shortcut_text_color: theme.text_secondary,
                menu_format_icon_size_px: 14.0,
                menu_format_icon_gap_px: 8.0,
                menu_format_icon_color: theme.text_secondary,
                menu_format_active_text_color: theme.text_bright,
                menu_separator_height_px: 1.0,
                menu_separator_margin_y_px: 0.0,
                menu_separator_color: theme.border_primary,
                menu_sidebar_width_px: 180.0,
                menu_format_width_px: 210.0,
                menu_tab_width_px: 180.0,
            },
            dialog: DialogStyles {
                dialog_backdrop_color: gpui::hsla(0.0, 0.0, 0.0, 0.3),
                dialog_bg: theme.bg_elevated,
                dialog_border_width_px: 1.0,
                dialog_border_color: theme.border_primary,
                dialog_radius_px: 12.0,
                dialog_width_px: 320.0,
                dialog_padding_x_px: 20.0,
                dialog_padding_y_px: 18.0,
                dialog_gap_px: 12.0,
                dialog_title_font_size_px: 14.0,
                dialog_title_text_color: theme.text_bright,
                dialog_message_font_size_px: 13.0,
                dialog_message_text_color: theme.text_secondary,
                dialog_btn_height_px: 28.0,
                dialog_btn_padding_x_px: 14.0,
                dialog_btn_font_size_px: 12.0,
                dialog_btn_radius_px: 6.0,
                dialog_btn_gap_px: 8.0,
                dialog_btn_hover_opacity: 0.9,
                dialog_btn_primary_bg: theme.text_accent,
                dialog_btn_primary_text_color: gpui::white(),
                dialog_btn_primary_hover_bg: theme.text_accent,
                dialog_btn_secondary_bg: theme.bg_hover,
                dialog_btn_secondary_text_color: theme.text_primary,
                dialog_btn_secondary_hover_bg: theme.bg_selected,
                dialog_btn_danger_bg: theme.text_danger,
                dialog_btn_danger_text_color: gpui::white(),
                dialog_btn_danger_hover_bg: theme.text_danger,
            },
            workspace_switcher: WorkspaceSwitcherStyles {
                ws_bar_height_px: 32.0,
                ws_bar_bg: gpui::transparent_black(),
                ws_bar_border_color: theme.border_secondary,
                ws_bar_padding_x_px: 14.0,
                ws_bar_gap_px: 6.0,
                ws_bar_icon_size_px: 14.0,
                ws_bar_icon_color: theme.text_secondary,
                ws_bar_font_size_px: 13.0,
                ws_bar_text_color: theme.text_secondary,
                ws_bar_hover_bg: theme.bg_hover,
                ws_menu_width_px: 220.0,
                ws_menu_item_check_size_px: 12.0,
                ws_menu_item_check_color: theme.text_accent,
                ws_menu_item_close_size_px: 14.0,
                ws_menu_item_close_color: theme.text_secondary,
                ws_menu_item_close_hover_color: theme.text_primary,
            },
        }
    }
}
