//! Image decode boundary for browser resources.
//!
//! The crate decodes compressed image bytes into a tightly bounded RGBA8
//! surface. Callers own fetch, cache, layout, and paint integration.

use image_webp::WebPDecoder;
use std::io::Cursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const JPEG_MAGIC: &[u8; 3] = b"\xff\xd8\xff";
const RIFF_MAGIC: &[u8; 4] = b"RIFF";
const WEBP_MAGIC: &[u8; 4] = b"WEBP";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageError {
    pub message: String,
}

impl ImageError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub fn decode_image(bytes: &[u8], content_type: Option<&str>) -> Result<DecodedImage, ImageError> {
    match sniff_format(bytes, content_type) {
        Some(ImageFormat::Png) => decode_png(bytes),
        Some(ImageFormat::Jpeg) => decode_jpeg(bytes),
        Some(ImageFormat::Webp) => decode_webp(bytes),
        None => Err(ImageError::new("Unsupported image format")),
    }
}

#[must_use]
pub fn sniff_format(bytes: &[u8], content_type: Option<&str>) -> Option<ImageFormat> {
    if bytes.starts_with(PNG_MAGIC) {
        return Some(ImageFormat::Png);
    }
    if bytes.starts_with(JPEG_MAGIC) {
        return Some(ImageFormat::Jpeg);
    }
    if has_webp_magic(bytes) {
        return Some(ImageFormat::Webp);
    }
    content_type.and_then(sniff_content_type)
}

fn has_webp_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(RIFF_MAGIC) && bytes.get(8..12) == Some(WEBP_MAGIC.as_slice())
}

fn sniff_content_type(content_type: &str) -> Option<ImageFormat> {
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match media_type.as_str() {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" | "image/jpg" => Some(ImageFormat::Jpeg),
        "image/webp" => Some(ImageFormat::Webp),
        _ => None,
    }
}

fn decode_png(bytes: &[u8]) -> Result<DecodedImage, ImageError> {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|e| ImageError::new(format!("PNG header decode: {e}")))?;
    let mut raw = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or_else(|| ImageError::new("PNG output buffer is too large"))?
    ];
    let output = reader
        .next_frame(&mut raw)
        .map_err(|e| ImageError::new(format!("PNG frame decode: {e}")))?;
    let frame = &raw[..output.buffer_size()];
    let rgba = png_frame_to_rgba(frame, output.color_type, output.bit_depth)?;
    Ok(DecodedImage {
        width: output.width,
        height: output.height,
        rgba,
    })
}

fn png_frame_to_rgba(
    frame: &[u8],
    color_type: png::ColorType,
    bit_depth: png::BitDepth,
) -> Result<Vec<u8>, ImageError> {
    if bit_depth != png::BitDepth::Eight {
        return Err(ImageError::new(format!(
            "PNG bit depth {bit_depth:?} is not supported"
        )));
    }
    match color_type {
        png::ColorType::Rgba => Ok(frame.to_vec()),
        png::ColorType::Rgb => Ok(rgb_to_rgba(frame)),
        png::ColorType::Grayscale => Ok(gray_to_rgba(frame)),
        png::ColorType::GrayscaleAlpha => Ok(gray_alpha_to_rgba(frame)),
        png::ColorType::Indexed => Err(ImageError::new("Indexed PNG did not expand to RGB")),
    }
}

