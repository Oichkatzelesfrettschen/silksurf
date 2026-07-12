/*
 * fused_pipeline.rs -- three-pass style+layout+paint pipeline.
 *
 * The cascade pass computes styles for every BFS node before taffy receives
 * the tree. The layout pass writes absolute rects for those styled nodes. The
 * paint pass emits display items from the completed layout state.
 */

use silksurf_css::{
    CascadeView, CascadeWorkspace, ComputedStyle, Display, Length, LengthOrAuto, StyleIndex,
    Stylesheet, WhiteSpace, compute_style_for_node_with_workspace,
};
use silksurf_dom::{Dom, NodeId, NodeKind, TagName};
use silksurf_layout::Rect;
use silksurf_layout::neighbor_table::LayoutNeighborTable;
use silksurf_layout::taffy_layout::TaffyLayout;
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
    /// Content rect per BFS-indexed node (valid after `run()`).
    pub node_rects: Vec<Rect>,
    /// Paint commands (valid after `run()`; order is BFS paint order).
    pub display_items: Vec<DisplayItem>,
    /// Cached tree-shape generation for the BFS table.
    table_generation: u64,
    /// Cached selector-input generation for the cascade view.
    cascade_generation: u64,
    /// Cached tree-shape generation for the taffy node graph.
    taffy_structure_generation: u64,
    /// Cached selector-input generation for the taffy style graph.
    taffy_style_generation: u64,
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
            cascade_view: CascadeView::new(),
            cascade_ws: CascadeWorkspace::new(0),
            taffy_layout: TaffyLayout::new(),
            styles: Vec::new(),
            node_rects: Vec::new(),
            display_items: Vec::new(),
            table_generation: u64::MAX,
            cascade_generation: u64::MAX,
            taffy_structure_generation: u64::MAX,
            taffy_style_generation: u64::MAX,
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
        self.display_items.clear();
        let root_suppressed = node_starts_non_rendered_subtree(dom, root);

        // Pass 1: cascade -- compute ComputedStyle for every BFS node.
        // Each node reads its parent's style (already computed, since BFS
        // processes parents before children).
        let phase_start = std::time::Instant::now();
        let mut rem_base_px = 16.0_f32;
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
            );
            if root_suppressed {
                style.display = Display::None;
            }
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
        {
            self.taffy_layout.rebuild(dom, &self.table, &self.styles);
            self.taffy_structure_generation = structure_gen;
            self.taffy_style_generation = style_gen;
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
        for (i, &node) in self.table.bfs_order.iter().enumerate() {
            let Some(ref style) = self.styles[i] else {
                continue;
            };
            if style.display == Display::None {
                continue;
            }
            if text_node_collapses_to_empty_render(dom, &self.table, &self.styles, i) {
                continue;
            }
            emit_workspace_paint(
                dom,
                node,
                style,
                self.node_rects[i],
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
            table: self.table.clone(),
        }
    }

    /// Recycle result vector storage for the next workspace run.
    pub fn recycle_result_storage(&mut self, mut result: FusedResult) {
        self.styles = std::mem::take(&mut result.styles);
        self.display_items = std::mem::take(&mut result.display_items);
        self.node_rects = std::mem::take(&mut result.node_rects);
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
    /// Content rect per node in BFS order.
    pub node_rects: Vec<Rect>,
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
        );
        if root_suppressed {
            style.display = Display::None;
        }
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
    for (i, &node) in table.bfs_order.iter().enumerate() {
        let Some(ref style) = styles[i] else {
            continue;
        };
        if style.display == Display::None {
            continue;
        }
        if text_node_collapses_to_empty_render(dom, &table, &styles, i) {
            continue;
        }
        emit_allocating_paint(dom, node, style, node_rects[i], &mut display_items);
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

fn emit_workspace_paint(
    dom: &Dom,
    node: NodeId,
    style: &ComputedStyle,
    content_rect: Rect,
    display_items: &mut Vec<DisplayItem>,
) {
    emit_box_shadow(style, content_rect, display_items);
    emit_rounded_background(style, content_rect, display_items);
    emit_text_content(dom, node, style, content_rect, display_items);
    emit_form_control_text(dom, node, style, content_rect, display_items);
}

fn emit_allocating_paint(
    dom: &Dom,
    node: NodeId,
    style: &ComputedStyle,
    content_rect: Rect,
    display_items: &mut Vec<DisplayItem>,
) {
    emit_square_background(style, content_rect, display_items);
    emit_text_content(dom, node, style, content_rect, display_items);
    emit_form_control_text(dom, node, style, content_rect, display_items);
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
            font_size: font_size_px(style),
            color: style.color,
        });
    }
}

fn emit_form_control_text(
    dom: &Dom,
    node: NodeId,
    style: &ComputedStyle,
    content_rect: Rect,
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
            font_size: font_size_px(style),
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
