use silksurf_dom::{Dom, Namespace, NodeKind};

#[test]
fn template_owns_detached_content_without_expanding_ordinary_children() {
    let mut dom = Dom::new();
    let template = dom.create_element("template");
    let content = dom
        .template_contents(template)
        .expect("HTML template content");
    assert!(matches!(
        dom.node(content).unwrap().kind(),
        NodeKind::DocumentFragment
    ));
    let text = dom.create_text("inert");
    dom.append_child(content, text).unwrap();
    assert!(dom.children(template).unwrap().is_empty());
    assert_eq!(dom.parent(content).unwrap(), None);
    let foreign = dom.create_element_ns("template", Namespace::Svg);
    assert_eq!(dom.template_contents(foreign), None);
    let mut destination = Dom::new();
    let root = destination.create_element("div");
    let imported = destination.import_subtree(&dom, template, root).unwrap();
    let imported_content = destination.template_contents(imported).unwrap();
    assert!(destination.children(imported).unwrap().is_empty());
    assert_eq!(destination.children(imported_content).unwrap().len(), 1);
}

#[test]
fn fragment_splice_preserves_order_identity_and_failure_atomicity() {
    let mut dom = Dom::new();
    let parent = dom.create_element("div");
    let end = dom.create_element("span");
    dom.append_child(parent, end).unwrap();
    let fragment = dom.create_document_fragment();
    let first = dom.create_text("one");
    let second = dom.create_text("two");
    dom.append_child(fragment, first).unwrap();
    dom.append_child(fragment, second).unwrap();
    let wrong_reference = dom.create_element("p");
    assert!(
        dom.insert_before(parent, fragment, wrong_reference)
            .is_err()
    );
    assert_eq!(dom.children(fragment).unwrap(), &[first, second]);
    dom.insert_before(parent, fragment, end).unwrap();
    assert_eq!(dom.children(parent).unwrap(), &[first, second, end]);
    assert!(dom.children(fragment).unwrap().is_empty());
    assert_eq!(dom.parent(fragment).unwrap(), None);
    assert!(dom.pre_insert(end, parent, None).is_err());
    assert_eq!(dom.children(parent).unwrap(), &[first, second, end]);
    assert!(dom.remove_child(fragment, first).is_err());
    assert_eq!(dom.parent(first).unwrap(), Some(parent));
}

#[test]
fn template_content_rejects_its_host_as_a_descendant() {
    let mut dom = Dom::new();
    let template = dom.create_element("template");
    let content = dom.template_contents(template).unwrap();
    assert!(dom.append_child(content, template).is_err());
    assert!(dom.children(content).unwrap().is_empty());
}
