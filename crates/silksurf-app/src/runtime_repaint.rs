// Module split from the former single-file binary; the crate root
// re-exports every module so sibling items resolve by bare name.
#[allow(clippy::wildcard_imports)]
use crate::*;

pub(crate) fn drain_initial_host_callbacks(js_ctx: &mut SilkContext) {
    if !js_ctx.has_pending_host_callbacks() {
        return;
    }

    // Initial-script fetches complete on worker threads; one-shot embedders
    // (headless render, first paint) pump briefly so those promises settle
    // before the page is declared built. Timers do NOT extend this window --
    // only in-flight network holds it open, bounded at 2 seconds.
    let settle_deadline = std::time::Instant::now() + std::time::Duration::from_millis(2000);
    let mut total = 0_usize;
    loop {
        match js_ctx.run_host_callbacks(64) {
            Ok(count) => total += count,
            Err(err) => {
                eprintln!("[SilkSurf] Initial host callback error: {err}");
                return;
            }
        }
        if js_ctx.inflight_network_requests() == 0 || std::time::Instant::now() >= settle_deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    if total > 0 {
        eprintln!("[SilkSurf] Initial host callbacks: {total}");
    }
}

pub(crate) fn tick_browser_runtime(state: &mut BrowserState) -> bool {
    let Some(mut runtime) = state.runtime.take() else {
        return false;
    };

    let redraw_mode = match repaint_runtime_host_callbacks(&mut runtime, &mut state.frame) {
        Ok(redraw_mode) => redraw_mode,
        Err(err) => {
            eprintln!("[SilkSurf] Runtime callback error: {err}");
            set_browser_status(state, "error");
            mark_redraw(state, BrowserRedrawMode::Chrome);
            state.runtime = Some(runtime);
            return true;
        }
    };

    // Same-document navigations from history.pushState/replaceState: record
    // them in session history and reflect the address bar, no reload.
    let intents = runtime.js_ctx.take_history_intents();
    let mut chrome_changed = false;
    for intent in intents {
        let resolved = url::Url::parse(&state.frame.url)
            .ok()
            .and_then(|base| base.join(&intent.url).ok())
            .map_or_else(|| intent.url.clone(), |joined| joined.to_string());
        if intent.replace {
            if let Some(entry) = state.history.get_mut(state.history_index) {
                entry.clone_from(&resolved);
            }
        } else {
            state.history.truncate(state.history_index + 1);
            state.history.push(resolved.clone());
            state.history_index = state.history.len() - 1;
        }
        state.frame.url.clone_from(&resolved);
        state.address_text = resolved;
        chrome_changed = true;
    }

    // localStorage writeback: flush dirtied entries to the origin store.
    if let Some(entries) = runtime.js_ctx.take_local_storage_if_dirty() {
        crate::profile::flush_local_storage(&state.frame.url, &entries);
    }

    state.runtime = Some(runtime);
    if chrome_changed {
        mark_redraw(state, BrowserRedrawMode::AddressChrome);
    }
    if let Some(redraw_mode) = redraw_mode {
        mark_redraw(state, redraw_mode);
        return true;
    }
    chrome_changed
}

pub(crate) fn repaint_runtime_host_callbacks(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
) -> Result<Option<BrowserRedrawMode>, String> {
    if !runtime.js_ctx.has_pending_host_callbacks() {
        return Ok(None);
    }

    let callback_count = runtime.js_ctx.run_host_callbacks(64)?;
    if callback_count == 0 {
        return Ok(None);
    }

    let dirty_nodes = {
        let mut dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        dom.take_dirty_nodes()
    };
    if dirty_nodes.is_empty() {
        return Ok(None);
    }

    let redraw_mode = repaint_runtime_dirty_nodes(runtime, frame, &dirty_nodes);
    eprintln!("[SilkSurf] Runtime host callbacks: {callback_count}");
    Ok(redraw_mode)
}

pub(crate) fn repaint_runtime_dirty_nodes(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
    dirty_nodes: &[silksurf_dom::NodeId],
) -> Option<BrowserRedrawMode> {
    if dirty_nodes.is_empty() {
        return None;
    }

    if let Some(redraw_mode) = repaint_runtime_text_only_dirty_nodes(runtime, frame, dirty_nodes) {
        return Some(redraw_mode);
    }

    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let replaced_sizes =
        collect_image_replaced_sizes(&dom, runtime.document, &frame.url, &runtime.images);
    runtime.fused_workspace.run_with_replaced_sizes(
        &dom,
        &runtime.stylesheet,
        &runtime.style_index,
        runtime.document,
        runtime.viewport,
        &replaced_sizes,
    );
    let mut new_fused = runtime.fused_workspace.take_result();
    let mut display_list = silksurf_render::DisplayList {
        items: std::mem::take(&mut new_fused.display_items),
        tiles: None,
    };
    append_image_display_items(
        &dom,
        &new_fused,
        &frame.url,
        &runtime.images,
        &mut display_list.items,
    );
    frame.link_targets = collect_link_targets(&dom, &display_list.items, &frame.url);
    frame.input_targets = collect_input_targets(&dom, &new_fused);
    let damage = dirty_nodes_damage_rect(&dom, dirty_nodes, &runtime.fused, &new_fused);
    drop(dom);

    let next_height = browser_frame_height(&display_list.items, BROWSER_CHROME_HEIGHT as u32);
    display_list = tile_browser_document_display_list(display_list, next_height);
    frame.raster_height = next_height;
    let redraw_mode = if let Some(damage) = damage {
        rasterize_browser_document_damage_into(
            &display_list,
            frame.bitmap_scroll_y,
            frame.bitmap_height,
            damage,
            &mut runtime.rgba,
            &mut runtime.damage_scratch,
        );
        if !sync_argb_damage_from_scratch(&runtime.damage_scratch, &mut frame.argb, FRAME_WIDTH) {
            sync_argb_damage_from_rgba(
                &runtime.rgba,
                &mut frame.argb,
                FRAME_WIDTH,
                frame.bitmap_height,
                viewport_damage_rect(damage, frame.bitmap_scroll_y),
            );
        }
        BrowserRedrawMode::Damage(damage)
    } else {
        rasterize_browser_viewport_argb_preferred(
            &display_list,
            frame.bitmap_scroll_y,
            frame.bitmap_height,
            &mut runtime.rgba,
            &mut frame.argb,
            &mut runtime.viewport_item_indices,
        );
        BrowserRedrawMode::Full
    };
    runtime.display_list = display_list;

    let old_fused = std::mem::replace(&mut runtime.fused, new_fused);
    runtime.fused_workspace.recycle_result_storage(old_fused);
    Some(redraw_mode)
}

pub(crate) fn repaint_runtime_text_only_dirty_nodes(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
    dirty_nodes: &[silksurf_dom::NodeId],
) -> Option<BrowserRedrawMode> {
    if let [node] = dirty_nodes {
        return repaint_single_runtime_text_node(runtime, frame, *node);
    }

    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut updates = Vec::with_capacity(dirty_nodes.len());
    let mut damage = None;
    for &node in dirty_nodes {
        let text = dirty_text_node_content(&dom, node)?;
        let item_index = dirty_text_display_item_index(&runtime.display_list.items, node)?;
        let item_damage =
            text_item_in_place_damage_rect(&runtime.display_list.items[item_index], text)?;
        let _ = fused_node_rect(&runtime.fused, node)?;
        damage = Some(match damage {
            Some(current) => union_rect(current, item_damage),
            None => item_damage,
        });
        updates.push((item_index, text.to_string()));
    }
    drop(dom);

    let damage = damage?;
    trace_runtime_text_repaint(dirty_nodes.len(), damage);
    let direct_item = (updates.len() == 1).then_some(updates[0].0);
    let mut direct_text = None;
    for (item_index, text) in updates {
        let text_paint =
            update_text_display_item_content(&mut runtime.display_list.items[item_index], &text)?;
        if Some(item_index) == direct_item {
            direct_text = Some((item_index, text, text_paint));
        }
    }
    if let Some((item_index, text, text_paint)) = direct_text
        && paint_text_damage_argb(
            &runtime.display_list.items,
            item_index,
            frame,
            damage,
            text_paint,
            &text,
        )
    {
        return Some(BrowserRedrawMode::Damage(damage));
    }
    rasterize_browser_document_damage_scratch(
        &runtime.display_list,
        frame.bitmap_scroll_y,
        frame.bitmap_height,
        damage,
        &mut runtime.damage_scratch,
    );
    if !sync_argb_damage_from_scratch(&runtime.damage_scratch, &mut frame.argb, FRAME_WIDTH) {
        rasterize_browser_document_damage_into(
            &runtime.display_list,
            frame.bitmap_scroll_y,
            frame.bitmap_height,
            damage,
            &mut runtime.rgba,
            &mut runtime.damage_scratch,
        );
        sync_argb_damage_from_rgba(
            &runtime.rgba,
            &mut frame.argb,
            FRAME_WIDTH,
            frame.bitmap_height,
            viewport_damage_rect(damage, frame.bitmap_scroll_y),
        );
    }
    Some(BrowserRedrawMode::Damage(damage))
}

pub(crate) fn repaint_single_runtime_text_node(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
    node: silksurf_dom::NodeId,
) -> Option<BrowserRedrawMode> {
    let (item_index, text, damage) = {
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let text = dirty_text_node_content(&dom, node)?.to_string();
        let item_index = dirty_text_display_item_index(&runtime.display_list.items, node)?;
        let damage =
            text_item_in_place_damage_rect(&runtime.display_list.items[item_index], &text)?;
        let _ = fused_node_rect(&runtime.fused, node)?;
        (item_index, text, damage)
    };

    trace_runtime_text_repaint(1, damage);
    let text_paint =
        update_text_display_item_content(&mut runtime.display_list.items[item_index], &text)?;
    if paint_text_damage_argb(
        &runtime.display_list.items,
        item_index,
        frame,
        damage,
        text_paint,
        &text,
    ) {
        return Some(BrowserRedrawMode::Damage(damage));
    }
    rasterize_browser_document_damage_scratch(
        &runtime.display_list,
        frame.bitmap_scroll_y,
        frame.bitmap_height,
        damage,
        &mut runtime.damage_scratch,
    );
    if !sync_argb_damage_from_scratch(&runtime.damage_scratch, &mut frame.argb, FRAME_WIDTH) {
        rasterize_browser_document_damage_into(
            &runtime.display_list,
            frame.bitmap_scroll_y,
            frame.bitmap_height,
            damage,
            &mut runtime.rgba,
            &mut runtime.damage_scratch,
        );
        sync_argb_damage_from_rgba(
            &runtime.rgba,
            &mut frame.argb,
            FRAME_WIDTH,
            frame.bitmap_height,
            viewport_damage_rect(damage, frame.bitmap_scroll_y),
        );
    }
    Some(BrowserRedrawMode::Damage(damage))
}

pub(crate) fn trace_runtime_text_repaint(dirty_count: usize, damage: Rect) {
    if std::env::var_os("SILKSURF_TRACE_RUNTIME_TEXT").is_none() {
        return;
    }
    eprintln!(
        "[SilkSurf] Runtime text repaint: dirty_nodes={} damage=({}, {}, {}, {})",
        dirty_count, damage.x, damage.y, damage.width, damage.height
    );
}

pub(crate) fn dirty_text_node_content(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<&str> {
    match dom.node(node).ok()?.kind() {
        silksurf_dom::NodeKind::Text { text } => Some(text.as_str()),
        _ => None,
    }
}

pub(crate) fn dirty_text_display_item_index(
    items: &[silksurf_render::DisplayItem],
    node: silksurf_dom::NodeId,
) -> Option<usize> {
    items.iter().position(|item| {
        matches!(
            item,
            silksurf_render::DisplayItem::Text {
                node: item_node,
                ..
            } if *item_node == node
        )
    })
}

pub(crate) fn text_item_in_place_damage_rect(
    item: &silksurf_render::DisplayItem,
    value: &str,
) -> Option<Rect> {
    let silksurf_render::DisplayItem::Text {
        rect, font_size, ..
    } = item
    else {
        return None;
    };
    if rect.width <= 0.0 || rect.height <= 0.0 || *font_size <= 0.0 || !font_size.is_finite() {
        return None;
    }
    let (width, height) = page_bitmap_text_bounds(value, *font_size)?;
    if width <= rect.width + 0.5 && height <= rect.height + 0.5 {
        Some(focused_input_text_damage_rect(item, value).unwrap_or(*rect))
    } else {
        None
    }
}

pub(crate) fn update_text_display_item_content(
    item: &mut silksurf_render::DisplayItem,
    value: &str,
) -> Option<TextItemPaint> {
    let silksurf_render::DisplayItem::Text {
        rect,
        text,
        text_len,
        font_size,
        color,
        ..
    } = item
    else {
        return None;
    };
    text.clear();
    text.push_str(value);
    *text_len = value.len() as u32;
    Some(TextItemPaint {
        rect: *rect,
        font_size: *font_size,
        color: *color,
    })
}

pub(crate) fn repaint_focused_input_value(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
    node: silksurf_dom::NodeId,
    value: &str,
) -> Option<BrowserRedrawMode> {
    let control_damage = frame
        .input_targets
        .iter()
        .find(|target| target.node == node)?
        .rect;
    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let text_index = runtime.display_list.items.iter().position(|item| {
        display_text_item_matches_input_node(&dom, item, node)
            || display_text_item_intersects_rect(item, control_damage)
    })?;
    drop(dom);
    let text_item = &runtime.display_list.items[text_index];
    let damage = focused_input_text_damage_rect(text_item, value).unwrap_or(control_damage);
    trace_focused_input_damage(node, damage);
    let text_paint =
        update_focused_input_text_item(&mut runtime.display_list.items[text_index], value)?;
    if paint_text_damage_argb(
        &runtime.display_list.items,
        text_index,
        frame,
        damage,
        text_paint,
        value,
    ) {
        return Some(BrowserRedrawMode::Damage(damage));
    }
    rasterize_browser_document_damage_scratch(
        &runtime.display_list,
        frame.bitmap_scroll_y,
        frame.bitmap_height,
        damage,
        &mut runtime.damage_scratch,
    );
    if !sync_argb_damage_from_scratch(&runtime.damage_scratch, &mut frame.argb, FRAME_WIDTH) {
        rasterize_browser_document_damage_into(
            &runtime.display_list,
            frame.bitmap_scroll_y,
            frame.bitmap_height,
            damage,
            &mut runtime.rgba,
            &mut runtime.damage_scratch,
        );
        sync_argb_damage_from_rgba(
            &runtime.rgba,
            &mut frame.argb,
            FRAME_WIDTH,
            frame.bitmap_height,
            viewport_damage_rect(damage, frame.bitmap_scroll_y),
        );
    }
    Some(BrowserRedrawMode::Damage(damage))
}

pub(crate) fn update_focused_input_text_item(
    item: &mut silksurf_render::DisplayItem,
    value: &str,
) -> Option<TextItemPaint> {
    let silksurf_render::DisplayItem::Text {
        rect,
        text,
        text_len,
        font_size,
        color,
        ..
    } = item
    else {
        return None;
    };
    text.clear();
    text.push_str(value);
    *text_len = value.len() as u32;
    Some(TextItemPaint {
        rect: *rect,
        font_size: *font_size,
        color: *color,
    })
}

pub(crate) fn paint_text_damage_argb(
    items: &[silksurf_render::DisplayItem],
    text_index: usize,
    frame: &mut BrowserFrame,
    damage: Rect,
    text_paint: TextItemPaint,
    value: &str,
) -> bool {
    if text_paint.color.a != 255 || !page_bitmap_text_supported(value, text_paint.font_size) {
        return false;
    }
    let viewport_damage = viewport_damage_rect(damage, frame.bitmap_scroll_y);
    let Some(pixel_rect) = pixel_rect_from_rect(viewport_damage, FRAME_WIDTH, frame.bitmap_height)
    else {
        return false;
    };
    let Some(background) = text_damage_background_argb(items, text_index, damage) else {
        return false;
    };
    let required = FRAME_WIDTH as usize * frame.bitmap_height as usize;
    if frame.argb.len() < required {
        return false;
    }
    fill_argb_rect(
        &mut frame.argb,
        FRAME_WIDTH,
        frame.bitmap_height,
        pixel_rect.x,
        pixel_rect.y,
        pixel_rect.width,
        pixel_rect.height,
        background,
    );
    draw_page_bitmap_text_clipped(
        &mut frame.argb,
        FRAME_WIDTH,
        frame.bitmap_height,
        text_paint.rect.x,
        text_paint.rect.y - frame.bitmap_scroll_y as f32,
        value,
        text_paint.font_size,
        css_color_to_argb(text_paint.color),
        pixel_rect,
    )
}

pub(crate) fn page_bitmap_text_supported(text: &str, font_size: f32) -> bool {
    page_bitmap_text_bounds(text, font_size).is_some()
}

pub(crate) fn page_bitmap_text_bounds(text: &str, font_size: f32) -> Option<(f32, f32)> {
    let (_, advance, line_height, space_advance) = page_bitmap_text_metrics(font_size)?;
    if text.is_empty() {
        return Some((0.0, 0.0));
    }
    let mut current_width = 0_i32;
    let mut widest_width = 0_i32;
    let mut line_count = 1_i32;
    for ch in text.chars() {
        match ch {
            '\n' => {
                widest_width = widest_width.max(current_width);
                current_width = 0;
                line_count = line_count.saturating_add(1);
            }
            '\r' => {}
            '\t' => current_width = current_width.saturating_add(space_advance.saturating_mul(4)),
            ' ' => current_width = current_width.saturating_add(space_advance),
            _ => {
                if !ch.is_ascii() || chrome_glyph_byte(ch as u8).is_none() {
                    return None;
                }
                current_width = current_width.saturating_add(advance);
            }
        }
    }
    widest_width = widest_width.max(current_width);
    Some((
        widest_width.max(0) as f32,
        line_count.max(1).saturating_mul(line_height).max(0) as f32,
    ))
}

pub(crate) fn text_damage_background_argb(
    items: &[silksurf_render::DisplayItem],
    text_index: usize,
    damage: Rect,
) -> Option<u32> {
    for item in items.iter().take(text_index).rev() {
        if !display_item_intersects_viewport(item, damage) {
            continue;
        }
        match item {
            silksurf_render::DisplayItem::SolidColor { color, .. }
            | silksurf_render::DisplayItem::RoundedRect { color, .. } => {
                if color.a != 255 {
                    return None;
                }
                return Some(css_color_to_argb(*color));
            }
            silksurf_render::DisplayItem::LinearGradient { .. }
            | silksurf_render::DisplayItem::Image { .. } => return None,
            silksurf_render::DisplayItem::Text { .. }
            | silksurf_render::DisplayItem::BoxShadow { .. } => {}
        }
    }
    Some(argb(255, 255, 255, 255))
}

pub(crate) fn pixel_rect_from_rect(rect: Rect, width: u32, height: u32) -> Option<PixelRect> {
    if width == 0 || height == 0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let x0 = rect.x.floor().max(0.0).min(width as f32) as u32;
    let y0 = rect.y.floor().max(0.0).min(height as f32) as u32;
    let x1 = (rect.x + rect.width).ceil().max(0.0).min(width as f32) as u32;
    let y1 = (rect.y + rect.height).ceil().max(0.0).min(height as f32) as u32;
    (x1 > x0 && y1 > y0).then_some(PixelRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

pub(crate) fn pixel_rect_intersection(a: PixelRect, b: PixelRect) -> Option<PixelRect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.x.saturating_add(a.width).min(b.x.saturating_add(b.width));
    let y1 =
        a.y.saturating_add(a.height)
            .min(b.y.saturating_add(b.height));
    (x1 > x0 && y1 > y0).then_some(PixelRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

pub(crate) fn trace_focused_input_damage(node: silksurf_dom::NodeId, damage: Rect) {
    if std::env::var_os("SILKSURF_TRACE_INPUT").is_none() {
        return;
    }
    eprintln!(
        "[SilkSurf] input repaint: node={} damage=({}, {}, {}, {})",
        node.raw(),
        damage.x,
        damage.y,
        damage.width,
        damage.height
    );
}

pub(crate) fn display_text_item_matches_input_node(
    dom: &silksurf_dom::Dom,
    item: &silksurf_render::DisplayItem,
    node: silksurf_dom::NodeId,
) -> bool {
    let silksurf_render::DisplayItem::Text {
        node: item_node, ..
    } = item
    else {
        return false;
    };
    let mut current = Some(*item_node);
    while let Some(current_node) = current {
        if current_node == node {
            return true;
        }
        current = dom.parent(current_node).ok().flatten();
    }
    false
}

pub(crate) fn display_text_item_intersects_rect(
    item: &silksurf_render::DisplayItem,
    rect: Rect,
) -> bool {
    let silksurf_render::DisplayItem::Text {
        rect: item_rect, ..
    } = item
    else {
        return false;
    };
    rects_intersect(*item_rect, rect)
}

pub(crate) fn focused_input_text_damage_rect(
    item: &silksurf_render::DisplayItem,
    new_value: &str,
) -> Option<Rect> {
    let silksurf_render::DisplayItem::Text {
        rect,
        text,
        font_size,
        ..
    } = item
    else {
        return None;
    };
    let common_prefix_bytes = common_prefix_byte_len(text, new_value);
    let char_width = (*font_size * 0.65).max(1.0);
    let line_height = (*font_size * 1.35 + 4.0).max(1.0);
    let (start_line, start_column) = text_position(&text[..common_prefix_bytes]);
    let old_suffix = &text[common_prefix_bytes..];
    let new_suffix = &new_value[common_prefix_bytes..];
    let old_columns = trailing_line_column_count(old_suffix);
    let new_columns = trailing_line_column_count(new_suffix);
    let dirty_lines = suffix_line_span(old_suffix).max(suffix_line_span(new_suffix));
    let y = rect.y + start_line as f32 * line_height;
    let x = if dirty_lines == 1 {
        (rect.x + start_column as f32 * char_width - 2.0).max(rect.x)
    } else {
        rect.x
    };
    let dirty_columns = old_columns.max(new_columns).max(1) as f32;
    let width = if dirty_lines == 1 {
        (dirty_columns * char_width + 4.0).min(rect.x + rect.width - x)
    } else {
        rect.x + rect.width - x
    };
    if y >= rect.y + rect.height {
        return None;
    }
    let height = (dirty_lines as f32 * line_height).min(rect.y + rect.height - y);
    (width > 0.0).then_some(Rect {
        x,
        y,
        width,
        height: height.max(1.0),
    })
}

pub(crate) fn focused_empty_insert_damage(
    frame: &BrowserFrame,
    node: silksurf_dom::NodeId,
    old_value: &str,
    new_value: &str,
) -> Option<Rect> {
    if !old_value.is_empty() || new_value.is_empty() {
        return None;
    }
    let rect = frame
        .input_targets
        .iter()
        .find(|target| target.node == node)?
        .rect;
    let columns = new_value
        .chars()
        .take_while(|ch| *ch != '\n' && *ch != '\r')
        .count()
        .max(1) as f32;
    let width = (columns * 10.0 + 8.0).min(rect.width);
    (width > 0.0 && rect.height > 0.0).then_some(Rect {
        x: rect.x,
        y: rect.y,
        width,
        height: rect.height,
    })
}

pub(crate) fn common_prefix_byte_len(a: &str, b: &str) -> usize {
    a.char_indices()
        .zip(b.char_indices())
        .take_while(|((_, a_ch), (_, b_ch))| a_ch == b_ch)
        .last()
        .map_or(0, |((idx, ch), _)| idx + ch.len_utf8())
}

pub(crate) fn text_position(text: &str) -> (usize, usize) {
    let mut line = 0usize;
    let mut column = 0usize;
    for ch in text.chars() {
        if ch == '\n' {
            line = line.saturating_add(1);
            column = 0;
        } else {
            column = column.saturating_add(1);
        }
    }
    (line, column)
}

pub(crate) fn trailing_line_column_count(text: &str) -> usize {
    text.rsplit('\n')
        .next()
        .map_or(0, |line| line.chars().count())
}

pub(crate) fn suffix_line_span(text: &str) -> usize {
    text.chars().filter(|ch| *ch == '\n').count() + 1
}

pub(crate) fn start_navigation_worker(
    state: &mut BrowserState,
    navigation_rx: &Rc<RefCell<Option<mpsc::Receiver<NavigationMessage>>>>,
    request: BrowserNavigationRequest,
    history_action: PendingHistoryAction,
    wake_handle: &silksurf_gui::WinitWakeHandle,
    render_config: &BrowserRenderConfig,
    image_cache: &Arc<Mutex<ImageResourceCache>>,
) -> bool {
    if state.navigation_pending {
        return false;
    }
    state.navigation_generation = state.navigation_generation.saturating_add(1);
    let generation = state.navigation_generation;
    state.navigation_pending = true;
    state.pending_history = Some(history_action);
    let navigation_start_retained_ready = state.frame.navigation_start_retained_sent;
    set_browser_status(state, "loading");
    mark_redraw(state, BrowserRedrawMode::NavigationStartChrome);
    if navigation_start_retained_ready {
        let damage =
            browser_navigation_start_present_damage(FRAME_WIDTH, state.frame.bitmap_height);
        if damage != silksurf_gui::WinitPresentDamage::Clean {
            state.retained_present = Some(BrowserRetainedPresent {
                tag: NAVIGATION_START_RETAINED_TAG,
                damage,
            });
        }
    }
    let (tx, rx) = mpsc::channel();
    *navigation_rx.borrow_mut() = Some(rx);
    let wake_handle = wake_handle.clone();
    let render_config = render_config.clone();
    let image_cache = Arc::clone(image_cache);
    thread::spawn(move || {
        let result = load_navigation_payload(&request, &render_config, &image_cache);
        let _ = tx.send((generation, result));
        wake_handle.wake();
    });
    true
}

#[cfg(test)]
mod tests {
    // Module split from the former single-file binary; the crate root
    // re-exports every module so sibling items resolve by bare name.
    #[allow(clippy::wildcard_imports)]
    use crate::*;

    #[test]
    fn runtime_text_damage_tracks_changed_suffix() {
        let item = silksurf_render::DisplayItem::Text {
            rect: Rect {
                x: 100.0,
                y: 200.0,
                width: 400.0,
                height: 64.0,
            },
            node: silksurf_dom::NodeId::from_raw(1),
            text_len: 6,
            text: "stable".to_string(),
            font_size: 16.0,
            color: rgba(0, 0, 0, 255),
        };

        let damage =
            text_item_in_place_damage_rect(&item, "staple").expect("text item gives damage");

        assert!(damage.x > 125.0);
        assert!(damage.width < 48.0);
        assert!(damage.height < 32.0);
    }

    #[test]
    fn text_only_dom_diff_produces_damage_rect() {
        let old_doc = parse_html("<!doctype html><html><body><p>Hello</p></body></html>")
            .expect("old html parses");
        let new_doc = parse_html("<!doctype html><html><body><p>World</p></body></html>")
            .expect("new html parses");
        let stylesheet = test_stylesheet(&old_doc.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let old_fused =
            fused_style_layout_paint(&old_doc.dom, &stylesheet, old_doc.document, viewport);
        let new_fused =
            fused_style_layout_paint(&new_doc.dom, &stylesheet, new_doc.document, viewport);
        let diff = silksurf_dom::diff::diff_doms(
            &old_doc.dom,
            old_doc.document,
            &new_doc.dom,
            new_doc.document,
        );

        let damage = text_only_diff_damage_rect(&diff, &old_fused, &new_fused)
            .expect("text-only diff yields damage");

        assert!(damage.width > 0.0);
        assert!(damage.height > 0.0);
        assert!(damage.y >= BROWSER_CHROME_HEIGHT);
    }

    #[test]
    fn structural_dom_diff_requires_full_repaint() {
        let old_doc =
            parse_html("<!doctype html><html><body><p>Hello</p></body></html>").expect("old html");
        let new_doc =
            parse_html("<!doctype html><html><body><p>Hello</p><p>Second</p></body></html>")
                .expect("new html");
        let stylesheet = test_stylesheet(&old_doc.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let old_fused =
            fused_style_layout_paint(&old_doc.dom, &stylesheet, old_doc.document, viewport);
        let new_fused =
            fused_style_layout_paint(&new_doc.dom, &stylesheet, new_doc.document, viewport);
        let diff = silksurf_dom::diff::diff_doms(
            &old_doc.dom,
            old_doc.document,
            &new_doc.dom,
            new_doc.document,
        );

        assert!(text_only_diff_damage_rect(&diff, &old_fused, &new_fused).is_none());
    }

    #[test]
    fn js_text_mutation_dirty_nodes_produce_damage_rect() {
        let document =
            parse_html("<!doctype html><html><body><p id=\"msg\">Hello</p></body></html>")
                .expect("html parses");
        let stylesheet = test_stylesheet(&document.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let old_fused =
            fused_style_layout_paint(&document.dom, &stylesheet, document.document, viewport);
        let dom_arc = Arc::new(Mutex::new(document.dom));
        {
            let mut dom = dom_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = dom.take_dirty_nodes();
        }

        let mut js_ctx = SilkContext::with_dom(&dom_arc);
        js_ctx
            .eval(
                "var el = document.getElementById('msg'); \
                 el.firstChild.textContent = 'Updated';",
            )
            .expect("script mutates text");

        let mut dom = dom_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dirty_nodes = dom.take_dirty_nodes();
        let new_fused = fused_style_layout_paint(&dom, &stylesheet, document.document, viewport);
        let damage = dirty_nodes_damage_rect(&dom, &dirty_nodes, &old_fused, &new_fused)
            .expect("dirty text node yields damage");

        assert_eq!(dirty_nodes.len(), 1);
        assert!(damage.width > 0.0);
        assert!(damage.height > 0.0);
    }

    #[test]
    fn initial_host_tick_runs_deferred_dom_text_mutation() {
        let document =
            parse_html("<!doctype html><html><body><p id=\"msg\">Hello</p></body></html>")
                .expect("html parses");
        let document_node = document.document;
        let dom_arc = Arc::new(Mutex::new(document.dom));
        {
            let mut dom = dom_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = dom.take_dirty_nodes();
        }

        let mut js_ctx = SilkContext::with_dom(&dom_arc);
        js_ctx
            .eval(
                "var el = document.getElementById('msg'); \
                 setTimeout(function () { el.firstChild.textContent = 'Deferred'; }, 0);",
            )
            .expect("script schedules deferred mutation");
        {
            let dom = dom_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert!(find_text_node(&dom, document_node, "Deferred").is_none());
        }

        drain_initial_host_callbacks(&mut js_ctx);

        let mut dom = dom_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let text_node =
            find_text_node(&dom, document_node, "Deferred").expect("host tick mutates text");
        assert_eq!(dom.take_dirty_nodes(), vec![text_node]);
    }

    #[test]
    fn retained_runtime_tick_repaints_dirty_text_damage() {
        let document =
            parse_html("<!doctype html><html><body><p id=\"msg\">Hello</p></body></html>")
                .expect("html parses");
        let stylesheet = test_stylesheet(&document.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let dom_arc = Arc::new(Mutex::new(document.dom));
        let mut js_ctx = SilkContext::with_dom(&dom_arc);
        let style_index = StyleIndex::for_viewport(&stylesheet, viewport.width, viewport.height);
        let mut fused = {
            let dom = dom_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            fused_style_layout_paint(&dom, &stylesheet, document.document, viewport)
        };
        let display_list = silksurf_render::DisplayList {
            items: std::mem::take(&mut fused.display_items),
            tiles: None,
        };
        let raster_height = browser_frame_height(&display_list.items, BROWSER_CHROME_HEIGHT as u32);
        let bitmap_height = initial_browser_window_height(raster_height);
        let mut rgba = Vec::new();
        let mut viewport_item_indices = Vec::new();
        rasterize_browser_viewport_into(
            &display_list,
            0,
            bitmap_height,
            &mut rgba,
            &mut viewport_item_indices,
        );
        let mut argb = Vec::new();
        rgba_bytes_to_argb_words_into(&rgba, &mut argb);
        let old_argb = argb.clone();
        let runtime_display_list = display_list.clone();

        {
            let mut dom = dom_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = dom.take_dirty_nodes();
        }
        js_ctx
            .eval(
                "var el = document.getElementById('msg'); \
                 requestAnimationFrame(function () { el.firstChild.textContent = 'Runtime'; });",
            )
            .expect("script schedules frame mutation");

        let mut state = BrowserState {
            frame: BrowserFrame {
                url: "https://example.com/".to_string(),
                argb,
                raster_height,
                bitmap_height,
                bitmap_scroll_y: 0,
                focus_viewport_cache: None,
                focus_viewport_retained_sent: false,
                current_view_retained_sent: false,
                navigation_start_retained_sent: false,
                scroll_viewport_caches: Vec::new(),
                link_targets: Vec::new(),
                input_targets: Vec::new(),
            },
            runtime: Some(BrowserPageRuntime {
                dom: Arc::clone(&dom_arc),
                document: document.document,
                stylesheet,
                style_index,
                viewport,
                js_ctx,
                fused,
                fused_workspace: FusedWorkspace::new(),
                display_list: runtime_display_list,
                images: Vec::new(),
                rgba,
                damage_scratch: silksurf_render::DamageScratch::default(),
                viewport_item_indices,
            }),
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec!["https://example.com/".to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: "https://example.com/".to_string(),
            address_cursor: 0,
            focused_input: None,
            redraw_mode: BrowserRedrawMode::Chrome,
            retained_present: None,
        };

        assert!(tick_browser_runtime(&mut state));
        assert!(matches!(
            state.redraw_mode,
            BrowserRedrawMode::DamageWithChrome(_)
        ));
        assert_ne!(state.frame.argb, old_argb);

        let dom = dom_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(find_text_node(&dom, document.document, "Runtime").is_some());
    }

    #[test]
    fn retained_runtime_text_mutation_skips_layout_when_text_fits() {
        let document =
            parse_html("<!doctype html><html><body><p id=\"msg\">Hello</p></body></html>")
                .expect("html parses");
        let stylesheet = test_stylesheet(&document.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let dom_arc = Arc::new(Mutex::new(document.dom));
        let mut js_ctx = SilkContext::with_dom(&dom_arc);
        let style_index = StyleIndex::for_viewport(&stylesheet, viewport.width, viewport.height);
        let mut fused = {
            let dom = dom_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            fused_style_layout_paint(&dom, &stylesheet, document.document, viewport)
        };
        let display_list = silksurf_render::DisplayList {
            items: std::mem::take(&mut fused.display_items),
            tiles: None,
        };
        let raster_height = browser_frame_height(&display_list.items, BROWSER_CHROME_HEIGHT as u32);
        let bitmap_height = initial_browser_window_height(raster_height);
        let mut rgba = Vec::new();
        let mut viewport_item_indices = Vec::new();
        rasterize_browser_viewport_into(
            &display_list,
            0,
            bitmap_height,
            &mut rgba,
            &mut viewport_item_indices,
        );
        let mut argb = Vec::new();
        rgba_bytes_to_argb_words_into(&rgba, &mut argb);
        let old_argb = argb.clone();
        let runtime_display_list = display_list.clone();

        {
            let mut dom = dom_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = dom.take_dirty_nodes();
        }
        js_ctx
            .eval(
                "var el = document.getElementById('msg'); \
                 requestAnimationFrame(function () { el.firstChild.textContent = 'Jello'; });",
            )
            .expect("script schedules frame mutation");

        let mut state = BrowserState {
            frame: BrowserFrame {
                url: "https://example.com/".to_string(),
                argb,
                raster_height,
                bitmap_height,
                bitmap_scroll_y: 0,
                focus_viewport_cache: None,
                focus_viewport_retained_sent: false,
                current_view_retained_sent: false,
                navigation_start_retained_sent: false,
                scroll_viewport_caches: Vec::new(),
                link_targets: Vec::new(),
                input_targets: Vec::new(),
            },
            runtime: Some(BrowserPageRuntime {
                dom: Arc::clone(&dom_arc),
                document: document.document,
                stylesheet,
                style_index,
                viewport,
                js_ctx,
                fused,
                fused_workspace: FusedWorkspace::new(),
                display_list: runtime_display_list,
                images: Vec::new(),
                rgba,
                damage_scratch: silksurf_render::DamageScratch::default(),
                viewport_item_indices,
            }),
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec!["https://example.com/".to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: "https://example.com/".to_string(),
            address_cursor: 0,
            focused_input: None,
            redraw_mode: BrowserRedrawMode::Chrome,
            retained_present: None,
        };

        assert!(tick_browser_runtime(&mut state));
        let BrowserRedrawMode::DamageWithChrome(damage) = state.redraw_mode else {
            panic!("runtime mutation produces damage with chrome");
        };
        assert!(damage.width < 80.0);
        assert_ne!(state.frame.argb, old_argb);

        let runtime = state.runtime.as_ref().expect("runtime remains installed");
        assert_eq!(runtime.fused_workspace.node_count(), 0);
        assert!(runtime.damage_scratch.last_damage().is_none());
        assert!(
            runtime
                .display_list
                .items
                .iter()
                .any(|item| matches!(item, silksurf_render::DisplayItem::Text { text, .. } if text == "Jello"))
        );
        let dom = dom_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(find_text_node(&dom, document.document, "Jello").is_some());
    }

    #[test]
    fn retained_runtime_repaints_js_input_value_viewport() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><input id=\"prompt\" value=\"Hi\"><script>requestAnimationFrame(function(){setTimeout(function(){document.getElementById('prompt').value='AI';},0);});</script></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };

        let mut page = build_browser_page(payload).expect("payload builds page");
        let input_node = page.frame.input_targets[0].node;

        assert!(matches!(
            repaint_runtime_host_callbacks(&mut page.runtime, &mut page.frame)
                .expect("runtime callback repaints"),
            Some(BrowserRedrawMode::Damage(_))
        ));
        let dom = page
            .runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(input_value(&dom, input_node), "AI");
    }
}
