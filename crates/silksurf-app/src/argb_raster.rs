// Raster and chrome-drawing functions take explicit pixel geometry
// (buffer, stride, x, y, width, height, color) as separate parameters;
// the SIMD packers reinterpret &[u8] pixel rows as &[u32] words, with
// alignment guaranteed because every destination buffer is a Vec<u32>.
#![allow(clippy::too_many_arguments, clippy::cast_ptr_alignment)]

// Module split from the former single-file binary; the crate root
// re-exports every module so sibling items resolve by bare name.
#[allow(clippy::wildcard_imports)]
use crate::*;

pub(crate) fn rgba_bytes_to_argb_words_into(rgba: &[u8], argb: &mut Vec<u32>) {
    let _ = rgba_bytes_to_argb_words_into_timed(rgba, argb);
}

pub(crate) fn rgba_bytes_to_argb_words_into_timed(
    rgba: &[u8],
    argb: &mut Vec<u32>,
) -> (std::time::Duration, std::time::Duration) {
    let resize_start = std::time::Instant::now();
    resize_argb_words_uninit(argb, rgba.len() / 4);
    let resize_elapsed = resize_start.elapsed();

    let pack_start = std::time::Instant::now();
    pack_rgba_bytes_to_argb_words(rgba, argb);
    (resize_elapsed, pack_start.elapsed())
}

pub(crate) fn resize_argb_words_uninit(argb: &mut Vec<u32>, target_len: usize) {
    if target_len <= argb.len() {
        argb.truncate(target_len);
        return;
    }
    if argb.capacity() < target_len {
        argb.reserve_exact(target_len - argb.len());
    }
    /*
     * SAFETY: each caller overwrites every exposed word before any framebuffer
     * read. u32 has no destructor, so an early panic only releases the
     * allocation.
     */
    unsafe {
        argb.set_len(target_len);
    }
}

pub(crate) fn pack_rgba_bytes_to_argb_words(rgba: &[u8], argb: &mut [u32]) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("avx2") {
            // SAFETY: the runtime feature gate above proves AVX2 support.
            let packed = unsafe { pack_rgba_bytes_to_argb_words_avx2(rgba, argb) };
            pack_rgba_bytes_to_argb_words_scalar(&rgba[packed * 4..], &mut argb[packed..]);
            return;
        }
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("sse2") {
            if std::is_x86_feature_detected!("ssse3") {
                // SAFETY: the runtime feature gate above proves SSSE3 support.
                let packed = unsafe { pack_rgba_bytes_to_argb_words_ssse3(rgba, argb) };
                pack_rgba_bytes_to_argb_words_scalar(&rgba[packed * 4..], &mut argb[packed..]);
                return;
            }
            // SAFETY: the runtime feature gate above proves SSE2 support.
            let packed = unsafe { pack_rgba_bytes_to_argb_words_sse2(rgba, argb) };
            pack_rgba_bytes_to_argb_words_scalar(&rgba[packed * 4..], &mut argb[packed..]);
            return;
        }
    }

    pack_rgba_bytes_to_argb_words_scalar(rgba, argb);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn pack_rgba_bytes_to_argb_words_avx2(rgba: &[u8], argb: &mut [u32]) -> usize {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{
        __m256i, _mm256_loadu_si256, _mm256_setr_epi8, _mm256_shuffle_epi8, _mm256_storeu_si256,
    };
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{
        __m256i, _mm256_loadu_si256, _mm256_setr_epi8, _mm256_shuffle_epi8, _mm256_storeu_si256,
    };

    let pixels = argb.len().min(rgba.len() / 4);
    let lanes = pixels / 8;
    let shuffle_mask = _mm256_setr_epi8(
        2, 1, 0, 3, 6, 5, 4, 7, 10, 9, 8, 11, 14, 13, 12, 15, 2, 1, 0, 3, 6, 5, 4, 7, 10, 9, 8, 11,
        14, 13, 12, 15,
    );

    for lane in 0..lanes {
        let rgba_offset = lane * 32;
        let argb_offset = lane * 8;
        // SAFETY: lane counts keep byte and word offsets inside both slices.
        let source = unsafe { rgba.as_ptr().add(rgba_offset).cast::<__m256i>() };
        // SAFETY: lane counts keep byte and word offsets inside both slices.
        let dest = unsafe { argb.as_mut_ptr().add(argb_offset).cast::<__m256i>() };
        // SAFETY: AVX2 unaligned load reads one complete 32-byte lane.
        let raw = unsafe { _mm256_loadu_si256(source) };
        let argb_words = _mm256_shuffle_epi8(raw, shuffle_mask);
        // SAFETY: AVX2 unaligned store writes one complete 8-word lane.
        unsafe {
            _mm256_storeu_si256(dest, argb_words);
        }
    }

    lanes * 8
}

pub(crate) fn pack_rgba_bytes_to_argb_words_scalar(rgba: &[u8], argb: &mut [u32]) {
    for (dst, px) in argb.iter_mut().zip(rgba.chunks_exact(4)) {
        *dst = argb_word_from_rgba(px);
    }
}

