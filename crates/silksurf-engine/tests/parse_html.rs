use silksurf_dom::{NodeKind, TagName};
use silksurf_engine::parse_html;

#[test]
fn parse_html_builds_dom() {
    let parsed = parse_html("<html><body>hi</body></html>").unwrap();
    let dom = parsed.dom;
    let doc = parsed.document;

    let children = dom.children(doc).unwrap();
    assert_eq!(children.len(), 1);

    match dom.node(children[0]).unwrap().kind() {
        NodeKind::Element { name, .. } => assert_eq!(name, &TagName::Html),
        _ => panic!("expected html element"),
    }
}
