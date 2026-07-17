use gpui::{Action, AnyElement, Hsla, SharedString, div, prelude::*, px};

use crate::{
    actions::*,
    i18n::{TextKey, UiLanguage},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CommandId {
    NewDocument,
    NewTab,
    OpenFile,
    QuickOpen,
    Save,
    SaveAs,
    OpenSettings,
    CloseTab,
    Find,
    FindInFiles,
    ToggleSidebar,
    ToggleOutline,
    ToggleChat,
    ToggleInfo,
    ToggleGitHistory,
    ToggleTheme,
    Bold,
    Italic,
    Underline,
    Strikethrough,
    InlineCode,
    Link,
    Image,
    Quote,
    Todo,
    UnorderedList,
    OrderedList,
    Table,
    CodeBlock,
    Paragraph,
    Fold,
    Unfold,
    FoldAll,
    UnfoldAll,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

#[derive(Clone, Copy)]
pub(crate) struct CommandSpec {
    pub id: CommandId,
    pub title: TextKey,
    pub category: &'static str,
    pub shortcut: Option<&'static str>,
    pub alternate_shortcuts: &'static [&'static str],
    pub markdown_only: bool,
    pub title_override: Option<fn(UiLanguage) -> &'static str>,
}

impl CommandSpec {
    pub(crate) fn localized_title(self, language: UiLanguage) -> &'static str {
        self.title_override
            .map(|title| title(language))
            .unwrap_or_else(|| crate::i18n::translate(language, self.title))
    }
}

impl CommandId {
    pub(crate) fn action(self) -> Box<dyn Action> {
        match self {
            Self::NewDocument => Box::new(NewDocument),
            Self::NewTab => Box::new(NewTab),
            Self::OpenFile => Box::new(OpenFileDialog),
            Self::QuickOpen => Box::new(QuickOpen),
            Self::Save => Box::new(Save),
            Self::SaveAs => Box::new(SaveAs),
            Self::OpenSettings => Box::new(OpenSettings),
            Self::CloseTab => Box::new(CloseActiveTab),
            Self::Find => Box::new(Find),
            Self::FindInFiles => Box::new(FindInFiles),
            Self::ToggleSidebar => Box::new(ToggleSidebar),
            Self::ToggleOutline => Box::new(ToggleOutline),
            Self::ToggleChat => Box::new(ToggleChatPanel),
            Self::ToggleInfo => Box::new(ToggleInfoPanel),
            Self::ToggleGitHistory => Box::new(ToggleGitHistory),
            Self::ToggleTheme => Box::new(ToggleTheme),
            Self::Bold => Box::new(ToggleBold),
            Self::Italic => Box::new(ToggleItalic),
            Self::Underline => Box::new(ToggleUnderline),
            Self::Strikethrough => Box::new(ToggleStrikethrough),
            Self::InlineCode => Box::new(ToggleInlineCode),
            Self::Link => Box::new(ToggleLink),
            Self::Image => Box::new(ToggleImage),
            Self::Quote => Box::new(ToggleQuote),
            Self::Todo => Box::new(ToggleTodo),
            Self::UnorderedList => Box::new(ToggleUnorderedList),
            Self::OrderedList => Box::new(ToggleOrderedList),
            Self::Table => Box::new(InsertTable),
            Self::CodeBlock => Box::new(InsertCodeBlock),
            Self::Paragraph => Box::new(SetParagraph),
            Self::Fold => Box::new(Fold),
            Self::Unfold => Box::new(Unfold),
            Self::FoldAll => Box::new(FoldAll),
            Self::UnfoldAll => Box::new(UnfoldAll),
            Self::ZoomIn => Box::new(ZoomIn),
            Self::ZoomOut => Box::new(ZoomOut),
            Self::ZoomReset => Box::new(ZoomReset),
        }
    }

