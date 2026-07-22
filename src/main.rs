#![allow(unexpected_cfgs)]

mod actions;
mod buffer;
mod checkbox;
mod cursor;
mod editor;
mod i18n;
mod markdown;
mod style;
mod table_ops;
mod text_input;
mod theme;

use crate::i18n::system_application_display_name;
use std::borrow::Cow;
#[cfg(target_os = "macos")]
use std::cell::RefCell;
#[cfg(target_os = "macos")]
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::rc::Rc;
#[cfg(target_os = "macos")]
use std::sync::Once;
use std::sync::OnceLock;

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
    static RUNTIME_SEARCH_ROOTS: OnceLock<Vec<PathBuf>> = OnceLock::new();

    RUNTIME_SEARCH_ROOTS
        .get_or_init(build_runtime_search_roots)
        .clone()
}

fn build_runtime_search_roots() -> Vec<PathBuf> {
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

#[cfg(target_os = "macos")]
fn macos_markdown_path_from_url(url: &str) -> Option<PathBuf> {
    let encoded_path = url.strip_prefix("file://")?;
    let encoded_path = encoded_path
        .strip_prefix("localhost")
        .unwrap_or(encoded_path);
    if !encoded_path.starts_with('/') {
        return None;
    }
    let decoded = percent_encoding::percent_decode_str(encoded_path)
        .decode_utf8()
        .ok()?;
    let path = PathBuf::from(decoded.as_ref());
    if !editor::is_markdown_file(&path) || !path.is_file() {
        return None;
    }
    std::fs::canonicalize(&path).ok().or(Some(path))
}

#[cfg(target_os = "macos")]
fn open_markdown_paths(editor: &Entity<EditorView>, paths: Vec<PathBuf>, cx: &mut App) {
    let path_count = paths.len();
    editor.update(cx, |editor, cx| {
        for (index, path) in paths.into_iter().enumerate() {
            editor.open_path_in_permanent_tab(path, index + 1 == path_count, cx);
        }
    });
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn class_replaceMethod(
        cls: *mut objc::runtime::Class,
        name: objc::runtime::Sel,
        imp: objc::runtime::Imp,
        types: *const c_char,
    ) -> objc::runtime::Imp;
}

#[cfg(target_os = "macos")]
extern "C" fn gpui_view_mouse_down_can_move_window(
    _: &objc::runtime::Object,
    _: objc::runtime::Sel,
) -> objc::runtime::BOOL {
    objc::runtime::NO
}

#[cfg(target_os = "macos")]
fn disable_gpui_content_view_window_drag(ns_view: *mut objc::runtime::Object) {
    use objc::runtime::{Class, Imp, Object, Sel};
    use objc::{msg_send, sel, sel_impl};

    fn replace_mouse_down_can_move_window(class: &Class) {
        let selector = Sel::register("mouseDownCanMoveWindow");
        let implementation = gpui_view_mouse_down_can_move_window
            as extern "C" fn(&Object, Sel) -> objc::runtime::BOOL;
        let encoding = if cfg!(target_arch = "aarch64") {
            b"B@:\0".as_ptr()
        } else {
            b"c@:\0".as_ptr()
        };

        unsafe {
            let _ = class_replaceMethod(
                class as *const Class as *mut Class,
                selector,
                std::mem::transmute::<extern "C" fn(&Object, Sel) -> objc::runtime::BOOL, Imp>(
                    implementation,
                ),
                encoding.cast(),
            );
        }
    }

    static INSTALL: Once = Once::new();

    INSTALL.call_once(|| {
        let Some(class) = Class::get("GPUIView") else {
            return;
        };
        replace_mouse_down_can_move_window(class);
    });

    unsafe {
        let mut current_view = ns_view;
        while !current_view.is_null() {
            replace_mouse_down_can_move_window((*current_view).class());
            current_view = msg_send![current_view, superview];
        }
    }
}

fn editor_window_options(cx: &App) -> WindowOptions {
    #[cfg(target_os = "macos")]
    let traffic_light_position =
        EditorView::traffic_light_position_for_ui_scale(editor::AppSettings::load().ui_scale);
    #[cfg(not(target_os = "macos"))]
    let traffic_light_position = point(px(10.0), px(10.0));

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1024.0), px(768.0)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(system_application_display_name().into()),
            appears_transparent: true,
            traffic_light_position: Some(traffic_light_position),
        }),
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

