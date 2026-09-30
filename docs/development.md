# Development guide

Moeka is a Rust/GPUI desktop Markdown editor. This guide collects the development and maintenance rules that are easy to miss when changing the editor. For ownership and pane-state details, read [architecture.md](architecture.md); for the official release procedure, read [releasing.md](releasing.md).

## Toolchain and local builds

- Use an up-to-date stable Rust toolchain with rustfmt and Clippy. The crate uses Rust edition 2024; there is no documented minimum supported Rust version. Keep `Cargo.lock`, `vendor/gpui`, assets and test fixtures with the checkout. `alacritty_terminal` is pinned to a Git revision.
- **Use the release profile for every Cargo command that supports `--release`**: `cargo build --release`, `cargo run --release --bin moeka`, `cargo test --release`, `cargo clippy --release`, and cross-target checks. `cargo fmt` does not accept `--release`; do not add it to formatting commands. Prefer `--locked` for validation and release builds, so a check cannot silently change the lockfile.
- A normal local build does not require Apple signing, notarization or GitHub release credentials. Publishing is a separate maintainer operation; never use `make publish` as a routine build or test command.

```sh
cargo fmt --all -- --check
cargo clippy --release --locked --all-targets
cargo build --release --locked
cargo test --release --locked
cargo run --release --locked --bin moeka
```

### Platform prerequisites

- **macOS:** Install Xcode, its macOS SDK and command-line tools, and select the active developer directory. GPUI generates Clang bindings and compiles Metal shaders; on Xcode versions with separately installed Metal tooling, install that component too. Signing/notarization is only needed for distribution.
- **Windows:** Install Visual Studio C++ Build Tools and the Windows SDK with a matching Rust MSVC toolchain. The vendored GPUI release build needs SDK `fxc.exe` on `PATH` or via `GPUI_FXC_PATH`. Native builds do not need `cargo-xwin`; see [Windows cross-build notes](../packaging/windows/README.md) for the macOS cross-build path.
- **Linux:** Install a compiler, Clang, CMake, pkg-config and the relevant audio, font, TLS, Vulkan, Wayland and X11 development libraries. The package workflow uses Ubuntu 22.04; other distributions need equivalent packages. Running the app requires a graphical session and suitable graphics drivers. See [GPU diagnostics](gpu-diagnostics.md). Packaging needs additional tools such as `patchelf` and `zip`.

Use a disposable workspace for manual testing: the app's normal editing, auto-save and session persistence are active. For a fresh-clone check, record the commit, OS/architecture and toolchain, and build from tracked sources rather than copying ignored caches. An exported source snapshot lacks Git revision metadata; review and scan it separately before considering any public distribution. See [open-source readiness](open-source-readiness.md).

## Where changes belong

