//! Paint invariants for an application shell, driven from the conformance
//! fixture the wpt catalog scores.
//!
//! The fixture reproduces the three structures a client-rendered application
//! shell depends on: a `position: fixed` root anchored to the viewport, a
//! nested stacking context whose opaque background sits above the boxes it
//! contains, and a deferred-UI subtree hidden with `display: none`. Each of
//! the three has its own assertion here, so `make test` covers what the
//! conformance run scores.

use silksurf_css::{Color, parse_stylesheet};
use silksurf_dom::{AttributeName, Dom, NodeId};
use silksurf_engine::fused_pipeline::{FusedResult, fused_style_layout_paint};
use silksurf_engine::parse_html;
use silksurf_layout::Rect;
use silksurf_render::DisplayItem;

const FIXTURE: &str = include_str!("../conformance/wpt/fixtures/css_spa_shell_stacking.html");

const VIEWPORT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 1280.0,
    height: 800.0,
};

/// The fixture carries exactly one `<style>` element, so the literal delimiters
/// bound its text without a tokenizer pass.
fn fixture_css() -> &'static str {
    let open = FIXTURE
        .find("<style>")
        .expect("fixture declares a style block")
        + "<style>".len();
    let close = FIXTURE.find("</style>").expect("the style block closes");
    &FIXTURE[open..close]
}

fn render_fixture() -> (Dom, NodeId, FusedResult) {
    let parsed = parse_html(FIXTURE).expect("the fixture parses");
    let stylesheet = parse_stylesheet(fixture_css()).expect("the fixture stylesheet parses");
    let fused = fused_style_layout_paint(&parsed.dom, &stylesheet, parsed.document, VIEWPORT);
    (parsed.dom, parsed.document, fused)
}

fn find_by_id(dom: &Dom, node: NodeId, id: &str) -> Option<NodeId> {
    if let Ok(attrs) = dom.attributes(node)
        && attrs
            .iter()
            .any(|attr| attr.name == AttributeName::Id && attr.value.as_str() == id)
    {
        return Some(node);
    }
    for child in dom.children(node).ok()? {
        if let Some(found) = find_by_id(dom, *child, id) {
            return Some(found);
        }
    }
    None
}

fn rect_by_id(fused: &FusedResult, dom: &Dom, document: NodeId, id: &str) -> Rect {
    let node = find_by_id(dom, document, id).unwrap_or_else(|| panic!("#{id} is in the fixture"));
    let idx = *fused
        .table
        .node_to_bfs_idx
        .get(&node)
        .unwrap_or_else(|| panic!("#{id} reached layout")) as usize;
    fused.node_rects[idx]
}

/// Paint-list index of the first opaque fill in the given color. The fixture
/// gives each box a distinct color, so the color identifies the box.
fn solid_color_step(fused: &FusedResult, r: u8, g: u8, b: u8) -> usize {
    let wanted = Color { r, g, b, a: 255 };
    fused
        .display_items
        .iter()
        .position(|item| matches!(item, DisplayItem::SolidColor { color, .. } if *color == wanted))
        .unwrap_or_else(|| panic!("no solid fill in #{r:02x}{g:02x}{b:02x}"))
}

fn text_step(fused: &FusedResult, needle: &str) -> usize {
    fused
        .display_items
        .iter()
        .position(|item| matches!(item, DisplayItem::Text { text, .. } if text.contains(needle)))
        .unwrap_or_else(|| panic!("{needle:?} did not reach the paint list"))
}

/// CSS Position 3 2.1 makes the viewport the containing block for a fixed box,
/// so `inset: 0` gives `#shell` the viewport rect. Resolving the insets against
/// the document root instead yields that root's content height.
#[test]
fn a_fixed_shell_takes_the_viewport_rect() {
    let (dom, document, fused) = render_fixture();
    let shell = rect_by_id(&fused, &dom, document, "shell");
    let tol = 0.01_f32;
    assert!(
        (shell.x - VIEWPORT.x).abs() < tol
            && (shell.y - VIEWPORT.y).abs() < tol
            && (shell.width - VIEWPORT.width).abs() < tol
            && (shell.height - VIEWPORT.height).abs() < tol,
        "expected the viewport rect, got {:.1}x{:.1} at ({:.1}, {:.1})",
        shell.width,
        shell.height,
        shell.x,
        shell.y
    );
}

/// CSS Display 3 3 suppresses the boxes of the whole subtree under a
/// `display: none` element. Suppressing only the declaring element leaves the
/// descendants painting at the collapsed origin, stacked into one band.
#[test]
fn a_deferred_subtree_contributes_no_paint_items() {
    let (_dom, _document, fused) = render_fixture();
    for needle in ["login modal", "settings dialog", "consent banner"] {
        assert!(
            !fused.display_items.iter().any(
                |item| matches!(item, DisplayItem::Text { text, .. } if text.contains(needle))
            ),
            "display:none subtree text reached paint: {needle:?}"
        );
    }
}

/// CSS 2.1 Appendix E orders a stacking context as its own background, then
/// negative z-index children, then in-flow members, then the remaining children
/// by z-index. Document order runs `#pane`, `#watermark`, `#flow`, so a BFS
/// paint order puts `#pane` under `#flow`.
#[test]
fn a_stacking_context_paints_its_children_in_z_order() {
    let (_dom, _document, fused) = render_fixture();
    let shell = solid_color_step(&fused, 0x11, 0x11, 0x11);
    let watermark = solid_color_step(&fused, 0xff, 0x00, 0x00);
    let flow = solid_color_step(&fused, 0x00, 0xff, 0x00);
    let pane = solid_color_step(&fused, 0xff, 0xff, 0xff);
    assert!(
        shell < watermark && watermark < flow && flow < pane,
        "expected shell < watermark < flow < pane, got \
         shell={shell} watermark={watermark} flow={flow} pane={pane}"
    );
}

/// A stacking context nests: `#pane` carries `z-index: 3` and still paints
/// before the text inside it. A flat sort over the same z-index keys puts the
/// pane's opaque background over its own descendant.
#[test]
fn a_stacking_context_paints_before_the_boxes_it_contains() {
    let (_dom, _document, fused) = render_fixture();
    let pane = solid_color_step(&fused, 0xff, 0xff, 0xff);
    let text = text_step(&fused, "pane content");
    assert!(
        pane < text,
        "expected the pane background before its text, got pane={pane} text={text}"
    );
}
