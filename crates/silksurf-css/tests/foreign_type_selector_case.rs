use silksurf_css::{compute_styles, parse_stylesheet};
use silksurf_dom::{Dom, Namespace};

#[test]
fn foreign_type_selectors_keep_case_in_the_indexed_cascade() {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let svg = dom.create_element_ns("svg", Namespace::Svg);
    let gradient = dom.create_element_ns("linearGradient", Namespace::Svg);
    dom.append_child(document, svg).expect("svg attaches");
    dom.append_child(svg, gradient).expect("gradient attaches");
    let stylesheet =
        parse_stylesheet("linearGradient { color: green } lineargradient { color: red }")
            .expect("stylesheet parses");
    let styles = compute_styles(&dom, document, &stylesheet);
    assert_eq!(styles[&gradient].color.g, 128);
    assert_eq!(styles[&gradient].color.r, 0);
}

#[test]
fn html_type_selectors_fold_case_for_preserved_uppercase_names() {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let element = dom.create_element_ns("TEMPLATE", Namespace::Html);
    dom.append_child(document, element)
        .expect("element attaches");
    let stylesheet = parse_stylesheet("template { color: red } TEMPLATE { color: blue }")
        .expect("stylesheet parses");
    let styles = compute_styles(&dom, document, &stylesheet);
    assert_eq!(styles[&element].color.b, 255);
    assert_eq!(styles[&element].color.r, 0);
}
