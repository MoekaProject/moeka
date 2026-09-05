//! Moving paths into the desktop environment's trash.

use std::path::Path;

pub(crate) fn move_to_trash(path: &Path) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use trash::TrashContext;
        use trash::macos::{DeleteMethod, TrashContextExtMacos};

        // Deliberately no Finder fallback: driving Finder needs AppleEvents,
        // which the App Store sandbox does not grant.
        let mut trash_context = TrashContext::new();
        trash_context.set_delete_method(DeleteMethod::NsFileManager);
        match trash_context.delete(path) {
            Ok(()) => Ok(()),
            Err(trash_error) if crate::platform::macos::path_is_on_network_volume(path) => {
                delete_permanently(path).map_err(|delete_error| {
                    anyhow::anyhow!(
                        "network volume has no Trash ({trash_error}); direct deletion failed ({delete_error})"
                    )
                })
            }
            Err(error) => Err(anyhow::Error::from(error)),
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(path).map_err(anyhow::Error::from)
    }
}

#[cfg(target_os = "macos")]
fn delete_permanently(path: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}
