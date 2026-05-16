//! html5ever TreeSink adapter.
//!
//! Bridges html5ever's WHATWG-conformant HTML5 parser to the silksurf_dom
//! arena-allocated DOM tree. The public entry point is `parse_html`.
//!
//! Design notes:
//! - All TreeSink methods take `&self` (not `&mut self`); interior
//!   mutability via `RefCell<Inner>` is required.
//! - elem_names stores `Box<QualName>` so the heap address is stable through
//!   HashMap reallocation; this makes the unsafe ptr extension in
//!   elem_name() sound.
//! - Text nodes are handled inline via NodeOrText::AppendText; html5ever
//!   does not use a separate create_text_node call.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;

use html5ever::parse_document;
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::{Attribute as Html5Attr, ExpandedName, ParseOpts, QualName};

use silksurf_dom::{Dom, Namespace, NodeId};

// ---- internal state ---------------------------------------------------------

struct Inner {
    dom: Dom,
    errors: Vec<String>,
    /// Stable-address QualName store for elem_name().
    /// `Box<QualName>` keeps the QualName at a fixed heap address even when
    /// the HashMap reallocates its backing buffer; the unsafe pointer
    /// extension in elem_name() relies on this stability guarantee.
    elem_names: HashMap<usize, Box<QualName>>,
}

// ---- builder ----------------------------------------------------------------

pub struct SilkDomBuilder {
    inner: RefCell<Inner>,
}

impl SilkDomBuilder {
    pub fn new() -> Self {
        let mut dom = Dom::new();
        dom.create_document(); // NodeId(0) = document root
        Self {
            inner: RefCell::new(Inner {
                dom,
                errors: Vec::new(),
                elem_names: HashMap::new(),
            }),
        }
    }
}

// ---- TreeSink implementation ------------------------------------------------

impl TreeSink for SilkDomBuilder {
    type Handle = usize;
    type Output = Dom;
    type ElemName<'a> = ExpandedName<'a>;

    // Returns the document root handle (always NodeId 0).
    fn get_document(&self) -> usize {
        0
    }

    fn elem_name<'a>(&'a self, target: &'a usize) -> ExpandedName<'a> {
        let inner = self.inner.borrow();
        // Auto-deref through Box<QualName> gives &QualName pointing at the
        // heap allocation; the Box is stored in elem_names which is
        // append-only (never removed), so the address is stable.
        // UNWRAP-OK: elem_names is populated in create_element for every
        // handle we return; html5ever only calls elem_name with handles it
        // received from us, so the entry always exists.
        let qname: &QualName = inner
            .elem_names
            .get(target)
            .expect("elem_name: handle not registered in elem_names");
        // SAFETY: The QualName lives in a Box<QualName> inside elem_names,
        // which is owned by self and is append-only. Box<T> guarantees a
        // stable heap address regardless of HashMap reallocation. self is
        // borrowed for 'a, so the QualName's address is valid for 'a.
        let qname: &'a QualName = unsafe { &*(qname as *const QualName) };
        qname.expanded()
    }

    fn same_node(&self, x: &usize, y: &usize) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

    fn parse_error(&self, msg: Cow<'static, str>) {
        self.inner.borrow_mut().errors.push(msg.into_owned());
    }

    fn create_element(&self, name: QualName, attrs: Vec<Html5Attr>, _flags: ElementFlags) -> usize {
        let mut inner = self.inner.borrow_mut();
        let local = name.local.as_ref();
        let ns = html5ever_ns_to_silk(name.ns.as_ref());
        let id = inner.dom.create_element_ns(local, ns);
        let raw = id.raw();
        for attr in attrs {
            let attr_local = attr.name.local.as_ref();
            let _ = inner.dom.set_attribute(id, attr_local, &*attr.value);
        }
        inner.elem_names.insert(raw, Box::new(name));
        raw
    }

    fn create_comment(&self, text: StrTendril) -> usize {
        self.inner.borrow_mut().dom.create_comment(&*text).raw()
    }

    // Processing instructions are treated as comments in HTML5 mode (the
    // parser itself emits PIs only in foreign content / SVG / XML).
    fn create_pi(&self, target: StrTendril, data: StrTendril) -> usize {
        let content = format!("?{} {}", &*target, &*data);
        self.inner.borrow_mut().dom.create_comment(&content).raw()
    }

    fn append(&self, parent: &usize, child: NodeOrText<usize>) {
        let mut inner = self.inner.borrow_mut();
        let parent_id = NodeId::from_raw(*parent);
        match child {
            NodeOrText::AppendNode(raw) => {
                let _ = inner.dom.append_child(parent_id, NodeId::from_raw(raw));
            }
            NodeOrText::AppendText(text) => {
                let _ = inner.dom.append_text(parent_id, &*text);
            }
        }
    }

