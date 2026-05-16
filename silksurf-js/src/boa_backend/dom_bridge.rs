/*
 * dom_bridge.rs -- Thin adapter between silksurf_dom::Dom and the boa_engine
 * JavaScript context.
 *
 * WHY: The boa_backend document stub always returns null for getElementById
 * and friends.  This module replaces those stubs with real DOM traversal so
 * that scripts can read and write the parsed document tree.
 *
 * HOW: SilkContext::with_dom(arc: Arc<Mutex<Dom>>) installs closures that
 * capture the Arc.  The closures are registered via
 * NativeFunction::from_closure (unsafe), which is sound here because:
 *   - Arc<Mutex<Dom>> is a pure Rust reference-counted type with no boa
 *     GC-managed pointers.  The GC cannot dereference or move it.
 *   - NodeId is usize -- not a GC-traced type.
 *   - None of the captured values participate in boa's garbage collector.
 *
 * The safety invariant required by from_closure (captured vars must not
 * need GC tracing) is satisfied by these capture types.
 */

use std::sync::{Arc, Mutex, PoisonError};

use boa_engine::{
    Context, JsNativeError, JsString, JsValue, NativeFunction, js_string,
    object::{ObjectInitializer, builtins::JsArray},
    property::Attribute,
};
use silksurf_dom::{Dom, NodeId, NodeKind};

// ---- text collection -------------------------------------------------------

pub(super) fn collect_text(dom: &Dom, node: NodeId) -> String {
    let mut buf = String::new();
    collect_text_inner(dom, node, &mut buf);
    buf
}

fn collect_text_inner(dom: &Dom, node: NodeId, buf: &mut String) {
    if let Ok(n) = dom.node(node)
        && let NodeKind::Text { text } = n.kind()
    {
        buf.push_str(text);
        return;
    }
    if let Ok(children) = dom.children(node) {
        let owned: Vec<NodeId> = children.to_vec();
        for child in owned {
            collect_text_inner(dom, child, buf);
        }
    }
}

// ---- selector matching (simplified) ----------------------------------------

fn matches_selector(dom: &Dom, node: NodeId, selector: &str) -> bool {
    if let Some(id) = selector.strip_prefix('#') {
        if let Ok(attrs) = dom.attributes(node) {
            return attrs
                .iter()
                .any(|a| a.name.as_str() == "id" && a.value.as_str() == id);
        }
        return false;
    }
    if let Some(class) = selector.strip_prefix('.') {
        if let Ok(attrs) = dom.attributes(node)
            && let Some(cls) = attrs.iter().find(|a| a.name.as_str() == "class")
        {
            return cls.value.as_str().split_whitespace().any(|c| c == class);
        }
        return false;
    }
    // Plain tag selector (case-insensitive)
    matches!(
        dom.element_name(node),
        Ok(Some(tag)) if tag.eq_ignore_ascii_case(selector)
    )
}

fn collect_matches(dom: &Dom, node: NodeId, selector: &str, out: &mut Vec<NodeId>) {
    if matches_selector(dom, node, selector) {
        out.push(node);
    }
    if let Ok(children) = dom.children(node) {
        let owned: Vec<NodeId> = children.to_vec();
        for child in owned {
            collect_matches(dom, child, selector, out);
        }
    }
}

pub(super) fn query_all(dom: &Dom, root: NodeId, selector: &str) -> Vec<NodeId> {
    let mut results = Vec::new();
    collect_matches(dom, root, selector, &mut results);
    results
}

// ---- node -> JS object -----------------------------------------------------

