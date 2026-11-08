---
name: moeka-development
description: "Use when developing Moeka's Rust/GPUI editor, investigating input/layout bugs, or changing document and pane state."
---

# Moeka development

Read the [development essentials](../../../AGENTS.md), especially architecture
and editor invariants, before editing unfamiliar modules.

- Use `cargo build --release --locked` for local compilation and `cargo test --release --locked`
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
  a code fix. Follow the [release policy](../../../AGENTS.md#release).
