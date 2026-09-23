//! HTML form ownership and checkedness shared by native input and JavaScript.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CheckedState {
    pub checked: bool,
    pub dirty: bool,
}

use crate::{Dom, DomError, Namespace, Node, NodeId, NodeKind};
use std::collections::HashMap;

fn attribute<'a>(dom: &'a Dom, node: NodeId, name: &str) -> Option<&'a str> {
    dom.attributes(node)
        .ok()?
        .iter()
        .find(|attribute| attribute.name.matches(name))
        .map(|attribute| attribute.value.as_str())
}

fn tag(dom: &Dom, node: NodeId) -> &str {
    match dom.node(node).map(Node::kind) {
        Ok(NodeKind::Element {
            name,
            namespace: Namespace::Html,
            ..
        }) => name.as_str(),
        _ => "",
    }
}

fn tree_nodes(dom: &Dom, node: NodeId) -> Vec<NodeId> {
    let mut root = node;
    while let Ok(Some(parent)) = dom.parent(root) {
        root = parent;
    }
    let mut pending = vec![root];
    let mut nodes = Vec::new();
    while let Some(node) = pending.pop() {
        nodes.push(node);
        if let Ok(children) = dom.children(node) {
            pending.extend(children.iter().rev());
        }
    }
    nodes
}

fn first_ids<'a>(dom: &'a Dom, nodes: &[NodeId]) -> HashMap<&'a str, NodeId> {
    let mut ids = HashMap::new();
    for &node in nodes {
        if let Some(id) = attribute(dom, node, "id").filter(|id| !id.is_empty()) {
            ids.entry(id).or_insert(node);
        }
    }
    ids
}

fn form_owner(dom: &Dom, node: NodeId, ids: &HashMap<&str, NodeId>) -> Option<NodeId> {
    if dom.is_connected(node)
        && let Some(id) = attribute(dom, node, "form")
    {
        return ids
            .get(id)
            .copied()
            .filter(|&owner| tag(dom, owner) == "form");
    }
    let mut ancestor = dom.parent(node).ok().flatten();
    while let Some(parent) = ancestor {
        if tag(dom, parent) == "form" {
            return Some(parent);
        }
        ancestor = dom.parent(parent).ok().flatten();
    }
    None
}

fn listed_control(dom: &Dom, node: NodeId) -> bool {
    match tag(dom, node) {
        "input" => {
            !attribute(dom, node, "type").is_some_and(|kind| kind.eq_ignore_ascii_case("image"))
        }
        "button" | "fieldset" | "object" | "output" | "select" | "textarea" => true,
        _ => false,
    }
}

fn controls(dom: &Dom, form: NodeId) -> Vec<NodeId> {
    let nodes = tree_nodes(dom, form);
    let ids = first_ids(dom, &nodes);
    nodes
        .into_iter()
        .filter(|&node| listed_control(dom, node) && form_owner(dom, node, &ids) == Some(form))
        .collect()
}

impl Dom {
    /// Resolve HTML's form owner from the ordinary tree and the form attribute.
    pub fn form_owner(&self, node: NodeId) -> Option<NodeId> {
        let nodes = tree_nodes(self, node);
        form_owner(self, node, &first_ids(self, &nodes))
    }

    /// Return listed controls in tree order, excluding image-state inputs.
    pub fn form_controls(&self, form: NodeId) -> Vec<NodeId> {
        controls(self, form)
    }

    /// Read dirty checkedness, or the checked content attribute before a state write.
    pub fn input_checked(&self, node: NodeId) -> bool {
        self.checked_states.get(&node).map_or_else(
            || attribute(self, node, "checked").is_some(),
            |state| state.checked,
        )
    }

    /// Copy HTML input state independently of content attributes during cloning.
    pub fn copy_input_state(&mut self, source: NodeId, destination: NodeId) {
        if let Some(state) = self.checked_states.get(&source).copied() {
            self.checked_states.insert(destination, state);
        }
    }

    pub(crate) fn checked_attribute_changed(&mut self, node: NodeId, present: bool) {
        if tag(self, node) != "input"
            || self
                .checked_states
                .get(&node)
                .is_some_and(|state| state.dirty)
        {
            return;
        }
        let _ = self.set_input_checked(node, present);
        if let Some(state) = self.checked_states.get_mut(&node) {
            state.dirty = false;
        }
    }

    /// Set checkedness and clear other radios in the same form-owner and name group.
    pub fn set_input_checked(&mut self, node: NodeId, checked: bool) -> Result<bool, DomError> {
        if tag(self, node) != "input" {
            return Err(DomError::NotElement(node));
        }
        let mut changed = self.input_checked(node) != checked;
        self.checked_states.insert(
            node,
            CheckedState {
                checked,
                dirty: true,
            },
        );
        self.mark_dirty(node);
        let group = if checked {
            radio_group(self, node)
        } else {
            Vec::new()
        };
        for sibling in group {
            if sibling != node {
                changed |= self.input_checked(sibling);
                let dirty = self
                    .checked_states
                    .get(&sibling)
                    .is_some_and(|state| state.dirty);
                self.checked_states.insert(
                    sibling,
                    CheckedState {
                        checked: false,
                        dirty,
                    },
                );
                self.mark_dirty(sibling);
            }
        }
        if changed {
            self.record_style_change();
        }
        Ok(changed)
    }
}

fn radio_group(dom: &Dom, node: NodeId) -> Vec<NodeId> {
    if !attribute(dom, node, "type").is_some_and(|kind| kind.eq_ignore_ascii_case("radio")) {
        return Vec::new();
    }
    let Some(name) = attribute(dom, node, "name").filter(|name| !name.is_empty()) else {
        return Vec::new();
    };
    let nodes = tree_nodes(dom, node);
    let ids = first_ids(dom, &nodes);
    let owner = form_owner(dom, node, &ids);
    nodes
        .into_iter()
        .filter(|&candidate| {
            tag(dom, candidate) == "input"
                && attribute(dom, candidate, "type")
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("radio"))
                && attribute(dom, candidate, "name") == Some(name)
                && form_owner(dom, candidate, &ids) == owner
        })
        .collect()
}
