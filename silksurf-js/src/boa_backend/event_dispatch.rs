/*
 * event_dispatch implements DOM event propagation over the bridge.
 *
 * The dispatch algorithm follows DOM Living Standard section 2.9 ("Dispatching
 * events"): the ancestor path is captured once at dispatch time, the capture
 * phase walks root-to-parent, the target phase runs both listener lists at the
 * target, and the bubble phase walks parent-to-root when `bubbles` is set.
 *
 * The listener registry stays on the JS heap (the hidden global
 * `__silksurfEventListeners`), so boa's GC traces callbacks for free. Each
 * registry value is `{ bubble: [...], capture: [...] }`; each entry is
 * `{ callback, once }`. A sibling counter object
 * `__silksurfListenerTypeCounts` maps event type -> live listener count, which
 * lets the embedder skip synthesizing input events on pages with no listeners.
 *
 * Lock discipline: the Dom mutex is held only while snapshotting the ancestor
 * path and never across a listener invocation. Listeners re-enter DOM APIs
 * that take the same lock; holding it across the call would deadlock.
 */

use std::sync::{Arc, Mutex, PoisonError};

use boa_engine::{
    Context, JsResult, JsString, JsValue, NativeFunction, js_string,
    object::{JsObject, ObjectInitializer, builtins::JsArray},
    property::Attribute,
};
use silksurf_dom::{Dom, NodeId};

pub(super) const EVENT_LISTENERS_REGISTRY: &str = "__silksurfEventListeners";
pub(super) const LISTENER_TYPE_COUNTS: &str = "__silksurfListenerTypeCounts";

const CAPTURING_PHASE: u32 = 1;
const AT_TARGET: u32 = 2;
const BUBBLING_PHASE: u32 = 3;

// ---- registry access --------------------------------------------------------

pub(super) fn hidden_global_object(name: &str, ctx: &mut Context) -> JsResult<JsObject> {
    let key = JsString::from(name);
    let global = ctx.global_object().clone();
    let existing = global.get(key.clone(), ctx)?;
    if let Some(object) = existing.as_object() {
        return Ok(object.clone());
    }
    let object = ObjectInitializer::new(ctx).build();
    global.set(key, object.clone(), false, ctx)?;
    Ok(object)
}

fn event_listener_registry(ctx: &mut Context) -> JsResult<JsObject> {
    hidden_global_object(EVENT_LISTENERS_REGISTRY, ctx)
}

fn listener_key(node_id: NodeId, event_type: &str) -> JsString {
    JsString::from(format!("{}:{event_type}", node_id.raw()).as_str())
}

fn phase_list_name(capture: bool) -> JsString {
    if capture {
        js_string!("capture")
    } else {
        js_string!("bubble")
    }
}

/// Fetch (or create) the `{bubble, capture}` record for a node/type pair and
/// return the requested phase list.
fn listener_list(
    node_id: NodeId,
    event_type: &str,
    capture: bool,
    create: bool,
    ctx: &mut Context,
) -> JsResult<Option<JsArray>> {
    let registry = event_listener_registry(ctx)?;
    let key = listener_key(node_id, event_type);
    let existing = registry.get(key.clone(), ctx)?;
    let record = if let Some(object) = existing.as_object() {
        object.clone()
    } else {
        if !create {
            return Ok(None);
        }
        let bubble = JsArray::new(ctx);
        let capture_list = JsArray::new(ctx);
        let record = ObjectInitializer::new(ctx)
            .property(js_string!("bubble"), bubble, Attribute::all())
            .property(js_string!("capture"), capture_list, Attribute::all())
            .build();
        registry.set(key, record.clone(), false, ctx)?;
        record
    };
    let list = record.get(phase_list_name(capture), ctx)?;
    match list.as_object() {
        Some(object) if object.is_array() => Ok(Some(JsArray::from_object(object.clone())?)),
        _ => Ok(None),
    }
}

// ---- listener-type counters --------------------------------------------------

