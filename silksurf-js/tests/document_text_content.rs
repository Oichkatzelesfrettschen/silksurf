use silksurf_dom::Dom;
use silksurf_js::SilkContext;
use std::sync::{Arc, Mutex};

#[test]
fn document_text_content_is_null_and_assignment_preserves_children() {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let html = dom.create_element("html");
    let body = dom.create_element("body");
    dom.append_child(document, html).expect("html attaches");
    dom.append_child(html, body).expect("body attaches");
    let mut context = SilkContext::with_dom(&Arc::new(Mutex::new(dom)));
    context
        .eval(
            "const before = document.documentElement; \
             if (document.textContent !== null) throw new Error('document getter'); \
             document.textContent = 'replacement'; \
             if (document.documentElement !== before || document.body === null) \
                 throw new Error('document setter replaced children');",
        )
        .expect("Document textContent follows DOM Standard");
}