pub(crate) fn argb_word_from_rgba(px: &[u8]) -> u32 {
    (u32::from(px[3]) << 24) | (u32::from(px[0]) << 16) | (u32::from(px[1]) << 8) | u32::from(px[2])
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "ssse3")]
pub(crate) unsafe fn pack_rgba_bytes_to_argb_words_ssse3(rgba: &[u8], argb: &mut [u32]) -> usize {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{
        __m128i, _mm_loadu_si128, _mm_setr_epi8, _mm_shuffle_epi8, _mm_storeu_si128,
    };
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{
        __m128i, _mm_loadu_si128, _mm_setr_epi8, _mm_shuffle_epi8, _mm_storeu_si128,
    };

    let pixels = argb.len().min(rgba.len() / 4);
    let lanes = pixels / 4;
    let shuffle_mask = _mm_setr_epi8(2, 1, 0, 3, 6, 5, 4, 7, 10, 9, 8, 11, 14, 13, 12, 15);

    for lane in 0..lanes {
        let rgba_offset = lane * 16;
        let argb_offset = lane * 4;
        // SAFETY: lane counts keep byte and word offsets inside both slices.
        let source = unsafe { rgba.as_ptr().add(rgba_offset).cast::<__m128i>() };
        // SAFETY: lane counts keep byte and word offsets inside both slices.
        let dest = unsafe { argb.as_mut_ptr().add(argb_offset).cast::<__m128i>() };
        // SAFETY: SSSE3 unaligned load reads one complete 16-byte lane.
        let raw = unsafe { _mm_loadu_si128(source) };
        let argb_words = _mm_shuffle_epi8(raw, shuffle_mask);
        // SAFETY: SSSE3 unaligned store writes one complete 4-word lane.
        unsafe {
            _mm_storeu_si128(dest, argb_words);
        }
    }

    lanes * 4
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn pack_rgba_bytes_to_argb_words_sse2(rgba: &[u8], argb: &mut [u32]) -> usize {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{
        __m128i, _mm_and_si128, _mm_loadu_si128, _mm_or_si128, _mm_set1_epi32, _mm_slli_epi32,
        _mm_srli_epi32, _mm_storeu_si128,
    };
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{
        __m128i, _mm_and_si128, _mm_loadu_si128, _mm_or_si128, _mm_set1_epi32, _mm_slli_epi32,
        _mm_srli_epi32, _mm_storeu_si128,
    };

    let pixels = argb.len().min(rgba.len() / 4);
    let lanes = pixels / 4;
    let red_blue_mask = _mm_set1_epi32(0x00ff_00ff);
    let green_alpha_mask = _mm_set1_epi32(0xff00_ff00u32 as i32);

    for lane in 0..lanes {
        let rgba_offset = lane * 16;
        let argb_offset = lane * 4;
        // SAFETY: lane counts keep byte and word offsets inside both slices.
        let source = unsafe { rgba.as_ptr().add(rgba_offset).cast::<__m128i>() };
        // SAFETY: lane counts keep byte and word offsets inside both slices.
        let dest = unsafe { argb.as_mut_ptr().add(argb_offset).cast::<__m128i>() };
        // SAFETY: SSE2 unaligned load reads one complete 16-byte lane.
        let raw = unsafe { _mm_loadu_si128(source) };
        let red_blue = _mm_and_si128(raw, red_blue_mask);
        let green_alpha = _mm_and_si128(raw, green_alpha_mask);
        let red = _mm_slli_epi32(red_blue, 16);
        let blue = _mm_srli_epi32(red_blue, 16);
        let argb_words = _mm_or_si128(green_alpha, _mm_or_si128(red, blue));
        // SAFETY: SSE2 unaligned store writes one complete 4-word lane.
        unsafe {
            _mm_storeu_si128(dest, argb_words);
        }
    }

    lanes * 4
}

pub(crate) fn sync_argb_damage_from_rgba(
    rgba: &[u8],
    argb: &mut [u32],
    width: u32,
    height: u32,
    damage: Rect,
) {
    let x0 = damage.x.floor().max(0.0).min(width as f32) as u32;
    let x1 = (damage.x + damage.width).ceil().max(0.0).min(width as f32) as u32;
    let y0 = damage.y.floor().max(0.0).min(height as f32) as u32;
    let y1 = (damage.y + damage.height)
        .ceil()
        .max(0.0)
        .min(height as f32) as u32;
    if x1 <= x0 || y1 <= y0 {
        return;
    }

    let width_usize = width as usize;
    for y in y0 as usize..y1 as usize {
        let row_start = y * width_usize;
        let argb_start = row_start + x0 as usize;
        let argb_end = row_start + x1 as usize;
        let rgba_start = argb_start * 4;
        let rgba_end = argb_end * 4;
        if argb_end > argb.len() || rgba_end > rgba.len() {
            return;
        }
        let argb_row = &mut argb[argb_start..argb_end];
        let rgba_row = &rgba[rgba_start..rgba_end];
        pack_rgba_bytes_to_argb_words(rgba_row, argb_row);
    }
}

pub(crate) fn sync_argb_damage_from_scratch(
    scratch: &silksurf_render::DamageScratch,
    argb: &mut [u32],
    frame_width: u32,
) -> bool {
    let Some(damage) = scratch.last_damage() else {
        return false;
    };
    let frame_width = frame_width as usize;
    let damage_x = damage.x as usize;
    let damage_y = damage.y as usize;
    let damage_width = damage.width as usize;
    let damage_height = damage.height as usize;
    if frame_width == 0 || damage_width == 0 || damage_height == 0 {
        return false;
    }
    let scratch_stride = damage_width * 4;
    let scratch_pixels = scratch.pixels();
    if scratch_pixels.len() < scratch_stride * damage_height {
        return false;
    }

    for row in 0..damage_height {
        let argb_start = (damage_y + row) * frame_width + damage_x;
        let argb_end = argb_start + damage_width;
        let scratch_start = row * scratch_stride;
        let scratch_end = scratch_start + scratch_stride;
        if argb_end > argb.len() || scratch_end > scratch_pixels.len() {
            return false;
        }
        let argb_row = &mut argb[argb_start..argb_end];
        let scratch_row = &scratch_pixels[scratch_start..scratch_end];
        pack_rgba_bytes_to_argb_words(scratch_row, argb_row);
    }
    true
}

pub(crate) fn browser_frame_height(
    items: &[silksurf_render::DisplayItem],
    minimum_height: u32,
) -> u32 {
    let content_bottom = items
        .iter()
        .map(display_item_bottom)
        .fold(minimum_height as f32, f32::max);
    content_bottom.ceil().max(minimum_height as f32) as u32
}

pub(crate) fn tile_browser_document_display_list(
    display_list: silksurf_render::DisplayList,
    document_height: u32,
) -> silksurf_render::DisplayList {
    display_list.with_tiles(FRAME_WIDTH, document_height, DOCUMENT_TILE_SIZE)
}

pub(crate) fn rasterize_browser_viewport_into(
    display_list: &silksurf_render::DisplayList,
    scroll_y: u32,
    bitmap_height: u32,
    rgba: &mut Vec<u8>,
    item_indices: &mut Vec<usize>,
) {
    let viewport_list =
        browser_viewport_display_list(display_list, scroll_y, bitmap_height, item_indices);
    silksurf_render::rasterize_skia_into(&viewport_list, FRAME_WIDTH, bitmap_height, rgba);
    fill_browser_toolbar_background_rgba(rgba, FRAME_WIDTH, bitmap_height);
}

pub(crate) fn rasterize_browser_viewport_argb_preferred(
    display_list: &silksurf_render::DisplayList,
    scroll_y: u32,
    bitmap_height: u32,
    rgba: &mut Vec<u8>,
    argb: &mut Vec<u32>,
    item_indices: &mut Vec<usize>,
) -> bool {
    if rasterize_browser_viewport_argb_direct(
        display_list,
        scroll_y,
        bitmap_height,
        argb,
        item_indices,
    ) {
        return true;
    }
    rasterize_browser_viewport_into(display_list, scroll_y, bitmap_height, rgba, item_indices);
    rgba_bytes_to_argb_words_into(rgba, argb);
    false
}

#[cfg(test)]
pub(crate) fn first_prepared_focus_target(
    dom: &silksurf_dom::Dom,
    input_targets: &[InputTarget],
) -> Option<InputTarget> {
    input_targets
        .iter()
        .find(|target| is_text_editable_input_node(dom, target.node))
        .or_else(|| input_targets.first())
        .cloned()
}

pub(crate) fn render_focus_viewport_cache(
    display_list: &silksurf_render::DisplayList,
    scroll_y: u32,
    bitmap_height: u32,
) -> FocusViewportCache {
    let mut rgba = Vec::new();
    let mut argb = Vec::new();
    let mut viewport_item_indices = Vec::new();
    rasterize_browser_viewport_argb_preferred(
        display_list,
        scroll_y,
        bitmap_height,
        &mut rgba,
        &mut argb,
        &mut viewport_item_indices,
    );
    FocusViewportCache {
        scroll_y,
        bitmap_height,
        argb,
    }
}

#[cfg(test)]
pub(crate) fn first_focus_target_scroll(
    input_targets: &[InputTarget],
    document_height: u32,
    bitmap_height: u32,
    chrome_height: u32,
) -> Option<u32> {
    let target = input_targets.first()?;
    focus_target_scroll(target, document_height, bitmap_height, chrome_height)
}

#[cfg(test)]
pub(crate) fn focus_target_scroll(
    target: &InputTarget,
    document_height: u32,
    bitmap_height: u32,
    chrome_height: u32,
) -> Option<u32> {
    let max_scroll = max_browser_scroll_offset(document_height, bitmap_height, chrome_height);
    let scroll =
        scroll_to_show_input_target(0.0, target.rect, max_scroll, chrome_height, bitmap_height);
    (scroll >= 0.5).then_some(scroll.round() as u32)
}

pub(crate) fn rasterize_browser_document_damage_into(
    display_list: &silksurf_render::DisplayList,
    scroll_y: u32,
    bitmap_height: u32,
    damage: Rect,
    rgba: &mut Vec<u8>,
    scratch: &mut silksurf_render::DamageScratch,
) {
    silksurf_render::rasterize_skia_translated_damage_into(
        display_list,
        FRAME_WIDTH,
        bitmap_height,
        viewport_damage_rect(damage, scroll_y),
        damage,
        (0.0, -(scroll_y as f32)),
        rgba,
        scratch,
    );
}

pub(crate) fn rasterize_browser_document_damage_scratch(
    display_list: &silksurf_render::DisplayList,
    scroll_y: u32,
    bitmap_height: u32,
    damage: Rect,
    scratch: &mut silksurf_render::DamageScratch,
) {
    silksurf_render::rasterize_skia_translated_damage_scratch(
        display_list,
        FRAME_WIDTH,
        bitmap_height,
        viewport_damage_rect(damage, scroll_y),
        damage,
        (0.0, -(scroll_y as f32)),
        scratch,
    );
}

pub(crate) fn trace_visible_document_raster(
    display_list: &silksurf_render::DisplayList,
    damage: Rect,
) {
    if std::env::var_os("SILKSURF_TRACE_APP_FRAME").is_none() {
        return;
    }
    let mut item_count = 0usize;
    let mut text_count = 0usize;
    let mut text_bytes = 0usize;
    let mut item_indices = Vec::new();
    browser_viewport_source_item_indices(display_list, damage, &mut item_indices);
    for item in display_items_for_indices(display_list, &item_indices) {
        if !display_item_intersects_viewport(item, damage) {
            continue;
        }
        item_count += 1;
        if let silksurf_render::DisplayItem::Text { text, .. } = item {
            text_count += 1;
            text_bytes += text.len();
        }
    }
    eprintln!(
        "[SilkSurf] visible document raster: total_items={} items={item_count} text_items={text_count} text_bytes={text_bytes} rect=({}, {}, {}, {})",
        display_list.items.len(),
        damage.x,
        damage.y,
        damage.width,
        damage.height
    );
}

pub(crate) fn fill_browser_toolbar_background_rgba(rgba: &mut [u8], width: u32, height: u32) {
    let toolbar_rows = (BROWSER_CHROME_HEIGHT as u32).min(height);
    if width == 0 || toolbar_rows == 0 {
        return;
    }
    let row_bytes = width as usize * 4;
    let toolbar_bytes = toolbar_rows as usize * row_bytes;
    if rgba.len() < toolbar_bytes {
        return;
    }
    for pixel in rgba[..toolbar_bytes].chunks_exact_mut(4) {
        pixel.copy_from_slice(&[243, 244, 246, 255]);
    }
    let separator_start = (toolbar_rows as usize - 1) * row_bytes;
    let separator_end = separator_start + row_bytes;
    for pixel in rgba[separator_start..separator_end].chunks_exact_mut(4) {
        pixel.copy_from_slice(&[209, 213, 219, 255]);
    }
}

pub(crate) fn fill_browser_toolbar_background_argb(pixels: &mut [u32], width: u32, height: u32) {
    let toolbar_rows = (BROWSER_CHROME_HEIGHT as u32).min(height);
    if width == 0 || toolbar_rows == 0 {
        return;
    }
    fill_argb_rect(
        pixels,
        width,
        height,
        0,
        0,
        width,
        toolbar_rows,
        argb(243, 244, 246, 255),
    );
    fill_argb_rect(
        pixels,
        width,
        height,
        0,
        toolbar_rows - 1,
        width,
        1,
        argb(209, 213, 219, 255),
    );
}

pub(crate) fn rasterize_browser_viewport_argb_direct(
    display_list: &silksurf_render::DisplayList,
    scroll_y: u32,
    bitmap_height: u32,
    pixels: &mut Vec<u32>,
    item_indices: &mut Vec<usize>,
) -> bool {
    let trace_argb = trace_argb_direct_enabled();
    let source_start = std::time::Instant::now();
    let viewport = scroll_visible_document_rect(scroll_y, bitmap_height);
    browser_viewport_source_item_indices(display_list, viewport, item_indices);
    let source_elapsed = source_start.elapsed();
    let support_start = std::time::Instant::now();
    let supported = viewport_argb_direct_items_supported(display_list, item_indices, viewport);
    let support_elapsed = support_start.elapsed();
    if !supported {
        trace_viewport_argb_direct_miss(display_list, item_indices, viewport);
        return false;
    }
    let resize_start = std::time::Instant::now();
    resize_argb_words_uninit(pixels, FRAME_WIDTH as usize * bitmap_height as usize);
    let resize_elapsed = resize_start.elapsed();
    let fill_start = std::time::Instant::now();
    let default_filled =
        viewport_argb_direct_needs_default_fill(display_list, item_indices, viewport);
    if default_filled {
        fill_argb_words(pixels, argb(255, 255, 255, 255));
    }
    let fill_elapsed = fill_start.elapsed();
    let toolbar_start = std::time::Instant::now();
    fill_browser_toolbar_background_argb(pixels, FRAME_WIDTH, bitmap_height);
    let toolbar_elapsed = toolbar_start.elapsed();
    let paint_start = std::time::Instant::now();
    let mut painted_items = 0usize;
    for item in display_items_for_indices(display_list, item_indices) {
        if display_item_intersects_viewport(item, viewport) {
            painted_items += 1;
            paint_viewport_argb_direct_item(pixels, bitmap_height, item, scroll_y);
        }
    }
    trace_argb_direct_phases(
        trace_argb,
        item_indices.len(),
        painted_items,
        default_filled,
        source_elapsed,
        support_elapsed,
        resize_elapsed,
        fill_elapsed,
        toolbar_elapsed,
        paint_start.elapsed(),
    );
    true
}

pub(crate) fn trace_argb_direct_enabled() -> bool {
    std::env::var_os("SILKSURF_TRACE_RENDER_FULL").is_some()
        || std::env::var_os("SILKSURF_TRACE_NAV_BUILD").is_some()
}

pub(crate) fn trace_argb_direct_phases(
    enabled: bool,
    source_items: usize,
    painted_items: usize,
    default_filled: bool,
    source_elapsed: std::time::Duration,
    support_elapsed: std::time::Duration,
    resize_elapsed: std::time::Duration,
    fill_elapsed: std::time::Duration,
    toolbar_elapsed: std::time::Duration,
    paint_elapsed: std::time::Duration,
) {
    if enabled {
        eprintln!(
            "[SilkSurf] argb-direct phases: source_items={source_items} painted_items={painted_items} default_fill={default_filled} source={source_elapsed:?} support={support_elapsed:?} resize={resize_elapsed:?} fill={fill_elapsed:?} toolbar={toolbar_elapsed:?} paint={paint_elapsed:?}"
        );
    }
}

pub(crate) fn rasterize_browser_document_damage_argb_direct(
    display_list: &silksurf_render::DisplayList,
    scroll_y: u32,
    bitmap_height: u32,
    damage: Rect,
    pixels: &mut [u32],
    item_indices: &mut Vec<usize>,
) -> bool {
    let viewport_damage = viewport_damage_rect(damage, scroll_y);
    let Some(clip) = pixel_rect_from_rect(viewport_damage, FRAME_WIDTH, bitmap_height) else {
        return true;
    };
    browser_viewport_source_item_indices(display_list, damage, item_indices);
    if !viewport_argb_direct_items_supported(display_list, item_indices, damage) {
        trace_viewport_argb_direct_miss(display_list, item_indices, damage);
        return false;
    }
    if viewport_argb_direct_needs_default_fill(display_list, item_indices, damage) {
        fill_argb_rect(
            pixels,
            FRAME_WIDTH,
            bitmap_height,
            clip.x,
            clip.y,
            clip.width,
            clip.height,
            argb(255, 255, 255, 255),
        );
    }
    for item in display_items_for_indices(display_list, item_indices) {
        if display_item_intersects_viewport(item, damage) {
            paint_viewport_argb_direct_item_clipped(pixels, bitmap_height, item, scroll_y, clip);
        }
    }
    true
}

pub(crate) fn viewport_argb_direct_needs_default_fill(
    display_list: &silksurf_render::DisplayList,
    item_indices: &[usize],
    viewport: Rect,
) -> bool {
    for item in display_items_for_indices(display_list, item_indices) {
        if !display_item_intersects_viewport(item, viewport) {
            continue;
        }
        return !opaque_fill_covers_rect(item, viewport);
    }
    true
}

pub(crate) fn opaque_fill_covers_rect(item: &silksurf_render::DisplayItem, rect: Rect) -> bool {
    match item {
        silksurf_render::DisplayItem::SolidColor {
            rect: item_rect,
            color,
        } => color.a == 255 && rect_contains_rect(*item_rect, rect),
        silksurf_render::DisplayItem::RoundedRect {
            rect: item_rect,
            radii,
            color,
        } => {
            color.a == 255
                && radii.iter().all(|radius| *radius <= 0.0)
                && rect_contains_rect(*item_rect, rect)
        }
        _ => false,
    }
}

pub(crate) fn viewport_argb_direct_items_supported(
    display_list: &silksurf_render::DisplayList,
    item_indices: &[usize],
    viewport: Rect,
) -> bool {
    display_items_for_indices(display_list, item_indices)
        .filter(|item| display_item_intersects_viewport(item, viewport))
        .all(viewport_argb_direct_item_supported)
}

pub(crate) fn viewport_argb_direct_item_supported(item: &silksurf_render::DisplayItem) -> bool {
    match item {
        silksurf_render::DisplayItem::SolidColor { color, .. } => color.a == 255,
        silksurf_render::DisplayItem::Text {
            text,
            font_size,
            color,
            ..
        } => color.a == 255 && page_bitmap_text_supported(text, *font_size),
        silksurf_render::DisplayItem::RoundedRect { radii, color, .. } => {
            color.a == 255 && radii.iter().all(|radius| *radius <= 0.0)
        }
        silksurf_render::DisplayItem::Image { image, .. } => image_has_full_rgba_argb(image),
        silksurf_render::DisplayItem::BoxShadow { .. }
        | silksurf_render::DisplayItem::LinearGradient { .. } => false,
    }
}

pub(crate) fn trace_viewport_argb_direct_miss(
    display_list: &silksurf_render::DisplayList,
    item_indices: &[usize],
    viewport: Rect,
) {
    if std::env::var_os("SILKSURF_TRACE_APP_FRAME").is_none()
        && std::env::var_os("SILKSURF_TRACE_NAV_BUILD").is_none()
    {
        return;
    }
    let mut unsupported_text = 0usize;
    let mut unsupported_rounding = 0usize;
    let mut unsupported_alpha = 0usize;
    let mut unsupported_shadow = 0usize;
    let mut unsupported_gradient = 0usize;
    let mut unsupported_image = 0usize;
    let mut visible = 0usize;
    for item in display_items_for_indices(display_list, item_indices) {
        if !display_item_intersects_viewport(item, viewport) {
            continue;
        }
        visible += 1;
        match item {
            silksurf_render::DisplayItem::SolidColor { color, .. } if color.a != 255 => {
                unsupported_alpha += 1;
            }
            silksurf_render::DisplayItem::Text {
                text,
                font_size,
                color,
                ..
            } if color.a != 255 || !page_bitmap_text_supported(text, *font_size) => {
                unsupported_text += 1;
            }
            silksurf_render::DisplayItem::RoundedRect { radii, color, .. } => {
                if color.a != 255 {
                    unsupported_alpha += 1;
                } else if radii.iter().any(|radius| *radius > 0.0) {
                    unsupported_rounding += 1;
                }
            }
            silksurf_render::DisplayItem::Image { image, .. }
                if !image_has_full_rgba_argb(image) =>
            {
                unsupported_image += 1;
            }
            silksurf_render::DisplayItem::BoxShadow { .. } => {
                unsupported_shadow += 1;
            }
            silksurf_render::DisplayItem::LinearGradient { .. } => {
                unsupported_gradient += 1;
            }
            _ => {}
        }
    }
    eprintln!(
        "[SilkSurf] argb direct miss: visible_items={visible} text={unsupported_text} rounding={unsupported_rounding} alpha={unsupported_alpha} shadow={unsupported_shadow} gradient={unsupported_gradient} image={unsupported_image}"
    );
}

pub(crate) fn paint_viewport_argb_direct_item(
    pixels: &mut [u32],
    bitmap_height: u32,
    item: &silksurf_render::DisplayItem,
    scroll_y: u32,
) {
    match item {
        silksurf_render::DisplayItem::SolidColor { rect, color }
        | silksurf_render::DisplayItem::RoundedRect { rect, color, .. } => {
            fill_shifted_argb_rect(
                pixels,
                bitmap_height,
                *rect,
                scroll_y,
                css_color_to_argb(*color),
            );
        }
        silksurf_render::DisplayItem::Text {
            rect,
            text,
            font_size,
            color,
            ..
        } => {
            draw_shifted_argb_text(
                pixels,
                bitmap_height,
                *rect,
                scroll_y,
                text,
                *font_size,
                *color,
            );
        }
        silksurf_render::DisplayItem::Image { rect, image } => {
            blit_shifted_argb_image(pixels, bitmap_height, *rect, scroll_y, image);
        }
        silksurf_render::DisplayItem::BoxShadow { .. }
        | silksurf_render::DisplayItem::LinearGradient { .. } => {}
    }
}

pub(crate) fn paint_viewport_argb_direct_item_clipped(
    pixels: &mut [u32],
    bitmap_height: u32,
    item: &silksurf_render::DisplayItem,
    scroll_y: u32,
    clip: PixelRect,
) {
    match item {
        silksurf_render::DisplayItem::SolidColor { rect, color }
        | silksurf_render::DisplayItem::RoundedRect { rect, color, .. } => {
            fill_shifted_argb_rect_clipped(
                pixels,
                bitmap_height,
                *rect,
                scroll_y,
                css_color_to_argb(*color),
                clip,
            );
        }
        silksurf_render::DisplayItem::Text {
            rect,
            text,
            font_size,
            color,
            ..
        } => {
            draw_shifted_argb_text_clipped(
                pixels,
                bitmap_height,
                *rect,
                scroll_y,
                text,
                *font_size,
                *color,
                clip,
            );
        }
        silksurf_render::DisplayItem::Image { rect, image } => {
            blit_shifted_argb_image_clipped(pixels, bitmap_height, *rect, scroll_y, image, clip);
        }
        silksurf_render::DisplayItem::BoxShadow { .. }
        | silksurf_render::DisplayItem::LinearGradient { .. } => {}
    }
}

pub(crate) fn fill_shifted_argb_rect(
    pixels: &mut [u32],
    bitmap_height: u32,
    rect: Rect,
    scroll_y: u32,
    color: u32,
) {
    let shifted = viewport_damage_rect(rect, scroll_y);
    if let Some(pixel_rect) = pixel_rect_from_rect(shifted, FRAME_WIDTH, bitmap_height) {
        fill_argb_rect(
            pixels,
            FRAME_WIDTH,
            bitmap_height,
            pixel_rect.x,
            pixel_rect.y,
            pixel_rect.width,
            pixel_rect.height,
            color,
        );
    }
}

pub(crate) fn fill_shifted_argb_rect_clipped(
    pixels: &mut [u32],
    bitmap_height: u32,
    rect: Rect,
    scroll_y: u32,
    color: u32,
    clip: PixelRect,
) {
    let shifted = viewport_damage_rect(rect, scroll_y);
    let Some(pixel_rect) = pixel_rect_from_rect(shifted, FRAME_WIDTH, bitmap_height) else {
        return;
    };
    let Some(pixel_rect) = pixel_rect_intersection(pixel_rect, clip) else {
        return;
    };
    fill_argb_rect(
        pixels,
        FRAME_WIDTH,
        bitmap_height,
        pixel_rect.x,
        pixel_rect.y,
        pixel_rect.width,
        pixel_rect.height,
        color,
    );
}

pub(crate) fn draw_shifted_argb_text(
    pixels: &mut [u32],
    bitmap_height: u32,
    rect: Rect,
    scroll_y: u32,
    text: &str,
    font_size: f32,
    color: silksurf_css::Color,
) {
    let shifted = viewport_damage_rect(rect, scroll_y);
    if let Some(pixel_rect) = pixel_rect_from_rect(shifted, FRAME_WIDTH, bitmap_height) {
        let _ = draw_page_bitmap_text_clipped(
            pixels,
            FRAME_WIDTH,
            bitmap_height,
            shifted.x,
            shifted.y,
            text,
            font_size,
            css_color_to_argb(color),
            pixel_rect,
        );
    }
}

pub(crate) fn draw_shifted_argb_text_clipped(
    pixels: &mut [u32],
    bitmap_height: u32,
    rect: Rect,
    scroll_y: u32,
    text: &str,
    font_size: f32,
    color: silksurf_css::Color,
    clip: PixelRect,
) {
    let shifted = viewport_damage_rect(rect, scroll_y);
    let _ = draw_page_bitmap_text_clipped(
        pixels,
        FRAME_WIDTH,
        bitmap_height,
        shifted.x,
        shifted.y,
        text,
        font_size,
        css_color_to_argb(color),
        clip,
    );
}

pub(crate) fn blit_shifted_argb_image(
    pixels: &mut [u32],
    bitmap_height: u32,
    rect: Rect,
    scroll_y: u32,
    image: &silksurf_render::ImageSurface,
) {
    let shifted = viewport_damage_rect(rect, scroll_y);
    let Some(dst) = pixel_rect_from_rect(shifted, FRAME_WIDTH, bitmap_height) else {
        return;
    };
    blit_argb_image_rect(pixels, shifted, image, dst);
}

pub(crate) fn blit_shifted_argb_image_clipped(
    pixels: &mut [u32],
    bitmap_height: u32,
    rect: Rect,
    scroll_y: u32,
    image: &silksurf_render::ImageSurface,
    clip: PixelRect,
) {
    let shifted = viewport_damage_rect(rect, scroll_y);
    let Some(dst) = pixel_rect_from_rect(shifted, FRAME_WIDTH, bitmap_height) else {
        return;
    };
    let Some(dst) = pixel_rect_intersection(dst, clip) else {
        return;
    };
    blit_argb_image_rect(pixels, shifted, image, dst);
}

pub(crate) fn blit_argb_image_rect(
    pixels: &mut [u32],
    shifted: Rect,
    image: &silksurf_render::ImageSurface,
    dst: PixelRect,
) {
    let dst_width = shifted.width.max(1.0);
    let dst_height = shifted.height.max(1.0);
    let surface_width = image.width as usize;
    for y in dst.y..dst.y + dst.height {
        let src_y = image_source_coord_argb(y as f32 - shifted.y, dst_height, image.height);
        for x in dst.x..dst.x + dst.width {
            let src_x = image_source_coord_argb(x as f32 - shifted.x, dst_width, image.width);
            let src = (src_y as usize * surface_width + src_x as usize) * 4;
            let dst = y as usize * FRAME_WIDTH as usize + x as usize;
            copy_image_pixel_argb(pixels, dst, &image.rgba, src);
        }
    }
}

pub(crate) fn image_has_full_rgba_argb(image: &silksurf_render::ImageSurface) -> bool {
    image.width > 0
        && image.height > 0
        && image.rgba.len() >= image.width as usize * image.height as usize * 4
}

pub(crate) fn image_source_coord_argb(dst_offset: f32, dst_extent: f32, src_extent: u32) -> u32 {
    let coord = (dst_offset.max(0.0) * src_extent as f32 / dst_extent).floor() as u32;
    coord.min(src_extent.saturating_sub(1))
}

pub(crate) fn copy_image_pixel_argb(pixels: &mut [u32], dst: usize, rgba: &[u8], src: usize) {
    if dst >= pixels.len() || src + 4 > rgba.len() {
        return;
    }
    let alpha = rgba[src + 3];
    if alpha == 255 {
        pixels[dst] = argb(rgba[src], rgba[src + 1], rgba[src + 2], 255);
        return;
    }
    pixels[dst] = blend_image_pixel_argb(pixels[dst], &rgba[src..src + 4]);
}

pub(crate) fn blend_image_pixel_argb(dst: u32, src: &[u8]) -> u32 {
    let alpha = u16::from(src[3]);
    let inv_alpha = 255 - alpha;
    let red = blend_argb_channel(src[0], (dst >> 16) as u8, alpha, inv_alpha);
    let green = blend_argb_channel(src[1], (dst >> 8) as u8, alpha, inv_alpha);
    let blue = blend_argb_channel(src[2], dst as u8, alpha, inv_alpha);
    argb(red, green, blue, 255)
}

pub(crate) fn blend_argb_channel(src: u8, dst: u8, alpha: u16, inv_alpha: u16) -> u8 {
    ((u16::from(src) * alpha + u16::from(dst) * inv_alpha + 127) / 255) as u8
}

pub(crate) fn viewport_damage_rect(damage: Rect, scroll_y: u32) -> Rect {
    Rect {
        x: damage.x,
        y: damage.y - scroll_y as f32,
        width: damage.width,
        height: damage.height,
    }
}

pub(crate) fn refresh_browser_frame_bitmap(
    state: &mut BrowserState,
    scroll_y: u32,
    bitmap_height: u32,
) -> BrowserBitmapRefresh {
    if state.frame.bitmap_scroll_y == scroll_y && state.frame.bitmap_height == bitmap_height {
        return BrowserBitmapRefresh::Clean;
    }
    if let Some(damage) = scroll_browser_frame_bitmap(state, scroll_y, bitmap_height) {
        return BrowserBitmapRefresh::ScrollReuse(damage);
    }
    let Some(runtime) = state.runtime.as_mut() else {
        return BrowserBitmapRefresh::Clean;
    };
    trace_visible_document_raster(
        &runtime.display_list,
        scroll_visible_document_rect(scroll_y, bitmap_height),
    );
    rasterize_browser_viewport_argb_preferred(
        &runtime.display_list,
        scroll_y,
        bitmap_height,
        &mut runtime.rgba,
        &mut state.frame.argb,
        &mut runtime.viewport_item_indices,
    );
    state.frame.bitmap_height = bitmap_height;
    state.frame.bitmap_scroll_y = scroll_y;
    BrowserBitmapRefresh::Full
}

pub(crate) fn scroll_browser_frame_bitmap(
    state: &mut BrowserState,
    scroll_y: u32,
    bitmap_height: u32,
) -> Option<Rect> {
    if state.frame.bitmap_height != bitmap_height || state.runtime.is_none() {
        return None;
    }
    let old_scroll_y = state.frame.bitmap_scroll_y;
    if old_scroll_y == scroll_y {
        return None;
    }
    let chrome_rows = BROWSER_CHROME_HEIGHT as u32;
    let content_rows = bitmap_height.saturating_sub(chrome_rows);
    let scroll_delta = i64::from(scroll_y) - i64::from(old_scroll_y);
    let delta_rows = scroll_delta.unsigned_abs() as u32;
    if !scroll_reuse_is_profitable(content_rows, delta_rows) {
        return None;
    }

    let runtime = state.runtime.as_mut()?;
    if !shift_browser_argb_content_rows(
        &mut state.frame.argb,
        FRAME_WIDTH,
        chrome_rows,
        content_rows,
        scroll_delta,
    ) {
        return None;
    }
    let exposed_damage = scroll_exposed_document_rect(scroll_y, bitmap_height, scroll_delta);
    if !rasterize_browser_document_damage_argb_direct(
        &runtime.display_list,
        scroll_y,
        bitmap_height,
        exposed_damage,
        &mut state.frame.argb,
        &mut runtime.viewport_item_indices,
    ) {
        rasterize_browser_document_damage_into(
            &runtime.display_list,
            scroll_y,
            bitmap_height,
            exposed_damage,
            &mut runtime.rgba,
            &mut runtime.damage_scratch,
        );
        if !sync_argb_damage_from_scratch(
            &runtime.damage_scratch,
            &mut state.frame.argb,
            FRAME_WIDTH,
        ) {
            sync_argb_damage_from_rgba(
                &runtime.rgba,
                &mut state.frame.argb,
                FRAME_WIDTH,
                bitmap_height,
                viewport_damage_rect(exposed_damage, scroll_y),
            );
        }
    }
    state.frame.bitmap_scroll_y = scroll_y;
    Some(exposed_damage)
}

pub(crate) fn scroll_reuse_is_profitable(content_rows: u32, delta_rows: u32) -> bool {
    delta_rows > 0 && delta_rows < content_rows && delta_rows <= content_rows / 4
}

pub(crate) fn scroll_exposed_document_rect(
    scroll_y: u32,
    bitmap_height: u32,
    scroll_delta: i64,
) -> Rect {
    let chrome_rows = BROWSER_CHROME_HEIGHT as u32;
    let content_rows = bitmap_height.saturating_sub(chrome_rows);
    let delta_rows = scroll_delta.unsigned_abs().min(u64::from(content_rows)) as u32;
    let y = if scroll_delta > 0 {
        chrome_rows
            .saturating_add(scroll_y)
            .saturating_add(content_rows.saturating_sub(delta_rows))
    } else {
        chrome_rows.saturating_add(scroll_y)
    };
    Rect {
        x: 0.0,
        y: y as f32,
        width: FRAME_WIDTH as f32,
        height: delta_rows as f32,
    }
}

pub(crate) fn scroll_visible_document_rect(scroll_y: u32, bitmap_height: u32) -> Rect {
    let chrome_rows = BROWSER_CHROME_HEIGHT as u32;
    Rect {
        x: 0.0,
        y: chrome_rows.saturating_add(scroll_y) as f32,
        width: FRAME_WIDTH as f32,
        height: bitmap_height.saturating_sub(chrome_rows) as f32,
    }
}

pub(crate) fn shift_browser_argb_content_rows(
    argb: &mut [u32],
    width: u32,
    chrome_rows: u32,
    content_rows: u32,
    scroll_delta: i64,
) -> bool {
    let delta_rows = scroll_delta.unsigned_abs() as u32;
    if width == 0 || delta_rows == 0 || delta_rows >= content_rows {
        return false;
    }
    let total_rows = chrome_rows.saturating_add(content_rows);
    if argb.len() < total_rows as usize * width as usize {
        return false;
    }
    if scroll_delta > 0 {
        copy_browser_argb_content_rows_up(argb, width, chrome_rows, content_rows, delta_rows);
    } else {
        copy_browser_argb_content_rows_down(argb, width, chrome_rows, content_rows, delta_rows);
    }
    true
}

pub(crate) fn copy_browser_argb_content_rows_up(
    argb: &mut [u32],
    width: u32,
    chrome_rows: u32,
    content_rows: u32,
    delta_rows: u32,
) {
    let preserved_rows = content_rows - delta_rows;
    copy_argb_rows(
        argb,
        width,
        chrome_rows + delta_rows,
        chrome_rows,
        preserved_rows,
    );
}

pub(crate) fn copy_browser_argb_content_rows_down(
    argb: &mut [u32],
    width: u32,
    chrome_rows: u32,
    content_rows: u32,
    delta_rows: u32,
) {
    let preserved_rows = content_rows - delta_rows;
    copy_argb_rows(
        argb,
        width,
        chrome_rows,
        chrome_rows + delta_rows,
        preserved_rows,
    );
}

pub(crate) fn copy_argb_rows(argb: &mut [u32], width: u32, source_y: u32, dest_y: u32, rows: u32) {
    let row_words = width as usize;
    let source_start = source_y as usize * row_words;
    let source_end = source_start + rows as usize * row_words;
    let dest_start = dest_y as usize * row_words;
    argb.copy_within(source_start..source_end, dest_start);
}

pub(crate) fn trace_browser_bitmap_refresh(
    enabled: bool,
    refresh: BrowserBitmapRefresh,
    elapsed: std::time::Duration,
) {
    if enabled && refresh != BrowserBitmapRefresh::Clean {
        eprintln!("[SilkSurf] bitmap refresh: {refresh:?} in {elapsed:?}");
    }
}

pub(crate) fn browser_viewport_display_list(
    display_list: &silksurf_render::DisplayList,
    scroll_y: u32,
    bitmap_height: u32,
    item_indices: &mut Vec<usize>,
) -> silksurf_render::DisplayList {
    let viewport = Rect {
        x: 0.0,
        y: BROWSER_CHROME_HEIGHT + scroll_y as f32,
        width: FRAME_WIDTH as f32,
        height: bitmap_height.saturating_sub(BROWSER_CHROME_HEIGHT as u32) as f32,
    };
    browser_viewport_source_item_indices(display_list, viewport, item_indices);
    let mut items = Vec::with_capacity(display_list.items.len().min(256));
    for item in display_items_for_indices(display_list, item_indices) {
        if !display_item_intersects_viewport(item, viewport) {
            continue;
        }
        items.push(shift_display_item_y(item.clone(), -(scroll_y as f32)));
    }
    silksurf_render::DisplayList { items, tiles: None }
}

pub(crate) fn browser_viewport_source_item_indices(
    display_list: &silksurf_render::DisplayList,
    viewport: Rect,
    item_indices: &mut Vec<usize>,
) {
    let Some(tiles) = &display_list.tiles else {
        item_indices.clear();
        item_indices.extend(0..display_list.items.len());
        return;
    };
    tiles.items_for_rect_into(viewport, item_indices);
    item_indices.sort_unstable();
    item_indices.dedup();
}

pub(crate) fn display_items_for_indices<'a>(
    display_list: &'a silksurf_render::DisplayList,
    item_indices: &'a [usize],
) -> impl Iterator<Item = &'a silksurf_render::DisplayItem> + 'a {
    item_indices
        .iter()
        .filter_map(|index| display_list.items.get(*index))
}

