/*
 * dom_bridge maps silksurf_dom::Dom into Boa host objects.
 *
 * SilkContext::with_dom installs closures that capture Arc<Mutex<Dom>> and
 * NodeId values. These capture types contain no Boa GC-managed pointers, so
 * NativeFunction::from_closure does not need GC tracing for them.
 *
 * Host callbacks drop the DOM lock before calling node_to_js_object. That
 * keeps recursive wrapper construction outside the non-reentrant mutex.
 */

use std::sync::{Arc, Mutex, PoisonError};

use boa_engine::{
    Context, JsNativeError, JsResult, JsString, JsValue, NativeFunction, js_string,
    object::{
        FunctionObjectBuilder, JsObject, ObjectInitializer,
        builtins::{JsArray, JsFunction},
    },
    property::Attribute,
};
use silksurf_dom::{Dom, DomError, NodeId, NodeKind, TagName};

const EVENT_LISTENERS_REGISTRY: &str = "__silksurfEventListeners";

// ---- helpers ---------------------------------------------------------------

/// Node wrappers carry `nodeId`, and mutation methods route it back to Dom.
fn extract_node_id(arg: Option<&JsValue>, ctx: &mut Context) -> JsResult<NodeId> {
    let v = arg.ok_or_else(|| JsNativeError::typ().with_message("expected a node argument"))?;
    let obj = v
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("argument is not a node object"))?;
    let raw = obj.get(js_string!("nodeId"), ctx)?.to_u32(ctx)?;
    Ok(NodeId::from_raw(raw as usize))
}

