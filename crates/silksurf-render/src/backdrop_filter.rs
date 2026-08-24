//! `backdrop-filter` as an in-place stage over the pixels already painted.
//!
//! The rasterizer walks the display list in document order into one RGBA
//! buffer, so at the moment a `BackdropFilter` item is reached that buffer
//! already holds the element's backdrop. The pipeline runs over that region
//! in place and the element's own background paints over the result, which is
//! the order CSS Filter Effects 2 defines for `backdrop-filter` without a
//! separate backdrop surface or a compositing layer.
//!
//! Both rasterizers reach this module, because the scalar path and the
//! tiny-skia path each own a premultiplied RGBA8 byte buffer.

use silksurf_css::FilterFunction;
use silksurf_layout::Rect;

/// Applies a filter pipeline to the backdrop inside `rect`.
///
/// `radii` is the element's corner radii in CSS clockwise order, and the
/// write-back is weighted by the coverage of that rounded border box so a
/// pill-shaped element does not leave filtered square corners behind.
pub(crate) fn apply_backdrop_filter(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    rect: Rect,
    radii: [f32; 4],
    filters: &[FilterFunction],
) {
    if filters.is_empty() || width == 0 || height == 0 {
        return;
    }
    let margin = sample_margin(filters);
    let Some(sample) = clip_rect(rect, margin, width, height, blur_scale(filters)) else {
        return;
    };
    let mut region = extract_region(buffer, width, &sample);
    run_pipeline(&mut region, &sample, filters);
    write_back(buffer, width, &sample, &region, rect, radii);
}

/// The factor the backdrop is sampled down by before blurring.
///
/// A box pass is a fixed-width average, so halving the resolution and halving
/// the radius describes the same Gaussian at half the cost per axis. The
/// divisor keeps at least four sampled pixels per standard deviation, which
/// leaves the box approximation the resolution it needs while cutting the
/// corpus declaration's `blur(25px)` from 37,636 sampled pixels to about
/// 1,100. A radius small enough to need every pixel takes scale 1 and the
/// path collapses to a direct blur.
fn blur_scale(filters: &[FilterFunction]) -> usize {
    let widest = filters.iter().fold(0.0f32, |widest, filter| match filter {
        FilterFunction::Blur(sigma) => widest.max(*sigma),
        _ => widest,
    });
    ((widest / 4.0).floor() as usize).max(1)
}

/// An integer pixel region of the frame buffer, and the grid it is sampled on.
///
/// `w` and `h` are frame pixels; `sw` and `sh` are the sampled grid the
/// pipeline runs over, which is the same thing when `scale` is 1.
struct Region {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    scale: usize,
    sw: usize,
    sh: usize,
}

/// How far beyond the element the blur reads real backdrop.
///
/// A Gaussian is negligible past three standard deviations, so sampling that
/// far out leaves the element's own edge indistinguishable from an unbounded
/// backdrop. Sampling only the element rect would instead clamp against its
/// boundary and darken the edge.
fn sample_margin(filters: &[FilterFunction]) -> u32 {
    let widest = filters.iter().fold(0.0f32, |widest, filter| match filter {
        FilterFunction::Blur(sigma) => widest.max(*sigma),
        _ => widest,
    });
    (widest * 3.0).ceil().max(0.0) as u32
}

/// Clips the element rect, grown by `margin`, to the frame buffer.
fn clip_rect(rect: Rect, margin: u32, width: u32, height: u32, scale: usize) -> Option<Region> {
    let margin = margin as f32;
    let x0 = (rect.x - margin).floor().max(0.0) as usize;
    let y0 = (rect.y - margin).floor().max(0.0) as usize;
    let x1 = ((rect.x + rect.width + margin).ceil().max(0.0) as usize).min(width as usize);
    let y1 = ((rect.y + rect.height + margin).ceil().max(0.0) as usize).min(height as usize);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (w, h) = (x1 - x0, y1 - y0);
    Some(Region {
        x: x0,
        y: y0,
        w,
        h,
        scale,
        sw: w.div_ceil(scale),
        sh: h.div_ceil(scale),
    })
}

