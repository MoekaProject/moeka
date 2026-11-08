use std::{io::Cursor, path::Path};

use anyhow::{Context, Result};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};

#[cfg(target_os = "macos")]
#[path = "image_decode/macos.rs"]
mod native;
#[cfg(target_os = "linux")]
#[path = "image_decode/linux.rs"]
mod native;
#[cfg(windows)]
#[path = "image_decode/windows.rs"]
mod native;

pub(crate) const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "apng", "jpg", "jpeg", "jpe", "jfif", "gif", "webp", "bmp", "ico", "tif", "tiff",
    "avif", "svg", "heic", "heif", "hif", "dds", "exr", "ff", "hdr", "pbm", "pgm", "ppm", "pam",
    "pnm", "qoi", "tga", "targa", "jxl",
];

pub(crate) fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            IMAGE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        })
}

fn is_heif(bytes: &[u8]) -> bool {
    bytes.get(4..8) == Some(b"ftyp")
        && bytes.get(8..12).is_some_and(|brand| {
            matches!(
                brand,
                b"heic"
                    | b"heix"
                    | b"hevc"
                    | b"hevx"
                    | b"heim"
                    | b"heis"
                    | b"mif1"
                    | b"msf1"
                    | b"avif"
                    | b"avis"
            )
        })
}

pub(super) fn rgba_len(width: u32, height: u32) -> Result<usize> {
    let length = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .context(crate::i18n::runtime::RuntimeText::InvalidDimensions.to_string())?;
    anyhow::ensure!(
        width > 0 && height > 0 && length <= 512 * 1024 * 1024,
        crate::i18n::runtime::rt!(ImageTooLarge, "512 MiB", format!("{width}×{height}"))
    );
    Ok(usize::try_from(length)?)
}

pub(crate) fn decode(bytes: &[u8], extension: Option<&str>) -> Result<DynamicImage> {
    if bytes.starts_with(&[0xff, 0x0a])
        || bytes.starts_with(b"\0\0\0\x0cJXL \x0d\x0a\x87\x0a")
        || extension.is_some_and(|extension| extension.eq_ignore_ascii_case("jxl"))
    {
        return decode_pixels(jxl_oxide::integration::JxlDecoder::new(Cursor::new(bytes))?);
    }
    if is_heif(bytes)
        || extension.is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "heic" | "heif" | "hif" | "avif"
            )
        })
    {
        return native::decode(bytes)
            .context(crate::i18n::runtime::RuntimeText::ImageDecode.to_string());
    }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    if reader.format().is_none()
        && let Some(format) =
            extension.and_then(|extension| match extension.to_ascii_lowercase().as_str() {
                "apng" => Some(ImageFormat::Png),
                "jpe" | "jfif" => Some(ImageFormat::Jpeg),
                "targa" => Some(ImageFormat::Tga),
                _ => ImageFormat::from_extension(extension),
            })
    {
        reader.set_format(format);
    }
    decode_pixels(reader.into_decoder()?)
}

fn decode_pixels(mut decoder: impl ImageDecoder) -> Result<DynamicImage> {
    rgba_len(decoder.dimensions().0, decoder.dimensions().1)?;
    decoder.set_limits(image::Limits::default())?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_extensions_cover_decoders_and_heif_consistently() {
        for extension in IMAGE_EXTENSIONS {
            assert!(is_image_path(
                &Path::new("image").with_extension(extension.to_uppercase())
            ));
            assert!(
                matches!(
                    *extension,
                    "svg" | "heic" | "heif" | "hif" | "apng" | "jpe" | "jfif" | "targa" | "jxl"
                ) || ImageFormat::from_extension(extension).is_some(),
                "unmapped image extension: {extension}"
            );
        }
        assert!(!is_image_path(Path::new("file.txt")));
        assert!(rgba_len(u32::MAX, u32::MAX).is_err());
    }

    #[test]
    fn image_decoding_accepts_real_encoded_formats_and_rejects_corruption() {
        let source = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            4,
            3,
            image::Rgba([32, 96, 160, 255]),
        ));
        for format in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Bmp,
            ImageFormat::Tiff,
            ImageFormat::WebP,
            ImageFormat::Qoi,
            ImageFormat::Tga,
            ImageFormat::Farbfeld,
            ImageFormat::Ico,
            ImageFormat::Gif,
            ImageFormat::Pnm,
            ImageFormat::Hdr,
            ImageFormat::OpenExr,
        ] {
            let mut encoded = Cursor::new(Vec::new());
            if format == ImageFormat::Farbfeld {
                DynamicImage::ImageRgba16(source.to_rgba16())
                    .write_to(&mut encoded, format)
                    .unwrap();
            } else if format == ImageFormat::Pnm || format == ImageFormat::Jpeg {
                DynamicImage::ImageRgb8(source.to_rgb8())
                    .write_to(&mut encoded, format)
                    .unwrap();
            } else if format == ImageFormat::Hdr || format == ImageFormat::OpenExr {
                DynamicImage::ImageRgb32F(source.to_rgb32f())
                    .write_to(&mut encoded, format)
                    .unwrap();
            } else {
                source.write_to(&mut encoded, format).unwrap();
            }
            let decoded =
                decode(encoded.get_ref(), format.extensions_str().first().copied()).unwrap();
            let decoded = decoded.to_rgba8();
            assert_eq!(decoded.dimensions(), (4, 3));
            for (actual, expected) in decoded.as_raw().iter().zip(source.to_rgba8().as_raw()) {
                assert!(
                    actual.abs_diff(*expected) <= if format == ImageFormat::Jpeg { 3 } else { 1 },
                    "{format:?}: {actual} != {expected}"
                );
            }
        }
        assert!(decode(b"not an image", Some("heic")).is_err());
    }

    #[test]
    fn jpeg_xl_decodes_real_pixels_with_and_without_an_extension() {
        use zune_core::{bit_depth::BitDepth, colorspace::ColorSpace, options::EncoderOptions};

        let pixels = [32, 96, 160, 255].repeat(12);
        let options = EncoderOptions::new(4, 3, ColorSpace::RGBA, BitDepth::Eight);
        let mut encoded = Vec::new();
        zune_jpegxl::JxlSimpleEncoder::new(&pixels, options)
            .encode(&mut encoded)
            .unwrap();
        for extension in [None, Some("JXL")] {
            let decoded = decode(&encoded, extension).unwrap().to_rgba8();
            assert_eq!(decoded.dimensions(), (4, 3));
            assert_eq!(decoded.as_raw(), &pixels);
        }
        assert!(decode(b"corrupt", Some("jxl")).is_err());
    }
}
