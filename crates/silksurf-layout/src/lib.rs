/*
 * layout/lib.rs -- CSS box model layout engine (block, inline, flex).
 *
 * WHY: Transforms a styled DOM tree into positioned layout boxes. Each box
 * has content, padding, border, and margin dimensions. Block boxes flow
 * vertically; inline boxes flow horizontally with line wrapping.
 *
 * Architecture:
 *   build_layout_tree: DOM + styles -> arena-allocated LayoutBox tree
 *   layout_block: computes block flow (vertical stacking, margin collapse)
 *   layout_inline: computes inline flow (text measurement, line breaking)
 *   layout_flex: delegates to flex.rs for display:flex containers
 *
 * Fixed-point arithmetic: uses FIXED_SCALE=64 (6 fractional bits) for
 * sub-pixel precision. Matches CSS spec requirement for sub-pixel layout.
 * /* FIXED_SCALE=64: 6 fractional bits, 1/64 px resolution */
 *
 * Memory: LayoutBox<'a> allocated in bumpalo arena (SilkArena).
 * Children stored in ArenaVec (bump-allocated Vec). Zero heap allocation
 * during layout computation -- all temp storage in arena.
 *
 * TODO(perf): SoA conversion for Dimensions (Phase 4.4)
 * DONE(perf): Fused style-layout-paint pass (Phase 4.5) -- see fused_pipeline.rs
 * DONE(perf): NeighborTable for BFS-level parallel layout (Phase 4.7) -- neighbor_table.rs
 *
 * See: flex.rs for CSS flexbox algorithm
 * See: style.rs ComputedStyle for input style data
 * See: render/lib.rs for display list generation from layout
 */

pub mod flex;
pub mod neighbor_table;

use rustc_hash::FxHashMap;
use silksurf_core::{ArenaVec, SilkArena};
use silksurf_css::{ComputedStyle, Display, Edges, Length};
use silksurf_dom::{Dom, NodeId, NodeKind, TagName};
use std::cell::Cell;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EdgeSizes {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct EdgeSizesFixed {
    top: i32,
    right: i32,
    bottom: i32,
    left: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Dimensions {
    pub content: Rect,
    pub padding: EdgeSizes,
    pub border: EdgeSizes,
    pub margin: EdgeSizes,
}

impl Dimensions {
    pub fn margin_box(&self) -> Rect {
        Rect {
            x: self.content.x - self.margin.left,
            y: self.content.y - self.margin.top,
            width: self.content.width + self.margin.left + self.margin.right,
            height: self.content.height + self.margin.top + self.margin.bottom,
        }
    }
}

const FIXED_SCALE: i32 = 64;

pub(crate) fn fixed_from_f32(value: f32) -> i32 {
    (value * FIXED_SCALE as f32).round() as i32
}

pub(crate) fn fixed_to_f32(value: i32) -> f32 {
    value as f32 / FIXED_SCALE as f32
}

#[derive(Debug)]
pub struct LayoutBox<'a> {
    pub box_type: BoxType,
    dimensions: Cell<Dimensions>,
    pub children: ArenaVec<'a, &'a LayoutBox<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxType {
    BlockNode(NodeId),
    InlineNode(NodeId),
    Anonymous,
}

#[derive(Debug)]
pub struct LayoutTree<'a> {
    pub root: &'a LayoutBox<'a>,
}

pub fn build_layout_tree<'a>(
    arena: &'a SilkArena,
    dom: &Dom,
    styles: &FxHashMap<NodeId, ComputedStyle>,
    root: NodeId,
    viewport: Rect,
) -> Option<LayoutTree<'a>> {
    let root_box = build_layout_box(arena, dom, styles, root)?;
    let mut cursor_y = viewport.y;
    root_box.layout(dom, styles, viewport, &mut cursor_y);
    Some(LayoutTree { root: root_box })
}

pub fn build_layout_tree_incremental<'a>(
    arena: &'a SilkArena,
    dom: &Dom,
    styles: &FxHashMap<NodeId, ComputedStyle>,
    root: NodeId,
    viewport: Rect,
    dirty_nodes: &[NodeId],
) -> Option<LayoutTree<'a>> {
    if dirty_nodes.is_empty() {
        return build_layout_tree(arena, dom, styles, root, viewport);
    }
    build_layout_tree(arena, dom, styles, root, viewport)
}

