//! WebKit rendering and CoreGraphics page composition, owned by the export flow.
use super::{CSS_PX_TO_PDF_PT, EXPORT_PAGE_MARGIN_PX, ExportLayout};
use objc::{class, msg_send, sel, sel_impl};
use std::path::Path;
#[link(name = "WebKit", kind = "framework")]
unsafe extern "C" {}

#[derive(Clone, Copy)]
#[repr(C)]
struct NSPoint {
    x: f64,
    y: f64,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct NSSize {
    width: f64,
    height: f64,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

fn nsstring(text: &str) -> *mut objc::runtime::Object {
    unsafe {
        let c_string = std::ffi::CString::new(text).expect("NSString input contains interior NUL");
        msg_send![class!(NSString), stringWithUTF8String: c_string.as_ptr()]
    }
}

fn nsurl_from_directory(path: &Path) -> *mut objc::runtime::Object {
    unsafe {
        msg_send![
            class!(NSURL),
            fileURLWithPath: nsstring(&path.to_string_lossy())
            isDirectory: true
        ]
    }
}

/// CoreGraphics PDF entry points the `core-graphics` crate does not expose.
#[allow(non_snake_case)]
pub(super) mod cg_pdf {
    use core_graphics::geometry::CGRect;
    use std::ffi::c_void;

    pub type CGContextRef = *mut c_void;
    pub type CGDataConsumerRef = *mut c_void;
    pub type CGDataProviderRef = *mut c_void;
    pub type CGPDFDocumentRef = *mut c_void;
    pub type CGPDFPageRef = *mut c_void;
    pub type CFDataRef = *const c_void;
    pub type CFMutableDataRef = *mut c_void;
    pub type CFDictionaryRef = *const c_void;

    pub const CG_PDF_MEDIA_BOX: i32 = 0;

    unsafe extern "C" {
        pub fn CGDataConsumerCreateWithCFData(data: CFMutableDataRef) -> CGDataConsumerRef;
        pub fn CGPDFContextCreate(
            consumer: CGDataConsumerRef,
            mediaBox: *const CGRect,
            auxiliaryInfo: CFDictionaryRef,
        ) -> CGContextRef;
        pub fn CGPDFContextBeginPage(context: CGContextRef, pageInfo: CFDictionaryRef);
        pub fn CGPDFContextEndPage(context: CGContextRef);
        pub fn CGPDFContextClose(context: CGContextRef);
        pub fn CGDataProviderCreateWithCFData(data: CFDataRef) -> CGDataProviderRef;
        pub fn CGPDFDocumentCreateWithProvider(provider: CGDataProviderRef) -> CGPDFDocumentRef;
        #[cfg_attr(not(test), allow(dead_code))]
        pub fn CGPDFDocumentGetNumberOfPages(document: CGPDFDocumentRef) -> usize;
        pub fn CGPDFDocumentGetPage(document: CGPDFDocumentRef, page: usize) -> CGPDFPageRef;
        pub fn CGPDFPageGetBoxRect(page: CGPDFPageRef, box_type: i32) -> CGRect;
        pub fn CGContextDrawPDFPage(context: CGContextRef, page: CGPDFPageRef);
        pub fn CGContextTranslateCTM(context: CGContextRef, tx: f64, ty: f64);
        pub fn CGContextScaleCTM(context: CGContextRef, sx: f64, sy: f64);
        pub fn CGContextSaveGState(context: CGContextRef);
        pub fn CGContextRestoreGState(context: CGContextRef);
        pub fn CGContextRelease(context: CGContextRef);
        pub fn CGDataConsumerRelease(consumer: CGDataConsumerRef);
        pub fn CGDataProviderRelease(provider: CGDataProviderRef);
        pub fn CGPDFDocumentRelease(document: CGPDFDocumentRef);
    }
}

/// Reports every offset a page may end at: the bottom of each block element and
/// of each line box, skipping content that must not be split.
const PDF_BREAK_OFFSET_SCRIPT: &str = r#"
(function () {
  var root = document.querySelector('.markdown-body') || document.body;
  var keep = 'pre, table, blockquote, img, figure, .export-preview';
  var seen = {};
  var add = function (value) { seen[Math.round(value)] = true; };
  var elements = root.querySelectorAll('*');
  for (var i = 0; i < elements.length; i++) {
    var element = elements[i];
    add(element.getBoundingClientRect().bottom + window.scrollY);
    if (element.children.length || element.closest(keep)) { continue; }
    var range = document.createRange();
    range.selectNodeContents(element);
    var rects = range.getClientRects();
    for (var j = 0; j < rects.length; j++) {
      add(rects[j].bottom + window.scrollY);
    }
  }
  var offsets = Object.keys(seen).map(Number).sort(function (a, b) { return a - b; });
  return JSON.stringify({ height: document.body.scrollHeight, breaks: offsets });
})()
"#;

/// Splits `total` content height into page-sized runs, preferring the lowest
/// candidate break that still fits so lines are never cut in half.
pub(super) fn pdf_page_slices(total: f64, breaks: &[f64], page_height: f64) -> Vec<(f64, f64)> {
    let mut slices = Vec::new();
    let mut top = 0.0_f64;
    while top < total - 0.5 {
        let limit = top + page_height;
        if limit >= total {
            slices.push((top, total - top));
            break;
        }
        let cut = breaks
            .iter()
            .copied()
            .filter(|offset| *offset > top + 1.0 && *offset <= limit)
            .next_back()
            .unwrap_or(limit);
        slices.push((top, cut - top));
        top = cut;
    }
    slices
}

/// Lays the export CSS out for a bare page box; the margins come from the
/// surrounding PDF page instead of `.markdown-body` padding.
pub(super) fn pdf_page_html(html: &str) -> String {
    const OVERRIDE: &str = concat!(
        "<style>",
        "html,body{margin:0;padding:0;background:#fff;}",
        ".markdown-body{max-width:none!important;padding:0!important;",
        "font-size:12pt;line-height:1.55;}",
        ".markdown-body a{color:inherit;text-decoration:none;}",
        ".markdown-body pre{white-space:pre-wrap;word-break:break-word;}",
        ".markdown-body table{display:table;width:100%;overflow:visible;}",
        ".markdown-body table th,.markdown-body table td{word-break:break-word;}",
        "</style></head>"
    );
    html.replacen("</head>", OVERRIDE, 1)
}

unsafe fn nsstring_to_string(value: *mut objc::runtime::Object) -> Option<String> {
    if value.is_null() {
        return None;
    }
    unsafe {
        let cstr: *const std::os::raw::c_char = msg_send![value, UTF8String];
        (!cstr.is_null()).then(|| {
            std::ffi::CStr::from_ptr(cstr)
                .to_string_lossy()
                .into_owned()
        })
    }
}

pub(super) async fn build_pdf_document_from_html_macos(
    html: &str,
    source_file_path: Option<&Path>,
    layout: ExportLayout,
    cx: &mut gpui::AsyncApp,
) -> anyhow::Result<Vec<u8>> {
    let content_width = layout.content_width_px;
    let content_height = layout.content_height_px();
    let html = pdf_page_html(html);

    unsafe {
        // Lay out at the printable width so the measured line boxes match the
        // slices that are later composed onto the configured-width pages.
        let frame = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: content_width,
                height: content_height,
            },
        };
        let nil = std::ptr::null_mut::<objc::runtime::Object>();
        // WKWebView renders its content out-of-process, so (unlike the legacy
        // WebHTMLView, which laid out but drew nothing here) it produces real PDF
        // output via `createPDFWithConfiguration:` even from a hidden window.
        let configuration: *mut objc::runtime::Object =
            msg_send![class!(WKWebViewConfiguration), new];
        let web_view: *mut objc::runtime::Object = msg_send![class!(WKWebView), alloc];
        let web_view: *mut objc::runtime::Object =
            msg_send![web_view, initWithFrame: frame configuration: configuration];

        // Host the WebView in a window placed ON SCREEN but fully transparent
        // (alpha 0); an offscreen or never-ordered window renders blank pages.
        let host_window: *mut objc::runtime::Object = msg_send![class!(NSWindow), alloc];
        let host_window: *mut objc::runtime::Object = msg_send![
            host_window,
            initWithContentRect: frame
            styleMask: 0u64
            backing: 2u64
            defer: false
        ];
        let () = msg_send![host_window, setReleasedWhenClosed: false];
        let () = msg_send![host_window, setAlphaValue: 0.0_f64];
        let () = msg_send![host_window, setIgnoresMouseEvents: true];
        let () = msg_send![host_window, setContentView: web_view];
        let on_screen_origin = NSPoint { x: 0.0, y: 0.0 };
        let () = msg_send![host_window, setFrameOrigin: on_screen_origin];
        let () = msg_send![host_window, orderFrontRegardless];

        let base_url = source_file_path
            .and_then(Path::parent)
            .map(nsurl_from_directory)
            .unwrap_or(nil);
        let () = msg_send![web_view, loadHTMLString: nsstring(&html) baseURL: base_url];

        let start = std::time::Instant::now();
        loop {
            // Yield to GPUI's real main run loop so WKWebView's out-of-process
            // content can actually load. A nested `runUntilDate` pump does not
            // service the WebContent process, which made loading hang.
            cx.background_executor()
                .timer(std::time::Duration::from_millis(20))
                .await;
            let is_loading: bool = msg_send![web_view, isLoading];
            if !is_loading {
                break;
            }
            if start.elapsed() > std::time::Duration::from_secs(20) {
                let () = msg_send![host_window, close];
                anyhow::bail!("timed out waiting for WebKit HTML rendering");
            }
        }
        // Allow a final layout/paint pass before measuring.
        cx.background_executor()
            .timer(std::time::Duration::from_millis(100))
            .await;

        let measurement: std::rc::Rc<std::cell::RefCell<Option<Result<String, String>>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let measurement_slot = measurement.clone();
        let measure_completion = block::ConcreteBlock::new(
            move |value: *mut objc::runtime::Object, error: *mut objc::runtime::Object| {
                let outcome = match nsstring_to_string(value) {
                    Some(text) => Ok(text),
                    None => Err(nsstring_to_string(msg_send![error, localizedDescription])
                        .unwrap_or_else(|| "page measurement returned nothing".to_string())),
                };
                *measurement_slot.borrow_mut() = Some(outcome);
            },
        );
        let measure_completion = measure_completion.copy();
        let () = msg_send![
            web_view,
            evaluateJavaScript: nsstring(PDF_BREAK_OFFSET_SCRIPT)
            completionHandler: &*measure_completion
        ];

        let measure_start = std::time::Instant::now();
        while measurement.borrow().is_none() {
            if measure_start.elapsed() > std::time::Duration::from_secs(20) {
                break;
            }
            cx.background_executor()
                .timer(std::time::Duration::from_millis(20))
                .await;
        }
        let measured = match measurement.borrow_mut().take() {
            Some(Ok(text)) => text,
            Some(Err(message)) => {
                let () = msg_send![host_window, close];
                anyhow::bail!("failed to measure page breaks: {message}");
            }
            None => {
                let () = msg_send![host_window, close];
                anyhow::bail!("timed out measuring page breaks");
            }
        };

        #[derive(serde::Deserialize)]
        struct Measurement {
            height: f64,
            breaks: Vec<f64>,
        }
        let measured: Measurement = match serde_json::from_str(&measured) {
            Ok(value) => value,
            Err(error) => {
                let () = msg_send![host_window, close];
                anyhow::bail!("failed to parse page breaks: {error}");
            }
        };

        let slices = pdf_page_slices(measured.height, &measured.breaks, content_height);
        let mut pages: Vec<Vec<u8>> = Vec::with_capacity(slices.len());
        for (top, height) in slices {
            let slot: std::rc::Rc<std::cell::RefCell<Option<Result<Vec<u8>, String>>>> =
                std::rc::Rc::new(std::cell::RefCell::new(None));
            let block_slot = slot.clone();
            let completion = block::ConcreteBlock::new(
                move |pdf_data: *mut objc::runtime::Object, error: *mut objc::runtime::Object| {
                    let outcome = if pdf_data.is_null() {
                        Err(nsstring_to_string(msg_send![error, localizedDescription])
                            .unwrap_or_else(|| "createPDF returned no data".to_string()))
                    } else {
                        let length: usize = msg_send![pdf_data, length];
                        let ptr: *const u8 = msg_send![pdf_data, bytes];
                        if ptr.is_null() || length == 0 {
                            Err("createPDF returned empty data".to_string())
                        } else {
                            Ok(std::slice::from_raw_parts(ptr, length).to_vec())
                        }
                    };
                    *block_slot.borrow_mut() = Some(outcome);
                },
            );
            let completion = completion.copy();
            let pdf_config: *mut objc::runtime::Object = msg_send![class!(WKPDFConfiguration), new];
            let slice_rect = core_graphics::geometry::CGRect::new(
                &core_graphics::geometry::CGPoint::new(0.0, top),
                &core_graphics::geometry::CGSize::new(content_width, height),
            );
            let () = msg_send![pdf_config, setRect: slice_rect];
            let () = msg_send![
                web_view,
                createPDFWithConfiguration: pdf_config
                completionHandler: &*completion
            ];

            let wait_start = std::time::Instant::now();
            while slot.borrow().is_none() {
                if wait_start.elapsed() > std::time::Duration::from_secs(30) {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(20))
                    .await;
            }
            match slot.borrow_mut().take() {
                Some(Ok(bytes)) => pages.push(bytes),
                Some(Err(message)) => {
                    let () = msg_send![host_window, close];
                    anyhow::bail!("WKWebView createPDF failed: {message}");
                }
                None => {
                    let () = msg_send![host_window, close];
                    anyhow::bail!("timed out waiting for WKWebView PDF generation");
                }
            }
        }

        let () = msg_send![host_window, close];

        if pages.is_empty() {
            anyhow::bail!("WKWebView produced no PDF pages");
        }
        compose_pdf_pages(&pages, layout)
    }
}

