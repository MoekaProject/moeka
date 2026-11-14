use gpui::Context;

use super::*;

impl EditorView {
    pub(super) fn sidebar_edit_text_input_styles(&self) -> crate::style::TextInputStyles {
        self.styles
            .input_styles(crate::style::InputRole::Rename, self.ui_scale)
    }

    fn open_sidebar_root_menu(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        kind: ContextMenuKind,
        clear_sidebar_edit: bool,
        cx: &mut Context<Self>,
    ) {
        let same_menu_open = self
            .context_menu
            .as_ref()
            .is_some_and(|menu| menu.kind == kind);
        if same_menu_open {
            self.close_context_menu(cx);
            return;
        }

        self.open_context_menu(ContextMenu { position, kind }, clear_sidebar_edit, cx);
        if clear_sidebar_edit {
            self.sidebar_edit = None;
        }
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
