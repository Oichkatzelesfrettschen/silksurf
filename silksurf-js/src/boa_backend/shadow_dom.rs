//! Shadow-root ownership accessors preserve separate ordinary DOM trees.
use boa_engine::{Context, JsResult, JsValue, NativeFunction, Source, js_string};
use silksurf_dom::{Dom, NodeId};
use std::sync::{Arc, Mutex, PoisonError};

fn access(dom_arc: &Arc<Mutex<Dom>>, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let node =
        NodeId::from_raw(args.first().unwrap_or(&JsValue::undefined()).to_u32(ctx)? as usize);
    let operation = args.get(1).unwrap_or(&JsValue::undefined()).to_u32(ctx)?;
    let mut dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    let result = match operation {
        0 => dom
            .shadow_root(node)
            .filter(|&root| dom.shadow_host(root).is_some_and(|(_, closed)| !closed)),
        1 => dom.shadow_host(node).map(|(host, _)| host),
        2 => {
            return Ok(dom
                .shadow_host(node)
                .map_or(JsValue::null(), |(_, closed)| {
                    js_string!(if closed { "closed" } else { "open" }).into()
                }));
        }
        3 => Some(
            dom.attach_shadow(node, args.get(2).is_some_and(JsValue::to_boolean))
                .map_err(|error| {
                    super::dom_interfaces::dom_exception(
                        "NotSupportedError",
                        &format!("{error:?}"),
                        ctx,
                    )
                })?,
        ),
        _ => None,
    };
    Ok(result.map_or(JsValue::null(), |node| JsValue::from(node.raw() as u32)))
}

pub(super) fn install(dom_arc: &Arc<Mutex<Dom>>, ctx: &mut Context) {
    let dom_arc = Arc::clone(dom_arc);
    // SAFETY: the closure owns the DOM handle and captures no GC-managed pointers.
    let native = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| access(&dom_arc, args, ctx))
    };
    let _ = ctx.register_global_callable(js_string!("__silksurfShadowAccess"), 3, native);
    if let Err(error) = ctx.eval(Source::from_bytes(BOOTSTRAP)) {
        eprintln!("silksurf-js: shadow DOM bootstrap failed: {error}");
    }
}

const BOOTSTRAP: &str = r"
(function () {
    'use strict';
    Element.prototype.attachShadow = function (options) {
        const mode = options ? String(options.mode) : '';
        if (mode !== 'open' && mode !== 'closed') {
            throw new TypeError('ShadowRoot mode must be open or closed');
        }
        const root = __silksurfShadowAccess(this.nodeId, 3, mode === 'closed');
        return __silksurfWrapNode(root);
    };
    Object.defineProperty(Element.prototype, 'shadowRoot', {
        get: function () {
            const root = __silksurfShadowAccess(this.nodeId, 0);
            return root === null ? null : __silksurfWrapNode(root);
        }, configurable: true, enumerable: true
    });
    Object.defineProperty(ShadowRoot.prototype, 'host', {
        get: function () {
            const host = __silksurfShadowAccess(this.nodeId, 1);
            if (host === null) { throw new TypeError('Invalid ShadowRoot receiver'); }
            return __silksurfWrapNode(host);
        }, configurable: true, enumerable: true
    });
    Object.defineProperty(ShadowRoot.prototype, 'mode', {
        get: function () {
            const mode = __silksurfShadowAccess(this.nodeId, 2);
            if (mode === null) { throw new TypeError('Invalid ShadowRoot receiver'); }
            return mode;
        }, configurable: true, enumerable: true
    });
})();
";
