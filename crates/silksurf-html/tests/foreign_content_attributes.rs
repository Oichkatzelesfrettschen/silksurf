use silksurf_dom::{Dom, Namespace, NodeId, NodeKind};
use silksurf_html::parse_html;

/// Finds the first element with `tag`, in document order.
fn first(dom: &Dom, node: NodeId, tag: &str) -> Option<NodeId> {
    if dom.element_name(node).ok().flatten() == Some(tag) {
        return Some(node);
    }
    for child in dom.children(node).ok()?.to_vec() {
        if let Some(found) = first(dom, child, tag) {
            return Some(found);
        }
    }
    None
}

fn attribute(dom: &Dom, node: NodeId, name: &str) -> Option<String> {
    dom.attributes(node)
        .ok()?
        .iter()
        .find(|attr| attr.name.as_str() == name)
        .map(|attr| attr.value.as_str().to_string())
}

/// The tree builder adjusts foreign attribute names to the case SVG defines
/// them with, and the DOM keeps that case. Lowercasing `viewBox` produces an
/// attribute no SVG consumer reads, so an icon would render unsized.
#[test]
fn a_parsed_svg_keeps_the_view_box_case() {
    let dom = parse_html(
        "<!DOCTYPE html><html><body><svg viewBox=\"0 0 16 16\" width=\"16\"></svg></body></html>",
    );
    let svg = first(&dom, NodeId::from_raw(0), "svg").expect("svg element");
    assert_eq!(
        attribute(&dom, svg, "viewBox").as_deref(),
        Some("0 0 16 16")
    );
    assert_eq!(attribute(&dom, svg, "width").as_deref(), Some("16"));
}

/// An `<svg>` subtree carries the SVG namespace, which is what selects the
/// case rule for its attributes.
#[test]
fn a_parsed_svg_subtree_carries_the_svg_namespace() {
    let dom = parse_html("<!DOCTYPE html><html><body><svg><path d=\"M1 2\"/></svg></body></html>");
    for tag in ["svg", "path"] {
        let node = first(&dom, NodeId::from_raw(0), tag).expect(tag);
        let NodeKind::Element { namespace, .. } = dom.node(node).expect(tag).kind() else {
            panic!("{tag} is not an element");
        };
        assert_eq!(*namespace, Namespace::Svg, "{tag}");
    }
}

/// An HTML element keeps the case-insensitive rule, so the same source
/// spelling reaches the lowercase name.
#[test]
fn a_parsed_html_element_still_lowercases_its_attributes() {
    let dom = parse_html("<!DOCTYPE html><html><body><div DATA-X=\"1\"></div></body></html>");
    let div = first(&dom, NodeId::from_raw(0), "div").expect("div");
    assert_eq!(attribute(&dom, div, "data-x").as_deref(), Some("1"));
}
