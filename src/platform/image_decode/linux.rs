use std::ffi::{CStr, c_char, c_void};

use anyhow::{Context, Result};
use image::{DynamicImage, RgbaImage};
use libloading::Library;

#[repr(C)]
struct HeifError {
    code: i32,
    subcode: i32,
    message: *const c_char,
}

impl HeifError {
    unsafe fn check(self) -> Result<()> {
        if self.code == 0 {
            return Ok(());
        }
        let message = if self.message.is_null() {
            crate::i18n::runtime::RuntimeText::ImageDecode
                .to_string()
                .into()
        } else {
            unsafe { CStr::from_ptr(self.message) }.to_string_lossy()
        };
        anyhow::bail!(crate::i18n::runtime::rt!(
            ImageDecode,
            "libheif",
            self.code,
            self.subcode,
            message
        ))
    }
}

struct Owned {
    pointer: *mut c_void,
    free: unsafe extern "C" fn(*mut c_void),
}

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { (self.free)(self.pointer) };
    }
}

pub(super) fn decode(bytes: &[u8]) -> Result<DynamicImage> {
    // Keep the system library alive until every handle and borrowed pixel plane is released.
    unsafe {
        let library = Library::new("libheif.so.1")
            .context(crate::i18n::runtime::RuntimeText::HeifRequired.to_string())?;
        let alloc =
            library.get::<unsafe extern "C" fn() -> *mut c_void>(b"heif_context_alloc\0")?;
        let free = *library.get(b"heif_context_free\0")?;
        let context = Owned {
            pointer: alloc(),
            free,
        };
        anyhow::ensure!(
            !context.pointer.is_null(),
            crate::i18n::runtime::RuntimeText::ImageDecode.to_string()
        );
        let read =
            library.get::<unsafe extern "C" fn(
                *mut c_void,
                *const u8,
                usize,
                *const c_void,
            ) -> HeifError>(b"heif_context_read_from_memory_without_copy\0")?;
        read(
            context.pointer,
            bytes.as_ptr(),
            bytes.len(),
            std::ptr::null(),
        )
        .check()?;
        let primary = library
            .get::<unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> HeifError>(
                b"heif_context_get_primary_image_handle\0",
            )?;
        let mut handle = std::ptr::null_mut();
        let release_handle = *library.get(b"heif_image_handle_release\0")?;
        primary(context.pointer, &mut handle).check()?;
        anyhow::ensure!(
            !handle.is_null(),
            crate::i18n::runtime::RuntimeText::ImageDecode.to_string()
        );
        let handle = Owned {
            pointer: handle,
            free: release_handle,
        };
        let width = library
            .get::<unsafe extern "C" fn(*mut c_void) -> i32>(b"heif_image_handle_get_width\0")?;
        let height = library
            .get::<unsafe extern "C" fn(*mut c_void) -> i32>(b"heif_image_handle_get_height\0")?;
        super::rgba_len(
            u32::try_from(width(handle.pointer))?,
            u32::try_from(height(handle.pointer))?,
        )?;
        let decode = library.get::<unsafe extern "C" fn(
            *mut c_void,
            *mut *mut c_void,
            i32,
            i32,
            *const c_void,
        ) -> HeifError>(b"heif_decode_image\0")?;
        let mut image = std::ptr::null_mut();
        let release_image = *library.get(b"heif_image_release\0")?;
        // libheif colorspace RGB=1, interleaved RGBA=11, channel interleaved=10.
        decode(handle.pointer, &mut image, 1, 11, std::ptr::null()).check()?;
        anyhow::ensure!(
            !image.is_null(),
            crate::i18n::runtime::RuntimeText::ImageDecode.to_string()
        );
        let image = Owned {
            pointer: image,
            free: release_image,
        };
        let width = library
            .get::<unsafe extern "C" fn(*mut c_void, i32) -> i32>(b"heif_image_get_width\0")?;
        let height = library
            .get::<unsafe extern "C" fn(*mut c_void, i32) -> i32>(b"heif_image_get_height\0")?;
        let width = u32::try_from(width(image.pointer, 10))?;
        let height = u32::try_from(height(image.pointer, 10))?;
        let length = super::rgba_len(width, height)?;
        let plane = library.get::<unsafe extern "C" fn(*mut c_void, i32, *mut i32) -> *const u8>(
            b"heif_image_get_plane_readonly\0",
        )?;
        let mut stride = 0;
        let plane = plane(image.pointer, 10, &mut stride);
        let stride = usize::try_from(stride)?;
        let row_bytes = width as usize * 4;
        anyhow::ensure!(
            !plane.is_null() && stride >= row_bytes,
            crate::i18n::runtime::RuntimeText::InvalidImageData.to_string()
        );
        anyhow::ensure!(
            stride
                .checked_mul(height as usize)
                .is_some_and(|length| length <= isize::MAX as usize),
            crate::i18n::runtime::RuntimeText::ImageTooLarge.to_string()
        );
        let mut pixels = vec![0; length];
        for (row, destination) in pixels.chunks_exact_mut(row_bytes).enumerate() {
            destination.copy_from_slice(std::slice::from_raw_parts(
                plane.add(row * stride),
                row_bytes,
            ));
        }
        Ok(DynamicImage::ImageRgba8(
            RgbaImage::from_raw(width, height, pixels)
                .context(crate::i18n::runtime::RuntimeText::InvalidDimensions.to_string())?,
        ))
    }
}
