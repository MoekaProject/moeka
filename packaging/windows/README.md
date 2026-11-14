# Windows builds from macOS

`make windows` produces self-contained executables with embedded UI assets, application icons, and manifests:

```text
dist/Moeka-windows-x86_64.zip
dist/Moeka-windows-arm64.zip
```

## Setup

```sh
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc

BIN="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | awk '/^host:/{print $2}')/bin"
ln -sf "$BIN/llvm-ar" ~/.cargo/bin/llvm-lib
ln -sf "$BIN/rust-lld" ~/.cargo/bin/lld-link

make windows
```

`cargo-xwin` downloads and caches the MSVC CRT and Windows SDK on first use. Native Windows builds use MSVC Build Tools, the Windows SDK, and `fxc.exe`.

## Cross-build maintenance

- Vendored GPUI embeds HLSL and uses runtime `D3DCompile` for cross-builds. Native builds retain precompiled shaders when `fxc` succeeds.
- `build.rs` generates icon/manifest resources directly and embeds assets in a single indexed blob. Disk assets take precedence during development.
- Moeka disables GPUI's `windows-manifest` feature to keep one application manifest. Its own manifest supplies per-monitor DPI, UTF-8, and Common Controls v6; native tests inspect the linked resource.
- `xwin-shims/` adapts ARM64 compiler flags for `ring` and assembly preprocessing for `psm`. `make windows` adds the shims to `PATH`.
- Review these adaptations when upgrading GPUI or build dependencies.

## Native verification

Run both executables on Windows; a Windows 11 ARM VM can also test x64 through emulation.

Taskbar previews and Alt+Tab represent real application windows, not individual
editor tabs. Check that each window's system title follows its focused document
(including Markdown heading and filename), dirty/saved state, tab and split-pane
changes, workspace switches, and interface language. Settings, help and terminal
tabs should use their tab titles; an empty window uses the application name.
Opening more tabs in one window must not add taskbar previews.

PDF/PNG export requires Microsoft Edge WebView2 Runtime. Its loader is statically linked, while rendering runs in an isolated helper with a temporary profile.

HTML/PDF/PNG embed images as PNG using the editor's decoded RGBA pixels, including
high-bit-depth and floating-point sources. Verify HEIC/HEIF/AVIF with the required
Windows image codecs installed, plus SVG, JPEG XL, TIFF and HDR/EXR; compare
orientation, transparency and colors with the editor. Missing system codecs or
corrupt images must produce an explicit error, not a broken exported image.

Set `MOEKA_EXPORT_SMOKE_DIR` to an empty disposable directory and launch `Moeka.exe`. The application exports the built-in gallery to HTML/PDF/PNG and exits. Inspect text formatting, tables, formulas, diagrams, images, and the final paragraph. Cross-compilation alone does not validate native behavior.

See [development and release guidance](../../AGENTS.md).
