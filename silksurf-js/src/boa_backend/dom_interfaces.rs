/*
 * dom_interfaces gives node wrappers a real prototype chain and the members
 * that hang off it.
 *
 * A wrapper carries its NodeId as the own property `nodeId`, so a shared
 * prototype method reaches the Dom through `this.nodeId` and one node-id-keyed
 * native. That is what makes a prototype chain worth having here: the members
 * below cost one function object per interface rather than one per node, and
 * `node instanceof HTMLElement` answers from the chain instead of reporting
 * false.
 *
 * The chain follows the DOM and HTML standards:
 *
 *   EventTarget <- Node <- Element <- HTMLElement <- HTMLAnchorElement, ...
 *   EventTarget <- Node <- CharacterData <- Text, Comment
 *   EventTarget <- Node <- Document
 *   EventTarget <- Node <- DocumentFragment
 *
 * `interface_prototype` maps a node to its interface prototype, and
 * `node_to_js_object` installs it on every wrapper it builds.
 */

use std::sync::{Arc, Mutex, PoisonError};

use boa_engine::{
    Context, JsObject, JsResult, JsValue, NativeFunction, Source, js_string,
    object::builtins::JsArray,
};
use silksurf_dom::{Dom, NodeId, NodeKind};

/// Hidden global holding `{ interfaceName: prototypeObject }`.
const INTERFACE_PROTOTYPES: &str = "__silksurfInterfacePrototypes";

/// Install the interface constructors, their prototype chain, and the
/// node-id-keyed natives the prototype methods call.
pub(super) fn install_dom_interfaces(dom_arc: &Arc<Mutex<Dom>>, ctx: &mut Context) {
    install_node_natives(dom_arc, ctx);
    if let Err(err) = ctx.eval(Source::from_bytes(INTERFACE_BOOTSTRAP.as_bytes())) {
        eprintln!("silksurf-js: DOM interface bootstrap failed: {err}");
    }
    // The reflection table reads __silksurfInterfacePrototypes, which the
    // interface bootstrap records, so it runs second.
    if let Err(err) = ctx.eval(Source::from_bytes(REFLECTION_BOOTSTRAP.as_bytes())) {
        eprintln!("silksurf-js: IDL reflection bootstrap failed: {err}");
    }
}

/// The prototype a wrapper for `node_id` inherits from, or `None` when the
/// bootstrap has not run in this context.
pub(super) fn interface_prototype(
    dom_arc: &Arc<Mutex<Dom>>,
    node_id: NodeId,
    ctx: &mut Context,
) -> Option<JsObject> {
    let name = {
        let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
        interface_name(&dom, node_id)
    };
    prototype_named(name, ctx)
}

/// The prototype every node collection carries, so `list.item(i)` resolves
/// beside array indexing and iteration.
pub(super) fn node_list_prototype(ctx: &mut Context) -> Option<JsObject> {
    prototype_named("NodeList", ctx)
}

fn prototype_named(name: &str, ctx: &mut Context) -> Option<JsObject> {
    let global = ctx.global_object().clone();
    let table = global.get(js_string!(INTERFACE_PROTOTYPES), ctx).ok()?;
    let table = table.as_object()?;
    let prototype = table.get(boa_engine::JsString::from(name), ctx).ok()?;
    prototype.as_object()
}

/// The IDL interface a node implements. Element names come from the HTML
/// element-interface table; an element the table omits is an `HTMLElement`, and a
/// namespaced element outside HTML is an Element.
fn interface_name(dom: &Dom, node_id: NodeId) -> &'static str {
    let Ok(node) = dom.node(node_id) else {
        return "Node";
    };
    match node.kind() {
        NodeKind::Text { .. } => return "Text",
        NodeKind::Comment { .. } => return "Comment",
        NodeKind::Document => return "Document",
        NodeKind::Element { .. } => {}
        NodeKind::Doctype { .. } => return "Node",
    }
    let Ok(Some(name)) = dom.element_name(node_id) else {
        return "HTMLElement";
    };
    html_element_interface(&name.to_ascii_lowercase())
}

fn html_element_interface(tag: &str) -> &'static str {
    match tag {
        "a" => "HTMLAnchorElement",
        "area" => "HTMLAreaElement",
        "audio" => "HTMLAudioElement",
        "br" => "HTMLBRElement",
        "base" => "HTMLBaseElement",
        "body" => "HTMLBodyElement",
        "button" => "HTMLButtonElement",
        "canvas" => "HTMLCanvasElement",
        "dl" => "HTMLDListElement",
        "data" => "HTMLDataElement",
        "datalist" => "HTMLDataListElement",
        "dialog" => "HTMLDialogElement",
        "div" => "HTMLDivElement",
        "embed" => "HTMLEmbedElement",
        "fieldset" => "HTMLFieldSetElement",
        "form" => "HTMLFormElement",
        "hr" => "HTMLHRElement",
        "head" => "HTMLHeadElement",
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "HTMLHeadingElement",
        "html" => "HTMLHtmlElement",
        "iframe" => "HTMLIFrameElement",
        "img" => "HTMLImageElement",
        "input" => "HTMLInputElement",
        "li" => "HTMLLIElement",
        "label" => "HTMLLabelElement",
        "legend" => "HTMLLegendElement",
        "link" => "HTMLLinkElement",
        "map" => "HTMLMapElement",
        "menu" => "HTMLMenuElement",
        "meta" => "HTMLMetaElement",
        "meter" => "HTMLMeterElement",
        "ol" => "HTMLOListElement",
        "object" => "HTMLObjectElement",
        "optgroup" => "HTMLOptGroupElement",
        "option" => "HTMLOptionElement",
        "output" => "HTMLOutputElement",
        "p" => "HTMLParagraphElement",
        "picture" => "HTMLPictureElement",
        "pre" => "HTMLPreElement",
        "progress" => "HTMLProgressElement",
        "blockquote" | "q" => "HTMLQuoteElement",
        "script" => "HTMLScriptElement",
        "select" => "HTMLSelectElement",
        "slot" => "HTMLSlotElement",
        "source" => "HTMLSourceElement",
        "span" => "HTMLSpanElement",
        "style" => "HTMLStyleElement",
        "caption" => "HTMLTableCaptionElement",
        "td" | "th" => "HTMLTableCellElement",
        "col" | "colgroup" => "HTMLTableColElement",
        "table" => "HTMLTableElement",
        "tr" => "HTMLTableRowElement",
        "tbody" | "tfoot" | "thead" => "HTMLTableSectionElement",
        "template" => "HTMLTemplateElement",
        "textarea" => "HTMLTextAreaElement",
        "time" => "HTMLTimeElement",
        "title" => "HTMLTitleElement",
        "track" => "HTMLTrackElement",
        "ul" => "HTMLUListElement",
        "video" => "HTMLVideoElement",
        "svg" | "path" | "circle" | "rect" | "g" | "defs" | "use" => "SVGElement",
        _ => "HTMLElement",
    }
}

