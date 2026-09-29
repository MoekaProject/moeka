//! Native file dialogs.

/// Runs a file dialog on a dedicated thread.
///
/// gpui's Windows background executor is a WinRT thread pool, and those
/// threads already belong to the multi-threaded apartment. `rfd` asks COM for
/// the single-threaded apartment, is refused with `RPC_E_CHANGED_MODE`, and
/// returns without ever showing a window. A fresh thread has no apartment yet.
#[cfg(not(target_os = "macos"))]
pub(crate) fn run<T>(dialog: impl FnOnce() -> Option<T> + Send + 'static) -> Option<T>
where
    T: Send + 'static,
{
    std::thread::spawn(dialog).join().ok().flatten()
}