/// Copies the sampled region out as premultiplied RGBA in [0.0, 1.0].
///
/// Each output cell averages the `scale` by `scale` block of frame pixels
/// beneath it, so the reduction is itself a box filter and composes with the
/// blur that follows. The pipeline runs in floating point, which keeps three
/// chained box passes from accumulating the rounding error eight-bit
/// intermediates would carry.
fn extract_region(buffer: &[u8], width: u32, region: &Region) -> Vec<f32> {
    let mut out = vec![0.0f32; region.sw * region.sh * 4];
    for cell_y in 0..region.sh {
        for cell_x in 0..region.sw {
            let mut sum = [0.0f32; 4];
            let mut count = 0.0f32;
            for row in 0..region.scale {
                let y = region.y + cell_y * region.scale + row;
                if cell_y * region.scale + row >= region.h {
                    break;
                }
                for column in 0..region.scale {
                    if cell_x * region.scale + column >= region.w {
                        break;
                    }
                    let x = region.x + cell_x * region.scale + column;
                    let src = (y * width as usize + x) * 4;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += f32::from(buffer[src + channel]);
                    }
                    count += 1.0;
                }
            }
            let dst = (cell_y * region.sw + cell_x) * 4;
            for (channel, total) in sum.iter().enumerate() {
                out[dst + channel] = total / (count.max(1.0) * 255.0);
            }
        }
    }
    out
}

/// Runs each filter function in source order.
///
/// The blur radius divides by the sampling scale, because the grid the
/// pipeline runs over is that many frame pixels per cell.
fn run_pipeline(pixels: &mut [f32], region: &Region, filters: &[FilterFunction]) {
    for filter in filters {
        match filter {
            FilterFunction::Blur(sigma) => {
                gaussian_blur(pixels, region.sw, region.sh, sigma / region.scale as f32);
            }
            other => apply_color_function(pixels, *other),
        }
    }
}

/// Approximates a Gaussian blur with three box passes per axis.
///
/// SVG 1.1 feGaussianBlur, which CSS Filter Effects 1 defers to for
/// `blur()`, specifies this approximation directly: box width
/// `d = floor(sigma * 3 * sqrt(2*PI) / 4 + 0.5)`. Each pass carries a running
/// sum, so the cost is constant per pixel regardless of the radius -- the
/// property matters here because the corpus declares `blur(25px)`.
fn gaussian_blur(region: &mut [f32], w: usize, h: usize, sigma: f32) {
    if sigma <= 0.0 || w == 0 || h == 0 {
        return;
    }
    let d = (sigma * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0 + 0.5).floor() as usize;
    if d < 2 {
        return;
    }
    let mut scratch = vec![0.0f32; region.len()];
    for (left, right) in box_sizes(d) {
        box_pass_rows(region, &mut scratch, w, h, left, right);
        box_pass_columns(&scratch, region, w, h, left, right);
    }
}

/// The three box windows the feGaussianBlur approximation uses.
///
/// An odd `d` centers all three boxes on the output pixel. An even `d` has no
/// centered window, so the spec pairs two boxes of width `d` straddling the
/// pixel boundary on either side with one centered box of width `d + 1`.
fn box_sizes(d: usize) -> [(usize, usize); 3] {
    if d % 2 == 1 {
        let radius = (d - 1) / 2;
        return [(radius, radius); 3];
    }
    let half = d / 2;
    [(half, half - 1), (half - 1, half), (half, half)]
}

/// One horizontal box pass, carrying a four-channel running sum per row.
///
/// Sampling clamps at each end, which holds the edge pixel rather than fading
/// the region toward black.
fn box_pass_rows(src: &[f32], dst: &mut [f32], w: usize, h: usize, left: usize, right: usize) {
    let window = (left + right + 1) as f32;
    let last = w as isize - 1;
    for row in 0..h {
        let base = row * w * 4;
        let at = |index: isize| base + (index.clamp(0, last) as usize) * 4;
        let mut sum = [0.0f32; 4];
        for offset in 0..=(left + right) {
            let source = at(offset as isize - left as isize);
            for (channel, total) in sum.iter_mut().enumerate() {
                *total += src[source + channel];
            }
        }
        for column in 0..w {
            let target = base + column * 4;
            let leaving = at(column as isize - left as isize);
            let entering = at(column as isize + right as isize + 1);
            for (channel, total) in sum.iter_mut().enumerate() {
                dst[target + channel] = *total / window;
                *total += src[entering + channel] - src[leaving + channel];
            }
        }
    }
}

