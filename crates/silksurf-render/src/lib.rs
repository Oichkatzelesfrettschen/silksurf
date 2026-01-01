//! Rendering and rasterization pipeline (cleanroom).

use silksurf_css::{Color, ComputedStyle};
use silksurf_dom::{Dom, NodeId, NodeKind};
use silksurf_layout::{LayoutTree, Rect};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayList {
    pub items: Vec<DisplayItem>,
    pub tiles: Option<DisplayListTiles>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayListTiles {
    tile_size: u32,
    tiles_x: u32,
    tiles_y: u32,
    buckets: Vec<Vec<usize>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayItem {
    SolidColor { rect: Rect, color: Color },
    Text {
        rect: Rect,
        node: NodeId,
        text_len: u32,
        color: Color,
    },
}

pub fn build_display_list(
    dom: &Dom,
    styles: &HashMap<NodeId, ComputedStyle>,
    layout: &LayoutTree<'_>,
) -> DisplayList {
    let capacity = estimate_display_items(&layout.root);
    let mut list = DisplayList {
        items: Vec::with_capacity(capacity),
        tiles: None,
    };
    build_display_list_for_box(dom, styles, &layout.root, &mut list);
    list
}

impl DisplayList {
    pub fn with_tiles(mut self, width: u32, height: u32, tile_size: u32) -> Self {
        if width == 0 || height == 0 || tile_size == 0 {
            return self;
        }
        self.tiles = Some(build_tiles(&self.items, width, height, tile_size));
        self
    }
}

fn build_display_list_for_box(
    dom: &Dom,
    styles: &HashMap<NodeId, ComputedStyle>,
    layout: &silksurf_layout::LayoutBox<'_>,
    list: &mut DisplayList,
) {
    match layout.box_type {
        silksurf_layout::BoxType::BlockNode(node_id)
        | silksurf_layout::BoxType::InlineNode(node_id) => {
            if let Some(style) = styles.get(&node_id) {
                if style.background_color.a > 0 {
                    list.items.push(DisplayItem::SolidColor {
                        rect: layout.dimensions().content,
                        color: style.background_color,
                    });
                }
                if let Ok(node) = dom.node(node_id) {
                    if let NodeKind::Text { .. } = node.kind() {
                        let text_len = match node.kind() {
                            NodeKind::Text { text } => text.len() as u32,
                            _ => 0,
                        };
                        list.items.push(DisplayItem::Text {
                            rect: layout.dimensions().content,
                            node: node_id,
                            text_len,
                            color: style.color,
                        });
                    }
                }
            }
        }
        silksurf_layout::BoxType::Anonymous => {}
    }

    for child in &layout.children {
        build_display_list_for_box(dom, styles, child, list);
    }
}

pub fn rasterize(display_list: &DisplayList, width: u32, height: u32) -> Vec<u8> {
    let damage = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    rasterize_damage(display_list, width, height, damage)
}

pub fn rasterize_damage(
    display_list: &DisplayList,
    width: u32,
    height: u32,
    damage: Rect,
) -> Vec<u8> {
    let mut buffer = vec![255; (width * height * 4) as usize];
    let item_indices = if let Some(tiles) = &display_list.tiles {
        tiles.items_for_rect(damage)
    } else {
        (0..display_list.items.len()).collect()
    };
    let mut seen = vec![false; display_list.items.len()];
    for index in item_indices {
        if index >= display_list.items.len() || seen[index] {
            continue;
        }
        seen[index] = true;
        let item = &display_list.items[index];
        let rect = item_rect(item);
        if !rect_intersects(rect, damage) {
            continue;
        }
        match item {
            DisplayItem::SolidColor { rect, color } => {
                fill_rect(&mut buffer, width, height, *rect, *color);
            }
            DisplayItem::Text { rect, color, .. } => {
                fill_rect(&mut buffer, width, height, *rect, *color);
            }
        }
    }
    buffer
}

fn item_rect(item: &DisplayItem) -> Rect {
    match item {
        DisplayItem::SolidColor { rect, .. } => *rect,
        DisplayItem::Text { rect, .. } => *rect,
    }
}

fn rect_intersects(a: Rect, b: Rect) -> bool {
    let ax1 = a.x + a.width;
    let ay1 = a.y + a.height;
    let bx1 = b.x + b.width;
    let by1 = b.y + b.height;
    a.x < bx1 && ax1 > b.x && a.y < by1 && ay1 > b.y
}

fn build_tiles(
    items: &[DisplayItem],
    width: u32,
    height: u32,
    tile_size: u32,
) -> DisplayListTiles {
    let tiles_x = (width + tile_size - 1) / tile_size;
    let tiles_y = (height + tile_size - 1) / tile_size;
    let mut buckets = vec![Vec::new(); (tiles_x * tiles_y) as usize];
    for (index, item) in items.iter().enumerate() {
        let rect = item_rect(item);
        let x0 = rect.x.max(0.0).floor() as i32;
        let y0 = rect.y.max(0.0).floor() as i32;
        let x1 = (rect.x + rect.width).min(width as f32).ceil() as i32;
        let y1 = (rect.y + rect.height).min(height as f32).ceil() as i32;
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        let tx0 = (x0.max(0) as u32) / tile_size;
        let ty0 = (y0.max(0) as u32) / tile_size;
        let tx1 = ((x1.max(1) as u32).saturating_sub(1)) / tile_size;
        let ty1 = ((y1.max(1) as u32).saturating_sub(1)) / tile_size;
        for ty in ty0..=ty1.min(tiles_y.saturating_sub(1)) {
            for tx in tx0..=tx1.min(tiles_x.saturating_sub(1)) {
                let tile_index = (ty * tiles_x + tx) as usize;
                if let Some(bucket) = buckets.get_mut(tile_index) {
                    bucket.push(index);
                }
            }
        }
    }
    DisplayListTiles {
        tile_size,
        tiles_x,
        tiles_y,
        buckets,
    }
}

impl DisplayListTiles {
    fn items_for_rect(&self, rect: Rect) -> Vec<usize> {
        let x0 = rect.x.max(0.0).floor() as i32;
        let y0 = rect.y.max(0.0).floor() as i32;
        let x1 = (rect.x + rect.width).max(0.0).ceil() as i32;
        let y1 = (rect.y + rect.height).max(0.0).ceil() as i32;
        let tx0 = (x0.max(0) as u32) / self.tile_size;
        let ty0 = (y0.max(0) as u32) / self.tile_size;
        let tx1 = ((x1.max(1) as u32).saturating_sub(1)) / self.tile_size;
        let ty1 = ((y1.max(1) as u32).saturating_sub(1)) / self.tile_size;
        let mut items = Vec::new();
        for ty in ty0..=ty1.min(self.tiles_y.saturating_sub(1)) {
            for tx in tx0..=tx1.min(self.tiles_x.saturating_sub(1)) {
                let tile_index = (ty * self.tiles_x + tx) as usize;
                if let Some(bucket) = self.buckets.get(tile_index) {
                    items.extend(bucket.iter().copied());
                }
            }
        }
        items
    }
}

fn estimate_display_items(layout: &silksurf_layout::LayoutBox<'_>) -> usize {
    let mut count = 1;
    for child in &layout.children {
        count += estimate_display_items(child);
    }
    count
}
fn fill_rect(buffer: &mut [u8], width: u32, height: u32, rect: Rect, color: Color) {
    let x0 = rect.x.max(0.0).floor() as i32;
    let y0 = rect.y.max(0.0).floor() as i32;
    let x1 = (rect.x + rect.width).min(width as f32).ceil() as i32;
    let y1 = (rect.y + rect.height).min(height as f32).ceil() as i32;

    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let width_u = width as usize;
    let pixel = u32::from_le_bytes([color.r, color.g, color.b, color.a]);
    let len_u32 = buffer.len() / 4;
    let buffer_u32 = unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u32, len_u32) };

    for y in y0..y1 {
        if y < 0 || y >= height as i32 {
            continue;
        }
        let row_start = y as usize * width_u + x0.max(0) as usize;
        let row_end = y as usize * width_u + x1.min(width as i32) as usize;
        if row_start >= row_end || row_end > buffer_u32.len() {
            continue;
        }
        fill_row_u32(&mut buffer_u32[row_start..row_end], pixel);
    }
}

fn fill_row_u32(row: &mut [u32], pixel: u32) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("sse2") {
            unsafe {
                fill_row_sse2(row, pixel);
            }
            return;
        }
    }
    row.fill(pixel);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
unsafe fn fill_row_sse2(row: &mut [u32], pixel: u32) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let len = row.len();
    if len == 0 {
        return;
    }
    let mut idx = 0usize;
    let ptr = row.as_mut_ptr();
    let value = unsafe { _mm_set1_epi32(pixel as i32) };
    while idx + 4 <= len {
        let dst = unsafe { ptr.add(idx) } as *mut __m128i;
        unsafe {
            _mm_storeu_si128(dst, value);
        }
        idx += 4;
    }
    while idx < len {
        unsafe {
            *ptr.add(idx) = pixel;
        }
        idx += 1;
    }
}
