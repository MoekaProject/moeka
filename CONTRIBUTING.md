# Contributing to Moeka

Contributions to code, documentation, translations, accessibility, and bug reports are welcome. English is the default language for shared documentation and technical discussion, but contributions and questions in other languages are welcome too.

Please follow the [code of conduct](CODE_OF_CONDUCT.md).

## Where to contribute

- **Bugs, feature requests, and usage questions:** [MoekaProject/community issues](https://github.com/MoekaProject/community/issues). Search for an existing report first.
- **Code and documentation pull requests:** [MoekaProject/moeka](https://github.com/MoekaProject/moeka/pulls). Link the community issue by its full URL so cross-repository references are unambiguous.
- **Security vulnerabilities:** follow [SECURITY.md](SECURITY.md); do not disclose details in a public issue or pull request.

For a bug report, include the Moeka version or source commit, operating system and architecture, reproduction steps, and expected versus actual behavior. A small, non-sensitive sample document is often more useful than an entire workspace. Remove private paths, document contents, tokens, and personal information from logs and screenshots.

For a substantial feature or architectural change, discuss the intended behavior in a community issue before investing in a large patch.

## Working on a change

1. Fork the source repository and create a focused branch.
2. Follow the [development guide](docs/development.md) for prerequisites and local builds. No signing or publishing credentials are needed.
3. Read the [architecture overview](docs/architecture.md), especially the pane-state invariants when changing editor state.
4. Keep the patch focused. Avoid unrelated formatting, generated files, or dependency updates.
5. Add or update tests for changed behavior and documentation for user-visible changes.
6. Open a pull request against the source repository's `main` branch. Explain the motivation, implementation, and verification performed.

## Verification

Use release-mode builds and tests for local development:

```sh
cargo fmt --check
cargo build --release
cargo test --release
```

For Rust changes, also consider `cargo clippy --release --all-targets`. The [development guide](docs/development.md#tests-and-checks) explains focused tests and the current workflow configuration.

Documentation-only changes normally need a content and link review, not a build. For behavior changes, report the commands you ran and their results; explicitly list checks or platforms you could not test. UI, IME, pane state, persistence, and platform changes may also need manual testing. Do not claim an untested platform works or assume that automated tests cover the interaction.

## Code and dependency guidelines

- Match the surrounding Rust style and use rustfmt. Prefer small, reviewable changes over broad rewrites.
- Keep text transformations independent of GPUI where practical; reuse existing buffer, cursor, Markdown, and table helpers.
- Preserve grapheme boundaries and be explicit when converting UTF-8 byte offsets, UTF-16 positions, and visual coordinates.
- Audit pane-state mirroring, cache invalidation, and asynchronous callbacks when adding editor state.
- Explain why a dependency is needed and retain its provenance and license notices. Changes to vendored GPUI should document the upstream behavior and the local reason for diverging.
- Never commit credentials, signing material, personal configuration, build output, or private example documents.

## Licensing contributions

Submit original code contributions under Moeka's planned **GPL-3.0-or-later** license, and only contribute material you have the right to share under the applicable terms. The full project license text is being added separately for the open-source release.

Third-party material retains its own license: identify its origin and preserve copyright, license, and attribution notices. Do not relabel third-party code or assets as your own or assume the project's license overrides theirs.
