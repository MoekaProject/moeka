//! Title bar gestures for the parts of the chrome the app draws itself.
//!
//! gpui leaves both of these to the embedder on most targets: `start_window_move`
//! and `titlebar_double_click` are empty defaults outside Linux, and `zoom` only
//! ever maximizes on Windows, so it can never restore.

use gpui::Window;

#[cfg(target_os = "macos")]
pub(crate) fn start_drag(window: &Window) {
    use objc::{class, msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };
    unsafe {
        let ns_view = appkit.ns_view.as_ptr() as *mut objc::runtime::Object;
        let ns_window: *mut objc::runtime::Object = msg_send![ns_view, window];
        let app: *mut objc::runtime::Object = msg_send![class!(NSApplication), sharedApplication];
        let event: *mut objc::runtime::Object = msg_send![app, currentEvent];
        if !ns_window.is_null() && !event.is_null() {
            let () = msg_send![ns_window, performWindowDragWithEvent: event];
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn toggle_maximize(window: &Window) {
    window.titlebar_double_click();
}

#[cfg(target_os = "windows")]
pub(crate) fn start_drag(window: &Window) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_SYSCOMMAND};

    // SC_MOVE | HTCAPTION: hands the press to the system move loop, which is
    // what brings Aero Snap along.
    const SC_MOVE_BY_MOUSE: usize = 0xF012;

    let Some(hwnd) = hwnd(window) else {
        return;
    };
    unsafe {
        // gpui captures the mouse on button down; the move loop needs it back.
        ReleaseCapture();
        PostMessageW(hwnd, WM_SYSCOMMAND, SC_MOVE_BY_MOUSE, 0);
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn toggle_maximize(window: &Window) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsZoomed, SW_MAXIMIZE, SW_RESTORE, ShowWindowAsync,
    };

    let Some(hwnd) = hwnd(window) else {
        return;
    };
    unsafe {
        let command = if IsZoomed(hwnd) != 0 {
            SW_RESTORE
        } else {
            SW_MAXIMIZE
        };
        ShowWindowAsync(hwnd, command);
    }
}

#[cfg(target_os = "windows")]
fn hwnd(window: &Window) -> Option<windows_sys::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => Some(win32.hwnd.get() as _),
        _ => None,
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn start_drag(window: &Window) {
    window.start_window_move();
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn toggle_maximize(window: &Window) {
    window.zoom_window();
}
