Please write the pull-request title and description in English. Keep original diagnostic output and reproduction data unchanged.

## Summary

What changes, and why?

- User-visible behavior, if any:
- Scope and compatibility considerations:

## Related community issue

Link the full MoekaProject/community issue URL, a source issue, or explain why no issue is needed.
Do not use a bare issue number for another repository.

## Verification

List commands run and results, plus any relevant manual checks. State explicitly what was not tested. For documentation-only changes, a content/link review is normally enough.

- OS and architecture:
- Commands and results:
- Manual checks, if applicable:
- Not tested / known limitations:

For Rust changes, use release mode and the committed lockfile:
`cargo fmt --all -- --check`, `cargo clippy --release --locked --all-targets`,
`cargo build --release --locked`, and the smallest relevant `cargo test --release --locked` selection.
Include a result for each command; do not list commands as evidence without running them.
For UI/event changes, include applicable `gpui-test-support` tests and distinguish them from native desktop checks.

## Review checklist

- [ ] My pull-request title and description are in English.
- [ ] The change is focused and includes relevant tests or documentation, or explains why they are not needed.
- [ ] I considered affected platforms, capability gates, and pane/shared-state behavior where applicable.
- [ ] I have the right to contribute this material under the applicable terms, including GPL-3.0-or-later for original code contributions; third-party notices and attribution are preserved.
- [ ] No credentials, private documents, or sensitive logs are included.
- [ ] Shared development documentation and new explanatory code comments are in English. Application translations and multilingual regression fixtures are preserved.
- [ ] Changes to dependencies, grammars, assets, or vendored code include their origin and license information, if applicable.

Do not submit vulnerability details in a public pull request. Follow the [security policy](https://github.com/MoekaProject/moeka/blob/main/SECURITY.md) instead.
