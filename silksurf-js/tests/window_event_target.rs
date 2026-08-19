//! The global object as an EventTarget.
//!
//! `window` and `self` alias globalThis, so a page registers window listeners
//! by calling `addEventListener` on the global. These tests pin that the call
//! resolves, that a bubbling event reaches the global after the document, and
//! that a capturing listener there runs before the target.

use std::sync::{Arc, Mutex};

use silksurf_dom::Dom;
use silksurf_js::SilkContext;

fn context_with_document() -> SilkContext {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let html = dom.create_element("html");
    let body = dom.create_element("body");
    let div = dom.create_element("div");
    dom.set_attribute(div, "id", "target").expect("id attaches");
    dom.append_child(document, html).expect("html attaches");
    dom.append_child(html, body).expect("body attaches");
    dom.append_child(body, div).expect("div attaches");
    SilkContext::with_dom(&Arc::new(Mutex::new(dom)))
}

#[test]
fn window_add_event_listener_is_callable() {
    let mut ctx = context_with_document();
    ctx.eval(
        "if (typeof window.addEventListener !== 'function') \
             throw new Error('window.addEventListener missing'); \
         if (typeof self.removeEventListener !== 'function') \
             throw new Error('self.removeEventListener missing'); \
         if (typeof window.dispatchEvent !== 'function') \
             throw new Error('window.dispatchEvent missing');",
    )
    .expect("window EventTarget methods resolve");
}

#[test]
fn a_bubbling_event_reaches_the_window_listener() {
    let mut ctx = context_with_document();
    ctx.eval(
        "globalThis.seen = []; \
         window.addEventListener('click', function () { seen.push('window'); }); \
         document.addEventListener('click', function () { seen.push('document'); }); \
         var target = document.getElementById('target'); \
         target.addEventListener('click', function () { seen.push('target'); }); \
         target.dispatchEvent({ type: 'click', bubbles: true }); \
         if (seen.join(',') !== 'target,document,window') \
             throw new Error('bubble order was ' + seen.join(','));",
    )
    .expect("bubble path ends at the window listener");
}

#[test]
fn a_capturing_window_listener_runs_before_the_target() {
    let mut ctx = context_with_document();
    ctx.eval(
        "globalThis.seen = []; \
         window.addEventListener('click', function () { seen.push('window'); }, true); \
         var target = document.getElementById('target'); \
         target.addEventListener('click', function () { seen.push('target'); }); \
         target.dispatchEvent({ type: 'click', bubbles: true }); \
         if (seen.join(',') !== 'window,target') \
             throw new Error('capture order was ' + seen.join(','));",
    )
    .expect("capture path starts at the window listener");
}

#[test]
fn a_non_bubbling_event_stops_before_the_window_listener() {
    let mut ctx = context_with_document();
    ctx.eval(
        "globalThis.seen = []; \
         window.addEventListener('focus', function () { seen.push('window'); }); \
         var target = document.getElementById('target'); \
         target.addEventListener('focus', function () { seen.push('target'); }); \
         target.dispatchEvent({ type: 'focus', bubbles: false }); \
         if (seen.join(',') !== 'target') \
             throw new Error('non-bubbling order was ' + seen.join(','));",
    )
    .expect("a non-bubbling event stays at its target");
}

#[test]
fn window_listeners_see_the_global_as_current_target() {
    let mut ctx = context_with_document();
    ctx.eval(
        "globalThis.sameAsWindow = false; \
         window.addEventListener('click', function (event) { \
             sameAsWindow = event.currentTarget === window; \
         }); \
         document.getElementById('target') \
             .dispatchEvent({ type: 'click', bubbles: true }); \
         if (!sameAsWindow) throw new Error('currentTarget was not window');",
    )
    .expect("currentTarget resolves to the global object");
}

#[test]
fn a_removed_window_listener_stops_running() {
    let mut ctx = context_with_document();
    ctx.eval(
        "globalThis.count = 0; \
         var handler = function () { count += 1; }; \
         window.addEventListener('click', handler); \
         var target = document.getElementById('target'); \
         target.dispatchEvent({ type: 'click', bubbles: true }); \
         window.removeEventListener('click', handler); \
         target.dispatchEvent({ type: 'click', bubbles: true }); \
         if (count !== 1) throw new Error('handler ran ' + count + ' times');",
    )
    .expect("removeEventListener detaches the window listener");
}
