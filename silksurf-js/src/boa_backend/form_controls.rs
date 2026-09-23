//! HTML form-owner resolution and live form-controls collections.

use std::sync::{Arc, Mutex, PoisonError};

use boa_engine::{
    Context, JsResult, JsValue, NativeFunction, Source, js_string, object::builtins::JsArray,
};
use silksurf_dom::{Dom, NodeId};

fn read_controls(
    dom_arc: &Arc<Mutex<Dom>>,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let raw = args.first().unwrap_or(&JsValue::undefined()).to_u32(ctx)?;
    let nodes = {
        let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
        dom.form_controls(NodeId::from_raw(raw as usize))
    };
    let array = JsArray::new(ctx);
    for node in nodes {
        array.push(
            super::dom_bridge::node_to_js_object(dom_arc, node, ctx),
            ctx,
        )?;
    }
    Ok(array.into())
}

fn control_state(
    dom_arc: &Arc<Mutex<Dom>>,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let node =
        NodeId::from_raw(args.first().unwrap_or(&JsValue::undefined()).to_u32(ctx)? as usize);
    let operation = args.get(1).unwrap_or(&JsValue::undefined()).to_u32(ctx)?;
    let mut dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    match operation {
        0 => Ok(dom
            .form_owner(node)
            .map_or(JsValue::null(), |owner| JsValue::from(owner.raw() as u32))),
        1 => Ok(JsValue::from(dom.input_checked(node))),
        _ => {
            dom.set_input_checked(node, args.get(2).is_some_and(JsValue::to_boolean))
                .map_err(|error| {
                    boa_engine::JsNativeError::typ().with_message(format!("{error:?}"))
                })?;
            Ok(JsValue::undefined())
        }
    }
}

pub(super) fn install(dom_arc: &Arc<Mutex<Dom>>, ctx: &mut Context) {
    let state_dom = Arc::clone(dom_arc);
    // SAFETY: the closure owns an Arc to the DOM and captures no GC-managed pointers.
    let state_native = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| control_state(&state_dom, args, ctx))
    };
    let _ = ctx.register_global_callable(js_string!("__silksurfControlState"), 3, state_native);
    let dom_arc = Arc::clone(dom_arc);
    // SAFETY: the closure owns an Arc to the DOM and captures no GC-managed pointers.
    let native = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| read_controls(&dom_arc, args, ctx))
    };
    let _ = ctx.register_global_callable(js_string!("__silksurfFormControls"), 1, native);
    if let Err(error) = ctx.eval(Source::from_bytes(include_str!("form_controls.js"))) {
        eprintln!("silksurf-js: form collection bootstrap failed: {error}");
    }
}
