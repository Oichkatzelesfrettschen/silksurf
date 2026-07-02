use cosmic_text::{Attrs, Buffer, Color as CosmicColor, Metrics, Shaping};
use silksurf_css::Color;
use tiny_skia::PremultipliedColorU8;

use crate::TEXT_STATE;

/// Rasterize shaped glyphs for `text` directly into `pixmap`.
///
/// Each glyph is alpha-composited (Porter-Duff "src over") into the pixmap
/// at `origin` (top-left of the text bounding box, in pixmap pixel space).
/// Pixels outside the pixmap bounds are silently clipped.
///
/// The pixmap must use premultiplied RGBA8 format (as produced by
/// `PixmapMut::from_bytes`). Straight-alpha `color` is premultiplied before
/// compositing so the output is correct for opaque and translucent text.
pub fn rasterize_glyphs(
    text: &str,
    font_size: f32,
    color: Color,
    pixmap: &mut tiny_skia::PixmapMut<'_>,
    origin: (f32, f32),
) {
    if text.is_empty() {
        return;
    }
    if rasterize_ascii_glyphs(text, font_size, color, pixmap, origin) {
        return;
    }

    let mut state = TEXT_STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let crate::TextState {
        font_system,
        swash_cache,
    } = &mut *state;

    let line_height = font_size * 1.2;
    let metrics = Metrics::new(font_size, line_height);
    let mut buffer = Buffer::new(font_system, metrics);
    buffer.set_size(None, None);
    buffer.set_text(text, &Attrs::new(), Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);

    let text_color = CosmicColor::rgba(color.r, color.g, color.b, color.a);
    let pw = pixmap.width() as i32;
    let ph = pixmap.height() as i32;

    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let physical = glyph.physical((origin.0, origin.1), 1.0);
            let glyph_color = glyph.color_opt.unwrap_or(text_color);

            swash_cache.with_pixels(font_system, physical.cache_key, glyph_color, |px, py, c| {
                let x = physical.x + px;
                let y = physical.y + py;
                if x < 0 || y < 0 || x >= pw || y >= ph {
                    return;
                }
                let idx = y as usize * pw as usize + x as usize;
                let pixels = pixmap.pixels_mut();
                if let Some(dst) = pixels.get_mut(idx) {
                    composite_over(dst, c);
                }
            });
        }
    }
}

fn rasterize_ascii_glyphs(
    text: &str,
    font_size: f32,
    color: Color,
    pixmap: &mut tiny_skia::PixmapMut<'_>,
    origin: (f32, f32),
) -> bool {
    if !font_size.is_finite()
        || font_size <= 0.0
        || !text.chars().all(|ch| fast_bitmap_glyph(ch).is_some())
    {
        return false;
    }

    let pixmap_width = pixmap.width() as i32;
    let pixmap_height = pixmap.height() as i32;
    let scale = ((font_size / 12.0).round() as i32).max(1);
    let advance = (font_size * 0.55).round().max(6.0) as i32;
    let line_height = (font_size * 1.2).round().max(8.0) as i32;
    let space_advance = (font_size * 0.33).round().max(3.0) as i32;
    let mut cursor_x = origin.0.round() as i32;
    let mut cursor_y = origin.1.round() as i32;
    let line_origin_x = cursor_x;
    let pixels = pixmap.pixels_mut();

    for ch in text.chars() {
        match ch {
            '\n' => {
                cursor_x = line_origin_x;
                cursor_y = cursor_y.saturating_add(line_height);
            }
            '\r' => {}
            '\t' => {
                cursor_x = cursor_x.saturating_add(space_advance.saturating_mul(4));
            }
            ' ' | '\u{00a0}' => {
                cursor_x = cursor_x.saturating_add(space_advance);
            }
            _ => {
                let Some(glyph_char) = fast_bitmap_glyph(ch) else {
                    return false;
                };
                let glyph = ascii_glyph(glyph_char);
                draw_ascii_glyph(
                    pixels,
                    pixmap_width,
                    pixmap_height,
                    cursor_x,
                    cursor_y,
                    scale,
                    glyph,
                    color,
                );
                cursor_x = cursor_x.saturating_add(advance);
            }
        }
    }

    true
}

fn fast_bitmap_glyph(ch: char) -> Option<char> {
    if ch.is_ascii() {
        return Some(ch);
    }
    match ch {
        '\u{00a0}' => Some(' '),
        '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2212}' => Some('-'),
        '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{2032}' => Some('\''),
        '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{2033}' => Some('"'),
        '\u{2026}' => Some('.'),
        '\u{00b7}' | '\u{2022}' => Some('*'),
        _ => None,
    }
}

fn draw_ascii_glyph(
    pixels: &mut [PremultipliedColorU8],
    pixmap_width: i32,
    pixmap_height: i32,
    x: i32,
    y: i32,
    scale: i32,
    glyph: [u8; 7],
    color: Color,
) {
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..5 {
            if (bits >> (4 - col)) & 1 == 0 {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    let pixel_x = x + col * scale + dx;
                    let pixel_y = y + row as i32 * scale + dy;
                    if pixel_x < 0
                        || pixel_y < 0
                        || pixel_x >= pixmap_width
                        || pixel_y >= pixmap_height
                    {
                        continue;
                    }
                    let idx = pixel_y as usize * pixmap_width as usize + pixel_x as usize;
                    if let Some(dst) = pixels.get_mut(idx) {
                        composite_over_rgba(dst, color.r, color.g, color.b, color.a);
                    }
                }
            }
        }
    }
}