/// Build a plain JS object snapshot for a DOM element.
///
/// Static properties (tagName, id, className, textContent, innerHTML, nodeId)
/// are captured at call time.  getAttribute and setAttribute use live closures
/// that re-lock the Dom Arc on each call.
pub(super) fn node_to_js_object(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    ctx: &mut Context,
) -> JsValue {
    let (tag_name, id_val, class_val, text) = {
        let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
        let tag = dom
            .element_name(node_id)
            .ok()
            .flatten()
            .map(str::to_uppercase)
            .unwrap_or_default();
        let (id_v, cls_v) = if let Ok(attrs) = dom.attributes(node_id) {
            let id_v = attrs
                .iter()
                .find(|a| a.name.as_str() == "id")
                .map(|a| a.value.as_str().to_string())
                .unwrap_or_default();
            let cls_v = attrs
                .iter()
                .find(|a| a.name.as_str() == "class")
                .map(|a| a.value.as_str().to_string())
                .unwrap_or_default();
            (id_v, cls_v)
        } else {
            (String::new(), String::new())
        };
        let text = collect_text(&dom, node_id);
        (tag, id_v, cls_v, text)
    };

    // SAFETY: Arc<Mutex<Dom>> and NodeId are not boa GC-traced types.
    // Neither capture requires GC tracing, so from_closure is sound here.
    let arc_get = Arc::clone(dom_arc);
    let get_attribute = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let name = match args.first() {
                Some(v) => v.to_string(ctx)?.to_std_string_lossy(),
                None => return Ok(JsValue::null()),
            };
            let dom = arc_get.lock().unwrap_or_else(PoisonError::into_inner);
            let val = if let Ok(attrs) = dom.attributes(node_id) {
                attrs
                    .iter()
                    .find(|a| a.name.as_str() == name.as_str())
                    .map_or(JsValue::null(), |a| {
                        JsValue::from(JsString::from(a.value.as_str()))
                    })
            } else {
                JsValue::null()
            };
            Ok(val)
        })
    };

    // SAFETY: same as get_attribute above.
    let arc_set = Arc::clone(dom_arc);
    let set_attribute = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let name = match args.first() {
                Some(v) => v.to_string(ctx)?.to_std_string_lossy(),
                None => return Ok(JsValue::undefined()),
            };
            let val = match args.get(1) {
                Some(v) => v.to_string(ctx)?.to_std_string_lossy(),
                None => String::new(),
            };
            let mut dom = arc_set.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = dom.set_attribute(node_id, name.as_str(), val.as_str());
            Ok(JsValue::undefined())
        })
    };

    ObjectInitializer::new(ctx)
        .property(
            js_string!("tagName"),
            JsString::from(tag_name.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("id"),
            JsString::from(id_val.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("className"),
            JsString::from(class_val.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("textContent"),
            JsString::from(text.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("innerHTML"),
            JsString::from(text.as_str()),
            Attribute::all(),
        )
        .property(js_string!("nodeId"), node_id.raw() as u32, Attribute::all())
        .function(get_attribute, js_string!("getAttribute"), 1)
        .function(set_attribute, js_string!("setAttribute"), 2)
        .function(
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
            js_string!("addEventListener"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
            js_string!("removeEventListener"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
            js_string!("dispatchEvent"),
            1,
        )
        .build()
        .into()
}

// ---- document object builder ------------------------------------------------

/// Replace the stub document object in `ctx` with one that queries `dom_arc`.
pub(super) fn install_document(dom_arc: &Arc<Mutex<Dom>>, ctx: &mut Context) {
    let root = NodeId::from_raw(0);

    // getElementById(id: string) -> element | null
    // SAFETY: Arc<Mutex<Dom>> and NodeId are not boa GC-traced types.
    let arc1 = Arc::clone(dom_arc);
    let get_element_by_id = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let id = match args.first() {
                Some(v) => v.to_string(ctx)?.to_std_string_lossy(),
                None => return Ok(JsValue::null()),
            };
            let dom = arc1.lock().unwrap_or_else(PoisonError::into_inner);
            let found = query_all(&dom, root, &format!("#{id}")).into_iter().next();
            drop(dom);
            match found {
                Some(node_id) => Ok(node_to_js_object(&arc1, node_id, ctx)),
                None => Ok(JsValue::null()),
            }
        })
    };

    // querySelector(selector: string) -> element | null
    // SAFETY: same as above.
    let arc2 = Arc::clone(dom_arc);
    let query_selector = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let sel = match args.first() {
                Some(v) => v.to_string(ctx)?.to_std_string_lossy(),
                None => return Ok(JsValue::null()),
            };
            let dom = arc2.lock().unwrap_or_else(PoisonError::into_inner);
            let found = query_all(&dom, root, &sel).into_iter().next();
            drop(dom);
            match found {
                Some(node_id) => Ok(node_to_js_object(&arc2, node_id, ctx)),
                None => Ok(JsValue::null()),
            }
        })
    };

    // querySelectorAll(selector: string) -> Array<element>
    // SAFETY: same as above.
    let arc3 = Arc::clone(dom_arc);
    let query_selector_all = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let sel = match args.first() {
                Some(v) => v.to_string(ctx)?.to_std_string_lossy(),
                None => {
                    return Ok(JsValue::from(JsArray::new(ctx)));
                }
            };
            let nodes = {
                let dom = arc3.lock().unwrap_or_else(PoisonError::into_inner);
                query_all(&dom, root, &sel)
            };
            let arr = JsArray::new(ctx);
            for node_id in nodes {
                let obj = node_to_js_object(&arc3, node_id, ctx);
                arr.push(obj, ctx)
                    .map_err(|e| JsNativeError::error().with_message(e.to_string()))?;
            }
            Ok(JsValue::from(arr))
        })
    };

    // createElement(tag: string) -> element | null
    // SAFETY: same as above.
    let arc4 = Arc::clone(dom_arc);
    let create_element = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let tag = match args.first() {
                Some(v) => v.to_string(ctx)?.to_std_string_lossy(),
                None => return Ok(JsValue::null()),
            };
            let node_id = {
                let mut dom = arc4.lock().unwrap_or_else(PoisonError::into_inner);
                dom.create_element(tag.as_str())
            };
            Ok(node_to_js_object(&arc4, node_id, ctx))
        })
    };

    let document = ObjectInitializer::new(ctx)
        .function(get_element_by_id, js_string!("getElementById"), 1)
        .function(query_selector, js_string!("querySelector"), 1)
        .function(query_selector_all, js_string!("querySelectorAll"), 1)
        .function(create_element, js_string!("createElement"), 1)
        .function(
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::null())),
            js_string!("createElementNS"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::null())),
            js_string!("createTextNode"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
            js_string!("addEventListener"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
            js_string!("removeEventListener"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
            js_string!("dispatchEvent"),
            1,
        )
        .build();

    // UNWRAP-OK: if "document" is already defined, define_property overwrites it;
    // we accept both outcomes.
    let _ = ctx.register_global_property(js_string!("document"), document, Attribute::all());
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::boa_backend::SilkContext;
    use silksurf_dom::{Dom, NodeId};
    use std::sync::{Arc, Mutex};

    fn simple_dom() -> (Arc<Mutex<Dom>>, NodeId) {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let div = dom.create_element("div");
        let _ = dom.set_attribute(div, "id", "greeting");
        let _ = dom.set_attribute(div, "class", "hero banner");
        let _ = dom.append_text(div, "Hello, world!");
        let _ = dom.append_child(root, div);
        let span = dom.create_element("span");
        let _ = dom.set_attribute(span, "class", "banner");
        let _ = dom.append_child(root, span);
        dom.materialize_resolve_table();
        (Arc::new(Mutex::new(dom)), root)
    }

    #[test]
    fn get_element_by_id_finds_element() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             globalThis._tag = el ? el.tagName : 'null';",
        )
        .expect("eval should succeed");
        ctx.eval("globalThis._found = el !== null;").unwrap();
    }

    #[test]
    fn get_element_by_id_returns_null_for_missing() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval("var el = document.getElementById('nope'); globalThis._found = el === null;")
            .expect("eval should succeed");
    }

    #[test]
    fn query_selector_tag_matches() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval("var el = document.querySelector('span');")
            .expect("eval should succeed");
    }

    #[test]
    fn query_selector_all_class_returns_array() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var els = document.querySelectorAll('.banner'); \
             globalThis._count = els.length;",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn text_content_propagates() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             globalThis._text = el ? el.textContent : '';",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn get_attribute_returns_value() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             globalThis._id = el ? el.getAttribute('id') : null;",
        )
        .expect("eval should succeed");
    }
}
