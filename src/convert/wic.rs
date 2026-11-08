//! Bilder über die Windows Imaging Component lesen. Damit gehen alle Formate, für die
//! Windows einen Codec hat – z. B. HEIC (iPhone-Fotos) und AVIF mit den Store-Erweiterungen.

use std::path::Path;

use image::metadata::Orientation;
use image::{DynamicImage, RgbaImage};
use windows::core::HSTRING;
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICBitmapFrameDecode, IWICImagingFactory,
    WICConvertBitmapSource, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantToUInt16, PROPVARIANT};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};

pub fn load(path: &Path) -> windows::core::Result<DynamicImage> {
    unsafe {
        // Schon initialisiert (auch in anderem Modus) ist kein Problem.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let decoder = factory.CreateDecoderFromFilename(
            &HSTRING::from(path.as_os_str()),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )?;
        let frame = decoder.GetFrame(0)?;
        let orientation = orientation(&frame);

        let rgba = WICConvertBitmapSource(&GUID_WICPixelFormat32bppRGBA, &frame)?;
        let (mut w, mut h) = (0, 0);
        rgba.GetSize(&mut w, &mut h)?;
        let mut pixels = vec![0u8; w as usize * h as usize * 4];
        rgba.CopyPixels(std::ptr::null(), w * 4, &mut pixels)?;

        let buffer = RgbaImage::from_raw(w, h, pixels).expect("Puffergröße passt");
        let mut image = DynamicImage::ImageRgba8(buffer);
        if let Some(orientation) = orientation {
            image.apply_orientation(orientation);
        }
        Ok(image)
    }
}

/// EXIF-Ausrichtung, falls vorhanden (HEIC: `/ifd/…`, JPEG: `/app1/ifd/…`).
fn orientation(frame: &IWICBitmapFrameDecode) -> Option<Orientation> {
    unsafe {
        let reader = frame.GetMetadataQueryReader().ok()?;
        for query in ["/ifd/{ushort=274}", "/app1/ifd/{ushort=274}"] {
            let mut value = PROPVARIANT::default();
            if reader.GetMetadataByName(&HSTRING::from(query), &mut value).is_ok() {
                if let Ok(v) = PropVariantToUInt16(&value) {
                    return Orientation::from_exif(v as u8);
                }
            }
        }
        None
    }
}
