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
        FunctionObjectBuilder, ObjectInitializer,
        builtins::{JsArray, JsFunction},
    },
    property::Attribute,
};
use silksurf_css::{CssTokenizer, SelectorList, matches_selector_list, parse_selector_list};
use silksurf_dom::{Dom, DomError, NodeId, NodeKind, TagName};

use super::event_dispatch;

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

// ---- selector matching (silksurf-css engine) --------------------------------

/// querySelector in a rAF loop is a hot path; the parse cache turns repeated
/// selector strings into one clone. The cache is thread-local because a
/// `SilkContext` never crosses threads; the cap bounds adversarial pages that
/// generate unbounded distinct selector strings.
const SELECTOR_CACHE_CAP: usize = 256;

thread_local! {
    static SELECTOR_CACHE: std::cell::RefCell<std::collections::HashMap<String, SelectorList>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Parse a selector string through the real CSS engine. Empty or unparseable
/// selectors yield None (querySelector then matches nothing, mirroring the
/// forgiving behavior the old shim had).
fn parse_selector_cached(selector: &str) -> Option<SelectorList> {
    let cached = SELECTOR_CACHE.with(|cache| cache.borrow().get(selector).cloned());
    if let Some(list) = cached {
        return Some(list);
    }
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed(selector).ok()?;
    tokens.extend(tokenizer.finish().ok()?);
    let list = parse_selector_list(tokens);
    if list.selectors.is_empty() {
        return None;
    }
    SELECTOR_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= SELECTOR_CACHE_CAP {
            cache.clear();
        }
        cache.insert(selector.to_string(), list.clone());
    });
    Some(list)
}

fn collect_matches(dom: &Dom, node: NodeId, list: &SelectorList, out: &mut Vec<NodeId>) {
    if dom.element_name(node).ok().flatten().is_some() && matches_selector_list(dom, node, list) {
        out.push(node);
    }
    if let Ok(children) = dom.children(node) {
        let owned: Vec<NodeId> = children.to_vec();
        for child in owned {
            collect_matches(dom, child, list, out);
        }
    }
}

pub(super) fn query_all(dom: &Dom, root: NodeId, selector: &str) -> Vec<NodeId> {
    let Some(list) = parse_selector_cached(selector) else {
        return Vec::new();
    };
    let mut results = Vec::new();
    collect_matches(dom, root, &list, &mut results);
    results
}

/// Element.matches(selector).
fn node_matches(dom: &Dom, node: NodeId, selector: &str) -> bool {
    parse_selector_cached(selector).is_some_and(|list| matches_selector_list(dom, node, &list))
}

/// Element.closest(selector): self first, then ancestors.
fn node_closest(dom: &Dom, node: NodeId, selector: &str) -> Option<NodeId> {
    let list = parse_selector_cached(selector)?;
    let mut current = Some(node);
    while let Some(candidate) = current {
        if dom.element_name(candidate).ok().flatten().is_some()
            && matches_selector_list(dom, candidate, &list)
        {
            return Some(candidate);
        }
        current = dom.parent(candidate).ok().flatten();
    }
    None
}

fn node_matches_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let Some(selector) = selector_arg(args.first(), ctx)? else {
                return Ok(JsValue::from(false));
            };
            let matched = {
                let dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                node_matches(&dom, node_id, &selector)
            };
            Ok(JsValue::from(matched))
        })
    }
}

fn node_closest_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let Some(selector) = selector_arg(args.first(), ctx)? else {
                return Ok(JsValue::null());
            };
            let found = {
                let dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                node_closest(&dom, node_id, &selector)
            };
            match found {
                Some(id) => Ok(node_to_js_object(&arc, id, ctx)),
                None => Ok(JsValue::null()),
            }
        })
    }
}

fn node_query_selector_native(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    all: bool,
) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let Some(selector) = selector_arg(args.first(), ctx)? else {
                return if all {
                    Ok(JsValue::from(JsArray::new(ctx)))
                } else {
                    Ok(JsValue::null())
                };
            };
            // Scoped query: match descendants only, not the context node.
            let matches = {
                let dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                let mut out = Vec::new();
                if let Some(list) = parse_selector_cached(&selector)
                    && let Ok(children) = dom.children(node_id)
                {
                    let owned: Vec<NodeId> = children.to_vec();
                    for child in owned {
                        collect_matches(&dom, child, &list, &mut out);
                    }
                }
                out
            };
            if all {
                node_array(&arc, matches, ctx)
            } else {
                match matches.into_iter().next() {
                    Some(id) => Ok(node_to_js_object(&arc, id, ctx)),
                    None => Ok(JsValue::null()),
                }
            }
        })
    }
}

// ---- node info snapshot (dom must be held by caller) -----------------------

/// The immutable-per-node properties that stay static on the wrapper. Mutable
/// properties (`nodeValue`, `data`, `id`, `className`) are live accessors so a
/// cached wrapper reflects later Dom writes instead of a first-access snapshot.
struct NodeSnapshot {
    tag_name: String,
    node_name: String,
    node_type: u32,
}