fn configure_editor_window(window: &mut Window) {
    window.set_background_appearance(WindowBackgroundAppearance::Transparent);
    #[cfg(target_os = "macos")]
    {
        use objc::{class, msg_send, sel, sel_impl};
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        if let Ok(handle) = window.window_handle()
            && let RawWindowHandle::AppKit(appkit_handle) = handle.as_raw()
        {
            unsafe {
                let ns_view = appkit_handle.ns_view.as_ptr() as *mut objc::runtime::Object;
                disable_gpui_content_view_window_drag(ns_view);
                let ns_window: *mut objc::runtime::Object = msg_send![ns_view, window];
                let () = msg_send![ns_window, setTitlebarSeparatorStyle: 0i64];
                let clear_color: *mut objc::runtime::Object =
                    msg_send![class!(NSColor), clearColor];
                let () = msg_send![ns_window, setBackgroundColor: clear_color];
                let () = msg_send![ns_window, setHasShadow: true];
                let () = msg_send![ns_window, setMovableByWindowBackground: false];
                let content_view: *mut objc::runtime::Object = msg_send![ns_window, contentView];
                let () = msg_send![content_view, setWantsLayer: true];
                let layer: *mut objc::runtime::Object = msg_send![content_view, layer];
                let () = msg_send![layer, setCornerRadius: 15.0_f64];
                let () = msg_send![layer, setMasksToBounds: true];
            }
        }
    }
}

fn open_terminal_window(cx: &mut App) -> anyhow::Result<()> {
    let options = editor_window_options(cx);
    cx.open_window(options, |window, cx| {
        configure_editor_window(window);
        let view = cx.new(|cx| {
            let mut view = EditorView::new(cx);
            let launch_cli = view.app_settings.terminal_launch_cli();
            view.create_terminal_tab(launch_cli, cx);
            view.sidebar_visible = false;
            view.sidebar_resizing = false;
            view.set_sidebar_width_immediately(0.0);
            view
        });
        window.focus(&view.focus_handle(cx));
        view
    })?;
    cx.activate(true);
    Ok(())
}

