/*
 * MutationObserver.
 *
 * DOM 4.3 queues a mutation record on the tree and delivers it from a
 * microtask, so the two halves live where each belongs: silksurf_dom owns the
 * record queue behind its mutators, and this module turns queued records into
 * the objects a callback receives and schedules the delivery.
 *
 * Delivery is a job enqueued the moment a mutation queues a record, which is
 * what puts the callback ahead of a promise continuation registered after the
 * mutation. `notify` is called by every native in dom_bridge that reaches a
 * Dom mutator -- ten of them -- and `deliver_pending` repeats the check at the
 * microtask checkpoint so a native added later without the call delivers late
 * rather than not at all.
 *
 * Recording opens when the first observer calls observe and closes when the
 * last one disconnects, so a document with no observer pays one bool test per
 * mutation.
 */

use std::sync::{Arc, Mutex, PoisonError};

use boa_engine::{
    Context, JsResult, JsValue, NativeFunction,
    job::PromiseJob,
    js_string,
    object::{JsObject, ObjectInitializer, builtins::JsArray},
    property::Attribute,
};
use silksurf_dom::{Dom, MutationKind, MutationRecord, NodeId};

use super::dom_bridge::node_to_js_object;

/// Global naming the JS half's delivery entry point.
const DELIVER: &str = "__silksurfDeliverMutations";
/// Global holding the "mutation observer compound microtask queued" flag.
const QUEUED: &str = "__silksurfMutationQueued";

/// Enqueue the delivery microtask when a mutation has queued a record and no
/// delivery is already pending. Called from every `dom_bridge` native that
/// reaches a `Dom` mutator.
pub(super) fn notify(dom_arc: &Arc<Mutex<Dom>>, ctx: &mut Context) {
    let pending = {
        let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
        dom.pending_mutation_records()
    };
    if pending == 0 {
        return;
    }
    let global = ctx.global_object();
    let already = global
        .get(js_string!(QUEUED), ctx)
        .map(|value| value.to_boolean())
        .unwrap_or(false);
    if already {
        return;
    }
    let _ = global.set(js_string!(QUEUED), JsValue::from(true), false, ctx);
    ctx.enqueue_job(PromiseJob::new(run_delivery).into());
}

/// Run the JS half's delivery function. The job is a promise job because
/// `SimpleJobExecutor` drains those first-in-first-out, which puts a delivery
/// enqueued at mutation time ahead of a continuation registered after it. The
/// job captures no JS value, so the garbage collector needs no knowledge of
/// it; the function is read from the global object when the job runs.
fn run_delivery(ctx: &mut Context) -> JsResult<JsValue> {
    let global = ctx.global_object();
    let deliver = global.get(js_string!(DELIVER), ctx)?;
    let Some(callable) = deliver.as_callable() else {
        return Ok(JsValue::undefined());
    };
    callable.call(&JsValue::undefined(), &[], ctx)
}

/// Deliver at the microtask checkpoint whatever the per-native calls missed.
/// A record left queued here reaches its callback one checkpoint late rather
/// than never.
pub(super) fn deliver_pending(dom_arc: &Arc<Mutex<Dom>>, ctx: &mut Context) {
    notify(dom_arc, ctx);
}

/// `__silksurfSetMutationRecording(on)`: open the queue while an observer is
/// registered.
fn set_recording(dom_arc: &Arc<Mutex<Dom>>) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: the closure owns an Arc handle and captures no JS value, so the
    // garbage collector traces nothing through it.
    unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let on = args.first().is_some_and(JsValue::to_boolean);
            let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            dom.set_mutation_recording(on);
            Ok(JsValue::undefined())
        })
    }
}

/// `__silksurfTakeMutationRecords()`: empty the queue and return the records
/// as objects, with every node wrapped through the bridge's wrapper cache so a
/// record's target compares equal to the element the page already holds.
fn take_records(dom_arc: &Arc<Mutex<Dom>>) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: the closure owns an Arc handle and captures no JS value, so the
    // garbage collector traces nothing through it.
    unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            // The wrapper builder re-locks the tree, so the queue drains and
            // the lock drops before any node is wrapped.
            let records = {
                let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                dom.take_mutation_records()
            };
            let array = JsArray::new(ctx);
            for record in records {
                let object = record_to_object(&arc, &record, ctx);
                array.push(object, ctx)?;
            }
            Ok(array.into())
        })
    }
}

