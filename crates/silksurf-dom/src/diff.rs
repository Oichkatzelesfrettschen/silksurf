/*
 * diff.rs -- structural DOM diffing for incremental re-render.
 *
 * WHY: After a background revalidation returns 200 (new content), re-rendering
 * the entire page wastes work when only a small number of nodes changed. This
 * module computes the minimal change set (added / removed / changed nodes)
 * by comparing two DOM snapshots structurally.
 *
 * Perturbation analogy (from the plan): the cached render is f_eq (equilibrium).
 * The diff result is h (perturbation). We only need to reprocess nodes in h.
 * For 304 Not Modified: h is empty, re-render cost is zero.
 * For minor content updates: h is a small set, re-render is proportional.
 *
 * Algorithm: recursive positional tree diffing.
 *   - Match nodes by position (parent + sibling index)
 *   - If node kinds differ: entire old subtree removed, new subtree added
 *   - If element tags differ: same (structural incompatibility)
 *   - If element tags match: compare attributes, recurse into children
 *   - If text nodes: compare text content
 *
 * Complexity: O(N) where N = total node count of the larger DOM.
 * Memory: O(depth) call stack + O(changes) for DomDiff output.
 *
 * Limitation: this is positional diffing, not key-based (no "key" attribute).
 * Insertions/deletions at the top of a list will appear as changes to all
 * subsequent siblings. For chatgpt.com (static server-rendered HTML with
 * minor content updates), positional diffing is correct and efficient.
 *
 * See: SpeculativeRenderer in silksurf-engine/src/speculative.rs for call site
 * See: fused_pipeline.rs for the incremental re-render target (Phase E.2)
 */

use crate::{Dom, NodeId, NodeKind};

/*
 * ChangeKind -- why a node appears in DomDiff::changed.
 *
 * TextContent: text in a Text node changed (most common for content updates).
 * Attributes: one or more attributes changed on an Element node.
 * Both: text and attributes changed (rare, but possible for elements with
 * text children whose sibling attributes also changed).
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// Text content of a Text node changed.
    TextContent,
    /// One or more attributes of an Element node changed.
    Attributes,
    /// Both text content (in children) and attributes changed.
    Both,
}

/*
 * DomDiff -- minimal change set between two DOM snapshots.
 *
 * NodeIds in `changed` and `removed` refer to nodes in the OLD dom.
 * NodeIds in `added` refer to nodes in the NEW dom.
 *
 * WHY separate old/new dom IDs: NodeIds are raw indices into their respective
 * Dom's nodes Vec; they are NOT stable across different Dom instances.
 * Callers must use the right Dom to look up each set of IDs.
 *
 * changed: in-place mutations -- same position, different content.
 * added:   nodes in new_dom with no positional counterpart in old_dom.
 * removed: nodes in old_dom with no positional counterpart in new_dom.
 */
#[derive(Debug, Default)]
pub struct DomDiff {
    /// Nodes that changed in place (text or attributes). IDs from OLD dom.
    pub changed: Vec<(NodeId, ChangeKind)>,
    /// Nodes added in the new dom. IDs from NEW dom.
    pub added: Vec<NodeId>,
    /// Nodes removed from the old dom. IDs from OLD dom.
    pub removed: Vec<NodeId>,
}

impl DomDiff {
    /// True if the two DOMs are identical (no changes, additions, or removals).
    #[must_use] 
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.added.is_empty() && self.removed.is_empty()
    }

    /// Total number of changed, added, and removed nodes.
    #[must_use] 
    pub fn total_changes(&self) -> usize {
        self.changed.len() + self.added.len() + self.removed.len()
    }
}

/*
 * diff_doms -- compare two DOM trees rooted at old_root / new_root.
 *
 * Returns a DomDiff describing what changed between them. The caller should
 * use `diff.is_empty()` to fast-path a no-op re-render (304 Not Modified
 * should produce an identical DOM, so diff should be empty).
 *
 * Complexity: O(N) -- one visit per node in the larger of the two DOMs.
 *
 * See: diff_subtree for the recursive implementation
 */
pub fn diff_doms(old_dom: &Dom, old_root: NodeId, new_dom: &Dom, new_root: NodeId) -> DomDiff {
    let mut result = DomDiff::default();
    diff_subtree(old_dom, old_root, new_dom, new_root, &mut result);
    result
}

/*
 * diff_subtree -- recursive positional tree comparison.
 *
 * Compares old_node (from old_dom) with new_node (from new_dom) at the same
 * tree position. Recurses into children only when the node kinds match.
 *
 * When kinds differ (e.g. Element vs Text at the same position): mark the
 * entire old subtree as removed and the entire new subtree as added.
 * This is correct for positional diffing: a kind change at position P means
 * the subtrees at P are structurally incompatible.
 *
 * Complexity: O(N_subtree) per call, total O(N) across all recursive calls.
 */
