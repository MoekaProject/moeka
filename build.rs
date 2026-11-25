use std::path::{Path, PathBuf};
use std::process::Command;

fn git_output(args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");

    let commit = git_output(&["rev-parse", "--short=12", "HEAD"]);
    // Builds outside a Git checkout still need a well-formed identity to bind
    // the embedded SSH server to this application.
    let source = Some(git_output(&["rev-parse", "HEAD"]))
        .filter(|sha| !sha.is_empty())
        .unwrap_or_else(|| "0".repeat(40));
    let tag = git_output(&["describe", "--tags", "--exact-match", "HEAD"]);

    println!("cargo:rustc-env=MOEKA_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=MOEKA_GIT_SOURCE_SHA={source}");
    println!("cargo:rustc-env=MOEKA_GIT_TAG={tag}");

    embedded_assets::generate(&source);

    // `cfg!(windows)` in a build script describes the host, not the target, so
    // resources would silently vanish when cross-compiling.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        windows_resources::embed();
    }
}

/// Bakes `assets/` into the binary so a bare `moeka.exe` still finds its icons.
/// `FsAssets` keeps preferring on-disk copies, which is what the macOS bundle
/// and local development use.
///
/// Everything goes into one blob rather than an `include_bytes!` per file:
/// hundreds of anonymous constants get scattered across the crate's 256
/// codegen units and the macOS linker then fails to resolve them.
mod embedded_assets {
    use super::*;

    pub(super) fn generate(source: &str) {
        println!("cargo:rerun-if-changed=assets");

        let mut files = Vec::new();
        collect(Path::new("assets"), &mut files);

        // Keyed and sorted as strings so the lookup can binary search. `Path`
        // ordering is by component and would disagree (`icons-x/` vs `icons/`).
        let mut entries: Vec<(String, PathBuf)> = files
            .into_iter()
            .map(|file| (file.to_string_lossy().replace('\\', "/"), file))
            .collect();
        if let Some(bundle) = remote_bundle(source) {
            entries.push(("remote/servers.mrb".into(), bundle));
        }
        entries.sort();

        let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
        let mut blob = Vec::new();
        let mut index = String::from("&[\n");

        for (key, file) in &entries {
            let bytes = std::fs::read(file)
                .unwrap_or_else(|error| panic!("read {}: {error}", file.display()));
            let start = blob.len();
            blob.extend_from_slice(&bytes);
            index.push_str(&format!("    ({key:?}, {start}, {}),\n", blob.len()));
        }
        index.push_str("]\n");

        std::fs::write(out_dir.join("assets.bin"), &blob).expect("write assets.bin");
        std::fs::write(out_dir.join("embedded_assets.rs"), index).expect("write index");
    }

    fn remote_bundle(source: &str) -> Option<PathBuf> {
        println!("cargo:rerun-if-env-changed=MOEKA_REMOTE_BUNDLE");
        println!("cargo:rerun-if-env-changed=MOEKA_REQUIRE_REMOTE_BUNDLE");
        if std::env::var_os("CARGO_FEATURE_MAS").is_some() {
            return None;
        }
        let target_dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"));
        let cached = target_dir.join("remote/servers.mrb");
        println!("cargo:rerun-if-changed={}", cached.display());
        let explicit = std::env::var_os("MOEKA_REMOTE_BUNDLE").map(PathBuf::from);
        let bundle = explicit
            .clone()
            .or_else(|| cached.is_file().then_some(cached));
        let required = match std::env::var("MOEKA_REQUIRE_REMOTE_BUNDLE").as_deref() {
            Ok("1") => true,
            Ok("") | Err(_) => false,
            Ok(_) => panic!("MOEKA_REQUIRE_REMOTE_BUNDLE must be 1 or unset"),
        };
        let Some(path) = bundle else {
            assert!(
                !required,
                "SSH-capable packages require MOEKA_REMOTE_BUNDLE"
            );
            return dev_bundle(source);
        };
        println!("cargo:rerun-if-changed={}", path.display());
        let verified = std::fs::read(&path)
            .map_err(|error| format!("read remote bundle {}: {error}", path.display()))
            .and_then(|bytes| {
                let bundle = moeka_ssh::bundle::Bundle::parse(&bytes)
                    .map_err(|error| format!("invalid remote bundle: {error}"))?;
                bundle
                    .validate_identity(env!("CARGO_PKG_VERSION"), source)
                    .map_err(|error| format!("remote bundle identity mismatch: {error}"))?;
                if required && !bundle.is_complete() {
                    return Err(
                        "SSH-capable packages require a bundle with every remote target".into(),
                    );
                }
                for payload in &bundle.manifest().payloads {
                    bundle.payload(payload.target).map_err(|error| {
                        format!("invalid {} remote payload: {error}", payload.target.name())
                    })?;
                }
                Ok(())
            });
        match verified {
            Ok(()) => {}
            Err(error) if explicit.is_none() && !required => {
                println!(
                    "cargo:warning={error}; rebuilding the local development bundle for {source}"
                );
                return dev_bundle(source);
            }
            Err(error) => panic!("{error}"),
        }
        Some(path)
    }

