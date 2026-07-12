//! 2D canvas backing store and rasterizer.
//!
//! `CanvasSurface` owns a straight-alpha RGBA8 framebuffer and implements the
//! geometric core of the HTML canvas 2D context: affine transforms, rectangle
//! fills, path construction and filling (nonzero winding), stroking, image
//! blits, and pixel readback. Color strings and the JS binding surface live in
//! the embedding layer; this module works entirely in pre-parsed `[u8; 4]`
//! RGBA and device pixels, so it is testable without a JS context.
//!
//! Compositing is straight-alpha source-over, matching the browser image blit
//! path: a canvas snapshot is a packed RGBA8 buffer (`pixels()`), so the page
//! paints it through the existing image display-item machinery once the
//! embedder wraps the pixels in a render `ImageSurface`.

/// A 2x3 affine transform stored column-major as [a, b, c, d, e, f], mapping
/// (x, y) -> (a*x + c*y + e, b*x + d*y + f). This is the canvas convention.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Affine {
    #[must_use]
    pub fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    #[must_use]
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// Right-multiply by `other`, so `other` applies first (canvas semantics
    /// for translate/scale/rotate/transform, which pre-concatenate).
    #[must_use]
    pub fn then(&self, other: &Affine) -> Affine {
        Affine {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            e: self.a * other.e + self.c * other.f + self.e,
            f: self.b * other.e + self.d * other.f + self.f,
        }
    }
}

/// Mutable drawing state saved and restored by `save()` / `restore()`.
#[derive(Clone, Copy, Debug)]
struct DrawState {
    fill: [u8; 4],
    stroke: [u8; 4],
    line_width: f32,
    global_alpha: f32,
    transform: Affine,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            fill: [0, 0, 0, 255],
            stroke: [0, 0, 0, 255],
            line_width: 1.0,
            global_alpha: 1.0,
            transform: Affine::identity(),
        }
    }
}

/// A path is a list of subpaths; each subpath is a run of device-space points.
/// Points are transformed into device space as they are appended, matching the
/// canvas spec (the CTM applies at path-construction time, not at fill time).
#[derive(Default)]
struct PathBuilder {
    subpaths: Vec<Vec<(f32, f32)>>,
}

impl PathBuilder {
    fn begin(&mut self) {
        self.subpaths.clear();
    }

    fn move_to(&mut self, x: f32, y: f32) {
        self.subpaths.push(vec![(x, y)]);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        if let Some(last) = self.subpaths.last_mut() {
            last.push((x, y));
        } else {
            self.subpaths.push(vec![(x, y)]);
        }
    }

    fn close(&mut self) {
        let first = self
            .subpaths
            .last()
            .filter(|last| last.len() > 1)
            .and_then(|last| last.first().copied());
        if let Some((x, y)) = first {
            self.line_to(x, y);
        }
    }
}

/// An RGBA8 canvas backing store with a 2D drawing context.
pub struct CanvasSurface {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    state: DrawState,
    stack: Vec<DrawState>,
    path: PathBuilder,
}