fn diff_subtree(
    old_dom: &Dom,
    old_node: NodeId,
    new_dom: &Dom,
    new_node: NodeId,
    result: &mut DomDiff,
) {
    let old_kind = match old_dom.node(old_node) {
        Ok(n) => n.kind().clone(),
        Err(_) => {
            // Old node doesn't exist; entire new subtree is added.
            collect_subtree_ids(new_dom, new_node, &mut result.added);
            return;
        }
    };
    let new_kind = match new_dom.node(new_node) {
        Ok(n) => n.kind().clone(),
        Err(_) => {
            // New node doesn't exist; entire old subtree is removed.
            collect_subtree_ids(old_dom, old_node, &mut result.removed);
            return;
        }
    };

    match (&old_kind, &new_kind) {
        /*
         * Document nodes: just recurse into children.
         * There is always exactly one Document at the root.
         */
        (NodeKind::Document, NodeKind::Document) => {
            diff_children(old_dom, old_node, new_dom, new_node, result);
        }

        /*
         * Doctype: compare names. Changes are rare (pages don't change doctype).
         */
        (NodeKind::Doctype { name: old_name, .. }, NodeKind::Doctype { name: new_name, .. }) => {
            if old_name != new_name {
                result.changed.push((old_node, ChangeKind::Attributes));
            }
        }

        /*
         * Text nodes: compare content.
         *
         * WHY text nodes are the most common change: server-rendered pages
         * update timestamps, counters, user data -- all expressed as text.
         */
        (NodeKind::Text { text: old_text }, NodeKind::Text { text: new_text }) => {
            if old_text != new_text {
                result.changed.push((old_node, ChangeKind::TextContent));
            }
        }

        /*
         * Comment nodes: compare data. Changes are very rare.
         */
        (NodeKind::Comment { data: old_data }, NodeKind::Comment { data: new_data }) => {
            if old_data != new_data {
                result.changed.push((old_node, ChangeKind::TextContent));
            }
        }

        /*
         * Element nodes: compare tag, then attributes, then children.
         *
         * If the tag changed: the subtrees are structurally incompatible --
         * mark old as removed and new as added (no point diffing children).
         *
         * If the tag matches: compare attributes for changes, then recurse
         * into children.
         */
        (NodeKind::Element { name: old_tag, .. }, NodeKind::Element { name: new_tag, .. }) => {
            if old_tag != new_tag {
                // Structurally incompatible: treat as full replacement.
                collect_subtree_ids(old_dom, old_node, &mut result.removed);
                collect_subtree_ids(new_dom, new_node, &mut result.added);
                return;
            }

            // Compare attributes.
            let attrs_changed = attributes_changed(old_dom, old_node, new_dom, new_node);
            if attrs_changed {
                result.changed.push((old_node, ChangeKind::Attributes));
            }

            diff_children(old_dom, old_node, new_dom, new_node, result);
        }

        /*
         * Kind mismatch (e.g. old=Text, new=Element at same position).
         * Structurally incompatible: mark old subtree removed, new subtree added.
         */
        _ => {
            collect_subtree_ids(old_dom, old_node, &mut result.removed);
            collect_subtree_ids(new_dom, new_node, &mut result.added);
        }
    }
}

/*
 * diff_children -- positional comparison of two nodes' children lists.
 *
 * Iterates both child lists in parallel. When one list is longer than the
 * other: extra old children are removed, extra new children are added.
 *
 * Complexity: O(max(|old_children|, |new_children|)) per call.
 */
fn diff_children(
    old_dom: &Dom,
    old_parent: NodeId,
    new_dom: &Dom,
    new_parent: NodeId,
    result: &mut DomDiff,
) {
    let old_children = match old_dom.children(old_parent) {
        Ok(c) => c.to_vec(),
        Err(_) => vec![],
    };
    let new_children = match new_dom.children(new_parent) {
        Ok(c) => c.to_vec(),
        Err(_) => vec![],
    };

    let common = old_children.len().min(new_children.len());

    // Recurse into children at matching positions.
    for i in 0..common {
        diff_subtree(old_dom, old_children[i], new_dom, new_children[i], result);
    }

    // Old DOM has more children: mark extras as removed.
    for &old_child in &old_children[common..] {
        collect_subtree_ids(old_dom, old_child, &mut result.removed);
    }

    // New DOM has more children: mark extras as added.
    for &new_child in &new_children[common..] {
        collect_subtree_ids(new_dom, new_child, &mut result.added);
    }
}

/*
 * attributes_changed -- return true if old_node and new_node have different attributes.
 *
 * Comparison is order-independent (uses set-like contains check): attributes
 * are considered equal if every (name, value) in old appears in new and vice versa.
 *
 * WHY order-independent: HTML parsers may not preserve attribute order.
 *
 * Complexity: O(N_attrs^2) in the worst case, but attributes are typically
 * small in number (< 10), so this is effectively O(1) per node.
 */
