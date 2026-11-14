# Moeka

Moeka is a local-first Markdown editor built with Rust and GPUI for macOS, Windows, and Linux.

[Website](https://moeka.app) · [Download](https://github.com/MoekaProject/community/releases) · [User guide](docs/help-en.md)

![Moeka Project](https://moeka.app/images/website.png)

## Features

- **GPU-powered performance**: a native Rust + GPUI editor supporting 120Hz displays, with on-demand rendering and background indexing. Actual frame rates depend on hardware and document workload.
- **Rich Markdown, live**: Edit, Reading, and Source modes with tables, task lists, LaTeX math, Mermaid diagrams, and syntax highlighting.
- **Powerful editing**: multiple cursors, regex search, a command palette, tabs, split panes, outlines, and minimap navigation.
- **Local-first knowledge**: organize workspaces, Wiki links, and attachments with full-text search, autosave, session recovery, and persistent undo.
- **Multiple workspaces**: reorder workspace roots in one FileTree, with one workspace expanded at a time and the other roots kept above or below its scrolling file list. A shared starred-files section above all workspaces provides cross-workspace access. Switching preserves each workspace's FileTree selection, scroll position and loaded directories, as well as tabs, split panes, unsaved edits, and running terminals; workspace layouts restore on restart.
- **Agent collaboration**: the standard desktop build integrates installed Codex, Claude Code, GitHub Copilot, and Gemini CLI tools with document context.
- **Beautiful exports**: share Markdown, HTML, PDF, and PNG with document formatting, code, formulas, and diagrams.
- **A personal writing space**: expressive themes, flexible typography, multilingual UI, and a focused minimal mode.

## Build from source

Install [rustup](https://rustup.rs/), use the project's Rust toolchain, and prepare your platform:

| Platform | Build dependencies                                                                                                      |
| -------- | ----------------------------------------------------------------------------------------------------------------------- |
| macOS    | Xcode, command-line tools, macOS SDK, Metal toolchain                                                                   |
| Windows  | MSVC Build Tools, Windows SDK, `fxc.exe`                                                                                |
| Linux    | C/C++, Clang, CMake, pkg-config, and development libraries for ALSA, fontconfig, OpenSSL, Vulkan, Wayland, XCB, and XKB |

```sh
git clone https://github.com/MoekaProject/moeka.git
cd moeka
cargo run --release --locked
```

The first build downloads locked dependencies. PDF/PNG export uses system WebKit on macOS, WebView2 Runtime on Windows, and WebKitGTK on Linux.

See the [Windows](packaging/windows/README.md) and [Linux](packaging/linux/README.txt) guides for platform setup.

## Contributing

Code, documentation, translations, and bug reports are welcome. Read [AGENTS.md](AGENTS.md) for contribution and verification guidelines. Write issue and pull-request titles and descriptions in English.

## License

Moeka's original code is licensed under **GPL-3.0-or-later**; see [LICENSE](LICENSE).

Third-party code, icons, fonts, data, and trademarks retain their respective terms. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for attribution and distribution requirements.
