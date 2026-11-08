use gpui::{Action, AnyElement, Hsla, SharedString, div, prelude::*, px};

use crate::{
    actions::*,
    i18n::{TextKey, UiLanguage},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CommandId {
    Extra(super::commands::ExtraCommand),
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
    ToggleTerminal,
    ToggleInfo,
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
    Undo,
    Redo,
    SelectAll,
    DuplicateLine,
    DeleteLine,
    MoveLineUp,
    MoveLineDown,
    JoinLines,
    ToggleComment,
    SelectNextOccurrence,
    SelectAllOccurrences,
    AddCursorAbove,
    AddCursorBelow,
    ToggleWordWrap,
    ToggleLeftAlign,
    SplitRight,
    SplitDown,
    FocusNextPane,
    ReopenClosedTab,
    NextTab,
    PreviousTab,
    PinTab,
    OpenTerminalWindow,
    HorizontalRule,
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
    /// Key into `i18n::COMMAND_LABELS` for commands with no dedicated `TextKey`.
    pub label_key: Option<&'static str>,
}

impl CommandSpec {
    pub(crate) fn localized_category(self, language: UiLanguage) -> &'static str {
        match self.category {
            "File" => crate::i18n::translate(language, TextKey::MenuFile),
            "Edit" => crate::i18n::translate(language, TextKey::MenuEdit),
            "Format" => crate::i18n::translate(language, TextKey::MenuFormat),
            "View" => crate::i18n::translate(language, TextKey::MenuView),
            "Application" => crate::i18n::application_display_name(language),
            "Export" => crate::i18n::translate(language, TextKey::Export),
            "Image" => crate::i18n::translate(language, TextKey::Image),
            "Settings" => crate::i18n::translate(language, TextKey::SettingsTitle),
            "Terminal" => crate::i18n::terminal_label(language),
            category => crate::i18n::menu_command_label(language, category),
        }
    }

    pub(crate) fn localized_title(self, language: UiLanguage) -> String {
        if let CommandId::Extra(command) = self.id {
            return command.title(language);
        }
        if let Some(label_key) = self.label_key {
            return crate::i18n::command_label(language, label_key).to_string();
        }
        self.title_override
            .map(|title| title(language))
            .unwrap_or_else(|| crate::i18n::translate(language, self.title))
            .to_string()
    }
}

impl CommandId {
    /// Whether this build can run the command at all.
    ///
    /// The terminal commands need child processes, which the App Store
    /// sandbox denies; hide them rather than offer a button that always fails.
    pub(crate) fn is_available(self) -> bool {
        match self {
            Self::Extra(command) => command.is_available(),
            Self::ToggleTerminal | Self::OpenTerminalWindow => {
                crate::platform::capabilities::CHILD_PROCESSES
            }
            _ => true,
        }
    }

    pub(crate) fn action(self) -> Box<dyn Action> {
        match self {
            Self::Extra(command) => command.native_action(),
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
            Self::ToggleTerminal => Box::new(ToggleTerminalPanel),
            Self::ToggleInfo => Box::new(ToggleInfoPanel),
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
            Self::Undo => Box::new(Undo),
            Self::Redo => Box::new(Redo),
            Self::SelectAll => Box::new(SelectAll),
            Self::DuplicateLine => Box::new(DuplicateLine),
            Self::DeleteLine => Box::new(DeleteLine),
            Self::MoveLineUp => Box::new(MoveLineUp),
            Self::MoveLineDown => Box::new(MoveLineDown),
            Self::JoinLines => Box::new(JoinLines),
            Self::ToggleComment => Box::new(ToggleLineComment),
            Self::SelectNextOccurrence => Box::new(SelectNextOccurrence),
            Self::SelectAllOccurrences => Box::new(SelectAllOccurrences),
            Self::AddCursorAbove => Box::new(AddCursorAbove),
            Self::AddCursorBelow => Box::new(AddCursorBelow),
            Self::ToggleWordWrap => Box::new(ToggleWordWrap),
            Self::ToggleLeftAlign => Box::new(ToggleEditorLeftAlign),
            Self::SplitRight => Box::new(SplitPaneRight),
            Self::SplitDown => Box::new(SplitPaneDown),
            Self::FocusNextPane => Box::new(FocusNextPane),
            Self::ReopenClosedTab => Box::new(ReopenClosedTab),
            Self::NextTab => Box::new(ActivateNextTab),
            Self::PreviousTab => Box::new(ActivatePreviousTab),
            Self::PinTab => Box::new(PinTab),
            Self::OpenTerminalWindow => Box::new(OpenTerminalWindow),
            Self::HorizontalRule => Box::new(InsertHorizontalRule),
        }
    }

