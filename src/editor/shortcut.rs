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
            Self::ToggleChat => "Show or hide Chat",
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
    command_without_shortcut(CommandId::SaveAs, TextKey::SaveFileDialog, "File", false),
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
        crate::i18n::chat_label,
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
