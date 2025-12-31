use silksurf_dom::NodeKind;
use silksurf_html::{Tokenizer, TreeBuilder};

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
    let doc_children = dom.children(doc).unwrap();
    assert_eq!(doc_children.len(), 1);

    let html = doc_children[0];
    match dom.node(html).unwrap().kind() {
        NodeKind::Element { name } => assert_eq!(name, "html"),
        _ => panic!("expected html element"),
    }

    let html_children = dom.children(html).unwrap();
    assert_eq!(html_children.len(), 1);
    let body = html_children[0];

    let body_children = dom.children(body).unwrap();
    assert_eq!(body_children.len(), 1);
    let text = body_children[0];
    match dom.node(text).unwrap().kind() {
        NodeKind::Text { text } => assert_eq!(text, "hi"),
        _ => panic!("expected text node"),
    }
}
