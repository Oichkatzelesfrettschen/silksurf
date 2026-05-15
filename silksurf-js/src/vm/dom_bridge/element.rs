//! Element `HostObject` -- exposes DOM elements to JavaScript.
//!
//! Provides: appendChild, removeChild, insertBefore, setAttribute,
//! getAttribute, className, classList, textContent, children, parentNode,
//! tagName, id, style, dataset, data, nodeValue, nextSibling, childNodes.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use silksurf_dom::{AttributeName, Dom, NodeId, NodeKind};

use super::{SharedDom, node_to_js_value};
use crate::vm::builtins::array::create_array;
use crate::vm::host::{HostObject, HostObjectRef, make_host_object};
use crate::vm::value::{NativeFunction, Value};

/// JS Element object backed by a `NodeId` + shared Dom reference.
pub struct ElementHost {
    dom: SharedDom,
    node_id: NodeId,
    /*
     * style and dataset are cached HostObjectRef so that repeated accesses
     * to element.style or element.dataset return the same object.
     * Without caching, element.style.display = "none" would set a property
     * on a freshly-allocated StyleHost that is immediately discarded.
     */
    style: HostObjectRef,
    dataset: HostObjectRef,
}

impl std::fmt::Debug for ElementHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ElementHost(node={:?})", self.node_id)
    }
}

impl ElementHost {
    pub fn new(dom: SharedDom, node_id: NodeId) -> Self {
        let dataset_dom = Rc::clone(&dom);
        Self {
            dom,
            node_id,
            style: make_host_object(StyleHost::default()),
            dataset: make_host_object(DatasetHost::new(dataset_dom, node_id)),
        }
    }

    #[must_use]
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    fn get_text_content(dom: &Dom, node: NodeId) -> String {
        let mut result = String::new();
        collect_text(dom, node, &mut result);
        result
    }
}

