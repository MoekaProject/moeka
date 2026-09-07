# Moeka

A local-first Markdown editor for writing, organizing notes, and working with code. Moeka brings live Markdown rendering and a native desktop interface to macOS, Windows, and Linux.

**[Website](https://moeka.app) · [Download Moeka](https://github.com/MoekaProject/community/releases) · [User guide](docs/help-en.md)**

## What you can do

- Write Markdown with inline formatting, editable tables, task lists, and code highlighting.
- Preview LaTeX math and Mermaid diagrams alongside your text.
- Organize local files with workspaces, wiki links, an outline, and workspace search.
- Work across tabs and split panes, with Git tools and an integrated terminal in the standard desktop build.
- Create whiteboards and export documents to PDF.
- Customize themes, fonts, and the interface language.

Release packaging targets macOS (Apple silicon and Intel), Windows (ARM64 and x86_64), and Linux (ARM64 and x86_64). Check the [release notes and available downloads](https://github.com/MoekaProject/community/releases) for your platform. The Mac App Store build disables terminal/child-process features, Git commands, and the built-in updater.

## Build from source

Moeka is written in Rust using [GPUI](https://www.gpui.rs/). Install stable Rust and the [platform prerequisites](docs/development.md#platform-prerequisites), then:

```sh
git clone https://github.com/MoekaProject/moeka.git
cd moeka
cargo build --release
cargo run --release --bin moeka
```

Local builds do not require signing certificates, notarization credentials, or release tokens. See the [development guide](docs/development.md) for toolchain details and tests.

## Help and contribute

- Report bugs, request features, and ask usage questions in [MoekaProject/community](https://github.com/MoekaProject/community/issues).
- Submit code and documentation pull requests to [MoekaProject/moeka](https://github.com/MoekaProject/moeka/pulls).
- Read the [contribution guide](CONTRIBUTING.md), [architecture overview](docs/architecture.md), and [code of conduct](CODE_OF_CONDUCT.md).
- Report vulnerabilities privately using the [security policy](SECURITY.md), not a public issue.

## License and acknowledgments

Moeka's original code is licensed under the **[GNU General Public License, version 3 or later](LICENSE) (GPL-3.0-or-later)**. Third-party distribution review is tracked in [open-source readiness](docs/open-source-readiness.md).

Moeka builds on GPUI, pulldown-cmark, syntect, Alacritty's terminal library, and other open-source projects. Third-party code and assets retain their respective licenses and attribution requirements; see [third-party notices](THIRD_PARTY_NOTICES.md). The code license does not grant rights to third-party trademarks or imply their endorsement.
