//! Element HostObject -- exposes DOM elements to JavaScript.
//!
//! Provides: appendChild, removeChild, insertBefore, setAttribute,
//! getAttribute, className, classList, textContent, children, parentNode,
//! tagName, id, style, nextSibling, childNodes.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use silksurf_dom::{AttributeName, Dom, NodeId, NodeKind};

use super::{SharedDom, node_to_js_value};
use crate::vm::builtins::array::create_array;
use crate::vm::host::HostObject;
use crate::vm::value::{NativeFunction, Value};

/// JS Element object backed by a NodeId + shared Dom reference.
pub struct ElementHost {
    dom: SharedDom,
    node_id: NodeId,
}

impl std::fmt::Debug for ElementHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ElementHost(node={:?})", self.node_id)
    }
}

impl ElementHost {
    pub fn new(dom: SharedDom, node_id: NodeId) -> Self {
        Self { dom, node_id }
    }

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
                    .map(|n| Value::string(&n.to_ascii_uppercase()))
                    .unwrap_or(Value::Undefined)
            }
            "id" => {
                let dom = dom_ref.borrow();
                get_attribute_value(&dom, nid, "id")
                    .map(Value::string_owned)
                    .unwrap_or(Value::string(""))
            }
            "className" => {
                let dom = dom_ref.borrow();
                get_attribute_value(&dom, nid, "class")
                    .map(Value::string_owned)
                    .unwrap_or(Value::string(""))
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
                create_array(children)
            }
            "firstChild" | "firstElementChild" => {
                let dom = dom_ref.borrow();
                dom.first_child(nid)
                    .ok()
                    .flatten()
                    .map(|child| node_to_js_value(dom_ref, child))
                    .unwrap_or(Value::Null)
            }
            "lastChild" | "lastElementChild" => {
                let dom = dom_ref.borrow();
                dom.last_child(nid)
                    .ok()
                    .flatten()
                    .map(|child| node_to_js_value(dom_ref, child))
                    .unwrap_or(Value::Null)
            }
            "parentNode" | "parentElement" => {
                let dom = dom_ref.borrow();
                dom.parent(nid)
                    .ok()
                    .flatten()
                    .map(|parent| node_to_js_value(dom_ref, parent))
                    .unwrap_or(Value::Null)
            }
            "nextSibling" | "nextElementSibling" => {
                let dom = dom_ref.borrow();
                dom.next_sibling(nid)
                    .ok()
                    .flatten()
                    .map(|sib| node_to_js_value(dom_ref, sib))
                    .unwrap_or(Value::Null)
            }
            "previousSibling" | "previousElementSibling" => {
                let dom = dom_ref.borrow();
                dom.previous_sibling(nid)
                    .ok()
                    .flatten()
                    .map(|sib| node_to_js_value(dom_ref, sib))
                    .unwrap_or(Value::Null)
            }
            "nodeType" => {
                let dom = dom_ref.borrow();
                let t = match dom.node(nid).ok().map(|n| n.kind()) {
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
                        .map(Value::string_owned)
                        .unwrap_or(Value::Null)
                })))
            }
            "classList" => {
                // Return a simple object with add/remove/toggle/contains
                let dom = Rc::clone(dom_ref);
                let node = nid;
                make_class_list(dom, node)
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
            _ => false,
        }
    }

    fn class_name(&self) -> &str {
        "HTMLElement"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Extract NodeId from a HostObject Value (if it's an ElementHost).
fn extract_node_id(val: Option<&Value>) -> Option<NodeId> {
    match val? {
        Value::HostObject(host) => {
            let borrowed = host.borrow();
            borrowed
                .as_any()
                .downcast_ref::<ElementHost>()
                .map(|e| e.node_id())
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
    if let Ok(n) = dom.node(node) {
        if let NodeKind::Text { text } = n.kind() {
            result.push_str(text);
        }
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_text(dom, child, result);
        }
    }
}

/// Create a classList object with add/remove/toggle/contains methods.
fn make_class_list(dom: SharedDom, node: NodeId) -> Value {
    use crate::vm::value::{Object, PropertyKey};

    let obj = Object::new();
    let obj_rc = Rc::new(RefCell::new(obj));

    let dom_add = Rc::clone(&dom);
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

    let dom_remove = Rc::clone(&dom);
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

    let dom_toggle = Rc::clone(&dom);
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

    let dom_contains = Rc::clone(&dom);
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
        o.set_by_key(PropertyKey::from_str("add"), add_fn);
        o.set_by_key(PropertyKey::from_str("remove"), remove_fn);
        o.set_by_key(PropertyKey::from_str("toggle"), toggle_fn);
        o.set_by_key(PropertyKey::from_str("contains"), contains_fn);
    }

    Value::Object(obj_rc)
}