pub(crate) fn display_item_intersects_viewport(
    item: &silksurf_render::DisplayItem,
    viewport: Rect,
) -> bool {
    rects_intersect(display_item_rect(item), viewport)
}

pub(crate) fn display_item_rect(item: &silksurf_render::DisplayItem) -> Rect {
    match item {
        silksurf_render::DisplayItem::SolidColor { rect, .. }
        | silksurf_render::DisplayItem::Text { rect, .. }
        | silksurf_render::DisplayItem::RoundedRect { rect, .. }
        | silksurf_render::DisplayItem::LinearGradient { rect, .. }
        | silksurf_render::DisplayItem::Image { rect, .. } => *rect,
        silksurf_render::DisplayItem::BoxShadow { rect, shadow } => Rect {
            x: rect.x + shadow.offset_x - shadow.spread_radius,
            y: rect.y + shadow.offset_y - shadow.spread_radius,
            width: rect.width + shadow.spread_radius * 2.0,
            height: rect.height + shadow.spread_radius * 2.0,
        },
    }
}

pub(crate) fn rects_intersect(a: Rect, b: Rect) -> bool {
    let ax1 = a.x + a.width;
    let ay1 = a.y + a.height;
    let bx1 = b.x + b.width;
    let by1 = b.y + b.height;
    a.x < bx1 && ax1 > b.x && a.y < by1 && ay1 > b.y
}

