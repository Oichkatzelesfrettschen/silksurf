use silksurf_dom::{AttributeName, Dom, DomError, NodeKind, TagName};

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
        NodeKind::Element { name, .. } => assert_eq!(name, &TagName::Html),
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

#[test]
fn sets_attributes_and_namespace() {
    let mut dom = Dom::new();
    let node = dom.create_element("div");
    dom.set_attribute(node, "class", "hero").unwrap();

    let attrs = dom.attributes(node).unwrap();
    assert_eq!(attrs.len(), 1);
    assert_eq!(attrs[0].name, AttributeName::Class);
    assert_eq!(attrs[0].value.as_str(), "hero");

    let svg = dom.create_element_ns("svg", silksurf_dom::Namespace::Svg);
    match dom.node(svg).unwrap().kind() {
        NodeKind::Element { namespace, .. } => {
            assert_eq!(namespace, &silksurf_dom::Namespace::Svg)
        }
        _ => panic!("expected element node"),
    }
}

#[test]
fn traversal_helpers_work() {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    let head = dom.create_element("head");
    let body = dom.create_element("body");

    dom.append_child(doc, html).unwrap();
    dom.append_child(html, head).unwrap();
    dom.append_child(html, body).unwrap();

    assert_eq!(dom.first_child(doc).unwrap(), Some(html));
    assert_eq!(dom.first_child(html).unwrap(), Some(head));
    assert_eq!(dom.next_sibling(head).unwrap(), Some(body));
    assert_eq!(dom.next_sibling(body).unwrap(), None);
    assert_eq!(dom.element_name(body).unwrap(), Some("body"));
}

#[test]
fn tracks_dirty_nodes_on_mutations() {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let dirty = dom.take_dirty_nodes();
    assert!(dirty.contains(&doc));
    assert!(dirty.contains(&html));

    dom.set_attribute(html, "class", "hero").unwrap();
    let dirty = dom.take_dirty_nodes();
    assert!(dirty.contains(&html));

    let text = dom.append_text(html, "hi").unwrap();
    let dirty = dom.take_dirty_nodes();
    assert!(dirty.contains(&html));
    assert!(dirty.contains(&text));
}

#[test]
fn batches_dirty_nodes_until_flush() {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");

    dom.begin_mutation_batch();
    dom.append_child(doc, html).unwrap();
    dom.set_attribute(html, "class", "hero").unwrap();

    let dirty = dom.take_dirty_nodes();
    assert!(dirty.is_empty());

    dom.end_mutation_batch();
    let dirty = dom.take_dirty_nodes();
    assert!(dirty.contains(&doc));
    assert!(dirty.contains(&html));
    assert_eq!(dirty.iter().filter(|&&id| id == html).count(), 1);
}