    pub(crate) fn english_description(self) -> &'static str {
        match self {
            Self::NewDocument => "Create a new document",
            Self::NewTab => "Open a new editor tab",
            Self::OpenFile => "Open a file from disk",
            Self::QuickOpen => "Quickly open a workspace file",
            Self::Save => "Save the current document",
            Self::SaveAs => "Save the document with a new name",
            Self::OpenSettings => "Open application settings",
            Self::CloseTab => "Close the active tab",
            Self::Find => "Find text in the current document",
            Self::FindInFiles => "Search across workspace files",
            Self::ToggleSidebar => "Show or hide the sidebar",
            Self::ToggleOutline => "Show or hide the document outline",
            Self::ToggleChat => "Show or hide Terminal",
            Self::ToggleInfo => "Show or hide file information",
            Self::ToggleGitHistory => "Show or hide Git history",
            Self::ToggleTheme => "Switch between light and dark themes",
            Self::Bold => "Toggle bold formatting",
            Self::Italic => "Toggle italic formatting",
            Self::Underline => "Toggle underline formatting",
            Self::Strikethrough => "Toggle strikethrough formatting",
            Self::InlineCode => "Toggle inline code formatting",
            Self::Link => "Insert or edit a link",
            Self::Image => "Insert an image",
            Self::Quote => "Toggle block quote formatting",
            Self::Todo => "Toggle task list formatting",
            Self::UnorderedList => "Toggle bulleted list formatting",
            Self::OrderedList => "Toggle numbered list formatting",
            Self::Table => "Insert a table",
            Self::CodeBlock => "Insert a fenced code block",
            Self::Paragraph => "Convert the block to a paragraph",
            Self::Fold => "Fold the current section",
            Self::Unfold => "Unfold the current section",
            Self::FoldAll => "Fold all sections",
            Self::UnfoldAll => "Unfold all sections",
            Self::ZoomIn => "Increase editor zoom",
            Self::ZoomOut => "Decrease editor zoom",
            Self::ZoomReset => "Reset editor zoom",
        }
    }

    /// Stable identifier used for keybinding overrides in config.yaml.
    pub(crate) fn config_key(self) -> &'static str {
        match self {
            Self::NewDocument => "new_document",
            Self::NewTab => "new_tab",
            Self::OpenFile => "open_file",
            Self::QuickOpen => "quick_open",
            Self::Save => "save",
            Self::SaveAs => "save_as",
            Self::OpenSettings => "open_settings",
            Self::CloseTab => "close_tab",
            Self::Find => "find",
            Self::FindInFiles => "find_in_files",
            Self::ToggleSidebar => "toggle_sidebar",
            Self::ToggleOutline => "toggle_outline",
            Self::ToggleChat => "toggle_chat",
            Self::ToggleInfo => "toggle_info",
            Self::ToggleGitHistory => "toggle_git_history",
            Self::ToggleTheme => "toggle_theme",
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Underline => "underline",
            Self::Strikethrough => "strikethrough",
            Self::InlineCode => "inline_code",
            Self::Link => "link",
            Self::Image => "image",
            Self::Quote => "quote",
            Self::Todo => "todo",
            Self::UnorderedList => "unordered_list",
            Self::OrderedList => "ordered_list",
            Self::Table => "table",
            Self::CodeBlock => "code_block",
            Self::Paragraph => "paragraph",
            Self::Fold => "fold",
            Self::Unfold => "unfold",
            Self::FoldAll => "fold_all",
            Self::UnfoldAll => "unfold_all",
            Self::ZoomIn => "zoom_in",
            Self::ZoomOut => "zoom_out",
            Self::ZoomReset => "zoom_reset",
        }
    }

    pub(crate) fn from_config_key(key: &str) -> Option<Self> {
        COMMAND_SPECS
            .iter()
            .map(|spec| spec.id)
            .find(|id| id.config_key() == key)
    }
}

/// Convert a gpui keystroke string (e.g. "cmd-shift-s") to the display form
/// used in menus and the command palette (e.g. "⌘⇧S").
pub(crate) fn display_shortcut_from_keystroke(keystroke: &str) -> String {
    let mut tokens: Vec<&str> = keystroke.split('-').collect();
    // A trailing empty token means the key itself is "-" (e.g. "cmd--").
    let key = match tokens.pop() {
        Some("") => "-",
        Some(key) => key,
        None => return String::new(),
    };
    let mut display = String::new();
    // Match the modifier order used by COMMAND_SPECS display strings (⌃⌘⌥⇧).
    for modifier in ["ctrl", "cmd", "alt", "shift"] {
        if tokens.iter().any(|token| *token == modifier) {
            display.push(match modifier {
                "ctrl" => '⌃',
                "cmd" => '⌘',
                "alt" => '⌥',
                _ => '⇧',
            });
        }
    }
    let key_display = match key {
        "enter" => "↩".to_string(),
        "backspace" => "⌫".to_string(),
        "delete" => "⌦".to_string(),
        "escape" => "⎋".to_string(),
        "tab" => "⇥".to_string(),
        "space" => "Space".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "minus" => "-".to_string(),
        other => other.to_uppercase(),
    };
    display.push_str(&key_display);
    display
}

