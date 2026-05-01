/*
 * fused_pipeline.rs -- single-pass style+layout+paint pipeline.
 *
 * WHY: The standard pipeline makes 3 full-DOM traversals:
 *   1. compute_styles(): walk tree, cascade CSS for each node
 *   2. build_layout_tree(): walk tree again, compute box dimensions
 *   3. build_display_list(): walk tree AGAIN, emit paint commands
 *
 * Each traversal reads from memory written by the previous one. For a 401-node
 * DOM, that's 3 * 401 = 1203 node visits with intermediate data structures
 * (ComputedStyle HashMap + LayoutBox arena + DisplayList Vec).
 *
 * The fused pipeline does all three in ONE BFS pass per tree level:
 *   For each level (root -> children -> grandchildren...):
 *     For each node in this level:
 *       1. Cascade CSS (read parent style, match selectors, apply declarations)
 *       2. Compute box dimensions (margin, padding, border, content rect)
 *       3. Emit display item (SolidColor or Text)
 *
 * Memory bandwidth: 3x reduction (read node data once, not three times).
 * Cache: parent styles remain hot in L1 during child processing.
 *
 * Inspired by the gororoba fused pull-collide kernel which halved memory
 * bandwidth by combining streaming + collision in one pass.
 * See: gororoba_app/crates/gororoba_bevy_lbm/src/soa_solver.rs:280
 * See: gororoba_app/docs/engine_optimizations.md Section 3
 *
 * See: style.rs for standalone cascade
 * See: layout/lib.rs for standalone layout
 * See: render/lib.rs for standalone display list building
 * See: neighbor_table.rs for the BFS-level decomposition this uses
 */

use silksurf_css::{
    CascadeWorkspace, ComputedStyle, Display, StyleIndex, Stylesheet, cascade_view::CascadeView,
    compute_style_for_node_with_workspace,
};
use silksurf_dom::{Dom, NodeId, NodeKind};
use silksurf_layout::Rect;
use silksurf_layout::neighbor_table::LayoutNeighborTable;
use silksurf_render::DisplayItem;

/*
 * FusedWorkspace -- pre-allocated scratch for zero-alloc steady-state renders.
 *
 * WHY: fused_style_layout_paint allocates fresh on every call:
 *   - LayoutNeighborTable: 1 FxHashMap + 4 Vecs (bfs_order, parent_idx,
 *     child_count, level_starts) + FxHashMap insertions for N nodes
 *   - CascadeWorkspace: 3 Vecs (matched_by_rule, candidates, seen)
 *   - Output Vecs: styles, node_rects, block_cursors, display_items
 *
 * FusedWorkspace holds all of these as owned fields.  Each run() call clears
 * them (O(1) capacity-preserving) and refills.  After the first call, no
 * allocator traffic occurs for the same or smaller DOM.
 *
 * High-water-mark growth: all containers grow to the peak node count seen
 * and never shrink.  Stable pages (cached re-renders) reach steady state
 * after the first render and stay there.
 *
 * INVARIANT: styles, node_rects, display_items are valid only until the next
 * run() call.  Callers must not hold references across run() calls.
 *
 * Usage:
 *   let style_index = StyleIndex::new(&stylesheet); // cache externally
 *   let mut ws = FusedWorkspace::default();
 *   loop {
 *       ws.run(&dom, &stylesheet, &style_index, root, viewport);
 *       consume(&ws.display_items);
 *   }
 *
 * See: fused_style_layout_paint for the allocating single-call version
 * See: LayoutNeighborTable::rebuild for the in-place BFS reuse
 * See: CascadeWorkspace for cascade scratch reuse semantics
 */
pub struct FusedWorkspace {
    /// BFS traversal table -- rebuilt only when DOM generation changes.
    table: LayoutNeighborTable,
    /// SoA cascade view -- materialized only when DOM generation changes.
    cascade_view: CascadeView,
    /// Cascade scratch -- grows to peak rule count, never shrinks.
    cascade_ws: CascadeWorkspace,
    /// Block-flow cursor per node (internal layout temp, not exposed).
    block_cursors: Vec<f32>,
    /// Computed style per BFS-indexed node (valid after run()).
    pub styles: Vec<Option<ComputedStyle>>,
    /// Content rect per BFS-indexed node (valid after run()).
    pub node_rects: Vec<Rect>,
    /// Paint commands (valid after run(); order is BFS paint order).
    pub display_items: Vec<DisplayItem>,
    /// Cached DOM generation to skip rebuild when DOM unchanged.
    dom_generation: u64,
}