fn extract_optional_node_id(arg: Option<&JsValue>, ctx: &mut Context) -> JsResult<Option<NodeId>> {
    match arg {
        None => Ok(None),
        Some(value) if value.is_null() || value.is_undefined() => Ok(None),
        Some(value) => extract_node_id(Some(value), ctx).map(Some),
    }
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

fn is_form_control_node(dom: &Dom, node_id: NodeId) -> bool {
    dom.element_name(node_id)
        .ok()
        .flatten()
        .is_some_and(|name| matches!(TagName::from_str(name), TagName::Input | TagName::Textarea))
}

fn form_control_value(dom: &Dom, node_id: NodeId) -> String {
    if !is_form_control_node(dom, node_id) {
        return String::new();
    }
    dom.attributes(node_id)
        .ok()
        .and_then(|attrs| {
            attrs
                .iter()
                .find(|attr| attr.name.as_str() == "value")
                .map(|attr| attr.value.to_string())
        })
        .unwrap_or_default()
}

fn event_listener_key(node_id: NodeId, event_type: &str) -> JsString {
    JsString::from(format!("{}:{event_type}", node_id.raw()).as_str())
}

fn event_listener_registry(ctx: &mut Context) -> JsResult<JsObject> {
    let key = js_string!(EVENT_LISTENERS_REGISTRY);
    let global = ctx.global_object().clone();
    let existing = global.get(key.clone(), ctx)?;
    if let Some(registry) = existing.as_object() {
        return Ok(registry.clone());
    }

    let registry = ObjectInitializer::new(ctx).build();
    global.set(key, registry.clone(), false, ctx)?;
    Ok(registry)
}

fn event_type_arg(arg: Option<&JsValue>, ctx: &mut Context) -> JsResult<Option<String>> {
    let Some(value) = arg else {
        return Ok(None);
    };
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    Ok(Some(value.to_string(ctx)?.to_std_string_lossy()))
}

fn event_type_from_dispatch_arg(arg: Option<&JsValue>, ctx: &mut Context) -> JsResult<String> {
    let Some(value) = arg else {
        return Ok(String::new());
    };
    if let Some(object) = value.as_object() {
        return Ok(object
            .get(js_string!("type"), ctx)?
            .to_string(ctx)?
            .to_std_string_lossy());
    }
    Ok(value.to_string(ctx)?.to_std_string_lossy())
}

fn listener_array(
    node_id: NodeId,
    event_type: &str,
    create: bool,
    ctx: &mut Context,
) -> JsResult<Option<JsArray>> {
    let registry = event_listener_registry(ctx)?;
    let key = event_listener_key(node_id, event_type);
    let existing = registry.get(key.clone(), ctx)?;
    if let Some(object) = existing.as_object()
        && object.is_array()
    {
        return Ok(Some(JsArray::from_object(object.clone())?));
    }
    if !create {
        return Ok(None);
    }

    let array = JsArray::new(ctx);
    registry.set(key, array.clone(), false, ctx)?;
    Ok(Some(array))
}

fn add_event_listener(
    node_id: NodeId,
    event_type: Option<&JsValue>,
    callback: Option<&JsValue>,
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(event_type) = event_type_arg(event_type, ctx)? else {
        return Ok(JsValue::undefined());
    };
    let Some(callback_object) = callback.and_then(JsValue::as_callable) else {
        return Ok(JsValue::undefined());
    };

    let Some(array) = listener_array(node_id, event_type.as_str(), true, ctx)? else {
        return Ok(JsValue::undefined());
    };
    let callback_value = JsValue::from(callback_object.clone());
    let length = array.length(ctx)?;
    for index in 0..length {
        if array.get(index, ctx)?.strict_equals(&callback_value) {
            return Ok(JsValue::undefined());
        }
    }
    array.push(callback_value, ctx)?;
    Ok(JsValue::undefined())
}

fn remove_event_listener(
    node_id: NodeId,
    event_type: Option<&JsValue>,
    callback: Option<&JsValue>,
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(event_type) = event_type_arg(event_type, ctx)? else {
        return Ok(JsValue::undefined());
    };
    let Some(callback_object) = callback.and_then(JsValue::as_callable) else {
        return Ok(JsValue::undefined());
    };
    let Some(array) = listener_array(node_id, event_type.as_str(), false, ctx)? else {
        return Ok(JsValue::undefined());
    };

    let callback_value = JsValue::from(callback_object.clone());
    let mut write_index = 0_u64;
    let length = array.length(ctx)?;
    for read_index in 0..length {
        let value = array.get(read_index, ctx)?;
        if value.strict_equals(&callback_value) {
            continue;
        }
        if write_index != read_index {
            array.set(write_index, value, false, ctx)?;
        }
        write_index += 1;
    }
    array.set(js_string!("length"), write_index, false, ctx)?;
    Ok(JsValue::undefined())
}

fn dispatch_event(
    this: &JsValue,
    node_id: NodeId,
    event_arg: Option<&JsValue>,
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let event_type = event_type_from_dispatch_arg(event_arg, ctx)?;
    if event_type.is_empty() {
        return Ok(JsValue::from(false));
    }
    let Some(array) = listener_array(node_id, event_type.as_str(), false, ctx)? else {
        return Ok(JsValue::from(true));
    };

    let event = if let Some(object) = event_arg.and_then(JsValue::as_object) {
        object.set(js_string!("target"), this.clone(), false, ctx)?;
        JsValue::from(object.clone())
    } else {
        let object = ObjectInitializer::new(ctx)
            .property(
                js_string!("type"),
                JsString::from(event_type.as_str()),
                Attribute::all(),
            )
            .property(js_string!("target"), this.clone(), Attribute::all())
            .build();
        JsValue::from(object)
    };

    let mut callbacks = Vec::new();
    let length = array.length(ctx)?;
    for index in 0..length {
        if let Some(callback) = array.get(index, ctx)?.as_callable() {
            callbacks.push(callback.clone());
        }
    }
    for callback in callbacks {
        callback.call(this, std::slice::from_ref(&event), ctx)?;
    }
    Ok(JsValue::from(true))
}

// ---- accessor getter builder -----------------------------------------------

/// `NativeFunction` becomes a `JsFunction` for `ObjectInitializer` accessors.
///
/// The returned function owns the built object after `ctx.realm()` is borrowed.
fn make_getter(ctx: &mut Context, f: NativeFunction) -> JsFunction {
    FunctionObjectBuilder::new(ctx.realm(), f).build()
}

// ---- node -> JS object -----------------------------------------------------

/// Build a JS object for a single DOM node.
///
/// Static properties snapshot the node at wrapper creation time.
///
/// Accessor properties re-lock the DOM and reflect current tree state.
///
/// Mutation methods acquire and release the DOM lock on each call.
///
/// Callers pass no held DOM lock into this function.
pub(super) fn node_to_js_object(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    ctx: &mut Context,
) -> JsValue {
    let snap = node_snapshot(dom_arc, node_id);
    let accessors = node_accessors(dom_arc, node_id, ctx);
    let methods = node_methods(dom_arc, node_id);
    let style = ObjectInitializer::new(ctx).build();
    let dataset = ObjectInitializer::new(ctx).build();

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
        .property(js_string!("style"), style, Attribute::all())
        .property(js_string!("dataset"), dataset, Attribute::all())
        .property(js_string!("nodeId"), node_id.raw() as u32, Attribute::all())
        // -- live accessor properties --
        .accessor(
            js_string!("parentNode"),
            Some(accessors.parent_node),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("childNodes"),
            Some(accessors.child_nodes),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("children"),
            Some(accessors.children),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("firstChild"),
            Some(accessors.first_child),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("lastChild"),
            Some(accessors.last_child),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("nextSibling"),
            Some(accessors.next_sibling),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("previousSibling"),
            Some(accessors.previous_sibling),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("textContent"),
            Some(accessors.text_content_get),
            Some(accessors.text_content_set),
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("innerHTML"),
            Some(accessors.inner_html_get),
            Some(accessors.inner_html_set),
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("value"),
            Some(accessors.value_get),
            Some(accessors.value_set),
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("src"),
            Some(accessors.src_get),
            Some(accessors.src_set),
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        // -- live mutation methods --
        .function(methods.get_attribute, js_string!("getAttribute"), 1)
        .function(methods.set_attribute, js_string!("setAttribute"), 2)
        .function(methods.append_child, js_string!("appendChild"), 1)
        .function(methods.remove_child, js_string!("removeChild"), 1)
        .function(methods.insert_before, js_string!("insertBefore"), 2)
        .function(methods.replace_child, js_string!("replaceChild"), 2)
        // -- event listener methods --
        .function(methods.add_event_listener, js_string!("addEventListener"), 2)
        .function(
            methods.remove_event_listener,
            js_string!("removeEventListener"),
            2,
        )
        .function(methods.dispatch_event, js_string!("dispatchEvent"), 1)
        .build()
        .into()
}

struct NodeAccessors {
    parent_node: JsFunction,
    child_nodes: JsFunction,
    children: JsFunction,
    first_child: JsFunction,
    last_child: JsFunction,
    next_sibling: JsFunction,
    previous_sibling: JsFunction,
    text_content_get: JsFunction,
    text_content_set: JsFunction,
    inner_html_get: JsFunction,
    inner_html_set: JsFunction,
    value_get: JsFunction,
    value_set: JsFunction,
    src_get: JsFunction,
    src_set: JsFunction,
}

struct NodeMethods {
    get_attribute: NativeFunction,
    set_attribute: NativeFunction,
    append_child: NativeFunction,
    remove_child: NativeFunction,
    insert_before: NativeFunction,
    replace_child: NativeFunction,
    add_event_listener: NativeFunction,
    remove_event_listener: NativeFunction,
    dispatch_event: NativeFunction,
}

fn node_snapshot(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NodeSnapshot {
    let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    snapshot_node(&dom, node_id)
}

fn node_accessors(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId, ctx: &mut Context) -> NodeAccessors {
    NodeAccessors {
        parent_node: make_getter(ctx, related_node_native(dom_arc, node_id, Dom::parent)),
        child_nodes: make_getter(ctx, child_nodes_native(dom_arc, node_id, false)),
        children: make_getter(ctx, child_nodes_native(dom_arc, node_id, true)),
        first_child: make_getter(ctx, related_node_native(dom_arc, node_id, Dom::first_child)),
        last_child: make_getter(ctx, related_node_native(dom_arc, node_id, Dom::last_child)),
        next_sibling: make_getter(
            ctx,
            related_node_native(dom_arc, node_id, Dom::next_sibling),
        ),
        previous_sibling: make_getter(
            ctx,
            related_node_native(dom_arc, node_id, Dom::previous_sibling),
        ),
        text_content_get: make_getter(ctx, text_content_get_native(dom_arc, node_id)),
        text_content_set: make_getter(ctx, text_content_set_native(dom_arc, node_id)),
        inner_html_get: make_getter(ctx, text_content_get_native(dom_arc, node_id)),
        inner_html_set: make_getter(ctx, text_content_set_native(dom_arc, node_id)),
        value_get: make_getter(ctx, value_get_native(dom_arc, node_id)),
        value_set: make_getter(ctx, value_set_native(dom_arc, node_id)),
        src_get: make_getter(ctx, attribute_get_native(dom_arc, node_id, "src")),
        src_set: make_getter(ctx, attribute_set_native(dom_arc, node_id, "src")),
    }
}

fn node_methods(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NodeMethods {
    NodeMethods {
        get_attribute: get_attribute_native(dom_arc, node_id),
        set_attribute: set_attribute_native(dom_arc, node_id),
        append_child: append_child_native(dom_arc, node_id),
        remove_child: remove_child_native(dom_arc, node_id),
        insert_before: insert_before_native(dom_arc, node_id),
        replace_child: replace_child_native(dom_arc, node_id),
        add_event_listener: node_add_event_listener_native(node_id),
        remove_event_listener: node_remove_event_listener_native(node_id),
        dispatch_event: node_dispatch_event_native(node_id),
    }
}

fn related_node_native(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    relation: fn(&Dom, NodeId) -> Result<Option<NodeId>, DomError>,
) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let related = {
                let dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                relation(&dom, node_id).unwrap_or(None)
            };
            match related {
                Some(id) => Ok(node_to_js_object(&arc, id, ctx)),
                None => Ok(JsValue::null()),
            }
        })
    }
}

