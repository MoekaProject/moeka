//! Native HTML export helpers. Helpers own their toolkit's main thread and
//! temporary profile; the editor only receives completed PDF/PNG bytes.
#[cfg(not(target_os = "macos"))]
use std::path::Path;
#[cfg(any(not(target_os = "macos"), test))]
use std::path::PathBuf;
#[cfg(not(target_os = "macos"))]
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

pub(crate) const MAX_PNG_PIXELS: f64 = 64.0 * 1024.0 * 1024.0;

pub(crate) const PREPARE_SCRIPT: &str = r#"
(() => {
  window.__moekaExport = {ready:false};
  (async () => {
    try {
      await document.fonts.ready;
      await Promise.all(Array.from(document.images, image => {
        if (image.complete) {
          if (!image.naturalWidth) throw new Error('moeka-image-load');
          return image.decode ? image.decode() : Promise.resolve();
        }
        return new Promise((resolve, reject) => {
          image.onload = resolve;
          image.onerror = () => reject(new Error('moeka-image-load'));
        });
      }));
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      window.__moekaExport = {ready:true};
    } catch (error) {
      window.__moekaExport = {error:String(error)};
    }
  })();
  return true;
})()
"#;

pub(crate) const STATUS_SCRIPT: &str = r#"
JSON.stringify(Object.assign({}, window.__moekaExport || {ready:false}, {
  width:document.documentElement.scrollWidth,
  height:Math.max(document.documentElement.scrollHeight, document.body.scrollHeight)
}))
"#;

#[derive(serde::Deserialize)]
pub(crate) struct RenderStatus {
    #[serde(default)]
    pub ready: bool,
    pub error: Option<String>,
    pub width: f64,
    pub height: f64,
}

impl RenderStatus {
    pub(crate) fn check(&self) -> Result<bool> {
        if let Some(error) = &self.error {
            if error == "Error: moeka-image-load" {
                anyhow::bail!(crate::i18n::runtime::RuntimeText::ResourceLoadFailed);
            }
            anyhow::bail!(crate::i18n::runtime::rt!(RenderFailed, error));
        }
        anyhow::ensure!(
            self.width.is_finite()
                && self.height.is_finite()
                && self.width > 0.0
                && self.height > 0.0,
            crate::i18n::runtime::RuntimeText::InvalidDimensions.to_string()
        );
        Ok(self.ready)
    }
}

pub(crate) fn check_png_size(width: f64, height: f64) -> Result<()> {
    anyhow::ensure!(
        width.is_finite()
            && height.is_finite()
            && width > 0.0
            && height > 0.0
            && width * height <= MAX_PNG_PIXELS
            && width <= 32767.0
            && height <= 32767.0,
        crate::i18n::runtime::RuntimeText::PngTooLarge.to_string()
    );
    Ok(())
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub(crate) enum NativeFormat {
    Pdf,
    Png,
}

#[cfg(any(not(target_os = "macos"), test))]
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Request {
    pub language: String,
    pub html: PathBuf,
    pub output: PathBuf,
    pub format: NativeFormat,
    pub page_width: f64,
    pub page_height: f64,
    pub margin: f64,
}

#[cfg(not(target_os = "macos"))]
const HELPER_ARGUMENT: &str = "--internal-native-export";

#[cfg(not(target_os = "macos"))]
pub(crate) fn run(
    html: &str,
    format: NativeFormat,
    page_width: f64,
    page_height: f64,
    margin: f64,
) -> Result<Vec<u8>> {
    use std::process::{Command, Stdio};
    anyhow::ensure!(
        super::capabilities::CHILD_PROCESSES,
        crate::i18n::runtime::RuntimeText::FeatureUnavailable.to_string()
    );
    let directory = tempfile::tempdir()
        .context(crate::i18n::runtime::RuntimeText::TempDirectory.to_string())?;
    let request = Request {
        language: crate::i18n::runtime::language().code().to_string(),
        html: directory.path().join("document.html"),
        output: directory.path().join("output"),
        format,
        page_width,
        page_height,
        margin,
    };
    std::fs::write(&request.html, html)
        .context(crate::i18n::runtime::RuntimeText::PrepareExport.to_string())?;
    let request_path = directory.path().join("request.json");
    std::fs::write(&request_path, serde_json::to_vec(&request)?)
        .context(crate::i18n::runtime::RuntimeText::PrepareExport.to_string())?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg(HELPER_ARGUMENT)
        .arg(&request_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = command
        .spawn()
        .context(crate::i18n::runtime::RuntimeText::ExportProcess.to_string())?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < Duration::from_secs(120) => {
                std::thread::sleep(Duration::from_millis(20));
            }
            outcome => {
                child
                    .kill()
                    .context(crate::i18n::runtime::RuntimeText::ExportProcess.to_string())?;
                child
                    .wait()
                    .context(crate::i18n::runtime::RuntimeText::ExportProcess.to_string())?;
                outcome.context(crate::i18n::runtime::RuntimeText::ExportProcess.to_string())?;
                anyhow::bail!(crate::i18n::runtime::RuntimeText::RenderTimedOut.to_string());
            }
        }
    };
    if !status.success() {
        let error = std::fs::read_to_string(request_path.with_extension("error"))
            .context(crate::i18n::runtime::RuntimeText::ExportProcess.to_string())?;
        anyhow::bail!("{error}");
    }
    let bytes = std::fs::read(&request.output)
        .context(crate::i18n::runtime::RuntimeText::ReadFile.to_string())?;
    validate_output(&bytes, format)?;
    Ok(bytes)
}

