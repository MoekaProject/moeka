# Icon assets

## Organization

The 44 SVG files formerly at the root of the icon tree now live in five
semantic groups. This is a path-only reorganization: SVG bytes, artwork,
embedded metadata and Rust symbol names are preserved. The groups describe
UI roles, **not authorship, license families or ownership**.

| Group      | Count | Role                                                                           |
| ---------- | ----: | ------------------------------------------------------------------------------ |
| navigation |    11 | Shared directional, disclosure, confirmation and search controls               |
| editor     |     8 | Editor alignment, folding, comments, heading and outline controls              |
| workspace  |    14 | Workspace panels, file browsing, assistant, information and external resources |
| appearance |     8 | Theme choices and appearance/settings entry points                             |
| branding   |     3 | Existing logo artwork and light/dark full-logo variants                        |

Existing app, file, filetree, format, git, menu, preview, search, whiteboard
and window subdirectories are unchanged. For example, the workspace-level
whiteboard icon is separate from the drawing tools in the existing whiteboard
directory. Shared controls may be used outside their owning group.

### Move inventory

Each link is a new location. Except for the assistant-search icon noted below,
the basename is unchanged from its former location at the icon-tree root.

- **navigation (11):** [arrow-down-circle](../assets/icons/navigation/arrow-down-circle.svg),
  [arrow-down](../assets/icons/navigation/arrow-down.svg),
  [arrow-left](../assets/icons/navigation/arrow-left.svg),
  [arrow-right](../assets/icons/navigation/arrow-right.svg),
  [arrow-up-circle](../assets/icons/navigation/arrow-up-circle.svg),
  [arrow-up](../assets/icons/navigation/arrow-up.svg),
  [back](../assets/icons/navigation/back.svg),
  [check](../assets/icons/navigation/check.svg),
  [expand](../assets/icons/navigation/expand.svg),
  [right](../assets/icons/navigation/right.svg),
  [search](../assets/icons/navigation/search.svg).
- **editor (8):** [align-left-mini](../assets/icons/editor/align-left-mini.svg),
  [comment](../assets/icons/editor/comment.svg),
  [editor-align-left](../assets/icons/editor/editor-align-left.svg),
  [fold-expanded](../assets/icons/editor/fold-expanded.svg),
  [fold](../assets/icons/editor/fold.svg),
  [hash](../assets/icons/editor/hash.svg),
  [outline-toggle-mini](../assets/icons/editor/outline-toggle-mini.svg),
  [outline-toggle](../assets/icons/editor/outline-toggle.svg).
- **workspace (14):** [chat](../assets/icons/workspace/chat.svg),
  [doc](../assets/icons/workspace/doc.svg),
  [file-list-toggle](../assets/icons/workspace/file-list-toggle.svg),
  [file-tree-toggle](../assets/icons/workspace/file-tree-toggle.svg),
  [filetree](../assets/icons/workspace/filetree.svg),
  [history](../assets/icons/workspace/history.svg),
  [info](../assets/icons/workspace/info.svg),
  [open](../assets/icons/workspace/open.svg),
  [search-assistant](../assets/icons/workspace/search-assistant.svg),
  [share](../assets/icons/workspace/share.svg),
  [sidebar-toggle-mini](../assets/icons/workspace/sidebar-toggle-mini.svg),
  [sidebar-toggle](../assets/icons/workspace/sidebar-toggle.svg),
  [site](../assets/icons/workspace/site.svg),
  [whiteboard](../assets/icons/workspace/whiteboard.svg).
- **appearance (8):** [flower](../assets/icons/appearance/flower.svg),
  [mountain](../assets/icons/appearance/mountain.svg),
  [setting](../assets/icons/appearance/setting.svg),
  [theme-auto-mini](../assets/icons/appearance/theme-auto-mini.svg),
  [theme-dark-mini](../assets/icons/appearance/theme-dark-mini.svg),
  [theme-dark](../assets/icons/appearance/theme-dark.svg),
  [theme-light-mini](../assets/icons/appearance/theme-light-mini.svg),
  [theme-light](../assets/icons/appearance/theme-light.svg).