fn node_list(arc: &Arc<Mutex<Dom>>, nodes: &[NodeId], ctx: &mut Context) -> JsValue {
    let array = JsArray::new(ctx);
    for node in nodes {
        let wrapper = node_to_js_object(arc, *node, ctx);
        let _ = array.push(wrapper, ctx);
    }
    array.into()
}

fn optional_node(arc: &Arc<Mutex<Dom>>, node: Option<NodeId>, ctx: &mut Context) -> JsValue {
    node.map_or_else(JsValue::null, |id| node_to_js_object(arc, id, ctx))
}

/// Build one record object in the shape `MutationRecord` gives it: every
/// property present on every record, null where the type does not carry one.
fn record_to_object(arc: &Arc<Mutex<Dom>>, record: &MutationRecord, ctx: &mut Context) -> JsObject {
    let target = node_to_js_object(arc, record.target, ctx);
    let empty = JsArray::new(ctx);
    let (kind, added, removed, previous, next, name, old) = match &record.kind {
        MutationKind::ChildList {
            added,
            removed,
            previous,
            next,
        } => (
            "childList",
            node_list(arc, added, ctx),
            node_list(arc, removed, ctx),
            optional_node(arc, *previous, ctx),
            optional_node(arc, *next, ctx),
            JsValue::null(),
            JsValue::null(),
        ),
        MutationKind::Attributes { name, old } => (
            "attributes",
            empty.clone().into(),
            JsArray::new(ctx).into(),
            JsValue::null(),
            JsValue::null(),
            JsValue::from(js_string!(name.as_str())),
            old.as_ref().map_or_else(JsValue::null, |text| {
                JsValue::from(js_string!(text.as_str()))
            }),
        ),
        MutationKind::CharacterData { old } => (
            "characterData",
            empty.clone().into(),
            JsArray::new(ctx).into(),
            JsValue::null(),
            JsValue::null(),
            JsValue::null(),
            JsValue::from(js_string!(old.as_str())),
        ),
    };
    ObjectInitializer::new(ctx)
        .property(js_string!("type"), js_string!(kind), Attribute::all())
        .property(js_string!("target"), target, Attribute::all())
        .property(js_string!("addedNodes"), added, Attribute::all())
        .property(js_string!("removedNodes"), removed, Attribute::all())
        .property(js_string!("previousSibling"), previous, Attribute::all())
        .property(js_string!("nextSibling"), next, Attribute::all())
        .property(js_string!("attributeName"), name, Attribute::all())
        .property(
            js_string!("attributeNamespace"),
            JsValue::null(),
            Attribute::all(),
        )
        .property(js_string!("oldValue"), old, Attribute::all())
        .build()
}

/// Install the natives and the object shape.
pub(super) fn install(ctx: &mut Context, dom_arc: &Arc<Mutex<Dom>>) {
    let _ = ctx.register_global_callable(
        js_string!("__silksurfSetMutationRecording"),
        1,
        set_recording(dom_arc),
    );
    let _ = ctx.register_global_callable(
        js_string!("__silksurfTakeMutationRecords"),
        0,
        take_records(dom_arc),
    );
    if let Err(err) = ctx.eval(boa_engine::Source::from_bytes(BOOTSTRAP.as_bytes())) {
        eprintln!("silksurf-js: MutationObserver bootstrap failed: {err}");
    }
}

/*
 * The observer registry and the record routing DOM 4.3.4 specifies. Matching
 * is expressed here rather than in Rust because it reads against the option
 * bag the page passed, and the option bag is a JS object.
 */