fn snapshot_node(dom: &Dom, node_id: NodeId) -> NodeSnapshot {
    if let Ok(n) = dom.node(node_id) {
        match n.kind() {
            NodeKind::Text { .. } => {
                return NodeSnapshot {
                    tag_name: String::new(),
                    node_name: "#text".into(),
                    node_type: 3,
                };
            }
            NodeKind::Document => {
                return NodeSnapshot {
                    tag_name: String::new(),
                    node_name: "#document".into(),
                    node_type: 9,
                };
            }
            NodeKind::Comment { .. } => {
                return NodeSnapshot {
                    tag_name: String::new(),
                    node_name: "#comment".into(),
                    node_type: 8,
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
    NodeSnapshot {
        node_name: tag.clone(),
        tag_name: tag,
        node_type: 1,
    }
}

/// The own character data of a Text or Comment node; empty for other kinds.
/// `nodeValue`/`data` reads route here so a cached wrapper stays live.
fn own_character_data(dom: &Dom, node_id: NodeId) -> String {
    if let Ok(n) = dom.node(node_id) {
        match n.kind() {
            NodeKind::Text { text } => return text.clone(),
            NodeKind::Comment { data: comment_text } => return comment_text.clone(),
            _ => {}
        }
    }
    String::new()
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

fn add_event_listener(
    node_id: NodeId,
    event_type: Option<&JsValue>,
    callback: Option<&JsValue>,
    options: Option<&JsValue>,
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(event_type) = event_type_arg(event_type, ctx)? else {
        return Ok(JsValue::undefined());
    };
    let Some(callback_object) = callback.and_then(JsValue::as_callable) else {
        return Ok(JsValue::undefined());
    };
    event_dispatch::add_listener(node_id, event_type.as_str(), &callback_object, options, ctx)?;
    Ok(JsValue::undefined())
}

fn remove_event_listener(
    node_id: NodeId,
    event_type: Option<&JsValue>,
    callback: Option<&JsValue>,
    options: Option<&JsValue>,
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(event_type) = event_type_arg(event_type, ctx)? else {
        return Ok(JsValue::undefined());
    };
    let Some(callback_object) = callback.and_then(JsValue::as_callable) else {
        return Ok(JsValue::undefined());
    };
    event_dispatch::remove_listener(node_id, event_type.as_str(), &callback_object, options, ctx)?;
    Ok(JsValue::undefined())
}

fn dispatch_event(
    this: &JsValue,
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    event_arg: Option<&JsValue>,
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let event_type = event_type_from_dispatch_arg(event_arg, ctx)?;
    if event_type.is_empty() {
        return Ok(JsValue::from(false));
    }
    let event = if let Some(object) = event_arg.and_then(JsValue::as_object) {
        object.clone()
    } else {
        event_dispatch::build_event_object(event_type.as_str(), false, false, false, ctx)
    };
    let proceed = event_dispatch::propagate_event(dom_arc, node_id, this, &event, ctx)?;
    Ok(JsValue::from(proceed))
}

// ---- accessor getter builder -----------------------------------------------

/// `NativeFunction` becomes a `JsFunction` for `ObjectInitializer` accessors.
///
/// The returned function owns the built object after `ctx.realm()` is borrowed.
fn make_getter(ctx: &mut Context, f: NativeFunction) -> JsFunction {
    FunctionObjectBuilder::new(ctx.realm(), f).build()
}

// ---- node -> JS object -----------------------------------------------------

/// Hidden global caching one JS wrapper per DOM node, keyed by `nodeId`.
///
/// Wrapper identity must persist across accesses. Frameworks stamp expando
/// properties on the node object -- React writes its fiber pointer at commit
/// and reads it back at event dispatch -- so `getElementById(x)`, the
/// `createElement` result, and the event target must all be the same object.
/// A fresh wrapper per access strands the expando on a dead object and drops
/// delegated events.
///
/// The key never remaps: `Dom::push_node` assigns `NodeId(nodes.len())` and
/// never recycles, so a nodeId denotes one node for the Dom's lifetime, even
/// across detach and reattach. No wrapper is evicted on removal; the arena
/// keeps the node, and React reattaches the same node expecting its expando to
/// survive. Each `SilkContext::with_dom` builds a fresh Context, so the
/// registry is per-Dom and never spans arenas.
const NODE_WRAPPER_REGISTRY: &str = "__silksurfNodeWrappers";

fn wrapper_key(node_id: NodeId) -> JsString {
    JsString::from(node_id.raw().to_string().as_str())
}

/// Return the wrapper built for `node_id` earlier in this realm, if any.
fn cached_wrapper(node_id: NodeId, ctx: &mut Context) -> Option<JsValue> {
    let registry = event_dispatch::hidden_global_object(NODE_WRAPPER_REGISTRY, ctx).ok()?;
    let existing = registry.get(wrapper_key(node_id), ctx).ok()?;
    existing.as_object().is_some().then_some(existing)
}

/// Publish the freshly built wrapper so later accesses share its identity.
fn store_wrapper(node_id: NodeId, wrapper: &JsValue, ctx: &mut Context) {
    if let Ok(registry) = event_dispatch::hidden_global_object(NODE_WRAPPER_REGISTRY, ctx) {
        let _ = registry.set(wrapper_key(node_id), wrapper.clone(), false, ctx);
    }
}

/// Build a JS object for a single DOM node.
///
/// Static properties snapshot the node at wrapper creation time.
///
/// Accessor properties re-lock the DOM and reflect current tree state.
///
/// Mutation methods acquire and release the DOM lock on each call.
///
/// A per-node wrapper cache (`NODE_WRAPPER_REGISTRY`) makes the returned object
/// stable across accesses, so expando properties persist.
///
/// Callers pass no held DOM lock into this function.
pub(super) fn node_to_js_object(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    ctx: &mut Context,
) -> JsValue {
    if let Some(cached) = cached_wrapper(node_id, ctx) {
        return cached;
    }
    let snap = node_snapshot(dom_arc, node_id);
    let accessors = node_accessors(dom_arc, node_id, ctx);
    let methods = node_methods(dom_arc, node_id);
    let style = super::css_object::make_proxy_for_node("__silksurfMakeStyleProxy", node_id, ctx);
    let dataset =
        super::css_object::make_proxy_for_node("__silksurfMakeDatasetProxy", node_id, ctx);

    // ---- assemble the JS object ---------------------------------------------

    let wrapper = ObjectInitializer::new(ctx)
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
        .property(js_string!("style"), style, Attribute::all())
        .property(js_string!("dataset"), dataset, Attribute::all())
        .property(js_string!("nodeId"), node_id.raw() as u32, Attribute::all())
        // -- live accessor properties --
        // nodeValue/data/id/className mutate after the wrapper is cached, so
        // they read and write the Dom rather than freezing a first-access value.
        .accessor(
            js_string!("nodeValue"),
            Some(accessors.node_value_get),
            Some(accessors.node_value_set),
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("data"),
            Some(accessors.data_get),
            Some(accessors.data_set),
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("id"),
            Some(accessors.id_get),
            Some(accessors.id_set),
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("className"),
            Some(accessors.class_name_get),
            Some(accessors.class_name_set),
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("ownerDocument"),
            Some(accessors.owner_document),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
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
        // -- selector queries (silksurf-css engine) --
        .function(node_matches_native(dom_arc, node_id), js_string!("matches"), 1)
        .function(node_closest_native(dom_arc, node_id), js_string!("closest"), 1)
        .function(
            node_query_selector_native(dom_arc, node_id, false),
            js_string!("querySelector"),
            1,
        )
        .function(
            node_query_selector_native(dom_arc, node_id, true),
            js_string!("querySelectorAll"),
            1,
        )
        // -- canvas 2D context (returns null for non-canvas elements) --
        .function(getcontext_native(dom_arc, node_id), js_string!("getContext"), 1)
        .build();
    let wrapper: JsValue = wrapper.into();
    store_wrapper(node_id, &wrapper, ctx);
    wrapper
}

struct NodeAccessors {
    owner_document: JsFunction,
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
    node_value_get: JsFunction,
    node_value_set: JsFunction,
    data_get: JsFunction,
    data_set: JsFunction,
    id_get: JsFunction,
    id_set: JsFunction,
    class_name_get: JsFunction,
    class_name_set: JsFunction,
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

/// Element wrappers report the global `document` object as their
/// ownerDocument; framework mount paths (react-dom's listener install walks
/// `container.ownerDocument`) read it before touching any other node API.
fn owner_document_native() -> NativeFunction {
    NativeFunction::from_fn_ptr(|_this, _args, ctx| {
        let global = ctx.global_object().clone();
        let document = global.get(js_string!("document"), ctx)?;
        if document.is_object() {
            Ok(document)
        } else {
            Ok(JsValue::null())
        }
    })
}

fn node_accessors(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId, ctx: &mut Context) -> NodeAccessors {
    NodeAccessors {
        owner_document: make_getter(ctx, owner_document_native()),
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
        inner_html_set: make_getter(ctx, inner_html_set_native(dom_arc, node_id)),
        value_get: make_getter(ctx, value_get_native(dom_arc, node_id)),
        value_set: make_getter(ctx, value_set_native(dom_arc, node_id)),
        src_get: make_getter(ctx, attribute_get_native(dom_arc, node_id, "src")),
        src_set: make_getter(ctx, attribute_set_native(dom_arc, node_id, "src")),
        node_value_get: make_getter(ctx, node_value_get_native(dom_arc, node_id)),
        node_value_set: make_getter(ctx, node_value_set_native(dom_arc, node_id)),
        data_get: make_getter(ctx, node_value_get_native(dom_arc, node_id)),
        data_set: make_getter(ctx, node_value_set_native(dom_arc, node_id)),
        id_get: make_getter(ctx, attribute_get_native(dom_arc, node_id, "id")),
        id_set: make_getter(ctx, attribute_set_native(dom_arc, node_id, "id")),
        class_name_get: make_getter(ctx, attribute_get_native(dom_arc, node_id, "class")),
        class_name_set: make_getter(ctx, attribute_set_native(dom_arc, node_id, "class")),
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
        dispatch_event: node_dispatch_event_native(dom_arc, node_id),
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
            let child_arg = args.first();
            let child = extract_node_id(child_arg, ctx)?;
            {
                let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                detach_from_parent(&mut dom, child);
                let _ = dom.append_child(node_id, child);
            }
            Ok(child_arg.cloned().unwrap_or(JsValue::undefined()))
        })
    }
}

fn remove_child_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let child_arg = args.first();
            let child = extract_node_id(child_arg, ctx)?;
            {
                let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = dom.remove_child(node_id, child);
            }
            Ok(child_arg.cloned().unwrap_or(JsValue::undefined()))
        })
    }
}

fn insert_before_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let child_arg = args.first();
            let child = extract_node_id(child_arg, ctx)?;
            let reference = extract_optional_node_id(args.get(1), ctx)?;
            insert_before_or_append(&arc, node_id, child, reference);
            Ok(child_arg.cloned().unwrap_or(JsValue::undefined()))
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
            let old_child_arg = args.get(1);
            let old_child = extract_node_id(old_child_arg, ctx)?;
            {
                let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = dom.insert_before(node_id, child, old_child);
                let _ = dom.remove_child(node_id, old_child);
            }
            Ok(old_child_arg.cloned().unwrap_or(JsValue::undefined()))
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

/// `nodeValue`/`data` getter. React reads the committed text back through
/// `node.data`; the wrapper reports the node's current character data rather
/// than a snapshot frozen at first access.
fn node_value_get_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            Ok(JsValue::from(JsString::from(
                own_character_data(&dom, node_id).as_str(),
            )))
        })
    }
}

