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
    AlignItems as CssAlignItems, AlignSelf as CssAlignSelf, ComputedStyle, Display as CssDisplay,
    FlexBasis, FlexDirection as CssFlexDirection, FlexWrap as CssFlexWrap,
    GridAutoFlow as CssGridAutoFlow, GridLine as CssGridLine, GridTrackMax as CssGridTrackMax,
    GridTrackMin as CssGridTrackMin, GridTrackSize as CssGridTrackSize,
    JustifyContent as CssJustifyContent, Length, LengthOrAuto, WhiteSpace,
};
use silksurf_dom::{Dom, NodeId as DomNodeId, NodeKind};
use taffy::{
    AlignItems, AlignSelf, AvailableSpace, Dimension, Display as TaffyDisplay, FlexDirection,
    FlexWrap, GridAutoFlow, GridPlacement, GridTemplateComponent, JustifyContent, LengthPercentage,
    LengthPercentageAuto, Line, MaxTrackSizingFunction, MinTrackSizingFunction, NodeId as TaffyId,
    Size, Style, TaffyTree, TrackSizingFunction,
    geometry::Rect as TaffyRect,
    style_helpers::{
        TaffyAuto as _, TaffyFitContent as _, TaffyMaxContent as _, TaffyMinContent as _, fr,
        length, line as taffy_line, minmax, percent, span as taffy_span,
    },
};

use crate::{Rect, neighbor_table::LayoutNeighborTable};

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
    /// Per-compute text measurement cache keyed by BFS index.
    text_measure_cache: Vec<CachedTextMeasures>,
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