pub(crate) fn rect_contains_rect(outer: Rect, inner: Rect) -> bool {
    outer.x <= inner.x
        && outer.y <= inner.y
        && outer.x + outer.width >= inner.x + inner.width
        && outer.y + outer.height >= inner.y + inner.height
}

pub(crate) fn shift_display_item_y(
    mut item: silksurf_render::DisplayItem,
    delta_y: f32,
) -> silksurf_render::DisplayItem {
    match &mut item {
        silksurf_render::DisplayItem::SolidColor { rect, .. }
        | silksurf_render::DisplayItem::Text { rect, .. }
        | silksurf_render::DisplayItem::RoundedRect { rect, .. }
        | silksurf_render::DisplayItem::BoxShadow { rect, .. }
        | silksurf_render::DisplayItem::LinearGradient { rect, .. }
        | silksurf_render::DisplayItem::Image { rect, .. } => {
            rect.y += delta_y;
        }
    }
    item
}

pub(crate) fn initial_browser_window_height(raster_height: u32) -> u32 {
    raster_height.clamp(MIN_INITIAL_WINDOW_HEIGHT, FRAME_HEIGHT)
}

pub(crate) fn window_size_exposes_unpainted_area(
    last_width: u32,
    last_height: u32,
    next_width: u32,
    next_height: u32,
) -> bool {
    last_width == 0 || last_height == 0 || next_width > last_width || next_height > last_height
}

pub(crate) fn display_item_bottom(item: &silksurf_render::DisplayItem) -> f32 {
    match item {
        silksurf_render::DisplayItem::SolidColor { rect, .. }
        | silksurf_render::DisplayItem::Text { rect, .. }
        | silksurf_render::DisplayItem::RoundedRect { rect, .. }
        | silksurf_render::DisplayItem::BoxShadow { rect, .. }
        | silksurf_render::DisplayItem::LinearGradient { rect, .. }
        | silksurf_render::DisplayItem::Image { rect, .. } => rect.y + rect.height,
    }
}

pub(crate) fn max_browser_scroll_offset(
    frame_height: u32,
    window_height: u32,
    chrome_height: u32,
) -> f32 {
    let source_content_height = frame_height.saturating_sub(chrome_height);
    let window_content_height = window_height.saturating_sub(chrome_height);
    source_content_height.saturating_sub(window_content_height) as f32
}

pub(crate) fn scroll_to_show_input_target(
    current_scroll: f32,
    rect: Rect,
    max_scroll: f32,
    chrome_height: u32,
    window_height: u32,
) -> f32 {
    let viewport_top = current_scroll + chrome_height as f32;
    let viewport_bottom = current_scroll + window_height as f32;
    let target_top = rect.y;
    let target_bottom = rect.y + rect.height;
    let padding = 24.0;
    let next_scroll = if target_bottom + padding > viewport_bottom {
        target_bottom + padding - window_height as f32
    } else if target_top < viewport_top + padding {
        target_top - chrome_height as f32 - padding
    } else {
        current_scroll
    };
    clamp_scroll_offset(next_scroll, max_scroll)
}

pub(crate) fn clamp_scroll_offset(scroll: f32, max_scroll: f32) -> f32 {
    if !scroll.is_finite() {
        return 0.0;
    }
    scroll.clamp(0.0, max_scroll.max(0.0))
}

pub(crate) fn blit_browser_frame(
    frame: &[u32],
    frame_width: u32,
    frame_height: u32,
    chrome_height: u32,
    scroll_y: u32,
    window_width: u32,
    window_height: u32,
    pixels: &mut [u32],
) {
    if window_width == 0 || window_height == 0 {
        return;
    }
    let background = 0xFFFF_FFFF;
    if scroll_y == 0 && frame_width == window_width && frame_height == window_height {
        let pixel_count = frame_width.saturating_mul(frame_height) as usize;
        if frame.len() >= pixel_count && pixels.len() >= pixel_count {
            pixels[..pixel_count].copy_from_slice(&frame[..pixel_count]);
            return;
        }
    }

    let copy_width = frame_width.min(window_width);
    let chrome_rows = chrome_height.min(frame_height).min(window_height);
    copy_frame_rows(
        frame,
        frame_width,
        0,
        pixels,
        window_width,
        0,
        copy_width,
        chrome_rows,
    );

    let content_rows = window_height.saturating_sub(chrome_rows);
    let source_y = chrome_height.saturating_add(scroll_y).min(frame_height);
    let available_rows = frame_height.saturating_sub(source_y);
    let rows = content_rows.min(available_rows);
    copy_frame_rows(
        frame,
        frame_width,
        source_y,
        pixels,
        window_width,
        chrome_rows,
        copy_width,
        rows,
    );
    let copied_rows = chrome_rows.saturating_add(rows).min(window_height);
    if copy_width < window_width {
        fill_argb_rect(
            pixels,
            window_width,
            window_height,
            copy_width,
            0,
            window_width - copy_width,
            copied_rows,
            background,
        );
    }
    if copied_rows < window_height {
        fill_argb_rect(
            pixels,
            window_width,
            window_height,
            0,
            copied_rows,
            window_width,
            window_height - copied_rows,
            background,
        );
    }
}

pub(crate) fn blit_browser_frame_damage(
    frame: &[u32],
    frame_width: u32,
    frame_height: u32,
    chrome_height: u32,
    scroll_y: u32,
    window_width: u32,
    window_height: u32,
    damage: Rect,
    pixels: &mut [u32],
) {
    if damage.width <= 0.0 || damage.height <= 0.0 {
        return;
    }

    let x0 = damage.x.floor().max(0.0) as u32;
    let y0 = damage.y.floor().max(0.0) as u32;
    let x1 = (damage.x + damage.width)
        .ceil()
        .max(0.0)
        .min(frame_width as f32)
        .min(window_width as f32) as u32;
    let y1 = (damage.y + damage.height).ceil().max(0.0) as u32;
    if x1 <= x0 || y1 <= y0 {
        return;
    }

    let chrome_rows = chrome_height.min(frame_height).min(window_height);
    if y0 < chrome_rows {
        let chrome_y1 = y1.min(chrome_rows);
        copy_frame_rect(
            frame,
            frame_width,
            x0,
            y0,
            pixels,
            window_width,
            x0,
            y0,
            x1 - x0,
            chrome_y1 - y0,
        );
    }

    let content_rows = window_height.saturating_sub(chrome_rows);
    let visible_source_y0 = chrome_height.saturating_add(scroll_y);
    let visible_source_y1 = visible_source_y0.saturating_add(content_rows);
    let source_y0 = y0.max(chrome_height).max(visible_source_y0);
    let source_y1 = y1.min(visible_source_y1);
    if source_y1 <= source_y0 {
        return;
    }
    let viewport_y = source_y0.saturating_sub(scroll_y);
    copy_frame_rect(
        frame,
        frame_width,
        x0,
        viewport_y,
        pixels,
        window_width,
        x0,
        viewport_y,
        x1 - x0,
        source_y1 - source_y0,
    );
}