fn build_layout_box<'a>(
    arena: &'a SilkArena,
    dom: &Dom,
    styles: &FxHashMap<NodeId, ComputedStyle>,
    node: NodeId,
) -> Option<&'a LayoutBox<'a>> {
    let display = match dom.node(node).ok().map(|node| node.kind()) {
        Some(NodeKind::Document) => Display::Block,
        Some(NodeKind::Text { .. }) => Display::Inline,
        Some(NodeKind::Element { name, .. }) => {
            let display = styles.get(&node)?.display;
            if matches!(name, TagName::Html | TagName::Body) && display == Display::Inline {
                Display::Block
            } else {
                display
            }
        }
        Some(NodeKind::Doctype { .. } | NodeKind::Comment { .. }) => Display::None,
        None => return None,
    };
    if display == Display::None {
        return None;
    }
    let box_type = match display {
        Display::Block | Display::Flex | Display::InlineFlex => BoxType::BlockNode(node),
        Display::Inline => BoxType::InlineNode(node),
        Display::None => BoxType::Anonymous,
    };
    let mut children = arena.vec();
    if let Ok(node_children) = dom.children(node) {
        for child in node_children {
            if let Some(child_box) = build_layout_box(arena, dom, styles, *child) {
                children.push(child_box);
            }
        }
    }
    Some(arena.alloc(LayoutBox {
        box_type,
        dimensions: Cell::new(Dimensions::default()),
        children,
    }))
}
impl<'a> LayoutBox<'a> {
    pub fn dimensions(&self) -> Dimensions {
        self.dimensions.get()
    }

    fn set_dimensions(&self, dimensions: Dimensions) {
        self.dimensions.set(dimensions);
    }

    pub fn layout(
        &self,
        dom: &Dom,
        styles: &FxHashMap<NodeId, ComputedStyle>,
        containing: Rect,
        cursor_y: &mut f32,
    ) {
        match self.box_type {
            BoxType::BlockNode(node) => {
                let is_flex = styles
                    .get(&node)
                    .is_some_and(|s| matches!(s.display, Display::Flex | Display::InlineFlex));
                if is_flex {
                    self.layout_flex(dom, styles, node, containing, cursor_y);
                } else {
                    self.layout_block(dom, styles, containing, cursor_y);
                }
            }
            BoxType::InlineNode(_) => self.layout_inline(dom, styles, containing, cursor_y),
            BoxType::Anonymous => {}
        }
    }

    fn layout_flex(
        &self,
        dom: &Dom,
        styles: &FxHashMap<NodeId, ComputedStyle>,
        node: NodeId,
        containing: Rect,
        cursor_y: &mut f32,
    ) {
        let style = self.style_for(styles);
        let mut dims = self.dimensions();
        let margin_fixed = edges_to_fixed(&style.margin);
        let padding_fixed = edges_to_fixed(&style.padding);
        let border_fixed = edges_to_fixed(&style.border);
        dims.margin = edges_from_fixed(margin_fixed);
        dims.padding = edges_from_fixed(padding_fixed);
        dims.border = edges_from_fixed(border_fixed);

        let containing_width_fixed = fixed_from_f32(containing.width);
        let content_width_fixed = (containing_width_fixed
            - margin_fixed.left
            - margin_fixed.right
            - border_fixed.left
            - border_fixed.right
            - padding_fixed.left
            - padding_fixed.right)
            .max(0);
        let x = fixed_to_f32(
            fixed_from_f32(containing.x)
                + margin_fixed.left
                + border_fixed.left
                + padding_fixed.left,
        );
        let y = fixed_to_f32(
            fixed_from_f32(*cursor_y) + margin_fixed.top + border_fixed.top + padding_fixed.top,
        );
        let content_width = fixed_to_f32(content_width_fixed);

        dims.content = Rect {
            x,
            y,
            width: content_width,
            height: 0.0,
        };

        // Run the flex layout algorithm
        let flex_containing = Rect {
            x,
            y,
            width: content_width,
            height: 0.0, // auto height
        };
        let flex_results = flex::layout_flex_container(dom, styles, node, flex_containing);

        // Position children using flex results and compute content height
        let mut max_bottom: f32 = y;
        for child in &self.children {
            let child_node = match child.box_type {
                BoxType::BlockNode(n) | BoxType::InlineNode(n) => n,
                BoxType::Anonymous => continue,
            };
            if let Some(result) = flex_results.get(&child_node) {
                let mut child_dims = child.dimensions();
                child_dims.content = result.content;
                child.set_dimensions(child_dims);
                let bottom = result.content.y + result.content.height;
                if bottom > max_bottom {
                    max_bottom = bottom;
                }
                // Recursively layout flex item children
                let child_style = styles.get(&child_node);
                let is_child_flex = child_style
                    .is_some_and(|s| matches!(s.display, Display::Flex | Display::InlineFlex));
                if is_child_flex {
                    let mut child_cursor = result.content.y;
                    child.layout_flex(dom, styles, child_node, result.content, &mut child_cursor);
                }
            }
        }

        let content_height = (max_bottom - y).max(0.0);
        dims.content.height = content_height;
        self.set_dimensions(dims);

        *cursor_y = y
            + content_height
            + fixed_to_f32(padding_fixed.bottom + border_fixed.bottom + margin_fixed.bottom);
    }

