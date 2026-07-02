/*
 * render/lib.rs -- display list construction and tile-based rasterization.
 *
 * The final rendering stage converts positioned layout boxes into a flat
 * DisplayList of paint commands and rasterizes them to an RGBA pixel buffer.
 *
 * build_display_list walks the layout tree depth-first. with_tiles partitions
 * display items into spatial tile buckets. rasterize_damage paints only items
 * that intersect the damage region. fill_rect uses the fastest row-fill path
 * available on the target CPU.
 *
 * Tile-based rendering divides the viewport into fixed-size buckets. Each
 * bucket stores display item indices. Damage rasterization processes buckets
 * that overlap the dirty rectangle.
 *
 * fill_row_sse2 fills four x86 pixels per store. fill_row_neon mirrors that
 * path on aarch64. Scalar fill remains the portable fallback.
 */
#![allow(
    clippy::collapsible_if,
    clippy::needless_borrow,
    clippy::manual_div_ceil
)]

use rustc_hash::FxHashMap;
use silksurf_css::{BoxShadow as CssBoxShadow, Color, ComputedStyle};
use silksurf_dom::{Dom, NodeId, NodeKind};
use silksurf_layout::{LayoutTree, Rect};
use std::sync::Arc;
use tiny_skia::{
    FillRule, GradientStop, LinearGradient, Paint, PathBuilder, PixmapMut, Point, SpreadMode,
    Transform,
};

/// Type-batched rasterization (feature "batched-raster").
///
/// WHY: Separating DisplayList items by type into two typed sub-lists lets
/// each rasterization pass iterate without per-element variant dispatch,
/// enabling branch-free inner loops and better auto-vectorization.
/// See display_list_batched.rs for the full design note.
#[cfg(feature = "batched-raster")]
pub mod display_list_batched;

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayList {
    pub items: Vec<DisplayItem>,
    pub tiles: Option<DisplayListTiles>,
}

#[derive(Default)]
pub struct DamageScratch {
    pixels: Vec<u8>,
    item_indices: Vec<usize>,
    seen_items: Vec<bool>,
    last_damage: Option<DamagePixelRect>,
}

