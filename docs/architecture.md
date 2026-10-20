# Architecture overview

Moeka is a Rust desktop application built around GPUI. This document describes the current implementation and identifies possible incremental boundaries; it does not describe a completed layered rewrite.

## Entry points and existing core logic

[src/main.rs](../src/main.rs) wires application startup, actions/keybindings, platform integration, and windows. [src/actions.rs](../src/actions.rs) declares application actions. [Cargo.toml](../Cargo.toml) defines the main executable and patches GPUI to the checked-in vendor copy.

There is already useful, GPUI-independent core logic. It is compiled as modules of the application rather than published as a separate domain library:

| Module                                              | Current responsibility                                                                                                                        |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| [src/buffer.rs](../src/buffer.rs)                   | `Buffer`: contiguous text storage, chunked/lazily shifted line indexes, coordinate conversion, edit recording, and grapheme-boundary helpers. |
| [src/cursor.rs](../src/cursor.rs)                   | `Cursor`: caret and selection anchor, movement, and grapheme-aware selection using `Buffer`.                                                  |
| [src/markdown.rs](../src/markdown.rs)               | Markdown parsing with pulldown-cmark and extensions; line decorations, block/inline metadata, links, and table syntax.                        |
| [src/markdown_format.rs](../src/markdown_format.rs) | Markdown source formatting and preservation rules.                                                                                            |
| [src/table_ops.rs](../src/table_ops.rs)             | Table/front-matter models and source transformations, including row/column operations and serialization.                                      |

Keep these transformations independent of window state where possible. Byte offsets, UTF-16 offsets, grapheme boundaries, source lines, and display coordinates are different units; conversions must preserve those distinctions.

## The editor is still the main integration point

[src/editor/mod.rs](../src/editor/mod.rs) defines `EditorView` and coordinates document state, caches, parsing updates, tabs, panels, background tasks, and application behavior. Many feature files extend the same `EditorView`; a separate file does not necessarily imply independent state ownership.

Three concentrations matter when planning a change:

- **[src/editor/mod.rs](../src/editor/mod.rs):** broad state ownership and orchestration, including incremental/lazy Markdown decoration paths and cache invalidation.
- **[src/editor/element.rs](../src/editor/element.rs):** `EditorElement` prepaint/paint, visible text and geometry, selections, decorations, tables, previews, and hit areas. It uses pane-specific state through the shared editor entity.
- **[src/editor/helpers.rs](../src/editor/helpers.rs):** display-text construction, syntax highlighting, table measurement/layout, preview sizing/rendering helpers, wrapping, and decoration geometry. Some helpers are pure calculations; others depend on GPUI, styles, or rendering resources.

[src/editor/text_layout.rs](../src/editor/text_layout.rs) contains four extracted pure numeric functions: `table_cell_content_offset`, `effective_wrap_width_for_letter_spacing`, `effective_monospace_wrap_width_for_letter_spacing`, and `code_block_soft_wrap_width`. Their implementations are unchanged, and [src/editor/helpers.rs](../src/editor/helpers.rs) reexports them with the existing `pub(crate)` API so callers and existing tests retain their paths. Other helpers remain in place. The new module's unit tests cover zero spacing, supplied character advance, narrow widths, and table alignment offsets; they do not exercise GPUI text shaping or establish visual wrapping/clipping correctness.

[src/editor/render.rs](../src/editor/render.rs) and [src/editor/render_pane.rs](../src/editor/render_pane.rs) assemble the UI. [src/editor/handlers.rs](../src/editor/handlers.rs) and [src/editor/input.rs](../src/editor/input.rs) route actions and input; [src/editor/input_entity.rs](../src/editor/input_entity.rs), [src/editor/input_ime.rs](../src/editor/input_ime.rs), [src/editor/input_table.rs](../src/editor/input_table.rs), and [src/editor/input_wrap.rs](../src/editor/input_wrap.rs) handle more specialized input and mapping concerns.

At a high level, input changes the source buffer and cursor, editor orchestration updates or invalidates parse/layout caches, and render/prepaint/paint produce visible content and hit geometry. Later pointer and IME operations use that geometry to map back to source positions. Not every edit triggers a full parse or layout, so a change must account for both cached and rebuilt paths.

## Mermaid preview pipeline

- [helpers.rs](../src/editor/helpers.rs) delegates Mermaid source rendering to the Ondine compiler and applies the current editor theme to its SVG through Ondine's CSS custom properties. The SVG canvas remains transparent; the same generated SVG is used for editor previews and exports.
- [mermaid_worker.rs](../src/editor/mermaid_worker.rs) owns the editor's async Mermaid build/raster queue: **one active job and at most eight queued jobs per window**, shared across panes. Its state stays on the root `EditorView`, never in the mirrored `PaneState`. Jobs snapshot source/rendering inputs and validate epoch, pane/tab, workspace/path and configuration before installation. Invalidating a running job drops its result, not its physical CPU slot; new work waits for its completion. Existing image/SVG/overlay caches and scroll compensation still belong to editor orchestration.

