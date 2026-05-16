/*
 * render/lib.rs -- display list construction and tile-based rasterization.
 *
 * WHY: Final stage of the rendering pipeline. Converts positioned layout
 * boxes into a flat DisplayList of paint commands (SolidColor, Text),
 * then rasterizes them to an RGBA pixel buffer.
 *
 * Architecture:
 *   build_display_list: layout tree -> Vec<DisplayItem> (depth-first walk)
 *   with_tiles: partition display items into spatial tile buckets
 *   rasterize_damage: paint only tiles intersecting the damage region
 *   fill_rect: per-row pixel fill with SSE2 SIMD (4 pixels/store)
 *
 * Tile-based rendering: viewport divided into 64x64 tiles. Each tile
 * has a bucket of display item indices. Damage rasterization only
 * processes tiles that overlap the dirty rectangle.
 *
 * SIMD: fill_row_sse2 (x86/x86_64) uses _mm_set1_epi32 + _mm_storeu_si128
 * to fill 4 pixels per instruction.  fill_row_neon (aarch64) mirrors this
 * with vdupq_n_u32 + vst1q_u32.  Both fall back to scalar .fill() when the
 * feature is absent (sse2 on x86; NEON is mandatory on aarch64 so the
 * fallback is unreachable in practice, but the code compiles cleanly).
 *
 * DONE(perf): Rayon tile parallelism (Phase 4.6) -- rasterize_parallel{,_into}
 * DONE(perf): Buffer reuse -- rasterize_parallel_into eliminates per-frame alloc
 * TODO(perf): SoA DisplayList for type-batched rasterization
 *
 * Memory: RGBA buffer = width * height * 4 bytes (4MB for 1280x800)
 *
 * See: layout/lib.rs for LayoutTree input
 * See: style.rs ComputedStyle for color/background data
 */
#![allow(
    clippy::collapsible_if,
    clippy::needless_borrow,
    clippy::manual_div_ceil
)]

