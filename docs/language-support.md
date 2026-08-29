# Syntax highlighting support

Moeka uses one syntax registry for code files, fenced Markdown code blocks, the language picker, and HTML/PDF export. The registry is provided by `two-face`, which packages the syntax definitions maintained by the `bat` project for `syntect`.

This extends syntect's small default set with commonly used languages and formats including TypeScript/TSX, Vue, Svelte, Dockerfile, TOML, Kotlin, Swift, Zig, WGSL, Terraform, Nix, Solidity, GraphQL, Dart, Elixir, Julia, Protobuf, QML, Rego, Typst, and VHDL.

Moeka uses syntect's pure-Rust `fancy-regex` backend. Definitions that require unsupported regular-expression features are intentionally excluded by two-face. At the time of integration this includes some definitions or variants for ARM Assembly, JavaScript (Babel), LiveScript, PowerShell, Sass, and Salt State. Unsupported or unrecognized language tokens fall back to plain text.

The syntax list is not a claim to cover every programming language. GitHub Linguist contains languages with no maintained Sublime/TextMate grammar, generated/vendor-only formats, historical languages, and prose classifications that are outside syntax highlighting. New grammars should be added to the shared registry so the editor, Markdown code blocks, exports, and language picker cannot diverge.

Representative registry coverage is checked by `extended_registry_covers_common_modern_languages` in `src/editor/helpers.rs`.