    /// Local builds have no CI-produced bundle, so SSH would be unusable. Build
    /// the helper for the host and for every other target whose Rust standard
    /// library is installed, and bundle whatever succeeds. Official packages
    /// never reach this: they must supply a complete bundle.
    fn dev_bundle(source: &str) -> Option<PathBuf> {
        use moeka_ssh::ServerTarget::{LinuxAarch64, LinuxX86_64, MacosAarch64, MacosX86_64};

        println!("cargo:rerun-if-env-changed=MOEKA_SKIP_DEV_REMOTE_BUNDLE");
        println!("cargo:rerun-if-changed=Cargo.lock");
        println!("cargo:rerun-if-changed=crates/moeka-fs");
        println!("cargo:rerun-if-changed=crates/moeka-remote-server");
        if std::env::var_os("MOEKA_SKIP_DEV_REMOTE_BUNDLE").is_some() {
            return None;
        }

        let targets = [
            (LinuxX86_64, "x86_64-unknown-linux-musl"),
            (LinuxAarch64, "aarch64-unknown-linux-musl"),
            (MacosX86_64, "x86_64-apple-darwin"),
            (MacosAarch64, "aarch64-apple-darwin"),
        ];
        let host = std::env::var("HOST").unwrap_or_default();
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
        let target_dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"))
            .join("remote-dev");
        let server_version = std::fs::read_to_string("crates/moeka-remote-server/Cargo.toml")
            .ok()?
            .lines()
            .find_map(|line| {
                line.strip_prefix("version = \"")
                    .and_then(|rest| rest.strip_suffix('"'))
                    .map(str::to_owned)
            })?;
        let build = moeka_fs::service::BuildIdentity {
            server_version,
            app_version: env!("CARGO_PKG_VERSION").into(),
            source_sha: source.into(),
        };

        let mut payloads = Vec::new();
        for (target, triple) in targets {
            let installed = triple == host
                || Command::new(&rustc)
                    .args(["--print", "target-libdir", "--target", triple])
                    .output()
                    .ok()
                    .filter(|output| output.status.success())
                    .is_some_and(|output| {
                        Path::new(String::from_utf8_lossy(&output.stdout).trim()).is_dir()
                    });
            if !installed {
                println!(
                    "cargo:warning=SSH server for {} skipped; run `rustup target add {triple}` and rebuild to include it",
                    target.name()
                );
                continue;
            }
            let mut command = Command::new(&cargo);
            command
                .args([
                    "build",
                    "--release",
                    "--locked",
                    "-p",
                    "moeka-remote-server",
                ])
                .args(["--target", triple])
                .env("CARGO_TARGET_DIR", &target_dir)
                .env("MOEKA_REMOTE_APP_VERSION", &build.app_version)
                .env("MOEKA_REMOTE_SOURCE_SHA", &build.source_sha)
                .env_remove("CARGO_ENCODED_RUSTFLAGS");
            if triple.ends_with("linux-musl") {
                // Pure-Rust static helper: LLVM's linker needs no cross toolchain.
                let key = format!(
                    "CARGO_TARGET_{}_LINKER",
                    triple.to_uppercase().replace('-', "_")
                );
                command.env(key, "rust-lld");
            }
            let built = command.status().is_ok_and(|status| status.success());
            let executable = target_dir.join(triple).join("release/moeka-remote-server");
            match std::fs::read(&executable) {
                Ok(bytes) if built => payloads.push((target, bytes)),
                _ => println!(
                    "cargo:warning=SSH server for {} failed to build",
                    target.name()
                ),
            }
        }
        if payloads.is_empty() {
            return None;
        }
        let inputs: Vec<_> = payloads
            .iter()
            .map(|(target, bytes)| (*target, bytes.as_slice()))
            .collect();
        let bytes = moeka_ssh::bundle::create(build, &inputs)
            .unwrap_or_else(|error| panic!("create local remote bundle: {error}"));
        let path = out_dir.join("servers.mrb");
        std::fs::write(&path, bytes).expect("write local remote bundle");
        Some(path)
    }

    fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'))
            {
                continue;
            }
            if path.is_dir() {
                collect(&path, files);
            } else {
                files.push(path);
            }
        }
    }
}

/// Builds a Win32 `.res` blob (icon + application manifest) without a resource
/// compiler, so `cargo xwin` cross-builds from macOS/Linux still produce a
/// self-contained `moeka.exe`.
///
/// Without the embedded manifest the loader binds comctl32 5.82 and `rfd`'s
/// `TaskDialogIndirect` import fails before `main` runs.
mod windows_resources {
    use super::*;

    const RT_ICON: u16 = 3;
    const RT_GROUP_ICON: u16 = 14;
    const RT_MANIFEST: u16 = 24;

    /// `CREATEPROCESS_MANIFEST_RESOURCE_ID`, and the id `moeka.rc` used for the icon.
    const RESOURCE_ID: u16 = 1;
    const LANG_EN_US: u16 = 0x0409;

    pub(super) fn embed() {
        let packaging = Path::new("packaging/windows");
        let icon_path = packaging.join("moeka.ico");
        let manifest_path = packaging.join("moeka.exe.manifest");

        println!("cargo:rerun-if-changed={}", icon_path.display());
        println!("cargo:rerun-if-changed={}", manifest_path.display());

        let icon = std::fs::read(&icon_path).expect("read packaging/windows/moeka.ico");
        let manifest =
            std::fs::read(&manifest_path).expect("read packaging/windows/moeka.exe.manifest");

        let mut res = Vec::new();
        // A `.res` file opens with an empty entry acting as the format marker.
        write_entry(&mut res, 0, 0, 0, 0, &[]);

        let (group, images) = parse_icon(&icon);
        for (index, image) in images.iter().enumerate() {
            write_entry(
                &mut res,
                RT_ICON,
                index as u16 + 1,
                LANG_EN_US,
                0x1010,
                image,
            );
        }
        write_entry(
            &mut res,
            RT_GROUP_ICON,
            RESOURCE_ID,
            LANG_EN_US,
            0x1030,
            &group,
        );
        write_entry(
            &mut res,
            RT_MANIFEST,
            RESOURCE_ID,
            LANG_EN_US,
            0x1030,
            &manifest,
        );

        let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("moeka.res");
        std::fs::write(&out, &res).expect("write moeka.res");
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
        }
        println!("cargo:rustc-link-arg-bins={}", out.display());
    }

    /// Splits an `.ico` into its `GRPICONDIR` directory and the individual images.
    fn parse_icon(icon: &[u8]) -> (Vec<u8>, Vec<&[u8]>) {
        assert!(icon.len() >= 6, "moeka.ico is truncated");
        let count = u16::from_le_bytes([icon[4], icon[5]]) as usize;

        let mut group = Vec::new();
        group.extend_from_slice(&icon[0..6]);
        let mut images = Vec::with_capacity(count);

        for index in 0..count {
            let start = 6 + index * 16;
            let entry = icon
                .get(start..start + 16)
                .expect("moeka.ico directory is truncated");
            let size = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
            let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;

            // `GRPICONDIRENTRY` matches `ICONDIRENTRY` up to the size, then
            // replaces the file offset with the `RT_ICON` resource id.
            group.extend_from_slice(&entry[0..12]);
            group.extend_from_slice(&(index as u16 + 1).to_le_bytes());

            images.push(
                icon.get(offset..offset + size)
                    .expect("moeka.ico image is truncated"),
            );
        }

        (group, images)
    }

    fn write_entry(
        out: &mut Vec<u8>,
        ty: u16,
        name: u16,
        lang: u16,
        memory_flags: u16,
        data: &[u8],
    ) {
        let mut header = Vec::with_capacity(32);
        header.extend_from_slice(&(data.len() as u32).to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes()); // header size, patched below
        // `0xFFFF` marks an ordinal (rather than string) type/name.
        header.extend_from_slice(&0xFFFFu16.to_le_bytes());
        header.extend_from_slice(&ty.to_le_bytes());
        header.extend_from_slice(&0xFFFFu16.to_le_bytes());
        header.extend_from_slice(&name.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes()); // data version
        header.extend_from_slice(&memory_flags.to_le_bytes());
        header.extend_from_slice(&lang.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes()); // version
        header.extend_from_slice(&0u32.to_le_bytes()); // characteristics

        let header_size = (header.len() as u32).to_le_bytes();
        header[4..8].copy_from_slice(&header_size);

        out.extend_from_slice(&header);
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
}
