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
        trash_context.delete(path).map_err(anyhow::Error::from)
    }

    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(path).map_err(anyhow::Error::from)
    }
}
