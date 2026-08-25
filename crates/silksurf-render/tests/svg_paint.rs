use silksurf_dom::{Dom, Namespace, NodeId};
use silksurf_render::svg::{rasterize_svg, serialize_svg, svg_intrinsic_size};

/// Builds an `<svg>` element with `attrs`, holding one child element.
fn svg_with(attrs: &[(&str, &str)], child: Option<(&str, &[(&str, &str)])>) -> (Dom, NodeId) {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let svg = dom.create_element_ns("svg", Namespace::Svg);
    dom.append_child(doc, svg).unwrap();
    for (name, value) in attrs {
        dom.set_attribute(svg, *name, *value).unwrap();
    }
    if let Some((tag, child_attrs)) = child {
        let node = dom.create_element_ns(tag, Namespace::Svg);
        dom.append_child(svg, node).unwrap();
        for (name, value) in child_attrs {
            dom.set_attribute(node, *name, *value).unwrap();
        }
    }
    (dom, svg)
}

/// The root gains an xmlns it does not carry, because the parser records the
/// namespace on the node rather than as an attribute and usvg reads text.
#[test]
fn the_root_declares_the_svg_namespace() {
    let (dom, svg) = svg_with(&[("viewBox", "0 0 16 16")], None);
    let source = serialize_svg(&dom, svg).expect("serialized");
    assert!(
        source.contains("xmlns=\"http://www.w3.org/2000/svg\""),
        "{source}"
    );
    assert!(source.contains("viewBox=\"0 0 16 16\""), "{source}");
}

/// A child element writes as its own start tag, children, and end tag.
#[test]
fn a_child_element_round_trips_with_its_attributes() {
    let (dom, svg) = svg_with(
        &[("viewBox", "0 0 10 10")],
        Some(("path", &[("d", "M1 1 L9 9"), ("fill", "red")])),
    );
    let source = serialize_svg(&dom, svg).expect("serialized");
    assert!(
        source.contains("<path d=\"M1 1 L9 9\" fill=\"red\"></path>"),
        "{source}"
    );
}

/// The characters XML gives syntactic meaning to are escaped, so an attribute
/// value carrying a quote does not close its own delimiter.
#[test]
fn syntactic_characters_are_escaped() {
    let (dom, svg) = svg_with(&[("data-label", "a\"b&c<d")], None);
    let source = serialize_svg(&dom, svg).expect("serialized");
    assert!(source.contains("a&quot;b&amp;c&lt;d"), "{source}");
}

/// A node that is not an `<svg>` element serializes to nothing.
#[test]
fn a_non_svg_node_serializes_to_nothing() {
    let mut dom = Dom::new();
    let div = dom.create_element("div");
    assert!(serialize_svg(&dom, div).is_none());
}

/// The width and height attributes give the intrinsic size when present, and
/// the viewBox extent supplies it otherwise -- the only intrinsic size an
/// icon defined in user units carries.
#[test]
fn the_intrinsic_size_comes_from_the_attributes_then_the_view_box() {
    let (dom, svg) = svg_with(&[("width", "24"), ("height", "18")], None);
    assert_eq!(svg_intrinsic_size(&dom, svg), Some((24.0, 18.0)));

    let (dom, svg) = svg_with(&[("viewBox", "0 0 16 16")], None);
    assert_eq!(svg_intrinsic_size(&dom, svg), Some((16.0, 16.0)));

    // A lowercased viewBox is a different attribute, which is what the
    // foreign-content case rule exists to prevent.
    let (dom, svg) = svg_with(&[("viewbox", "0 0 16 16")], None);
    assert_eq!(svg_intrinsic_size(&dom, svg), None);

    let (dom, svg) = svg_with(&[], None);
    assert_eq!(svg_intrinsic_size(&dom, svg), None);
}

/// A filled rectangle covering its whole viewBox rasterizes to that color at
/// whatever pixel size layout gave it, because the tree maps its own user
/// coordinates onto the requested extent.
#[test]
fn a_filled_rect_rasterizes_to_its_color_at_the_requested_size() {
    let source = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 4 4\">\
                  <rect x=\"0\" y=\"0\" width=\"4\" height=\"4\" fill=\"#ff0000\"/></svg>";
    let surface = rasterize_svg(source, 32, 32).expect("rasterized");
    assert_eq!(surface.width, 32);
    assert_eq!(surface.height, 32);
    assert_eq!(surface.rgba.len(), 32 * 32 * 4);
    // Premultiplied RGBA over an opaque red fill.
    let center = ((16 * 32 + 16) * 4) as usize;
    assert_eq!(&surface.rgba[center..center + 4], &[255, 0, 0, 255]);
}

/// Half the viewBox filled leaves the other half transparent, which is what
/// lets an icon composite over whatever it sits on.
#[test]
fn the_uncovered_half_stays_transparent() {
    let source = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 4 4\">\
                  <rect x=\"0\" y=\"0\" width=\"2\" height=\"4\" fill=\"#0000ff\"/></svg>";
    let surface = rasterize_svg(source, 16, 16).expect("rasterized");
    let left = ((8 * 16 + 3) * 4) as usize;
    let right = ((8 * 16 + 12) * 4) as usize;
    assert_eq!(
        &surface.rgba[left..left + 4],
        &[0, 0, 255, 255],
        "filled half"
    );
    assert_eq!(surface.rgba[right + 3], 0, "uncovered half is transparent");
}

/// A source usvg rejects yields nothing rather than a blank surface, so the
/// caller paints what it already had.
#[test]
fn an_unreadable_source_yields_nothing() {
    assert!(rasterize_svg("<svg", 8, 8).is_none());
    assert!(rasterize_svg("not svg at all", 8, 8).is_none());
}

/// A zero-sized box has no pixels to rasterize into.
#[test]
fn a_zero_sized_box_rasterizes_to_nothing() {
    let source = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 4 4\"/>";
    assert!(rasterize_svg(source, 0, 8).is_none());
    assert!(rasterize_svg(source, 8, 0).is_none());
}

/// The serializer's output is what the rasterizer reads, so a subtree the
/// parser built reaches pixels without a hand-written source in between.
/// Finds the first `<svg>` element in a tree.
fn find_svg(dom: &Dom, node: NodeId) -> Option<NodeId> {
    if dom.element_name(node).ok().flatten() == Some("svg") {
        return Some(node);
    }
    dom.children(node)
        .ok()?
        .to_vec()
        .into_iter()
        .find_map(|child| find_svg(dom, child))
}

#[test]
fn a_serialized_subtree_rasterizes() {
    let dom = silksurf_html::parse_html(
        "<!DOCTYPE html><html><body><svg viewBox=\"0 0 4 4\">\
         <rect x=\"0\" y=\"0\" width=\"4\" height=\"4\" fill=\"#00ff00\"/></svg></body></html>",
    );
    let svg = find_svg(&dom, NodeId::from_raw(0)).expect("svg");
    let source = serialize_svg(&dom, svg).expect("serialized");
    let surface = rasterize_svg(&source, 8, 8).expect("rasterized");
    let center = ((4 * 8 + 4) * 4) as usize;
    assert_eq!(&surface.rgba[center..center + 4], &[0, 255, 0, 255]);
}
