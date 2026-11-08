use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use base64::Engine;
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
        UI::WindowsAndMessaging::*,
    },
    core::{HSTRING, Interface, PCWSTR},
};

use super::{NativeFormat, PREPARE_SCRIPT, RenderStatus, Request, STATUS_SCRIPT};

type Slot<T> = Rc<RefCell<Option<Result<T>>>>;

fn pump() {
    unsafe {
        let mut message = MSG::default();
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn wait<T>(slot: &Slot<T>, operation: &str) -> Result<T> {
    let start = Instant::now();
    loop {
        pump();
        if let Some(result) = slot.borrow_mut().take() {
            return result.with_context(|| crate::i18n::runtime::rt!(RenderFailed, operation));
        }
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(45),
            crate::i18n::runtime::rt!(RenderTimedOut, operation)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct Host(HWND);
impl Drop for Host {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
            CoUninitialize();
        }
    }
}

struct Controller(ICoreWebView2Controller);
impl Drop for Controller {
    fn drop(&mut self) {
        if let Err(error) = unsafe { self.0.Close() } {
            eprintln!("Close export WebView2: {error}");
        }
    }
}

fn evaluate(view: &ICoreWebView2, script: &str) -> Result<serde_json::Value> {
    let slot: Slot<String> = Rc::default();
    let result = slot.clone();
    let callback = ExecuteScriptCompletedHandler::create(Box::new(move |status, value| {
        *result.borrow_mut() = Some(status.map(|()| value).map_err(Into::into));
        Ok(())
    }));
    unsafe { view.ExecuteScript(&HSTRING::from(script), &callback)? };
    Ok(serde_json::from_str(&wait(
        &slot,
        "WebView2.ExecuteScript",
    )?)?)
}

fn devtools(
    view: &ICoreWebView2,
    method: &str,
    parameters: serde_json::Value,
) -> Result<serde_json::Value> {
    let slot: Slot<String> = Rc::default();
    let result = slot.clone();
    let callback =
        CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |status, value| {
            *result.borrow_mut() = Some(status.map(|()| value).map_err(Into::into));
            Ok(())
        }));
    unsafe {
        view.CallDevToolsProtocolMethod(
            &HSTRING::from(method),
            &HSTRING::from(parameters.to_string()),
            &callback,
        )?
    };
    Ok(serde_json::from_str(&wait(&slot, method)?)?)
}