fn bump_listener_type_count(event_type: &str, delta: f64, ctx: &mut Context) -> JsResult<()> {
    let counts = hidden_global_object(LISTENER_TYPE_COUNTS, ctx)?;
    let key = JsString::from(event_type);
    let current = counts.get(key.clone(), ctx)?.to_number(ctx)?;
    let current = if current.is_finite() { current } else { 0.0 };
    let next = (current + delta).max(0.0);
    counts.set(key, JsValue::from(next), false, ctx)?;
    Ok(())
}

/// True when at least one listener for `event_type` is registered anywhere in
/// the document. Embedders consult this before synthesizing input events.
pub(super) fn any_listener_for_type(event_type: &str, ctx: &mut Context) -> JsResult<bool> {
    let counts = hidden_global_object(LISTENER_TYPE_COUNTS, ctx)?;
    let count = counts
        .get(JsString::from(event_type), ctx)?
        .to_number(ctx)?;
    Ok(count.is_finite() && count >= 1.0)
}

// ---- add / remove ------------------------------------------------------------

/// Third-argument form of addEventListener: bool-or-`{capture, once}`.
struct ListenerOptions {
    capture: bool,
    once: bool,
}

fn parse_listener_options(arg: Option<&JsValue>, ctx: &mut Context) -> JsResult<ListenerOptions> {
    let Some(value) = arg else {
        return Ok(ListenerOptions {
            capture: false,
            once: false,
        });
    };
    if let Some(object) = value.as_object() {
        let capture = object.get(js_string!("capture"), ctx)?.to_boolean();
        let once = object.get(js_string!("once"), ctx)?.to_boolean();
        return Ok(ListenerOptions { capture, once });
    }
    Ok(ListenerOptions {
        capture: value.to_boolean(),
        once: false,
    })
}

pub(super) fn add_listener(
    node_id: NodeId,
    event_type: &str,
    callback: &JsObject,
    options: Option<&JsValue>,
    ctx: &mut Context,
) -> JsResult<()> {
    let options = parse_listener_options(options, ctx)?;
    let Some(list) = listener_list(node_id, event_type, options.capture, true, ctx)? else {
        return Ok(());
    };
    let callback_value = JsValue::from(callback.clone());
    let length = list.length(ctx)?;
    for index in 0..length {
        let entry = list.get(index, ctx)?;
        if let Some(entry) = entry.as_object()
            && entry
                .get(js_string!("callback"), ctx)?
                .strict_equals(&callback_value)
        {
            return Ok(());
        }
    }
    let entry = ObjectInitializer::new(ctx)
        .property(js_string!("callback"), callback_value, Attribute::all())
        .property(js_string!("once"), options.once, Attribute::all())
        .build();
    list.push(entry, ctx)?;
    bump_listener_type_count(event_type, 1.0, ctx)
}

pub(super) fn remove_listener(
    node_id: NodeId,
    event_type: &str,
    callback: &JsObject,
    options: Option<&JsValue>,
    ctx: &mut Context,
) -> JsResult<()> {
    let options = parse_listener_options(options, ctx)?;
    let Some(list) = listener_list(node_id, event_type, options.capture, false, ctx)? else {
        return Ok(());
    };
    let callback_value = JsValue::from(callback.clone());
    let mut write_index = 0_u64;
    let mut removed = 0.0_f64;
    let length = list.length(ctx)?;
    for read_index in 0..length {
        let entry = list.get(read_index, ctx)?;
        let matches = entry
            .as_object()
            .map(|object| {
                object
                    .get(js_string!("callback"), ctx)
                    .map(|cb| cb.strict_equals(&callback_value))
            })
            .transpose()?
            .unwrap_or(false);
        if matches {
            removed += 1.0;
            continue;
        }
        if write_index != read_index {
            list.set(write_index, entry, false, ctx)?;
        }
        write_index += 1;
    }
    list.set(js_string!("length"), write_index, false, ctx)?;
    if removed > 0.0 {
        bump_listener_type_count(event_type, -removed, ctx)?;
    }
    Ok(())
}

