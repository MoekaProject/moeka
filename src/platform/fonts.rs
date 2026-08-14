//! Font families and font files available on the host platform.

#[cfg(not(target_os = "macos"))]
use std::path::PathBuf;

/// Family name that resolves to the platform's UI font at render time.
pub(crate) const SYSTEM_UI: &str = ".SystemUIFont";

#[cfg(target_os = "macos")]
pub(crate) const MONOSPACE: &str = "Menlo";
#[cfg(windows)]
pub(crate) const MONOSPACE: &str = "Consolas";
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) const MONOSPACE: &str = "DejaVu Sans Mono";

#[cfg(target_os = "macos")]
pub(crate) const CJK: &str = "PingFang SC";
#[cfg(windows)]
pub(crate) const CJK: &str = "Microsoft YaHei UI";
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) const CJK: &str = "Noto Sans CJK SC";

/// CJK face used for italics, which needs a family shipping an oblique cut.
#[cfg(target_os = "macos")]
pub(crate) const ITALIC_CJK: &str = "Maple Mono NF CN";
#[cfg(windows)]
pub(crate) const ITALIC_CJK: &str = "Microsoft YaHei UI";
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) const ITALIC_CJK: &str = "Noto Sans CJK SC";

/// Proportional face used where a concrete family name is required.
#[cfg(target_os = "macos")]
pub(crate) const PROPORTIONAL: &str = "Helvetica Neue";
#[cfg(windows)]
pub(crate) const PROPORTIONAL: &str = "Segoe UI";
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) const PROPORTIONAL: &str = "DejaVu Sans";

// CJK-capable files come first so Chinese text is not dropped from the PDF.
#[cfg(windows)]
const PDF_FONT_FILES: &[&str] = &[
    "C:/Windows/Fonts/msyh.ttc",
    "C:/Windows/Fonts/msyh.ttf",
    "C:/Windows/Fonts/simsun.ttc",
    "C:/Windows/Fonts/arial.ttf",
];

#[cfg(all(unix, not(target_os = "macos")))]
const PDF_FONT_FILES: &[&str] = &[
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/source-han-sans/SourceHanSansSC-Regular.otf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
];

/// Font files to try, in order, when drawing a PDF without a web view.
///
/// Callers must tolerate entries that exist but fail to parse: collections
/// and variable fonts are not loadable by every PDF backend.
#[cfg(not(target_os = "macos"))]
pub(crate) fn pdf_font_candidates() -> Vec<PathBuf> {
    PDF_FONT_FILES.iter().map(PathBuf::from).collect()
}
