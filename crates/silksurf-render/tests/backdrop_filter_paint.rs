use silksurf_css::{Color, FilterFunction, FilterList};
use silksurf_layout::Rect;
use silksurf_render::{DisplayItem, DisplayList, rasterize_damage, rasterize_skia};

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

/// A black bar across the upper half, then a backdrop-filter over the middle.
///
/// The bar paints first, so the filter item that follows reads it as its
/// backdrop, which is the ordering `build_display_list_for_box` produces by
/// pushing the filter ahead of everything the element paints for itself.
fn list_with(filters: FilterList) -> DisplayList {
    DisplayList {
        items: vec![
            DisplayItem::SolidColor {
                rect: rect(0.0, 0.0, 64.0, 32.0),
                color: Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                },
            },
            DisplayItem::BackdropFilter {
                rect: rect(16.0, 16.0, 32.0, 32.0),
                radii: [0.0; 4],
                filters,
            },
        ],
        tiles: None,
    }
}

fn red_at(buffer: &[u8], x: u32, y: u32) -> u8 {
    buffer[((y * 64 + x) * 4) as usize]
}

/// The blur pulls the white lower half up across the step at y = 32, so the
/// element's own region no longer holds the hard edge the bar painted.
#[test]
fn a_backdrop_filter_item_blurs_what_the_list_painted_before_it() {
    let mut filters = FilterList::new();
    filters.push(FilterFunction::Blur(4.0));
    let buffer = rasterize_skia(&list_with(filters), 64, 64);

    // Inside the element, the step at y = 32 has spread in both directions.
    assert!(red_at(&buffer, 32, 30) > 0, "above the step stayed black");
    assert!(red_at(&buffer, 32, 33) < 255, "below the step stayed white");

    // Outside the element the bar keeps its hard edge.
    assert_eq!(red_at(&buffer, 4, 30), 0, "bar outside the element");
    assert_eq!(
        red_at(&buffer, 4, 33),
        255,
        "background outside the element"
    );
}

/// The scalar rasterizer reaches the same stage, because both paths own a
/// premultiplied RGBA8 buffer.
#[test]
fn the_scalar_rasterizer_applies_the_same_stage() {
    let mut filters = FilterList::new();
    filters.push(FilterFunction::Invert(1.0));
    let buffer = rasterize_damage(&list_with(filters), 64, 64, rect(0.0, 0.0, 64.0, 64.0));

    // The bar is black under the element and inverts to white.
    assert_eq!(red_at(&buffer, 32, 20), 255, "inverted bar");
    // Outside the element the bar stays black.
    assert_eq!(red_at(&buffer, 4, 20), 0, "bar outside the element");
}

/// An empty pipeline leaves the frame identical to one carrying no item at
/// all, so `backdrop-filter: none` costs nothing downstream of the cascade.
#[test]
fn an_empty_pipeline_matches_a_list_without_the_item() {
    let filtered = rasterize_skia(&list_with(FilterList::new()), 64, 64);
    let mut plain = list_with(FilterList::new());
    plain.items.truncate(1);
    assert_eq!(filtered, rasterize_skia(&plain, 64, 64));
}

/// Damage culling bounds a backdrop filter by the region it samples, not by
/// the element it writes.
///
/// `rasterize_damage` clears to white and skips items that miss the damage
/// rect. The stage samples three standard deviations past the element, so an
/// item living only in that margin has to survive the cull; culling it would
/// leave the margin white and the blur would pull that white inward as a
/// bright fringe around the element.
#[test]
fn partial_damage_keeps_the_items_the_filter_samples() {
    let mut filters = FilterList::new();
    filters.push(FilterFunction::Blur(6.0));
    let element = rect(24.0, 24.0, 16.0, 16.0);
    // A black stripe entirely outside the element, inside the sampled margin.
    let list = DisplayList {
        items: vec![
            DisplayItem::SolidColor {
                rect: rect(0.0, 4.0, 64.0, 12.0),
                color: Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                },
            },
            DisplayItem::BackdropFilter {
                rect: element,
                radii: [0.0; 4],
                filters,
            },
        ],
        tiles: None,
    };
    let full = rasterize_damage(&list, 64, 64, rect(0.0, 0.0, 64.0, 64.0));
    let partial = rasterize_damage(&list, 64, 64, element);

    // The stripe darkens the element's top edge, and both damage rects agree.
    assert!(
        red_at(&full, 32, 24) < 255,
        "the sampled stripe never reached the element"
    );
    for y in 24..40 {
        for x in 24..40 {
            assert_eq!(
                red_at(&partial, x, y),
                red_at(&full, x, y),
                "pixel {x},{y} disagrees between damage rects"
            );
        }
    }
}
