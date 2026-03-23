//! `document` HostObject -- exposes DOM document to JavaScript.

use std::any::Any;
use std::rc::Rc;

use silksurf_dom::NodeId;

use super::{SharedDom, node_to_js_value};
use crate::vm::host::HostObject;
use crate::vm::value::{NativeFunction, Value};

/// JS `document` object backed by silksurf_dom::Dom.
pub struct DocumentHost {
    dom: SharedDom,
    document_node: NodeId,
}

impl std::fmt::Debug for DocumentHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DocumentHost(root={:?})", self.document_node)
    }
}

impl DocumentHost {
    pub fn new(dom: SharedDom, document_node: NodeId) -> Self {
        Self { dom, document_node }
    }

    fn find_body(&self) -> Option<NodeId> {
        self.find_element_by_tag("body")
    }

    fn find_head(&self) -> Option<NodeId> {
        self.find_element_by_tag("head")
    }

    fn find_document_element(&self) -> Option<NodeId> {
        self.find_element_by_tag("html")
    }

    fn find_element_by_tag(&self, tag: &str) -> Option<NodeId> {
        let dom = self.dom.borrow();
        find_tag_recursive(&dom, self.document_node, tag)
    }

    fn find_element_by_id(&self, id: &str) -> Option<NodeId> {
        let dom = self.dom.borrow();
        find_by_id_recursive(&dom, self.document_node, id)
    }
}

impl HostObject for DocumentHost {
    fn get_property(&self, name: &str) -> Value {
        let dom_ref = &self.dom;
        match name {
            "body" => self
                .find_body()
                .map(|n| node_to_js_value(dom_ref, n))
                .unwrap_or(Value::Null),
            "head" => self
                .find_head()
                .map(|n| node_to_js_value(dom_ref, n))
                .unwrap_or(Value::Null),
            "documentElement" => self
                .find_document_element()
                .map(|n| node_to_js_value(dom_ref, n))
                .unwrap_or(Value::Null),
            "createElement" => {
                let dom = Rc::clone(dom_ref);
                Value::NativeFunction(Rc::new(NativeFunction::new(
                    "document.createElement",
                    move |args| {
                        let tag = args
                            .first()
                            .map(|v| {
                                let s = v.to_js_string();
                                s.as_str().unwrap_or("div").to_string()
                            })
                            .unwrap_or_else(|| "div".to_string());
                        let node_id = dom.borrow_mut().create_element(tag);
                        node_to_js_value(&dom, node_id)
                    },
                )))
            }
            "createTextNode" => {
                let dom = Rc::clone(dom_ref);
                Value::NativeFunction(Rc::new(NativeFunction::new(
                    "document.createTextNode",
                    move |args| {
                        let text = args
                            .first()
                            .map(|v| {
                                let s = v.to_js_string();
                                s.as_str().unwrap_or("").to_string()
                            })
                            .unwrap_or_default();
                        let node_id = dom.borrow_mut().create_text(text);
                        node_to_js_value(&dom, node_id)
                    },
                )))
            }
            "getElementById" => {
                let dom = Rc::clone(dom_ref);
                let doc_node = self.document_node;
                Value::NativeFunction(Rc::new(NativeFunction::new(
                    "document.getElementById",
                    move |args| {
                        let id = args
                            .first()
                            .map(|v| {
                                let s = v.to_js_string();
                                s.as_str().unwrap_or("").to_string()
                            })
                            .unwrap_or_default();
                        let dom_borrow = dom.borrow();
                        find_by_id_recursive(&dom_borrow, doc_node, &id)
                            .map(|n| {
                                drop(dom_borrow);
                                node_to_js_value(&dom, n)
                            })
                            .unwrap_or(Value::Null)
                    },
                )))
            }
            "querySelector" => {
                let dom = Rc::clone(dom_ref);
                let doc_node = self.document_node;
                Value::NativeFunction(Rc::new(NativeFunction::new(
                    "document.querySelector",
                    move |args| {
                        let _selector = args
                            .first()
                            .map(|v| {
                                let s = v.to_js_string();
                                s.as_str().unwrap_or("").to_string()
                            })
                            .unwrap_or_default();
                        // Simplified: for now, just return first child element
                        // Full impl would parse the selector and use silksurf-css matching
                        let dom_borrow = dom.borrow();
                        let result = dom_borrow
                            .children(doc_node)
                            .ok()
                            .and_then(|children| children.first().copied());
                        drop(dom_borrow);
                        result
                            .map(|n| node_to_js_value(&dom, n))
                            .unwrap_or(Value::Null)
                    },
                )))
            }
            "createDocumentFragment" => {
                let dom = Rc::clone(dom_ref);
                Value::NativeFunction(Rc::new(NativeFunction::new(
                    "document.createDocumentFragment",
                    move |_args| {
                        let node_id = dom.borrow_mut().create_element("__fragment__");
                        node_to_js_value(&dom, node_id)
                    },
                )))
            }
            /*
             * getElementsByTagName -- collect all elements with matching tag.
             * Returns a live-ish array (snapshotted at call time).
             * "*" matches all elements per the HTML spec.
             */
            "getElementsByTagName" => {
                let dom = Rc::clone(dom_ref);
                let doc_node = self.document_node;
                Value::NativeFunction(Rc::new(NativeFunction::new(
                    "document.getElementsByTagName",
                    move |args| {
                        let tag = args
                            .first()
                            .map(|v| {
                                let s = v.to_js_string();
                                s.as_str().unwrap_or("*").to_lowercase()
                            })
                            .unwrap_or_else(|| "*".to_string());
                        let dom_borrow = dom.borrow();
                        let mut found = Vec::new();
                        collect_by_tag(&dom_borrow, doc_node, &tag, &mut found);
                        drop(dom_borrow);
                        use crate::vm::builtins::array::create_array;
                        let values: Vec<_> =
                            found.iter().map(|&n| node_to_js_value(&dom, n)).collect();
                        create_array(values)
                    },
                )))
            }
            /*
             * querySelectorAll -- simplified: same as getElementsByTagName("*")
             * for now. Full CSS selector parsing is deferred.
             */
            "querySelectorAll" => {
                let dom = Rc::clone(dom_ref);
                let doc_node = self.document_node;
                Value::NativeFunction(Rc::new(NativeFunction::new(
                    "document.querySelectorAll",
                    move |_args| {
                        let dom_borrow = dom.borrow();
                        let mut found = Vec::new();
                        collect_by_tag(&dom_borrow, doc_node, "*", &mut found);
                        drop(dom_borrow);
                        use crate::vm::builtins::array::create_array;
                        let values: Vec<_> =
                            found.iter().map(|&n| node_to_js_value(&dom, n)).collect();
                        create_array(values)
                    },
                )))
            }
            /*
             * addEventListener / removeEventListener / dispatchEvent stubs.
             *
             * WHY: Scripts call document.addEventListener('DOMContentLoaded', fn)
             * at init time. The handlers never fire in our headless VM, but
             * absorbing the registration prevents TypeError on the call.
             */
            "addEventListener" | "removeEventListener" => {
                Value::NativeFunction(Rc::new(NativeFunction::new(name, |_| Value::Undefined)))
            }
            "dispatchEvent" => {
                Value::NativeFunction(Rc::new(NativeFunction::new("dispatchEvent", |_| {
                    Value::Boolean(true)
                })))
            }
            _ => Value::Undefined,
        }
    }

