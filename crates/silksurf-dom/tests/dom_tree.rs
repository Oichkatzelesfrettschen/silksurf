use silksurf_dom::{Dom, DomError, NodeKind};

#[test]
fn append_child_sets_relationships() {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    let text = dom.create_text("hello");

    dom.append_child(doc, html).unwrap();
    dom.append_child(html, text).unwrap();

    assert_eq!(dom.children(doc).unwrap(), &[html]);
    assert_eq!(dom.parent(html).unwrap(), Some(doc));
    assert_eq!(dom.parent(text).unwrap(), Some(html));

    match dom.node(html).unwrap().kind() {
        NodeKind::Element { name } => assert_eq!(name, "html"),
        _ => panic!("expected element node"),
    }

    match dom.node(text).unwrap().kind() {
        NodeKind::Text { text } => assert_eq!(text, "hello"),
        _ => panic!("expected text node"),
    }
}

#[test]
fn append_child_rejects_second_parent() {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let first = dom.create_element("first");
    let second = dom.create_element("second");

    dom.append_child(doc, first).unwrap();
    let result = dom.append_child(second, first);

    assert_eq!(result, Err(DomError::AlreadyHasParent(first)));
}
