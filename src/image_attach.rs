//! Pasted / dropped images: format sniffing, downscaling and the API content block.

use anyhow::{bail, Context, Result};
use base64::Engine;
use image::{DynamicImage, ImageFormat, ImageReader};
use serde_json::{json, Value};
use std::io::Cursor;

/// Long-edge cap. Safe on every model tier (standard tier downscales above 1568 px
/// server-side anyway) and keeps token cost bounded (~1.5k tokens max per image).
pub const MAX_EDGE: u32 = 1568;
/// Raw-byte cap per image: 5 MB raw -> ~6.7 MB base64, under the API's 10 MB limit.
const MAX_RAW_BYTES: usize = 5 * 1024 * 1024;

/// An image ready to ride on a user message.
#[derive(Debug, Clone, PartialEq)]
pub struct PastedImage {
    pub media_type: &'static str,
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Largest size that fits `max_edge` on the long side, aspect preserved (never upscales).
pub fn downscale_dims(w: u32, h: u32, max_edge: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= max_edge {
        return (w, h);
    }
    let scale = max_edge as f64 / long as f64;
    let scaled = |v: u32| ((v as f64 * scale).round() as u32).max(1);
    (scaled(w), scaled(h))
}

fn media_type(f: ImageFormat) -> Option<&'static str> {
    match f {
        ImageFormat::Png => Some("image/png"),
        ImageFormat::Jpeg => Some("image/jpeg"),
        ImageFormat::Gif => Some("image/gif"),
        ImageFormat::WebP => Some("image/webp"),
        _ => None,
    }
}

/// File bytes -> `PastedImage`. The format is sniffed from the bytes (never the file
/// extension). A valid image within both caps is passed through byte-for-byte; anything
/// larger is decoded, downscaled and re-encoded: JPEG stays JPEG, others PNG (JPEG if the
/// PNG is still too big).
pub fn prepare(bytes: Vec<u8>) -> Result<PastedImage> {
    let reader = ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .context("cannot read image")?;
    let format = reader.format().context("unknown image format")?;
    let Some(mt) = media_type(format) else {
        bail!("unsupported image format {format:?} (png, jpeg, gif, webp only)");
    };
    let (w, h) = reader.into_dimensions().context("cannot read image size")?;
    if w.max(h) <= MAX_EDGE && bytes.len() <= MAX_RAW_BYTES {
        return Ok(PastedImage {
            media_type: mt,
            data: bytes,
            width: w,
            height: h,
        });
    }
    let img = image::load_from_memory_with_format(&bytes, format).context("cannot decode image")?;
    shrink(img, format)
}

/// Clipboard pixels (RGBA8, row-major) -> `PastedImage`.
pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<PastedImage> {
    let buf = image::RgbaImage::from_raw(width, height, rgba)
        .context("clipboard image has an inconsistent size")?;
    shrink(DynamicImage::ImageRgba8(buf), ImageFormat::Png)
}

/// JPEG at quality 85 (RGB; alpha dropped).
fn encode_jpeg(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut jpg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 85)
        .encode_image(&DynamicImage::ImageRgb8(img.to_rgb8()))
        .context("cannot encode image")?;
    Ok(jpg)
}

/// Downscale to `MAX_EDGE` and encode. A JPEG source stays JPEG (q85); anything else
/// becomes PNG, or JPEG q85 when the PNG still exceeds `MAX_RAW_BYTES`.
fn shrink(img: DynamicImage, src: ImageFormat) -> Result<PastedImage> {
    let (w, h) = (img.width(), img.height());
    let (nw, nh) = downscale_dims(w, h, MAX_EDGE);
    let img = if (nw, nh) == (w, h) {
        img
    } else {
        img.resize_exact(nw, nh, image::imageops::FilterType::Triangle)
    };
    let done = |media_type, data| PastedImage {
        media_type,
        data,
        width: nw,
        height: nh,
    };
    if src != ImageFormat::Jpeg {
        let mut png = Vec::new();
        img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .context("cannot encode image")?;
        if png.len() <= MAX_RAW_BYTES {
            return Ok(done("image/png", png));
        }
    }
    let jpg = encode_jpeg(&img)?;
    if jpg.len() > MAX_RAW_BYTES {
        bail!("image is too large even after downscaling");
    }
    Ok(done("image/jpeg", jpg))
}

