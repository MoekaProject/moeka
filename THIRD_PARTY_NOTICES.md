# Third-party notices and distribution review

Moeka's original code is licensed under GPL-3.0-or-later (see LICENSE).
Third-party code, data, fonts, icons and trademarks retain their own terms.
This inventory is a review record, not a substitute for their license texts
or a declaration that all bundled materials have been cleared for distribution.

## Rust dependencies

Cargo.lock pins the dependency graph. Generate metadata with
`cargo metadata --locked --format-version 1` and inspect the features used on
each supported platform. Most declarations offer MIT, Apache-2.0, BSD, ISC or
similar permissive terms. Components declaring MPL-2.0, Unicode-3.0,
CDLA-Permissive-2.0, and combined SPDX expressions require their respective
notices and obligations; do not erase these under the project's GPL label.

GPUI 0.2.2 is vendored under Apache-2.0. Its license is preserved at
[vendor/gpui/LICENSE-APACHE](vendor/gpui/LICENSE-APACHE). Local changes include
Windows cross-compilation support documented in
[packaging/windows/README.md](packaging/windows/README.md) and X11 window-title
handling: initial and updated titles set `_NET_WM_NAME` as UTF-8 and `WM_NAME`
as a Latin-1-compatible fallback. Follow the
[dependency maintenance guidance](AGENTS.md#dependencies-and-distribution)
and review the complete current diff against the published crate when updating.

The optional flamegraph feature of pprof previously introduced inferno under
CDDL-1.0. That feature has been removed from this source tree. CPU sampling now
exports plain folded stacks; independently installed visualization tools are
not bundled or invoked automatically. Check the resolved dependency graph
again before every release rather than relying on this historical statement.

Before distributing binaries, collect the actual license/NOTICE files and
copyright statements for resolved dependencies (including transitive code and
data bundled within crates), and place the reviewed collection in `licenses/`.
The release packager includes that directory when present. A metadata list is
not a complete attribution bundle.

## Other inputs requiring provenance review

| Material                                                       | Status and action                                                                                                                                  |
| -------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| UI icons and integration logos                                 | See [asset review requirements](AGENTS.md#dependencies-and-distribution). Verify upstream origin/license and preserve required attribution. Brand marks do not imply endorsement. |
| App logo, app icons and installation artwork                   | Maintainer confirmation of authorship and redistribution rights is required.                                                                       |
| Site fonts, illustrations, screenshots and SVG embedded images | Verify source and license, add notices, or remove/replace before public distribution.                                                              |
| CommonMark specification fixture                               | Record its upstream version/source and retain the applicable specification/data notices.                                                           |
| Linux bundled runtime libraries                                | Collect licenses, notices and required corresponding sources for the exact packaged builds.                                                        |
| Root SVG                                                      | Review embedded content and provenance before including in the public snapshot.                                                                    |

## Corresponding source

The current release helper publishes binaries only; GitHub's automatic source
archives refer to the public community repository, not the private application
source. They do not satisfy the application's corresponding-source obligations.
Before approving distribution, maintainers must review and provide the exact
required application, dependency and bundled-library sources and build scripts
through an explicitly authorized channel. Do not expose private source history
or assume the helper performs this review.
Do not treat a link to a moving main branch, Cargo.lock alone, or this document
as satisfying all corresponding-source requirements.

The GPL does not grant trademark rights or change the license of independently
licensed components. See the [distribution gates](AGENTS.md#dependencies-and-distribution).