pub(crate) fn validate_output(bytes: &[u8], format: NativeFormat) -> Result<()> {
    match format {
        NativeFormat::Pdf => anyhow::ensure!(
            bytes.starts_with(b"%PDF-"),
            crate::i18n::runtime::RuntimeText::InvalidOutput.to_string()
        ),
        NativeFormat::Png => {
            anyhow::ensure!(
                bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
                crate::i18n::runtime::RuntimeText::InvalidOutput.to_string()
            );
            let dimensions = image::ImageReader::with_format(
                std::io::Cursor::new(bytes),
                image::ImageFormat::Png,
            )
            .into_dimensions()
            .context(crate::i18n::runtime::RuntimeText::InvalidDimensions.to_string())?;
            check_png_size(f64::from(dimensions.0), f64::from(dimensions.1))?;
            image::load_from_memory(bytes)
                .context(crate::i18n::runtime::RuntimeText::InvalidOutput.to_string())?;
        }
    }
    Ok(())
}

/// Runs before GPUI, single-instance forwarding, or session restoration.
#[cfg(not(target_os = "macos"))]
pub(crate) fn helper_main() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(HELPER_ARGUMENT)) {
        return None;
    }
    let Some(path) = args.next().map(PathBuf::from) else {
        return Some(2);
    };
    let result = (|| {
        anyhow::ensure!(
            args.next().is_none(),
            crate::i18n::runtime::RuntimeText::InvalidRequest.to_string()
        );
        let request: Request = serde_json::from_slice(&std::fs::read(&path)?)?;
        crate::i18n::runtime::set_language(
            crate::i18n::UiLanguage::from_code(&request.language)
                .context(crate::i18n::runtime::RuntimeText::InvalidRequest)?,
        );
        anyhow::ensure!(
            request.page_width.is_finite()
                && request.page_width > 0.0
                && request.page_height.is_finite()
                && request.page_height > 0.0
                && request.margin.is_finite()
                && request.margin >= 0.0
                && request.page_width > request.margin * 2.0,
            crate::i18n::runtime::RuntimeText::InvalidDimensions.to_string()
        );
        #[cfg(windows)]
        windows::render(&request)?;
        #[cfg(target_os = "linux")]
        linux::render(&request)?;
        Ok::<_, anyhow::Error>(())
    })();
    Some(match result {
        Ok(()) => 0,
        Err(error) => {
            if let Err(write_error) =
                std::fs::write(path.with_extension("error"), format!("{error:#}"))
            {
                eprintln!("Could not record export failure: {write_error}; {error:#}");
            }
            1
        }
    })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn file_url(path: &Path) -> Result<String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    super::paths::path_to_file_url(&absolute)
        .context(crate::i18n::runtime::RuntimeText::InvalidPath.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_export_request_preserves_every_interface_locale() {
        for &language in crate::i18n::UiLanguage::supported() {
            let request = Request {
                language: language.code().to_string(),
                html: "document.html".into(),
                output: "document.pdf".into(),
                format: NativeFormat::Pdf,
                page_width: 800.0,
                page_height: 1100.0,
                margin: 20.0,
            };
            let decoded: Request =
                serde_json::from_slice(&serde_json::to_vec(&request).unwrap()).unwrap();
            assert_eq!(
                crate::i18n::UiLanguage::from_code(&decoded.language),
                Some(language)
            );
            assert_eq!(decoded.html, request.html);
            assert_eq!(decoded.output, request.output);
        }
    }

    #[test]
    fn native_export_readiness_translates_owned_errors_but_not_external_causes() {
        use crate::i18n::{UiLanguage, runtime};
        runtime::set_language(UiLanguage::ChineseSimplified);
        let mut status = RenderStatus {
            ready: false,
            error: Some("Error: moeka-image-load".into()),
            width: 100.0,
            height: 100.0,
        };
        assert_eq!(
            status.check().unwrap_err().to_string(),
            "无法加载导出所需的图片或字体"
        );
        status.error = Some("WebKit E_ACCESSDENIED".into());
        let message = status.check().unwrap_err().to_string();
        assert!(message.starts_with("无法渲染文档"));
        assert!(message.ends_with("WebKit E_ACCESSDENIED"));
        runtime::set_language(UiLanguage::English);
    }

    #[test]
    fn native_export_rejects_truncation_and_invalid_output() {
        assert!(check_png_size(2000.0, 10000.0).is_ok());
        assert!(check_png_size(2000.0, 40000.0).is_err());
        assert!(check_png_size(f64::NAN, 100.0).is_err());
        assert!(validate_output(b"plain text", NativeFormat::Pdf).is_err());
        assert!(validate_output(b"", NativeFormat::Png).is_err());
    }
}
