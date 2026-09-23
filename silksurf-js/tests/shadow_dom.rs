use silksurf_dom::Dom;
use silksurf_js::SilkContext;
use std::sync::{Arc, Mutex};

fn context() -> SilkContext {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let doctype = dom.create_doctype(Some("html".into()), None, None);
    let html = dom.create_element("html");
    let body = dom.create_element("body");
    dom.append_child(document, doctype).unwrap();
    dom.append_child(document, html).unwrap();
    dom.append_child(html, body).unwrap();
    SilkContext::with_dom(&Arc::new(Mutex::new(dom)))
}

#[test]
fn shadow_roots_preserve_identity_scoping_and_composed_connectivity() {
    context().eval(r#"
        function check(condition, label) { if (!condition) throw new Error(label); }
        const host = document.createElement('div'); document.body.appendChild(host);
        const root = host.attachShadow({mode: 'open'});
        check(root instanceof ShadowRoot && root instanceof DocumentFragment, 'interface chain');
        check(root === host.shadowRoot && root.host === host && root.mode === 'open', 'identity');
        root.innerHTML = '<span id="inner">shadow</span>';
        const child = root.querySelector('#inner');
        check(root.parentNode === null && host.childNodes.length === 0, 'separate tree');
        check(document.querySelector('#inner') === null && !host.contains(child) && !document.contains(child), 'ordinary tree boundary');
        check(root.contains(child) && child.isConnected && child.getRootNode() === root, 'root connectedness');
        check(child.getRootNode({composed:true}) === document, 'composed root');
        host.remove(); check(!child.isConnected && child.getRootNode({composed:true}) === host, 'detached');
        document.body.appendChild(host); check(child.isConnected, 'reattached');
        const nested = child.attachShadow({mode:'closed'});
        check(child.shadowRoot === null && nested.host === child && nested.mode === 'closed', 'closed visibility');
        check(nested.getRootNode({composed:true}) === document, 'nested composed root');
        check(document.childNodes[0] instanceof DocumentType && document.childNodes[0].nodeType === 10 && document.childNodes[0].nodeName === 'html', 'doctype interface');
        check(document.ownerDocument === null && root.ownerDocument === document, 'document ownership');
    "#).expect("shadow ownership and ordinary-tree isolation");
}

#[test]
fn shadow_roots_reject_duplicate_hosts_cloning_and_host_including_cycles() {
    context().eval(r"
        function rejects(operation) { let threw = false; try { operation(); } catch (_) { threw = true; } if (!threw) throw new Error('expected rejection'); }
        const host = document.createElement('div'), root = host.attachShadow({mode:'open'});
        rejects(() => host.attachShadow({mode:'open'}));
        rejects(() => document.createElement('input').attachShadow({mode:'open'}));
        rejects(() => document.createElement('div').attachShadow({mode:'invalid'}));
        rejects(() => root.appendChild(host));

        rejects(() => root.cloneNode(true));
        const fragment = document.createDocumentFragment(); fragment.appendChild(host);
        rejects(() => root.appendChild(fragment));
        if (fragment.firstChild !== host || root.childNodes.length !== 0) throw new Error('failed insertion mutates tree');
    ").expect("shadow mutation validation");
}

#[test]
fn shadow_fragment_transfer_mode_conversion_and_exception_names_follow_dom() {
    context().eval(r"
        function check(condition, label) { if (!condition) throw new Error(label); }
        const host = document.createElement('x-\u00e9'); let reads = 0;
        const root = host.attachShadow({get mode() { reads++; return reads === 1 ? 'open' : 'closed'; }});
        check(reads === 1 && root.mode === 'open', 'single dictionary conversion');
        const child = document.createElement('span'); root.appendChild(child);
        const destination = document.createElement('div');
        check(destination.appendChild(root) === root, 'fragment insertion result');
        check(root.childNodes.length === 0 && destination.firstChild === child && host.shadowRoot === root, 'fragment transfer preserves owner');
        try { host.attachShadow({mode:'open'}); throw new Error('accepted'); }
        catch (error) { check(error.name === 'NotSupportedError', 'attachment exception'); }
        try { root.cloneNode(); throw new Error('accepted'); }
        catch (error) { check(error.name === 'NotSupportedError', 'clone exception'); }
    ").expect("shadow fragment and IDL rules");
}
