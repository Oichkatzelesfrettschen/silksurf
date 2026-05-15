//! `document` `HostObject` -- exposes DOM document to JavaScript.

use std::any::Any;
use std::rc::Rc;

use silksurf_dom::NodeId;

use super::{SharedDom, node_to_js_value};
use crate::vm::host::HostObject;
use crate::vm::value::{NativeFunction, Value};

/// JS `document` object backed by `silksurf_dom::Dom`.
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
                .map_or(Value::Null, |n| node_to_js_value(dom_ref, n)),
            "head" => self
                .find_head()
                .map_or(Value::Null, |n| node_to_js_value(dom_ref, n)),
            "documentElement" => self
                .find_document_element()
                .map_or(Value::Null, |n| node_to_js_value(dom_ref, n)),
            "createElement" => {
                let dom = Rc::clone(dom_ref);
                Value::NativeFunction(Rc::new(NativeFunction::new(
                    "document.createElement",
                    move |args| {
                        let tag = args.first().map_or_else(
                            || "div".to_string(),
                            |v| {
                                let s = v.to_js_string();
                                s.as_str().unwrap_or("div").to_string()
                            },
                        );
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
                        find_by_id_recursive(&dom_borrow, doc_node, &id).map_or(Value::Null, |n| {
                            drop(dom_borrow);
                            node_to_js_value(&dom, n)
                        })
                    },
                )))
            }
            "querySelector" => {
                let dom = Rc::clone(dom_ref);
                let doc_node = self.document_node;
                Value::NativeFunction(Rc::new(NativeFunction::new(
                    "document.querySelector",
                    move |args| {
                        let selector_str = args
                            .first()
                            .map(|v| {
                                let s = v.to_js_string();
                                s.as_str().unwrap_or("").to_string()
                            })
                            .unwrap_or_default();
                        // Tokenize + parse selector using the DOM's shared interner.
                        let selector = parse_selector(&dom, &selector_str);
                        let Some(selector) = selector else {
                            return Value::Null;
                        };
                        let dom_borrow = dom.borrow();
                        let result = find_first_matching(&dom_borrow, doc_node, &selector);
                        drop(dom_borrow);
                        result.map_or(Value::Null, |n| node_to_js_value(&dom, n))
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
                        let tag = args.first().map_or_else(
                            || "*".to_string(),
                            |v| {
                                let s = v.to_js_string();
                                s.as_str().unwrap_or("*").to_lowercase()
                            },
                        );
                        let dom_borrow = dom.borrow();
                        let mut found = Vec::new();
                        collect_by_tag(&dom_borrow, doc_node, &tag, &mut found);
                        drop(dom_borrow);
                        use crate::vm::builtins::array::create_array;
                        let values: Vec<_> =
                            found.iter().map(|&n| node_to_js_value(&dom, n)).collect();
                        create_array(&values)
                    },
                )))
            }
            /*
             * getElementsByClassName -- collect all elements with matching class.
             * Accepts a space-separated list of class names (all must be present).
             */
            "getElementsByClassName" => {
                let dom = Rc::clone(dom_ref);
                let doc_node = self.document_node;
                Value::NativeFunction(Rc::new(NativeFunction::new(
                    "document.getElementsByClassName",
                    move |args| {
                        let class_str = args
                            .first()
                            .map(|v| {
                                let s = v.to_js_string();
                                s.as_str().unwrap_or("").to_string()
                            })
                            .unwrap_or_default();
                        let classes: Vec<&str> = class_str.split_whitespace().collect();
                        let dom_borrow = dom.borrow();
                        let mut found = Vec::new();
                        collect_by_class(&dom_borrow, doc_node, &classes, &mut found);
                        drop(dom_borrow);
                        use crate::vm::builtins::array::create_array;
                        let values: Vec<_> =
                            found.iter().map(|&n| node_to_js_value(&dom, n)).collect();
                        create_array(&values)
                    },
                )))
            }
            "querySelectorAll" => {
                let dom = Rc::clone(dom_ref);
                let doc_node = self.document_node;
                Value::NativeFunction(Rc::new(NativeFunction::new(
                    "document.querySelectorAll",
                    move |args| {
                        let selector_str = args.first().map_or_else(
                            || "*".to_string(),
                            |v| {
                                let s = v.to_js_string();
                                s.as_str().unwrap_or("*").to_string()
                            },
                        );
                        let selector = parse_selector(&dom, &selector_str);
                        // Fall back to all elements if selector parse fails
                        let dom_borrow = dom.borrow();
                        let mut found = Vec::new();
                        if let Some(sel) = selector {
                            collect_matching(&dom_borrow, doc_node, &sel, &mut found);
                        } else {
                            collect_by_tag(&dom_borrow, doc_node, "*", &mut found);
                        }
                        drop(dom_borrow);
                        use crate::vm::builtins::array::create_array;
                        let values: Vec<_> =
                            found.iter().map(|&n| node_to_js_value(&dom, n)).collect();
                        create_array(&values)
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

    fn class_name(&self) -> &'static str {
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
    if let Ok(name) = dom.element_name(node)
        && let Some(name) = name
        && name.eq_ignore_ascii_case(target_tag)
    {
        return Some(node);
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
fn collect_by_tag(dom: &silksurf_dom::Dom, node: NodeId, target: &str, out: &mut Vec<NodeId>) {
    if let Ok(Some(name)) = dom.element_name(node)
        && (target == "*" || name.eq_ignore_ascii_case(target))
    {
        out.push(node);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_by_tag(dom, child, target, out);
        }
    }
}

/*
 * parse_selector -- tokenize + parse a CSS selector string.
 *
 * WHY: querySelector/querySelectorAll receive selector strings from JS.
 * silksurf-css requires pre-tokenized Vec<CssToken>. This helper tokenizes
 * the string and parses it using the DOM's shared interner for atom reuse.
 * Returns None if tokenization fails (e.g. empty or malformed selector).
 *
 * See: silksurf_css::CssTokenizer for tokenization
 * See: silksurf_css::parse_selector_list_with_interner for parsing
 */
/// Collect elements whose class attribute contains ALL of the given class names.
fn collect_by_class(
    dom: &silksurf_dom::Dom,
    node: NodeId,
    classes: &[&str],
    out: &mut Vec<NodeId>,
) {
    if let Ok(attrs) = dom.attributes(node) {
        let has_all = attrs.iter().any(|attr| {
            if attr.name == silksurf_dom::AttributeName::Class {
                let val = attr.value.as_str();
                classes
                    .iter()
                    .all(|c| val.split_whitespace().any(|token| token == *c))
            } else {
                false
            }
        });
        if has_all && !classes.is_empty() {
            out.push(node);
        }
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_by_class(dom, child, classes, out);
        }
    }
}

fn parse_selector(dom: &super::SharedDom, selector: &str) -> Option<silksurf_css::SelectorList> {
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
 * find_first_matching -- DFS search for first node matching a CSS selector list.
 *
 * WHY: querySelector() must return the first matching node in tree order.
 * Uses silksurf_css::matches_selector_list which implements full CSS
 * selector matching (tag, class, id, attribute, pseudo-classes).
 *
 * Complexity: O(N * S) where N=nodes, S=selector complexity
 * See: silksurf_css::matches_selector_list (matching.rs)
 */
pub fn find_first_matching_pub(
    dom: &silksurf_dom::Dom,
    node: NodeId,
    selector: &silksurf_css::SelectorList,
) -> Option<NodeId> {
    find_first_matching(dom, node, selector)
}

fn find_first_matching(
    dom: &silksurf_dom::Dom,
    node: NodeId,
    selector: &silksurf_css::SelectorList,
) -> Option<NodeId> {
    if dom.element_name(node).ok().flatten().is_some()
        && silksurf_css::matches_selector_list(dom, node, selector)
    {
        return Some(node);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            if let Some(found) = find_first_matching(dom, child, selector) {
                return Some(found);
            }
        }
    }
    None
}

/*
 * collect_matching -- DFS collection of all nodes matching a CSS selector list.
 *
 * WHY: querySelectorAll() returns all matching nodes in tree order.
 * Descends the entire subtree and pushes matching element nodes.
 *
 * Complexity: O(N * S) where N=nodes, S=selector complexity
 * See: find_first_matching for single-match variant
 */
fn collect_matching(
    dom: &silksurf_dom::Dom,
    node: NodeId,
    selector: &silksurf_css::SelectorList,
    out: &mut Vec<NodeId>,
) {
    if dom.element_name(node).ok().flatten().is_some()
        && silksurf_css::matches_selector_list(dom, node, selector)
    {
        out.push(node);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_matching(dom, child, selector, out);
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
