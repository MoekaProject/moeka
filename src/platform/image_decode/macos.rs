use std::{ffi::c_void, ptr};

use anyhow::{Context, Result};
use image::DynamicImage;
use objc::{class, msg_send, sel, sel_impl};

type Cf = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: Cf);
}

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageSourceCreateWithData(data: Cf, options: Cf) -> Cf;
    fn CGImageSourceCreateThumbnailAtIndex(source: Cf, index: usize, options: Cf) -> Cf;
    fn CGImageSourceCopyPropertiesAtIndex(source: Cf, index: usize, options: Cf) -> Cf;
    fn CGImageDestinationCreateWithData(data: Cf, kind: Cf, count: usize, options: Cf) -> Cf;
    fn CGImageDestinationAddImage(destination: Cf, image: Cf, properties: Cf);
    fn CGImageDestinationFinalize(destination: Cf) -> bool;
    #[cfg(test)]
    fn CGImageSourceCreateImageAtIndex(source: Cf, index: usize, options: Cf) -> Cf;
    static kCGImagePropertyPixelWidth: Cf;
    static kCGImagePropertyPixelHeight: Cf;
    #[cfg(test)]
    static kCGImagePropertyOrientation: Cf;
    static kCGImageSourceCreateThumbnailFromImageAlways: Cf;
    static kCGImageSourceCreateThumbnailWithTransform: Cf;
    static kCGImageSourceThumbnailMaxPixelSize: Cf;
}

#[cfg(test)]
#[test]
fn imageio_decodes_real_heic_pixels() {
    use std::io::Cursor;

    let original = image::RgbaImage::from_pixel(64, 48, image::Rgba([40, 100, 180, 255]));
    let mut png = Cursor::new(Vec::new());
    original
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    unsafe {
        let pool: *mut objc::runtime::Object = msg_send![class!(NSAutoreleasePool), new];
        let data: *mut objc::runtime::Object = msg_send![class!(NSData), dataWithBytes: png.get_ref().as_ptr() length: png.get_ref().len()];
        let source = Owned::new(
            CGImageSourceCreateWithData(data.cast(), ptr::null()),
            "read fixture",
        )
        .unwrap();
        let image = Owned::new(
            CGImageSourceCreateImageAtIndex(source.0, 0, ptr::null()),
            "decode fixture",
        )
        .unwrap();
        for orientation in [1u32, 6] {
            let output: *mut objc::runtime::Object = msg_send![class!(NSMutableData), data];
            let heic: *mut objc::runtime::Object =
                msg_send![class!(NSString), stringWithUTF8String: c"public.heic".as_ptr()];
            let destination = Owned::new(
                CGImageDestinationCreateWithData(output.cast(), heic.cast(), 1, ptr::null()),
                "create HEIC fixture",
            )
            .unwrap();
            let properties: *mut objc::runtime::Object =
                msg_send![class!(NSMutableDictionary), dictionary];
            let rotation: *mut objc::runtime::Object =
                msg_send![class!(NSNumber), numberWithUnsignedInt: orientation];
            let _: () =
                msg_send![properties, setObject: rotation forKey: kCGImagePropertyOrientation];
            CGImageDestinationAddImage(destination.0, image.0, properties.cast());
            assert!(CGImageDestinationFinalize(destination.0));
            let length: usize = msg_send![output, length];
            let bytes: *const u8 = msg_send![output, bytes];
            let decoded = super::decode(std::slice::from_raw_parts(bytes, length), Some("heic"))
                .unwrap()
                .to_rgba8();
            let expected = if orientation == 6 { (48, 64) } else { (64, 48) };
            assert_eq!(decoded.dimensions(), expected);
            for channel in 0..3 {
                assert!(
                    decoded.get_pixel(24, 24)[channel]
                        .abs_diff(original.get_pixel(24, 24)[channel])
                        < 12
                );
            }
        }
        let _: () = msg_send![pool, drain];
    }
}