/// One vertical box pass, carrying one running sum per column.
///
/// The accumulator spans the row rather than the column, so the sweep reads
/// and writes whole rows in address order. A per-column running sum would
/// stride by the row length on every access and miss cache on each one, which
/// measured as the dominant cost of the stage.
fn box_pass_columns(src: &[f32], dst: &mut [f32], w: usize, h: usize, left: usize, right: usize) {
    let window = (left + right + 1) as f32;
    let last = h as isize - 1;
    let row_at = |index: isize| (index.clamp(0, last) as usize) * w * 4;
    let stride = w * 4;
    let mut sums = vec![0.0f32; stride];
    for offset in 0..=(left + right) {
        let source = row_at(offset as isize - left as isize);
        for (index, total) in sums.iter_mut().enumerate() {
            *total += src[source + index];
        }
    }
    for row in 0..h {
        let target = row * stride;
        let leaving = row_at(row as isize - left as isize);
        let entering = row_at(row as isize + right as isize + 1);
        for (index, total) in sums.iter_mut().enumerate() {
            dst[target + index] = *total / window;
            *total += src[entering + index] - src[leaving + index];
        }
    }
}

/// Applies one per-pixel filter function.
///
/// The region holds premultiplied color, so each pixel is divided back out to
/// straight color before the function runs and multiplied in again after.
/// Running a color matrix on premultiplied channels would scale every result
/// by the pixel's own alpha.
fn apply_color_function(region: &mut [f32], filter: FilterFunction) {
    for pixel in region.chunks_exact_mut(4) {
        let alpha = pixel[3];
        if alpha <= 0.0 {
            // A fully transparent pixel carries no color to transform, and
            // every function in this set leaves a zero alpha at zero.
            continue;
        }
        let mut rgb = [pixel[0] / alpha, pixel[1] / alpha, pixel[2] / alpha];
        let alpha = filter_pixel(&mut rgb, alpha, filter);
        pixel[0] = rgb[0].clamp(0.0, 1.0) * alpha;
        pixel[1] = rgb[1].clamp(0.0, 1.0) * alpha;
        pixel[2] = rgb[2].clamp(0.0, 1.0) * alpha;
        pixel[3] = alpha;
    }
}

/// Transforms one straight-color pixel, returning its new alpha.
///
/// The saturate, grayscale, and sepia matrices are the ones CSS Filter
/// Effects 1 lists; grayscale is saturate's complement and both share the
/// luminance coefficients, so one matrix serves the three.
fn filter_pixel(rgb: &mut [f32; 3], alpha: f32, filter: FilterFunction) -> f32 {
    match filter {
        FilterFunction::Saturate(amount) => saturate(rgb, amount),
        FilterFunction::Grayscale(amount) => saturate(rgb, 1.0 - amount),
        FilterFunction::Sepia(amount) => sepia(rgb, amount),
        FilterFunction::Brightness(amount) => {
            for channel in rgb.iter_mut() {
                *channel *= amount;
            }
        }
        FilterFunction::Contrast(amount) => {
            let intercept = 0.5 - 0.5 * amount;
            for channel in rgb.iter_mut() {
                *channel = *channel * amount + intercept;
            }
        }
        FilterFunction::Invert(amount) => {
            for channel in rgb.iter_mut() {
                *channel += amount * (1.0 - 2.0 * *channel);
            }
        }
        FilterFunction::Opacity(amount) => return alpha * amount,
        FilterFunction::Blur(_) => {}
    }
    alpha
}

/// Luminance coefficients shared by saturate, grayscale, and sepia.
const LUMA: [f32; 3] = [0.213, 0.715, 0.072];

/// Interpolates each channel between full luminance and the source color.
///
/// `amount` of 0.0 is the grayscale endpoint and 1.0 is the identity, so
/// values above 1.0 extrapolate past the source and saturate the pixel.
fn saturate(rgb: &mut [f32; 3], amount: f32) {
    let luma = LUMA[0] * rgb[0] + LUMA[1] * rgb[1] + LUMA[2] * rgb[2];
    for channel in rgb.iter_mut() {
        *channel = luma + (*channel - luma) * amount;
    }
}