// ---- node-id-keyed natives ---------------------------------------------------

fn node_id_arg(args: &[JsValue], index: usize, ctx: &mut Context) -> JsResult<Option<NodeId>> {
    let Some(value) = args.get(index) else {
        return Ok(None);
    };
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    let raw = value.to_number(ctx)?;
    if !raw.is_finite() || raw < 0.0 {
        return Ok(None);
    }
    Ok(Some(NodeId::from_raw(raw as usize)))
}

fn string_arg(args: &[JsValue], index: usize, ctx: &mut Context) -> JsResult<String> {
    match args.get(index) {
        Some(value) => Ok(value.to_string(ctx)?.to_std_string_lossy()),
        None => Ok(String::new()),
    }
}

macro_rules! dom_native {
    ($ctx:expr, $dom:expr, $name:literal, $arity:expr, $body:expr) => {{
        let arc = Arc::clone($dom);
        // SAFETY: the closure owns an Arc clone and holds no GC pointers, so
        // boa may store it for the function's lifetime.
        let native =
            unsafe { NativeFunction::from_closure(move |_this, args, ctx| $body(&arc, args, ctx)) };
        let _ = $ctx.register_global_callable(js_string!($name), $arity, native);
    }};
}

fn has_attribute(
    dom_arc: &Arc<Mutex<Dom>>,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(node_id) = node_id_arg(args, 0, ctx)? else {
        return Ok(JsValue::from(false));
    };
    let name = string_arg(args, 1, ctx)?;
    let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    let Ok(attributes) = dom.attributes(node_id) else {
        return Ok(JsValue::from(false));
    };
    Ok(JsValue::from(
        attributes.iter().any(|a| a.name.matches(&name)),
    ))
}

fn attribute_names(
    dom_arc: &Arc<Mutex<Dom>>,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(node_id) = node_id_arg(args, 0, ctx)? else {
        return Ok(JsArray::new(ctx).into());
    };
    let names: Vec<String> = {
        let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
        match dom.attributes(node_id) {
            Ok(attributes) => attributes
                .iter()
                .map(|a| a.name.as_str().to_string())
                .collect(),
            Err(_) => Vec::new(),
        }
    };
    let array = JsArray::new(ctx);
    for name in names {
        array.push(js_string!(name.as_str()), ctx)?;
    }
    Ok(array.into())
}

fn remove_attribute(
    dom_arc: &Arc<Mutex<Dom>>,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(node_id) = node_id_arg(args, 0, ctx)? else {
        return Ok(JsValue::undefined());
    };
    let name = string_arg(args, 1, ctx)?;
    let mut dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = dom.remove_attribute(node_id, &name);
    Ok(JsValue::undefined())
}

/// Read one content attribute by node id, or null when it is absent. IDL
/// attribute reflection reads through this rather than through a per-node
/// accessor, so the getter lives once on the interface prototype.
fn get_attribute_by_id(
    dom_arc: &Arc<Mutex<Dom>>,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(node_id) = node_id_arg(args, 0, ctx)? else {
        return Ok(JsValue::null());
    };
    let name = string_arg(args, 1, ctx)?;
    let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    let Ok(attributes) = dom.attributes(node_id) else {
        return Ok(JsValue::null());
    };
    Ok(attributes
        .iter()
        .find(|a| a.name.matches(&name))
        .map_or_else(JsValue::null, |a| {
            JsValue::from(js_string!(a.value.as_str()))
        }))
}

/// Write one content attribute by node id, which marks the node dirty and
/// advances `Dom::style_generation` so the cascade and the stylesheet set both
/// observe the change.
fn set_attribute_by_id(
    dom_arc: &Arc<Mutex<Dom>>,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(node_id) = node_id_arg(args, 0, ctx)? else {
        return Ok(JsValue::undefined());
    };
    let name = string_arg(args, 1, ctx)?;
    let value = string_arg(args, 2, ctx)?;
    let mut dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = dom.set_attribute(node_id, name, value);
    Ok(JsValue::undefined())
}