/// Validated keybinding overrides from settings: `(command, keystroke)` pairs
/// plus human-readable warnings for entries that could not be applied.
pub(crate) fn keybinding_overrides(
    keybindings: &std::collections::BTreeMap<String, String>,
) -> (Vec<(CommandId, String)>, Vec<String>) {
    let mut overrides = Vec::new();
    let mut warnings = Vec::new();
    let mut used_keystrokes: std::collections::HashMap<&str, CommandId> =
        std::collections::HashMap::new();
    for (config_key, keystroke) in keybindings {
        let Some(command) = CommandId::from_config_key(config_key) else {
            warnings.push(format!("unknown command \"{config_key}\""));
            continue;
        };
        let keystroke = keystroke.trim();
        if keystroke.is_empty()
            || keystroke
                .split_whitespace()
                .any(|stroke| gpui::Keystroke::parse(stroke).is_err())
        {
            warnings.push(format!(
                "invalid keystroke \"{keystroke}\" for \"{config_key}\""
            ));
            continue;
        }
        if let Some(existing) = used_keystrokes.insert(keystroke, command) {
            warnings.push(format!(
                "\"{keystroke}\" is bound to both \"{}\" and \"{config_key}\"",
                existing.config_key()
            ));
        }
        overrides.push((command, keystroke.to_string()));
    }
    (overrides, warnings)
}

/// Register user keybinding overrides on top of the default bindings. Later
/// bindings take precedence in GPUI, so overrides win over the defaults
/// registered in `main.rs`.
pub(crate) fn register_keybinding_overrides(cx: &mut gpui::App) {
    let settings = super::AppSettings::load();
    let (overrides, _warnings) = keybinding_overrides(&settings.keybindings);
    let context = gpui::KeyBindingContextPredicate::parse("MarkdownEditor")
        .ok()
        .map(std::rc::Rc::new);
    let bindings: Vec<gpui::KeyBinding> = overrides
        .into_iter()
        .filter_map(|(command, keystroke)| {
            gpui::KeyBinding::load(
                &keystroke,
                command.action(),
                context.clone(),
                false,
                None,
                &gpui::DummyKeyboardMapper,
            )
            .ok()
        })
        .collect();
    if !bindings.is_empty() {
        cx.bind_keys(bindings);
    }
}

