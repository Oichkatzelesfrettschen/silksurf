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

use silksurf_css::{compute_style_for_node, ComputedStyle, Display, Stylesheet};
use silksurf_dom::{Dom, NodeId, NodeKind};
use silksurf_layout::neighbor_table::LayoutNeighborTable;
use silksurf_layout::Rect;
use silksurf_render::DisplayItem;
use rustc_hash::FxHashMap;

/// Result of the fused pipeline: styles + display list in one pass.
pub struct FusedResult {
    pub styles: FxHashMap<NodeId, ComputedStyle>,
    pub display_items: Vec<DisplayItem>,
    pub node_rects: FxHashMap<NodeId, Rect>,
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
    let table = LayoutNeighborTable::build(dom, root);
    let mut styles: FxHashMap<NodeId, ComputedStyle> = FxHashMap::default();
    let mut display_items: Vec<DisplayItem> = Vec::new();
    let mut node_rects: FxHashMap<NodeId, Rect> = FxHashMap::default();

    // Track cursor positions per parent for block flow
    let mut block_cursors: FxHashMap<NodeId, f32> = FxHashMap::default();
    block_cursors.insert(root, viewport.y);

    for level in &table.levels {
        for &node in level {
            // 1. CASCADE: compute style for this node
            let parent_style = dom
                .parent(node)
                .ok()
                .flatten()
                .and_then(|p| styles.get(&p));
            let style = compute_style_for_node(dom, node, stylesheet, parent_style);

            // Skip display:none
            if style.display == Display::None {
                styles.insert(node, style);
                continue;
            }

            // 2. LAYOUT: compute position (simplified block flow)
            let parent_rect = dom
                .parent(node)
                .ok()
                .flatten()
                .and_then(|p| node_rects.get(&p))
                .copied()
                .unwrap_or(viewport);

            let margin_top = length_px(style.margin.top);
            let padding_top = length_px(style.padding.top);
            let border_top = length_px(style.border.top);

            let cursor = block_cursors
                .get(&dom.parent(node).ok().flatten().unwrap_or(root))
                .copied()
                .unwrap_or(parent_rect.y);

            let x = parent_rect.x + length_px(style.margin.left)
                + length_px(style.padding.left) + length_px(style.border.left);
            let y = cursor + margin_top + padding_top + border_top;
            let width = parent_rect.width
                - length_px(style.margin.left) - length_px(style.margin.right)
                - length_px(style.padding.left) - length_px(style.padding.right)
                - length_px(style.border.left) - length_px(style.border.right);

            // Estimate height from line-height
            let height = length_px(style.line_height);

            let content_rect = Rect { x, y, width, height };
            node_rects.insert(node, content_rect);

            // Update parent's block cursor
            let parent_id = dom.parent(node).ok().flatten().unwrap_or(root);
            let new_cursor = y + height
                + length_px(style.padding.bottom) + length_px(style.border.bottom)
                + length_px(style.margin.bottom);
            block_cursors.insert(parent_id, new_cursor);

            // Initialize this node's cursor for its children
            block_cursors.entry(node).or_insert(y);

            // 3. PAINT: emit display items
            if style.background_color.a > 0 {
                display_items.push(DisplayItem::SolidColor {
                    rect: content_rect,
                    color: style.background_color,
                });
            }

            if let Ok(n) = dom.node(node) {
                if let NodeKind::Text { text } = n.kind() {
                    display_items.push(DisplayItem::Text {
                        rect: content_rect,
                        node,
                        text_len: text.len() as u32,
                        color: style.color,
                    });
                }
            }

            styles.insert(node, style);
        }
    }

    FusedResult {
        styles,
        display_items,
        node_rects,
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
        let viewport = Rect { x: 0.0, y: 0.0, width: 1280.0, height: 800.0 };

        let result = fused_style_layout_paint(&dom, &stylesheet, root, viewport);
        assert!(result.styles.contains_key(&root));
    }
}