    pub(crate) fn english_description(self) -> &'static str {
        match self {
            Self::Extra(command) => command.description(),
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
            Self::ToggleTerminal => "Show or hide Terminal",
            Self::ToggleInfo => "Show or hide file information",
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
            Self::Undo => "Undo the last edit",
            Self::Redo => "Redo the last undone edit",
            Self::SelectAll => "Select the whole document",
            Self::DuplicateLine => "Duplicate the current line",
            Self::DeleteLine => "Delete the current line",
            Self::MoveLineUp => "Move the current line up",
            Self::MoveLineDown => "Move the current line down",
            Self::JoinLines => "Join the current line with the next",
            Self::ToggleComment => "Comment or uncomment the selection",
            Self::SelectNextOccurrence => "Select the next occurrence of the selection",
            Self::SelectAllOccurrences => "Select every occurrence of the selection",
            Self::AddCursorAbove => "Add a cursor on the line above",
            Self::AddCursorBelow => "Add a cursor on the line below",
            Self::ToggleWordWrap => "Turn soft wrapping on or off",
            Self::ToggleLeftAlign => "Left align or centre the editor content",
            Self::SplitRight => "Split the editor to the right",
            Self::SplitDown => "Split the editor downwards",
            Self::FocusNextPane => "Move focus to the next pane",
            Self::ReopenClosedTab => "Reopen the most recently closed tab",
            Self::NextTab => "Activate the next tab",
            Self::PreviousTab => "Activate the previous tab",
            Self::PinTab => "Pin or unpin the active tab",
            Self::OpenTerminalWindow => "Open a standalone terminal window",
            Self::HorizontalRule => "Insert a horizontal rule",
        }
    }

    /// Stable identifier used for keybinding overrides in settings.yaml.
    pub(crate) fn config_key(self) -> &'static str {
        match self {
            Self::Extra(command) => command.key(),
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
            Self::ToggleTerminal => "toggle_terminal",
            Self::ToggleInfo => "toggle_info",
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
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::SelectAll => "select_all",
            Self::DuplicateLine => "duplicate_line",
            Self::DeleteLine => "delete_line",
            Self::MoveLineUp => "move_line_up",
            Self::MoveLineDown => "move_line_down",
            Self::JoinLines => "join_lines",
            Self::ToggleComment => "toggle_comment",
            Self::SelectNextOccurrence => "select_next_occurrence",
            Self::SelectAllOccurrences => "select_all_occurrences",
            Self::AddCursorAbove => "add_cursor_above",
            Self::AddCursorBelow => "add_cursor_below",
            Self::ToggleWordWrap => "toggle_word_wrap",
            Self::ToggleLeftAlign => "toggle_left_align",
            Self::SplitRight => "split_right",
            Self::SplitDown => "split_down",
            Self::FocusNextPane => "focus_next_pane",
            Self::ReopenClosedTab => "reopen_closed_tab",
            Self::NextTab => "next_tab",
            Self::PreviousTab => "previous_tab",
            Self::PinTab => "pin_tab",
            Self::OpenTerminalWindow => "open_terminal_window",
            Self::HorizontalRule => "horizontal_rule",
        }
    }

    pub(crate) fn from_config_key(key: &str) -> Option<Self> {
        COMMAND_SPECS
            .iter()
            .map(|spec| spec.id)
            .find(|id| id.config_key() == key)
    }
}

/// Convert a gpui keystroke string (e.g. "secondary-shift-s") to the display
/// form used in menus and the command palette ("⌘⇧S", or "Ctrl+Shift+S").
pub(crate) fn display_shortcut_from_keystroke(keystroke: &str) -> String {
    let mut tokens: Vec<&str> = keystroke.split('-').collect();
    // A trailing empty token means the key itself is "-" (e.g. "cmd--").
    let key = match tokens.pop() {
        Some("") => "-",
        Some(key) => key,
        None => return String::new(),
    };

    let is_primary =
        |token: &str| token == "cmd" || token == "super" || token == "win" || token == "secondary";
    // gpui resolves `secondary` to Control everywhere except macOS.
    let has_ctrl = tokens.iter().any(|token| *token == "ctrl")
        || (!cfg!(target_os = "macos") && tokens.iter().any(|token| is_primary(token)));
    let has_primary = cfg!(target_os = "macos") && tokens.iter().any(|token| is_primary(token));
    let has_alt = tokens.iter().any(|token| *token == "alt");
    let has_shift = tokens.iter().any(|token| *token == "shift");

    let mut display = String::new();
    if cfg!(target_os = "macos") {
        // Match the modifier order used by the macOS menu bar (⌃⌘⌥⇧).
        for (present, symbol) in [
            (has_ctrl, '⌃'),
            (has_primary, '⌘'),
            (has_alt, '⌥'),
            (has_shift, '⇧'),
        ] {
            if present {
                display.push(symbol);
            }
        }
    } else {
        for (present, name) in [
            (has_ctrl, "Ctrl+"),
            (has_alt, "Alt+"),
            (has_shift, "Shift+"),
        ] {
            if present {
                display.push_str(name);
            }
        }
    }

    display.push_str(&display_key(key));
    display
}

/// Renders a keystroke for a tooltip or menu hint.
pub(crate) fn hint(keystroke: &str) -> gpui::SharedString {
    display_shortcut_from_keystroke(keystroke).into()
}