/// Build the native macOS menu bar for the given UI language.
pub(crate) fn app_menus(language: UiLanguage) -> Vec<gpui::Menu> {
    use gpui::{Menu, MenuItem};

    let t = |key: TextKey| crate::i18n::translate(language, key);
    let command_item = |id: CommandId| -> MenuItem {
        let spec = COMMAND_SPECS
            .iter()
            .find(|spec| spec.id == id)
            .expect("command spec exists for menu command");
        MenuItem::Action {
            name: spec.localized_title(language).into(),
            action: id.action(),
            os_action: None,
        }
    };

    vec![
        Menu {
            name: crate::i18n::application_display_name(language).into(),
            items: vec![
                command_item(CommandId::OpenSettings),
                MenuItem::separator(),
                MenuItem::action(
                    format!(
                        "{} {}",
                        t(TextKey::QuitApp),
                        crate::i18n::application_display_name(language)
                    ),
                    crate::actions::Quit,
                ),
            ],
        },
        Menu {
            name: t(TextKey::MenuFile).into(),
            items: vec![
                command_item(CommandId::NewDocument),
                command_item(CommandId::NewTab),
                MenuItem::separator(),
                command_item(CommandId::OpenFile),
                command_item(CommandId::QuickOpen),
                MenuItem::separator(),
                command_item(CommandId::Save),
                command_item(CommandId::SaveAs),
                MenuItem::separator(),
                command_item(CommandId::CloseTab),
            ],
        },
        Menu {
            name: t(TextKey::MenuEdit).into(),
            items: vec![
                MenuItem::action(t(TextKey::Undo), crate::actions::Undo),
                MenuItem::action(t(TextKey::Redo), crate::actions::Redo),
                MenuItem::separator(),
                MenuItem::action(t(TextKey::Cut), crate::actions::Cut),
                MenuItem::action(t(TextKey::Copy), crate::actions::Copy),
                MenuItem::action(t(TextKey::Paste), crate::actions::Paste),
                MenuItem::action(t(TextKey::SelectAll), crate::actions::SelectAll),
                MenuItem::separator(),
                command_item(CommandId::Find),
                command_item(CommandId::FindInFiles),
            ],
        },
        Menu {
            name: t(TextKey::MenuFormat).into(),
            items: vec![
                command_item(CommandId::Bold),
                command_item(CommandId::Italic),
                command_item(CommandId::Underline),
                command_item(CommandId::Strikethrough),
                command_item(CommandId::InlineCode),
                MenuItem::separator(),
                command_item(CommandId::Link),
                command_item(CommandId::Quote),
                command_item(CommandId::Todo),
                command_item(CommandId::UnorderedList),
                command_item(CommandId::OrderedList),
                command_item(CommandId::Table),
                command_item(CommandId::CodeBlock),
                command_item(CommandId::Paragraph),
            ],
        },
        Menu {
            name: t(TextKey::MenuView).into(),
            items: vec![
                command_item(CommandId::ToggleSidebar),
                command_item(CommandId::ToggleOutline),
                command_item(CommandId::ToggleChat),
                command_item(CommandId::ToggleInfo),
                command_item(CommandId::ToggleGitHistory),
                MenuItem::separator(),
                command_item(CommandId::ZoomIn),
                command_item(CommandId::ZoomOut),
                command_item(CommandId::ZoomReset),
                MenuItem::separator(),
                command_item(CommandId::ToggleTheme),
            ],
        },
        // GPUI registers a menu named exactly "Window" as NSApplication's
        // windows menu. AppKit then keeps every open window in this menu and
        // mirrors the same list in the Dock context menu.
        Menu {
            name: "Window".into(),
            items: Vec::new(),
        },
    ]
}

pub(crate) fn dock_menu(language: UiLanguage) -> Vec<gpui::MenuItem> {
    vec![gpui::MenuItem::action(
        crate::i18n::open_terminal_window_label(language),
        crate::actions::OpenTerminalWindow,
    )]
}

/// Install the native menu bar and Dock menu using the persisted UI language.
pub(crate) fn set_application_menus(cx: &mut gpui::App) {
    let language = super::AppSettings::load().ui_language();
    cx.set_menus(app_menus(language));
    cx.set_dock_menu(dock_menu(language));
}

