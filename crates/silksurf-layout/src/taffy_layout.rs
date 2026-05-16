/*
 * taffy_layout.rs -- CSS Flexbox + Grid layout via the taffy crate.
 *
 * WHY: The hand-written flex.rs algorithm covers flex-direction, flex-grow,
 * flex-shrink, flex-basis, and justify-content, but has no CSS Grid support
 * and does not handle the taffy measure protocol needed for correct text
 * intrinsic sizing.  taffy is a production layout engine (used by Bevy, Dioxus,
 * Slint) that covers Block, Flexbox, and Grid with a stable, audited API.
 *
 * WHAT: TaffyLayout holds a cached TaffyTree<()> plus a mapping from taffy
 * NodeId to BFS index.  rebuild() reconstructs the tree from the BFS traversal
 * table and the per-node ComputedStyles.  compute() runs the layout algorithm
 * with a measure function that calls silksurf_text::measure_text for text leaf
 * nodes.  write_rects() extracts absolute positions into node_rects[].
 *
 * HOW:
 *   let mut tl = TaffyLayout::new();
 *   tl.rebuild(&table, &styles);
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
    JustifyContent as CssJustifyContent, Length,
};
use silksurf_dom::{Dom, NodeId as DomNodeId, NodeKind};
use taffy::{
    AlignItems, AlignSelf, AvailableSpace, Dimension, Display as TaffyDisplay, FlexDirection,
    FlexWrap, JustifyContent, LengthPercentage, LengthPercentageAuto, NodeId as TaffyId, Size,
    Style, TaffyTree, geometry::Rect as TaffyRect,
};

use crate::{Rect, neighbor_table::LayoutNeighborTable};

pub type SilkTaffy = TaffyTree<()>;

/// Cached taffy layout state held inside FusedWorkspace.
///
/// Invariant: taffy_nodes[i] corresponds to bfs_order[i] from the last rebuild().
pub struct TaffyLayout {
    tree: SilkTaffy,
    /// BFS index -> taffy node id.
    taffy_nodes: Vec<Option<TaffyId>>,
    /// Reverse map: taffy id -> BFS index (for the measure-function lookup).
    taffy_to_bfs: FxHashMap<TaffyId, usize>,
}

impl TaffyLayout {
    pub fn new() -> Self {
        Self {
            tree: TaffyTree::new(),
            taffy_nodes: Vec::new(),
            taffy_to_bfs: FxHashMap::default(),
        }
    }

    /// Reconstruct the taffy tree from BFS table + computed styles.
    ///
    /// Must be called before compute() whenever the DOM or styles have changed.
    pub fn rebuild(&mut self, table: &LayoutNeighborTable, styles: &[Option<ComputedStyle>]) {
        self.tree = TaffyTree::new();
        let n = table.len();
        self.taffy_nodes.clear();
        self.taffy_nodes.resize(n, None);
        self.taffy_to_bfs.clear();

        // For each BFS node, collect its taffy children so we can build
        // taffy parent nodes after their children are created.
        let mut children_of: Vec<Vec<usize>> = vec![vec![]; n];
        for i in 1..n {
            let pidx = table.parent_idx[i] as usize;
            children_of[pidx].push(i);
        }

        // Process in reverse BFS order: children before parents so
        // taffy node IDs are available when we build the parent node.
        for i in (0..n).rev() {
            let taffy_style = css_to_taffy_style(styles.get(i).and_then(Option::as_ref));

            let child_ids: Vec<TaffyId> = children_of[i]
                .iter()
                .filter_map(|&c| self.taffy_nodes[c])
                .collect();

            let result = if child_ids.is_empty() {
                self.tree.new_leaf(taffy_style)
            } else {
                self.tree.new_with_children(taffy_style, &child_ids)
            };

            if let Ok(tn) = result {
                self.taffy_to_bfs.insert(tn, i);
                self.taffy_nodes[i] = Some(tn);
            }
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
        let root = match self.taffy_nodes.first().and_then(|n| *n) {
            Some(r) => r,
            None => return false,
        };
        let available = Size {
            width: AvailableSpace::Definite(viewport.width),
            height: AvailableSpace::Definite(viewport.height),
        };

        // Split borrow: tree needs &mut, taffy_to_bfs needs &.
        let TaffyLayout {
            tree, taffy_to_bfs, ..
        } = self;

        tree.compute_layout_with_measure(
            root,
            available,
            |known, avail, taffy_node_id, _ctx, _style| {
                let bfs_idx = match taffy_to_bfs.get(&taffy_node_id) {
                    Some(&idx) => idx,
                    None => return Size::ZERO,
                };

                let font_size = styles
                    .get(bfs_idx)
                    .and_then(Option::as_ref)
                    .map(|s| match s.font_size {
                        Length::Px(px) => px,
                        _ => 16.0,
                    })
                    .unwrap_or(16.0);

                let max_w = match avail.width {
                    AvailableSpace::Definite(w) => Some(w),
                    _ => None,
                };

                let dom_node_id = match bfs_order.get(bfs_idx) {
                    Some(&id) => id,
                    None => return Size::ZERO,
                };

                if let Ok(node) = dom.node(dom_node_id)
                    && let NodeKind::Text { text } = node.kind()
                {
                    let (w, h) = silksurf_text::measure_text(text, font_size, max_w);
                    return Size {
                        width: w,
                        height: h,
                    };
                }

                // Element leaf node with no text: use line_height as minimum height.
                let line_h = styles
                    .get(bfs_idx)
                    .and_then(Option::as_ref)
                    .map(|s| match s.line_height {
                        Length::Px(px) => px,
                        _ => 16.0,
                    })
                    .unwrap_or(16.0);

                Size {
                    width: known.width.unwrap_or(0.0),
                    height: known.height.unwrap_or(line_h),
                }
            },
        )
        .is_ok()
    }

    /// Write absolute positions from taffy layout results into node_rects.
    ///
    /// taffy's Layout.location is parent-relative, so we accumulate offsets
    /// down the BFS tree (parents are always processed before children in
    /// BFS order, so node_rects[parent] is already filled when we process child).
    pub fn write_rects(&self, parent_idx: &[u32], node_rects: &mut [Rect], viewport: Rect) {
        let n = self.taffy_nodes.len().min(node_rects.len());
        for i in 0..n {
            let tn = match self.taffy_nodes[i] {
                Some(t) => t,
                None => continue,
            };
            let layout = match self.tree.layout(tn) {
                Ok(l) => l,
                Err(_) => continue,
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

fn length_auto(l: Length) -> LengthPercentageAuto {
    match l {
        Length::Px(px) => LengthPercentageAuto::length(px),
        Length::Percent(p) => LengthPercentageAuto::percent(p / 100.0),
    }
}

fn length_pct(l: Length) -> LengthPercentage {
    match l {
        Length::Px(px) => LengthPercentage::length(px),
        Length::Percent(p) => LengthPercentage::percent(p / 100.0),
    }
}

/// Convert a silksurf-css ComputedStyle to a taffy Style.
///
/// Width/height are AUTO unless the style has explicit pixel/percent values.
/// (ComputedStyle does not carry width/height yet; that is deferred to Phase 4.4.)
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

    // AlignItems::Baseline does not exist in taffy 0.10; use FlexStart as fallback.
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
    };

    let gap_col = LengthPercentage::length(
        style
            .flex_container
            .column_gap
            .max(style.flex_container.gap),
    );
    let gap_row =
        LengthPercentage::length(style.flex_container.row_gap.max(style.flex_container.gap));

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
            left: length_auto(style.margin.left),
            right: length_auto(style.margin.right),
            top: length_auto(style.margin.top),
            bottom: length_auto(style.margin.bottom),
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
        ..Default::default()
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
    fn rebuild_produces_nodes_for_each_bfs_entry() {
        let (dom, root) = make_dom_with_text();
        let table = LayoutNeighborTable::build(&dom, root);
        let styles: Vec<Option<ComputedStyle>> = vec![None; table.len()];
        let mut tl = TaffyLayout::new();
        tl.rebuild(&table, &styles);
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
        tl.rebuild(&table, &styles);
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
        tl.rebuild(&table, &styles);
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
        tl.rebuild(&table, &styles);
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
}