pub(crate) fn browser_present_damage(
    redraw_mode: BrowserRedrawMode,
    frame_height: u32,
    chrome_height: u32,
    scroll_y: u32,
    window_width: u32,
    window_height: u32,
) -> silksurf_gui::WinitPresentDamage {
    match redraw_mode {
        BrowserRedrawMode::Clean | BrowserRedrawMode::Scroll => {
            silksurf_gui::WinitPresentDamage::Clean
        }
        BrowserRedrawMode::Full => silksurf_gui::WinitPresentDamage::Full,
        BrowserRedrawMode::AddressChrome | BrowserRedrawMode::AddressFocusChrome => {
            silksurf_gui::WinitPresentDamage::rect(
                ADDRESS_BAR_X,
                ADDRESS_BAR_Y,
                ADDRESS_BAR_WIDTH,
                ADDRESS_BAR_HEIGHT,
            )
        }
        BrowserRedrawMode::AddressFullTextChrome => silksurf_gui::WinitPresentDamage::rect(
            ADDRESS_BAR_X + 10,
            ADDRESS_BAR_Y + 7,
            ADDRESS_BAR_WIDTH - 22,
            ADDRESS_BAR_HEIGHT - 14,
        ),
        BrowserRedrawMode::AddressTextChrome => silksurf_gui::WinitPresentDamage::rect(
            ADDRESS_BAR_X + 10,
            ADDRESS_BAR_Y + 7,
            ADDRESS_BAR_WIDTH - 22,
            ADDRESS_BAR_HEIGHT - 14,
        ),
        BrowserRedrawMode::StatusChrome => {
            browser_status_present_damage(window_width, window_height)
        }
        BrowserRedrawMode::NavigationStartChrome => {
            browser_navigation_start_present_damage(window_width, window_height)
        }
        BrowserRedrawMode::Chrome => {
            silksurf_gui::WinitPresentDamage::rect(0, 0, window_width, chrome_height)
        }
        BrowserRedrawMode::Damage(damage) => browser_content_damage_rect(
            damage,
            frame_height,
            chrome_height,
            scroll_y,
            window_width,
            window_height,
        ),
        BrowserRedrawMode::PageInputFocus(damage) => browser_content_damage_rect(
            damage,
            frame_height,
            chrome_height,
            scroll_y,
            window_width,
            window_height,
        ),
        BrowserRedrawMode::DamageWithChrome(damage) => browser_content_damage_with_chrome_rect(
            damage,
            frame_height,
            chrome_height,
            scroll_y,
            window_width,
            window_height,
        ),
    }
}

pub(crate) fn browser_render_seeds_full_buffer(
    redraw_mode: BrowserRedrawMode,
    buffer_age: u8,
) -> bool {
    buffer_age == 0
        && matches!(
            redraw_mode,
            BrowserRedrawMode::Damage(_)
                | BrowserRedrawMode::PageInputFocus(_)
                | BrowserRedrawMode::DamageWithChrome(_)
                | BrowserRedrawMode::AddressChrome
                | BrowserRedrawMode::AddressFocusChrome
                | BrowserRedrawMode::AddressFullTextChrome
                | BrowserRedrawMode::AddressTextChrome
                | BrowserRedrawMode::NavigationStartChrome
                | BrowserRedrawMode::StatusChrome
                | BrowserRedrawMode::Chrome
        )
}

pub(crate) fn browser_status_present_damage(
    window_width: u32,
    window_height: u32,
) -> silksurf_gui::WinitPresentDamage {
    let Some(rect) = browser_status_text_band_rect(window_width, window_height) else {
        return silksurf_gui::WinitPresentDamage::Clean;
    };
    silksurf_gui::WinitPresentDamage::Rect(rect)
}

pub(crate) fn browser_navigation_start_present_damage(
    window_width: u32,
    window_height: u32,
) -> silksurf_gui::WinitPresentDamage {
    silksurf_gui::WinitPresentDamage::rects(&[
        navigation_button_present_damage_rect(RELOAD_BUTTON_X, window_width, window_height),
        navigation_button_present_damage_rect(STOP_BUTTON_X, window_width, window_height),
        browser_status_text_band_rect(window_width, window_height).unwrap_or_else(zero_damage_rect),
    ])
}

pub(crate) fn zero_damage_rect() -> silksurf_gui::WinitDamageRect {
    silksurf_gui::WinitDamageRect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    }
}

pub(crate) fn navigation_button_present_damage_rect(
    x: u32,
    window_width: u32,
    window_height: u32,
) -> silksurf_gui::WinitDamageRect {
    silksurf_gui::WinitDamageRect {
        x,
        y: NAV_BUTTON_Y,
        width: window_width.saturating_sub(x).min(NAV_BUTTON_WIDTH),
        height: window_height
            .saturating_sub(NAV_BUTTON_Y)
            .min(NAV_BUTTON_HEIGHT),
    }
}

pub(crate) fn browser_content_damage_with_chrome_rect(
    damage: Rect,
    frame_height: u32,
    chrome_height: u32,
    scroll_y: u32,
    window_width: u32,
    window_height: u32,
) -> silksurf_gui::WinitPresentDamage {
    let mut output = match browser_content_damage_rect(
        damage,
        frame_height,
        chrome_height,
        scroll_y,
        window_width,
        window_height,
    ) {
        silksurf_gui::WinitPresentDamage::Clean => None,
        silksurf_gui::WinitPresentDamage::Full => return silksurf_gui::WinitPresentDamage::Full,
        silksurf_gui::WinitPresentDamage::Rect(rect) => Some(rect),
        silksurf_gui::WinitPresentDamage::Rects(rects) => {
            let mut output = None;
            for rect in rects.as_slice() {
                union_present_damage_rect(&mut output, *rect);
            }
            output
        }
    };
    union_present_damage_rect(
        &mut output,
        silksurf_gui::WinitDamageRect {
            x: 0,
            y: 0,
            width: window_width,
            height: chrome_height.min(window_height),
        },
    );
    output.map_or(
        silksurf_gui::WinitPresentDamage::Clean,
        silksurf_gui::WinitPresentDamage::Rect,
    )
}

pub(crate) fn browser_content_damage_rect(
    damage: Rect,
    frame_height: u32,
    chrome_height: u32,
    scroll_y: u32,
    window_width: u32,
    window_height: u32,
) -> silksurf_gui::WinitPresentDamage {
    if damage.width <= 0.0 || damage.height <= 0.0 || window_width == 0 || window_height == 0 {
        return silksurf_gui::WinitPresentDamage::Clean;
    }

    let x0 = damage.x.floor().max(0.0).min(window_width as f32) as u32;
    let x1 = (damage.x + damage.width)
        .ceil()
        .max(0.0)
        .min(window_width as f32) as u32;
    let y0 = damage.y.floor().max(0.0) as u32;
    let y1 = (damage.y + damage.height)
        .ceil()
        .max(0.0)
        .min(frame_height as f32) as u32;
    if x1 <= x0 || y1 <= y0 {
        return silksurf_gui::WinitPresentDamage::Clean;
    }

    let chrome_rows = chrome_height.min(frame_height).min(window_height);
    let mut output: Option<silksurf_gui::WinitDamageRect> = None;
    if y0 < chrome_rows {
        let chrome_y1 = y1.min(chrome_rows);
        union_present_damage_rect(
            &mut output,
            silksurf_gui::WinitDamageRect {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: chrome_y1 - y0,
            },
        );
    }

    let content_rows = window_height.saturating_sub(chrome_rows);
    let visible_source_y0 = chrome_height.saturating_add(scroll_y);
    let visible_source_y1 = visible_source_y0
        .saturating_add(content_rows)
        .min(frame_height);
    let source_y0 = y0.max(chrome_height).max(visible_source_y0);
    let source_y1 = y1.min(visible_source_y1);
    if source_y1 > source_y0 {
        union_present_damage_rect(
            &mut output,
            silksurf_gui::WinitDamageRect {
                x: x0,
                y: chrome_rows.saturating_add(source_y0.saturating_sub(visible_source_y0)),
                width: x1 - x0,
                height: source_y1 - source_y0,
            },
        );
    }

    match output {
        Some(rect) => silksurf_gui::WinitPresentDamage::Rect(rect),
        None => silksurf_gui::WinitPresentDamage::Clean,
    }
}

pub(crate) fn union_present_damage_rect(
    output: &mut Option<silksurf_gui::WinitDamageRect>,
    rect: silksurf_gui::WinitDamageRect,
) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    *output = Some(match *output {
        None => rect,
        Some(existing) => {
            let x0 = existing.x.min(rect.x);
            let y0 = existing.y.min(rect.y);
            let x1 = existing
                .x
                .saturating_add(existing.width)
                .max(rect.x.saturating_add(rect.width));
            let y1 = existing
                .y
                .saturating_add(existing.height)
                .max(rect.y.saturating_add(rect.height));
            silksurf_gui::WinitDamageRect {
                x: x0,
                y: y0,
                width: x1.saturating_sub(x0),
                height: y1.saturating_sub(y0),
            }
        }
    });
}

pub(crate) fn copy_frame_rows(
    frame: &[u32],
    frame_width: u32,
    source_y: u32,
    pixels: &mut [u32],
    window_width: u32,
    dest_y: u32,
    copy_width: u32,
    rows: u32,
) {
    if frame_width == 0 || window_width == 0 || copy_width == 0 || rows == 0 {
        return;
    }

    let frame_stride = frame_width as usize;
    let window_stride = window_width as usize;
    let copy_width = copy_width as usize;
    if copy_width == frame_stride && copy_width == window_stride {
        let frame_start = source_y as usize * frame_stride;
        let frame_end = frame_start + rows as usize * frame_stride;
        let window_start = dest_y as usize * window_stride;
        let window_end = window_start + rows as usize * window_stride;
        if frame_end <= frame.len() && window_end <= pixels.len() {
            pixels[window_start..window_end].copy_from_slice(&frame[frame_start..frame_end]);
        }
        return;
    }
    for row in 0..rows as usize {
        let frame_start = (source_y as usize + row) * frame_stride;
        let frame_end = frame_start + copy_width;
        let window_start = (dest_y as usize + row) * window_stride;
        let window_end = window_start + copy_width;
        if frame_end <= frame.len() && window_end <= pixels.len() {
            pixels[window_start..window_end].copy_from_slice(&frame[frame_start..frame_end]);
        }
    }
}

pub(crate) fn copy_frame_rect(
    frame: &[u32],
    frame_width: u32,
    source_x: u32,
    source_y: u32,
    pixels: &mut [u32],
    window_width: u32,
    dest_x: u32,
    dest_y: u32,
    copy_width: u32,
    rows: u32,
) {
    if frame_width == 0 || window_width == 0 || copy_width == 0 || rows == 0 {
        return;
    }

    let frame_stride = frame_width as usize;
    let window_stride = window_width as usize;
    let source_x = source_x as usize;
    let dest_x = dest_x as usize;
    let copy_width = copy_width as usize;
    if source_x == 0 && dest_x == 0 && copy_width == frame_stride && copy_width == window_stride {
        let frame_start = source_y as usize * frame_stride;
        let frame_end = frame_start + rows as usize * frame_stride;
        let window_start = dest_y as usize * window_stride;
        let window_end = window_start + rows as usize * window_stride;
        if frame_end <= frame.len() && window_end <= pixels.len() {
            pixels[window_start..window_end].copy_from_slice(&frame[frame_start..frame_end]);
        }
        return;
    }
    for row in 0..rows as usize {
        let frame_start = (source_y as usize + row) * frame_stride + source_x;
        let frame_end = frame_start + copy_width;
        let window_start = (dest_y as usize + row) * window_stride + dest_x;
        let window_end = window_start + copy_width;
        if frame_end <= frame.len() && window_end <= pixels.len() {
            pixels[window_start..window_end].copy_from_slice(&frame[frame_start..frame_end]);
        }
    }
}

pub(crate) fn draw_browser_chrome_overlays(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    draw_browser_navigation_buttons(state, pixels, window_width, window_height);
    draw_browser_status_from_state(state, pixels, window_width, window_height);
    draw_browser_address_from_state(state, pixels, window_width, window_height);
}

