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

#[test]
fn contents_wrapper_flattens_children_in_layout_tree() {
    let stylesheet = parse_stylesheet(
        "body { margin: 0 } #wrapper { display: contents } \
         #first, #second { display: block } \
         #first { height: 10px } #second { height: 20px }",
    )
    .expect("stylesheet parses");
    let mut dom = Dom::new();
    let document = dom.create_document();
    let body = dom.create_element("body");
    dom.append_child(document, body).expect("body attaches");
    let wrapper = dom.create_element("div");
    dom.set_attribute(wrapper, "id", "wrapper")
        .expect("wrapper id attaches");
    dom.append_child(body, wrapper).expect("wrapper attaches");
    let first = dom.create_element("div");
    dom.set_attribute(first, "id", "first")
        .expect("first id attaches");
    dom.append_child(wrapper, first).expect("first attaches");
    let second = dom.create_element("div");
    dom.set_attribute(second, "id", "second")
        .expect("second id attaches");
    dom.append_child(body, second).expect("second attaches");
    let styles = compute_styles(&dom, document, &stylesheet);
    let arena = SilkArena::new();
    let tree = build_layout_tree(
        &arena,
        &dom,
        &styles,
        document,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 300.0,
        },
    )
    .expect("layout tree exists");
    assert!(find_box(tree.root, wrapper).is_none());
    let body_box = find_box(tree.root, body).expect("body has a box");
    let child_nodes: Vec<_> = body_box
        .children
        .iter()
        .map(|child| child.box_type)
        .collect();
    assert_eq!(
        child_nodes,
        vec![
            silksurf_layout::BoxType::BlockNode(first),
            silksurf_layout::BoxType::BlockNode(second)
        ]
    );
}
