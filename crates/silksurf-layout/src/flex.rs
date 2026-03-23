//! Flexbox layout algorithm per CSS Flexible Box Layout Module Level 1.
//!
//! Implements the core flex layout:
//! 1. Determine main/cross axes from flex-direction
//! 2. Collect flex items, compute flex-basis
//! 3. Distribute free space per flex-grow/flex-shrink
//! 4. Line wrapping (flex-wrap)
//! 5. Cross-axis alignment (align-items, align-self)
//! 6. Main-axis alignment (justify-content)

use silksurf_css::{
    AlignItems, AlignSelf, ComputedStyle, Display, FlexBasis, FlexDirection, FlexWrap,
    JustifyContent, Length,
};
use silksurf_dom::{Dom, NodeId, NodeKind};

use crate::{Rect, length_to_px};
use rustc_hash::FxHashMap;

/// A resolved flex item for layout computation.
struct FlexItem {
    node: NodeId,
    /// Base size before flex-grow/shrink
    flex_basis: f32,
    flex_grow: f32,
    flex_shrink: f32,
    /// Margin/padding/border in main axis
    main_margin_start: f32,
    main_margin_end: f32,
    /// Margin/padding/border in cross axis
    cross_margin_start: f32,
    cross_margin_end: f32,
    /// Intrinsic content size (fallback when basis is auto)
    _content_width: f32,
    _content_height: f32,
    /// Resolved main/cross sizes after flex distribution
    main_size: f32,
    cross_size: f32,
    /// Final position
    x: f32,
    y: f32,
    order: i32,
    align_self: AlignSelf,
}

/// A flex line (for wrapping).
struct FlexLine {
    items: Vec<usize>, // indices into FlexItem vec
}