/// Remove a spent `once` entry by callback identity from a specific phase list.
fn remove_once_entry(
    node_id: NodeId,
    event_type: &str,
    capture: bool,
    callback: &JsValue,
    ctx: &mut Context,
) -> JsResult<()> {
    let Some(list) = listener_list(node_id, event_type, capture, false, ctx)? else {
        return Ok(());
    };
    let mut write_index = 0_u64;
    let mut removed = 0.0_f64;
    let length = list.length(ctx)?;
    for read_index in 0..length {
        let entry = list.get(read_index, ctx)?;
        let matches = entry
            .as_object()
            .map(|object| {
                object
                    .get(js_string!("callback"), ctx)
                    .map(|cb| cb.strict_equals(callback))
            })
            .transpose()?
            .unwrap_or(false);
        if matches && removed == 0.0 {
            removed = 1.0;
            continue;
        }
        if write_index != read_index {
            list.set(write_index, entry, false, ctx)?;
        }
        write_index += 1;
    }
    list.set(js_string!("length"), write_index, false, ctx)?;
    if removed > 0.0 {
        bump_listener_type_count(event_type, -removed, ctx)?;
    }
    Ok(())
}

// ---- event object ------------------------------------------------------------

fn stop_propagation_native() -> NativeFunction {
    NativeFunction::from_fn_ptr(|this, _args, ctx| {
        if let Some(object) = this.as_object() {
            object.set(js_string!("__stopPropagation"), true, false, ctx)?;
        }
        Ok(JsValue::undefined())
    })
}

fn stop_immediate_propagation_native() -> NativeFunction {
    NativeFunction::from_fn_ptr(|this, _args, ctx| {
        if let Some(object) = this.as_object() {
            object.set(js_string!("__stopPropagation"), true, false, ctx)?;
            object.set(js_string!("__stopImmediate"), true, false, ctx)?;
        }
        Ok(JsValue::undefined())
    })
}

fn prevent_default_native() -> NativeFunction {
    NativeFunction::from_fn_ptr(|this, _args, ctx| {
        if let Some(object) = this.as_object()
            && object.get(js_string!("cancelable"), ctx)?.to_boolean()
        {
            object.set(js_string!("defaultPrevented"), true, false, ctx)?;
        }
        Ok(JsValue::undefined())
    })
}

/// Build a fresh event object carrying the control methods and flags.
pub(super) fn build_event_object(
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
    is_trusted: bool,
    ctx: &mut Context,
) -> JsObject {
    ObjectInitializer::new(ctx)
        .property(
            js_string!("type"),
            JsString::from(event_type),
            Attribute::all(),
        )
        .property(js_string!("bubbles"), bubbles, Attribute::all())
        .property(js_string!("cancelable"), cancelable, Attribute::all())
        .property(js_string!("defaultPrevented"), false, Attribute::all())
        .property(js_string!("eventPhase"), 0, Attribute::all())
        .property(js_string!("target"), JsValue::null(), Attribute::all())
        .property(
            js_string!("currentTarget"),
            JsValue::null(),
            Attribute::all(),
        )
        .property(js_string!("isTrusted"), is_trusted, Attribute::all())
        .property(
            js_string!("__stopPropagation"),
            false,
            Attribute::WRITABLE | Attribute::CONFIGURABLE,
        )
        .property(
            js_string!("__stopImmediate"),
            false,
            Attribute::WRITABLE | Attribute::CONFIGURABLE,
        )
        .function(stop_propagation_native(), js_string!("stopPropagation"), 0)
        .function(
            stop_immediate_propagation_native(),
            js_string!("stopImmediatePropagation"),
            0,
        )
        .function(prevent_default_native(), js_string!("preventDefault"), 0)
        .build()
}