See [Mermaid development invariants](development.md#mermaid-preview-pipeline) for renderer and worker verification details.

## Split panes: state-swap invariants

Read [src/editor/pane.rs](../src/editor/pane.rs) before changing document state.

- Outside a temporary swap, the root document fields of `EditorView` hold the focused pane's live state.
- Other panes store their state in `PaneState`. The focused pane's stored slot is a detached placeholder, not a second live copy.
- `with_pane()` temporarily swaps another pane into the root fields so existing editing/rendering code can operate on it. `focus_pane()` transfers state permanently when focus changes.
- The `pane_state!` list generates the mirrored state, detached defaults, and swap routine. A new per-document field on `EditorView` must also be added there; otherwise state can leak between panes.

Important review risks:

1. Classify new state as per-document/per-pane or shared application state. Do not mirror shared state indiscriminately.
2. Do not nest swaps across different panes or call `focus_pane()` inside a temporary swap. The current code checks these invariants with debug assertions, which are not a release-mode safety mechanism.
3. Pair `begin_pane_swap()` and `end_pane_swap()` on every normal control-flow path. Custom element prepaint/paint uses these lower-level primitives. The closure helper restores state after the closure returns; it is not an unwind-restoration guard.
4. Use `current_pane_id()` when you mean the state currently in the root fields; it may differ from `focused_pane_id` during rendering or event routing.
5. Background callbacks must retain the intended pane identity and account for closure/focus changes. `with_pane_if_alive()` exists to avoid applying delayed work from a closed pane to another pane. Also preserve the relevant task cancellation, epoch, and cache-validity checks.

Changes here should exercise at least two panes with different documents, focus switches, tab moves, closing a pane with pending work, and per-pane selection/scroll/undo behavior.

## Persistence, services, and platform integration

- [src/editor/tabs.rs](../src/editor/tabs.rs), [src/editor/types.rs](../src/editor/types.rs), and [src/editor/session_store.rs](../src/editor/session_store.rs) coordinate tab data and session restoration.
- [src/editor/history_store.rs](../src/editor/history_store.rs), [src/editor/auto_save.rs](../src/editor/auto_save.rs), and [src/editor/temp_store.rs](../src/editor/temp_store.rs) cover history and saving/recovery concerns.
- [src/editor/git_ops.rs](../src/editor/git_ops.rs), [src/editor/git_panel.rs](../src/editor/git_panel.rs), [src/editor/chat_panel.rs](../src/editor/chat_panel.rs), [src/editor/search_index.rs](../src/editor/search_index.rs), and [src/editor/workspace_search.rs](../src/editor/workspace_search.rs) integrate Git, terminal UI, indexing, and workspace search with the editor. These are not yet a uniform service layer.
- [src/platform](../src/platform) provides adapters for clipboard, dialogs, fonts, keys, paths, child processes, terminals, window behavior, and platform-specific integration. Editor code still calls these adapters directly in places.
- [src/platform/capabilities.rs](../src/platform/capabilities.rs) gates child-process and self-update functionality for the Mac App Store feature; respect these gates when adding integrations.
- [src/theme.rs](../src/theme.rs), [src/style.rs](../src/style.rs), and [src/i18n.rs](../src/i18n.rs) supply theme, style, and localization infrastructure.

[build.rs](../build.rs) generates embedded asset data and Windows resources. [vendor/gpui](../vendor/gpui) is an active local dependency, not disposable build output. Preserve its license notices and check platform-specific behavior when updating it.

## Incremental target boundaries — not completed work

Prefer a small extraction with unchanged behavior and focused tests over an all-at-once rewrite. Useful directions include:

1. **Core transformations:** extend the existing buffer/cursor/Markdown/table modules instead of re-creating them inside UI handlers. Move additional pure algorithms only when their inputs and outputs can be explicit.
2. **State ownership:** group cohesive per-document, view, and shared application state while preserving pane swapping and persistence compatibility. Independent pane entities would require a deliberate migration, not just moving struct fields.
3. **Layout versus painting:** extract testable text/wrap/table/preview calculations from helpers, then pass explicit results to GPUI rendering. Preserve source-to-display mapping and cache invalidation contracts.
4. **Effects versus decisions:** separate file, Git, terminal, and search effects from presentation where useful, with explicit result delivery, cancellation, and pane identity. No universal service interface exists today.

Do not introduce a new abstraction solely to reduce a file's line count. State the behavior being preserved, identify all callers and shared state, and verify the affected interactions. Existing inline unit tests and the CommonMark fixture provide starting points; they do not replace GUI or cross-platform checks. See [development.md](development.md) for commands and verification guidance.