/// Compute flexbox layout for a flex container.
///
/// Returns a map of NodeId -> (Rect content area, EdgeSizes margin/padding/border).
pub fn layout_flex_container(
    dom: &Dom,
    styles: &FxHashMap<NodeId, ComputedStyle>,
    container_node: NodeId,
    containing: Rect,
) -> FxHashMap<NodeId, FlexLayoutResult> {
    let mut results = FxHashMap::default();

    let container_style = match styles.get(&container_node) {
        Some(s) => s,
        None => return results,
    };

    let flex = &container_style.flex_container;
    let is_row = matches!(
        flex.direction,
        FlexDirection::Row | FlexDirection::RowReverse
    );
    let is_reversed = matches!(
        flex.direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );

    // Available space in main axis
    let container_main = if is_row {
        containing.width
    } else {
        containing.height
    };
    let container_cross = if is_row {
        containing.height
    } else {
        containing.width
    };

    // Collect flex items from children
    let children = match dom.children(container_node) {
        Ok(c) => c.to_vec(),
        Err(_) => return results,
    };

    let mut items: Vec<FlexItem> = Vec::with_capacity(children.len());
    for child in &children {
        let child_style = match styles.get(child) {
            Some(s) => s,
            None => continue,
        };
        if child_style.display == Display::None {
            continue;
        }

        let margin = (
            length_to_px(child_style.margin.top),
            length_to_px(child_style.margin.right),
            length_to_px(child_style.margin.bottom),
            length_to_px(child_style.margin.left),
        );
        let padding = (
            length_to_px(child_style.padding.top),
            length_to_px(child_style.padding.right),
            length_to_px(child_style.padding.bottom),
            length_to_px(child_style.padding.left),
        );
        let border = (
            length_to_px(child_style.border.top),
            length_to_px(child_style.border.right),
            length_to_px(child_style.border.bottom),
            length_to_px(child_style.border.left),
        );

        let (main_margin_start, main_margin_end, cross_margin_start, cross_margin_end) = if is_row {
            (
                margin.3 + padding.3 + border.3,
                margin.1 + padding.1 + border.1,
                margin.0 + padding.0 + border.0,
                margin.2 + padding.2 + border.2,
            )
        } else {
            (
                margin.0 + padding.0 + border.0,
                margin.2 + padding.2 + border.2,
                margin.3 + padding.3 + border.3,
                margin.1 + padding.1 + border.1,
            )
        };

        // Determine flex-basis
        let fi = &child_style.flex_item;
        let content_width = intrinsic_width(dom, *child, child_style);
        let content_height = length_to_px(child_style.line_height);

        let flex_basis = match fi.flex_basis {
            FlexBasis::Length(Length::Px(px)) => px,
            FlexBasis::Length(Length::Percent(pct)) => {
                // Resolve percentage against container main size
                if is_row {
                    containing.width * pct / 100.0
                } else {
                    containing.height * pct / 100.0
                }
            }
            FlexBasis::Auto => {
                if is_row {
                    content_width
                } else {
                    content_height
                }
            }
        };

        items.push(FlexItem {
            node: *child,
            flex_basis,
            flex_grow: fi.flex_grow,
            flex_shrink: fi.flex_shrink,
            main_margin_start,
            main_margin_end,
            cross_margin_start,
            cross_margin_end,
            _content_width: content_width,
            _content_height: content_height,
            main_size: flex_basis,
            cross_size: if is_row {
                content_height
            } else {
                content_width
            },
            x: 0.0,
            y: 0.0,
            order: fi.order,
            align_self: fi.align_self,
        });
    }

    // Sort by order property (stable sort preserves source order for equal values)
    items.sort_by_key(|item| item.order);

    // Determine gap
    let main_gap = if is_row {
        flex.column_gap.max(flex.gap)
    } else {
        flex.row_gap.max(flex.gap)
    };
    let cross_gap = if is_row {
        flex.row_gap.max(flex.gap)
    } else {
        flex.column_gap.max(flex.gap)
    };

    // Build flex lines (wrap or single line)
    let lines = build_flex_lines(&items, container_main, main_gap, flex.wrap);

    // Resolve main axis sizes per line (flex-grow/shrink distribution)
    for line in &lines {
        resolve_main_sizes(&mut items, line, container_main, main_gap);
    }

    // Resolve cross axis sizes per line
    let mut line_cross_sizes: Vec<f32> = lines
        .iter()
        .map(|line| {
            line.items
                .iter()
                .map(|&idx| {
                    items[idx].cross_size
                        + items[idx].cross_margin_start
                        + items[idx].cross_margin_end
                })
                .fold(0.0f32, f32::max)
        })
        .collect();

    // If single line and container has definite cross size, use it
    if lines.len() == 1 && container_cross > 0.0 {
        line_cross_sizes[0] = container_cross;
    }

    // Position items
    let mut cross_offset = if is_row { containing.y } else { containing.x };

    for (line_idx, line) in lines.iter().enumerate() {
        let line_cross = line_cross_sizes[line_idx];

        // Main axis alignment (justify-content)
        let total_main: f32 = line
            .items
            .iter()
            .map(|&idx| {
                items[idx].main_size + items[idx].main_margin_start + items[idx].main_margin_end
            })
            .sum();
        let total_gaps = if line.items.len() > 1 {
            (line.items.len() - 1) as f32 * main_gap
        } else {
            0.0
        };
        let free_space = (container_main - total_main - total_gaps).max(0.0);

        let (mut main_offset, gap_extra) =
            justify_content_offsets(flex.justify_content, free_space, line.items.len());
        main_offset += if is_row { containing.x } else { containing.y };

        let line_items: Vec<usize> = if is_reversed {
            line.items.iter().rev().copied().collect()
        } else {
            line.items.clone()
        };

        for (i, &idx) in line_items.iter().enumerate() {
            let item = &mut items[idx];
            let item_main_start = main_offset + item.main_margin_start;

            // Cross axis alignment
            let effective_align = match item.align_self {
                AlignSelf::Auto => flex.align_items,
                AlignSelf::FlexStart => AlignItems::FlexStart,
                AlignSelf::FlexEnd => AlignItems::FlexEnd,
                AlignSelf::Center => AlignItems::Center,
                AlignSelf::Stretch => AlignItems::Stretch,
                AlignSelf::Baseline => AlignItems::Baseline,
            };
            let item_cross_total =
                item.cross_size + item.cross_margin_start + item.cross_margin_end;
            let cross_free = (line_cross - item_cross_total).max(0.0);
            let item_cross_start = match effective_align {
                AlignItems::FlexStart | AlignItems::Baseline => {
                    cross_offset + item.cross_margin_start
                }
                AlignItems::FlexEnd => cross_offset + cross_free + item.cross_margin_start,
                AlignItems::Center => cross_offset + cross_free / 2.0 + item.cross_margin_start,
                AlignItems::Stretch => {
                    item.cross_size =
                        (line_cross - item.cross_margin_start - item.cross_margin_end).max(0.0);
                    cross_offset + item.cross_margin_start
                }
            };

            if is_row {
                item.x = item_main_start;
                item.y = item_cross_start;
            } else {
                item.x = item_cross_start;
                item.y = item_main_start;
            }

            main_offset = item_main_start + item.main_size + item.main_margin_end;
            if i < line_items.len() - 1 {
                main_offset += main_gap + gap_extra;
            }
        }

        cross_offset += line_cross + cross_gap;
    }

    // Write results
    for item in &items {
        let (width, height) = if is_row {
            (item.main_size, item.cross_size)
        } else {
            (item.cross_size, item.main_size)
        };
        results.insert(
            item.node,
            FlexLayoutResult {
                content: Rect {
                    x: item.x,
                    y: item.y,
                    width,
                    height,
                },
            },
        );
    }

    results
}

