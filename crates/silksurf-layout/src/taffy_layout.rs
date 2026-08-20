/*
 * taffy_layout.rs -- CSS Flexbox + Grid layout via the taffy crate.
 *
 * TaffyLayout holds a cached TaffyTree<()> plus a mapping from taffy NodeId to
 * BFS index.  rebuild() reconstructs the tree from the DOM, BFS traversal
 * table, and per-node ComputedStyles.  Single direct text children merge into
 * their parent taffy leaf.  compute() runs layout with a measure function that
 * calls silksurf_text::measure_text for text leaves.  write_rects() extracts
 * absolute positions into node_rects[].
 *
 *   let mut tl = TaffyLayout::new();
 *   tl.rebuild(dom, &table, &styles);
 *   tl.compute(dom, &styles, &table.bfs_order, viewport);
 *   tl.write_rects(&table.parent_idx, &mut node_rects, viewport);
 *
 * See: crates/silksurf-engine/src/fused_pipeline.rs for integration point.
 * See: crates/silksurf-layout/src/flex.rs for the hand-written flex baseline.
 */

use rustc_hash::FxHashMap;
use silksurf_css::{
    AlignItems as CssAlignItems, AlignSelf as CssAlignSelf, BoxSizing as CssBoxSizing,
    ComputedStyle, Display as CssDisplay, FlexBasis, FlexDirection as CssFlexDirection,
    FlexWrap as CssFlexWrap, GridAutoFlow as CssGridAutoFlow, GridLine as CssGridLine,
    GridTrackMax as CssGridTrackMax, GridTrackMin as CssGridTrackMin,
    GridTrackSize as CssGridTrackSize, JustifyContent as CssJustifyContent, Length, LengthOrAuto,
    Position as CssPosition, WhiteSpace,
};
use silksurf_dom::{Dom, NodeId as DomNodeId, NodeKind, TagName};
use taffy::{
    AlignItems, AlignSelf, AvailableSpace, BoxSizing as TaffyBoxSizing, Dimension,
    Display as TaffyDisplay, FlexDirection, FlexWrap, GridAutoFlow, GridPlacement,
    GridTemplateComponent, JustifyContent, LengthPercentage, LengthPercentageAuto, Line,
    MaxTrackSizingFunction, MinTrackSizingFunction, NodeId as TaffyId, Position as TaffyPosition,
    Size, Style, TaffyTree, TrackSizingFunction,
    geometry::Rect as TaffyRect,
    style_helpers::{
        TaffyAuto as _, TaffyFitContent as _, TaffyMaxContent as _, TaffyMinContent as _, fr,
        length, line as taffy_line, minmax, percent, span as taffy_span,
    },
};

use crate::{Rect, neighbor_table::LayoutNeighborTable, unresolved_font_relative_px};

pub type SilkTaffy = TaffyTree<()>;

/// Cached taffy layout state held inside `FusedWorkspace`.
///
/// Invariant: `taffy_nodes[i]` corresponds to `bfs_order[i]` from the last `rebuild()`.
pub struct TaffyLayout {
    tree: SilkTaffy,
    /// BFS index -> taffy node id.
    taffy_nodes: Vec<Option<TaffyId>>,
    /// Reverse map: taffy id -> BFS index (for the measure-function lookup).
    taffy_to_bfs: FxHashMap<TaffyId, usize>,
    /// Reused child-id list for parent node construction.
    child_ids_scratch: Vec<TaffyId>,
    /// Text measurement cache keyed by BFS index and guarded by DOM generation.
    text_measure_cache: Vec<CachedTextMeasures>,
    text_measure_generation: u64,
    /// Second taffy root sized to the viewport, holding the subtrees whose
    /// containing block is the viewport or the initial containing block.
    /// `None` for a document that reparents no box onto it.
    viewport_root: Option<TaffyId>,
    /// Per-BFS-index containing block and static-position axes.
    placements: Vec<Placement>,
    /// Absolute boxes reparented onto a positioned ancestor, grouped by that
    /// ancestor's BFS index. `adopted_start[i]..adopted_start[i + 1]` indexes
    /// the run belonging to `i`.
    adopted: Vec<u32>,
    adopted_start: Vec<u32>,
    /// Per-BFS-index result of `taffy_node_merges_into_parent`, computed once
    /// per rebuild. `assign_placements` reads it to skip merged ancestors when
    /// it walks for a containing block, and the tree-building pass reads the
    /// same entry to decide whether the node exists at all, so an adopted run
    /// can never name a node the build skipped.
    merges_into_parent: Vec<bool>,
    /// Nearest ancestor-or-none whose position is not static, per BFS index.
    /// Retained so a rebuild reuses the allocation.
    positioned_ancestor: Vec<Option<u32>>,
    /// Write cursor into `adopted`, one entry per group. Retained for the same
    /// reason.
    adopted_cursor: Vec<u32>,
    /// Per-BFS-index: the element has children in the BFS table. An element
    /// whose text child merged into it also carries this flag, and
    /// `measure_taffy_text_node` returns that merged text's size before the
    /// measure closure consults the flag, so the flag decides only the case
    /// where no text remains to measure: children whose boxes lay out
    /// elsewhere. CSS 2.1 10.6.3 gives that box an auto height of zero.
    generates_no_line_box: Vec<bool>,
}

/*
 * CSS Position 3 2.1 gives a positioned box a containing block that the DOM
 * parent need not supply: the viewport for `position: fixed`, and the nearest
 * ancestor whose position is not static for `position: absolute`.  taffy
 * resolves an absolutely positioned child against its taffy parent, so a box
 * whose containing block is not its DOM parent hangs off the taffy node of
 * the block that owns it.
 */
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ContainingBlock {
    /// The box resolves against its DOM parent, which is what taffy already
    /// does. Every in-flow box and every absolute box whose DOM parent is
    /// itself positioned lands here.
    #[default]
    DomParent,
    /// `position: fixed` resolves against the viewport.
    Viewport,
    /// `position: absolute` with no positioned ancestor resolves against the
    /// initial containing block: the viewport rect anchored at the document
    /// origin. It coincides with the viewport until the page scrolls, and
    /// `viewport_root` carries both.
    InitialBlock,
    /// `position: absolute` resolves against the nearest ancestor whose
    /// position is not static, named by its BFS index.
    Ancestor(u32),
}

/*
 * `static_x` and `static_y` mark the axes where both insets compute to auto.
 * CSS keeps the static position in that case, which is the position the box
 * would occupy in its own flow parent rather than in the containing block it
 * was reparented onto. `write_rects` restores it from the DOM parent's origin.
 */
#[derive(Clone, Copy, Default)]
struct Placement {
    block: ContainingBlock,
    static_x: bool,
    static_y: bool,
}

impl Placement {
    /// `positioned_ancestor` is the nearest ancestor-or-none whose position is
    /// not static and whose box survives into the taffy tree.
    fn for_style(style: Option<&ComputedStyle>, positioned_ancestor: Option<u32>) -> Self {
        let Some(style) = style else {
            return Self::default();
        };
        let block = match style.position {
            CssPosition::Fixed => ContainingBlock::Viewport,
            CssPosition::Absolute => positioned_ancestor
                .map_or(ContainingBlock::InitialBlock, |ancestor| {
                    ContainingBlock::Ancestor(ancestor)
                }),
            _ => return Self::default(),
        };
        Self {
            block,
            static_x: matches!(style.left, LengthOrAuto::Auto)
                && matches!(style.right, LengthOrAuto::Auto),
            static_y: matches!(style.top, LengthOrAuto::Auto)
                && matches!(style.bottom, LengthOrAuto::Auto),
        }
    }
}

impl ContainingBlock {
    /// A box whose containing block is the viewport or the initial containing
    /// block lays out under `viewport_root`.
    fn is_viewport_rooted(self) -> bool {
        matches!(self, Self::Viewport | Self::InitialBlock)
    }

    /// The BFS index whose taffy node adopts this box, when the containing
    /// block is an ancestor other than the DOM parent.
    fn adopting_ancestor(self, dom_parent: u32) -> Option<u32> {
        match self {
            Self::Ancestor(ancestor) if ancestor != dom_parent => Some(ancestor),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct CachedTextMeasure {
    font_size: f32,
    max_width: Option<f32>,
    width: f32,
    height: f32,
    text_len: usize,
}

impl CachedTextMeasure {
    fn matches(self, font_size: f32, max_width: Option<f32>) -> bool {
        self.font_size.to_bits() == font_size.to_bits()
            && optional_f32_bits_equal(self.max_width, max_width)
    }
}

fn optional_f32_bits_equal(left: Option<f32>, right: Option<f32>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.to_bits() == right.to_bits(),
        (None, None) => true,
        _ => false,
    }
}

#[derive(Clone, Copy, Default)]
struct CachedTextMeasures {
    entries: [Option<CachedTextMeasure>; 4],
    next_replace: usize,
}

impl CachedTextMeasures {
    fn get(self, font_size: f32, max_width: Option<f32>) -> Option<CachedTextMeasure> {
        self.entries
            .into_iter()
            .flatten()
            .find(|cached| cached.matches(font_size, max_width))
    }

    fn insert(&mut self, measure: CachedTextMeasure) {
        if let Some(slot) = self.entries.iter_mut().find(|entry| entry.is_none()) {
            *slot = Some(measure);
            return;
        }
        self.entries[self.next_replace] = Some(measure);
        self.next_replace = (self.next_replace + 1) % self.entries.len();
    }
}

#[derive(Default)]
struct TaffyRebuildStats {
    created: usize,
    leaves: usize,
    parents: usize,
    child_edges: usize,
    skipped: usize,
    skipped_display_none: usize,
    skipped_whitespace: usize,
    skipped_text_merge: usize,
    skip_time: std::time::Duration,
    style_time: std::time::Duration,
    child_time: std::time::Duration,
    tree_time: std::time::Duration,
    map_time: std::time::Duration,
}

impl TaffyRebuildStats {
    fn record_skip(
        &mut self,
        dom: &Dom,
        table: &LayoutNeighborTable,
        styles: &[Option<ComputedStyle>],
        index: usize,
    ) {
        self.skipped += 1;
        if text_node_collapses_to_empty_layout(dom, table, styles, index) {
            self.skipped_whitespace += 1;
        } else if styles
            .get(index)
            .and_then(Option::as_ref)
            .is_none_or(|style| style.display == CssDisplay::None)
        {
            self.skipped_display_none += 1;
        } else {
            self.skipped_text_merge += 1;
        }
    }
}

fn trace_start(trace_enabled: bool) -> Option<std::time::Instant> {
    trace_enabled.then(std::time::Instant::now)
}

fn record_elapsed(total: &mut std::time::Duration, start: Option<std::time::Instant>) {
    if let Some(start) = start {
        *total += start.elapsed();
    }
}

impl TaffyLayout {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tree: new_taffy_tree(16),
            taffy_nodes: Vec::new(),
            taffy_to_bfs: FxHashMap::default(),
            child_ids_scratch: Vec::new(),
            text_measure_cache: Vec::new(),
            text_measure_generation: u64::MAX,
            viewport_root: None,
            placements: Vec::new(),
            adopted: Vec::new(),
            adopted_start: Vec::new(),
            merges_into_parent: Vec::new(),
            positioned_ancestor: Vec::new(),
            adopted_cursor: Vec::new(),
            generates_no_line_box: Vec::new(),
        }
    }

