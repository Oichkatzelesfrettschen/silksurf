/*
 * tests/batched.rs -- integration tests for DisplayListBatched.
 *
 * WHY: Verify that the batched rasterization path produces pixel-identical
 * output to the scalar rasterize_damage path in lib.rs. Any divergence
 * indicates a semantics bug in the type-batched implementation.
 *
 * WHAT: Three tests:
 *   round_trip_solid_colors -- 2 SolidColor items, full viewport damage.
 *   round_trip_mixed        -- SolidColor + Text items, full viewport damage.
 *   item_count_correct      -- item_count() matches total pushed items.
 *
 * HOW: Construct a DisplayList, rasterize with the scalar path to get the
 * reference buffer, then convert to DisplayListBatched and rasterize_damage
 * with the same arguments. Assert buffers are equal.
 *
 * NOTE: Text items currently stub-rasterize as colored rects (no glyph
 * rasterizer yet). Both paths behave identically for that stub, so the
 * round-trip comparison is valid.
 *
 * Gated on feature "batched-raster" so the test file is absent from normal
 * CI builds that do not opt in.
 */

#![cfg(feature = "batched-raster")]

use silksurf_css::Color;
use silksurf_dom::NodeId;
use silksurf_layout::Rect;
use silksurf_render::{
    DisplayItem, DisplayList, display_list_batched::DisplayListBatched, rasterize_damage,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a Rect with integer coordinates for readability in tests.
fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

/// Build an opaque Color.
fn color(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

/// Full-viewport damage rect.
fn full_damage(width: u32, height: u32) -> Rect {
    rect(0.0, 0.0, width as f32, height as f32)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/*
 * round_trip_solid_colors -- two SolidColor items, full viewport damage.
 *
 * WHY: The simplest case -- no Text items -- exercises the solid_colors
 * sub-list in isolation. Ensures partition and fill_rect semantics match.
 *
 * WHAT: A 4x4 viewport with two non-overlapping 2x2 rects in distinct colors.
 * The scalar path and the batched path must produce byte-identical output.
 *
 * HOW: Build DisplayList, rasterize_damage (scalar), convert to
 * DisplayListBatched, rasterize_damage (batched), compare.
 */
#[test]
fn round_trip_solid_colors() {
    const W: u32 = 4;
    const H: u32 = 4;

    let list = DisplayList {
        items: vec![
            DisplayItem::SolidColor {
                rect: rect(0.0, 0.0, 2.0, 2.0),
                color: color(200, 10, 10),
            },
            DisplayItem::SolidColor {
                rect: rect(2.0, 2.0, 2.0, 2.0),
                color: color(10, 200, 10),
            },
        ],
        tiles: None,
    };

    let damage = full_damage(W, H);
    let reference = rasterize_damage(&list, W, H, damage);

    let batched = DisplayListBatched::from_display_list(&list);
    let result = batched.rasterize_damage(W, H, damage);

    assert_eq!(
        reference, result,
        "batched output must be pixel-identical to scalar output (solid_colors only)"
    );
}

/*
 * round_trip_mixed -- SolidColor and Text items, full viewport damage.
 *
 * WHY: Exercises both sub-lists together. Confirms that the cross-type
 * painting order (solid_colors then texts) produces the same pixels as
 * the original in-order iteration when rects do not overlap.
 *
 * WHAT: 6x4 viewport. First two items are SolidColor rects in the left half;
 * the third is a Text item in the right half. NodeId::from_raw(0) is used
 * as a valid sentinel -- the batched path stores it but does not dereference
 * it during rasterization (glyph rasterizer is future work).
 *
 * HOW: Same round-trip pattern as round_trip_solid_colors.
 */
#[test]
fn round_trip_mixed() {
    const W: u32 = 6;
    const H: u32 = 4;

    // NodeId::from_raw is documented for testing/FFI use.
    let fake_node = NodeId::from_raw(0);

    let list = DisplayList {
        items: vec![
            DisplayItem::SolidColor {
                rect: rect(0.0, 0.0, 3.0, 2.0),
                color: color(80, 40, 200),
            },
            DisplayItem::SolidColor {
                rect: rect(0.0, 2.0, 3.0, 2.0),
                color: color(40, 200, 80),
            },
            DisplayItem::Text {
                rect: rect(3.0, 0.0, 3.0, 4.0),
                node: fake_node,
                text_len: 5,
                text: "hello".to_string(),
                font_size: 16.0,
                color: color(200, 200, 40),
            },
        ],
        tiles: None,
    };

    let damage = full_damage(W, H);
    let reference = rasterize_damage(&list, W, H, damage);

    let batched = DisplayListBatched::from_display_list(&list);
    let result = batched.rasterize_damage(W, H, damage);

    assert_eq!(
        reference, result,
        "batched output must be pixel-identical to scalar output (mixed items)"
    );
}

/*
 * item_count_correct -- item_count() equals the total number of items pushed.
 *
 * WHY: item_count() is the public API for "how many logical paint commands
 * does this batched list represent". It must equal DisplayList.items.len()
 * for the same source list.
 *
 * WHAT: Push 3 SolidColor and 2 Text items (5 total). Verify item_count() == 5.
 * Also verify the sub-list lengths individually.
 */
#[test]
fn item_count_correct() {
    let fake_node = NodeId::from_raw(0);

    let list = DisplayList {
        items: vec![
            DisplayItem::SolidColor {
                rect: rect(0.0, 0.0, 10.0, 10.0),
                color: color(255, 0, 0),
            },
            DisplayItem::Text {
                rect: rect(10.0, 0.0, 10.0, 10.0),
                node: fake_node,
                text_len: 3,
                text: "abc".to_string(),
                font_size: 16.0,
                color: color(0, 255, 0),
            },
            DisplayItem::SolidColor {
                rect: rect(20.0, 0.0, 10.0, 10.0),
                color: color(0, 0, 255),
            },
            DisplayItem::Text {
                rect: rect(30.0, 0.0, 10.0, 10.0),
                node: fake_node,
                text_len: 7,
                text: "abcdefg".to_string(),
                font_size: 16.0,
                color: color(128, 128, 0),
            },
            DisplayItem::SolidColor {
                rect: rect(40.0, 0.0, 10.0, 10.0),
                color: color(0, 128, 128),
            },
        ],
        tiles: None,
    };

    let batched = DisplayListBatched::from_display_list(&list);

    assert_eq!(
        batched.solid_colors.len(),
        3,
        "solid_colors sub-list must contain exactly 3 items"
    );
    assert_eq!(
        batched.texts.len(),
        2,
        "texts sub-list must contain exactly 2 items"
    );
    assert_eq!(
        batched.item_count(),
        5,
        "item_count() must equal total items in the source DisplayList"
    );
    assert_eq!(
        batched.item_count(),
        list.items.len(),
        "item_count() must equal DisplayList.items.len() for the same source"
    );
}