fn child_nodes_native(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    elements_only: bool,
) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let nodes = node_children(&arc, node_id, elements_only);
            node_array(&arc, nodes, ctx)
        })
    }
}

fn node_children(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId, elements_only: bool) -> Vec<NodeId> {
    let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    let Ok(children) = dom.children(node_id) else {
        return Vec::new();
    };
    children
        .iter()
        .copied()
        .filter(|id| !elements_only || dom.element_name(*id).ok().flatten().is_some())
        .collect()
}

fn node_array(
    dom_arc: &Arc<Mutex<Dom>>,
    nodes: Vec<NodeId>,
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let arr = JsArray::new(ctx);
    for node_id in nodes {
        arr.push(node_to_js_object(dom_arc, node_id, ctx), ctx)?;
    }
    Ok(JsValue::from(arr))
}

fn get_attribute_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let name = match args.first() {
                Some(value) => value.to_string(ctx)?.to_std_string_lossy(),
                None => return Ok(JsValue::null()),
            };
            let value = {
                let dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                dom.attributes(node_id).ok().and_then(|attrs| {
                    attrs
                        .iter()
                        .find(|attr| attr.name.as_str() == name)
                        .map(|attr| attr.value.to_string())
                })
            };
            Ok(value.map_or_else(JsValue::null, |value| {
                JsValue::from(JsString::from(value.as_str()))
            }))
        })
    }
}

