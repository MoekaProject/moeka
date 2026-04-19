# Markdown Editor — Architecture & Development Guide

## Overview

A macOS-native Markdown WYSIWYG editor built with [gpui](https://crates.io/crates/gpui) (Zed's UI framework). Renders Markdown with live syntax folding, inline previews, and a file tree sidebar.

**Stack:** Rust edition 2024 · gpui 0.2.2 · pulldown-cmark 0.13.0 · syntect 5.2

## Project Structure

```
src/
├── main.rs              # Application setup, keybindings, window creation (315 lines)
├── actions.rs           # gpui actions (keyboard shortcuts → handler dispatch) (160 lines)
├── buffer.rs            # Buffer: text storage with line_starts index (140 lines)
├── cursor.rs            # Cursor: offset + anchor, selection helpers (235 lines)
├── markdown.rs          # parse_decorations(): pulldown-cmark → LineDecoration[] (750 lines)
├── text_input.rs        # Reusable text input component with IME, selection, events (1271 lines)
├── theme.rs             # ThemeKind enum, 6 color themes, hex_to_hsla() (477 lines)
├── style.rs             # Styles struct: per-component style config from Theme (1136 lines)
└── editor/
    ├── mod.rs           # EditorView struct, AppSettings, data types, shared utils (2378 lines)
    ├── scaling.rs       # UI scaling: scaled(), font_size_for_block(), layout calcs (330 lines)
    ├── tabs.rs          # Tab management + tab bar rendering (831 lines)
    ├── sidebar.rs       # File tree sidebar ops + rendering (1370 lines)
    ├── fold.rs          # Section/image folding logic (143 lines)
    ├── menus.rs         # Context menu, format menu, slash menu (1196 lines)
    ├── handlers.rs      # All on_* action handlers (keyboard shortcuts) (1269 lines)
    ├── input.rs         # Mouse/scroll events, EntityInputHandler (IME) (711 lines)
    ├── render.rs        # Render trait impl (main UI div tree) (617 lines)
    ├── element.rs       # EditorElement: custom gpui Element (prepaint/paint) (1519 lines)
    ├── helpers.rs       # Display text, highlighting, images, table rendering (1568 lines)
    ├── search.rs        # Find/replace UI within current buffer (721 lines)
    ├── quick_open.rs    # File picker (fuzzy search across workspace) (638 lines)
    ├── outline.rs       # Document outline (heading hierarchy navigation) (254 lines)
    ├── settings_panel.rs # Settings UI, font picker with directional dropdown (971 lines)
    └── workspace_search.rs # Workspace-wide search across multiple files (842 lines)
```

Total: ~19,800 lines of Rust.

## Key Types

### AppSettings (`editor/mod.rs`)

Persistent user settings, serialized to `~/.config/markdown-editor/settings.json`:

- `theme: String` — theme name (e.g. "OneDark")
- `ui_font`, `body_latin_font`, `body_cjk_font`, `code_font` — font family names
- `ui_scale: f32` — global UI scale factor
- `font_size: f32` — editor font size

### EditorView (`editor/mod.rs`)

The central model. Owns:

- `buffer: Buffer` — the text content
- `cursor: Cursor` — primary caret position + selection anchor
- `extra_cursors: Vec<Cursor>` — additional cursors for multi-cursor editing
- `decorations: Vec<LineDecoration>` — per-line markdown parse results
- `theme: Theme` — active color palette
- `theme_kind: ThemeKind` — current theme variant
- `tabs: Vec<OpenTab>` — open file tabs
- `focus_handle: FocusHandle` — gpui focus management
- `app_settings: AppSettings` — persistent user preferences

### Buffer (`buffer.rs`)

Simple text buffer with a `line_starts: Vec<usize>` index. Provides:

- `line_count()`, `line_content(n)`, `line_start(n)`, `line_end(n)`
- `offset_to_line_col()`, `line_col_to_offset()`
- `replace(range, text)` — core edit operation

### Cursor (`cursor.rs`)

Tracks `offset` (caret) and `anchor` (selection start).

- `move_to(offset)` — collapse selection
- `select_to(offset)` — extend selection
- `selection_range()` — ordered start..end
- `has_selection()` — anchor ≠ offset

### LineDecoration (`markdown.rs`)

Per-line parse output:

- `block_type: BlockType` — Normal, Heading(1-6), CodeBlock, Table, etc.
- `inline_spans: Vec<InlineSpan>` — Bold, Italic, Link, Image ranges
- `syntax_ranges: Vec<SyntaxRange>` — markdown syntax characters to hide/show
- `image_path`, `code_language`, `table_info`

## Theme System (`theme.rs`)

6 themes organized in light/dark pairs:

| Theme Kind | Label | Dark? |
|------------|-------|-------|
| `OneDark` | One Dark | ✓ |
| `OneLight` | One Light | |
| `AyuDark` | Ayu Dark | ✓ |
| `AyuLight` | Ayu Light | |
| `Milky` | Milky | |
| `MilkyDark` | Milky Dark | ✓ |

`ThemeKind` enum provides:
- `build(self) -> Theme` — constructs the full color palette
- `is_dark(self) -> bool` — used for theme-conditional rendering
- `paired_appearance(self) -> ThemeKind` — returns the light↔dark counterpart
- `dark_themes()` / `light_themes()` — static slices for each group

### Font Configuration

Fonts are user-configurable via `AppSettings` (persisted to `~/.config/markdown-editor/settings.json`):

- **Body Latin**: default "Menlo" — for Latin/ASCII text
- **Body CJK**: default "PingFang SC" — for CJK characters (GPUI font fallback does NOT work for CJK)
- **Code**: default "Menlo" — for code blocks and inline code
- **UI Font**: default ".SystemUIFont" — for sidebar, menus, tabs

Per-character CJK/Latin font routing is required for ALL non-code text. The `build_text_runs_for_display()` function in `helpers.rs` handles this splitting.

### Naming Convention: `{category}_{depth}`

| Category  | Depth examples                                                                                                                              |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `text_`   | `primary`, `secondary`, `bright`, `accent`, `danger`                                                                                        |
| `bg_`     | `primary`, `secondary`, `elevated`, `sunken`, `hover`, `selected`, `active`, `bar`, `tab`, `tab_hover`, `tab_active`, `code`, `code_inline` |
| `border_` | `primary`, `secondary`, `tertiary`, `decorative`                                                                                            |

**Editor semantic colors** (cursor, selection, heading, bold, italic, link, list_marker, syntax_marker, strikethrough, inline_code, gutter_heading) are kept as-is — they describe editor-specific rendering semantics, not UI-layer colors.

### Color Functions

- `hsla_hex(0xRRGGBB, alpha)` — u32 hex literal → Hsla
- `hex_to_hsla("#ffffff")` — CSS hex string → Hsla (supports #RGB, #RRGGBB, #RRGGBBAA)

### Adding a theme color

1. Add field to `Theme` following the `{category}_{depth}` naming convention
2. Set value in all 6 theme constructors (`one_dark()`, `one_light()`, `ayu_dark()`, `ayu_light()`, `milky()`, `milky_dark()`)
3. Update corresponding style fields in `Styles::from_theme()` if needed

## Style System (`style.rs`)

`Styles::from_theme(&theme)` produces per-component style configs with **all** visual properties extracted as configurable fields.

### Naming Convention: `功能_位置_状态_样式_单位`

Field names follow this pattern (omit segments that don't apply):

- **功能 (function)**: what component/element — e.g. `editor`, `sidebar`, `tab`, `toolbar`, `cursor`, `menu`
- **位置 (position)**: where — e.g. `left`, `right`, `top`, `bottom`, `header`, `item`, `container`
- **状态 (state)**: when — e.g. `hover`, `active`, `selected`, `dirty`, `error`
- **样式 (style)**: what visual property — e.g. `bg`, `text`, `border`, `font_size`, `padding`, `margin`, `width`, `height`, `opacity`, `radius`, `shadow`, `gap`, `brightness`
- **单位 (unit)**: `px`, `factor`, `ms`, `max`, `min` (omit for colors and dimensionless values)

Examples:

```
sidebar_item_hover_bg           // 功能=sidebar, 位置=item, 状态=hover, 样式=bg
editor_padding_left_px          // 功能=editor, 位置=left, 样式=padding, 单位=px
tab_close_hover_opacity         // 功能=tab, 位置=close, 状态=hover, 样式=opacity
cursor_blink_interval_ms        // 功能=cursor, 样式=blink_interval, 单位=ms
toolbar_primary_btn_size_px     // 功能=toolbar, 位置=primary_btn, 样式=size, 单位=px
```

### State styles

Each component that has interactive states (hover/active/selected) has its states represented as separate fields with the state name in the field:

```rust
// Example: sidebar item states
sidebar_item_bg: Hsla,
sidebar_item_hover_bg: Hsla,
sidebar_item_selected_bg: Hsla,
sidebar_item_active_bg: Hsla,
```

### Style structs

| Struct               | Scope                                                                              |
| -------------------- | ---------------------------------------------------------------------------------- |
| `WindowStyles`       | Window-level bg                                                                    |
| `EditorStyles`       | Main editor pane: padding, max-width, font sizes, scale                            |
| `SidebarStyles`      | File tree: logo, header, items, edit row, error, empty, resize handle (~40 fields) |
| `TabBarStyles`       | Tab bar: container, items, icons, labels, close btn, new-tab btn (~30 fields)      |
| `ToolbarStyles`      | Toolbar buttons: sizes, gap, offset, opacity, colors                               |
| `CursorStyles`       | Cursor: color, width, blink timing, opacity                                        |
| `CodeBlockStyles`    | Code block bg, text padding                                                        |
| `SelectionStyles`    | Selection highlight bg                                                             |
| `CheckboxStyles`     | Checkbox size, radius, colors                                                      |
| `TableStyles`        | Table cell padding, border                                                         |
| `GutterStyles`       | Fold icon, heading hash sizes (h1–h4), blockquote marker                           |
| `ImagePreviewStyles` | Inline image max-height factor                                                     |
| `HrStyles`           | Horizontal rule height, color                                                      |
| `ImeStyles`          | IME composition underline                                                          |
| `ContextMenuStyles`  | Menu container, items, separator, shortcut labels, per-kind widths                 |

## Rendering Pipeline

1. **Render trait** (`render.rs`): Builds the gpui div tree (sidebar + tab bar + editor pane + context menu backdrop)
2. **EditorElement** (`element.rs`): Custom `Element` impl for the main text area
   - `request_layout()` — requests full-size layout
   - `prepaint()` — ~1000 lines: iterates all buffer lines, shapes text via `shape_text()`, computes cursor/selection quads, gutter decorations, image previews, table borders
   - `paint()` — paints all computed quads, SVG icons, shaped text, images

### Display Text Pipeline (`helpers.rs`)

For each line:

1. `build_display_text()` — folds syntax markers not near cursor → display string + offset mappings
2. `build_text_runs_for_display()` — maps buffer styling (bold/italic/link colors) to display positions → `Vec<TextRun>`
3. `shape_text()` / `shape_line()` — gpui text shaping with word wrap
4. Offset mappings (`display_to_buffer`, `buffer_to_display`) enable cursor ↔ display position conversion

## Event Flow

1. Keyboard: keybinding → gpui action → `on_action(cx.listener(Self::on_*))` in render.rs → handler in `handlers.rs`
2. Mouse: `on_mouse_down/move/up` in render.rs → handler in `input.rs` → `offset_for_position()` hit-test
3. IME: `EntityInputHandler` in `input.rs` — `replace_text_in_range()`, `replace_and_mark_text_in_range()`

## Key Subsystems

### Slash Menu (`menus.rs`)

Typing `/` at line start triggers a popup with 4 items: Todo, CodeBlock, Link, Image. Items are filtered by typed text and navigated with arrow keys. The slash menu has a separate render path from the right-click context menu (`is_slash_menu` flag) — it displays items in a fixed order matching keyboard navigation.

### Multi-Cursor (`input.rs`, `handlers.rs`)

Cmd+click adds cursors at clicked positions. `extra_cursors: Vec<Cursor>` stores additional cursors beyond the primary. All text operations (insert, delete, format) apply to all cursors. Cursors are not guaranteed sorted — sort by offset before multi-cursor edits that shift offsets.

### Search & Replace (`search.rs`)

Buffer-local find/replace with case sensitivity, whole word, and regex toggles. Match highlighting painted as background quads in `element.rs`. Active match scrolled into view.

### Workspace Search (`workspace_search.rs`)

Cross-file search using `grep` or Rust `regex` crate. Results displayed in a sidebar panel grouped by file. Clicking a result opens the file and jumps to the match.

### Outline (`outline.rs`)

Collects heading hierarchy from `decorations`. Displays as a navigable tree in the sidebar. Clicking a heading scrolls the editor to that line.

### Settings Panel (`settings_panel.rs`)

Font picker with directional dropdown (auto-detects above/below based on available space). Theme selector, font size, UI scale controls. Changes persisted via `AppSettings::save()`.

### Heading Syntax Folding (`markdown.rs`, `element.rs`, `helpers.rs`)

Markdown heading prefixes (e.g. `## `) are collapsed when the cursor is not near them:
- `markdown.rs` emits `SyntaxRange { range: 0..prefix_len, trigger_range: 0..prefix_len }`
- `element.rs` computes `cursor_col` — when the cursor is on a heading line with text selected, returns `Some(0)` to force expansion
- `helpers.rs` `build_display_text()` hides syntax characters when `cursor_col` falls outside `trigger_range`

## Coding Conventions

- All editor submodule files use `use super::*;` for shared imports from mod.rs
- Cross-module helper calls: `helpers::build_display_text()`, `helpers::pick_folder_blocking()`
- EditorView fields are `pub(crate)` for cross-module access within the editor crate
- Constants in `editor/mod.rs` are `pub(crate)` — only SVG paths and `KNOWLEDGE_BASE_ROOT` remain; all numeric style constants live in `style.rs`
- CJK/Latin font routing: `build_text_runs_for_display()` splits text into CJK vs Latin segments with separate font families; GPUI font fallback does NOT work for CJK
- syntect "OneHalfDark" / "OneHalfLight" themes for code block syntax highlighting (selected based on `ThemeKind::is_dark()`)
- Image loading: `resolve_local_image_path()` → `load_render_image_from_path()` → cached in `image_cache: HashMap<String, Option<Arc<RenderImage>>>`
- File/folder pickers: `osascript` commands (macOS-specific)
- Undo: manual `checkpoint()` before edits, stored in `undo_stack: Vec<EditorSnapshot>`
- Heading syntax folding: `SyntaxRange { trigger_range: 0..prefix_len }` — headings expand when cursor column falls within trigger_range

## Adding a New Feature

### New keyboard action

1. Add action in `actions.rs`: `actions!(markdown_editor, [MyAction])`
2. Add keybinding in `main.rs` `register_keybindings()`
3. Add handler in `editor/handlers.rs`: `pub(crate) fn on_my_action(...)`
4. Register in `editor/render.rs`: `.on_action(cx.listener(Self::on_my_action))`

### New theme color

1. Add field to `Theme` in `theme.rs` following `{category}_{depth}` naming (e.g. `bg_tooltip`, `text_muted`)
2. Set value in all 6 theme constructors
3. Wire into the relevant style struct in `Styles::from_theme()`

### New component style

1. Add fields to the relevant struct in `style.rs` following `功能_位置_状态_样式_单位` naming
2. Initialize in `Styles::from_theme()` from theme colors and/or numeric defaults
3. Reference via `self.styles.{component}.{field}` in rendering code