impl PastedImage {
    /// The Messages API `image` content block (base64 source).
    pub fn to_block(&self) -> Value {
        json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": self.media_type,
                "data": base64::engine::general_purpose::STANDARD.encode(&self.data),
            }
        })
    }

    /// Chat echo line, `n` is the 1-based position among the message's images.
    pub fn echo_label(&self, n: usize) -> String {
        format!("image {n} ({}×{})", self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode a solid RGBA image of the given size as PNG bytes.
    pub(crate) fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([220, 20, 20, 255]));
        let mut out = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    /// Deterministic noisy RGB image (compresses badly, like a photo).
    fn noise(w: u32, h: u32) -> DynamicImage {
        let mut s: u32 = 12345;
        let img = image::RgbImage::from_fn(w, h, |_, _| {
            s = s.wrapping_mul(1103515245).wrapping_add(12345);
            let v = (s >> 16) as u8;
            image::Rgb([v, v.wrapping_mul(3), v.wrapping_add(90)])
        });
        DynamicImage::ImageRgb8(img)
    }

    fn encode(img: &DynamicImage, f: ImageFormat) -> Vec<u8> {
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), f).unwrap();
        out
    }

    #[test]
    fn prepare_keeps_an_oversized_jpeg_as_jpeg() {
        let src = noise(3000, 2000);
        let img = prepare(encode(&src, ImageFormat::Jpeg)).unwrap();
        assert_eq!(img.media_type, "image/jpeg");
        assert_eq!((img.width, img.height), (1568, 1045));
        let resized = src.resize_exact(1568, 1045, image::imageops::FilterType::Triangle);
        assert!(img.data.len() < encode(&resized, ImageFormat::Png).len());
    }

    #[test]
    fn q85_noise_jpeg_at_max_edge_fits_the_raw_cap() {
        let jpg = encode_jpeg(&noise(MAX_EDGE, MAX_EDGE)).unwrap();
        assert!(jpg.len() <= MAX_RAW_BYTES);
    }

    #[test]
    fn downscale_dims_leaves_small_images_alone() {
        assert_eq!(downscale_dims(800, 600, 1568), (800, 600));
        assert_eq!(downscale_dims(1568, 1568, 1568), (1568, 1568));
    }

    #[test]
    fn downscale_dims_caps_the_long_edge_and_keeps_aspect() {
        assert_eq!(downscale_dims(4000, 2000, 1568), (1568, 784));
        assert_eq!(downscale_dims(2000, 4000, 1568), (784, 1568));
        assert_eq!(downscale_dims(10_000, 1, 1568), (1568, 1));
    }

    #[test]
    fn prepare_passes_a_small_png_through_untouched() {
        let bytes = png_bytes(64, 32);
        let img = prepare(bytes.clone()).unwrap();
        assert_eq!(img.media_type, "image/png");
        assert_eq!((img.width, img.height), (64, 32));
        assert_eq!(img.data, bytes);
    }

    #[test]
    fn prepare_downscales_an_oversized_png() {
        let img = prepare(png_bytes(3136, 1000)).unwrap();
        assert_eq!(img.media_type, "image/png");
        assert_eq!((img.width, img.height), (1568, 500));
        let back = image::load_from_memory(&img.data).unwrap();
        assert_eq!((back.width(), back.height()), (1568, 500));
    }

    #[test]
    fn prepare_rejects_non_image_bytes() {
        assert!(prepare(b"definitely not an image".to_vec()).is_err());
    }

    #[test]
    fn from_rgba_encodes_clipboard_pixels_as_png() {
        let img = from_rgba(4, 2, vec![255; 4 * 2 * 4]).unwrap();
        assert_eq!(img.media_type, "image/png");
        assert_eq!((img.width, img.height), (4, 2));
        assert!(img.data.starts_with(b"\x89PNG"));
    }

    #[test]
    fn from_rgba_rejects_a_buffer_of_the_wrong_length() {
        assert!(from_rgba(4, 2, vec![0; 5]).is_err());
    }

    #[test]
    fn to_block_is_a_base64_image_block() {
        let img = PastedImage {
            media_type: "image/png",
            data: vec![1, 2, 3],
            width: 1,
            height: 1,
        };
        let b = img.to_block();
        assert_eq!(b["type"], "image");
        assert_eq!(b["source"]["type"], "base64");
        assert_eq!(b["source"]["media_type"], "image/png");
        assert_eq!(b["source"]["data"], "AQID");
    }

    #[test]
    fn echo_label_shows_index_and_size() {
        let img = PastedImage {
            media_type: "image/png",
            data: vec![],
            width: 1280,
            height: 720,
        };
        assert_eq!(img.echo_label(1), "image 1 (1280×720)");
    }
}
