use silksurf_dom::{AttributeName, Dom, Namespace};

/// The attribute names an element carries, in document order.
fn names(dom: &Dom, node: silksurf_dom::NodeId) -> Vec<String> {
    dom.attributes(node)
        .expect("element")
        .iter()
        .map(|attr| attr.name.as_str().to_string())
        .collect()
}

/// HTML matches attribute names case-insensitively, so an author writing
/// `DATA-X` reaches the same attribute as `data-x`.
#[test]
fn an_html_element_lowercases_its_attribute_names() {
    let mut dom = Dom::new();
    let div = dom.create_element("div");
    dom.set_attribute(div, "DATA-X", "1").unwrap();
    assert_eq!(names(&dom, div), vec!["data-x"]);
}

/// SVG defines `viewBox` with camel case, and lowercasing it produces a
/// different attribute that no SVG consumer reads. The corpus names
/// `viewBox` 296 times across its bundles.
#[test]
fn an_svg_element_keeps_its_attribute_name_case() {
    let mut dom = Dom::new();
    let svg = dom.create_element_ns("svg", Namespace::Svg);
    dom.set_attribute(svg, "viewBox", "0 0 16 16").unwrap();
    dom.set_attribute(svg, "preserveAspectRatio", "xMidYMid")
        .unwrap();
    assert_eq!(names(&dom, svg), vec!["viewBox", "preserveAspectRatio"]);
}

/// A name SVG shares with HTML keeps its interned variant, because those
/// names are lowercase in both languages.
#[test]
fn a_shared_name_keeps_its_interned_variant() {
    let mut dom = Dom::new();
    let svg = dom.create_element_ns("svg", Namespace::Svg);
    dom.set_attribute(svg, "id", "logo").unwrap();
    dom.set_attribute(svg, "class", "icon").unwrap();
    let attrs = dom.attributes(svg).expect("element");
    assert_eq!(attrs[0].name, AttributeName::Id);
    assert_eq!(attrs[1].name, AttributeName::Class);
}

/// Reading and removing an attribute reach the same name the write stored,
/// so a camel-cased SVG attribute is addressable after it is set.
#[test]
fn a_camel_cased_attribute_round_trips_through_read_and_remove() {
    let mut dom = Dom::new();
    let svg = dom.create_element_ns("svg", Namespace::Svg);
    dom.set_attribute(svg, "viewBox", "0 0 24 24").unwrap();
    let stored = dom
        .attributes(svg)
        .expect("element")
        .iter()
        .find(|attr| attr.name.as_str() == "viewBox")
        .map(|attr| attr.value.as_str().to_string());
    assert_eq!(stored.as_deref(), Some("0 0 24 24"));
    assert!(dom.remove_attribute(svg, "viewBox").unwrap());
    assert!(names(&dom, svg).is_empty());
}
