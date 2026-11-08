use anyhow::{Context, Result};
use image::{DynamicImage, RgbaImage};
use windows::Win32::{
    Foundation::RPC_E_CHANGED_MODE,
    Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory, IWICPalette,
        WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnLoad,
    },
    System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    },
};

struct Apartment;

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub(super) fn decode(bytes: &[u8]) -> Result<DynamicImage> {
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED);
        let _apartment = if initialized == RPC_E_CHANGED_MODE {
            None
        } else {
            initialized
                .ok()
                .context(crate::i18n::runtime::RuntimeText::ImageDecode.to_string())?;
            Some(Apartment)
        };
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = factory.CreateStream()?;
        stream.InitializeFromMemory(bytes)?;
        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnLoad)
            .context(crate::i18n::runtime::RuntimeText::WindowsHeifRequired.to_string())?;
        let frame = decoder.GetFrame(0)?;
        let (mut width, mut height) = (0, 0);
        frame.GetSize(&mut width, &mut height)?;
        let length = super::rgba_len(width, height)?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppRGBA,
            WICBitmapDitherTypeNone,
            None::<&IWICPalette>,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let mut pixels = vec![0; length];
        converter.CopyPixels(std::ptr::null(), width * 4, &mut pixels)?;
        Ok(DynamicImage::ImageRgba8(
            RgbaImage::from_raw(width, height, pixels)
                .context(crate::i18n::runtime::RuntimeText::InvalidDimensions.to_string())?,
        ))
    }
}