pub(crate) const COMMAND_SPECS: &[CommandSpec] = &[
    command_with_title(
        CommandId::NewDocument,
        TextKey::NewFile,
        "File",
        Some("⌘N"),
        false,
        new_document_title,
    ),
    command_with_title(
        CommandId::NewTab,
        TextKey::NewFile,
        "File",
        Some("⌘T"),
        false,
        new_tab_title,
    ),
    command(CommandId::OpenFile, TextKey::OpenFile, "File", "⌘O", false),
    command_with_title(
        CommandId::QuickOpen,
        TextKey::QuickOpenPlaceholder,
        "File",
        Some("⌘P"),
        false,
        quick_open_title,
    ),
    command(CommandId::Save, TextKey::Save, "File", "⌘S", false),
    command_without_shortcut(CommandId::SaveAs, TextKey::SaveAs, "File", false),
    command(
        CommandId::OpenSettings,
        TextKey::SettingsTitle,
        "Application",
        "⌘,",
        false,
    ),
    command(CommandId::CloseTab, TextKey::Close, "Tab", "⌘W", false),
    command_with_title(
        CommandId::Find,
        TextKey::SearchPlaceholder,
        "Search",
        Some("⌘F"),
        false,
        find_document_title,
    ),
    command_with_title(
        CommandId::FindInFiles,
        TextKey::WorkspaceSearchPlaceholder,
        "Search",
        Some("⌘⇧F"),
        false,
        find_files_title,
    ),
    command(
        CommandId::ToggleSidebar,
        TextKey::ToolbarSidebarTooltip,
        "View",
        "⌘⇧S",
        false,
    ),
    command(
        CommandId::ToggleOutline,
        TextKey::ToolbarOutlineTooltip,
        "View",
        "⌃⌘1",
        false,
    ),
    command_with_title(
        CommandId::ToggleChat,
        TextKey::AddFileToChat,
        "View",
        Some("⌃⌘I"),
        false,
        crate::i18n::terminal_label,
    ),
    command(
        CommandId::ToggleInfo,
        TextKey::FileInfo,
        "View",
        "⌘⌥I",
        false,
    ),
    command(
        CommandId::ToggleGitHistory,
        TextKey::GitHistory,
        "View",
        "⌃⇧G",
        false,
    ),
    command_without_shortcut(
        CommandId::ToggleTheme,
        TextKey::ToolbarThemeTooltip,
        "View",
        false,
    ),
    command(CommandId::Bold, TextKey::Bold, "Format", "⌘B", true),
    command(CommandId::Italic, TextKey::Italic, "Format", "⌘I", true),
    command_with_title(
        CommandId::Underline,
        TextKey::Italic,
        "Format",
        Some("⌘U"),
        true,
        underline_title,
    ),
    command(
        CommandId::Strikethrough,
        TextKey::Strikethrough,
        "Format",
        "⌘⇧X",
        true,
    ),
    command(
        CommandId::InlineCode,
        TextKey::InlineCode,
        "Format",
        "⌘E",
        true,
    ),
    command(CommandId::Link, TextKey::Link, "Format", "⌘K", true),
    command_without_shortcut(CommandId::Image, TextKey::Image, "Format", true),
    command(CommandId::Quote, TextKey::Quote, "Format", "⌘⌥Q", true),
    command(CommandId::Todo, TextKey::Todo, "Format", "⌘⌥↩", true),
    command(
        CommandId::UnorderedList,
        TextKey::UnorderedList,
        "Format",
        "⌘⌥U",
        true,
    ),
    command(
        CommandId::OrderedList,
        TextKey::OrderedList,
        "Format",
        "⌘⌥O",
        true,
    ),
    command(CommandId::Table, TextKey::Table, "Format", "⌘⌥T", true),
    command(
        CommandId::CodeBlock,
        TextKey::CodeBlock,
        "Format",
        "⌘⌥C",
        true,
    ),
    command(
        CommandId::Paragraph,
        TextKey::Paragraph,
        "Format",
        "⌘0",
        true,
    ),
    command_with_title(
        CommandId::Fold,
        TextKey::CollapseAllDirectories,
        "Fold",
        Some("⌘⌥["),
        false,
        fold_section_title,
    ),
    command_with_title(
        CommandId::Unfold,
        TextKey::ExpandAllDirectories,
        "Fold",
        Some("⌘⌥]"),
        false,
        unfold_section_title,
    ),
    command_with_title(
        CommandId::FoldAll,
        TextKey::CollapseAllDirectories,
        "Fold",
        None,
        false,
        fold_all_title,
    ),
    command_with_title(
        CommandId::UnfoldAll,
        TextKey::ExpandAllDirectories,
        "Fold",
        None,
        false,
        unfold_all_title,
    ),
    command_with_title(
        CommandId::ZoomIn,
        TextKey::Reset,
        "View",
        Some("⌘="),
        false,
        zoom_in_title,
    ),
    command_with_title(
        CommandId::ZoomOut,
        TextKey::Reset,
        "View",
        Some("⌘-"),
        false,
        zoom_out_title,
    ),
    command_with_title(
        CommandId::ZoomReset,
        TextKey::Reset,
        "View",
        Some("⌘⌥0"),
        false,
        zoom_reset_title,
    ),
];

const fn command(
    id: CommandId,
    title: TextKey,
    category: &'static str,
    shortcut: &'static str,
    markdown_only: bool,
) -> CommandSpec {
    CommandSpec {
        id,
        title,
        category,
        shortcut: Some(shortcut),
        alternate_shortcuts: &[],
        markdown_only,
        title_override: None,
    }
}