impl HostObject for ElementHost {
    fn get_property(&self, name: &str) -> Value {
        let dom_ref = &self.dom;
        let nid = self.node_id;

        match name {
            "tagName" | "nodeName" => {
                let dom = dom_ref.borrow();
                dom.element_name(nid)
                    .ok()
                    .flatten()
                    .map_or(Value::Undefined, |n| Value::string(&n.to_ascii_uppercase()))
            }
            "id" => {
                let dom = dom_ref.borrow();
                get_attribute_value(&dom, nid, "id").map_or(Value::string(""), Value::string_owned)
            }
            "className" => {
                let dom = dom_ref.borrow();
                get_attribute_value(&dom, nid, "class")
                    .map_or(Value::string(""), Value::string_owned)
            }
            "textContent" => {
                let dom = dom_ref.borrow();
                Value::string_owned(Self::get_text_content(&dom, nid))
            }
            "innerHTML" => {
                // Simplified: return text content (full impl would serialize child HTML)
                let dom = dom_ref.borrow();
                Value::string_owned(Self::get_text_content(&dom, nid))
            }
            "children" | "childNodes" => {
                let dom = dom_ref.borrow();
                let children: Vec<Value> = dom
                    .children(nid)
                    .map(|kids| {
                        kids.iter()
                            .map(|&child| node_to_js_value(dom_ref, child))
                            .collect()
                    })
                    .unwrap_or_default();
                create_array(&children)
            }
            "firstChild" | "firstElementChild" => {
                let dom = dom_ref.borrow();
                dom.first_child(nid)
                    .ok()
                    .flatten()
                    .map_or(Value::Null, |child| node_to_js_value(dom_ref, child))
            }
            "lastChild" | "lastElementChild" => {
                let dom = dom_ref.borrow();
                dom.last_child(nid)
                    .ok()
                    .flatten()
                    .map_or(Value::Null, |child| node_to_js_value(dom_ref, child))
            }
            "parentNode" | "parentElement" => {
                let dom = dom_ref.borrow();
                dom.parent(nid)
                    .ok()
                    .flatten()
                    .map_or(Value::Null, |parent| node_to_js_value(dom_ref, parent))
            }
            "nextSibling" | "nextElementSibling" => {
                let dom = dom_ref.borrow();
                dom.next_sibling(nid)
                    .ok()
                    .flatten()
                    .map_or(Value::Null, |sib| node_to_js_value(dom_ref, sib))
            }
            "previousSibling" | "previousElementSibling" => {
                let dom = dom_ref.borrow();
                dom.previous_sibling(nid)
                    .ok()
                    .flatten()
                    .map_or(Value::Null, |sib| node_to_js_value(dom_ref, sib))
            }
            "nodeType" => {
                let dom = dom_ref.borrow();
                let t = match dom.node(nid).ok().map(silksurf_dom::Node::kind) {
                    Some(NodeKind::Element { .. }) => 1,
                    Some(NodeKind::Text { .. }) => 3,
                    Some(NodeKind::Comment { .. }) => 8,
                    Some(NodeKind::Document) => 9,
                    Some(NodeKind::Doctype { .. }) => 10,
                    None => 0,
                };
                Value::Number(f64::from(t))
            }
            "appendChild" => {
                let dom = Rc::clone(dom_ref);
                let parent = nid;
                Value::NativeFunction(Rc::new(NativeFunction::new("appendChild", move |args| {
                    let child_id = extract_node_id(args.first());
                    if let Some(child) = child_id {
                        let _ = dom.borrow_mut().append_child(parent, child);
                        node_to_js_value(&dom, child)
                    } else {
                        Value::Null
                    }
                })))
            }
            "removeChild" => {
                let dom = Rc::clone(dom_ref);
                let parent = nid;
                Value::NativeFunction(Rc::new(NativeFunction::new("removeChild", move |args| {
                    let child_id = extract_node_id(args.first());
                    if let Some(child) = child_id {
                        let _ = dom.borrow_mut().remove_child(parent, child);
                        node_to_js_value(&dom, child)
                    } else {
                        Value::Null
                    }
                })))
            }
            "insertBefore" => {
                let dom = Rc::clone(dom_ref);
                let parent = nid;
                Value::NativeFunction(Rc::new(NativeFunction::new("insertBefore", move |args| {
                    let new_child = extract_node_id(args.first());
                    let ref_child = extract_node_id(args.get(1));
                    if let Some(new_node) = new_child {
                        if let Some(ref_node) = ref_child {
                            let _ = dom.borrow_mut().insert_before(parent, new_node, ref_node);
                        } else {
                            let _ = dom.borrow_mut().append_child(parent, new_node);
                        }
                        node_to_js_value(&dom, new_node)
                    } else {
                        Value::Null
                    }
                })))
            }
            "setAttribute" => {
                let dom = Rc::clone(dom_ref);
                let node = nid;
                Value::NativeFunction(Rc::new(NativeFunction::new("setAttribute", move |args| {
                    let attr_name = args
                        .first()
                        .map(|v| {
                            let s = v.to_js_string();
                            s.as_str().unwrap_or("").to_string()
                        })
                        .unwrap_or_default();
                    let attr_value = args
                        .get(1)
                        .map(|v| {
                            let s = v.to_js_string();
                            s.as_str().unwrap_or("").to_string()
                        })
                        .unwrap_or_default();
                    let _ = dom.borrow_mut().set_attribute(node, attr_name, attr_value);
                    Value::Undefined
                })))
            }
            "getAttribute" => {
                let dom = Rc::clone(dom_ref);
                let node = nid;
                Value::NativeFunction(Rc::new(NativeFunction::new("getAttribute", move |args| {
                    let attr_name = args
                        .first()
                        .map(|v| {
                            let s = v.to_js_string();
                            s.as_str().unwrap_or("").to_string()
                        })
                        .unwrap_or_default();
                    let dom_borrow = dom.borrow();
                    get_attribute_value(&dom_borrow, node, &attr_name)
                        .map_or(Value::Null, Value::string_owned)
                })))
            }
            "classList" => {
                // Return a simple object with add/remove/toggle/contains
                make_class_list(dom_ref, nid)
            }
            /*
             * data / nodeValue -- raw text content of Text and Comment nodes.
             * In the W3C DOM, Text.data and Text.nodeValue both return the
             * character data string. Element.nodeValue is null.
             */
            "data" | "nodeValue" => {
                let dom = dom_ref.borrow();
                match dom.node(nid).ok().map(silksurf_dom::Node::kind) {
                    Some(NodeKind::Text { text }) => Value::string_owned(text.clone()),
                    Some(NodeKind::Comment { data: comment_text }) => {
                        Value::string_owned(comment_text.clone())
                    }
                    _ => Value::Null,
                }
            }
            /*
             * style -- inline CSS style proxy.
             * Returns a cached StyleHost so that `element.style.display = "none"`
             * persists across repeated accesses.
             */
            "style" => Value::HostObject(Rc::clone(&self.style)),
            /*
             * dataset -- data-* attribute proxy.
             * element.dataset.fooBar maps to attribute data-foo-bar.
             */
            "dataset" => Value::HostObject(Rc::clone(&self.dataset)),
            /*
             * addEventListener / removeEventListener / dispatchEvent stubs.
             * Scripts add event listeners to elements at init time.
             * Handlers never fire (no event loop), but absorbing the call
             * prevents TypeError from aborting the script.
             */
            "addEventListener" | "removeEventListener" => {
                Value::NativeFunction(Rc::new(NativeFunction::new(name, |_| Value::Undefined)))
            }
            "dispatchEvent" => {
                Value::NativeFunction(Rc::new(NativeFunction::new("dispatchEvent", |_| {
                    Value::Boolean(true)
                })))
            }
            /*
             * hasAttribute / removeAttribute -- attribute existence check and deletion.
             */
            "hasAttribute" => {
                let dom = Rc::clone(dom_ref);
                let node = nid;
                Value::NativeFunction(Rc::new(NativeFunction::new("hasAttribute", move |args| {
                    let attr = args
                        .first()
                        .map(|v| {
                            let s = v.to_js_string();
                            s.as_str().unwrap_or("").to_string()
                        })
                        .unwrap_or_default();
                    let dom_borrow = dom.borrow();
                    Value::Boolean(get_attribute_value(&dom_borrow, node, &attr).is_some())
                })))
            }
            "removeAttribute" => {
                let dom = Rc::clone(dom_ref);
                let node = nid;
                Value::NativeFunction(Rc::new(NativeFunction::new(
                    "removeAttribute",
                    move |args| {
                        let attr = args
                            .first()
                            .map(|v| {
                                let s = v.to_js_string();
                                s.as_str().unwrap_or("").to_string()
                            })
                            .unwrap_or_default();
                        let _ = dom.borrow_mut().set_attribute(node, attr, "");
                        Value::Undefined
                    },
                )))
            }
            /*
             * getBoundingClientRect -- returns a stub DOMRect.
             * WHY: Scripts use this to measure element dimensions for layout logic.
             * We return zeros since we have no display to measure against.
             * This prevents TypeError on .top/.left/.width/.height access.
             */
            "getBoundingClientRect" => Value::NativeFunction(Rc::new(NativeFunction::new(
                "getBoundingClientRect",
                |_| {
                    use crate::vm::value::Object;
                    let rect = Rc::new(RefCell::new(Object::new()));
                    let mut r = rect.borrow_mut();
                    for prop in &[
                        "top", "left", "right", "bottom", "width", "height", "x", "y",
                    ] {
                        r.set_by_str(prop, Value::Number(0.0));
                    }
                    drop(r);
                    Value::Object(rect)
                },
            ))),
            /*
             * querySelector / querySelectorAll on element -- same as document
             * but scoped to this element's subtree.
             */
            "querySelector" => {
                let dom = Rc::clone(dom_ref);
                let root = nid;
                Value::NativeFunction(Rc::new(NativeFunction::new("querySelector", move |args| {
                    let sel_str = args
                        .first()
                        .map(|v| {
                            let s = v.to_js_string();
                            s.as_str().unwrap_or("").to_string()
                        })
                        .unwrap_or_default();
                    use super::document::find_first_matching_pub;
                    let selector = parse_selector_for_element(&dom, &sel_str);
                    let Some(selector) = selector else {
                        return Value::Null;
                    };
                    let dom_borrow = dom.borrow();
                    let result = find_first_matching_pub(&dom_borrow, root, &selector);
                    drop(dom_borrow);
                    result.map_or(Value::Null, |n| node_to_js_value(&dom, n))
                })))
            }
            _ => Value::Undefined,
        }
    }