impl CanvasSurface {
    /// Create a transparent-black surface. A zero dimension clamps to 1 so the
    /// backing buffer is always addressable.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            width,
            height,
            pixels: vec![0; width as usize * height as usize * 4],
            state: DrawState::default(),
            stack: Vec::new(),
            path: PathBuilder::default(),
        }
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Resize the backing store, resetting it to transparent black and clearing
    /// all drawing state -- this matches setting `canvas.width`/`canvas.height`.
    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        self.width = width;
        self.height = height;
        self.pixels = vec![0; width as usize * height as usize * 4];
        self.state = DrawState::default();
        self.stack.clear();
        self.path = PathBuilder::default();
    }

    /// Borrow the packed RGBA8 backing pixels (row-major, `width*height*4`).
    /// The embedder wraps these in a render `ImageSurface` to composite.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    // -- state --------------------------------------------------------------

    pub fn set_fill_style(&mut self, rgba: [u8; 4]) {
        self.state.fill = rgba;
    }

    #[must_use]
    pub fn fill_style(&self) -> [u8; 4] {
        self.state.fill
    }

    pub fn set_stroke_style(&mut self, rgba: [u8; 4]) {
        self.state.stroke = rgba;
    }

    #[must_use]
    pub fn stroke_style(&self) -> [u8; 4] {
        self.state.stroke
    }

    pub fn set_line_width(&mut self, width: f32) {
        if width > 0.0 && width.is_finite() {
            self.state.line_width = width;
        }
    }

    #[must_use]
    pub fn line_width(&self) -> f32 {
        self.state.line_width
    }

    pub fn set_global_alpha(&mut self, alpha: f32) {
        if alpha.is_finite() {
            self.state.global_alpha = alpha.clamp(0.0, 1.0);
        }
    }

    #[must_use]
    pub fn global_alpha(&self) -> f32 {
        self.state.global_alpha
    }

    pub fn save(&mut self) {
        self.stack.push(self.state);
    }

    pub fn restore(&mut self) {
        if let Some(state) = self.stack.pop() {
            self.state = state;
        }
    }

    // -- transforms ---------------------------------------------------------

    pub fn translate(&mut self, x: f32, y: f32) {
        let t = Affine {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: x,
            f: y,
        };
        self.state.transform = self.state.transform.then(&t);
    }

    pub fn scale(&mut self, x: f32, y: f32) {
        let t = Affine {
            a: x,
            b: 0.0,
            c: 0.0,
            d: y,
            e: 0.0,
            f: 0.0,
        };
        self.state.transform = self.state.transform.then(&t);
    }

    pub fn rotate(&mut self, radians: f32) {
        let (sin, cos) = radians.sin_cos();
        let t = Affine {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            e: 0.0,
            f: 0.0,
        };
        self.state.transform = self.state.transform.then(&t);
    }

    pub fn transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        let t = Affine { a, b, c, d, e, f };
        self.state.transform = self.state.transform.then(&t);
    }

    pub fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.state.transform = Affine { a, b, c, d, e, f };
    }

    pub fn reset_transform(&mut self) {
        self.state.transform = Affine::identity();
    }

    // -- rectangles ---------------------------------------------------------

    /// Clear a rectangle to transparent black, honoring the current transform.
    pub fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let quad = self.transformed_rect(x, y, w, h);
        self.fill_polygon(&[quad.to_vec()], [0, 0, 0, 0], 1.0, true);
    }

    /// Fill a rectangle with the current fill style, honoring the transform.
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let quad = self.transformed_rect(x, y, w, h);
        let fill = self.state.fill;
        let alpha = self.state.global_alpha;
        self.fill_polygon(&[quad.to_vec()], fill, alpha, false);
    }

    /// Stroke a rectangle outline with the current stroke style and line width.
    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let quad = self.transformed_rect(x, y, w, h);
        let edges = [
            (quad[0], quad[1]),
            (quad[1], quad[2]),
            (quad[2], quad[3]),
            (quad[3], quad[0]),
        ];
        let stroke = self.state.stroke;
        let alpha = self.state.global_alpha;
        let device_width = self.device_line_width();
        for (start, end) in edges {
            self.stroke_segment(start, end, stroke, alpha, device_width);
        }
    }

    fn transformed_rect(&self, x: f32, y: f32, w: f32, h: f32) -> [(f32, f32); 4] {
        let t = &self.state.transform;
        [
            t.apply(x, y),
            t.apply(x + w, y),
            t.apply(x + w, y + h),
            t.apply(x, y + h),
        ]
    }

    // -- paths --------------------------------------------------------------

    pub fn begin_path(&mut self) {
        self.path.begin();
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        let (dx, dy) = self.state.transform.apply(x, y);
        self.path.move_to(dx, dy);
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        let (dx, dy) = self.state.transform.apply(x, y);
        self.path.line_to(dx, dy);
    }

    pub fn close_path(&mut self) {
        self.path.close();
    }

    /// Append a rectangle as its own closed subpath (canvas `rect()`).
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.move_to(x, y);
        self.line_to(x + w, y);
        self.line_to(x + w, y + h);
        self.line_to(x, y + h);
        self.close_path();
    }

    /// Append an arc, flattening to line segments in user space before the
    /// transform applies. `anticlockwise` reverses the sweep direction.
    pub fn arc(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        start_angle: f32,
        end_angle: f32,
        anticlockwise: bool,
    ) {
        if radius <= 0.0 || !radius.is_finite() {
            self.line_to(cx, cy);
            return;
        }
        let mut sweep = end_angle - start_angle;
        if anticlockwise {
            if sweep > 0.0 {
                sweep -= std::f32::consts::TAU * (sweep / std::f32::consts::TAU).ceil().max(1.0);
            }
            sweep = sweep.max(-std::f32::consts::TAU);
        } else {
            if sweep < 0.0 {
                sweep += std::f32::consts::TAU * (-sweep / std::f32::consts::TAU).ceil().max(1.0);
            }
            sweep = sweep.min(std::f32::consts::TAU);
        }
        // Flatten to at least 8 segments, more for large arcs, so the polygon
        // fill stays smooth without unbounded work.
        let segments = ((sweep.abs() / std::f32::consts::TAU) * 64.0).ceil() as usize;
        let segments = segments.clamp(8, 256);
        for step in 0..=segments {
            let angle = start_angle + sweep * (step as f32 / segments as f32);
            let x = cx + radius * angle.cos();
            let y = cy + radius * angle.sin();
            if step == 0
                && self
                    .path
                    .subpaths
                    .last()
                    .is_none_or(std::vec::Vec::is_empty)
            {
                self.move_to(x, y);
            } else {
                self.line_to(x, y);
            }
        }
    }

    /// Flatten a quadratic Bezier to line segments through the transform.
    pub fn quadratic_curve_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let Some(&start) = self.path.subpaths.last().and_then(|subpath| subpath.last()) else {
            self.move_to(x, y);
            return;
        };
        let inverse = self.state.transform;
        let (sx, sy) = start;
        // start is device-space; flatten in device space directly by
        // transforming the control/end points too.
        let (dcx, dcy) = inverse.apply(cx, cy);
        let (dx, dy) = inverse.apply(x, y);
        let segments = 24;
        for step in 1..=segments {
            let t = step as f32 / segments as f32;
            let inv = 1.0 - t;
            let px = inv * inv * sx + 2.0 * inv * t * dcx + t * t * dx;
            let py = inv * inv * sy + 2.0 * inv * t * dcy + t * t * dy;
            self.path.line_to(px, py);
        }
    }

    /// Flatten a cubic Bezier to line segments through the transform.
    pub fn bezier_curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        let Some(&start) = self.path.subpaths.last().and_then(|subpath| subpath.last()) else {
            self.move_to(x, y);
            return;
        };
        let (sx, sy) = start;
        let t = self.state.transform;
        let (p1x, p1y) = t.apply(c1x, c1y);
        let (p2x, p2y) = t.apply(c2x, c2y);
        let (ex, ey) = t.apply(x, y);
        let segments = 32;
        for step in 1..=segments {
            let u = step as f32 / segments as f32;
            let inv = 1.0 - u;
            let b0 = inv * inv * inv;
            let b1 = 3.0 * inv * inv * u;
            let b2 = 3.0 * inv * u * u;
            let b3 = u * u * u;
            let px = b0 * sx + b1 * p1x + b2 * p2x + b3 * ex;
            let py = b0 * sy + b1 * p1y + b2 * p2y + b3 * ey;
            self.path.line_to(px, py);
        }
    }

    /// Fill the current path with the fill style using nonzero winding.
    pub fn fill(&mut self) {
        let polygons = self.path.subpaths.clone();
        let fill = self.state.fill;
        let alpha = self.state.global_alpha;
        self.fill_polygon(&polygons, fill, alpha, false);
    }

    /// Stroke the current path with the stroke style and line width.
    pub fn stroke(&mut self) {
        let polygons = self.path.subpaths.clone();
        let stroke = self.state.stroke;
        let alpha = self.state.global_alpha;
        let device_width = self.device_line_width();
        for subpath in &polygons {
            for window in subpath.windows(2) {
                self.stroke_segment(window[0], window[1], stroke, alpha, device_width);
            }
        }
    }

    fn device_line_width(&self) -> f32 {
        // Approximate the transformed line width by the mean axis scale.
        let t = &self.state.transform;
        let scale_x = (t.a * t.a + t.b * t.b).sqrt();
        let scale_y = (t.c * t.c + t.d * t.d).sqrt();
        (self.state.line_width * (scale_x + scale_y) * 0.5).max(1.0)
    }

    // -- images -------------------------------------------------------------

    /// Draw a source RGBA image, scaling from its intrinsic size into the
    /// destination rectangle (transformed). This covers the 3-, 5-, and 9-arg
    /// `drawImage` forms once the caller resolves the source sub-rectangle.
    // The source and destination sub-rectangles are the 9-argument canvas
    // drawImage signature; grouping them into structs would only obscure the
    // 1:1 mapping to the spec parameters.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image(
        &mut self,
        src: &[u8],
        src_width: u32,
        src_height: u32,
        src_x: f32,
        src_y: f32,
        src_w: f32,
        src_h: f32,
        dst_x: f32,
        dst_y: f32,
        dst_w: f32,
        dst_h: f32,
    ) {
        if src_width == 0 || src_height == 0 || src.len() < (src_width * src_height * 4) as usize {
            return;
        }
        if dst_w <= 0.0 || dst_h <= 0.0 {
            return;
        }
        // Rasterize the destination as an axis-aligned box in device space.
        // Rotation/shear degrade to the bounding box; the common MotionMark
        // path uses translate/scale only, which this reproduces exactly.
        let corners = self.transformed_rect(dst_x, dst_y, dst_w, dst_h);
        let (min_x, min_y, max_x, max_y) = bounding_box(&corners);
        let x0 = min_x.floor().max(0.0) as u32;
        let y0 = min_y.floor().max(0.0) as u32;
        let x1 = (max_x.ceil() as i64).clamp(0, self.width as i64) as u32;
        let y1 = (max_y.ceil() as i64).clamp(0, self.height as i64) as u32;
        let device_w = (max_x - min_x).max(1.0);
        let device_h = (max_y - min_y).max(1.0);
        let alpha = self.state.global_alpha;
        for y in y0..y1 {
            let v = (y as f32 + 0.5 - min_y) / device_h;
            let sy = src_y + v * src_h;
            let sample_y = (sy.floor() as i64).clamp(0, src_height as i64 - 1) as u32;
            for x in x0..x1 {
                let u = (x as f32 + 0.5 - min_x) / device_w;
                let sx = src_x + u * src_w;
                let sample_x = (sx.floor() as i64).clamp(0, src_width as i64 - 1) as u32;
                let si = ((sample_y * src_width + sample_x) * 4) as usize;
                let rgba = [src[si], src[si + 1], src[si + 2], src[si + 3]];
                self.blend_pixel(x, y, rgba, alpha);
            }
        }
    }

    // -- pixel access -------------------------------------------------------

    /// Read a rectangle of RGBA pixels. Out-of-bounds pixels read as
    /// transparent black, matching `getImageData`.
    #[must_use]
    pub fn get_image_data(&self, x: i32, y: i32, w: u32, h: u32) -> Vec<u8> {
        let mut out = vec![0u8; w as usize * h as usize * 4];
        for row in 0..h {
            let sy = y + row as i32;
            if sy < 0 || sy >= self.height as i32 {
                continue;
            }
            for col in 0..w {
                let sx = x + col as i32;
                if sx < 0 || sx >= self.width as i32 {
                    continue;
                }
                let si = ((sy as u32 * self.width + sx as u32) * 4) as usize;
                let di = ((row * w + col) * 4) as usize;
                out[di..di + 4].copy_from_slice(&self.pixels[si..si + 4]);
            }
        }
        out
    }

    /// Write a rectangle of RGBA pixels, replacing (not blending) the target,
    /// matching `putImageData`.
    pub fn put_image_data(&mut self, data: &[u8], x: i32, y: i32, w: u32, h: u32) {
        if data.len() < (w * h * 4) as usize {
            return;
        }
        for row in 0..h {
            let dy = y + row as i32;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            for col in 0..w {
                let dx = x + col as i32;
                if dx < 0 || dx >= self.width as i32 {
                    continue;
                }
                let si = ((row * w + col) * 4) as usize;
                let di = ((dy as u32 * self.width + dx as u32) * 4) as usize;
                self.pixels[di..di + 4].copy_from_slice(&data[si..si + 4]);
            }
        }
    }

    // -- rasterization core -------------------------------------------------

    /// Fill polygons with nonzero winding. When `replace` is set the target is
    /// overwritten (used by `clearRect`); otherwise source-over blending runs.
    fn fill_polygon(
        &mut self,
        polygons: &[Vec<(f32, f32)>],
        rgba: [u8; 4],
        alpha: f32,
        replace: bool,
    ) {
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        for polygon in polygons {
            for &(_, py) in polygon {
                min_y = min_y.min(py);
                max_y = max_y.max(py);
            }
        }
        if !min_y.is_finite() || !max_y.is_finite() {
            return;
        }
        let y_start = (min_y.floor().max(0.0)) as u32;
        let y_end = (max_y.ceil().clamp(0.0, self.height as f32)) as u32;
        let mut crossings: Vec<(f32, i32)> = Vec::new();
        for y in y_start..y_end {
            let sample_y = y as f32 + 0.5;
            crossings.clear();
            for polygon in polygons {
                let count = polygon.len();
                if count < 2 {
                    continue;
                }
                for i in 0..count {
                    let (x0, y0) = polygon[i];
                    let (x1, y1) = polygon[(i + 1) % count];
                    if (y0 <= sample_y && y1 > sample_y) || (y1 <= sample_y && y0 > sample_y) {
                        let t = (sample_y - y0) / (y1 - y0);
                        let x = x0 + t * (x1 - x0);
                        let winding = if y1 > y0 { 1 } else { -1 };
                        crossings.push((x, winding));
                    }
                }
            }
            if crossings.is_empty() {
                continue;
            }
            crossings.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut winding = 0;
            for pair in crossings.windows(2) {
                winding += pair[0].1;
                if winding != 0 {
                    let x_start = (pair[0].0.ceil().max(0.0)) as u32;
                    let x_end = (pair[1].0.ceil().clamp(0.0, self.width as f32)) as u32;
                    for x in x_start..x_end {
                        if replace {
                            self.set_pixel(x, y, rgba);
                        } else {
                            self.blend_pixel(x, y, rgba, alpha);
                        }
                    }
                }
            }
        }
    }

    fn stroke_segment(
        &mut self,
        start: (f32, f32),
        end: (f32, f32),
        rgba: [u8; 4],
        alpha: f32,
        width: f32,
    ) {
        // Expand the segment into a quad of the given width and fill it.
        let (dx, dy) = (end.0 - start.0, end.1 - start.1);
        let len = (dx * dx + dy * dy).sqrt();
        if len < f32::EPSILON {
            return;
        }
        let (nx, ny) = (-dy / len, dx / len);
        let half = width * 0.5;
        let quad = vec![
            (start.0 + nx * half, start.1 + ny * half),
            (end.0 + nx * half, end.1 + ny * half),
            (end.0 - nx * half, end.1 - ny * half),
            (start.0 - nx * half, start.1 - ny * half),
        ];
        self.fill_polygon(&[quad], rgba, alpha, false);
    }

    fn set_pixel(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let i = ((y * self.width + x) * 4) as usize;
        self.pixels[i..i + 4].copy_from_slice(&rgba);
    }

    fn blend_pixel(&mut self, x: u32, y: u32, rgba: [u8; 4], global_alpha: f32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let src_alpha = (f32::from(rgba[3]) / 255.0) * global_alpha;
        if src_alpha <= 0.0 {
            return;
        }
        let i = ((y * self.width + x) * 4) as usize;
        let dst = &self.pixels[i..i + 4];
        let dst_alpha = f32::from(dst[3]) / 255.0;
        let out_alpha = src_alpha + dst_alpha * (1.0 - src_alpha);
        if out_alpha <= 0.0 {
            self.pixels[i..i + 4].copy_from_slice(&[0, 0, 0, 0]);
            return;
        }
        let mut out = [0u8; 4];
        for channel in 0..3 {
            let src_c = f32::from(rgba[channel]) / 255.0;
            let dst_c = f32::from(dst[channel]) / 255.0;
            let blended = (src_c * src_alpha + dst_c * dst_alpha * (1.0 - src_alpha)) / out_alpha;
            out[channel] = (blended * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        out[3] = (out_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        self.pixels[i..i + 4].copy_from_slice(&out);
    }
}

fn bounding_box(points: &[(f32, f32)]) -> (f32, f32, f32, f32) {
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for &(x, y) in points {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    (min_x, min_y, max_x, max_y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(surface: &CanvasSurface, x: u32, y: u32) -> [u8; 4] {
        let data = surface.get_image_data(x as i32, y as i32, 1, 1);
        [data[0], data[1], data[2], data[3]]
    }

    #[test]
    fn fill_rect_paints_opaque_region() {
        let mut surface = CanvasSurface::new(10, 10);
        surface.set_fill_style([255, 0, 0, 255]);
        surface.fill_rect(2.0, 2.0, 4.0, 4.0);
        assert_eq!(pixel(&surface, 3, 3), [255, 0, 0, 255]);
        assert_eq!(pixel(&surface, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&surface, 6, 6), [0, 0, 0, 0]);
    }

    #[test]
    fn clear_rect_restores_transparency() {
        let mut surface = CanvasSurface::new(8, 8);
        surface.set_fill_style([0, 0, 255, 255]);
        surface.fill_rect(0.0, 0.0, 8.0, 8.0);
        surface.clear_rect(2.0, 2.0, 3.0, 3.0);
        assert_eq!(pixel(&surface, 3, 3), [0, 0, 0, 0]);
        assert_eq!(pixel(&surface, 0, 0), [0, 0, 255, 255]);
    }

    #[test]
    fn translate_offsets_fill() {
        let mut surface = CanvasSurface::new(12, 12);
        surface.set_fill_style([0, 255, 0, 255]);
        surface.translate(4.0, 4.0);
        surface.fill_rect(0.0, 0.0, 2.0, 2.0);
        assert_eq!(pixel(&surface, 5, 5), [0, 255, 0, 255]);
        assert_eq!(pixel(&surface, 1, 1), [0, 0, 0, 0]);
    }

    #[test]
    fn path_triangle_fills_interior() {
        let mut surface = CanvasSurface::new(16, 16);
        surface.set_fill_style([255, 255, 0, 255]);
        surface.begin_path();
        surface.move_to(2.0, 2.0);
        surface.line_to(14.0, 2.0);
        surface.line_to(2.0, 14.0);
        surface.close_path();
        surface.fill();
        // Interior near the right-angle corner is inside the triangle.
        assert_eq!(pixel(&surface, 4, 4), [255, 255, 0, 255]);
        // The far corner opposite the hypotenuse is outside.
        assert_eq!(pixel(&surface, 13, 13), [0, 0, 0, 0]);
    }

    #[test]
    fn half_alpha_fill_blends_with_backdrop() {
        let mut surface = CanvasSurface::new(4, 4);
        surface.set_fill_style([255, 255, 255, 255]);
        surface.fill_rect(0.0, 0.0, 4.0, 4.0);
        surface.set_fill_style([255, 0, 0, 128]);
        surface.fill_rect(0.0, 0.0, 4.0, 4.0);
        let px = pixel(&surface, 1, 1);
        assert_eq!(px[0], 255);
        // Green channel blends white(255) under red(0) at ~50% -> ~127.
        assert!((px[1] as i32 - 127).abs() <= 2, "green={}", px[1]);
        assert_eq!(px[3], 255);
    }

    #[test]
    fn put_and_get_image_data_round_trip() {
        let mut surface = CanvasSurface::new(6, 6);
        let data: Vec<u8> = (0..(2 * 2 * 4)).map(|i| (i * 3) as u8).collect();
        surface.put_image_data(&data, 1, 1, 2, 2);
        let read = surface.get_image_data(1, 1, 2, 2);
        assert_eq!(read, data);
    }

    #[test]
    fn draw_image_scales_source_into_destination() {
        let mut surface = CanvasSurface::new(8, 8);
        // 1x1 opaque magenta source scaled into a 4x4 box.
        let src = [255u8, 0, 255, 255];
        surface.draw_image(&src, 1, 1, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 4.0, 4.0);
        assert_eq!(pixel(&surface, 3, 3), [255, 0, 255, 255]);
        assert_eq!(pixel(&surface, 0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn pixels_match_backing_dimensions() {
        let mut surface = CanvasSurface::new(5, 7);
        surface.set_fill_style([1, 2, 3, 255]);
        surface.fill_rect(0.0, 0.0, 5.0, 7.0);
        assert_eq!(surface.width(), 5);
        assert_eq!(surface.height(), 7);
        assert_eq!(surface.pixels().len(), 5 * 7 * 4);
        assert_eq!(&surface.pixels()[0..4], &[1, 2, 3, 255]);
    }

    #[test]
    fn save_restore_isolates_state() {
        let mut surface = CanvasSurface::new(4, 4);
        surface.set_fill_style([10, 20, 30, 255]);
        surface.save();
        surface.set_fill_style([200, 100, 50, 255]);
        surface.restore();
        surface.fill_rect(0.0, 0.0, 1.0, 1.0);
        assert_eq!(pixel(&surface, 0, 0), [10, 20, 30, 255]);
    }
}
