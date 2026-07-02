use silksurf_css::Color;
use silksurf_layout::Rect;
use silksurf_render::{
    DamagePixelRect, DamageScratch, DisplayItem, DisplayList, rasterize_skia,
    rasterize_skia_damage_into, rasterize_skia_into, rasterize_skia_translated_damage_into,
};

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

fn color(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

fn empty_list() -> DisplayList {
    DisplayList {
        items: vec![],
        tiles: None,
    }
}

/// `rasterize_skia` produces a buffer of exactly width * height * 4 bytes.
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

/// A `SolidColor` item paints the interior pixel with the expected color.
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

/// `rasterize_skia_into` reuses the buffer when dimensions are unchanged.
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

#[test]
fn skia_damage_updates_only_dirty_rectangle() {
    let blue = color(0, 0, 200);
    let red = color(220, 0, 0);
    let base = DisplayList {
        items: vec![DisplayItem::SolidColor {
            rect: rect(0.0, 0.0, 16.0, 16.0),
            color: blue,
        }],
        tiles: None,
    };
    let updated = DisplayList {
        items: vec![
            DisplayItem::SolidColor {
                rect: rect(0.0, 0.0, 16.0, 16.0),
                color: blue,
            },
            DisplayItem::SolidColor {
                rect: rect(4.0, 4.0, 4.0, 4.0),
                color: red,
            },
        ],
        tiles: None,
    };

    let mut buf = Vec::new();
    let mut scratch = DamageScratch::default();
    rasterize_skia_into(&base, 16, 16, &mut buf);
    rasterize_skia_damage_into(
        &updated,
        16,
        16,
        rect(4.0, 4.0, 4.0, 4.0),
        &mut buf,
        &mut scratch,
    );

    let dirty_pixel = (5 * 16 + 5) * 4;
    assert_eq!(&buf[dirty_pixel..dirty_pixel + 4], &[220, 0, 0, 255]);
    assert_eq!(
        scratch.last_damage(),
        Some(DamagePixelRect {
            x: 4,
            y: 4,
            width: 4,
            height: 4,
        })
    );

    let retained_pixel = (1 * 16 + 1) * 4;
    assert_eq!(&buf[retained_pixel..retained_pixel + 4], &[0, 0, 200, 255]);
}

#[test]
fn skia_translated_damage_maps_document_rect_into_viewport() {
    let blue = color(0, 0, 200);
    let red = color(220, 0, 0);
    let dl = DisplayList {
        items: vec![
            DisplayItem::SolidColor {
                rect: rect(0.0, 10.0, 16.0, 16.0),
                color: blue,
            },
            DisplayItem::SolidColor {
                rect: rect(4.0, 14.0, 4.0, 4.0),
                color: red,
            },
        ],
        tiles: None,
    };

    let mut buf = vec![17; 16 * 16 * 4];
    let mut scratch = DamageScratch::default();
    rasterize_skia_translated_damage_into(
        &dl,
        16,
        16,
        rect(4.0, 4.0, 4.0, 4.0),
        rect(4.0, 14.0, 4.0, 4.0),
        (0.0, -10.0),
        &mut buf,
        &mut scratch,
    );

    let dirty_pixel = (5 * 16 + 5) * 4;
    assert_eq!(&buf[dirty_pixel..dirty_pixel + 4], &[220, 0, 0, 255]);
    assert_eq!(
        scratch.last_damage(),
        Some(DamagePixelRect {
            x: 4,
            y: 4,
            width: 4,
            height: 4,
        })
    );

    let retained_pixel = (1 * 16 + 1) * 4;
    assert_eq!(&buf[retained_pixel..retained_pixel + 4], &[17, 17, 17, 17]);
}

#[test]
fn skia_damage_reuses_scratch_for_same_size() {
    let dl = empty_list();
    let mut buf = Vec::new();
    let mut scratch = DamageScratch::default();
    rasterize_skia_into(&dl, 16, 16, &mut buf);
    rasterize_skia_damage_into(
        &dl,
        16,
        16,
        rect(2.0, 2.0, 4.0, 4.0),
        &mut buf,
        &mut scratch,
    );
    let ptr_before = scratch.pixel_ptr();
    rasterize_skia_damage_into(
        &dl,
        16,
        16,
        rect(8.0, 8.0, 4.0, 4.0),
        &mut buf,
        &mut scratch,
    );

    assert_eq!(scratch.pixel_ptr(), ptr_before);
}

#[test]
fn skia_damage_microbench_reports_cost() {
    let background = color(250, 250, 250);
    let highlight = color(40, 100, 220);
    let dl = DisplayList {
        items: vec![
            DisplayItem::SolidColor {
                rect: rect(0.0, 0.0, 1280.0, 800.0),
                color: background,
            },
            DisplayItem::SolidColor {
                rect: rect(96.0, 96.0, 96.0, 32.0),
                color: highlight,
            },
        ],
        tiles: None,
    };
    let damage = rect(96.0, 96.0, 96.0, 32.0);
    let iterations = 200_u32;
    let mut full_buf = Vec::new();
    let full_start = std::time::Instant::now();
    for _ in 0..iterations {
        rasterize_skia_into(&dl, 1280, 800, &mut full_buf);
    }
    let full_avg = full_start.elapsed() / iterations;

    let mut damage_buf = Vec::new();
    let mut scratch = DamageScratch::default();
    rasterize_skia_into(&dl, 1280, 800, &mut damage_buf);
    let damage_start = std::time::Instant::now();
    for _ in 0..iterations {
        rasterize_skia_damage_into(&dl, 1280, 800, damage, &mut damage_buf, &mut scratch);
    }
    let damage_avg = damage_start.elapsed() / iterations;

    eprintln!("[SilkSurf] skia damage avg: {damage_avg:?}; full avg: {full_avg:?}");
    assert!(
        damage_avg < full_avg,
        "damage raster should be faster than full raster"
    );
}

/// A `RoundedRect` item paints the center of the bounding box.
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

/// A `RoundedRect` with zero radii behaves identically to a `SolidColor` rect.
#[test]
fn skia_rounded_rect_zero_radii_matches_solid() {
    let green = color(0, 180, 0);
    let r = rect(8.0, 8.0, 16.0, 16.0);

    let dl_solid = DisplayList {
        items: vec![DisplayItem::SolidColor {
            rect: r,
            color: green,
        }],
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