const BOOTSTRAP: &str = r"
(function () {
    'use strict';
    var observers = [];

    function isInclusiveAncestor(root, node) {
        for (var n = node; n; n = n.parentNode) { if (n === root) { return true; } }
        return false;
    }

    // One registration decides whether it wants a record, then how much of it.
    function matched(reg, record) {
        var o = reg.options;
        if (record.target !== reg.target && !(o.subtree && isInclusiveAncestor(reg.target, record.target))) { return null; }
        if (record.type === 'childList') { return o.childList ? clone(record, false) : null; }
        if (record.type === 'attributes') {
            if (!o.attributes) { return null; }
            if (o.attributeFilter && o.attributeFilter.indexOf(record.attributeName) === -1) { return null; }
            return clone(record, !!o.attributeOldValue);
        }
        if (record.type === 'characterData') {
            return o.characterData ? clone(record, !!o.characterDataOldValue) : null;
        }
        return null;
    }

    // oldValue is present only when the registration asked for it, which is
    // what lets a page tell 'no previous value' from 'not requested'.
    function clone(record, withOld) {
        return {
            type: record.type, target: record.target,
            addedNodes: record.addedNodes, removedNodes: record.removedNodes,
            previousSibling: record.previousSibling, nextSibling: record.nextSibling,
            attributeName: record.attributeName, attributeNamespace: record.attributeNamespace,
            oldValue: withOld ? record.oldValue : null,
        };
    }

    function route() {
        var raw = __silksurfTakeMutationRecords();
        for (var i = 0; i < raw.length; i++) {
            for (var j = 0; j < observers.length; j++) {
                var observer = observers[j];
                for (var k = 0; k < observer._registrations.length; k++) {
                    var taken = matched(observer._registrations[k], raw[i]);
                    if (taken) { observer._queue.push(taken); break; }
                }
            }
        }
    }

    globalThis.__silksurfDeliverMutations = function () {
        globalThis.__silksurfMutationQueued = false;
        route();
        // The queue is emptied before the callback runs, so a mutation the
        // callback makes queues for the next delivery rather than this one.
        for (var i = 0; i < observers.length; i++) {
            var observer = observers[i];
            if (!observer._queue.length) { continue; }
            var records = observer._queue;
            observer._queue = [];
            try { observer._callback.call(observer, records, observer); }
            catch (e) { globalThis.reportError ? reportError(e) : console.error(String(e)); }
        }
    };

    function MutationObserver(callback) {
        if (typeof callback !== 'function') { throw new TypeError('MutationObserver requires a callback'); }
        this._callback = callback;
        this._registrations = [];
        this._queue = [];
    }
    // The live list holds an observer only while it has a registration, so a
    // page constructing one per component and disconnecting it leaves nothing
    // behind. A browser keeps the observer alive through the node it watches;
    // this list is that reference.
    function track(observer) {
        if (observers.indexOf(observer) === -1) { observers.push(observer); }
    }
    function untrack(observer) {
        var at = observers.indexOf(observer);
        if (at !== -1) { observers.splice(at, 1); }
    }
    MutationObserver.prototype.observe = function (target, options) {
        if (!target) { throw new TypeError('MutationObserver.observe requires a target'); }
        var o = options || {};
        var wanted = {
            childList: !!o.childList,
            attributes: o.attributes === undefined ? !!o.attributeFilter || !!o.attributeOldValue : !!o.attributes,
            characterData: o.characterData === undefined ? !!o.characterDataOldValue : !!o.characterData,
            subtree: !!o.subtree,
            attributeOldValue: !!o.attributeOldValue,
            characterDataOldValue: !!o.characterDataOldValue,
            attributeFilter: o.attributeFilter ? Array.prototype.slice.call(o.attributeFilter) : null,
        };
        if (!wanted.childList && !wanted.attributes && !wanted.characterData) {
            throw new TypeError('MutationObserver.observe needs childList, attributes, or characterData');
        }
        // A second observe on one target replaces the first registration,
        // which is what makes re-observing with new options not accumulate.
        track(this);
        for (var i = 0; i < this._registrations.length; i++) {
            if (this._registrations[i].target === target) { this._registrations[i].options = wanted; return; }
        }
        this._registrations.push({ target: target, options: wanted });
        __silksurfSetMutationRecording(true);
    };
    MutationObserver.prototype.disconnect = function () {
        this._registrations = [];
        this._queue = [];
        untrack(this);
        if (!observers.length) { __silksurfSetMutationRecording(false); }
    };
    MutationObserver.prototype.takeRecords = function () {
        route();
        var records = this._queue;
        this._queue = [];
        return records;
    };
    Object.defineProperty(MutationObserver.prototype, Symbol.toStringTag, {
        configurable: true, value: 'MutationObserver',
    });
    globalThis.MutationObserver = MutationObserver;
    globalThis.__silksurfMutationQueued = false;
})();
";
