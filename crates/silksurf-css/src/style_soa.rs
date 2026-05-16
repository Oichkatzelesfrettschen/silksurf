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

use crate::{
    BoxShadow, Color, ComputedStyle, Display, Edges, FlexContainerStyle, FlexItemStyle, FontStyle,
    FontWeight, Length, LengthOrAuto, LinearGradient, Overflow, Position, TextAlign,
};
use rustc_hash::FxHashMap;
use silksurf_dom::NodeId;
use smallvec::SmallVec;
use smol_str::SmolStr;

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
// ALLOW: StyleSoA is a future-use SoA cache type; callers (fused_pipeline.rs)
// will wire it in during Phase 4.4. Not constructed outside tests yet.
#[allow(dead_code)]
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

// ALLOW: Methods wired in at Phase 4.4 via fused_pipeline; unused until then.
#[allow(dead_code)]
impl StyleSoA {
    /*
     * from_bfs -- build SoA directly from the fused pipeline's BFS-ordered output.
     *
     * WHY: fused_pipeline produces Vec<Option<ComputedStyle>> indexed by BFS position
     * alongside the BFS node order Vec<NodeId>. Converting via FxHashMap would re-hash
     * every node; this constructor fills columns in one O(N) pass with no extra allocation.
     *
     * Invariant: bfs_order.len() == styles.len(). None slots (display:none) are skipped
     * and do not receive a column entry -- so soa.len() <= bfs_order.len().
     *
     * See: fused_pipeline.rs FusedResult for the source arrays.
     * See: from_computed for the legacy FxHashMap path.
     */
    pub fn from_bfs(bfs_order: &[NodeId], styles: &[Option<ComputedStyle>]) -> Self {
        let n = styles.iter().filter(|s| s.is_some()).count();
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

        for (&node_id, style) in bfs_order.iter().zip(styles.iter()) {
            let Some(style) = style else { continue };
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

// ALLOW: Used only by StyleSoA which is itself dead until Phase 4.4 wiring.
#[allow(dead_code)]
fn length_to_f32(length: Length) -> f32 {
    match length {
        Length::Px(v) => v,
        Length::Percent(v) => v, // Store percentage as-is; resolve at layout time
    }
}

/*
 * ComputedStyleSoA -- per-property parallel arrays for ComputedStyle.
 *
 * WHY: Mirrors the AoS ComputedStyle exactly, one Vec<T> per field, indexed
 * by insertion order. This enables reading a single property (e.g. display)
 * for all N nodes in a contiguous cache line sweep rather than striding
 * through full ComputedStyle structs. All field types match ComputedStyle.
 *
 * The type is intentionally simple: push inserts one AoS entry into all
 * columns simultaneously; get reconstructs the AoS entry for one index.
 * Consumers that need column-oriented access read the public(crate) Vecs
 * directly.
 *
 * Memory invariant: all column Vecs always have equal length == self.len().
 * This invariant is maintained by push only -- never mutate columns directly.
 *
 * See: style.rs ComputedStyle for the AoS original this mirrors exactly.
 * See: StyleSoA for a flattened variant that expands Edges into four columns.
 */
// ALLOW: ComputedStyleSoA is an internal SoA API; callers will be wired in
// during Phase 4.4. The type is exercised by tests under the cs-soa feature.
#[allow(dead_code)]
pub(crate) struct ComputedStyleSoA {
    pub(crate) display: Vec<Display>,
    pub(crate) color: Vec<Color>,
    pub(crate) background_color: Vec<Color>,
    pub(crate) font_size: Vec<Length>,
    pub(crate) line_height: Vec<Length>,
    pub(crate) font_family: Vec<SmallVec<[SmolStr; 2]>>,
    pub(crate) margin: Vec<Edges>,
    pub(crate) padding: Vec<Edges>,
    pub(crate) border: Vec<Edges>,
    pub(crate) flex_container: Vec<FlexContainerStyle>,
    pub(crate) flex_item: Vec<FlexItemStyle>,
    pub(crate) position: Vec<Position>,
    pub(crate) top: Vec<LengthOrAuto>,
    pub(crate) right: Vec<LengthOrAuto>,
    pub(crate) bottom: Vec<LengthOrAuto>,
    pub(crate) left: Vec<LengthOrAuto>,
    pub(crate) z_index: Vec<i32>,
    pub(crate) overflow_x: Vec<Overflow>,
    pub(crate) overflow_y: Vec<Overflow>,
    pub(crate) opacity: Vec<f32>,
    // Text
    pub(crate) text_align: Vec<TextAlign>,
    pub(crate) font_weight: Vec<FontWeight>,
    pub(crate) font_style: Vec<FontStyle>,
    // Decoration
    pub(crate) border_radius: Vec<f32>,
    pub(crate) box_shadow: Vec<Option<BoxShadow>>,
    pub(crate) background_image: Vec<Option<LinearGradient>>,
}

impl Default for ComputedStyleSoA {
    fn default() -> Self {
        Self::new()
    }
}

// ALLOW: All methods are future-use API exercised by tests; no production
// callers exist yet (wired in Phase 4.4).
#[allow(dead_code)]
impl ComputedStyleSoA {
    /// Create an empty SoA store with all columns at zero capacity.
    pub(crate) fn new() -> Self {
        Self {
            display: Vec::new(),
            color: Vec::new(),
            background_color: Vec::new(),
            font_size: Vec::new(),
            line_height: Vec::new(),
            font_family: Vec::new(),
            margin: Vec::new(),
            padding: Vec::new(),
            border: Vec::new(),
            flex_container: Vec::new(),
            flex_item: Vec::new(),
            position: Vec::new(),
            top: Vec::new(),
            right: Vec::new(),
            bottom: Vec::new(),
            left: Vec::new(),
            z_index: Vec::new(),
            overflow_x: Vec::new(),
            overflow_y: Vec::new(),
            opacity: Vec::new(),
            text_align: Vec::new(),
            font_weight: Vec::new(),
            font_style: Vec::new(),
            border_radius: Vec::new(),
            box_shadow: Vec::new(),
            background_image: Vec::new(),
        }
    }

    /*
     * push -- append one node's ComputedStyle into every column.
     *
     * WHY: Keeps all column Vecs at equal length after every call.
     * Clones font_family (SmallVec<[SmolStr; 2]>) because SmolStr is Clone
     * but not Copy. All other fields are Copy types and are copied directly.
     */
    pub(crate) fn push(&mut self, style: &ComputedStyle) {
        self.display.push(style.display);
        self.color.push(style.color);
        self.background_color.push(style.background_color);
        self.font_size.push(style.font_size);
        self.line_height.push(style.line_height);
        self.font_family.push(style.font_family.clone());
        self.margin.push(style.margin);
        self.padding.push(style.padding);
        self.border.push(style.border);
        self.flex_container.push(style.flex_container);
        self.flex_item.push(style.flex_item);
        self.position.push(style.position);
        self.top.push(style.top);
        self.right.push(style.right);
        self.bottom.push(style.bottom);
        self.left.push(style.left);
        self.z_index.push(style.z_index);
        self.overflow_x.push(style.overflow_x);
        self.overflow_y.push(style.overflow_y);
        self.opacity.push(style.opacity);
        self.text_align.push(style.text_align);
        self.font_weight.push(style.font_weight);
        self.font_style.push(style.font_style);
        self.border_radius.push(style.border_radius);
        self.box_shadow.push(style.box_shadow);
        self.background_image.push(style.background_image.clone());
    }

    /*
     * get -- reconstruct the AoS ComputedStyle for one index.
     *
     * Returns None if index >= self.len(). Clones font_family to satisfy
     * the owned return type; all other fields are Copy.
     */
    pub(crate) fn get(&self, index: usize) -> Option<ComputedStyle> {
        if index >= self.len() {
            return None;
        }
        Some(ComputedStyle {
            display: self.display[index],
            color: self.color[index],
            background_color: self.background_color[index],
            font_size: self.font_size[index],
            line_height: self.line_height[index],
            font_family: self.font_family[index].clone(),
            margin: self.margin[index],
            padding: self.padding[index],
            border: self.border[index],
            flex_container: self.flex_container[index],
            flex_item: self.flex_item[index],
            position: self.position[index],
            top: self.top[index],
            right: self.right[index],
            bottom: self.bottom[index],
            left: self.left[index],
            z_index: self.z_index[index],
            overflow_x: self.overflow_x[index],
            overflow_y: self.overflow_y[index],
            opacity: self.opacity[index],
            text_align: self.text_align[index],
            font_weight: self.font_weight[index],
            font_style: self.font_style[index],
            border_radius: self.border_radius[index],
            box_shadow: self.box_shadow[index],
            background_image: self.background_image[index].clone(),
        })
    }

    /// Number of nodes stored.
    pub(crate) fn len(&self) -> usize {
        self.display.len()
    }

    /// True when no nodes have been pushed.
    pub(crate) fn is_empty(&self) -> bool {
        self.display.is_empty()
    }
}

#[cfg(test)]
mod computed_soa_tests {
    use super::*;

    fn make_style(opacity: f32) -> ComputedStyle {
        ComputedStyle {
            opacity,
            ..ComputedStyle::default()
        }
    }

    /*
     * round_trip -- push 3 ComputedStyle values, get them back, assert equality.
     *
     * WHY: Validates that push -> get is lossless for all fields and that
     * each index is independent. Uses opacity as a discriminator because
     * it is a plain f32 that differs per call.
     */
    #[test]
    fn round_trip() {
        let mut soa = ComputedStyleSoA::new();
        let a = make_style(0.25);
        let b = make_style(0.5);
        let c = make_style(0.75);
        soa.push(&a);
        soa.push(&b);
        soa.push(&c);

        // UNWRAP-OK: we just pushed 3 entries so indices 0, 1, 2 are valid.
        let got_a = soa.get(0).expect("index 0 must exist");
        // UNWRAP-OK: we just pushed 3 entries so indices 0, 1, 2 are valid.
        let got_b = soa.get(1).expect("index 1 must exist");
        // UNWRAP-OK: we just pushed 3 entries so indices 0, 1, 2 are valid.
        let got_c = soa.get(2).expect("index 2 must exist");

        assert_eq!(got_a.opacity, a.opacity);
        assert_eq!(got_b.opacity, b.opacity);
        assert_eq!(got_c.opacity, c.opacity);

        assert_eq!(got_a.display, a.display);
        assert_eq!(got_a.color, a.color);
        assert_eq!(got_a.background_color, a.background_color);
        assert_eq!(got_a.font_size, a.font_size);
        assert_eq!(got_a.line_height, a.line_height);
        assert_eq!(got_a.font_family, a.font_family);
        assert_eq!(got_a.margin, a.margin);
        assert_eq!(got_a.padding, a.padding);
        assert_eq!(got_a.border, a.border);
        assert_eq!(got_a.position, a.position);
        assert_eq!(got_a.top, a.top);
        assert_eq!(got_a.right, a.right);
        assert_eq!(got_a.bottom, a.bottom);
        assert_eq!(got_a.left, a.left);
        assert_eq!(got_a.z_index, a.z_index);
        assert_eq!(got_a.overflow_x, a.overflow_x);
        assert_eq!(got_a.overflow_y, a.overflow_y);

        assert!(soa.get(3).is_none(), "out-of-bounds get must return None");
    }

    /*
     * len_matches_push_count -- push N styles, assert len() == N.
     *
     * WHY: Catches any column length drift introduced by future edits to push.
     */
    #[test]
    fn len_matches_push_count() {
        let mut soa = ComputedStyleSoA::new();
        assert_eq!(soa.len(), 0);
        assert!(soa.is_empty());
        for count in 1..=7usize {
            soa.push(&ComputedStyle::default());
            assert_eq!(soa.len(), count, "len after {count} pushes");
            assert!(!soa.is_empty());
        }
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
