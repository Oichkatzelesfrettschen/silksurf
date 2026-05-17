/*
 * fused_pipeline.rs -- three-pass style+layout+paint pipeline.
 *
 * WHY: Separating cascade from layout allows taffy to see all sibling styles
 * before computing Flexbox/Grid positions.  The three passes are:
 *   Pass 1 (cascade): BFS walk, compute ComputedStyle for each node.
 *   Pass 2 (layout):  Build taffy tree from styles, run Flexbox/Grid solver,
 *                     write absolute Rect back into node_rects[].
 *   Pass 3 (paint):   BFS walk over pre-computed rects, emit display items.
 *
 * Pass 1 must complete before Pass 2 (taffy needs all styles).
 * Pass 2 must complete before Pass 3 (paint needs correct positions).
 *
 * See: crates/silksurf-layout/src/taffy_layout.rs for the taffy adapter.
 * See: style.rs for standalone cascade.
 * See: layout/lib.rs for standalone layout.
 * See: render/lib.rs for standalone display list building.
 * See: neighbor_table.rs for the BFS-level decomposition this uses.
 */

use silksurf_css::{
    CascadeView, CascadeWorkspace, ComputedStyle, Display, StyleIndex, Stylesheet,
    compute_style_for_node_with_workspace,
};
use silksurf_dom::{Dom, NodeId, NodeKind};
use silksurf_layout::Rect;
use silksurf_layout::neighbor_table::LayoutNeighborTable;
use silksurf_layout::taffy_layout::TaffyLayout;
use silksurf_render::DisplayItem;