/// Ensure a caller-supplied event object carries the control methods and flags
/// the propagation loop reads. Missing pieces are installed in place so
/// `el.dispatchEvent({type: 'input'})` keeps working.
pub(super) fn normalize_event_object(event: &JsObject, ctx: &mut Context) -> JsResult<()> {
    for (name, default) in [
        (js_string!("bubbles"), JsValue::from(false)),
        (js_string!("cancelable"), JsValue::from(false)),
        (js_string!("defaultPrevented"), JsValue::from(false)),
        (js_string!("eventPhase"), JsValue::from(0)),
        (js_string!("isTrusted"), JsValue::from(false)),
        (js_string!("__stopPropagation"), JsValue::from(false)),
        (js_string!("__stopImmediate"), JsValue::from(false)),
    ] {
        if !event.has_own_property(name.clone(), ctx)? {
            event.set(name, default, false, ctx)?;
        }
    }
    if !event.get(js_string!("stopPropagation"), ctx)?.is_callable() {
        install_method(event, "stopPropagation", stop_propagation_native(), ctx)?;
    }
    if !event
        .get(js_string!("stopImmediatePropagation"), ctx)?
        .is_callable()
    {
        install_method(
            event,
            "stopImmediatePropagation",
            stop_immediate_propagation_native(),
            ctx,
        )?;
    }
    if !event.get(js_string!("preventDefault"), ctx)?.is_callable() {
        install_method(event, "preventDefault", prevent_default_native(), ctx)?;
    }
    Ok(())
}

fn install_method(
    target: &JsObject,
    name: &str,
    function: NativeFunction,
    ctx: &mut Context,
) -> JsResult<()> {
    let function = boa_engine::object::FunctionObjectBuilder::new(ctx.realm(), function).build();
    target.set(JsString::from(name), function, false, ctx)?;
    Ok(())
}

// ---- propagation -------------------------------------------------------------

/// Snapshot the target-to-root ancestor chain. The Dom lock is taken and
/// released inside this function; the propagation loop below runs unlocked.
fn ancestor_path(dom_arc: &Arc<Mutex<Dom>>, target: NodeId) -> Vec<NodeId> {
    let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    let mut path = vec![target];
    let mut current = target;
    while let Ok(Some(parent)) = dom.parent(current) {
        path.push(parent);
        current = parent;
    }
    path
}

/// The JS object listeners see as `currentTarget` for `node_id`. The document
/// node maps to the global `document` object so delegation code that compares
/// against `document` behaves; other nodes get fresh wrappers.
fn current_target_object(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    target_value: &JsValue,
    target_id: NodeId,
    ctx: &mut Context,
) -> JsResult<JsValue> {
    if node_id == target_id {
        return Ok(target_value.clone());
    }
    if node_id.raw() == 0 {
        let global = ctx.global_object().clone();
        let document = global.get(js_string!("document"), ctx)?;
        if document.is_object() {
            return Ok(document);
        }
    }
    Ok(super::dom_bridge::node_to_js_object(dom_arc, node_id, ctx))
}

struct ListenerEntry {
    callback: JsValue,
    once: bool,
}

fn snapshot_entries(
    node_id: NodeId,
    event_type: &str,
    capture: bool,
    ctx: &mut Context,
) -> JsResult<Vec<ListenerEntry>> {
    let Some(list) = listener_list(node_id, event_type, capture, false, ctx)? else {
        return Ok(Vec::new());
    };
    let mut entries = Vec::new();
    let length = list.length(ctx)?;
    for index in 0..length {
        let entry = list.get(index, ctx)?;
        if let Some(object) = entry.as_object() {
            let callback = object.get(js_string!("callback"), ctx)?;
            if callback.is_callable() {
                let once = object.get(js_string!("once"), ctx)?.to_boolean();
                entries.push(ListenerEntry { callback, once });
            }
        }
    }
    Ok(entries)
}

fn event_flag(event: &JsObject, name: JsString, ctx: &mut Context) -> bool {
    event
        .get(name, ctx)
        .map(|value| value.to_boolean())
        .unwrap_or(false)
}

