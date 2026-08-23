/*
 * fused_pipeline.rs -- three-pass style+layout+paint pipeline.
 *
 * The cascade pass computes styles for every BFS node before taffy receives
 * the tree. The layout pass writes absolute rects for those styled nodes. The
 * paint pass emits display items from the completed layout state.
 */

use silksurf_css::{
    CascadeView, CascadeWorkspace, ComputedStyle, Display, Length, LengthOrAuto,
    Position as CssPosition, StyleIndex, Stylesheet, WhiteSpace,
    compute_style_for_node_with_workspace,
};
use silksurf_dom::{Dom, NodeId, NodeKind, TagName};
use silksurf_layout::neighbor_table::LayoutNeighborTable;
use silksurf_layout::taffy_layout::TaffyLayout;
use silksurf_layout::{EdgeSizes, Rect};
use silksurf_render::DisplayItem;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplacedSize {
    pub node: NodeId,
    pub width: f32,
    pub height: f32,
}

/*
 * FusedWorkspace -- pre-allocated scratch for zero-alloc steady-state renders.
 *
 * fused_style_layout_paint allocates fresh on every call:
 *   - LayoutNeighborTable: 1 FxHashMap + 5 Vecs (bfs_order, parent_idx,
 *     child_start, child_count, level_starts) + FxHashMap insertions for N nodes
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
 * fused_style_layout_paint supplies the allocating single-call version.
 * LayoutNeighborTable::rebuild supplies in-place BFS reuse.
 * CascadeWorkspace supplies cascade scratch reuse.
 */
pub struct FusedWorkspace {
    /// BFS traversal table -- rebuilt only when DOM generation changes.
    table: LayoutNeighborTable,
    /// `SoA` cascade view -- materialized only when DOM generation changes.
    cascade_view: CascadeView,
    /// Cascade scratch -- grows to peak rule count, never shrinks.
    cascade_ws: CascadeWorkspace,
    /// Taffy layout state -- rebuilt when DOM generation changes.
    taffy_layout: TaffyLayout,
    /// Computed style per BFS-indexed node (valid after `run()`).
    pub styles: Vec<Option<ComputedStyle>>,
    /// Border-box rect per BFS-indexed node, in document coordinates (valid
    /// after `run()`). taffy reports `layout.size` as the border box, so this
    /// is the rect `Element.getBoundingClientRect` reports once the viewport
    /// scroll offset is subtracted.
    pub node_rects: Vec<Rect>,
    /// Border widths per BFS-indexed node, which separate the border box above
    /// from the padding box `Element.clientWidth` reports.
    pub node_borders: Vec<EdgeSizes>,
    /// Paint commands (valid after `run()`; order is BFS paint order).
    pub display_items: Vec<DisplayItem>,
    /// Accumulated transform translation per BFS-indexed node, retained so a
    /// repaint reuses the allocation.
    node_transforms: Vec<PaintTransform>,
    /// Stacking-context scratch and the resulting paint sequence, both
    /// retained so a repaint reuses the allocation.
    stacking: StackingOrder,
    paint_order: Vec<u32>,
    /// Whether each BFS-indexed node generates a box, retained so a repaint
    /// reuses the allocation.
    rendered: Vec<bool>,
    /// Cached tree-shape generation for the BFS table.
    table_generation: u64,
    /// Cached selector-input generation for the cascade view.
    cascade_generation: u64,
    /// Cached tree-shape generation for the taffy node graph.
    taffy_structure_generation: u64,
    /// The viewport the retained taffy styles were built from. The cascade
    /// resolves `vw`, `vh`, and the `@media` branch against the viewport, so a
    /// viewport that moved changes the ComputedStyle that `rebuild` reads
    /// while both DOM generations stand still.
    taffy_viewport: Rect,
    /// Cached selector-input generation for the taffy style graph.
    taffy_style_generation: u64,
    /// The StyleIndex the retained taffy styles were built from. A rebuilt
    /// index carries new ComputedStyle for the same DOM, which CSSStyleSheet
    /// insertRule produces while both DOM generations stand still.
    taffy_style_index: u64,
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
    #[must_use]
    pub fn new() -> Self {
        Self {
            table: LayoutNeighborTable::default(),
            node_transforms: Vec::new(),
            stacking: StackingOrder::default(),
            paint_order: Vec::new(),
            rendered: Vec::new(),
            cascade_view: CascadeView::new(),
            cascade_ws: CascadeWorkspace::new(0),
            taffy_layout: TaffyLayout::new(),
            styles: Vec::new(),
            node_rects: Vec::new(),
            node_borders: Vec::new(),
            display_items: Vec::new(),
            table_generation: u64::MAX,
            cascade_generation: u64::MAX,
            taffy_structure_generation: u64::MAX,
            taffy_viewport: Rect {
                x: f32::NAN,
                y: f32::NAN,
                width: f32::NAN,
                height: f32::NAN,
            },
            taffy_style_generation: u64::MAX,
            taffy_style_index: u64::MAX,
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
        self.run_with_replaced_sizes(dom, stylesheet, style_index, root, viewport, &[]);
    }

    pub fn run_with_replaced_sizes(
        &mut self,
        dom: &Dom,
        stylesheet: &Stylesheet,
        style_index: &StyleIndex,
        root: NodeId,
        viewport: Rect,
        replaced_sizes: &[ReplacedSize],
    ) {
        let trace_fused = std::env::var_os("SILKSURF_TRACE_FUSED").is_some();
        let total_start = std::time::Instant::now();
        /*
         * DOM structure and selector-input generations separate text edits
         * from tree or attribute changes. Text-only mutations keep the BFS
         * table, cascade view, and taffy node graph warm while layout computes
         * with the updated text contents.
         */
        let structure_gen = dom.structure_generation();
        let style_gen = dom.style_generation();
        let phase_start = std::time::Instant::now();
        if structure_gen != self.table_generation {
            self.table
                .rebuild_filtered(dom, root, node_starts_non_rendered_subtree);
            self.table_generation = structure_gen;
        }
        if style_gen != self.cascade_generation {
            self.cascade_view.rebuild(dom);
            self.cascade_generation = style_gen;
        }
        let n = self.table.len();
        trace_fused_phase(
            trace_fused,
            "table",
            phase_start.elapsed(),
            n,
            style_index.active_rules.len(),
            0,
        );

        self.styles.clear();
        self.styles.resize(n, None);
        self.node_rects.clear();
        self.node_rects.resize(n, viewport);
        self.node_borders.clear();
        self.node_borders.resize(n, EdgeSizes::default());
        self.display_items.clear();
        let root_suppressed = node_starts_non_rendered_subtree(dom, root);

        // Pass 1: cascade -- compute ComputedStyle for every BFS node.
        // Each node reads its parent's style (already computed, since BFS
        // processes parents before children).
        let phase_start = std::time::Instant::now();
        let mut rem_base_px = 16.0_f32;
        let mut any_transform = false;
        let mut any_positioned = false;
        for (i, &node) in self.table.bfs_order.iter().enumerate() {
            let pidx = self.table.parent_idx[i];
            let parent_style = if pidx == u32::MAX {
                None
            } else {
                self.styles[pidx as usize].as_ref()
            };
            let mut style = compute_style_for_node_with_workspace(
                dom,
                node,
                stylesheet,
                style_index,
                parent_style,
                &mut self.cascade_ws,
                Some(&self.cascade_view),
                rem_base_px,
                (viewport.width, viewport.height),
            );
            if root_suppressed {
                style.display = Display::None;
            }
            any_transform |= !style.transform.is_none();
            any_positioned |= style.position != CssPosition::Static;
            apply_replaced_size(dom, node, &mut style, replaced_sizes);
            if dom
                .element_name(node)
                .ok()
                .flatten()
                .is_some_and(|n| n.eq_ignore_ascii_case("html"))
                && let silksurf_css::Length::Px(v) = style.font_size
            {
                rem_base_px = v;
            }
            self.styles[i] = Some(style);
        }
        trace_fused_phase(
            trace_fused,
            "cascade",
            phase_start.elapsed(),
            n,
            style_index.active_rules.len(),
            0,
        );

        // Pass 2: layout -- rebuild taffy tree from styles and compute
        // Flexbox/Grid positions, then write absolute rects into node_rects[].
        let phase_start = std::time::Instant::now();
        if structure_gen != self.taffy_structure_generation
            || style_gen != self.taffy_style_generation
            || viewport != self.taffy_viewport
            || style_index.build_id() != self.taffy_style_index
        {
            self.taffy_layout.rebuild(dom, &self.table, &self.styles);
            self.taffy_structure_generation = structure_gen;
            self.taffy_style_generation = style_gen;
            self.taffy_viewport = viewport;
            self.taffy_style_index = style_index.build_id();
        }
        trace_fused_phase(
            trace_fused,
            "taffy-rebuild",
            phase_start.elapsed(),
            n,
            style_index.active_rules.len(),
            0,
        );
        let phase_start = std::time::Instant::now();
        self.taffy_layout
            .compute(dom, &self.styles, &self.table.bfs_order, viewport);
        trace_fused_phase(
            trace_fused,
            "taffy-compute",
            phase_start.elapsed(),
            n,
            style_index.active_rules.len(),
            0,
        );
        let phase_start = std::time::Instant::now();
        self.taffy_layout
            .write_rects(&self.table.parent_idx, &mut self.node_rects, viewport);
        self.taffy_layout
            .write_border_insets(&mut self.node_borders);
        trace_fused_phase(
            trace_fused,
            "rects",
            phase_start.elapsed(),
            n,
            style_index.active_rules.len(),
            0,
        );

        // Pass 3: paint -- emit display items for each visible node.
        let phase_start = std::time::Instant::now();
        let transformed = accumulate_paint_transforms(
            &self.table,
            &self.styles,
            &self.node_rects,
            &mut self.node_transforms,
            any_transform,
        );
        mark_rendered_boxes(&self.table, &self.styles, &mut self.rendered);
        let stacked = build_paint_order(
            &self.table,
            &self.styles,
            &mut self.stacking,
            &mut self.paint_order,
            any_positioned,
        );
        let paint_steps = if stacked {
            self.paint_order.len()
        } else {
            self.table.len()
        };
        for step in 0..paint_steps {
            let i = paint_step(&self.paint_order, stacked, step);
            let Some(node) = self.table.bfs_order.get(i).copied() else {
                continue;
            };
            if !self.rendered[i] {
                continue;
            }
            let Some(ref style) = self.styles[i] else {
                continue;
            };
            if text_node_collapses_to_empty_render(dom, &self.table, &self.styles, i) {
                continue;
            }
            let paint_transform = if transformed {
                self.node_transforms[i]
            } else {
                PaintTransform::IDENTITY
            };
            let rect = paint_transform.apply(self.node_rects[i]);
            emit_workspace_paint(
                dom,
                node,
                style,
                rect,
                paint_transform.font_scale(),
                &mut self.display_items,
            );
        }
        trace_fused_phase(
            trace_fused,
            "paint",
            phase_start.elapsed(),
            n,
            style_index.active_rules.len(),
            self.display_items.len(),
        );
        trace_fused_phase(
            trace_fused,
            "total",
            total_start.elapsed(),
            n,
            style_index.active_rules.len(),
            self.display_items.len(),
        );
    }

    /// Number of BFS-ordered nodes from the last `run()` call.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.table.len()
    }

