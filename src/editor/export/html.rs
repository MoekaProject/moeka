use std::{
    collections::HashMap,
    io::{Cursor, Read},
    ops::Range,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use base64::Engine;
use lol_html::{RewriteStrSettings, element, rewrite_str};

use super::html_escape;

pub(super) fn editor_typography_css(styles: &crate::style::EditorStyles) -> String {
    fn family(name: &str) -> String {
        format!(
            "\"{}\"",
            name.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('<', "\\3c ")
        )
    }
    let mut css = format!(
        ".markdown-body{{font-family:{},{};font-size:{}px;line-height:{};}}\
         .markdown-body pre,.markdown-body code{{font-family:{};}}\
         .markdown-body em{{font-family:{},{};}}",
        family(styles.editor_body_latin_font_family),
        family(styles.editor_body_cjk_font_family),
        styles.editor_font_size_px,
        styles.editor_paragraph_line_height_factor,
        family(styles.editor_monospace_font_family),
        family(styles.editor_italic_latin_font_family),
        family(styles.editor_italic_cjk_font_family),
    );
    for (index, size) in [
        styles.editor_h1_font_size_px,
        styles.editor_h2_font_size_px,
        styles.editor_h3_font_size_px,
        styles.editor_h4_font_size_px,
        styles.editor_h5_font_size_px,
        styles.editor_h6_font_size_px,
    ]
    .into_iter()
    .enumerate()
    {
        css.push_str(&format!(
            ".markdown-body h{}{{font-family:{},{};font-size:{size}px;line-height:{};}}",
            index + 1,
            family(styles.editor_heading_latin_font_family),
            family(styles.editor_heading_cjk_font_family),
            styles.editor_heading_line_height_factor,
        ));
    }
    css
}

pub(super) fn default_typography_css() -> String {
    let theme = crate::theme::ThemeKind::PaperWhite.build();
    editor_typography_css(
        &crate::style::Styles::from_theme_with_options(&theme, &theme, false).editor,
    )
}

/// Export uses the editor's source ranges, including nesting and protected
/// code/link targets, instead of independently guessing custom delimiters.
pub(super) fn adapt_editor_syntax(source: &str) -> String {
    let decorations = crate::markdown::parse_decorations(source);
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut offset = 0;
    let front_matter_end = decorations
        .first()
        .and_then(|line| line.table_info.as_ref())
        .filter(|table| table.is_front_matter)
        .map(|table| table.table_end_line);
    let mut metadata = String::new();
    for (index, (line, decoration)) in source.split_inclusive('\n').zip(&decorations).enumerate() {
        let text = line.trim_end_matches(['\r', '\n']);
        if front_matter_end.is_some_and(|end| index < end) {
            if let Some(info) = &decoration.front_matter {
                let key = info
                    .key_range
                    .as_ref()
                    .and_then(|range| text.get(range.clone()))
                    .unwrap_or("");
                let raw_value = text.get(info.value_range.clone()).unwrap_or("");
                let value = match &info.value {
                    crate::front_matter::FrontMatterValue::Text(value) => html_escape(value),
                    crate::front_matter::FrontMatterValue::Boolean(value) => format!(
                        "<input type=\"checkbox\" disabled{}>",
                        if *value { " checked" } else { "" }
                    ),
                    crate::front_matter::FrontMatterValue::BlockHeader => String::new(),
                    _ => html_escape(raw_value),
                };
                metadata.push_str(&format!(
                    "<tr><th>{}</th><td style=\"padding-left:{}ch\">{}{}</td></tr>",
                    html_escape(key),
                    info.indent_columns,
                    html_escape(&info.display_prefix),
                    value,
                ));
            }
            offset += line.len();
            if Some(index + 1) == front_matter_end {
                edits.push((
                    0..offset,
                    format!("<table class=\"front-matter\"><tbody>{metadata}</tbody></table>\n\n"),
                ));
            }
            continue;
        }
        for (spans, delimiter, tag) in [
            (&decoration.highlight_spans, "==", "mark"),
            (&decoration.superscript_spans, "^", "sup"),
            (&decoration.subscript_spans, "~", "sub"),
        ] {
            for range in spans {
                paired_edit(&mut edits, text, offset, range, delimiter, tag);
            }
        }
        for span in &decoration.inline_spans {
            match span.kind {
                crate::markdown::InlineKind::Underline => {
                    paired_edit(&mut edits, text, offset, &span.range, "++", "u")
                }
                crate::markdown::InlineKind::Strikethrough => {
                    paired_edit(&mut edits, text, offset, &span.range, "~~", "del")
                }
                _ => {}
            }
        }
        for replacement in &decoration.display_replacements {
            if text
                .get(replacement.source_range.clone())
                .is_some_and(|text| text.starts_with(':'))
            {
                edits.push((
                    offset + replacement.source_range.start..offset + replacement.source_range.end,
                    html_escape(&replacement.display_text),
                ));
            }
        }
        for link in &decoration.wiki_links {
            if link.full_range.start > 0
                && text.as_bytes()[link.full_range.start - 1] == b'!'
                && crate::platform::image_decode::is_image_path(Path::new(&link.target_path))
            {
                edits.push((
                    offset + link.full_range.start - 1..offset + link.full_range.end,
                    format!(
                        "<img src=\"{}\" alt=\"{}\">",
                        html_escape(&link.target_path),
                        html_escape(text.get(link.collapsed_range.clone()).unwrap_or(""))
                    ),
                ));
            }
        }
        offset += line.len();
    }
    edits.sort_by_key(|(range, _)| (range.start, std::cmp::Reverse(range.end)));
    let mut output = String::with_capacity(source.len());
    let mut copied = 0;
    for (range, replacement) in edits {
        if range.start < copied {
            continue;
        }
        output.push_str(&source[copied..range.start]);
        output.push_str(&replacement);
        copied = range.end;
    }
    output.push_str(&source[copied..]);
    output
}

fn paired_edit(
    edits: &mut Vec<(Range<usize>, String)>,
    text: &str,
    offset: usize,
    range: &Range<usize>,
    delimiter: &str,
    tag: &str,
) {
    if range.start >= delimiter.len()
        && text.get(range.start - delimiter.len()..range.start) == Some(delimiter)
        && text.get(range.end..range.end + delimiter.len()) == Some(delimiter)
    {
        edits.push((
            offset + range.start - delimiter.len()..offset + range.start,
            format!("<{tag}>"),
        ));
        edits.push((
            offset + range.end..offset + range.end + delimiter.len(),
            format!("</{tag}>"),
        ));
    }
}

const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_DOCUMENT_IMAGE_BYTES: usize = 128 * 1024 * 1024;

pub(super) fn embed_images(
    html: &str,
    source: Option<&Path>,
    workspace_root: &Path,
    workspace_roots: &[PathBuf],
) -> Result<String> {
    let mut cache = HashMap::new();
    let mut total = 0;
    let mut failure = None;
    let document = rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![
                element!("script, iframe, object, embed", |element| {
                    element.remove();
                    Ok(())
                }),
                element!("img", |element| {
                    let Some(target) = element.get_attribute("src") else {
                        return Ok(());
                    };
                    if failure.is_some() {
                        return Ok(());
                    }
                    let data = if let Some(data) = cache.get(&target) {
                        data
                    } else {
                        match image_data_url(&target, source, workspace_root, workspace_roots) {
                            Ok(data) => cache.entry(target.clone()).or_insert(data),
                            Err(error) => {
                                failure = Some(
                                    error.context(crate::i18n::runtime::rt!(ImageRead, target)),
                                );
                                return Ok(());
                            }
                        }
                    };
                    total += data.len();
                    if total > MAX_DOCUMENT_IMAGE_BYTES {
                        failure = Some(anyhow::anyhow!(
                            crate::i18n::runtime::RuntimeText::ImagesTooLarge.to_string()
                        ));
                        return Ok(());
                    }
                    element.set_attribute("src", data)?;
                    element.remove_attribute("srcset");
                    element.remove_attribute("loading");
                    Ok(())
                }),
            ],
            ..Default::default()
        },
    )
    .context(crate::i18n::runtime::RuntimeText::PrepareExport.to_string())?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(document)
}