/// Mixes the source color toward the sepia matrix by `amount`.
fn sepia(rgb: &mut [f32; 3], amount: f32) {
    const MATRIX: [[f32; 3]; 3] = [
        [0.393, 0.769, 0.189],
        [0.349, 0.686, 0.168],
        [0.272, 0.534, 0.131],
    ];
    let source = *rgb;
    for (row, channel) in MATRIX.iter().zip(rgb.iter_mut()) {
        let toned = row[0] * source[0] + row[1] * source[1] + row[2] * source[2];
        *channel += amount * (toned - *channel);
    }
}

/// Writes the filtered region back, weighted by rounded-border-box coverage.
///
/// The sweep covers the element's own rect rather than the sampled region:
/// the margin exists so the blur reads real backdrop, and writing it back
/// would filter the element's surroundings as well. Bounding the sweep this
/// way also keeps the per-pixel coverage test off the margin, which for the
/// corpus element is 37,636 sampled pixels against 1,936 written ones.
///
/// Each written pixel samples the reduced grid bilinearly, so a scale above 1
/// resolves to a smooth gradient rather than to visible cells.
fn write_back(
    buffer: &mut [u8],
    width: u32,
    region: &Region,
    filtered: &[f32],
    rect: Rect,
    radii: [f32; 4],
) {
    let first_column = (rect.x.floor().max(0.0) as usize).saturating_sub(region.x);
    let first_row = (rect.y.floor().max(0.0) as usize).saturating_sub(region.y);
    let last_column = (((rect.x + rect.width).ceil().max(0.0) as usize) - region.x).min(region.w);
    let last_row = (((rect.y + rect.height).ceil().max(0.0) as usize) - region.y).min(region.h);
    for row in first_row..last_row {
        let y = region.y + row;
        for column in first_column..last_column {
            let x = region.x + column;
            let coverage = rounded_coverage(x as f32 + 0.5, y as f32 + 0.5, rect, radii);
            if coverage <= 0.0 {
                continue;
            }
            let sample = sample_bilinear(filtered, region, column, row);
            let dst = (y * width as usize + x) * 4;
            for (channel, value) in sample.iter().enumerate() {
                let filtered = (value.clamp(0.0, 1.0) * 255.0).round();
                let existing = f32::from(buffer[dst + channel]);
                buffer[dst + channel] = (existing + (filtered - existing) * coverage)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Samples the reduced grid at a frame pixel inside the region.
///
/// The half-pixel offsets place the frame pixel's center against the cell
/// centers the reduction averaged onto, so scale 1 reproduces the cell exactly.
fn sample_bilinear(filtered: &[f32], region: &Region, column: usize, row: usize) -> [f32; 4] {
    let scale = region.scale as f32;
    let gx = ((column as f32 + 0.5) / scale - 0.5).max(0.0);
    let gy = ((row as f32 + 0.5) / scale - 0.5).max(0.0);
    let x0 = (gx.floor() as usize).min(region.sw - 1);
    let y0 = (gy.floor() as usize).min(region.sh - 1);
    let x1 = (x0 + 1).min(region.sw - 1);
    let y1 = (y0 + 1).min(region.sh - 1);
    let fx = gx - x0 as f32;
    let fy = gy - y0 as f32;
    let cell = |cx: usize, cy: usize| (cy * region.sw + cx) * 4;
    let (a, b, c, d) = (cell(x0, y0), cell(x1, y0), cell(x0, y1), cell(x1, y1));
    let mut out = [0.0f32; 4];
    for (channel, value) in out.iter_mut().enumerate() {
        let top = filtered[a + channel] + (filtered[b + channel] - filtered[a + channel]) * fx;
        let bottom = filtered[c + channel] + (filtered[d + channel] - filtered[c + channel]) * fx;
        *value = top + (bottom - top) * fy;
    }
    out
}

/// Coverage of the element's rounded border box at a pixel center.
///
/// The one-pixel falloff around the boundary matches the anti-aliased
/// background tiny-skia paints over this region, so the filtered corner and
/// the painted corner share an edge.
fn rounded_coverage(px: f32, py: f32, rect: Rect, radii: [f32; 4]) -> f32 {
    (0.5 - rounded_rect_distance(px, py, rect, radii)).clamp(0.0, 1.0)
}

/// Signed distance from a point to a rounded rectangle, positive outside.
///
/// `radii` runs top-left, top-right, bottom-right, bottom-left, so the
/// quadrant the point falls in selects its corner.
fn rounded_rect_distance(px: f32, py: f32, rect: Rect, radii: [f32; 4]) -> f32 {
    let half_width = rect.width / 2.0;
    let half_height = rect.height / 2.0;
    let dx = px - (rect.x + half_width);
    let dy = py - (rect.y + half_height);
    let radius = match (dx >= 0.0, dy >= 0.0) {
        (false, false) => radii[0],
        (true, false) => radii[1],
        (true, true) => radii[2],
        (false, true) => radii[3],
    }
    .min(half_width)
    .min(half_height)
    .max(0.0);
    let ex = dx.abs() - (half_width - radius);
    let ey = dy.abs() - (half_height - radius);
    let outside = (ex.max(0.0).powi(2) + ey.max(0.0).powi(2)).sqrt();
    outside + ex.max(ey).min(0.0) - radius
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rect covering the whole `size` x `size` buffer, with square corners.
    fn whole(size: u32) -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: size as f32,
            height: size as f32,
        }
    }

    /// An opaque buffer whose pixels come from `shade`, as premultiplied RGBA8.
    fn buffer(size: u32, shade: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        let mut out = Vec::with_capacity((size * size * 4) as usize);
        for y in 0..size {
            for x in 0..size {
                let value = shade(x, y);
                out.extend_from_slice(&[value, value, value, 255]);
            }
        }
        out
    }

    /// The red channel at a pixel.
    fn red(buffer: &[u8], size: u32, x: u32, y: u32) -> u8 {
        buffer[((y * size + x) * 4) as usize]
    }

    /// The three box windows sum to a normalized kernel, so a region of one
    /// constant value blurs to that same value. A kernel that failed to
    /// normalize would drift the whole region toward black or white.
    #[test]
    fn a_blur_leaves_a_constant_region_at_its_own_value() {
        let size = 48;
        let mut pixels = buffer(size, |_, _| 200);
        apply_backdrop_filter(
            &mut pixels,
            size,
            size,
            whole(size),
            [0.0; 4],
            &[FilterFunction::Blur(6.0)],
        );
        for y in 0..size {
            for x in 0..size {
                assert_eq!(red(&pixels, size, x, y), 200, "pixel {x},{y}");
            }
        }
    }

    /// A symmetric normalized kernel over a step edge produces a profile
    /// antisymmetric about the step: the value that far above black equals
    /// 255 less the value the same distance below white.
    #[test]
    fn a_blur_over_a_step_edge_stays_antisymmetric_about_the_step() {
        let size = 64;
        let edge = size / 2;
        let mut pixels = buffer(size, |_, y| if y < edge { 0 } else { 255 });
        apply_backdrop_filter(
            &mut pixels,
            size,
            size,
            whole(size),
            [0.0; 4],
            &[FilterFunction::Blur(4.0)],
        );
        for distance in 0..8u32 {
            let below = u32::from(red(&pixels, size, 10, edge - 1 - distance));
            let above = u32::from(red(&pixels, size, 10, edge + distance));
            assert!(
                below.abs_diff(255 - above) <= 1,
                "distance {distance}: {below} against {}",
                255 - above
            );
        }
        // The step actually spread, rather than surviving as a hard edge.
        assert!(red(&pixels, size, 10, edge) < 255);
        assert!(red(&pixels, size, 10, edge - 1) > 0);
    }

    /// The corpus declares `blur(25px)`, which samples the backdrop down by
    /// six before blurring. A constant region survives that round trip at its
    /// own value, which holds the reduction, the blur, and the bilinear
    /// upsample all to unit gain together.
    #[test]
    fn a_reduced_resolution_blur_leaves_a_constant_region_at_its_own_value() {
        let size = 160;
        assert!(super::blur_scale(&[FilterFunction::Blur(25.0)]) > 1);
        let mut pixels = buffer(size, |_, _| 111);
        apply_backdrop_filter(
            &mut pixels,
            size,
            size,
            whole(size),
            [0.0; 4],
            &[FilterFunction::Blur(25.0)],
        );
        for y in 0..size {
            for x in 0..size {
                assert_eq!(red(&pixels, size, x, y), 111, "pixel {x},{y}");
            }
        }
    }

    /// A reduced-resolution blur still spreads a step monotonically, and the
    /// bilinear upsample leaves no cell-sized plateau along the gradient.
    #[test]
    fn a_reduced_resolution_blur_spreads_a_step_monotonically() {
        let size = 192;
        let edge = size / 2;
        let mut pixels = buffer(size, |_, y| if y < edge { 0 } else { 255 });
        apply_backdrop_filter(
            &mut pixels,
            size,
            size,
            whole(size),
            [0.0; 4],
            &[FilterFunction::Blur(25.0)],
        );
        let column = size / 2;
        let profile: Vec<u8> = (edge - 40..edge + 40)
            .map(|y| red(&pixels, size, column, y))
            .collect();
        for pair in profile.windows(2) {
            assert!(pair[0] <= pair[1], "profile fell: {pair:?}");
        }
        assert!(profile[0] < 40, "far side of the step: {}", profile[0]);
        assert!(profile[79] > 215, "near side of the step: {}", profile[79]);
        // The step spread rather than surviving as a hard edge.
        assert!(profile[38] > 20 && profile[41] < 235, "step stayed sharp");
    }

    /// `saturate(0)` collapses every channel onto the luminance CSS Filter
    /// Effects 1 defines, so pure red reaches 0.213 of full scale.
    #[test]
    fn a_full_desaturation_reaches_the_specified_luminance() {
        let size = 4;
        let mut pixels = Vec::new();
        for _ in 0..size * size {
            pixels.extend_from_slice(&[255, 0, 0, 255]);
        }
        apply_backdrop_filter(
            &mut pixels,
            size,
            size,
            whole(size),
            [0.0; 4],
            &[FilterFunction::Saturate(0.0)],
        );
        let expected = (0.213f32 * 255.0).round() as u8;
        assert_eq!(pixels[0], expected);
        assert_eq!(pixels[1], expected);
        assert_eq!(pixels[2], expected);
        assert_eq!(pixels[3], 255);
    }

    /// An empty pipeline is the `none` value, and leaves the backdrop byte
    /// for byte as it was painted.
    #[test]
    fn an_empty_pipeline_leaves_the_backdrop_untouched() {
        let size = 8;
        let original = buffer(size, |x, y| (x * 8 + y * 4) as u8);
        let mut pixels = original.clone();
        apply_backdrop_filter(&mut pixels, size, size, whole(size), [0.0; 4], &[]);
        assert_eq!(pixels, original);
    }

    /// The write-back is clipped to the rounded border box, so the corner
    /// outside a full pill radius keeps the pixels painted beneath it.
    #[test]
    fn the_rounded_clip_spares_the_corner_outside_the_border_box() {
        let size = 32;
        let original = buffer(size, |_, _| 0);
        let mut pixels = original.clone();
        apply_backdrop_filter(
            &mut pixels,
            size,
            size,
            whole(size),
            [16.0; 4],
            &[FilterFunction::Invert(1.0)],
        );
        // The corner sits outside a radius-16 pill inscribed in the rect.
        assert_eq!(red(&pixels, size, 0, 0), 0, "corner");
        assert_eq!(red(&pixels, size, 31, 0), 0, "corner");
        // The center is inside it, and inverted.
        assert_eq!(red(&pixels, size, 16, 16), 255, "center");
    }

    /// The filter reads beyond the element so the blur samples real backdrop.
    /// Sampling only the element rect would clamp at its edge, so a bright
    /// element over a dark surround would keep its own edge value instead of
    /// darkening toward the surround.
    #[test]
    fn the_blur_samples_backdrop_beyond_the_element_edge() {
        let size = 64;
        let element = Rect {
            x: 24.0,
            y: 24.0,
            width: 16.0,
            height: 16.0,
        };
        // White element region over a black surround.
        let mut pixels = buffer(size, |x, y| {
            if (24..40).contains(&x) && (24..40).contains(&y) {
                255
            } else {
                0
            }
        });
        apply_backdrop_filter(
            &mut pixels,
            size,
            size,
            element,
            [0.0; 4],
            &[FilterFunction::Blur(4.0)],
        );
        // Pulling the black surround in drops the element's own edge well
        // below the 255 a clamped-edge blur would have preserved.
        assert!(
            red(&pixels, size, 24, 32) < 200,
            "element edge stayed at {}",
            red(&pixels, size, 24, 32)
        );
        // Outside the element nothing is written.
        assert_eq!(red(&pixels, size, 20, 32), 0, "surround");
    }
}
