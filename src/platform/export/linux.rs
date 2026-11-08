//! Runtime-linked WebKitGTK: portable builds do not acquire a hard dependency
//! on a particular distro's GTK/Soup ABI. This runs only in the isolated helper.
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use libloading::Library;

use super::{NativeFormat, PREPARE_SCRIPT, RenderStatus, Request, STATUS_SCRIPT};

type Object = *mut c_void;
type AsyncCallback = unsafe extern "C" fn(Object, Object, Object);
#[repr(C)]
struct GError {
    domain: u32,
    code: i32,
    message: *const c_char,
}

macro_rules! api {
    ($($name:ident: $ty:ty),* $(,)?) => {
        struct Api { _library: std::mem::ManuallyDrop<Library>, $($name: $ty,)* }
        impl Api {
            unsafe fn load() -> Result<Self> {
                let library = unsafe { Library::new("libwebkit2gtk-4.1.so.0") }
                    .or_else(|_| unsafe { Library::new("libwebkit2gtk-4.0.so.37") })
                    .context(crate::i18n::runtime::RuntimeText::WebKitRequired.to_string())?;
                Ok(Self {
                    $($name: *unsafe { library.get::<$ty>(concat!(stringify!($name), "\0").as_bytes()) }
                        .with_context(|| crate::i18n::runtime::rt!(NativeRendererFailed, stringify!($name)))?,)*
                    _library: std::mem::ManuallyDrop::new(library),
                })
            }
        }
    };
}

api! {
    gtk_init_check: unsafe extern "C" fn(Object, Object) -> i32,
    gtk_window_new: unsafe extern "C" fn(i32) -> Object,
    gtk_window_set_default_size: unsafe extern "C" fn(Object, i32, i32),
    gtk_window_resize: unsafe extern "C" fn(Object, i32, i32),
    gtk_widget_get_scale_factor: unsafe extern "C" fn(Object) -> i32,
    gtk_window_set_accept_focus: unsafe extern "C" fn(Object, i32),
    gtk_window_set_focus_on_map: unsafe extern "C" fn(Object, i32),
    gtk_widget_set_opacity: unsafe extern "C" fn(Object, f64),
    gtk_container_add: unsafe extern "C" fn(Object, Object),
    gtk_widget_show_all: unsafe extern "C" fn(Object),
    gtk_widget_destroy: unsafe extern "C" fn(Object),
    g_main_context_iteration: unsafe extern "C" fn(Object, i32) -> i32,
    g_signal_connect_data: unsafe extern "C" fn(Object, *const c_char, unsafe extern "C" fn(), Object, Object, i32) -> u64,
    g_object_unref: unsafe extern "C" fn(Object),
    g_free: unsafe extern "C" fn(Object),
    g_error_free: unsafe extern "C" fn(*mut GError),
    webkit_web_context_new_ephemeral: unsafe extern "C" fn() -> Object,
    webkit_web_view_new_with_context: unsafe extern "C" fn(Object) -> Object,
    webkit_web_view_load_uri: unsafe extern "C" fn(Object, *const c_char),
    webkit_web_view_set_zoom_level: unsafe extern "C" fn(Object, f64),
    webkit_web_view_get_settings: unsafe extern "C" fn(Object) -> Object,
    webkit_settings_set_enable_write_console_messages_to_stdout: unsafe extern "C" fn(Object, i32),
    webkit_settings_set_print_backgrounds: unsafe extern "C" fn(Object, i32),
    webkit_web_view_run_javascript: unsafe extern "C" fn(Object, *const c_char, Object, AsyncCallback, Object),
    webkit_web_view_run_javascript_finish: unsafe extern "C" fn(Object, Object, *mut *mut GError) -> Object,
    webkit_javascript_result_get_js_value: unsafe extern "C" fn(Object) -> Object,
    webkit_javascript_result_unref: unsafe extern "C" fn(Object),
    jsc_value_to_string: unsafe extern "C" fn(Object) -> *mut c_char,
    webkit_web_view_get_snapshot: unsafe extern "C" fn(Object, i32, i32, Object, AsyncCallback, Object),
    webkit_web_view_get_snapshot_finish: unsafe extern "C" fn(Object, Object, *mut *mut GError) -> Object,
    cairo_surface_write_to_png: unsafe extern "C" fn(Object, *const c_char) -> i32,
    cairo_surface_destroy: unsafe extern "C" fn(Object),
    gtk_print_settings_new: unsafe extern "C" fn() -> Object,
    gtk_print_settings_set: unsafe extern "C" fn(Object, *const c_char, *const c_char),
    gtk_page_setup_new: unsafe extern "C" fn() -> Object,
    gtk_paper_size_new_custom: unsafe extern "C" fn(*const c_char, *const c_char, f64, f64, i32) -> Object,
    gtk_page_setup_set_paper_size: unsafe extern "C" fn(Object, Object),
    gtk_paper_size_free: unsafe extern "C" fn(Object),
    gtk_page_setup_set_top_margin: unsafe extern "C" fn(Object, f64, i32),
    gtk_page_setup_set_bottom_margin: unsafe extern "C" fn(Object, f64, i32),
    gtk_page_setup_set_left_margin: unsafe extern "C" fn(Object, f64, i32),
    gtk_page_setup_set_right_margin: unsafe extern "C" fn(Object, f64, i32),
    webkit_print_operation_new: unsafe extern "C" fn(Object) -> Object,
    webkit_print_operation_set_print_settings: unsafe extern "C" fn(Object, Object),
    webkit_print_operation_set_page_setup: unsafe extern "C" fn(Object, Object),
    webkit_print_operation_print: unsafe extern "C" fn(Object),
}

