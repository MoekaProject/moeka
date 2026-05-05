# Markdown Editor — Architecture & Development Guide

## Overview

A macOS-native Markdown WYSIWYG editor built with [gpui](https://crates.io/crates/gpui) (Zed's UI framework). Renders Markdown with live syntax folding, inline previews, and a file tree sidebar.

**Stack:** Rust edition 2024 · gpui 0.2.2 · pulldown-cmark 0.13.0 · syntect 5.2

## Project Structure

```
src/
├── main.rs              # Application setup, keybindings, window creation
├── actions.rs           # gpui actions (keyboard shortcuts → handler dispatch)
├── buffer.rs            # Buffer: text storage with line_starts index
├── cursor.rs            # Cursor: offset + anchor, selection helpers
├── markdown.rs          # parse_decorations(): pulldown-cmark → LineDecoration[]
├── text_input.rs        # Reusable text input component with IME, selection, events
├── theme.rs             # ThemeKind enum, 6 color themes, hex_to_hsla()
├── style.rs             # Styles struct: per-component style config from Theme
└── editor/
    ├── mod.rs           # EditorView struct, shared state wiring, module re-exports
    ├── app_settings.rs  # Persistent settings load/save + ThemeKind mapping
    ├── assets.rs        # Shared SVG/icon asset path constants
    ├── types.rs         # Shared editor/tab/sidebar data types
    ├── scaling.rs       # UI scaling: scaled(), font_size_for_block(), layout calcs
    ├── tabs.rs          # Tab management + tab bar rendering + right-panel toggles
    ├── sidebar_render.rs # Sidebar render tree and render-time interaction nodes
    ├── sidebar_ops.rs   # Sidebar selection, drag/drop targeting, move logic
    ├── sidebar_files.rs # Sidebar rename/create/delete/paste + workspace switching
    ├── sidebar_nav.rs   # Sidebar keyboard navigation + focused-item actions
    ├── sidebar_state.rs # Sidebar git state, reveal, drag hover/autoscroll coordination
    ├── sidebar_tree.rs  # Sidebar tree sorting, expand/collapse, item collection/cache
    ├── sidebar_ui.rs    # Sidebar menu wiring, edit-input styles, shared render helpers
    ├── fold.rs          # Section/image folding logic
    ├── menus.rs         # Context menu, format menu, slash menu
    ├── handlers.rs      # All on_* action handlers (keyboard shortcuts)
    ├── input.rs         # Mouse/scroll events, pointer hit-testing, shared display helpers, remaining control flow
    ├── input_entity.rs  # EntityInputHandler + Focusable impls and IME-visible range/bounds conversion
    ├── input_ime.rs     # Delimiter pairing, IME composition helpers, code-block autocomplete
    ├── input_table.rs   # Pure table hit-testing and visual/source row mapping helpers
    ├── input_wrap.rs    # Wrap-boundary cursor helpers + UTF-16/UTF-8 conversion
    ├── render.rs        # Render trait impl (main UI div tree)
    ├── element.rs       # EditorElement: custom gpui Element (prepaint/paint)
    ├── helpers.rs       # Display text, highlighting, images, table rendering
    ├── info_panel.rs    # Outline / Info / Git history utility panels + cached data
    ├── search.rs        # Find/replace UI within current buffer
    ├── quick_open.rs    # File picker (fuzzy search across workspace)
    ├── outline.rs       # Document outline (heading hierarchy navigation)
    ├── settings_panel.rs # Settings UI, font picker, shared numeric steppers
    └── workspace_search.rs # Workspace-wide search across multiple files
```

## Key Types

### AppSettings (`editor/app_settings.rs`)

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

### Shared Editor Types (`editor/types.rs`)

Reusable editor model types are defined outside `mod.rs` and re-exported back through it:

- `EditorSnapshot` — persisted text/cursor/scroll snapshot for tabs and undo history
- `OpenTab`, `TabKind`, `FileKind` — tab/session model shared by tabs, session restore, info panels
- `RightUtilityPanel` — right-side panel selector (`Outline`, `Info`, `GitHistory`)
- `GitDiffState`, `SidebarItem`, `SidebarGitStatus` — shared sidebar and diff presentation types

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

| Theme Kind  | Label      | Dark? |
| ----------- | ---------- | ----- |
| `OneDark`   | One Dark   | ✓     |
| `OneLight`  | One Light  |       |
| `AyuDark`   | Ayu Dark   | ✓     |
| `AyuLight`  | Ayu Light  |       |
| `Milky`     | Milky      |       |
| `MilkyDark` | Milky Dark | ✓     |

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

1. **Render trait** (`render.rs`): Builds the gpui div tree (sidebar + tab bar + utility panel strip + editor pane + overlays)
2. **EditorElement** (`element.rs`): Custom `Element` impl for the main text area
   - `request_layout()` — requests full-size layout
   - `prepaint()` — ~1000 lines: iterates all buffer lines, shapes text via `shape_text()`, computes cursor/selection quads, gutter decorations, image previews, table borders
   - `paint()` — paints all computed quads, SVG icons, shaped text, images

### Right Utility Panels

- `tabs.rs` owns the tab-strip actions that toggle the right utility panels.
- `render.rs` places the active utility panel below the tab bar, not beside it.
- `info_panel.rs` renders the panel bodies for `Outline`, `Info`, and `GitHistory`.
- Any layout math that reserves right-side space must account for the currently visible utility panel width, not just outline state.

### Display Text Pipeline (`helpers.rs`)

For each line:

1. `build_display_text()` — folds syntax markers not near cursor → display string + offset mappings
2. `build_text_runs_for_display()` — maps buffer styling (bold/italic/link colors) to display positions → `Vec<TextRun>`
3. `shape_text()` / `shape_line()` — gpui text shaping with word wrap
4. Offset mappings (`display_to_buffer`, `buffer_to_display`) enable cursor ↔ display position conversion

## Event Flow

1. Keyboard: keybinding → gpui action → `on_action(cx.listener(Self::on_*))` in render.rs → handler in `handlers.rs`
2. Mouse: `on_mouse_down/move/up` in render.rs → handler in `input.rs` → `offset_for_position()` hit-test
3. IME: macOS text input enters through `EntityInputHandler` in `input_entity.rs`, which delegates shared offset/display helpers in `input.rs`

## Key Subsystems

### Slash Menu (`menus.rs`)

Typing `/` at line start triggers a popup with 4 items: Todo, CodeBlock, Link, Image. Items are filtered by typed text and navigated with arrow keys. The slash menu has a separate render path from the right-click context menu (`is_slash_menu` flag) — it displays items in a fixed order matching keyboard navigation.

### Multi-Cursor (`input.rs`, `handlers.rs`)

Cmd+click adds cursors at clicked positions. `extra_cursors: Vec<Cursor>` stores additional cursors beyond the primary. All text operations (insert, delete, format) apply to all cursors. Cursors are not guaranteed sorted — sort by offset before multi-cursor edits that shift offsets.

`input.rs` should stay focused on event/control flow. Pure table geometry and wrap/UTF conversion helpers belong in `input_table.rs` and `input_wrap.rs` so they can be validated independently.

### Search & Replace (`search.rs`)

Buffer-local find/replace with case sensitivity, whole word, and regex toggles. Match highlighting painted as background quads in `element.rs`. Active match scrolled into view.

### Workspace Search (`workspace_search.rs`)

Cross-file search using `grep` or Rust `regex` crate. Results displayed in a sidebar panel grouped by file. Clicking a result opens the file and jumps to the match.

### Outline (`outline.rs`)

Collects heading hierarchy from `decorations`. Displays as a navigable tree in the sidebar. Clicking a heading scrolls the editor to that line.

### Settings Panel (`settings_panel.rs`)

Font picker with directional dropdown (auto-detects above/below based on available space). Theme selector, UI scale controls, line height and letter spacing controls. Inline numeric steppers share one TextInput styling/sync helper so value clamping, persistence, and UI refresh stay consistent across controls.

### Info / Git History Panels (`info_panel.rs`)

- Expensive panel data must be cached outside render.
- Use the existing invalidation entry points (`invalidate_info_panel_cache()`, `invalidate_git_history_cache()`, `invalidate_active_file_panel_caches()`) when file state, git state, or active tab context changes.
- Do not add polling, git commands, or filesystem work directly inside render methods.

### Heading Syntax Folding (`markdown.rs`, `element.rs`, `helpers.rs`)

Markdown heading prefixes (e.g. `## `) are collapsed when the cursor is not near them:

- `markdown.rs` emits `SyntaxRange { range: 0..prefix_len, trigger_range: 0..prefix_len }`
- `element.rs` computes `cursor_col` — when the cursor is on a heading line with text selected, returns `Some(0)` to force expansion
- `helpers.rs` `build_display_text()` hides syntax characters when `cursor_col` falls outside `trigger_range`

## Coding Conventions

- `editor/mod.rs` is the re-export boundary for shared editor modules; prefer moving pure settings/constants/types out first before splitting behavior-heavy files.
- Editor submodules may use `use super::*;`, but shared definitions that fan out broadly should live in dedicated modules like `app_settings.rs`, `assets.rs`, or `types.rs`.
- For sidebar work, keep rendering in `sidebar_render.rs`, selection/drag logic in `sidebar_ops.rs`, filesystem/workspace actions in `sidebar_files.rs`, keyboard navigation in `sidebar_nav.rs`, reveal/drag/git state in `sidebar_state.rs`, tree state/sorting in `sidebar_tree.rs`, and shared menu/UI helpers in `sidebar_ui.rs`.
- For input work, keep `EntityInputHandler` and `Focusable` in `input_entity.rs`, delimiter and IME composition helpers in `input_ime.rs`, reusable table helpers in `input_table.rs`, and wrap-boundary or UTF conversion helpers in `input_wrap.rs`; reserve `input.rs` for pointer/selection control flow plus shared display-hit-testing helpers used by the entity bridge.
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