fn display_key(key: &str) -> String {
    if cfg!(target_os = "macos") {
        return match key {
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
    }

    match key {
        "enter" => "Enter".to_string(),
        "backspace" => "Backspace".to_string(),
        "delete" => "Del".to_string(),
        "escape" => "Esc".to_string(),
        "tab" => "Tab".to_string(),
        "space" => "Space".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "home" => "Home".to_string(),
        "end" => "End".to_string(),
        "minus" => "-".to_string(),
        other => other.to_uppercase(),
    }
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
            warnings.push(crate::i18n::runtime::rt!(UnknownCommand, config_key).to_string());
            continue;
        };
        let keystroke = keystroke.trim();
        if keystroke.is_empty()
            || keystroke
                .split_whitespace()
                .any(|stroke| gpui::Keystroke::parse(stroke).is_err())
        {
            warnings.push(
                crate::i18n::runtime::rt!(InvalidShortcut, keystroke, config_key).to_string(),
            );
            continue;
        }
        if let Some(existing) = used_keystrokes.insert(keystroke, command) {
            warnings.push(
                crate::i18n::runtime::rt!(
                    DuplicateShortcut,
                    keystroke,
                    existing.config_key(),
                    config_key
                )
                .to_string(),
            );
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

fn menu_commands(
    language: UiLanguage,
    commands: impl IntoIterator<Item = CommandId>,
) -> Vec<gpui::MenuItem> {
    use super::commands::ExtraCommand as E;
    use gpui::{MenuItem, OsAction};
    commands
        .into_iter()
        .filter(|id| id.is_available())
        .map(|id| {
            let spec = COMMAND_SPECS
                .iter()
                .find(|spec| spec.id == id)
                .expect("menu commands must be registered");
            let os_action = match id {
                CommandId::Undo => Some(OsAction::Undo),
                CommandId::Redo => Some(OsAction::Redo),
                CommandId::SelectAll => Some(OsAction::SelectAll),
                CommandId::Extra(E::Cut) => Some(OsAction::Cut),
                CommandId::Extra(E::Copy) => Some(OsAction::Copy),
                CommandId::Extra(E::Paste) => Some(OsAction::Paste),
                _ => None,
            };
            let name = if let CommandId::Extra(command) = id
                && let Some((format, _)) = command.export()
            {
                format.label().to_string()
            } else {
                spec.localized_title(language)
            };
            MenuItem::Action {
                name: name.into(),
                action: id.action(),
                os_action,
            }
        })
        .collect()
}

fn menu_sections(sections: impl IntoIterator<Item = Vec<gpui::MenuItem>>) -> Vec<gpui::MenuItem> {
    let mut items = Vec::new();
    for section in sections.into_iter().filter(|section| !section.is_empty()) {
        if !items.is_empty() {
            items.push(gpui::MenuItem::separator());
        }
        items.extend(section);
    }
    items
}

fn menu_submenu(
    name: &str,
    sections: impl IntoIterator<Item = Vec<gpui::MenuItem>>,
) -> Vec<gpui::MenuItem> {
    let items = menu_sections(sections);
    if items.is_empty() {
        return Vec::new();
    }
    vec![gpui::MenuItem::submenu(gpui::Menu {
        name: name.to_string().into(),
        items,
    })]
}

/// Shared hierarchy for native menus and the Linux application menu.
/// Frequent actions stay at the top level; specialized operations are one submenu deep.
pub(crate) fn app_menus(language: UiLanguage) -> Vec<gpui::Menu> {
    use self::menu_submenu as sub;
    use super::commands::ExtraCommand as E;
    use CommandId as C;
    use gpui::Menu;
    let t = |key| crate::i18n::translate(language, key);
    let label = |key| crate::i18n::menu_command_label(language, key);
    let c = |ids: &[C]| menu_commands(language, ids.iter().copied());
    let e = |ids: &[E]| menu_commands(language, ids.iter().copied().map(C::Extra));

    vec![
        Menu {
            name: crate::i18n::application_display_name(language).into(),
            items: menu_sections([
                c(&[C::OpenSettings]),
                sub(
                    label("menu_settings"),
                    [
                        e(&[
                            E::ThemeSettings,
                            E::FontSettings,
                            E::FormatSettings,
                            E::TerminalSettings,
                            E::AttachmentSettings,
                        ]),
                        e(&[E::OpenConfig]),
                    ],
                ),
                e(&[E::Quit]),
            ]),
        },
        Menu {
            name: t(TextKey::MenuFile).into(),
            items: menu_sections([
                c(&[C::NewDocument, C::NewTab, C::OpenFile, C::QuickOpen]),
                sub(
                    label("menu_workspaces"),
                    [e(&[
                        E::OpenWorkspace,
                        E::ChooseWorkspace,
                        E::CloseWorkspace,
                    ])],
                ),
                c(&[C::Save, C::SaveAs]),
                e(&[E::AutoSave]),
                [
                    sub(
                        t(TextKey::ExportToFile),
                        [e(&[
                            E::ExportMarkdown,
                            E::ExportHtml,
                            E::ExportPdf,
                            E::ExportPng,
                        ])],
                    ),
                    sub(
                        t(TextKey::CopyToClipboard),
                        [e(&[E::CopyMarkdown, E::CopyHtml, E::CopyPdf, E::CopyPng])],
                    ),
                    sub(
                        label("menu_file_location"),
                        [
                            e(&[E::LocateFile, E::RevealFile]),
                            e(&[E::CopyPath, E::CopyRelativePath]),
                        ],
                    ),
                ]
                .into_iter()
                .flatten()
                .collect(),
                c(&[C::CloseTab]),
            ]),
        },
        Menu {
            name: t(TextKey::MenuEdit).into(),
            items: menu_sections([
                c(&[C::Undo, C::Redo]),
                e(&[E::Cut, E::Copy, E::Paste]),
                sub(
                    label("menu_clipboard"),
                    [e(&[E::PastePlain, E::CopyPlain, E::Yank])],
                ),
                [
                    sub(
                        label("menu_line_editing"),
                        [
                            e(&[E::InsertAbove, E::InsertBelow, E::OpenLine]),
                            c(&[
                                C::DuplicateLine,
                                C::MoveLineUp,
                                C::MoveLineDown,
                                C::JoinLines,
                            ]),
                            e(&[E::CopyLineUp, E::CopyLineDown]),
                        ],
                    ),
                    sub(
                        label("menu_text_editing"),
                        [
                            e(&[E::Indent, E::Outdent]),
                            c(&[C::ToggleComment]),
                            e(&[E::Transpose]),
                        ],
                    ),
                    sub(
                        t(TextKey::Delete),
                        [
                            c(&[C::DeleteLine]),
                            e(&[
                                E::DeleteWordLeft,
                                E::DeleteWordRight,
                                E::DeleteToStart,
                                E::DeleteToEnd,
                                E::KillLine,
                            ]),
                        ],
                    ),
                    sub(
                        label("Selection"),
                        [
                            c(&[C::SelectAll]),
                            e(&[
                                E::SelectLine,
                                E::SelectWordLeft,
                                E::SelectWordRight,
                                E::SelectLineStart,
                                E::SelectLineEnd,
                                E::SelectDocumentStart,
                                E::SelectDocumentEnd,
                            ]),
                            c(&[
                                C::SelectNextOccurrence,
                                C::SelectAllOccurrences,
                                C::AddCursorAbove,
                                C::AddCursorBelow,
                            ]),
                        ],
                    ),
                ]
                .into_iter()
                .flatten()
                .collect(),
                [
                    sub(
                        crate::i18n::command_action_label(language, "find_document"),
                        [
                            c(&[C::Find]),
                            e(&[
                                E::FindNext,
                                E::FindPrevious,
                                E::Replace,
                                E::ReplaceNext,
                                E::ReplaceAll,
                            ]),
                            e(&[E::SearchCase, E::SearchWord, E::SearchRegex]),
                            e(&[E::SwitchSearchInput, E::CloseSearch]),
                        ],
                    ),
                    sub(
                        crate::i18n::command_action_label(language, "find_files"),
                        [
                            c(&[C::FindInFiles]),
                            e(&[
                                E::WorkspaceFilters,
                                E::WorkspaceCase,
                                E::WorkspaceWord,
                                E::WorkspaceRegex,
                                E::CloseWorkspaceSearch,
                            ]),
                        ],
                    ),
                ]
                .into_iter()
                .flatten()
                .collect(),
                e(&[E::CharacterPalette]),
            ]),
        },
        Menu {
            name: t(TextKey::MenuFormat).into(),
            items: menu_sections([
                c(&[
                    C::Bold,
                    C::Italic,
                    C::Underline,
                    C::Strikethrough,
                    C::InlineCode,
                ]),
                e(&[E::Highlight]),
                [
                    sub(
                        label("menu_text_style"),
                        [e(&[E::Superscript, E::Subscript])],
                    ),
                    sub(
                        t(TextKey::Heading),
                        [
                            e(&[
                                E::Heading1,
                                E::Heading2,
                                E::Heading3,
                                E::Heading4,
                                E::Heading5,
                                E::Heading6,
                            ]),
                            c(&[C::Paragraph]),
                        ],
                    ),
                    sub(
                        label("menu_lists"),
                        [c(&[C::UnorderedList, C::OrderedList, C::Todo])],
                    ),
                    sub(
                        label("menu_insert"),
                        [
                            c(&[
                                C::Link,
                                C::Image,
                                C::Table,
                                C::CodeBlock,
                                C::Quote,
                                C::HorizontalRule,
                            ]),
                            e(&[E::WikiLink, E::Footnote, E::DateTime, E::Callout]),
                        ],
                    ),
                    sub(
                        label("Fold"),
                        [
                            c(&[C::Fold, C::Unfold]),
                            e(&[E::ToggleFold]),
                            c(&[C::FoldAll, C::UnfoldAll]),
                        ],
                    ),
                ]
                .into_iter()
                .flatten()
                .collect(),
                e(&[E::FormatMarkdown]),
            ]),
        },
        Menu {
            name: t(TextKey::MenuView).into(),
            items: menu_sections([
                e(&[E::Palette]),
                [
                    sub(
                        label("menu_document_mode"),
                        [
                            e(&[E::EditMode, E::ReadingMode, E::SourceMode]),
                            e(&[E::CycleMode, E::PreviousMode]),
                        ],
                    ),
                    sub(
                        label("menu_panels"),
                        [
                            c(&[
                                C::ToggleSidebar,
                                C::ToggleOutline,
                                C::ToggleInfo,
                                C::ToggleTerminal,
                            ]),
                            e(&[E::FileList, E::OutlinePanel]),
                        ],
                    ),
                    sub(
                        crate::i18n::settings_label(language, "minimap"),
                        [
                            e(&[E::Minimap]),
                            e(&[E::MinimapLeft, E::MinimapRight, E::MinimapOff]),
                        ],
                    ),
                    sub(
                        crate::i18n::sidebar_action_label(language, "files"),
                        [
                            e(&[E::FilterFiles, E::FileSort]),
                            e(&[
                                E::StarredSection,
                                E::CollapseDirectories,
                                E::ExpandDirectories,
                            ]),
                        ],
                    ),
                ]
                .into_iter()
                .flatten()
                .collect(),
                [
                    sub(
                        label("menu_document_options"),
                        [
                            c(&[C::ToggleWordWrap, C::ToggleLeftAlign]),
                            e(&[E::Language, E::Indentation, E::Whitespace, E::HighlightLine]),
                        ],
                    ),
                    sub(
                        label("menu_appearance"),
                        [
                            c(&[C::ToggleTheme]),
                            e(&[E::ThemeAuto, E::ThemeLight, E::ThemeDark]),
                            e(&[E::Colorful, E::Monochrome, E::DarkSidebar, E::SimpleMode]),
                        ],
                    ),
                    sub(
                        label("menu_interface_zoom"),
                        [c(&[C::ZoomIn, C::ZoomOut, C::ZoomReset])],
                    ),
                    sub(
                        t(TextKey::Image),
                        [
                            e(&[E::ImageZoomIn, E::ImageZoomOut, E::ImageReset]),
                            e(&[E::CopyImage]),
                        ],
                    ),
                ]
                .into_iter()
                .flatten()
                .collect(),
            ]),
        },
        Menu {
            name: label("Navigation").into(),
            items: menu_sections([
                e(&[E::Back, E::Forward]),
                c(&[C::PreviousTab, C::NextTab, C::FocusNextPane]),
                sub(
                    label("menu_cursor_navigation"),
                    [
                        e(&[E::WordLeft, E::WordRight, E::LineStart, E::LineEnd]),
                        e(&[E::DocumentStart, E::DocumentEnd, E::PageDown]),
                    ],
                ),
            ]),
        },
        Menu {
            name: label("Window").into(),
            items: menu_sections([
                e(&[E::Minimize, E::Maximize, E::Fullscreen]),
                [
                    sub(
                        label("menu_tabs"),
                        [
                            c(&[C::PinTab, C::ReopenClosedTab]),
                            e(&[E::KeepTab]),
                            e(&[E::CloseOthers, E::CloseLeft, E::CloseRight, E::CloseSaved]),
                        ],
                    ),
                    sub(
                        label("menu_panes"),
                        [
                            c(&[C::SplitRight, C::SplitDown]),
                            e(&[E::SplitLeft, E::SplitUp, E::MoveTab]),
                        ],
                    ),
                    sub(
                        crate::i18n::terminal_label(language),
                        [
                            c(&[C::OpenTerminalWindow]),
                            e(&[E::Terminal, E::Copilot, E::Codex, E::Claude, E::Gemini]),
                            e(&[E::AttachFile, E::AttachSelection]),
                            e(&[E::ClearTerminal, E::CloseTerminal]),
                        ],
                    ),
                ]
                .into_iter()
                .flatten()
                .collect(),
                e(&[E::CloseWindow]),
            ]),
        },
        Menu {
            name: label("menu_help").into(),
            items: menu_sections([
                e(&[E::Help, E::Website]),
                e(&[E::CheckUpdates, E::InstallUpdate]),
                sub(
                    label("Developer"),
                    [
                        e(&[E::ThemeDebugger, E::ExportTheme]),
                        e(&[E::RuntimeProfile, E::ProfileSnapshot]),
                    ],
                ),
            ]),
        },
    ]
}

pub(super) fn contextual_app_menus(
    language: UiLanguage,
    pinned: bool,
    workspaces: &[std::path::PathBuf],
) -> Vec<gpui::Menu> {
    fn populate(
        items: &mut Vec<gpui::MenuItem>,
        language: UiLanguage,
        pinned: bool,
        workspaces: &[std::path::PathBuf],
    ) {
        let is_workspace_group = items.iter().any(|item| {
            matches!(
                item,
                gpui::MenuItem::Action { action, .. }
                    if super::commands::id_for_action(action.as_ref())
                        == Some(CommandId::Extra(super::commands::ExtraCommand::ChooseWorkspace))
            )
        });
        for item in items.iter_mut() {
            match item {
                gpui::MenuItem::Action { name, action, .. }
                    if super::commands::id_for_action(action.as_ref())
                        == Some(CommandId::PinTab) =>
                {
                    *name = crate::i18n::translate(
                        language,
                        if pinned {
                            TextKey::UnpinTab
                        } else {
                            TextKey::PinTab
                        },
                    )
                    .into();
                }
                gpui::MenuItem::Submenu(menu) => {
                    populate(&mut menu.items, language, pinned, workspaces)
                }
                _ => {}
            }
        }
        if is_workspace_group && !workspaces.is_empty() {
            items.push(gpui::MenuItem::separator());
            items.extend(workspaces.iter().map(|root| {
                gpui::MenuItem::action(
                    root.file_name()
                        .unwrap_or(root.as_os_str())
                        .to_string_lossy()
                        .into_owned(),
                    SwitchWorkspace {
                        path: root.to_string_lossy().into_owned(),
                    },
                )
            }));
        }
    }

    let mut menus = app_menus(language);
    for menu in &mut menus {
        populate(&mut menu.items, language, pinned, workspaces);
    }
    menus
}

pub(crate) fn dock_menu(language: UiLanguage) -> Vec<gpui::MenuItem> {
    if !CommandId::OpenTerminalWindow.is_available() {
        return Vec::new();
    }

    vec![gpui::MenuItem::action(
        crate::i18n::open_terminal_window_label(language),
        crate::actions::OpenTerminalWindow,
    )]
}

/// Install the native menu bar and Dock menu using the persisted UI language.
pub(crate) fn set_application_menus(cx: &mut gpui::App) {
    let language = super::AppSettings::load().ui_language();
    apply_application_menus(cx, app_menus(language), language);
    cx.set_dock_menu(dock_menu(language));
    super::commands::install_menu_state(cx);
}

pub(super) fn apply_application_menus(
    cx: &mut gpui::App,
    menus: Vec<gpui::Menu>,
    language: UiLanguage,
) {
    cx.set_global(language);
    crate::i18n::runtime::set_language(language);
    cx.set_menus(menus);
    cx.set_window_menu_name(crate::i18n::menu_command_label(language, "Window"));
}

pub(crate) static COMMAND_SPECS: std::sync::LazyLock<Vec<CommandSpec>> =
    std::sync::LazyLock::new(|| {
        BASE_COMMAND_SPECS
            .iter()
            .copied()
            .chain(
                super::commands::ExtraCommand::ALL
                    .iter()
                    .map(|command| CommandSpec {
                        id: CommandId::Extra(*command),
                        title: TextKey::Reset,
                        category: command.category(),
                        shortcut: None,
                        alternate_shortcuts: &[],
                        markdown_only: command.markdown_only(),
                        title_override: None,
                        label_key: None,
                    }),
            )
            .collect()
    });

const BASE_COMMAND_SPECS: &[CommandSpec] = &[
    command_with_title(
        CommandId::NewDocument,
        TextKey::NewFile,
        "File",
        Some("secondary-n"),
        false,
        new_document_title,
    ),
    command_with_title(
        CommandId::NewTab,
        TextKey::NewFile,
        "File",
        Some("secondary-t"),
        false,
        new_tab_title,
    ),
    command(
        CommandId::OpenFile,
        TextKey::OpenFile,
        "File",
        "secondary-o",
        false,
    ),
    command_with_title(
        CommandId::QuickOpen,
        TextKey::QuickOpenPlaceholder,
        "File",
        Some("secondary-p"),
        false,
        quick_open_title,
    ),
    command(CommandId::Save, TextKey::Save, "File", "secondary-s", false),
    command_without_shortcut(CommandId::SaveAs, TextKey::SaveAs, "File", false),
    command(
        CommandId::OpenSettings,
        TextKey::SettingsTitle,
        "Application",
        "secondary-,",
        false,
    ),
    command(
        CommandId::CloseTab,
        TextKey::Close,
        "Tab",
        "secondary-w",
        false,
    ),
    command_with_title(
        CommandId::Find,
        TextKey::SearchPlaceholder,
        "Search",
        Some("secondary-f"),
        false,
        find_document_title,
    ),
    command_with_title(
        CommandId::FindInFiles,
        TextKey::WorkspaceSearchPlaceholder,
        "Search",
        Some("secondary-shift-f"),
        false,
        find_files_title,
    ),
    command(
        CommandId::ToggleSidebar,
        TextKey::ToolbarSidebarTooltip,
        "View",
        "secondary-shift-s",
        false,
    ),
    command(
        CommandId::ToggleOutline,
        TextKey::ToolbarOutlineTooltip,
        "View",
        crate::platform::keys::TOGGLE_OUTLINE,
        false,
    ),
    command_with_title(
        CommandId::ToggleTerminal,
        TextKey::ClearTerminal,
        "View",
        Some(crate::platform::keys::TOGGLE_TERMINAL_PANEL),
        false,
        crate::i18n::terminal_label,
    ),
    command(
        CommandId::ToggleInfo,
        TextKey::FileInfo,
        "View",
        "secondary-alt-i",
        false,
    ),
    command_without_shortcut(
        CommandId::ToggleTheme,
        TextKey::ToolbarThemeTooltip,
        "View",
        false,
    ),
    command(
        CommandId::Bold,
        TextKey::Bold,
        "Format",
        "secondary-b",
        true,
    ),
    command(
        CommandId::Italic,
        TextKey::Italic,
        "Format",
        "secondary-i",
        true,
    ),
    command_with_title(
        CommandId::Underline,
        TextKey::Italic,
        "Format",
        Some("secondary-u"),
        true,
        underline_title,
    ),
    command(
        CommandId::Strikethrough,
        TextKey::Strikethrough,
        "Format",
        "secondary-shift-x",
        true,
    ),
    command(
        CommandId::InlineCode,
        TextKey::InlineCode,
        "Format",
        "secondary-e",
        true,
    ),
    command(
        CommandId::Link,
        TextKey::Link,
        "Format",
        "secondary-k",
        true,
    ),
    command_without_shortcut(CommandId::Image, TextKey::Image, "Format", true),
    command(
        CommandId::Quote,
        TextKey::Quote,
        "Format",
        "secondary-alt-q",
        true,
    ),
    command(
        CommandId::Todo,
        TextKey::Todo,
        "Format",
        "secondary-alt-enter",
        true,
    ),
    command(
        CommandId::UnorderedList,
        TextKey::UnorderedList,
        "Format",
        "secondary-alt-u",
        true,
    ),
    command(
        CommandId::OrderedList,
        TextKey::OrderedList,
        "Format",
        "secondary-alt-o",
        true,
    ),
    command(
        CommandId::Table,
        TextKey::Table,
        "Format",
        "secondary-alt-t",
        true,
    ),
    command(
        CommandId::CodeBlock,
        TextKey::CodeBlock,
        "Format",
        "secondary-alt-c",
        true,
    ),
    command(
        CommandId::Paragraph,
        TextKey::Paragraph,
        "Format",
        "secondary-0",
        true,
    ),
    command_with_title(
        CommandId::Fold,
        TextKey::CollapseAllDirectories,
        "Fold",
        Some("secondary-alt-["),
        false,
        fold_section_title,
    ),
    command_with_title(
        CommandId::Unfold,
        TextKey::ExpandAllDirectories,
        "Fold",
        Some("secondary-alt-]"),
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
        Some("secondary-="),
        false,
        zoom_in_title,
    ),
    command_with_title(
        CommandId::ZoomOut,
        TextKey::Reset,
        "View",
        Some("secondary-minus"),
        false,
        zoom_out_title,
    ),
    command_with_title(
        CommandId::ZoomReset,
        TextKey::Reset,
        "View",
        Some("secondary-alt-0"),
        false,
        zoom_reset_title,
    ),
    command(CommandId::Undo, TextKey::Undo, "Edit", "secondary-z", false),
    command(
        CommandId::Redo,
        TextKey::Redo,
        "Edit",
        "secondary-shift-z",
        false,
    ),
    command(
        CommandId::SelectAll,
        TextKey::SelectAll,
        "Edit",
        "secondary-a",
        false,
    ),
    command_labelled(CommandId::DuplicateLine, "Edit", "duplicate_line"),
    command_labelled(CommandId::DeleteLine, "Edit", "delete_line"),
    command_labelled(CommandId::MoveLineUp, "Edit", "move_line_up"),
    command_labelled(CommandId::MoveLineDown, "Edit", "move_line_down"),
    command_labelled(CommandId::JoinLines, "Edit", "join_lines"),
    command_labelled(CommandId::ToggleComment, "Edit", "toggle_comment"),
    command_labelled(
        CommandId::SelectNextOccurrence,
        "Edit",
        "select_next_occurrence",
    ),
    command_labelled(
        CommandId::SelectAllOccurrences,
        "Edit",
        "select_all_occurrences",
    ),
    command_labelled(CommandId::AddCursorAbove, "Edit", "add_cursor_above"),
    command_labelled(CommandId::AddCursorBelow, "Edit", "add_cursor_below"),
    command_labelled(CommandId::ToggleWordWrap, "View", "toggle_word_wrap"),
    command_labelled(CommandId::ToggleLeftAlign, "View", "toggle_left_align"),
    command_labelled(CommandId::SplitRight, "View", "split_right"),
    command_labelled(CommandId::SplitDown, "View", "split_down"),
    command_labelled(CommandId::FocusNextPane, "View", "focus_next_pane"),
    command_labelled(CommandId::ReopenClosedTab, "Tab", "reopen_closed_tab"),
    command_labelled(CommandId::NextTab, "Tab", "next_tab"),
    command_labelled(CommandId::PreviousTab, "Tab", "previous_tab"),
    command_without_shortcut(CommandId::PinTab, TextKey::PinTab, "Tab", false),
    command_labelled(
        CommandId::OpenTerminalWindow,
        "Application",
        "open_terminal_window",
    ),
    command_without_shortcut(
        CommandId::HorizontalRule,
        TextKey::HorizontalRule,
        "Format",
        true,
    ),
];

/// Command whose visible title comes from the `COMMAND_LABELS` table rather
/// than a `TextKey`.
const fn command_labelled(
    id: CommandId,
    category: &'static str,
    label_key: &'static str,
) -> CommandSpec {
    CommandSpec {
        id,
        title: TextKey::Reset,
        category,
        shortcut: None,
        alternate_shortcuts: &[],
        markdown_only: false,
        title_override: None,
        label_key: Some(label_key),
    }
}

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
        label_key: None,
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
        label_key: None,
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
        label_key: None,
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
        for spec in COMMAND_SPECS.iter() {
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
    fn git_history_is_not_a_registered_command() {
        assert!(
            COMMAND_SPECS
                .iter()
                .all(|spec| spec.id.config_key() != "toggle_git_history")
        );
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

        assert_eq!(sidebar.shortcut, Some("secondary-shift-s"));
        assert_eq!(save_as.shortcut, None);
    }

    #[test]
    fn command_titles_are_unique_in_every_language() {
        let mut duplicates = Vec::new();
        for &language in UiLanguage::supported() {
            let mut titles = HashMap::new();
            for spec in COMMAND_SPECS.iter() {
                let title = spec.localized_title(language);
                assert!(
                    !spec.localized_category(language).is_empty(),
                    "untranslated command category: {}",
                    spec.category
                );
                assert!(
                    !title.trim().is_empty(),
                    "empty command title for {language:?}: {:?}",
                    spec.id
                );
                if let Some(previous) = titles.insert(title.clone(), spec.id) {
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
        for spec in COMMAND_SPECS.iter() {
            let key = spec.id.config_key();
            assert!(
                seen.insert(key, spec.id).is_none(),
                "duplicate config key {key:?}"
            );
            assert_eq!(super::CommandId::from_config_key(key), Some(spec.id));
        }
        assert_eq!(super::CommandId::from_config_key("unknown"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn keystroke_display_conversion() {
        assert_eq!(
            super::display_shortcut_from_keystroke("secondary-shift-s"),
            "⌘⇧S"
        );
        assert_eq!(super::display_shortcut_from_keystroke("secondary-,"), "⌘,");
        assert_eq!(
            super::display_shortcut_from_keystroke("secondary-minus"),
            "⌘-"
        );
        assert_eq!(
            super::display_shortcut_from_keystroke("secondary-alt-enter"),
            "⌘⌥↩"
        );
        assert_eq!(
            super::display_shortcut_from_keystroke("ctrl-shift-g"),
            "⌃⇧G"
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn keystroke_display_conversion() {
        assert_eq!(
            super::display_shortcut_from_keystroke("secondary-shift-s"),
            "Ctrl+Shift+S"
        );
        assert_eq!(
            super::display_shortcut_from_keystroke("secondary-,"),
            "Ctrl+,"
        );
        assert_eq!(
            super::display_shortcut_from_keystroke("secondary-alt-enter"),
            "Ctrl+Alt+Enter"
        );
        // `secondary` already means Control here, so it must not double up.
        assert_eq!(
            super::display_shortcut_from_keystroke("secondary-ctrl-t"),
            "Ctrl+T"
        );
        assert_eq!(super::display_shortcut_from_keystroke("home"), "Home");
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

    /// Convert a COMMAND_SPECS keystroke to the long form used by the help
    /// documents ("Command / Ctrl + Shift + X"), which name both platforms.
    #[cfg(target_os = "macos")]
    fn docs_shortcut(keystroke: &str) -> String {
        super::display_shortcut_from_keystroke(keystroke)
            .chars()
            .map(|ch| match ch {
                '⌃' => "Control".to_string(),
                '⌥' => "Option / Alt".to_string(),
                '⇧' => "Shift".to_string(),
                '⌘' => "Command / Ctrl".to_string(),
                '↩' => "Enter".to_string(),
                other => other.to_string(),
            })
            .collect::<Vec<_>>()
            .join(" + ")
    }

    #[cfg(target_os = "macos")]
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
            for spec in COMMAND_SPECS.iter() {
                if !matches!(
                    spec.id,
                    CommandId::Bold
                        | CommandId::Italic
                        | CommandId::Underline
                        | CommandId::Strikethrough
                        | CommandId::InlineCode
                ) {
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
            crate::ui::typography::keycap(
                crate::style::TextStyle::new(style.font_size_px, style.text_color),
                width,
            )
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
    fn native_menu_groups_are_compact_localized_and_only_one_submenu_deep() {
        fn inspect(items: &[gpui::MenuItem], depth: usize) {
            assert!(!items.is_empty(), "empty menu group");
            assert!(
                items.len() <= if depth == 0 { 20 } else { 16 },
                "overcrowded menu group: {} rows",
                items.len()
            );
            assert!(!matches!(items.first(), Some(gpui::MenuItem::Separator)));
            assert!(!matches!(items.last(), Some(gpui::MenuItem::Separator)));
            assert!(!items.windows(2).any(|pair| matches!(
                pair,
                [gpui::MenuItem::Separator, gpui::MenuItem::Separator]
            )));
            let mut labels = std::collections::HashSet::new();
            for item in items {
                let name = match item {
                    gpui::MenuItem::Action { name, .. } => name,
                    gpui::MenuItem::Submenu(menu) => {
                        assert_eq!(depth, 0, "avoid third-level menus: {}", menu.name);
                        inspect(&menu.items, depth + 1);
                        &menu.name
                    }
                    _ => continue,
                };
                assert!(!name.trim().is_empty(), "untranslated menu label");
                assert!(
                    labels.insert(name.to_string()),
                    "duplicate sibling label: {name}"
                );
            }
        }
        for &language in UiLanguage::supported() {
            let menus = app_menus(language);
            assert_eq!(menus.len(), 8);
            for menu in menus {
                inspect(&menu.items, 0);
            }
        }
    }

    #[test]
    fn native_menu_workflow_groups_preserve_frequent_actions_and_export_formats() {
        use super::CommandId as C;
        use crate::editor::commands::{ExtraCommand as E, id_for_action};
        let menus = app_menus(UiLanguage::English);
        assert_eq!(
            menus
                .iter()
                .map(|menu| menu.name.as_ref())
                .collect::<Vec<_>>(),
            [
                "Moeka",
                "File",
                "Edit",
                "Format",
                "View",
                "Navigation",
                "Window",
                "Help"
            ]
        );
        let direct = |index: usize| {
            menus[index]
                .items
                .iter()
                .filter_map(|item| match item {
                    gpui::MenuItem::Action { action, .. } => id_for_action(action.as_ref()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert!(direct(1).contains(&C::Save));
        assert!(direct(1).contains(&C::OpenFile));
        assert!(direct(2).contains(&C::Undo));
        assert!(direct(2).contains(&C::Extra(E::Paste)));
        assert!(!direct(2).contains(&C::DuplicateLine));
        assert!(!direct(4).contains(&C::Extra(E::MinimapLeft)));
        let exports = menus[1].items.iter().filter_map(|item| match item {
            gpui::MenuItem::Submenu(menu) if menu.items.iter().any(|item| matches!(
                item, gpui::MenuItem::Action { action, .. }
                    if matches!(id_for_action(action.as_ref()), Some(C::Extra(command)) if command.export().is_some())
            )) => Some(menu),
            _ => None,
        }).collect::<Vec<_>>();
        assert_eq!(exports.len(), 2);
        for menu in exports {
            assert_eq!(
                menu.items
                    .iter()
                    .map(|item| match item {
                        gpui::MenuItem::Action { name, .. } => name.as_ref(),
                        _ => panic!("export submenu should contain four formats"),
                    })
                    .collect::<Vec<_>>(),
                ["Markdown", "HTML", "PDF", "PNG"]
            );
        }
    }

    #[test]
    fn native_nested_tab_title_and_workspace_targets_follow_live_context() {
        use super::CommandId;
        use crate::editor::commands::id_for_action;
        fn inspect(items: &[gpui::MenuItem], pinned: bool, workspace_count: &mut usize) {
            for item in items {
                match item {
                    gpui::MenuItem::Action { name, action, .. }
                        if id_for_action(action.as_ref()) == Some(CommandId::PinTab) =>
                    {
                        assert_eq!(
                            name.as_ref(),
                            crate::i18n::translate(
                                UiLanguage::English,
                                if pinned {
                                    crate::i18n::TextKey::UnpinTab
                                } else {
                                    crate::i18n::TextKey::PinTab
                                },
                            )
                        );
                    }
                    gpui::MenuItem::Action { action, .. }
                        if action.as_any().is::<crate::actions::SwitchWorkspace>() =>
                    {
                        *workspace_count += 1;
                    }
                    gpui::MenuItem::Submenu(menu) => {
                        assert!(
                            !menu
                                .items
                                .iter()
                                .any(|item| matches!(item, gpui::MenuItem::Submenu(_)))
                        );
                        inspect(&menu.items, pinned, workspace_count);
                    }
                    _ => {}
                }
            }
        }
        for pinned in [false, true] {
            let workspaces = [
                std::path::PathBuf::from("first"),
                std::path::PathBuf::from("second"),
            ];
            let menus = super::contextual_app_menus(UiLanguage::English, pinned, &workspaces);
            let mut count = 0;
            for menu in menus {
                inspect(&menu.items, pinned, &mut count);
            }
            assert_eq!(count, 2);
        }
    }

    #[test]
    fn edit_menu_routes_paste_to_native_text_fields() {
        let menus = app_menus(UiLanguage::English);
        let edit = menus
            .iter()
            .find(|menu| menu.name.as_ref() == "Edit")
            .unwrap();
        assert!(edit.items.iter().any(|item| matches!(
            item,
            gpui::MenuItem::Action {
                name,
                os_action: Some(gpui::OsAction::Paste),
                ..
            } if name.as_ref() == "Paste"
        )));
    }

    #[test]
    fn native_menus_register_window_list_and_localized_terminal_action() {
        let menus = app_menus(UiLanguage::ChineseSimplified);
        let window_name = crate::i18n::menu_command_label(UiLanguage::ChineseSimplified, "Window");
        assert!(menus.iter().any(|menu| menu.name.as_ref() == window_name));
        assert_eq!(window_name, "窗口");
        assert!(!menus.iter().any(|menu| menu.name.as_ref() == "Window"));

        let dock_items = dock_menu(UiLanguage::ChineseSimplified);
        if crate::platform::capabilities::CHILD_PROCESSES {
            assert!(matches!(
                dock_items.as_slice(),
                [gpui::MenuItem::Action { name, .. }] if name.as_ref() == "打开终端"
            ));
        } else {
            assert!(dock_items.is_empty());
        }
    }
}
