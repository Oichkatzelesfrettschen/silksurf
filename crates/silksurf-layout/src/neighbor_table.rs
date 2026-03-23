/*
 * neighbor_table.rs -- pre-computed BFS-level tree traversal for layout.
 *
 * WHY: Recursive tree traversal (DFS) for layout has poor cache locality
 * because siblings are scattered in memory. BFS-level traversal processes
 * all nodes at the same tree depth together, enabling:
 *   1. Sequential memory access per level (cache-friendly)
 *   2. Parallel processing of nodes within a level (no cross-level deps)
 *   3. Pre-computed parent indices for O(1) parent lookup
 *
 * Inspired by the gororoba NeighborTable which pre-computes cell adjacency
 * to eliminate modular arithmetic from the LBM hot loop.
 * See: gororoba_app/crates/gororoba_bevy_lbm/src/soa_solver.rs:100
 *
 * Construction: O(N) BFS traversal of DOM tree
 * Memory: O(N) flat arrays + O(depth) level offsets
 * Layout pass: O(N) total, O(N/depth) per level, parallelizable per level
 *
 * See: layout/lib.rs build_layout_tree() for current recursive approach
 * See: style.rs compute_styles() for BFS-level style computation
 */

use silksurf_dom::{Dom, NodeId};

/// Pre-computed BFS-level decomposition for parallel layout.
pub struct LayoutNeighborTable {
    /// Nodes grouped by BFS depth level.
    /// levels[0] = [root], levels[1] = [root's children], etc.
    pub levels: Vec<Vec<NodeId>>,
    /// For each node (by flat BFS index), the index of its parent in the flat array.
    /// Root has parent_idx = u32::MAX (sentinel).
    pub parent_idx: Vec<u32>,
    /// Flat BFS-order list of all node IDs.
    pub bfs_order: Vec<NodeId>,
    /// Number of children per node (by flat BFS index).
    pub child_count: Vec<u16>,
    /// Reverse map: NodeId -> flat BFS index.
    /// Enables O(1) node lookup for callers that hold a NodeId.
    pub node_to_bfs_idx: rustc_hash::FxHashMap<NodeId, u32>,
}

impl LayoutNeighborTable {
    /*
     * build -- construct the neighbor table from a DOM tree via BFS.
     *
     * Complexity: O(N) where N = number of nodes
     * Memory: O(N) for flat arrays + O(depth) for level boundaries
     */
    pub fn build(dom: &Dom, root: NodeId) -> Self {
        let mut levels: Vec<Vec<NodeId>> = Vec::new();
        let mut bfs_order: Vec<NodeId> = Vec::new();
        let mut parent_idx: Vec<u32> = Vec::new();
        let mut child_count: Vec<u16> = Vec::new();

        // BFS index lookup: NodeId -> flat index (exposed as node_to_bfs_idx)
        let mut node_to_idx: rustc_hash::FxHashMap<NodeId, u32> = rustc_hash::FxHashMap::default();

        // Seed BFS with root
        let mut current_level = vec![root];

        while !current_level.is_empty() {
            let mut next_level = Vec::new();

            for &node in &current_level {
                let flat_idx = bfs_order.len() as u32;
                node_to_idx.insert(node, flat_idx);
                bfs_order.push(node);

                // Parent index: look up parent's flat index
                let pidx = dom
                    .parent(node)
                    .ok()
                    .flatten()
                    .and_then(|p| node_to_idx.get(&p).copied())
                    .unwrap_or(u32::MAX);
                parent_idx.push(pidx);

                // Count and queue children
                let children = dom.children(node).unwrap_or(&[]);
                child_count.push(children.len().min(u16::MAX as usize) as u16);
                for &child in children {
                    next_level.push(child);
                }
            }

            levels.push(current_level);
            current_level = next_level;
        }

        Self {
            levels,
            parent_idx,
            bfs_order,
            child_count,
            node_to_bfs_idx: node_to_idx,
        }
    }

    /// Total number of nodes.
    pub fn len(&self) -> usize {
        self.bfs_order.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.bfs_order.is_empty()
    }

    /// Number of BFS depth levels.
    pub fn depth(&self) -> usize {
        self.levels.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_node() {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let table = LayoutNeighborTable::build(&dom, root);
        assert_eq!(table.len(), 1);
        assert_eq!(table.depth(), 1);
        assert_eq!(table.parent_idx[0], u32::MAX);
    }

    #[test]
    fn test_two_levels() {
        let mut dom = Dom::new();
        let root = dom.create_element("div");
        let child1 = dom.create_element("span");
        let child2 = dom.create_element("p");
        dom.append_child(root, child1).unwrap();
        dom.append_child(root, child2).unwrap();

        let table = LayoutNeighborTable::build(&dom, root);
        assert_eq!(table.len(), 3);
        assert_eq!(table.depth(), 2);
        assert_eq!(table.levels[0].len(), 1); // root
        assert_eq!(table.levels[1].len(), 2); // children
        assert_eq!(table.child_count[0], 2); // root has 2 children
    }
}