pub(super) fn render(request: &Request) -> Result<()> {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            windows::core::w!("STATIC"),
            windows::core::w!("Moeka export"),
            WS_POPUP | WS_VISIBLE,
            -32000,
            -32000,
            request.page_width.ceil() as i32,
            request.page_height.ceil() as i32,
            None,
            None,
            None,
            None,
        )?
    };
    let _host = Host(hwnd);
    let profile = request.output.with_extension("webview-profile");
    let environment_slot: Slot<ICoreWebView2Environment> = Rc::default();
    let result = environment_slot.clone();
    let callback = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |status, environment| {
            *result.borrow_mut() = Some(status.map_err(anyhow::Error::from).and_then(|()| {
                environment
                    .context(crate::i18n::runtime::RuntimeText::NativeRendererFailed.to_string())
            }));
            Ok(())
        },
    ));
    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            &HSTRING::from(profile.as_os_str()),
            None,
            &callback,
        )
    }
    .context(crate::i18n::runtime::RuntimeText::WebView2Required.to_string())?;
    let environment = wait(&environment_slot, "WebView2.Environment")?;
    let controller_slot: Slot<ICoreWebView2Controller> = Rc::default();
    let result = controller_slot.clone();
    let callback = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |status, controller| {
            *result.borrow_mut() = Some(status.map_err(anyhow::Error::from).and_then(|()| {
                controller
                    .context(crate::i18n::runtime::RuntimeText::NativeRendererFailed.to_string())
            }));
            Ok(())
        },
    ));
    unsafe { environment.CreateCoreWebView2Controller(hwnd, &callback)? };
    let controller = Controller(wait(&controller_slot, "WebView2.Controller")?);
    let view = unsafe {
        controller.0.SetBounds(RECT {
            left: 0,
            top: 0,
            right: request.page_width.ceil() as i32,
            bottom: request.page_height.ceil() as i32,
        })?;
        controller.0.SetIsVisible(true)?;
        let view = controller.0.CoreWebView2()?;
        let settings = view.Settings()?;
        settings.SetAreDefaultScriptDialogsEnabled(false)?;
        settings.SetAreDevToolsEnabled(false)?;
        settings.SetAreDefaultContextMenusEnabled(false)?;
        settings.SetIsWebMessageEnabled(false)?;
        view
    };
    let loaded: Slot<()> = Rc::default();
    let result = loaded.clone();
    let callback = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
        let outcome = (|| -> Result<()> {
            let args =
                args.context(crate::i18n::runtime::RuntimeText::InvalidOutput.to_string())?;
            let mut success = windows::core::BOOL::default();
            unsafe { args.IsSuccess(&mut success)? };
            anyhow::ensure!(
                success.as_bool(),
                crate::i18n::runtime::RuntimeText::RenderFailed.to_string()
            );
            Ok(())
        })();
        *result.borrow_mut() = Some(outcome);
        Ok(())
    }));
    let mut token = 0;
    unsafe {
        view.add_NavigationCompleted(&callback, &mut token)?;
        view.Navigate(&HSTRING::from(super::file_url(&request.html)?))?;
    }
    wait(&loaded, "WebView2.Navigate")?;
    unsafe { view.remove_NavigationCompleted(token)? };
    // Keep CSS pixels independent of Windows display scaling.
    devtools(
        &view,
        "Emulation.setDeviceMetricsOverride",
        serde_json::json!({
            "width": request.page_width.ceil() as u32,
            "height": request.page_height.ceil() as u32,
            "deviceScaleFactor": 2,
            "mobile": false
        }),
    )?;
    evaluate(&view, PREPARE_SCRIPT)?;
    let start = Instant::now();
    let dimensions = loop {
        let status = evaluate(&view, STATUS_SCRIPT)?;
        let status: RenderStatus = serde_json::from_str(
            status
                .as_str()
                .context(crate::i18n::runtime::RuntimeText::InvalidOutput.to_string())?,
        )?;
        if status.check()? {
            break status;
        }
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(45),
            crate::i18n::runtime::RuntimeText::ResourceLoadFailed.to_string()
        );
        pump();
        std::thread::sleep(Duration::from_millis(20));
    };
    match request.format {
        NativeFormat::Pdf => {
            let settings = unsafe {
                environment
                    .cast::<ICoreWebView2Environment6>()?
                    .CreatePrintSettings()?
            };
            unsafe {
                settings.SetPageWidth(request.page_width / 96.0)?;
                settings.SetPageHeight(request.page_height / 96.0)?;
                settings.SetMarginTop(request.margin / 96.0)?;
                settings.SetMarginBottom(request.margin / 96.0)?;
                settings.SetMarginLeft(request.margin / 96.0)?;
                settings.SetMarginRight(request.margin / 96.0)?;
                settings.SetShouldPrintBackgrounds(true)?;
                settings.SetShouldPrintHeaderAndFooter(false)?;
                settings.SetScaleFactor(1.0)?;
            }
            let slot: Slot<()> = Rc::default();
            let result = slot.clone();
            let callback = PrintToPdfCompletedHandler::create(Box::new(move |status, success| {
                *result.borrow_mut() = Some(status.map_err(anyhow::Error::from).and_then(|()| {
                    anyhow::ensure!(
                        success,
                        crate::i18n::runtime::RuntimeText::InvalidOutput.to_string()
                    );
                    Ok(())
                }));
                Ok(())
            }));
            unsafe {
                view.cast::<ICoreWebView2_7>()?.PrintToPdf(
                    &HSTRING::from(request.output.as_os_str()),
                    &settings,
                    &callback,
                )?
            };
            wait(&slot, "WebView2.PrintToPdf")?;
        }
        NativeFormat::Png => {
            super::check_png_size(
                request.page_width.ceil() * 2.0,
                dimensions.height.ceil() * 2.0,
            )?;
            let screenshot = devtools(
                &view,
                "Page.captureScreenshot",
                serde_json::json!({
                    "format": "png", "captureBeyondViewport": true,
                    "clip": {"x":0, "y":0, "width":request.page_width.ceil(), "height":dimensions.height.ceil(), "scale":1}
                }),
            )?;
            let data = screenshot["data"]
                .as_str()
                .context(crate::i18n::runtime::RuntimeText::InvalidOutput.to_string())?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(data)?;
            super::validate_output(&bytes, NativeFormat::Png)?;
            std::fs::write(&request.output, bytes)?;
        }
    }
    Ok(())
}
