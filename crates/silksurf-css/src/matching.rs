/*
 * matching.rs -- CSS selector matching and specificity calculation.
 *
 * WHY: For each DOM node, the cascade must determine which CSS rules apply.
 * This module implements right-to-left selector matching (CSS Selectors L4)
 * and specificity computation (CSS Cascade L4 Section 6).
 *
 * Architecture:
 *   matches_selector: public API, no CascadeView (for external callers)
 *   matches_selector_with_view: internal, uses CascadeView when available
 *   matches_compound: tag + modifier checks per compound selector
 *   matches_class/matches_id: atom-based O(1) comparison via CascadeView
 *
 * CascadeView integration (SoA hot path):
 *   When a CascadeView is provided, matches_compound reads the 40-byte
 *   CascadeEntry instead of the 168-byte Node. Tag comparison uses
 *   entry.tag directly. Class/id matching uses pre-constructed
 *   SelectorIdents from the flat idents array. This eliminates all
 *   dom.node() and dom.attributes() calls from the cascade hot path.
 *
 *   Fallback to dom.node() occurs only for:
 *     - matches_attribute (arbitrary attribute selectors, rare)
 *     - matches_pseudo_class (needs DOM topology: parent/children/siblings)
 *     - External callers without CascadeView
 *
 * See: cascade_view.rs for the SoA layout
 * See: style.rs cascade_for_node() for the caller
 */
