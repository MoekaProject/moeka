# Markdown Editor — Architecture & Development Guide

## Overview

A macOS-native Markdown WYSIWYG editor built with [gpui](https://crates.io/crates/gpui) (Zed's UI framework). Renders Markdown with live syntax folding, inline previews, and a file tree sidebar.

**Stack:** Rust edition 2024 · gpui 0.2.2 · pulldown-cmark 0.13.0 · syntect 5.2

## Project Structure

```
src/
├── main.rs            # Application setup, keybindings, window creation
├── actions.rs         # 55 gpui actions (keyboard shortcuts → handler dispatch)
├── buffer.rs          # Buffer: text storage with line_starts index
├── cursor.rs          # Cursor: offset + anchor, selection helpers
├── markdown.rs        # parse_decorations(): pulldown-cmark → LineDecoration[]
├── theme.rs           # Theme struct (One Dark Pro palette), hex_to_hsla()
├── style.rs           # Styles struct: per-component style config from Theme
└── editor/
    ├── mod.rs         # EditorView struct, constants, data types, shared utils
    ├── scaling.rs     # UI scaling: scaled(), font_size_for_block(), layout calcs
    ├── tabs.rs        # Tab management + tab bar rendering
    ├── sidebar.rs     # File tree sidebar ops + rendering
    ├── fold.rs        # Section/image folding logic
    ├── menus.rs       # Context menu, format menu, slash menu
    ├── handlers.rs    # All on_* action handlers (keyboard shortcuts)
    ├── input.rs       # Mouse/scroll events, EntityInputHandler (IME)
    ├── render.rs      # Render trait impl (main UI div tree)
    ├── element.rs     # EditorElement: custom gpui Element (prepaint/paint)
    └── helpers.rs     # Standalone functions: display text, highlighting, images
```

## Key Types

### EditorView (`editor/mod.rs`)

The central model. Owns:

- `buffer: Buffer` — the text content
- `cursor: Cursor` — caret position + selection anchor
- `decorations: Vec<LineDecoration>` — per-line markdown parse results
- `theme: Theme` — active color palette
- `tabs: Vec<OpenTab>` — open file tabs
- `focus_handle: FocusHandle` — gpui focus management

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

Colors organized in 4 categories:

- **Text** (5): foreground, panel_muted, tab_active_text, accent, danger
- **Background** (13): background, panel_bg, panel_surface, ..., inline_code_bg
- **Border** (4): panel_border, tab_border, checkbox_border, blockquote_border
- **Editor** (13): cursor, selection, heading, bold, italic, ..., gutter_heading

### Color Functions

- `hsla_hex(0xRRGGBB, alpha)` — u32 hex literal → Hsla
- `hex_to_hsla("#ffffff")` — CSS hex string → Hsla (supports #RGB, #RRGGBB, #RRGGBBAA)

## Style System (`style.rs`)

`Styles::from_theme(&theme)` produces per-component style configs:

- `EditorStyles` — padding, max_width, font sizes, UI scale limits
- `SidebarStyles` — width, row_height, colors
- `TabBarStyles` — height, colors, close button size
- `CursorStyles` — color, blink timings
- `CodeBlockStyles` — bg, text padding
- `CheckboxStyles` — size, corner radius, colors
- `TableStyles` — cell padding, border color
- `GutterStyles` — fold icon size, heading color
- `ContextMenuStyles` — menu width
- `ToolbarStyles` — button size, gap, toggle button colors

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

## Coding Conventions

- All editor submodule files use `use super::*;` for shared imports from mod.rs
- Cross-module helper calls: `helpers::build_display_text()`, `helpers::pick_folder_blocking()`
- EditorView fields are `pub(crate)` for cross-module access within the editor crate
- Constants in `editor/mod.rs` are `pub(crate)` (e.g., `LINE_HEIGHT_FACTOR`, `HASH_SVG_PATH`)
- Font: "Menlo" monospace throughout; syntect "OneHalfDark" theme for code blocks
- Image loading: `resolve_local_image_path()` → `load_render_image_from_path()` → cached in `image_cache: HashMap<String, Option<Arc<RenderImage>>>`
- File/folder pickers: `osascript` commands (macOS-specific)
- Undo: manual `checkpoint()` before edits, stored in `undo_stack: Vec<EditorSnapshot>`

## Adding a New Feature

### New keyboard action

1. Add action in `actions.rs`: `actions!(markdown_editor, [MyAction])`
2. Add keybinding in `main.rs` `register_keybindings()`
3. Add handler in `editor/handlers.rs`: `pub(crate) fn on_my_action(...)`
4. Register in `editor/render.rs`: `.on_action(cx.listener(Self::on_my_action))`

### New theme color

1. Add field to `Theme` in `theme.rs` under the right category
2. Set value in `one_dark()` constructor
3. Use as `self.theme.new_color` or `theme.new_color`

### New component style

1. Add fields to the relevant struct in `style.rs` (or create a new struct)
2. Initialize in `Styles::from_theme()`