const fn command_without_shortcut(
    id: CommandId,
    title: TextKey,
    category: &'static str,
    markdown_only: bool,
) -> CommandSpec {
    CommandSpec {
        id,
        title,
        category,
        shortcut: None,
        alternate_shortcuts: &[],
        markdown_only,
        title_override: None,
    }
}

const fn command_with_title(
    id: CommandId,
    title: TextKey,
    category: &'static str,
    shortcut: Option<&'static str>,
    markdown_only: bool,
    title_override: fn(UiLanguage) -> &'static str,
) -> CommandSpec {
    CommandSpec {
        id,
        title,
        category,
        shortcut,
        alternate_shortcuts: &[],
        markdown_only,
        title_override: Some(title_override),
    }
}

fn underline_title(language: UiLanguage) -> &'static str {
    crate::i18n::format_extra_label(language, "underline")
}

fn new_document_title(language: UiLanguage) -> &'static str {
    crate::i18n::command_action_label(language, "new_document")
}

fn new_tab_title(language: UiLanguage) -> &'static str {
    crate::i18n::command_action_label(language, "new_tab")
}

fn quick_open_title(language: UiLanguage) -> &'static str {
    crate::i18n::command_action_label(language, "quick_open")
}

fn find_document_title(language: UiLanguage) -> &'static str {
    crate::i18n::command_action_label(language, "find_document")
}

fn find_files_title(language: UiLanguage) -> &'static str {
    crate::i18n::command_action_label(language, "find_files")
}

fn fold_section_title(language: UiLanguage) -> &'static str {
    crate::i18n::command_action_label(language, "fold_section")
}

fn unfold_section_title(language: UiLanguage) -> &'static str {
    crate::i18n::command_action_label(language, "unfold_section")
}

fn fold_all_title(language: UiLanguage) -> &'static str {
    crate::i18n::command_action_label(language, "fold_all")
}

fn unfold_all_title(language: UiLanguage) -> &'static str {
    crate::i18n::command_action_label(language, "unfold_all")
}

fn zoom_in_title(language: UiLanguage) -> &'static str {
    crate::i18n::preview_action_label(language, "zoom_in")
}

fn zoom_out_title(language: UiLanguage) -> &'static str {
    crate::i18n::preview_action_label(language, "zoom_out")
}

fn zoom_reset_title(language: UiLanguage) -> &'static str {
    crate::i18n::command_action_label(language, "reset_zoom")
}

#[cfg(test)]
mod command_tests {
    use std::collections::{HashMap, HashSet};

    use super::{COMMAND_SPECS, CommandId, estimate_text_width};
    use crate::i18n::UiLanguage;

    #[test]
    fn command_shortcuts_are_unique() {
        let mut shortcuts = HashSet::new();
        for spec in COMMAND_SPECS {
            for shortcut in spec
                .shortcut
                .into_iter()
                .chain(spec.alternate_shortcuts.iter().copied())
            {
                assert!(
                    shortcuts.insert(shortcut),
                    "duplicate command shortcut: {shortcut}"
                );
            }
        }
    }

    #[test]
    fn cjk_text_width_uses_full_em_glyphs() {
        assert_eq!(estimate_text_width("内容大纲", 13.0), 52.0);
    }

    #[test]
    fn sidebar_owns_cmd_shift_s_and_save_as_has_no_default_shortcut() {
        let sidebar = COMMAND_SPECS
            .iter()
            .find(|spec| spec.id == CommandId::ToggleSidebar)
            .unwrap();
        let save_as = COMMAND_SPECS
            .iter()
            .find(|spec| spec.id == CommandId::SaveAs)
            .unwrap();

        assert_eq!(sidebar.shortcut, Some("⌘⇧S"));
        assert_eq!(save_as.shortcut, None);
    }

    #[test]
    fn command_titles_are_unique_in_every_language() {
        let mut duplicates = Vec::new();
        for &language in UiLanguage::supported() {
            let mut titles = HashMap::new();
            for spec in COMMAND_SPECS {
                let title = spec.localized_title(language);
                assert!(
                    !title.trim().is_empty(),
                    "empty command title for {language:?}: {:?}",
                    spec.id
                );
                if let Some(previous) = titles.insert(title, spec.id) {
                    duplicates.push(format!(
                        "{language:?}: {title:?} ({previous:?}, {:?})",
                        spec.id
                    ));
                }
            }
        }
        assert!(
            duplicates.is_empty(),
            "duplicate command titles:\n{}",
            duplicates.join("\n")
        );
    }