    /// BFS traversal table from the last `run()` call.
    #[must_use]
    pub fn table(&self) -> &LayoutNeighborTable {
        &self.table
    }

    /// Clone the current workspace output into the owned result shape.
    #[must_use]
    pub fn snapshot_result(&self) -> FusedResult {
        FusedResult {
            styles: self.styles.clone(),
            display_items: self.display_items.clone(),
            node_rects: self.node_rects.clone(),
            node_borders: self.node_borders.clone(),
            table: self.table.clone(),
        }
    }

    /// Move the current workspace output into the owned result shape.
    #[must_use]
    pub fn take_result(&mut self) -> FusedResult {
        FusedResult {
            styles: std::mem::take(&mut self.styles),
            display_items: std::mem::take(&mut self.display_items),
            node_rects: std::mem::take(&mut self.node_rects),
            node_borders: std::mem::take(&mut self.node_borders),
            table: self.table.clone(),
        }
    }

    /// Recycle result vector storage for the next workspace run.
    pub fn recycle_result_storage(&mut self, mut result: FusedResult) {
        self.styles = std::mem::take(&mut result.styles);
        self.display_items = std::mem::take(&mut result.display_items);
        self.node_rects = std::mem::take(&mut result.node_rects);
        self.node_borders = std::mem::take(&mut result.node_borders);
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
 * WHY row-oriented styles: a column-oriented StyleSoA view was measured at
 * ~4us of construction cost for 50 nodes (FxHashMap insertions + 25 Vec
 * pushes), which eliminates the fused pipeline's speedup advantage over
 * the 3-pass baseline. The speculative StyleSoA surface is removed on that
 * evidence; the SoA idea lives where it pays -- CascadeView materializes a
 * compact column view for the cascade hot path (silksurf_css cascade_view).
 */
pub struct FusedResult {
    /// Style per node in BFS order. None for display:none or skipped nodes.
    pub styles: Vec<Option<ComputedStyle>>,
    pub display_items: Vec<DisplayItem>,
    /// Border-box rect per node in BFS order, in document coordinates.
    pub node_rects: Vec<Rect>,
    /// Border widths per node in BFS order.
    pub node_borders: Vec<EdgeSizes>,
    /// BFS traversal table; use `node_to_bfs_idx` for `NodeId` -> index mapping.
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
    fused_style_layout_paint_with_replaced_sizes(dom, stylesheet, root, viewport, &[])
}

pub fn fused_style_layout_paint_with_replaced_sizes(
    dom: &Dom,
    stylesheet: &Stylesheet,
    root: NodeId,
    viewport: Rect,
    replaced_sizes: &[ReplacedSize],
) -> FusedResult {
    let trace_fused = std::env::var_os("SILKSURF_TRACE_FUSED").is_some();
    let total_start = std::time::Instant::now();
    let phase_start = std::time::Instant::now();
    /*
     * Build StyleIndex once for all nodes.
     *
     * compute_style_for_node builds StyleIndex from the stylesheet. This fused
     * path builds the index once and passes it to every node cascade.
     */
    let style_index = StyleIndex::new(stylesheet);
    trace_fused_phase(
        trace_fused,
        "style-index",
        phase_start.elapsed(),
        0,
        style_index.active_rules.len(),
        0,
    );
    let phase_start = std::time::Instant::now();
    /*
     * CascadeWorkspace allocates once and serves every node in the BFS
     * traversal. The matched-rule, candidate, seen-bit, and class-key buffers
     * stay owned by the workspace.
     */
    let mut cascade_ws = CascadeWorkspace::new(style_index.active_rules.len());
    let table = LayoutNeighborTable::build_filtered(dom, root, node_starts_non_rendered_subtree);
    let n = table.len();
    trace_fused_phase(
        trace_fused,
        "table",
        phase_start.elapsed(),
        n,
        style_index.active_rules.len(),
        0,
    );

    let mut styles: Vec<Option<ComputedStyle>> = vec![None; n];
    let mut node_rects: Vec<Rect> = vec![viewport; n];
    let mut display_items: Vec<DisplayItem> = Vec::new();
    let root_suppressed = node_starts_non_rendered_subtree(dom, root);

    // Pass 1: cascade
    let phase_start = std::time::Instant::now();
    let mut rem_base_px = 16.0_f32;
    let mut any_transform = false;
    let mut any_positioned = false;
    for (i, &node) in table.bfs_order.iter().enumerate() {
        let pidx = table.parent_idx[i];
        let parent_style = if pidx == u32::MAX {
            None
        } else {
            styles[pidx as usize].as_ref()
        };
        let mut style = compute_style_for_node_with_workspace(
            dom,
            node,
            stylesheet,
            &style_index,
            parent_style,
            &mut cascade_ws,
            None,
            rem_base_px,
            (viewport.width, viewport.height),
        );
        if root_suppressed {
            style.display = Display::None;
        }
        any_transform |= !style.transform.is_none();
        any_positioned |= style.position != CssPosition::Static;
        apply_replaced_size(dom, node, &mut style, replaced_sizes);
        if dom
            .element_name(node)
            .ok()
            .flatten()
            .is_some_and(|n| n.eq_ignore_ascii_case("html"))
            && let silksurf_css::Length::Px(v) = style.font_size
        {
            rem_base_px = v;
        }
        styles[i] = Some(style);
    }
    trace_fused_phase(
        trace_fused,
        "cascade",
        phase_start.elapsed(),
        n,
        style_index.active_rules.len(),
        0,
    );

    // Pass 2: taffy layout
    let phase_start = std::time::Instant::now();
    let mut taffy_layout = TaffyLayout::new();
    taffy_layout.rebuild(dom, &table, &styles);
    trace_fused_phase(
        trace_fused,
        "taffy-rebuild",
        phase_start.elapsed(),
        n,
        style_index.active_rules.len(),
        0,
    );
    let phase_start = std::time::Instant::now();
    taffy_layout.compute(dom, &styles, &table.bfs_order, viewport);
    trace_fused_phase(
        trace_fused,
        "taffy-compute",
        phase_start.elapsed(),
        n,
        style_index.active_rules.len(),
        0,
    );
    let phase_start = std::time::Instant::now();
    taffy_layout.write_rects(&table.parent_idx, &mut node_rects, viewport);
    let mut node_borders = vec![EdgeSizes::default(); node_rects.len()];
    taffy_layout.write_border_insets(&mut node_borders);
    trace_fused_phase(
        trace_fused,
        "rects",
        phase_start.elapsed(),
        n,
        style_index.active_rules.len(),
        0,
    );

    // Pass 3: paint
    let phase_start = std::time::Instant::now();
    let mut node_transforms = Vec::new();
    let transformed = accumulate_paint_transforms(
        &table,
        &styles,
        &node_rects,
        &mut node_transforms,
        any_transform,
    );
    let mut stacking = StackingOrder::default();
    let mut paint_order = Vec::new();
    let mut rendered = Vec::new();
    mark_rendered_boxes(&table, &styles, &mut rendered);
    let stacked = build_paint_order(
        &table,
        &styles,
        &mut stacking,
        &mut paint_order,
        any_positioned,
    );
    let paint_steps = if stacked {
        paint_order.len()
    } else {
        table.len()
    };
    for step in 0..paint_steps {
        let i = paint_step(&paint_order, stacked, step);
        let Some(node) = table.bfs_order.get(i).copied() else {
            continue;
        };
        if !rendered[i] {
            continue;
        }
        let Some(ref style) = styles[i] else {
            continue;
        };
        if text_node_collapses_to_empty_render(dom, &table, &styles, i) {
            continue;
        }
        let paint_transform = if transformed {
            node_transforms[i]
        } else {
            PaintTransform::IDENTITY
        };
        let rect = paint_transform.apply(node_rects[i]);
        emit_allocating_paint(
            dom,
            node,
            style,
            rect,
            paint_transform.font_scale(),
            &mut display_items,
        );
    }
    trace_fused_phase(
        trace_fused,
        "paint",
        phase_start.elapsed(),
        n,
        style_index.active_rules.len(),
        display_items.len(),
    );
    trace_fused_phase(
        trace_fused,
        "total",
        total_start.elapsed(),
        n,
        style_index.active_rules.len(),
        display_items.len(),
    );

    FusedResult {
        styles,
        display_items,
        node_rects,
        node_borders,
        table,
    }
}

fn trace_fused_phase(
    enabled: bool,
    phase: &str,
    elapsed: std::time::Duration,
    nodes: usize,
    active_rules: usize,
    display_items: usize,
) {
    if enabled {
        eprintln!(
            "[SilkSurf] fused {phase}: {elapsed:?}, nodes={nodes}, active_rules={active_rules}, display_items={display_items}"
        );
    }
}

fn apply_replaced_size(
    dom: &Dom,
    node: NodeId,
    style: &mut ComputedStyle,
    replaced_sizes: &[ReplacedSize],
) {
    if style.display == Display::None || !is_image_element(dom, node) {
        return;
    }
    if style.width == LengthOrAuto::Auto
        && let Some(width) = image_replaced_width(node, replaced_sizes)
    {
        style.width = LengthOrAuto::Length(Length::Px(width));
    }
    if style.height == LengthOrAuto::Auto
        && let Some(height) = image_replaced_height(node, replaced_sizes)
    {
        style.height = LengthOrAuto::Length(Length::Px(height));
    }
}

fn image_replaced_width(node: NodeId, replaced_sizes: &[ReplacedSize]) -> Option<f32> {
    replaced_sizes
        .iter()
        .find(|size| size.node == node && size.width > 0.0)
        .map(|size| size.width)
}

fn image_replaced_height(node: NodeId, replaced_sizes: &[ReplacedSize]) -> Option<f32> {
    replaced_sizes
        .iter()
        .find(|size| size.node == node && size.height > 0.0)
        .map(|size| size.height)
}

fn is_image_element(dom: &Dom, node: NodeId) -> bool {
    // Canvas is a replaced element too: its intrinsic size comes from the
    // width/height attributes, substituted the same way as an image's.
    dom.element_name(node)
        .ok()
        .flatten()
        .is_some_and(|name| matches!(TagName::from_str(name), TagName::Img | TagName::Canvas))
}

/*
 * mark_rendered_boxes -- which nodes generate a box.
 *
 * CSS Display 3 3 makes `display: none` suppress the element's box and the
 * boxes of every descendant, so a subtree under a none-valued ancestor takes
 * no part in layout or paint. taffy already collapses such a subtree to zero
 * size, and the paint pass previously tested only the node's own display, so
 * every descendant emitted its text at the collapsed origin. chatgpt.com
 * carries its deferred UI that way, and the collapsed subtrees stacked several
 * hundred text runs into one illegible band.
 *
 * BFS order puts a parent before its children, so one forward pass carries the
 * suppression down the tree.
 */
fn mark_rendered_boxes(
    table: &LayoutNeighborTable,
    styles: &[Option<ComputedStyle>],
    rendered: &mut Vec<bool>,
) {
    let n = table.len().min(styles.len());
    rendered.clear();
    rendered.resize(n, false);
    for (i, style) in styles.iter().take(n).enumerate() {
        let generates_box = style
            .as_ref()
            .is_some_and(|style| style.display != Display::None);
        let parent_renders = table
            .parent_idx
            .get(i)
            .copied()
            .filter(|&parent| (parent as usize) < i)
            .is_none_or(|parent| rendered[parent as usize]);
        rendered[i] = generates_box && parent_renders;
    }
}

/// The BFS index to paint at `step`: the stacking sequence when one was built,
/// and plain tree order otherwise. One predictable branch per node keeps a
/// document of ordinary flow content off the indirect-call path.
#[inline]
fn paint_step(order: &[u32], stacked: bool, step: usize) -> usize {
    if stacked { order[step] as usize } else { step }
}

/*
 * StackingOrder -- the sequence in which nodes hand their display items to
 * the painter.
 *
 * CSS 2.1 Appendix E orders one stacking context as: the context element's own
 * box, then descendant contexts with a negative z-index, then the context's
 * in-flow content, then descendant contexts with a zero or positive z-index,
 * each z group in tree order. Contexts nest, so the sequence is a depth-first
 * walk of the context tree rather than one flat sort -- a z-index-3 pane paints
 * its own background before the subtree it contains, and that subtree stays
 * above it however its members' own z-index reads.
 *
 * `ComputedStyle::z_index` resolves `auto` to 0, so this treats every
 * positioned element as establishing a context. The spec lets a positioned
 * z-auto element's positioned descendants escape into the ancestor context;
 * `docs/roadmaps/SPA-CAPABILITY-ROADMAP.md` carries that as
 * z-index-auto-context-escape.
 *
 * Every buffer is retained across repaints, and the whole pass runs only when
 * the cascade reports a positioned node.
 */
#[derive(Default)]
pub(crate) struct StackingOrder {
    /// Nearest positioned ancestor-or-self per BFS index; `context[i] == i`
    /// marks a context root, and index 0 is the root context.
    context: Vec<u32>,
    /// The context holding each context root, which is the context of its DOM
    /// parent. Index 0 carries itself.
    parent_context: Vec<u32>,
    /// Computed z-index per BFS index, read while ordering child contexts.
    z: Vec<i32>,
    /// BFS indices grouped by context, each run in tree order.
    members: Vec<u32>,
    /// First `members` slot belonging to the context rooted at this index.
    member_start: Vec<u32>,
    /// Context roots grouped by parent context, each run ordered by z-index
    /// then tree order.
    children: Vec<u32>,
    /// First `children` slot belonging to the context rooted at this index.
    child_start: Vec<u32>,
    /// Depth-first walk state, one frame per open context.
    frames: Vec<StackingFrame>,
}

#[derive(Clone, Copy)]
struct StackingFrame {
    context: u32,
    child: usize,
    child_end: usize,
    member: usize,
    member_end: usize,
    members_emitted: bool,
}

/*
 * build_paint_order -- sequence the BFS indices by stacking context.
 *
 * `any_positioned` is the cascade's report that at least one node resolved a
 * position other than static; false leaves `order` untouched and the caller
 * walks `bfs_order` directly, which keeps a document of ordinary flow content
 * at the cost it had before stacking existed.
 */
fn build_paint_order(
    table: &LayoutNeighborTable,
    styles: &[Option<ComputedStyle>],
    state: &mut StackingOrder,
    order: &mut Vec<u32>,
    any_positioned: bool,
) -> bool {
    if !any_positioned {
        return false;
    }
    let n = table.len().min(styles.len());
    if n == 0 {
        return false;
    }
    assign_contexts(table, styles, state, n);
    group_members_by_context(state, n);
    group_children_by_parent_context(state, n);
    emit_stacking_order(state, order, n);
    true
}

/// A node's stacking context: itself when it is positioned, and its parent's
/// context otherwise. BFS order puts a parent before its children, so one
/// forward pass resolves the whole tree.
fn assign_contexts(
    table: &LayoutNeighborTable,
    styles: &[Option<ComputedStyle>],
    state: &mut StackingOrder,
    n: usize,
) {
    state.context.clear();
    state.context.resize(n, 0);
    state.parent_context.clear();
    state.parent_context.resize(n, 0);
    state.z.clear();
    state.z.resize(n, 0);
    for (i, style) in styles.iter().take(n).enumerate() {
        let index = u32::try_from(i).unwrap_or(u32::MAX);
        let style = style.as_ref();
        let positioned = style.is_some_and(|style| style.position != CssPosition::Static);
        state.z[i] = style.map_or(0, |style| style.z_index);
        let parent_context = table
            .parent_idx
            .get(i)
            .copied()
            .filter(|&parent| (parent as usize) < i)
            .map_or(0, |parent| state.context[parent as usize]);
        state.parent_context[i] = parent_context;
        state.context[i] = if i == 0 || positioned {
            index
        } else {
            parent_context
        };
    }
}

/// `members` holds every BFS index grouped into a contiguous run per context,
/// tree order preserved inside each run; `member_start` indexes those runs by
/// context root.
fn group_members_by_context(state: &mut StackingOrder, n: usize) {
    state.members.clear();
    state
        .members
        .extend(0..u32::try_from(n).unwrap_or(u32::MAX));
    // The BFS index is the second key: an unstable sort reorders equal keys,
    // and emit_stacking_order reads each run as tree order.
    let context = &state.context;
    state
        .members
        .sort_unstable_by_key(|&i| (context[i as usize], i));
    state.member_start.clear();
    state.member_start.resize(n + 1, u32::MAX);
    for (slot, &i) in state.members.iter().enumerate() {
        let context_root = state.context[i as usize] as usize;
        let slot = u32::try_from(slot).unwrap_or(u32::MAX);
        if state.member_start[context_root] == u32::MAX {
            state.member_start[context_root] = slot;
        }
    }
}

/// `children` holds every context root except the root context, grouped into a
/// contiguous run per parent context and ordered by z-index then tree order;
/// `child_start` indexes those runs by parent context root.
fn group_children_by_parent_context(state: &mut StackingOrder, n: usize) {
    state.children.clear();
    for i in 1..n {
        if state.context[i] as usize == i {
            state.children.push(u32::try_from(i).unwrap_or(u32::MAX));
        }
    }
    let (parent_context, z) = (&state.parent_context, &state.z);
    state
        .children
        .sort_unstable_by_key(|&i| (parent_context[i as usize], z[i as usize], i));
    state.child_start.clear();
    state.child_start.resize(n + 1, u32::MAX);
    for (slot, &i) in state.children.iter().enumerate() {
        let owner = state.parent_context[i as usize] as usize;
        let slot = u32::try_from(slot).unwrap_or(u32::MAX);
        if state.child_start[owner] == u32::MAX {
            state.child_start[owner] = slot;
        }
    }
}

/// The half-open `slots` run that starts at `start[key]`, ending where the run
/// of a different key begins.
fn run_of(start: &[u32], slots: &[u32], owner: impl Fn(u32) -> u32, key: u32) -> (usize, usize) {
    let Some(&first) = start.get(key as usize).filter(|&&s| s != u32::MAX) else {
        return (0, 0);
    };
    let mut end = first as usize;
    while end < slots.len() && owner(slots[end]) == key {
        end += 1;
    }
    (first as usize, end)
}

/// Walk the context tree depth-first, writing the painting sequence into
/// `order`. An explicit frame stack keeps a deep document off the call stack.
fn emit_stacking_order(state: &mut StackingOrder, order: &mut Vec<u32>, n: usize) {
    order.clear();
    order.reserve(n);
    state.frames.clear();
    order.push(0);
    state.frames.push(open_context(state, 0));
    while let Some(&frame) = state.frames.last() {
        let mut frame = frame;
        // A negative z-index context paints between the parent context's own
        // box and the parent's in-flow content.
        let next_child = (frame.child < frame.child_end).then(|| state.children[frame.child]);
        if !frame.members_emitted
            && let Some(child) = next_child
            && state.z[child as usize] < 0
        {
            frame.child += 1;
            *replace_top(state) = frame;
            order.push(child);
            let opened = open_context(state, child);
            state.frames.push(opened);
            continue;
        }
        if !frame.members_emitted {
            while frame.member < frame.member_end {
                let member = state.members[frame.member];
                frame.member += 1;
                if member != frame.context {
                    order.push(member);
                }
            }
            frame.members_emitted = true;
        }
        if frame.child < frame.child_end {
            let child = state.children[frame.child];
            frame.child += 1;
            *replace_top(state) = frame;
            order.push(child);
            let opened = open_context(state, child);
            state.frames.push(opened);
            continue;
        }
        state.frames.pop();
    }
}

fn replace_top(state: &mut StackingOrder) -> &mut StackingFrame {
    let top = state.frames.len() - 1;
    &mut state.frames[top]
}

fn open_context(state: &StackingOrder, context: u32) -> StackingFrame {
    let (member, member_end) = run_of(
        &state.member_start,
        &state.members,
        |i| state.context[i as usize],
        context,
    );
    let (child, child_end) = run_of(
        &state.child_start,
        &state.children,
        |i| state.parent_context[i as usize],
        context,
    );
    StackingFrame {
        context,
        child,
        child_end,
        member,
        member_end,
        members_emitted: false,
    }
}

/*
 * PaintTransform -- the part of a CSS transform that keeps a rect a rect.
 *
 * A scale and a translation map an axis-aligned rect onto another axis-aligned
 * rect, so they fold into the `Rect` every DisplayItem already carries and
 * reach neither the three rasterizers nor the tiling, hit-test, and damage
 * code that assume axis alignment. Rotation and skew do not, and AD-031 cuts
 * them by name as `transform-rotation-and-skew`.
 *
 * The mapping is `x' = scale_x * x + dx` on each axis.
 */
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaintTransform {
    scale_x: f32,
    scale_y: f32,
    dx: f32,
    dy: f32,
}

impl PaintTransform {
    const IDENTITY: Self = Self {
        scale_x: 1.0,
        scale_y: 1.0,
        dx: 0.0,
        dy: 0.0,
    };

    fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }

    /// `self` applied to the result of `inner`, which is the order CSS
    /// Transforms 1, 7.1 multiplies a function list in and the order an
    /// ancestor's transform applies to a descendant's.
    fn then(self, inner: Self) -> Self {
        Self {
            scale_x: self.scale_x * inner.scale_x,
            scale_y: self.scale_y * inner.scale_y,
            dx: self.scale_x * inner.dx + self.dx,
            dy: self.scale_y * inner.dy + self.dy,
        }
    }

    /// Recentre on `origin`, which is where CSS Transforms 1, 6 anchors a
    /// transform. The default `50% 50%` is the only origin this engine reads.
    fn about(self, origin: (f32, f32)) -> Self {
        Self {
            scale_x: self.scale_x,
            scale_y: self.scale_y,
            dx: self.dx + origin.0 * (1.0 - self.scale_x),
            dy: self.dy + origin.1 * (1.0 - self.scale_y),
        }
    }

    fn apply(self, rect: Rect) -> Rect {
        Rect {
            x: self.scale_x * rect.x + self.dx,
            y: self.scale_y * rect.y + self.dy,
            width: self.scale_x * rect.width,
            height: self.scale_y * rect.height,
        }
    }

    /// The factor a glyph run's font size takes. cosmic-text re-rasterizes at
    /// the new size rather than magnifying a coverage bitmap, so a scaled run
    /// is shaped rather than resampled.
    fn font_scale(self) -> f32 {
        self.scale_y
    }
}

/// One node's own transform, composed from its function list about its
/// border-box centre.
fn local_paint_transform(style: &ComputedStyle, rect: Rect) -> PaintTransform {
    let mut composed = PaintTransform::IDENTITY;
    for function in style.transform.functions() {
        composed = composed.then(paint_transform_for(style, function, rect));
    }
    composed.about((rect.x + rect.width / 2.0, rect.y + rect.height / 2.0))
}

fn paint_transform_for(
    style: &ComputedStyle,
    function: &silksurf_css::TransformFunction,
    rect: Rect,
) -> PaintTransform {
    match function {
        silksurf_css::TransformFunction::Translate { x, y } => PaintTransform {
            dx: translation_px(style, *x, rect.width),
            dy: translation_px(style, *y, rect.height),
            ..PaintTransform::IDENTITY
        },
        silksurf_css::TransformFunction::Scale { x, y } => PaintTransform {
            scale_x: *x,
            scale_y: *y,
            ..PaintTransform::IDENTITY
        },
        // A matrix whose b or c term is non-zero rotates or skews, and its a
        // and d terms are then cosines rather than scale factors: reading them
        // as scale would shrink the box toward zero at 90 degrees.
        silksurf_css::TransformFunction::Matrix { a, b, c, d, e, f } if *b == 0.0 && *c == 0.0 => {
            PaintTransform {
                scale_x: *a,
                scale_y: *d,
                dx: *e,
                dy: *f,
            }
        }
        silksurf_css::TransformFunction::Rotate { .. }
        | silksurf_css::TransformFunction::Skew { .. }
        | silksurf_css::TransformFunction::Matrix { .. } => PaintTransform::IDENTITY,
    }
}

/*
 * accumulate_paint_transforms -- compose each node's transform with its
 * ancestors' and record the result per node.
 *
 * `any_transform` is the cascade's report that at least one node declared a
 * transform; false skips the pass entirely, which keeps the repaint hot path
 * at the cost it had before transforms existed.
 *
 * CSS Transforms 1, 3 makes a transform apply to the element and everything it
 * contains, so the transform composes down the tree. BFS order guarantees a
 * parent is resolved before its children, which is what lets one forward pass
 * over `bfs_order` carry the product. A parent scale multiplies a child's
 * translation as well as its size, which plain offset addition could not
 * express.
 *
 * The rects layout produced stay the input to hit testing and damage; only
 * the paint rect moves, matching the CSS rule that a transform takes no
 * layout space.
 */
fn accumulate_paint_transforms(
    table: &LayoutNeighborTable,
    styles: &[Option<ComputedStyle>],
    node_rects: &[Rect],
    transforms: &mut Vec<PaintTransform>,
    any_transform: bool,
) -> bool {
    // The cascade pass already visited every node and reported whether any
    // declared a transform, so a document with none pays one bool here rather
    // than a second walk of the tree on the repaint path.
    if !any_transform {
        return false;
    }
    transforms.clear();
    transforms.resize(node_rects.len(), PaintTransform::IDENTITY);
    let mut any = false;
    for (i, _) in table.bfs_order.iter().enumerate() {
        // parent_idx carries u32::MAX for the root, which has no ancestor
        // transform to inherit.
        let parent = table
            .parent_idx
            .get(i)
            .copied()
            .filter(|&parent| (parent as usize) < transforms.len());
        let inherited = parent.map_or(PaintTransform::IDENTITY, |p| transforms[p as usize]);
        let combined = styles[i].as_ref().map_or(inherited, |style| {
            inherited.then(local_paint_transform(style, node_rects[i]))
        });
        any |= !combined.is_identity();
        transforms[i] = combined;
    }
    any
}

/// A length in pixels, with `fallback` standing in for a relative unit whose
/// basis this pass does not carry.
fn length_px(length: Length, fallback: f32) -> f32 {
    match length {
        Length::Px(value) => value,
        Length::Em(value) | Length::Rem(value) => value * fallback,
        Length::Percent(_) | Length::Vw(_) | Length::Vh(_) | Length::Vmin(_) | Length::Vmax(_) => {
            fallback
        }
        Length::Calc(_) => fallback,
    }
}

/// One translation component in pixels. A percentage resolves against the
/// element's own border-box extent along that axis.
fn translation_px(style: &ComputedStyle, length: Length, extent: f32) -> f32 {
    match length {
        Length::Px(value) => value,
        Length::Percent(value) => extent * value / 100.0,
        Length::Em(value) => value * length_px(style.font_size, 16.0),
        Length::Rem(value) => value * 16.0,
        // The cascade resolves viewport units to px, so a translation still
        // carrying one came from a caller that skipped resolve.
        Length::Vw(value) | Length::Vh(value) | Length::Vmin(value) | Length::Vmax(value) => value,
        Length::Calc(_) => style
            .resolve_calc_length(length, extent, (0.0, 0.0))
            .unwrap_or(0.0),
    }
}

fn emit_workspace_paint(
    dom: &Dom,
    node: NodeId,
    style: &ComputedStyle,
    content_rect: Rect,
    font_scale: f32,
    display_items: &mut Vec<DisplayItem>,
) {
    emit_box_shadow(style, content_rect, display_items);
    emit_rounded_background(style, content_rect, display_items);
    emit_text_content(dom, node, style, content_rect, font_scale, display_items);
    emit_form_control_text(dom, node, style, content_rect, font_scale, display_items);
}

fn emit_allocating_paint(
    dom: &Dom,
    node: NodeId,
    style: &ComputedStyle,
    content_rect: Rect,
    font_scale: f32,
    display_items: &mut Vec<DisplayItem>,
) {
    emit_square_background(style, content_rect, display_items);
    emit_text_content(dom, node, style, content_rect, font_scale, display_items);
    emit_form_control_text(dom, node, style, content_rect, font_scale, display_items);
}

fn emit_box_shadow(style: &ComputedStyle, rect: Rect, display_items: &mut Vec<DisplayItem>) {
    // Box-shadow paints below the background in CSS paint order.
    if let Some(shadow) = style.box_shadow
        && !shadow.inset
    {
        display_items.push(DisplayItem::BoxShadow { rect, shadow });
    }
}

fn emit_rounded_background(
    style: &ComputedStyle,
    rect: Rect,
    display_items: &mut Vec<DisplayItem>,
) {
    if let Some(ref gradient) = style.background_image {
        display_items.push(DisplayItem::LinearGradient {
            rect,
            angle: gradient.angle_deg,
            stops: gradient.stops.clone(),
        });
    } else if style.background_color.a > 0 {
        emit_solid_or_rounded_rect(style, rect, display_items);
    }
}

fn emit_square_background(style: &ComputedStyle, rect: Rect, display_items: &mut Vec<DisplayItem>) {
    if let Some(ref gradient) = style.background_image {
        display_items.push(DisplayItem::LinearGradient {
            rect,
            angle: gradient.angle_deg,
            stops: gradient.stops.clone(),
        });
    } else if style.background_color.a > 0 {
        display_items.push(DisplayItem::SolidColor {
            rect,
            color: style.background_color,
        });
    }
}

fn emit_solid_or_rounded_rect(
    style: &ComputedStyle,
    rect: Rect,
    display_items: &mut Vec<DisplayItem>,
) {
    if style.border_radius > 0.0 {
        display_items.push(DisplayItem::RoundedRect {
            rect,
            radii: [style.border_radius; 4],
            color: style.background_color,
        });
    } else {
        display_items.push(DisplayItem::SolidColor {
            rect,
            color: style.background_color,
        });
    }
}

fn emit_text_content(
    dom: &Dom,
    node: NodeId,
    style: &ComputedStyle,
    rect: Rect,
    font_scale: f32,
    display_items: &mut Vec<DisplayItem>,
) {
    if let Ok(dom_node) = dom.node(node)
        && let NodeKind::Text { text } = dom_node.kind()
    {
        display_items.push(DisplayItem::Text {
            rect,
            node,
            text_len: text.len() as u32,
            text: text.clone(),
            font_size: font_size_px(style) * font_scale,
            color: style.color,
        });
    }
}

fn emit_form_control_text(
    dom: &Dom,
    node: NodeId,
    style: &ComputedStyle,
    content_rect: Rect,
    font_scale: f32,
    display_items: &mut Vec<DisplayItem>,
) {
    if is_form_control(dom, node)
        && let Some(text) = form_control_text(dom, node)
    {
        let rect = Rect {
            x: content_rect.x + 4.0,
            y: content_rect.y + 2.0,
            width: (content_rect.width - 8.0).max(1.0),
            height: (content_rect.height - 4.0).max(1.0),
        };
        display_items.push(DisplayItem::Text {
            rect,
            node,
            text_len: text.len() as u32,
            text,
            font_size: font_size_px(style) * font_scale,
            color: style.color,
        });
    }
}

fn font_size_px(style: &ComputedStyle) -> f32 {
    match style.font_size {
        silksurf_css::Length::Px(px) => px,
        _ => 16.0,
    }
}

fn is_form_control(dom: &Dom, node: NodeId) -> bool {
    dom.element_name(node).ok().flatten().is_some_and(|name| {
        matches!(
            TagName::from_str(name),
            TagName::Input | TagName::Textarea | TagName::Select
        )
    })
}

fn form_control_text(dom: &Dom, node: NodeId) -> Option<String> {
    if dom
        .element_name(node)
        .ok()
        .flatten()
        .is_some_and(|name| TagName::from_str(name) == TagName::Select)
    {
        return selected_option_text(dom, node);
    }
    let attrs = dom.attributes(node).ok()?;
    if input_type_matches(attrs, "checkbox") {
        return attrs
            .iter()
            .any(|attr| attr.name.as_str() == "checked")
            .then(|| "x".to_string());
    }
    if input_type_matches(attrs, "radio") {
        return attrs
            .iter()
            .any(|attr| attr.name.as_str() == "checked")
            .then(|| "*".to_string());
    }
    let value = attrs
        .iter()
        .find(|attr| attr.name.as_str() == "value")
        .map(|attr| attr.value.as_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| textarea_text(dom, node))
        .or_else(|| {
            attrs
                .iter()
                .find(|attr| attr.name.as_str() == "placeholder")
                .map(|attr| attr.value.as_str())
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })?;
    Some(value)
}

fn selected_option_text(dom: &Dom, select: NodeId) -> Option<String> {
    let mut options = Vec::new();
    collect_enabled_option_nodes(dom, select, &mut options);
    let option = options
        .iter()
        .copied()
        .find(|&option| option_selected(dom, option))
        .or_else(|| options.first().copied())?;
    let text = descendant_text(dom, option);
    (!text.is_empty()).then_some(text)
}

fn collect_enabled_option_nodes(dom: &Dom, node: NodeId, options: &mut Vec<NodeId>) {
    if dom
        .element_name(node)
        .ok()
        .flatten()
        .is_some_and(|name| TagName::from_str(name) == TagName::Option)
        && dom
            .attributes(node)
            .ok()
            .is_none_or(|attrs| attrs.iter().all(|attr| attr.name.as_str() != "disabled"))
    {
        options.push(node);
    }
    let Ok(children) = dom.children(node) else {
        return;
    };
    for &child in children {
        collect_enabled_option_nodes(dom, child, options);
    }
}

fn option_selected(dom: &Dom, option: NodeId) -> bool {
    dom.attributes(option)
        .ok()
        .is_some_and(|attrs| attrs.iter().any(|attr| attr.name.as_str() == "selected"))
}

fn input_type_matches(attrs: &[silksurf_dom::Attribute], target: &str) -> bool {
    attrs
        .iter()
        .find(|attr| attr.name.as_str() == "type")
        .is_some_and(|attr| attr.value.as_str().eq_ignore_ascii_case(target))
}

fn textarea_text(dom: &Dom, node: NodeId) -> Option<String> {
    let name = dom.element_name(node).ok().flatten()?;
    if TagName::from_str(name) != TagName::Textarea {
        return None;
    }
    let text = descendant_text(dom, node);
    (!text.is_empty()).then_some(text)
}

fn descendant_text(dom: &Dom, node: NodeId) -> String {
    let mut text = String::new();
    append_text_descendants(dom, node, &mut text);
    text
}

fn append_text_descendants(dom: &Dom, node: NodeId, text: &mut String) {
    let Ok(children) = dom.children(node) else {
        return;
    };
    for &child in children {
        if let Ok(dom_node) = dom.node(child)
            && let NodeKind::Text { text: child_text } = dom_node.kind()
        {
            text.push_str(child_text);
            continue;
        }
        append_text_descendants(dom, child, text);
    }
}

fn node_starts_non_rendered_subtree(dom: &Dom, node: NodeId) -> bool {
    let Ok(dom_node) = dom.node(node) else {
        return true;
    };
    match dom_node.kind() {
        NodeKind::Doctype { .. } | NodeKind::Comment { .. } => true,
        NodeKind::Element { name, .. } => matches!(
            name,
            TagName::Head
                | TagName::Title
                | TagName::Meta
                | TagName::Link
                | TagName::Script
                | TagName::Style
                | TagName::Option
        ),
        NodeKind::Document | NodeKind::Text { .. } => false,
    }
}

fn text_node_collapses_to_empty_render(
    dom: &Dom,
    table: &LayoutNeighborTable,
    styles: &[Option<ComputedStyle>],
    index: usize,
) -> bool {
    let Some(node_id) = table.bfs_order.get(index).copied() else {
        return false;
    };
    let Some(text) = text_node_contents(dom, node_id) else {
        return false;
    };
    if !collapsible_ascii_whitespace(text)
        || !style_collapses_whitespace(styles.get(index).and_then(Option::as_ref))
    {
        return false;
    }
    whitespace_parent_has_no_inline_text_flow(dom, table, styles, index)
}

fn text_node_contents(dom: &Dom, node: NodeId) -> Option<&str> {
    let node = dom.node(node).ok()?;
    match node.kind() {
        NodeKind::Text { text } => Some(text),
        _ => None,
    }
}

fn collapsible_ascii_whitespace(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_whitespace())
}