fn set_attribute_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let name = match args.first() {
                Some(value) => value.to_string(ctx)?.to_std_string_lossy(),
                None => return Ok(JsValue::undefined()),
            };
            let value = args
                .get(1)
                .map(|value| value.to_string(ctx).map(|s| s.to_std_string_lossy()))
                .transpose()?
                .unwrap_or_default();
            let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = dom.set_attribute(node_id, name, value);
            Ok(JsValue::undefined())
        })
    }
}

fn attribute_get_native(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    attribute_name: &'static str,
) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let value = {
                let dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                dom.attributes(node_id).ok().and_then(|attrs| {
                    attrs
                        .iter()
                        .find(|attr| attr.name.as_str() == attribute_name)
                        .map(|attr| attr.value.to_string())
                })
            };
            Ok(JsValue::from(JsString::from(
                value.unwrap_or_default().as_str(),
            )))
        })
    }
}

fn attribute_set_native(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    attribute_name: &'static str,
) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let value = args
                .first()
                .map(|value| value.to_string(ctx).map(|s| s.to_std_string_lossy()))
                .transpose()?
                .unwrap_or_default();
            let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = dom.set_attribute(node_id, attribute_name, value);
            Ok(JsValue::undefined())
        })
    }
}

fn append_child_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let child = extract_node_id(args.first(), ctx)?;
            {
                let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                detach_from_parent(&mut dom, child);
                let _ = dom.append_child(node_id, child);
            }
            Ok(node_to_js_object(&arc, child, ctx))
        })
    }
}

