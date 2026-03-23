/*
 * style_soa.rs -- Structure-of-Arrays style storage for cache-efficient cascade.
 *
 * WHY: The standard FxHashMap<NodeId, ComputedStyle> stores ~300 bytes per node
 * in scattered heap locations. When the cascade reads "display" for ALL nodes,
 * it touches 300 bytes per node but only needs 1 byte (the Display enum).
 * Cache utilization: 1/300 = 0.3%.
 *
 * SoA stores each property in its own Vec, indexed by node order. Reading
 * "display" for all 401 ChatGPT nodes touches 401 bytes contiguously.
 * Cache utilization: 100%. That's a 300x improvement in cache efficiency
 * for property-at-a-time access patterns.
 *
 * Inspired by the gororoba LBM SoA solver which achieved 41.9 MLUPS via
 * the same transformation: one Vec<f32> per lattice direction instead of
 * a struct with 19 f64 fields per cell. 16:1 cache-line reuse.
 * See: gororoba_app/crates/gororoba_bevy_lbm/src/soa_solver.rs:100
 * See: gororoba_app/docs/engine_optimizations.md Section 3
 *
 * Construction: O(N) from FxHashMap<NodeId, ComputedStyle>
 * Access: O(1) per property per node via index
 * Memory: same total bytes, but contiguous per-property
 *
 * See: style.rs ComputedStyle for the AoS original
 * See: style.rs compute_styles() for the cascade that produces styles
 */

use crate::{Color, ComputedStyle, Display, FlexContainerStyle, FlexItemStyle,
            Length, Overflow, Position};
use rustc_hash::FxHashMap;
use silksurf_dom::NodeId;

/*
 * StyleSoA -- column-oriented style storage.
 *
 * Each CSS property gets its own Vec, indexed by a compact node index
 * (not NodeId directly -- we build a NodeId -> usize mapping).
 *
 * For ChatGPT (401 nodes, 30+ properties):
 *   AoS: 401 * ~300 bytes = 120KB scattered across HashMap buckets
 *   SoA: 30 * 401 * avg_size = same ~120KB but contiguous per-property
 */
pub struct StyleSoA {
    /// NodeId -> compact index mapping
    pub node_index: FxHashMap<NodeId, usize>,
    /// Compact index -> NodeId (reverse mapping)
    pub nodes: Vec<NodeId>,

    // Core properties (one Vec per CSS property)
    pub display: Vec<Display>,
    pub color: Vec<Color>,
    pub background_color: Vec<Color>,
    pub font_size: Vec<f32>,
    pub line_height: Vec<f32>,
    pub margin_top: Vec<f32>,
    pub margin_right: Vec<f32>,
    pub margin_bottom: Vec<f32>,
    pub margin_left: Vec<f32>,
    pub padding_top: Vec<f32>,
    pub padding_right: Vec<f32>,
    pub padding_bottom: Vec<f32>,
    pub padding_left: Vec<f32>,
    pub border_top: Vec<f32>,
    pub border_right: Vec<f32>,
    pub border_bottom: Vec<f32>,
    pub border_left: Vec<f32>,

    // Positioning
    pub position: Vec<Position>,
    pub z_index: Vec<i32>,
    pub overflow_x: Vec<Overflow>,
    pub overflow_y: Vec<Overflow>,
    pub opacity: Vec<f32>,

    // Flex container
    pub flex_container: Vec<FlexContainerStyle>,
    // Flex item
    pub flex_item: Vec<FlexItemStyle>,
}

impl StyleSoA {
    /*
     * from_computed -- convert AoS HashMap to SoA column storage.
     *
     * Complexity: O(N) where N = number of styled nodes
     * Memory: same total, redistributed into contiguous columns
     */
    pub fn from_computed(styles: &FxHashMap<NodeId, ComputedStyle>) -> Self {
        let n = styles.len();
        let mut soa = Self {
            node_index: FxHashMap::default(),
            nodes: Vec::with_capacity(n),
            display: Vec::with_capacity(n),
            color: Vec::with_capacity(n),
            background_color: Vec::with_capacity(n),
            font_size: Vec::with_capacity(n),
            line_height: Vec::with_capacity(n),
            margin_top: Vec::with_capacity(n),
            margin_right: Vec::with_capacity(n),
            margin_bottom: Vec::with_capacity(n),
            margin_left: Vec::with_capacity(n),
            padding_top: Vec::with_capacity(n),
            padding_right: Vec::with_capacity(n),
            padding_bottom: Vec::with_capacity(n),
            padding_left: Vec::with_capacity(n),
            border_top: Vec::with_capacity(n),
            border_right: Vec::with_capacity(n),
            border_bottom: Vec::with_capacity(n),
            border_left: Vec::with_capacity(n),
            position: Vec::with_capacity(n),
            z_index: Vec::with_capacity(n),
            overflow_x: Vec::with_capacity(n),
            overflow_y: Vec::with_capacity(n),
            opacity: Vec::with_capacity(n),
            flex_container: Vec::with_capacity(n),
            flex_item: Vec::with_capacity(n),
        };

        for (&node_id, style) in styles {
            let idx = soa.nodes.len();
            soa.node_index.insert(node_id, idx);
            soa.nodes.push(node_id);

            soa.display.push(style.display);
            soa.color.push(style.color);
            soa.background_color.push(style.background_color);
            soa.font_size.push(length_to_f32(style.font_size));
            soa.line_height.push(length_to_f32(style.line_height));

            soa.margin_top.push(length_to_f32(style.margin.top));
            soa.margin_right.push(length_to_f32(style.margin.right));
            soa.margin_bottom.push(length_to_f32(style.margin.bottom));
            soa.margin_left.push(length_to_f32(style.margin.left));
            soa.padding_top.push(length_to_f32(style.padding.top));
            soa.padding_right.push(length_to_f32(style.padding.right));
            soa.padding_bottom.push(length_to_f32(style.padding.bottom));
            soa.padding_left.push(length_to_f32(style.padding.left));
            soa.border_top.push(length_to_f32(style.border.top));
            soa.border_right.push(length_to_f32(style.border.right));
            soa.border_bottom.push(length_to_f32(style.border.bottom));
            soa.border_left.push(length_to_f32(style.border.left));

            soa.position.push(style.position);
            soa.z_index.push(style.z_index);
            soa.overflow_x.push(style.overflow_x);
            soa.overflow_y.push(style.overflow_y);
            soa.opacity.push(style.opacity);

            soa.flex_container.push(style.flex_container);
            soa.flex_item.push(style.flex_item);
        }

        soa
    }

    /// Number of styled nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Look up the compact index for a NodeId.
    pub fn index_of(&self, node: NodeId) -> Option<usize> {
        self.node_index.get(&node).copied()
    }
}

fn length_to_f32(length: Length) -> f32 {
    match length {
        Length::Px(v) => v,
        Length::Percent(v) => v, // Store percentage as-is; resolve at layout time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soa_from_empty() {
        let styles = FxHashMap::default();
        let soa = StyleSoA::from_computed(&styles);
        assert!(soa.is_empty());
        assert_eq!(soa.len(), 0);
    }

    #[test]
    fn test_soa_from_single_node() {
        let mut styles = FxHashMap::default();
        let node = NodeId::from_raw(0);
        styles.insert(node, ComputedStyle::default());

        let soa = StyleSoA::from_computed(&styles);
        assert_eq!(soa.len(), 1);
        assert_eq!(soa.display[0], Display::Inline);
        assert_eq!(soa.opacity[0], 1.0);
        assert_eq!(soa.index_of(node), Some(0));
    }
}
