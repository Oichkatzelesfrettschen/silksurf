use silksurf_css::Color;
use silksurf_text::{measure_text, rasterize_glyphs};

/// Empty string returns zero dimensions without panicking.
#[test]
fn measure_empty_text_is_zero() {
    let (w, h) = measure_text("", 16.0, None);
    assert_eq!(w, 0.0);
    assert_eq!(h, 0.0);
}

/// Non-empty text has positive width and height.
#[test]
fn measure_nonempty_text_is_positive() {
    let (w, h) = measure_text("Hello", 16.0, None);
    assert!(w > 0.0, "expected positive width, got {w}");
    assert!(h > 0.0, "expected positive height, got {h}");
}

/// Larger font size produces greater or equal dimensions.
#[test]
fn measure_larger_font_gives_larger_dims() {
    let (w16, h16) = measure_text("Hi", 16.0, None);
    let (w32, h32) = measure_text("Hi", 32.0, None);
    assert!(
        w32 >= w16,
        "width at 32px ({w32}) must be >= width at 16px ({w16})"
    );
    assert!(
        h32 >= h16,
        "height at 32px ({h32}) must be >= height at 16px ({h16})"
    );
}

/// Max-width constraint causes text to wrap, producing more height.
#[test]
fn measure_narrow_max_width_increases_height() {
    let (_, h_unconstrained) = measure_text("Hello World", 16.0, None);
    let (_, h_constrained) = measure_text("Hello World", 16.0, Some(1.0));
    assert!(
        h_constrained >= h_unconstrained,
        "narrow max_width should increase height (wrapping): {h_constrained} vs {h_unconstrained}"
    );
}

/// rasterize_glyphs does not panic on empty text.
#[test]
fn rasterize_glyphs_empty_text_is_noop() {
    let mut buf = vec![0xffu8; 32 * 32 * 4];
    let color = Color { r: 0, g: 0, b: 0, a: 255 };
    let Some(mut pixmap) = tiny_skia::PixmapMut::from_bytes(&mut buf, 32, 32) else {
        panic!("could not create pixmap");
    };
    rasterize_glyphs("", 16.0, color, &mut pixmap, (0.0, 0.0));
}

/// rasterize_glyphs draws at least one non-white pixel when given opaque text.
#[test]
fn rasterize_glyphs_marks_pixels() {
    let mut buf = vec![0xffu8; 64 * 64 * 4];
    let color = Color { r: 0, g: 0, b: 0, a: 255 };
    {
        let Some(mut pixmap) = tiny_skia::PixmapMut::from_bytes(&mut buf, 64, 64) else {
            panic!("could not create pixmap");
        };
        rasterize_glyphs("X", 16.0, color, &mut pixmap, (4.0, 4.0));
    }
    // At least one pixel should differ from the all-white background.
    let has_non_white = buf
        .chunks_exact(4)
        .any(|p| p[0] != 255 || p[1] != 255 || p[2] != 255);
    assert!(has_non_white, "expected glyphs to modify at least one pixel");
}
