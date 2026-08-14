//! Moving paths into the desktop environment's trash.

use std::path::Path;

pub(crate) fn move_to_trash(path: &Path) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use trash::TrashContext;
        use trash::macos::{DeleteMethod, TrashContextExtMacos};

        let mut trash_context = TrashContext::new();
        trash_context.set_delete_method(DeleteMethod::NsFileManager);
        match trash_context.delete(path) {
            Ok(()) => Ok(()),
            Err(file_manager_error) => {
                // Finder can trash files on SMB volumes that reject trashItemAtURL.
                let mut finder_context = TrashContext::new();
                finder_context.set_delete_method(DeleteMethod::Finder);
                finder_context.delete(path).map_err(|finder_error| {
                    anyhow::anyhow!("NSFileManager: {file_manager_error}; Finder: {finder_error}")
                })
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(path).map_err(anyhow::Error::from)
    }
}