impl Default for FusedWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

impl FusedWorkspace {
    /*
     * new -- create an empty workspace.
     *
     * All internal containers start empty (zero allocation beyond struct
     * overhead).  The first run() call allocates to fit the given DOM.
     * Subsequent calls with the same or smaller DOM are zero-alloc.
     */
    pub fn new() -> Self {
        Self {
            table: LayoutNeighborTable::default(),
            cascade_view: CascadeView::new(),
            cascade_ws: CascadeWorkspace::new(0),
            block_cursors: Vec::new(),
            styles: Vec::new(),
            node_rects: Vec::new(),
            display_items: Vec::new(),
            dom_generation: u64::MAX, // force first rebuild
        }
    }

    /*
     * run -- execute fused style+layout+paint, filling workspace output fields.
     *
     * Takes `style_index` as a parameter to allow the caller to cache it
     * across calls when the stylesheet does not change.  Building StyleIndex
     * is O(rules) -- for 13 rules it is trivial; for ChatGPT-scale stylesheets
     * (hundreds of rules) the caller should build it once and reuse it.
     *
     * After run() returns:
     *   ws.display_items -- paint commands in BFS order
     *   ws.styles        -- per-node ComputedStyle (BFS indexed)
     *   ws.node_rects    -- per-node content rect (BFS indexed)
     *
     * Complexity: O(N * R_avg) where N=nodes, R_avg=matching rules per node
     * Allocations: 0 after first call on same or smaller DOM
     *
     * See: fused_style_layout_paint for context on algorithm
     */
    pub fn run(
        &mut self,
        dom: &Dom,
        stylesheet: &Stylesheet,
        style_index: &StyleIndex,
        root: NodeId,
        viewport: Rect,
    ) {
        /*
         * Conditional rebuild: skip BFS table and CascadeView materialization
         * when the DOM has not changed since the last run(). This hoists ~2us
         * of rebuild cost out of the steady-state re-render path (e.g., hover
         * state changes, media query re-evaluation on the same DOM).
         *
         * The DOM's mutation_generation increments on end_mutation_batch() and
         * materialize_resolve_table(). If it matches our cached value, the
         * topology and attribute data are identical -- skip rebuild.
         */
        let dom_gen = dom.generation();
        if dom_gen != self.dom_generation {
            self.table.rebuild(dom, root);
            self.cascade_view.rebuild(dom);
            self.dom_generation = dom_gen;
        }
        let n = self.table.len();

        // Resize output/temp Vecs to n.  clear() retains heap allocation when
        // n <= previous capacity (the common case for stable pages).
        self.styles.clear();
        self.styles.resize(n, None);
        self.node_rects.clear();
        self.node_rects.resize(n, viewport);
        self.block_cursors.clear();
        self.block_cursors.resize(n, viewport.y);
        self.display_items.clear();

        for (i, &node) in self.table.bfs_order.iter().enumerate() {
            let pidx = self.table.parent_idx[i];

            let (parent_style, parent_rect, cursor) = if pidx == u32::MAX {
                (None, viewport, viewport.y)
            } else {
                let p = pidx as usize;
                (
                    self.styles[p].as_ref(),
                    self.node_rects[p],
                    self.block_cursors[p],
                )
            };

            let style = compute_style_for_node_with_workspace(
                dom,
                node,
                stylesheet,
                style_index,
                parent_style,
                &mut self.cascade_ws,
                Some(&self.cascade_view),
            );

            if style.display == Display::None {
                self.styles[i] = Some(style);
                continue;
            }

            let margin_top = length_px(style.margin.top);
            let padding_top = length_px(style.padding.top);
            let border_top = length_px(style.border.top);

            let x = parent_rect.x
                + length_px(style.margin.left)
                + length_px(style.padding.left)
                + length_px(style.border.left);
            let y = cursor + margin_top + padding_top + border_top;
            let width = parent_rect.width
                - length_px(style.margin.left)
                - length_px(style.margin.right)
                - length_px(style.padding.left)
                - length_px(style.padding.right)
                - length_px(style.border.left)
                - length_px(style.border.right);
            let height = length_px(style.line_height);

            let content_rect = Rect {
                x,
                y,
                width,
                height,
            };
            self.node_rects[i] = content_rect;

            if pidx != u32::MAX {
                self.block_cursors[pidx as usize] = y
                    + height
                    + length_px(style.padding.bottom)
                    + length_px(style.border.bottom)
                    + length_px(style.margin.bottom);
            }
            self.block_cursors[i] = y;

            if style.background_color.a > 0 {
                self.display_items.push(DisplayItem::SolidColor {
                    rect: content_rect,
                    color: style.background_color,
                });
            }

            if let Ok(dom_node) = dom.node(node)
                && let NodeKind::Text { text } = dom_node.kind()
            {
                self.display_items.push(DisplayItem::Text {
                    rect: content_rect,
                    node,
                    text_len: text.len() as u32,
                    color: style.color,
                });
            }

            self.styles[i] = Some(style);
        }
    }

