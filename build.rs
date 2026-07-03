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
}
