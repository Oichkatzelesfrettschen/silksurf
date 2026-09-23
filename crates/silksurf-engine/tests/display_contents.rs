use silksurf_css::{Display, parse_stylesheet};
use silksurf_dom::{AttributeName, Dom, NodeId};
use silksurf_engine::fused_pipeline::{FusedResult, fused_style_layout_paint};
use silksurf_engine::parse_html;
use silksurf_layout::Rect;
use silksurf_render::DisplayItem;

const VIEWPORT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 400.0,
    height: 300.0,
};

fn find_by_id(dom: &Dom, node: NodeId, id: &str) -> Option<NodeId> {
    if dom.attributes(node).ok().is_some_and(|attributes| {
        attributes
            .iter()
            .any(|attribute| attribute.name == AttributeName::Id && attribute.value.as_str() == id)
    }) {
        return Some(node);
    }
    dom.children(node)
        .ok()?
        .iter()
        .find_map(|&child| find_by_id(dom, child, id))
}

fn node_index(fused: &FusedResult, node: NodeId) -> usize {
    fused.table.node_to_bfs_idx[&node] as usize
}

fn assert_close(actual: f32, expected: f32, label: &str) {
    assert!(
        (actual - expected).abs() < 0.01,
        "{label}: {actual} != {expected}"
    );
}

#[test]
fn contents_children_join_parent_flow_without_painting_the_wrapper() {
    let parsed = parse_html(
        "<html><body><div id='first'></div><div id='wrapper'>\
         <div id='inside'></div></div><div id='last'></div></body></html>",
    )
    .expect("fixture parses");
    let stylesheet = parse_stylesheet(
        "body { margin: 0 } #first { height: 10px } \
         #wrapper { display: contents; background: #ff0000 } \
         #inside { height: 20px; background: #00ff00 } \
         #last { height: 10px; background: #0000ff }",
    )
    .expect("stylesheet parses");
    let fused = fused_style_layout_paint(&parsed.dom, &stylesheet, parsed.document, VIEWPORT);
    let wrapper = find_by_id(&parsed.dom, parsed.document, "wrapper").expect("wrapper exists");
    let inside = find_by_id(&parsed.dom, parsed.document, "inside").expect("child exists");
    let last = find_by_id(&parsed.dom, parsed.document, "last").expect("sibling exists");
    assert_eq!(
        fused.styles[node_index(&fused, wrapper)]
            .as_ref()
            .expect("wrapper has a computed style")
            .display,
        Display::Contents
    );
    assert_close(
        fused.node_rects[node_index(&fused, inside)].y,
        10.0,
        "inside y",
    );
    assert_close(fused.node_rects[node_index(&fused, last)].y, 30.0, "last y");
    assert!(fused.display_items.iter().all(|item| {
        !matches!(item, DisplayItem::SolidColor { color, .. } if color.r == 255 && color.g == 0 && color.b == 0)
    }));
    assert!(fused.display_items.iter().any(|item| {
        matches!(item, DisplayItem::SolidColor { color, .. } if color.r == 0 && color.g == 255 && color.b == 0)
    }));
}

#[test]
fn direct_text_inside_contents_wrapper_keeps_its_inline_box() {
    let parsed = parse_html(
        "<html><body><div id='wrapper'>visible text</div><div id='after'></div></body></html>",
    )
    .expect("fixture parses");
    let stylesheet = parse_stylesheet(
        "body { margin: 0 } #wrapper { display: contents } #after { height: 10px }",
    )
    .expect("stylesheet parses");
    let fused = fused_style_layout_paint(&parsed.dom, &stylesheet, parsed.document, VIEWPORT);
    let text_rect = fused
        .display_items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Text { text, rect, .. } if text.contains("visible text") => Some(rect),
            _ => None,
        })
        .expect("direct text paints");
    let after = find_by_id(&parsed.dom, parsed.document, "after").expect("sibling exists");
    let after_rect = fused.node_rects[node_index(&fused, after)];
    assert!(after_rect.y >= text_rect.y + text_rect.height);
}

