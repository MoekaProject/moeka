//! Spawning helper processes.

use std::ffi::OsStr;
use std::io;
use std::process::Command;

/// A command that does not flash a console window on platforms that have one.
///
/// Returns an error on builds whose host forbids child processes, so callers
/// fail the same way they would when the program is missing.
pub(crate) fn headless_command(program: impl AsRef<OsStr>) -> io::Result<Command> {
    if !super::capabilities::CHILD_PROCESSES {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "this build cannot run helper programs",
        ));
    }

    #[allow(unused_mut)]
    let mut command = Command::new(program);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    Ok(command)
}