    /// Number of BFS-ordered nodes from the last run() call.
    pub fn node_count(&self) -> usize {
        self.table.len()
    }

    /// BFS traversal table from the last run() call.
    pub fn table(&self) -> &LayoutNeighborTable {
        &self.table
    }
}

/*
 * FusedResult -- output of the single-pass pipeline.
 *
 * All per-node arrays are indexed by BFS order (same as table.bfs_order[i]).
 * To look up a specific node by NodeId, use table.node_to_bfs_idx[&node_id].
 *
 * WHY Vec over FxHashMap: O(1) array index vs O(1)-amortised hash with higher
 * constant -- no hashing, no collision chains, contiguous cache lines.
 * For 50 nodes this is ~3x faster in the parent lookup hot path.
 */
/*
 * FusedResult -- output of the single-pass pipeline.
 *
 * All per-node arrays are indexed by BFS order (same as table.bfs_order[i]).
 * To look up a specific node by NodeId, use table.node_to_bfs_idx[&node_id].
 *
 * WHY Vec over FxHashMap: O(1) array index vs O(1)-amortised hash with higher
 * constant -- no hashing, no collision chains, contiguous cache lines.
 * For 50 nodes this is ~3x faster in the parent lookup hot path.
 *
 * WHY no StyleSoA field: building StyleSoA unconditionally costs ~4us for
 * 50 nodes (FxHashMap insertions + 25 Vec pushes) and eliminates the fused
 * pipeline's speedup advantage over the 3-pass baseline.  Instead, callers
 * that need column-oriented access call StyleSoA::from_bfs on demand.
 * See: silksurf_css::style_soa::StyleSoA::from_bfs
 */
pub struct FusedResult {
    /// Style per node in BFS order. None for display:none or skipped nodes.
    pub styles: Vec<Option<ComputedStyle>>,
    pub display_items: Vec<DisplayItem>,
    /// Content rect per node in BFS order.
    pub node_rects: Vec<Rect>,
    /// BFS traversal table; use node_to_bfs_idx for NodeId -> index mapping.
    pub table: LayoutNeighborTable,
}

/*
 * fused_style_layout_paint -- single BFS pass producing styles + display list.
 *
 * Uses LayoutNeighborTable for BFS-level traversal. Each level is processed
 * sequentially (parent data must be ready before children), but within a
 * level all nodes are independent and could be parallelized.
 *
 * Complexity: O(N * R_avg) where N=nodes, R_avg=matching rules per node
 * Memory: O(N) for styles + O(items) for display list
 */
