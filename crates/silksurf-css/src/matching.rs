use crate::{
    AttributeOperator, AttributeSelector, Combinator, CompoundSelector, Selector, SelectorIdent,
    SelectorList, SelectorModifier, TypeSelector,
};
use silksurf_dom::{Attribute, AttributeName, Dom, NodeId, NodeKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity {
    pub ids: u32,
    pub classes: u32,
    pub elements: u32,
}

impl Specificity {
    pub fn zero() -> Self {
        Self {
            ids: 0,
            classes: 0,
            elements: 0,
        }
    }
}

pub fn selector_specificity(selector: &Selector) -> Specificity {
    let mut spec = Specificity::zero();
    for step in &selector.steps {
        if let Some(type_selector) = &step.compound.type_selector {
            if let TypeSelector::Tag(_) = type_selector {
                spec.elements += 1;
            }
        }
        for modifier in &step.compound.modifiers {
            match modifier {
                SelectorModifier::Id(_) => spec.ids += 1,
                SelectorModifier::Class(_)
                | SelectorModifier::Attribute(_)
                | SelectorModifier::PseudoClass(_) => spec.classes += 1,
            }
        }
    }
    spec
}

pub fn matches_selector_list(dom: &Dom, node: NodeId, list: &SelectorList) -> bool {
    list.selectors
        .iter()
        .any(|selector| matches_selector(dom, node, selector))
}

/*
 * matches_selector -- right-to-left CSS selector matching, zero allocation.
 *
 * WHY: CSS selectors match right-to-left (rightmost compound first, then
 * walk up the tree checking combinators). The previous implementation
 * allocated a Vec and called reverse() on EVERY matches_selector call.
 * For ~427 calls per render, that was ~8.5us of heap allocation -- 70%
 * of the 12us ws.run() budget -- hidden inside the matching hot path.
 *
 * FIX: Use reverse index arithmetic. steps[from] is the rightmost step
 * (from = steps.len() - 1), walking toward steps[0] (leftmost).
 * Zero heap allocation. Same semantic behavior as the Vec+reverse path.
 *
 * Complexity: O(steps * ancestors) worst case for descendant combinators
 */
pub fn matches_selector(dom: &Dom, node: NodeId, selector: &Selector) -> bool {
    let n = selector.steps.len();
    if n == 0 {
        return false;
    }
    matches_steps_rev(dom, node, &selector.steps, n - 1)
}

fn matches_steps_rev(
    dom: &Dom,
    node: NodeId,
    steps: &[crate::SelectorStep],
    from: usize,
) -> bool {
    if !matches_compound(dom, node, &steps[from].compound) {
        return false;
    }
    if from == 0 {
        return true;
    }
    let combinator = match steps[from].combinator {
        Some(combinator) => combinator,
        None => return false,
    };
    match combinator {
        Combinator::Descendant => {
            let mut current = dom.parent(node).ok().flatten();
            while let Some(ancestor) = current {
                if matches_steps_rev(dom, ancestor, steps, from - 1) {
                    return true;
                }
                current = dom.parent(ancestor).ok().flatten();
            }
            false
        }
        Combinator::Child => dom
            .parent(node)
            .ok()
            .flatten()
            .is_some_and(|parent| matches_steps_rev(dom, parent, steps, from - 1)),
        Combinator::NextSibling => previous_element_sibling(dom, node)
            .is_some_and(|sibling| matches_steps_rev(dom, sibling, steps, from - 1)),
        Combinator::SubsequentSibling => previous_element_siblings(dom, node)
            .any(|sibling| matches_steps_rev(dom, sibling, steps, from - 1)),
    }
}

fn matches_compound(dom: &Dom, node: NodeId, compound: &CompoundSelector) -> bool {
    let name = match dom.node(node).ok().map(|node| node.kind()) {
        Some(NodeKind::Element { name, .. }) => name,
        _ => return false,
    };
    if let Some(type_selector) = &compound.type_selector {
        match type_selector {
            TypeSelector::Any => {}
            TypeSelector::Tag(expected) => {
                if name != expected {
                    return false;
                }
            }
        }
    }
    for modifier in &compound.modifiers {
        if !matches_modifier(dom, node, modifier) {
            return false;
        }
    }
    true
}
fn matches_modifier(dom: &Dom, node: NodeId, modifier: &SelectorModifier) -> bool {
    match modifier {
        SelectorModifier::Class(name) => matches_class(dom, node, name),
        SelectorModifier::Id(name) => matches_id(dom, node, name),
        SelectorModifier::Attribute(attribute) => matches_attribute(dom, node, attribute),
        SelectorModifier::PseudoClass(name) => matches_pseudo_class(dom, node, name),
    }
}

fn matches_attribute(dom: &Dom, node: NodeId, attribute: &AttributeSelector) -> bool {
    let attr = match attribute_record(dom, node, &attribute.name) {
        Some(attr) => attr,
        None => return false,
    };
    let operator = match attribute.operator {
        Some(operator) => operator,
        None => return true,
    };
    let expected = match attribute.value.as_ref() {
        Some(expected) => expected,
        None => return false,
    };
    if let (Some(atom), Some(value_atom)) = (expected.atom(), attr.value_atom) {
        if atom == value_atom {
            return true;
        }
    }
    let value = attr.value.as_str();
    let expected = expected.as_str();
    match operator {
        AttributeOperator::Equals => value == expected,
        AttributeOperator::Includes => value.split_whitespace().any(|part| part == expected),
        AttributeOperator::DashMatch => {
            value == expected || value.starts_with(&format!("{}-", expected))
        }
        AttributeOperator::PrefixMatch => value.starts_with(expected),
        AttributeOperator::SuffixMatch => value.ends_with(expected),
        AttributeOperator::SubstringMatch => value.contains(expected),
    }
}

fn attribute_record<'a>(dom: &'a Dom, node: NodeId, name: &AttributeName) -> Option<&'a Attribute> {
    let attrs = dom.attributes(node).ok()?;
    attrs.iter().find(|attr| attr.name == *name)
}