/// `nodeValue`/`data` setter. React commits a text update by assigning the text
/// node `nodeValue`/`data`; `set_text_content` rewrites the Text node in place
/// and marks it dirty, so the paint tree observes the new text. The write is
/// gated to Text nodes: `nodeValue` assignment is ignored on elements per the
/// DOM spec, and `set_text_content` would otherwise replace an element's whole
/// subtree with a single text node.
fn node_value_set_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
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
            let is_text = dom
                .node(node_id)
                .map(|n| matches!(n.kind(), NodeKind::Text { .. }))
                .unwrap_or(false);
            if is_text {
                let _ = dom.set_text_content(node_id, text);
            }
            Ok(JsValue::undefined())
        })
    }
}

/// innerHTML setter: clear existing children, fragment-parse the markup in
/// this element's context, splice the result. Scripts in the fragment stay
/// inert (fragment parsing semantics). The whole operation runs under one
/// Dom lock acquisition; no JS executes while it is held.
fn inner_html_set_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let html = args
                .first()
                .map(|value| value.to_string(ctx).map(|s| s.to_std_string_lossy()))
                .transpose()?
                .unwrap_or_default();
            let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            let context_tag = dom
                .element_name(node_id)
                .ok()
                .flatten()
                .map_or_else(|| "div".to_string(), std::string::ToString::to_string);
            let existing: Vec<NodeId> = dom
                .children(node_id)
                .map(<[NodeId]>::to_vec)
                .unwrap_or_default();
            for child in existing {
                let _ = dom.remove_child(node_id, child);
            }
            silksurf_html::parse_fragment_into(&mut dom, node_id, &context_tag, &html);
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
            add_event_listener(node_id, args.first(), args.get(1), args.get(2), ctx)
        })
    }
}

fn node_remove_event_listener_native(node_id: NodeId) -> NativeFunction {
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            remove_event_listener(node_id, args.first(), args.get(1), args.get(2), ctx)
        })
    }
}