use rustc_hash::FxHashMap;
use silksurf_css::{Color, ComputedStyle};
use silksurf_dom::{Dom, NodeId, NodeKind};
use silksurf_layout::{LayoutTree, Rect};
use tiny_skia::{FillRule, Paint, PathBuilder, PixmapMut, Transform};

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
        /// time. Required by cosmic-text shaping in rasterize_skia_into.
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
}

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
                if style.background_color.a > 0 {
                    list.items.push(DisplayItem::SolidColor {
                        rect: layout.dimensions().content,
                        color: style.background_color,
                    });
                }
                if let Ok(node) = dom.node(node_id) {
                    if let NodeKind::Text { .. } = node.kind() {
                        let (text_len, text_content) = match node.kind() {
                            NodeKind::Text { text } => (text.len() as u32, text.to_string()),
                            _ => (0, String::new()),
                        };
                        let font_size_px = match style.font_size {
                            silksurf_css::Length::Px(px) => px,
                            _ => 16.0,
                        };
                        list.items.push(DisplayItem::Text {
                            rect: layout.dimensions().content,
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

pub fn rasterize(display_list: &DisplayList, width: u32, height: u32) -> Vec<u8> {
    let damage = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    rasterize_damage(display_list, width, height, damage)
}

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
        }
    }
    buffer
}

fn item_rect(item: &DisplayItem) -> Rect {
    match item {
        DisplayItem::SolidColor { rect, .. } => *rect,
        DisplayItem::Text { rect, .. } => *rect,
        DisplayItem::RoundedRect { rect, .. } => *rect,
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
    fn items_for_rect(&self, rect: Rect) -> Vec<usize> {
        let x0 = rect.x.max(0.0).floor() as i32;
        let y0 = rect.y.max(0.0).floor() as i32;
        let x1 = (rect.x + rect.width).max(0.0).ceil() as i32;
        let y1 = (rect.y + rect.height).max(0.0).ceil() as i32;
        let tx0 = (x0.max(0) as u32) / self.tile_size;
        let ty0 = (y0.max(0) as u32) / self.tile_size;
        let tx1 = ((x1.max(1) as u32).saturating_sub(1)) / self.tile_size;
        let ty1 = ((y1.max(1) as u32).saturating_sub(1)) / self.tile_size;
        let mut items = Vec::new();
        for ty in ty0..=ty1.min(self.tiles_y.saturating_sub(1)) {
            for tx in tx0..=tx1.min(self.tiles_x.saturating_sub(1)) {
                let tile_index = (ty * self.tiles_x + tx) as usize;
                if let Some(bucket) = self.buckets.get(tile_index) {
                    items.extend(bucket.iter().copied());
                }
            }
        }
        items
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
    let buffer_u32 =
        unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u32, len_u32) };

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
    use std::arch::x86_64::*;

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
        let dst = unsafe { ptr.add(idx) } as *mut __m128i;
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
pub fn srgb_to_linear(c: u8) -> f32 {
    let c_f = c as f32 / 255.0;
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
pub fn linear_to_srgb(c: f32) -> u8 {
    let c_clamped = c.clamp(0.0, 1.0);
    let encoded = if c_clamped <= 0.0031308 {
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
pub fn premultiply(r: u8, g: u8, b: u8, a: u8) -> (u8, u8, u8) {
    let alpha = a as u32;
    let premult = |c: u8| -> u8 {
        let ca = c as u32 * alpha + 127;
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
pub fn unpremultiply(r: u8, g: u8, b: u8, a: u8) -> (u8, u8, u8) {
    if a == 0 {
        return (0, 0, 0);
    }
    let alpha = a as u32;
    let unpremult = |c: u8| -> u8 {
        // round(c * 255 / a), clamped to [0, 255]
        let numerator = c as u32 * 255 + (alpha / 2);
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

    // Wrapper to make raw pointer Send (safe because writes are disjoint)
    struct SendPtr(*mut u8, usize);
    unsafe impl Send for SendPtr {}
    unsafe impl Sync for SendPtr {}

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
            let color = match item {
                DisplayItem::SolidColor { color, .. } => color,
                DisplayItem::Text { color, .. } => color,
                DisplayItem::RoundedRect { color, .. } => color,
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

            let pixel_bytes = [color.r, color.g, color.b, color.a];
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
pub fn rasterize_skia(display_list: &DisplayList, width: u32, height: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    rasterize_skia_into(display_list, width, height, &mut buf);
    buf
}

/// Rasterize a display list into a caller-owned buffer using tiny-skia.
///
/// Resizes `buf` only when `width * height * 4` does not match the current
/// length, then resets to an opaque white background before painting.
pub fn rasterize_skia_into(
    display_list: &DisplayList,
    width: u32,
    height: u32,
    buf: &mut Vec<u8>,
) {
    let required = (width * height * 4) as usize;
    if buf.len() != required {
        buf.resize(required, 0xffu8);
    }
    // White, fully-opaque background. In premultiplied RGBA8 this is
    // [255, 255, 255, 255], identical to straight RGBA for alpha = 255.
    buf.fill(0xffu8);

    debug_assert_eq!(
        buf.len(),
        required,
        "skia buffer length mismatch after resize"
    );

    let slice = buf.as_mut_slice();
    let Some(mut pixmap) = PixmapMut::from_bytes(slice, width, height) else {
        return;
    };

    for item in &display_list.items {
        match item {
            DisplayItem::SolidColor { rect, color } => {
                let Some(sk_r) = sk_rect(*rect) else {
                    continue;
                };
                let paint = sk_paint(*color);
                pixmap.fill_rect(sk_r, &paint, Transform::identity(), None);
            }
            DisplayItem::RoundedRect { rect, radii, color } => {
                let Some(path) = rounded_rect_path(*rect, *radii) else {
                    continue;
                };
                let paint = sk_paint(*color);
                pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
            }
            DisplayItem::Text { rect, text, font_size, color, .. } => {
                silksurf_text::rasterize_glyphs(
                    text,
                    *font_size,
                    *color,
                    &mut pixmap,
                    (rect.x, rect.y),
                );
            }
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
    const K: f32 = 0.5522847498;

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
