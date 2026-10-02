//! Screenshot processing: agents get small JPEGs, not full-resolution PNGs.

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use mdh_core::{Error, Result};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Jpeg {
    #[serde(skip)]
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Factor from screenshot pixels to device pixels (≥ 1 when downscaled).
    pub scale: f32,
}

/// Downscales a PNG screenshot so its long edge is at most `max_edge` and encodes it as JPEG.
pub fn screenshot_jpeg(png: &[u8], max_edge: u32, quality: u8) -> Result<Jpeg> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).map_err(|e| {
        Error::Parse {
            tool: "screencap".into(),
            detail: e.to_string(),
        }
    })?;
    let long_edge = image.width().max(image.height());
    let image = if long_edge > max_edge {
        image.resize(max_edge, max_edge, FilterType::Triangle)
    } else {
        image
    };
    let rgb = image.to_rgb8();
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, quality)
        .encode_image(&rgb)
        .map_err(|e| Error::Parse {
            tool: "jpeg encoder".into(),
            detail: e.to_string(),
        })?;
    Ok(Jpeg {
        bytes,
        width: rgb.width(),
        height: rgb.height(),
        scale: long_edge as f32 / rgb.width().max(rgb.height()) as f32,
    })
}