/// True when `other` is `node` or a descendant of it, matching Node.contains.
fn contains_node(
    dom_arc: &Arc<Mutex<Dom>>,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let (Some(node_id), Some(other_id)) = (node_id_arg(args, 0, ctx)?, node_id_arg(args, 1, ctx)?)
    else {
        return Ok(JsValue::from(false));
    };
    let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    let mut current = other_id;
    loop {
        if current == node_id {
            return Ok(JsValue::from(true));
        }
        match dom.parent(current) {
            Ok(Some(parent)) => current = parent,
            _ => return Ok(JsValue::from(false)),
        }
    }
}

/// True when the node's root is the document node, matching Node.isConnected.
fn is_connected(
    dom_arc: &Arc<Mutex<Dom>>,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(node_id) = node_id_arg(args, 0, ctx)? else {
        return Ok(JsValue::from(false));
    };
    let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    let mut current = node_id;
    while let Ok(Some(parent)) = dom.parent(current) {
        current = parent;
    }
    Ok(JsValue::from(matches!(
        dom.node(current).map(silksurf_dom::Node::kind),
        Ok(NodeKind::Document)
    )))
}

fn local_name(dom_arc: &Arc<Mutex<Dom>>, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(node_id) = node_id_arg(args, 0, ctx)? else {
        return Ok(JsValue::from(js_string!("")));
    };
    let dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    let name = dom
        .element_name(node_id)
        .ok()
        .flatten()
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    Ok(js_string!(name.as_str()).into())
}

/// Copy a node, and its subtree when `deep`, returning the new node's id.
fn clone_node(dom_arc: &Arc<Mutex<Dom>>, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(node_id) = node_id_arg(args, 0, ctx)? else {
        return Ok(JsValue::null());
    };
    let deep = args.get(1).is_some_and(JsValue::to_boolean);
    let mut dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(copy) = clone_subtree(&mut dom, node_id, deep) else {
        return Ok(JsValue::null());
    };
    Ok(JsValue::from(copy.raw() as u32))
}

fn clone_subtree(dom: &mut Dom, node_id: NodeId, deep: bool) -> Option<NodeId> {
    let (copy, children) = {
        let node = dom.node(node_id).ok()?;
        let copy = match node.kind() {
            NodeKind::Text { text, .. } => dom.create_text(text.clone()),
            NodeKind::Comment { data: comment, .. } => dom.create_comment(comment.clone()),
            NodeKind::Element { .. } => {
                let name = dom.element_name(node_id).ok().flatten()?.to_string();
                dom.create_element(name)
            }
            NodeKind::Document | NodeKind::Doctype { .. } => return None,
        };
        let children = dom
            .children(node_id)
            .map(<[NodeId]>::to_vec)
            .unwrap_or_default();
        (copy, children)
    };
    let attributes: Vec<(String, String)> = dom
        .attributes(node_id)
        .map(|list| {
            list.iter()
                .map(|a| (a.name.as_str().to_string(), a.value.to_string()))
                .collect()
        })
        .unwrap_or_default();
    for (name, value) in attributes {
        let _ = dom.set_attribute(copy, &name, &value);
    }
    if deep {
        for child in children {
            if let Some(child_copy) = clone_subtree(dom, child, true) {
                let _ = dom.append_child(copy, child_copy);
            }
        }
    }
    Some(copy)
}

/// Create a detached element, text, or comment node and return its id. The
/// bootstrap wraps the id with `__silksurfWrapNode`.
fn create_detached(
    dom_arc: &Arc<Mutex<Dom>>,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let kind = string_arg(args, 0, ctx)?;
    let contents = string_arg(args, 1, ctx)?;
    let mut dom = dom_arc.lock().unwrap_or_else(PoisonError::into_inner);
    let created = match kind.as_str() {
        "comment" => dom.create_comment(contents),
        "text" => dom.create_text(contents),
        "fragment" => dom.create_element("#document-fragment"),
        _ => return Ok(JsValue::null()),
    };
    Ok(JsValue::from(created.raw() as u32))
}

/*
 * set_current_script -- point `document.currentScript` at the running script.
 *
 * HTML defines currentScript as the `<script>` element whose evaluation is in
 * progress, and a page reads its own tag through it: chatgpt.com's stylesheet
 * swap takes `document.currentScript.previousElementSibling` to find the
 * `<link>` it upgrades. The embedder sets the node before each classic script
 * evaluates and clears it after, which is the null the property reports at
 * every other moment.
 */
pub(super) fn set_current_script(
    dom_arc: &Arc<Mutex<Dom>>,
    node: Option<NodeId>,
    ctx: &mut Context,
) {
    let value = node.map_or_else(JsValue::null, |id| {
        super::dom_bridge::node_to_js_object(dom_arc, id, ctx)
    });
    let Some(document) = ctx
        .global_object()
        .get(js_string!("document"), ctx)
        .ok()
        .and_then(|document| document.as_object())
    else {
        return;
    };
    let _ = document.set(js_string!("currentScript"), value, false, ctx);
}