/// Places rendered CSS-pixel slices on configured-width pages, converting
/// pixels to PDF points so font sizes match the HTML print stylesheet.
pub(super) fn compose_pdf_pages(
    pages: &[Vec<u8>],
    layout: ExportLayout,
) -> anyhow::Result<Vec<u8>> {
    use cg_pdf::*;
    use core_graphics::geometry::{CGPoint, CGRect, CGSize};

    unsafe {
        let output: *mut objc::runtime::Object = msg_send![class!(NSMutableData), data];
        let consumer = CGDataConsumerCreateWithCFData(output as CFMutableDataRef);
        if consumer.is_null() {
            anyhow::bail!("failed to create PDF data consumer");
        }
        let media_box = CGRect::new(
            &CGPoint::new(0.0, 0.0),
            &CGSize::new(
                layout.page_width_px * CSS_PX_TO_PDF_PT,
                layout.page_height_px * CSS_PX_TO_PDF_PT,
            ),
        );
        let context = CGPDFContextCreate(consumer, &media_box, std::ptr::null());
        CGDataConsumerRelease(consumer);
        if context.is_null() {
            anyhow::bail!("failed to create PDF context");
        }

        for page in pages {
            let data: *mut objc::runtime::Object = msg_send![
                class!(NSData),
                dataWithBytes: page.as_ptr() as *const std::ffi::c_void
                length: page.len()
            ];
            let provider = CGDataProviderCreateWithCFData(data as CFDataRef);
            if provider.is_null() {
                continue;
            }
            let document = CGPDFDocumentCreateWithProvider(provider);
            CGDataProviderRelease(provider);
            if document.is_null() {
                continue;
            }
            let pdf_page = CGPDFDocumentGetPage(document, 1);
            if !pdf_page.is_null() {
                let box_rect = CGPDFPageGetBoxRect(pdf_page, CG_PDF_MEDIA_BOX);
                CGPDFContextBeginPage(context, std::ptr::null());
                CGContextSaveGState(context);
                CGContextTranslateCTM(
                    context,
                    EXPORT_PAGE_MARGIN_PX * CSS_PX_TO_PDF_PT,
                    (layout.page_height_px - EXPORT_PAGE_MARGIN_PX - box_rect.size.height)
                        * CSS_PX_TO_PDF_PT,
                );
                CGContextScaleCTM(context, CSS_PX_TO_PDF_PT, CSS_PX_TO_PDF_PT);
                CGContextDrawPDFPage(context, pdf_page);
                CGContextRestoreGState(context);
                CGPDFContextEndPage(context);
            }
            CGPDFDocumentRelease(document);
        }

        CGPDFContextClose(context);
        CGContextRelease(context);

        let length: usize = msg_send![output, length];
        let ptr: *const u8 = msg_send![output, bytes];
        if ptr.is_null() || length == 0 {
            anyhow::bail!("composed PDF is empty");
        }
        Ok(std::slice::from_raw_parts(ptr, length).to_vec())
    }
}