    // Called by the foster-parenting algorithm for malformed table content.
    // If element has a parent, insert child before element in that parent.
    // Otherwise append child to prev_element.
    fn append_based_on_parent_node(
        &self,
        element: &usize,
        prev_element: &usize,
        child: NodeOrText<usize>,
    ) {
        let has_parent = {
            let inner = self.inner.borrow();
            inner
                .dom
                .parent(NodeId::from_raw(*element))
                .ok()
                .flatten()
                .is_some()
        };
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_before_sibling(&self, sibling: &usize, new_node: NodeOrText<usize>) {
        let mut inner = self.inner.borrow_mut();
        let sibling_id = NodeId::from_raw(*sibling);
        let parent_id = match inner.dom.parent(sibling_id) {
            Ok(Some(p)) => p,
            _ => return,
        };
        match new_node {
            NodeOrText::AppendNode(raw) => {
                let child_id = NodeId::from_raw(raw);
                let _ = inner.dom.insert_before(parent_id, child_id, sibling_id);
            }
            NodeOrText::AppendText(text) => {
                let text_id = inner.dom.create_text(&*text);
                let _ = inner.dom.insert_before(parent_id, text_id, sibling_id);
            }
        }
    }

    fn append_doctype_to_document(
        &self,
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    ) {
        let mut inner = self.inner.borrow_mut();
        let pub_str = &*public_id;
        let sys_str = &*system_id;
        let dt = inner.dom.create_doctype(
            Some(name.as_ref().to_string()),
            if pub_str.is_empty() {
                None
            } else {
                Some(pub_str.to_string())
            },
            if sys_str.is_empty() {
                None
            } else {
                Some(sys_str.to_string())
            },
        );
        let _ = inner.dom.append_child(NodeId::from_raw(0), dt);
    }

    // Template element content fragments: return the element itself as a
    // placeholder until template content is properly supported.
    fn get_template_contents(&self, target: &usize) -> usize {
        *target
    }

    // Add attributes to target only for names not already present.
    fn add_attrs_if_missing(&self, target: &usize, attrs: Vec<Html5Attr>) {
        let mut inner = self.inner.borrow_mut();
        let target_id = NodeId::from_raw(*target);
        // Collect existing names first so the immutable borrow on dom is
        // dropped before the mutable set_attribute calls below.
        let existing: Vec<String> = inner
            .dom
            .attributes(target_id)
            .map(|slice| slice.iter().map(|a| a.name.as_str().to_string()).collect())
            .unwrap_or_default();
        for attr in attrs {
            let local = attr.name.local.as_ref();
            if !existing.iter().any(|n| n.eq_ignore_ascii_case(local)) {
                let _ = inner.dom.set_attribute(target_id, local, &*attr.value);
            }
        }
    }

    fn remove_from_parent(&self, target: &usize) {
        let mut inner = self.inner.borrow_mut();
        let target_id = NodeId::from_raw(*target);
        if let Ok(Some(parent_id)) = inner.dom.parent(target_id) {
            let _ = inner.dom.remove_child(parent_id, target_id);
        }
    }

    fn reparent_children(&self, node: &usize, new_parent: &usize) {
        let mut inner = self.inner.borrow_mut();
        let node_id = NodeId::from_raw(*node);
        let new_parent_id = NodeId::from_raw(*new_parent);
        // Collect children first; the Vec<NodeId> releases the slice borrow
        // before the mutable remove_child / append_child calls below.
        let children: Vec<NodeId> = inner
            .dom
            .children(node_id)
            .map(<[NodeId]>::to_vec)
            .unwrap_or_default();
        for child in children {
            let _ = inner.dom.remove_child(node_id, child);
            let _ = inner.dom.append_child(new_parent_id, child);
        }
    }

    fn finish(self) -> Dom {
        let mut inner = self.inner.into_inner();
        inner.dom.materialize_resolve_table();
        inner.dom
    }
}

// ---- namespace conversion ---------------------------------------------------

fn html5ever_ns_to_silk(ns: &str) -> Namespace {
    match ns {
        "http://www.w3.org/1999/xhtml" => Namespace::Html,
        "http://www.w3.org/2000/svg" => Namespace::Svg,
        "http://www.w3.org/1998/Math/MathML" => Namespace::MathMl,
        other => Namespace::Other(other.to_string()),
    }
}

// ---- public entry point -----------------------------------------------------

/// Parse `input` as an HTML5 document and return a materialized DOM tree.
///
/// Parse errors are silently discarded; the returned DOM is always
/// structurally well-formed.
#[must_use]
pub fn parse_html(input: &str) -> Dom {
    let sink = SilkDomBuilder::new();
    parse_document(sink, ParseOpts::default()).one(input)
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use silksurf_dom::NodeKind;

    fn first_element_child(dom: &Dom, parent: NodeId) -> Option<NodeId> {
        dom.children(parent)
            .ok()?
            .iter()
            .copied()
            .find(|&child| dom.element_name(child).ok().flatten().is_some())
    }

    #[test]
    fn parse_returns_document_root() {
        let dom = parse_html("<html><body><p>hello</p></body></html>");
        // NodeId(0) is always the document root.
        let root = NodeId::from_raw(0);
        assert!(matches!(dom.node(root).unwrap().kind(), NodeKind::Document));
    }

    #[test]
    fn parse_builds_element_tree() {
        let dom = parse_html("<html><body><div id=\"x\"></div></body></html>");
        let root = NodeId::from_raw(0);
        // html element is a child of the document.
        let html = first_element_child(&dom, root).expect("html child");
        assert_eq!(dom.element_name(html).unwrap(), Some("html"));
    }

    #[test]
    fn parse_doctype_appended() {
        let dom = parse_html("<!DOCTYPE html><html><body></body></html>");
        let root = NodeId::from_raw(0);
        let children = dom.children(root).unwrap();
        let has_doctype = children
            .iter()
            .any(|&child| matches!(dom.node(child).unwrap().kind(), NodeKind::Doctype { .. }));
        assert!(has_doctype);
    }
}
