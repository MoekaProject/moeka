# Building `moeka.exe` from macOS

`make windows` cross-builds a single self-contained `moeka.exe` for both
`x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`, without a Windows
machine or CI runner. Output lands in `dist/`:

```
dist/moeka-windows-x86_64.exe
dist/moeka-windows-arm64.exe
```

Nothing has to be shipped alongside the binary — `assets/`, the DPI/comctl32
manifest, and the application icon are all embedded.

## One-time setup

```sh
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc

# LLVM's MSVC-compatible archiver and linker, from the Rust toolchain itself.
BIN="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | awk '/^host:/{print $2}')/bin"
ln -sf "$BIN/llvm-ar"  ~/.cargo/bin/llvm-lib
ln -sf "$BIN/rust-lld" ~/.cargo/bin/lld-link
```

`cargo xwin` downloads the MSVC CRT and Windows SDK headers/libraries on first
use and caches them under `~/Library/Caches/cargo-xwin`.

## What the cross-build needs on top of `cargo-xwin`

Four upstream assumptions break when the *host* is not Windows, or when there is
no `.app`-style bundle to hold resources. Each is worked around in-tree so the
build stays a plain `make windows`.

### 1. `vendor/gpui` — shaders

gpui's build script guards its Windows work with `#[cfg(target_os = "windows")]`,
which describes the host, so cross-builds never run it:

- Release builds `include!` a `shaders_bytes.rs` that is only produced by
  `fxc.exe`, so linking fails outright.
- Debug builds fall back to `D3DCompileFromFile` against a path baked in from
  `CARGO_MANIFEST_DIR`, producing an `.exe` that only runs on the build machine.

`vendor/gpui` (wired up through `[patch.crates-io]`) replaces
`directx_renderer::shader_resources` with a version that `include_str!`s the
HLSL and compiles it with `D3DCompile` at startup. The prebuilt path is kept
behind a `gpui_prebuilt_shaders` cfg that the build script only sets when it
actually ran `fxc`, so native Windows builds are unchanged.

Re-vendor after every gpui upgrade.

### 2. `build.rs` — icon and manifest

`embed-resource` needs `rc.exe` or `llvm-rc`, and `rustup` ships neither. Without
the embedded manifest the loader binds comctl32 5.82 and `rfd`'s
`TaskDialogIndirect` import fails before `main` runs.

`build.rs` instead writes the `.res` blob itself (`RT_ICON`, `RT_GROUP_ICON`,
`RT_MANIFEST`) and hands it to the linker via `cargo:rustc-link-arg-bins`. This
is pure `std`, so it behaves identically on every host.

### 3. `build.rs` — UI assets

`AssetSource` reads `assets/` from disk, relative to the executable. The macOS
bundle satisfies that via `Contents/Resources`, but a lone `moeka.exe` has
nowhere to look, so every SVG icon silently renders as nothing.

`build.rs` concatenates `assets/` into a single blob with a path-sorted offset
index, which `FsAssets` falls back to when the on-disk lookup misses. Disk still
wins, so development and the macOS bundle are unaffected. One blob rather than
an `include_bytes!` per file is deliberate: hundreds of anonymous constants get
scattered across the crate's 256 codegen units and the macOS linker then fails
to resolve them.

Help documents already had an `include_str!` fallback, and fonts come from the
OS, so those needed no change.

### 4. `xwin-shims/` — ARM64 C and assembly

Two crates disagree with `cargo-xwin` about compiler flavour, so `make windows`
prepends `packaging/windows/xwin-shims` to `PATH`:

- `clang` — `ring` invokes the GNU driver for aarch64 while `cargo-xwin` exports
  MSVC-style `/imsvc` include flags. The shim rewrites them to `-isystem`.
- `clang-cl` — `psm` selects its GAS assembly source but skips
  `-xassembler-with-cpp` and the `CFG_TARGET_*` defines whenever the target env
  is MSVC, leaving its macros unexpanded. The shim assembles `.s` inputs with
  the GNU driver and supplies the missing flags; everything else passes through.

## Verifying an artefact

The exes cannot be run on macOS. To sanity-check one without Windows:

```sh
python3 - <<'PY'
import struct
d = open('dist/moeka-windows-arm64.exe','rb').read()
pe = struct.unpack_from('<I', d, 0x3c)[0]
print(hex(struct.unpack_from('<H', d, pe + 4)[0]))  # 0x8664 x64, 0xaa64 ARM64
PY
```

A real smoke test still needs Windows — a Windows 11 ARM VM can run both
binaries (x64 via emulation).
