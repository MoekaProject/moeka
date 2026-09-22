# Development guide

This guide describes local development of the standard desktop build. It is not a signing, notarization, or release-publishing guide.

## Rust toolchain

[Cargo.toml](../Cargo.toml) uses Rust edition **2024**. The current workflows install **stable** Rust; the repository does not pin a toolchain or declare a `rust-version` minimum. Edition support alone is not a verified minimum supported Rust version for the dependency graph.

Use an up-to-date stable toolchain through [rustup](https://rustup.rs/), with rustfmt and Clippy:

```sh
rustup toolchain install stable --profile minimal --component rustfmt clippy
rustup override set stable
rustc --version
cargo --version
```

Run the override command from the repository root. During preparation of this guide, the local environment reported Rust and Cargo **1.95.0** on the stable channel. That is an observed environment, not a minimum-version promise or a claim that a fresh build was verified with it.

## Platform prerequisites

All platforms need Git, Rust, a native compiler/linker, and network access for the initial Cargo dependency download. Most dependencies come from crates.io; `alacritty_terminal` is pinned to a Git revision. GPUI is supplied by the checked-in [vendor/gpui](../vendor/gpui) directory through a Cargo patch. Keep the [Cargo.lock](../Cargo.lock), vendored sources, assets, help documents, and test fixtures when preparing a source snapshot.

### macOS

Install Xcode with its macOS SDK and command-line tools, and select it as the active developer directory. The vendored GPUI build generates bindings using Clang and invokes `xcrun` to compile Metal shaders and a Metal library. Command Line Tools alone may not provide the required Metal tools. On Xcode versions with separately installed Metal tooling, install that component as well.

No Apple developer account, signing identity, provisioning profile, or notarization credentials are needed for a local Cargo build. These prerequisites do not establish a minimum supported macOS version.

### Windows

Install Visual Studio Build Tools with the C++ build tools and Windows SDK, and use a Rust MSVC toolchain matching your target architecture. A developer shell is useful for making the native tools available.

Native release builds of vendored GPUI compile HLSL with the Windows SDK's `fxc.exe`. Make it discoverable on `PATH`, or set `GPUI_FXC_PATH` to its installed location. Do not assume that another machine's SDK directory is valid on yours. If a native dependency needs Clang, install the LLVM/Clang tools as well.

The application build script embeds its icon, manifest, and UI assets; it does not require a signing certificate. For the separate macOS-to-Windows cross-build path, see [packaging/windows/README.md](../packaging/windows/README.md). Native builds do not need `cargo-xwin`.

### Linux

The package workflows currently use Ubuntu 22.04 runners for x86_64 and ARM64. Their development-library list is a useful starting point on Debian/Ubuntu, not a universal package list or a minimum-OS guarantee:

```sh
sudo apt-get update
sudo apt-get install -y \
  build-essential clang cmake pkg-config \
  libasound2-dev libfontconfig-dev libssl-dev libvulkan-dev libwayland-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-shm0-dev libxcb-xfixes0-dev \
  libxcb-xkb-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev
```

Other distributions need equivalent packages. Running the app requires a graphical session and suitable graphics drivers; a successful compiler invocation alone does not validate rendering. Packaging additionally uses `patchelf` and `zip`; neither is required just to invoke a native Cargo build. Linux graphics troubleshooting is covered in [gpu-diagnostics.md](gpu-diagnostics.md).

## Fresh clone and source snapshots

Start in a new directory rather than depending on an existing developer checkout:

```sh
git clone https://github.com/MoekaProject/moeka.git
cd moeka
cargo build --release
cargo run --release --bin moeka
```

These commands build and launch the default desktop application. Without an explicit target override, output is placed under the repository's release output directory; Windows produces an executable with the `.exe` suffix. No signing credentials, release tokens, private configuration, or pre-populated build directory should be necessary.

For a reproducibility check, record the commit, OS/architecture, and Rust/Cargo versions. Use a fresh clone or an unpacked source snapshot that includes all tracked build inputs, rather than copying ignored caches or output from another checkout. `cargo build --release --locked` and `cargo test --release --locked` can check that Cargo does not need to change the dependency lockfile. They still need to download dependencies on an uncached machine.

[build.rs](../build.rs) reads Git commit/tag metadata for the application and falls back to empty values when Git metadata is unavailable. An archive without repository metadata therefore cannot report the same revision information as a Git checkout. Archive builds and fresh-clone builds should be verified independently before describing either as reproducible.

For preparation, `node scripts/export-source-snapshot.mjs <new-directory>`
exports current working files (including non-ignored untracked files, excluding
working-tree deletions) outside the repository. The parent directory must exist;
the destination must not exist. The exporter rejects symlinks and selected
credential/cache paths and never copies Git history. It is not a secret scrubber:
review and scan the snapshot before initializing any public Git repository.
It does not push, create a commit or change visibility. See
[open-source readiness](open-source-readiness.md) for the remaining gates.

Use disposable documents or a temporary workspace for manual tests: the app's normal file editing, auto-save, and persistence behavior is active during development.

## Tests and checks

Recommended local commands:

```sh
cargo fmt --check
cargo clippy --release --all-targets
cargo build --release
cargo test --release
```

Tests are largely inline Rust test modules compiled with the application. There is no separate library target to test with `--lib`. Focused examples:

```sh
cargo test --release buffer::tests
cargo test --release cursor::tests
cargo test --release markdown::tests
cargo test --release table_ops::tests
cargo test --release editor::chat_panel::tests -- --nocapture
```

Markdown tests load the checked-in [CommonMark fixture](../tests/fixtures/commonmark-0.31.2-spec.json); retain it in a fresh clone or source snapshot. A focused filter is not a substitute for the full suite, and unit tests do not establish end-to-end GUI coverage.

The [CI configuration](../.github/workflows/ci.yml) defines lint/test jobs for macOS, Windows, and Linux using `--release --locked`; formatting is checked on macOS. Asset validation, secret scanning and offline release-safety tests run separately. These are workflow definitions, not evidence that a particular commit passed.

For changes involving shared state or interactions, manually check the affected paths: edit/undo/redo, save/reopen, pane switching and closing, search, selection, Unicode/IME input, and relevant platform behavior. Include the platforms tested and any known gaps in the pull request. Documentation-only changes normally need only content and link review.

## External file updates

The file watcher runs even without an open workspace. It reconciles the current
workspace and documents from every pane every 500ms: the workspace is recursive,
while standalone files share non-recursive parent-directory watches. Watching the
parent keeps tracking a document when an external editor replaces it by rename.
Closing or switching the workspace does not stop watches needed by open tabs.

Newly opened paths and newly registered watches request a disk check, covering
changes before registration. Canonical file events are mapped back to the paths
used by tabs, including directory aliases. Explicitly opened files remain eligible
for reload even when their directories are excluded from sidebar indexing.
Dirty tabs are never overwritten by external reloads.

Run `cargo test --release editor::file_watch::tests` for watcher registration,
workspace transitions, path aliases, clean/dirty snapshots and native write/atomic
replacement checks. The native test uses disposable files, not user documents.

## Markdown block dragging

The format handle supports click-on-release and drag-after-threshold. The interaction
follows the block handle and blue guide approach described in
[Notion's editing guide](https://www.notion.com/help/writing-and-editing-basics),
but operates on Markdown source rather than Notion's database or column model.

- Drag ranges use pulldown-cmark block boundaries plus Moeka's table, front matter,
  and generated-preview metadata. Paragraph continuations, code fences, tables,
  equations, quotes/callouts, HTML blocks and footnotes move together. List items
  carry their descendants; dragging a nested item does not carry its parent.
- An expanded heading moves on its own; a folded heading carries its hidden section.
  Multiline selections expand to complete blocks. Front matter stays at the top.
- The target block supplies the default indentation. Horizontal movement selects
  a valid parent, sibling or child indentation level, including the unindented root.
  Child indentation is offered only beneath list items. Tabs use Markdown's
  four-column stops; relative indentation and indented-code payload are preserved.
- Non-list block moves preserve separating blank lines. The move, indentation and
  separator changes form one undo transaction. A successful drop moves the caret
  to the first inserted line and flashes the inserted range.
- Ordered list moves renumber affected source/destination lists, retaining their
  explicit starting numbers and adjusting child indentation when digit widths change.
  Numbering is part of the same undo transaction; code samples are not renumbered.
- The ghost and source content both use 50% opacity. The ghost reuses a snapshot
  of visible source text layout, block decoration, tables and images at the original
  content width, without caret/selection/controls. Long blocks are clipped to the
  captured viewport. The 2px blue insertion guide ends at the content column edge and
  reflects the target indentation. Wheel scrolling continues during dragging and
  briefly takes priority over edge scrolling; guides use current prepaint geometry.
- The format toolbar uses three rows of seven controls: headings/paragraph,
  inline formats/link/image, and block formats/more. The horizontal rule is in More.
  Image format handles align with the preview fold handle; heading handle spacing
  is independently configurable in `FormatToolbarStyles`.
- Escape, leaving the source pane on release, switching documents and content edits
  cancel the gesture. New drag behavior should be exercised with split panes,
  wrapped paragraphs, nested lists, folded blocks and undo/redo in disposable files.

Focused regression tests are in `editor::line_drag::tests`. They check source
ranges, indentation, separators and undo data, but do not replace GUI verification.

## Packaging is separate from local development

The [Makefile](../Makefile) and [release workflow](../.github/workflows/release.yml) contain platform packaging and publishing steps. macOS app bundles and disk images, Windows cross-builds, and Linux portable archives have additional tooling requirements. Ordinary source contributors should start with Cargo rather than a publishing target.

Do not use `make publish`, signed-release targets, or Mac App Store packaging as a prerequisite for contributing. Official publishing is a local, maintainer-only operation guarded by distribution review; it never replaces published releases. Hosted workflows only create unsigned candidates. See [releasing](releasing.md). Public downloads remain at [MoekaProject/community releases](https://github.com/MoekaProject/community/releases).

The `mas` feature selects capability restrictions used by the Mac App Store channel: child-process features (including terminal and Git) and self-updating are disabled through [src/platform/capabilities.rs](../src/platform/capabilities.rs). Compiling with that feature alone does not create a signed, sandboxed App Store package.

## Finding your way around

See the [architecture overview](architecture.md) before adding state or moving editor logic, [vendored GPUI maintenance](vendored-gpui.md) for upstream patches, and [CONTRIBUTING.md](../CONTRIBUTING.md) for pull-request expectations. Include a minimal, sanitized reproduction with build failures; never attach credentials or a personal workspace archive.