- `src/buffer.rs`, `src/cursor.rs`, `src/markdown.rs` and `src/table_ops.rs` own source coordinates, parsing and pure text/table transformations. Byte offsets, UTF-16 offsets, grapheme positions, source lines and display coordinates are **not interchangeable**.
- `src/editor/mod.rs` owns `EditorView`, document state, parsing and cache orchestration; `element.rs` and `helpers.rs` measure, prepaint and render visible lines, tables and previews. `input.rs` routes pointer, wheel and keyboard input. Hit areas are generated during layout and must match the rendered geometry.
- Split panes share `EditorView` through pane-state swapping. Add per-document fields to the mirrored state in `src/editor/pane.rs`; shared app state must not be mirrored. Async callbacks must validate the originating pane/tab and cancellation or epoch state before applying results. See [architecture.md](architecture.md#split-panes-state-swap-invariants).
- Preserve undo transaction boundaries, source-to-display mappings, focus and caret behavior, and incremental **and** full-parse paths. A numerical layout test does not prove that the GUI behaves correctly.

## Editor interaction invariants

### Formatting and tables

- The formatting toolbar outside the content is available without a settings toggle; do not restore the old `format_toolbar_visible` preference or an inline floating formatting toolbar. Legacy saved values of that preference are ignored. Formatting controls and the table handle must not obscure the editing caret or steal a drag gesture.
- A table cell and a row/column handle right-click render through the **same table-menu path**, with the same ordering and width. Their action targets differ: a cell uses its rectangular selection when the clicked cell belongs to it; a handle uses its row/column segment selection. Preserve copy/cut, insert/overwrite paste, clearing, alignment, insertion and deletion on both paths. Front Matter supports only actions appropriate to its key/value structure; do not route its cell contents through Markdown-table serialization. Reject unsupported nested or comment rows rather than rewriting them incorrectly.
- On multi-cell selection, **insert above/left relative to the first selected cell and below/right relative to the last**. Handle multi-selection uses its first/last selected segment on the relevant axis. Clearing cell contents must not turn into deletion of an entire row or column; structural deletion remains a separate action. Keep source-table mode, selection and undo/redo consistent when editing pipes, alignment rows or whole cells.
- A handle click selects a row or column; Shift extends a contiguous range and Cmd/Ctrl toggles segments. Hold-and-drag reorders selected segments. Test right-clicks on selected and unselected segments, rectangular cell selections, header cells, Front Matter, noncontiguous selections and destructive operations in split panes.

### Outline, scrolling and generated previews

- Outline navigation must scroll to the **measured heading position**, not just a source-line estimate: lazy chart/math rendering and line wrapping can change measured heights. Allow visible lazy previews to settle before confirming navigation. User scrolling or a newer navigation cancels the old animation/confirmation; a preview refresh must not cancel the active outline navigation.
- The active outline item and outline rail should agree and track the section at the top of the editor viewport, with a short clicked-heading override while the caret remains on that heading. Editing body text without changing heading syntax must not make the indicator flip merely because the caret moved. Invalidate the outline index after lazy-document edits, not just after obvious heading edits. Exercise long documents with many charts, headings, folds and different scroll heights.
- Lazy Mermaid/math previews keep the previous image as a layout placeholder during refresh. When their measured height changes above the viewport, prepaint compensates for the difference. Wheel input cancels stale pending preview-reveal and click-anchor restoration; never replay an older scroll offset after a newer user scroll. Check for vertical jitter while scrolling through multiple lazy previews.
- Block math is enlarged relative to its former size by 1.5×; inline math stays constrained to the line height. Mermaid retains its general text size, with larger **sequence diagram** labels/messages. Do not globally enlarge all diagram text. Fullscreen Mermaid initially occupies roughly 80% of the padded viewport; rendering and pan clamps use the same inset, and the chart is clipped to that area. Clicking a non-button location closes fullscreen **only when it was a click**; dragging pans without closing it. Buttons still perform their own actions. Recheck fullscreen over tabs, sidebars and split panes.

### Other input and background work

- Fenced-code input, selection, IME, cursor reveal and wrapping share source/display mapping; do not reset scrolling to the caret on unrelated preview refreshes. Explicit wheel scrolling takes priority over pending typing-scroll restoration.
- The file watcher covers open documents even without a workspace and tracks parent directories so atomic external replacement remains visible. Never overwrite dirty tabs with an external reload. Background file operations should not block GPUI; late results must be checked against the current pane/tab before modifying contents.
- Block dragging operates on Markdown blocks, including folded sections, nested lists, code fences, tables, Front Matter and generated previews. Preserve indentation, list renumbering and blank-line separators in one undo transaction. Escape, document switches and content edits cancel the gesture. Exercise dragging after scrolling and across wrapped lines; see `editor::line_drag::tests`.

## Tests and manual verification

Run the narrowest relevant tests first, then the broader release suite. Examples:

```sh
cargo test --release --locked table
cargo test --release --locked editor::outline::tests
cargo test --release --locked generated_math
cargo test --release --locked generated_mermaid
cargo test --release --locked editor::preview_overlay::tests
cargo test --release --locked editor::line_drag::tests
cargo test --release --locked editor::file_watch::tests
make test-publish-release
```

`cargo fmt` is the exception to the `--release` rule. The native filesystem-watcher integration test can time out on some machines; report that explicitly if it does. Skipping just that named test for a local diagnostic run is **not** equivalent to a clean full-suite pass. Unit tests do not substitute for GUI mouse, IME, split-pane, long-document, platform and save/reopen checks. Include failures and untested paths in the handoff.

GitHub Actions definitions are useful references, but **Actions are neither the official publishing entry point nor a technical gate enforced by `make publish`**. Do not infer local build or release status from an Actions run, and do not claim CI passed unless that exact commit actually passed. Use local release-mode checks and the explicit publishing preflight below. This does not waive human distribution/licensing review.

## Official publishing: local and deliberate

Only maintainers publish binaries, from the macOS signing machine, to `MoekaProject/community`. Application source and its tag stay in the private `MoekaProject/moeka` repository. The public community tag refers to *community's own* `main` commit; never push the application's private Git history to the public repository. **Never republish or modify `v0.1.12`.** Read [releasing.md](releasing.md) for signing, notarization, artifacts, required human licensing/source review and safe retries. The unchecked items in [open-source-readiness.md](open-source-readiness.md) are not automatically cleared by a successful build or publish command.

1. Review the release contents and version. Update `Cargo.toml` **and** the `moeka` entry in `Cargo.lock`. Run local release-mode validation and complete the relevant human review.
2. Commit the intended changes and synchronize `main` with `origin/main` through the normal process. Publishing never commits or pushes `main` for you. Verify the destination repository and push URL before pushing.
3. Run `make publish-check TAG=vX.Y.Z`. It is read-only and requires a completely clean tree, a version-matching tag, `main` tracking the current live `origin/main`, required distribution files, and authenticated release-repository write access.
4. When publishing a **new** release, run `make publish vX.Y.Z` (or `make publish-beta vX.Y.Z` for a prerelease). The command performs signing/notarization, cross-platform packaging, safe tag/draft handling and asset verification. It can take a long time and changes remote state; do not invoke it to inspect release status. A published release is immutable to this helper. If a maintainer already ran the command, **verify instead of rerunning blindly**:

```sh
git ls-remote origin refs/tags/vX.Y.Z
gh release view vX.Y.Z -R MoekaProject/community
```

A successful local `make publish-check`, a version bump, a claim that the command was run, or a draft is **not** proof of a published release: verify the public release and its assets. On failure, preserve the original `dist` files and inspect any existing tags/draft before retrying; never force-push, delete or overwrite a released tag or asset to make a check pass. GitHub Actions results are not a prerequisite in the `make publish` implementation; follow the human review and retry requirements in [releasing.md](releasing.md).