    fn set_property(&mut self, name: &str, value: Value) -> bool {
        match name {
            "textContent" => {
                let text = value.to_js_string();
                let text_str = text.as_str().unwrap_or("");
                let mut dom = self.dom.borrow_mut();
                // Remove existing children
                if let Ok(children) = dom.children(self.node_id) {
                    let children: Vec<NodeId> = children.to_vec();
                    for child in children {
                        let _ = dom.remove_child(self.node_id, child);
                    }
                }
                // Add new text node
                let text_node = dom.create_text(text_str);
                let _ = dom.append_child(self.node_id, text_node);
                true
            }
            "className" => {
                let cls = value.to_js_string();
                let cls_str = cls.as_str().unwrap_or("");
                let _ = self
                    .dom
                    .borrow_mut()
                    .set_attribute(self.node_id, "class", cls_str);
                true
            }
            "id" => {
                let id = value.to_js_string();
                let id_str = id.as_str().unwrap_or("");
                let _ = self
                    .dom
                    .borrow_mut()
                    .set_attribute(self.node_id, "id", id_str);
                true
            }
            "data" | "nodeValue" => {
                // Accept but ignore: Dom has no node_mut() API.
                // Scripts can set .data on text nodes without throwing.
                true
            }
            _ => false,
        }
    }