fn style_collapses_whitespace(style: Option<&ComputedStyle>) -> bool {
    matches!(
        style.map_or(WhiteSpace::Normal, |style| style.white_space),
        WhiteSpace::Normal | WhiteSpace::Nowrap
    )
}

fn whitespace_parent_has_no_inline_text_flow(
    dom: &Dom,
    table: &LayoutNeighborTable,
    styles: &[Option<ComputedStyle>],
    index: usize,
) -> bool {
    let parent = table.parent_idx.get(index).copied().unwrap_or(u32::MAX);
    if parent == u32::MAX {
        return false;
    }
    let Some(first_child) = table.child_start.get(parent as usize).copied() else {
        return false;
    };
    if first_child == u32::MAX {
        return false;
    }
    let start = first_child as usize;
    let end = start + usize::from(table.child_count[parent as usize]);
    let previous_keeps_space = index > start
        && node_participates_in_inline_text_flow(dom, table, styles, index.saturating_sub(1));
    let next_keeps_space =
        index + 1 < end && node_participates_in_inline_text_flow(dom, table, styles, index + 1);
    !previous_keeps_space && !next_keeps_space
}

fn node_participates_in_inline_text_flow(
    dom: &Dom,
    table: &LayoutNeighborTable,
    styles: &[Option<ComputedStyle>],
    index: usize,
) -> bool {
    let Some(node_id) = table.bfs_order.get(index).copied() else {
        return false;
    };
    if text_node_contents(dom, node_id).is_some_and(|text| !collapsible_ascii_whitespace(text)) {
        return true;
    }
    styles
        .get(index)
        .and_then(Option::as_ref)
        .is_some_and(|style| style.display == Display::Inline)
}