    fn layout_block(
        &self,
        dom: &Dom,
        styles: &FxHashMap<NodeId, ComputedStyle>,
        containing: Rect,
        cursor_y: &mut f32,
    ) {
        let style = self.style_for(styles);
        let mut dims = self.dimensions();
        let margin_fixed = edges_to_fixed(&style.margin);
        let padding_fixed = edges_to_fixed(&style.padding);
        let border_fixed = edges_to_fixed(&style.border);
        dims.margin = edges_from_fixed(margin_fixed);
        dims.padding = edges_from_fixed(padding_fixed);
        dims.border = edges_from_fixed(border_fixed);

        let containing_width_fixed = fixed_from_f32(containing.width);
        let content_width_fixed = (containing_width_fixed
            - margin_fixed.left
            - margin_fixed.right
            - border_fixed.left
            - border_fixed.right
            - padding_fixed.left
            - padding_fixed.right)
            .max(0);
        let x_fixed = fixed_from_f32(containing.x)
            + margin_fixed.left
            + border_fixed.left
            + padding_fixed.left;
        let y_fixed =
            fixed_from_f32(*cursor_y) + margin_fixed.top + border_fixed.top + padding_fixed.top;
        let content_width = fixed_to_f32(content_width_fixed);
        let x = fixed_to_f32(x_fixed);
        let y = fixed_to_f32(y_fixed);
        dims.content = Rect {
            x,
            y,
            width: content_width,
            height: 0.0,
        };

        let mut line_x = x_fixed;
        let mut line_y = y_fixed;
        let mut line_height = 0;
        for child in &self.children {
            match child.box_type {
                BoxType::InlineNode(_) => {
                    let (width, height) = child.inline_intrinsic_size(dom, styles);
                    let width_fixed = fixed_from_f32(width);
                    let height_fixed = fixed_from_f32(height);
                    if line_x + width_fixed > x_fixed + content_width_fixed && line_x > x_fixed {
                        line_y += line_height;
                        line_x = x_fixed;
                        line_height = 0;
                    }
                    let mut child_dims = child.dimensions();
                    child_dims.content = Rect {
                        x: fixed_to_f32(line_x),
                        y: fixed_to_f32(line_y),
                        width,
                        height,
                    };
                    child.set_dimensions(child_dims);
                    line_x += width_fixed;
                    line_height = line_height.max(height_fixed);
                }
                BoxType::BlockNode(_) => {
                    if line_x > x_fixed {
                        line_y += line_height;
                        line_x = x_fixed;
                        line_height = 0;
                    }
                    let mut block_cursor = fixed_to_f32(line_y);
                    child.layout(dom, styles, dims.content, &mut block_cursor);
                    line_y = fixed_from_f32(block_cursor);
                }
                BoxType::Anonymous => {}
            }
        }

        let mut content_height_fixed = if line_x > x_fixed {
            line_y + line_height - y_fixed
        } else {
            line_y - y_fixed
        };
        if self.children.is_empty() {
            content_height_fixed = length_to_fixed(style.line_height);
        }
        dims.content.height = fixed_to_f32(content_height_fixed);
        self.set_dimensions(dims);

        *cursor_y = fixed_to_f32(
            y_fixed
                + content_height_fixed
                + padding_fixed.bottom
                + border_fixed.bottom
                + margin_fixed.bottom,
        );
    }

