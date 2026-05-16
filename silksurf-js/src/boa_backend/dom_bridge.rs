/*
 * dom_bridge.rs -- Thin adapter between silksurf_dom::Dom and the boa_engine
 * JavaScript context.
 *
 * WHY: The boa_backend document stub always returns null for getElementById
 * and friends.  This module replaces those stubs with real DOM traversal so
 * that scripts can read and write the parsed document tree.
 *
 * HOW: SilkContext::with_dom(arc: &Arc<Mutex<Dom>>) installs closures that
 * capture the Arc.  The closures are registered via
 * NativeFunction::from_closure (unsafe), which is sound here because:
 *   - Arc<Mutex<Dom>> is a pure Rust reference-counted type with no boa
 *     GC-managed pointers.  The GC cannot dereference or move it.
 *   - NodeId is usize -- not a GC-traced type.
 *   - None of the captured values participate in boa's garbage collector.
 *
 * The safety invariant required by from_closure (captured vars must not
 * need GC tracing) is satisfied by these capture types.
 *
 * Mutex acquisition discipline: always drop the dom lock guard BEFORE calling
 * node_to_js_object recursively.  node_to_js_object itself acquires the lock,
 * and Mutex is not reentrant -- holding the lock across a recursive call
 * would deadlock.
 */

use std::sync::{Arc, Mutex, PoisonError};

use boa_engine::{
    Context, JsNativeError, JsResult, JsString, JsValue, NativeFunction, js_string,
    object::{
        FunctionObjectBuilder, ObjectInitializer,
        builtins::{JsArray, JsFunction},
    },
    property::Attribute,
};
use silksurf_dom::{Dom, Node, NodeId, NodeKind};

// ---- helpers ---------------------------------------------------------------

/// Extract a `NodeId` from the first argument of a DOM method.
///
/// All node objects built by `node_to_js_object` carry a `nodeId` u32 property.
/// This helper reads that property back so callers can route mutations to the
/// right slot in the Dom arena.
fn extract_node_id(arg: Option<&JsValue>, ctx: &mut Context) -> JsResult<NodeId> {
    let v = arg.ok_or_else(|| JsNativeError::typ().with_message("expected a node argument"))?;
    let obj = v
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("argument is not a node object"))?;
    let raw = obj.get(js_string!("nodeId"), ctx)?.to_u32(ctx)?;
    Ok(NodeId::from_raw(raw as usize))
}

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

// ---- node info snapshot (dom must be held by caller) -----------------------

struct NodeSnapshot {
    tag_name: String,
    node_name: String,
    node_type: u32,
    node_value: String,
    id_val: String,
    class_val: String,
    text: String,
}

fn snapshot_node(dom: &Dom, node_id: NodeId) -> NodeSnapshot {
    if let Ok(n) = dom.node(node_id) {
        match n.kind() {
            NodeKind::Text { text } => {
                return NodeSnapshot {
                    tag_name: String::new(),
                    node_name: "#text".into(),
                    node_type: 3,
                    node_value: text.clone(),
                    id_val: String::new(),
                    class_val: String::new(),
                    text: text.clone(),
                };
            }
            NodeKind::Document => {
                return NodeSnapshot {
                    tag_name: String::new(),
                    node_name: "#document".into(),
                    node_type: 9,
                    node_value: String::new(),
                    id_val: String::new(),
                    class_val: String::new(),
                    text: String::new(),
                };
            }
            NodeKind::Comment { data: comment_text } => {
                return NodeSnapshot {
                    tag_name: String::new(),
                    node_name: "#comment".into(),
                    node_type: 8,
                    node_value: comment_text.clone(),
                    id_val: String::new(),
                    class_val: String::new(),
                    text: String::new(),
                };
            }
            _ => {}
        }
    }
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
    let text = collect_text(dom, node_id);
    NodeSnapshot {
        node_name: tag.clone(),
        tag_name: tag,
        node_type: 1,
        node_value: String::new(),
        id_val: id_v,
        class_val: cls_v,
        text,
    }
}

// ---- accessor getter builder -----------------------------------------------

/// Wrap a `NativeFunction` as a `JsFunction` for use with `ObjectInitializer::accessor`.
///
/// Borrows `ctx.realm()` briefly; the returned `JsFunction` is independent of ctx's
/// lifetime after `build()` returns.
fn make_getter(ctx: &mut Context, f: NativeFunction) -> JsFunction {
    FunctionObjectBuilder::new(ctx.realm(), f).build()
}

// ---- node -> JS object -----------------------------------------------------