    /// Reconstruct the taffy tree from BFS table + computed styles.
    ///
    /// Must be called before `compute()` whenever the DOM or styles have changed.
    /*
     * Resolve every box's containing block in one forward BFS pass.
     *
     * `parent_idx[i] < i` holds for the BFS table, so the nearest positioned
     * ancestor of `i` is the DOM parent when that parent is positioned and the
     * parent's own nearest positioned ancestor otherwise. A node that merges
     * into its parent contributes no taffy node, so it cannot adopt anything
     * and the walk passes through it.
     *
     * Returns whether any box lays out under `viewport_root`.
     */
    fn assign_placements(
        &mut self,
        dom: &Dom,
        table: &LayoutNeighborTable,
        styles: &[Option<ComputedStyle>],
    ) -> bool {
        let n = table.len();
        self.merges_into_parent.clear();
        self.merges_into_parent
            .extend((0..n).map(|i| taffy_node_merges_into_parent(dom, table, styles, i)));
        self.placements.clear();
        self.placements.resize(n, Placement::default());
        self.positioned_ancestor.clear();
        self.positioned_ancestor.reserve(n);
        let mut any_viewport_rooted = false;
        for i in 0..n {
            let parent = table
                .parent_idx
                .get(i)
                .copied()
                .filter(|&p| (p as usize) < i);
            let inherited = parent.and_then(|p| {
                let p = p as usize;
                let parent_positioned = styles
                    .get(p)
                    .and_then(Option::as_ref)
                    .is_some_and(|style| style.position != CssPosition::Static)
                    && !self.merges_into_parent[p];
                if parent_positioned {
                    Some(p as u32)
                } else {
                    self.positioned_ancestor[p]
                }
            });
            self.positioned_ancestor.push(inherited);
            // Index 0 is the document root; reparenting it would leave the
            // tree without the in-flow root that sizes the page.
            if i == 0 {
                continue;
            }
            self.placements[i] =
                Placement::for_style(styles.get(i).and_then(Option::as_ref), inherited);
            any_viewport_rooted |= self.placements[i].block.is_viewport_rooted();
        }
        any_viewport_rooted
    }

    /// Group the reparented absolute boxes by the BFS index that adopts them,
    /// so `rebuild` reads one contiguous run per taffy node it builds.
    fn group_adopted_by_ancestor(&mut self, table: &LayoutNeighborTable) {
        let n = table.len();
        self.adopted.clear();
        self.adopted_start.clear();
        // A document that reparents nothing leaves the table empty, and
        // `adopted_run_bounds` reads an empty run for every index.
        if !self
            .placements
            .iter()
            .any(|placement| matches!(placement.block, ContainingBlock::Ancestor(_)))
        {
            return;
        }
        self.adopted_start.resize(n + 1, 0);
        for i in 0..n {
            let dom_parent = table.parent_idx.get(i).copied().unwrap_or(u32::MAX);
            if let Some(ancestor) = self.placements[i].block.adopting_ancestor(dom_parent) {
                self.adopted_start[ancestor as usize + 1] += 1;
            }
        }
        for i in 0..n {
            self.adopted_start[i + 1] += self.adopted_start[i];
        }
        self.adopted
            .resize(self.adopted_start[n] as usize, u32::MAX);
        self.adopted_cursor.clear();
        self.adopted_cursor.extend_from_slice(&self.adopted_start);
        for i in 0..n {
            let dom_parent = table.parent_idx.get(i).copied().unwrap_or(u32::MAX);
            if let Some(ancestor) = self.placements[i].block.adopting_ancestor(dom_parent) {
                let slot = &mut self.adopted_cursor[ancestor as usize];
                self.adopted[*slot as usize] = i as u32;
                *slot += 1;
            }
        }
    }

    pub fn rebuild(
        &mut self,
        dom: &Dom,
        table: &LayoutNeighborTable,
        styles: &[Option<ComputedStyle>],
    ) {
        let trace_taffy = std::env::var_os("SILKSURF_TRACE_TAFFY").is_some();
        let mut stats = TaffyRebuildStats::default();
        let n = table.len();
        if self.taffy_nodes.capacity() < n {
            self.tree = new_taffy_tree(n);
        } else {
            self.tree.clear();
        }
        self.taffy_nodes.clear();
        self.taffy_nodes.resize(n, None);
        self.taffy_to_bfs.clear();
        self.viewport_root = None;
        let any_viewport_rooted = self.assign_placements(dom, table, styles);
        self.group_adopted_by_ancestor(table);
        self.generates_no_line_box.clear();
        self.generates_no_line_box
            .extend((0..n).map(|i| table.child_count[i] > 0));

        // Process in reverse BFS order: children before parents so
        // taffy node IDs are available when we build the parent node.
        for i in (0..n).rev() {
            let skip_start = trace_start(trace_taffy);
            let node_merges_into_parent = self.merges_into_parent[i];
            record_elapsed(&mut stats.skip_time, skip_start);
            if node_merges_into_parent {
                if trace_taffy {
                    stats.record_skip(dom, table, styles, i);
                }
                continue;
            }
            let style_start = trace_start(trace_taffy);
            let taffy_style = css_to_taffy_style_for_index(table, styles, i);
            record_elapsed(&mut stats.style_time, style_start);
            let child_start = trace_start(trace_taffy);
            self.child_ids_scratch.clear();
            let first_child = table.child_start[i];
            if first_child != u32::MAX {
                let start = first_child as usize;
                let end = start + usize::from(table.child_count[i]);
                self.child_ids_scratch.extend(
                    (start..end)
                        .filter(|&child_idx| {
                            self.placements[child_idx].block == ContainingBlock::DomParent
                                || self.placements[child_idx].block
                                    == ContainingBlock::Ancestor(i as u32)
                        })
                        .filter_map(|child_idx| self.taffy_nodes[child_idx]),
                );
            }
            // An absolute box whose nearest positioned ancestor is not its DOM
            // parent joins that ancestor's taffy children instead, so taffy
            // resolves its insets against the box CSS names.
            let adopted_run = adopted_run_bounds(&self.adopted_start, i);
            self.child_ids_scratch.extend(
                self.adopted[adopted_run]
                    .iter()
                    .filter_map(|&adopted| self.taffy_nodes[adopted as usize]),
            );
            record_elapsed(&mut stats.child_time, child_start);

            if trace_taffy {
                stats.child_edges += self.child_ids_scratch.len();
            }
            let tree_start = trace_start(trace_taffy);
            let result = if self.child_ids_scratch.is_empty() {
                if trace_taffy {
                    stats.leaves += 1;
                }
                self.tree.new_leaf(taffy_style)
            } else {
                if trace_taffy {
                    stats.parents += 1;
                }
                self.tree
                    .new_with_children(taffy_style, &self.child_ids_scratch)
            };
            record_elapsed(&mut stats.tree_time, tree_start);

            if let Ok(tn) = result {
                let map_start = trace_start(trace_taffy);
                self.taffy_to_bfs.insert(tn, i);
                self.taffy_nodes[i] = Some(tn);
                record_elapsed(&mut stats.map_time, map_start);
            }
        }
        if any_viewport_rooted {
            self.child_ids_scratch.clear();
            self.child_ids_scratch.extend(
                (0..n)
                    .filter(|&i| self.placements[i].block.is_viewport_rooted())
                    .filter_map(|i| self.taffy_nodes[i]),
            );
            // compute() overwrites the size with the live viewport before it
            // lays this root out; block display gives these children a
            // containing block rather than a flex line.
            self.viewport_root = self
                .tree
                .new_with_children(
                    Style {
                        display: TaffyDisplay::Block,
                        ..Default::default()
                    },
                    &self.child_ids_scratch,
                )
                .ok();
        }
        if trace_taffy {
            stats.created = self.taffy_to_bfs.len();
            eprintln!(
                "[SilkSurf] taffy rebuild: bfs_nodes={n}, created={}, leaves={}, parents={}, child_edges={}, skipped={}, skipped_display_none={}, skipped_whitespace={}, skipped_text_merge={}, skip_time={:?}, style_time={:?}, child_time={:?}, tree_time={:?}, map_time={:?}",
                stats.created,
                stats.leaves,
                stats.parents,
                stats.child_edges,
                stats.skipped,
                stats.skipped_display_none,
                stats.skipped_whitespace,
                stats.skipped_text_merge,
                stats.skip_time,
                stats.style_time,
                stats.child_time,
                stats.tree_time,
                stats.map_time
            );
        }
    }

