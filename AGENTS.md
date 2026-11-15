# Moeka development guide

## Contributing

- Use English for issue and pull-request titles, descriptions, shared developer documentation, and explanatory comments. Preserve application translations, user content, technical identifiers, and multilingual test data.
- Use the [issue form](https://github.com/MoekaProject/moeka/issues/new/choose) for bug fixes, features, and improvements; submit patches through [pull requests](https://github.com/MoekaProject/moeka/pulls). Link related community reports by full URL.
- Discuss substantial changes first. Include the version or commit, platform, minimal reproduction, and expected result; submit focused patches against `main` with relevant tests and documentation.
- Follow the [code of conduct](CODE_OF_CONDUCT.md). Report vulnerabilities through the [private security process](SECURITY.md).
- Contribute material you have the right to share under the applicable license. Retain dependency provenance and attribution; protect credentials, private documents, paths, and recovery data.

## Development principles

- Prefer complete, focused changes that reuse existing implementations. Keep implementation details in code and regression tests.
- Keep the README focused on major capabilities; describe individual fixes and interaction details in tests, pull requests, or release notes.
- Preserve others' pending changes. Commits, pushes, history changes, and publication require appropriate authorization.
- [docs](docs) contains application help only; modify it only when explicitly requested. Use [Simplified Chinese help](docs/help-zh-cn.md) as the reference when synchronizing translations, preserving its scope.

## Build and test

Use the project's Rust toolchain and `--release --locked` for Cargo builds and tests. Platform prerequisites are in the [README](README.md#build-from-source).

```sh
cargo fmt --all -- --check
cargo clippy --release --locked --all-targets
cargo test --release --locked
cargo build --release --locked
git diff --check
```

- Start with the smallest relevant test selection. Documentation-only changes need content, link, and existing documentation checks.
- Cross-compilation checks include `--all-targets` to compile platform-specific tests as well as application code. Test queue/deadline ordering directly instead of relying on narrow wall-clock margins.
- Keep tracked text LF-normalized through `.gitattributes`; use explicit CRLF fixtures for line-ending tests. Compare paths using `Path`, and use portable error fixtures rather than assuming identical OS error codes.
- Enable `gpui-test-support` for UI event tests. Isolate `XDG_CONFIG_HOME` on Unix/macOS or `APPDATA`/`LOCALAPPDATA` on Windows in both runners and spawned test processes, and use disposable workspaces.
- Validate input, IME, panes, persistence, and exports on the target platform. Report commands, results, and unverified behavior; compilation and test-platform checks do not replace native UI testing.
- For local profiling on Unix, use `MOEKA_PROFILE=1 MOEKA_PROFILE_CPU=1 MOEKA_PROFILE_INTERVAL_MS=250 cargo run --release --locked` with disposable documents.
- Run `node scripts/check-assets.mjs` for icon changes; `ruby scripts/test-check-community.rb` and `ruby scripts/check-community.rb` for contributor metadata/docs; `make test-publish-release` and, when relevant, `make test-publish-homebrew` for release scripts. Never test with a real publish command.
- Contributor CI and unsigned candidate builds stay read-only and secret-free. Official releases use the manually dispatched `publish.yml` workflow on `main` in `MoekaProject/moeka`. Only its isolated credential preflight and reusable macOS signing job use `release-signing`; only the publish and Homebrew jobs have contents write access via the repository's `GITHUB_TOKEN`, for the source tag/Release and Cask-only main update respectively. No cross-repository release token is required. Pin third-party actions to commit SHAs and disable persistent checkout credentials.
- CI runs the full `gpui-test-support` suite once, alongside separate default-feature production builds. Build caches share explicit targets and compatible runner/SDK families; only trusted main runs write caches. Never restore build caches in signing or publication jobs. Use the always-reporting `CI result` check for branch protection; documentation/Cask-only changes skip native jobs, not metadata, release-safety or secret checks.

## Architecture

- **Ownership**: [editor](src/editor) manages documents and interaction; [ui](src/ui) provides shared controls; [style.rs](src/style.rs) owns component rules; [theme.rs](src/theme.rs) owns semantic colors; [platform](src/platform) wraps system capabilities.
- **Pane state**: document fields belong in `pane_state!`; window-shared fields stay separate. Root fields hold the active pane's live state. Pair temporary swaps, avoid nested cross-pane swaps, and keep focus stable inside them.
- **Async identity**: validate the originating document, pane/tab, version, epoch, configuration, and cancellation before applying results. Closed panes must not redirect results elsewhere.
- **Geometry**: distinguish UTF-8 bytes, UTF-16, graphemes, source lines, and display coordinates. Layout, painting, hit testing, selection, and IME share mappings; full, incremental, and lazy parsing agree.
- **Editing**: one operation is one undo transaction; plan multi-cursor edits against the same original text. Preserve read-only behavior, structural content, literal composition/paste, and the viewport while the caret is visible.
- **Caches**: propagate edit, theme, font, and zoom invalidation across affected panes and tabs. Previews and minimaps reuse accepted rendering resources within visibility and memory budgets.
- **Consistency**: menus and the palette share validation and dispatch. Controls use semantic styles, readable contrast, and system motion preferences; presentation animation stays separate from document updates. Preserve settings/session compatibility.
- **Localization**: use catalogs for UI and first-party runtime messages, including background/native helpers. Preserve external causes and document-controlled text; business logic uses stable identities rather than translated strings.
- **Export**: HTML, PDF, and PNG share content and layout. Inspect text, images, formulas, and diagrams on each target platform.

## Data and performance safety

- Route filesystem probes, I/O, and watcher lifetime operations through bounded background tasks, preserving foreground capacity.
- A timeout may leave a write running. Keep dirty buffers and recovery data; retry only after establishing the original operation's state.
- Preserve content on query failures; only confirmed `NotFound` means deletion. Report the operation, path, and original cause.
- Use native Trash on local or unknown volumes; permanent deletion requires a confirmed network volume. Reject dangerous targets, operate on symlinks themselves, and retain the destination if source removal fails after copying.
- Saves and async loads check identity and revision to prevent stale overwrites. Honor platform capabilities and `mas` restrictions.

## Release

Publish only when explicitly requested, through [GitHub Actions](.github/workflows/publish.yml). `make publish TAG=vX.Y.Z` dispatches that workflow; it never builds, signs, or uploads locally.

1. Confirm `main` tracks `origin/main`, inspect pending changes, and complete relevant validation. Increment PATCH in `Cargo.toml` and the root `Cargo.lock` package without updating dependencies.
2. Before source publication, review the exact history, branches, tags, LFS, pending files, and retained artifacts for private data; rotate exposed credentials. Verify external build access, fork-safe CI, repository rules, private reporting, and [license/corresponding-source obligations](THIRD_PARTY_NOTICES.md#distribution-review). Visibility and history publication need explicit approval.
3. Commit authorized release changes and English release notes under `packaging/release-notes/`, then push to origin. Confirm a clean worktree and matching local/tracking/remote `main`; keep the dispatch SHA fixed during builds.
4. Run `make publish TAG=vX.Y.Z` (or dispatch `publish.yml` directly) and monitor the run to completion: dispatch success is not publication success. All platform builds, macOS signing/notarization/stapling, and uploads run in Actions. See [macOS signing](packaging/macos/README.md) and [CI release operations](packaging/release-notes/README.md). Use `make publish-beta TAG=vX.Y.Z` only for requested prereleases.
5. Publish binaries to `MoekaProject/moeka`, using one source tag at the immutable build SHA. Do not update community or the legacy Tap as part of ordinary publication. Publish serially, verifying all eight attachments: two macOS DMGs and dual-architecture ZIPs for macOS, Windows, and Linux. Verify each artifact's version, dispatch SHA, run ID, size and SHA-256 before creating refs or uploading; download and compare byte-for-byte when a remote digest is unavailable. Windows/Linux packages retain their existing unsigned status until platform signing credentials are explicitly configured.
6. Set `GH_PAGER=cat` and specify the repository in GitHub commands. Add English `Features`, `Fixes`, and `Improvements` notes; verify draft/prerelease flags, version, attachments, and stable Tap. Return the Release link and Chinese notes.

### Homebrew

Stable publication updates [Casks/moeka.rb](Casks/moeka.rb) in this repository in the separate `homebrew` job. Check with `make homebrew-check TAG=vX.Y.Z`; retry only that failed Actions job, not the release. Validate both public signed DMGs, update only Cask version/digests, and preserve other settings and unrelated source commits. Reject downgrades or changed digests for an existing version; revalidate after concurrent updates. The Cask commit follows publication and must never move the source tag.

### Recovery

Preserve signed artifacts, drafts, and tags. Reconcile remote state before retrying writes; network errors do not establish absence. Resume the failed step, never overwrite attachments or move published tags. Retry transient reads at most three times; stop on permission, integrity, or unidentified outcomes. Resume known notarization submissions and complete staple, signature, and Gatekeeper checks. Published versions, including `v0.1.12`, must not be republished. Release-note or Tap failures need only their own retry.

If GitHub HTTPS transport fails but authenticated SSH returns the same repository refs, use a per-process Git URL rewrite to SSH. Preserve global configuration and all repository, permission, and integrity checks.
