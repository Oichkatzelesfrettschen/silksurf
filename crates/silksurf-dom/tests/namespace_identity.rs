use silksurf_dom::diff::diff_doms;
use silksurf_dom::{Dom, Namespace};

#[test]
fn prefixed_elements_store_the_local_name_and_survive_import() {
    let mut source = Dom::new();
    let source_document = source.create_document();
    let source_svg = source.create_element_ns("s:svg", Namespace::Svg);
    source
        .append_child(source_document, source_svg)
        .expect("source element attaches");
    assert_eq!(
        source.element_name(source_svg).expect("name reads"),
        Some("svg")
    );
    assert_eq!(
        source.element_prefix(source_svg).expect("prefix reads"),
        Some("s")
    );

    let mut destination = Dom::new();
    let destination_document = destination.create_document();
    let imported = destination
        .import_subtree(&source, source_svg, destination_document)
        .expect("prefixed subtree imports");
    assert_eq!(
        destination.element_name(imported).expect("name reads"),
        Some("svg")
    );
    assert_eq!(
        destination.element_prefix(imported).expect("prefix reads"),
        Some("s")
    );
    assert_eq!(destination.element_namespace(imported), Namespace::Svg);
}

#[test]
fn namespace_and_prefix_changes_replace_the_element_identity() {
    let mut old_dom = Dom::new();
    let old = old_dom.create_element_ns("s:svg", Namespace::Svg);
    let mut new_dom = Dom::new();
    let new = new_dom.create_element_ns("t:svg", Namespace::Svg);
    let prefix_diff = diff_doms(&old_dom, old, &new_dom, new);
    assert_eq!(prefix_diff.removed, vec![old]);
    assert_eq!(prefix_diff.added, vec![new]);

    let other_namespace = new_dom.create_element_ns("s:svg", Namespace::Other("urn:other".into()));
    let namespace_diff = diff_doms(&old_dom, old, &new_dom, other_namespace);
    assert_eq!(namespace_diff.removed, vec![old]);
    assert_eq!(namespace_diff.added, vec![other_namespace]);
}

#[test]
fn importing_an_html_local_name_with_a_colon_keeps_it_unprefixed() {
    let mut source = Dom::new();
    let element = source.create_element("xyz:abc");
    let mut target = Dom::new();
    let target_document = target.create_document();
    let imported = target
        .import_subtree(&source, element, target_document)
        .expect("imports element");
    assert_eq!(
        target.element_name(imported).expect("name reads"),
        Some("xyz:abc")
    );
    assert_eq!(target.element_prefix(imported).expect("prefix reads"), None);
}