fn node_dispatch_event_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: Boa stores the native closure with owned DOM handles for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |this, args, ctx| {
            dispatch_event(this, &arc, node_id, args.first(), ctx)
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
pub(super) fn install_document(
    dom_arc: &Arc<Mutex<Dom>>,
    ctx: &mut Context,
    cookie_jar: &super::CookieJar,
    cookie_top_level_site: &str,
    cookie_host: &str,
) {
    let root = NodeId::from_raw(0);
    super::css_object::install_style_dataset_natives(dom_arc, ctx);
    // DOM interface constructors: frameworks probe them with instanceof and
    // typeof (react-dom evaluates `node instanceof win.HTMLIFrameElement`
    // during selection restore). Bridge wrappers are plain objects, so
    // instanceof correctly reports false; the constructors exist so the
    // expression evaluates instead of throwing on undefined.
    let interface_bootstrap = r"
        (function () {
            var names = ['Node', 'Element', 'Document', 'HTMLElement',
                'HTMLIFrameElement', 'HTMLInputElement', 'HTMLTextAreaElement',
                'HTMLSelectElement', 'HTMLAnchorElement', 'CharacterData',
                'Text', 'Comment', 'DocumentFragment', 'SVGElement'];
            for (var i = 0; i < names.length; i++) {
                if (typeof globalThis[names[i]] === 'undefined') {
                    globalThis[names[i]] = function () {};
                }
            }
        })();
    ";
    if let Err(err) = ctx.eval(boa_engine::Source::from_bytes(
        interface_bootstrap.as_bytes(),
    )) {
        eprintln!("silksurf-js: DOM interface bootstrap failed: {err}");
    }
    let methods = document_methods(dom_arc, root);
    let accessors = document_accessors(dom_arc, root, ctx);
    let cookie_getter =
        super::document_cookie_getter(ctx, cookie_jar, cookie_top_level_site, cookie_host);
    let cookie_setter =
        super::document_cookie_setter(ctx, cookie_jar, cookie_top_level_site, cookie_host);

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
        dispatch_event: node_dispatch_event_native(dom_arc, root),
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

// ---- canvas 2D context -----------------------------------------------------

/// Read a JS argument as an f32, defaulting to 0.0 (canvas coerces missing or
/// non-finite coordinates toward 0).
fn arg_f32(args: &[JsValue], index: usize, ctx: &mut Context) -> f32 {
    args.get(index)
        .and_then(|value| value.to_number(ctx).ok())
        .map_or(0.0, |n| n as f32)
}

/// Read the canvas element's `width`/`height` content attributes, defaulting to
/// the HTML canvas intrinsic size (300 x 150) when absent or unparseable.
fn canvas_dimensions(dom: &Dom, node_id: NodeId) -> (u32, u32) {
    let mut width = 300u32;
    let mut height = 150u32;
    if let Ok(attrs) = dom.attributes(node_id) {
        for attr in attrs {
            match attr.name.as_str() {
                "width" => {
                    if let Ok(value) = attr.value.to_string().trim().parse::<u32>() {
                        width = value.max(1);
                    }
                }
                "height" => {
                    if let Ok(value) = attr.value.to_string().trim().parse::<u32>() {
                        height = value.max(1);
                    }
                }
                _ => {}
            }
        }
    }
    (width, height)
}

/// `element.getContext(type)` -- returns a 2D context for canvas elements
/// requesting "2d", else null (unsupported context types and non-canvas
/// elements both yield null, matching the HTML spec).
fn getcontext_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: the closure captures an Arc<Mutex<Dom>> and a NodeId, neither of
    // which is a Boa GC pointer, so it needs no trace hook.
    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let context_type = args
                .first()
                .map(|value| value.to_string(ctx).map(|s| s.to_std_string_lossy()))
                .transpose()?
                .unwrap_or_default();
            if context_type != "2d" {
                return Ok(JsValue::null());
            }
            {
                let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                let is_canvas = matches!(
                    dom.element_name(node_id),
                    Ok(Some(name)) if name.eq_ignore_ascii_case("canvas")
                );
                if !is_canvas {
                    return Ok(JsValue::null());
                }
                let (width, height) = canvas_dimensions(&dom, node_id);
                dom.ensure_canvas_surface(node_id, width, height);
            }
            Ok(canvas_context_object(&arc, node_id, ctx))
        })
    }
}

/// A geometric context op taking N pre-parsed f32 arguments and mutating the
/// backing surface. Covers fillRect, moveTo, translate, transform, and the
/// zero-argument state ops (fill, stroke, save, restore, beginPath...).
fn canvas_geom_native<const N: usize>(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    op: fn(&mut silksurf_dom::CanvasSurface, [f32; N]),
) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: captures Arc<Mutex<Dom>>, NodeId, and a fn pointer -- no GC state.
    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let mut nums = [0.0f32; N];
            for (index, slot) in nums.iter_mut().enumerate() {
                *slot = arg_f32(args, index, ctx);
            }
            let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(surface) = dom.canvas_surface_mut(node_id) {
                op(surface, nums);
            }
            Ok(JsValue::undefined())
        })
    }
}

/// A context op that sets a `[u8; 4]` color parsed from a CSS color string.
fn canvas_color_native(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    set: fn(&mut silksurf_dom::CanvasSurface, [u8; 4]),
) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: captures Arc<Mutex<Dom>>, NodeId, and a fn pointer -- no GC state.
    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let text = args
                .first()
                .map(|value| value.to_string(ctx).map(|s| s.to_std_string_lossy()))
                .transpose()?
                .unwrap_or_default();
            let rgba = parse_css_color(&text);
            let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(surface) = dom.canvas_surface_mut(node_id) {
                set(surface, rgba);
            }
            Ok(JsValue::undefined())
        })
    }
}

/// Getter native returning a color state value formatted as `rgba(r, g, b, a)`.
fn canvas_color_getter_native(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    get: fn(&silksurf_dom::CanvasSurface) -> [u8; 4],
) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: captures Arc<Mutex<Dom>>, NodeId, and a fn pointer -- no GC state.
    unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            let rgba = dom.canvas_surface(node_id).map_or([0, 0, 0, 255], get);
            let alpha = f32::from(rgba[3]) / 255.0;
            let text = format!("rgba({}, {}, {}, {})", rgba[0], rgba[1], rgba[2], alpha);
            Ok(JsValue::from(JsString::from(text.as_str())))
        })
    }
}