use crate::cascade_view::CascadeView;
use crate::{
    AttributeOperator, AttributeSelector, Combinator, CompoundSelector, PseudoClassArg, Selector,
    SelectorIdent, SelectorList, SelectorModifier, TypeSelector,
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
                SelectorModifier::FunctionalPseudoClass { name, arg } => {
                    // CSS Selectors L4: :where() contributes 0 specificity.
                    // :not(), :is(), :has() use the max specificity of their args.
                    // :nth-child() and structural functions count as one class.
                    let lower = name.as_str().to_ascii_lowercase();
                    match lower.as_str() {
                        "where" => {}
                        "not" | "is" | "has" => {
                            if let PseudoClassArg::SelectorList(list) = arg {
                                if let Some(max) =
                                    list.selectors.iter().map(selector_specificity).max()
                                {
                                    spec.ids += max.ids;
                                    spec.classes += max.classes;
                                    spec.elements += max.elements;
                                }
                            }
                        }
                        _ => spec.classes += 1,
                    }
                }
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
 * matches_selector -- public API, right-to-left matching without CascadeView.
 *
 * Used by external callers (e.g., querySelector). Falls back to dom.node()
 * for all DOM access. For the cascade hot path, use matches_selector_with_view.
 */
pub fn matches_selector(dom: &Dom, node: NodeId, selector: &Selector) -> bool {
    matches_selector_inner(dom, node, selector, None)
}

/*
 * matches_selector_with_view -- internal fast path using CascadeView.
 *
 * WHY: During the cascade, every matches_selector call previously fetched
 * the 168-byte Node via dom.node(). With CascadeView, tag/id/class matching
 * reads from the 40-byte CascadeEntry + flat SelectorIdent array instead.
 * This eliminates all dom.node() and dom.attributes() calls from the
 * matching hot path (except for rare attribute selectors and pseudo-classes).
 *
 * CascadeView is indexed by NodeId.raw(), so ancestor/sibling lookups
 * during combinator matching also use the SoA path -- no Node fetches
 * anywhere in the recursive match chain.
 */
pub(crate) fn matches_selector_with_view(
    dom: &Dom,
    node: NodeId,
    selector: &Selector,
    view: &CascadeView,
) -> bool {
    matches_selector_inner(dom, node, selector, Some(view))
}

fn matches_selector_inner(
    dom: &Dom,
    node: NodeId,
    selector: &Selector,
    view: Option<&CascadeView>,
) -> bool {
    let n = selector.steps.len();
    if n == 0 {
        return false;
    }
    matches_steps_rev(dom, node, &selector.steps, n - 1, view)
}

fn matches_steps_rev(
    dom: &Dom,
    node: NodeId,
    steps: &[crate::SelectorStep],
    from: usize,
    view: Option<&CascadeView>,
) -> bool {
    if !matches_compound(dom, node, &steps[from].compound, view) {
        return false;
    }
    if from == 0 {
        return true;
    }
    let combinator = match steps[from].combinator {
        Some(combinator) => combinator,
        None => return false,
    };
    // Helper: get parent via CascadeView (flat array) or dom.parent() (168-byte Node).
    let get_parent = |n: NodeId| -> Option<NodeId> {
        if let Some(v) = view {
            let idx = n.raw();
            if idx < v.entries.len() {
                return v.parent_of(&v.entries[idx]);
            }
        }
        dom.parent(n).ok().flatten()
    };

    match combinator {
        Combinator::Descendant => {
            let mut current = get_parent(node);
            while let Some(ancestor) = current {
                if matches_steps_rev(dom, ancestor, steps, from - 1, view) {
                    return true;
                }
                current = get_parent(ancestor);
            }
            false
        }
        Combinator::Child => get_parent(node)
            .is_some_and(|parent| matches_steps_rev(dom, parent, steps, from - 1, view)),
        Combinator::NextSibling => previous_element_sibling(dom, node)
            .is_some_and(|sibling| matches_steps_rev(dom, sibling, steps, from - 1, view)),
        Combinator::SubsequentSibling => previous_element_siblings(dom, node)
            .any(|sibling| matches_steps_rev(dom, sibling, steps, from - 1, view)),
    }
}

/*
 * matches_compound -- check a compound selector against a DOM node.
 *
 * WHY two paths:
 *   CascadeView path: reads CascadeEntry.tag (24 bytes) for type selector,
 *   then checks modifiers via SoA idents. No dom.node() fetch (168 bytes).
 *   Fallback path: dom.node() for tag, dom.attributes() for modifiers.
 *
 * The CascadeView path is used during cascade (hot). The fallback path
 * is used by external callers and for nodes outside CascadeView bounds.
 */
fn matches_compound(
    dom: &Dom,
    node: NodeId,
    compound: &CompoundSelector,
    view: Option<&CascadeView>,
) -> bool {
    // CascadeView fast path: read 40-byte entry instead of 168-byte Node
    if let Some(view) = view {
        let idx = node.raw();
        if idx < view.entries.len() {
            let entry = &view.entries[idx];
            if let Some(type_selector) = &compound.type_selector {
                match type_selector {
                    TypeSelector::Any => {}
                    TypeSelector::Tag(expected) => {
                        if &entry.tag != expected {
                            return false;
                        }
                    }
                }
            }
            for modifier in &compound.modifiers {
                if !matches_modifier_with_view(dom, node, modifier, view, entry) {
                    return false;
                }
            }
            return true;
        }
    }

    // Fallback: fetch full Node from DOM
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

/*
 * matches_modifier_with_view -- SoA modifier matching via CascadeView.
 *
 * Class: O(classes) atom comparison against pre-constructed SelectorIdents.
 * Id: O(1) SelectorIdent comparison against entry's id ident.
 * Attribute/PseudoClass: falls back to dom (rare, needs topology/raw attrs).
 */
fn matches_modifier_with_view(
    dom: &Dom,
    node: NodeId,
    modifier: &SelectorModifier,
    view: &CascadeView,
    entry: &crate::cascade_view::CascadeEntry,
) -> bool {
    match modifier {
        SelectorModifier::Class(name) => {
            let class_idents = view.class_idents(entry);
            class_idents.iter().any(|ident| ident == name)
        }
        SelectorModifier::Id(name) => view.id_ident(entry).is_some_and(|ident| ident == name),
        // Attribute selectors and pseudo-classes need raw DOM access
        SelectorModifier::Attribute(attribute) => matches_attribute(dom, node, attribute),
        SelectorModifier::PseudoClass(name) => matches_pseudo_class(dom, node, name),
        SelectorModifier::FunctionalPseudoClass { name, arg } => {
            matches_functional_pseudo_class(dom, node, name, arg)
        }
    }
}

fn matches_modifier(dom: &Dom, node: NodeId, modifier: &SelectorModifier) -> bool {
    match modifier {
        SelectorModifier::Class(name) => matches_class(dom, node, name),
        SelectorModifier::Id(name) => matches_id(dom, node, name),
        SelectorModifier::Attribute(attribute) => matches_attribute(dom, node, attribute),
        SelectorModifier::PseudoClass(name) => matches_pseudo_class(dom, node, name),
        SelectorModifier::FunctionalPseudoClass { name, arg } => {
            matches_functional_pseudo_class(dom, node, name, arg)
        }
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
        // DOM-topology pseudo-classes
        "root" => is_root(dom, node),
        "empty" => is_empty(dom, node),
        "first-child" => is_first_child(dom, node),
        "last-child" => is_last_child(dom, node),
        "only-child" => is_only_child(dom, node),
        "first-of-type" => is_first_of_type(dom, node),
        "last-of-type" => is_last_of_type(dom, node),
        "only-of-type" => is_first_of_type(dom, node) && is_last_of_type(dom, node),
        // Interaction-state pseudo-classes: static renderer has no hover/focus
        // state, so :hover, :focus, :active, :focus-visible, :focus-within,
        // and :target all evaluate to false. CSS that uses these exclusively
        // for cosmetic enhancement (e.g. hover colour change) is unaffected
        // in the rendered layout; only the enhancement is absent.
        "hover" | "focus" | "active" | "focus-visible" | "focus-within" | "target"
        | "local-link" => false,
        // :visited: we have no navigation history in a static renderer.
        "visited" => false,
        // :any-link / :link: true when the element is an anchor, area, or link
        // element with an href attribute present.
        "any-link" | "link" => {
            let tag = dom.element_name(node).ok().flatten().unwrap_or("").to_ascii_lowercase();
            matches!(tag.as_str(), "a" | "area" | "link")
                && has_attr(dom, node, "href")
        }
        // :disabled / :enabled: reflect the disabled attribute on form elements.
        "disabled" => has_attr(dom, node, "disabled"),
        "enabled" => !has_attr(dom, node, "disabled"),
        // :checked: true when input[type=checkbox|radio] has the checked attr,
        // or <option> has the selected attribute.
        "checked" => {
            let tag = dom.element_name(node).ok().flatten().unwrap_or("").to_ascii_lowercase();
            match tag.as_str() {
                "input" => has_attr(dom, node, "checked"),
                "option" => has_attr(dom, node, "selected"),
                _ => false,
            }
        }
        // :indeterminate: only meaningful for checkboxes set via JS; false
        // in a static renderer with no script execution.
        "indeterminate" => false,
        // :required / :optional: reflect the required attribute.
        "required" => has_attr(dom, node, "required"),
        "optional" => {
            let tag = dom.element_name(node).ok().flatten().unwrap_or("").to_ascii_lowercase();
            matches!(tag.as_str(), "input" | "select" | "textarea")
                && !has_attr(dom, node, "required")
        }
        // :read-only / :read-write: reflect the readonly attribute.
        // Elements without a relevant tag are read-only by default in CSS.
        "read-write" => {
            let tag = dom.element_name(node).ok().flatten().unwrap_or("").to_ascii_lowercase();
            matches!(tag.as_str(), "input" | "textarea") && !has_attr(dom, node, "readonly")
        }
        "read-only" => {
            let tag = dom.element_name(node).ok().flatten().unwrap_or("").to_ascii_lowercase();
            !matches!(tag.as_str(), "input" | "textarea") || has_attr(dom, node, "readonly")
        }
        // :placeholder-shown: true when a form element displays its placeholder.
        // Static renderer has no value; assume placeholder shows when present.
        "placeholder-shown" => has_attr(dom, node, "placeholder"),
        // :scope: true for the root element in a stylesheet (same as :root
        // in a non-scoped context).
        "scope" => is_root(dom, node),
        // :defined: always true; custom elements are considered defined in a
        // static renderer with no custom-element registry.
        "defined" => true,
        // Form-validation pseudo-classes: no constraint validation in a static
        // renderer; default to false so validation styles don't appear.
        "valid" | "invalid" | "in-range" | "out-of-range" | "user-valid"
        | "user-invalid" => false,
        // :playing / :paused: media element playback; false in static render.
        "playing" | "paused" => false,
        _ => false,
    }
}

/// Return true when the DOM node has an attribute whose name (case-insensitive)
/// matches `attr_name`. Presence is sufficient; value is irrelevant.
fn has_attr(dom: &Dom, node: NodeId, attr_name: &str) -> bool {
    dom.attributes(node)
        .map(|attrs| attrs.iter().any(|a| a.name.matches(attr_name)))
        .unwrap_or(false)
}

fn matches_functional_pseudo_class(
    dom: &Dom,
    node: NodeId,
    name: &SelectorIdent,
    arg: &PseudoClassArg,
) -> bool {
    let lower = name.as_str().to_ascii_lowercase();
    match lower.as_str() {
        "nth-child" => {
            matches!(arg, PseudoClassArg::Nth(nth) if nth.matches(element_child_index(dom, node)))
        }
        "nth-last-child" => {
            matches!(arg, PseudoClassArg::Nth(nth) if nth.matches(element_child_index_from_end(dom, node)))
        }
        "nth-of-type" => {
            matches!(arg, PseudoClassArg::Nth(nth) if nth.matches(element_child_index_of_type(dom, node)))
        }
        "nth-last-of-type" => {
            matches!(arg, PseudoClassArg::Nth(nth) if nth.matches(element_child_index_of_type_from_end(dom, node)))
        }
        "not" => match arg {
            PseudoClassArg::SelectorList(list) => !matches_selector_list(dom, node, list),
            _ => false,
        },
        "is" | "where" => match arg {
            PseudoClassArg::SelectorList(list) => matches_selector_list(dom, node, list),
            _ => false,
        },
        "has" => match arg {
            PseudoClassArg::SelectorList(list) => matches_has(dom, node, list),
            _ => false,
        },
        _ => false,
    }
}

// :has() -- true when any descendant of node matches the selector list.
fn matches_has(dom: &Dom, node: NodeId, list: &SelectorList) -> bool {
    let children = match dom.children(node).ok() {
        Some(c) => c,
        None => return false,
    };
    for child in children {
        if matches_selector_list(dom, *child, list) {
            return true;
        }
        if matches_has(dom, *child, list) {
            return true;
        }
    }
    false
}

// Returns 1-based position of node among element siblings from the start.
fn element_child_index(dom: &Dom, node: NodeId) -> usize {
    let parent = match dom.parent(node).ok().flatten() {
        Some(p) => p,
        None => return 0,
    };
    let siblings = match dom.children(parent).ok() {
        Some(s) => s,
        None => return 0,
    };
    let mut index = 0usize;
    for sibling in siblings {
        if dom.element_name(*sibling).ok().flatten().is_some() {
            index += 1;
        }
        if *sibling == node {
            return index;
        }
    }
    0
}

// Returns 1-based position of node among element siblings from the end.
fn element_child_index_from_end(dom: &Dom, node: NodeId) -> usize {
    let parent = match dom.parent(node).ok().flatten() {
        Some(p) => p,
        None => return 0,
    };
    let siblings = match dom.children(parent).ok() {
        Some(s) => s,
        None => return 0,
    };
    let mut index = 0usize;
    for sibling in siblings.iter().rev() {
        if dom.element_name(*sibling).ok().flatten().is_some() {
            index += 1;
        }
        if *sibling == node {
            return index;
        }
    }
    0
}

// Returns 1-based position of node among siblings with the same tag, from start.
fn element_child_index_of_type(dom: &Dom, node: NodeId) -> usize {
    let tag = match dom.element_name(node).ok().flatten() {
        Some(t) => t.to_owned(),
        None => return 0,
    };
    let parent = match dom.parent(node).ok().flatten() {
        Some(p) => p,
        None => return 0,
    };
    let siblings = match dom.children(parent).ok() {
        Some(s) => s,
        None => return 0,
    };
    let mut index = 0usize;
    for sibling in siblings {
        let same_tag = dom
            .element_name(*sibling)
            .ok()
            .flatten()
            .is_some_and(|t| t == tag);
        if same_tag {
            index += 1;
        }
        if *sibling == node {
            return index;
        }
    }
    0
}

// Returns 1-based position of node among siblings with the same tag, from end.
fn element_child_index_of_type_from_end(dom: &Dom, node: NodeId) -> usize {
    let tag = match dom.element_name(node).ok().flatten() {
        Some(t) => t.to_owned(),
        None => return 0,
    };
    let parent = match dom.parent(node).ok().flatten() {
        Some(p) => p,
        None => return 0,
    };
    let siblings = match dom.children(parent).ok() {
        Some(s) => s,
        None => return 0,
    };
    let mut index = 0usize;
    for sibling in siblings.iter().rev() {
        let same_tag = dom
            .element_name(*sibling)
            .ok()
            .flatten()
            .is_some_and(|t| t == tag);
        if same_tag {
            index += 1;
        }
        if *sibling == node {
            return index;
        }
    }
    0
}

fn is_first_of_type(dom: &Dom, node: NodeId) -> bool {
    element_child_index_of_type(dom, node) == 1
}

fn is_last_of_type(dom: &Dom, node: NodeId) -> bool {
    element_child_index_of_type_from_end(dom, node) == 1
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