    #[test]
    fn config_keys_are_unique_and_roundtrip() {
        let mut seen = HashMap::new();
        for spec in COMMAND_SPECS {
            let key = spec.id.config_key();
            assert!(
                seen.insert(key, spec.id).is_none(),
                "duplicate config key {key:?}"
            );
            assert_eq!(super::CommandId::from_config_key(key), Some(spec.id));
        }
        assert_eq!(super::CommandId::from_config_key("unknown"), None);
    }

    #[test]
    fn keystroke_display_conversion() {
        assert_eq!(super::display_shortcut_from_keystroke("cmd-shift-s"), "⌘⇧S");
        assert_eq!(super::display_shortcut_from_keystroke("cmd-,"), "⌘,");
        assert_eq!(super::display_shortcut_from_keystroke("cmd--"), "⌘-");
        assert_eq!(
            super::display_shortcut_from_keystroke("cmd-alt-enter"),
            "⌘⌥↩"
        );
        assert_eq!(
            super::display_shortcut_from_keystroke("ctrl-shift-g"),
            "⌃⇧G"
        );
    }

    #[test]
    fn keybinding_overrides_validate_entries() {
        let mut keybindings = std::collections::BTreeMap::new();
        keybindings.insert("save".to_string(), "cmd-shift-s".to_string());
        keybindings.insert("bold".to_string(), "cmd-shift-s".to_string());
        keybindings.insert("nonexistent".to_string(), "cmd-x".to_string());
        keybindings.insert("italic".to_string(), "".to_string());

        let (overrides, warnings) = super::keybinding_overrides(&keybindings);
        assert_eq!(overrides.len(), 2);
        assert!(
            overrides
                .iter()
                .any(|(id, keys)| *id == CommandId::Save && keys == "cmd-shift-s")
        );
        assert!(
            overrides
                .iter()
                .any(|(id, keys)| *id == CommandId::Bold && keys == "cmd-shift-s")
        );
        // Unknown command, invalid keystroke, and the duplicate binding each warn.
        assert_eq!(warnings.len(), 3);
    }

    /// Convert a COMMAND_SPECS display shortcut ("⌘⇧X") to the long form used
    /// by the help documents ("Command + Shift + X").
    fn docs_shortcut(display: &str) -> String {
        display
            .chars()
            .map(|ch| match ch {
                '⌃' => "Control".to_string(),
                '⌥' => "Option".to_string(),
                '⇧' => "Shift".to_string(),
                '⌘' => "Command".to_string(),
                '↩' => "Enter".to_string(),
                other => other.to_string(),
            })
            .collect::<Vec<_>>()
            .join(" + ")
    }

