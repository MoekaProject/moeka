use std::path::Path;

pub const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "apng", "jpg", "jpeg", "jpe", "jfif", "gif", "webp", "bmp", "ico", "tif", "tiff",
    "avif", "svg", "heic", "heif", "hif", "dds", "exr", "ff", "hdr", "pbm", "pgm", "ppm", "pam",
    "pnm", "qoi", "tga", "targa", "jxl",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentKind {
    Markdown,
    Image,
    Pdf,
    Text,
    Binary,
}

impl ContentKind {
    pub fn supports_text_search(self) -> bool {
        matches!(self, Self::Markdown | Self::Text)
    }
}

pub fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdx"
            )
        })
}

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            IMAGE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        })
}

pub fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.contains(&0) {
        return false;
    }
    if bytes.is_empty() {
        return true;
    }
    let sample = &bytes[..bytes.len().min(4096)];
    let controls = sample
        .iter()
        .filter(|&&byte| byte < 0x20 && !matches!(byte, b'\n' | b'\r' | b'\t' | 0x08 | 0x0c))
        .count();
    if controls * 10 > sample.len() {
        return false;
    }
    if std::str::from_utf8(sample).is_ok() {
        return true;
    }
    let lossy = String::from_utf8_lossy(sample);
    let replacements = lossy.chars().filter(|&ch| ch == '\u{fffd}').count();
    replacements * 20 <= lossy.chars().count().max(1)
}

pub fn classify_content(path: &Path, bytes: &[u8]) -> ContentKind {
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
        || bytes.starts_with(b"%PDF-")
    {
        ContentKind::Pdf
    } else if is_markdown_path(path) {
        ContentKind::Markdown
    } else if is_image_path(path) {
        ContentKind::Image
    } else if looks_like_text(bytes) {
        ContentKind::Text
    } else {
        ContentKind::Binary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_preserves_markdown_pdf_and_binary_precedence() {
        assert_eq!(
            classify_content(Path::new("x.MDX"), b"\0\xff"),
            ContentKind::Markdown
        );
        assert_eq!(
            classify_content(Path::new("x.md"), b"%PDF-1.7"),
            ContentKind::Pdf
        );
        assert_eq!(
            classify_content(Path::new("x.PDF"), b"text"),
            ContentKind::Pdf
        );
        for extension in IMAGE_EXTENSIONS {
            assert_eq!(
                classify_content(&Path::new("x").with_extension(extension), b"text"),
                ContentKind::Image
            );
        }
        let mut bytes = vec![b'x'; 8192];
        bytes.push(0);
        assert_eq!(
            classify_content(Path::new("x.txt"), &bytes),
            ContentKind::Binary
        );
    }

    #[test]
    fn classification_uses_existing_control_and_replacement_thresholds() {
        let mut bytes = vec![b'x'; 100];
        bytes[..10].fill(1);
        assert!(looks_like_text(&bytes));
        bytes[10] = 1;
        assert!(!looks_like_text(&bytes));
        bytes.fill(b'x');
        bytes[..5].fill(0xff);
        assert!(looks_like_text(&bytes));
        bytes[5] = 0xff;
        assert!(!looks_like_text(&bytes));
        assert!(looks_like_text(b"\n\r\t\x08\x0c"));
    }
}
