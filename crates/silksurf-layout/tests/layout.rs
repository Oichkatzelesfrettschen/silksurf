use silksurf_core::SilkArena;
use silksurf_css::{compute_styles, parse_stylesheet};
use silksurf_dom::Dom;
use silksurf_layout::{LayoutBox, Rect, build_layout_tree};

fn find_box<'a>(
    layout: &'a LayoutBox<'a>,
    target: silksurf_dom::NodeId,
) -> Option<&'a LayoutBox<'a>> {
    if matches!(
        layout.box_type,
        silksurf_layout::BoxType::BlockNode(id) | silksurf_layout::BoxType::InlineNode(id)
            if id == target
    ) {
        return Some(layout);
    }
    for child in &layout.children {
        if let Some(found) = find_box(child, target) {
            return Some(found);
        }
    }
    None
}

#[test]
fn lays_out_block_boxes_vertically() {
    let stylesheet =
        parse_stylesheet("div { display: block; margin: 10px; padding: 5px; }").unwrap();

    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let body = dom.create_element("body");
    dom.append_child(html, body).unwrap();
    let div1 = dom.create_element("div");
    dom.append_child(body, div1).unwrap();
    let div2 = dom.create_element("div");
    dom.append_child(body, div2).unwrap();

    let styles = compute_styles(&dom, doc, &stylesheet);
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    };
    let arena = SilkArena::new();
    let tree = build_layout_tree(&arena, &dom, &styles, doc, viewport).expect("layout tree");

    let box1 = find_box(tree.root, div1).expect("div1 box");
    let box2 = find_box(tree.root, div2).expect("div2 box");

    assert!(box2.dimensions().content.y > box1.dimensions().content.y);
}
