//! Shared command coverage, target ownership and business-state validation.
#[cfg(target_os = "linux")]
use gpui::AppContext;
use gpui::{Action, App, Context, WeakFocusHandle, Window};

use super::{
    shortcut::{COMMAND_SPECS, CommandId},
    *,
};
use crate::{
    actions::*,
    i18n::{self, TextKey, UiLanguage},
};

macro_rules! commands {
    ($($name:ident => ($key:literal, $category:literal, $description:literal)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub(crate) enum ExtraCommand { $($name),+ }
        impl ExtraCommand {
            pub(crate) fn native_action(self) -> Box<dyn Action> {
                use ExtraCommand as E;
                match self {
                    E::Cut => Box::new(Cut), E::Copy => Box::new(Copy), E::Paste => Box::new(Paste),
                    E::PastePlain => Box::new(PastePlainText),
                    E::SelectLine => Box::new(SelectLine), E::InsertAbove => Box::new(InsertLineAbove),
                    E::InsertBelow => Box::new(InsertLineBelow), E::CopyLineUp => Box::new(CopyLineUp),
                    E::CopyLineDown => Box::new(CopyLineDown), E::Indent => Box::new(Tab),
                    E::Outdent => Box::new(BackTab), E::Transpose => Box::new(TransposeChars),
                    E::DeleteWordLeft => Box::new(DeleteWordLeft), E::DeleteWordRight => Box::new(DeleteWordRight),
                    E::DeleteToStart => Box::new(DeleteToLineStart), E::DeleteToEnd => Box::new(DeleteToLineEnd),
                    E::KillLine => Box::new(KillToLineEnd), E::Yank => Box::new(Yank), E::OpenLine => Box::new(OpenLine),
                    E::WordLeft => Box::new(MoveWordLeft), E::WordRight => Box::new(MoveWordRight),
                    E::LineStart => Box::new(Home), E::LineEnd => Box::new(End),
                    E::DocumentStart => Box::new(MoveToDocumentStart), E::DocumentEnd => Box::new(MoveToDocumentEnd),
                    E::PageDown => Box::new(MovePageDown), E::SelectWordLeft => Box::new(SelectWordLeft),
                    E::SelectWordRight => Box::new(SelectWordRight), E::SelectLineStart => Box::new(SelectHome),
                    E::SelectLineEnd => Box::new(SelectEnd), E::SelectDocumentStart => Box::new(SelectToDocumentStart),
                    E::SelectDocumentEnd => Box::new(SelectToDocumentEnd), E::CharacterPalette => Box::new(ShowCharacterPalette),
                    E::ToggleFold => Box::new(ToggleFold),
                    E::Heading1 => Box::new(InsertHeading1), E::Heading2 => Box::new(InsertHeading2),
                    E::Heading3 => Box::new(InsertHeading3), E::Heading4 => Box::new(InsertHeading4),
                    E::Heading5 => Box::new(InsertHeading5), E::Heading6 => Box::new(InsertHeading6),
                    E::FindNext => Box::new(FindNext), E::FindPrevious => Box::new(FindPrevious),
                    E::Replace => Box::new(ToggleReplace), E::ReplaceNext => Box::new(ReplaceNext),
                    E::ReplaceAll => Box::new(ReplaceAll), E::CloseSearch => Box::new(DismissSearch),
                    E::SearchCase => Box::new(ToggleSearchCaseSensitive), E::SearchWord => Box::new(ToggleSearchWholeWord),
                    E::SearchRegex => Box::new(ToggleSearchRegex),
                    E::SplitLeft => Box::new(SplitPaneLeft), E::SplitUp => Box::new(SplitPaneUp),
                    E::Palette => Box::new(ToggleCommandPalette), E::Quit => Box::new(Quit),
                    E::Fullscreen => Box::new(ToggleFullscreen), E::ThemeDebugger => Box::new(ToggleThemeDebugger),
                    E::RuntimeProfile => Box::new(ToggleRuntimeProfile),
                    E::ProfileSnapshot => Box::new(WriteRuntimeProfileSnapshot),
                    _ => Box::new(RunCommand { id: self.key().into() }),
                }
            }

            pub(crate) const ALL: &'static [Self] = &[$(Self::$name),+];
            pub(crate) fn key(self) -> &'static str { match self { $(Self::$name => $key),+ } }
            pub(crate) fn category(self) -> &'static str { match self { $(Self::$name => $category),+ } }
            pub(crate) fn description(self) -> &'static str { match self { $(Self::$name => $description),+ } }
        }
    };
}

commands! {
    ExportMarkdown => ("export_markdown", "Export", "Export Markdown to a file"),
    ExportHtml => ("export_html", "Export", "Export HTML to a file"),
    ExportPdf => ("export_pdf", "Export", "Export PDF to a file"),
    ExportPng => ("export_png", "Export", "Export PNG to a file"),
    CopyMarkdown => ("copy_markdown", "Export", "Copy document as Markdown"),
    CopyHtml => ("copy_html", "Export", "Copy document as HTML"),
    CopyPdf => ("copy_pdf", "Export", "Copy document as PDF"),
    CopyPng => ("copy_png", "Export", "Copy document as PNG"),
    OpenWorkspace => ("open_workspace", "File", "Open a workspace folder"),
    ChooseWorkspace => ("switch_workspace", "File", "Switch workspace"),
    CloseWorkspace => ("close_workspace", "File", "Close the current workspace"),
    RevealFile => ("reveal_file", "File", "Reveal current file in the file manager"),
    LocateFile => ("locate_file", "File", "Reveal current file in the file tree"),
    CopyPath => ("copy_path", "File", "Copy current file path"),
    CopyRelativePath => ("copy_relative_path", "File", "Copy current relative path"),
    Cut => ("cut", "Edit", "Cut selected text"),
    Copy => ("copy", "Edit", "Copy selected text"),
    Paste => ("paste", "Edit", "Paste from clipboard"),
    PastePlain => ("paste_plain_text", "Edit", "Paste plain text"),
    CopyPlain => ("copy_plain_text", "Edit", "Copy text without Markdown syntax"),
    SelectLine => ("select_line", "Selection", "Select the current line"),
    InsertAbove => ("insert_line_above", "Edit", "Insert a line above"),
    InsertBelow => ("insert_line_below", "Edit", "Insert a line below"),
    CopyLineUp => ("copy_line_up", "Edit", "Copy line up"),
    CopyLineDown => ("copy_line_down", "Edit", "Copy line down"),
    Indent => ("indent", "Edit", "Indent the current selection"),
    Outdent => ("outdent", "Edit", "Outdent the current selection"),
    Transpose => ("transpose_chars", "Edit", "Transpose characters"),
    DeleteWordLeft => ("delete_word_left", "Edit", "Delete previous word"),
    DeleteWordRight => ("delete_word_right", "Edit", "Delete next word"),
    DeleteToStart => ("delete_to_line_start", "Edit", "Delete to line start"),
    DeleteToEnd => ("delete_to_line_end", "Edit", "Delete to line end"),
    KillLine => ("kill_to_line_end", "Edit", "Cut to line end"),
    Yank => ("yank", "Edit", "Paste from kill buffer"),
    OpenLine => ("open_line", "Edit", "Open line without moving cursor"),
    WordLeft => ("word_left", "Navigation", "Move to previous word"),
    WordRight => ("word_right", "Navigation", "Move to next word"),
    LineStart => ("line_start", "Navigation", "Move to line start"),
    LineEnd => ("line_end", "Navigation", "Move to line end"),
    DocumentStart => ("document_start", "Navigation", "Move to document start"),
    DocumentEnd => ("document_end", "Navigation", "Move to document end"),
    PageDown => ("page_down", "Navigation", "Move one page down"),
    SelectWordLeft => ("select_word_left", "Selection", "Select previous word"),
    SelectWordRight => ("select_word_right", "Selection", "Select next word"),
    SelectLineStart => ("select_line_start", "Selection", "Select to line start"),
    SelectLineEnd => ("select_line_end", "Selection", "Select to line end"),
    SelectDocumentStart => ("select_document_start", "Selection", "Select to document start"),
    SelectDocumentEnd => ("select_document_end", "Selection", "Select to document end"),
    CharacterPalette => ("character_palette", "Edit", "Open emoji and symbols"),
    ToggleFold => ("toggle_fold", "Fold", "Toggle current section folding"),
    Heading1 => ("heading_1", "Format", "Heading 1"),
    Heading2 => ("heading_2", "Format", "Heading 2"),
    Heading3 => ("heading_3", "Format", "Heading 3"),
    Heading4 => ("heading_4", "Format", "Heading 4"),
    Heading5 => ("heading_5", "Format", "Heading 5"),
    Heading6 => ("heading_6", "Format", "Heading 6"),
    Highlight => ("highlight", "Format", "Highlight text"),
    Superscript => ("superscript", "Format", "Superscript"),
    Subscript => ("subscript", "Format", "Subscript"),
    Footnote => ("footnote", "Format", "Insert a footnote"),
    WikiLink => ("wiki_link", "Format", "Insert a Wiki link"),
    DateTime => ("date_time", "Format", "Insert current date and time"),
    Callout => ("callout_tip", "Format", "Insert a tip callout"),
    FormatMarkdown => ("format_markdown", "Format", "Format Markdown selection or document"),
    FindNext => ("find_next", "Search", "Find next match"),
    FindPrevious => ("find_previous", "Search", "Find previous match"),
    Replace => ("replace", "Search", "Find and replace"),
    ReplaceNext => ("replace_next", "Search", "Replace current match"),
    ReplaceAll => ("replace_all", "Search", "Replace all matches"),
    CloseSearch => ("close_search", "Search", "Close document search"),
    SwitchSearchInput => ("switch_search_input", "Search", "Switch search and replacement input"),
    SearchCase => ("search_case", "Search", "Toggle case sensitive search"),
    SearchWord => ("search_word", "Search", "Toggle whole word search"),
    SearchRegex => ("search_regex", "Search", "Toggle regular expression search"),
    WorkspaceFilters => ("workspace_filters", "Search", "Toggle workspace include/exclude filters"),
    WorkspaceCase => ("workspace_case", "Search", "Toggle workspace case sensitive search"),
    WorkspaceWord => ("workspace_word", "Search", "Toggle workspace whole word search"),
    WorkspaceRegex => ("workspace_regex", "Search", "Toggle workspace regular expression search"),
    CloseWorkspaceSearch => ("close_workspace_search", "Search", "Close workspace search"),
    EditMode => ("edit_mode", "View", "Switch to Edit mode"),
    ReadingMode => ("reading_mode", "View", "Switch to Reading mode"),
    SourceMode => ("source_mode", "View", "Switch to Source mode"),
    CycleMode => ("cycle_document_mode", "View", "Next document mode"),
    PreviousMode => ("previous_document_mode", "View", "Previous document mode"),
    Language => ("document_language", "View", "Choose document language"),
    Indentation => ("document_indentation", "View", "Choose document indentation"),
    FileList => ("toggle_file_list", "View", "Toggle file list"),
    OutlinePanel => ("toggle_outline_panel", "View", "Toggle outline panel"),
    Minimap => ("toggle_minimap", "View", "Toggle minimap"),
    MinimapLeft => ("minimap_left", "View", "Move minimap to the left"),
    MinimapRight => ("minimap_right", "View", "Move minimap to the right"),
    MinimapOff => ("minimap_off", "View", "Hide minimap"),
    StarredSection => ("toggle_starred_section", "View", "Toggle starred section"),
    CollapseDirectories => ("collapse_directories", "View", "Collapse all directories"),
    ExpandDirectories => ("expand_directories", "View", "Expand all directories"),
    FilterFiles => ("filter_files", "View", "Focus file name filter"),
    FileSort => ("file_sort", "View", "Choose file sorting and title display"),
    CloseOthers => ("close_other_tabs", "Tab", "Close other tabs"),
    CloseLeft => ("close_left_tabs", "Tab", "Close tabs to the left"),
    CloseRight => ("close_right_tabs", "Tab", "Close tabs to the right"),
    CloseSaved => ("close_saved_tabs", "Tab", "Close saved tabs"),
    KeepTab => ("keep_tab", "Tab", "Keep preview tab open"),
    SplitLeft => ("split_left", "View", "Split pane left"),
    SplitUp => ("split_up", "View", "Split pane up"),
    MoveTab => ("move_tab_to_pane", "Tab", "Move current tab to another pane"),
    Back => ("navigate_back", "Navigation", "Navigate back"),
    Forward => ("navigate_forward", "Navigation", "Navigate forward"),
    Terminal => ("new_terminal", "Terminal", "Create a terminal tab"),
    Copilot => ("new_copilot", "Terminal", "Create a GitHub Copilot tab"),
    Codex => ("new_codex", "Terminal", "Create an OpenAI Codex tab"),
    Claude => ("new_claude", "Terminal", "Create a Claude Code tab"),
    Gemini => ("new_gemini", "Terminal", "Create a Gemini CLI tab"),
    CloseTerminal => ("close_terminal", "Terminal", "Close active terminal"),
    ClearTerminal => ("clear_terminal", "Terminal", "Clear active terminal"),
    AttachFile => ("attach_file", "Terminal", "Add current file to chat"),
    AttachSelection => ("attach_selection", "Terminal", "Add selection to chat"),
    CopyImage => ("copy_image", "Image", "Copy current image"),
    ImageZoomIn => ("image_zoom_in", "Image", "Zoom image in"),
    ImageZoomOut => ("image_zoom_out", "Image", "Zoom image out"),
    ImageReset => ("image_reset", "Image", "Fit image"),
    ThemeSettings => ("settings_theme", "Settings", "Open theme settings"),
    FontSettings => ("settings_fonts", "Settings", "Open font and language settings"),
    FormatSettings => ("settings_format", "Settings", "Open format settings"),
    TerminalSettings => ("settings_terminal", "Settings", "Open terminal settings"),
    AttachmentSettings => ("settings_attachments", "Settings", "Open attachment settings"),
    OpenConfig => ("open_config", "Settings", "Open settings configuration file"),
    AutoSave => ("toggle_auto_save", "Settings", "Toggle automatic saving"),
    Whitespace => ("toggle_whitespace", "Settings", "Toggle prose whitespace markers"),
    HighlightLine => ("toggle_highlight_line", "Settings", "Toggle current line highlight"),
    SimpleMode => ("toggle_simple_mode", "Settings", "Toggle simple mode"),
    DarkSidebar => ("toggle_dark_sidebar", "Settings", "Toggle dark sidebar"),
    ThemeAuto => ("theme_auto", "Settings", "Follow system appearance"),
    ThemeLight => ("theme_light", "Settings", "Use light appearance"),
    ThemeDark => ("theme_dark", "Settings", "Use dark appearance"),
    Colorful => ("theme_colorful", "Settings", "Use colorful Markdown"),
    Monochrome => ("theme_monochrome", "Settings", "Use monochrome Markdown"),
    Palette => ("command_palette", "Application", "Open command palette"),
    Help => ("help", "Application", "Open help manual"),
    Website => ("website", "Application", "Open official website"),
    CheckUpdates => ("check_updates", "Application", "Check for updates"),
    InstallUpdate => ("install_update", "Application", "Review prepared update"),
    Quit => ("quit", "Application", "Quit the application"),
    Minimize => ("minimize_window", "Window", "Minimize current window"),
    Maximize => ("maximize_window", "Window", "Maximize or restore current window"),
    Fullscreen => ("fullscreen", "Window", "Toggle fullscreen"),
    CloseWindow => ("close_window", "Window", "Close current window"),
    ThemeDebugger => ("theme_debugger", "Developer", "Toggle theme debugger"),
    ExportTheme => ("export_theme", "Developer", "Export debugger theme"),
    RuntimeProfile => ("runtime_profile", "Developer", "Toggle runtime profiling"),
    ProfileSnapshot => ("profile_snapshot", "Developer", "Write diagnostic snapshot"),
}

impl ExtraCommand {
    pub(crate) fn markdown_only(self) -> bool {
        self.category() == "Format" || matches!(self, Self::CopyPlain)
    }

    pub(crate) fn is_available(self) -> bool {
        match self {
            Self::Terminal
            | Self::Copilot
            | Self::Codex
            | Self::Claude
            | Self::Gemini
            | Self::CloseTerminal
            | Self::ClearTerminal
            | Self::AttachFile
            | Self::AttachSelection
            | Self::TerminalSettings => crate::platform::capabilities::CHILD_PROCESSES,
            Self::CheckUpdates | Self::InstallUpdate => crate::platform::capabilities::SELF_UPDATE,
            Self::CharacterPalette => cfg!(target_os = "macos"),
            _ => true,
        }
    }

    pub(crate) fn title(self, language: UiLanguage) -> String {
        use ExtraCommand::*;
        let t = |key| i18n::translate(language, key).to_string();
        if let Some((format, target)) = self.export() {
            return format!(
                "{} · {}",
                t(if target == export::ExportTarget::File {
                    TextKey::ExportToFile
                } else {
                    TextKey::CopyToClipboard
                }),
                format.label()
            );
        }
        match self {
            Cut => t(TextKey::Cut),
            Copy => t(TextKey::Copy),
            Paste => t(TextKey::Paste),
            CopyPlain => t(TextKey::CopyPlainText),
            OpenWorkspace => t(TextKey::OpenFolder),
            ChooseWorkspace => i18n::command_label(language, "switch_workspace").into(),
            CloseWorkspace => t(TextKey::CloseFolder),
            RevealFile => t(TextKey::RevealInFinder),
            LocateFile => t(TextKey::RevealInFileTree),
            CopyPath => t(TextKey::CopyPath),
            CopyRelativePath => t(TextKey::CopyRelativePath),
            Heading1 | Heading2 | Heading3 | Heading4 | Heading5 | Heading6 => {
                i18n::heading_level_label(language, self.heading().unwrap())
            }
            Highlight | Superscript | Subscript | Footnote | Callout => {
                i18n::format_extra_label(language, self.key()).into()
            }
            WikiLink => t(TextKey::WikiLink),
            DateTime => t(TextKey::InsertDateTime),
            FormatMarkdown => i18n::format_markdown_label(language).into(),
            Replace => t(TextKey::Replace),
            ReplaceAll => t(TextKey::ReplaceAll),
            SearchCase | SearchWord | SearchRegex | WorkspaceCase | WorkspaceWord
            | WorkspaceRegex => {
                let option = match self {
                    SearchCase | WorkspaceCase => "case",
                    SearchWord | WorkspaceWord => "word",
                    _ => "regex",
                };
                let prefix = if matches!(self, WorkspaceCase | WorkspaceWord | WorkspaceRegex) {
                    i18n::command_action_label(language, "find_files")
                } else {
                    i18n::command_action_label(language, "find_document")
                };
                format!("{prefix} · {}", i18n::search_option_label(language, option))
            }
            Language => i18n::status_language_label(language).into(),
            FilterFiles => t(TextKey::SidebarFilterPlaceholder),
            CollapseDirectories => t(TextKey::CollapseAllDirectories),
            ExpandDirectories => t(TextKey::ExpandAllDirectories),
            CloseOthers => t(TextKey::CloseOthers),
            CloseLeft => t(TextKey::CloseLeft),
            CloseRight => t(TextKey::CloseRight),
            CloseSaved => t(TextKey::CloseSaved),
            SplitLeft => t(TextKey::SplitTabLeft),
            SplitUp => t(TextKey::SplitTabUp),
            ClearTerminal => t(TextKey::ClearTerminal),
            AttachFile => i18n::add_file_to_chat_label(language).into(),
            AttachSelection => i18n::add_selection_to_chat_label(language).into(),
            Terminal | Copilot | Codex | Claude | Gemini => format!(
                "{} · {}",
                i18n::command_action_label(language, "new_tab"),
                self.agent().unwrap().localized_label(language)
            ),
            ThemeSettings | FontSettings | FormatSettings | TerminalSettings
            | AttachmentSettings => format!(
                "{} · {}",
                t(TextKey::AppSettingsMenu),
                match self {
                    ThemeSettings => t(TextKey::ThemeSection),
                    FontSettings => t(TextKey::FontsSection),
                    FormatSettings => t(TextKey::FormatSection),
                    TerminalSettings => i18n::terminal_label(language).into(),
                    _ => t(TextKey::Attachments),
                }
            ),
            OpenConfig => i18n::config_file_label(language).into(),
            AutoSave | Whitespace | HighlightLine | SimpleMode | DarkSidebar => {
                i18n::settings_label(
                    language,
                    match self {
                        AutoSave => "auto_save",
                        Whitespace => "show_whitespace",
                        HighlightLine => "highlight_line",
                        SimpleMode => "simple_mode",
                        _ => "sidebar_always_dark",
                    },
                )
                .into()
            }
            ThemeAuto => i18n::theme_auto_label(language).into(),
            ThemeLight => t(TextKey::ThemeLight),
            ThemeDark => t(TextKey::ThemeDark),
            Colorful => t(TextKey::ThemeModeColorful),
            Monochrome => t(TextKey::ThemeModeMonochrome),
            Help => t(TextKey::HelpManual),
            Website => t(TextKey::OfficialWebsite),
            CheckUpdates => i18n::check_updates_label(language).into(),
            InstallUpdate => i18n::install_update_label(language).into(),
            Quit => t(TextKey::QuitApp),
            ThemeDebugger => t(TextKey::ThemeDebugger),
            _ => i18n::menu_command_label(language, self.key()).into(),
        }
    }

    fn heading(self) -> Option<usize> {
        use ExtraCommand::*;
        match self {
            Heading1 => Some(1),
            Heading2 => Some(2),
            Heading3 => Some(3),
            Heading4 => Some(4),
            Heading5 => Some(5),
            Heading6 => Some(6),
            _ => None,
        }
    }

    fn agent(self) -> Option<ChatAgentCli> {
        use ExtraCommand::*;
        Some(match self {
            Terminal => ChatAgentCli::Terminal,
            Copilot => ChatAgentCli::GithubCopilot,
            Codex => ChatAgentCli::OpenAICodex,
            Claude => ChatAgentCli::ClaudeCode,
            Gemini => ChatAgentCli::GeminiCli,
            _ => return None,
        })
    }

    pub(super) fn export(self) -> Option<(export::ExportFormat, export::ExportTarget)> {
        use ExtraCommand::*;
        use export::{ExportFormat as F, ExportTarget as T};
        Some(match self {
            ExportMarkdown => (F::Markdown, T::File),
            ExportHtml => (F::Html, T::File),
            ExportPdf => (F::Pdf, T::File),
            ExportPng => (F::Png, T::File),
            CopyMarkdown => (F::Markdown, T::Clipboard),
            CopyHtml => (F::Html, T::Clipboard),
            CopyPdf => (F::Pdf, T::Clipboard),
            CopyPng => (F::Png, T::Clipboard),
            _ => return None,
        })
    }
}

#[derive(Clone)]
pub(super) struct CommandOrigin {
    focus: Option<WeakFocusHandle>,
    pane: PaneId,
    tab: Option<String>,
    text_input: bool,
    shortcuts: HashMap<CommandId, String>,
}

impl CommandOrigin {
    fn is_current(&self, editor: &EditorView) -> bool {
        self.pane == editor.current_pane_id()
            && self.tab == editor.active_tab_id
            && self
                .focus
                .as_ref()
                .is_none_or(|focus| focus.upgrade().is_some())
    }
}

#[derive(Default)]
struct MenuEditors {
    editors: HashMap<gpui::WindowId, gpui::WeakEntity<EditorView>>,
    signature: Option<MenuSignature>,
}
impl gpui::Global for MenuEditors {}

#[derive(PartialEq)]
struct MenuSignature {
    editor: gpui::EntityId,
    language: UiLanguage,
    workspaces: Vec<PathBuf>,
    panes: Vec<PaneId>,
    pinned: bool,
}

pub(super) struct CommandState {
    pub enabled: bool,
    pub checked: Option<bool>,
    pub reason: Option<&'static str>,
}

impl EditorView {
    #[cfg(target_os = "linux")]
    pub(super) fn on_toggle_application_menu(
        &mut self,
        _: &ToggleApplicationMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.application_menu.is_none() {
            let menu = cx.new(|cx| {
                crate::ui::app_menu::AppMenu::new(
                    self.styles.context_menu.clone(),
                    self.ui_scale,
                    self.styles.ui_font_family,
                    |action| {
                        id_for_action(action.as_ref()).map_or(action, |id| {
                            Box::new(RunCommand {
                                id: id.config_key().into(),
                            }) as Box<dyn Action>
                        })
                    },
                    cx,
                )
            });
            cx.subscribe(&menu, |this, _, _: &crate::ui::app_menu::MenuClosed, _| {
                // Keep the source identity through deferred dispatch. Dismissal
                // restores focus in the menu itself, not at the next render.
                this.clear_menu_origin_after_dispatch = true;
            })
            .detach();
            self.application_menu = Some(menu);
        }
        let menu = self.application_menu.clone().unwrap();
        if !menu.read(cx).is_open() {
            self.capture_command_origin(window, cx);
        }
        menu.update(cx, |menu, cx| {
            menu.set_styles(
                self.styles.context_menu.clone(),
                self.ui_scale,
                self.styles.ui_font_family,
                cx,
            );
            menu.toggle(window, cx);
        });
        cx.notify();
    }

    pub(super) fn sync_command_window(&mut self, window: &Window, cx: &mut Context<Self>) {
        let terminal_focus = self
            .active_terminal_tab_id()
            .and_then(|tab| self.terminal_tab_state(&tab))
            .map(|state| &state.focus_handle);
        self.command_focus_is_text_input = window
            .focused(cx)
            .as_ref()
            .is_some_and(|focus| focus != &self.focus_handle && Some(focus) != terminal_focus);
        let entity = cx.entity();
        let pinned = self
            .active_tab_index()
            .is_some_and(|index| self.tabs[index].is_pinned);
        let signature = MenuSignature {
            editor: cx.entity_id(),
            language: self.ui_language,
            workspaces: self.workspaces.clone(),
            panes: self.pane_ids(),
            pinned,
        };
        let registry = cx.default_global::<MenuEditors>();
        registry
            .editors
            .retain(|_, editor| editor.upgrade().is_some());
        registry
            .editors
            .insert(window.window_handle().window_id(), entity.downgrade());
        if !window.is_window_active() || registry.signature.as_ref() == Some(&signature) {
            return;
        }
        registry.signature = Some(signature);
        let menus = shortcut::contextual_app_menus(self.ui_language, pinned, &self.workspaces);
        let language = self.ui_language;
        cx.spawn(async move |_, cx| {
            cx.update(|cx| shortcut::apply_application_menus(cx, menus, language))
                .ok();
        })
        .detach();
    }

    pub(super) fn on_switch_workspace_command(
        &mut self,
        action: &SwitchWorkspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = PathBuf::from(&action.path);
        self.restore_command_focus(window, cx);
        if self.workspaces.contains(&root) {
            self.switch_workspace(root, cx);
        } else {
            self.notify_error(
                i18n::menu_command_label(self.ui_language, "command_target_changed"),
                cx,
            );
        }
    }

    pub(super) fn on_move_tab_command(
        &mut self,
        action: &MoveActiveTabToPane,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let valid_origin = self
            .command_origin
            .as_ref()
            .is_none_or(|origin| origin.is_current(self));
        self.restore_command_focus(window, cx);
        let Ok(pane) = usize::try_from(action.pane) else {
            self.notify_error(
                i18n::menu_command_label(self.ui_language, "command_target_changed"),
                cx,
            );
            return;
        };
        if !valid_origin || !self.pane_ids().contains(&pane) {
            self.notify_error(
                i18n::menu_command_label(self.ui_language, "command_target_changed"),
                cx,
            );
            return;
        }
        if let Some(tab) = self.active_tab_id.clone() {
            self.move_tab_to_pane(&tab, pane, None, cx);
        }
    }

    pub(super) fn capture_command_origin(&mut self, window: &Window, cx: &Context<Self>) {
        let focus = window.focused(cx);
        let shortcuts = COMMAND_SPECS
            .iter()
            .filter_map(|spec| {
                let binding = window.highest_precedence_binding_for_action_in(
                    spec.id.action().as_ref(),
                    focus.as_ref()?,
                )?;
                Some((
                    spec.id,
                    binding
                        .keystrokes()
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" "),
                ))
            })
            .collect();
        self.command_origin = Some(CommandOrigin {
            text_input: focus.as_ref().is_some_and(|focus| {
                focus != &self.focus_handle
                    && !self
                        .active_terminal_tab_id()
                        .and_then(|tab| self.terminal_tab_state(&tab))
                        .is_some_and(|state| &state.focus_handle == focus)
            }),
            focus: focus.map(|focus| focus.downgrade()),
            pane: self.current_pane_id(),
            tab: self.active_tab_id.clone(),
            shortcuts,
        });
    }

    pub(super) fn command_shortcut_hint(&self, id: CommandId) -> Option<String> {
        self.app_settings
            .keybindings
            .get(id.config_key())
            .or_else(|| {
                self.command_origin
                    .as_ref()
                    .and_then(|origin| origin.shortcuts.get(&id))
            })
            .map(|keystroke| shortcut::display_shortcut_from_keystroke(keystroke))
    }

    pub(super) fn restore_command_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "linux")]
        {
            self.clear_menu_origin_after_dispatch = false;
        }
        if let Some(focus) = self
            .command_origin
            .as_ref()
            .and_then(|origin| origin.focus.as_ref())
            .and_then(WeakFocusHandle::upgrade)
        {
            window.focus(&focus);
        }
        self.command_origin = None;
        self.pending_command_focus_restore = false;
        cx.notify();
    }

    pub(super) fn command_state(&self, id: CommandId) -> CommandState {
        use CommandId as C;
        use ExtraCommand as E;
        let mut state = CommandState {
            enabled: true,
            checked: None,
            reason: None,
        };
        let input = self
            .command_origin
            .as_ref()
            .map(|origin| origin.text_input)
            .unwrap_or(self.command_focus_is_text_input);
        let has_tab = self.active_tab_id.is_some();
        let text = self.active_tab_supports_text_buffer();
        let markdown = self.active_tab_is_markdown();
        let writable =
            text && !self.active_tab_is_read_only() && self.line_diff_selection.is_none();
        let root = !self.sidebar_root.as_os_str().is_empty();
        let terminal = self.active_tab_is_terminal();
        let mut require = |condition: bool, key: &'static str| {
            if !condition && state.reason.is_none() {
                state.enabled = false;
                state.reason = Some(i18n::menu_command_label(self.ui_language, key));
            }
        };
        require(id.is_available(), "command_unavailable");
        require(
            self.command_origin
                .as_ref()
                .is_none_or(|origin| origin.is_current(self)),
            "command_target_changed",
        );
        require(
            self.dialog.is_none()
                || matches!(
                    id,
                    C::Extra(E::Copy | E::Cut | E::Paste | E::PastePlain)
                        | C::SelectAll
                        | C::Undo
                        | C::Redo
                ),
            "command_unavailable",
        );
        if COMMAND_SPECS
            .iter()
            .find(|spec| spec.id == id)
            .is_some_and(|spec| spec.markdown_only)
        {
            require(markdown, "requires_markdown");
        }
        match id {
            C::Save | C::SaveAs => {
                require(self.active_tab_supports_persistent_save(), "requires_text")
            }
            C::CloseTab | C::PinTab | C::SplitRight | C::SplitDown => {
                require(has_tab, "requires_document")
            }
            C::NextTab | C::PreviousTab => require(self.tabs.len() > 1, "requires_other_tab"),
            C::FocusNextPane => require(self.pane_ids().len() > 1, "requires_other_pane"),
            C::Find => require(text || terminal, "requires_text"),
            C::FindInFiles | C::QuickOpen => require(root, "requires_workspace"),
            C::Bold
            | C::Italic
            | C::Underline
            | C::Strikethrough
            | C::InlineCode
            | C::Link
            | C::Image
            | C::Quote
            | C::Todo
            | C::UnorderedList
            | C::OrderedList
            | C::Table
            | C::CodeBlock
            | C::Paragraph
            | C::HorizontalRule => require(writable && !input, "requires_editable"),
            C::DuplicateLine
            | C::DeleteLine
            | C::MoveLineUp
            | C::MoveLineDown
            | C::JoinLines
            | C::ToggleComment
            | C::AddCursorAbove
            | C::AddCursorBelow => require(writable && !input, "requires_editable"),
            C::Undo => require(
                input
                    || (writable
                        && (self.pending_history_transaction.is_some()
                            || self.active_tab_index().is_some_and(|index| {
                                !self.tabs[index].history.undo_stack.is_empty()
                            }))),
                "command_unavailable",
            ),
            C::Redo => require(
                !input
                    && writable
                    && self
                        .active_tab_index()
                        .is_some_and(|index| !self.tabs[index].history.redo_stack.is_empty()),
                "command_unavailable",
            ),
            C::ReopenClosedTab => {
                require(!self.recently_closed_tabs.is_empty(), "command_unavailable")
            }
            C::SelectAll => require(input || text || terminal, "requires_text"),
            C::SelectNextOccurrence
            | C::SelectAllOccurrences
            | C::Fold
            | C::Unfold
            | C::FoldAll
            | C::UnfoldAll
            | C::ToggleLeftAlign => require(text && !input, "requires_text"),
            C::ToggleWordWrap => require(self.editor_uses_code_gutter(), "requires_text"),
            C::Extra(extra) => {
                if extra.export().is_some() {
                    require(text, "requires_text");
                    require(!export::export_in_progress(), "command_busy");
                }
                if extra.markdown_only() && extra != E::CopyPlain {
                    require(writable && !input, "requires_editable");
                }
                match extra {
                    E::Copy | E::Paste => require(input || text || terminal, "requires_text"),
                    E::Cut | E::PastePlain => {
                        require(input || writable || terminal, "requires_editable")
                    }
                    E::CopyPlain => require(markdown, "requires_markdown"),
                    E::WordLeft
                    | E::WordRight
                    | E::LineStart
                    | E::LineEnd
                    | E::SelectWordLeft
                    | E::SelectWordRight
                    | E::SelectLineStart
                    | E::SelectLineEnd => require(input || text, "requires_text"),
                    E::SelectLine
                    | E::DocumentStart
                    | E::DocumentEnd
                    | E::PageDown
                    | E::SelectDocumentStart
                    | E::SelectDocumentEnd
                    | E::ToggleFold
                    | E::Language
                    | E::Indentation => require(text && !input, "requires_text"),
                    E::DeleteWordLeft | E::DeleteWordRight | E::DeleteToStart | E::DeleteToEnd => {
                        require(input || writable, "requires_editable")
                    }
                    E::InsertAbove
                    | E::InsertBelow
                    | E::CopyLineUp
                    | E::CopyLineDown
                    | E::Indent
                    | E::Outdent
                    | E::Transpose
                    | E::KillLine
                    | E::Yank
                    | E::OpenLine => require(writable && !input, "requires_editable"),
                    E::CloseWorkspace
                    | E::FileList
                    | E::StarredSection
                    | E::CollapseDirectories
                    | E::ExpandDirectories
                    | E::FilterFiles
                    | E::FileSort => require(root, "requires_workspace"),
                    E::ChooseWorkspace => require(self.workspaces.len() > 1, "requires_workspace"),
                    E::RevealFile | E::LocateFile | E::CopyPath | E::AttachFile => {
                        require(self.current_file_path.is_some(), "requires_saved_file")
                    }
                    E::CopyRelativePath => require(
                        self.current_file_path
                            .as_ref()
                            .and_then(|p| self.path_copy_text(p, true))
                            .is_some(),
                        "requires_saved_file",
                    ),
                    E::AttachSelection => require(
                        self.current_file_path.is_some() && self.cursor.has_selection(),
                        "requires_selection",
                    ),
                    E::EditMode
                    | E::ReadingMode
                    | E::SourceMode
                    | E::CycleMode
                    | E::PreviousMode => require(text, "requires_text"),
                    E::FindNext | E::FindPrevious => require(
                        self.search_visible
                            && if let Some(tab) = self.active_terminal_tab_id() {
                                self.terminal_search_match_count(&tab) > 0
                            } else {
                                !self.search_matches.is_empty()
                            },
                        "requires_search",
                    ),
                    E::CloseSearch | E::SearchCase | E::SearchWord | E::SearchRegex => {
                        require(self.search_visible, "requires_search")
                    }
                    E::Replace => require(writable, "requires_editable"),
                    E::ReplaceNext | E::ReplaceAll => require(
                        writable
                            && self.search_visible
                            && self.search_replace_visible
                            && !self.search_matches.is_empty(),
                        "requires_search",
                    ),
                    E::SwitchSearchInput => require(
                        writable && self.search_visible && self.search_replace_visible,
                        "requires_search",
                    ),
                    E::WorkspaceFilters
                    | E::WorkspaceCase
                    | E::WorkspaceWord
                    | E::WorkspaceRegex
                    | E::CloseWorkspaceSearch => {
                        require(self.workspace_search_visible, "requires_search")
                    }
                    E::CloseOthers | E::CloseLeft | E::CloseRight => require(
                        self.active_tab_index().is_some_and(|active| {
                            self.tabs.iter().enumerate().any(|(index, tab)| {
                                !tab.is_pinned
                                    && match extra {
                                        E::CloseLeft => index < active,
                                        E::CloseRight => index > active,
                                        _ => index != active,
                                    }
                            })
                        }),
                        "requires_other_tab",
                    ),
                    E::CloseSaved => require(
                        self.tabs.iter().any(|tab| {
                            !tab.is_dirty && self.active_tab_id.as_ref() != Some(&tab.id)
                        }),
                        "command_unavailable",
                    ),
                    E::KeepTab => require(
                        self.active_tab_index()
                            .is_some_and(|index| self.tabs[index].is_preview),
                        "command_unavailable",
                    ),
                    E::SplitLeft | E::SplitUp => require(has_tab, "requires_document"),
                    E::MoveTab => {
                        require(has_tab && self.pane_ids().len() > 1, "requires_other_pane")
                    }
                    E::Back => require(self.can_navigate_tab_history_back(), "command_unavailable"),
                    E::Forward => require(
                        self.can_navigate_tab_history_forward(),
                        "command_unavailable",
                    ),
                    E::CloseTerminal | E::ClearTerminal => require(terminal, "requires_terminal"),
                    E::CopyImage | E::ImageZoomIn | E::ImageZoomOut | E::ImageReset => {
                        require(self.active_tab_is_image(), "requires_image")
                    }
                    E::InstallUpdate => {
                        require(self.prepared_update().is_some(), "command_unavailable")
                    }
                    E::CheckUpdates => require(!self.update_check_in_progress(), "command_busy"),
                    _ => {}
                }
            }
            _ => {}
        }
        state.checked = match id {
            C::PinTab => self
                .active_tab_index()
                .map(|index| self.tabs[index].is_pinned),
            C::ToggleSidebar => Some(self.sidebar_visible),
            C::ToggleInfo => Some(self.utility_panel == Some(RightUtilityPanel::Info)),
            C::ToggleOutline => Some(self.outline_rail_visible),
            C::Extra(E::EditMode) => Some(self.document_mode() == scaling::DocumentMode::Edit),
            C::Extra(E::ReadingMode) => {
                Some(self.document_mode() == scaling::DocumentMode::Reading)
            }
            C::Extra(E::SourceMode) => Some(self.document_mode() == scaling::DocumentMode::Source),
            C::Extra(E::AutoSave) => Some(self.app_settings.auto_save),
            C::Extra(E::Whitespace) => Some(self.app_settings.show_whitespace),
            C::Extra(E::HighlightLine) => Some(self.app_settings.highlight_line_background),
            C::Extra(E::SimpleMode) => Some(self.app_settings.simple_mode),
            C::Extra(E::DarkSidebar) => Some(self.app_settings.sidebar_always_dark),
            C::Extra(E::FileList) => Some(self.file_list_visible),
            C::Extra(E::StarredSection) => Some(!self.sidebar_starred_collapsed),
            C::Extra(E::SearchCase) => Some(self.search_case_sensitive),
            C::Extra(E::SearchWord) => Some(self.search_whole_word),
            C::Extra(E::SearchRegex) => Some(self.search_use_regex),
            C::Extra(E::WorkspaceCase) => Some(self.workspace_search_case_sensitive),
            C::Extra(E::WorkspaceWord) => Some(self.workspace_search_whole_word),
            C::Extra(E::WorkspaceRegex) => Some(self.workspace_search_use_regex),
            C::Extra(E::WorkspaceFilters) => Some(self.workspace_search_filters_visible),
            C::Extra(E::OutlinePanel) => {
                Some(self.utility_panel == Some(RightUtilityPanel::Outline))
            }
            C::Extra(E::Minimap) => {
                Some(self.app_settings.minimap_position != app_settings::MinimapPosition::Off)
            }
            C::Extra(E::MinimapLeft) => {
                Some(self.app_settings.minimap_position == app_settings::MinimapPosition::Left)
            }
            C::Extra(E::MinimapRight) => {
                Some(self.app_settings.minimap_position == app_settings::MinimapPosition::Right)
            }
            C::Extra(E::MinimapOff) => {
                Some(self.app_settings.minimap_position == app_settings::MinimapPosition::Off)
            }
            C::Extra(E::ThemeAuto) => {
                Some(self.app_settings.theme_mode() == app_settings::ThemeMode::Auto)
            }
            C::Extra(E::ThemeLight) => Some(
                self.app_settings.theme_mode() == app_settings::ThemeMode::Manual
                    && !self.theme_kind.is_dark(),
            ),
            C::Extra(E::ThemeDark) => Some(
                self.app_settings.theme_mode() == app_settings::ThemeMode::Manual
                    && self.theme_kind.is_dark(),
            ),
            C::Extra(E::Colorful) => {
                Some(self.editor_theme_mode == crate::theme::EditorThemeMode::Colorful)
            }
            C::Extra(E::Monochrome) => {
                Some(self.editor_theme_mode == crate::theme::EditorThemeMode::Monochrome)
            }
            C::Extra(E::ThemeDebugger) => Some(self.theme_debugger_visible),
            C::Extra(E::RuntimeProfile) => Some(self.runtime_profiler.is_some()),
            C::Extra(E::Fullscreen) => Some(self.window_is_fullscreen),
            _ => None,
        };
        state
    }

    pub(super) fn on_run_command(
        &mut self,
        action: &RunCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = CommandId::from_config_key(&action.id) else {
            self.notify_error(
                format!(
                    "{}: {}",
                    self.message(i18n::MessageKey::UnknownCommand),
                    action.id
                ),
                cx,
            );
            return;
        };
        if self
            .command_origin
            .as_ref()
            .is_some_and(|origin| !origin.is_current(self))
        {
            self.restore_command_focus(window, cx);
            self.notify_error(
                i18n::menu_command_label(self.ui_language, "command_target_changed"),
                cx,
            );
            return;
        }
        let state = self.command_state(id);
        if !state.enabled {
            self.restore_command_focus(window, cx);
            self.notify_info(state.reason.unwrap_or(self.t(TextKey::Unavailable)), cx);
            return;
        }
        self.restore_command_focus(window, cx);
        if id == CommandId::PinTab {
            if let Some(tab) = self.active_tab_id.clone() {
                self.toggle_pin_tab(&tab, cx);
            }
            return;
        }
        if let CommandId::Extra(command) = id {
            self.execute_extra_command(command, window, cx);
        } else {
            window.dispatch_action(id.action(), cx);
        }
    }

    fn execute_extra_command(
        &mut self,
        command: ExtraCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use ExtraCommand as E;
        if let Some((format, target)) = command.export() {
            self.export_current(format, target, cx);
            return;
        }
        if let Some(agent) = command.agent() {
            self.create_terminal_tab(agent, cx);
            return;
        }
        if let Some(level) = command.heading() {
            self.line_format_toolbar_line = None;
            self.execute_format_menu_action(
                match level {
                    1 => FormatMenuAction::Heading1,
                    2 => FormatMenuAction::Heading2,
                    3 => FormatMenuAction::Heading3,
                    4 => FormatMenuAction::Heading4,
                    5 => FormatMenuAction::Heading5,
                    _ => FormatMenuAction::Heading6,
                },
                cx,
            );
            return;
        }
        macro_rules! dispatch {
            ($action:ident) => {
                window.dispatch_action(Box::new(crate::actions::$action), cx)
            };
        }
        let anchor = gpui::point(
            gpui::px(self.last_window_width / 2.0),
            gpui::px(self.tab_bar_height()),
        );
        match command {
            E::Cut => dispatch!(Cut),
            E::Copy => dispatch!(Copy),
            E::Paste => dispatch!(Paste),
            E::PastePlain => dispatch!(PastePlainText),
            E::CopyPlain => self.copy_plain_text_to_clipboard(cx),
            E::SelectLine => dispatch!(SelectLine),
            E::InsertAbove => dispatch!(InsertLineAbove),
            E::InsertBelow => dispatch!(InsertLineBelow),
            E::CopyLineUp => dispatch!(CopyLineUp),
            E::CopyLineDown => dispatch!(CopyLineDown),
            E::Indent => dispatch!(Tab),
            E::Outdent => dispatch!(BackTab),
            E::Transpose => dispatch!(TransposeChars),
            E::DeleteWordLeft => dispatch!(DeleteWordLeft),
            E::DeleteWordRight => dispatch!(DeleteWordRight),
            E::DeleteToStart => dispatch!(DeleteToLineStart),
            E::DeleteToEnd => dispatch!(DeleteToLineEnd),
            E::KillLine => dispatch!(KillToLineEnd),
            E::Yank => dispatch!(Yank),
            E::OpenLine => dispatch!(OpenLine),
            E::WordLeft => dispatch!(MoveWordLeft),
            E::WordRight => dispatch!(MoveWordRight),
            E::LineStart => dispatch!(Home),
            E::LineEnd => dispatch!(End),
            E::DocumentStart => dispatch!(MoveToDocumentStart),
            E::DocumentEnd => dispatch!(MoveToDocumentEnd),
            E::PageDown => dispatch!(MovePageDown),
            E::SelectWordLeft => dispatch!(SelectWordLeft),
            E::SelectWordRight => dispatch!(SelectWordRight),
            E::SelectLineStart => dispatch!(SelectHome),
            E::SelectLineEnd => dispatch!(SelectEnd),
            E::SelectDocumentStart => dispatch!(SelectToDocumentStart),
            E::SelectDocumentEnd => dispatch!(SelectToDocumentEnd),
            E::CharacterPalette => dispatch!(ShowCharacterPalette),
            E::ToggleFold => dispatch!(ToggleFold),
            E::Highlight
            | E::Superscript
            | E::Subscript
            | E::Footnote
            | E::WikiLink
            | E::DateTime
            | E::Callout => {
                self.line_format_toolbar_line = None;
                self.execute_format_menu_action(
                    match command {
                        E::Highlight => FormatMenuAction::Highlight,
                        E::Superscript => FormatMenuAction::Superscript,
                        E::Subscript => FormatMenuAction::Subscript,
                        E::Footnote => FormatMenuAction::Footnote,
                        E::WikiLink => FormatMenuAction::WikiLink,
                        E::DateTime => FormatMenuAction::DateTime,
                        _ => FormatMenuAction::CalloutTip,
                    },
                    cx,
                );
            }
            E::FormatMarkdown => self.format_markdown_selection_or_prompt(cx),
            E::OpenWorkspace => self.pick_workspace_folder(cx),
            E::ChooseWorkspace => self.open_command_workspace_picker(window, cx),
            E::CloseWorkspace => self.close_workspace(&self.sidebar_root.clone(), cx),
            E::RevealFile | E::LocateFile | E::CopyPath | E::CopyRelativePath => {
                if let Some(path) = self.current_file_path.clone() {
                    match command {
                        E::RevealFile => self.reveal_in_file_manager(&path, cx),
                        E::LocateFile => self.locate_path_in_sidebar(path, cx),
                        _ => {
                            if let Some(text) =
                                self.path_copy_text(&path, command == E::CopyRelativePath)
                            {
                                self.copy_text_to_clipboard(text, cx);
                            }
                        }
                    }
                }
            }
            E::FindNext => dispatch!(FindNext),
            E::FindPrevious => dispatch!(FindPrevious),
            E::Replace => dispatch!(ToggleReplace),
            E::ReplaceNext => dispatch!(ReplaceNext),
            E::ReplaceAll => dispatch!(ReplaceAll),
            E::CloseSearch => dispatch!(DismissSearch),
            E::SwitchSearchInput => {
                self.switch_search_input(&crate::actions::SwitchSearchInput, window, cx)
            }
            E::SearchCase => dispatch!(ToggleSearchCaseSensitive),
            E::SearchWord => dispatch!(ToggleSearchWholeWord),
            E::SearchRegex => dispatch!(ToggleSearchRegex),
            E::WorkspaceFilters => self.toggle_workspace_search_filters(window, cx),
            E::WorkspaceCase | E::WorkspaceWord | E::WorkspaceRegex => {
                match command {
                    E::WorkspaceCase => {
                        self.workspace_search_case_sensitive = !self.workspace_search_case_sensitive
                    }
                    E::WorkspaceWord => {
                        self.workspace_search_whole_word = !self.workspace_search_whole_word
                    }
                    _ => self.workspace_search_use_regex = !self.workspace_search_use_regex,
                }
                self.run_workspace_search(cx);
            }
            E::CloseWorkspaceSearch => self.close_workspace_search(window, cx),
            E::EditMode | E::ReadingMode | E::SourceMode => self.set_document_mode(
                match command {
                    E::EditMode => scaling::DocumentMode::Edit,
                    E::ReadingMode => scaling::DocumentMode::Reading,
                    _ => scaling::DocumentMode::Source,
                },
                cx,
            ),
            E::CycleMode => self.cycle_document_mode(cx),
            E::PreviousMode => self.cycle_document_mode_backwards(cx),
            E::Language => self.open_language_picker(anchor, window, cx),
            E::Indentation => self.open_indentation_picker(anchor, window, cx),
            E::FileList => self.toggle_file_list_visible(cx),
            E::OutlinePanel => self.toggle_outline_panel(cx),
            E::Minimap => self.toggle_minimap_visibility(cx),
            E::MinimapLeft | E::MinimapRight | E::MinimapOff => {
                self.apply_settings_option_action(
                    settings_panel::SettingsOptionAction::Minimap(match command {
                        E::MinimapLeft => app_settings::MinimapPosition::Left,
                        E::MinimapRight => app_settings::MinimapPosition::Right,
                        _ => app_settings::MinimapPosition::Off,
                    }),
                    cx,
                );
            }
            E::StarredSection => self.toggle_starred_section(cx),
            E::CollapseDirectories => self.collapse_all_dirs(cx),
            E::ExpandDirectories => self.expand_all_dirs(cx),
            E::FilterFiles => self.focus_sidebar_filter(window, cx),
            E::FileSort => self.open_sidebar_sort_menu(anchor, cx),
            E::CloseOthers | E::CloseLeft | E::CloseRight | E::KeepTab => {
                if let Some(tab) = self.active_tab_id.clone() {
                    match command {
                        E::CloseOthers => self.close_other_tabs(&tab, cx),
                        E::CloseLeft => self.close_left_tabs(&tab, cx),
                        E::CloseRight => self.close_right_tabs(&tab, cx),
                        _ => self.promote_tab_to_permanent(&tab, cx),
                    }
                }
            }
            E::CloseSaved => self.close_saved_tabs(cx),
            E::SplitLeft => dispatch!(SplitPaneLeft),
            E::SplitUp => dispatch!(SplitPaneUp),
            E::MoveTab => self.open_command_pane_picker(window, cx),
            E::Back => self.navigate_tab_history_back(cx),
            E::Forward => self.navigate_tab_history_forward(cx),
            E::CloseTerminal => {
                if let Some(tab) = self.active_terminal_tab_id() {
                    self.request_close_tab(&tab, cx);
                }
            }
            E::ClearTerminal => {
                if let Some(tab) = self.active_terminal_tab_id() {
                    self.clear_terminal(&tab, cx);
                }
            }
            E::AttachFile | E::AttachSelection => {
                if let Some(path) = self.current_file_path.clone() {
                    if command == E::AttachFile {
                        self.add_path_to_chat(&path, cx);
                    } else {
                        self.add_selection_to_chat(&path, self.cursor.selection_range(), cx);
                    }
                }
            }
            E::CopyImage => self.copy_active_image_to_clipboard(cx),
            E::ImageZoomIn => {
                self.zoom_active_image_view(1.2, None);
                cx.notify();
            }
            E::ImageZoomOut => {
                self.zoom_active_image_view(1.0 / 1.2, None);
                cx.notify();
            }
            E::ImageReset => {
                self.reset_active_image_view();
                cx.notify();
            }
            E::ThemeSettings
            | E::FontSettings
            | E::FormatSettings
            | E::TerminalSettings
            | E::AttachmentSettings => self.open_settings_section(
                match command {
                    E::ThemeSettings => settings_panel::SettingsSection::Theme,
                    E::FontSettings => settings_panel::SettingsSection::Fonts,
                    E::FormatSettings => settings_panel::SettingsSection::Format,
                    E::TerminalSettings => settings_panel::SettingsSection::Terminal,
                    _ => settings_panel::SettingsSection::Attachments,
                },
                cx,
            ),
            E::OpenConfig => self.open_config_file_tab(cx),
            E::AutoSave | E::Whitespace | E::HighlightLine | E::SimpleMode | E::DarkSidebar => {
                use settings_panel::SettingsOptionAction as S;
                self.apply_settings_option_action(
                    match command {
                        E::AutoSave => S::AutoSave(!self.app_settings.auto_save),
                        E::Whitespace => S::ShowWhitespace(!self.app_settings.show_whitespace),
                        E::HighlightLine => {
                            S::HighlightLine(!self.app_settings.highlight_line_background)
                        }
                        E::SimpleMode => S::SimpleMode(!self.app_settings.simple_mode),
                        _ => S::SidebarAlwaysDark(!self.app_settings.sidebar_always_dark),
                    },
                    cx,
                );
            }
            E::ThemeAuto | E::ThemeLight | E::ThemeDark => self.select_theme_appearance(
                match command {
                    E::ThemeAuto => settings_panel::ThemeAppearanceChoice::Auto,
                    E::ThemeLight => settings_panel::ThemeAppearanceChoice::Light,
                    _ => settings_panel::ThemeAppearanceChoice::Dark,
                },
                window,
                cx,
            ),
            E::Colorful | E::Monochrome => self.set_editor_theme_mode(
                if command == E::Colorful {
                    crate::theme::EditorThemeMode::Colorful
                } else {
                    crate::theme::EditorThemeMode::Monochrome
                },
                cx,
            ),
            E::Palette => self.toggle_command_palette(window, cx),
            E::Help => self.open_welcome_tab(cx),
            E::Website => {
                if let Err(error) = crate::platform::shell::open_url("https://moeka.app/") {
                    self.notify_diagnostic(error, cx);
                }
            }
            E::CheckUpdates => self.start_manual_update_check(cx),
            E::InstallUpdate => self.show_prepared_update_dialog(cx),
            E::Quit => cx.quit(),
            E::Minimize => window.minimize_window(),
            E::Maximize => crate::platform::window_chrome::toggle_maximize(window),
            E::Fullscreen => dispatch!(ToggleFullscreen),
            E::CloseWindow => window.remove_window(),
            E::ThemeDebugger => self.toggle_theme_debugger_visibility(cx),
            E::ExportTheme => self.export_theme_debugger_theme(cx),
            E::RuntimeProfile => dispatch!(ToggleRuntimeProfile),
            E::ProfileSnapshot => dispatch!(WriteRuntimeProfileSnapshot),
            E::ExportMarkdown
            | E::ExportHtml
            | E::ExportPdf
            | E::ExportPng
            | E::CopyMarkdown
            | E::CopyHtml
            | E::CopyPdf
            | E::CopyPng
            | E::Terminal
            | E::Copilot
            | E::Codex
            | E::Claude
            | E::Gemini
            | E::Heading1
            | E::Heading2
            | E::Heading3
            | E::Heading4
            | E::Heading5
            | E::Heading6 => unreachable!(),
        }
    }
}

