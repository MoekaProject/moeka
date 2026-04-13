#![allow(dead_code)]

use std::time::Duration;

use gpui::Hsla;

use crate::theme::Theme;

/// Per-component style configuration, derived from the active theme.
pub struct Styles {
    pub editor: EditorStyles,
    pub sidebar: SidebarStyles,
    pub tab_bar: TabBarStyles,
    pub cursor: CursorStyles,
    pub code_block: CodeBlockStyles,
    pub checkbox: CheckboxStyles,
    pub table: TableStyles,
    pub gutter: GutterStyles,
    pub context_menu: ContextMenuStyles,
    pub toolbar: ToolbarStyles,
}

pub struct EditorStyles {
    pub base_font_size: f32,
    pub line_height_factor: f32,
    pub padding_left: f32,
    pub padding_top: f32,
    pub padding_right: f32,
    pub padding_bottom: f32,
    pub max_width: f32,
    pub ui_scale_step: f32,
    pub min_ui_scale: f32,
    pub max_ui_scale: f32,
}

pub struct SidebarStyles {
    pub width: f32,
    pub row_height: f32,
    pub bg: Hsla,
    pub header_bg: Hsla,
    pub hover: Hsla,
    pub selected: Hsla,
    pub border: Hsla,
    pub muted_text: Hsla,
}

pub struct TabBarStyles {
    pub height: f32,
    pub bg: Hsla,
    pub tab_bg: Hsla,
    pub tab_hover: Hsla,
    pub tab_active_bg: Hsla,
    pub tab_active_text: Hsla,
    pub tab_border: Hsla,
    pub close_button_size: f32,
}

pub struct CursorStyles {
    pub color: Hsla,
    pub blink_tick: Duration,
    pub blink_pause_delay: Duration,
    pub blink_visible_duration: Duration,
    pub blink_transparent_duration: Duration,
    pub min_opacity: f32,
    pub max_opacity: f32,
    pub opacity_step: f32,
}

pub struct CodeBlockStyles {
    pub bg: Hsla,
    pub text_left_padding: f32,
}

pub struct CheckboxStyles {
    pub size_factor: f32,
    pub text_gap: f32,
    pub corner_radius: f32,
    pub border_color: Hsla,
    pub checked_color: Hsla,
    pub check_icon_scale: f32,
}

pub struct TableStyles {
    pub cell_text_padding: f32,
    pub border_color: Hsla,
}

pub struct GutterStyles {
    pub fold_icon_size: f32,
    pub heading_color: Hsla,
}

pub struct ContextMenuStyles {
    pub format_menu_width: f32,
}

pub struct ToolbarStyles {
    pub button_size: f32,
    pub button_gap: f32,
    pub sidebar_toggle_bg: Hsla,
    pub align_toggle_bg: Hsla,
}

impl Styles {
    pub fn from_theme(theme: &Theme) -> Self {
        Self {
            editor: EditorStyles {
                base_font_size: 15.0,
                line_height_factor: 1.6,
                padding_left: 40.0,
                padding_top: 50.0,
                padding_right: 40.0,
                padding_bottom: 200.0,
                max_width: 750.0,
                ui_scale_step: 0.1,
                min_ui_scale: 0.8,
                max_ui_scale: 1.6,
            },
            sidebar: SidebarStyles {
                width: 280.0,
                row_height: 28.0,
                bg: theme.panel_bg,
                header_bg: theme.panel_header_bg,
                hover: theme.panel_hover,
                selected: theme.panel_selected,
                border: theme.panel_border,
                muted_text: theme.panel_muted,
            },
            tab_bar: TabBarStyles {
                height: 38.0,
                bg: theme.tab_bar_bg,
                tab_bg: theme.tab_bg,
                tab_hover: theme.tab_hover,
                tab_active_bg: theme.tab_active_bg,
                tab_active_text: theme.tab_active_text,
                tab_border: theme.tab_border,
                close_button_size: 18.0,
            },
            cursor: CursorStyles {
                color: theme.cursor,
                blink_tick: Duration::from_millis(5),
                blink_pause_delay: Duration::from_millis(300),
                blink_visible_duration: Duration::from_millis(200),
                blink_transparent_duration: Duration::from_millis(300),
                min_opacity: 0.0,
                max_opacity: 1.0,
                opacity_step: 0.03,
            },
            code_block: CodeBlockStyles {
                bg: theme.code_block_bg,
                text_left_padding: 10.0,
            },
            checkbox: CheckboxStyles {
                size_factor: 1.2,
                text_gap: 24.0,
                corner_radius: 5.0,
                border_color: theme.checkbox_border,
                checked_color: theme.checkbox_checked,
                check_icon_scale: 0.52,
            },
            table: TableStyles {
                cell_text_padding: 5.0,
                border_color: theme.blockquote_border,
            },
            gutter: GutterStyles {
                fold_icon_size: 10.0,
                heading_color: theme.gutter_heading,
            },
            context_menu: ContextMenuStyles {
                format_menu_width: 250.0,
            },
            toolbar: ToolbarStyles {
                button_size: 34.0,
                button_gap: 8.0,
                sidebar_toggle_bg: crate::hex_hsla!("#910913"),
                align_toggle_bg: gpui::hsla(0.61, 0.12, 0.25, 0.85),
            },
        }
    }
}