pub(crate) fn draw_browser_navigation_start_from_state(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    draw_browser_navigation_start_buttons(state, pixels, window_width, window_height);
    draw_browser_status_from_state(state, pixels, window_width, window_height);
}

pub(crate) fn browser_status_text(state: &BrowserState) -> &str {
    state
        .hover_status_text
        .as_deref()
        .unwrap_or(state.status_text.as_str())
}

pub(crate) fn set_browser_status(state: &mut BrowserState, status: impl Into<String>) {
    state.status_text = status.into();
    state.hover_status_text = None;
}

pub(crate) fn draw_browser_status_from_state(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    draw_browser_status(
        pixels,
        window_width,
        window_height,
        browser_status_text(state),
    );
}

pub(crate) fn draw_browser_navigation_buttons(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    for action in [
        BrowserChromeAction::Back,
        BrowserChromeAction::Forward,
        BrowserChromeAction::Home,
        BrowserChromeAction::Reload,
        BrowserChromeAction::Stop,
    ] {
        draw_browser_navigation_button(
            pixels,
            window_width,
            window_height,
            chrome_action_button_x(action),
            chrome_action_label(action),
            chrome_action_enabled(state, action),
        );
    }
}

pub(crate) fn draw_browser_navigation_start_buttons(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    for action in [BrowserChromeAction::Reload, BrowserChromeAction::Stop] {
        draw_browser_navigation_button(
            pixels,
            window_width,
            window_height,
            chrome_action_button_x(action),
            chrome_action_label(action),
            chrome_action_enabled(state, action),
        );
    }
}

pub(crate) fn draw_navigation_start_retained_chrome(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    draw_browser_navigation_button(
        pixels,
        window_width,
        window_height,
        RELOAD_BUTTON_X,
        chrome_action_label(BrowserChromeAction::Reload),
        false,
    );
    draw_browser_navigation_button(
        pixels,
        window_width,
        window_height,
        STOP_BUTTON_X,
        chrome_action_label(BrowserChromeAction::Stop),
        true,
    );
    draw_browser_status(pixels, window_width, window_height, "loading");
}

pub(crate) fn chrome_action_button_x(action: BrowserChromeAction) -> u32 {
    match action {
        BrowserChromeAction::Back => BACK_BUTTON_X,
        BrowserChromeAction::Forward => FORWARD_BUTTON_X,
        BrowserChromeAction::Home => HOME_BUTTON_X,
        BrowserChromeAction::Reload => RELOAD_BUTTON_X,
        BrowserChromeAction::Stop => STOP_BUTTON_X,
    }
}

pub(crate) fn chrome_action_label(action: BrowserChromeAction) -> &'static str {
    match action {
        BrowserChromeAction::Back => "B",
        BrowserChromeAction::Forward => "F",
        BrowserChromeAction::Home => "H",
        BrowserChromeAction::Reload => "R",
        BrowserChromeAction::Stop => "S",
    }
}

pub(crate) fn draw_browser_navigation_button(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    x: u32,
    label: &str,
    enabled: bool,
) {
    let fill = if enabled {
        argb(229, 231, 235, 255)
    } else {
        argb(243, 244, 246, 255)
    };
    let label_color = if enabled {
        argb(31, 41, 55, 255)
    } else {
        argb(156, 163, 175, 255)
    };
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        x,
        NAV_BUTTON_Y,
        NAV_BUTTON_WIDTH,
        NAV_BUTTON_HEIGHT,
        fill,
    );
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        x,
        NAV_BUTTON_Y,
        NAV_BUTTON_WIDTH,
        1,
        argb(209, 213, 219, 255),
    );
    draw_bitmap_text(
        pixels,
        window_width,
        window_height,
        x.saturating_add(4),
        NAV_BUTTON_Y.saturating_add(10),
        label,
        x.saturating_add(NAV_BUTTON_WIDTH),
        label_color,
    );
}

pub(crate) fn draw_browser_address_from_state(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    let address_text = if state.address_editing {
        state.address_text.as_str()
    } else {
        state.frame.url.as_str()
    };
    draw_browser_address_overlay(
        pixels,
        window_width,
        window_height,
        address_text,
        address_cursor_for_state(state, address_text),
        state.address_editing,
    );
}

pub(crate) fn draw_browser_address_text_from_state(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    let address_text = if state.address_editing {
        state.address_text.as_str()
    } else {
        state.frame.url.as_str()
    };
    draw_browser_address_text_strip(
        pixels,
        window_width,
        window_height,
        address_text,
        address_cursor_for_state(state, address_text),
    );
}

pub(crate) fn draw_browser_address_focus_from_state(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    let address_text = if state.address_editing {
        state.address_text.as_str()
    } else {
        state.frame.url.as_str()
    };
    draw_browser_address_focus_overlay(
        pixels,
        window_width,
        window_height,
        address_text,
        address_cursor_for_state(state, address_text),
    );
}

pub(crate) fn draw_browser_address_full_text_from_state(
    state: &BrowserState,
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
) {
    let address_text = if state.address_editing {
        state.address_text.as_str()
    } else {
        state.frame.url.as_str()
    };
    draw_browser_address_full_text_strip(
        pixels,
        window_width,
        window_height,
        address_text,
        address_cursor_for_state(state, address_text),
    );
}

pub(crate) fn address_cursor_for_state(state: &BrowserState, text: &str) -> usize {
    if state.address_editing {
        clamp_address_cursor(text, state.address_cursor)
    } else {
        text.len()
    }
}

pub(crate) fn draw_browser_address_overlay(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    text: &str,
    cursor_byte: usize,
    editing: bool,
) {
    let fill = argb(255, 255, 255, 255);
    let border = if editing {
        argb(37, 99, 235, 255)
    } else {
        argb(209, 213, 219, 255)
    };
    fill_address_bar_box(pixels, window_width, window_height, fill, border);
    let text_x = ADDRESS_BAR_X + 10;
    let text_y = ADDRESS_BAR_Y + 10;
    draw_bitmap_text(
        pixels,
        window_width,
        window_height,
        text_x,
        text_y,
        text,
        ADDRESS_BAR_X + ADDRESS_BAR_WIDTH - 12,
        argb(31, 41, 55, 255),
    );
    if editing {
        let cursor_x = bitmap_text_prefix_end_x(
            text_x,
            text,
            cursor_byte,
            ADDRESS_BAR_X + ADDRESS_BAR_WIDTH - 12,
        );
        fill_argb_rect(
            pixels,
            window_width,
            window_height,
            cursor_x.saturating_add(1),
            ADDRESS_BAR_Y + 7,
            1,
            ADDRESS_BAR_HEIGHT - 14,
            argb(31, 41, 55, 255),
        );
    }
}

pub(crate) fn draw_browser_address_focus_overlay(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    text: &str,
    cursor_byte: usize,
) {
    fill_address_bar_border(pixels, window_width, window_height, argb(37, 99, 235, 255));
    let cursor_x = bitmap_text_prefix_end_x(
        ADDRESS_BAR_X + 10,
        text,
        cursor_byte,
        ADDRESS_BAR_X + ADDRESS_BAR_WIDTH - 12,
    );
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        cursor_x.saturating_add(1),
        ADDRESS_BAR_Y + 7,
        1,
        ADDRESS_BAR_HEIGHT - 14,
        argb(31, 41, 55, 255),
    );
}

pub(crate) fn draw_browser_address_text_strip(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    text: &str,
    cursor_byte: usize,
) {
    let text_x = ADDRESS_BAR_X + 10;
    let text_y = ADDRESS_BAR_Y + 10;
    let strip_y = ADDRESS_BAR_Y + 7;
    let text_max_x = ADDRESS_BAR_X + ADDRESS_BAR_WIDTH - 12;
    let cursor_x = bitmap_text_prefix_end_x(text_x, text, cursor_byte, text_max_x);
    let text_end_x = bitmap_text_prefix_end_x(text_x, text, text.len(), text_max_x);
    let strip_end_x = text_end_x
        .max(cursor_x.saturating_add(2))
        .saturating_add(6)
        .min(text_max_x);
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        text_x,
        strip_y,
        strip_end_x.saturating_sub(text_x),
        ADDRESS_BAR_HEIGHT - 14,
        argb(255, 255, 255, 255),
    );
    draw_bitmap_text(
        pixels,
        window_width,
        window_height,
        text_x,
        text_y,
        text,
        text_max_x,
        argb(31, 41, 55, 255),
    );
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        cursor_x.saturating_add(1),
        strip_y,
        1,
        ADDRESS_BAR_HEIGHT - 14,
        argb(31, 41, 55, 255),
    );
}

pub(crate) fn draw_browser_address_full_text_strip(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    text: &str,
    cursor_byte: usize,
) {
    let text_x = ADDRESS_BAR_X + 10;
    let text_y = ADDRESS_BAR_Y + 10;
    let strip_y = ADDRESS_BAR_Y + 7;
    let text_max_x = ADDRESS_BAR_X + ADDRESS_BAR_WIDTH - 12;
    let cursor_x = bitmap_text_prefix_end_x(text_x, text, cursor_byte, text_max_x);
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        text_x,
        strip_y,
        text_max_x.saturating_sub(text_x),
        ADDRESS_BAR_HEIGHT - 14,
        argb(255, 255, 255, 255),
    );
    draw_bitmap_text(
        pixels,
        window_width,
        window_height,
        text_x,
        text_y,
        text,
        text_max_x,
        argb(31, 41, 55, 255),
    );
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        cursor_x.saturating_add(1),
        strip_y,
        1,
        ADDRESS_BAR_HEIGHT - 14,
        argb(31, 41, 55, 255),
    );
}

pub(crate) fn fill_address_bar_box(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    fill: u32,
    border: u32,
) {
    if window_width == 0 || window_height == 0 {
        return;
    }
    let x0 = ADDRESS_BAR_X.min(window_width);
    let x1 = ADDRESS_BAR_X
        .saturating_add(ADDRESS_BAR_WIDTH)
        .min(window_width);
    let y0 = ADDRESS_BAR_Y.min(window_height);
    let y1 = ADDRESS_BAR_Y
        .saturating_add(ADDRESS_BAR_HEIGHT)
        .min(window_height);
    if x1 <= x0 || y1 <= y0 {
        return;
    }

    let stride = window_width as usize;
    let x0_usize = x0 as usize;
    let x1_usize = x1 as usize;
    let left_x = ADDRESS_BAR_X;
    let right_x = ADDRESS_BAR_X.saturating_add(ADDRESS_BAR_WIDTH - 1);
    let bottom_y = ADDRESS_BAR_Y.saturating_add(ADDRESS_BAR_HEIGHT - 1);
    for y in y0..y1 {
        let row_start = y as usize * stride + x0_usize;
        let row_end = y as usize * stride + x1_usize;
        if row_end > pixels.len() {
            return;
        }
        if y == ADDRESS_BAR_Y || y == bottom_y {
            pixels[row_start..row_end].fill(border);
            continue;
        }
        pixels[row_start..row_end].fill(fill);
        if left_x >= x0 && left_x < x1 {
            pixels[y as usize * stride + left_x as usize] = border;
        }
        if right_x >= x0 && right_x < x1 {
            pixels[y as usize * stride + right_x as usize] = border;
        }
    }
}

pub(crate) fn fill_address_bar_border(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    color: u32,
) {
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        ADDRESS_BAR_X,
        ADDRESS_BAR_Y,
        ADDRESS_BAR_WIDTH,
        1,
        color,
    );
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        ADDRESS_BAR_X,
        ADDRESS_BAR_Y + ADDRESS_BAR_HEIGHT - 1,
        ADDRESS_BAR_WIDTH,
        1,
        color,
    );
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        ADDRESS_BAR_X,
        ADDRESS_BAR_Y,
        1,
        ADDRESS_BAR_HEIGHT,
        color,
    );
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        ADDRESS_BAR_X + ADDRESS_BAR_WIDTH - 1,
        ADDRESS_BAR_Y,
        1,
        ADDRESS_BAR_HEIGHT,
        color,
    );
}

pub(crate) fn draw_browser_status(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    status: &str,
) {
    let Some((x, y, width, height)) = browser_status_rect(window_width, window_height) else {
        return;
    };
    let text_x = x.saturating_add(10);
    let text_y = y.saturating_add(6);
    fill_argb_rect(
        pixels,
        window_width,
        window_height,
        text_x,
        text_y,
        width.saturating_sub(10),
        height.saturating_sub(6).min(7),
        argb(243, 244, 246, 255),
    );
    draw_bitmap_text(
        pixels,
        window_width,
        window_height,
        text_x,
        text_y,
        status,
        x.saturating_add(160),
        argb(75, 85, 99, 255),
    );
}

pub(crate) fn browser_status_rect(
    window_width: u32,
    window_height: u32,
) -> Option<(u32, u32, u32, u32)> {
    if window_width == 0 || window_height == 0 {
        return None;
    }
    let x = 1000_u32.min(window_width.saturating_sub(1));
    let y = 8_u32.min(window_height.saturating_sub(1));
    let width = window_width.saturating_sub(x).min(170);
    let height = 28_u32.min(window_height.saturating_sub(y));
    (width > 0 && height > 0).then_some((x, y, width, height))
}