    #[test]
    fn help_docs_list_format_command_shortcuts_for_every_language() {
        let docs_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs");
        let mut problems = Vec::new();
        for &language in UiLanguage::supported() {
            let file = docs_dir.join(format!("help-{}.md", language.code().to_lowercase()));
            let Ok(content) = std::fs::read_to_string(&file) else {
                problems.push(format!("missing help doc {}", file.display()));
                continue;
            };
            for spec in COMMAND_SPECS {
                if spec.category != "Format" {
                    continue;
                }
                let Some(display) = spec.shortcut else {
                    continue;
                };
                let expected = docs_shortcut(display);
                if !content.contains(&expected) {
                    problems.push(format!(
                        "{}: missing \"{expected}\" for {:?}",
                        file.display(),
                        spec.id
                    ));
                }
            }
        }
        assert!(
            problems.is_empty(),
            "help docs out of sync with COMMAND_SPECS:\n{}",
            problems.join("\n")
        );
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ShortcutRenderStyle {
    pub font_size_px: f32,
    pub font_family: &'static str,
    pub text_color: Hsla,
    pub gap_px: f32,
}

pub(crate) fn render_shortcut(shortcut: &str, style: ShortcutRenderStyle) -> AnyElement {
    let tokens = tokenize_shortcut(shortcut);

    div()
        .flex()
        .items_center()
        .gap(px(style.gap_px))
        .font_family(SharedString::from(style.font_family))
        .children(tokens.into_iter().map(|token| {
            let width = estimate_token_width(&token, style.font_size_px);
            div()
                .w(px(width))
                .h(px(style.font_size_px))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .text_size(px(style.font_size_px))
                .line_height(px(style.font_size_px))
                .text_color(style.text_color)
                .child(token)
        }))
        .into_any_element()
}

pub(crate) fn estimate_shortcut_width(shortcut: &str, style: ShortcutRenderStyle) -> f32 {
    let tokens = tokenize_shortcut(shortcut);
    if tokens.is_empty() {
        return 0.0;
    }

    tokens
        .iter()
        .enumerate()
        .map(|(index, token)| {
            estimate_token_width(token, style.font_size_px)
                + if index + 1 < tokens.len() {
                    style.gap_px
                } else {
                    0.0
                }
        })
        .sum()
}

pub(crate) fn estimate_text_width(text: &str, font_size_px: f32) -> f32 {
    text.chars()
        .map(|ch| match ch {
            ' ' => font_size_px * 0.36,
            _ if ch.is_ascii_uppercase() => font_size_px * 0.62,
            _ if ch.is_ascii_lowercase() || ch.is_ascii_digit() => font_size_px * 0.56,
            _ if ch.is_ascii_punctuation() => font_size_px * 0.45,
            _ => font_size_px,
        })
        .sum()
}

fn tokenize_shortcut(shortcut: &str) -> Vec<String> {
    if shortcut.is_empty() {
        return Vec::new();
    }

    if shortcut.contains('-') && !shortcut.chars().any(is_special_key_symbol) {
        return shortcut
            .split('-')
            .filter_map(normalize_token)
            .collect::<Vec<_>>();
    }

    if !shortcut.chars().any(is_special_key_symbol) {
        return normalize_token(shortcut).into_iter().collect();
    }

    let mut tokens = Vec::new();
    let mut buffer = String::new();

    for ch in shortcut.chars() {
        if is_special_key_symbol(ch) {
            if !buffer.is_empty() {
                if let Some(token) = normalize_token(&buffer) {
                    tokens.push(token);
                }
                buffer.clear();
            }
            tokens.push(ch.to_string());
        } else {
            buffer.push(ch);
        }
    }

    if !buffer.is_empty() {
        if let Some(token) = normalize_token(&buffer) {
            tokens.push(token);
        }
    }

    tokens
}

fn estimate_token_width(token: &str, font_size_px: f32) -> f32 {
    let count = token.chars().count();
    if count == 1 {
        font_size_px.max(1.0)
    } else {
        estimate_text_width(token, font_size_px)
    }
}

fn normalize_token(token: &str) -> Option<String> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    let normalized = match token.to_ascii_lowercase().as_str() {
        "cmd" | "command" | "platform" | "super" | "win" => "⌘".to_string(),
        "alt" | "option" => "⌥".to_string(),
        "ctrl" | "control" => "⌃".to_string(),
        "shift" => "⇧".to_string(),
        "enter" | "return" => "↩".to_string(),
        "backspace" => "⌫".to_string(),
        "delete" => "⌦".to_string(),
        "tab" => "⇥".to_string(),
        "escape" | "esc" => "⎋".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        _ if token.len() == 1 && token.is_ascii() => token.to_ascii_uppercase(),
        _ => token.to_string(),
    };

    Some(normalized)
}

fn is_special_key_symbol(ch: char) -> bool {
    matches!(
        ch,
        '⌘' | '⌥' | '⌃' | '⇧' | '↩' | '⌫' | '⌦' | '⇥' | '⎋' | '↑' | '↓' | '←' | '→'
    )
}

#[cfg(test)]
mod native_menu_tests {
    use super::{app_menus, dock_menu};
    use crate::i18n::UiLanguage;

    #[test]
    fn native_menus_register_window_list_and_localized_terminal_action() {
        let menus = app_menus(UiLanguage::ChineseSimplified);
        assert!(menus.iter().any(|menu| menu.name.as_ref() == "Window"));

        let dock_items = dock_menu(UiLanguage::ChineseSimplified);
        assert!(matches!(
            dock_items.as_slice(),
            [gpui::MenuItem::Action { name, .. }] if name.as_ref() == "打开终端"
        ));
    }
}