fn image_data_url(
    target: &str,
    source: Option<&Path>,
    workspace_root: &Path,
    workspace_roots: &[PathBuf],
) -> Result<String> {
    crate::platform::fs_task::check_deadline()?;
    let mut bytes = Vec::new();
    let extension;
    if let Some(data) = target.strip_prefix("data:") {
        let (kind, encoded) = data
            .split_once(',')
            .context(crate::i18n::runtime::RuntimeText::InvalidImageData.to_string())?;
        anyhow::ensure!(
            encoded.len() as u64 <= MAX_IMAGE_BYTES * 4 / 3 + 4,
            crate::i18n::runtime::RuntimeText::ImageTooLarge.to_string()
        );
        anyhow::ensure!(
            kind.starts_with("image/"),
            crate::i18n::runtime::RuntimeText::InvalidImageData.to_string()
        );
        bytes = if kind.ends_with(";base64") {
            base64::engine::general_purpose::STANDARD.decode(encoded)?
        } else {
            percent_encoding::percent_decode_str(encoded).collect()
        };
        extension = kind
            .strip_prefix("image/")
            .and_then(|kind| kind.split(';').next())
            .map(|kind| if kind == "svg+xml" { "svg" } else { kind }.to_string());
    } else {
        let path = Path::new(target.split(['?', '#']).next().unwrap_or(target));
        extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_string);
        if super::helpers::is_remote_image_url(target) {
            let response = ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .get(target)
                .call()
                .context(crate::i18n::runtime::RuntimeText::DownloadImage.to_string())?;
            response
                .into_reader()
                .take(MAX_IMAGE_BYTES + 1)
                .read_to_end(&mut bytes)?;
        } else {
            let resolved = super::super::navigation_io::resolve_document_image_path(
                target,
                source,
                workspace_root,
                workspace_roots,
            )?;
            std::fs::File::open(&resolved)
                .with_context(|| crate::i18n::runtime::rt!(ReadFile, resolved.display()))?
                .take(MAX_IMAGE_BYTES + 1)
                .read_to_end(&mut bytes)?;
        }
    }
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_IMAGE_BYTES,
        crate::i18n::runtime::RuntimeText::ImageTooLarge.to_string()
    );
    let image = if extension
        .as_deref()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
    {
        let rendered = super::helpers::load_render_image_from_svg_with_scale(
            std::str::from_utf8(&bytes)?,
            1.0,
        )?;
        let size = rendered.size(0);
        let mut rgba = rendered
            .as_bytes(0)
            .context(crate::i18n::runtime::RuntimeText::InvalidOutput.to_string())?
            .to_vec();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(size.width.0.try_into()?, size.height.0.try_into()?, rgba)
                .context(crate::i18n::runtime::RuntimeText::InvalidDimensions.to_string())?,
        )
    } else {
        crate::platform::image_decode::decode(&bytes, extension.as_deref())?
    };
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png)?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png.into_inner())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embedded_image(html: &str) -> image::RgbaImage {
        let encoded = html
            .split_once("src=\"data:image/png;base64,")
            .unwrap()
            .1
            .split('"')
            .next()
            .unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        image::load_from_memory(&bytes).unwrap().to_rgba8()
    }

    #[test]
    fn export_images_share_document_workspace_suffix_resolution() {
        let workspace = tempfile::tempdir().unwrap();
        let documents = tempfile::tempdir().unwrap();
        let other_workspace = tempfile::tempdir().unwrap();
        let source = documents.path().join("note.md");
        let attachments = workspace.path().join("attachments/note");
        std::fs::create_dir_all(&attachments).unwrap();
        let image = image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]));
        let image_path = attachments.join("sample image.png");
        image.save(&image_path).unwrap();
        // The current workspace need not be the document's parent or appear in
        // the additional workspace list.
        let roots = vec![other_workspace.path().to_path_buf()];
        for target in ["sample%20image.png", "note/sample%20image.png"] {
            assert_eq!(
                super::super::super::navigation_io::resolve_document_image_path(
                    target,
                    Some(&source),
                    workspace.path(),
                    &roots,
                )
                .unwrap(),
                image_path,
            );
            assert!(
                super::super::super::navigation_io::load_document_image(
                    target,
                    Some(&source),
                    workspace.path(),
                    &roots,
                )
                .is_ok()
            );
            assert_eq!(
                super::super::super::navigation_io::read_document_image_bytes(
                    target,
                    Some(&source),
                    workspace.path(),
                    &roots,
                )
                .unwrap(),
                std::fs::read(&image_path).unwrap(),
            );
            let html = embed_images(
                &format!("<img src=\"{target}\">"),
                Some(&source),
                workspace.path(),
                &roots,
            )
            .unwrap();
            assert_eq!(embedded_image(&html), image);
        }
        let markdown = "![](sample%20image.png)\n\n![[sample image.png]]";
        let document = super::super::build_html_document(
            "Image export",
            markdown,
            Some(&source),
            super::super::ExportLayout::from_settings(&super::super::AppSettings::default()),
        );
        let html = embed_images(&document, Some(&source), workspace.path(), &roots).unwrap();
        assert_eq!(html.matches("src=\"data:image/png;base64,").count(), 2);
        assert_eq!(embedded_image(&html), image);
    }

    #[test]
    fn export_images_keep_direct_path_precedence_and_file_urls() {
        let workspace = tempfile::tempdir().unwrap();
        let documents = tempfile::tempdir().unwrap();
        let additional = tempfile::tempdir().unwrap();
        let source = documents.path().join("note.md");
        let roots = vec![additional.path().to_path_buf()];
        let expected = image::RgbaImage::from_pixel(2, 1, image::Rgba([20, 40, 60, 255]));
        let other = image::RgbaImage::from_pixel(1, 2, image::Rgba([60, 40, 20, 255]));
        expected.save(documents.path().join("image.png")).unwrap();
        other.save(workspace.path().join("image.png")).unwrap();
        let html = embed_images(
            "<img src=\"image.png\">",
            Some(&source),
            workspace.path(),
            &roots,
        )
        .unwrap();
        assert_eq!(embedded_image(&html), expected);

        expected
            .save(additional.path().join("additional.png"))
            .unwrap();
        let html = embed_images(
            "<img src=\"additional.png\">",
            Some(&source),
            workspace.path(),
            &roots,
        )
        .unwrap();
        assert_eq!(embedded_image(&html), expected);
        let url =
            crate::platform::paths::path_to_file_url(&additional.path().join("additional.png"))
                .unwrap();
        let html = embed_images(
            &format!("<img src=\"{url}\">"),
            Some(&source),
            workspace.path(),
            &roots,
        )
        .unwrap();
        assert_eq!(embedded_image(&html), expected);
    }

    #[test]
    fn export_images_reject_missing_and_ambiguous_workspace_matches() {
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("note.md");
        for folder in ["first", "second"] {
            let directory = workspace.path().join(folder);
            std::fs::create_dir(&directory).unwrap();
            image::RgbaImage::new(1, 1)
                .save(directory.join("duplicate.png"))
                .unwrap();
        }
        for name in ["missing.png", "duplicate.png"] {
            let error = embed_images(
                &format!("<img src=\"{name}\">"),
                Some(&source),
                workspace.path(),
                &[],
            )
            .unwrap_err();
            assert!(format!("{error:#}").contains(name));
            assert_eq!(
                error.downcast_ref::<std::io::Error>().unwrap().kind(),
                std::io::ErrorKind::NotFound
            );
        }
        let html = embed_images(
            "<img src=\"first/duplicate.png\">",
            Some(&source),
            workspace.path(),
            &[],
        )
        .unwrap();
        assert_eq!(embedded_image(&html).dimensions(), (1, 1));
    }

    #[test]
    fn export_image_resolution_keeps_direct_path_io_errors() {
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("note.md");
        let error = embed_images(
            "<img src=\"invalid%00image.png\">",
            Some(&source),
            workspace.path(),
            &[],
        )
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("invalid%00image.png"));
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn export_typography_uses_editor_metrics_and_escapes_font_names() {
        let theme = crate::theme::ThemeKind::PaperWhite.build();
        let mut styles = crate::style::Styles::from_theme_with_options(&theme, &theme, false);
        styles.editor.editor_font_size_px = 19.25;
        styles.editor.editor_h1_font_size_px = 37.0;
        styles.editor.editor_body_latin_font_family = "Export \"Font\" </style>";
        let css = editor_typography_css(&styles.editor);
        assert!(css.contains("font-size:19.25px;"));
        assert!(css.contains("font-size:37px;"));
        assert!(css.contains("\\\"Font\\\""));
        assert!(!css.contains("</style>"));
    }

    #[test]
    fn export_images_are_portable_and_use_editor_decoders() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("note.md");
        let image = image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]));
        image.save(directory.path().join("image.tiff")).unwrap();
        let html = embed_images(
            "<img src=\"image.tiff\"><img src=\"image.tiff\">",
            Some(&source),
            directory.path(),
            &[],
        )
        .unwrap();
        assert_eq!(html.matches("data:image/png;base64,").count(), 2);
        assert!(!html.contains("image.tiff"));
        assert!(
            embed_images(
                "<img src=\"missing.png\">",
                Some(&source),
                directory.path(),
                &[]
            )
            .is_err()
        );
    }
}
