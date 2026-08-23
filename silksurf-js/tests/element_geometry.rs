/*
 * The layout-reading DOM accessors, checked against a provider that answers a
 * known border box.
 *
 * The provider is the seam between the engine and script: the engine writes
 * viewport-relative border boxes, and these cases pin what each accessor
 * derives from one. A context with no provider installed reports zeros, which
 * is the state a document that has run no layout is in.
 */

use silksurf_dom::Dom;
use std::sync::{Arc, Mutex};

fn context_with_box(element_box: Option<silksurf_js::ElementBox>) -> silksurf_js::SilkContext {
    let mut dom = Dom::new();
    let root = dom.create_document();
    let div = dom.create_element("div");
    let _ = dom.set_attribute(div, "id", "t");
    let _ = dom.append_child(root, div);
    dom.materialize_resolve_table();
    let mut ctx = silksurf_js::SilkContext::with_dom(&Arc::new(Mutex::new(dom)));
    if let Some(found) = element_box {
        ctx.set_geometry_provider(std::rc::Rc::new(move |_node| Some(found)));
    }
    ctx
}

#[test]
fn a_context_with_no_provider_reports_a_zero_rect() {
    let mut ctx = context_with_box(None);
    ctx.eval(
        r"
        var r = document.getElementById('t').getBoundingClientRect();
        if (r.width !== 0 || r.height !== 0 || r.x !== 0 || r.y !== 0) {
            throw new Error('want a zero rect, got ' + JSON.stringify(r));
        }
    ",
    )
    .expect("a document that has run no layout reports zeros");
}

#[test]
fn every_accessor_derives_from_the_one_border_box() {
    // x 10, y 20, 300 wide, 100 tall, borders top 1, right 2, bottom 3, left 4.
    let mut ctx = context_with_box(Some([10.0, 20.0, 300.0, 100.0, 1.0, 2.0, 3.0, 4.0]));
    ctx.eval(
        r"
        function eq(got, want, label) {
            if (got !== want) { throw new Error(label + ': want ' + want + ' got ' + got); }
        }
        var el = document.getElementById('t');
        var r = el.getBoundingClientRect();
        eq(r.x, 10, 'x'); eq(r.y, 20, 'y');
        eq(r.width, 300, 'width'); eq(r.height, 100, 'height');
        eq(r.left, 10, 'left'); eq(r.top, 20, 'top');
        eq(r.right, 310, 'right'); eq(r.bottom, 120, 'bottom');
        eq(JSON.stringify(r.toJSON()), JSON.stringify(r), 'toJSON');
        // offsetWidth is the border box; clientWidth subtracts the borders to
        // reach the padding box.
        eq(el.offsetWidth, 300, 'offsetWidth');
        eq(el.offsetHeight, 100, 'offsetHeight');
        eq(el.clientWidth, 294, 'clientWidth');
        eq(el.clientHeight, 96, 'clientHeight');
        var rects = el.getClientRects();
        eq(rects.length, 1, 'getClientRects length');
        eq(rects[0].width, 300, 'getClientRects width');
        eq(rects.item(0).x, 10, 'getClientRects item');
        eq(rects.item(1), null, 'getClientRects past the end');
    ",
    )
    .expect("every accessor reads the provider's box");
}

#[test]
fn a_node_the_layout_produced_no_box_for_reports_zeros() {
    let mut dom = Dom::new();
    let root = dom.create_document();
    let div = dom.create_element("div");
    let _ = dom.set_attribute(div, "id", "t");
    let _ = dom.append_child(root, div);
    dom.materialize_resolve_table();
    let mut ctx = silksurf_js::SilkContext::with_dom(&Arc::new(Mutex::new(dom)));
    ctx.set_geometry_provider(std::rc::Rc::new(|_node| None));
    ctx.eval(
        r"
        var el = document.getElementById('t');
        if (el.getBoundingClientRect().width !== 0) { throw new Error('rect'); }
        if (el.offsetWidth !== 0 || el.clientWidth !== 0) { throw new Error('dimensions'); }
    ",
    )
    .expect("an unrendered element reports zeros rather than throwing");
}