fn ascii_glyph(ch: char) -> [u8; 7] {
    match ch.to_ascii_lowercase() {
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        'a' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'b' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'c' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'd' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'e' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'f' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'g' => [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        'h' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'i' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'j' => [
            0b00001, 0b00001, 0b00001, 0b00001, 0b10001, 0b10001, 0b01110,
        ],
        'k' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'l' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'm' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'n' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'o' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'p' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'r' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        's' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        't' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'u' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'v' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b01010, 0b00100,
        ],
        'w' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'x' => [
            0b10001, 0b01010, 0b01010, 0b00100, 0b01010, 0b01010, 0b10001,
        ],
        'y' => [
            0b10001, 0b01010, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100,
        ],
        ',' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100, 0b01000,
        ],
        ':' => [
            0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000,
        ],
        ';' => [
            0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b01000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '_' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        '\\' => [
            0b10000, 0b01000, 0b01000, 0b00100, 0b00010, 0b00010, 0b00001,
        ],
        '?' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
        '!' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
        '=' => [
            0b00000, 0b11111, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000,
        ],
        '&' => [
            0b01100, 0b10010, 0b10100, 0b01000, 0b10101, 0b10010, 0b01101,
        ],
        '#' => [
            0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010,
        ],
        '%' => [
            0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011,
        ],
        '+' => [
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
        '*' => [
            0b00000, 0b10101, 0b01110, 0b11111, 0b01110, 0b10101, 0b00000,
        ],
        '@' => [
            0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01111,
        ],
        '(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        ')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        '[' => [
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
        ],
        ']' => [
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
        ],
        '{' => [
            0b00010, 0b00100, 0b00100, 0b01000, 0b00100, 0b00100, 0b00010,
        ],
        '}' => [
            0b01000, 0b00100, 0b00100, 0b00010, 0b00100, 0b00100, 0b01000,
        ],
        '<' => [
            0b00010, 0b00100, 0b01000, 0b10000, 0b01000, 0b00100, 0b00010,
        ],
        '>' => [
            0b01000, 0b00100, 0b00010, 0b00001, 0b00010, 0b00100, 0b01000,
        ],
        '\'' => [
            0b00100, 0b00100, 0b01000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '"' => [
            0b01010, 0b01010, 0b01010, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '|' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        '`' => [
            0b01000, 0b00100, 0b00010, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '~' => [
            0b00000, 0b00000, 0b01000, 0b10101, 0b00010, 0b00000, 0b00000,
        ],
        '^' => [
            0b00100, 0b01010, 0b10001, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '$' => [
            0b00100, 0b01111, 0b10100, 0b01110, 0b00101, 0b11110, 0b00100,
        ],
        _ => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
    }
}

/// Porter-Duff "src over dst" composite in premultiplied RGBA8 space.
///
/// `src` carries straight RGBA from the cosmic-text pixel callback; it is
/// premultiplied here before the blend so the output satisfies the invariant
/// `red <= alpha` required by `PremultipliedColorU8`.
fn composite_over(dst: &mut PremultipliedColorU8, src: CosmicColor) {
    composite_over_rgba(dst, src.r(), src.g(), src.b(), src.a());
}

fn composite_over_rgba(dst: &mut PremultipliedColorU8, r: u8, g: u8, b: u8, a: u8) {
    let sa = u32::from(a);
    if sa == 0 {
        return;
    }

    let sr = (u32::from(r) * sa + 127) / 255;
    let sg = (u32::from(g) * sa + 127) / 255;
    let sb = (u32::from(b) * sa + 127) / 255;

    let inv_sa = 255 - sa;
    let dr = u32::from(dst.red());
    let dg = u32::from(dst.green());
    let db = u32::from(dst.blue());
    let da = u32::from(dst.alpha());

    let out_r = (sr + (dr * inv_sa + 127) / 255).min(255) as u8;
    let out_g = (sg + (dg * inv_sa + 127) / 255).min(255) as u8;
    let out_b = (sb + (db * inv_sa + 127) / 255).min(255) as u8;
    let out_a = (sa + (da * inv_sa + 127) / 255).min(255) as u8;

    if let Some(blended) = PremultipliedColorU8::from_rgba(out_r, out_g, out_b, out_a) {
        *dst = blended;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_raster_path_marks_expected_pixels() {
        let mut buf = vec![0xffu8; 32 * 32 * 4];
        let color = Color {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        };
        {
            let Some(mut pixmap) = tiny_skia::PixmapMut::from_bytes(&mut buf, 32, 32) else {
                panic!("could not create pixmap");
            };
            rasterize_glyphs("X", 12.0, color, &mut pixmap, (2.0, 3.0));
        }

        let idx = (3 * 32 + 2) * 4;
        assert_eq!(&buf[idx..idx + 4], &[0, 0, 0, 255]);
    }

    #[test]
    fn fast_bitmap_glyph_maps_common_unicode_punctuation() {
        assert_eq!(fast_bitmap_glyph('\u{00a0}'), Some(' '));
        assert_eq!(fast_bitmap_glyph('\u{2014}'), Some('-'));
        assert_eq!(fast_bitmap_glyph('\u{2019}'), Some('\''));
        assert_eq!(fast_bitmap_glyph('\u{201c}'), Some('"'));
        assert_eq!(fast_bitmap_glyph('\u{2026}'), Some('.'));
        assert_eq!(fast_bitmap_glyph('\u{4e2d}'), None);
    }
}