    fn set_property(&mut self, _name: &str, _value: Value) -> bool {
        false // document properties are read-only
    }

    fn class_name(&self) -> &str {
        "HTMLDocument"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Recursively find an element by tag name.
fn find_tag_recursive(dom: &silksurf_dom::Dom, node: NodeId, target_tag: &str) -> Option<NodeId> {
    if let Ok(name) = dom.element_name(node) {
        if let Some(name) = name {
            if name.eq_ignore_ascii_case(target_tag) {
                return Some(node);
            }
        }
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            if let Some(found) = find_tag_recursive(dom, child, target_tag) {
                return Some(found);
            }
        }
    }
    None
}

/// Collect all elements whose tag name matches `target` (or all if `target == "*"`).
fn collect_by_tag(
    dom: &silksurf_dom::Dom,
    node: NodeId,
    target: &str,
    out: &mut Vec<NodeId>,
) {
    if let Ok(Some(name)) = dom.element_name(node) {
        if target == "*" || name.eq_ignore_ascii_case(target) {
            out.push(node);
        }
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_by_tag(dom, child, target, out);
        }
    }
}

/// Recursively find an element by id attribute.
fn find_by_id_recursive(dom: &silksurf_dom::Dom, node: NodeId, target_id: &str) -> Option<NodeId> {
    if let Ok(attrs) = dom.attributes(node) {
        for attr in attrs {
            if attr.name == silksurf_dom::AttributeName::Id && attr.value.as_str() == target_id {
                return Some(node);
            }
        }
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            if let Some(found) = find_by_id_recursive(dom, child, target_id) {
                return Some(found);
            }
        }
    }
    None
}