fn decode_jpeg(bytes: &[u8]) -> Result<DecodedImage, ImageError> {
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder = JpegDecoder::new_with_options(Cursor::new(bytes), options);
    decoder
        .decode_headers()
        .map_err(|e| ImageError::new(format!("JPEG header decode: {e}")))?;
    let (width, height) = decoder
        .dimensions()
        .ok_or_else(|| ImageError::new("JPEG dimensions are missing"))?;
    let rgba = decoder
        .decode()
        .map_err(|e| ImageError::new(format!("JPEG frame decode: {e}")))?;
    Ok(DecodedImage {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}

fn decode_webp(bytes: &[u8]) -> Result<DecodedImage, ImageError> {
    let mut decoder = WebPDecoder::new(Cursor::new(bytes))
        .map_err(|e| ImageError::new(format!("WebP header decode: {e}")))?;
    let (width, height) = decoder.dimensions();
    let mut pixels = vec![
        0;
        decoder
            .output_buffer_size()
            .ok_or_else(|| ImageError::new("WebP output buffer is too large"))?
    ];
    decoder
        .read_image(&mut pixels)
        .map_err(|e| ImageError::new(format!("WebP frame decode: {e}")))?;
    let rgba = if decoder.has_alpha() {
        pixels
    } else {
        rgb_to_rgba(&pixels)
    };
    Ok(DecodedImage {
        width,
        height,
        rgba,
    })
}

fn rgb_to_rgba(rgb: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(rgb.len() / 3 * 4);
    for pixel in rgb.chunks_exact(3) {
        rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
    }
    rgba
}

fn gray_to_rgba(gray: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(gray.len() * 4);
    for luma in gray {
        rgba.extend_from_slice(&[*luma, *luma, *luma, 255]);
    }
    rgba
}

fn gray_alpha_to_rgba(gray_alpha: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(gray_alpha.len() / 2 * 4);
    for pixel in gray_alpha.chunks_exact(2) {
        rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::{ImageFormat, decode_image, sniff_format};

    const PNG_2X1_RGBA: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 244, 34, 127, 138, 0, 0, 0, 17, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240,
        159, 225, 63, 67, 3, 0, 16, 121, 3, 126, 33, 192, 253, 141, 0, 0, 0, 0, 73, 69, 78, 68,
        174, 66, 96, 130,
    ];

    const JPEG_2X1_RGB_BASE64: &str = concat!(
        "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAIBAQEBAQIBAQECAgICAgQDAgICAgUEBAMEBgUG",
        "BgYFBgYGBwkIBgcJBwYGCAsICQoKCgoKBggLDAsKDAkKCgr/2wBDAQICAgICAgUDAwUKBwYH",
        "CgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgr/wAAR",
        "CAABAAIDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAA",
        "AgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkK",
        "FhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWG",
        "h4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl",
        "5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREA",
        "AgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYk",
        "NOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOE",
        "hYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk",
        "5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDs/gx/yR7wn/2LVh/6TpRRRX+XOO/32r/i",
        "l+bP8jfFH/k5mef9hmJ/9PTP/9k="
    );

    const WEBP_2X2_RGB: &[u8] = &[
        0x52, 0x49, 0x46, 0x46, 0x3c, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38,
        0x20, 0x30, 0x00, 0x00, 0x00, 0xd0, 0x01, 0x00, 0x9d, 0x01, 0x2a, 0x02, 0x00, 0x02, 0x00,
        0x02, 0x00, 0x34, 0x25, 0xa0, 0x02, 0x74, 0xba, 0x01, 0xf8, 0x00, 0x03, 0xb0, 0x00, 0xfe,
        0xf0, 0xc4, 0x0b, 0xff, 0x20, 0xb9, 0x61, 0x75, 0xc8, 0xd7, 0xff, 0x20, 0x3f, 0xe4, 0x07,
        0xfc, 0x80, 0xff, 0xf8, 0xf2, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn sniff_format_uses_magic_before_content_type() {
        assert_eq!(
            sniff_format(PNG_2X1_RGBA, Some("image/jpeg")),
            Some(ImageFormat::Png)
        );
        assert_eq!(
            sniff_format(&[], Some("image/jpeg")),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(
            sniff_format(&[], Some("image/webp")),
            Some(ImageFormat::Webp)
        );
    }

    #[test]
    fn decode_png_returns_rgba_surface() {
        let decoded = decode_image(PNG_2X1_RGBA, None).expect("PNG decodes");

        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 1);
        assert_eq!(decoded.rgba.len(), 8);
        assert_eq!(&decoded.rgba[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn decode_jpeg_returns_rgba_surface() {
        let jpeg = decode_base64(JPEG_2X1_RGB_BASE64);
        let decoded = decode_image(&jpeg, Some("image/jpeg")).expect("JPEG decodes");

        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 1);
        assert_eq!(decoded.rgba.len(), 8);
        assert_eq!(decoded.rgba[3], 255);
        assert_eq!(decoded.rgba[7], 255);
    }

    #[test]
    fn decode_webp_returns_rgba_surface() {
        let decoded = decode_image(WEBP_2X2_RGB, Some("image/webp")).expect("WebP decodes");

        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded.rgba.len(), 16);
        assert_eq!(decoded.rgba[3], 255);
    }

    #[test]
    fn decode_rejects_unknown_format() {
        let err = decode_image(b"not an image", None).expect_err("unknown format fails");

        assert!(err.message.contains("Unsupported image format"));
    }

    fn decode_base64(input: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len() * 3 / 4);
        let mut accum = 0u32;
        let mut bits = 0u8;
        for byte in input.bytes().filter(|b| !b.is_ascii_whitespace()) {
            if byte == b'=' {
                break;
            }
            accum = (accum << 6) | u32::from(base64_value(byte));
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push(((accum >> bits) & 0xff) as u8);
            }
        }
        out
    }

    fn base64_value(byte: u8) -> u8 {
        match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("invalid base64 byte"),
        }
    }
}
