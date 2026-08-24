/*
 * ResizeObserver against the layout observation checkpoint.
 *
 * The provider stands in for the engine: it answers one border box, and the
 * test moves it the way a completed layout does. What the cases pin is the
 * checkpoint contract -- an observation reports once when observed, then only
 * when its observed box moves -- and the entry shape a page reads.
 */

use silksurf_dom::Dom;
use std::{
    cell::Cell,
    rc::Rc,
    sync::{Arc, Mutex},
};

/// A document holding one div, with a provider whose box the test moves.
fn context() -> (silksurf_js::SilkContext, Rc<Cell<silksurf_js::ElementBox>>) {
    let mut dom = Dom::new();
    let root = dom.create_document();
    let div = dom.create_element("div");
    let _ = dom.set_attribute(div, "id", "target");
    let _ = dom.append_child(root, div);
    let other = dom.create_element("div");
    let _ = dom.set_attribute(other, "id", "other");
    let _ = dom.append_child(root, other);
    dom.materialize_resolve_table();
    let mut ctx = silksurf_js::SilkContext::with_dom(&Arc::new(Mutex::new(dom)));
    // 300x100 border box, borders 1/2/3/4, paddings 5/6/7/8: content box
    // 300 - 4 - 2 - 8 - 6 = 280 wide, 100 - 1 - 3 - 5 - 7 = 84 tall.
    let element_box: Rc<Cell<silksurf_js::ElementBox>> = Rc::new(Cell::new([
        10.0, 20.0, 300.0, 100.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0,
    ]));
    let handle = Rc::clone(&element_box);
    ctx.set_geometry_provider(Rc::new(move |_node| Some(handle.get())));
    (ctx, element_box)
}

/*
 * Assertions record rather than throw: a throw inside an observer callback
 * reaches the page's error path and never the embedder, so a test written
 * that way passes whatever it asserts. The runner reads the recorded failure
 * after delivery and requires a minimum assertion count, so a callback that
 * never ran fails rather than passes empty.
 */
const ASSERT: &str = r"
globalThis.__failure = null;
globalThis.__checked = 0;
function eq(got, want, label) {
    globalThis.__checked++;
    if (got !== want && !globalThis.__failure) {
        globalThis.__failure = label + ': want ' + JSON.stringify(want) + ' got ' + JSON.stringify(got);
    }
}
var target = document.getElementById('target');
var seen = [];
";

fn check(ctx: &mut silksurf_js::SilkContext, expected_assertions: u32) {
    ctx.eval(&format!(
        "if (globalThis.__failure) {{ throw new Error(globalThis.__failure); }} \
         if (globalThis.__checked < {expected_assertions}) {{ \
             throw new Error('only ' + globalThis.__checked + ' of {expected_assertions} assertions ran'); \
         }}"
    ))
    .expect("every assertion ran and agreed");
}

#[test]
fn the_constructor_and_prototype_exist() {
    let (mut ctx, _box) = context();
    ctx.eval(
        r"
        if (typeof ResizeObserver !== 'function') { throw new Error('constructor'); }
        var o = new ResizeObserver(function () {});
        if (typeof o.observe !== 'function') { throw new Error('observe'); }
        if (typeof o.unobserve !== 'function') { throw new Error('unobserve'); }
        if (typeof o.disconnect !== 'function') { throw new Error('disconnect'); }
        if (Object.prototype.toString.call(o) !== '[object ResizeObserver]') { throw new Error('toStringTag'); }
        var threw = false;
        try { new ResizeObserver(); } catch (e) { threw = e instanceof TypeError; }
        if (!threw) { throw new Error('a missing callback is a TypeError'); }
    ",
    )
    .expect("the object shape a page reads");
}

#[test]
fn an_observation_reports_once_when_observed() {
    // A static element never resizes, so a differ alone would deliver nothing
    // and every lazy-loading page would stall. The first pass reports
    // unconditionally.
    let (mut ctx, _box) = context();
    ctx.eval(&format!(
        "{ASSERT}
        var o = new ResizeObserver(function (entries, observer) {{
            eq(entries.length, 1, 'one entry');
            eq(entries[0].target, target, 'target');
            eq(observer, o, 'the observer is the second argument');
            eq(entries[0].contentRect.width, 280, 'contentRect width');
            eq(entries[0].contentRect.height, 84, 'contentRect height');
            eq(entries[0].contentRect.x, 8, 'contentRect x is the left padding');
            eq(entries[0].contentRect.y, 5, 'contentRect y is the top padding');
            eq(entries[0].borderBoxSize[0].inlineSize, 300, 'borderBoxSize inline');
            eq(entries[0].borderBoxSize[0].blockSize, 100, 'borderBoxSize block');
            eq(entries[0].contentBoxSize[0].inlineSize, 280, 'contentBoxSize inline');
            seen.push('delivered');
        }});
        o.observe(target);
        eq(seen.length, 0, 'observe alone delivers nothing');
    "
    ))
    .expect("the script runs");
    assert_eq!(
        ctx.deliver_layout_observations(),
        1,
        "the checkpoint delivers the first observation"
    );
    check(&mut ctx, 11);
    assert_eq!(
        ctx.deliver_layout_observations(),
        0,
        "a checkpoint no geometry moved for delivers nothing"
    );
}

