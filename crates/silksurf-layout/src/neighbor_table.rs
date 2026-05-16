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
 * REPRESENTATION: levels are stored as flat offsets into bfs_order rather
 * than Vec<Vec<NodeId>>.  This eliminates O(depth) inner-Vec allocations
 * per build() call -- for a typical 6-level page that is 6 saved heap allocs.
 * level(i) returns &bfs_order[level_starts[i]..level_starts[i+1]] cheaply.
 *
 * rebuild() clears all Vecs (retaining capacity) and refills from scratch.
 * After warm-up, rebuild() requires zero allocator calls for the same DOM.
 *
 * See: layout/lib.rs build_layout_tree() for current recursive approach
 * See: style.rs compute_styles() for BFS-level style computation
 * See: fused_pipeline.rs FusedWorkspace for the reuse pattern
 */

use silksurf_dom::{Dom, NodeId};

/// Pre-computed BFS-level decomposition for parallel layout.
#[derive(Default)]
pub struct LayoutNeighborTable {
    /// Start index of each BFS level in bfs_order.
    /// Level i contains `bfs_order[level_starts[i]..level_starts[i+1]]`.
    /// The last level's end is bfs_order.len() (no sentinel stored).
    /// Use level(i) for safe slice access.
    ///
    /// WHY flat offsets over `Vec<Vec<NodeId>>`: eliminates O(depth) inner-Vec
    /// allocations per call. A 6-level DOM saves 6 heap allocs on every
    /// rebuild() call while level(i) remains O(1) slice arithmetic.
    pub level_starts: Vec<u32>,
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
     * Allocates fresh Vecs and delegates to rebuild().
     * For repeated calls on the same or similar DOM, prefer rebuild() on an
     * existing table to reuse allocated capacity.
     *
     * Complexity: O(N) where N = number of nodes
     * Memory: O(N) for flat arrays + O(depth) for level_starts
     */
    pub fn build(dom: &Dom, root: NodeId) -> Self {
        let mut table = Self {
            level_starts: Vec::new(),
            parent_idx: Vec::new(),
            bfs_order: Vec::new(),
            child_count: Vec::new(),
            node_to_bfs_idx: rustc_hash::FxHashMap::default(),
        };
        table.rebuild(dom, root);
        table
    }

    /*
     * rebuild -- refill the table in-place, reusing allocated capacity.
     *
     * WHY: fused_style_layout_paint calls build() on every render.  For a
     * 50-node DOM at 1000 iterations that is 1000 * (1 FxHashMap + 4 Vecs +
     * 6 inner level Vecs) = >10000 allocator calls.  rebuild() clears each
     * container (O(1) capacity-preserving clear) and refills, yielding zero
     * heap traffic after the first call once capacity is established.
     *
     * High-water-mark growth: if the new DOM has more nodes than the previous
     * one, the Vecs grow as needed.  If it is smaller they just use less of
     * the same allocation.  The FxHashMap grows and never shrinks.
     *
     * INVARIANT: After rebuild(), all arrays are consistent (same length N,
     * same BFS ordering, valid parent_idx/child_count entries).
     *
     * Complexity: O(N) where N = new node count
     * Allocations: 0 after capacity reaches steady state
     */
    pub fn rebuild(&mut self, dom: &Dom, root: NodeId) {
        self.bfs_order.clear();
        self.parent_idx.clear();
        self.child_count.clear();
        self.node_to_bfs_idx.clear();
        self.level_starts.clear();

        // Seed BFS with root at flat index 0.
        self.bfs_order.push(root);
        self.node_to_bfs_idx.insert(root, 0);
        self.parent_idx.push(u32::MAX);
        self.child_count.push(0); // filled when root is processed below
        self.level_starts.push(0);

        let mut level_start = 0usize;

        // BFS loop: process nodes at [level_start..level_end],
        // append their children to the end of bfs_order.
        loop {
            let level_end = self.bfs_order.len();
            if level_start >= level_end {
                break;
            }

            // Record where the next level begins (after children are appended).
            let next_level_start = level_end;

            for i in level_start..level_end {
                let node = self.bfs_order[i];
                let children = dom.children(node).unwrap_or(&[]);
                // Set child count for this node (was placeholder 0).
                self.child_count[i] = children.len().min(u16::MAX as usize) as u16;

                let pidx = i as u32;
                for &child in children {
                    let flat_idx = self.bfs_order.len() as u32;
                    self.node_to_bfs_idx.insert(child, flat_idx);
                    self.bfs_order.push(child);
                    self.parent_idx.push(pidx);
                    self.child_count.push(0); // placeholder, filled next iteration
                }
            }

            // Only record a new level start if children were added.
            if self.bfs_order.len() > next_level_start {
                self.level_starts.push(next_level_start as u32);
            }

            level_start = level_end;
        }
    }

    /*
     * level -- return the slice of node IDs at BFS depth i.
     *
     * Computed as bfs_order[level_starts[i]..level_starts[i+1]].
     * For the last level, end = bfs_order.len() (no sentinel stored).
     *
     * Complexity: O(1)
     */
    pub fn level(&self, i: usize) -> &[NodeId] {
        let start = self.level_starts[i] as usize;
        let end = if i + 1 < self.level_starts.len() {
            self.level_starts[i + 1] as usize
        } else {
            self.bfs_order.len()
        };
        &self.bfs_order[start..end]
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
        self.level_starts.len()
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
        assert_eq!(table.level(0).len(), 1); // root
        assert_eq!(table.level(1).len(), 2); // children
        assert_eq!(table.child_count[0], 2); // root has 2 children
    }

    #[test]
    fn test_rebuild_reuses_capacity() {
        let mut dom = Dom::new();
        let root = dom.create_element("div");
        let child = dom.create_element("span");
        dom.append_child(root, child).unwrap();

        let mut table = LayoutNeighborTable::build(&dom, root);
        assert_eq!(table.len(), 2);

        // Rebuild into same table -- should produce identical result.
        table.rebuild(&dom, root);
        assert_eq!(table.len(), 2);
        assert_eq!(table.depth(), 2);
        assert_eq!(table.level(0)[0], root);
        assert_eq!(table.level(1)[0], child);
        assert_eq!(table.parent_idx[1], 0);
    }
}
