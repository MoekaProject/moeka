//! Locating the user's shell and the tool directories a GUI process misses.

use std::ffi::OsString;
use std::path::PathBuf;

#[cfg(unix)]
const SHELL_VARIABLE: &str = "SHELL";
#[cfg(unix)]
const SHELL_FALLBACK: &str = "/bin/sh";

#[cfg(windows)]
const SHELL_VARIABLE: &str = "COMSPEC";
#[cfg(windows)]
const SHELL_FALLBACK: &str = "cmd.exe";

/// Shell offered as the default in settings, before the user picks one.
#[cfg(target_os = "macos")]
pub(crate) const DEFAULT_SHELL_PROGRAM: &str = "/bin/zsh";
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) const DEFAULT_SHELL_PROGRAM: &str = "/bin/bash";
#[cfg(windows)]
pub(crate) const DEFAULT_SHELL_PROGRAM: &str = "cmd.exe";

/// Program used for the built-in terminal and for shell helper commands.
pub(crate) fn default_shell_program() -> String {
    std::env::var(SHELL_VARIABLE)
        .ok()
        .filter(|shell| !shell.trim().is_empty())
        .unwrap_or_else(|| SHELL_FALLBACK.to_string())
}

/// Arguments that make the shell run `command_line` and then exit.
pub(crate) fn shell_command_args(command_line: &str) -> Vec<String> {
    #[cfg(unix)]
    {
        // `exec` hands the terminal to the command instead of nesting a shell.
        vec!["-lc".to_string(), format!("exec {command_line}")]
    }

    #[cfg(windows)]
    {
        vec!["/C".to_string(), command_line.to_string()]
    }
}

#[cfg(unix)]
const HOME_RELATIVE_TOOL_DIRS: &[&str] = &[".local/bin", ".cargo/bin", "bin"];
#[cfg(windows)]
const HOME_RELATIVE_TOOL_DIRS: &[&str] = &[".cargo\\bin", "AppData\\Local\\Microsoft\\WindowsApps"];

#[cfg(target_os = "macos")]
const SYSTEM_TOOL_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/usr/bin",
    "/bin",
    "/usr/sbin",
    "/sbin",
];
#[cfg(all(unix, not(target_os = "macos")))]
const SYSTEM_TOOL_DIRS: &[&str] = &[
    "/home/linuxbrew/.linuxbrew/bin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/usr/bin",
    "/bin",
    "/usr/sbin",
    "/sbin",
];
#[cfg(windows)]
const SYSTEM_TOOL_DIRS: &[&str] = &[];

/// Tool directories a GUI-launched process does not inherit in its `PATH`.
pub(crate) fn extra_path_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(home) = super::paths::home_dir() {
        candidates.extend(HOME_RELATIVE_TOOL_DIRS.iter().map(|dir| home.join(dir)));
    }
    candidates.extend(SYSTEM_TOOL_DIRS.iter().map(PathBuf::from));

    candidates
}

/// `PATH` as reported by the user's login shell.
///
/// A Finder-launched app inherits a minimal `PATH` that misses user tool
/// directories, so the login shell is asked for the real one. Windows has no
/// equivalent step: processes already inherit the full user `PATH`.
pub(crate) fn login_shell_path() -> Option<OsString> {
    #[cfg(unix)]
    {
        let output = std::process::Command::new(default_shell_program())
            .args(["-lc", "printf '%s' \"$PATH\""])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        let path = String::from_utf8_lossy(&output.stdout);
        let trimmed = path.trim();
        (!trimmed.is_empty()).then(|| OsString::from(trimmed))
    }

    #[cfg(windows)]
    {
        None
    }
}

/// File names to probe on `PATH` when looking for `command`.
pub(crate) fn executable_names(command: &str) -> Vec<String> {
    #[cfg(unix)]
    {
        vec![command.to_string()]
    }

    #[cfg(windows)]
    {
        // npm-installed CLIs land on `PATH` as `.cmd` shims, not bare binaries.
        let extensions =
            std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        std::iter::once(command.to_string())
            .chain(
                extensions
                    .split(';')
                    .map(str::trim)
                    .filter(|extension| !extension.is_empty())
                    .map(|extension| format!("{command}{extension}")),
            )
            .collect()
    }
}

/// Locale variables the terminal should carry, empty where they do not apply.
pub(crate) fn locale_env_vars() -> Vec<(&'static str, OsString)> {
    #[cfg(unix)]
    {
        ["LANG", "LC_ALL", "LC_CTYPE"]
            .into_iter()
            .map(|key| (key, utf8_locale(key)))
            .collect()
    }

    #[cfg(windows)]
    {
        Vec::new()
    }
}

#[cfg(unix)]
fn utf8_locale(key: &str) -> OsString {
    std::env::var_os(key)
        .filter(|value| {
            value
                .to_string_lossy()
                .to_ascii_uppercase()
                .contains("UTF-8")
        })
        .unwrap_or_else(|| OsString::from("en_US.UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_the_bare_command_name_first() {
        assert_eq!(executable_names("claude")[0], "claude");
    }

    #[cfg(unix)]
    #[test]
    fn unix_probes_only_the_bare_name() {
        assert_eq!(executable_names("claude"), vec!["claude".to_string()]);
    }

    #[cfg(windows)]
    #[test]
    fn windows_probes_pathext_suffixes() {
        let names = executable_names("claude");
        assert!(
            names
                .iter()
                .any(|name| name.eq_ignore_ascii_case("claude.cmd"))
        );
        assert!(
            names
                .iter()
                .any(|name| name.eq_ignore_ascii_case("claude.exe"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_runs_command_lines_through_a_login_shell() {
        assert_eq!(
            shell_command_args("cargo run"),
            vec!["-lc".to_string(), "exec cargo run".to_string()]
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_runs_command_lines_through_cmd() {
        assert_eq!(
            shell_command_args("cargo run"),
            vec!["/C".to_string(), "cargo run".to_string()]
        );
    }
}