/// Build a JS object for a single DOM node.
///
/// Static properties (tagName, id, className, textContent, innerHTML, nodeId,
/// nodeType, nodeName, nodeValue) are snapshotted at call time.
///
/// Accessor properties (parentNode, childNodes, children, firstChild, lastChild,
/// nextSibling, previousSibling) use lazy getters that re-lock the Dom Arc on
/// each access -- they always reflect current tree state.
///
/// Mutation methods (appendChild, removeChild, insertBefore, replaceChild)
/// acquire and release the lock on each call.
///
/// Invariant: the Dom lock is NOT held when this function is called.
/// Do not call this function while holding the `Arc<Mutex<Dom>>` lock.
pub(super) fn node_to_js_object(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    ctx: &mut Context,
) -> JsValue {
    // Snapshot static properties with a single lock/unlock.
    let snap = {
        let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
        snapshot_node(&dom, node_id)
    };

    // ---- accessor getter closures (all lazy, lock released before each call) --

    // SAFETY: Arc<Mutex<Dom>> and NodeId (usize) are not boa GC-traced types;
    // from_closure is sound because none of the captures need GC tracing.
    let arc_pn = Arc::clone(dom_arc);
    let pn_native = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let parent_id = {
                let dom = arc_pn.lock().unwrap_or_else(PoisonError::into_inner);
                dom.parent(node_id).ok().flatten()
            };
            match parent_id {
                Some(pid) => Ok(node_to_js_object(&arc_pn, pid, ctx)),
                None => Ok(JsValue::null()),
            }
        })
    };

    // SAFETY: same -- Arc<Mutex<Dom>> and NodeId captures are not GC-traced.
    let arc_cn = Arc::clone(dom_arc);
    let child_nodes_native = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let child_ids = {
                let dom = arc_cn.lock().unwrap_or_else(PoisonError::into_inner);
                dom.children(node_id)
                    .map(<[NodeId]>::to_vec)
                    .unwrap_or_default()
            };
            let arr = JsArray::new(ctx);
            for cid in child_ids {
                let obj = node_to_js_object(&arc_cn, cid, ctx);
                arr.push(obj, ctx)
                    .map_err(|e| JsNativeError::error().with_message(e.to_string()))?;
            }
            Ok(JsValue::from(arr))
        })
    };

    // SAFETY: same -- Arc<Mutex<Dom>> and NodeId captures are not GC-traced.
    let arc_ch = Arc::clone(dom_arc);
    let children_native = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let child_ids = {
                let dom = arc_ch.lock().unwrap_or_else(PoisonError::into_inner);
                dom.children(node_id)
                    .map(<[NodeId]>::to_vec)
                    .unwrap_or_default()
            };
            // children only includes element nodes (nodeType == 1)
            let mut element_ids: Vec<NodeId> = Vec::new();
            for cid in &child_ids {
                let is_element = {
                    let dom = arc_ch.lock().unwrap_or_else(PoisonError::into_inner);
                    matches!(dom.node(*cid).map(Node::kind), Ok(NodeKind::Element { .. }))
                };
                if is_element {
                    element_ids.push(*cid);
                }
            }
            let arr = JsArray::new(ctx);
            for cid in element_ids {
                let obj = node_to_js_object(&arc_ch, cid, ctx);
                arr.push(obj, ctx)
                    .map_err(|e| JsNativeError::error().with_message(e.to_string()))?;
            }
            Ok(JsValue::from(arr))
        })
    };

    // SAFETY: same -- Arc<Mutex<Dom>> and NodeId captures are not GC-traced.
    let arc_fc = Arc::clone(dom_arc);
    let first_child_native = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let fid = {
                let dom = arc_fc.lock().unwrap_or_else(PoisonError::into_inner);
                dom.first_child(node_id).ok().flatten()
            };
            match fid {
                Some(id) => Ok(node_to_js_object(&arc_fc, id, ctx)),
                None => Ok(JsValue::null()),
            }
        })
    };

    // SAFETY: same -- Arc<Mutex<Dom>> and NodeId captures are not GC-traced.
    let arc_lc = Arc::clone(dom_arc);
    let last_child_native = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let lid = {
                let dom = arc_lc.lock().unwrap_or_else(PoisonError::into_inner);
                dom.last_child(node_id).ok().flatten()
            };
            match lid {
                Some(id) => Ok(node_to_js_object(&arc_lc, id, ctx)),
                None => Ok(JsValue::null()),
            }
        })
    };

    // SAFETY: same -- Arc<Mutex<Dom>> and NodeId captures are not GC-traced.
    let arc_ns = Arc::clone(dom_arc);
    let next_sibling_native = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let sid = {
                let dom = arc_ns.lock().unwrap_or_else(PoisonError::into_inner);
                dom.next_sibling(node_id).ok().flatten()
            };
            match sid {
                Some(id) => Ok(node_to_js_object(&arc_ns, id, ctx)),
                None => Ok(JsValue::null()),
            }
        })
    };

    // SAFETY: same -- Arc<Mutex<Dom>> and NodeId captures are not GC-traced.
    let arc_ps = Arc::clone(dom_arc);
    let prev_sibling_native = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let sid = {
                let dom = arc_ps.lock().unwrap_or_else(PoisonError::into_inner);
                dom.previous_sibling(node_id).ok().flatten()
            };
            match sid {
                Some(id) => Ok(node_to_js_object(&arc_ps, id, ctx)),
                None => Ok(JsValue::null()),
            }
        })
    };

    // ---- mutation method closures (NativeFunction, passed to .function()) ----

    // SAFETY: same -- Arc<Mutex<Dom>> and NodeId captures are not GC-traced.
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

    // SAFETY: same -- Arc<Mutex<Dom>> and NodeId captures are not GC-traced.
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

    // SAFETY: same -- Arc<Mutex<Dom>> and NodeId captures are not GC-traced.
    let arc_ac = Arc::clone(dom_arc);
    let append_child = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let child_id = extract_node_id(args.first(), ctx)?;
            {
                let mut dom = arc_ac.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = dom.append_child(node_id, child_id);
            }
            Ok(node_to_js_object(&arc_ac, child_id, ctx))
        })
    };

    // SAFETY: same -- Arc<Mutex<Dom>> and NodeId captures are not GC-traced.
    let arc_rc = Arc::clone(dom_arc);
    let remove_child = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let child_id = extract_node_id(args.first(), ctx)?;
            {
                let mut dom = arc_rc.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = dom.remove_child(node_id, child_id);
            }
            Ok(node_to_js_object(&arc_rc, child_id, ctx))
        })
    };

    // SAFETY: same -- Arc<Mutex<Dom>> and NodeId captures are not GC-traced.
    let arc_ib = Arc::clone(dom_arc);
    let insert_before = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let new_id = extract_node_id(args.first(), ctx)?;
            let ref_id = extract_node_id(args.get(1), ctx)?;
            {
                let mut dom = arc_ib.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = dom.insert_before(node_id, new_id, ref_id);
            }
            Ok(node_to_js_object(&arc_ib, new_id, ctx))
        })
    };

    // SAFETY: same -- Arc<Mutex<Dom>> and NodeId captures are not GC-traced.
    let arc_rp = Arc::clone(dom_arc);
    let replace_child = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let new_id = extract_node_id(args.first(), ctx)?;
            let old_id = extract_node_id(args.get(1), ctx)?;
            {
                let mut dom = arc_rp.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = dom.insert_before(node_id, new_id, old_id);
                let _ = dom.remove_child(node_id, old_id);
            }
            Ok(node_to_js_object(&arc_rp, old_id, ctx))
        })
    };

    // ---- convert accessor NativeFunctions to JsFunctions --------------------
    // This borrows ctx.realm() briefly; borrows end before ObjectInitializer::new(ctx).

    let pn_getter: JsFunction = make_getter(ctx, pn_native);
    let child_nodes_getter: JsFunction = make_getter(ctx, child_nodes_native);
    let children_getter: JsFunction = make_getter(ctx, children_native);
    let first_child_getter: JsFunction = make_getter(ctx, first_child_native);
    let last_child_getter: JsFunction = make_getter(ctx, last_child_native);
    let next_sibling_getter: JsFunction = make_getter(ctx, next_sibling_native);
    let prev_sibling_getter: JsFunction = make_getter(ctx, prev_sibling_native);

    // ---- assemble the JS object ---------------------------------------------

    ObjectInitializer::new(ctx)
        // -- static snapshot properties --
        .property(
            js_string!("tagName"),
            JsString::from(snap.tag_name.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("nodeName"),
            JsString::from(snap.node_name.as_str()),
            Attribute::all(),
        )
        .property(js_string!("nodeType"), snap.node_type, Attribute::all())
        .property(
            js_string!("nodeValue"),
            JsString::from(snap.node_value.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("id"),
            JsString::from(snap.id_val.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("className"),
            JsString::from(snap.class_val.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("textContent"),
            JsString::from(snap.text.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("innerHTML"),
            JsString::from(snap.text.as_str()),
            Attribute::all(),
        )
        .property(js_string!("nodeId"), node_id.raw() as u32, Attribute::all())
        // -- live accessor properties --
        .accessor(
            js_string!("parentNode"),
            Some(pn_getter),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("childNodes"),
            Some(child_nodes_getter),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("children"),
            Some(children_getter),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("firstChild"),
            Some(first_child_getter),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("lastChild"),
            Some(last_child_getter),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("nextSibling"),
            Some(next_sibling_getter),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("previousSibling"),
            Some(prev_sibling_getter),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        // -- live mutation methods --
        .function(get_attribute, js_string!("getAttribute"), 1)
        .function(set_attribute, js_string!("setAttribute"), 2)
        .function(append_child, js_string!("appendChild"), 1)
        .function(remove_child, js_string!("removeChild"), 1)
        .function(insert_before, js_string!("insertBefore"), 2)
        .function(replace_child, js_string!("replaceChild"), 2)
        // -- event stubs (no-op; event loop deferred to a future phase) --
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
            let found = {
                let dom = arc1.lock().unwrap_or_else(PoisonError::into_inner);
                query_all(&dom, root, &format!("#{id}")).into_iter().next()
            };
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
            let found = {
                let dom = arc2.lock().unwrap_or_else(PoisonError::into_inner);
                query_all(&dom, root, &sel).into_iter().next()
            };
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

    // createElement(tag: string) -> element
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

    // createTextNode(text: string) -> text node
    // SAFETY: same as above.
    let arc5 = Arc::clone(dom_arc);
    let create_text_node = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let text = match args.first() {
                Some(v) => v.to_string(ctx)?.to_std_string_lossy(),
                None => String::new(),
            };
            let node_id = {
                let mut dom = arc5.lock().unwrap_or_else(PoisonError::into_inner);
                dom.create_text(text.as_str())
            };
            Ok(node_to_js_object(&arc5, node_id, ctx))
        })
    };

    // document.body getter
    // SAFETY: same as above.
    let arc_body = Arc::clone(dom_arc);
    let body_native = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let found = {
                let dom = arc_body.lock().unwrap_or_else(PoisonError::into_inner);
                query_all(&dom, root, "body").into_iter().next()
            };
            match found {
                Some(id) => Ok(node_to_js_object(&arc_body, id, ctx)),
                None => Ok(JsValue::null()),
            }
        })
    };

    // document.head getter
    // SAFETY: same as above.
    let arc_head = Arc::clone(dom_arc);
    let head_native = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let found = {
                let dom = arc_head.lock().unwrap_or_else(PoisonError::into_inner);
                query_all(&dom, root, "head").into_iter().next()
            };
            match found {
                Some(id) => Ok(node_to_js_object(&arc_head, id, ctx)),
                None => Ok(JsValue::null()),
            }
        })
    };

    // document.documentElement getter (the <html> element)
    // SAFETY: same as above.
    let arc_de = Arc::clone(dom_arc);
    let doc_el_native = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let found = {
                let dom = arc_de.lock().unwrap_or_else(PoisonError::into_inner);
                query_all(&dom, root, "html").into_iter().next()
            };
            match found {
                Some(id) => Ok(node_to_js_object(&arc_de, id, ctx)),
                None => Ok(JsValue::null()),
            }
        })
    };

    // Convert accessor NativeFunctions to JsFunctions before borrowing ctx mutably.
    let body_getter = make_getter(ctx, body_native);
    let head_getter = make_getter(ctx, head_native);
    let doc_el_getter = make_getter(ctx, doc_el_native);

    let document = ObjectInitializer::new(ctx)
        .function(get_element_by_id, js_string!("getElementById"), 1)
        .function(query_selector, js_string!("querySelector"), 1)
        .function(query_selector_all, js_string!("querySelectorAll"), 1)
        .function(create_element, js_string!("createElement"), 1)
        .function(create_text_node, js_string!("createTextNode"), 1)
        .function(
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::null())),
            js_string!("createElementNS"),
            2,
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
        .accessor(
            js_string!("body"),
            Some(body_getter),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("head"),
            Some(head_getter),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("documentElement"),
            Some(doc_el_getter),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .build();

    // UNWRAP-OK: if "document" is already defined, register_global_property overwrites it.
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

    #[test]
    fn append_child_and_children_getter() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var parent = document.getElementById('greeting'); \
             var child = document.createElement('p'); \
             parent.appendChild(child); \
             var kids = parent.children; \
             globalThis._len = kids.length;",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn create_text_node_returns_text_node() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var tn = document.createTextNode('hello'); \
             globalThis._type = tn.nodeType; \
             globalThis._name = tn.nodeName;",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn node_type_and_node_name_on_element() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.querySelector('div'); \
             globalThis._type = el ? el.nodeType : -1; \
             globalThis._name = el ? el.nodeName : '';",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn parent_node_accessor_returns_parent() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             var pn = el ? el.parentNode : null; \
             globalThis._has_parent = pn !== null;",
        )
        .expect("eval should succeed");
    }
}
