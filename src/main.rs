mod actions;
mod buffer;
mod cursor;
mod editor;
mod markdown;
mod theme;

use std::borrow::Cow;

use gpui::*;

use actions::*;
use editor::EditorView;

struct FsAssets;

impl AssetSource for FsAssets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(Some(std::fs::read(path)?.into()))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(std::fs::read_dir(path)?
            .filter_map(|entry| {
                entry
                    .ok()
                    .map(|entry| SharedString::from(entry.path().to_string_lossy().into_owned()))
            })
            .collect())
    }
}

fn main() {
    Application::new().with_assets(FsAssets).run(|cx| {
        cx.bind_keys([
            KeyBinding::new("cmd-z", Undo, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-z", Redo, Some("MarkdownEditor")),
            KeyBinding::new("backspace", Backspace, Some("MarkdownEditor")),
            KeyBinding::new("delete", Delete, Some("MarkdownEditor")),
            KeyBinding::new("left", Left, Some("MarkdownEditor")),
            KeyBinding::new("right", Right, Some("MarkdownEditor")),
            KeyBinding::new("up", Up, Some("MarkdownEditor")),
            KeyBinding::new("down", Down, Some("MarkdownEditor")),
            KeyBinding::new("shift-left", SelectLeft, Some("MarkdownEditor")),
            KeyBinding::new("shift-right", SelectRight, Some("MarkdownEditor")),
            KeyBinding::new("shift-up", SelectUp, Some("MarkdownEditor")),
            KeyBinding::new("shift-down", SelectDown, Some("MarkdownEditor")),
            KeyBinding::new("cmd-a", SelectAll, Some("MarkdownEditor")),
            KeyBinding::new("cmd-left", Home, Some("MarkdownEditor")),
            KeyBinding::new("cmd-right", End, Some("MarkdownEditor")),
            KeyBinding::new("shift-cmd-left", SelectHome, Some("MarkdownEditor")),
            KeyBinding::new("shift-cmd-right", SelectEnd, Some("MarkdownEditor")),
            KeyBinding::new("enter", Enter, Some("MarkdownEditor")),
            KeyBinding::new("tab", Tab, Some("MarkdownEditor")),
            KeyBinding::new("cmd-c", Copy, Some("MarkdownEditor")),
            KeyBinding::new("cmd-x", Cut, Some("MarkdownEditor")),
            KeyBinding::new("cmd-v", Paste, Some("MarkdownEditor")),
            KeyBinding::new(
                "cmd-ctrl-space",
                ShowCharacterPalette,
                Some("MarkdownEditor"),
            ),
            KeyBinding::new("alt-left", MoveWordLeft, Some("MarkdownEditor")),
            KeyBinding::new("alt-right", MoveWordRight, Some("MarkdownEditor")),
            KeyBinding::new("alt-backspace", DeleteWordLeft, Some("MarkdownEditor")),
            KeyBinding::new("cmd-b", ToggleBold, Some("MarkdownEditor")),
            KeyBinding::new("cmd-i", ToggleItalic, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-s", ToggleStrikethrough, Some("MarkdownEditor")),
            KeyBinding::new("cmd-e", ToggleInlineCode, Some("MarkdownEditor")),
            KeyBinding::new("cmd-k", ToggleLink, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-i", ToggleImage, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-q", ToggleQuote, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-t", ToggleTodo, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-u", ToggleUnorderedList, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-o", ToggleOrderedList, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-c", InsertCodeBlock, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-c", InsertCodeBlock, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-h", InsertHorizontalRule, Some("MarkdownEditor")),
            KeyBinding::new("cmd-1", InsertHeading1, Some("MarkdownEditor")),
            KeyBinding::new("cmd-2", InsertHeading2, Some("MarkdownEditor")),
            KeyBinding::new("cmd-3", InsertHeading3, Some("MarkdownEditor")),
            KeyBinding::new("cmd-4", InsertHeading4, Some("MarkdownEditor")),
            KeyBinding::new("cmd-5", InsertHeading5, Some("MarkdownEditor")),
            KeyBinding::new("cmd-6", InsertHeading6, Some("MarkdownEditor")),
            KeyBinding::new("cmd-=", ZoomIn, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-=", ZoomIn, Some("MarkdownEditor")),
            KeyBinding::new("cmd-minus", ZoomOut, Some("MarkdownEditor")),
            KeyBinding::new("cmd-0", ZoomReset, Some("MarkdownEditor")),
            KeyBinding::new("cmd-q", Quit, Some("MarkdownEditor")),
            KeyBinding::new("escape", Escape, Some("MarkdownEditor")),
        ]);

        cx.on_action(|_: &Quit, cx| cx.quit());

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1360.0), px(820.0)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Markdown Editor".into()),
                    appears_transparent: false,
                    traffic_light_position: None,
                }),
                window_background: WindowBackgroundAppearance::Opaque,
                ..Default::default()
            },
            |window, cx| {
                window.set_background_appearance(WindowBackgroundAppearance::Opaque);
                cx.new(|cx| {
                    let view = EditorView::new(cx);
                    window.focus(&view.focus_handle(cx));
                    view
                })
            },
        )
        .unwrap();
    });
}
