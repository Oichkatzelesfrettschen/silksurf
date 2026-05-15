/*
 * dimensions_soa.rs -- Structure-of-Arrays layout for Dimensions.
 *
 * WHY: A reflow pass that touches one dimension field (e.g. content_width)
 * across N nodes accesses only the matching Vec, keeping that single array
 * hot in L1/L2 cache.  The AoS layout (Dimensions stored per-node) would
 * load 16 f32 words per node even when only one word is needed.
 *
 * WHAT: DimensionsSoA holds 16 parallel Vec<f32> -- one per scalar field of
 * Dimensions.  The naming scheme is <box>_<field> where <box> is one of
 * {content, padding, border, margin} and <field> is one of {x, y, width,
 * height} (for content) or {top, right, bottom, left} (for edge boxes).
 *
 * HOW: push() decomposes a Dimensions into 16 scalars and appends one value
 * to each Vec.  get() reconstructs a Dimensions from the 16 parallel arrays
 * at a given index.  All Vecs are grown in lock-step so len() is the length
 * of any one of them.
 *
 * Feature gate: compiled only when --features dim-soa is passed.
 */

use crate::{Dimensions, EdgeSizes, Rect};

/// DimensionsSoA -- SoA mirror of the per-node Dimensions type.
///
/// Each field of Dimensions is stored as a separate Vec<f32>, ordered
/// identically to the logical node sequence so that index i in every Vec
/// corresponds to the same layout node.
///
/// content:  x, y, width, height  (Rect)
/// padding:  top, right, bottom, left  (EdgeSizes)
/// border:   top, right, bottom, left  (EdgeSizes)
/// margin:   top, right, bottom, left  (EdgeSizes)
#[derive(Debug, Default)]
pub struct DimensionsSoA {
    // content box
    content_x: Vec<f32>,
    content_y: Vec<f32>,
    content_width: Vec<f32>,
    content_height: Vec<f32>,

    // padding edges
    padding_top: Vec<f32>,
    padding_right: Vec<f32>,
    padding_bottom: Vec<f32>,
    padding_left: Vec<f32>,

    // border edges
    border_top: Vec<f32>,
    border_right: Vec<f32>,
    border_bottom: Vec<f32>,
    border_left: Vec<f32>,

    // margin edges
    margin_top: Vec<f32>,
    margin_right: Vec<f32>,
    margin_bottom: Vec<f32>,
    margin_left: Vec<f32>,
}

impl DimensionsSoA {
    /// new -- create an empty SoA container.
    pub fn new() -> Self {
        Self::default()
    }

    /// push -- append one Dimensions value to the container.
    ///
    /// WHY: Decomposing into parallel arrays at insertion time means readers
    /// never pay the decomposition cost; they access only the Vecs they need.
    pub fn push(&mut self, dims: &Dimensions) {
        self.content_x.push(dims.content.x);
        self.content_y.push(dims.content.y);
        self.content_width.push(dims.content.width);
        self.content_height.push(dims.content.height);

        self.padding_top.push(dims.padding.top);
        self.padding_right.push(dims.padding.right);
        self.padding_bottom.push(dims.padding.bottom);
        self.padding_left.push(dims.padding.left);

        self.border_top.push(dims.border.top);
        self.border_right.push(dims.border.right);
        self.border_bottom.push(dims.border.bottom);
        self.border_left.push(dims.border.left);

        self.margin_top.push(dims.margin.top);
        self.margin_right.push(dims.margin.right);
        self.margin_bottom.push(dims.margin.bottom);
        self.margin_left.push(dims.margin.left);
    }

    /// get -- reconstruct a Dimensions value at the given index.
    ///
    /// Returns None when index is out of bounds.  All 16 Vecs are kept in
    /// lock-step by push(), so checking content_x.get() is sufficient.
    pub fn get(&self, index: usize) -> Option<Dimensions> {
        // Bounds check via the first Vec; all Vecs have identical length.
        if index >= self.content_x.len() {
            return None;
        }
        Some(Dimensions {
            content: Rect {
                x: self.content_x[index],
                y: self.content_y[index],
                width: self.content_width[index],
                height: self.content_height[index],
            },
            padding: EdgeSizes {
                top: self.padding_top[index],
                right: self.padding_right[index],
                bottom: self.padding_bottom[index],
                left: self.padding_left[index],
            },
            border: EdgeSizes {
                top: self.border_top[index],
                right: self.border_right[index],
                bottom: self.border_bottom[index],
                left: self.border_left[index],
            },
            margin: EdgeSizes {
                top: self.margin_top[index],
                right: self.margin_right[index],
                bottom: self.margin_bottom[index],
                left: self.margin_left[index],
            },
        })
    }

    /// len -- number of Dimensions entries stored.
    pub fn len(&self) -> usize {
        self.content_x.len()
    }

    /// is_empty -- true when no entries have been pushed.
    pub fn is_empty(&self) -> bool {
        self.content_x.is_empty()
    }
}