/// Run one node's listeners for one phase list. Returns false when
/// stopImmediatePropagation fired and the dispatch must halt entirely.
#[allow(clippy::too_many_arguments)]
fn invoke_listeners_at(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    event_type: &str,
    capture: bool,
    phase: u32,
    event: &JsObject,
    target_value: &JsValue,
    target_id: NodeId,
    ctx: &mut Context,
) -> JsResult<bool> {
    let entries = snapshot_entries(node_id, event_type, capture, ctx)?;
    if entries.is_empty() {
        return Ok(true);
    }
    let current_target = current_target_object(dom_arc, node_id, target_value, target_id, ctx)?;
    event.set(
        js_string!("currentTarget"),
        current_target.clone(),
        false,
        ctx,
    )?;
    event.set(js_string!("eventPhase"), phase, false, ctx)?;
    let event_value = JsValue::from(event.clone());
    for entry in entries {
        if event_flag(event, js_string!("__stopImmediate"), ctx) {
            return Ok(false);
        }
        // debug builds assert the Dom lock is free before re-entering JS: a
        // listener that touches document.* would otherwise deadlock.
        debug_assert!(
            dom_arc.try_lock().is_ok(),
            "Dom lock held across event listener invocation"
        );
        if let Some(callback) = entry.callback.as_callable()
            && let Err(err) =
                callback.call(&current_target, std::slice::from_ref(&event_value), ctx)
        {
            // Listener exceptions are reported and dispatch continues, per
            // the DOM spec's "report the exception" step.
            eprintln!("silksurf-js: event listener error ({event_type}): {err}");
        }
        if entry.once {
            remove_once_entry(node_id, event_type, capture, &entry.callback, ctx)?;
        }
    }
    Ok(!event_flag(event, js_string!("__stopImmediate"), ctx))
}

/// Dispatch `event` at `target` with full capture/target/bubble propagation.
///
/// Returns true when the default action should proceed (no listener called
/// preventDefault on a cancelable event).
pub(super) fn propagate_event(
    dom_arc: &Arc<Mutex<Dom>>,
    target_id: NodeId,
    target_value: &JsValue,
    event: &JsObject,
    ctx: &mut Context,
) -> JsResult<bool> {
    normalize_event_object(event, ctx)?;
    event.set(js_string!("target"), target_value.clone(), false, ctx)?;
    let event_type = event
        .get(js_string!("type"), ctx)?
        .to_string(ctx)?
        .to_std_string_lossy();
    let bubbles = event_flag(event, js_string!("bubbles"), ctx);
    let path = ancestor_path(dom_arc, target_id);

    // Capture phase: root -> parent-of-target.
    let mut halted = false;
    for &node_id in path.iter().skip(1).rev() {
        if event_flag(event, js_string!("__stopPropagation"), ctx) {
            halted = true;
            break;
        }
        if !invoke_listeners_at(
            dom_arc,
            node_id,
            &event_type,
            true,
            CAPTURING_PHASE,
            event,
            target_value,
            target_id,
            ctx,
        )? {
            halted = true;
            break;
        }
    }

    // Target phase: capture list then bubble list at the target itself.
    if !halted && !event_flag(event, js_string!("__stopPropagation"), ctx) {
        for capture in [true, false] {
            if !invoke_listeners_at(
                dom_arc,
                target_id,
                &event_type,
                capture,
                AT_TARGET,
                event,
                target_value,
                target_id,
                ctx,
            )? {
                halted = true;
                break;
            }
        }
    }

    // Bubble phase: parent-of-target -> root.
    if !halted && bubbles {
        for &node_id in path.iter().skip(1) {
            if event_flag(event, js_string!("__stopPropagation"), ctx) {
                break;
            }
            if !invoke_listeners_at(
                dom_arc,
                node_id,
                &event_type,
                false,
                BUBBLING_PHASE,
                event,
                target_value,
                target_id,
                ctx,
            )? {
                break;
            }
        }
    }

    event.set(js_string!("eventPhase"), 0, false, ctx)?;
    event.set(js_string!("currentTarget"), JsValue::null(), false, ctx)?;
    Ok(!event_flag(event, js_string!("defaultPrevented"), ctx))
}

#[cfg(test)]
mod tests {
    use crate::boa_backend::{SilkContext, SyntheticEvent};
    use silksurf_dom::{Dom, NodeId};
    use std::sync::{Arc, Mutex};

