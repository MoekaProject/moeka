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
    let tag = git_output(&["describe", "--tags", "--exact-match", "HEAD"]);

    println!("cargo:rustc-env=MOEKA_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=MOEKA_GIT_TAG={tag}");

    embedded_assets::generate();

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

    pub(super) fn generate() {
        println!("cargo:rerun-if-changed=assets");

        let mut files = Vec::new();
        collect(Path::new("assets"), &mut files);

        // Keyed and sorted as strings so the lookup can binary search. `Path`
        // ordering is by component and would disagree (`icons-x/` vs `icons/`).
        let mut entries: Vec<(String, PathBuf)> = files
            .into_iter()
            .map(|file| (file.to_string_lossy().replace('\\', "/"), file))
            .collect();
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