/// Result of flex layout for a single item.
#[derive(Debug, Clone, Copy)]
pub struct FlexLayoutResult {
    pub content: Rect,
}

/// Build flex lines from items (handles wrapping).
fn build_flex_lines(
    items: &[FlexItem],
    container_main: f32,
    main_gap: f32,
    wrap: FlexWrap,
) -> Vec<FlexLine> {
    if items.is_empty() {
        return vec![];
    }

    let should_wrap = !matches!(wrap, FlexWrap::Nowrap);
    let mut lines: Vec<FlexLine> = Vec::new();
    let mut current_items: Vec<usize> = Vec::new();
    let mut current_main: f32 = 0.0;

    for (idx, item) in items.iter().enumerate() {
        let item_outer = item.flex_basis + item.main_margin_start + item.main_margin_end;
        let gap = if current_items.is_empty() {
            0.0
        } else {
            main_gap
        };

        if should_wrap
            && !current_items.is_empty()
            && current_main + gap + item_outer > container_main
        {
            lines.push(FlexLine {
                items: std::mem::take(&mut current_items),
            });
            current_main = 0.0;
        }

        if !current_items.is_empty() {
            current_main += main_gap;
        }
        current_main += item_outer;
        current_items.push(idx);
    }

    if !current_items.is_empty() {
        lines.push(FlexLine {
            items: current_items,
        });
    }

    // Handle wrap-reverse
    if matches!(wrap, FlexWrap::WrapReverse) {
        lines.reverse();
    }

    lines
}

/// Distribute free space on the main axis among flex items in a line.
fn resolve_main_sizes(items: &mut [FlexItem], line: &FlexLine, container_main: f32, main_gap: f32) {
    let total_gaps = if line.items.len() > 1 {
        (line.items.len() - 1) as f32 * main_gap
    } else {
        0.0
    };

    let total_basis: f32 = line
        .items
        .iter()
        .map(|&idx| {
            items[idx].flex_basis + items[idx].main_margin_start + items[idx].main_margin_end
        })
        .sum();

    let free_space = container_main - total_basis - total_gaps;

    if free_space > 0.0 {
        // Grow
        let total_grow: f32 = line.items.iter().map(|&idx| items[idx].flex_grow).sum();
        if total_grow > 0.0 {
            for &idx in &line.items {
                let grow_share = (items[idx].flex_grow / total_grow) * free_space;
                items[idx].main_size = items[idx].flex_basis + grow_share;
            }
        } else {
            for &idx in &line.items {
                items[idx].main_size = items[idx].flex_basis;
            }
        }
    } else if free_space < 0.0 {
        // Shrink
        let total_shrink: f32 = line
            .items
            .iter()
            .map(|&idx| items[idx].flex_shrink * items[idx].flex_basis)
            .sum();
        if total_shrink > 0.0 {
            let overflow = -free_space;
            for &idx in &line.items {
                let shrink_ratio = (items[idx].flex_shrink * items[idx].flex_basis) / total_shrink;
                items[idx].main_size = (items[idx].flex_basis - shrink_ratio * overflow).max(0.0);
            }
        } else {
            for &idx in &line.items {
                items[idx].main_size = items[idx].flex_basis;
            }
        }
    } else {
        for &idx in &line.items {
            items[idx].main_size = items[idx].flex_basis;
        }
    }
}

/// Compute main-axis starting offset and per-gap extra space for justify-content.
fn justify_content_offsets(jc: JustifyContent, free_space: f32, item_count: usize) -> (f32, f32) {
    if item_count == 0 || free_space <= 0.0 {
        return (0.0, 0.0);
    }
    match jc {
        JustifyContent::FlexStart => (0.0, 0.0),
        JustifyContent::FlexEnd => (free_space, 0.0),
        JustifyContent::Center => (free_space / 2.0, 0.0),
        JustifyContent::SpaceBetween => {
            if item_count <= 1 {
                (0.0, 0.0)
            } else {
                (0.0, free_space / (item_count - 1) as f32)
            }
        }
        JustifyContent::SpaceAround => {
            let gap = free_space / item_count as f32;
            (gap / 2.0, gap)
        }
        JustifyContent::SpaceEvenly => {
            let gap = free_space / (item_count + 1) as f32;
            (gap, gap)
        }
    }
}

