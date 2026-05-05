use gpui::Context;

use super::*;

impl EditorView {
    pub(crate) fn sidebar_nav_up(&mut self, cx: &mut Context<Self>) {
        let items = self.sidebar_items();
        if items.is_empty() {
            return;
        }
        let new_path = if let Some(ref path) = self.focused_sidebar_path {
            if let Some(idx) = items.iter().position(|item| &item.path == path) {
                if idx > 0 {
                    Some(items[idx - 1].path.clone())
                } else {
                    Some(items[0].path.clone())
                }
            } else {
                Some(items[0].path.clone())
            }
        } else {
            items.last().map(|item| item.path.clone())
        };
        self.focused_sidebar_path = new_path;
        cx.notify();
    }

    pub(crate) fn sidebar_nav_down(&mut self, cx: &mut Context<Self>) {
        let items = self.sidebar_items();
        if items.is_empty() {
            return;
        }
        let new_path = if let Some(ref path) = self.focused_sidebar_path {
            if let Some(idx) = items.iter().position(|item| &item.path == path) {
                Some(items[(idx + 1).min(items.len() - 1)].path.clone())
            } else {
                Some(items[0].path.clone())
            }
        } else {
            Some(items[0].path.clone())
        };
        self.focused_sidebar_path = new_path;
        cx.notify();
    }

    pub(crate) fn sidebar_nav_left(&mut self, cx: &mut Context<Self>) {
        let path = match self.focused_sidebar_path.clone() {
            Some(path) => path,
            None => return,
        };
        if path.is_dir() && self.expanded_dirs.contains(&path) {
            self.expanded_dirs.remove(&path);
            self.invalidate_sidebar_items_cache();
            cx.notify();
        } else if let Some(parent) = path.parent()
            && parent != self.sidebar_root
            && parent.starts_with(&self.sidebar_root)
        {
            self.focused_sidebar_path = Some(parent.to_path_buf());
            cx.notify();
        }
    }

    pub(crate) fn sidebar_nav_right(&mut self, cx: &mut Context<Self>) {
        let path = match self.focused_sidebar_path.clone() {
            Some(path) => path,
            None => return,
        };
        if path.is_dir() {
            if !self.expanded_dirs.contains(&path) {
                self.expanded_dirs.insert(path.clone());
                self.invalidate_sidebar_items_cache();
                cx.notify();
            } else {
                let items = self.sidebar_items();
                if let Some(child) = items.iter().find(|item| item.path.parent() == Some(&path)) {
                    self.focused_sidebar_path = Some(child.path.clone());
                    cx.notify();
                }
            }
        }
    }

    pub(crate) fn sidebar_confirm_focused(&mut self, cx: &mut Context<Self>) {
        let path = match self.focused_sidebar_path.clone() {
            Some(path) => path,
            None => return,
        };
        if self.sidebar_edit.is_some() {
            self.commit_sidebar_edit(cx);
        } else {
            self.sidebar_select_item(&path, false, false);
            if path.is_dir() {
                self.toggle_sidebar_directory(path, cx);
            } else {
                self.open_sidebar_file(path, cx);
            }
        }
    }

    pub(crate) fn sidebar_delete_focused(&mut self, cx: &mut Context<Self>) {
        let path = match self.focused_sidebar_path.clone() {
            Some(path) => path,
            None => return,
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.show_dialog(
            Dialog {
                title: "删除".to_string(),
                message: format!("确定要删除 \"{}\" 吗？此操作不可撤销。", name),
                save_label: None,
                confirm_label: "删除".to_string(),
                cancel_label: "取消".to_string(),
                is_danger: true,
                action: DialogAction::DeletePath { path },
            },
            cx,
        );
    }
}
