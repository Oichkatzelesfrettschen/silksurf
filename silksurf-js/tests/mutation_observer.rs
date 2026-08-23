/*
 * MutationObserver's object behavior and record routing.
 *
 * crates/silksurf-dom/tests/mutation_records.rs covers what the tree queues;
 * these cases cover what reaches a callback: option matching, subtree scope,
 * attribute filters, old values, delivery timing against promise
 * continuations, and the registration lifecycle.
 */

use silksurf_dom::Dom;
use std::sync::{Arc, Mutex};

/// A document holding one div, with the JS half installed.
fn context() -> silksurf_js::SilkContext {
    let mut dom = Dom::new();
    let root = dom.create_document();
    let body = dom.create_element("body");
    let _ = dom.set_attribute(body, "id", "body");
    let div = dom.create_element("div");
    let _ = dom.set_attribute(div, "id", "target");
    let _ = dom.append_child(body, div);
    let _ = dom.append_child(root, body);
    dom.materialize_resolve_table();
    silksurf_js::SilkContext::with_dom(&Arc::new(Mutex::new(dom)))
}

/*
 * Assertions record rather than throw, because a throw inside a promise
 * callback rejects that promise and the rejection never reaches the embedder:
 * a test written that way passes whatever it asserts. The runner reads the
 * recorded failure after the microtask queue drains, and requires a minimum
 * assertion count so a chain that never ran fails rather than passes empty.
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

fn run(script: &str, expected_assertions: u32) {
    let mut ctx = context();
    ctx.eval(&format!("{ASSERT}{script}"))
        .expect("script runs without throwing");
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
    run(
        r"
        eq(typeof MutationObserver, 'function', 'constructor');
        var o = new MutationObserver(function () {});
        eq(typeof o.observe, 'function', 'observe');
        eq(typeof o.disconnect, 'function', 'disconnect');
        eq(typeof o.takeRecords, 'function', 'takeRecords');
        eq(Object.prototype.toString.call(o), '[object MutationObserver]', 'toStringTag');
        var threw = false;
        try { new MutationObserver(); } catch (e) { threw = e instanceof TypeError; }
        eq(threw, true, 'a missing callback is a TypeError');
        threw = false;
        try { o.observe(target, {}); } catch (e) { threw = e instanceof TypeError; }
        eq(threw, true, 'an option bag naming nothing is a TypeError');
    ",
        7,
    );
}

#[test]
fn a_child_list_record_reaches_the_callback_with_the_added_node() {
    run(
        r"
        var o = new MutationObserver(function (records) { seen = records; });
        o.observe(target, { childList: true });
        var child = document.createElement('span');
        target.appendChild(child);
        eq(seen.length, 0, 'delivery waits for the microtask checkpoint');
        Promise.resolve().then(function () {
            eq(seen.length, 1, 'one record');
            eq(seen[0].type, 'childList', 'type');
            eq(seen[0].target, target, 'target identity');
            eq(seen[0].addedNodes.length, 1, 'one added node');
            eq(seen[0].addedNodes[0], child, 'added node identity');
            eq(seen[0].removedNodes.length, 0, 'nothing removed');
            globalThis.__done = true;
        });
    ",
        7,
    );
}

#[test]
fn delivery_runs_before_a_continuation_registered_after_the_mutation() {
    // DOM 4.4.3 queues the notify-mutation-observers microtask when the record
    // is queued, so a then registered afterwards runs second.
    run(
        r"
        var order = [];
        var o = new MutationObserver(function () { order.push('observer'); });
        o.observe(target, { attributes: true });
        target.setAttribute('data-x', '1');
        Promise.resolve().then(function () { order.push('promise'); });
        Promise.resolve().then(function () {}).then(function () {
            eq(order.join(','), 'observer,promise', 'ordering');
        });
    ",
        1,
    );
}

#[test]
fn subtree_decides_whether_a_descendant_mutation_is_reported() {
    run(
        r"
        var inner = document.createElement('p');
        target.appendChild(inner);
        var shallow = [], deep = [];
        var a = new MutationObserver(function (r) { shallow = shallow.concat(r); });
        var b = new MutationObserver(function (r) { deep = deep.concat(r); });
        a.observe(target, { attributes: true });
        b.observe(target, { attributes: true, subtree: true });
        Promise.resolve().then(function () {
            shallow = []; deep = [];
            inner.setAttribute('data-y', '2');
            return Promise.resolve();
        }).then(function () {
            eq(shallow.length, 0, 'a shallow registration skips a descendant');
            eq(deep.length, 1, 'a subtree registration takes it');
            eq(deep[0].target, inner, 'the descendant is the target');
        });
    ",
        3,
    );
}

#[test]
fn an_attribute_filter_drops_the_names_it_does_not_list() {
    run(
        r"
        var o = new MutationObserver(function (r) { seen = seen.concat(r); });
        o.observe(target, { attributes: true, attributeFilter: ['data-keep'] });
        target.setAttribute('data-drop', '1');
        target.setAttribute('data-keep', '2');
        Promise.resolve().then(function () {
            eq(seen.length, 1, 'one record survives the filter');
            eq(seen[0].attributeName, 'data-keep', 'the listed name');
        });
    ",
        2,
    );
}

#[test]
fn old_value_is_present_only_when_the_registration_asks_for_it() {
    run(
        r"
        target.setAttribute('data-v', 'first');
        var withOld = [], without = [];
        var a = new MutationObserver(function (r) { withOld = withOld.concat(r); });
        var b = new MutationObserver(function (r) { without = without.concat(r); });
        a.observe(target, { attributes: true, attributeOldValue: true });
        b.observe(target, { attributes: true });
        Promise.resolve().then(function () {
            withOld = []; without = [];
            target.setAttribute('data-v', 'second');
            return Promise.resolve();
        }).then(function () {
            eq(withOld.length, 1, 'requested');
            eq(withOld[0].oldValue, 'first', 'the replaced value');
            eq(without.length, 1, 'not requested');
            eq(without[0].oldValue, null, 'null when the registration did not ask');
        });
    ",
        4,
    );
}

#[test]
fn character_data_reports_the_text_a_write_replaced() {
    run(
        r"
        var text = document.createTextNode('one');
        target.appendChild(text);
        var o = new MutationObserver(function (r) { seen = seen.concat(r); });
        o.observe(target, { characterData: true, characterDataOldValue: true, subtree: true });
        Promise.resolve().then(function () {
            seen = [];
            text.textContent = 'two';
            return Promise.resolve();
        }).then(function () {
            eq(seen.length, 1, 'one record');
            eq(seen[0].type, 'characterData', 'type');
            eq(seen[0].oldValue, 'one', 'the replaced text');
        });
    ",
        3,
    );
}

#[test]
fn take_records_drains_the_queue_before_the_callback_would_run() {
    run(
        r"
        var fired = 0;
        var o = new MutationObserver(function () { fired++; });
        o.observe(target, { childList: true });
        target.appendChild(document.createElement('i'));
        var taken = o.takeRecords();
        eq(taken.length, 1, 'takeRecords returns the pending record');
        eq(o.takeRecords().length, 0, 'the queue is empty after a take');
        Promise.resolve().then(function () { eq(fired, 0, 'a drained queue fires no callback'); });
    ",
        3,
    );
}

#[test]
fn disconnect_stops_delivery_and_a_second_observe_replaces_its_registration() {
    run(
        r"
        var fired = 0;
        var o = new MutationObserver(function () { fired++; });
        o.observe(target, { childList: true });
        o.observe(target, { attributes: true });
        target.appendChild(document.createElement('u'));
        Promise.resolve().then(function () {
            eq(fired, 0, 'the second observe replaced the childList registration');
            o.disconnect();
            target.setAttribute('data-z', '1');
            return Promise.resolve();
        }).then(function () {
            eq(fired, 0, 'a disconnected observer takes nothing');
        });
    ",
        2,
    );
}

#[test]
fn inner_html_reports_one_record_for_the_splice() {
    run(
        r"
        var o = new MutationObserver(function (r) { seen = seen.concat(r); });
        o.observe(target, { childList: true, subtree: true });
        target.innerHTML = '<section><p>deep</p></section>';
        Promise.resolve().then(function () {
            eq(seen.length, 1, 'the splice is one record, not one per parsed node');
            eq(seen[0].type, 'childList', 'type');
            eq(seen[0].addedNodes.length, 1, 'the section is the addition');
        });
    ",
        3,
    );
}

#[test]
fn a_disconnected_observer_leaves_the_live_list() {
    // The live list is what keeps an observer reachable, so a page that
    // constructs one per component and disconnects it accumulates nothing.
    run(
        r"
        var fired = 0;
        for (var i = 0; i < 50; i++) {
            var o = new MutationObserver(function () { fired++; });
            o.observe(target, { childList: true });
            o.disconnect();
        }
        var live = new MutationObserver(function () { fired++; });
        live.observe(target, { childList: true });
        target.appendChild(document.createElement('b'));
    ",
        0,
    );
}

#[test]
fn a_document_the_html_parser_built_queues_records() {
    // Connectedness is measured against the root `Dom::create_document`
    // registers, and the tree sink calls it; a parser that stopped would leave
    // every record filtered out and every observer silent.
    let dom = silksurf_html::parse_html("<html><body><div id='t'>hi</div></body></html>");
    let mut ctx = silksurf_js::SilkContext::with_dom(&Arc::new(Mutex::new(dom)));
    ctx.eval(
        r"
        var t = document.getElementById('t');
        globalThis.got = 0;
        var o = new MutationObserver(function (r) { globalThis.got = r.length; });
        o.observe(t, { childList: true });
        t.appendChild(document.createElement('span'));
    ",
    )
    .expect("setup runs");
    ctx.eval(
        "if (globalThis.got !== 1) { throw new Error('queued ' + globalThis.got + ' records'); }",
    )
    .expect("a parser-built document queues records");
}