fn remove_child_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let child = extract_node_id(args.first(), ctx)?;
            {
                let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = dom.remove_child(node_id, child);
            }
            Ok(node_to_js_object(&arc, child, ctx))
        })
    }
}

fn insert_before_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let child = extract_node_id(args.first(), ctx)?;
            let reference = extract_optional_node_id(args.get(1), ctx)?;
            insert_before_or_append(&arc, node_id, child, reference);
            Ok(node_to_js_object(&arc, child, ctx))
        })
    }
}

fn insert_before_or_append(
    dom_arc: &Arc<Mutex<Dom>>,
    parent: NodeId,
    child: NodeId,
    reference: Option<NodeId>,
) {
    let mut dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(reference) = reference {
        let _ = dom.insert_before(parent, child, reference);
    } else {
        detach_from_parent(&mut dom, child);
        let _ = dom.append_child(parent, child);
    }
}

fn replace_child_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let child = extract_node_id(args.first(), ctx)?;
            let old_child = extract_node_id(args.get(1), ctx)?;
            {
                let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = dom.insert_before(node_id, child, old_child);
                let _ = dom.remove_child(node_id, old_child);
            }
            Ok(node_to_js_object(&arc, old_child, ctx))
        })
    }
}

fn text_content_get_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            Ok(JsValue::from(JsString::from(
                collect_text(&dom, node_id).as_str(),
            )))
        })
    }
}

fn text_content_set_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let text = args
                .first()
                .map(|value| value.to_string(ctx).map(|s| s.to_std_string_lossy()))
                .transpose()?
                .unwrap_or_default();
            let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = dom.set_text_content(node_id, text);
            Ok(JsValue::undefined())
        })
    }
}

fn value_get_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            Ok(JsValue::from(JsString::from(
                form_control_value(&dom, node_id).as_str(),
            )))
        })
    }
}

fn value_set_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let value = args
                .first()
                .map(|value| value.to_string(ctx).map(|s| s.to_std_string_lossy()))
                .transpose()?
                .unwrap_or_default();
            let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = dom.set_attribute(node_id, "value", value);
            Ok(JsValue::undefined())
        })
    }
}

fn node_add_event_listener_native(node_id: NodeId) -> NativeFunction {
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            add_event_listener(node_id, args.first(), args.get(1), ctx)
        })
    }
}

fn node_remove_event_listener_native(node_id: NodeId) -> NativeFunction {
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            remove_event_listener(node_id, args.first(), args.get(1), ctx)
        })
    }
}

fn node_dispatch_event_native(node_id: NodeId) -> NativeFunction {
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |this, args, ctx| {
            dispatch_event(this, node_id, args.first(), ctx)
        })
    }
}

fn detach_from_parent(dom: &mut Dom, node_id: NodeId) {
    if let Ok(Some(parent_id)) = dom.parent(node_id) {
        let _ = dom.remove_child(parent_id, node_id);
    }
}

// ---- document object builder ------------------------------------------------