pub fn fused_style_layout_paint(
    dom: &Dom,
    stylesheet: &Stylesheet,
    root: NodeId,
    viewport: Rect,
) -> FusedResult {
    /*
     * Build StyleIndex once for all nodes.
     *
     * WHY: compute_style_for_node rebuilds StyleIndex on every call (O(rules)).
     * For 401 nodes that is 401 redundant index constructions. Building once
     * and passing via compute_style_for_node_with_workspace saves ~400 allocs.
     * See: style.rs StyleIndex::new() for construction cost.
     */
    let style_index = StyleIndex::new(stylesheet);
    /*
     * Shared CascadeWorkspace: allocated once, reused for every node in the
     * BFS traversal. Eliminates per-node allocations (matched_by_rule Vec,
     * candidates Vec, seen_bits bitvec, class_keys Vec) -- ~200 allocs saved
     * for a 50-node page. See: silksurf_css::CascadeWorkspace for lifecycle.
     */
    let mut cascade_ws = CascadeWorkspace::new(stylesheet.rules.len());
    let table = LayoutNeighborTable::build(dom, root);
    let n = table.len();

    /*
     * Pre-allocate BFS-indexed Vecs -- one slot per node, indexed by flat BFS index.
     *
     * WHY: The original implementation used FxHashMap<NodeId, T> for styles,
     * node_rects, and block_cursors. Each parent lookup in the hot loop required
     * a hash + equality check (~10-15 cycles). With Vecs, parent lookup is
     * table.parent_idx[i] (one array read) + Vec index (~2 cycles).
     * For 50 nodes at 1000 iterations: ~3x reduction in lookup cost.
     *
     * parent_idx[i] == u32::MAX means root (no parent); handled explicitly.
     * See: neighbor_table.rs LayoutNeighborTable::build() for index construction.
     */
    let mut styles: Vec<Option<ComputedStyle>> = vec![None; n];
    let mut node_rects: Vec<Rect> = vec![viewport; n];
    let mut block_cursors: Vec<f32> = vec![viewport.y; n];
    let mut display_items: Vec<DisplayItem> = Vec::new();

    for (i, &node) in table.bfs_order.iter().enumerate() {
        let pidx = table.parent_idx[i];

        // O(1) parent data -- no HashMap lookup
        let (parent_style, parent_rect, cursor) = if pidx == u32::MAX {
            (None, viewport, viewport.y)
        } else {
            let p = pidx as usize;
            (styles[p].as_ref(), node_rects[p], block_cursors[p])
        };

        // 1. CASCADE: reuses pre-built index and shared workspace (zero alloc after first node)
        let style = compute_style_for_node_with_workspace(
            dom,
            node,
            stylesheet,
            &style_index,
            parent_style,
            &mut cascade_ws,
            None, // no CascadeView in cold path
        );

        // Skip display:none; still store style for child inheritance
        if style.display == Display::None {
            styles[i] = Some(style);
            continue;
        }

        // 2. LAYOUT: compute position (simplified block flow)
        let margin_top = length_px(style.margin.top);
        let padding_top = length_px(style.padding.top);
        let border_top = length_px(style.border.top);

        let x = parent_rect.x
            + length_px(style.margin.left)
            + length_px(style.padding.left)
            + length_px(style.border.left);
        let y = cursor + margin_top + padding_top + border_top;
        let width = parent_rect.width
            - length_px(style.margin.left)
            - length_px(style.margin.right)
            - length_px(style.padding.left)
            - length_px(style.padding.right)
            - length_px(style.border.left)
            - length_px(style.border.right);

        // Estimate height from line-height
        let height = length_px(style.line_height);

        let content_rect = Rect {
            x,
            y,
            width,
            height,
        };
        node_rects[i] = content_rect;

        // Advance parent's block cursor past this node
        if pidx != u32::MAX {
            block_cursors[pidx as usize] = y
                + height
                + length_px(style.padding.bottom)
                + length_px(style.border.bottom)
                + length_px(style.margin.bottom);
        }
        // Seed this node's cursor for its own children
        block_cursors[i] = y;

        // 3. PAINT: emit display items
        if style.background_color.a > 0 {
            display_items.push(DisplayItem::SolidColor {
                rect: content_rect,
                color: style.background_color,
            });
        }

        if let Ok(dom_node) = dom.node(node)
            && let NodeKind::Text { text } = dom_node.kind()
        {
            display_items.push(DisplayItem::Text {
                rect: content_rect,
                node,
                text_len: text.len() as u32,
                color: style.color,
            });
        }

        styles[i] = Some(style);
    }

    FusedResult {
        styles,
        display_items,
        node_rects,
        table,
    }
}

fn length_px(length: silksurf_css::Length) -> f32 {
    match length {
        silksurf_css::Length::Px(v) => v,
        silksurf_css::Length::Percent(_) => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fused_empty_dom() {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let stylesheet = silksurf_css::parse_stylesheet("").unwrap();
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 800.0,
        };

        let result = fused_style_layout_paint(&dom, &stylesheet, root, viewport);
        // BFS index 0 is always the root node; its style must be computed.
        assert_eq!(result.table.bfs_order[0], root);
        assert!(result.styles[0].is_some());
    }
}
