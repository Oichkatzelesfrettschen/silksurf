/*
 * display_list_batched.rs -- type-batched rasterization for DisplayList.
 *
 * WHY: The standard DisplayList is a Vec<DisplayItem> with a variant enum.
 * Each rasterization pass dispatches on the variant for every element, which
 * causes branch mispredictions in the inner loop once the list grows.
 * Separating items by type into two typed sub-lists (solid_colors, texts)
 * makes each pass branch-free within its loop: solid_colors is iterated with
 * only fill_rect calls; texts is iterated separately with the same fill_rect
 * stub (a full glyph rasterizer is future work, P8.S8).
 *
 * WHAT: DisplayListBatched holds two flat Vec fields partitioned from a
 * DisplayList. rasterize_damage applies the same damage-rect intersection
 * guard as the scalar path, but iterates solid_colors then texts without
 * any per-element variant dispatch.
 *
 * HOW: Enabled by Cargo feature "batched-raster" (additive, off by default).
 *   Build: cargo build -p silksurf-render --features batched-raster
 *   Test:  cargo test -p silksurf-render --features batched-raster
 *
 * See: lib.rs rasterize_damage -- the scalar reference implementation.
 * See: SNAZZY-WAFFLE roadmap TODO(perf): SoA DisplayList.
 */

use silksurf_css::Color;
use silksurf_dom::NodeId;
use silksurf_layout::Rect;

use crate::{DisplayItem, DisplayList};

/*
 * DisplayListBatched -- type-partitioned display list.
 *
 * WHY: Two typed Vecs eliminate per-element variant dispatch in hot loops.
 * solid_colors carries (Rect, Color) pairs; texts carries the full
 * Text payload needed for glyph layout once that stage is implemented.
 *
 * INVARIANT: Partition preserves document order within each sub-list.
 * Painting order is preserved per-type, not globally interleaved.
 * For the current opaque-rect model this is semantically equivalent;
 * a future compositor with blending must revisit this.
 */
#[derive(Debug, Clone)]
pub struct DisplayListBatched {
    /// Solid-color rectangle items: (rect, color).
    pub solid_colors: Vec<(Rect, Color)>,
    /// Text items: (rect, node_id, text_len, color).
    ///
    /// text_len is the character count of the source text node, carried for
    /// future glyph rasterization (P8.S8). The color fills the rect today.
    pub texts: Vec<(Rect, NodeId, u32, Color)>,
}

impl DisplayListBatched {
    /*
     * from_display_list -- partition a DisplayList into typed sub-lists.
     *
     * WHY: Single-pass scan preserves allocation locality: solid_colors and
     * texts are each populated in document order, minimizing cache thrash at
     * rasterization time.
     *
     * WHAT: Iterates display_list.items once; each SolidColor pushes into
     * solid_colors, each Text pushes into texts.
     *
     * HOW: O(n) time and O(n) space where n = display_list.items.len().
     */
    pub fn from_display_list(dl: &DisplayList) -> Self {
        let mut solid_colors: Vec<(Rect, Color)> = Vec::new();
        let mut texts: Vec<(Rect, NodeId, u32, Color)> = Vec::new();

        for item in &dl.items {
            match item {
                DisplayItem::SolidColor { rect, color } => {
                    solid_colors.push((*rect, *color));
                }
                DisplayItem::Text {
                    rect,
                    node,
                    text_len,
                    color,
                } => {
                    texts.push((*rect, *node, *text_len, *color));
                }
            }
        }

        Self {
            solid_colors,
            texts,
        }
    }

    /*
     * rasterize_damage -- rasterize items that overlap `damage` into an RGBA buffer.
     *
     * WHY: Same semantics as rasterize_damage in lib.rs but with branch-free
     * inner loops: solid_colors is iterated first (no variant check), then
     * texts (no variant check). This mirrors the SoA layout pattern described
     * in docs/design/RENDER-ARCHITECTURE.md.
     *
     * WHAT: Allocates width*height*4 bytes initialised to 0xFF (white
     * background). Clips each rect to the damage region before painting to
     * avoid touching pixels outside the dirty area. Delegates to fill_rect
     * from the parent crate for the actual pixel write.
     *
     * HOW: Item order: solid_colors before texts. Within each sub-list,
     * document (push) order is preserved. Items whose rects do not intersect
     * `damage` are skipped with a single f32 comparison, matching lib.rs.
     */
    pub fn rasterize_damage(&self, width: u32, height: u32, damage: Rect) -> Vec<u8> {
        let mut buffer = vec![255u8; (width * height * 4) as usize];

        for (rect, color) in &self.solid_colors {
            if !rect_intersects(*rect, damage) {
                continue;
            }
            fill_rect(&mut buffer, width, height, *rect, *color);
        }

        for (rect, _node, _text_len, color) in &self.texts {
            if !rect_intersects(*rect, damage) {
                continue;
            }
            fill_rect(&mut buffer, width, height, *rect, *color);
        }

        buffer
    }

    /*
     * item_count -- total number of display items across both sub-lists.
     *
     * WHY: Convenience for callers that need the logical item count without
     * caring about the type split. Equivalent to DisplayList.items.len() for
     * the same source list.
     */
    pub fn item_count(&self) -> usize {
        self.solid_colors.len() + self.texts.len()
    }
}

// ---------------------------------------------------------------------------
// Private helpers -- mirrors the private fill_rect / rect_intersects in lib.rs.
// Kept private to this module; pub re-export from lib.rs is the stable API.
// ---------------------------------------------------------------------------

fn rect_intersects(a: Rect, b: Rect) -> bool {
    let ax1 = a.x + a.width;
    let ay1 = a.y + a.height;
    let bx1 = b.x + b.width;
    let by1 = b.y + b.height;
    a.x < bx1 && ax1 > b.x && a.y < by1 && ay1 > b.y
}

fn fill_rect(buffer: &mut [u8], width: u32, height: u32, rect: Rect, color: Color) {
    let x0 = rect.x.max(0.0).floor() as i32;
    let y0 = rect.y.max(0.0).floor() as i32;
    let x1 = (rect.x + rect.width).min(width as f32).ceil() as i32;
    let y1 = (rect.y + rect.height).min(height as f32).ceil() as i32;

    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let width_usize = width as usize;
    let pixel = u32::from_le_bytes([color.r, color.g, color.b, color.a]);
    let len_u32 = buffer.len() / 4;

    // SAFETY: Vec<u8> allocated by vec![255u8; ...] in rasterize_damage has
    // alignment >= alignof::<u32>() (Rust global allocator guarantees max
    // alignment). len_u32 = buffer.len() / 4 is the exact number of u32
    // chunks that fit. We hold the exclusive &mut borrow on buffer for the
    // duration of this call, so no aliasing is possible.
    let buffer_u32 =
        unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u32, len_u32) };

    for y in y0..y1 {
        if y < 0 || y >= height as i32 {
            continue;
        }
        let row_start = y as usize * width_usize + x0.max(0) as usize;
        let row_end = y as usize * width_usize + x1.min(width as i32) as usize;
        if row_start >= row_end || row_end > buffer_u32.len() {
            continue;
        }
        buffer_u32[row_start..row_end].fill(pixel);
    }
}
