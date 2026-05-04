use gpui::actions;

actions!(
    markdown_editor,
    [
        Undo,
        Redo,
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectAll,
        Home,
        End,
        SelectHome,
        SelectEnd,
        Copy,
        Cut,
        Paste,
        Enter,
        Tab,
        BackTab,
        ShowCharacterPalette,
        MoveWordLeft,
        MoveWordRight,
        DeleteWordLeft,
        ToggleBold,
        ToggleItalic,
        ToggleStrikethrough,
        ToggleInlineCode,
        ToggleLink,
        ToggleImage,
        ToggleQuote,
        ToggleTodo,
        ToggleUnorderedList,
        ToggleOrderedList,
        InsertCodeBlock,
        InsertHorizontalRule,
        InsertHeading1,
        InsertHeading2,
        InsertHeading3,
        InsertHeading4,
        InsertHeading5,
        InsertHeading6,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        ToggleSidebar,
        ToggleTheme,
        ToggleEditorLeftAlign,
        SelectLine,
        InsertLineBelow,
        InsertLineAbove,
        DuplicateLine,
        DeleteLine,
        MoveLineUp,
        MoveLineDown,
        Save,
        NewTab,
        Quit,
        Escape,
        ToggleOutline,
        ToggleInfoPanel,
        ToggleGitHistory,
        ToggleThemeDebugger,
        AddCursorAbove,
        AddCursorBelow,
        SelectNextOccurrence,
        SelectAllOccurrences,
        // Tab management
        CloseActiveTab,
        ReopenClosedTab,
        ActivatePreviousTab,
        ActivateNextTab,
        PinTab,
        // Search
        Find,
        FindNext,
        FindPrevious,
        DismissSearch,
        ToggleReplace,
        ReplaceNext,
        ReplaceAll,
        // Global search
        FindInFiles,
        QuickOpen,
    ]
);

pub fn toggle_wrap(
    text: &str,
    sel_start: usize,
    sel_end: usize,
    prefix: &str,
    suffix: &str,
) -> (String, usize, usize) {
    if sel_start == sel_end {
        let mut result = String::with_capacity(text.len() + prefix.len() + suffix.len());
        result.push_str(&text[..sel_start]);
        result.push_str(prefix);
        result.push_str(suffix);
        result.push_str(&text[sel_end..]);
        let cursor = sel_start + prefix.len();
        (result, cursor, cursor)
    } else {
        let selected = &text[sel_start..sel_end];
        if selected.starts_with(prefix)
            && selected.ends_with(suffix)
            && selected.len() >= prefix.len() + suffix.len()
        {
            let inner = &selected[prefix.len()..selected.len() - suffix.len()];
            let mut result = String::with_capacity(text.len());
            result.push_str(&text[..sel_start]);
            result.push_str(inner);
            result.push_str(&text[sel_end..]);
            (result, sel_start, sel_start + inner.len())
        } else {
            let mut result = String::with_capacity(text.len() + prefix.len() + suffix.len());
            result.push_str(&text[..sel_start]);
            result.push_str(prefix);
            result.push_str(selected);
            result.push_str(suffix);
            result.push_str(&text[sel_end..]);
            (result, sel_start + prefix.len(), sel_end + prefix.len())
        }
    }
}

pub fn toggle_heading(
    text: &str,
    line_start: usize,
    line_end: usize,
    level: u8,
) -> (String, usize) {
    let line = &text[line_start..line_end];
    let target_prefix = format!("{} ", "#".repeat(level as usize));

    if line.starts_with(&target_prefix) {
        let mut result = String::with_capacity(text.len());
        result.push_str(&text[..line_start]);
        result.push_str(&line[target_prefix.len()..]);
        result.push_str(&text[line_end..]);
        (result, line_start)
    } else {
        let stripped = line.trim_start_matches('#');
        let existing_hashes = line.len() - stripped.len();
        let content = if existing_hashes > 0 {
            stripped.strip_prefix(' ').unwrap_or(stripped)
        } else {
            line
        };
        let mut result = String::with_capacity(text.len() + target_prefix.len());
        result.push_str(&text[..line_start]);
        result.push_str(&target_prefix);
        result.push_str(content);
        result.push_str(&text[line_end..]);
        (result, line_start + target_prefix.len())
    }
}
