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

    let mut state = TEXT_STATE.lock().unwrap_or_else(|e| e.into_inner());
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
        for glyph in run.glyphs.iter() {
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

/// Porter-Duff "src over dst" composite in premultiplied RGBA8 space.
///
/// `src` carries straight RGBA from the cosmic-text pixel callback; it is
/// premultiplied here before the blend so the output satisfies the invariant
/// `red <= alpha` required by `PremultipliedColorU8`.
fn composite_over(dst: &mut PremultipliedColorU8, src: CosmicColor) {
    let sa = src.a() as u32;
    if sa == 0 {
        return;
    }

    // Premultiply source (straight -> premultiplied, rounding to nearest).
    let sr = (src.r() as u32 * sa + 127) / 255;
    let sg = (src.g() as u32 * sa + 127) / 255;
    let sb = (src.b() as u32 * sa + 127) / 255;

    let inv_sa = 255 - sa;
    let dr = dst.red() as u32;
    let dg = dst.green() as u32;
    let db = dst.blue() as u32;
    let da = dst.alpha() as u32;

    let out_r = (sr + (dr * inv_sa + 127) / 255).min(255) as u8;
    let out_g = (sg + (dg * inv_sa + 127) / 255).min(255) as u8;
    let out_b = (sb + (db * inv_sa + 127) / 255).min(255) as u8;
    let out_a = (sa + (da * inv_sa + 127) / 255).min(255) as u8;

    if let Some(blended) = PremultipliedColorU8::from_rgba(out_r, out_g, out_b, out_a) {
        *dst = blended;
    }
}