#[test]
fn contents_wrapper_transform_does_not_move_descendant_paint() {
    let parsed =
        parse_html("<html><body><div id='wrapper'><div id='child'></div></div></body></html>")
            .expect("fixture parses");
    let stylesheet = parse_stylesheet(
        "body { margin: 0 } #wrapper { display: contents; transform: translateX(50px) } \
         #child { width: 20px; height: 10px; background: #00ff00 }",
    )
    .expect("stylesheet parses");
    let fused = fused_style_layout_paint(&parsed.dom, &stylesheet, parsed.document, VIEWPORT);
    let rect = fused
        .display_items
        .iter()
        .find_map(|item| match item {
            DisplayItem::SolidColor { rect, color }
                if color.r == 0 && color.g == 255 && color.b == 0 =>
            {
                Some(rect)
            }
            _ => None,
        })
        .expect("child paints");
    assert_close(rect.x, 0.0, "child paint x");
}

#[test]
fn nested_contents_children_keep_order_in_a_flex_container() {
    let parsed = parse_html(
        "<html><body><div id='row'><div id='outer'><div id='left'></div>\
         <div id='inner'><div id='middle'></div></div></div>\
         <div id='right'></div></div></body></html>",
    )
    .expect("fixture parses");
    let stylesheet = parse_stylesheet(
        "body { margin: 0 } #row { display: flex } \
         #outer, #inner { display: contents } \
         #left, #middle, #right { width: 20px; height: 10px }",
    )
    .expect("stylesheet parses");
    let fused = fused_style_layout_paint(&parsed.dom, &stylesheet, parsed.document, VIEWPORT);
    for (id, expected_x) in [("left", 0.0), ("middle", 20.0), ("right", 40.0)] {
        let node = find_by_id(&parsed.dom, parsed.document, id).expect("fixture node exists");
        assert_close(fused.node_rects[node_index(&fused, node)].x, expected_x, id);
    }
}

#[test]
fn absolute_descendant_uses_positioned_ancestor_through_contents() {
    let parsed = parse_html(
        "<html><body><div id='outer'><div id='wrapper'>\
         <div id='absolute'></div></div></div></body></html>",
    )
    .expect("fixture parses");
    let stylesheet = parse_stylesheet(
        "body { margin: 0 } #outer { position: relative; margin-left: 30px; \
         margin-top: 20px; width: 100px; height: 100px } \
         #wrapper { display: contents } \
         #absolute { position: absolute; left: 10px; top: 5px; \
         width: 20px; height: 10px }",
    )
    .expect("stylesheet parses");
    let fused = fused_style_layout_paint(&parsed.dom, &stylesheet, parsed.document, VIEWPORT);
    let absolute = find_by_id(&parsed.dom, parsed.document, "absolute").expect("box exists");
    let rect = fused.node_rects[node_index(&fused, absolute)];
    for (label, actual, expected) in [
        ("x", rect.x, 40.0),
        ("y", rect.y, 25.0),
        ("width", rect.width, 20.0),
        ("height", rect.height, 10.0),
    ] {
        assert_close(actual, expected, label);
    }
}

#[test]
fn absolute_auto_insets_keep_static_position_through_contents() {
    let parsed = parse_html(
        "<html><body><div id='outer'><div id='first'></div><div id='wrapper'>\
         <div id='absolute'></div></div></div></body></html>",
    )
    .expect("fixture parses");
    let stylesheet = parse_stylesheet(
        "body { margin: 0 } #outer { position: relative; width: 100px; height: 100px } \
         #first { height: 20px } #wrapper { display: contents } \
         #absolute { position: absolute; width: 10px; height: 10px }",
    )
    .expect("stylesheet parses");
    let fused = fused_style_layout_paint(&parsed.dom, &stylesheet, parsed.document, VIEWPORT);
    let absolute = find_by_id(&parsed.dom, parsed.document, "absolute").expect("box exists");
    let rect = fused.node_rects[node_index(&fused, absolute)];
    assert_close(rect.y, 20.0, "absolute static y");
}

#[test]
fn static_position_placeholder_does_not_add_a_flex_gap() {
    let parsed = parse_html(
        "<html><body><div id='row'><div id='first'></div><div id='wrapper'>\
         <div id='absolute'></div></div><div id='last'></div></div></body></html>",
    )
    .expect("fixture parses");
    let stylesheet = parse_stylesheet(
        "body { margin: 0 } #row { display: flex; gap: 10px; position: relative } \
         #first, #last { width: 20px; height: 10px } #wrapper { display: contents } \
         #absolute { position: absolute; width: 10px; height: 10px }",
    )
    .expect("stylesheet parses");
    let fused = fused_style_layout_paint(&parsed.dom, &stylesheet, parsed.document, VIEWPORT);
    let last = find_by_id(&parsed.dom, parsed.document, "last").expect("last exists");
    assert_close(fused.node_rects[node_index(&fused, last)].x, 30.0, "last x");
}