    /// Run taffy layout with a text-aware measure function.
    ///
    /// Returns true if layout completed successfully.
    pub fn compute(
        &mut self,
        dom: &Dom,
        styles: &[Option<ComputedStyle>],
        bfs_order: &[DomNodeId],
        viewport: Rect,
    ) -> bool {
        let trace_taffy = std::env::var_os("SILKSURF_TRACE_TAFFY").is_some();
        let mut trace_stats = trace_taffy.then(TaffyMeasureStats::default);
        let Some(root) = self.taffy_nodes.first().and_then(|n| *n) else {
            return false;
        };
        let available = Size {
            width: AvailableSpace::Definite(viewport.width),
            height: AvailableSpace::Definite(viewport.height),
        };
        self.refresh_text_measure_cache(dom.generation());

        // Split borrow: tree needs &mut, taffy_to_bfs needs &.
        let TaffyLayout {
            tree,
            taffy_to_bfs,
            text_measure_cache,
            viewport_root,
            generates_no_line_box,
            ..
        } = self;

        // The viewport root exists only for `position: fixed` subtrees, and it
        // carries the viewport as a definite size so `inset` and percentage
        // lengths inside those subtrees resolve against the viewport.  The
        // document root keeps `size: auto`, so this second pass leaves in-flow
        // geometry untouched.
        if let Some(anchored_root) = *viewport_root {
            let _ = tree.set_style(
                anchored_root,
                Style {
                    display: TaffyDisplay::Block,
                    size: Size {
                        width: Dimension::length(viewport.width),
                        height: Dimension::length(viewport.height),
                    },
                    ..Default::default()
                },
            );
        }
        let mut laid_out = true;
        for target in [Some(root), *viewport_root].into_iter().flatten() {
            let result = tree.compute_layout_with_measure(
                target,
                available,
                |known, avail, taffy_node_id, _context, _style| {
                    if let Some(stats) = trace_stats.as_mut() {
                        stats.calls += 1;
                    }
                    if let Some(size) = known_measure_size(known) {
                        if let Some(stats) = trace_stats.as_mut() {
                            stats.known_size_hits += 1;
                        }
                        return size;
                    }
                    let Some(&bfs_idx) = taffy_to_bfs.get(&taffy_node_id) else {
                        return Size::ZERO;
                    };

                    let font_size =
                        styles
                            .get(bfs_idx)
                            .and_then(Option::as_ref)
                            .map_or(16.0, |s| match s.font_size {
                                Length::Px(px) => px,
                                _ => 16.0,
                            });

                    let max_w = match avail.width {
                        AvailableSpace::Definite(w) => Some(w),
                        _ => None,
                    };

                    if let Some((size, text_len, elapsed, cache_hit)) = measure_taffy_text_node(
                        dom,
                        bfs_order,
                        bfs_idx,
                        font_size,
                        max_w,
                        text_measure_cache,
                        trace_taffy,
                    ) {
                        if let Some(stats) = trace_stats.as_mut() {
                            if cache_hit {
                                stats.text_cache_hits += 1;
                            } else {
                                stats.text_elapsed += elapsed;
                            }
                            stats.text_calls += 1;
                            stats.text_bytes += text_len;
                            stats.max_text_bytes = stats.max_text_bytes.max(text_len);
                        }
                        return size;
                    }

                    if bfs_order.get(bfs_idx).is_none() {
                        return Size::ZERO;
                    }

                    // Reaching here means the node measured no text of its
                    // own. Its children's boxes therefore lay out elsewhere --
                    // absolutely positioned, reparented onto another containing
                    // block, or suppressed -- so it holds no line box, and CSS
                    // 2.1 10.6.3 gives it an auto height of zero. The text
                    // measure above must stay ahead of this check: an element
                    // that absorbed its text child carries the same flag.
                    if generates_no_line_box.get(bfs_idx).copied().unwrap_or(false) {
                        return Size {
                            width: known.width.unwrap_or(0.0),
                            height: known.height.unwrap_or(0.0),
                        };
                    }

                    if let Some(line_h) =
                        styles
                            .get(bfs_idx)
                            .and_then(Option::as_ref)
                            .map(|s| match s.line_height {
                                Length::Px(px) => px,
                                _ => 16.0,
                            })
                    {
                        return Size {
                            width: known.width.unwrap_or(0.0),
                            height: known.height.unwrap_or(line_h),
                        };
                    }

                    // Element leaf node with no text: use line_height as minimum height.
                    Size {
                        width: known.width.unwrap_or(0.0),
                        height: known.height.unwrap_or(16.0),
                    }
                },
            );
            laid_out &= result.is_ok();
        }
        if let Some(stats) = trace_stats {
            eprintln!(
                "[SilkSurf] taffy measure: calls={}, known_size_hits={}, text_calls={}, text_cache_hits={}, text_bytes={}, max_text_bytes={}, text_time={:?}",
                stats.calls,
                stats.known_size_hits,
                stats.text_calls,
                stats.text_cache_hits,
                stats.text_bytes,
                stats.max_text_bytes,
                stats.text_elapsed
            );
        }
        laid_out
    }

    fn refresh_text_measure_cache(&mut self, generation: u64) {
        if self.text_measure_generation != generation {
            self.text_measure_cache.clear();
            self.text_measure_generation = generation;
        }
        self.text_measure_cache
            .resize(self.taffy_nodes.len(), CachedTextMeasures::default());
    }

    /// Write absolute positions from taffy layout results into `node_rects`.
    ///
    /// taffy's Layout.location is parent-relative, so we accumulate offsets
    /// down the BFS tree (parents are always processed before children in
    /// BFS order, so `node_rects[parent]` is already filled when we process child).
    pub fn write_rects(&self, parent_idx: &[u32], node_rects: &mut [Rect], viewport: Rect) {
        let n = self.taffy_nodes.len().min(node_rects.len());
        for i in 0..n {
            let Some(tn) = self.taffy_nodes[i] else {
                if parent_idx[i] != u32::MAX {
                    let parent = parent_idx[i] as usize;
                    if parent < node_rects.len() {
                        node_rects[i] = node_rects[parent];
                    }
                }
                continue;
            };
            let Ok(layout) = self.tree.layout(tn) else {
                continue;
            };

            let dom_parent = (parent_idx[i] != u32::MAX)
                .then(|| parent_idx[i] as usize)
                .filter(|&p| p < node_rects.len());
            let (dom_parent_x, dom_parent_y) = dom_parent.map_or((viewport.x, viewport.y), |p| {
                (node_rects[p].x, node_rects[p].y)
            });

            // taffy reports a location relative to the taffy parent, which is
            // the node of the containing block the box was reparented onto.
            // An axis whose insets both compute to auto keeps the CSS static
            // position instead, which the DOM parent's origin supplies.
            let placement = self.placements.get(i).copied().unwrap_or_default();
            let (block_x, block_y) = match placement.block {
                ContainingBlock::DomParent => (dom_parent_x, dom_parent_y),
                ContainingBlock::Viewport | ContainingBlock::InitialBlock => {
                    (viewport.x, viewport.y)
                }
                // An ancestor holds a lower BFS index than the box it
                // contains, so BFS order has already written its rect.
                ContainingBlock::Ancestor(ancestor) => {
                    let ancestor = ancestor as usize;
                    node_rects
                        .get(ancestor)
                        .map_or((dom_parent_x, dom_parent_y), |rect| (rect.x, rect.y))
                }
            };
            let reparented = placement.block != ContainingBlock::DomParent;
            let static_x = reparented && placement.static_x;
            let static_y = reparented && placement.static_y;

            node_rects[i] = Rect {
                x: if static_x {
                    dom_parent_x
                } else {
                    block_x + layout.location.x
                },
                y: if static_y {
                    dom_parent_y
                } else {
                    block_y + layout.location.y
                },
                width: layout.size.width,
                height: layout.size.height,
            };
        }
    }
}