#[cfg(test)]
mod tests {
    use super::*;
    use silksurf_render::DisplayItem;

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

    #[test]
    fn metadata_subtrees_do_not_emit_text_items() {
        let document = silksurf_html::parse_html(
            "<!doctype html><html><head><title>Hidden title</title><style>body{color:red}</style></head><body><p>Visible body</p></body></html>",
        );
        let stylesheet = silksurf_css::parse_stylesheet("").unwrap();
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 800.0,
        };

        let result =
            fused_style_layout_paint(&document, &stylesheet, NodeId::from_raw(0), viewport);
        let text_items: Vec<&str> = result
            .display_items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(text_items, vec!["Visible body"]);
    }

    #[test]
    fn non_rendered_root_suppresses_descendant_text() {
        let mut dom = Dom::new();
        let script = dom.create_element("script");
        let text = dom.create_text("hidden script text");
        dom.append_child(script, text).unwrap();
        let stylesheet = silksurf_css::parse_stylesheet("").unwrap();
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 800.0,
        };

        let result = fused_style_layout_paint(&dom, &stylesheet, script, viewport);

        assert!(result.display_items.is_empty());
        assert!(
            result
                .styles
                .iter()
                .flatten()
                .all(|style| style.display == Display::None)
        );
    }

    #[test]
    fn block_indentation_whitespace_does_not_emit_text_items() {
        let document = silksurf_html::parse_html(
            "<!doctype html><html><body>\n  <main>\n    <p>Visible body</p>\n  </main>\n</body></html>",
        );
        let stylesheet = silksurf_css::parse_stylesheet(
            "html, body, main, p { display: block; white-space: normal; }",
        )
        .unwrap();
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };

        let result =
            fused_style_layout_paint(&document, &stylesheet, NodeId::from_raw(0), viewport);
        let text_items: Vec<&str> = result
            .display_items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(text_items, vec!["Visible body"]);
    }

    #[test]
    fn a_display_none_subtree_emits_no_text() {
        let document = silksurf_html::parse_html(
            "<!doctype html><html><body><p>Visible body</p>\
             <div class=\"gone\"><section><p>Hidden deep</p></section></div>\
             </body></html>",
        );
        let stylesheet = silksurf_css::parse_stylesheet(
            "html, body, p, div, section { display: block; } .gone { display: none; }",
        )
        .unwrap();
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };

        let result =
            fused_style_layout_paint(&document, &stylesheet, NodeId::from_raw(0), viewport);
        let text_items: Vec<&str> = result
            .display_items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(text_items, vec!["Visible body"]);
    }

    #[test]
    fn inline_text_flow_keeps_separator_whitespace() {
        let document = silksurf_html::parse_html(
            "<!doctype html><html><body><p><span>left</span> <span>right</span></p></body></html>",
        );
        let stylesheet = silksurf_css::parse_stylesheet(
            "html, body, p { display: block; } span { display: inline; }",
        )
        .unwrap();
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };

        let result =
            fused_style_layout_paint(&document, &stylesheet, NodeId::from_raw(0), viewport);
        let text_items: Vec<&str> = result
            .display_items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert!(text_items.contains(&" "));
    }

    #[test]
    fn textarea_text_content_emits_form_control_text() {
        let document = silksurf_html::parse_html(
            "<!doctype html><html><body><textarea>Prompt text</textarea></body></html>",
        );
        let stylesheet = silksurf_css::parse_stylesheet("").unwrap();
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 800.0,
        };

        let result =
            fused_style_layout_paint(&document, &stylesheet, NodeId::from_raw(0), viewport);
        let text_items: Vec<&str> = result
            .display_items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert!(text_items.contains(&"Prompt text"));
    }

    #[test]
    fn checked_controls_emit_form_control_markers() {
        let document = silksurf_html::parse_html(
            "<!doctype html><html><body><input type=\"checkbox\" checked><input type=\"radio\" checked></body></html>",
        );
        let stylesheet = silksurf_css::parse_stylesheet("").unwrap();
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 800.0,
        };

        let result =
            fused_style_layout_paint(&document, &stylesheet, NodeId::from_raw(0), viewport);
        let text_items: Vec<&str> = result
            .display_items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert!(text_items.contains(&"x"));
        assert!(text_items.contains(&"*"));
    }

    #[test]
    fn select_emits_selected_option_text_only() {
        let document = silksurf_html::parse_html(
            "<!doctype html><html><body><select><option value=\"old\">Old</option><option selected value=\"new\">New</option></select></body></html>",
        );
        let stylesheet = silksurf_css::parse_stylesheet("").unwrap();
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 800.0,
        };

        let result =
            fused_style_layout_paint(&document, &stylesheet, NodeId::from_raw(0), viewport);
        let text_items: Vec<&str> = result
            .display_items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert!(text_items.contains(&"New"));
        assert!(!text_items.contains(&"Old"));
    }
}