fn install_node_natives(dom_arc: &Arc<Mutex<Dom>>, ctx: &mut Context) {
    dom_native!(ctx, dom_arc, "__silksurfNodeHasAttribute", 2, has_attribute);
    dom_native!(
        ctx,
        dom_arc,
        "__silksurfNodeAttributeNames",
        1,
        attribute_names
    );
    dom_native!(
        ctx,
        dom_arc,
        "__silksurfNodeRemoveAttribute",
        2,
        remove_attribute
    );
    dom_native!(
        ctx,
        dom_arc,
        "__silksurfNodeGetAttribute",
        2,
        get_attribute_by_id
    );
    dom_native!(
        ctx,
        dom_arc,
        "__silksurfNodeSetAttribute",
        3,
        set_attribute_by_id
    );
    dom_native!(ctx, dom_arc, "__silksurfNodeContains", 2, contains_node);
    dom_native!(ctx, dom_arc, "__silksurfNodeIsConnected", 1, is_connected);
    dom_native!(ctx, dom_arc, "__silksurfNodeLocalName", 1, local_name);
    dom_native!(ctx, dom_arc, "__silksurfNodeClone", 2, clone_node);
    dom_native!(ctx, dom_arc, "__silksurfCreateDetached", 2, create_detached);

    let arc = Arc::clone(dom_arc);
    // SAFETY: the closure owns an Arc clone and holds no GC pointers.
    let wrap = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let Some(node_id) = node_id_arg(args, 0, ctx)? else {
                return Ok(JsValue::null());
            };
            Ok(super::dom_bridge::node_to_js_object(&arc, node_id, ctx))
        })
    };
    let _ = ctx.register_global_callable(js_string!("__silksurfWrapNode"), 1, wrap);
}

// ---- bootstrap --------------------------------------------------------------

/*
 * The bootstrap builds the constructor chain, records each prototype in
 * __silksurfInterfacePrototypes for interface_prototype to read, and defines
 * the members that a NodeId plus one native fully determines.
 *
 * DOMTokenList is a live view: classList reads and writes `className` on each
 * operation rather than caching tokens, so a direct className assignment stays
 * visible.
 */
