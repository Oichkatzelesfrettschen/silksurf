use silksurf_css::Color;
use silksurf_layout::Rect;
use silksurf_render::{DisplayItem, DisplayList, rasterize_skia, rasterize_skia_into};

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, width: w, height: h }
}

fn color(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

fn empty_list() -> DisplayList {
    DisplayList { items: vec![], tiles: None }
}

/// rasterize_skia produces a buffer of exactly width * height * 4 bytes.
#[test]
fn skia_buffer_size() {
    let buf = rasterize_skia(&empty_list(), 32, 16);
    assert_eq!(buf.len(), 32 * 16 * 4);
}

/// An empty display list produces an all-white opaque background.
#[test]
fn skia_white_background() {
    let buf = rasterize_skia(&empty_list(), 8, 8);
    for chunk in buf.chunks_exact(4) {
        assert_eq!(chunk, [255, 255, 255, 255], "background pixel not white");
    }
}

/// A SolidColor item paints the interior pixel with the expected color.
/// For alpha == 255, premultiplied and straight RGBA are identical.
#[test]
fn skia_solid_color_fills_interior() {
    let red = color(200, 0, 0);
    let dl = DisplayList {
        items: vec![DisplayItem::SolidColor {
            rect: rect(10.0, 10.0, 10.0, 10.0),
            color: red,
        }],
        tiles: None,
    };
    let buf = rasterize_skia(&dl, 32, 32);

    // Center pixel of the rect (15, 15) must be red.
    let off = (15 * 32 + 15) * 4;
    assert_eq!(buf[off], 200, "R channel");
    assert_eq!(buf[off + 1], 0, "G channel");
    assert_eq!(buf[off + 2], 0, "B channel");
    assert_eq!(buf[off + 3], 255, "A channel");

    // Pixel outside the rect at (5, 5) must still be white.
    let off2 = (5 * 32 + 5) * 4;
    assert_eq!(&buf[off2..off2 + 4], &[255, 255, 255, 255]);
}

/// rasterize_skia_into reuses the buffer when dimensions are unchanged.
#[test]
fn skia_into_reuses_buffer() {
    let dl = empty_list();
    let mut buf = Vec::new();
    rasterize_skia_into(&dl, 16, 16, &mut buf);
    assert_eq!(buf.len(), 16 * 16 * 4);
    let ptr_before = buf.as_ptr();
    rasterize_skia_into(&dl, 16, 16, &mut buf);
    // Same capacity -- no reallocation.
    assert_eq!(buf.as_ptr(), ptr_before);
}

/// A RoundedRect item paints the center of the bounding box.
#[test]
fn skia_rounded_rect_fills_center() {
    let blue = color(0, 0, 200);
    let dl = DisplayList {
        items: vec![DisplayItem::RoundedRect {
            rect: rect(4.0, 4.0, 24.0, 24.0),
            radii: [4.0, 4.0, 4.0, 4.0],
            color: blue,
        }],
        tiles: None,
    };
    let buf = rasterize_skia(&dl, 32, 32);

    // Center of the rounded rect at (16, 16).
    let off = (16 * 32 + 16) * 4;
    assert_eq!(buf[off], 0, "R");
    assert_eq!(buf[off + 1], 0, "G");
    assert_eq!(buf[off + 2], 200, "B");
    assert_eq!(buf[off + 3], 255, "A");
}

/// A RoundedRect with zero radii behaves identically to a SolidColor rect.
#[test]
fn skia_rounded_rect_zero_radii_matches_solid() {
    let green = color(0, 180, 0);
    let r = rect(8.0, 8.0, 16.0, 16.0);

    let dl_solid = DisplayList {
        items: vec![DisplayItem::SolidColor { rect: r, color: green }],
        tiles: None,
    };
    let dl_rounded = DisplayList {
        items: vec![DisplayItem::RoundedRect {
            rect: r,
            radii: [0.0; 4],
            color: green,
        }],
        tiles: None,
    };

    let buf_solid = rasterize_skia(&dl_solid, 32, 32);
    let buf_rounded = rasterize_skia(&dl_rounded, 32, 32);

    // Interior pixels must match; anti-aliased edges may differ by rounding.
    let cx = 16usize;
    let cy = 16usize;
    let off = (cy * 32 + cx) * 4;
    assert_eq!(&buf_solid[off..off + 4], &buf_rounded[off..off + 4]);
}
