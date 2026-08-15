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

    // `cfg!(windows)` in a build script describes the host, not the target, so
    // resources would silently vanish when cross-compiling.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=packaging/windows/moeka.rc");
        println!("cargo:rerun-if-changed=packaging/windows/moeka.ico");
        println!("cargo:rerun-if-changed=packaging/windows/moeka.exe.manifest");

        // Cross-compiling usually has no resource compiler; ship
        // `moeka.exe.manifest` beside the binary in that case.
        if let Err(error) =
            embed_resource::compile("packaging/windows/moeka.rc", embed_resource::NONE)
                .manifest_optional()
        {
            println!("cargo:warning=Windows resources were not embedded: {error}");
        }
    }
}