    fn class_name(&self) -> &'static str {
        "HTMLElement"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Extract `NodeId` from a `HostObject` Value (if it's an `ElementHost`).
fn extract_node_id(val: Option<&Value>) -> Option<NodeId> {
    match val? {
        Value::HostObject(host) => {
            let borrowed = host.borrow();
            borrowed
                .as_any()
                .downcast_ref::<ElementHost>()
                .map(ElementHost::node_id)
        }
        _ => None,
    }
}

/// Get an attribute value by name.
fn get_attribute_value(dom: &Dom, node: NodeId, name: &str) -> Option<String> {
    let attrs = dom.attributes(node).ok()?;
    let target = AttributeName::from_str(name);
    attrs
        .iter()
        .find(|a| a.name == target)
        .map(|a| a.value.to_string())
}

/// Collect text content recursively.
fn collect_text(dom: &Dom, node: NodeId, result: &mut String) {
    if let Ok(n) = dom.node(node)
        && let NodeKind::Text { text } = n.kind()
    {
        result.push_str(text);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_text(dom, child, result);
        }
    }
}

/// Tokenize + parse a CSS selector string using the DOM's shared interner.
fn parse_selector_for_element(
    dom: &SharedDom,
    selector: &str,
) -> Option<silksurf_css::SelectorList> {
    let mut tokenizer = silksurf_css::CssTokenizer::new();
    let mut tokens = tokenizer.feed(selector).ok()?;
    tokens.extend(tokenizer.finish().ok()?);
    let sel = dom.borrow().with_interner_mut(|interner| {
        silksurf_css::parse_selector_list_with_interner(tokens, Some(interner))
    });
    if sel.selectors.is_empty() {
        None
    } else {
        Some(sel)
    }
}

/*
 * StyleHost -- inline CSS property storage for element.style.X access.
 *
 * Stores CSS property values set via JS (e.g. element.style.display = "none")
 * in a HashMap. The cascade engine reads inline styles separately; this host
 * stores them so scripts can read them back without error.
 *
 * Note: CSS property names are stored as-is (camelCase from JS side).
 */
#[derive(Debug, Default)]
struct StyleHost {
    props: HashMap<String, String>,
}

impl HostObject for StyleHost {
    fn get_property(&self, name: &str) -> Value {
        self.props
            .get(name)
            .map_or_else(|| Value::string(""), |v| Value::string_owned(v.clone()))
    }

    fn set_property(&mut self, name: &str, value: Value) -> bool {
        let v = value.to_js_string();
        let v_str = v.as_str().unwrap_or("").to_string();
        self.props.insert(name.to_string(), v_str);
        true
    }

    fn class_name(&self) -> &'static str {
        "CSSStyleDeclaration"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/*
 * DatasetHost -- data-* attribute proxy for element.dataset.X access.
 *
 * Maps camelCase property names to kebab-case data-* attributes:
 *   element.dataset.fooBar  <->  data-foo-bar attribute
 *
 * See: https://html.spec.whatwg.org/multipage/dom.html#dom-dataset
 */
struct DatasetHost {
    dom: SharedDom,
    node: NodeId,
}

impl std::fmt::Debug for DatasetHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DatasetHost(node={:?})", self.node)
    }
}

impl DatasetHost {
    fn new(dom: SharedDom, node: NodeId) -> Self {
        Self { dom, node }
    }
}