impl Default for TaffyLayout {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn new_taffy_tree(capacity: usize) -> SilkTaffy {
    let mut tree = TaffyTree::with_capacity(capacity);
    tree.disable_rounding();
    tree
}

#[derive(Default)]
struct TaffyMeasureStats {
    calls: usize,
    known_size_hits: usize,
    text_calls: usize,
    text_bytes: usize,
    max_text_bytes: usize,
    text_elapsed: std::time::Duration,
    text_cache_hits: usize,
}

fn known_measure_size(known: Size<Option<f32>>) -> Option<Size<f32>> {
    match known {
        Size {
            width: Some(width),
            height: Some(height),
        } => Some(Size { width, height }),
        _ => None,
    }
}

fn measure_taffy_text_node(
    dom: &Dom,
    bfs_order: &[DomNodeId],
    bfs_idx: usize,
    font_size: f32,
    max_width: Option<f32>,
    cache: &mut [CachedTextMeasures],
    trace_taffy: bool,
) -> Option<(Size<f32>, usize, std::time::Duration, bool)> {
    if let Some(cached) = cache
        .get(bfs_idx)
        .and_then(|entries| entries.get(font_size, max_width))
    {
        return Some((
            Size {
                width: cached.width,
                height: cached.height,
            },
            cached.text_len,
            std::time::Duration::ZERO,
            true,
        ));
    }

    let dom_node_id = *bfs_order.get(bfs_idx)?;
    let text = taffy_measure_text(dom, dom_node_id)?;
    let (width, height, elapsed) = measure_text_for_taffy(text, font_size, max_width, trace_taffy);
    let text_len = text.len();
    if let Some(slot) = cache.get_mut(bfs_idx) {
        slot.insert(CachedTextMeasure {
            font_size,
            max_width,
            width,
            height,
            text_len,
        });
    }
    Some((Size { width, height }, text_len, elapsed, false))
}

fn measure_text_for_taffy(
    text: &str,
    font_size: f32,
    max_width: Option<f32>,
    trace_taffy: bool,
) -> (f32, f32, std::time::Duration) {
    if !trace_taffy {
        let (width, height) = silksurf_text::measure_text(text, font_size, max_width);
        return (width, height, std::time::Duration::ZERO);
    }
    let measure_start = std::time::Instant::now();
    let (width, height) = silksurf_text::measure_text(text, font_size, max_width);
    (width, height, measure_start.elapsed())
}

fn taffy_measure_text(dom: &Dom, node_id: DomNodeId) -> Option<&str> {
    let node = dom.node(node_id).ok()?;
    if let NodeKind::Text { text } = node.kind() {
        return Some(text);
    }
    single_direct_text_child(dom, node_id)
}

fn single_direct_text_child(dom: &Dom, node_id: DomNodeId) -> Option<&str> {
    let children = dom.children(node_id).ok()?;
    let mut text = None;
    for &child in children {
        let child_node = dom.node(child).ok()?;
        match child_node.kind() {
            NodeKind::Text { text: child_text } => {
                if text.replace(child_text.as_str()).is_some() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    text
}

/// Range of `adopted` holding the boxes that BFS index `i` adopts. Empty when
/// `i` adopts nothing, and empty for an index past the grouped table.
fn adopted_run_bounds(adopted_start: &[u32], i: usize) -> std::ops::Range<usize> {
    let (Some(&start), Some(&end)) = (adopted_start.get(i), adopted_start.get(i + 1)) else {
        return 0..0;
    };
    start as usize..end as usize
}

fn taffy_node_merges_into_parent(
    dom: &Dom,
    table: &LayoutNeighborTable,
    styles: &[Option<ComputedStyle>],
    index: usize,
) -> bool {
    if text_node_collapses_to_empty_layout(dom, table, styles, index) {
        return index != 0;
    }
    if styles
        .get(index)
        .and_then(Option::as_ref)
        .is_none_or(|style| style.display == CssDisplay::None)
    {
        return index != 0;
    }
    if transparent_code_wrapper_merges_into_parent(dom, table, styles, index) {
        return true;
    }
    let Some(node_id) = table.bfs_order.get(index).copied() else {
        return false;
    };
    let Ok(node) = dom.node(node_id) else {
        return false;
    };
    matches!(node.kind(), NodeKind::Text { .. })
        && text_node_parent_is_text_leaf(dom, table, styles, index)
}

fn transparent_code_wrapper_merges_into_parent(
    dom: &Dom,
    table: &LayoutNeighborTable,
    styles: &[Option<ComputedStyle>],
    index: usize,
) -> bool {
    let Some(node_id) = table.bfs_order.get(index).copied() else {
        return false;
    };
    let Ok(node) = dom.node(node_id) else {
        return false;
    };
    if !matches!(node.kind(), NodeKind::Element { name, .. } if *name == TagName::Code) {
        return false;
    }
    let Some(style) = styles.get(index).and_then(Option::as_ref) else {
        return false;
    };
    style.display == CssDisplay::Inline
        && style_has_no_layout_or_paint_box(style)
        && single_direct_text_child(dom, node_id).is_some()
        && parent_has_only_this_rendered_child(table, styles, index)
}

fn style_has_no_layout_or_paint_box(style: &ComputedStyle) -> bool {
    margins_are_zero(style.margin)
        && edges_are_zero(style.padding)
        && edges_are_zero(style.border)
        && style.background_color.a == 0
        && style.background_image.is_none()
        && style.box_shadow.is_none()
        && style.border_radius == 0.0
        && style.width == LengthOrAuto::Auto
        && style.height == LengthOrAuto::Auto
}

fn margins_are_zero(margins: silksurf_css::Margins) -> bool {
    [margins.top, margins.right, margins.bottom, margins.left]
        .into_iter()
        .all(|length| length == LengthOrAuto::Length(Length::zero()))
}

fn edges_are_zero(edges: silksurf_css::Edges) -> bool {
    [edges.top, edges.right, edges.bottom, edges.left]
        .into_iter()
        .all(|length| length == Length::zero())
}

fn parent_has_only_this_rendered_child(
    table: &LayoutNeighborTable,
    styles: &[Option<ComputedStyle>],
    index: usize,
) -> bool {
    let parent = table.parent_idx.get(index).copied().unwrap_or(u32::MAX);
    if parent == u32::MAX {
        return false;
    }
    let parent = parent as usize;
    let Some(first_child) = table.child_start.get(parent).copied() else {
        return false;
    };
    if first_child == u32::MAX {
        return false;
    }
    let start = first_child as usize;
    let end = start + usize::from(table.child_count[parent]);
    (start..end).all(|child| {
        child == index
            || styles
                .get(child)
                .and_then(Option::as_ref)
                .is_some_and(|style| style.display == CssDisplay::None)
    })
}

fn text_node_collapses_to_empty_layout(
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

fn text_node_contents(dom: &Dom, node_id: DomNodeId) -> Option<&str> {
    let node = dom.node(node_id).ok()?;
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
        .is_some_and(|style| style.display == CssDisplay::Inline)
}

fn text_node_parent_is_text_leaf(
    dom: &Dom,
    table: &LayoutNeighborTable,
    styles: &[Option<ComputedStyle>],
    index: usize,
) -> bool {
    let parent = table.parent_idx.get(index).copied().unwrap_or(u32::MAX);
    if parent == u32::MAX {
        return false;
    }
    let parent = parent as usize;
    let Some(parent_node) = table.bfs_order.get(parent).copied() else {
        return false;
    };
    if single_direct_text_child(dom, parent_node).is_none() {
        return false;
    }
    let Some(first_child) = table.child_start.get(parent).copied() else {
        return false;
    };
    if first_child == u32::MAX {
        return false;
    }
    let start = first_child as usize;
    let end = start + usize::from(table.child_count[parent]);
    (start..end).all(|child| {
        child == index
            || styles
                .get(child)
                .and_then(Option::as_ref)
                .is_some_and(|style| style.display == CssDisplay::None)
    })
}

fn length_auto(l: Length) -> LengthPercentageAuto {
    match l {
        Length::Px(px) => LengthPercentageAuto::length(px),
        Length::Percent(p) => LengthPercentageAuto::percent(p / 100.0),
        Length::Em(_)
        | Length::Rem(_)
        | Length::Vw(_)
        | Length::Vh(_)
        | Length::Vmin(_)
        | Length::Vmax(_) => LengthPercentageAuto::length(unresolved_font_relative_px()),
    }
}

fn length_or_auto_lpa(v: LengthOrAuto) -> LengthPercentageAuto {
    match v {
        LengthOrAuto::Auto => LengthPercentageAuto::auto(),
        LengthOrAuto::Length(l) => length_auto(l),
    }
}

fn length_pct(l: Length) -> LengthPercentage {
    match l {
        Length::Px(px) => LengthPercentage::length(px),
        Length::Percent(p) => LengthPercentage::percent(p / 100.0),
        Length::Em(_)
        | Length::Rem(_)
        | Length::Vw(_)
        | Length::Vh(_)
        | Length::Vmin(_)
        | Length::Vmax(_) => LengthPercentage::length(unresolved_font_relative_px()),
    }
}

/// Convert a silksurf-css `ComputedStyle` to a taffy Style.
///
/// Converts `ComputedStyle` to `taffy::Style` for layout computation.
///
/// Width/height/min/max are converted from `LengthOrAuto` / `Option<Length>` to
/// taffy Dimension values. AUTO passes through as `Dimension::auto()`.
fn css_to_taffy_style_for_index(
    table: &LayoutNeighborTable,
    styles: &[Option<ComputedStyle>],
    index: usize,
) -> Style {
    let style = styles.get(index).and_then(Option::as_ref);
    let mut taffy_style = css_to_taffy_style(style);
    if simple_fr_grid_container_columns(table, styles, index).is_some() {
        taffy_style.display = TaffyDisplay::Flex;
        taffy_style.flex_direction = FlexDirection::Row;
        taffy_style.flex_wrap = FlexWrap::Wrap;
    }
    if let Some(columns) = parent_simple_fr_grid_columns(table, styles, index) {
        taffy_style.flex_basis = Dimension::percent(1.0 / columns as f32);
        taffy_style.flex_grow = 0.0;
        taffy_style.flex_shrink = 1.0;
    }
    taffy_style
}

fn simple_fr_grid_container_columns(
    table: &LayoutNeighborTable,
    styles: &[Option<ComputedStyle>],
    index: usize,
) -> Option<usize> {
    let style = styles.get(index).and_then(Option::as_ref)?;
    if style.display != CssDisplay::Grid
        || style.grid_container.auto_flow != CssGridAutoFlow::Row
        || !style.grid_container.template_rows.is_empty()
        || !style.grid_container.auto_columns.is_empty()
        || !style.grid_container.auto_rows.is_empty()
    {
        return None;
    }
    let columns = equal_fr_track_count(&style.grid_container.template_columns)?;
    children_have_auto_grid_placement(table, styles, index).then_some(columns)
}

fn parent_simple_fr_grid_columns(
    table: &LayoutNeighborTable,
    styles: &[Option<ComputedStyle>],
    index: usize,
) -> Option<usize> {
    let parent = table.parent_idx.get(index).copied().unwrap_or(u32::MAX);
    if parent == u32::MAX {
        return None;
    }
    simple_fr_grid_container_columns(table, styles, parent as usize)
}

fn equal_fr_track_count(tracks: &[CssGridTrackSize]) -> Option<usize> {
    let [first, rest @ ..] = tracks else {
        return None;
    };
    let CssGridTrackSize::Fr(first_fr) = first else {
        return None;
    };
    if !first_fr.is_finite() || *first_fr <= 0.0 {
        return None;
    }
    rest.iter()
        .all(
            |track| matches!(track, CssGridTrackSize::Fr(fr) if fr.to_bits() == first_fr.to_bits()),
        )
        .then_some(tracks.len())
}

fn children_have_auto_grid_placement(
    table: &LayoutNeighborTable,
    styles: &[Option<ComputedStyle>],
    index: usize,
) -> bool {
    let Some(first_child) = table.child_start.get(index).copied() else {
        return false;
    };
    if first_child == u32::MAX {
        return false;
    }
    let start = first_child as usize;
    let end = start + usize::from(table.child_count[index]);
    (start..end).all(|child| {
        styles
            .get(child)
            .and_then(Option::as_ref)
            .is_some_and(|style| {
                style.display == CssDisplay::None || grid_item_uses_auto_placement(style)
            })
    })
}

fn grid_item_uses_auto_placement(style: &ComputedStyle) -> bool {
    style.grid_item.column_start == CssGridLine::Auto
        && style.grid_item.column_end == CssGridLine::Auto
        && style.grid_item.row_start == CssGridLine::Auto
        && style.grid_item.row_end == CssGridLine::Auto
}

fn css_to_taffy_style(style: Option<&ComputedStyle>) -> Style {
    let Some(style) = style else {
        // Return a block style that fills available space.
        // Style::default() has display:Flex in taffy when the flexbox feature
        // is enabled (its DEFAULT const takes Flex over Block), which would
        // make unstyled container nodes into flex containers and break layout.
        return Style {
            display: TaffyDisplay::Block,
            ..Default::default()
        };
    };

    // Inline elements are mapped to Block as a coarse fallback because
    // taffy 0.10 has no native inline formatting context. Keeping the
    // arms separate documents the semantic difference; the lint is
    // suppressed here so future Inline-specific handling stays distinct.
    #[allow(clippy::match_same_arms)]
    let display = match style.display {
        CssDisplay::Block => TaffyDisplay::Block,
        CssDisplay::Flex | CssDisplay::InlineFlex => TaffyDisplay::Flex,
        CssDisplay::Grid => TaffyDisplay::Grid,
        CssDisplay::None => TaffyDisplay::None,
        CssDisplay::Inline => TaffyDisplay::Block,
    };

    let flex_direction = match style.flex_container.direction {
        CssFlexDirection::Row => FlexDirection::Row,
        CssFlexDirection::RowReverse => FlexDirection::RowReverse,
        CssFlexDirection::Column => FlexDirection::Column,
        CssFlexDirection::ColumnReverse => FlexDirection::ColumnReverse,
    };

    let flex_wrap = match style.flex_container.wrap {
        CssFlexWrap::Nowrap => FlexWrap::NoWrap,
        CssFlexWrap::Wrap => FlexWrap::Wrap,
        CssFlexWrap::WrapReverse => FlexWrap::WrapReverse,
    };

    let justify_content = Some(match style.flex_container.justify_content {
        CssJustifyContent::FlexStart => JustifyContent::FlexStart,
        CssJustifyContent::FlexEnd => JustifyContent::FlexEnd,
        CssJustifyContent::Center => JustifyContent::Center,
        CssJustifyContent::SpaceBetween => JustifyContent::SpaceBetween,
        CssJustifyContent::SpaceAround => JustifyContent::SpaceAround,
        CssJustifyContent::SpaceEvenly => JustifyContent::SpaceEvenly,
    });

    /*
     * taffy models out-of-flow boxes as Position::Absolute and lays them out
     * against the containing block's padding box with the `inset` offsets.
     * CSS `fixed` positions against the viewport instead; the engine has one
     * scrolling root and no separate fixed containing block, so it takes the
     * same arm and a fixed element scrolls with the page.
     * `sticky` computes as `relative` until a scroll-position constraint
     * exists to relax it against.
     */
    let position = match style.position {
        CssPosition::Absolute | CssPosition::Fixed => TaffyPosition::Absolute,
        CssPosition::Static | CssPosition::Relative | CssPosition::Sticky => {
            TaffyPosition::Relative
        }
    };

    // AlignItems::Baseline does not exist in taffy 0.10; use FlexStart as
    // fallback. Keeping the Baseline arm separate from FlexStart documents
    // the semantic fallback so a future taffy upgrade can replace it.
    #[allow(clippy::match_same_arms)]
    let align_items = Some(match style.flex_container.align_items {
        CssAlignItems::Stretch => AlignItems::Stretch,
        CssAlignItems::FlexStart => AlignItems::FlexStart,
        CssAlignItems::FlexEnd => AlignItems::FlexEnd,
        CssAlignItems::Center => AlignItems::Center,
        CssAlignItems::Baseline => AlignItems::FlexStart,
    });

    let align_self = match style.flex_item.align_self {
        CssAlignSelf::Auto => None,
        CssAlignSelf::FlexStart => Some(AlignSelf::FlexStart),
        CssAlignSelf::FlexEnd => Some(AlignSelf::FlexEnd),
        CssAlignSelf::Center => Some(AlignSelf::Center),
        CssAlignSelf::Stretch => Some(AlignSelf::Stretch),
        CssAlignSelf::Baseline => Some(AlignSelf::Baseline),
    };

    let flex_basis = match style.flex_item.flex_basis {
        FlexBasis::Auto => Dimension::auto(),
        FlexBasis::Length(Length::Px(px)) => Dimension::length(px),
        FlexBasis::Length(Length::Percent(p)) => Dimension::percent(p / 100.0),
        FlexBasis::Length(
            Length::Em(_)
            | Length::Rem(_)
            | Length::Vw(_)
            | Length::Vh(_)
            | Length::Vmin(_)
            | Length::Vmax(_),
        ) => Dimension::length(unresolved_font_relative_px()),
    };

    let gap_col = LengthPercentage::length(
        style
            .flex_container
            .column_gap
            .max(style.flex_container.gap),
    );
    let gap_row =
        LengthPercentage::length(style.flex_container.row_gap.max(style.flex_container.gap));

    // CSS Grid container properties.
    // GridTemplateComponent<String>: String is taffy's DefaultCheapStr for
    // named-line support; we only produce Single (unnamed) variants here.
    let grid_template_columns: Vec<GridTemplateComponent<String>> = style
        .grid_container
        .template_columns
        .iter()
        .map(|t| GridTemplateComponent::Single(track_size_to_taffy(t)))
        .collect();
    let grid_template_rows: Vec<GridTemplateComponent<String>> = style
        .grid_container
        .template_rows
        .iter()
        .map(|t| GridTemplateComponent::Single(track_size_to_taffy(t)))
        .collect();
    let grid_auto_columns: Vec<TrackSizingFunction> = style
        .grid_container
        .auto_columns
        .iter()
        .map(track_size_to_taffy)
        .collect();
    let grid_auto_rows: Vec<TrackSizingFunction> = style
        .grid_container
        .auto_rows
        .iter()
        .map(track_size_to_taffy)
        .collect();
    let grid_auto_flow = match style.grid_container.auto_flow {
        CssGridAutoFlow::Row => GridAutoFlow::Row,
        CssGridAutoFlow::Column => GridAutoFlow::Column,
        CssGridAutoFlow::RowDense => GridAutoFlow::RowDense,
        CssGridAutoFlow::ColumnDense => GridAutoFlow::ColumnDense,
    };
    let grid_column: Line<GridPlacement<String>> = Line {
        start: grid_line_to_taffy(style.grid_item.column_start),
        end: grid_line_to_taffy(style.grid_item.column_end),
    };
    let grid_row: Line<GridPlacement<String>> = Line {
        start: grid_line_to_taffy(style.grid_item.row_start),
        end: grid_line_to_taffy(style.grid_item.row_end),
    };
    let box_sizing = match style.box_sizing {
        CssBoxSizing::ContentBox => TaffyBoxSizing::ContentBox,
        CssBoxSizing::BorderBox => TaffyBoxSizing::BorderBox,
    };

    Style {
        display,
        box_sizing,
        flex_direction,
        flex_wrap,
        justify_content,
        align_items,
        align_self,
        flex_grow: style.flex_item.flex_grow,
        flex_shrink: style.flex_item.flex_shrink,
        flex_basis,
        margin: TaffyRect {
            left: length_or_auto_lpa(style.margin.left),
            right: length_or_auto_lpa(style.margin.right),
            top: length_or_auto_lpa(style.margin.top),
            bottom: length_or_auto_lpa(style.margin.bottom),
        },
        padding: TaffyRect {
            left: length_pct(style.padding.left),
            right: length_pct(style.padding.right),
            top: length_pct(style.padding.top),
            bottom: length_pct(style.padding.bottom),
        },
        border: TaffyRect {
            left: length_pct(style.border.left),
            right: length_pct(style.border.right),
            top: length_pct(style.border.top),
            bottom: length_pct(style.border.bottom),
        },
        gap: Size {
            width: gap_col,
            height: gap_row,
        },
        size: Size {
            width: length_or_auto_dim(style.width),
            height: length_or_auto_dim(style.height),
        },
        min_size: Size {
            width: length_dim(style.min_width),
            height: length_dim(style.min_height),
        },
        max_size: Size {
            width: opt_length_dim(style.max_width),
            height: opt_length_dim(style.max_height),
        },
        grid_template_columns,
        grid_template_rows,
        grid_auto_columns,
        grid_auto_rows,
        grid_auto_flow,
        grid_column,
        grid_row,
        position,
        inset: TaffyRect {
            left: length_or_auto_lpa(style.left),
            right: length_or_auto_lpa(style.right),
            top: length_or_auto_lpa(style.top),
            bottom: length_or_auto_lpa(style.bottom),
        },
        ..Default::default()
    }
}

/// Convert a silksurf-css `GridTrackSize` to a taffy `TrackSizingFunction`.
fn track_size_to_taffy(track: &CssGridTrackSize) -> TrackSizingFunction {
    match track {
        CssGridTrackSize::Auto => TrackSizingFunction::AUTO,
        CssGridTrackSize::MinContent => TrackSizingFunction::MIN_CONTENT,
        CssGridTrackSize::MaxContent => TrackSizingFunction::MAX_CONTENT,
        CssGridTrackSize::Length(Length::Px(px)) => length(*px),
        CssGridTrackSize::Length(Length::Percent(p)) => percent(*p / 100.0),
        CssGridTrackSize::Length(
            Length::Em(_)
            | Length::Rem(_)
            | Length::Vw(_)
            | Length::Vh(_)
            | Length::Vmin(_)
            | Length::Vmax(_),
        ) => length(unresolved_font_relative_px()),
        CssGridTrackSize::Fr(fr_val) => fr(*fr_val),
        CssGridTrackSize::Minmax(min, max) => {
            minmax(grid_track_min_to_taffy(*min), grid_track_max_to_taffy(*max))
        }
        CssGridTrackSize::FitContent(Length::Px(px)) => {
            TrackSizingFunction::fit_content(LengthPercentage::length(*px))
        }
        CssGridTrackSize::FitContent(Length::Percent(p)) => {
            TrackSizingFunction::fit_content(LengthPercentage::percent(*p / 100.0))
        }
        CssGridTrackSize::FitContent(
            Length::Em(_)
            | Length::Rem(_)
            | Length::Vw(_)
            | Length::Vh(_)
            | Length::Vmin(_)
            | Length::Vmax(_),
        ) => TrackSizingFunction::fit_content(LengthPercentage::length(
            unresolved_font_relative_px(),
        )),
    }
}

fn grid_track_min_to_taffy(min: CssGridTrackMin) -> MinTrackSizingFunction {
    match min {
        CssGridTrackMin::Auto => MinTrackSizingFunction::AUTO,
        CssGridTrackMin::MinContent => MinTrackSizingFunction::MIN_CONTENT,
        CssGridTrackMin::MaxContent => MinTrackSizingFunction::MAX_CONTENT,
        CssGridTrackMin::Length(Length::Px(px)) => MinTrackSizingFunction::length(px),
        CssGridTrackMin::Length(Length::Percent(p)) => MinTrackSizingFunction::percent(p / 100.0),
        CssGridTrackMin::Length(
            Length::Em(_)
            | Length::Rem(_)
            | Length::Vw(_)
            | Length::Vh(_)
            | Length::Vmin(_)
            | Length::Vmax(_),
        ) => MinTrackSizingFunction::length(unresolved_font_relative_px()),
    }
}

fn grid_track_max_to_taffy(max: CssGridTrackMax) -> MaxTrackSizingFunction {
    match max {
        CssGridTrackMax::Auto => MaxTrackSizingFunction::AUTO,
        CssGridTrackMax::MinContent => MaxTrackSizingFunction::MIN_CONTENT,
        CssGridTrackMax::MaxContent => MaxTrackSizingFunction::MAX_CONTENT,
        CssGridTrackMax::Length(Length::Px(px)) => MaxTrackSizingFunction::length(px),
        CssGridTrackMax::Length(Length::Percent(p)) => MaxTrackSizingFunction::percent(p / 100.0),
        CssGridTrackMax::Length(
            Length::Em(_)
            | Length::Rem(_)
            | Length::Vw(_)
            | Length::Vh(_)
            | Length::Vmin(_)
            | Length::Vmax(_),
        ) => MaxTrackSizingFunction::length(unresolved_font_relative_px()),
        CssGridTrackMax::Fr(fr_val) => MaxTrackSizingFunction::fr(fr_val),
    }
}

fn grid_line_to_taffy(line: CssGridLine) -> GridPlacement<String> {
    match line {
        CssGridLine::Auto => GridPlacement::Auto,
        CssGridLine::Line(n) => taffy_line(n),
        CssGridLine::Span(s) => taffy_span(s),
    }
}

fn length_or_auto_dim(v: LengthOrAuto) -> Dimension {
    match v {
        LengthOrAuto::Auto => Dimension::auto(),
        LengthOrAuto::Length(Length::Px(px)) => Dimension::length(px),
        LengthOrAuto::Length(Length::Percent(p)) => Dimension::percent(p / 100.0),
        LengthOrAuto::Length(
            Length::Em(_)
            | Length::Rem(_)
            | Length::Vw(_)
            | Length::Vh(_)
            | Length::Vmin(_)
            | Length::Vmax(_),
        ) => Dimension::length(unresolved_font_relative_px()),
    }
}

fn length_dim(v: Length) -> Dimension {
    match v {
        Length::Px(px) => Dimension::length(px),
        Length::Percent(p) => Dimension::percent(p / 100.0),
        Length::Em(_)
        | Length::Rem(_)
        | Length::Vw(_)
        | Length::Vh(_)
        | Length::Vmin(_)
        | Length::Vmax(_) => Dimension::length(unresolved_font_relative_px()),
    }
}

fn opt_length_dim(v: Option<Length>) -> Dimension {
    match v {
        None => Dimension::auto(),
        Some(Length::Px(px)) => Dimension::length(px),
        Some(Length::Percent(p)) => Dimension::percent(p / 100.0),
        Some(
            Length::Em(_)
            | Length::Rem(_)
            | Length::Vw(_)
            | Length::Vh(_)
            | Length::Vmin(_)
            | Length::Vmax(_),
        ) => Dimension::length(unresolved_font_relative_px()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn absolute_and_fixed_position_take_the_box_out_of_flow() {
        for position in [
            silksurf_css::Position::Absolute,
            silksurf_css::Position::Fixed,
        ] {
            let style = ComputedStyle {
                position,
                ..Default::default()
            };
            assert_eq!(
                css_to_taffy_style(Some(&style)).position,
                TaffyPosition::Absolute,
                "{position:?}"
            );
        }
    }

    #[test]
    fn static_relative_and_sticky_stay_in_flow() {
        for position in [
            silksurf_css::Position::Static,
            silksurf_css::Position::Relative,
            silksurf_css::Position::Sticky,
        ] {
            let style = ComputedStyle {
                position,
                ..Default::default()
            };
            assert_eq!(
                css_to_taffy_style(Some(&style)).position,
                TaffyPosition::Relative,
                "{position:?}"
            );
        }
    }

    #[test]
    fn the_offset_properties_reach_taffy_inset() {
        let style = ComputedStyle {
            position: silksurf_css::Position::Absolute,
            top: LengthOrAuto::Length(Length::Px(20.0)),
            left: LengthOrAuto::Length(Length::Px(40.0)),
            ..Default::default()
        };
        let taffy_style = css_to_taffy_style(Some(&style));
        assert_eq!(taffy_style.inset.top, LengthPercentageAuto::length(20.0));
        assert_eq!(taffy_style.inset.left, LengthPercentageAuto::length(40.0));
        assert_eq!(taffy_style.inset.right, LengthPercentageAuto::AUTO);
    }

    use super::*;
    use silksurf_dom::Dom;

    fn make_dom_with_text() -> (Dom, DomNodeId) {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let div = dom.create_element("div");
        let text = dom.create_text("Hello world");
        dom.append_child(root, div).unwrap();
        dom.append_child(div, text).unwrap();
        (dom, root)
    }

    #[test]
    fn rebuild_produces_slots_for_each_bfs_entry() {
        let (dom, root) = make_dom_with_text();
        let table = LayoutNeighborTable::build(&dom, root);
        let styles: Vec<Option<ComputedStyle>> = vec![None; table.len()];
        let mut tl = TaffyLayout::new();
        tl.rebuild(&dom, &table, &styles);
        assert_eq!(tl.taffy_nodes.len(), table.len());
        assert!(tl.taffy_nodes[0].is_some(), "root must have a taffy node");
    }

    #[test]
    fn compute_returns_true_for_non_empty_tree() {
        let (dom, root) = make_dom_with_text();
        let table = LayoutNeighborTable::build(&dom, root);
        let styles: Vec<Option<ComputedStyle>> = vec![None; table.len()];
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let mut tl = TaffyLayout::new();
        tl.rebuild(&dom, &table, &styles);
        let ok = tl.compute(&dom, &styles, &table.bfs_order, viewport);
        assert!(ok);
    }

    #[test]
    fn text_measure_cache_survives_same_generation() {
        let mut tl = TaffyLayout::new();
        tl.text_measure_generation = 7;
        tl.taffy_nodes.resize(2, None);
        tl.text_measure_cache
            .resize(2, CachedTextMeasures::default());
        tl.text_measure_cache[1].insert(CachedTextMeasure {
            font_size: 16.0,
            max_width: Some(80.0),
            width: 40.0,
            height: 16.0,
            text_len: 5,
        });

        tl.refresh_text_measure_cache(7);
        assert_eq!(tl.text_measure_generation, 7);
        assert_eq!(text_measure_cache_entry_count(&tl), 1);

        tl.refresh_text_measure_cache(8);
        assert_eq!(tl.text_measure_generation, 8);
        assert_eq!(text_measure_cache_entry_count(&tl), 0);
    }

    #[test]
    fn compute_sets_text_measure_generation() {
        let (dom, root) = make_dom_with_text();
        let table = LayoutNeighborTable::build(&dom, root);
        let styles: Vec<Option<ComputedStyle>> = vec![None; table.len()];
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let mut tl = TaffyLayout::new();
        tl.rebuild(&dom, &table, &styles);

        assert!(tl.compute(&dom, &styles, &table.bfs_order, viewport));
        assert_eq!(tl.text_measure_generation, dom.generation());
    }

    #[test]
    fn known_measure_size_returns_complete_dimensions() {
        assert_eq!(
            known_measure_size(Size {
                width: Some(21.0),
                height: Some(34.0),
            }),
            Some(Size {
                width: 21.0,
                height: 34.0,
            })
        );
        assert_eq!(
            known_measure_size(Size {
                width: Some(21.0),
                height: None,
            }),
            None
        );
    }

    fn text_measure_cache_entry_count(layout: &TaffyLayout) -> usize {
        layout
            .text_measure_cache
            .iter()
            .flat_map(|entries| entries.entries)
            .flatten()
            .count()
    }

    #[test]
    fn write_rects_fills_root_within_viewport() {
        let (dom, root) = make_dom_with_text();
        let table = LayoutNeighborTable::build(&dom, root);
        let styles: Vec<Option<ComputedStyle>> = vec![None; table.len()];
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let mut tl = TaffyLayout::new();
        tl.rebuild(&dom, &table, &styles);
        tl.compute(&dom, &styles, &table.bfs_order, viewport);
        let mut node_rects = vec![Rect::default(); table.len()];
        tl.write_rects(&table.parent_idx, &mut node_rects, viewport);
        assert!(node_rects[0].width <= viewport.width + 1.0);
        assert!(node_rects[0].height <= viewport.height + 1.0);
    }

    #[test]
    fn flex_row_places_two_children_side_by_side() {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let container = dom.create_element("div");
        let child_a = dom.create_element("div");
        let child_b = dom.create_element("div");
        dom.append_child(root, container).unwrap();
        dom.append_child(container, child_a).unwrap();
        dom.append_child(container, child_b).unwrap();

        let table = LayoutNeighborTable::build(&dom, root);
        let n = table.len();
        let mut styles: Vec<Option<ComputedStyle>> = vec![None; n];

        let container_style = ComputedStyle {
            display: CssDisplay::Flex,
            flex_container: silksurf_css::FlexContainerStyle {
                direction: CssFlexDirection::Row,
                ..Default::default()
            },
            ..Default::default()
        };

        let item_style = ComputedStyle {
            flex_item: silksurf_css::FlexItemStyle {
                flex_grow: 1.0,
                ..Default::default()
            },
            ..Default::default()
        };

        for (i, &node) in table.bfs_order.iter().enumerate() {
            if node == container {
                styles[i] = Some(container_style.clone());
            } else if node == child_a || node == child_b {
                styles[i] = Some(item_style.clone());
            }
        }

        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 100.0,
        };
        let mut tl = TaffyLayout::new();
        tl.rebuild(&dom, &table, &styles);
        tl.compute(&dom, &styles, &table.bfs_order, viewport);
        let mut node_rects = vec![Rect::default(); n];
        tl.write_rects(&table.parent_idx, &mut node_rects, viewport);

        let idx_a = table.node_to_bfs_idx[&child_a] as usize;
        let idx_b = table.node_to_bfs_idx[&child_b] as usize;

        let rect_a = node_rects[idx_a];
        let rect_b = node_rects[idx_b];

        assert!(rect_a.width > 0.0, "child_a width={}", rect_a.width);
        assert!(rect_b.width > 0.0, "child_b width={}", rect_b.width);
        assert!(
            rect_b.x >= rect_a.x + rect_a.width - 1.0,
            "child_b.x={} should be right of child_a end={}",
            rect_b.x,
            rect_a.x + rect_a.width
        );
    }

    #[test]
    fn collapsed_block_whitespace_does_not_create_taffy_node() {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let container = dom.create_element("div");
        let whitespace = dom.create_text("\n  ");
        let child = dom.create_element("p");
        dom.append_child(root, container).unwrap();
        dom.append_child(container, whitespace).unwrap();
        dom.append_child(container, child).unwrap();

        let table = LayoutNeighborTable::build(&dom, root);
        let styles: Vec<Option<ComputedStyle>> = vec![
            Some(ComputedStyle {
                display: CssDisplay::Block,
                ..Default::default()
            });
            table.len()
        ];
        let whitespace_idx = table.node_to_bfs_idx[&whitespace] as usize;

        let mut tl = TaffyLayout::new();
        tl.rebuild(&dom, &table, &styles);

        assert!(tl.taffy_nodes[whitespace_idx].is_none());
    }

    #[test]
    fn inline_text_flow_keeps_whitespace_taffy_node() {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let container = dom.create_element("p");
        let left = dom.create_text("left");
        let whitespace = dom.create_text(" ");
        let right = dom.create_text("right");
        dom.append_child(root, container).unwrap();
        dom.append_child(container, left).unwrap();
        dom.append_child(container, whitespace).unwrap();
        dom.append_child(container, right).unwrap();

        let table = LayoutNeighborTable::build(&dom, root);
        let styles: Vec<Option<ComputedStyle>> = vec![
            Some(ComputedStyle {
                display: CssDisplay::Inline,
                ..Default::default()
            });
            table.len()
        ];
        let whitespace_idx = table.node_to_bfs_idx[&whitespace] as usize;

        let mut tl = TaffyLayout::new();
        tl.rebuild(&dom, &table, &styles);

        assert!(tl.taffy_nodes[whitespace_idx].is_some());
    }

    #[test]
    fn transparent_code_wrapper_reuses_parent_layout_node() {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let pre = dom.create_element("pre");
        let code = dom.create_element("code");
        let text = dom.create_text("fn main() {}");
        dom.append_child(root, pre).unwrap();
        dom.append_child(pre, code).unwrap();
        dom.append_child(code, text).unwrap();

        let table = LayoutNeighborTable::build(&dom, root);
        let mut styles: Vec<Option<ComputedStyle>> = vec![
            Some(ComputedStyle {
                display: CssDisplay::Block,
                ..Default::default()
            });
            table.len()
        ];
        let code_idx = table.node_to_bfs_idx[&code] as usize;
        let text_idx = table.node_to_bfs_idx[&text] as usize;
        styles[code_idx] = Some(ComputedStyle {
            display: CssDisplay::Inline,
            ..Default::default()
        });
        styles[text_idx] = Some(ComputedStyle {
            display: CssDisplay::Inline,
            ..Default::default()
        });

        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let mut tl = TaffyLayout::new();
        tl.rebuild(&dom, &table, &styles);
        tl.compute(&dom, &styles, &table.bfs_order, viewport);
        let mut node_rects = vec![Rect::default(); table.len()];
        tl.write_rects(&table.parent_idx, &mut node_rects, viewport);

        assert!(tl.taffy_nodes[code_idx].is_none());
        assert!(tl.taffy_nodes[text_idx].is_none());
        assert_eq!(node_rects[code_idx], node_rects[text_idx]);
    }

    #[test]
    fn simple_equal_fr_grid_uses_flex_lowering() {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let grid = dom.create_element("div");
        let first = dom.create_element("article");
        let second = dom.create_element("article");
        dom.append_child(root, grid).unwrap();
        dom.append_child(grid, first).unwrap();
        dom.append_child(grid, second).unwrap();

        let table = LayoutNeighborTable::build(&dom, root);
        let mut styles: Vec<Option<ComputedStyle>> = vec![
            Some(ComputedStyle {
                display: CssDisplay::Block,
                ..Default::default()
            });
            table.len()
        ];
        let grid_idx = table.node_to_bfs_idx[&grid] as usize;
        let first_idx = table.node_to_bfs_idx[&first] as usize;
        styles[grid_idx] = Some(ComputedStyle {
            display: CssDisplay::Grid,
            grid_container: silksurf_css::GridContainerStyle {
                template_columns: vec![CssGridTrackSize::Fr(1.0), CssGridTrackSize::Fr(1.0)],
                ..Default::default()
            },
            ..Default::default()
        });

        let grid_style = css_to_taffy_style_for_index(&table, &styles, grid_idx);
        let child_style = css_to_taffy_style_for_index(&table, &styles, first_idx);

        assert_eq!(grid_style.display, TaffyDisplay::Flex);
        assert_eq!(grid_style.flex_wrap, FlexWrap::Wrap);
        assert_eq!(child_style.flex_basis, Dimension::percent(0.5));
    }

    #[test]
    fn css_box_sizing_maps_to_taffy() {
        let mut content = ComputedStyle {
            display: CssDisplay::Block,
            box_sizing: CssBoxSizing::ContentBox,
            ..Default::default()
        };
        let border = ComputedStyle {
            display: CssDisplay::Block,
            box_sizing: CssBoxSizing::BorderBox,
            ..Default::default()
        };

        assert_eq!(
            css_to_taffy_style(Some(&content)).box_sizing,
            TaffyBoxSizing::ContentBox
        );
        content.box_sizing = CssBoxSizing::BorderBox;
        assert_eq!(
            css_to_taffy_style(Some(&content)).box_sizing,
            TaffyBoxSizing::BorderBox
        );
        assert_eq!(
            css_to_taffy_style(Some(&border)).box_sizing,
            TaffyBoxSizing::BorderBox
        );
    }

    #[test]
    fn explicit_grid_child_placement_keeps_grid_solver() {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let grid = dom.create_element("div");
        let child = dom.create_element("article");
        dom.append_child(root, grid).unwrap();
        dom.append_child(grid, child).unwrap();

        let table = LayoutNeighborTable::build(&dom, root);
        let mut styles: Vec<Option<ComputedStyle>> = vec![
            Some(ComputedStyle {
                display: CssDisplay::Block,
                ..Default::default()
            });
            table.len()
        ];
        let grid_idx = table.node_to_bfs_idx[&grid] as usize;
        let child_idx = table.node_to_bfs_idx[&child] as usize;
        styles[grid_idx] = Some(ComputedStyle {
            display: CssDisplay::Grid,
            grid_container: silksurf_css::GridContainerStyle {
                template_columns: vec![CssGridTrackSize::Fr(1.0), CssGridTrackSize::Fr(1.0)],
                ..Default::default()
            },
            ..Default::default()
        });
        styles[child_idx] = Some(ComputedStyle {
            grid_item: silksurf_css::GridItemStyle {
                column_start: CssGridLine::Line(1),
                ..Default::default()
            },
            ..Default::default()
        });

        let grid_style = css_to_taffy_style_for_index(&table, &styles, grid_idx);

        assert_eq!(grid_style.display, TaffyDisplay::Grid);
    }
}

#[cfg(test)]
mod absolute_containing_block_tests {
    use super::*;
    use silksurf_dom::Dom;

    const VIEWPORT: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 600.0,
    };

    #[track_caller]
    fn assert_px(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.01,
            "expected {expected}, got {actual}"
        );
    }

    fn block(width: LengthOrAuto, height: LengthOrAuto, margin_left: f32) -> ComputedStyle {
        ComputedStyle {
            display: silksurf_css::Display::Block,
            width,
            height,
            margin: silksurf_css::Margins {
                top: LengthOrAuto::Length(Length::Px(0.0)),
                right: LengthOrAuto::Length(Length::Px(0.0)),
                bottom: LengthOrAuto::Length(Length::Px(0.0)),
                left: LengthOrAuto::Length(Length::Px(margin_left)),
            },
            ..Default::default()
        }
    }

    fn px(value: f32) -> LengthOrAuto {
        LengthOrAuto::Length(Length::Px(value))
    }

    /// Builds document -> host(relative, margin-left 40) -> mid(static,
    /// margin-left 25) -> target. `mid` offsets the target from `host`, so the
    /// two candidate containing blocks sit at different origins.
    fn target_under_static_wrapper(
        host_position: CssPosition,
        target_style: ComputedStyle,
    ) -> (Dom, LayoutNeighborTable, Vec<Option<ComputedStyle>>) {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let host = dom.create_element("div");
        let mid = dom.create_element("div");
        let target = dom.create_element("div");
        // UNWRAP-OK: every id came from this Dom, so no append can fail.
        dom.append_child(root, host).unwrap();
        // UNWRAP-OK: every id came from this Dom, so no append can fail.
        dom.append_child(host, mid).unwrap();
        // UNWRAP-OK: every id came from this Dom, so no append can fail.
        dom.append_child(mid, target).unwrap();
        let table = LayoutNeighborTable::build(&dom, root);
        let styles = vec![
            Some(block(px(1000.0), px(600.0), 0.0)),
            Some(ComputedStyle {
                position: host_position,
                ..block(px(400.0), px(200.0), 40.0)
            }),
            Some(block(px(300.0), LengthOrAuto::Auto, 25.0)),
            Some(target_style),
        ];
        (dom, table, styles)
    }

    fn rects(
        dom: &Dom,
        table: &LayoutNeighborTable,
        styles: &[Option<ComputedStyle>],
    ) -> Vec<Rect> {
        let mut layout = TaffyLayout::new();
        layout.rebuild(dom, table, styles);
        assert!(layout.compute(dom, styles, &table.bfs_order, VIEWPORT));
        let mut node_rects = vec![Rect::default(); table.len()];
        layout.write_rects(&table.parent_idx, &mut node_rects, VIEWPORT);
        node_rects
    }

    /// CSS Position 3 2.1 names the nearest ancestor whose position is not
    /// static. `mid` is static, so `host` at x=40 supplies the origin and the
    /// target lands at 40 + 20, not at `mid`'s 65 + 20.
    #[test]
    fn an_absolute_inset_resolves_against_the_nearest_positioned_ancestor() {
        let (dom, table, styles) = target_under_static_wrapper(
            CssPosition::Relative,
            ComputedStyle {
                position: CssPosition::Absolute,
                left: px(20.0),
                top: px(10.0),
                ..block(px(50.0), px(50.0), 0.0)
            },
        );
        let node_rects = rects(&dom, &table, &styles);
        assert_px(node_rects[1].x, 40.0);
        assert_px(node_rects[2].x, 65.0);
        assert_px(node_rects[3].x, 60.0);
        assert_px(node_rects[3].y, 10.0);
    }

    /// A percentage inset resolves against the containing block's size, which
    /// is what proves the box was reparented rather than merely re-offset:
    /// `host` is 400x200 while the static wrapper is 300 wide.
    #[test]
    fn an_absolute_percentage_resolves_against_the_containing_block_size() {
        let (dom, table, styles) = target_under_static_wrapper(
            CssPosition::Relative,
            ComputedStyle {
                position: CssPosition::Absolute,
                left: LengthOrAuto::Length(Length::Percent(50.0)),
                top: LengthOrAuto::Length(Length::Percent(25.0)),
                ..block(px(50.0), px(50.0), 0.0)
            },
        );
        let node_rects = rects(&dom, &table, &styles);
        // 50% of host's 400 gives 200, offset from host's own x=40.
        assert_px(node_rects[3].x, 240.0);
        // 25% of host's 200 gives 50.
        assert_px(node_rects[3].y, 50.0);
    }

    /// With no positioned ancestor the containing block is the initial
    /// containing block, which sits at the viewport origin.
    #[test]
    fn an_absolute_box_without_a_positioned_ancestor_takes_the_initial_block() {
        let (dom, table, styles) = target_under_static_wrapper(
            CssPosition::Static,
            ComputedStyle {
                position: CssPosition::Absolute,
                left: px(20.0),
                top: px(10.0),
                ..block(px(50.0), px(50.0), 0.0)
            },
        );
        let node_rects = rects(&dom, &table, &styles);
        assert_px(node_rects[3].x, 20.0);
        assert_px(node_rects[3].y, 10.0);
    }

    /// An absolute box whose DOM parent is already the containing block keeps
    /// taffy's own placement, so this path is unchanged by the reparenting.
    #[test]
    fn an_absolute_box_under_a_positioned_parent_keeps_its_taffy_placement() {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let host = dom.create_element("div");
        let target = dom.create_element("div");
        // UNWRAP-OK: every id came from this Dom, so no append can fail.
        dom.append_child(root, host).unwrap();
        // UNWRAP-OK: every id came from this Dom, so no append can fail.
        dom.append_child(host, target).unwrap();
        let table = LayoutNeighborTable::build(&dom, root);
        let styles = vec![
            Some(block(px(1000.0), px(600.0), 0.0)),
            Some(ComputedStyle {
                position: CssPosition::Relative,
                ..block(px(400.0), px(200.0), 40.0)
            }),
            Some(ComputedStyle {
                position: CssPosition::Absolute,
                left: px(20.0),
                top: px(10.0),
                ..block(px(50.0), px(50.0), 0.0)
            }),
        ];
        let node_rects = rects(&dom, &table, &styles);
        assert_px(node_rects[2].x, 60.0);
        assert_px(node_rects[2].y, 10.0);
    }

    /// CSS 2.1 10.6.3 gives a block box with no in-flow line box an auto
    /// height of zero. The static wrapper's only child lays out under `host`,
    /// so the wrapper collapses instead of taking the line-height floor.
    #[test]
    fn a_wrapper_emptied_by_reparenting_collapses_to_zero_height() {
        let (dom, table, styles) = target_under_static_wrapper(
            CssPosition::Relative,
            ComputedStyle {
                position: CssPosition::Absolute,
                left: px(20.0),
                top: px(10.0),
                ..block(px(50.0), px(50.0), 0.0)
            },
        );
        let node_rects = rects(&dom, &table, &styles);
        assert_px(node_rects[2].height, 0.0);
    }
}

#[cfg(test)]
mod viewport_anchor_tests {
    use super::*;
    use silksurf_dom::Dom;

    /// document > spacer(200px tall) > holder(margin-left 80px) > fixed
    fn fixed_under_offset_ancestor(
        fixed_style: ComputedStyle,
    ) -> (Dom, LayoutNeighborTable, Vec<Option<ComputedStyle>>) {
        let mut dom = Dom::new();
        let root = dom.create_document();
        let spacer = dom.create_element("div");
        let holder = dom.create_element("div");
        let fixed = dom.create_element("div");
        // UNWRAP-OK: every id came from this Dom, so no append can fail.
        dom.append_child(root, spacer).unwrap();
        dom.append_child(root, holder).unwrap();
        dom.append_child(holder, fixed).unwrap();
        let table = LayoutNeighborTable::build(&dom, root);
        let block = |width: f32, height: f32, margin_left: f32| ComputedStyle {
            display: silksurf_css::Display::Block,
            width: LengthOrAuto::Length(Length::Px(width)),
            height: LengthOrAuto::Length(Length::Px(height)),
            margin: silksurf_css::Margins {
                top: LengthOrAuto::Length(Length::Px(0.0)),
                right: LengthOrAuto::Length(Length::Px(0.0)),
                bottom: LengthOrAuto::Length(Length::Px(0.0)),
                left: LengthOrAuto::Length(Length::Px(margin_left)),
            },
            ..Default::default()
        };
        let styles = vec![
            Some(block(1000.0, 400.0, 0.0)),
            Some(block(1000.0, 200.0, 0.0)),
            Some(block(500.0, 100.0, 80.0)),
            Some(fixed_style),
        ];
        (dom, table, styles)
    }

    fn rects(
        dom: &Dom,
        table: &LayoutNeighborTable,
        styles: &[Option<ComputedStyle>],
        viewport: Rect,
    ) -> Vec<Rect> {
        let mut layout = TaffyLayout::new();
        layout.rebuild(dom, table, styles);
        assert!(layout.compute(dom, styles, &table.bfs_order, viewport));
        let mut node_rects = vec![Rect::default(); table.len()];
        layout.write_rects(&table.parent_idx, &mut node_rects, viewport);
        node_rects
    }

    /// taffy resolves inset arithmetic in f32, so a laid-out coordinate is
    /// compared within a sub-pixel tolerance rather than bit-for-bit.
    #[track_caller]
    fn assert_px(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.01,
            "expected {expected}, got {actual}"
        );
    }

    const VIEWPORT: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 600.0,
    };

    #[test]
    fn a_fixed_box_with_inset_zero_fills_the_viewport() {
        let zero = LengthOrAuto::Length(Length::Px(0.0));
        let (dom, table, styles) = fixed_under_offset_ancestor(ComputedStyle {
            position: CssPosition::Fixed,
            left: zero,
            right: zero,
            top: zero,
            bottom: zero,
            ..Default::default()
        });
        let node_rects = rects(&dom, &table, &styles, VIEWPORT);
        assert_px(node_rects[3].x, 0.0);
        assert_px(node_rects[3].y, 0.0);
        assert_px(node_rects[3].width, 1000.0);
        assert_px(node_rects[3].height, 600.0);
    }

    #[test]
    fn a_fixed_inset_resolves_against_the_viewport_rather_than_the_dom_parent() {
        let ten = LengthOrAuto::Length(Length::Px(10.0));
        let (dom, table, styles) = fixed_under_offset_ancestor(ComputedStyle {
            position: CssPosition::Fixed,
            right: ten,
            top: ten,
            width: LengthOrAuto::Length(Length::Px(100.0)),
            height: LengthOrAuto::Length(Length::Px(40.0)),
            ..Default::default()
        });
        let node_rects = rects(&dom, &table, &styles, VIEWPORT);
        // The holder sits at x=80, y=200; a viewport-anchored box ignores both.
        assert_px(node_rects[2].x, 80.0);
        assert_px(node_rects[2].y, 200.0);
        assert_px(node_rects[3].x, 1000.0 - 10.0 - 100.0);
        assert_px(node_rects[3].y, 10.0);
    }

    #[test]
    fn a_percentage_height_inside_a_fixed_box_resolves_against_the_viewport() {
        let zero = LengthOrAuto::Length(Length::Px(0.0));
        let (dom, table, styles) = fixed_under_offset_ancestor(ComputedStyle {
            position: CssPosition::Fixed,
            left: zero,
            top: zero,
            width: LengthOrAuto::Length(Length::Px(50.0)),
            height: LengthOrAuto::Length(Length::Percent(100.0)),
            ..Default::default()
        });
        let node_rects = rects(&dom, &table, &styles, VIEWPORT);
        assert_px(node_rects[3].height, 600.0);
    }

    #[test]
    fn a_fixed_box_with_auto_insets_keeps_its_static_position() {
        let (dom, table, styles) = fixed_under_offset_ancestor(ComputedStyle {
            position: CssPosition::Fixed,
            width: LengthOrAuto::Length(Length::Px(100.0)),
            height: LengthOrAuto::Length(Length::Px(40.0)),
            ..Default::default()
        });
        let node_rects = rects(&dom, &table, &styles, VIEWPORT);
        assert_px(node_rects[3].x, 80.0);
        assert_px(node_rects[3].y, 200.0);
    }

    #[test]
    fn a_document_without_a_fixed_box_lays_out_unchanged() {
        let (dom, table, styles) = fixed_under_offset_ancestor(ComputedStyle {
            display: silksurf_css::Display::Block,
            width: LengthOrAuto::Length(Length::Px(100.0)),
            height: LengthOrAuto::Length(Length::Px(40.0)),
            ..Default::default()
        });
        let mut layout = TaffyLayout::new();
        layout.rebuild(&dom, &table, &styles);
        assert!(layout.viewport_root.is_none());
        let node_rects = rects(&dom, &table, &styles, VIEWPORT);
        assert_px(node_rects[3].x, 80.0);
        assert_px(node_rects[3].y, 200.0);
    }
}