fn matches_id(dom: &Dom, node: NodeId, name: &SelectorIdent) -> bool {
    let attrs = match dom.attributes(node) {
        Ok(attrs) => attrs,
        Err(_) => return false,
    };
    let Some(attr) = attrs.iter().find(|attr| attr.name == AttributeName::Id) else {
        return false;
    };
    if let (Some(atom), Some(value_atom)) = (name.atom(), attr.value_atom) {
        if atom == value_atom {
            return true;
        }
    }
    attr.value.as_str() == name.as_str()
}

fn matches_class(dom: &Dom, node: NodeId, name: &SelectorIdent) -> bool {
    let attrs = match dom.attributes(node) {
        Ok(attrs) => attrs,
        Err(_) => return false,
    };
    let Some(attr) = attrs.iter().find(|attr| attr.name == AttributeName::Class) else {
        return false;
    };
    if let Some(atom) = name.atom() {
        if attr.value_atoms.iter().any(|value| *value == atom) {
            return true;
        }
    }
    attr.value
        .as_str()
        .split_whitespace()
        .any(|part| part == name.as_str())
}
fn matches_pseudo_class(dom: &Dom, node: NodeId, name: &SelectorIdent) -> bool {
    let lower = name.as_str().to_ascii_lowercase();
    match lower.as_str() {
        "root" => is_root(dom, node),
        "empty" => is_empty(dom, node),
        "first-child" => is_first_child(dom, node),
        "last-child" => is_last_child(dom, node),
        "only-child" => is_only_child(dom, node),
        _ => false,
    }
}

fn is_root(dom: &Dom, node: NodeId) -> bool {
    let parent = match dom.parent(node).ok().flatten() {
        Some(parent) => parent,
        None => return false,
    };
    dom.node(parent)
        .map(|node| matches!(node.kind(), NodeKind::Document))
        .unwrap_or(false)
}

fn is_empty(dom: &Dom, node: NodeId) -> bool {
    dom.children(node)
        .map(|children| children.is_empty())
        .unwrap_or(false)
}

fn is_first_child(dom: &Dom, node: NodeId) -> bool {
    dom.parent(node)
        .ok()
        .flatten()
        .and_then(|parent| first_element_child(dom, parent))
        .is_some_and(|first| first == node)
}

fn is_last_child(dom: &Dom, node: NodeId) -> bool {
    dom.parent(node)
        .ok()
        .flatten()
        .and_then(|parent| last_element_child(dom, parent))
        .is_some_and(|last| last == node)
}

fn is_only_child(dom: &Dom, node: NodeId) -> bool {
    dom.parent(node)
        .ok()
        .flatten()
        .and_then(|parent| {
            let first = first_element_child(dom, parent)?;
            let last = last_element_child(dom, parent)?;
            Some((first, last))
        })
        .is_some_and(|(first, last)| first == node && last == node)
}

fn first_element_child(dom: &Dom, parent: NodeId) -> Option<NodeId> {
    let children = dom.children(parent).ok()?;
    children
        .iter()
        .copied()
        .find(|child| dom.element_name(*child).ok().flatten().is_some())
}

fn last_element_child(dom: &Dom, parent: NodeId) -> Option<NodeId> {
    let children = dom.children(parent).ok()?;
    children
        .iter()
        .rev()
        .copied()
        .find(|child| dom.element_name(*child).ok().flatten().is_some())
}

fn previous_element_sibling(dom: &Dom, node: NodeId) -> Option<NodeId> {
    let parent = dom.parent(node).ok().flatten()?;
    let siblings = dom.children(parent).ok()?;
    let mut previous = None;
    for sibling in siblings {
        if *sibling == node {
            break;
        }
        if dom.element_name(*sibling).ok().flatten().is_some() {
            previous = Some(*sibling);
        }
    }
    previous
}

fn previous_element_siblings<'a>(dom: &'a Dom, node: NodeId) -> impl Iterator<Item = NodeId> + 'a {
    let siblings = dom
        .parent(node)
        .ok()
        .flatten()
        .and_then(|parent| dom.children(parent).ok())
        .unwrap_or(&[]);

    let pos = siblings.iter().position(|&s| s == node).unwrap_or(0);

    siblings[..pos]
        .iter()
        .rev()
        .copied()
        .filter(move |&child| dom.element_name(child).ok().flatten().is_some())
}