/// Setter native for a scalar f32 context property (lineWidth, globalAlpha).
fn canvas_scalar_setter_native(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    set: fn(&mut silksurf_dom::CanvasSurface, f32),
) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: captures Arc<Mutex<Dom>>, NodeId, and a fn pointer -- no GC state.
    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let value = arg_f32(args, 0, ctx);
            let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(surface) = dom.canvas_surface_mut(node_id) {
                set(surface, value);
            }
            Ok(JsValue::undefined())
        })
    }
}

/// Getter native returning a scalar f32 context property.
fn canvas_scalar_getter_native(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    get: fn(&silksurf_dom::CanvasSurface) -> f32,
) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: captures Arc<Mutex<Dom>>, NodeId, and a fn pointer -- no GC state.
    unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            let value = dom.canvas_surface(node_id).map_or(0.0, get);
            Ok(JsValue::from(f64::from(value)))
        })
    }
}

/// `ctx.arc(cx, cy, r, start, end, anticlockwise)`.
fn canvas_arc_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: captures Arc<Mutex<Dom>> and NodeId -- no GC state.
    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let cx = arg_f32(args, 0, ctx);
            let cy = arg_f32(args, 1, ctx);
            let radius = arg_f32(args, 2, ctx);
            let start = arg_f32(args, 3, ctx);
            let end = arg_f32(args, 4, ctx);
            let anticlockwise = args.get(5).is_some_and(boa_engine::JsValue::to_boolean);
            let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(surface) = dom.canvas_surface_mut(node_id) {
                surface.arc(cx, cy, radius, start, end, anticlockwise);
            }
            Ok(JsValue::undefined())
        })
    }
}

/// `ctx.getImageData(x, y, w, h)` -> `{ width, height, data: [u8; w*h*4] }`.
/// The data is a plain JS array (not a `Uint8ClampedArray`), which reads and
/// round-trips through `putImageData` correctly.
fn canvas_get_image_data_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: captures Arc<Mutex<Dom>> and NodeId -- no GC state.
    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let x = arg_f32(args, 0, ctx) as i32;
            let y = arg_f32(args, 1, ctx) as i32;
            let w = arg_f32(args, 2, ctx).max(0.0) as u32;
            let h = arg_f32(args, 3, ctx).max(0.0) as u32;
            let pixels = {
                let dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                dom.canvas_surface(node_id)
                    .map(|surface| surface.get_image_data(x, y, w, h))
                    .unwrap_or_default()
            };
            let pixel_array = JsArray::new(ctx);
            for byte in &pixels {
                pixel_array.push(JsValue::from(u32::from(*byte)), ctx)?;
            }
            let image_data = ObjectInitializer::new(ctx)
                .property(js_string!("width"), w, Attribute::all())
                .property(js_string!("height"), h, Attribute::all())
                .property(js_string!("data"), pixel_array, Attribute::all())
                .build();
            Ok(image_data.into())
        })
    }
}

/// `ctx.putImageData(imageData, dx, dy)`.
fn canvas_put_image_data_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: captures Arc<Mutex<Dom>> and NodeId -- no GC state.
    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let Some(image_data) = args.first().and_then(JsValue::as_object) else {
                return Ok(JsValue::undefined());
            };
            let width = image_data.get(js_string!("width"), ctx)?.to_number(ctx)? as u32;
            let height = image_data.get(js_string!("height"), ctx)?.to_number(ctx)? as u32;
            let data_value = image_data.get(js_string!("data"), ctx)?;
            let Some(data_object) = data_value.as_object() else {
                return Ok(JsValue::undefined());
            };
            let data_array = JsArray::from_object(data_object.clone())?;
            let length = data_array.length(ctx)? as usize;
            let mut bytes = Vec::with_capacity(length);
            for index in 0..length {
                let value = data_array.get(index as u64, ctx)?.to_number(ctx)?;
                bytes.push(value.clamp(0.0, 255.0) as u8);
            }
            let dx = arg_f32(args, 1, ctx) as i32;
            let dy = arg_f32(args, 2, ctx) as i32;
            let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(surface) = dom.canvas_surface_mut(node_id) {
                surface.put_image_data(&bytes, dx, dy, width, height);
            }
            Ok(JsValue::undefined())
        })
    }
}

/// `ctx.drawImage(source, ...)` where `source` is another canvas element. The
/// 3-, 5-, and 9-argument forms are supported. Non-canvas sources (e.g. `<img>`
/// whose pixels live in the URL-keyed resource cache, not the DOM) draw
/// nothing -- image sources are a follow-on once decoded pixels reach the DOM.
// Canvas dimensions are small integers; u32 -> f32 loses no meaningful
// precision when resolving the src/dst rectangles.
#[allow(clippy::cast_precision_loss)]
fn canvas_draw_image_native(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId) -> NativeFunction {
    let arc = Arc::clone(dom_arc);
    // SAFETY: captures Arc<Mutex<Dom>> and NodeId -- no GC state.
    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let Some(source) = args.first().and_then(JsValue::as_object) else {
                return Ok(JsValue::undefined());
            };
            let Ok(source_id) = extract_node_id(Some(&JsValue::from(source.clone())), ctx) else {
                return Ok(JsValue::undefined());
            };
            let numeric: Vec<f32> = (1..args.len()).map(|i| arg_f32(args, i, ctx)).collect();
            let (src_pixels, src_w, src_h) = {
                let dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
                match dom.canvas_surface(source_id) {
                    Some(surface) => (surface.pixels().to_vec(), surface.width(), surface.height()),
                    None => return Ok(JsValue::undefined()),
                }
            };
            // Resolve the argument form into src/dst rectangles.
            let (sx, sy, sw, sh, dx, dy, dw, dh) = match numeric.len() {
                2 => (
                    0.0,
                    0.0,
                    src_w as f32,
                    src_h as f32,
                    numeric[0],
                    numeric[1],
                    src_w as f32,
                    src_h as f32,
                ),
                4 => (
                    0.0,
                    0.0,
                    src_w as f32,
                    src_h as f32,
                    numeric[0],
                    numeric[1],
                    numeric[2],
                    numeric[3],
                ),
                8 => (
                    numeric[0], numeric[1], numeric[2], numeric[3], numeric[4], numeric[5],
                    numeric[6], numeric[7],
                ),
                _ => return Ok(JsValue::undefined()),
            };
            let mut dom = arc.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(surface) = dom.canvas_surface_mut(node_id) {
                surface.draw_image(&src_pixels, src_w, src_h, sx, sy, sw, sh, dx, dy, dw, dh);
            }
            Ok(JsValue::undefined())
        })
    }
}