pub(crate) fn browser_status_text_band_rect(
    window_width: u32,
    window_height: u32,
) -> Option<silksurf_gui::WinitDamageRect> {
    let (x, y, width, height) = browser_status_rect(window_width, window_height)?;
    let text_x = x.saturating_add(10);
    let text_y = y.saturating_add(6);
    let width = width.saturating_sub(10);
    let height = height.saturating_sub(6).min(7);
    (width > 0 && height > 0).then_some(silksurf_gui::WinitDamageRect {
        x: text_x,
        y: text_y,
        width,
        height,
    })
}

pub(crate) fn draw_bitmap_text(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    x: u32,
    y: u32,
    text: &str,
    max_x: u32,
    color: u32,
) -> u32 {
    let mut cursor_x = x;
    for byte in text.bytes() {
        if cursor_x.saturating_add(5) > max_x {
            break;
        }
        draw_bitmap_byte(
            pixels,
            window_width,
            window_height,
            cursor_x,
            y,
            byte,
            color,
        );
        cursor_x = cursor_x.saturating_add(6);
    }
    cursor_x
}

pub(crate) fn bitmap_text_prefix_end_x(x: u32, text: &str, cursor_byte: usize, max_x: u32) -> u32 {
    let cursor_byte = clamp_address_cursor(text, cursor_byte);
    let mut cursor_x = x;
    for (index, _) in text.char_indices() {
        if index >= cursor_byte || cursor_x.saturating_add(5) > max_x {
            break;
        }
        cursor_x = cursor_x.saturating_add(6);
    }
    cursor_x
}

pub(crate) fn draw_bitmap_byte(
    pixels: &mut [u32],
    window_width: u32,
    window_height: u32,
    x: u32,
    y: u32,
    byte: u8,
    color: u32,
) {
    let Some(glyph) = chrome_glyph_byte(byte) else {
        return;
    };
    if x.saturating_add(5) <= window_width && y.saturating_add(7) <= window_height {
        let stride = window_width as usize;
        let base_x = x as usize;
        let base_y = y as usize;
        for (row, bits) in glyph.iter().enumerate() {
            let row_start = (base_y + row) * stride + base_x;
            for col in 0..5 {
                if (bits >> (4 - col)) & 1 == 1 {
                    pixels[row_start + col] = color;
                }
            }
        }
        return;
    }

    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..5 {
            if (bits >> (4 - col)) & 1 == 1 {
                put_argb_pixel(
                    pixels,
                    window_width,
                    window_height,
                    x + col,
                    y + row as u32,
                    color,
                );
            }
        }
    }
}

pub(crate) fn draw_page_bitmap_text_clipped(
    pixels: &mut [u32],
    width: u32,
    height: u32,
    x: f32,
    y: f32,
    text: &str,
    font_size: f32,
    color: u32,
    clip: PixelRect,
) -> bool {
    let Some((scale, advance, line_height, space_advance)) = page_bitmap_text_metrics(font_size)
    else {
        return false;
    };
    let mut cursor_x = x.round() as i32;
    let mut cursor_y = y.round() as i32;
    let line_origin_x = cursor_x;
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
            ' ' => {
                cursor_x = cursor_x.saturating_add(space_advance);
            }
            _ => {
                if !ch.is_ascii() {
                    return false;
                }
                let Some(glyph) = chrome_glyph_byte(ch as u8) else {
                    return false;
                };
                draw_page_bitmap_glyph_clipped(
                    pixels, width, height, cursor_x, cursor_y, scale, glyph, color, clip,
                );
                cursor_x = cursor_x.saturating_add(advance);
            }
        }
    }
    true
}

pub(crate) fn page_bitmap_text_metrics(font_size: f32) -> Option<(i32, i32, i32, i32)> {
    if !font_size.is_finite() || font_size <= 0.0 {
        return None;
    }
    Some((
        ((font_size / 12.0).round() as i32).max(1),
        (font_size * 0.55).round().max(6.0) as i32,
        (font_size * 1.2).round().max(8.0) as i32,
        (font_size * 0.33).round().max(3.0) as i32,
    ))
}

pub(crate) fn draw_page_bitmap_glyph_clipped(
    pixels: &mut [u32],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    scale: i32,
    glyph: [u8; 7],
    color: u32,
    clip: PixelRect,
) {
    let clip_x1 = clip.x.saturating_add(clip.width);
    let clip_y1 = clip.y.saturating_add(clip.height);
    if let Some((glyph_x, glyph_y, glyph_scale)) =
        page_bitmap_glyph_fast_bounds(width, height, x, y, scale, clip)
    {
        draw_page_bitmap_glyph_unchecked(
            pixels,
            width,
            glyph_x,
            glyph_y,
            glyph_scale,
            glyph,
            color,
        );
        return;
    }
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..5 {
            if (bits >> (4 - col)) & 1 == 0 {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    let pixel_x = x + col * scale + dx;
                    let pixel_y = y + row as i32 * scale + dy;
                    if pixel_x < clip.x as i32
                        || pixel_y < clip.y as i32
                        || pixel_x >= clip_x1 as i32
                        || pixel_y >= clip_y1 as i32
                    {
                        continue;
                    }
                    put_argb_pixel(pixels, width, height, pixel_x as u32, pixel_y as u32, color);
                }
            }
        }
    }
}

pub(crate) fn page_bitmap_glyph_fast_bounds(
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    scale: i32,
    clip: PixelRect,
) -> Option<(u32, u32, u32)> {
    if scale <= 0 {
        return None;
    }
    if x < 0 {
        return None;
    }
    if y < 0 {
        return None;
    }
    let glyph_x = x as u32;
    let glyph_y = y as u32;
    let glyph_scale = scale as u32;
    let glyph_right = glyph_x.checked_add(5_u32.saturating_mul(glyph_scale))?;
    let glyph_bottom = glyph_y.checked_add(7_u32.saturating_mul(glyph_scale))?;
    if glyph_x < clip.x {
        return None;
    }
    if glyph_y < clip.y {
        return None;
    }
    if glyph_right > clip.x.saturating_add(clip.width) {
        return None;
    }
    if glyph_bottom > clip.y.saturating_add(clip.height) {
        return None;
    }
    if glyph_right > width {
        return None;
    }
    if glyph_bottom > height {
        return None;
    }
    Some((glyph_x, glyph_y, glyph_scale))
}

pub(crate) fn draw_page_bitmap_glyph_unchecked(
    pixels: &mut [u32],
    width: u32,
    x: u32,
    y: u32,
    scale: u32,
    glyph: [u8; 7],
    color: u32,
) {
    let stride = width as usize;
    let base_x = x as usize;
    let base_y = y as usize;
    let scale = scale as usize;
    for (row, bits) in glyph.iter().enumerate() {
        let row_base = base_y + row * scale;
        for col in 0..5 {
            if (bits >> (4 - col)) & 1 == 0 {
                continue;
            }
            let col_base = base_x + col * scale;
            for dy in 0..scale {
                let row_start = (row_base + dy) * stride + col_base;
                for dx in 0..scale {
                    pixels[row_start + dx] = color;
                }
            }
        }
    }
}