const INTERFACE_BOOTSTRAP: &str = r"
(function () {
    'use strict';

    var chain = {
        EventTarget: null,
        Node: 'EventTarget',
        Element: 'Node',
        CharacterData: 'Node',
        Document: 'Node',
        DocumentFragment: 'Node',
        Text: 'CharacterData',
        Comment: 'CharacterData',
        HTMLElement: 'Element',
        SVGElement: 'Element'
    };
    var htmlInterfaces = ['HTMLAnchorElement', 'HTMLAreaElement', 'HTMLAudioElement',
        'HTMLBRElement', 'HTMLBaseElement', 'HTMLBodyElement', 'HTMLButtonElement',
        'HTMLCanvasElement', 'HTMLDListElement', 'HTMLDataElement', 'HTMLDataListElement',
        'HTMLDialogElement', 'HTMLDivElement', 'HTMLEmbedElement', 'HTMLFieldSetElement',
        'HTMLFormElement', 'HTMLHRElement', 'HTMLHeadElement', 'HTMLHeadingElement',
        'HTMLHtmlElement', 'HTMLIFrameElement', 'HTMLImageElement', 'HTMLInputElement',
        'HTMLLIElement', 'HTMLLabelElement', 'HTMLLegendElement', 'HTMLLinkElement',
        'HTMLMapElement', 'HTMLMediaElement', 'HTMLMenuElement', 'HTMLMetaElement',
        'HTMLMeterElement', 'HTMLOListElement', 'HTMLObjectElement', 'HTMLOptGroupElement',
        'HTMLOptionElement', 'HTMLOutputElement', 'HTMLParagraphElement', 'HTMLPictureElement',
        'HTMLPreElement', 'HTMLProgressElement', 'HTMLQuoteElement', 'HTMLScriptElement',
        'HTMLSelectElement', 'HTMLSlotElement', 'HTMLSourceElement', 'HTMLSpanElement',
        'HTMLStyleElement', 'HTMLTableCaptionElement', 'HTMLTableCellElement',
        'HTMLTableColElement', 'HTMLTableElement', 'HTMLTableRowElement',
        'HTMLTableSectionElement', 'HTMLTemplateElement', 'HTMLTextAreaElement',
        'HTMLTimeElement', 'HTMLTitleElement', 'HTMLTrackElement', 'HTMLUListElement',
        'HTMLVideoElement'];
    for (var h = 0; h < htmlInterfaces.length; h++) { chain[htmlInterfaces[h]] = 'HTMLElement'; }

    var names = Object.keys(chain);
    for (var i = 0; i < names.length; i++) {
        if (typeof globalThis[names[i]] !== 'function') {
            globalThis[names[i]] = function () {
                throw new TypeError('Illegal constructor');
            };
        }
    }
    for (var j = 0; j < names.length; j++) {
        var parent = chain[names[j]];
        if (parent) {
            Object.setPrototypeOf(globalThis[names[j]].prototype, globalThis[parent].prototype);
        }
    }
    // NodeList extends Array so a collection keeps indexing, length, and
    // iteration while gaining item(). node_array stamps this prototype on every
    // collection it builds.
    function NodeList() { throw new TypeError('Illegal constructor'); }
    Object.setPrototypeOf(NodeList.prototype, Array.prototype);
    NodeList.prototype.item = function (index) {
        index = Number(index);
        return index >= 0 && index < this.length ? this[index] : null;
    };
    globalThis.NodeList = NodeList;

    var table = {};
    for (var k = 0; k < names.length; k++) { table[names[k]] = globalThis[names[k]].prototype; }
    table.NodeList = NodeList.prototype;
    globalThis.__silksurfInterfacePrototypes = table;

    function Event(type, init) {
        init = init || {};
        this.type = String(type);
        this.bubbles = !!init.bubbles;
        this.cancelable = !!init.cancelable;
        this.composed = !!init.composed;
        this.defaultPrevented = false;
        this.target = null;
        this.currentTarget = null;
        this.eventPhase = 0;
        this.isTrusted = false;
        this.timeStamp = performance.now();
    }
    Event.prototype.preventDefault = function () {
        if (this.cancelable) { this.defaultPrevented = true; }
    };
    Event.prototype.stopPropagation = function () { this.__stopPropagation = true; };
    Event.prototype.stopImmediatePropagation = function () {
        this.__stopPropagation = true;
        this.__stopImmediate = true;
    };
    globalThis.Event = Event;

    function CustomEvent(type, init) {
        Event.call(this, type, init);
        this.detail = init && 'detail' in init ? init.detail : null;
    }
    CustomEvent.prototype = Object.create(Event.prototype);
    CustomEvent.prototype.constructor = CustomEvent;
    globalThis.CustomEvent = CustomEvent;

    var eventSubclasses = ['UIEvent', 'MouseEvent', 'PointerEvent', 'KeyboardEvent',
        'FocusEvent', 'InputEvent', 'WheelEvent', 'TouchEvent', 'ErrorEvent',
        'MessageEvent', 'PopStateEvent', 'ProgressEvent', 'StorageEvent',
        'SubmitEvent', 'CloseEvent'];
    for (var e = 0; e < eventSubclasses.length; e++) {
        (function (name) {
            function Sub(type, init) { Event.call(this, type, init); Object.assign(this, init || {}); }
            Sub.prototype = Object.create(Event.prototype);
            Sub.prototype.constructor = Sub;
            globalThis[name] = Sub;
        })(eventSubclasses[e]);
    }

    // ---- Node members ----
    Node.prototype.contains = function (other) {
        if (!other || other.nodeId === undefined) { return false; }
        return __silksurfNodeContains(this.nodeId, other.nodeId);
    };
    Node.prototype.cloneNode = function (deep) {
        var id = __silksurfNodeClone(this.nodeId, !!deep);
        return id === null ? null : __silksurfWrapNode(id);
    };
    Object.defineProperty(Node.prototype, 'isConnected', {
        get: function () { return __silksurfNodeIsConnected(this.nodeId); }
    });
    Object.defineProperty(Node.prototype, 'parentElement', {
        get: function () {
            var parent = this.parentNode;
            return parent && parent.nodeType === 1 ? parent : null;
        }
    });
    Node.prototype.hasChildNodes = function () { return this.childNodes.length > 0; };
    Node.prototype.getRootNode = function () {
        var current = this;
        while (current.parentNode) { current = current.parentNode; }
        return current;
    };
    Node.prototype.remove = function () {
        var parent = this.parentNode;
        if (parent) { parent.removeChild(this); }
    };

    // ---- Element members ----
    Element.prototype.hasAttribute = function (name) {
        return __silksurfNodeHasAttribute(this.nodeId, String(name));
    };
    Element.prototype.removeAttribute = function (name) {
        __silksurfNodeRemoveAttribute(this.nodeId, String(name));
    };
    Element.prototype.getAttributeNames = function () {
        return __silksurfNodeAttributeNames(this.nodeId);
    };
    Element.prototype.toggleAttribute = function (name, force) {
        var present = this.hasAttribute(name);
        var next = force === undefined ? !present : !!force;
        if (next) { this.setAttribute(name, ''); } else { this.removeAttribute(name); }
        return next;
    };
    Element.prototype.hasAttributes = function () {
        return this.getAttributeNames().length > 0;
    };
    Object.defineProperty(Element.prototype, 'localName', {
        get: function () { return __silksurfNodeLocalName(this.nodeId); }
    });
    Object.defineProperty(Element.prototype, 'namespaceURI', {
        get: function () { return 'http://www.w3.org/1999/xhtml'; }
    });
    Object.defineProperty(Element.prototype, 'childElementCount', {
        get: function () { return this.children.length; }
    });
    Object.defineProperty(Element.prototype, 'firstElementChild', {
        get: function () { var c = this.children; return c.length ? c[0] : null; }
    });
    Object.defineProperty(Element.prototype, 'lastElementChild', {
        get: function () { var c = this.children; return c.length ? c[c.length - 1] : null; }
    });
    function siblingElement(node, forward) {
        var current = forward ? node.nextSibling : node.previousSibling;
        while (current && current.nodeType !== 1) {
            current = forward ? current.nextSibling : current.previousSibling;
        }
        return current || null;
    }
    Object.defineProperty(Element.prototype, 'nextElementSibling', {
        get: function () { return siblingElement(this, true); }
    });
    Object.defineProperty(Element.prototype, 'previousElementSibling', {
        get: function () { return siblingElement(this, false); }
    });
    Element.prototype.append = function () {
        for (var i = 0; i < arguments.length; i++) {
            var node = arguments[i];
            this.appendChild(typeof node === 'object' ? node : document.createTextNode(String(node)));
        }
    };
    Element.prototype.prepend = function () {
        var first = this.firstChild;
        for (var i = 0; i < arguments.length; i++) {
            var node = arguments[i];
            node = typeof node === 'object' ? node : document.createTextNode(String(node));
            this.insertBefore(node, first);
        }
    };
    Element.prototype.before = function () {
        var parent = this.parentNode;
        if (!parent) { return; }
        for (var i = 0; i < arguments.length; i++) {
            var node = arguments[i];
            node = typeof node === 'object' ? node : document.createTextNode(String(node));
            parent.insertBefore(node, this);
        }
    };
    Element.prototype.after = function () {
        var parent = this.parentNode;
        if (!parent) { return; }
        var anchor = this.nextSibling;
        for (var i = 0; i < arguments.length; i++) {
            var node = arguments[i];
            node = typeof node === 'object' ? node : document.createTextNode(String(node));
            parent.insertBefore(node, anchor);
        }
    };
    Element.prototype.replaceWith = function () {
        var parent = this.parentNode;
        if (!parent) { return; }
        for (var i = 0; i < arguments.length; i++) {
            var node = arguments[i];
            node = typeof node === 'object' ? node : document.createTextNode(String(node));
            parent.insertBefore(node, this);
        }
        parent.removeChild(this);
    };
    // Focus, blur, and scrolling need a view; the engine reports the no-op the
    // methods perform on a document with no focus ring or scroll offset.
    Element.prototype.focus = function () {};
    Element.prototype.blur = function () {};
    Element.prototype.scrollIntoView = function () {};
    // A context with no geometry provider has run no layout, so every box is
    // zero. SilkContext::set_geometry_provider replaces this with the accessor
    // that reads the last completed layout.
    Element.prototype.getBoundingClientRect = function () {
        return {
            x: 0, y: 0, width: 0, height: 0, top: 0, left: 0, right: 0, bottom: 0,
            toJSON: function () { return this; }
        };
    };

    // ---- DOMTokenList over className ----
    function tokensOf(element) {
        var value = element.className;
        if (typeof value !== 'string' || value.length === 0) { return []; }
        return value.split(/\s+/).filter(function (t) { return t.length > 0; });
    }
    function DOMTokenList(element) { this._element = element; }
    Object.defineProperty(DOMTokenList.prototype, 'length', {
        get: function () { return tokensOf(this._element).length; }
    });
    Object.defineProperty(DOMTokenList.prototype, 'value', {
        get: function () { return this._element.className || ''; },
        set: function (v) { this._element.className = String(v); }
    });
    DOMTokenList.prototype.item = function (index) {
        var tokens = tokensOf(this._element);
        return index >= 0 && index < tokens.length ? tokens[index] : null;
    };
    DOMTokenList.prototype.contains = function (token) {
        return tokensOf(this._element).indexOf(String(token)) !== -1;
    };
    DOMTokenList.prototype.add = function () {
        var tokens = tokensOf(this._element);
        for (var i = 0; i < arguments.length; i++) {
            var token = String(arguments[i]);
            if (tokens.indexOf(token) === -1) { tokens.push(token); }
        }
        this._element.className = tokens.join(' ');
    };
    DOMTokenList.prototype.remove = function () {
        var drop = [];
        for (var i = 0; i < arguments.length; i++) { drop.push(String(arguments[i])); }
        this._element.className = tokensOf(this._element).filter(function (t) {
            return drop.indexOf(t) === -1;
        }).join(' ');
    };
    DOMTokenList.prototype.toggle = function (token, force) {
        token = String(token);
        var present = this.contains(token);
        var next = force === undefined ? !present : !!force;
        if (next) { this.add(token); } else { this.remove(token); }
        return next;
    };
    DOMTokenList.prototype.replace = function (oldToken, newToken) {
        if (!this.contains(oldToken)) { return false; }
        this.remove(oldToken);
        this.add(newToken);
        return true;
    };
    DOMTokenList.prototype.forEach = function (callback, thisArg) {
        tokensOf(this._element).forEach(callback, thisArg);
    };
    DOMTokenList.prototype.toString = function () { return this.value; };
    DOMTokenList.prototype[Symbol.iterator] = function () {
        return tokensOf(this._element)[Symbol.iterator]();
    };
    globalThis.DOMTokenList = DOMTokenList;
    Object.defineProperty(Element.prototype, 'classList', {
        get: function () {
            if (!this.__classList) {
                Object.defineProperty(this, '__classList', {
                    value: new DOMTokenList(this), enumerable: false
                });
            }
            return this.__classList;
        }
    });

    // ---- Document members ----
    document.createComment = function (data) {
        var id = __silksurfCreateDetached('comment', String(data));
        return id === null ? null : __silksurfWrapNode(id);
    };
    document.createDocumentFragment = function () {
        var id = __silksurfCreateDetached('fragment', '');
        return id === null ? null : __silksurfWrapNode(id);
    };
    document.getElementsByClassName = function (names) {
        var selector = String(names).trim().split(/\s+/).map(function (n) {
            return '.' + n;
        }).join('');
        return selector === '' ? [] : document.querySelectorAll(selector);
    };
    document.getElementsByName = function (name) {
        return document.querySelectorAll('[name=' + JSON.stringify(String(name)) + ']');
    };
    document.contains = function (node) {
        return !!node && !!node.nodeId && __silksurfNodeIsConnected(node.nodeId);
    };
    Object.defineProperty(document, 'defaultView', { get: function () { return globalThis; } });
    Object.defineProperty(document, 'activeElement', { get: function () { return this.body; } });
    Object.defineProperty(document, 'title', {
        get: function () {
            var element = this.querySelector('title');
            return element ? element.textContent : '';
        },
        set: function (value) {
            var element = this.querySelector('title');
            if (element) { element.textContent = String(value); }
        }
    });
    if (document.referrer === undefined) { document.referrer = ''; }
    if (document.currentScript === undefined) { document.currentScript = null; }
    if (document.visibilityState === undefined) { document.visibilityState = 'visible'; }
    if (document.hidden === undefined) { document.hidden = false; }
    if (document.characterSet === undefined) { document.characterSet = 'UTF-8'; }
    if (document.contentType === undefined) { document.contentType = 'text/html'; }
    if (document.compatMode === undefined) { document.compatMode = 'CSS1Compat'; }
})();
";