#[cfg(test)]
mod paint_order_tests {
    use super::*;
    use silksurf_dom::Dom;

    /// document > [flow, outer > inner, later_flow]
    ///
    /// BFS indices: 0 document, 1 flow, 2 outer, 3 later_flow, 4 inner.
    fn four_child_document() -> LayoutNeighborTable {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let flow = dom.create_element("div");
        let outer = dom.create_element("div");
        let inner = dom.create_element("div");
        let later_flow = dom.create_element("div");
        // UNWRAP-OK: every id came from this Dom, so no append can fail.
        dom.append_child(root, flow).unwrap();
        dom.append_child(root, outer).unwrap();
        dom.append_child(outer, inner).unwrap();
        dom.append_child(root, later_flow).unwrap();
        LayoutNeighborTable::build(&dom, root)
    }

    fn positioned(z: i32) -> ComputedStyle {
        ComputedStyle {
            position: CssPosition::Absolute,
            z_index: z,
            ..Default::default()
        }
    }

    fn order_for(styles: &[Option<ComputedStyle>], table: &LayoutNeighborTable) -> Vec<u32> {
        let mut state = StackingOrder::default();
        let mut order = Vec::new();
        assert!(build_paint_order(
            table, styles, &mut state, &mut order, true
        ));
        order
    }