    /// document > div#outer > p#middle > button#inner
    fn nested_dom() -> (Arc<Mutex<Dom>>, NodeId) {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let outer = dom.create_element("div");
        let _ = dom.set_attribute(outer, "id", "outer");
        let middle = dom.create_element("p");
        let _ = dom.set_attribute(middle, "id", "middle");
        let inner = dom.create_element("button");
        let _ = dom.set_attribute(inner, "id", "inner");
        let _ = dom.append_child(root, outer);
        let _ = dom.append_child(outer, middle);
        let _ = dom.append_child(middle, inner);
        dom.materialize_resolve_table();
        (Arc::new(Mutex::new(dom)), inner)
    }

    #[test]
    fn capture_runs_before_target_before_bubble() {
        let (arc, _inner) = nested_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var order = []; \
             var outer = document.getElementById('outer'); \
             var inner = document.getElementById('inner'); \
             outer.addEventListener('ping', function() { order.push('outer-capture'); }, true); \
             outer.addEventListener('ping', function() { order.push('outer-bubble'); }); \
             inner.addEventListener('ping', function(e) { \
               order.push('target:' + e.eventPhase); \
             }); \
             inner.dispatchEvent({ type: 'ping', bubbles: true }); \
             if (order.join(',') !== 'outer-capture,target:2,outer-bubble') { \
               throw new Error('phase order was ' + order.join(',')); \
             }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn non_bubbling_event_skips_ancestor_bubble_listeners() {
        let (arc, _inner) = nested_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var fired = []; \
             var outer = document.getElementById('outer'); \
             var inner = document.getElementById('inner'); \
             outer.addEventListener('ping', function() { fired.push('outer'); }); \
             inner.addEventListener('ping', function() { fired.push('inner'); }); \
             inner.dispatchEvent({ type: 'ping' }); \
             if (fired.join(',') !== 'inner') { throw new Error('fired: ' + fired.join(',')); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn stop_propagation_halts_ancestor_listeners() {
        let (arc, _inner) = nested_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var fired = []; \
             var outer = document.getElementById('outer'); \
             var inner = document.getElementById('inner'); \
             outer.addEventListener('ping', function() { fired.push('outer'); }); \
             inner.addEventListener('ping', function(e) { fired.push('inner'); e.stopPropagation(); }); \
             inner.dispatchEvent({ type: 'ping', bubbles: true }); \
             if (fired.join(',') !== 'inner') { throw new Error('fired: ' + fired.join(',')); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn stop_immediate_propagation_halts_same_node_siblings() {
        let (arc, _inner) = nested_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var fired = []; \
             var inner = document.getElementById('inner'); \
             inner.addEventListener('ping', function(e) { fired.push('a'); e.stopImmediatePropagation(); }); \
             inner.addEventListener('ping', function() { fired.push('b'); }); \
             inner.dispatchEvent({ type: 'ping' }); \
             if (fired.join(',') !== 'a') { throw new Error('fired: ' + fired.join(',')); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn prevent_default_returns_false_from_dispatch() {
        let (arc, _inner) = nested_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var inner = document.getElementById('inner'); \
             inner.addEventListener('ping', function(e) { e.preventDefault(); }); \
             var proceed = inner.dispatchEvent({ type: 'ping', cancelable: true }); \
             if (proceed !== false) { throw new Error('dispatchEvent returned ' + proceed); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn once_listener_fires_exactly_once() {
        let (arc, _inner) = nested_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var count = 0; \
             var inner = document.getElementById('inner'); \
             inner.addEventListener('ping', function() { count += 1; }, { once: true }); \
             inner.dispatchEvent({ type: 'ping' }); \
             inner.dispatchEvent({ type: 'ping' }); \
             if (count !== 1) { throw new Error('once listener fired ' + count + ' times'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn listener_exception_does_not_abort_dispatch() {
        let (arc, _inner) = nested_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var fired = []; \
             var inner = document.getElementById('inner'); \
             inner.addEventListener('ping', function() { throw new Error('boom'); }); \
             inner.addEventListener('ping', function() { fired.push('after'); }); \
             inner.dispatchEvent({ type: 'ping' }); \
             if (fired.join(',') !== 'after') { throw new Error('fired: ' + fired.join(',')); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn reentrant_dispatch_from_listener_is_safe() {
        let (arc, _inner) = nested_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var fired = []; \
             var inner = document.getElementById('inner'); \
             inner.addEventListener('pong', function() { fired.push('pong'); }); \
             inner.addEventListener('ping', function() { \
               fired.push('ping'); \
               inner.dispatchEvent({ type: 'pong' }); \
             }); \
             inner.dispatchEvent({ type: 'ping' }); \
             if (fired.join(',') !== 'ping,pong') { throw new Error('fired: ' + fired.join(',')); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn listener_can_mutate_dom_during_dispatch() {
        let (arc, _inner) = nested_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var inner = document.getElementById('inner'); \
             inner.addEventListener('ping', function() { \
               var marker = document.createElement('em'); \
               marker.setAttribute('id', 'marker'); \
               document.getElementById('outer').appendChild(marker); \
             }); \
             inner.dispatchEvent({ type: 'ping' }); \
             if (document.getElementById('marker') === null) { throw new Error('marker missing'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn document_level_delegation_sees_bubbling_event() {
        let (arc, _inner) = nested_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var seen = ''; \
             document.addEventListener('click', function(e) { \
               seen = e.target.id + ':' + (e.currentTarget === document); \
             }); \
             var inner = document.getElementById('inner'); \
             inner.dispatchEvent({ type: 'click', bubbles: true }); \
             if (seen !== 'inner:true') { throw new Error('seen: ' + seen); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn synthetic_dispatch_reports_prevent_default() {
        let (arc, inner) = nested_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "document.getElementById('inner').addEventListener('click', function(e) { \
               if (!e.isTrusted) { throw new Error('synthetic event must be trusted'); } \
               e.preventDefault(); \
             });",
        )
        .expect("eval should succeed");
        let outcome = ctx
            .dispatch_dom_event(inner, &SyntheticEvent::new("click", true, true))
            .expect("dispatch should succeed");
        assert!(outcome.default_prevented);
    }

    #[test]
    fn synthetic_dispatch_without_listeners_allows_default() {
        let (arc, inner) = nested_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        let outcome = ctx
            .dispatch_dom_event(inner, &SyntheticEvent::new("click", true, true))
            .expect("dispatch should succeed");
        assert!(!outcome.default_prevented);
    }

    #[test]
    fn has_dom_listeners_tracks_add_and_remove() {
        let (arc, _inner) = nested_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        assert!(!ctx.has_dom_listeners("click"));
        ctx.eval(
            "function onClick() {} \
             document.getElementById('inner').addEventListener('click', onClick);",
        )
        .expect("eval should succeed");
        assert!(ctx.has_dom_listeners("click"));
        assert!(!ctx.has_dom_listeners("keydown"));
        ctx.eval("document.getElementById('inner').removeEventListener('click', onClick);")
            .expect("eval should succeed");
        assert!(!ctx.has_dom_listeners("click"));
    }

    #[test]
    fn once_listener_decrements_type_count() {
        let (arc, _inner) = nested_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "document.getElementById('inner') \
               .addEventListener('ping', function() {}, { once: true });",
        )
        .expect("eval should succeed");
        assert!(ctx.has_dom_listeners("ping"));
        ctx.eval("document.getElementById('inner').dispatchEvent({ type: 'ping' });")
            .expect("eval should succeed");
        assert!(!ctx.has_dom_listeners("ping"));
    }

    #[test]
    fn synthetic_event_fields_reach_listener() {
        let (arc, inner) = nested_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "globalThis._key = ''; \
             document.getElementById('inner').addEventListener('keydown', function(e) { \
               globalThis._key = e.key; \
             });",
        )
        .expect("eval should succeed");
        let event = SyntheticEvent::new("keydown", true, true)
            .with_field("key", crate::boa_backend::SyntheticField::Text("a".into()));
        let _ = ctx
            .dispatch_dom_event(inner, &event)
            .expect("dispatch should succeed");
        ctx.eval("if (globalThis._key !== 'a') { throw new Error('key: ' + globalThis._key); }")
            .expect("eval should succeed");
    }
}
