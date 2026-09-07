# Vendored GPUI maintenance

Moeka uses GPUI **0.2.2** through the `[patch.crates-io]` override in
[Cargo.toml](../Cargo.toml), pointing to [vendor/gpui](../vendor/gpui).
This is a locally modified dependency, not an unchanged upstream checkout or a
single-patch fork.

## Upstream and comparison baseline

- Published release: [gpui 0.2.2 on crates.io](https://crates.io/crates/gpui/0.2.2).
- Release archive: <https://crates.io/api/v1/crates/gpui/0.2.2/download>.
- Upstream repository: [zed-industries/zed](https://github.com/zed-industries/zed),
  crate location `crates/gpui`.
- The archive's Cargo VCS metadata reports `sha1` =
  `69e2130295c2649963eb639fc70b4f2ee8ea1624`, `dirty` = **true**, and
  `path_in_vcs` = `crates/gpui`.

**The published crate archive is the authoritative comparison baseline, not an
exact pristine checkout of that Git SHA.** The dirty flag means the recorded
revision alone does not establish the complete published source contents.
Cargo's published manifest also differs in form from a repository manifest.

The inventory below comes from a read-only recursive comparison against the
local registry extraction of gpui 0.2.2 in the USTC mirror cache, followed by
per-file unified diffs. This inspection did not download or checksum-verify an
archive, build the project, or execute tests. It describes source differences,
not independently verified runtime results.

## Local changes

Paths below identify every modified file found in that comparison. The reasons
describe the behavior visible in the diff, not inferred historical bug reports.

| File                                                                                                            | Change and maintenance rationale                                                                                                                                                                                                                                                                                                                                                               |
| --------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [vendor/gpui/Cargo.toml](../vendor/gpui/Cargo.toml)                                                             | Removes the explicit `[[example]]` targets, matching the omitted examples directory. The `[[test]]` target declarations remain even though the standalone tests directory is absent; this copy is not a complete upstream test distribution.                                                                                                                                                   |
| [vendor/gpui/build.rs](../vendor/gpui/build.rs)                                                                 | Registers `gpui_prebuilt_shaders` and sets it only after the Windows release shader compilation step runs. Shader availability is therefore explicit rather than inferred solely from release/debug mode, allowing the runtime fallback below when cross-compiling without that step.                                                                                                          |
| [vendor/gpui/src/platform/windows/directx_renderer.rs](../vendor/gpui/src/platform/windows/directx_renderer.rs) | Selects embedded prebuilt shader bytes using `gpui_prebuilt_shaders`. Otherwise embeds HLSL text, substitutes the alpha-correction include, and uses in-memory `D3DCompile` instead of reading shaders from the build machine's source path. Debug compilation keeps debug/skip-optimization flags; release fallback uses optimization level 3. Keep this paired with the build-script change. |
| [vendor/gpui/src/platform/windows/window.rs](../vendor/gpui/src/platform/windows/window.rs)                     | Implements `HasDisplayHandle` with a `WindowsDisplayHandle` instead of `unimplemented!()`, so requesting the display handle no longer takes that explicit panic path.                                                                                                                                                                                                                          |
| [vendor/gpui/src/platform/blade/blade_renderer.rs](../vendor/gpui/src/platform/blade/blade_renderer.rs)         | Adds rounded-window clipping bounds, radii, and enable flag to `GlobalParams`, plus `draw_with_rounded_clip()`. Ordinary `draw()` passes no clip; offscreen path rasterization disables it. Supplies the GPU parameters used by the Wayland clipping path.                                                                                                                                     |
| [vendor/gpui/src/platform/blade/shaders.wgsl](../vendor/gpui/src/platform/blade/shaders.wgsl)                   | Mirrors the Rust uniform layout and applies rounded-window signed-distance alpha clipping to quad, path, underline, monochrome/polychrome sprite, and surface fragment outputs. RGB is attenuated for premultiplied alpha. Keep the Rust/WGSL layouts and clipping semantics synchronized.                                                                                                     |
| [vendor/gpui/src/platform/linux/wayland/window.rs](../vendor/gpui/src/platform/linux/wayland/window.rs)         | Computes client-decoration clipping from scaled insets and viewport size; tiled edges lose their inset and adjacent corner radii. Passes the result to the Blade renderer. When the decoration protocol object is absent, selects client decorations (logging a requested server-decoration fallback) and still updates the window.                                                            |
| [vendor/gpui/src/platform/mac/window.rs](../vendor/gpui/src/platform/mac/window.rs)                             | Removes the private `CGSMainConnectionID` / `CGSSetWindowBackgroundBlurRadius` declarations and pre-macOS-12 blur branch, retaining the `NSVisualEffectView` path. The patch comment identifies private-API/App Store concerns and a macOS 12+ deployment assumption; this is not an App Store approval or older-macOS compatibility guarantee.                                                |
| [vendor/gpui/src/platform.rs](../vendor/gpui/src/platform.rs)                                                   | Gives X11 priority when `MOEKA_FORCE_X11` is present and `DISPLAY` is nonempty. Otherwise keeps the Wayland-then-X11 selection order. Presence, not the variable's string value, enables the override; it does not create an X11 display when none is available.                                                                                                                               |

## Intentionally omitted material and license

The vendor copy intentionally omits the archive's documentation, examples, and
standalone tests directories, its crate-local lockfile and original manifest,
and the Cargo VCS metadata. The registry extraction's Cargo completion marker
is also absent. These correspond to docs/, examples/, tests/, Cargo.lock,
Cargo.toml.orig, .cargo_vcs_info.json, and .cargo-ok in the comparison baseline.
The completion marker is registry-cache bookkeeping, not upstream runtime code.

These omissions describe packaging scope, **not** a promise of unchanged runtime
behavior or a claim that upstream tests pass. In particular, do not assume all
vendor test targets can run without restoring their omitted inputs. The
application's [Cargo.lock](../Cargo.lock) is separate from the omitted
crate-local lockfile.

The retained [vendor/gpui/LICENSE-APACHE](../vendor/gpui/LICENSE-APACHE) is
byte-identical to the compared upstream copy. GPUI remains under **Apache-2.0**;
Moeka's own license does not replace it. Preserve upstream license and copyright
notices when refreshing the dependency; see also
[THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md).

## Updating the vendor copy

1. Retrieve the desired published GPUI release archive into a separate staging
   directory. Verify its SHA-256 against the checksum in the trusted crates.io
   registry index before extraction. Record the version, download URL, verified
   checksum, and archive VCS metadata. Do not substitute a Git checkout merely
   because its revision matches the metadata.
2. Preserve the existing full diff against the checksum-verified old release.
   Compare the new archive with both the old archive and the current vendor
   tree. Review every changed, added, and omitted file rather than searching only
   for Moeka-specific identifiers.
3. Replay the changes above onto the new baseline, adapting APIs as needed.
   Drop a local change only after establishing that the new upstream code covers
   its behavior. Treat the Windows build/renderer pair and the Blade/WGSL/Wayland
   group as coupled changes. Reconsider omissions and dangling test targets
   explicitly; retain the license and notices.
4. Update the dependency version, application lockfile, this inventory, and
   third-party notices as needed. Review the resulting complete diff. Follow
   [development.md](development.md) for release builds and tests, and restore
   upstream test inputs in a separate validation copy where necessary. A build
   alone does not validate platform behavior.
5. Run native regression checks on **Windows, Linux/Wayland, and macOS**:
   - **Windows:** native prebuilt-shader and runtime-compiled-shader paths,
     including launching a cross-built binary without the original source tree;
     text, emoji, images, display-handle consumers, resize, and mixed-DPI moves.
   - **Wayland:** client/server decoration negotiation, a compositor without
     server decorations, rounded clipping across primitive types, transparency,
     tiling/maximization, resizing, and fractional scaling. Also check ordinary
     X11 selection and `MOEKA_FORCE_X11` with and without an available X11 display.
   - **macOS:** opaque/transparent/blurred appearance transitions, resize, Retina
     scaling, and supported deployment targets; check that removed private blur
     APIs have not returned.
   - **All three:** clipboard text/image copy and paste, keyboard focus,
     non-ASCII input, IME composition/commit/cancel and candidate placement,
     especially after scaling or moving between displays.
6. Record tested OS/toolchain/graphics environments and failures or untested
   cases. Cross-compilation is useful coverage but is not a substitute for these
   native checks. None of these update-time checks was performed for this
   documentation-only inventory.