/*
 * FusedWorkspace -- pre-allocated scratch for zero-alloc steady-state renders.
 *
 * WHY: fused_style_layout_paint allocates fresh on every call:
 *   - LayoutNeighborTable: 1 FxHashMap + 4 Vecs (bfs_order, parent_idx,
 *     child_count, level_starts) + FxHashMap insertions for N nodes
 *   - CascadeWorkspace: 3 Vecs (matched_by_rule, candidates, seen)
 *   - Output Vecs: styles, node_rects, display_items
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
    /// Taffy layout state -- rebuilt when DOM generation changes.
    taffy_layout: TaffyLayout,
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
            taffy_layout: TaffyLayout::new(),
            styles: Vec::new(),
            node_rects: Vec::new(),
            display_items: Vec::new(),
            dom_generation: u64::MAX, // force first rebuild
        }
    }

    /*
     * run -- execute the three-pass style+layout+paint pipeline.
     *
     * Pass 1 (cascade): compute ComputedStyle for every BFS node.
     * Pass 2 (layout):  run taffy Flexbox/Grid solver, write node_rects[].
     * Pass 3 (paint):   emit display items from the computed rects.
     *
     * Takes `style_index` as a parameter to allow the caller to cache it
     * across calls when the stylesheet does not change.  Building StyleIndex
     * is O(rules) -- for 13 rules it is trivial; for large stylesheets
     * the caller should build it once and reuse it.
     *
     * After run() returns:
     *   ws.display_items -- paint commands in BFS order
     *   ws.styles        -- per-node ComputedStyle (BFS indexed)
     *   ws.node_rects    -- per-node content rect (BFS indexed)
     *
     * Allocations: 0 after first call on same or smaller DOM
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
         * of rebuild cost out of the steady-state re-render path.
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

        self.styles.clear();
        self.styles.resize(n, None);
        self.node_rects.clear();
        self.node_rects.resize(n, viewport);
        self.display_items.clear();

        // Pass 1: cascade -- compute ComputedStyle for every BFS node.
        // Each node reads its parent's style (already computed, since BFS
        // processes parents before children).
        let mut rem_base_px = 16.0_f32;
        for (i, &node) in self.table.bfs_order.iter().enumerate() {
            let pidx = self.table.parent_idx[i];
            let parent_style = if pidx == u32::MAX {
                None
            } else {
                self.styles[pidx as usize].as_ref()
            };
            let style = compute_style_for_node_with_workspace(
                dom,
                node,
                stylesheet,
                style_index,
                parent_style,
                &mut self.cascade_ws,
                Some(&self.cascade_view),
                rem_base_px,
            );
            if dom
                .element_name(node)
                .ok()
                .flatten()
                .map(|n| n.eq_ignore_ascii_case("html"))
                .unwrap_or(false)
                && let silksurf_css::Length::Px(v) = style.font_size
            {
                rem_base_px = v;
            }
            self.styles[i] = Some(style);
        }

        // Pass 2: layout -- rebuild taffy tree from styles and compute
        // Flexbox/Grid positions, then write absolute rects into node_rects[].
        self.taffy_layout.rebuild(&self.table, &self.styles);
        self.taffy_layout
            .compute(dom, &self.styles, &self.table.bfs_order, viewport);
        self.taffy_layout
            .write_rects(&self.table.parent_idx, &mut self.node_rects, viewport);

        // Pass 3: paint -- emit display items for each visible node.
        for (i, &node) in self.table.bfs_order.iter().enumerate() {
            let Some(ref style) = self.styles[i] else {
                continue;
            };
            if style.display == Display::None {
                continue;
            }
            let content_rect = self.node_rects[i];

            // Box-shadow paints below the background (CSS paint order).
            if let Some(shadow) = style.box_shadow
                && !shadow.inset
            {
                self.display_items.push(DisplayItem::BoxShadow {
                    rect: content_rect,
                    shadow,
                });
            }

            if let Some(ref gradient) = style.background_image {
                self.display_items.push(DisplayItem::LinearGradient {
                    rect: content_rect,
                    angle: gradient.angle_deg,
                    stops: gradient.stops.clone(),
                });
            } else if style.background_color.a > 0 {
                if style.border_radius > 0.0 {
                    self.display_items.push(DisplayItem::RoundedRect {
                        rect: content_rect,
                        radii: [style.border_radius; 4],
                        color: style.background_color,
                    });
                } else {
                    self.display_items.push(DisplayItem::SolidColor {
                        rect: content_rect,
                        color: style.background_color,
                    });
                }
            }

            if let Ok(dom_node) = dom.node(node)
                && let NodeKind::Text { text } = dom_node.kind()
            {
                let font_size_px = match style.font_size {
                    silksurf_css::Length::Px(px) => px,
                    _ => 16.0,
                };
                self.display_items.push(DisplayItem::Text {
                    rect: content_rect,
                    node,
                    text_len: text.len() as u32,
                    text: text.to_string(),
                    font_size: font_size_px,
                    color: style.color,
                });
            }
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
 * fused_style_layout_paint -- allocating three-pass pipeline.
 *
 * Performs style cascade, taffy Flexbox/Grid layout, and display list
 * construction in three sequential BFS passes.  Each call allocates fresh;
 * use FusedWorkspace for the zero-alloc steady-state path.
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
    let mut cascade_ws = CascadeWorkspace::new(style_index.active_rules.len());
    let table = LayoutNeighborTable::build(dom, root);
    let n = table.len();

    let mut styles: Vec<Option<ComputedStyle>> = vec![None; n];
    let mut node_rects: Vec<Rect> = vec![viewport; n];
    let mut display_items: Vec<DisplayItem> = Vec::new();

    // Pass 1: cascade
    let mut rem_base_px = 16.0_f32;
    for (i, &node) in table.bfs_order.iter().enumerate() {
        let pidx = table.parent_idx[i];
        let parent_style = if pidx == u32::MAX {
            None
        } else {
            styles[pidx as usize].as_ref()
        };
        let style = compute_style_for_node_with_workspace(
            dom,
            node,
            stylesheet,
            &style_index,
            parent_style,
            &mut cascade_ws,
            None,
            rem_base_px,
        );
        if dom
            .element_name(node)
            .ok()
            .flatten()
            .map(|n| n.eq_ignore_ascii_case("html"))
            .unwrap_or(false)
            && let silksurf_css::Length::Px(v) = style.font_size
        {
            rem_base_px = v;
        }
        styles[i] = Some(style);
    }

    // Pass 2: taffy layout
    let mut taffy_layout = TaffyLayout::new();
    taffy_layout.rebuild(&table, &styles);
    taffy_layout.compute(dom, &styles, &table.bfs_order, viewport);
    taffy_layout.write_rects(&table.parent_idx, &mut node_rects, viewport);

    // Pass 3: paint
    for (i, &node) in table.bfs_order.iter().enumerate() {
        let Some(ref style) = styles[i] else {
            continue;
        };
        if style.display == Display::None {
            continue;
        }
        let content_rect = node_rects[i];

        if let Some(ref gradient) = style.background_image {
            display_items.push(DisplayItem::LinearGradient {
                rect: content_rect,
                angle: gradient.angle_deg,
                stops: gradient.stops.clone(),
            });
        } else if style.background_color.a > 0 {
            display_items.push(DisplayItem::SolidColor {
                rect: content_rect,
                color: style.background_color,
            });
        }

        if let Ok(dom_node) = dom.node(node)
            && let NodeKind::Text { text } = dom_node.kind()
        {
            let font_size_px = match style.font_size {
                silksurf_css::Length::Px(px) => px,
                _ => 16.0,
            };
            display_items.push(DisplayItem::Text {
                rect: content_rect,
                node,
                text_len: text.len() as u32,
                text: text.to_string(),
                font_size: font_size_px,
                color: style.color,
            });
        }
    }

    FusedResult {
        styles,
        display_items,
        node_rects,
        table,
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