fn main() {
    #[cfg(target_os = "macos")]
    let pending_open_paths = Rc::new(RefCell::new(Vec::<PathBuf>::new()));
    #[cfg(target_os = "macos")]
    let editor_for_open_urls = Rc::new(RefCell::new(None::<Entity<EditorView>>));
    #[cfg(target_os = "macos")]
    let async_app_for_open_urls = Rc::new(RefCell::new(None::<AsyncApp>));

    let application = Application::new().with_assets(FsAssets);
    #[cfg(target_os = "macos")]
    {
        let pending_open_paths = pending_open_paths.clone();
        let editor_for_open_urls = editor_for_open_urls.clone();
        let async_app_for_open_urls = async_app_for_open_urls.clone();
        application.on_open_urls(move |urls| {
            let paths = urls
                .into_iter()
                .filter_map(|url| macos_markdown_path_from_url(&url))
                .collect::<Vec<_>>();
            if paths.is_empty() {
                return;
            }

            let editor = editor_for_open_urls.borrow().clone();
            let async_app = async_app_for_open_urls.borrow().clone();
            if let (Some(editor), Some(async_app)) = (editor, async_app) {
                async_app
                    .spawn(async move |cx| {
                        let _ = cx.update(|cx| open_markdown_paths(&editor, paths, cx));
                    })
                    .detach();
            } else {
                pending_open_paths.borrow_mut().extend(paths);
            }
        });
    }

    application.run(move |cx| {
        #[cfg(target_os = "macos")]
        {
            *async_app_for_open_urls.borrow_mut() = Some(cx.to_async());
        }
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
            KeyBinding::new("cmd-up", MoveToDocumentStart, Some("MarkdownEditor")),
            KeyBinding::new("cmd-down", MoveToDocumentEnd, Some("MarkdownEditor")),
            KeyBinding::new(
                "shift-cmd-up",
                SelectToDocumentStart,
                Some("MarkdownEditor"),
            ),
            KeyBinding::new(
                "shift-cmd-down",
                SelectToDocumentEnd,
                Some("MarkdownEditor"),
            ),
            KeyBinding::new("enter", Enter, Some("MarkdownEditor")),
            KeyBinding::new("shift-enter", InsertSoftBreak, Some("MarkdownEditor")),
            KeyBinding::new("tab", Tab, Some("MarkdownEditor")),
            KeyBinding::new("cmd-c", Copy, Some("MarkdownEditor")),
            KeyBinding::new("cmd-x", Cut, Some("MarkdownEditor")),
            KeyBinding::new("cmd-v", Paste, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-v", PastePlainText, Some("MarkdownEditor")),
            KeyBinding::new(
                "cmd-ctrl-space",
                ShowCharacterPalette,
                Some("MarkdownEditor"),
            ),
            KeyBinding::new("alt-left", MoveWordLeft, Some("MarkdownEditor")),
            KeyBinding::new("alt-right", MoveWordRight, Some("MarkdownEditor")),
            KeyBinding::new("shift-alt-left", SelectWordLeft, Some("MarkdownEditor")),
            KeyBinding::new("shift-alt-right", SelectWordRight, Some("MarkdownEditor")),
            KeyBinding::new("alt-backspace", DeleteWordLeft, Some("MarkdownEditor")),
            KeyBinding::new("alt-delete", DeleteWordRight, Some("MarkdownEditor")),
            KeyBinding::new(
                "cmd-backspace",
                DeleteToLineStart,
                Some("MarkdownEditor && !Sidebar"),
            ),
            KeyBinding::new("cmd-delete", DeleteToLineEnd, Some("MarkdownEditor")),
            KeyBinding::new("cmd-/", ToggleLineComment, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-[", Fold, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-]", Unfold, Some("MarkdownEditor")),
            KeyBinding::new("ctrl-j", JoinLines, Some("MarkdownEditor")),
            KeyBinding::new("cmd-b", ToggleBold, Some("MarkdownEditor")),
            KeyBinding::new("cmd-i", ToggleItalic, Some("MarkdownEditor")),
            KeyBinding::new("cmd-u", ToggleUnderline, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-x", ToggleStrikethrough, Some("MarkdownEditor")),
            KeyBinding::new("cmd-e", ToggleInlineCode, Some("MarkdownEditor")),
            KeyBinding::new("cmd-k", ToggleLink, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-q", ToggleQuote, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-enter", ToggleTodo, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-u", ToggleUnorderedList, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-o", ToggleOrderedList, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-t", InsertTable, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-c", InsertCodeBlock, Some("MarkdownEditor")),
            KeyBinding::new("cmd-1", InsertHeading1, Some("MarkdownEditor")),
            KeyBinding::new("cmd-2", InsertHeading2, Some("MarkdownEditor")),
            KeyBinding::new("cmd-3", InsertHeading3, Some("MarkdownEditor")),
            KeyBinding::new("cmd-4", InsertHeading4, Some("MarkdownEditor")),
            KeyBinding::new("cmd-5", InsertHeading5, Some("MarkdownEditor")),
            KeyBinding::new("cmd-6", InsertHeading6, Some("MarkdownEditor")),
            KeyBinding::new("cmd-0", SetParagraph, Some("MarkdownEditor")),
            KeyBinding::new("cmd-=", ZoomIn, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-=", ZoomIn, Some("MarkdownEditor")),
            KeyBinding::new("cmd-minus", ZoomOut, Some("MarkdownEditor")),
            KeyBinding::new("cmd--", ZoomOut, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-0", ZoomReset, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-s", ToggleSidebar, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-l", ToggleEditorLeftAlign, Some("MarkdownEditor")),
            KeyBinding::new("cmd-l", SelectLine, Some("MarkdownEditor")),
            KeyBinding::new("cmd-enter", InsertLineBelow, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-enter", InsertLineAbove, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-d", DuplicateLine, Some("MarkdownEditor")),
            KeyBinding::new("shift-alt-up", CopyLineUp, Some("MarkdownEditor")),
            KeyBinding::new("shift-alt-down", CopyLineDown, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-k", DeleteLine, Some("MarkdownEditor")),
            KeyBinding::new("shift-tab", BackTab, Some("MarkdownEditor")),
            KeyBinding::new("alt-up", MoveLineUp, Some("MarkdownEditor")),
            KeyBinding::new("alt-down", MoveLineDown, Some("MarkdownEditor")),
            KeyBinding::new("cmd-s", Save, Some("MarkdownEditor")),
            KeyBinding::new("cmd-n", NewDocument, Some("MarkdownEditor && !Sidebar")),
            KeyBinding::new("cmd-t", NewTab, Some("MarkdownEditor")),
            KeyBinding::new("cmd-,", OpenSettings, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-p", ToggleCommandPalette, Some("MarkdownEditor")),
            KeyBinding::new("cmd-q", Quit, Some("MarkdownEditor")),
            KeyBinding::new("escape", Escape, Some("MarkdownEditor")),
            KeyBinding::new("cmd-ctrl-1", ToggleOutline, Some("MarkdownEditor")),
            KeyBinding::new("cmd-ctrl-t", ToggleTerminalPanel, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-i", ToggleInfoPanel, Some("MarkdownEditor")),
            KeyBinding::new("ctrl-shift-g", ToggleGitHistory, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-y", ToggleThemeDebugger, Some("MarkdownEditor")),
            KeyBinding::new("alt-cmd-up", AddCursorAbove, Some("MarkdownEditor")),
            KeyBinding::new("alt-cmd-down", AddCursorBelow, Some("MarkdownEditor")),
            KeyBinding::new("cmd-d", SelectNextOccurrence, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-l", SelectAllOccurrences, Some("MarkdownEditor")),
            // Tab management
            KeyBinding::new("cmd-w", CloseActiveTab, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-t", ReopenClosedTab, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-[", ActivatePreviousTab, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-]", ActivateNextTab, Some("MarkdownEditor")),
            KeyBinding::new("cmd-\\", SplitPaneRight, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-\\", SplitPaneDown, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-\\", FocusNextPane, Some("MarkdownEditor")),
            // Search
            KeyBinding::new("cmd-f", Find, Some("MarkdownEditor")),
            KeyBinding::new("cmd-g", FindNext, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-g", FindPrevious, Some("MarkdownEditor")),
            KeyBinding::new("cmd-alt-f", ToggleReplace, Some("MarkdownEditor")),
            KeyBinding::new("cmd-shift-f", FindInFiles, Some("MarkdownEditor")),
            KeyBinding::new("cmd-p", QuickOpen, Some("MarkdownEditor")),
            KeyBinding::new("cmd-o", OpenFileDialog, None),
            KeyBinding::new("alt-z", ToggleWordWrap, Some("MarkdownEditor")),
            KeyBinding::new(
                "cmd-alt-ctrl-p",
                ToggleRuntimeProfile,
                Some("MarkdownEditor"),
            ),
            KeyBinding::new(
                "cmd-shift-alt-ctrl-p",
                WriteRuntimeProfileSnapshot,
                Some("MarkdownEditor"),
            ),
        ]);

        cx.bind_keys([
            KeyBinding::new("cmd-alt-c", ToggleSearchCaseSensitive, Some("SearchBar")),
            KeyBinding::new("cmd-alt-w", ToggleSearchWholeWord, Some("SearchBar")),
            KeyBinding::new("cmd-alt-r", ToggleSearchRegex, Some("SearchBar")),
        ]);

        cx.bind_keys([
            KeyBinding::new("cmd-ctrl-t", ToggleTerminalPanel, Some("ChatTerminal")),
            KeyBinding::new("cmd-n", NewChatTab, Some("ChatTerminal")),
            KeyBinding::new("cmd-w", CloseActiveChatTab, Some("ChatTerminal")),
            KeyBinding::new("cmd-k", ClearTerminal, Some("ChatTerminal")),
        ]);

        cx.bind_keys([
            KeyBinding::new("v", WhiteboardSelectTool, Some("Whiteboard && !TextInput")),
            KeyBinding::new(
                "r",
                WhiteboardRectangleTool,
                Some("Whiteboard && !TextInput"),
            ),
            KeyBinding::new("o", WhiteboardEllipseTool, Some("Whiteboard && !TextInput")),
            KeyBinding::new("a", WhiteboardArrowTool, Some("Whiteboard && !TextInput")),
            KeyBinding::new("l", WhiteboardLineTool, Some("Whiteboard && !TextInput")),
            KeyBinding::new("t", WhiteboardTextTool, Some("Whiteboard && !TextInput")),
        ]);

        cx.bind_keys([
            KeyBinding::new("cmd-n", SidebarCreateFile, Some("Sidebar && !TextInput")),
            KeyBinding::new(
                "cmd-shift-n",
                SidebarCreateFolder,
                Some("Sidebar && !TextInput"),
            ),
            KeyBinding::new("f2", SidebarRename, Some("Sidebar && !TextInput")),
            KeyBinding::new(
                "cmd-backspace",
                SidebarDelete,
                Some("Sidebar && !TextInput"),
            ),
        ]);

        // TextInput keybindings
        cx.bind_keys([
            KeyBinding::new("left", text_input::TextInputLeft, Some("TextInput")),
            KeyBinding::new("right", text_input::TextInputRight, Some("TextInput")),
            KeyBinding::new("cmd-left", text_input::TextInputHome, Some("TextInput")),
            KeyBinding::new("cmd-right", text_input::TextInputEnd, Some("TextInput")),
            KeyBinding::new(
                "shift-left",
                text_input::TextInputSelectLeft,
                Some("TextInput"),
            ),
            KeyBinding::new(
                "shift-right",
                text_input::TextInputSelectRight,
                Some("TextInput"),
            ),
            KeyBinding::new(
                "shift-cmd-left",
                text_input::TextInputSelectHome,
                Some("TextInput"),
            ),
            KeyBinding::new(
                "shift-cmd-right",
                text_input::TextInputSelectEnd,
                Some("TextInput"),
            ),
            KeyBinding::new("cmd-a", text_input::TextInputSelectAll, Some("TextInput")),
            KeyBinding::new(
                "backspace",
                text_input::TextInputBackspace,
                Some("TextInput"),
            ),
            KeyBinding::new("delete", text_input::TextInputDelete, Some("TextInput")),
            KeyBinding::new(
                "alt-backspace",
                text_input::TextInputDeleteWordLeft,
                Some("TextInput"),
            ),
            KeyBinding::new("cmd-c", text_input::TextInputCopy, Some("TextInput")),
            KeyBinding::new("cmd-x", text_input::TextInputCut, Some("TextInput")),
            KeyBinding::new("cmd-v", text_input::TextInputPaste, Some("TextInput")),
            KeyBinding::new(
                "alt-left",
                text_input::TextInputMoveWordLeft,
                Some("TextInput"),
            ),
            KeyBinding::new(
                "alt-right",
                text_input::TextInputMoveWordRight,
                Some("TextInput"),
            ),
            KeyBinding::new(
                "shift-alt-left",
                text_input::TextInputSelectWordLeft,
                Some("TextInput"),
            ),
            KeyBinding::new(
                "shift-alt-right",
                text_input::TextInputSelectWordRight,
                Some("TextInput"),
            ),
            KeyBinding::new(
                "alt-delete",
                text_input::TextInputDeleteWordRight,
                Some("TextInput"),
            ),
            KeyBinding::new(
                "cmd-backspace",
                text_input::TextInputDeleteToStart,
                Some("TextInput"),
            ),
            KeyBinding::new(
                "ctrl-k",
                text_input::TextInputDeleteToEnd,
                Some("TextInput"),
            ),
            KeyBinding::new("cmd-z", text_input::TextInputUndo, Some("TextInput")),
            KeyBinding::new("up", text_input::TextInputPickerUp, Some("TextInput")),
            KeyBinding::new("down", text_input::TextInputPickerDown, Some("TextInput")),
            KeyBinding::new("enter", text_input::TextInputSubmit, Some("TextInput")),
            KeyBinding::new(
                "shift-enter",
                text_input::TextInputSubmitBackward,
                Some("TextInput"),
            ),
        ]);

        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &OpenTerminalWindow, cx| {
            if let Err(error) = open_terminal_window(cx) {
                eprintln!("failed to open terminal window: {error:#}");
            }
        });

        // User keybinding overrides from config.yaml are registered last so
        // they take precedence over the default bindings above.
        editor::register_keybinding_overrides(cx);
        editor::set_application_menus(cx);

        #[cfg(target_os = "macos")]
        let editor_for_window = editor_for_open_urls.clone();
        #[cfg(target_os = "macos")]
        let pending_paths_for_window = pending_open_paths.clone();
        let options = editor_window_options(cx);

        cx.open_window(options, |window, cx| {
            configure_editor_window(window);
            let view = cx.new(|cx| EditorView::new(cx));
            window.focus(&view.focus_handle(cx));
            #[cfg(target_os = "macos")]
            {
                *editor_for_window.borrow_mut() = Some(view.clone());
                let pending_paths = std::mem::take(&mut *pending_paths_for_window.borrow_mut());
                if !pending_paths.is_empty() {
                    open_markdown_paths(&view, pending_paths, cx);
                }
            }
            view
        })
        .expect("failed to open main window");
    });
}

#[cfg(all(test, target_os = "macos"))]
mod macos_file_open_tests {
    use super::macos_markdown_path_from_url;

    #[test]
    fn decodes_markdown_file_urls_with_spaces_and_unicode() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("示例 note.mdx");
        std::fs::write(&path, "# MDX").expect("write mdx");
        let encoded_path = path
            .to_string_lossy()
            .replace(' ', "%20")
            .replace("示例", "%E7%A4%BA%E4%BE%8B");

        assert_eq!(
            macos_markdown_path_from_url(&format!("file://{encoded_path}")),
            std::fs::canonicalize(path).ok(),
        );
    }

    #[test]
    fn rejects_non_markdown_file_urls() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("notes.txt");
        std::fs::write(&path, "notes").expect("write text");

        assert_eq!(
            macos_markdown_path_from_url(&format!("file://{}", path.display())),
            None,
        );
        assert_eq!(
            macos_markdown_path_from_url("https://example.com/a.md"),
            None
        );
    }
}