/// Replace the stub document object in `ctx` with one that queries `dom_arc`.
pub(super) fn install_document(dom_arc: &Arc<Mutex<Dom>>, ctx: &mut Context) {
    let root = NodeId::from_raw(0);
    let methods = document_methods(dom_arc, root);
    let accessors = document_accessors(dom_arc, root, ctx);
    let cookie_jar = super::new_cookie_jar();
    let cookie_getter = super::document_cookie_getter(ctx, &cookie_jar);
    let cookie_setter = super::document_cookie_setter(ctx, &cookie_jar);

    let document = ObjectInitializer::new(ctx)
        .function(methods.get_element_by_id, js_string!("getElementById"), 1)
        .function(methods.query_selector, js_string!("querySelector"), 1)
        .function(
            methods.query_selector_all,
            js_string!("querySelectorAll"),
            1,
        )
        .function(
            methods.get_elements_by_tag_name,
            js_string!("getElementsByTagName"),
            1,
        )
        .function(methods.create_element, js_string!("createElement"), 1)
        .function(methods.create_text_node, js_string!("createTextNode"), 1)
        .function(
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::null())),
            js_string!("createElementNS"),
            2,
        )
        .function(
            methods.add_event_listener,
            js_string!("addEventListener"),
            2,
        )
        .function(
            methods.remove_event_listener,
            js_string!("removeEventListener"),
            2,
        )
        .function(methods.dispatch_event, js_string!("dispatchEvent"), 1)
        .property(
            js_string!("readyState"),
            js_string!("loading"),
            Attribute::all(),
        )
        .accessor(
            js_string!("body"),
            Some(accessors.body),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("head"),
            Some(accessors.head),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("documentElement"),
            Some(accessors.document_element),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("cookie"),
            Some(cookie_getter),
            Some(cookie_setter),
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .build();

    // UNWRAP-OK: if "document" is already defined, register_global_property overwrites it.
    let _ = ctx.register_global_property(js_string!("document"), document, Attribute::all());
}

struct DocumentAccessors {
    body: JsFunction,
    head: JsFunction,
    document_element: JsFunction,
}

struct DocumentMethods {
    get_element_by_id: NativeFunction,
    query_selector: NativeFunction,
    query_selector_all: NativeFunction,
    get_elements_by_tag_name: NativeFunction,
    create_element: NativeFunction,
    create_text_node: NativeFunction,
    add_event_listener: NativeFunction,
    remove_event_listener: NativeFunction,
    dispatch_event: NativeFunction,
}

fn document_accessors(
    dom_arc: &Arc<Mutex<Dom>>,
    root: NodeId,
    ctx: &mut Context,
) -> DocumentAccessors {
    DocumentAccessors {
        body: make_getter(ctx, document_selector_getter_native(dom_arc, root, "body")),
        head: make_getter(ctx, document_selector_getter_native(dom_arc, root, "head")),
        document_element: make_getter(ctx, document_selector_getter_native(dom_arc, root, "html")),
    }
}

fn document_methods(dom_arc: &Arc<Mutex<Dom>>, root: NodeId) -> DocumentMethods {
    DocumentMethods {
        get_element_by_id: document_get_element_by_id_native(dom_arc, root),
        query_selector: document_query_selector_native(dom_arc, root),
        query_selector_all: document_query_selector_all_native(dom_arc, root),
        get_elements_by_tag_name: document_get_elements_by_tag_name_native(dom_arc, root),
        create_element: document_create_element_native(dom_arc),
        create_text_node: document_create_text_node_native(dom_arc),
        add_event_listener: node_add_event_listener_native(root),
        remove_event_listener: node_remove_event_listener_native(root),
        dispatch_event: node_dispatch_event_native(root),
    }
}

fn selector_arg(arg: Option<&JsValue>, ctx: &mut Context) -> JsResult<Option<String>> {
    match arg {
        Some(value) => Ok(Some(value.to_string(ctx)?.to_std_string_lossy())),
        None => Ok(None),
    }
}

fn query_first_value(
    dom_arc: &Arc<Mutex<Dom>>,
    root: NodeId,
    selector: &str,
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let found = {
        let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
        query_all(&dom, root, selector).into_iter().next()
    };
    match found {
        Some(node_id) => Ok(node_to_js_object(dom_arc, node_id, ctx)),
        None => Ok(JsValue::null()),
    }
}