impl HostObject for DatasetHost {
    fn get_property(&self, name: &str) -> Value {
        let attr_name = format!("data-{}", camel_to_kebab(name));
        let dom = self.dom.borrow();
        get_attribute_value(&dom, self.node, &attr_name)
            .map_or(Value::Undefined, Value::string_owned)
    }

    fn set_property(&mut self, name: &str, value: Value) -> bool {
        let attr_name = format!("data-{}", camel_to_kebab(name));
        let v = value.to_js_string();
        let v_str = v.as_str().unwrap_or("").to_string();
        let _ = self
            .dom
            .borrow_mut()
            .set_attribute(self.node, attr_name, v_str);
        true
    }

    fn class_name(&self) -> &'static str {
        "DOMStringMap"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Convert camelCase dataset key to kebab-case for data-* attribute names.
/// "fooBar" -> "foo-bar", "myDataValue" -> "my-data-value"
fn camel_to_kebab(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for ch in s.chars() {
        if ch.is_ascii_uppercase() {
            result.push('-');
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}

/// Create a classList object with add/remove/toggle/contains methods.
fn make_class_list(dom: &SharedDom, node: NodeId) -> Value {
    use crate::vm::value::{Object, PropertyKey};

    let obj = Object::new();
    let obj_rc = Rc::new(RefCell::new(obj));

    let dom_add = Rc::clone(dom);
    let add_fn = Value::NativeFunction(Rc::new(NativeFunction::new("add", move |args| {
        for arg in args {
            let cls = arg.to_js_string();
            let cls_str = cls.as_str().unwrap_or("");
            if cls_str.is_empty() {
                continue;
            }
            let mut d = dom_add.borrow_mut();
            let current = get_attribute_value(&d, node, "class").unwrap_or_default();
            if !current.split_whitespace().any(|c| c == cls_str) {
                let new = if current.is_empty() {
                    cls_str.to_string()
                } else {
                    format!("{current} {cls_str}")
                };
                let _ = d.set_attribute(node, "class", new);
            }
        }
        Value::Undefined
    })));

    let dom_remove = Rc::clone(dom);
    let remove_fn = Value::NativeFunction(Rc::new(NativeFunction::new("remove", move |args| {
        for arg in args {
            let cls = arg.to_js_string();
            let cls_str = cls.as_str().unwrap_or("");
            let mut d = dom_remove.borrow_mut();
            let current = get_attribute_value(&d, node, "class").unwrap_or_default();
            let new: Vec<&str> = current
                .split_whitespace()
                .filter(|c| *c != cls_str)
                .collect();
            let _ = d.set_attribute(node, "class", new.join(" "));
        }
        Value::Undefined
    })));

    let dom_toggle = Rc::clone(dom);
    let toggle_fn = Value::NativeFunction(Rc::new(NativeFunction::new("toggle", move |args| {
        let cls = args
            .first()
            .map(|v| {
                let s = v.to_js_string();
                s.as_str().unwrap_or("").to_string()
            })
            .unwrap_or_default();
        let mut d = dom_toggle.borrow_mut();
        let current = get_attribute_value(&d, node, "class").unwrap_or_default();
        let has = current.split_whitespace().any(|c| c == cls);
        if has {
            let new: Vec<&str> = current.split_whitespace().filter(|c| *c != cls).collect();
            let _ = d.set_attribute(node, "class", new.join(" "));
            Value::Boolean(false)
        } else {
            let new = if current.is_empty() {
                cls.clone()
            } else {
                format!("{current} {cls}")
            };
            let _ = d.set_attribute(node, "class", new);
            Value::Boolean(true)
        }
    })));

    let dom_contains = Rc::clone(dom);
    let contains_fn =
        Value::NativeFunction(Rc::new(NativeFunction::new("contains", move |args| {
            let cls = args
                .first()
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            let d = dom_contains.borrow();
            let current = get_attribute_value(&d, node, "class").unwrap_or_default();
            Value::Boolean(current.split_whitespace().any(|c| c == cls))
        })));

    {
        let mut o = obj_rc.borrow_mut();
        o.set_by_key(PropertyKey::string_key("add"), add_fn);
        o.set_by_key(PropertyKey::string_key("remove"), remove_fn);
        o.set_by_key(PropertyKey::string_key("toggle"), toggle_fn);
        o.set_by_key(PropertyKey::string_key("contains"), contains_fn);
    }

    Value::Object(obj_rc)
}