/*
 * IDL attribute reflection.
 *
 * HTML defines most element properties as reflections of a content attribute:
 * `link.rel` is the `rel` attribute, `img.src` is `src` resolved against the
 * document base, `input.disabled` is the presence of `disabled`. Without the
 * accessor a page's `element.rel = "stylesheet"` writes a plain own property
 * and the DOM never moves, so nothing downstream -- cascade, layout, resource
 * fetch -- observes the change.
 *
 * The accessors live on the interface prototype and reach the node through
 * `this.nodeId`, so a document pays nothing per element. Four reflection kinds
 * cover the table: `s` string, `u` URL resolved against document.URL, `b`
 * boolean by attribute presence, and `l` long parsed with a zero default.
 */
const REFLECTION_BOOTSTRAP: &str = r"
(function () {
    'use strict';
    var protos = globalThis.__silksurfInterfacePrototypes;
    if (!protos) { return; }

    function read(element, attribute) {
        return element.nodeId
            ? __silksurfNodeGetAttribute(element.nodeId, attribute)
            : null;
    }
    function write(element, attribute, value) {
        if (element.nodeId) { __silksurfNodeSetAttribute(element.nodeId, attribute, value); }
    }
    function drop(element, attribute) {
        if (element.nodeId) { __silksurfNodeRemoveAttribute(element.nodeId, attribute); }
    }
    function absolute(raw) {
        if (raw === null) { return ''; }
        try {
            return new URL(raw, document.URL).href;
        } catch (e) {
            return raw;
        }
    }

    var accessors = {
        s: function (attribute) {
            return {
                get: function () { var v = read(this, attribute); return v === null ? '' : v; },
                set: function (value) { write(this, attribute, String(value)); }
            };
        },
        u: function (attribute) {
            return {
                get: function () { return absolute(read(this, attribute)); },
                set: function (value) { write(this, attribute, String(value)); }
            };
        },
        b: function (attribute) {
            return {
                get: function () { return read(this, attribute) !== null; },
                set: function (value) {
                    if (value) { write(this, attribute, ''); } else { drop(this, attribute); }
                }
            };
        },
        l: function (attribute) {
            return {
                get: function () {
                    var parsed = parseInt(read(this, attribute), 10);
                    return Number.isNaN(parsed) ? 0 : parsed;
                },
                set: function (value) { write(this, attribute, String(value)); }
            };
        }
    };

    // interface: [[idlName, contentAttribute, kind], ...]
    var table = {
        Element: [['slot', 'slot', 's']],
        HTMLElement: [['title', 'title', 's'], ['lang', 'lang', 's'], ['dir', 'dir', 's'],
            ['hidden', 'hidden', 'b'], ['accessKey', 'accesskey', 's'],
            ['tabIndex', 'tabindex', 'l'], ['draggable', 'draggable', 'b'],
            ['spellcheck', 'spellcheck', 's'], ['translate', 'translate', 's']],
        HTMLAnchorElement: [['href', 'href', 'u'], ['target', 'target', 's'],
            ['rel', 'rel', 's'], ['download', 'download', 's'], ['type', 'type', 's'],
            ['hreflang', 'hreflang', 's'], ['referrerPolicy', 'referrerpolicy', 's'],
            ['ping', 'ping', 's']],
        HTMLAreaElement: [['href', 'href', 'u'], ['target', 'target', 's'],
            ['rel', 'rel', 's'], ['alt', 'alt', 's'], ['coords', 'coords', 's'],
            ['shape', 'shape', 's']],
        HTMLBaseElement: [['href', 'href', 'u'], ['target', 'target', 's']],
        HTMLButtonElement: [['type', 'type', 's'], ['name', 'name', 's'],
            ['value', 'value', 's'], ['disabled', 'disabled', 'b'],
            ['formAction', 'formaction', 'u'], ['formMethod', 'formmethod', 's']],
        HTMLCanvasElement: [['width', 'width', 'l'], ['height', 'height', 'l']],
        HTMLDialogElement: [['open', 'open', 'b'], ['returnValue', 'returnvalue', 's']],
        HTMLDetailsElement: [['open', 'open', 'b']],
        HTMLEmbedElement: [['src', 'src', 'u'], ['type', 'type', 's'],
            ['width', 'width', 's'], ['height', 'height', 's']],
        HTMLFieldSetElement: [['name', 'name', 's'], ['disabled', 'disabled', 'b']],
        HTMLFormElement: [['action', 'action', 'u'], ['method', 'method', 's'],
            ['name', 'name', 's'], ['target', 'target', 's'], ['enctype', 'enctype', 's'],
            ['acceptCharset', 'accept-charset', 's'], ['noValidate', 'novalidate', 'b'],
            ['autocomplete', 'autocomplete', 's']],
        HTMLIFrameElement: [['src', 'src', 'u'], ['srcdoc', 'srcdoc', 's'],
            ['name', 'name', 's'], ['width', 'width', 's'], ['height', 'height', 's'],
            ['allow', 'allow', 's'], ['loading', 'loading', 's'],
            ['referrerPolicy', 'referrerpolicy', 's'], ['sandbox', 'sandbox', 's']],
        HTMLImageElement: [['src', 'src', 'u'], ['alt', 'alt', 's'],
            ['srcset', 'srcset', 's'], ['sizes', 'sizes', 's'], ['useMap', 'usemap', 's'],
            ['crossOrigin', 'crossorigin', 's'], ['referrerPolicy', 'referrerpolicy', 's'],
            ['loading', 'loading', 's'], ['decoding', 'decoding', 's'],
            ['width', 'width', 'l'], ['height', 'height', 'l'],
            ['isMap', 'ismap', 'b']],
        HTMLInputElement: [['type', 'type', 's'], ['name', 'name', 's'],
            ['placeholder', 'placeholder', 's'], ['disabled', 'disabled', 'b'],
            ['required', 'required', 'b'], ['readOnly', 'readonly', 'b'],
            ['multiple', 'multiple', 'b'], ['maxLength', 'maxlength', 'l'],
            ['minLength', 'minlength', 'l'], ['min', 'min', 's'], ['max', 'max', 's'],
            ['step', 'step', 's'], ['pattern', 'pattern', 's'],
            ['autocomplete', 'autocomplete', 's'], ['accept', 'accept', 's'],
            ['src', 'src', 'u'], ['alt', 'alt', 's'],
            ['defaultChecked', 'checked', 'b'], ['defaultValue', 'value', 's']],
        HTMLLabelElement: [['htmlFor', 'for', 's']],
        HTMLLinkElement: [['href', 'href', 'u'], ['rel', 'rel', 's'],
            ['media', 'media', 's'], ['type', 'type', 's'], ['as', 'as', 's'],
            ['crossOrigin', 'crossorigin', 's'], ['integrity', 'integrity', 's'],
            ['hreflang', 'hreflang', 's'], ['disabled', 'disabled', 'b'],
            ['referrerPolicy', 'referrerpolicy', 's']],
        HTMLMetaElement: [['name', 'name', 's'], ['content', 'content', 's'],
            ['httpEquiv', 'http-equiv', 's'], ['media', 'media', 's']],
        HTMLObjectElement: [['data', 'data', 'u'], ['type', 'type', 's'],
            ['name', 'name', 's'], ['width', 'width', 's'], ['height', 'height', 's']],
        HTMLOptGroupElement: [['label', 'label', 's'], ['disabled', 'disabled', 'b']],
        HTMLOptionElement: [['label', 'label', 's'], ['disabled', 'disabled', 'b'],
            ['defaultSelected', 'selected', 'b'], ['value', 'value', 's']],
        HTMLOutputElement: [['name', 'name', 's'], ['htmlFor', 'for', 's']],
        HTMLScriptElement: [['src', 'src', 'u'], ['type', 'type', 's'],
            ['noModule', 'nomodule', 'b'], ['async', 'async', 'b'],
            ['defer', 'defer', 'b'], ['crossOrigin', 'crossorigin', 's'],
            ['integrity', 'integrity', 's'], ['referrerPolicy', 'referrerpolicy', 's']],
        HTMLSelectElement: [['name', 'name', 's'], ['disabled', 'disabled', 'b'],
            ['multiple', 'multiple', 'b'], ['required', 'required', 'b'],
            ['size', 'size', 'l'], ['autocomplete', 'autocomplete', 's']],
        HTMLSlotElement: [['name', 'name', 's']],
        HTMLSourceElement: [['src', 'src', 'u'], ['srcset', 'srcset', 's'],
            ['sizes', 'sizes', 's'], ['type', 'type', 's'], ['media', 'media', 's']],
        HTMLStyleElement: [['media', 'media', 's'], ['type', 'type', 's'],
            ['disabled', 'disabled', 'b']],
        HTMLTextAreaElement: [['name', 'name', 's'], ['placeholder', 'placeholder', 's'],
            ['disabled', 'disabled', 'b'], ['required', 'required', 'b'],
            ['readOnly', 'readonly', 'b'], ['rows', 'rows', 'l'], ['cols', 'cols', 'l'],
            ['maxLength', 'maxlength', 'l'], ['wrap', 'wrap', 's'],
            ['autocomplete', 'autocomplete', 's']],
        HTMLTimeElement: [['dateTime', 'datetime', 's']],
        HTMLTrackElement: [['src', 'src', 'u'], ['kind', 'kind', 's'],
            ['srclang', 'srclang', 's'], ['label', 'label', 's'],
            ['default', 'default', 'b']],
        HTMLMediaElement: [['src', 'src', 'u'], ['preload', 'preload', 's'],
            ['autoplay', 'autoplay', 'b'], ['controls', 'controls', 'b'],
            ['loop', 'loop', 'b'], ['muted', 'muted', 'b'],
            ['crossOrigin', 'crossorigin', 's']],
        HTMLVideoElement: [['poster', 'poster', 'u'], ['width', 'width', 'l'],
            ['height', 'height', 'l'], ['playsInline', 'playsinline', 'b']],
        HTMLOListElement: [['reversed', 'reversed', 'b'], ['start', 'start', 'l'],
            ['type', 'type', 's']],
        HTMLLIElement: [['value', 'value', 'l']],
        HTMLTableCellElement: [['colSpan', 'colspan', 'l'], ['rowSpan', 'rowspan', 'l'],
            ['headers', 'headers', 's'], ['scope', 'scope', 's'], ['abbr', 'abbr', 's']],
        HTMLProgressElement: [['max', 'max', 'l']],
        HTMLMeterElement: [['min', 'min', 'l'], ['max', 'max', 'l'], ['low', 'low', 'l'],
            ['high', 'high', 'l'], ['optimum', 'optimum', 'l']],
        HTMLQuoteElement: [['cite', 'cite', 'u']],
        HTMLModElement: [['cite', 'cite', 'u'], ['dateTime', 'datetime', 's']],
        HTMLDataElement: [['value', 'value', 's']]
    };

    Object.keys(table).forEach(function (name) {
        var proto = protos[name];
        if (!proto) { return; }
        table[name].forEach(function (entry) {
            var descriptor = accessors[entry[2]](entry[1]);
            descriptor.configurable = true;
            descriptor.enumerable = false;
            Object.defineProperty(proto, entry[0], descriptor);
        });
    });
})();
";