    fn layout_inline(
        &self,
        dom: &Dom,
        styles: &FxHashMap<NodeId, ComputedStyle>,
        containing: Rect,
        cursor_y: &mut f32,
    ) {
        let style = self.style_for(styles);
        let mut dims = self.dimensions();
        let margin_fixed = edges_to_fixed(&style.margin);
        let padding_fixed = edges_to_fixed(&style.padding);
        let border_fixed = edges_to_fixed(&style.border);
        dims.margin = edges_from_fixed(margin_fixed);
        dims.padding = edges_from_fixed(padding_fixed);
        dims.border = edges_from_fixed(border_fixed);

        let (width, height) = self.inline_intrinsic_size(dom, styles);
        let x_fixed = fixed_from_f32(containing.x)
            + margin_fixed.left
            + border_fixed.left
            + padding_fixed.left;
        let y_fixed =
            fixed_from_f32(*cursor_y) + margin_fixed.top + border_fixed.top + padding_fixed.top;
        let width_fixed = fixed_from_f32(width);
        let height_fixed = fixed_from_f32(height);
        dims.content = Rect {
            x: fixed_to_f32(x_fixed),
            y: fixed_to_f32(y_fixed),
            width: fixed_to_f32(width_fixed),
            height: fixed_to_f32(height_fixed),
        };
        self.set_dimensions(dims);

        *cursor_y = fixed_to_f32(
            y_fixed
                + height_fixed
                + padding_fixed.bottom
                + border_fixed.bottom
                + margin_fixed.bottom,
        );
    }

    fn inline_intrinsic_size(
        &self,
        dom: &Dom,
        styles: &FxHashMap<NodeId, ComputedStyle>,
    ) -> (f32, f32) {
        let style = self.style_for(styles);
        let font_size_fixed = length_to_fixed(style.font_size);
        let line_height_fixed = length_to_fixed(style.line_height);
        let width_fixed = match self.box_type {
            BoxType::InlineNode(node) => inline_text_width_fixed(dom, node, font_size_fixed),
            _ => 0,
        };
        (fixed_to_f32(width_fixed), fixed_to_f32(line_height_fixed))
    }

    fn style_for<'style>(
        &self,
        styles: &'style FxHashMap<NodeId, ComputedStyle>,
    ) -> &'style ComputedStyle {
        match self.box_type {
            BoxType::BlockNode(node) | BoxType::InlineNode(node) => {
                // UNWRAP-OK: the cascade pass always produces a ComputedStyle for every node
                // in the layout tree before this is called; missing entries are a pipeline bug.
                styles.get(&node).expect("style missing for node")
            }
            // UNWRAP-OK: anonymous boxes are only created when at least one styled box exists,
            // so styles is non-empty.
            BoxType::Anonymous => styles.values().next().unwrap(),
        }
    }
}

pub(crate) fn length_to_px(length: Length) -> f32 {
    match length {
        Length::Px(value) => value,
        Length::Percent(_) => 0.0, // Needs context; use length_to_px_with_context for percentages
    }
}

fn length_to_fixed(length: Length) -> i32 {
    fixed_from_f32(length_to_px(length))
}

fn edges_to_fixed(edges: &Edges) -> EdgeSizesFixed {
    EdgeSizesFixed {
        top: length_to_fixed(edges.top),
        right: length_to_fixed(edges.right),
        bottom: length_to_fixed(edges.bottom),
        left: length_to_fixed(edges.left),
    }
}

fn edges_from_fixed(edges: EdgeSizesFixed) -> EdgeSizes {
    EdgeSizes {
        top: fixed_to_f32(edges.top),
        right: fixed_to_f32(edges.right),
        bottom: fixed_to_f32(edges.bottom),
        left: fixed_to_f32(edges.left),
    }
}

fn inline_text_width_fixed(dom: &Dom, node: NodeId, font_size_fixed: i32) -> i32 {
    let text = match dom.node(node).ok().map(|node| node.kind()) {
        Some(NodeKind::Text { text }) => text,
        _ => return font_size_fixed,
    };
    let count = collapsed_char_count(text);
    (count as i32 * font_size_fixed) / 2
}

fn collapsed_char_count(text: &str) -> usize {
    let mut count = 0usize;
    let mut in_whitespace = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !in_whitespace {
                count += 1;
                in_whitespace = true;
            }
        } else {
            count += 1;
            in_whitespace = false;
        }
    }
    count
}