    #[test]
    fn a_positioned_subtree_paints_after_later_in_flow_content() {
        let table = four_child_document();
        let mut styles = vec![Some(ComputedStyle::default()); 5];
        styles[2] = Some(ComputedStyle {
            position: CssPosition::Fixed,
            ..Default::default()
        });
        assert_eq!(order_for(&styles, &table), vec![0, 1, 3, 2, 4]);
    }

    #[test]
    fn a_negative_z_index_sinks_below_in_flow_content() {
        let table = four_child_document();
        let mut styles = vec![Some(ComputedStyle::default()); 5];
        styles[2] = Some(positioned(-1));
        assert_eq!(order_for(&styles, &table), vec![0, 2, 4, 1, 3]);
    }

    #[test]
    fn a_higher_z_index_paints_above_a_lower_one() {
        let table = four_child_document();
        let mut styles = vec![Some(ComputedStyle::default()); 5];
        styles[2] = Some(positioned(1));
        styles[3] = Some(positioned(5));
        assert_eq!(order_for(&styles, &table), vec![0, 1, 2, 4, 3]);
    }

    #[test]
    fn a_nested_context_paints_above_the_ancestor_that_contains_it() {
        let table = four_child_document();
        let mut styles = vec![Some(ComputedStyle::default()); 5];
        // The inner box carries the lower z-index, but it is inside the outer
        // context, so it still paints above the outer box.
        styles[2] = Some(positioned(3));
        styles[4] = Some(positioned(1));
        assert_eq!(order_for(&styles, &table), vec![0, 1, 3, 2, 4]);
    }