/// Build the `CanvasRenderingContext2D` JS object bound to `node_id`'s surface.
fn canvas_context_object(dom_arc: &Arc<Mutex<Dom>>, node_id: NodeId, ctx: &mut Context) -> JsValue {
    use silksurf_dom::CanvasSurface;

    let (canvas_width, canvas_height) = {
        let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
        dom.canvas_surface(node_id)
            .map_or((300, 150), |surface| (surface.width(), surface.height()))
    };
    let canvas_ref = ObjectInitializer::new(ctx)
        .property(js_string!("width"), canvas_width, Attribute::all())
        .property(js_string!("height"), canvas_height, Attribute::all())
        .property(js_string!("nodeId"), node_id.raw() as u32, Attribute::all())
        .build();

    let fill_get = make_getter(
        ctx,
        canvas_color_getter_native(dom_arc, node_id, CanvasSurface::fill_style),
    );
    let fill_set = make_getter(
        ctx,
        canvas_color_native(dom_arc, node_id, CanvasSurface::set_fill_style),
    );
    let stroke_get = make_getter(
        ctx,
        canvas_color_getter_native(dom_arc, node_id, CanvasSurface::stroke_style),
    );
    let stroke_set = make_getter(
        ctx,
        canvas_color_native(dom_arc, node_id, CanvasSurface::set_stroke_style),
    );
    let line_get = make_getter(
        ctx,
        canvas_scalar_getter_native(dom_arc, node_id, CanvasSurface::line_width),
    );
    let line_set = make_getter(
        ctx,
        canvas_scalar_setter_native(dom_arc, node_id, CanvasSurface::set_line_width),
    );
    let alpha_get = make_getter(
        ctx,
        canvas_scalar_getter_native(dom_arc, node_id, CanvasSurface::global_alpha),
    );
    let alpha_set = make_getter(
        ctx,
        canvas_scalar_setter_native(dom_arc, node_id, CanvasSurface::set_global_alpha),
    );

    ObjectInitializer::new(ctx)
        .property(js_string!("canvas"), canvas_ref, Attribute::all())
        .accessor(
            js_string!("fillStyle"),
            Some(fill_get),
            Some(fill_set),
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("strokeStyle"),
            Some(stroke_get),
            Some(stroke_set),
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("lineWidth"),
            Some(line_get),
            Some(line_set),
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .accessor(
            js_string!("globalAlpha"),
            Some(alpha_get),
            Some(alpha_set),
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, [x, y, w, h]| s.fill_rect(x, y, w, h)),
            js_string!("fillRect"),
            4,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, [x, y, w, h]| s.clear_rect(x, y, w, h)),
            js_string!("clearRect"),
            4,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, [x, y, w, h]| {
                s.stroke_rect(x, y, w, h);
            }),
            js_string!("strokeRect"),
            4,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, []| s.begin_path()),
            js_string!("beginPath"),
            0,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, [x, y]| s.move_to(x, y)),
            js_string!("moveTo"),
            2,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, [x, y]| s.line_to(x, y)),
            js_string!("lineTo"),
            2,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, [x, y, w, h]| s.rect(x, y, w, h)),
            js_string!("rect"),
            4,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, [cx, cy, x, y]| {
                s.quadratic_curve_to(cx, cy, x, y);
            }),
            js_string!("quadraticCurveTo"),
            4,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, [a, b, c, d, x, y]| {
                s.bezier_curve_to(a, b, c, d, x, y);
            }),
            js_string!("bezierCurveTo"),
            6,
        )
        .function(canvas_arc_native(dom_arc, node_id), js_string!("arc"), 5)
        .function(
            canvas_geom_native(dom_arc, node_id, |s, []| s.close_path()),
            js_string!("closePath"),
            0,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, []| s.fill()),
            js_string!("fill"),
            0,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, []| s.stroke()),
            js_string!("stroke"),
            0,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, []| s.save()),
            js_string!("save"),
            0,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, []| s.restore()),
            js_string!("restore"),
            0,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, [x, y]| s.translate(x, y)),
            js_string!("translate"),
            2,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, [x, y]| s.scale(x, y)),
            js_string!("scale"),
            2,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, [angle]| s.rotate(angle)),
            js_string!("rotate"),
            1,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, [a, b, c, d, e, f]| {
                s.transform(a, b, c, d, e, f);
            }),
            js_string!("transform"),
            6,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, [a, b, c, d, e, f]| {
                s.set_transform(a, b, c, d, e, f);
            }),
            js_string!("setTransform"),
            6,
        )
        .function(
            canvas_geom_native(dom_arc, node_id, |s, []| s.reset_transform()),
            js_string!("resetTransform"),
            0,
        )
        .function(
            canvas_get_image_data_native(dom_arc, node_id),
            js_string!("getImageData"),
            4,
        )
        .function(
            canvas_put_image_data_native(dom_arc, node_id),
            js_string!("putImageData"),
            3,
        )
        .function(
            canvas_draw_image_native(dom_arc, node_id),
            js_string!("drawImage"),
            3,
        )
        .build()
        .into()
}

/// Parse a CSS color string into straight-alpha RGBA bytes. Supports `#rgb`,
/// `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`/`rgba()`, and a small set of named colors;
/// unrecognized input falls back to opaque black (canvas ignores invalid
/// assignments, but a substrate that draws *something* is more useful here).
// Named colors are listed individually for readability even where two share
// a byte value (e.g. black and the fallback).
#[allow(clippy::match_same_arms)]
fn parse_css_color(input: &str) -> [u8; 4] {
    let text = input.trim();
    if let Some(hex) = text.strip_prefix('#') {
        return parse_hex_color(hex).unwrap_or([0, 0, 0, 255]);
    }
    if let Some(inner) = text
        .strip_prefix("rgba(")
        .or_else(|| text.strip_prefix("rgb("))
        .and_then(|rest| rest.strip_suffix(')'))
    {
        return parse_rgb_function(inner).unwrap_or([0, 0, 0, 255]);
    }
    match text.to_ascii_lowercase().as_str() {
        "transparent" => [0, 0, 0, 0],
        "black" => [0, 0, 0, 255],
        "white" => [255, 255, 255, 255],
        "red" => [255, 0, 0, 255],
        "green" => [0, 128, 0, 255],
        "lime" => [0, 255, 0, 255],
        "blue" => [0, 0, 255, 255],
        "yellow" => [255, 255, 0, 255],
        "cyan" | "aqua" => [0, 255, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255, 255],
        "gray" | "grey" => [128, 128, 128, 255],
        "orange" => [255, 165, 0, 255],
        _ => [0, 0, 0, 255],
    }
}

