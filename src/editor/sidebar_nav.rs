use gpui::Context;

use super::*;

impl EditorView {
    pub(crate) fn sidebar_nav_up(&mut self, cx: &mut Context<Self>) {
        self.navigate_workspace_tree(-1, cx);
    }

    pub(crate) fn sidebar_nav_down(&mut self, cx: &mut Context<Self>) {
        self.navigate_workspace_tree(1, cx);
    }

    pub(crate) fn sidebar_nav_left(&mut self, cx: &mut Context<Self>) {
        if self.focused_starred_path.as_ref() == self.focused_sidebar_path.as_ref()
            && self.focused_starred_path.is_some()
        {
            return;
        }
        if self.focused_workspace_root.is_some() {
            return;
        }
        let path = match self.focused_sidebar_path.clone() {
            Some(path) => path,
            None => return,
        };
        let is_expanded = self.expanded_dirs.contains(&path);
        if self.sidebar_cached_path_is_dir(&path) == Some(true) && is_expanded {
            self.toggle_sidebar_directory(path, cx);
        } else if let Some(parent) = path.parent()
            && parent.starts_with(&self.sidebar_root)
        {
            if parent == self.sidebar_root {
                self.focus_workspace_row(parent.to_path_buf(), cx);
                return;
            }
            self.focused_sidebar_path = Some(parent.to_path_buf());
            let items = self.sidebar_items();
            if let Some(index) = items.iter().position(|item| item.path == parent)
                && self.ensure_sidebar_item_minimally_visible(index, items.len())
            {
                cx.notify();
            }
            cx.notify();
        }
    }

    pub(crate) fn sidebar_nav_right(&mut self, cx: &mut Context<Self>) {
        if self.focused_starred_path.as_ref() == self.focused_sidebar_path.as_ref()
            && self.focused_starred_path.is_some()
        {
            self.sidebar_confirm_focused(cx);
            return;
        }
        if let Some(root) = self.focused_workspace_root.clone() {
            if root == self.sidebar_root {
                let items = self.sidebar_items();
                if let Some(first) = items.first() {
                    self.focused_sidebar_path = Some(first.path.clone());
                    self.focused_workspace_root = None;
                    self.ensure_sidebar_item_minimally_visible(0, items.len());
                    cx.notify();
                }
            } else {
                self.switch_workspace(root.clone(), cx);
                self.focus_workspace_row(root, cx);
            }
            return;
        }
        let path = match self.focused_sidebar_path.clone() {
            Some(path) => path,
            None => return,
        };
        if self.sidebar_cached_path_is_dir(&path) == Some(true) {
            let is_expanded = self.expanded_dirs.contains(&path);
            if !is_expanded {
                self.toggle_sidebar_directory(path, cx);
            } else {
                let items = self.sidebar_items();
                if let Some(child) = items.iter().find(|item| item.path.parent() == Some(&path)) {
                    self.focused_sidebar_path = Some(child.path.clone());
                    if let Some(index) = items.iter().position(|item| item.path == child.path)
                        && self.ensure_sidebar_item_minimally_visible(index, items.len())
                    {
                        cx.notify();
                    }
                    cx.notify();
                }
            }
        }
    }

    pub(crate) fn sidebar_confirm_focused(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.focused_starred_path.clone()
            && self.focused_sidebar_path.as_ref() == Some(&path)
        {
            self.activate_starred_workspace(&path, cx);
            self.open_sidebar_file(path, cx);
            return;
        }
        if let Some(root) = self.focused_workspace_root.clone() {
            self.switch_workspace(root.clone(), cx);
            self.focus_workspace_row(root, cx);
            return;
        }
        let path = match self.focused_sidebar_path.clone() {
            Some(path) => path,
            None => return,
        };
        if self.sidebar_edit.is_some() {
            self.commit_sidebar_edit(cx);
        } else {
            self.sidebar_select_item(&path, false, false);
            if self.sidebar_cached_path_is_dir(&path) == Some(true) {
                self.toggle_sidebar_directory(path, cx);
            } else {
                self.open_sidebar_file(path, cx);
            }
        }
    }

    pub(crate) fn sidebar_delete_focused(&mut self, cx: &mut Context<Self>) {
        if self.focused_workspace_root.is_some() {
            return;
        }
        let path = match self.focused_sidebar_path.clone() {
            Some(path) => path,
            None => return,
        };
        self.show_sidebar_delete_dialog(vec![path], cx);
    }

    pub(crate) fn show_sidebar_delete_dialog(
        &mut self,
        paths: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }

        let dialog = if paths.len() == 1 {
            let path = paths[0].clone();
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            Dialog {
                title: self.t(crate::i18n::TextKey::Delete).to_string(),
                message: crate::i18n::delete_path_message(self.ui_language, &name),
                save_label: None,
                confirm_label: self.t(crate::i18n::TextKey::Delete).to_string(),
                cancel_label: self.t(crate::i18n::TextKey::Cancel).to_string(),
                is_danger: true,
                action: DialogAction::DeletePath { path },
            }
        } else {
            Dialog {
                title: self.t(crate::i18n::TextKey::Delete).to_string(),
                message: crate::i18n::delete_items_message(self.ui_language, paths.len()),
                save_label: None,
                confirm_label: crate::i18n::delete_items_label(self.ui_language, paths.len()),
                cancel_label: self.t(crate::i18n::TextKey::Cancel).to_string(),
                is_danger: true,
                action: DialogAction::DeletePaths { paths },
            }
        };

        self.show_dialog(dialog, cx);
    }
}