#[test]
fn a_moved_box_reports_and_a_still_one_does_not() {
    let (mut ctx, element_box) = context();
    ctx.eval(&format!(
        "{ASSERT}
        var o = new ResizeObserver(function (entries) {{
            seen.push(entries[0].contentBoxSize[0].inlineSize);
        }});
        o.observe(target);
    "
    ))
    .expect("the script runs");
    assert_eq!(ctx.deliver_layout_observations(), 1, "the first pass");

    // A layout that changed nothing marks the checkpoint and reports nothing.
    ctx.request_layout_observation();
    assert_eq!(ctx.deliver_layout_observations(), 0, "an unmoved box");

    let mut moved = element_box.get();
    moved[2] = 500.0;
    element_box.set(moved);
    ctx.request_layout_observation();
    assert_eq!(ctx.deliver_layout_observations(), 1, "a moved box");

    ctx.eval(
        r"
        eq(seen.length, 2, 'two deliveries');
        eq(seen[0], 280, 'the observed content width');
        eq(seen[1], 480, 'the content width after the box grew');
    ",
    )
    .expect("the script runs");
    check(&mut ctx, 3);
}

#[test]
fn the_border_box_option_observes_the_border_box() {
    let (mut ctx, element_box) = context();
    ctx.eval(&format!(
        "{ASSERT}
        var o = new ResizeObserver(function (entries) {{
            seen.push(entries[0].borderBoxSize[0].inlineSize);
        }});
        o.observe(target, {{ box: 'border-box' }});
    "
    ))
    .expect("the script runs");
    assert_eq!(ctx.deliver_layout_observations(), 1, "the first pass");

    // The border box holds while the paddings move, so a content-box
    // observation would report and a border-box one does not.
    let mut repadded = element_box.get();
    repadded[8] = 20.0;
    element_box.set(repadded);
    ctx.request_layout_observation();
    assert_eq!(
        ctx.deliver_layout_observations(),
        0,
        "the border box did not move"
    );
    ctx.eval("eq(seen.length, 1, 'one delivery'); eq(seen[0], 300, 'the border width');")
        .expect("the script runs");
    check(&mut ctx, 2);
}

#[test]
fn unobserve_and_disconnect_end_the_observation() {
    let (mut ctx, element_box) = context();
    ctx.eval(&format!(
        "{ASSERT}
        var kept = document.getElementById('other');
        var o = new ResizeObserver(function (entries) {{ seen.push(entries.length); }});
        o.observe(target);
        o.observe(kept);
    "
    ))
    .expect("the script runs");
    assert_eq!(ctx.deliver_layout_observations(), 1, "the first pass");
    ctx.eval("eq(seen[0], 2, 'both observations ride one callback'); o.unobserve(target);")
        .expect("the script runs");

    let mut moved = element_box.get();
    moved[2] = 640.0;
    element_box.set(moved);
    ctx.request_layout_observation();
    assert_eq!(
        ctx.deliver_layout_observations(),
        1,
        "the remaining observation still reports"
    );
    ctx.eval("eq(seen[1], 1, 'the unobserved target is gone'); o.disconnect();")
        .expect("the script runs");

    let mut moved_again = element_box.get();
    moved_again[2] = 700.0;
    element_box.set(moved_again);
    ctx.request_layout_observation();
    assert_eq!(
        ctx.deliver_layout_observations(),
        0,
        "a disconnected observer reports nothing"
    );
    check(&mut ctx, 2);
}

#[test]
fn a_page_with_no_observation_skips_the_checkpoint() {
    // The count gate is what keeps the per-frame cost at one Cell read for a
    // page that constructs no observer.
    let (mut ctx, _box) = context();
    ctx.request_layout_observation();
    assert!(
        !ctx.layout_observation_pending(),
        "a mark with no observation stays unmarked"
    );
    assert_eq!(ctx.deliver_layout_observations(), 0, "nothing to deliver");
}