fn parse_hex_color(hex: &str) -> Option<[u8; 4]> {
    let expand = |c: char| {
        let d = c.to_digit(16)? as u8;
        Some(d << 4 | d)
    };
    match hex.len() {
        3 => {
            let mut chars = hex.chars();
            Some([
                expand(chars.next()?)?,
                expand(chars.next()?)?,
                expand(chars.next()?)?,
                255,
            ])
        }
        4 => {
            let mut chars = hex.chars();
            Some([
                expand(chars.next()?)?,
                expand(chars.next()?)?,
                expand(chars.next()?)?,
                expand(chars.next()?)?,
            ])
        }
        6 => Some([
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            255,
        ]),
        8 => Some([
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            u8::from_str_radix(&hex[6..8], 16).ok()?,
        ]),
        _ => None,
    }
}

fn parse_rgb_function(inner: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() < 3 {
        return None;
    }
    let channel =
        |text: &str| -> Option<u8> { Some(text.parse::<f32>().ok()?.clamp(0.0, 255.0) as u8) };
    let red = channel(parts[0])?;
    let green = channel(parts[1])?;
    let blue = channel(parts[2])?;
    let alpha = if parts.len() >= 4 {
        (parts[3].parse::<f32>().ok()?.clamp(0.0, 1.0) * 255.0).round() as u8
    } else {
        255
    };
    Some([red, green, blue, alpha])
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

    #[test]
    fn owner_document_resolves_to_the_global_document() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             if (el.ownerDocument !== document) { throw new Error('ownerDocument'); } \
             var fresh = document.createElement('div'); \
             if (fresh.ownerDocument !== document) { throw new Error('created node'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn dom_interface_constructors_support_instanceof_probes() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             if (el instanceof HTMLIFrameElement) { throw new Error('iframe'); } \
             if (typeof HTMLElement !== 'function') { throw new Error('HTMLElement'); } \
             if (typeof Node !== 'function') { throw new Error('Node'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn style_write_lands_in_style_attribute_and_marks_dirty() {
        let (arc, _root) = simple_dom();
        {
            let mut dom = arc.lock().unwrap();
            let _ = dom.take_dirty_nodes();
        }
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             el.style.backgroundColor = 'red'; \
             el.style.width = '5px'; \
             var attr = el.getAttribute('style'); \
             if (attr !== 'background-color: red; width: 5px') { throw new Error('attr: ' + attr); } \
             if (el.style.backgroundColor !== 'red') { throw new Error('readback failed'); } \
             if (el.style.getPropertyValue('width') !== '5px') { throw new Error('getPropertyValue'); } \
             el.style.removeProperty('width'); \
             if (el.getAttribute('style') !== 'background-color: red') { throw new Error('remove'); } \
             if (el.style.cssText !== 'background-color: red') { throw new Error('cssText read'); }",
        )
        .expect("eval should succeed");
        let mut dom = arc.lock().unwrap();
        assert!(!dom.take_dirty_nodes().is_empty(), "style write must dirty");
    }

    #[test]
    fn style_css_text_write_replaces_declarations() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             el.style.cssText = 'color: blue; margin: 4px'; \
             if (el.style.color !== 'blue') { throw new Error('color'); } \
             if (el.style.margin !== '4px') { throw new Error('margin'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn dataset_maps_camel_case_to_data_attributes() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             el.dataset.userKind = 'probe'; \
             if (el.getAttribute('data-user-kind') !== 'probe') { throw new Error('attr'); } \
             if (el.dataset.userKind !== 'probe') { throw new Error('readback'); } \
             if (el.dataset.missing !== undefined) { throw new Error('missing'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn inner_html_reparses_markup_into_live_children() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             el.innerHTML = '<b class=\"loud\">Hi</b><span>there</span>'; \
             var b = el.querySelector('b.loud'); \
             if (b === null) { throw new Error('fragment element missing'); } \
             if (b.textContent !== 'Hi') { throw new Error('fragment text: ' + b.textContent); } \
             if (el.children.length !== 2) { throw new Error('children: ' + el.children.length); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn inner_html_replaces_existing_children_and_marks_dirty() {
        let (arc, _root) = simple_dom();
        {
            let mut dom = arc.lock().unwrap();
            let _ = dom.take_dirty_nodes();
        }
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             el.innerHTML = '<i>only</i>'; \
             if (el.children.length !== 1) { throw new Error('old children remain'); } \
             if (el.textContent !== 'only') { throw new Error('text: ' + el.textContent); }",
        )
        .expect("eval should succeed");
        let mut dom = arc.lock().unwrap();
        assert!(!dom.take_dirty_nodes().is_empty());
    }

    #[test]
    fn query_selector_supports_descendant_and_attribute_selectors() {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let outer = dom.create_element("div");
        let _ = dom.set_attribute(outer, "class", "wrap");
        let inner = dom.create_element("span");
        let _ = dom.set_attribute(inner, "data-kind", "probe");
        let _ = dom.set_attribute(inner, "class", "a b");
        let _ = dom.append_child(root, outer);
        let _ = dom.append_child(outer, inner);
        dom.materialize_resolve_table();
        let arc = Arc::new(Mutex::new(dom));
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "if (document.querySelector('div.wrap > span.a') === null) { throw new Error('child combinator'); } \
             if (document.querySelector('div span[data-kind=probe]') === null) { throw new Error('attribute'); } \
             if (document.querySelector('div.wrap > em') !== null) { throw new Error('non-match matched'); } \
             if (document.querySelectorAll('.a.b').length !== 1) { throw new Error('compound class'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn element_matches_and_closest_walk_ancestors() {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let form = dom.create_element("form");
        let _ = dom.set_attribute(form, "id", "f");
        let field = dom.create_element("input");
        let _ = dom.set_attribute(field, "id", "field");
        let _ = dom.append_child(root, form);
        let _ = dom.append_child(form, field);
        dom.materialize_resolve_table();
        let arc = Arc::new(Mutex::new(dom));
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var field = document.getElementById('field'); \
             if (!field.matches('input')) { throw new Error('matches tag'); } \
             if (field.matches('div')) { throw new Error('matches wrong tag'); } \
             var form = field.closest('form'); \
             if (form === null || form.nodeId !== document.getElementById('f').nodeId) { \
               throw new Error('closest form'); \
             } \
             if (field.closest('table') !== null) { throw new Error('closest non-match'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn scoped_query_selector_excludes_context_node() {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let outer = dom.create_element("div");
        let _ = dom.set_attribute(outer, "id", "outer");
        let inner = dom.create_element("div");
        let _ = dom.set_attribute(inner, "id", "inner");
        let _ = dom.append_child(root, outer);
        let _ = dom.append_child(outer, inner);
        dom.materialize_resolve_table();
        let arc = Arc::new(Mutex::new(dom));
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var outer = document.getElementById('outer'); \
             var hits = outer.querySelectorAll('div'); \
             if (hits.length !== 1) { throw new Error('scoped count ' + hits.length); } \
             if (hits[0].nodeId !== document.getElementById('inner').nodeId) { \
               throw new Error('scoped target'); \
             }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn repeated_lookups_return_the_same_wrapper_object() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var a = document.getElementById('greeting'); \
             var b = document.getElementById('greeting'); \
             var c = document.querySelector('#greeting'); \
             if (a !== b) { throw new Error('getElementById identity'); } \
             if (a !== c) { throw new Error('querySelector identity'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn expando_properties_survive_across_lookups() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        // A framework stamps a hidden fiber pointer on the node object during
        // commit and reads it back at event dispatch; the wrapper must carry it
        // to the next lookup for delegation to resolve the handler.
        ctx.eval(
            "document.getElementById('greeting').__reactFiber = { tag: 5 }; \
             var again = document.getElementById('greeting'); \
             if (!again.__reactFiber || again.__reactFiber.tag !== 5) { \
               throw new Error('expando lost across lookup'); \
             }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn created_node_keeps_identity_after_insertion() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        // React stamps the fiber on the createElement result, appends it, then
        // reaches the node again through the tree at event dispatch.
        ctx.eval(
            "var parent = document.getElementById('greeting'); \
             var child = document.createElement('button'); \
             child.__reactFiber = 42; \
             parent.appendChild(child); \
             var seen = parent.children[parent.children.length - 1]; \
             if (seen !== child) { throw new Error('created-node identity'); } \
             if (seen.__reactFiber !== 42) { throw new Error('created-node expando'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn dispatch_target_shares_identity_with_lookup() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             el.__reactFiber = 'stamped'; \
             el.addEventListener('click', function (event) { \
               globalThis._targetMatches = event.target === el \
                 && event.target.__reactFiber === 'stamped'; \
             }); \
             globalThis._targetMatches = false;",
        )
        .expect("eval should succeed");
        let target = document_greeting(&arc.lock().unwrap());
        let event = crate::boa_backend::SyntheticEvent::new("click", true, true);
        ctx.dispatch_dom_event(target, &event)
            .expect("dispatch should succeed");
        ctx.eval(
            "if (!globalThis._targetMatches) { throw new Error('target identity at dispatch'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn node_value_write_updates_the_text_node() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        // React commits text by assigning the text node's nodeValue/data; the
        // write must reach the Dom, and a re-read must reflect it live.
        ctx.eval(
            "var text = document.getElementById('greeting').firstChild; \
             if (text.nodeValue !== 'Hello, world!') { throw new Error('initial nodeValue'); } \
             text.nodeValue = 'clicks:1'; \
             if (text.data !== 'clicks:1') { throw new Error('data getter stale'); } \
             if (document.getElementById('greeting').textContent !== 'clicks:1') { \
               throw new Error('Dom text not updated'); \
             }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn data_write_updates_the_text_node() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var text = document.getElementById('greeting').firstChild; \
             text.data = 'via-data'; \
             if (text.nodeValue !== 'via-data') { throw new Error('nodeValue getter stale'); } \
             if (document.getElementById('greeting').textContent !== 'via-data') { \
               throw new Error('Dom text not updated'); \
             }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn node_value_write_on_element_leaves_children_intact() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        // nodeValue assignment on an element is a DOM-spec no-op; the setter
        // must not fall through to set_text_content, which would replace the
        // element subtree with a single text node.
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             el.nodeValue = 'wiped'; \
             if (el.textContent !== 'Hello, world!') { throw new Error('element subtree clobbered'); } \
             if (el.firstChild === null) { throw new Error('text child destroyed'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn id_write_reaches_the_dom_and_reads_live() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        // The cached wrapper must reflect an id change, and getElementById must
        // resolve the new id (write-through to the id attribute).
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             el.id = 'renamed'; \
             if (el.id !== 'renamed') { throw new Error('id getter stale'); } \
             if (document.getElementById('renamed') === null) { throw new Error('id not in Dom'); } \
             if (document.getElementById('greeting') !== null) { throw new Error('old id lingers'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn class_name_write_reaches_the_dom_and_reads_live() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             el.className = 'promoted'; \
             if (el.className !== 'promoted') { throw new Error('className getter stale'); } \
             if (document.querySelector('.promoted') === null) { throw new Error('class not in Dom'); }",
        )
        .expect("eval should succeed");
    }

    #[test]
    fn cached_wrapper_reflects_attribute_change_via_setattribute() {
        let (arc, _root) = simple_dom();
        let mut ctx = SilkContext::with_dom(&arc);
        // The staleness regression the wrapper cache introduced: a cached
        // wrapper reads id/className live rather than freezing them.
        ctx.eval(
            "var el = document.getElementById('greeting'); \
             el.setAttribute('id', 'changed'); \
             el.setAttribute('class', 'newclass'); \
             if (el.id !== 'changed') { throw new Error('id read stale'); } \
             if (el.className !== 'newclass') { throw new Error('className read stale'); }",
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
