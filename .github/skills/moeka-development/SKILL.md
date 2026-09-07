---
name: moeka-development
description: "Use when developing Moeka's Rust/GPUI editor, investigating input/layout bugs, or changing document and pane state."
---

# Moeka development

Read [architecture](../../../docs/architecture.md) and
[development](../../../docs/development.md) before editing unfamiliar modules.

- Use `cargo build --release` for local compilation and `cargo test --release`
  for tests. Keep the lockfile and vendored GPUI changes intentional.
- Preserve UTF-8/grapheme boundaries, undo/redo transactions, IME composition,
  pane state ownership and asynchronous UI task cancellation.
- Reuse shared text geometry for rendering, selection and hit testing. A numeric
  width test alone does not prove the final UI avoids clipping.
- Keep OS calls in platform adapters where practical and filesystem/network IO
  off the UI thread. Do not bypass error handling or read-only protections.
- After icon path changes run `node scripts/check-assets.mjs`.
- Do not put credentials, personal sessions or private paths into fixtures.
- Do not publish, rewrite history or change repository visibility as part of
  a code fix. Release policy is in [releasing](../../../docs/releasing.md).
