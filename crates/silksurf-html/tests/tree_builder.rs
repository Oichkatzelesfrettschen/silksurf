use silksurf_dom::{Dom, NodeId, NodeKind, TagName};
use silksurf_html::{Token, Tokenizer, TreeBuilder};

fn find_child_element(dom: &Dom, parent: NodeId, tag: TagName) -> Option<NodeId> {
    let children = dom.children(parent).ok()?;
    children.iter().copied().find(|child| {
        matches!(
            dom.node(*child).ok().map(|node| node.kind()),
            Some(NodeKind::Element { name, .. }) if *name == tag
        )
    })
}

#[test]
fn builds_dom_tree() {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer
        .feed("<html><body>hi</body></html>")
        .unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let mut builder = TreeBuilder::new();
    builder.process_tokens(tokens).unwrap();

    let dom = builder.dom();
    let doc = builder.document_id();
    let html = find_child_element(dom, doc, TagName::Html).expect("html element");

    let body = find_child_element(dom, html, TagName::Body).expect("body element");

    let body_children = dom.children(body).unwrap();
    assert_eq!(body_children.len(), 1);
    let text = body_children[0];
    match dom.node(text).unwrap().kind() {
        NodeKind::Text { text } => assert_eq!(text, "hi"),
        _ => panic!("expected text node"),
    }
}

#[test]
fn builds_attributes() {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer.feed("<div class='hero'></div>").unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let mut builder = TreeBuilder::new();
    builder.process_tokens(tokens).unwrap();

    let dom = builder.dom();
    let doc = builder.document_id();
    let html = find_child_element(dom, doc, TagName::Html).expect("html element");
    let body = find_child_element(dom, html, TagName::Body).expect("body element");
    let div = find_child_element(dom, body, TagName::Div).expect("div element");
    let attrs = dom.attributes(div).unwrap();
    assert_eq!(attrs.len(), 1);
    assert_eq!(attrs[0].name.as_str(), "class");
    assert_eq!(attrs[0].value.as_str(), "hero");
}

#[test]
fn inserts_head_and_body_implicitly() {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer.feed("<title>hi</title><p>ok</p>").unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let mut builder = TreeBuilder::new();
    builder.process_tokens(tokens).unwrap();

    let dom = builder.dom();
    let doc = builder.document_id();
    let html = dom.children(doc).unwrap()[0];
    let html_children = dom.children(html).unwrap();

    let mut head = None;
    let mut body = None;
    for child in html_children {
        if let NodeKind::Element { name, .. } = dom.node(*child).unwrap().kind() {
            if *name == TagName::Head {
                head = Some(*child);
            } else if *name == TagName::Body {
                body = Some(*child);
            }
        }
    }

    let head = head.expect("head element");
    let body = body.expect("body element");

    let head_children = dom.children(head).unwrap();
    assert!(!head_children.is_empty());

    let body_children = dom.children(body).unwrap();
    assert!(!body_children.is_empty());
}

#[test]
fn fosters_text_out_of_table() {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer.feed("<table>text</table>").unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let mut builder = TreeBuilder::new();
    builder.process_tokens(tokens).unwrap();

    let dom = builder.dom();
    let doc = builder.document_id();
    let html = find_child_element(dom, doc, TagName::Html).expect("html element");
    let body = find_child_element(dom, html, TagName::Body).expect("body element");
    let body_children = dom.children(body).unwrap();

    assert!(body_children.len() >= 1);
    let mut saw_text = false;
    let mut saw_table = false;
    for child in body_children {
        match dom.node(*child).unwrap().kind() {
            NodeKind::Text { text } => {
                if text == "text" {
                    saw_text = true;
                }
            }
            NodeKind::Element { name, .. } => {
                if *name == TagName::Table {
                    saw_table = true;
                }
            }
            _ => {}
        }
    }

    assert!(saw_text);
    assert!(saw_table);
}

#[test]
fn inserts_doctype_and_comment_nodes() {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer
        .feed("<!doctype html><!-- ok --><html></html>")
        .unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let mut builder = TreeBuilder::new();
    builder.process_tokens(tokens).unwrap();

    let dom = builder.dom();
    let doc = builder.document_id();
    let children = dom.children(doc).unwrap();
    assert!(children.iter().any(|node| matches!(
        dom.node(*node).unwrap().kind(),
        NodeKind::Doctype { .. }
    )));
    assert!(children.iter().any(|node| matches!(
        dom.node(*node).unwrap().kind(),
        NodeKind::Comment { .. }
    )));
}

#[test]
fn merges_adjacent_text_nodes() {
    let tokens = vec![
        Token::StartTag {
            name: "p".into(),
            attributes: vec![],
            self_closing: false,
        },
        Token::Character {
            data: "hi".into(),
        },
        Token::Character {
            data: " there".into(),
        },
        Token::EndTag { name: "p".into() },
        Token::Eof,
    ];

    let mut builder = TreeBuilder::new();
    builder.process_tokens(tokens).unwrap();

    let dom = builder.dom();
    let doc = builder.document_id();
    let html = find_child_element(dom, doc, TagName::Html).expect("html element");
    let body = find_child_element(dom, html, TagName::Body).expect("body element");
    let p = find_child_element(dom, body, TagName::P).expect("p element");
    let children = dom.children(p).unwrap();
    assert_eq!(children.len(), 1);
    match dom.node(children[0]).unwrap().kind() {
        NodeKind::Text { text } => assert_eq!(text, "hi there"),
        _ => panic!("expected merged text node"),
    }
}

#[test]
fn inserts_text_before_html_into_body() {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer.feed("hello<p>world</p>").unwrap();
    tokens.extend(tokenizer.finish().unwrap());

    let mut builder = TreeBuilder::new();
    builder.process_tokens(tokens).unwrap();

    let dom = builder.dom();
    let doc = builder.document_id();
    let html = dom.children(doc).unwrap()[0];
    let html_children = dom.children(html).unwrap();
    let body = html_children
        .iter()
        .copied()
        .find(|child| matches!(dom.node(*child).unwrap().kind(), NodeKind::Element { name, .. } if *name == TagName::Body))
        .expect("body element");
    let body_children = dom.children(body).unwrap();
    assert!(body_children.len() >= 2);

    match dom.node(body_children[0]).unwrap().kind() {
        NodeKind::Text { text } => assert_eq!(text, "hello"),
        _ => panic!("expected text node"),
    }
    match dom.node(body_children[1]).unwrap().kind() {
        NodeKind::Element { name, .. } => assert_eq!(name, &TagName::P),
        _ => panic!("expected p element"),
    }
}