fn query_array_value(
    dom_arc: &Arc<Mutex<Dom>>,
    root: NodeId,
    selector: &str,
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let nodes = {
        let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
        query_all(&dom, root, selector)
    };
    node_array(dom_arc, nodes, ctx)
}

fn document_selector_getter_native(
    dom_arc: &Arc<Mutex<Dom>>,
    root: NodeId,
    selector: &'static str,
) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            query_first_value(&arc, root, selector, ctx)
        })
    }
}

fn document_get_element_by_id_native(dom_arc: &Arc<Mutex<Dom>>, root: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let Some(id) = selector_arg(args.first(), ctx)? else {
                return Ok(JsValue::null());
            };
            query_first_value(&arc, root, &format!("#{id}"), ctx)
        })
    }
}

fn document_query_selector_native(dom_arc: &Arc<Mutex<Dom>>, root: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let Some(selector) = selector_arg(args.first(), ctx)? else {
                return Ok(JsValue::null());
            };
            query_first_value(&arc, root, &selector, ctx)
        })
    }
}

fn document_query_selector_all_native(dom_arc: &Arc<Mutex<Dom>>, root: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let Some(selector) = selector_arg(args.first(), ctx)? else {
                return Ok(JsValue::from(JsArray::new(ctx)));
            };
            query_array_value(&arc, root, &selector, ctx)
        })
    }
}

fn document_get_elements_by_tag_name_native(
    dom_arc: &Arc<Mutex<Dom>>,
    root: NodeId,
) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let Some(tag) = selector_arg(args.first(), ctx)? else {
                return Ok(JsValue::from(JsArray::new(ctx)));
            };
            query_array_value(&arc, root, &tag, ctx)
        })
    }
}

fn document_create_element_native(dom_arc: &Arc<Mutex<Dom>>) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let Some(tag) = selector_arg(args.first(), ctx)? else {
                return Ok(JsValue::null());
            };
            let node_id = {
                let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                dom.create_element(tag.as_str())
            };
            Ok(node_to_js_object(&arc, node_id, ctx))
        })
    }
}

