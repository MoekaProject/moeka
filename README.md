# Moeka

A local-first Markdown editor for writing, organizing notes, and working with code. Moeka brings live Markdown rendering and a native desktop interface to macOS, Windows, and Linux.

**[Website](https://moeka.app) · [Download Moeka](https://github.com/MoekaProject/community/releases) · [User guide](docs/help-en.md)**

## What you can do

- Write Markdown with inline formatting, editable tables, task lists, and code highlighting.
- Preview LaTeX math and Mermaid diagrams alongside your text.
- Organize local files with workspaces, wiki links, an outline, and workspace search.
- Work across tabs and split panes, with an integrated terminal in the standard desktop build.
- Export documents as Markdown, HTML, or PDF; PNG export is available on macOS.
- Customize themes, fonts, interface language, corner shape, and outer shadows.

Release packaging targets macOS (Apple silicon and Intel), Windows (ARM64 and x86_64), and Linux (ARM64 and x86_64). Check the [release notes and available downloads](https://github.com/MoekaProject/community/releases) for your platform. The Mac App Store build disables terminal/child-process features and the built-in updater.

PDF rendering is platform-dependent: macOS uses WebKit, while Windows and Linux
currently use a text-oriented PDF backend rather than the full visual preview.

Whiteboard editing is no longer available. Existing `.moeka` files open as plain text without conversion. If a restored file cannot be verified, unsaved content is kept as a detached draft rather than overwriting the file.

Built-in Git tools and diff editors have been removed. Use an external Git client
for version control; ordinary file-change detection remains available.

## Build from source

Moeka is written in Rust using [GPUI](https://www.gpui.rs/). Install [rustup](https://rustup.rs/); `rust-toolchain.toml` selects the project's Rust toolchain, rustfmt, and Clippy. A first build also needs network access to fetch the locked dependencies.

Platform prerequisites:

- **macOS:** Xcode, its command-line tools, the macOS SDK, and the Metal toolchain.
- **Windows:** Visual Studio Build Tools with MSVC, the Windows SDK, and `fxc.exe`. See the [Windows build notes](packaging/windows/README.md) for cross-compiling from macOS.
- **Linux:** a C/C++ toolchain, Clang, CMake, pkg-config, and the ALSA, fontconfig, OpenSSL, Vulkan, Wayland, XCB, and XKB development libraries. See the [Linux build notes](packaging/linux/README.txt) for distribution-specific package lists.

Then clone and build:

```sh
git clone https://github.com/MoekaProject/moeka.git
cd moeka
cargo build --release --locked
cargo run --release --locked
```

`moeka` is the default executable, so `cargo run --release` does not need `--bin moeka`. Developer tools such as `language-support-report` must be selected explicitly with `--bin`.

Run `cargo test --release --locked` for the test suite. Local builds do not require signing certificates, notarization credentials, or release tokens. See the [contribution guide](CONTRIBUTING.md) and [development essentials](AGENTS.md#build-and-test) for checks and platform limitations.

## Local data

Settings and session data are stored under `$XDG_CONFIG_HOME/moeka` (or `~/.config/moeka`) on Unix, including macOS, and `%APPDATA%\moeka` on Windows. The application also keeps a platform-specific cache, including undo history. These files can contain document contents and paths; review and redact them before sharing diagnostics. Running a development build uses the same user data as the installed application, so use a separate user account or configuration environment when testing potentially destructive changes.

## Source layout

| Area | Responsibility |
| --- | --- |
| [Text and Markdown core](src/markdown.rs) | Parsing, text transformations, buffer and cursor behavior |
| [Editor](src/editor) | Document/pane ownership, input, rendering, navigation and persistence |
| [Export](src/editor/export.rs) | Portable document conversion and export orchestration, with an isolated [macOS backend](src/editor/export/macos.rs) |
| [UI components](src/ui) | Shared controls; appearance is resolved by [styles](src/style.rs) and [themes](src/theme.rs) |
| [Platform adapters](src/platform) | Filesystem workers, desktop APIs, clipboard, terminal and window integration |
| [Website](site) | Independent Nuxt landing page |

The editor uses a shared GPUI view with per-pane state swapping. Its module files
are not independent services; read the [ownership and geometry invariants](AGENTS.md#architecture)
before changing cross-pane behavior.

## Automation and releases

[PR CI](.github/workflows/ci.yml) builds, lints, and tests the desktop application on
macOS, Windows, and Linux without signing credentials. The separate
[candidate workflow](.github/workflows/release.yml) is manually triggered and
uploads unsigned build artifacts only. Official signing, notarization and
publication remain in the [maintainer-operated local release flow](AGENTS.md#release);
pushing a version tag does not publish an official release through Actions.

## Help and contribute

- Report bugs, request features, and ask usage questions in [MoekaProject/community](https://github.com/MoekaProject/community/issues).
- Submit code and documentation pull requests to [MoekaProject/moeka](https://github.com/MoekaProject/moeka/pulls).
- Read the [contribution guide](CONTRIBUTING.md), [architecture overview](AGENTS.md#architecture), and [code of conduct](CODE_OF_CONDUCT.md).
- Report vulnerabilities privately using the [security policy](SECURITY.md), not a public issue.

## License and acknowledgments

Moeka's original code is licensed under the **[GNU General Public License, version 3 or later](LICENSE) (GPL-3.0-or-later)**. See the [distribution review requirements](AGENTS.md#dependencies-and-distribution) before publishing.

Moeka builds on GPUI, pulldown-cmark, syntect, Alacritty's terminal library, and other open-source projects. Third-party code and assets retain their respective licenses and attribution requirements; see [third-party notices](THIRD_PARTY_NOTICES.md). That inventory identifies outstanding provenance and distribution reviews; it is not a declaration that all materials have been cleared. The code license does not grant rights to third-party trademarks or imply their endorsement.