pub(super) fn id_for_action(action: &dyn Action) -> Option<CommandId> {
    if let Some(command) = action.as_any().downcast_ref::<RunCommand>() {
        return CommandId::from_config_key(&command.id);
    }
    static TYPES: std::sync::LazyLock<HashMap<std::any::TypeId, CommandId>> =
        std::sync::LazyLock::new(|| {
            COMMAND_SPECS
                .iter()
                .filter_map(|spec| {
                    let action = spec.id.action();
                    (!action.as_any().is::<RunCommand>())
                        .then_some((action.as_any().type_id(), spec.id))
                })
                .collect()
        });
    TYPES.get(&action.as_any().type_id()).copied()
}

pub(super) fn install_menu_state(cx: &mut App) {
    cx.set_menu_action_state_handler(|action, cx| {
        let Some(window) = cx.active_window().map(|window| window.window_id()) else {
            let id = id_for_action(action)?;
            let enabled = matches!(
                id,
                CommandId::NewDocument
                    | CommandId::NewTab
                    | CommandId::OpenFile
                    | CommandId::OpenSettings
                    | CommandId::OpenTerminalWindow
                    | CommandId::Extra(
                        ExtraCommand::Quit
                            | ExtraCommand::OpenWorkspace
                            | ExtraCommand::Help
                            | ExtraCommand::Website
                            | ExtraCommand::CheckUpdates
                            | ExtraCommand::OpenConfig
                            | ExtraCommand::ThemeSettings
                            | ExtraCommand::FontSettings
                            | ExtraCommand::FormatSettings
                            | ExtraCommand::TerminalSettings
                            | ExtraCommand::AttachmentSettings
                    )
            );
            return Some(gpui::AppMenuActionState {
                enabled: enabled && id.is_available(),
                checked: None,
            });
        };
        let entity = cx
            .try_global::<MenuEditors>()?
            .editors
            .get(&window)?
            .upgrade()?;
        let editor = entity.read(cx);
        if let Some(action) = action.as_any().downcast_ref::<SwitchWorkspace>() {
            return Some(gpui::AppMenuActionState {
                enabled: editor
                    .workspaces
                    .iter()
                    .any(|root| root == Path::new(&action.path)),
                checked: Some(editor.sidebar_root == Path::new(&action.path)),
            });
        }

        let state = editor.command_state(id_for_action(action)?);
        Some(gpui::AppMenuActionState {
            enabled: state.enabled,
            checked: state.checked,
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_inventory_covers_reviewed_ui_operation_families() {
        // Independent acceptance inventory: comparing the menu to the palette
        // alone would pass when both accidentally dropped a feature.
        for key in [
            "export_markdown",
            "export_html",
            "export_pdf",
            "export_png",
            "copy_markdown",
            "copy_html",
            "copy_pdf",
            "copy_png",
            "open_workspace",
            "close_workspace",
            "reveal_file",
            "locate_file",
            "copy_path",
            "copy_relative_path",
            "paste_plain_text",
            "copy_plain_text",
            "select_line",
            "insert_line_above",
            "insert_line_below",
            "copy_line_up",
            "copy_line_down",
            "indent",
            "outdent",
            "transpose_chars",
            "kill_to_line_end",
            "yank",
            "heading_1",
            "heading_2",
            "heading_3",
            "heading_4",
            "heading_5",
            "heading_6",
            "highlight",
            "superscript",
            "subscript",
            "footnote",
            "wiki_link",
            "date_time",
            "callout_tip",
            "format_markdown",
            "find_next",
            "find_previous",
            "replace",
            "replace_next",
            "replace_all",
            "close_search",
            "switch_search_input",
            "workspace_filters",
            "workspace_case",
            "workspace_word",
            "workspace_regex",
            "edit_mode",
            "reading_mode",
            "source_mode",
            "cycle_document_mode",
            "document_language",
            "document_indentation",
            "toggle_file_list",
            "toggle_outline_panel",
            "minimap_left",
            "minimap_right",
            "minimap_off",
            "filter_files",
            "file_sort",
            "collapse_directories",
            "expand_directories",
            "close_other_tabs",
            "close_left_tabs",
            "close_right_tabs",
            "close_saved_tabs",
            "keep_tab",
            "pin_tab",
            "split_left",
            "split_right",
            "split_up",
            "split_down",
            "move_tab_to_pane",
            "navigate_back",
            "navigate_forward",
            "new_terminal",
            "new_copilot",
            "new_codex",
            "new_claude",
            "new_gemini",
            "clear_terminal",
            "attach_file",
            "attach_selection",
            "copy_image",
            "image_zoom_in",
            "image_zoom_out",
            "image_reset",
            "settings_theme",
            "settings_fonts",
            "settings_format",
            "settings_terminal",
            "settings_attachments",
            "open_config",
            "toggle_auto_save",
            "toggle_whitespace",
            "toggle_highlight_line",
            "toggle_simple_mode",
            "toggle_dark_sidebar",
            "theme_auto",
            "theme_light",
            "theme_dark",
            "theme_colorful",
            "theme_monochrome",
            "command_palette",
            "help",
            "website",
            "check_updates",
            "install_update",
            "quit",
            "minimize_window",
            "maximize_window",
            "fullscreen",
            "close_window",
            "theme_debugger",
            "export_theme",
            "runtime_profile",
            "profile_snapshot",
        ] {
            assert!(
                CommandId::from_config_key(key).is_some(),
                "uncovered UI command: {key}"
            );
        }
    }

    #[test]
    fn command_menu_and_palette_have_exactly_the_same_available_commands() {
        fn collect(items: &[gpui::MenuItem], ids: &mut Vec<CommandId>) {
            for item in items {
                match item {
                    gpui::MenuItem::Action { action, .. } => {
                        ids.push(id_for_action(action.as_ref()).expect("unregistered menu action"))
                    }
                    gpui::MenuItem::Submenu(menu) => collect(&menu.items, ids),
                    _ => {}
                }
            }
        }
        for &language in UiLanguage::supported() {
            let mut ids = Vec::new();
            for menu in shortcut::app_menus(language) {
                collect(&menu.items, &mut ids);
            }
            let unique = ids.iter().copied().collect::<HashSet<_>>();
            assert_eq!(unique.len(), ids.len(), "duplicate native-menu command");
            let expected = COMMAND_SPECS
                .iter()
                .filter(|spec| spec.id.is_available())
                .map(|spec| spec.id)
                .collect::<HashSet<_>>();
            assert_eq!(unique, expected);
        }
    }

    #[cfg(feature = "gpui-test-support")]
    mod dispatch {
        use super::*;
        use gpui::{AppContext, IntoElement, Render, div, prelude::*};

        #[gpui::test]
        fn command_declared_actions_have_coverage_or_an_explicit_exclusion(
            cx: &mut gpui::TestAppContext,
        ) {
            cx.update(|cx| {
                let covered = COMMAND_SPECS
                    .iter()
                    .map(|spec| spec.id.action().name())
                    .collect::<HashSet<_>>();
                let namespace = Undo.name().rsplit_once("::").unwrap().0;
                let excluded = [
                    ("Backspace", "ordinary character input"),
                    ("Delete", "ordinary character input"),
                    ("Left", "ordinary character navigation"),
                    ("Right", "ordinary character navigation"),
                    ("Up", "ordinary row navigation"),
                    ("Down", "ordinary row navigation"),
                    ("SelectLeft", "ordinary character selection"),
                    ("SelectRight", "ordinary character selection"),
                    ("SelectUp", "ordinary row selection"),
                    ("SelectDown", "ordinary row selection"),
                    ("Enter", "ordinary text input"),
                    ("InsertSoftBreak", "ordinary text input"),
                    ("Escape", "transient overlay dismissal"),
                    ("SidebarCreateFile", "selected file-tree object"),
                    ("SidebarCreateFolder", "selected file-tree object"),
                    ("SidebarRename", "selected file-tree object"),
                    ("SidebarDelete", "selected file-tree object"),
                    (
                        "NewChatTab",
                        "current-terminal shortcut; creation templates are registered separately",
                    ),
                    (
                        "CloseActiveChatTab",
                        "focused-terminal alias of close_terminal",
                    ),
                    ("ClearTerminal", "focused-terminal alias of clear_terminal"),
                    (
                        "SwitchSearchInput",
                        "focused-input alias of switch_search_input",
                    ),
                    (
                        "ToggleApplicationMenu",
                        "platform menu activation, not a document operation",
                    ),
                ];
                for name in cx
                    .all_action_names()
                    .iter()
                    .filter(|name| name.starts_with(namespace))
                {
                    if covered.contains(name) {
                        continue;
                    }
                    let short = name.rsplit("::").next().unwrap();
                    assert!(
                        excluded
                            .iter()
                            .any(|(excluded, reason)| *excluded == short && !reason.is_empty()),
                        "New action must be classified for menu/palette coverage: {name}"
                    );
                }
            });
        }

        struct Harness {
            editor: gpui::Entity<EditorView>,
            input: gpui::Entity<crate::text_input::TextInput>,
        }
        struct EditorHarness(gpui::Entity<EditorView>);
        impl Render for EditorHarness {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                self.0.clone()
            }
        }
        impl Render for Harness {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div()
                    .size_full()
                    .child(self.editor.clone())
                    .child(self.input.clone())
            }
        }

        #[gpui::test]
        fn command_registered_actions_are_wired_to_a_real_handler(cx: &mut gpui::TestAppContext) {
            let editor = cx.update(|cx| {
                cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some("# Example"), false))
            });
            let (_, cx) = cx.add_window_view(|_, _| EditorHarness(editor.clone()));
            cx.update(|window, cx| {
                window.focus(&editor.read(cx).focus_handle);
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                for spec in COMMAND_SPECS.iter().filter(|spec| spec.id.is_available()) {
                    if matches!(
                        spec.id,
                        CommandId::OpenTerminalWindow | CommandId::Extra(ExtraCommand::Quit)
                    ) {
                        continue; // Application listeners are registered in main, not this harness.
                    }
                    assert!(
                        window.is_action_available(spec.id.action().as_ref(), cx),
                        "missing handler: {}",
                        spec.id.config_key()
                    );
                }
            });
        }

        #[gpui::test]
        fn command_palette_restores_input_focus_for_copy_and_paste(cx: &mut gpui::TestAppContext) {
            let editor = cx.update(|cx| {
                cx.new(|cx| {
                    EditorView::new_with_mode(cx, true, false, Some("document unchanged"), false)
                })
            });
            let input = cx.update(|cx| {
                let styles = editor.read(cx).styles.text_input.clone();
                cx.new(|cx| {
                    let mut input = crate::text_input::TextInput::new(styles, "", cx);
                    input.set_text("field text", cx);
                    input.select_all(cx);
                    input
                })
            });
            let (_, cx) = cx.add_window_view(|_, _| Harness {
                editor: editor.clone(),
                input: input.clone(),
            });
            cx.update(|window, cx| {
                window.focus(&input.read(cx).focus_handle);
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.toggle_command_palette(window, cx);
                    editor.command_palette_pending = Some(CommandId::Extra(ExtraCommand::Copy));
                    cx.notify();
                });
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert_eq!(
                    cx.read_from_clipboard().and_then(|item| item.text()),
                    Some("field text".into())
                );
                cx.write_to_clipboard(gpui::ClipboardItem::new_string("replaced".into()));
                editor.update(cx, |editor, cx| {
                    editor.toggle_command_palette(window, cx);
                    editor.command_palette_pending = Some(CommandId::Extra(ExtraCommand::Paste));
                    cx.notify();
                });
            });
            cx.run_until_parked();
            cx.update(|_, cx| {
                assert_eq!(input.read(cx).text(), "replaced");
                assert_eq!(editor.read(cx).buffer.text(), "document unchanged");
            });
        }

        #[gpui::test]
        fn command_pin_mode_and_stale_target_are_validated(cx: &mut gpui::TestAppContext) {
            let editor = cx.update(|cx| {
                cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some("# Example"), false))
            });
            let (_, cx) = cx.add_window_view(|_, _| EditorHarness(editor.clone()));
            cx.update(|window, cx| {
                window.focus(&editor.read(cx).focus_handle);
                editor.update(cx, |editor, cx| {
                    editor.on_run_command(
                        &RunCommand {
                            id: "pin_tab".into(),
                        },
                        window,
                        cx,
                    );
                    assert!(editor.tabs[editor.active_tab_index().unwrap()].is_pinned);
                    editor.on_run_command(
                        &RunCommand {
                            id: "reading_mode".into(),
                        },
                        window,
                        cx,
                    );
                    assert!(editor.reading_mode);
                    assert!(!editor.command_state(CommandId::Bold).enabled);
                    editor.on_run_command(
                        &RunCommand {
                            id: "edit_mode".into(),
                        },
                        window,
                        cx,
                    );
                    editor.capture_command_origin(window, cx);
                    editor.command_origin.as_mut().unwrap().tab = Some("closed-source-tab".into());
                    editor.on_run_command(
                        &RunCommand {
                            id: "pin_tab".into(),
                        },
                        window,
                        cx,
                    );
                    assert!(editor.tabs[editor.active_tab_index().unwrap()].is_pinned);
                    assert!(!editor.notifications.is_empty());
                });
            });
        }

        #[gpui::test]
        fn command_states_cover_exports_read_only_search_and_tab_boundaries(
            cx: &mut gpui::TestAppContext,
        ) {
            let editor = cx.update(|cx| {
                cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some("# Example"), false))
            });
            editor.update(cx, |editor, _| {
                let export_commands = ExtraCommand::ALL
                    .iter()
                    .copied()
                    .filter(|command| command.export().is_some())
                    .collect::<Vec<_>>();
                assert_eq!(export_commands.len(), 8);
                for command in &export_commands {
                    assert!(editor.command_state(CommandId::Extra(*command)).enabled);
                }
                editor.reading_mode = true;
                assert!(!editor.command_state(CommandId::Bold).enabled);
                assert!(
                    editor
                        .command_state(CommandId::Extra(ExtraCommand::ExportPng))
                        .enabled
                );
                assert!(
                    !editor
                        .command_state(CommandId::Extra(ExtraCommand::Replace))
                        .enabled
                );
                assert!(
                    !editor
                        .command_state(CommandId::Extra(ExtraCommand::CopyImage))
                        .enabled
                );
                editor.reading_mode = false;
                editor.search_visible = true;
                assert!(
                    !editor
                        .command_state(CommandId::Extra(ExtraCommand::FindNext))
                        .enabled
                );
                assert!(
                    editor
                        .command_state(CommandId::Extra(ExtraCommand::SearchCase))
                        .enabled
                );
                assert!(
                    !editor
                        .command_state(CommandId::Extra(ExtraCommand::CloseLeft))
                        .enabled
                );
                assert!(
                    !editor
                        .command_state(CommandId::Extra(ExtraCommand::CloseSaved))
                        .enabled
                );
                let mut other = editor.tabs[0].clone();
                other.id = "command-test-second".into();
                other.is_pinned = false;
                editor.tabs.push(other);
                assert!(
                    !editor
                        .command_state(CommandId::Extra(ExtraCommand::CloseLeft))
                        .enabled
                );
                assert!(
                    editor
                        .command_state(CommandId::Extra(ExtraCommand::CloseRight))
                        .enabled
                );
                assert!(
                    editor
                        .command_state(CommandId::Extra(ExtraCommand::CloseSaved))
                        .enabled
                );
                editor.active_file_kind_cache = Some(FileKind::Image);
                for command in export_commands {
                    assert!(!editor.command_state(CommandId::Extra(command)).enabled);
                }
                assert!(
                    editor
                        .command_state(CommandId::Extra(ExtraCommand::ImageZoomIn))
                        .enabled
                );
            });
        }
    }
}
