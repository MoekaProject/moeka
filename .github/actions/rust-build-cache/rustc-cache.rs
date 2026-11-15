use std::env;
use std::ffi::OsString;
use std::process::{Command, exit};

fn cache_gpui(args: &[OsString]) -> bool {
    args.windows(2)
        .any(|pair| pair[0] == "--crate-name" && pair[1] == "gpui")
}

fn main() {
    let mut args = env::args_os().skip(1);
    let Some(compiler) = args.next() else {
        eprintln!("Missing compiler argument for the Rust cache wrapper");
        exit(1);
    };
    let args: Vec<_> = args.collect();
    // rust-cache retains external dependencies, but excludes the vendored GPUI
    // path dependency. Caching every crate twice overwhelms the cache API.
    let mut command = if cache_gpui(&args) {
        let mut command = Command::new("sccache");
        command.arg(compiler);
        command
    } else {
        Command::new(compiler)
    };
    match command.args(args).status() {
        Ok(status) => exit(status.code().unwrap_or(1)),
        Err(error) => {
            eprintln!("Could not execute Rust compiler: {error}");
            exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caches_only_the_exact_gpui_crate() {
        let args = |values: &[&str]| values.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(cache_gpui(&args(&["--crate-name", "gpui", "src/lib.rs"])));
        assert!(!cache_gpui(&args(&["--crate-name", "gpui_macros"])));
        assert!(!cache_gpui(&args(&["--crate-name", "moeka", "gpui"])));
        assert!(!cache_gpui(&args(&["-vV"])));
        assert!(!cache_gpui(&args(&["--crate-name"])));
    }
}