- **branding (3):** [logo-full-dark](../assets/icons/branding/logo-full-dark.svg),
  [logo-full-light](../assets/icons/branding/logo-full-light.svg),
  [logo](../assets/icons/branding/logo.svg).

### Naming and variants

The former basename `search2` becomes `search-assistant`. Its existing SVG
title is `search_2_ai_fill`, which supports an AI/assistant-search description
rather than an unexplained numeric suffix. This title is a naming clue only:
it is **not evidence of a particular author, icon library or license**. The
`SEARCH2_SVG_PATH` constant is intentionally retained to avoid a symbol refactor.
Other basenames, including ambiguous ones such as `right`, `doc`, `flower`
and `mountain`, are retained rather than given speculative new meanings.

Treat `mini` assets as optical variants for small UI controls, **not duplicates
to deduplicate or replace by scaling another SVG**. Preserve their geometry,
view boxes and intended stroke/spacing differences. Likewise, preserve the
light/dark logo and theme variants. No artwork normalization, optimization,
metadata removal or license-header rewriting is part of this change.

## References and verification

Central constants remain in [the editor asset registry](../src/editor/assets.rs).
Direct string paths in source files are updated in place. The embedding
assertions in [the application entry point](../src/main.rs) already use
`LOGO_SVG_PATH` and an unchanged filetree asset, so they need no rewrite.
The build-time asset collector recursively embeds the asset tree and requires
no changes for the new directories.

Run `node scripts/check-assets.mjs` from the repository root. The
[checker](../scripts/check-assets.mjs) also resolves the project root from its
own location, so it works from other working directories. It uses only Node
standard-library modules, is read-only and does not invoke Cargo.

It scans text files under the source directory and the build script for literal
icon paths, including source-relative include paths and Rust raw strings.
Comments and character literals are ignored. Missing literal files, root-level
SVG leftovers and scan errors produce exit status 1.

This is a conservative lexical check, not a Rust parser or runtime test:

- Directory/prefix literals are reported for manual review, not mistaken for
  missing SVGs. The existing `TOOL_ICON` whiteboard prefix is one such case.
- Interpolation, escapes, glob patterns and non-path strings containing an
  icon reference are reported as unresolved instead of guessed.
- Joins, `format!`, `concat!`, variable paths and computed children are not
  evaluated. In particular, references assembled without a literal icon-tree
  prefix cannot be enumerated by this check. Independently complete literal
  paths within expressions are still checked for existence.
- Binary inputs are reported as skipped. The scanner is Rust-oriented; it
  does not validate source syntax, rendered appearance, platform packaging,
  runtime loading or license compliance. A static pass is not full coverage.

## Source rights: pending, not a blanket GPL declaration

**Provenance and redistribution rights remain pending verification. Unknown
provenance is a release blocker for any distribution containing that asset.**
Moving files does not establish permission. Do not assume all icons are GPL
merely because the application's source is GPL-licensed. Individual artwork,
third-party icons and brand marks may have different terms; this document
does not assign or assert unverified licenses or authorship.

Before a release, record verifiable evidence per asset or demonstrably covered
asset family: original source URL and revision, author/rights holder, exact
license text/version, required notices/attribution, modifications and any
redistribution restrictions. For original artwork, obtain a confirmed rights
statement rather than infer ownership from its name. If evidence cannot be
obtained, resolve the blocker with a separately authorized removal or a
rights-cleared replacement; do not silently relabel it as GPL.

Preserve existing notices, SVG titles and metadata. Preserve trademarks and
brand identities, including third-party marks in the unchanged menu assets:
copyright permission does not automatically grant trademark rights or imply
endorsement. This reorganization neither changes those marks nor grants new
rights to use them.