fn document_create_text_node_native(dom_arc: &Arc<Mutex<Dom>>) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let text = selector_arg(args.first(), ctx)?.unwrap_or_default();
            let node_id = {
                let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                dom.create_text(text.as_str())
            };
            Ok(node_to_js_object(&arc, node_id, ctx))
        })
    }
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::boa_backend::SilkContext;
    use silksurf_dom::{Dom, NodeId, NodeKind};
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

    fn input_dom() -> (Arc<Mutex<Dom>>, NodeId) {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let input = dom.create_element("input");
        let _ = dom.set_attribute(input, "id", "prompt");
        let _ = dom.set_attribute(input, "value", "Hi");
        let _ = dom.append_child(root, input);
        dom.materialize_resolve_table();
        (Arc::new(Mutex::new(dom)), input)
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
    fn text_content_assignment_marks_text_node_dirty() {
        let (arc, _root) = simple_dom();
        let text_node = {
            let mut dom = arc.lock().unwrap();
            let parent = document_greeting(&dom);
            let text_node = dom.first_child(parent).unwrap().expect("text child");
            let _ = dom.take_dirty_nodes();
            text_node
        };

        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             el.firstChild.textContent = 'Updated';",
        )
        .expect("eval should succeed");

        let mut dom = arc.lock().unwrap();
        assert_eq!(
            dom.node(text_node).unwrap().kind(),
            &NodeKind::Text {
                text: "Updated".to_string()
            }
        );
        assert_eq!(dom.take_dirty_nodes(), vec![text_node]);
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
    fn input_value_accessor_reads_attribute() {
        let (arc, _input) = input_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('prompt'); \
             if (el.value !== 'Hi') { throw new Error('input value mismatch'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn input_value_assignment_marks_input_dirty() {
        let (arc, input) = input_dom();
        {
            let mut dom = arc.lock().unwrap();
            let _ = dom.take_dirty_nodes();
        }

        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('prompt'); \
             el.value = 'Updated';",
        )
        .expect("eval should succeed");

        let mut dom = arc.lock().unwrap();
        let value = dom
            .attributes(input)
            .unwrap()
            .iter()
            .find(|attr| attr.name.as_str() == "value")
            .map(|attr| attr.value.as_str().to_string());
        assert_eq!(value.as_deref(), Some("Updated"));
        assert_eq!(dom.take_dirty_nodes(), vec![input]);
    }

    #[test]
    fn event_listener_dispatch_invokes_callback() {
        let (arc, _input) = input_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('prompt'); \
             var count = 0; \
             function onInput(event) { \
               if (event.type !== 'input') { throw new Error('event type mismatch'); } \
               if (event.target !== el) { throw new Error('event target mismatch'); } \
               count = count + 1; \
             } \
             el.addEventListener('input', onInput); \
             if (el.dispatchEvent({ type: 'input' }) !== true) { throw new Error('dispatch failed'); } \
             if (count !== 1) { throw new Error('listener count mismatch'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn event_listener_deduplicates_same_callback() {
        let (arc, _input) = input_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('prompt'); \
             var count = 0; \
             function onInput() { count = count + 1; } \
             el.addEventListener('input', onInput); \
             el.addEventListener('input', onInput); \
             el.dispatchEvent('input'); \
             if (count !== 1) { throw new Error('duplicate listener fired'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn remove_event_listener_skips_removed_callback() {
        let (arc, _input) = input_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('prompt'); \
             var count = 0; \
             function onInput() { count = count + 1; } \
             el.addEventListener('input', onInput); \
             el.removeEventListener('input', onInput); \
             el.dispatchEvent({ type: 'input' }); \
             if (count !== 0) { throw new Error('removed listener fired'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn document_event_listener_dispatch_invokes_callback() {
        let (arc, _input) = input_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var count = 0; \
             document.addEventListener('visibilitychange', function(event) { \
               if (event.target !== document) { throw new Error('document target mismatch'); } \
               count = count + 1; \
             }); \
             document.dispatchEvent({ type: 'visibilitychange' }); \
             if (count !== 1) { throw new Error('document listener count mismatch'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn document_cookie_accessor_survives_dom_document_install() {
        let (arc, _input) = input_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "document.cookie = 'sid=dom'; \
             document.cookie = 'mode=chat'; \
             if (document.cookie !== 'sid=dom; mode=chat') { throw new Error('cookie mismatch'); }",
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
    fn insert_before_null_appends_child() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var parent = document.getElementById('greeting'); \
             var child = document.createElement('span'); \
             var returned = parent.insertBefore(child, null); \
             if (returned.nodeId !== child.nodeId) { throw new Error('insertBefore return mismatch'); } \
             if (parent.lastChild.nodeId !== child.nodeId) { throw new Error('insertBefore did not append'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn insert_before_moves_existing_child() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var parent = document.getElementById('greeting'); \
             var child = document.querySelector('span'); \
             parent.insertBefore(child, null); \
             if (child.parentNode.nodeId !== parent.nodeId) { throw new Error('child parent mismatch'); } \
             if (parent.lastChild.nodeId !== child.nodeId) { throw new Error('child did not move'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn element_style_dataset_and_tag_lookup_exist() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.createElement('iframe'); \
             el.style.position = 'absolute'; \
             el.dataset.kind = 'probe'; \
             if (el.style.position !== 'absolute') { throw new Error('style write failed'); } \
             if (el.dataset.kind !== 'probe') { throw new Error('dataset write failed'); } \
             if (document.readyState !== 'loading') { throw new Error('readyState mismatch'); } \
             if (document.getElementsByTagName('body').length !== 0) { throw new Error('unexpected body'); }",
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

    fn document_greeting(dom: &Dom) -> NodeId {
        super::query_all(dom, NodeId::from_raw(0), "#greeting")
            .into_iter()
            .next()
            .expect("greeting node")
    }
}