impl TaffyLayout {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tree: new_taffy_tree(16),
            taffy_nodes: Vec::new(),
            taffy_to_bfs: FxHashMap::default(),
            child_ids_scratch: Vec::new(),
            text_measure_cache: Vec::new(),
        }
    }

    /// Reconstruct the taffy tree from BFS table + computed styles.
    ///
    /// Must be called before `compute()` whenever the DOM or styles have changed.
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

        // Process in reverse BFS order: children before parents so
        // taffy node IDs are available when we build the parent node.
        for i in (0..n).rev() {
            if taffy_node_merges_into_parent(dom, table, styles, i) {
                if trace_taffy {
                    stats.record_skip(dom, table, styles, i);
                }
                continue;
            }
            let taffy_style = css_to_taffy_style(styles.get(i).and_then(Option::as_ref));
            self.child_ids_scratch.clear();
            let first_child = table.child_start[i];
            if first_child != u32::MAX {
                let start = first_child as usize;
                let end = start + usize::from(table.child_count[i]);
                self.child_ids_scratch
                    .extend((start..end).filter_map(|child_idx| self.taffy_nodes[child_idx]));
            }

            if trace_taffy {
                stats.child_edges += self.child_ids_scratch.len();
            }
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

            if let Ok(tn) = result {
                self.taffy_to_bfs.insert(tn, i);
                self.taffy_nodes[i] = Some(tn);
            }
        }
        if trace_taffy {
            stats.created = self.taffy_to_bfs.len();
            eprintln!(
                "[SilkSurf] taffy rebuild: bfs_nodes={n}, created={}, leaves={}, parents={}, child_edges={}, skipped={}, skipped_display_none={}, skipped_whitespace={}, skipped_text_merge={}",
                stats.created,
                stats.leaves,
                stats.parents,
                stats.child_edges,
                stats.skipped,
                stats.skipped_display_none,
                stats.skipped_whitespace,
                stats.skipped_text_merge
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
        self.text_measure_cache.clear();
        self.text_measure_cache
            .resize(self.taffy_nodes.len(), CachedTextMeasures::default());

        // Split borrow: tree needs &mut, taffy_to_bfs needs &.
        let TaffyLayout {
            tree,
            taffy_to_bfs,
            text_measure_cache,
            ..
        } = self;

        let result = tree.compute_layout_with_measure(
            root,
            available,
            |known, avail, taffy_node_id, _ctx, _style| {
                if let Some(stats) = trace_stats.as_mut() {
                    stats.calls += 1;
                }
                let Some(&bfs_idx) = taffy_to_bfs.get(&taffy_node_id) else {
                    return Size::ZERO;
                };

                let font_size = styles
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

                if let Some(line_h) = styles.get(bfs_idx).and_then(Option::as_ref).map(|s| match s
                    .line_height
                {
                    Length::Px(px) => px,
                    _ => 16.0,
                }) {
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
        if let Some(stats) = trace_stats {
            eprintln!(
                "[SilkSurf] taffy measure: calls={}, text_calls={}, text_cache_hits={}, text_bytes={}, max_text_bytes={}, text_time={:?}",
                stats.calls,
                stats.text_calls,
                stats.text_cache_hits,
                stats.text_bytes,
                stats.max_text_bytes,
                stats.text_elapsed
            );
        }
        result.is_ok()
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

            let (parent_x, parent_y) = if parent_idx[i] == u32::MAX {
                (viewport.x, viewport.y)
            } else {
                let p = parent_idx[i] as usize;
                if p < node_rects.len() {
                    (node_rects[p].x, node_rects[p].y)
                } else {
                    (viewport.x, viewport.y)
                }
            };

            node_rects[i] = Rect {
                x: parent_x + layout.location.x,
                y: parent_y + layout.location.y,
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
    text_calls: usize,
    text_bytes: usize,
    max_text_bytes: usize,
    text_elapsed: std::time::Duration,
    text_cache_hits: usize,
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
    let Some(node_id) = table.bfs_order.get(index).copied() else {
        return false;
    };
    let Ok(node) = dom.node(node_id) else {
        return false;
    };
    matches!(node.kind(), NodeKind::Text { .. })
        && text_node_parent_is_text_leaf(dom, table, styles, index)
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
        Length::Em(_) | Length::Rem(_) => {
            unreachable!("em/rem units must be resolved at cascade time before layout")
        }
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
        Length::Em(_) | Length::Rem(_) => {
            unreachable!("em/rem units must be resolved at cascade time before layout")
        }
    }
}

/// Convert a silksurf-css `ComputedStyle` to a taffy Style.
///
/// Converts `ComputedStyle` to `taffy::Style` for layout computation.
///
/// Width/height/min/max are converted from `LengthOrAuto` / `Option<Length>` to
/// taffy Dimension values. AUTO passes through as `Dimension::auto()`.
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
        FlexBasis::Length(Length::Em(_) | Length::Rem(_)) => {
            unreachable!("em/rem units must be resolved at cascade time before layout")
        }
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

    Style {
        display,
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
        CssGridTrackSize::Length(Length::Em(_) | Length::Rem(_)) => {
            unreachable!("em/rem units must be resolved at cascade time before layout")
        }
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
        CssGridTrackSize::FitContent(Length::Em(_) | Length::Rem(_)) => {
            unreachable!("em/rem units must be resolved at cascade time before layout")
        }
    }
}

fn grid_track_min_to_taffy(min: CssGridTrackMin) -> MinTrackSizingFunction {
    match min {
        CssGridTrackMin::Auto => MinTrackSizingFunction::AUTO,
        CssGridTrackMin::MinContent => MinTrackSizingFunction::MIN_CONTENT,
        CssGridTrackMin::MaxContent => MinTrackSizingFunction::MAX_CONTENT,
        CssGridTrackMin::Length(Length::Px(px)) => MinTrackSizingFunction::length(px),
        CssGridTrackMin::Length(Length::Percent(p)) => MinTrackSizingFunction::percent(p / 100.0),
        CssGridTrackMin::Length(Length::Em(_) | Length::Rem(_)) => {
            unreachable!("em/rem units must be resolved at cascade time before layout")
        }
    }
}

fn grid_track_max_to_taffy(max: CssGridTrackMax) -> MaxTrackSizingFunction {
    match max {
        CssGridTrackMax::Auto => MaxTrackSizingFunction::AUTO,
        CssGridTrackMax::MinContent => MaxTrackSizingFunction::MIN_CONTENT,
        CssGridTrackMax::MaxContent => MaxTrackSizingFunction::MAX_CONTENT,
        CssGridTrackMax::Length(Length::Px(px)) => MaxTrackSizingFunction::length(px),
        CssGridTrackMax::Length(Length::Percent(p)) => MaxTrackSizingFunction::percent(p / 100.0),
        CssGridTrackMax::Length(Length::Em(_) | Length::Rem(_)) => {
            unreachable!("em/rem units must be resolved at cascade time before layout")
        }
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
        LengthOrAuto::Length(Length::Em(_) | Length::Rem(_)) => {
            unreachable!("em/rem units must be resolved at cascade time before layout")
        }
    }
}

fn length_dim(v: Length) -> Dimension {
    match v {
        Length::Px(px) => Dimension::length(px),
        Length::Percent(p) => Dimension::percent(p / 100.0),
        Length::Em(_) | Length::Rem(_) => {
            unreachable!("em/rem units must be resolved at cascade time before layout")
        }
    }
}

fn opt_length_dim(v: Option<Length>) -> Dimension {
    match v {
        None => Dimension::auto(),
        Some(Length::Px(px)) => Dimension::length(px),
        Some(Length::Percent(p)) => Dimension::percent(p / 100.0),
        Some(Length::Em(_) | Length::Rem(_)) => {
            unreachable!("em/rem units must be resolved at cascade time before layout")
        }
    }
}

#[cfg(test)]
mod tests {
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
}
