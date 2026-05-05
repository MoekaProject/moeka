use gpui::Context;

use super::*;

impl EditorView {
    pub(super) fn maybe_extend_sidebar_items(&mut self, total_items: usize, cx: &mut Context<Self>) {
        let _ = (total_items, cx);
    }

    pub(super) fn sidebar_edit_text_input_styles(&self) -> crate::style::TextInputStyles {
        let mut styles = self.styles.text_input.clone();
        let sidebar_styles = &self.styles.sidebar;
        styles.text_input_height_px = sidebar_styles.sidebar_edit_input_height_px;
        styles.text_input_padding_left_px = sidebar_styles.sidebar_edit_input_padding_x_px;
        styles.text_input_padding_right_px = sidebar_styles.sidebar_edit_input_padding_x_px;
        styles.text_input_font_size_px = sidebar_styles.sidebar_edit_input_font_size_px;
        styles.text_input_bg = sidebar_styles.sidebar_edit_input_bg;
        styles.text_input_focus_bg = sidebar_styles.sidebar_edit_input_bg;
        styles.text_input_border_width_px = sidebar_styles.sidebar_edit_input_border_width_px;
        styles.text_input_border_color = sidebar_styles.sidebar_edit_input_border_color;
        styles.text_input_focus_border_color = sidebar_styles.sidebar_edit_input_border_color;
        styles.text_input_radius_px = sidebar_styles.sidebar_edit_input_radius_px;
        styles.text_input_text_color = sidebar_styles.sidebar_edit_input_text_color;
        styles.text_input_placeholder_color = sidebar_styles.sidebar_edit_input_placeholder_color;
        styles.text_input_cursor_height_ratio =
            sidebar_styles.sidebar_edit_input_cursor_height_ratio;
        styles
    }

    fn open_sidebar_root_menu(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        kind: ContextMenuKind,
        clear_sidebar_edit: bool,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenu { position, kind });
        if clear_sidebar_edit {
            self.sidebar_edit = None;
        }
        cx.notify();
    }

    pub(crate) fn open_workspace_menu(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_sidebar_root_menu(position, ContextMenuKind::Workspace, false, cx);
    }

    pub(crate) fn open_sidebar_sort_menu(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_sidebar_root_menu(position, ContextMenuKind::SidebarSort, true, cx);
    }

    pub(crate) fn open_sidebar_create_menu(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_sidebar_root_menu(position, ContextMenuKind::SidebarCreate, true, cx);
    }
}
