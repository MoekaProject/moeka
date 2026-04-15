mod actions;
mod buffer;
mod cursor;
mod editor;
mod markdown;
mod style;
mod theme;

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use gpui::*;

use actions::*;
use editor::EditorView;

struct FsAssets;

impl AssetSource for FsAssets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let Some(resolved_path) = resolve_runtime_path(path) else {
            return Ok(None);
        };

        Ok(Some(std::fs::read(resolved_path)?.into()))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        let Some(resolved_path) = resolve_runtime_path(path) else {
            return Ok(Vec::new());
        };

        Ok(std::fs::read_dir(resolved_path)?
            .filter_map(|entry| {
                entry
                    .ok()
                    .map(|entry| SharedString::from(entry.path().to_string_lossy().into_owned()))
            })
            .collect())
    }
}

pub(crate) fn resolve_runtime_path(path: impl AsRef<Path>) -> Option<PathBuf> {
    let path = path.as_ref();
    if path.is_absolute() {
        return path.exists().then(|| path.to_path_buf());
    }

    runtime_search_roots()
        .into_iter()
        .map(|root| root.join(path))
        .find(|candidate| candidate.exists())
}

fn runtime_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(executable_path) = std::env::current_exe() {
        if let Some(executable_dir) = executable_path.parent() {
            roots.push(executable_dir.to_path_buf());

            if let Some(contents_dir) = executable_dir.parent() {
                roots.push(contents_dir.join("Resources"));
            }
        }
    }

    if let Ok(current_dir) = std::env::current_dir() {
        roots.push(current_dir.clone());
        roots.push(current_dir.join("editor"));
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    roots.push(manifest_dir.clone());
    if let Some(parent) = manifest_dir.parent() {
        roots.push(parent.to_path_buf());
    }

    roots
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
            KeyBinding::new("cmd--", ZoomOut, Some("MarkdownEditor")),
            KeyBinding::new("cmd-0", ZoomReset, Some("MarkdownEditor")),
            KeyBinding::new("cmd-\\", ToggleSidebar, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-l", ToggleEditorLeftAlign, Some("MarkdownEditor")),
            KeyBinding::new("cmd-l", SelectLine, Some("MarkdownEditor")),
            KeyBinding::new("cmd-enter", InsertLineBelow, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-enter", InsertLineAbove, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-d", DuplicateLine, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-k", DeleteLine, Some("MarkdownEditor")),
            KeyBinding::new("alt-up", MoveLineUp, Some("MarkdownEditor")),
            KeyBinding::new("alt-down", MoveLineDown, Some("MarkdownEditor")),
            KeyBinding::new("cmd-s", Save, Some("MarkdownEditor")),
            KeyBinding::new("cmd-n", NewTab, Some("MarkdownEditor")),
            KeyBinding::new("cmd-q", Quit, Some("MarkdownEditor")),
            KeyBinding::new("escape", Escape, Some("MarkdownEditor")),
            KeyBinding::new("alt-cmd-up", AddCursorAbove, Some("MarkdownEditor")),
            KeyBinding::new("alt-cmd-down", AddCursorBelow, Some("MarkdownEditor")),
            KeyBinding::new("cmd-d", SelectNextOccurrence, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-l", SelectAllOccurrences, Some("MarkdownEditor")),
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
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(15.0), px(13.0))),
                }),
                window_background: WindowBackgroundAppearance::Transparent,
                ..Default::default()
            },
            |window, cx| {
                window.set_background_appearance(WindowBackgroundAppearance::Transparent);
                #[cfg(target_os = "macos")]
                {
                    use objc::{class, msg_send, sel, sel_impl};
                    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                    if let Ok(handle) = window.window_handle() {
                        if let RawWindowHandle::AppKit(appkit_handle) = handle.as_raw() {
                            unsafe {
                                let ns_view =
                                    appkit_handle.ns_view.as_ptr() as *mut objc::runtime::Object;
                                let ns_window: *mut objc::runtime::Object =
                                    msg_send![ns_view, window];
                                // NSTitlebarSeparatorStyleNone = 0
                                let () = msg_send![ns_window, setTitlebarSeparatorStyle: 0i64];
                                // Use fully clear background (like Clipboarder's
                                // transparent(true)) and re-enable shadow manually.
                                let clear_color: *mut objc::runtime::Object =
                                    msg_send![class!(NSColor), clearColor];
                                let () = msg_send![ns_window, setBackgroundColor: clear_color];
                                let () = msg_send![ns_window, setHasShadow: true];
                                // Prevent titlebar from starting window drag on mouse down
                                let () = msg_send![ns_window, setMovableByWindowBackground: false];
                                let content_view: *mut objc::runtime::Object =
                                    msg_send![ns_window, contentView];
                                let () = msg_send![content_view, setWantsLayer: true];
                                let layer: *mut objc::runtime::Object =
                                    msg_send![content_view, layer];
                                let () = msg_send![layer, setCornerRadius: 15.0_f64];
                                let () = msg_send![layer, setMasksToBounds: true];
                            }
                        }
                    }
                }
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
