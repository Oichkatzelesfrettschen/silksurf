//! Transforms folded into the paint rect.
//!
//! A scale and a translation map an axis-aligned rect onto another
//! axis-aligned rect, so AD-031 folds them into the `Rect` every DisplayItem
//! carries rather than giving the item a matrix the three rasterizers, the
//! tiling, the hit test, and the damage union would all have to read.

use silksurf_css::parse_stylesheet;
use silksurf_engine::fused_pipeline::fused_style_layout_paint;
use silksurf_engine::parse_html;
use silksurf_layout::Rect;
use silksurf_render::DisplayItem;

const VIEWPORT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 1000.0,
    height: 800.0,
};

/// Paint the fixture and return the rect of the solid fill with `want` colour.
fn painted(html: &str, css: &str, want: (u8, u8, u8)) -> Option<Rect> {
    let parsed = parse_html(html).expect("fixture parses");
    let stylesheet = parse_stylesheet(css).expect("stylesheet parses");
    let fused = fused_style_layout_paint(&parsed.dom, &stylesheet, parsed.document, VIEWPORT);
    fused.display_items.iter().find_map(|item| match item {
        DisplayItem::SolidColor { rect, color } if (color.r, color.g, color.b) == want => {
            Some(*rect)
        }
        _ => None,
    })
}

const GREEN: (u8, u8, u8) = (0, 255, 0);

const BOX_HTML: &str = "<!doctype html><html><body><div id=\"b\"></div></body></html>";

fn box_css(transform: &str) -> String {
    format!(
        "body {{ margin: 0 }} \
         #b {{ position: absolute; left: 100px; top: 50px; \
               width: 200px; height: 100px; background: #00ff00; {transform} }}"
    )
}

#[test]
fn an_untransformed_box_paints_where_layout_put_it() {
    let rect = painted(BOX_HTML, &box_css(""), GREEN).expect("the box paints");
    assert_eq!(
        (rect.x, rect.y, rect.width, rect.height),
        (100.0, 50.0, 200.0, 100.0)
    );
}

#[test]
fn a_zero_scale_paints_nothing() {
    let rect = painted(BOX_HTML, &box_css("transform: scale(0)"), GREEN)
        .expect("the item is still emitted");
    assert_eq!(
        (rect.width, rect.height),
        (0.0, 0.0),
        "a zero-area rect is what sk_rect and pixel_rect_from_rect both refuse"
    );
}

/// CSS Transforms 1, 6 anchors a transform at `50% 50%` by default, so a
/// halved box keeps its centre.
#[test]
fn a_scale_shrinks_about_the_box_centre() {
    let rect = painted(BOX_HTML, &box_css("transform: scale(.5)"), GREEN).expect("the box paints");
    assert_eq!(
        (rect.x, rect.y, rect.width, rect.height),
        (150.0, 75.0, 100.0, 50.0)
    );
}

#[test]
fn a_percentage_translation_resolves_against_the_box_itself() {
    let rect =
        painted(BOX_HTML, &box_css("transform: translate(-50%)"), GREEN).expect("the box paints");
    assert_eq!((rect.x, rect.width), (0.0, 200.0));
}

/// A matrix carries a translation and a scale that the engine dropped whole
/// before, because only `translate` functions were read.
#[test]
fn a_matrix_contributes_its_scale_and_translation() {
    let rect = painted(
        BOX_HTML,
        &box_css("transform: matrix(2, 0, 0, 1, 10, 20)"),
        GREEN,
    )
    .expect("the box paints");
    assert_eq!(
        (rect.x, rect.y, rect.width, rect.height),
        (10.0, 70.0, 400.0, 100.0),
        "scale about the centre, then the matrix's own translation"
    );
}

/// A matrix whose b or c term is non-zero rotates or skews, and its a and d
/// terms are cosines rather than scale factors.
#[test]
fn an_oblique_matrix_contributes_nothing() {
    let rect = painted(
        BOX_HTML,
        &box_css("transform: matrix(0, 1, -1, 0, 0, 0)"),
        GREEN,
    )
    .expect("the box paints");
    assert_eq!(
        (rect.x, rect.y, rect.width, rect.height),
        (100.0, 50.0, 200.0, 100.0)
    );
}

#[test]
fn rotation_leaves_the_scale_beside_it_intact() {
    let rect = painted(
        BOX_HTML,
        &box_css("transform: scale(.5) rotate(-90deg)"),
        GREEN,
    )
    .expect("the box paints");
    assert_eq!(
        (rect.width, rect.height),
        (100.0, 50.0),
        "rotate contributes identity; the scale beside it still applies"
    );
}

/// The discriminating case for composition: a parent scale multiplies a
/// child's translation as well as its size, which plain offset addition
/// could not express.
#[test]
fn a_parent_scale_multiplies_a_child_translation() {
    let html = "<!doctype html><html><body><div id=\"p\"><div id=\"c\"></div></div></body></html>";
    let css = "body { margin: 0 } \
               #p { position: absolute; left: 0; top: 0; width: 400px; height: 400px; \
                    transform: scale(.5) } \
               #c { position: absolute; left: 0; top: 0; width: 100px; height: 100px; \
                    background: #00ff00; transform: translateX(100px) }";
    let rect = painted(html, css, GREEN).expect("the child paints");
    // The parent halves about its own centre (200, 200), so its content maps
    // x -> 0.5x + 100. The child's own 100px translation is halved to 50.
    assert_eq!(
        (rect.x, rect.width),
        (150.0, 50.0),
        "the child moves 50px, not 100px, and paints at half size"
    );
}