#[cfg(test)]
#[test]
fn imageio_decodes_actual_avif_pixels() {
    let original = image::RgbImage::from_pixel(16, 12, image::Rgb([40, 100, 180]));
    let mut encoded = std::io::Cursor::new(Vec::new());
    original
        .write_to(&mut encoded, image::ImageFormat::Avif)
        .unwrap();
    let decoded = super::decode(encoded.get_ref(), None).unwrap().to_rgb8();
    assert_eq!(decoded.dimensions(), original.dimensions());
    for channel in 0..3 {
        assert!(decoded.get_pixel(8, 6)[channel].abs_diff(original.get_pixel(8, 6)[channel]) < 12);
    }
}

struct Owned(Cf);

impl Owned {
    fn new(value: Cf, operation: &str) -> Result<Self> {
        anyhow::ensure!(
            !value.is_null(),
            crate::i18n::runtime::rt!(ImageDecode, operation)
        );
        Ok(Self(value))
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

pub(super) fn decode(bytes: &[u8]) -> Result<DynamicImage> {
    unsafe {
        let pool: *mut objc::runtime::Object = msg_send![class!(NSAutoreleasePool), new];
        let result = decode_in_pool(bytes);
        let _: () = msg_send![pool, drain];
        result
    }
}

unsafe fn decode_in_pool(bytes: &[u8]) -> Result<DynamicImage> {
    unsafe {
        let data: *mut objc::runtime::Object =
            msg_send![class!(NSData), dataWithBytes: bytes.as_ptr() length: bytes.len()];
        let source = Owned::new(
            CGImageSourceCreateWithData(data.cast(), ptr::null()),
            "CGImageSourceCreateWithData",
        )?;
        let properties = Owned::new(
            CGImageSourceCopyPropertiesAtIndex(source.0, 0, ptr::null()),
            "CGImageSourceCopyPropertiesAtIndex",
        )?;
        let properties = properties.0 as *mut objc::runtime::Object;
        let width: *mut objc::runtime::Object =
            msg_send![properties, objectForKey: kCGImagePropertyPixelWidth];
        let height: *mut objc::runtime::Object =
            msg_send![properties, objectForKey: kCGImagePropertyPixelHeight];
        let width: u32 = msg_send![width, unsignedIntValue];
        let height: u32 = msg_send![height, unsignedIntValue];
        super::rgba_len(width, height)?;
        let options: *mut objc::runtime::Object =
            msg_send![class!(NSMutableDictionary), dictionary];
        let yes: *mut objc::runtime::Object =
            msg_send![class!(NSNumber), numberWithBool: objc::runtime::YES];
        let max: *mut objc::runtime::Object =
            msg_send![class!(NSNumber), numberWithUnsignedInt: width.max(height)];
        let _: () =
            msg_send![options, setObject: yes forKey: kCGImageSourceCreateThumbnailFromImageAlways];
        let _: () =
            msg_send![options, setObject: yes forKey: kCGImageSourceCreateThumbnailWithTransform];
        let _: () = msg_send![options, setObject: max forKey: kCGImageSourceThumbnailMaxPixelSize];
        let image = Owned::new(
            CGImageSourceCreateThumbnailAtIndex(source.0, 0, options.cast()),
            "CGImageSourceCreateThumbnailAtIndex",
        )?;
        let output: *mut objc::runtime::Object = msg_send![class!(NSMutableData), data];
        let png: *mut objc::runtime::Object =
            msg_send![class!(NSString), stringWithUTF8String: c"public.png".as_ptr()];
        let destination = Owned::new(
            CGImageDestinationCreateWithData(output.cast(), png.cast(), 1, ptr::null()),
            "CGImageDestinationCreateWithData",
        )?;
        CGImageDestinationAddImage(destination.0, image.0, ptr::null());
        anyhow::ensure!(
            CGImageDestinationFinalize(destination.0),
            crate::i18n::runtime::RuntimeText::ImageDecode.to_string()
        );
        let length: usize = msg_send![output, length];
        let bytes: *const u8 = msg_send![output, bytes];
        anyhow::ensure!(
            !bytes.is_null() && length > 0,
            crate::i18n::runtime::RuntimeText::InvalidImageData.to_string()
        );
        image::load_from_memory_with_format(
            std::slice::from_raw_parts(bytes, length),
            image::ImageFormat::Png,
        )
        .context(crate::i18n::runtime::RuntimeText::ImageDecode.to_string())
    }
}