fn attributes_changed(old_dom: &Dom, old_node: NodeId, new_dom: &Dom, new_node: NodeId) -> bool {
    let Ok(old_attrs) = old_dom.attributes(old_node) else { return false; };
    let Ok(new_attrs) = new_dom.attributes(new_node) else { return false; };

    if old_attrs.len() != new_attrs.len() {
        return true;
    }

    for old_attr in old_attrs {
        let found = new_attrs
            .iter()
            .any(|new_attr| new_attr.name == old_attr.name && new_attr.value == old_attr.value);
        if !found {
            return true;
        }
    }

    false
}

/*
 * collect_subtree_ids -- collect all NodeIds in a subtree (root-inclusive).
 *
 * WHY: when a subtree is structurally incompatible with the other DOM, we
 * mark every node in it as added or removed rather than recursing further.
 * This gives the caller a complete picture of what was gained or lost.
 *
 * Complexity: O(N_subtree). Uses an explicit stack to avoid deep recursion
 * on large DOMs (chatgpt.com ~400 nodes -> max depth ~20).
 */
fn collect_subtree_ids(dom: &Dom, root: NodeId, out: &mut Vec<NodeId>) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        out.push(node);
        if let Ok(children) = dom.children(node) {
            for &child in children.iter().rev() {
                stack.push(child);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Dom;

    fn build_simple_dom() -> (Dom, NodeId) {
        let mut dom = Dom::new();
        let doc = dom.create_document();
        let body = dom.create_element("body");
        dom.append_child(doc, body).unwrap();
        let para = dom.create_element("p");
        dom.append_child(body, para).unwrap();
        let text = dom.create_text("Hello");
        dom.append_child(para, text).unwrap();
        (dom, doc)
    }

    #[test]
    fn test_identical_doms_produce_empty_diff() {
        let (old_dom, old_root) = build_simple_dom();
        let (new_dom, new_root) = build_simple_dom();
        let diff = diff_doms(&old_dom, old_root, &new_dom, new_root);
        assert!(diff.is_empty(), "identical DOMs must produce empty diff");
    }

    #[test]
    fn test_text_change_detected() {
        let (old_dom, old_root) = build_simple_dom();
        let mut new_dom = Dom::new();
        let doc = new_dom.create_document();
        let body = new_dom.create_element("body");
        new_dom.append_child(doc, body).unwrap();
        let para = new_dom.create_element("p");
        new_dom.append_child(body, para).unwrap();
        let text = new_dom.create_text("World"); // changed
        new_dom.append_child(para, text).unwrap();

        let diff = diff_doms(&old_dom, old_root, &new_dom, doc);
        assert!(!diff.is_empty());
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].1, ChangeKind::TextContent);
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
    }

    #[test]
    fn test_added_node_detected() {
        let (old_dom, old_root) = build_simple_dom();
        let mut new_dom = Dom::new();
        let doc = new_dom.create_document();
        let body = new_dom.create_element("body");
        new_dom.append_child(doc, body).unwrap();
        let para = new_dom.create_element("p");
        new_dom.append_child(body, para).unwrap();
        let text = new_dom.create_text("Hello");
        new_dom.append_child(para, text).unwrap();
        // Extra element added
        let extra = new_dom.create_element("span");
        new_dom.append_child(body, extra).unwrap();

        let diff = diff_doms(&old_dom, old_root, &new_dom, doc);
        assert!(!diff.is_empty());
        assert!(
            !diff.added.is_empty(),
            "new span should be detected as added"
        );
        assert!(diff.changed.is_empty());
    }

    #[test]
    fn test_removed_node_detected() {
        let (old_dom, old_root) = build_simple_dom();
        // New DOM has body but no children
        let mut new_dom = Dom::new();
        let doc = new_dom.create_document();
        let body = new_dom.create_element("body");
        new_dom.append_child(doc, body).unwrap();

        let diff = diff_doms(&old_dom, old_root, &new_dom, doc);
        assert!(!diff.is_empty());
        assert!(
            !diff.removed.is_empty(),
            "old children should be detected as removed"
        );
        assert!(diff.added.is_empty());
    }

    #[test]
    fn test_total_changes_counts_all() {
        let (old_dom, old_root) = build_simple_dom();
        // New DOM with text change and an extra node
        let mut new_dom = Dom::new();
        let doc = new_dom.create_document();
        let body = new_dom.create_element("body");
        new_dom.append_child(doc, body).unwrap();
        let para = new_dom.create_element("p");
        new_dom.append_child(body, para).unwrap();
        let text = new_dom.create_text("Changed");
        new_dom.append_child(para, text).unwrap();
        let extra = new_dom.create_element("div");
        new_dom.append_child(body, extra).unwrap();

        let diff = diff_doms(&old_dom, old_root, &new_dom, doc);
        assert_eq!(
            diff.total_changes(),
            diff.changed.len() + diff.added.len() + diff.removed.len()
        );
        assert!(diff.total_changes() > 0);
    }
}