/// Renders the same HTML the PDF export uses into a single full-page PNG.
pub(super) async fn build_png_document_from_html_macos(
    html: &str,
    source_file_path: Option<&Path>,
    layout: ExportLayout,
    cx: &mut gpui::AsyncApp,
) -> anyhow::Result<Vec<u8>> {
    unsafe {
        // The outer width includes margins; the article's usable width is
        // exactly the setting. The height grows to fit the whole document.
        let page_width = layout.page_width_px;
        let mut frame = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: page_width,
                height: layout.page_height_px,
            },
        };
        let nil = std::ptr::null_mut::<objc::runtime::Object>();
        let configuration: *mut objc::runtime::Object =
            msg_send![class!(WKWebViewConfiguration), new];
        let web_view: *mut objc::runtime::Object = msg_send![class!(WKWebView), alloc];
        let web_view: *mut objc::runtime::Object =
            msg_send![web_view, initWithFrame: frame configuration: configuration];

        let host_window: *mut objc::runtime::Object = msg_send![class!(NSWindow), alloc];
        let host_window: *mut objc::runtime::Object = msg_send![
            host_window,
            initWithContentRect: frame
            styleMask: 0u64
            backing: 2u64
            defer: false
        ];
        let () = msg_send![host_window, setReleasedWhenClosed: false];
        let () = msg_send![host_window, setAlphaValue: 0.0_f64];
        let () = msg_send![host_window, setIgnoresMouseEvents: true];
        let () = msg_send![host_window, setContentView: web_view];
        let () = msg_send![host_window, orderFrontRegardless];

        let base_url = source_file_path
            .and_then(Path::parent)
            .map(nsurl_from_directory)
            .unwrap_or(nil);
        let () = msg_send![web_view, loadHTMLString: nsstring(html) baseURL: base_url];

        let start = std::time::Instant::now();
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(20))
                .await;
            let is_loading: bool = msg_send![web_view, isLoading];
            if !is_loading {
                break;
            }
            if start.elapsed() > std::time::Duration::from_secs(20) {
                let () = msg_send![host_window, close];
                anyhow::bail!("timed out waiting for WebKit HTML rendering");
            }
        }
        cx.background_executor()
            .timer(std::time::Duration::from_millis(100))
            .await;

        // Grow the view to the full document height so the snapshot is not
        // clipped to one viewport.
        let height_slot: std::rc::Rc<std::cell::RefCell<Option<f64>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let height_block_slot = height_slot.clone();
        let height_completion = block::ConcreteBlock::new(
            move |value: *mut objc::runtime::Object, _error: *mut objc::runtime::Object| {
                let height = if value.is_null() {
                    0.0
                } else {
                    msg_send![value, doubleValue]
                };
                *height_block_slot.borrow_mut() = Some(height);
            },
        );
        let height_completion = height_completion.copy();
        let () = msg_send![
            web_view,
            evaluateJavaScript: nsstring("document.documentElement.scrollHeight")
            completionHandler: &*height_completion
        ];
        let height_start = std::time::Instant::now();
        while height_slot.borrow().is_none() {
            if height_start.elapsed() > std::time::Duration::from_secs(10) {
                break;
            }
            cx.background_executor()
                .timer(std::time::Duration::from_millis(20))
                .await;
        }
        let content_height = height_slot
            .borrow_mut()
            .take()
            .filter(|height| *height > 1.0)
            .unwrap_or(frame.size.height)
            .min(20_000.0);
        frame.size.height = content_height;
        let () = msg_send![host_window, setContentSize: frame.size];
        let () = msg_send![web_view, setFrame: frame];
        cx.background_executor()
            .timer(std::time::Duration::from_millis(200))
            .await;

        let result: std::rc::Rc<std::cell::RefCell<Option<Result<Vec<u8>, String>>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let block_result = result.clone();
        let completion = block::ConcreteBlock::new(
            move |image: *mut objc::runtime::Object, error: *mut objc::runtime::Object| {
                let outcome = if image.is_null() {
                    let message = if error.is_null() {
                        "takeSnapshot returned no image".to_string()
                    } else {
                        let desc: *mut objc::runtime::Object =
                            msg_send![error, localizedDescription];
                        let cstr: *const std::os::raw::c_char = msg_send![desc, UTF8String];
                        if cstr.is_null() {
                            "unknown takeSnapshot error".to_string()
                        } else {
                            std::ffi::CStr::from_ptr(cstr)
                                .to_string_lossy()
                                .into_owned()
                        }
                    };
                    Err(message)
                } else {
                    let tiff: *mut objc::runtime::Object = msg_send![image, TIFFRepresentation];
                    if tiff.is_null() {
                        Err("snapshot image had no bitmap data".to_string())
                    } else {
                        let rep: *mut objc::runtime::Object =
                            msg_send![class!(NSBitmapImageRep), imageRepWithData: tiff];
                        let properties: *mut objc::runtime::Object =
                            msg_send![class!(NSDictionary), dictionary];
                        // NSBitmapImageFileTypePNG
                        let png: *mut objc::runtime::Object =
                            msg_send![rep, representationUsingType: 4u64 properties: properties];
                        let length: usize = msg_send![png, length];
                        let ptr: *const u8 = msg_send![png, bytes];
                        if png.is_null() || ptr.is_null() || length == 0 {
                            Err("PNG encoding produced no data".to_string())
                        } else {
                            Ok(std::slice::from_raw_parts(ptr, length).to_vec())
                        }
                    }
                };
                *block_result.borrow_mut() = Some(outcome);
            },
        );
        let completion = completion.copy();
        let snapshot_config: *mut objc::runtime::Object =
            msg_send![class!(WKSnapshotConfiguration), new];
        let () = msg_send![snapshot_config, setRect: frame];
        // snapshotWidth is in points, and the bitmap also carries the window's
        // backing scale. Avoid accidentally producing 4x images on Retina.
        let backing_scale: f64 = msg_send![host_window, backingScaleFactor];
        let snapshot_width: *mut objc::runtime::Object = msg_send![
            class!(NSNumber),
            numberWithDouble: layout.png_snapshot_width_pt(backing_scale)
        ];
        let () = msg_send![snapshot_config, setSnapshotWidth: snapshot_width];
        let () = msg_send![
            web_view,
            takeSnapshotWithConfiguration: snapshot_config
            completionHandler: &*completion
        ];

        let wait_start = std::time::Instant::now();
        while result.borrow().is_none() {
            if wait_start.elapsed() > std::time::Duration::from_secs(30) {
                break;
            }
            cx.background_executor()
                .timer(std::time::Duration::from_millis(20))
                .await;
        }

        let () = msg_send![host_window, close];

        match result.borrow_mut().take() {
            Some(Ok(bytes)) => Ok(bytes),
            Some(Err(message)) => anyhow::bail!("WKWebView takeSnapshot failed: {message}"),
            None => anyhow::bail!("timed out waiting for WKWebView PNG generation"),
        }
    }
}
