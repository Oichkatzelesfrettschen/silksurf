/*
 * IntersectionObserver against the layout observation checkpoint.
 *
 * The provider stands in for the engine: it answers one border box per node,
 * and the test moves the target the way a scroll does. What the cases pin is
 * the geometry -- the root rectangle, rootMargin in pixels and percent, and
 * the threshold crossing -- against a viewport the test sets.
 */

use silksurf_dom::Dom;
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Mutex},
};

/// Boxes keyed by node id, so the root and the target answer separately.
type Boxes = Rc<RefCell<HashMap<usize, silksurf_js::ElementBox>>>;

/// A border box carrying no borders and no paddings.
fn plain(x: f32, y: f32, width: f32, height: f32) -> silksurf_js::ElementBox {
    [x, y, width, height, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
}

/// A document holding a target and a root element, with a 1000x800 viewport.
fn context() -> (silksurf_js::SilkContext, Boxes, usize) {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let root = dom.create_element("div");
    let _ = dom.set_attribute(root, "id", "root");
    let target = dom.create_element("div");
    let _ = dom.set_attribute(target, "id", "target");
    let _ = dom.append_child(root, target);
    let _ = dom.append_child(doc, root);
    dom.materialize_resolve_table();
    let target_raw = target.raw();
    let mut ctx = silksurf_js::SilkContext::with_dom(&Arc::new(Mutex::new(dom)));
    ctx.set_viewport(1000.0, 800.0);
    let boxes: Boxes = Rc::new(RefCell::new(HashMap::new()));
    boxes
        .borrow_mut()
        .insert(root.raw(), plain(0.0, 0.0, 400.0, 300.0));
    // The target starts fully inside both the viewport and the root element.
    boxes
        .borrow_mut()
        .insert(target_raw, plain(10.0, 10.0, 100.0, 100.0));
    let handle = Rc::clone(&boxes);
    ctx.set_geometry_provider(Rc::new(move |node| {
        handle.borrow().get(&node.raw()).copied()
    }));
    (ctx, boxes, target_raw)
}

/*
 * Assertions record rather than throw: a throw inside an observer callback
 * reaches the page's error path and never the embedder, so a test written
 * that way passes whatever it asserts.
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
function near(got, want, label) {
    globalThis.__checked++;
    if (Math.abs(got - want) > 0.001 && !globalThis.__failure) {
        globalThis.__failure = label + ': want ' + want + ' got ' + got;
    }
}
var target = document.getElementById('target');
var root = document.getElementById('root');
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

/// Move the target's border box and mark the checkpoint the way a scroll does.
fn move_target(ctx: &silksurf_js::SilkContext, boxes: &Boxes, target: usize, y: f32) {
    let mut found = boxes.borrow_mut();
    // UNWRAP-OK: the fixture inserted the target's box before any test ran.
    let existing = found.get_mut(&target).expect("the target has a box");
    existing[1] = y;
    drop(found);
    ctx.request_layout_observation();
}

#[test]
fn the_constructor_and_prototype_exist() {
    let (mut ctx, _boxes, _target) = context();
    ctx.eval(
        r"
        if (typeof IntersectionObserver !== 'function') { throw new Error('constructor'); }
        var o = new IntersectionObserver(function () {});
        if (typeof o.observe !== 'function') { throw new Error('observe'); }
        if (typeof o.unobserve !== 'function') { throw new Error('unobserve'); }
        if (typeof o.disconnect !== 'function') { throw new Error('disconnect'); }
        if (typeof o.takeRecords !== 'function') { throw new Error('takeRecords'); }
        if (Object.prototype.toString.call(o) !== '[object IntersectionObserver]') { throw new Error('toStringTag'); }
        if (o.root !== null) { throw new Error('a missing root is the viewport'); }
        if (o.thresholds.length !== 1 || o.thresholds[0] !== 0) { throw new Error('the default threshold is 0'); }
        var threw = false;
        try { new IntersectionObserver(); } catch (e) { threw = e instanceof TypeError; }
        if (!threw) { throw new Error('a missing callback is a TypeError'); }
        threw = false;
        try { new IntersectionObserver(function () {}, { threshold: 1.5 }); } catch (e) { threw = e instanceof RangeError; }
        if (!threw) { throw new Error('a threshold outside [0, 1] is a RangeError'); }
        threw = false;
        try { new IntersectionObserver(function () {}, { rootMargin: '10em' }); } catch (e) { threw = e instanceof SyntaxError; }
        if (!threw) { throw new Error('a rootMargin unit that is not px or % is a SyntaxError'); }
    ",
    )
    .expect("the object shape a page reads");
}

#[test]
fn an_observation_reports_once_against_the_viewport() {
    // A sentinel already on screen reports without waiting for a scroll,
    // which is what a lazy loader depends on to fetch its first page.
    let (mut ctx, _boxes, _target) = context();
    ctx.eval(&format!(
        "{ASSERT}
        var o = new IntersectionObserver(function (entries, observer) {{
            eq(entries.length, 1, 'one entry');
            eq(entries[0].target, target, 'target');
            eq(observer, o, 'the observer is the second argument');
            eq(entries[0].isIntersecting, true, 'isIntersecting');
            near(entries[0].intersectionRatio, 1, 'a fully visible target');
            eq(entries[0].rootBounds.width, 1000, 'the root is the viewport');
            eq(entries[0].rootBounds.height, 800, 'the viewport height');
            eq(entries[0].boundingClientRect.top, 10, 'the target rect');
            eq(entries[0].intersectionRect.width, 100, 'the intersection rect');
            seen.push('delivered');
        }});
        o.observe(target);
        eq(seen.length, 0, 'observe alone delivers nothing');
    "
    ))
    .expect("the script runs");
    assert_eq!(ctx.deliver_layout_observations(), 1, "the first pass");
    check(&mut ctx, 10);
    assert_eq!(
        ctx.deliver_layout_observations(),
        0,
        "a checkpoint no geometry moved for delivers nothing"
    );
}

#[test]
fn a_target_leaving_the_viewport_reports_the_crossing() {
    let (mut ctx, boxes, target) = context();
    ctx.eval(&format!(
        "{ASSERT}
        var o = new IntersectionObserver(function (entries) {{
            seen.push(entries[0].isIntersecting);
        }});
        o.observe(target);
    "
    ))
    .expect("the script runs");
    assert_eq!(ctx.deliver_layout_observations(), 1, "the first pass");

    // Still inside the 800-tall viewport, so the threshold index holds.
    move_target(&ctx, &boxes, target, 400.0);
    assert_eq!(ctx.deliver_layout_observations(), 0, "no crossing");

    // Scrolled past the bottom edge.
    move_target(&ctx, &boxes, target, 900.0);
    assert_eq!(ctx.deliver_layout_observations(), 1, "the target left");

    move_target(&ctx, &boxes, target, 100.0);
    assert_eq!(ctx.deliver_layout_observations(), 1, "the target returned");
    ctx.eval(
        r"
        eq(seen.length, 3, 'three deliveries');
        eq(seen[0], true, 'observed on screen');
        eq(seen[1], false, 'left the viewport');
        eq(seen[2], true, 'returned to the viewport');
    ",
    )
    .expect("the script runs");
    check(&mut ctx, 4);
}

#[test]
fn a_pixel_root_margin_reports_the_target_before_it_arrives() {
    // rootMargin `0px 0px 200px 0px` is the shape a lazy loader uses to fetch
    // ahead of the viewport, and the bundles carry `1000px 0px 1000px 0px`.
    let (mut ctx, boxes, target) = context();
    ctx.eval(&format!(
        "{ASSERT}
        var o = new IntersectionObserver(function (entries) {{
            seen.push(entries[0].isIntersecting);
            near(entries[0].rootBounds.bottom, 1000, 'the margin extends the root');
        }}, {{ rootMargin: '0px 0px 200px 0px' }});
        o.observe(target);
    "
    ))
    .expect("the script runs");
    assert_eq!(ctx.deliver_layout_observations(), 1, "the first pass");

    // Below the 800-tall viewport, inside the 200px margin.
    move_target(&ctx, &boxes, target, 850.0);
    assert_eq!(
        ctx.deliver_layout_observations(),
        0,
        "the margin keeps it intersecting"
    );

    // Past the margin.
    move_target(&ctx, &boxes, target, 1050.0);
    assert_eq!(ctx.deliver_layout_observations(), 1, "past the margin");
    ctx.eval("eq(seen.length, 2, 'two deliveries'); eq(seen[1], false, 'left the extended root');")
        .expect("the script runs");
    check(&mut ctx, 4);
}

#[test]
fn a_percent_root_margin_resolves_against_the_root_rectangle() {
    // A percentage resolves against the root's own height on the top and
    // bottom edges: 25% of the 800-tall viewport is 200px.
    let (mut ctx, boxes, target) = context();
    ctx.eval(&format!(
        "{ASSERT}
        var o = new IntersectionObserver(function (entries) {{
            near(entries[0].rootBounds.bottom, 1000, 'the bottom edge');
            near(entries[0].rootBounds.top, -200, 'the top edge');
            seen.push(entries[0].isIntersecting);
        }}, {{ rootMargin: '25%' }});
        o.observe(target);
    "
    ))
    .expect("the script runs");
    assert_eq!(ctx.deliver_layout_observations(), 1, "the first pass");
    move_target(&ctx, &boxes, target, 950.0);
    assert_eq!(
        ctx.deliver_layout_observations(),
        0,
        "inside the extended root"
    );
    move_target(&ctx, &boxes, target, 1100.0);
    assert_eq!(ctx.deliver_layout_observations(), 1, "past it");
    check(&mut ctx, 4);
}

#[test]
fn a_threshold_reports_when_the_ratio_crosses_it() {
    let (mut ctx, boxes, target) = context();
    ctx.eval(&format!(
        "{ASSERT}
        var o = new IntersectionObserver(function (entries) {{
            seen.push(Math.round(entries[0].intersectionRatio * 100) / 100);
        }}, {{ threshold: 0.5 }});
        o.observe(target);
    "
    ))
    .expect("the script runs");
    assert_eq!(ctx.deliver_layout_observations(), 1, "the first pass");

    // 30 of the target's 100 rows remain above the viewport's bottom edge.
    move_target(&ctx, &boxes, target, 770.0);
    assert_eq!(
        ctx.deliver_layout_observations(),
        1,
        "the ratio fell below the threshold"
    );

    // A further scroll inside the same band reports nothing.
    move_target(&ctx, &boxes, target, 780.0);
    assert_eq!(ctx.deliver_layout_observations(), 0, "the same band");

    // Leaving the viewport entirely holds the threshold index at zero, so
    // isIntersecting is the dimension that reports it.
    move_target(&ctx, &boxes, target, 900.0);
    assert_eq!(
        ctx.deliver_layout_observations(),
        1,
        "leaving under an unreached threshold still reports"
    );
    ctx.eval(
        r"
        eq(seen.length, 3, 'three deliveries');
        near(seen[0], 1, 'fully visible');
        near(seen[1], 0.3, 'partly visible');
        near(seen[2], 0, 'gone');
    ",
    )
    .expect("the script runs");
    check(&mut ctx, 4);
}

#[test]
fn an_element_root_clips_against_its_padding_box() {
    // The root element is 400x300 at the origin while the viewport is
    // 1000x800, so a target inside the viewport and below the root does not
    // intersect.
    let (mut ctx, boxes, target) = context();
    ctx.eval(&format!(
        "{ASSERT}
        var o = new IntersectionObserver(function (entries) {{
            seen.push(entries[0].isIntersecting);
            eq(entries[0].rootBounds.height, 300, 'the root element bounds');
        }}, {{ root: root }});
        o.observe(target);
    "
    ))
    .expect("the script runs");
    assert_eq!(ctx.deliver_layout_observations(), 1, "the first pass");
    move_target(&ctx, &boxes, target, 400.0);
    assert_eq!(
        ctx.deliver_layout_observations(),
        1,
        "below the root and inside the viewport"
    );
    ctx.eval("eq(seen[0], true, 'inside the root'); eq(seen[1], false, 'below the root');")
        .expect("the script runs");
    check(&mut ctx, 4);
}

#[test]
fn unobserve_and_disconnect_end_the_observation() {
    let (mut ctx, boxes, target) = context();
    ctx.eval(&format!(
        "{ASSERT}
        var o = new IntersectionObserver(function (entries) {{ seen.push(entries.length); }});
        o.observe(target);
        o.observe(target);
        eq(seen.length, 0, 'a second observe on one target adds nothing');
    "
    ))
    .expect("the script runs");
    assert_eq!(
        ctx.deliver_layout_observations(),
        1,
        "one observation, not two"
    );
    ctx.eval("eq(seen[0], 1, 'one entry'); o.unobserve(target);")
        .expect("the script runs");
    move_target(&ctx, &boxes, target, 900.0);
    assert_eq!(
        ctx.deliver_layout_observations(),
        0,
        "an unobserved target reports nothing"
    );
    check(&mut ctx, 2);
}