impl DamageScratch {
    pub fn pixel_ptr(&self) -> *const u8 {
        self.pixels.as_ptr()
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn last_damage(&self) -> Option<DamagePixelRect> {
        self.last_damage
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayListTiles {
    tile_size: u32,
    tiles_x: u32,
    tiles_y: u32,
    buckets: Vec<Vec<usize>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayItem {
    SolidColor {
        rect: Rect,
        color: Color,
    },
    Text {
        rect: Rect,
        node: NodeId,
        text_len: u32,
        /// Shaped text content (UTF-8). Carried for glyph rasterization in
        /// the tiny-skia path; also available for accessibility consumers.
        text: String,
        /// Font size in pixels, from the computed style at display list build
        /// time. Required by cosmic-text shaping in `rasterize_skia_into`.
        font_size: f32,
        color: Color,
    },
    /// Anti-aliased rounded rectangle.
    ///
    /// `radii` is `[top-left, top-right, bottom-right, bottom-left]` corner
    /// radii in CSS clockwise order. Scalar rasterizers fall back to a plain
    /// `fill_rect`; the tiny-skia path renders anti-aliased cubic bezier arcs.
    RoundedRect {
        rect: Rect,
        radii: [f32; 4],
        color: Color,
    },
    /// CSS box-shadow drop shadow (outset only; inset is deferred).
    ///
    /// `rect` is the element's content rect. Renderers compute the shadow's
    /// actual bounds from the CSS offset and spread fields inside `shadow`.
    /// Scalar paths fall back to a solid rect fill; the tiny-skia path will
    /// add blur in a future pass.
    BoxShadow {
        rect: Rect,
        shadow: CssBoxShadow,
    },
    /// CSS linear-gradient background.
    ///
    /// `angle` follows the CSS convention: 0.0 = to top, 90.0 = to right.
    /// `stops` is a list of (position [0.0, 1.0], color) pairs in order.
    /// Scalar paths fill with the first stop color; the tiny-skia path
    /// renders the gradient through the full element rect.
    LinearGradient {
        rect: Rect,
        angle: f32,
        stops: Vec<(f32, Color)>,
    },
    Image {
        rect: Rect,
        image: ImageSurface,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSurface {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

// The styles map is pinned to FxHashMap by the layout/render pipeline
// for performance (FxHasher on NodeId integer keys vs default SipHash).
// Loosening this to a generic BuildHasher would force the same change
// through every inner display-list builder.
#[allow(clippy::implicit_hasher)]
pub fn build_display_list(
    dom: &Dom,
    styles: &FxHashMap<NodeId, ComputedStyle>,
    layout: &LayoutTree<'_>,
) -> DisplayList {
    let capacity = estimate_display_items(&layout.root);
    let mut list = DisplayList {
        items: Vec::with_capacity(capacity),
        tiles: None,
    };
    build_display_list_for_box(dom, styles, &layout.root, &mut list);
    list
}

impl DisplayList {
    #[must_use]
    pub fn with_tiles(mut self, width: u32, height: u32, tile_size: u32) -> Self {
        if width == 0 || height == 0 || tile_size == 0 {
            return self;
        }
        self.tiles = Some(build_tiles(&self.items, width, height, tile_size));
        self
    }
}

fn build_display_list_for_box(
    dom: &Dom,
    styles: &FxHashMap<NodeId, ComputedStyle>,
    layout: &silksurf_layout::LayoutBox<'_>,
    list: &mut DisplayList,
) {
    match layout.box_type {
        silksurf_layout::BoxType::BlockNode(node_id)
        | silksurf_layout::BoxType::InlineNode(node_id) => {
            if let Some(style) = styles.get(&node_id) {
                let content_rect = layout.dimensions().content;
                // Box-shadow paints below the background (CSS paint order).
                if let Some(shadow) = style.box_shadow {
                    if !shadow.inset {
                        list.items.push(DisplayItem::BoxShadow {
                            rect: content_rect,
                            shadow,
                        });
                    }
                }
                if style.background_color.a > 0 {
                    if style.border_radius > 0.0 {
                        list.items.push(DisplayItem::RoundedRect {
                            rect: content_rect,
                            radii: [style.border_radius; 4],
                            color: style.background_color,
                        });
                    } else {
                        list.items.push(DisplayItem::SolidColor {
                            rect: content_rect,
                            color: style.background_color,
                        });
                    }
                }
                if let Ok(node) = dom.node(node_id) {
                    if let NodeKind::Text { .. } = node.kind() {
                        let (text_len, text_content) = match node.kind() {
                            NodeKind::Text { text } => (text.len() as u32, text.clone()),
                            _ => (0, String::new()),
                        };
                        let font_size_px = match style.font_size {
                            silksurf_css::Length::Px(px) => px,
                            _ => 16.0,
                        };
                        list.items.push(DisplayItem::Text {
                            rect: content_rect,
                            node: node_id,
                            text_len,
                            text: text_content,
                            font_size: font_size_px,
                            color: style.color,
                        });
                    }
                }
            }
        }
        silksurf_layout::BoxType::Anonymous => {}
    }

    for child in &layout.children {
        build_display_list_for_box(dom, styles, child, list);
    }
}

#[must_use]
pub fn rasterize(display_list: &DisplayList, width: u32, height: u32) -> Vec<u8> {
    let damage = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    rasterize_damage(display_list, width, height, damage)
}

#[must_use]
pub fn rasterize_damage(
    display_list: &DisplayList,
    width: u32,
    height: u32,
    damage: Rect,
) -> Vec<u8> {
    let mut buffer = vec![255; (width * height * 4) as usize];
    let item_indices = if let Some(tiles) = &display_list.tiles {
        tiles.items_for_rect(damage)
    } else {
        (0..display_list.items.len()).collect()
    };
    let mut seen = vec![false; display_list.items.len()];
    for index in item_indices {
        if index >= display_list.items.len() || seen[index] {
            continue;
        }
        seen[index] = true;
        let item = &display_list.items[index];
        let rect = item_rect(item);
        if !rect_intersects(rect, damage) {
            continue;
        }
        // The Text and RoundedRect arms currently degrade to a solid fill,
        // matching the scalar pre-shaping pipeline. Keeping them as separate
        // arms documents the planned divergence (text shaping, corner
        // antialiasing) and ensures the dispatcher stays in one place.
        #[allow(clippy::match_same_arms)]
        match item {
            DisplayItem::SolidColor { rect, color } => {
                fill_rect(&mut buffer, width, height, *rect, *color);
            }
            DisplayItem::Text { rect, color, .. } => {
                fill_rect(&mut buffer, width, height, *rect, *color);
            }
            DisplayItem::RoundedRect { rect, color, .. } => {
                fill_rect(&mut buffer, width, height, *rect, *color);
            }
            DisplayItem::BoxShadow { rect, shadow } => {
                if !shadow.inset {
                    let shadow_rect = box_shadow_rect(*rect, shadow);
                    fill_rect(&mut buffer, width, height, shadow_rect, shadow.color);
                }
            }
            DisplayItem::LinearGradient { rect, stops, .. } => {
                // Scalar fallback: fill with first stop color.
                if let Some(pair) = stops.first() {
                    fill_rect(&mut buffer, width, height, *rect, pair.1);
                }
            }
            DisplayItem::Image { rect, image } => {
                blit_image_nearest(&mut buffer, width, height, *rect, image);
            }
        }
    }
    buffer
}

fn item_rect(item: &DisplayItem) -> Rect {
    match item {
        DisplayItem::SolidColor { rect, .. }
        | DisplayItem::Text { rect, .. }
        | DisplayItem::RoundedRect { rect, .. }
        | DisplayItem::LinearGradient { rect, .. }
        | DisplayItem::Image { rect, .. } => *rect,
        DisplayItem::BoxShadow { rect, shadow } => box_shadow_rect(*rect, shadow),
    }
}

fn rect_intersects(a: Rect, b: Rect) -> bool {
    let ax1 = a.x + a.width;
    let ay1 = a.y + a.height;
    let bx1 = b.x + b.width;
    let by1 = b.y + b.height;
    a.x < bx1 && ax1 > b.x && a.y < by1 && ay1 > b.y
}

fn build_tiles(items: &[DisplayItem], width: u32, height: u32, tile_size: u32) -> DisplayListTiles {
    let tiles_x = (width + tile_size - 1) / tile_size;
    let tiles_y = (height + tile_size - 1) / tile_size;
    let mut buckets = vec![Vec::new(); (tiles_x * tiles_y) as usize];
    for (index, item) in items.iter().enumerate() {
        let rect = item_rect(item);
        let x0 = rect.x.max(0.0).floor() as i32;
        let y0 = rect.y.max(0.0).floor() as i32;
        let x1 = (rect.x + rect.width).min(width as f32).ceil() as i32;
        let y1 = (rect.y + rect.height).min(height as f32).ceil() as i32;
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        let tx0 = (x0.max(0) as u32) / tile_size;
        let ty0 = (y0.max(0) as u32) / tile_size;
        let tx1 = ((x1.max(1) as u32).saturating_sub(1)) / tile_size;
        let ty1 = ((y1.max(1) as u32).saturating_sub(1)) / tile_size;
        for ty in ty0..=ty1.min(tiles_y.saturating_sub(1)) {
            for tx in tx0..=tx1.min(tiles_x.saturating_sub(1)) {
                let tile_index = (ty * tiles_x + tx) as usize;
                if let Some(bucket) = buckets.get_mut(tile_index) {
                    bucket.push(index);
                }
            }
        }
    }
    DisplayListTiles {
        tile_size,
        tiles_x,
        tiles_y,
        buckets,
    }
}

impl DisplayListTiles {
    pub fn items_for_rect(&self, rect: Rect) -> Vec<usize> {
        let mut items = Vec::new();
        self.items_for_rect_into(rect, &mut items);
        items
    }

    fn items_for_rect_into(&self, rect: Rect, items: &mut Vec<usize>) {
        let x0 = rect.x.max(0.0).floor() as i32;
        let y0 = rect.y.max(0.0).floor() as i32;
        let x1 = (rect.x + rect.width).max(0.0).ceil() as i32;
        let y1 = (rect.y + rect.height).max(0.0).ceil() as i32;
        let tx0 = (x0.max(0) as u32) / self.tile_size;
        let ty0 = (y0.max(0) as u32) / self.tile_size;
        let tx1 = ((x1.max(1) as u32).saturating_sub(1)) / self.tile_size;
        let ty1 = ((y1.max(1) as u32).saturating_sub(1)) / self.tile_size;
        items.clear();
        for ty in ty0..=ty1.min(self.tiles_y.saturating_sub(1)) {
            for tx in tx0..=tx1.min(self.tiles_x.saturating_sub(1)) {
                let tile_index = (ty * self.tiles_x + tx) as usize;
                if let Some(bucket) = self.buckets.get(tile_index) {
                    items.extend(bucket.iter().copied());
                }
            }
        }
    }
}

fn estimate_display_items(layout: &silksurf_layout::LayoutBox<'_>) -> usize {
    let mut count = 1;
    for child in &layout.children {
        count += estimate_display_items(child);
    }
    count
}
fn fill_rect(buffer: &mut [u8], width: u32, height: u32, rect: Rect, color: Color) {
    let x0 = rect.x.max(0.0).floor() as i32;
    let y0 = rect.y.max(0.0).floor() as i32;
    let x1 = (rect.x + rect.width).min(width as f32).ceil() as i32;
    let y1 = (rect.y + rect.height).min(height as f32).ceil() as i32;

    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let width_u = width as usize;
    let pixel = u32::from_le_bytes([color.r, color.g, color.b, color.a]);
    let len_u32 = buffer.len() / 4;
    // SAFETY: Vec<u8> from a u32 framebuffer is always 4-byte aligned (Vec
    // alignment >= alignof::<u32>) and len_u32 = buffer.len() / 4 is the
    // exact number of u32-sized chunks that fit. The returned slice
    // covers the same memory as `buffer` for its lifetime; we hold the
    // exclusive &mut borrow on `buffer` so no aliasing is possible.
    // SAFETY for the cast: framebuffer allocation is always 4-byte aligned
    // because Vec<u8>::as_mut_ptr() returns a pointer to a u32-aligned
    // allocation (the framebuffer is sized in u32 units; see comment above).
    // clippy::cast_ptr_alignment fires conservatively for any u8 -> wider
    // pointer cast even when the underlying allocation guarantees alignment.
    #[allow(clippy::cast_ptr_alignment)]
    let buffer_u32 =
        // SAFETY: the buffer allocation uses u32 framebuffer alignment and len_u32 stays in bounds.
        unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<u32>(), len_u32) };

    for y in y0..y1 {
        if y < 0 || y >= height as i32 {
            continue;
        }
        let row_start = y as usize * width_u + x0.max(0) as usize;
        let row_end = y as usize * width_u + x1.min(width as i32) as usize;
        if row_start >= row_end || row_end > buffer_u32.len() {
            continue;
        }
        fill_row_u32(&mut buffer_u32[row_start..row_end], pixel);
    }
}

fn fill_row_u32(row: &mut [u32], pixel: u32) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("sse2") {
            // SAFETY: is_x86_feature_detected!("sse2") gates the call;
            // fill_row_sse2 only uses SSE2 intrinsics (_mm_set1_epi32 and
            // _mm_storeu_si128), both available whenever SSE2 is.
            unsafe {
                fill_row_sse2(row, pixel);
            }
            return;
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon") {
            // SAFETY: is_aarch64_feature_detected!("neon") gates the call.
            // fill_row_neon uses only vdupq_n_u32 + vst1q_u32, both
            // available whenever NEON is present. NEON is mandatory on every
            // aarch64-unknown-linux-gnu target so this branch is always taken
            // in practice; the scalar fallback below is kept for correctness.
            unsafe {
                fill_row_neon(row, pixel);
            }
            return;
        }
    }
    row.fill(pixel);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
unsafe fn fill_row_sse2(row: &mut [u32], pixel: u32) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{__m128i, _mm_set1_epi32, _mm_storeu_si128};

    let len = row.len();
    if len == 0 {
        return;
    }
    let mut idx = 0usize;
    let ptr = row.as_mut_ptr();
    // SAFETY: _mm_set1_epi32 has no preconditions beyond SSE2 availability,
    // which the caller has already verified (see fill_row_u32).
    let value = unsafe { _mm_set1_epi32(pixel as i32) };
    while idx + 4 <= len {
        // SAFETY: idx + 4 <= len guarantees ptr.add(idx) is in-bounds and
        // ptr.add(idx + 3) is the last element of the chunk. The cast to
        // *mut __m128i is sound because u32 is 4-byte aligned and 4*u32 =
        // 16 bytes which __m128i expects (and _mm_storeu_si128 tolerates
        // unaligned pointers anyway).
        #[allow(clippy::cast_ptr_alignment)]
        let dst = unsafe { ptr.add(idx) }.cast::<__m128i>();
        // SAFETY: dst points to 16 valid writable bytes (4 * u32) within
        // the row borrow held by the caller; storeu does not require
        // alignment.
        unsafe {
            _mm_storeu_si128(dst, value);
        }
        idx += 4;
    }
    while idx < len {
        // SAFETY: idx < len guarantees ptr.add(idx) is in-bounds and we
        // hold the exclusive &mut on row, so the write is sound.
        unsafe {
            *ptr.add(idx) = pixel;
        }
        idx += 1;
    }
}

/*
 * fill_row_neon -- AArch64 NEON 4-pixel fill (P8.S7).
 *
 * WHY: AArch64 carries mandatory NEON; mirroring the SSE2 path closes the
 * throughput gap on AArch64 Linux / Apple Silicon cross-builds.
 *
 * WHAT: vdupq_n_u32 broadcasts the pixel value to a uint32x4_t lane.
 * vst1q_u32 stores 4 lanes per iteration (16 bytes, unaligned-safe on
 * AArch64). Tail bytes are stored with scalar writes.
 *
 * HOW: Called from fill_row_u32 after is_aarch64_feature_detected!("neon").
 * See UNSAFE-CONTRACTS.md #U11 for the invariant record.
 */
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn fill_row_neon(row: &mut [u32], pixel: u32) {
    use std::arch::aarch64::*;

    let len = row.len();
    if len == 0 {
        return;
    }
    let mut idx = 0usize;
    let ptr = row.as_mut_ptr();
    // SAFETY: vdupq_n_u32 has no preconditions beyond NEON availability,
    // which the caller has verified via is_aarch64_feature_detected!("neon")
    // and which the #[target_feature] attribute enforces at the call site.
    let value = unsafe { vdupq_n_u32(pixel) };
    while idx + 4 <= len {
        // SAFETY: idx + 4 <= len guarantees the 4-element window
        // ptr.add(idx)..ptr.add(idx+4) lies within the slice borrow.
        // vst1q_u32 is unaligned-safe on AArch64 (no alignment requirement
        // unlike SSE storeu which documents 1-byte alignment anyway).
        // We hold the exclusive &mut on row so concurrent aliasing is ruled out.
        unsafe {
            vst1q_u32(ptr.add(idx), value);
        }
        idx += 4;
    }
    while idx < len {
        // SAFETY: idx < len guarantees ptr.add(idx) is in-bounds; exclusive
        // &mut on row rules out aliasing.
        unsafe {
            *ptr.add(idx) = pixel;
        }
        idx += 1;
    }
}

// ============================================================================
// Determinism test helpers: scalar and SIMD fill entry points
// ============================================================================

/*
 * fill_scalar -- plain scalar fill of a u32 pixel row.
 *
 * WHY: Exposes the scalar fallback path so the determinism test in
 * tests/determinism.rs can compare it byte-for-byte against fill_simd.
 * Both must produce identical output for every (buf, color) pair.
 *
 * WHAT: Iterates each element and writes `color` directly. No intrinsics.
 * HOW: Called from tests/determinism.rs via the public crate API.
 */
pub fn fill_scalar(buf: &mut [u32], color: u32) {
    buf.fill(color);
}

/*
 * fill_simd -- SIMD-accelerated fill of a u32 pixel row.
 *
 * WHY: Exposes the SSE2 fast path so the determinism test can verify it
 * produces exactly the same bytes as fill_scalar for every input.
 * On non-x86 targets this delegates to fill_scalar so the test still
 * compiles and passes (scalar == scalar is trivially true, but ensures
 * no platform-specific divergence is accidentally introduced later).
 *
 * WHAT: On x86/x86_64 with SSE2: uses _mm_set1_epi32 + _mm_storeu_si128
 * to write 4 pixels per store, then a scalar tail. Delegates to fill_scalar
 * on all other targets.
 * HOW: Called from tests/determinism.rs via the public crate API.
 */
pub fn fill_simd(buf: &mut [u32], color: u32) {
    fill_row_u32(buf, color);
}

// ============================================================================
// Color science -- sRGB <-> linear and alpha premultiplication
// ============================================================================

/*
 * srgb_to_linear -- map an sRGB encoded byte to linear-light f32 [0.0, 1.0].
 *
 * WHY: Compositing in perceptually-encoded (sRGB) space produces incorrect
 * results because sRGB is not additive. All blending and alpha compositing
 * must happen in linear light. The transfer function applied here is the
 * IEC 61966-2-1 (sRGB) piecewise formula.
 *
 * WHAT: Converts a u8 sRGB channel to f32 linear. The two-segment formula:
 *   c_srgb / 255.0 <= 0.04045 => c_lin = c_srgb / (255.0 * 12.92)
 *   c_srgb / 255.0 >  0.04045 => c_lin = ((c_srgb/255.0 + 0.055) / 1.055)^2.4
 *
 * Output is clamped to [0.0, 1.0] to guard against floating-point surprises
 * near the boundary.
 *
 * See: IEC 61966-2-1:1999, section 4.2.
 * See: docs/design/COLOR.md, section "sRGB <-> Linear Conversion".
 */
#[must_use]
pub fn srgb_to_linear(c: u8) -> f32 {
    let c_f = f32::from(c) / 255.0;
    let linear = if c_f <= 0.04045 {
        c_f / 12.92
    } else {
        ((c_f + 0.055) / 1.055).powf(2.4)
    };
    linear.clamp(0.0, 1.0)
}

/*
 * linear_to_srgb -- map a linear-light f32 [0.0, 1.0] back to a u8 sRGB byte.
 *
 * WHY: After compositing in linear light we must encode back to sRGB for
 * display and for storage in the ARGB u32 framebuffer format.
 *
 * WHAT: Inverse of the IEC 61966-2-1 piecewise formula, then round to u8:
 *   c_lin <= 0.0031308 => c_srgb = c_lin * 12.92
 *   c_lin >  0.0031308 => c_srgb = 1.055 * c_lin^(1/2.4) - 0.055
 * Result multiplied by 255.0 and rounded to nearest integer, then clamped
 * to [0, 255] to handle floating-point edge values >= 1.0.
 *
 * See: IEC 61966-2-1:1999, section 4.2.
 * See: docs/design/COLOR.md, section "sRGB <-> Linear Conversion".
 */
#[must_use]
pub fn linear_to_srgb(c: f32) -> u8 {
    let c_clamped = c.clamp(0.0, 1.0);
    let encoded = if c_clamped <= 0.003_130_8 {
        c_clamped * 12.92
    } else {
        1.055 * c_clamped.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

/*
 * premultiply -- apply alpha premultiplication to sRGB r, g, b channels.
 *
 * WHY: Straight (unassociated) alpha stored in ARGB must be converted to
 * premultiplied (associated) alpha before compositing. Premultiplied form
 * avoids a division in the Porter-Duff compositing equations and produces
 * correct results at alpha edges.
 *
 * WHAT: premult_channel = round(straight_channel * a / 255). Uses the
 * integer approximation (c * a + 127 + ((c * a + 127) >> 8)) >> 8 to achieve
 * correctly-rounded results without floating-point conversion overhead.
 * This approximation is used by Cairo, Skia, and pixman.
 *
 * NOTE: operates in sRGB encoded space. Full compositing correctness requires
 * linear-light premultiplication; this function is for framebuffer packing
 * where premultiplied-sRGB is the convention. See docs/design/COLOR.md.
 *
 * See: Porter, T. and Duff, T. "Compositing Digital Images." SIGGRAPH 1984.
 * See: docs/design/COLOR.md, section "Alpha Premultiplication Policy".
 */
#[must_use]
pub fn premultiply(r: u8, g: u8, b: u8, a: u8) -> (u8, u8, u8) {
    let alpha = u32::from(a);
    let premult = |c: u8| -> u8 {
        let ca = u32::from(c) * alpha + 127;
        ((ca + (ca >> 8)) >> 8) as u8
    };
    (premult(r), premult(g), premult(b))
}

/*
 * unpremultiply -- recover straight alpha r, g, b from premultiplied channels.
 *
 * WHY: Premultiplied pixels stored in the framebuffer must be divided back
 * out before re-encoding to sRGB for export or colour picking. This is the
 * inverse of premultiply().
 *
 * WHAT: straight_channel = round(premult_channel * 255 / a). When a == 0
 * all channels are defined as 0 (fully transparent; no colour data to recover).
 * The result is clamped to [0, 255] to guard against accumulated rounding.
 *
 * See: docs/design/COLOR.md, section "Alpha Premultiplication Policy".
 */
#[must_use]
pub fn unpremultiply(r: u8, g: u8, b: u8, a: u8) -> (u8, u8, u8) {
    if a == 0 {
        return (0, 0, 0);
    }
    let alpha = u32::from(a);
    let unpremult = |c: u8| -> u8 {
        // round(c * 255 / a), clamped to [0, 255]
        let numerator = u32::from(c) * 255 + (alpha / 2);
        (numerator / alpha).min(255) as u8
    };
    (unpremult(r), unpremult(g), unpremult(b))
}

// ============================================================================
// Parallel tile rasterization (behind "parallel" feature flag)
// ============================================================================

/*
 * rasterize_parallel_into -- tile-parallel rasterization into a caller-owned buffer.
 *
 * WHY: rasterize_parallel() allocates a 4MB Vec on every call (~1ms cold,
 * ~115us warm). For interactive rendering the allocation dominates each frame.
 * rasterize_parallel_into() accepts &mut Vec<u8>, resizes only when dimensions
 * change, and reuses the allocation across frames.
 *
 * For a 1280x800 viewport: first call allocates 4MB; subsequent calls with
 * the same dimensions skip the alloc and go straight to the fill (~115us).
 * At 60fps this saves 60 * ~900us = ~54ms/s of allocator pressure.
 *
 * INVARIANT: caller must pass the SAME buf across frames. Passing a fresh
 * empty Vec each call degrades to rasterize_parallel performance.
 *
 * See: rasterize_parallel (below) for the owned-output one-shot variant.
 * See: gororoba soa_solver.rs:280 for the reuse-buffer pattern.
 */
#[cfg(feature = "parallel")]
// Wrapper to make a raw pointer Send/Sync inside the parallel raster loop.
// SAFETY: per-tile writes target disjoint regions, so concurrent writers
// never alias the same byte; see rasterize_parallel_into for the partition.
struct SendPtr(*mut u8, usize);

// SAFETY: the rayon closure partitions the buffer into disjoint tiles, so no
// two workers write the same byte.
#[cfg(feature = "parallel")]
unsafe impl Send for SendPtr {}

// SAFETY: the shared pointer-length pair is immutable inside the parallel
// closure. Tile partitioning controls mutation through the raw pointer.
#[cfg(feature = "parallel")]
unsafe impl Sync for SendPtr {}

#[cfg(feature = "parallel")]
pub fn rasterize_parallel_into(
    display_list: &DisplayList,
    width: u32,
    height: u32,
    tile_size: u32,
    buf: &mut Vec<u8>,
) {
    use rayon::prelude::*;

    let required = (width * height * 4) as usize;
    if buf.len() != required {
        buf.resize(required, 255u8);
    }
    // Reset to white background -- LLVM auto-vectorizes this to AVX2 fill
    buf.fill(255u8);

    let tile_size = tile_size.max(1);
    let tiles_x = (width + tile_size - 1) / tile_size;
    let tiles_y = (height + tile_size - 1) / tile_size;
    let total_tiles = (tiles_x * tiles_y) as usize;

    // SAFETY: We use a raw pointer to allow parallel writes to disjoint regions.
    // Each tile writes to a unique rectangular region of the buffer.
    // The SendPtr wrapper (see gororoba pattern) makes this safe to send across threads.
    let buf_ptr = buf.as_mut_ptr();
    let buf_len = buf.len();

    let shared = &SendPtr(buf_ptr, buf_len);

    (0..total_tiles).into_par_iter().for_each(|tile_idx| {
        let tx = (tile_idx % tiles_x as usize) as u32;
        let ty = (tile_idx / tiles_x as usize) as u32;

        let tile_x0 = tx * tile_size;
        let tile_y0 = ty * tile_size;
        let tile_x1 = (tile_x0 + tile_size).min(width);
        let tile_y1 = (tile_y0 + tile_size).min(height);

        let tile_rect = Rect {
            x: tile_x0 as f32,
            y: tile_y0 as f32,
            width: (tile_x1 - tile_x0) as f32,
            height: (tile_y1 - tile_y0) as f32,
        };

        // Get items for this tile
        let items = if let Some(tiles) = &display_list.tiles {
            tiles.items_for_rect(tile_rect)
        } else {
            (0..display_list.items.len()).collect()
        };

        // Rasterize items into the tile's region of the shared buffer
        for idx in items {
            if idx >= display_list.items.len() {
                continue;
            }
            let item = &display_list.items[idx];
            let item_r = item_rect(item);
            if !rect_intersects(item_r, tile_rect) {
                continue;
            }
            // Extract a solid fill color for each item type. BoxShadow uses
            // shadow.color; LinearGradient uses its first stop as a scalar
            // fallback (the tiny-skia path renders the full gradient).
            let fill_color: Color = match item {
                DisplayItem::SolidColor { color, .. }
                | DisplayItem::Text { color, .. }
                | DisplayItem::RoundedRect { color, .. } => *color,
                DisplayItem::BoxShadow { shadow, .. } => shadow.color,
                DisplayItem::LinearGradient { stops, .. } => stops.first().map_or(
                    Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 0,
                    },
                    |pair| pair.1,
                ),
                DisplayItem::Image { .. } => continue,
            };

            // Clip to tile bounds
            let x0 = (item_r.x.max(tile_x0 as f32).floor() as i32).max(0);
            let y0 = (item_r.y.max(tile_y0 as f32).floor() as i32).max(0);
            let x1 =
                ((item_r.x + item_r.width).min(tile_x1 as f32).ceil() as i32).min(width as i32);
            let y1 =
                ((item_r.y + item_r.height).min(tile_y1 as f32).ceil() as i32).min(height as i32);

            if x0 >= x1 || y0 >= y1 {
                continue;
            }

            let pixel_bytes = [fill_color.r, fill_color.g, fill_color.b, fill_color.a];
            let width_u = width as usize;

            for y in y0..y1 {
                let row_offset = (y as usize * width_u + x0 as usize) * 4;
                let row_len = ((x1 - x0) as usize) * 4;
                if row_offset + row_len <= shared.1 {
                    // SAFETY: disjoint tile regions guarantee no data race
                    unsafe {
                        let row = std::slice::from_raw_parts_mut(shared.0.add(row_offset), row_len);
                        for pixel in row.chunks_exact_mut(4) {
                            pixel.copy_from_slice(&pixel_bytes);
                        }
                    }
                }
            }
        }
    });
}

/*
 * rasterize_parallel -- tile-based parallel rasterization, owned output.
 *
 * WHY: Convenience wrapper around rasterize_parallel_into for callers that
 * need a fresh owned Vec (e.g. one-shot renders, tests, CLI tools).
 * Interactive renderers should use rasterize_parallel_into with a reused
 * buffer to eliminate the per-frame 4MB allocation cost (~900us cold).
 *
 * Architecture (inspired by gororoba LBM z-slice parallelism):
 *   1. Divide viewport into NxM tiles (default 64x64 pixels each)
 *   2. For each tile: collect display items that overlap it
 *   3. Rasterize each tile independently via rayon::par_iter
 *   4. Each rayon worker writes to disjoint buffer rows -- no sync needed
 *
 * See: rasterize_parallel_into (above) for the buffer-reuse variant.
 * See: gororoba_app/crates/gororoba_bevy_lbm/src/soa_solver.rs:1254
 */
#[cfg(feature = "parallel")]
#[must_use]
pub fn rasterize_parallel(
    display_list: &DisplayList,
    width: u32,
    height: u32,
    tile_size: u32,
) -> Vec<u8> {
    let mut buffer = Vec::new();
    rasterize_parallel_into(display_list, width, height, tile_size, &mut buffer);
    buffer
}

// ============================================================================
// tiny-skia anti-aliased rasterization path
// ============================================================================

/// Rasterize a display list into a new RGBA8 buffer using tiny-skia.
///
/// Output pixels are premultiplied RGBA8, matching `PixmapMut` conventions.
/// For fully-opaque colors (alpha == 255) this is identical to straight RGBA.
#[must_use]
pub fn rasterize_skia(display_list: &DisplayList, width: u32, height: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    rasterize_skia_into(display_list, width, height, &mut buf);
    buf
}

/// Rasterize a display list into a caller-owned buffer using tiny-skia.
///
/// Resizes `buf` only when `width * height * 4` does not match the current
/// length, then resets to an opaque white background before painting.
pub fn rasterize_skia_into(display_list: &DisplayList, width: u32, height: u32, buf: &mut Vec<u8>) {
    let trace_full = std::env::var_os("SILKSURF_TRACE_RENDER_FULL").is_some();
    let total_start = std::time::Instant::now();
    let required = (width * height * 4) as usize;
    let resize_start = std::time::Instant::now();
    if buf.len() != required {
        buf.resize(required, 0xffu8);
    }
    trace_render_full_phase(trace_full, "resize", resize_start.elapsed());
    // White, fully-opaque background. In premultiplied RGBA8 this is
    // [255, 255, 255, 255], identical to straight RGBA for alpha = 255.
    let fill_start = std::time::Instant::now();
    buf.fill(0xffu8);
    trace_render_full_phase(trace_full, "fill", fill_start.elapsed());

    debug_assert_eq!(
        buf.len(),
        required,
        "skia buffer length mismatch after resize"
    );

    let pixmap_start = std::time::Instant::now();
    let slice = buf.as_mut_slice();
    let Some(mut pixmap) = PixmapMut::from_bytes(slice, width, height) else {
        return;
    };
    trace_render_full_phase(trace_full, "pixmap", pixmap_start.elapsed());

    let paint_start = std::time::Instant::now();
    for item in &display_list.items {
        paint_skia_item(&mut pixmap, item, (0.0, 0.0));
    }
    trace_render_full_phase(trace_full, "paint", paint_start.elapsed());
    trace_render_full_total(
        trace_full,
        display_list.items.len(),
        width,
        height,
        total_start.elapsed(),
    );
}

/// Rasterize only `damage` into a retained RGBA8 buffer using tiny-skia.
///
/// The dirty rectangle is rendered into `scratch` with item coordinates
/// translated by the negative damage origin. The scratch rectangle then copies
/// into `buf`, so pixels outside `damage` stay untouched.
pub fn rasterize_skia_damage_into(
    display_list: &DisplayList,
    width: u32,
    height: u32,
    damage: Rect,
    buf: &mut Vec<u8>,
    scratch: &mut DamageScratch,
) {
    scratch.last_damage = None;
    let required = (width * height * 4) as usize;
    if buf.len() != required {
        buf.resize(required, 0xffu8);
    }

    let Some(damage_pixels) = damage_pixel_rect(damage, width, height) else {
        return;
    };
    let scratch_required = damage_pixels.width as usize * damage_pixels.height as usize * 4;
    if scratch.pixels.len() != scratch_required {
        scratch.pixels.resize(scratch_required, 0xffu8);
    }
    scratch.pixels.fill(0xffu8);

    let Some(mut pixmap) = PixmapMut::from_bytes(
        scratch.pixels.as_mut_slice(),
        damage_pixels.width,
        damage_pixels.height,
    ) else {
        return;
    };

    collect_damage_item_indices(display_list, damage, &mut scratch.item_indices);
    prepare_seen_items(&mut scratch.seen_items, display_list.items.len());
    let offset = (-(damage_pixels.x as f32), -(damage_pixels.y as f32));
    for index in scratch.item_indices.iter().copied() {
        if index >= display_list.items.len() || scratch.seen_items[index] {
            continue;
        }
        scratch.seen_items[index] = true;
        let item = &display_list.items[index];
        if rect_intersects(item_rect(item), damage) {
            paint_skia_item(&mut pixmap, item, offset);
        }
    }

    copy_damage_scratch_to_buffer(buf, width, damage_pixels, &scratch.pixels);
    scratch.last_damage = Some(damage_pixels);
}

/// Rasterize translated display-list damage into a retained RGBA8 buffer.
///
/// `buffer_damage` names the destination pixels. `item_damage` names the
/// same damage in display-list coordinates. `paint_offset` maps display-list
/// coordinates into destination buffer coordinates before clipping to damage.
#[allow(clippy::too_many_arguments)]
pub fn rasterize_skia_translated_damage_into(
    display_list: &DisplayList,
    width: u32,
    height: u32,
    buffer_damage: Rect,
    item_damage: Rect,
    paint_offset: (f32, f32),
    buf: &mut Vec<u8>,
    scratch: &mut DamageScratch,
) {
    let trace_damage = std::env::var_os("SILKSURF_TRACE_RENDER_DAMAGE").is_some();
    let total_start = std::time::Instant::now();
    rasterize_skia_translated_damage_scratch_impl(
        display_list,
        width,
        height,
        buffer_damage,
        item_damage,
        paint_offset,
        scratch,
        trace_damage,
    );
    let Some(damage_pixels) = scratch.last_damage else {
        return;
    };
    let required = (width * height * 4) as usize;
    if buf.len() != required {
        buf.resize(required, 0xffu8);
    }
    let copy_start = std::time::Instant::now();
    copy_damage_scratch_to_buffer(buf, width, damage_pixels, &scratch.pixels);
    trace_damage_phase(trace_damage, "translated-copy", copy_start.elapsed());
    if trace_damage {
        eprintln!(
            "[SilkSurf] render damage: translated-total {:?}, damage={}x{} at ({}, {})",
            total_start.elapsed(),
            damage_pixels.width,
            damage_pixels.height,
            damage_pixels.x,
            damage_pixels.y
        );
    }
}

/// Rasterize translated display-list damage into reusable scratch pixels.
#[allow(clippy::too_many_arguments)]
pub fn rasterize_skia_translated_damage_scratch(
    display_list: &DisplayList,
    width: u32,
    height: u32,
    buffer_damage: Rect,
    item_damage: Rect,
    paint_offset: (f32, f32),
    scratch: &mut DamageScratch,
) {
    rasterize_skia_translated_damage_scratch_impl(
        display_list,
        width,
        height,
        buffer_damage,
        item_damage,
        paint_offset,
        scratch,
        std::env::var_os("SILKSURF_TRACE_RENDER_DAMAGE").is_some(),
    );
}

#[allow(clippy::too_many_arguments)]
fn rasterize_skia_translated_damage_scratch_impl(
    display_list: &DisplayList,
    width: u32,
    height: u32,
    buffer_damage: Rect,
    item_damage: Rect,
    paint_offset: (f32, f32),
    scratch: &mut DamageScratch,
    trace_damage: bool,
) {
    let total_start = std::time::Instant::now();
    scratch.last_damage = None;
    let setup_start = std::time::Instant::now();
    let Some(damage_pixels) = damage_pixel_rect(buffer_damage, width, height) else {
        return;
    };
    let scratch_required = damage_pixels.width as usize * damage_pixels.height as usize * 4;
    if scratch.pixels.len() != scratch_required {
        scratch.pixels.resize(scratch_required, 0xffu8);
    }
    trace_damage_phase(trace_damage, "translated-setup", setup_start.elapsed());

    let fill_start = std::time::Instant::now();
    scratch.pixels.fill(0xffu8);
    trace_damage_phase(trace_damage, "translated-fill", fill_start.elapsed());

    let pixmap_start = std::time::Instant::now();
    let Some(mut pixmap) = PixmapMut::from_bytes(
        scratch.pixels.as_mut_slice(),
        damage_pixels.width,
        damage_pixels.height,
    ) else {
        return;
    };
    trace_damage_phase(trace_damage, "translated-pixmap", pixmap_start.elapsed());

    let collect_start = std::time::Instant::now();
    collect_damage_item_indices(display_list, item_damage, &mut scratch.item_indices);
    prepare_seen_items(&mut scratch.seen_items, display_list.items.len());
    trace_damage_phase(trace_damage, "translated-collect", collect_start.elapsed());

    let paint_start = std::time::Instant::now();
    let offset = (
        paint_offset.0 - damage_pixels.x as f32,
        paint_offset.1 - damage_pixels.y as f32,
    );
    let mut painted_items = 0usize;
    for index in scratch.item_indices.iter().copied() {
        if index >= display_list.items.len() || scratch.seen_items[index] {
            continue;
        }
        scratch.seen_items[index] = true;
        let item = &display_list.items[index];
        if rect_intersects(shift_rect(item_rect(item), paint_offset), buffer_damage) {
            painted_items += 1;
            let item_start = std::time::Instant::now();
            paint_skia_item(&mut pixmap, item, offset);
            trace_damage_item(trace_damage, index, item, item_start.elapsed());
        }
    }
    trace_damage_phase(trace_damage, "translated-paint", paint_start.elapsed());

    scratch.last_damage = Some(damage_pixels);
    if trace_damage {
        eprintln!(
            "[SilkSurf] render damage: translated-scratch {:?}, damage={}x{} at ({}, {}), candidates={}, painted={painted_items}",
            total_start.elapsed(),
            damage_pixels.width,
            damage_pixels.height,
            damage_pixels.x,
            damage_pixels.y,
            scratch.item_indices.len()
        );
    }
}

fn trace_damage_phase(enabled: bool, name: &str, elapsed: std::time::Duration) {
    if enabled {
        eprintln!("[SilkSurf] render damage: {name} {elapsed:?}");
    }
}

fn trace_render_full_phase(enabled: bool, name: &str, elapsed: std::time::Duration) {
    if enabled {
        eprintln!("[SilkSurf] render full: {name} {elapsed:?}");
    }
}

fn trace_render_full_total(
    enabled: bool,
    item_count: usize,
    width: u32,
    height: u32,
    elapsed: std::time::Duration,
) {
    if enabled {
        eprintln!(
            "[SilkSurf] render full: total {elapsed:?}, size={}x{}, items={item_count}",
            width, height
        );
    }
}

fn trace_damage_item(
    enabled: bool,
    index: usize,
    item: &DisplayItem,
    elapsed: std::time::Duration,
) {
    if !enabled || elapsed < std::time::Duration::from_micros(100) {
        return;
    }
    let kind = match item {
        DisplayItem::SolidColor { .. } => "solid",
        DisplayItem::Text { .. } => "text",
        DisplayItem::RoundedRect { .. } => "rounded-rect",
        DisplayItem::BoxShadow { .. } => "box-shadow",
        DisplayItem::LinearGradient { .. } => "linear-gradient",
        DisplayItem::Image { .. } => "image",
    };
    let text_len = match item {
        DisplayItem::Text { text, .. } => text.len(),
        _ => 0,
    };
    let ascii = match item {
        DisplayItem::Text { text, .. } => text.is_ascii(),
        _ => true,
    };
    let font_size = match item {
        DisplayItem::Text { font_size, .. } => *font_size,
        _ => 0.0,
    };
    eprintln!(
        "[SilkSurf] render damage item: index={index} kind={kind} text_len={text_len} ascii={ascii} font_size={font_size} elapsed={elapsed:?}"
    );
}

fn collect_damage_item_indices(
    display_list: &DisplayList,
    damage: Rect,
    item_indices: &mut Vec<usize>,
) {
    if let Some(tiles) = &display_list.tiles {
        tiles.items_for_rect_into(damage, item_indices);
    } else {
        item_indices.clear();
        item_indices.extend(0..display_list.items.len());
    }
}

fn prepare_seen_items(seen_items: &mut Vec<bool>, item_count: usize) {
    if seen_items.len() == item_count {
        seen_items.fill(false);
    } else {
        seen_items.resize(item_count, false);
    }
}

fn paint_skia_item(pixmap: &mut PixmapMut<'_>, item: &DisplayItem, offset: (f32, f32)) {
    match item {
        DisplayItem::SolidColor { rect, color } => {
            let rect = shift_rect(*rect, offset);
            let Some(sk_r) = sk_rect(rect) else {
                return;
            };
            let paint = sk_paint(*color);
            pixmap.fill_rect(sk_r, &paint, Transform::identity(), None);
        }
        DisplayItem::RoundedRect { rect, radii, color } => {
            let rect = shift_rect(*rect, offset);
            let Some(path) = rounded_rect_path(rect, *radii) else {
                return;
            };
            let paint = sk_paint(*color);
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        DisplayItem::Text {
            rect,
            text,
            font_size,
            color,
            ..
        } => {
            silksurf_text::rasterize_glyphs(
                text,
                *font_size,
                *color,
                pixmap,
                (rect.x + offset.0, rect.y + offset.1),
            );
        }
        DisplayItem::BoxShadow { rect, shadow } => {
            if shadow.inset {
                return;
            }
            let shadow_rect = box_shadow_rect(shift_rect(*rect, offset), shadow);
            let Some(sk_r) = sk_rect(shadow_rect) else {
                return;
            };
            let paint = sk_paint(shadow.color);
            pixmap.fill_rect(sk_r, &paint, Transform::identity(), None);
        }
        DisplayItem::LinearGradient { rect, angle, stops } => {
            if stops.is_empty() {
                return;
            }
            let rect = shift_rect(*rect, offset);
            let Some(sk_r) = sk_rect(rect) else {
                return;
            };
            let (start, end) = gradient_endpoints(rect, *angle);
            let grad_stops: Vec<GradientStop> = stops
                .iter()
                .map(|(pos, color)| {
                    GradientStop::new(
                        *pos,
                        tiny_skia::Color::from_rgba8(color.r, color.g, color.b, color.a),
                    )
                })
                .collect();
            let Some(gradient) = LinearGradient::new(
                start,
                end,
                grad_stops,
                SpreadMode::Pad,
                Transform::identity(),
            ) else {
                return;
            };
            let paint = Paint {
                anti_alias: true,
                shader: gradient,
                ..Paint::default()
            };
            pixmap.fill_rect(sk_r, &paint, Transform::identity(), None);
        }
        DisplayItem::Image { rect, image } => {
            let rect = shift_rect(*rect, offset);
            let (width, height) = (pixmap.width(), pixmap.height());
            blit_image_nearest(pixmap.data_mut(), width, height, rect, image);
        }
    }
}

fn blit_image_nearest(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    rect: Rect,
    image: &ImageSurface,
) {
    if !image_has_full_rgba(image) {
        return;
    }
    let Some(dst) = image_dest_rect(width, height, rect) else {
        return;
    };
    let dst_width = rect.width.max(1.0);
    let dst_height = rect.height.max(1.0);
    let surface_width = image.width as usize;
    for y in dst.y..dst.y + dst.height {
        let src_y = image_source_coord(y as f32 - rect.y, dst_height, image.height);
        for x in dst.x..dst.x + dst.width {
            let src_x = image_source_coord(x as f32 - rect.x, dst_width, image.width);
            let src = (src_y as usize * surface_width + src_x as usize) * 4;
            let dst = (y as usize * width as usize + x as usize) * 4;
            copy_image_pixel(buffer, dst, &image.rgba, src);
        }
    }
}

fn image_has_full_rgba(image: &ImageSurface) -> bool {
    image.width > 0
        && image.height > 0
        && image.rgba.len() >= image.width as usize * image.height as usize * 4
}

fn image_dest_rect(width: u32, height: u32, rect: Rect) -> Option<DamagePixelRect> {
    if width == 0 || height == 0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let x0 = rect.x.max(0.0).floor() as u32;
    let y0 = rect.y.max(0.0).floor() as u32;
    let x1 = (rect.x + rect.width).clamp(0.0, width as f32).ceil() as u32;
    let y1 = (rect.y + rect.height).clamp(0.0, height as f32).ceil() as u32;
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    Some(DamagePixelRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

fn image_source_coord(dst_offset: f32, dst_extent: f32, src_extent: u32) -> u32 {
    let coord = (dst_offset.max(0.0) * src_extent as f32 / dst_extent).floor() as u32;
    coord.min(src_extent.saturating_sub(1))
}

fn copy_image_pixel(dst: &mut [u8], dst_offset: usize, src: &[u8], src_offset: usize) {
    if dst_offset + 4 > dst.len() || src_offset + 4 > src.len() {
        return;
    }
    let alpha = u16::from(src[src_offset + 3]);
    if alpha == 255 {
        dst[dst_offset..dst_offset + 4].copy_from_slice(&src[src_offset..src_offset + 4]);
        return;
    }
    blend_image_pixel(dst, dst_offset, src, src_offset, alpha);
}

fn blend_image_pixel(dst: &mut [u8], dst_offset: usize, src: &[u8], src_offset: usize, alpha: u16) {
    let inv_alpha = 255 - alpha;
    for channel in 0..3 {
        let src_value = u16::from(src[src_offset + channel]);
        let dst_value = u16::from(dst[dst_offset + channel]);
        dst[dst_offset + channel] = ((src_value * alpha + dst_value * inv_alpha + 127) / 255) as u8;
    }
    dst[dst_offset + 3] = 255;
}

fn shift_rect(rect: Rect, offset: (f32, f32)) -> Rect {
    Rect {
        x: rect.x + offset.0,
        y: rect.y + offset.1,
        width: rect.width,
        height: rect.height,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamagePixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

fn damage_pixel_rect(rect: Rect, width: u32, height: u32) -> Option<DamagePixelRect> {
    if width == 0 || height == 0 {
        return None;
    }
    let x0 = rect.x.max(0.0).floor() as i32;
    let y0 = rect.y.max(0.0).floor() as i32;
    let x1 = (rect.x + rect.width).min(width as f32).ceil() as i32;
    let y1 = (rect.y + rect.height).min(height as f32).ceil() as i32;
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    Some(DamagePixelRect {
        x: x0 as u32,
        y: y0 as u32,
        width: (x1 - x0) as u32,
        height: (y1 - y0) as u32,
    })
}

fn copy_damage_scratch_to_buffer(
    buf: &mut [u8],
    width: u32,
    damage: DamagePixelRect,
    scratch: &[u8],
) {
    let frame_stride = width as usize * 4;
    let scratch_stride = damage.width as usize * 4;
    for row in 0..damage.height as usize {
        let src_start = row * scratch_stride;
        let src_end = src_start + scratch_stride;
        let dst_start = ((damage.y as usize + row) * frame_stride) + damage.x as usize * 4;
        let dst_end = dst_start + scratch_stride;
        if src_end <= scratch.len() && dst_end <= buf.len() {
            buf[dst_start..dst_end].copy_from_slice(&scratch[src_start..src_end]);
        }
    }
}

/// Convert a layout `Rect` to a tiny-skia `Rect`. Returns `None` for
/// degenerate rects (zero or negative dimension).
fn sk_rect(rect: Rect) -> Option<tiny_skia::Rect> {
    tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height)
}

/// Build a tiny-skia `Paint` from a `Color`. Anti-aliasing is always on.
fn sk_paint(color: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
    paint.anti_alias = true;
    paint
}

/// Construct an anti-aliased rounded rectangle path using cubic bezier arcs.
///
/// `radii` is `[top-left, top-right, bottom-right, bottom-left]` in CSS
/// clockwise order. Each radius is clamped to half the shorter dimension to
/// prevent arc overlap. Returns `None` only if the `PathBuilder` produces an
/// empty path (unreachable for non-degenerate input).
///
/// Bezier approximation: Kappa = 4/3 * tan(pi/8) ~= 0.5522847498 gives the
/// control-point offset for a quarter-circle of any radius.
fn rounded_rect_path(rect: Rect, radii: [f32; 4]) -> Option<tiny_skia::Path> {
    // Kappa: cubic bezier control-point factor for quarter-circle approximation.
    const K: f32 = 0.552_284_8;

    let x = rect.x;
    let y = rect.y;
    let w = rect.width;
    let h = rect.height;

    // Clamp every radius so no two adjacent arcs overlap.
    let max_r = (w * 0.5).min(h * 0.5).max(0.0);
    let [r_tl, r_tr, r_br, r_bl] = radii.map(|r| r.min(max_r).max(0.0));

    let mut pb = PathBuilder::new();

    // Begin at the end of the top-left arc, travel clockwise.
    pb.move_to(x + r_tl, y);

    // Top edge -> top-right arc
    pb.line_to(x + w - r_tr, y);
    pb.cubic_to(
        x + w - r_tr * (1.0 - K),
        y,
        x + w,
        y + r_tr * (1.0 - K),
        x + w,
        y + r_tr,
    );

    // Right edge -> bottom-right arc
    pb.line_to(x + w, y + h - r_br);
    pb.cubic_to(
        x + w,
        y + h - r_br * (1.0 - K),
        x + w - r_br * (1.0 - K),
        y + h,
        x + w - r_br,
        y + h,
    );

    // Bottom edge -> bottom-left arc
    pb.line_to(x + r_bl, y + h);
    pb.cubic_to(
        x + r_bl * (1.0 - K),
        y + h,
        x,
        y + h - r_bl * (1.0 - K),
        x,
        y + h - r_bl,
    );

    // Left edge -> top-left arc
    pb.line_to(x, y + r_tl);
    pb.cubic_to(
        x,
        y + r_tl * (1.0 - K),
        x + r_tl * (1.0 - K),
        y,
        x + r_tl,
        y,
    );

    pb.close();
    pb.finish()
}

/// Compute the shadow's fill rect from the element rect and CSS shadow params.
///
/// Expands the element rect by `spread_radius` on all sides, then offsets by
/// `(offset_x, offset_y)`. Blur is not applied here; the scalar paths use this
/// rect for a solid fill fallback. `rasterize_skia_into` will add blur in a
/// future pass. Used by both `item_rect` (for tile culling) and the scalar
/// rasterization paths.
fn box_shadow_rect(element_rect: Rect, shadow: &CssBoxShadow) -> Rect {
    let spread = shadow.spread_radius;
    Rect {
        x: element_rect.x + shadow.offset_x - spread,
        y: element_rect.y + shadow.offset_y - spread,
        width: element_rect.width + 2.0 * spread,
        height: element_rect.height + 2.0 * spread,
    }
}

/// Compute gradient line endpoints for a CSS linear-gradient angle.
///
/// CSS angle convention: 0.0 = to top, 90.0 = to right, 180.0 = to bottom.
/// The gradient line passes through the center of `rect`. Its half-length is
/// `|half_w * sin(theta)| + |half_h * cos(theta)|`, which projects the box
/// corners onto the gradient direction (CSS spec section 6.1).
fn gradient_endpoints(rect: Rect, angle_deg: f32) -> (Point, Point) {
    let cx = rect.x + rect.width * 0.5;
    let cy = rect.y + rect.height * 0.5;
    let theta = angle_deg.to_radians();
    let dx = theta.sin();
    let dy = -theta.cos();
    let half_len = (rect.width * 0.5 * dx.abs()) + (rect.height * 0.5 * dy.abs());
    let start = Point::from_xy(cx - dx * half_len, cy - dy * half_len);
    let end = Point::from_xy(cx + dx * half_len, cy + dy * half_len);
    (start, end)
}