/// Estimate intrinsic width for a node (simplified).
fn intrinsic_width(dom: &Dom, node: NodeId, style: &ComputedStyle) -> f32 {
    let font_size = length_to_px(style.font_size);
    match dom.node(node).ok().map(|n| n.kind()) {
        Some(NodeKind::Text { text }) => {
            // Rough estimate: char count * font_size * 0.6
            let count = text.chars().count();
            count as f32 * font_size * 0.6
        }
        _ => font_size * 2.0, // Minimum content width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_justify_content_offsets() {
        // flex-start: no offset
        assert_eq!(
            justify_content_offsets(JustifyContent::FlexStart, 100.0, 3),
            (0.0, 0.0)
        );
        // flex-end: all offset
        assert_eq!(
            justify_content_offsets(JustifyContent::FlexEnd, 100.0, 3),
            (100.0, 0.0)
        );
        // center: half offset
        assert_eq!(
            justify_content_offsets(JustifyContent::Center, 100.0, 3),
            (50.0, 0.0)
        );
        // space-between: gap = 50 each
        assert_eq!(
            justify_content_offsets(JustifyContent::SpaceBetween, 100.0, 3),
            (0.0, 50.0)
        );
        // space-around: gap = 33.33, start = 16.67
        let (start, gap) = justify_content_offsets(JustifyContent::SpaceAround, 99.0, 3);
        assert!((start - 16.5).abs() < 0.01);
        assert!((gap - 33.0).abs() < 0.01);
        // space-evenly: gap = 25, start = 25
        assert_eq!(
            justify_content_offsets(JustifyContent::SpaceEvenly, 100.0, 3),
            (25.0, 25.0)
        );
    }

    #[test]
    fn test_resolve_main_sizes_grow() {
        let mut items = vec![
            FlexItem {
                node: NodeId::from_raw(0),
                flex_basis: 100.0,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                main_margin_start: 0.0,
                main_margin_end: 0.0,
                cross_margin_start: 0.0,
                cross_margin_end: 0.0,
                _content_width: 100.0,
                _content_height: 20.0,
                main_size: 100.0,
                cross_size: 20.0,
                x: 0.0,
                y: 0.0,
                order: 0,
                align_self: AlignSelf::Auto,
            },
            FlexItem {
                node: NodeId::from_raw(0),
                flex_basis: 100.0,
                flex_grow: 2.0,
                flex_shrink: 1.0,
                main_margin_start: 0.0,
                main_margin_end: 0.0,
                cross_margin_start: 0.0,
                cross_margin_end: 0.0,
                _content_width: 100.0,
                _content_height: 20.0,
                main_size: 100.0,
                cross_size: 20.0,
                x: 0.0,
                y: 0.0,
                order: 0,
                align_self: AlignSelf::Auto,
            },
        ];
        let line = FlexLine { items: vec![0, 1] };
        // Container is 500px, items take 200px, 300px free space
        // Item 0 gets 100 (1/3 of 300), item 1 gets 200 (2/3 of 300)
        resolve_main_sizes(&mut items, &line, 500.0, 0.0);
        assert!((items[0].main_size - 200.0).abs() < 0.01);
        assert!((items[1].main_size - 300.0).abs() < 0.01);
    }

    #[test]
    fn test_resolve_main_sizes_shrink() {
        let mut items = vec![
            FlexItem {
                node: NodeId::from_raw(0),
                flex_basis: 200.0,
                flex_grow: 0.0,
                flex_shrink: 1.0,
                main_margin_start: 0.0,
                main_margin_end: 0.0,
                cross_margin_start: 0.0,
                cross_margin_end: 0.0,
                _content_width: 200.0,
                _content_height: 20.0,
                main_size: 200.0,
                cross_size: 20.0,
                x: 0.0,
                y: 0.0,
                order: 0,
                align_self: AlignSelf::Auto,
            },
            FlexItem {
                node: NodeId::from_raw(0),
                flex_basis: 200.0,
                flex_grow: 0.0,
                flex_shrink: 1.0,
                main_margin_start: 0.0,
                main_margin_end: 0.0,
                cross_margin_start: 0.0,
                cross_margin_end: 0.0,
                _content_width: 200.0,
                _content_height: 20.0,
                main_size: 200.0,
                cross_size: 20.0,
                x: 0.0,
                y: 0.0,
                order: 0,
                align_self: AlignSelf::Auto,
            },
        ];
        let line = FlexLine { items: vec![0, 1] };
        // Container is 300px, items take 400px, -100px free space
        // Both shrink equally: each loses 50px
        resolve_main_sizes(&mut items, &line, 300.0, 0.0);
        assert!((items[0].main_size - 150.0).abs() < 0.01);
        assert!((items[1].main_size - 150.0).abs() < 0.01);
    }
}