    #[test]
    fn a_negative_z_child_stays_inside_its_parent_context() {
        let table = four_child_document();
        let mut styles = vec![Some(ComputedStyle::default()); 5];
        styles[2] = Some(positioned(3));
        styles[4] = Some(positioned(-1));
        // The inner box sinks below the outer box's in-flow content, and the
        // outer box's own background still precedes it.
        assert_eq!(order_for(&styles, &table), vec![0, 1, 3, 2, 4]);
    }

    #[test]
    fn a_document_of_static_boxes_keeps_tree_order() {
        let table = four_child_document();
        let styles = vec![Some(ComputedStyle::default()); 5];
        let mut state = StackingOrder::default();
        let mut order = Vec::new();
        assert!(!build_paint_order(
            &table, &styles, &mut state, &mut order, false
        ));
        assert!(order.is_empty(), "the caller walks bfs_order directly");
        assert!(build_paint_order(
            &table, &styles, &mut state, &mut order, true
        ));
        assert_eq!(order, vec![0, 1, 2, 3, 4]);
    }

    /*
     * document > [left, right], each positioned, each with `width` in-flow
     * children that each carry one in-flow grandchild.
     *
     * BFS order interleaves the two contexts -- left owns the child band and
     * the grandchild band, right owns the slots between them -- so a context's
     * members are not contiguous in index order. That is the shape where an
     * unstable sort keyed on the context alone can reorder a context's members
     * against tree order.
     */
    fn interleaved_contexts(width: usize) -> (LayoutNeighborTable, Vec<Option<ComputedStyle>>) {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let mut branches = Vec::new();
        for _ in 0..2 {
            let branch = dom.create_element("div");
            // UNWRAP-OK: every id came from this Dom, so no append can fail.
            dom.append_child(root, branch).unwrap();
            branches.push(branch);
        }
        for &branch in &branches {
            for _ in 0..width {
                let child = dom.create_element("div");
                let grandchild = dom.create_element("div");
                // UNWRAP-OK: every id came from this Dom, so no append can fail.
                dom.append_child(branch, child).unwrap();
                dom.append_child(child, grandchild).unwrap();
            }
        }
        let table = LayoutNeighborTable::build(&dom, root);
        let mut styles = vec![Some(ComputedStyle::default()); table.len()];
        styles[1] = Some(positioned(0));
        styles[2] = Some(positioned(0));
        (table, styles)
    }

    #[test]
    fn each_context_emits_its_members_in_tree_order() {
        let (table, styles) = interleaved_contexts(24);
        let order = order_for(&styles, &table);
        assert_eq!(order.len(), table.len());
        // Within one stacking context, a member never precedes an earlier one.
        let mut state = StackingOrder::default();
        let mut ignored = Vec::new();
        assert!(build_paint_order(
            &table,
            &styles,
            &mut state,
            &mut ignored,
            true
        ));
        let mut last_seen = vec![None::<u32>; table.len()];
        for &i in &order {
            let context = state.context[i as usize] as usize;
            if let Some(previous) = last_seen[context] {
                assert!(
                    previous < i,
                    "context {context} emitted {i} after {previous}"
                );
            }
            last_seen[context] = Some(i);
        }
    }

    #[test]
    fn every_node_appears_exactly_once() {
        let table = four_child_document();
        let mut styles = vec![Some(ComputedStyle::default()); 5];
        styles[2] = Some(positioned(2));
        styles[3] = Some(positioned(-3));
        styles[4] = Some(positioned(7));
        let mut order = order_for(&styles, &table);
        order.sort_unstable();
        assert_eq!(order, vec![0, 1, 2, 3, 4]);
    }
}
