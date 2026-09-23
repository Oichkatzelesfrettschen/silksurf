use std::sync::{Arc, Mutex};

use silksurf_dom::Dom;
use silksurf_js::SilkContext;

fn context_with_document() -> SilkContext {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let html = dom.create_element("html");
    let body = dom.create_element("body");
    dom.append_child(document, html).expect("html attaches");
    dom.append_child(html, body).expect("body attaches");
    SilkContext::with_dom(&Arc::new(Mutex::new(dom)))
}

#[test]
fn namespaced_elements_retain_namespace_case_and_prototype() {
    let mut context = context_with_document();
    context
        .eval(
            "const svg = 'http://www.w3.org/2000/svg';
             const rect = document.createElementNS(svg, 'linearGradient');
             if (rect.namespaceURI !== svg || rect.localName !== 'linearGradient' ||
                 rect.tagName !== 'linearGradient' || rect.prefix !== null ||
                 !(rect instanceof SVGElement) || rect instanceof HTMLElement)
                 throw new Error('SVG identity');
             const copied = rect.cloneNode();
             if (copied.namespaceURI !== svg || copied.localName !== 'linearGradient')
                 throw new Error('clone identity');
             const custom = document.createElementNS('urn:example', 'x:Node');
             if (custom.namespaceURI !== 'urn:example' || custom.prefix !== 'x' ||
                 custom.localName !== 'Node' || custom.tagName !== 'x:Node' ||
                 !(custom instanceof Element) || custom instanceof HTMLElement)
                 throw new Error('custom identity');
             const bare = document.createElementNS(null, 'Node');
             if (bare.namespaceURI !== null || bare.localName !== 'Node')
                 throw new Error('null namespace');
             const html = document.createElement('DiV');
             if (html.localName !== 'div' || html.tagName !== 'DIV' ||
                 html.namespaceURI !== 'http://www.w3.org/1999/xhtml')
                 throw new Error('HTML identity');",
        )
        .expect("namespace identity follows the DOM node");
}

#[test]
fn create_element_ns_validates_xml_names_and_reserved_prefixes() {
    let mut context = context_with_document();
    context
        .eval(
            "function rejects(namespace, name, expected) {
                 try { document.createElementNS(namespace, name); }
                 catch (error) { if (error.name === expected) return; }
                 throw new Error(name + ' should throw ' + expected);
             }
             rejects(null, '1bad', 'InvalidCharacterError');
             rejects(null, 'a:b:c', 'NamespaceError');
             rejects(null, 'p:node', 'NamespaceError');
             rejects('urn:example', 'xml:node', 'NamespaceError');
             rejects('urn:example', 'xmlns:node', 'NamespaceError');
             rejects('http://www.w3.org/2000/xmlns/', 'node', 'NamespaceError');
             const xml = document.createElementNS(
                 'http://www.w3.org/XML/1998/namespace', 'xml:node');
             if (xml.prefix !== 'xml') throw new Error('XML prefix');",
        )
        .expect("namespace validation follows DOM validate and extract");
}