pub(crate) const CHROME_GLYPHS: &[(u8, [u8; 7])] = &[
    (b' ', [0, 0, 0, 0, 0, 0, 0]),
    (
        b'!',
        [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
    ),
    (
        b'"',
        [
            0b01010, 0b01010, 0b01010, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
    ),
    (
        b'#',
        [
            0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010,
        ],
    ),
    (
        b'$',
        [
            0b00100, 0b01111, 0b10100, 0b01110, 0b00101, 0b11110, 0b00100,
        ],
    ),
    (
        b'%',
        [
            0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011,
        ],
    ),
    (
        b'&',
        [
            0b01100, 0b10010, 0b10100, 0b01000, 0b10101, 0b10010, 0b01101,
        ],
    ),
    (
        b'\'',
        [
            0b00100, 0b00100, 0b01000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
    ),
    (
        b'(',
        [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
    ),
    (
        b')',
        [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
    ),
    (
        b'*',
        [
            0b00000, 0b10101, 0b01110, 0b11111, 0b01110, 0b10101, 0b00000,
        ],
    ),
    (
        b'+',
        [
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
    ),
    (
        b',',
        [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100, 0b01000,
        ],
    ),
    (
        b'-',
        [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
    ),
    (
        b'.',
        [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100,
        ],
    ),
    (
        b'/',
        [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
    ),
    (
        b'0',
        [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
    ),
    (
        b'1',
        [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
    ),
    (
        b'2',
        [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
    ),
    (
        b'3',
        [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
    ),
    (
        b'4',
        [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
    ),
    (
        b'5',
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
    ),
    (
        b'6',
        [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        b'7',
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
    ),
    (
        b'8',
        [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        b'9',
        [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
    ),
    (
        b':',
        [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
    ),
    (
        b';',
        [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b01000,
        ],
    ),
    (
        b'<',
        [
            0b00010, 0b00100, 0b01000, 0b10000, 0b01000, 0b00100, 0b00010,
        ],
    ),
    (
        b'=',
        [
            0b00000, 0b11111, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000,
        ],
    ),
    (
        b'>',
        [
            0b01000, 0b00100, 0b00010, 0b00001, 0b00010, 0b00100, 0b01000,
        ],
    ),
    (
        b'?',
        [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
    ),
    (
        b'@',
        [
            0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01110,
        ],
    ),
    (
        b'[',
        [
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
        ],
    ),
    (
        b'\\',
        [
            0b10000, 0b01000, 0b01000, 0b00100, 0b00010, 0b00010, 0b00001,
        ],
    ),
    (
        b']',
        [
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
        ],
    ),
    (
        b'^',
        [
            0b00100, 0b01010, 0b10001, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
    ),
    (
        b'_',
        [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
    ),
    (
        b'`',
        [
            0b01000, 0b00100, 0b00010, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
    ),
    (
        b'a',
        [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        b'b',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
    ),
    (
        b'c',
        [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
    ),
    (
        b'd',
        [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
    ),
    (
        b'e',
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
    ),
    (
        b'f',
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        b'g',
        [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
    ),
    (
        b'h',
        [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        b'i',
        [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
    ),
    (
        b'j',
        [
            0b00001, 0b00001, 0b00001, 0b00001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        b'k',
        [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
    ),
    (
        b'l',
        [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
    ),
    (
        b'm',
        [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        b'n',
        [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        b'o',
        [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        b'p',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        b'q',
        [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
    ),
    (
        b'r',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
    ),
    (
        b's',
        [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
    ),
    (
        b't',
        [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        b'u',
        [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        b'v',
        [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
    ),
    (
        b'w',
        [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
    ),
    (
        b'x',
        [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
    ),
    (
        b'y',
        [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        b'z',
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
    ),
    (
        b'{',
        [
            0b00010, 0b00100, 0b00100, 0b01000, 0b00100, 0b00100, 0b00010,
        ],
    ),
    (
        b'|',
        [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        b'}',
        [
            0b01000, 0b00100, 0b00100, 0b00010, 0b00100, 0b00100, 0b01000,
        ],
    ),
    (
        b'~',
        [
            0b00000, 0b00000, 0b01000, 0b10101, 0b00010, 0b00000, 0b00000,
        ],
    ),
];

pub(crate) const fn build_chrome_glyph_table() -> [[u8; 7]; 128] {
    let mut table = [[0; 7]; 128];
    let mut index = 0;
    while index < CHROME_GLYPHS.len() {
        let (byte, glyph) = CHROME_GLYPHS[index];
        table[byte as usize] = glyph;
        index += 1;
    }
    table
}

pub(crate) const fn build_chrome_glyph_presence_table() -> [bool; 128] {
    let mut present = [false; 128];
    let mut index = 0;
    while index < CHROME_GLYPHS.len() {
        let (byte, _) = CHROME_GLYPHS[index];
        present[byte as usize] = true;
        index += 1;
    }
    present
}

pub(crate) const CHROME_GLYPH_TABLE: [[u8; 7]; 128] = build_chrome_glyph_table();
pub(crate) const CHROME_GLYPH_PRESENT: [bool; 128] = build_chrome_glyph_presence_table();

pub(crate) fn chrome_glyph_byte(byte: u8) -> Option<[u8; 7]> {
    let ascii = byte.to_ascii_lowercase();
    if ascii < 128 && CHROME_GLYPH_PRESENT[ascii as usize] {
        Some(CHROME_GLYPH_TABLE[ascii as usize])
    } else {
        None
    }
}

pub(crate) fn fill_argb_rect(
    pixels: &mut [u32],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    rect_width: u32,
    rect_height: u32,
    color: u32,
) {
    if width == 0 || height == 0 || rect_width == 0 || rect_height == 0 {
        return;
    }
    let x_end = x.saturating_add(rect_width).min(width);
    let y_end = y.saturating_add(rect_height).min(height);
    if x == 0 && x_end == width {
        let start = y as usize * width as usize;
        let end = y_end as usize * width as usize;
        if end <= pixels.len() {
            fill_argb_words(&mut pixels[start..end], color);
        }
        return;
    }
    for row in y..y_end {
        let start = row as usize * width as usize + x as usize;
        let end = row as usize * width as usize + x_end as usize;
        if end <= pixels.len() {
            fill_argb_words(&mut pixels[start..end], color);
        }
    }
}

pub(crate) fn fill_argb_words(pixels: &mut [u32], color: u32) {
    match color {
        0 => {
            /*
             * SAFETY: the slice is valid for pixels.len() u32 writes, and
             * every byte pattern is a valid u32 value.
             */
            unsafe {
                std::ptr::write_bytes(pixels.as_mut_ptr(), 0, pixels.len());
            }
        }
        0xffff_ffff => {
            /*
             * SAFETY: the slice is valid for pixels.len() u32 writes, and
             * every byte pattern is a valid u32 value.
             */
            unsafe {
                std::ptr::write_bytes(pixels.as_mut_ptr(), 0xff, pixels.len());
            }
        }
        _ if pixels.len() >= 64 => fill_argb_words_by_copy(pixels, color),
        _ => pixels.fill(color),
    }
}

pub(crate) fn fill_argb_words_by_copy(pixels: &mut [u32], color: u32) {
    let Some(first) = pixels.first_mut() else {
        return;
    };
    *first = color;
    let mut initialized = 1usize;
    while initialized < pixels.len() {
        let copy_len = initialized.min(pixels.len() - initialized);
        let (src, dst) = pixels.split_at_mut(initialized);
        dst[..copy_len].copy_from_slice(&src[..copy_len]);
        initialized += copy_len;
    }
}

pub(crate) fn put_argb_pixel(
    pixels: &mut [u32],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    color: u32,
) {
    if x >= width || y >= height {
        return;
    }
    let index = y as usize * width as usize + x as usize;
    if index < pixels.len() {
        pixels[index] = color;
    }
}

pub(crate) fn argb(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (u32::from(a) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

pub(crate) fn css_color_to_argb(color: silksurf_css::Color) -> u32 {
    argb(color.r, color.g, color.b, color.a)
}

#[cfg(test)]
mod tests {
    // Module split from the former single-file binary; the crate root
    // re-exports every module so sibling items resolve by bare name.
    #[allow(clippy::wildcard_imports)]
    use crate::*;
    use silksurf_render::DisplayItem;

    #[test]
    fn viewport_argb_direct_paints_supported_items() {
        let image_rgba: Arc<[u8]> = Arc::from(vec![255, 0, 0, 255].into_boxed_slice());
        let display_list = silksurf_render::DisplayList {
            items: vec![
                DisplayItem::SolidColor {
                    rect: Rect {
                        x: 0.0,
                        y: BROWSER_CHROME_HEIGHT,
                        width: 32.0,
                        height: 32.0,
                    },
                    color: rgba(1, 2, 3, 255),
                },
                DisplayItem::Text {
                    rect: Rect {
                        x: 2.0,
                        y: BROWSER_CHROME_HEIGHT + 2.0,
                        width: 64.0,
                        height: 16.0,
                    },
                    node: silksurf_dom::NodeId::from_raw(1),
                    text_len: 2,
                    text: "ok".to_string(),
                    font_size: 12.0,
                    color: rgba(255, 255, 255, 255),
                },
                DisplayItem::Image {
                    rect: Rect {
                        x: 8.0,
                        y: BROWSER_CHROME_HEIGHT + 8.0,
                        width: 1.0,
                        height: 1.0,
                    },
                    image: silksurf_render::ImageSurface {
                        width: 1,
                        height: 1,
                        rgba: image_rgba,
                    },
                },
            ],
            tiles: None,
        };
        let mut pixels = Vec::new();
        let mut item_indices = Vec::new();

        assert!(rasterize_browser_viewport_argb_direct(
            &display_list,
            0,
            96,
            &mut pixels,
            &mut item_indices
        ));
        assert_eq!(pixels.len(), FRAME_WIDTH as usize * 96);
        assert_eq!(
            pixels[BROWSER_CHROME_HEIGHT as usize * FRAME_WIDTH as usize],
            argb(1, 2, 3, 255)
        );
        assert_eq!(
            pixels[(BROWSER_CHROME_HEIGHT as usize + 8) * FRAME_WIDTH as usize + 8],
            argb(255, 0, 0, 255)
        );
    }

    #[test]
    fn viewport_argb_direct_rejects_unsupported_items() {
        let display_list = silksurf_render::DisplayList {
            items: vec![DisplayItem::LinearGradient {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT,
                    width: 32.0,
                    height: 32.0,
                },
                angle: 90.0,
                stops: vec![(0.0, rgba(0, 0, 0, 255)), (1.0, rgba(255, 255, 255, 255))],
            }],
            tiles: None,
        };
        let mut pixels = vec![0x1234_5678];
        let mut item_indices = Vec::new();

        assert!(!rasterize_browser_viewport_argb_direct(
            &display_list,
            0,
            96,
            &mut pixels,
            &mut item_indices
        ));
        assert_eq!(pixels, vec![0x1234_5678]);
    }

    #[test]
    fn viewport_argb_preferred_keeps_rgba_empty_on_direct_hit() {
        let display_list = silksurf_render::DisplayList {
            items: vec![DisplayItem::SolidColor {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT,
                    width: FRAME_WIDTH as f32,
                    height: 52.0,
                },
                color: rgba(7, 8, 9, 255),
            }],
            tiles: None,
        };
        let mut rgba = Vec::new();
        let mut argb = Vec::new();
        let mut item_indices = Vec::new();

        assert!(rasterize_browser_viewport_argb_preferred(
            &display_list,
            0,
            96,
            &mut rgba,
            &mut argb,
            &mut item_indices
        ));
        assert!(rgba.is_empty());
        assert_eq!(argb.len(), FRAME_WIDTH as usize * 96);
    }

    #[test]
    fn viewport_argb_preferred_falls_back_for_gradient() {
        let display_list = silksurf_render::DisplayList {
            items: vec![DisplayItem::LinearGradient {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT,
                    width: 32.0,
                    height: 32.0,
                },
                angle: 90.0,
                stops: vec![(0.0, rgba(0, 0, 0, 255)), (1.0, rgba(255, 255, 255, 255))],
            }],
            tiles: None,
        };
        let mut rgba = Vec::new();
        let mut argb = Vec::new();
        let mut item_indices = Vec::new();

        assert!(!rasterize_browser_viewport_argb_preferred(
            &display_list,
            0,
            96,
            &mut rgba,
            &mut argb,
            &mut item_indices
        ));
        assert_eq!(rgba.len(), FRAME_WIDTH as usize * 96 * 4);
        assert_eq!(argb.len(), FRAME_WIDTH as usize * 96);
    }

    #[test]
    fn document_damage_argb_direct_paints_clipped_strip() {
        let display_list = silksurf_render::DisplayList {
            items: vec![DisplayItem::SolidColor {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT,
                    width: FRAME_WIDTH as f32,
                    height: 96.0,
                },
                color: rgba(11, 12, 13, 255),
            }],
            tiles: None,
        };
        let mut pixels = vec![0; FRAME_WIDTH as usize * 96];
        let mut item_indices = Vec::new();
        let damage = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT + 12.0,
            width: FRAME_WIDTH as f32,
            height: 4.0,
        };

        assert!(rasterize_browser_document_damage_argb_direct(
            &display_list,
            0,
            96,
            damage,
            &mut pixels,
            &mut item_indices
        ));
        assert_eq!(
            pixels[(BROWSER_CHROME_HEIGHT as usize + 11) * FRAME_WIDTH as usize],
            0
        );
        assert_eq!(
            pixels[(BROWSER_CHROME_HEIGHT as usize + 12) * FRAME_WIDTH as usize],
            argb(11, 12, 13, 255)
        );
        assert_eq!(
            pixels[(BROWSER_CHROME_HEIGHT as usize + 16) * FRAME_WIDTH as usize],
            0
        );
    }

    #[test]
    fn document_damage_argb_direct_rejects_gradient() {
        let display_list = silksurf_render::DisplayList {
            items: vec![DisplayItem::LinearGradient {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT,
                    width: 32.0,
                    height: 32.0,
                },
                angle: 90.0,
                stops: vec![(0.0, rgba(0, 0, 0, 255)), (1.0, rgba(255, 255, 255, 255))],
            }],
            tiles: None,
        };
        let mut pixels = vec![0x1234_5678; FRAME_WIDTH as usize * 96];
        let mut item_indices = Vec::new();
        let damage = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: 32.0,
            height: 32.0,
        };

        assert!(!rasterize_browser_document_damage_argb_direct(
            &display_list,
            0,
            96,
            damage,
            &mut pixels,
            &mut item_indices
        ));
        assert!(pixels.iter().all(|pixel| *pixel == 0x1234_5678));
    }

    #[test]
    fn rgba_bytes_to_argb_words_into_packs_and_reuses_capacity() {
        let rgba = [
            0x11, 0x22, 0x33, 0x44, 0xaa, 0xbb, 0xcc, 0xdd, 0x10, 0x20, 0x30, 0x40, 0xab, 0xbc,
            0xcd, 0xde, 0x01, 0x02, 0x03, 0x04,
        ];
        let mut argb = Vec::with_capacity(8);
        rgba_bytes_to_argb_words_into(&rgba, &mut argb);
        let capacity = argb.capacity();

        assert_eq!(
            argb,
            vec![
                0x4411_2233,
                0xddaa_bbcc,
                0x4010_2030,
                0xdeab_bccd,
                0x0401_0203
            ]
        );
        rgba_bytes_to_argb_words_into(&rgba[..4], &mut argb);
        assert_eq!(argb, vec![0x4411_2233]);
        assert_eq!(argb.capacity(), capacity);
    }

    #[test]
    fn rgba_bytes_to_argb_words_into_packs_simd_lanes_and_tail() {
        let mut rgba = Vec::new();
        let mut expected = Vec::new();
        for index in 0..17u8 {
            let r = index.wrapping_mul(3).wrapping_add(1);
            let g = index.wrapping_mul(5).wrapping_add(2);
            let b = index.wrapping_mul(7).wrapping_add(3);
            let a = index.wrapping_mul(11).wrapping_add(4);
            rgba.extend_from_slice(&[r, g, b, a]);
            expected.push(
                (u32::from(a) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
            );
        }

        let mut argb = Vec::new();
        rgba_bytes_to_argb_words_into(&rgba, &mut argb);

        assert_eq!(argb, expected);
    }

    #[test]
    fn fill_argb_words_preserves_byte_pattern_colors() {
        let mut pixels = vec![0x1234_5678; 5];

        fill_argb_words(&mut pixels, 0xffff_ffff);
        assert_eq!(pixels, vec![0xffff_ffff; 5]);

        fill_argb_words(&mut pixels[1..4], 0);
        assert_eq!(pixels, vec![0xffff_ffff, 0, 0, 0, 0xffff_ffff]);

        let mut arbitrary = vec![0; 96];
        fill_argb_words(&mut arbitrary, 0xfff3_f4f6);
        assert!(arbitrary.iter().all(|pixel| *pixel == 0xfff3_f4f6));
    }

    #[test]
    fn chrome_glyph_lookup_preserves_ascii_contract() {
        assert_eq!(chrome_glyph_byte(b'A'), chrome_glyph_byte(b'a'));
        assert_eq!(chrome_glyph_byte(b' '), Some([0, 0, 0, 0, 0, 0, 0]));
        assert!(chrome_glyph_byte(b'!').is_some());
        assert_eq!(chrome_glyph_byte(0x7f), None);
    }

    #[test]
    fn shift_browser_argb_content_rows_reuses_rows_when_scrolling_down() {
        let width = 2;
        let chrome_rows = 1;
        let content_rows = 4;
        let mut argb = (0..10).collect::<Vec<u32>>();

        assert!(shift_browser_argb_content_rows(
            &mut argb,
            width,
            chrome_rows,
            content_rows,
            1,
        ));

        assert_eq!(argb, vec![0, 1, 4, 5, 6, 7, 8, 9, 8, 9]);
    }

    #[test]
    fn shift_browser_argb_content_rows_reuses_rows_when_scrolling_up() {
        let width = 2;
        let chrome_rows = 1;
        let content_rows = 4;
        let mut argb = (0..10).collect::<Vec<u32>>();

        assert!(shift_browser_argb_content_rows(
            &mut argb,
            width,
            chrome_rows,
            content_rows,
            -1,
        ));

        assert_eq!(argb, vec![0, 1, 2, 3, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn browser_toolbar_background_fills_cached_rgba_rows() {
        let mut rgba = vec![0; 1100 * 44 * 4];

        fill_browser_toolbar_background_rgba(&mut rgba, 1100, 44);

        assert_eq!(&rgba[0..4], &[243, 244, 246, 255]);
        let separator_offset = 43 * 1100 * 4;
        assert_eq!(
            &rgba[separator_offset..separator_offset + 4],
            &[209, 213, 219, 255]
        );
    }
}
