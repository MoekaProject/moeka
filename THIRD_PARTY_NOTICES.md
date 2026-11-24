# Third-party notices

Moeka's original code uses [GPL-3.0-or-later](LICENSE). Third-party code, data, fonts, icons, and trademarks retain their own terms. This inventory records attribution and review requirements; it is not a license clearance or a substitute for license texts.

## Components and assets

| Material | License and provenance |
| --- | --- |
| Rust dependencies | Resolve versions and enabled features from `Cargo.lock` with `cargo metadata --locked --format-version 1`. Review direct/transitive dependencies and bundled data, including MIT, Apache, BSD, ISC, MPL, Unicode, CDLA, and combined SPDX obligations. |
| PDF rendering | [hayro 0.9](https://github.com/LaurenzV/hayro/tree/hayro-v0.9.0) and its interpreter/parser use Apache-2.0 OR MIT. Its [upstream notice](https://github.com/LaurenzV/hayro/blob/hayro-v0.9.0/NOTICE.md) covers Apache-2.0 adaptations from PDFBox (standard-font encoding maps and dictionary keys), pdf.js (CalRGB/CalGray, flate and cryptography), and image-png (PNG predictors). Default embedded font substitutes retain the [PDFium font notice](licenses/hayro-pdfium-fonts.txt); embedded Adobe CMaps retain the [Adobe notice](licenses/hayro-adobe-cmaps.txt). |
| Vendored GPUI | Upstream code retains [Apache-2.0](vendor/gpui/LICENSE-APACHE). Local changes cover platform builds, menus, input, gestures, and paint-data access; review the full upstream diff when updating. |
| Moeka's GPUI additions | The original [hover-transition](vendor/gpui/src/hover_transition.rs) and [scene-color transition](vendor/gpui/src/scene_transition.rs) modules use GPL-3.0-or-later; upstream GPUI keeps its own license. |
| Lucide replacement icons | [Lucide](https://github.com/lucide-icons/lucide), with the [ISC notice](licenses/lucide-icons.txt). |
| MingCute minimap and PDF rotation icons | `map_fill.svg` and `design/anticlockwise_line.svg` from [MingCute](https://github.com/mingcute-design/mingcute-icons), with the [Apache-2.0 license](licenses/mingcute-apache-2.0.txt). The latter is stored as `assets/icons/preview/rotate-left.svg`. |
| Other icons, logos, fonts, grammars, and artwork | Verify origin, exact license, redistribution rights, and required attribution. Brand marks do not imply endorsement. |
| CommonMark fixture | Record the upstream version/source and preserve specification/data notices. |
| Linux runtime libraries | Collect licenses, notices, and required corresponding sources for the exact packaged builds. |

## Distribution review

- Confirm rights to distribute original code and bundled assets. Recheck the resolved dependency graph and platform features for each release.
- Collect actual license/NOTICE texts and copyright statements in `licenses/`, including transitive and crate-bundled materials. Inspect each final package: current packaging does not automatically include the complete collection on every platform.
- Provide exact corresponding application, dependency, and bundled-library sources and build scripts through an authorized channel. Releases in MoekaProject/moeka tag the built source commit; older community repository source archives do not contain application source. A source archive, moving branch, or lockfile alone does not establish complete corresponding-source coverage for bundled dependencies.
- Keep private history private until publication is explicitly approved. Follow the [release process](AGENTS.md#release). Code licenses do not grant trademark rights or override independently licensed components.