struct State<'a> {
    api: &'a Api,
    loaded: bool,
    error: Option<String>,
    script: Option<String>,
    snapshot: Object,
    printed: bool,
}

unsafe fn error_text(error: *mut GError) -> String {
    if error.is_null() {
        return crate::i18n::runtime::RuntimeText::InvalidOutput
            .to_string()
            .into();
    }
    unsafe {
        CStr::from_ptr((*error).message)
            .to_string_lossy()
            .into_owned()
    }
}

unsafe extern "C" fn loaded(_: Object, event: i32, data: Object) {
    if event == 3 {
        unsafe { (*(data as *mut State<'_>)).loaded = true };
    }
}

unsafe extern "C" fn load_failed(
    _: Object,
    _: i32,
    _: *const c_char,
    error: *mut GError,
    data: Object,
) -> i32 {
    unsafe { (*(data as *mut State<'_>)).error = Some(error_text(error)) };
    1
}

unsafe extern "C" fn script_finished(view: Object, result: Object, data: Object) {
    unsafe {
        let state = &mut *(data as *mut State<'_>);
        let mut error = ptr::null_mut();
        let result = (state.api.webkit_web_view_run_javascript_finish)(view, result, &mut error);
        if result.is_null() {
            state.error = Some(error_text(error));
        } else {
            let value = (state.api.webkit_javascript_result_get_js_value)(result);
            let text = (state.api.jsc_value_to_string)(value);
            if text.is_null() {
                state.error = Some(
                    crate::i18n::runtime::RuntimeText::InvalidOutput
                        .to_string()
                        .into(),
                );
            } else {
                state.script = Some(CStr::from_ptr(text).to_string_lossy().into_owned());
                (state.api.g_free)(text.cast());
            }
            (state.api.webkit_javascript_result_unref)(result);
        }
        if !error.is_null() {
            (state.api.g_error_free)(error);
        }
    }
}

unsafe extern "C" fn snapshot_finished(view: Object, result: Object, data: Object) {
    unsafe {
        let state = &mut *(data as *mut State<'_>);
        let mut error = ptr::null_mut();
        state.snapshot = (state.api.webkit_web_view_get_snapshot_finish)(view, result, &mut error);
        if state.snapshot.is_null() {
            state.error = Some(error_text(error));
        }
        if !error.is_null() {
            (state.api.g_error_free)(error);
        }
    }
}

unsafe extern "C" fn print_finished(_: Object, data: Object) {
    unsafe { (*(data as *mut State<'_>)).printed = true };
}

unsafe extern "C" fn print_failed(_: Object, error: *mut GError, data: Object) {
    unsafe { (*(data as *mut State<'_>)).error = Some(error_text(error)) };
}

fn wait(state: &mut State<'_>, operation: &str, done: impl Fn(&State<'_>) -> bool) -> Result<()> {
    let start = Instant::now();
    loop {
        unsafe {
            (state.api.g_main_context_iteration)(ptr::null_mut(), 0);
        }
        if let Some(error) = state.error.take() {
            anyhow::bail!(crate::i18n::runtime::rt!(RenderFailed, operation, error));
        }
        if done(state) {
            return Ok(());
        }
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(45),
            crate::i18n::runtime::rt!(RenderTimedOut, operation)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn evaluate(state: &mut State<'_>, view: Object, script: &str) -> Result<String> {
    state.script = None;
    let script = CString::new(script)?;
    unsafe {
        (state.api.webkit_web_view_run_javascript)(
            view,
            script.as_ptr(),
            ptr::null_mut(),
            script_finished,
            (state as *mut State<'_>).cast(),
        );
    }
    wait(state, "WebKitGTK.run_javascript", |state| {
        state.script.is_some()
    })?;
    state
        .script
        .take()
        .context(crate::i18n::runtime::RuntimeText::InvalidOutput.to_string())
}

pub(super) fn render(request: &Request) -> Result<()> {
    unsafe {
        let api = Api::load()?;
        anyhow::ensure!(
            (api.gtk_init_check)(ptr::null_mut(), ptr::null_mut()) != 0,
            crate::i18n::runtime::RuntimeText::DesktopRequired.to_string()
        );
        let window = (api.gtk_window_new)(0);
        let context = (api.webkit_web_context_new_ephemeral)();
        let view = (api.webkit_web_view_new_with_context)(context);
        anyhow::ensure!(
            !window.is_null() && !view.is_null(),
            crate::i18n::runtime::RuntimeText::NativeRendererFailed.to_string()
        );
        let scale = if matches!(request.format, NativeFormat::Png) {
            2.0
        } else {
            1.0
        };
        (api.gtk_window_set_default_size)(
            window,
            (request.page_width * scale).ceil() as i32,
            (request.page_height * scale).ceil() as i32,
        );
        (api.webkit_web_view_set_zoom_level)(view, scale);
        (api.gtk_window_set_accept_focus)(window, 0);
        (api.gtk_window_set_focus_on_map)(window, 0);
        (api.gtk_widget_set_opacity)(window, 0.0);
        (api.gtk_container_add)(window, view);
        let settings = (api.webkit_web_view_get_settings)(view);
        (api.webkit_settings_set_enable_write_console_messages_to_stdout)(settings, 0);
        (api.webkit_settings_set_print_backgrounds)(settings, 1);
        let mut state = Box::new(State {
            api: &api,
            loaded: false,
            error: None,
            script: None,
            snapshot: ptr::null_mut(),
            printed: false,
        });
        let data = (&mut *state as *mut State<'_>).cast();
        (api.g_signal_connect_data)(
            view,
            c"load-changed".as_ptr(),
            std::mem::transmute::<unsafe extern "C" fn(Object, i32, Object), unsafe extern "C" fn()>(
                loaded,
            ),
            data,
            ptr::null_mut(),
            0,
        );
        (api.g_signal_connect_data)(
            view,
            c"load-failed".as_ptr(),
            std::mem::transmute::<
                unsafe extern "C" fn(Object, i32, *const c_char, *mut GError, Object) -> i32,
                unsafe extern "C" fn(),
            >(load_failed),
            data,
            ptr::null_mut(),
            0,
        );
        (api.gtk_widget_show_all)(window);
        if matches!(request.format, NativeFormat::Png) {
            let backing_scale = (api.gtk_widget_get_scale_factor)(view).max(1) as f64;
            let zoom = 2.0 / backing_scale;
            (api.webkit_web_view_set_zoom_level)(view, zoom);
            (api.gtk_window_resize)(
                window,
                (request.page_width * zoom).ceil() as i32,
                (request.page_height * zoom).ceil() as i32,
            );
        }
        let uri = CString::new(super::file_url(&request.html)?)?;
        (api.webkit_web_view_load_uri)(view, uri.as_ptr());
        // On error the helper exits, destroying the toolkit and any pending
        // callbacks together. No callback can survive into the editor process.
        wait(&mut state, "WebKitGTK.load_uri", |state| state.loaded)?;
        evaluate(&mut state, view, PREPARE_SCRIPT)?;
        let start = Instant::now();
        let dimensions = loop {
            let status: RenderStatus =
                serde_json::from_str(&evaluate(&mut state, view, STATUS_SCRIPT)?)?;
            if status.check()? {
                break status;
            }
            anyhow::ensure!(
                start.elapsed() < Duration::from_secs(45),
                crate::i18n::runtime::RuntimeText::ResourceLoadFailed.to_string()
            );
        };
        match request.format {
            NativeFormat::Png => {
                super::check_png_size(
                    request.page_width.ceil() * 2.0,
                    dimensions.height.ceil() * 2.0,
                )?;
                (api.webkit_web_view_get_snapshot)(
                    view,
                    1,
                    0,
                    ptr::null_mut(),
                    snapshot_finished,
                    data,
                );
                wait(&mut state, "WebKitGTK.get_snapshot", |state| {
                    !state.snapshot.is_null()
                })?;
                use std::os::unix::ffi::OsStrExt;
                let output = CString::new(request.output.as_os_str().as_bytes())?;
                let status = (api.cairo_surface_write_to_png)(state.snapshot, output.as_ptr());
                (api.cairo_surface_destroy)(state.snapshot);
                anyhow::ensure!(
                    status == 0,
                    crate::i18n::runtime::rt!(RenderFailed, "Cairo", status)
                );
            }
            NativeFormat::Pdf => {
                let print = (api.webkit_print_operation_new)(view);
                let settings = (api.gtk_print_settings_new)();
                let output = CString::new(super::file_url(&request.output)?)?;
                (api.gtk_print_settings_set)(
                    settings,
                    c"printer".as_ptr(),
                    c"Print to File".as_ptr(),
                );
                (api.gtk_print_settings_set)(
                    settings,
                    c"output-file-format".as_ptr(),
                    c"pdf".as_ptr(),
                );
                (api.gtk_print_settings_set)(settings, c"output-uri".as_ptr(), output.as_ptr());
                let setup = (api.gtk_page_setup_new)();
                // GTK_UNIT_POINTS = 1; CSS pixels are 72/96 PDF points.
                let paper = (api.gtk_paper_size_new_custom)(
                    c"moeka".as_ptr(),
                    c"Moeka".as_ptr(),
                    request.page_width * 0.75,
                    request.page_height * 0.75,
                    1,
                );
                (api.gtk_page_setup_set_paper_size)(setup, paper);
                (api.gtk_paper_size_free)(paper);
                for set in [
                    api.gtk_page_setup_set_top_margin,
                    api.gtk_page_setup_set_bottom_margin,
                    api.gtk_page_setup_set_left_margin,
                    api.gtk_page_setup_set_right_margin,
                ] {
                    set(setup, request.margin * 0.75, 1);
                }
                (api.webkit_print_operation_set_print_settings)(print, settings);
                (api.webkit_print_operation_set_page_setup)(print, setup);
                (api.g_signal_connect_data)(
                    print,
                    c"finished".as_ptr(),
                    std::mem::transmute::<
                        unsafe extern "C" fn(Object, Object),
                        unsafe extern "C" fn(),
                    >(print_finished),
                    data,
                    ptr::null_mut(),
                    0,
                );
                (api.g_signal_connect_data)(
                    print,
                    c"failed".as_ptr(),
                    std::mem::transmute::<
                        unsafe extern "C" fn(Object, *mut GError, Object),
                        unsafe extern "C" fn(),
                    >(print_failed),
                    data,
                    ptr::null_mut(),
                    0,
                );
                (api.webkit_print_operation_print)(print);
                wait(&mut state, "WebKitGTK.print", |state| state.printed)?;
                (api.g_object_unref)(print);
                (api.g_object_unref)(settings);
                (api.g_object_unref)(setup);
            }
        }
        (api.gtk_widget_destroy)(window);
        (api.g_object_unref)(context);
        // GLib may have process-global finalizers using the loaded library.
        // The helper exits immediately; keep code mapped through process exit.
        Ok(())
    }
}
