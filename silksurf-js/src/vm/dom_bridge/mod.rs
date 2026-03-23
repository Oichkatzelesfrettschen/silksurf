//! JS-DOM bridge: exposes silksurf_dom to JavaScript via HostObject.
//!
//! Architecture:
//! - `DocumentHost` wraps `Rc<RefCell<Dom>>` as a HostObject
//! - `ElementHost` wraps a `NodeId` + `Rc<RefCell<Dom>>` reference
//! - Both dispatch property access to the underlying DOM
//! - The shared `Rc<RefCell<Dom>>` ensures JS mutations are visible to layout/render

mod document;
mod element;

use std::cell::RefCell;
use std::rc::Rc;

pub use document::DocumentHost;
pub use element::ElementHost;
use silksurf_dom::{Dom, NodeId};

use super::host::make_host_object;
use super::value::Value;

/// Shared DOM reference used by all bridge objects.
pub type SharedDom = Rc<RefCell<Dom>>;

/// Create a JS Value wrapping a DOM node as an ElementHost.
pub fn node_to_js_value(dom: &SharedDom, node_id: NodeId) -> Value {
    Value::HostObject(make_host_object(ElementHost::new(Rc::clone(dom), node_id)))
}

/// Install the `document` global on a VM given a shared DOM and root node.
pub fn install_document(
    global: &Rc<RefCell<super::value::Object>>,
    dom: SharedDom,
    document_node: NodeId,
) {
    let doc_host = DocumentHost::new(dom, document_node);
    let doc_value = Value::HostObject(make_host_object(doc_host));
    global.borrow_mut().set_by_str("document", doc_value);
}
