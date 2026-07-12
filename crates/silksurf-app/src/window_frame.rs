// Frame-present functions pass window plus frame geometry and damage
// rects as explicit parameters into the retained-buffer protocol.
#![allow(clippy::too_many_arguments)]

// Module split from the former single-file binary; the crate root
// re-exports every module so sibling items resolve by bare name.
#[allow(clippy::wildcard_imports)]
use crate::*;

pub(crate) fn render_browser_window_frame(
    state_ref: &Rc<RefCell<BrowserState>>,
    scroll_ref: &Cell<f32>,
    last_width: &Cell<u32>,
    last_height: &Cell<u32>,
    chrome_height: u32,
    trace_app_frame: bool,
    window_width: u32,
    window_height: u32,
    buffer_age: u8,
    pixels: &mut [u32],
) -> silksurf_gui::WinitPresentDamage {
    let mut state = state_ref.borrow_mut();
    let max_scroll =
        max_browser_scroll_offset(state.frame.raster_height, window_height, chrome_height);
    let scroll = clamp_scroll_offset(scroll_ref.get(), max_scroll);
    scroll_ref.set(scroll);
    prepare_browser_bitmap_for_window(
        &mut state,
        last_width,
        last_height,
        chrome_height,
        window_width,
        window_height,
        scroll,
        trace_app_frame,
    );
    let render_mode = state.redraw_mode;
    let seed_full_buffer = browser_render_seeds_full_buffer(render_mode, buffer_age);
    let blit_start = std::time::Instant::now();
    blit_browser_window_frame(
        &state,
        seed_full_buffer,
        render_mode,
        chrome_height,
        window_width,
        window_height,
        pixels,
    );
    let blit_elapsed = blit_start.elapsed();
    let chrome_start = std::time::Instant::now();
    draw_browser_window_chrome(
        &state,
        seed_full_buffer,
        render_mode,
        window_width,
        window_height,
        pixels,
    );
    let chrome_elapsed = chrome_start.elapsed();
    trace_browser_window_frame(
        trace_app_frame,
        window_width,
        window_height,
        buffer_age,
        render_mode,
        seed_full_buffer,
        blit_elapsed,
        chrome_elapsed,
    );
    if render_mode != BrowserRedrawMode::Clean {
        last_width.set(window_width);
        last_height.set(window_height);
    }
    state.retained_present = None;
    state.redraw_mode = BrowserRedrawMode::Clean;
    browser_present_damage(
        render_mode,
        state.frame.raster_height,
        chrome_height,
        scroll.round() as u32,
        window_width,
        window_height,
    )
}

pub(crate) fn prepare_browser_bitmap_for_window(
    state: &mut BrowserState,
    last_width: &Cell<u32>,
    last_height: &Cell<u32>,
    chrome_height: u32,
    window_width: u32,
    window_height: u32,
    scroll: f32,
    trace_app_frame: bool,
) {
    let exposes_unpainted_area = window_size_exposes_unpainted_area(
        last_width.get(),
        last_height.get(),
        window_width,
        window_height,
    );
    let refresh_start = std::time::Instant::now();
    let bitmap_refresh = refresh_browser_frame_bitmap(
        state,
        scroll.round() as u32,
        window_height.max(chrome_height),
    );
    trace_browser_bitmap_refresh(trace_app_frame, bitmap_refresh, refresh_start.elapsed());
    if exposes_unpainted_area || bitmap_refresh == BrowserBitmapRefresh::Full {
        last_width.set(window_width);
        last_height.set(window_height);
        state.redraw_mode = BrowserRedrawMode::Full;
    } else if let BrowserBitmapRefresh::ScrollReuse(damage) = bitmap_refresh {
        mark_redraw(state, BrowserRedrawMode::Damage(damage));
    }
}

pub(crate) fn blit_browser_window_frame(
    state: &BrowserState,
    seed_full_buffer: bool,
    render_mode: BrowserRedrawMode,
    chrome_height: u32,
    window_width: u32,
    window_height: u32,
    pixels: &mut [u32],
) {
    if seed_full_buffer || render_mode == BrowserRedrawMode::Full {
        blit_browser_frame(
            &state.frame.argb,
            FRAME_WIDTH,
            state.frame.bitmap_height,
            chrome_height,
            0,
            window_width,
            window_height,
            pixels,
        );
        return;
    }
    if let BrowserRedrawMode::Damage(damage) | BrowserRedrawMode::DamageWithChrome(damage) =
        render_mode
    {
        blit_browser_frame_damage(
            &state.frame.argb,
            FRAME_WIDTH,
            state.frame.bitmap_height,
            chrome_height,
            state.frame.bitmap_scroll_y,
            window_width,
            window_height,
            damage,
            pixels,
        );
    }
}

pub(crate) fn draw_browser_window_chrome(
    state: &BrowserState,
    seed_full_buffer: bool,
    render_mode: BrowserRedrawMode,
    window_width: u32,
    window_height: u32,
    pixels: &mut [u32],
) {
    match (seed_full_buffer, render_mode) {
        (_, BrowserRedrawMode::Clean | BrowserRedrawMode::Scroll) => {}
        (false, BrowserRedrawMode::AddressFocusChrome) => {
            draw_browser_address_focus_from_state(state, pixels, window_width, window_height);
        }
        (false, BrowserRedrawMode::AddressFullTextChrome) => {
            draw_browser_address_full_text_from_state(state, pixels, window_width, window_height);
        }
        (false, BrowserRedrawMode::AddressChrome) => {
            draw_browser_address_from_state(state, pixels, window_width, window_height);
        }
        (false, BrowserRedrawMode::AddressTextChrome) => {
            draw_browser_address_text_from_state(state, pixels, window_width, window_height);
        }
        (false, BrowserRedrawMode::NavigationStartChrome) => {
            draw_browser_navigation_start_from_state(state, pixels, window_width, window_height);
        }
        (false, BrowserRedrawMode::StatusChrome) => {
            draw_browser_status_from_state(state, pixels, window_width, window_height);
        }
        (false, BrowserRedrawMode::Damage(_) | BrowserRedrawMode::PageInputFocus(_)) => {}
        (true, _)
        | (
            false,
            BrowserRedrawMode::Full
            | BrowserRedrawMode::DamageWithChrome(_)
            | BrowserRedrawMode::Chrome,
        ) => {
            draw_browser_chrome_overlays(state, pixels, window_width, window_height);
        }
    }
}

pub(crate) fn trace_browser_window_frame(
    enabled: bool,
    window_width: u32,
    window_height: u32,
    buffer_age: u8,
    render_mode: BrowserRedrawMode,
    seed_full_buffer: bool,
    blit_elapsed: std::time::Duration,
    chrome_elapsed: std::time::Duration,
) {
    if enabled {
        eprintln!(
            "[SilkSurf] app frame: {window_width}x{window_height} age {buffer_age} mode {render_mode:?}, seed_full {seed_full_buffer}, blit {blit_elapsed:?}, chrome {chrome_elapsed:?}"
        );
    }
}

pub(crate) fn browser_render_ready(
    state: &Rc<RefCell<BrowserState>>,
    last_width: &Cell<u32>,
    last_height: &Cell<u32>,
    window_width: u32,
    window_height: u32,
) -> bool {
    window_size_exposes_unpainted_area(
        last_width.get(),
        last_height.get(),
        window_width,
        window_height,
    ) || state.borrow().redraw_mode != BrowserRedrawMode::Clean
}

pub(crate) fn browser_render_action(
    state: &Rc<RefCell<BrowserState>>,
    last_width: &Cell<u32>,
    last_height: &Cell<u32>,
    window_width: u32,
    window_height: u32,
) -> silksurf_gui::WinitRenderAction {
    if window_size_exposes_unpainted_area(
        last_width.get(),
        last_height.get(),
        window_width,
        window_height,
    ) {
        return silksurf_gui::WinitRenderAction::Render;
    }
    let Some(retained) = state.borrow().retained_present else {
        return silksurf_gui::WinitRenderAction::Render;
    };
    silksurf_gui::WinitRenderAction::Retained {
        tag: retained.tag,
        damage: retained.damage,
    }
}

pub(crate) fn browser_retained_buffer_update(
    state: &Rc<RefCell<BrowserState>>,
    window_width: u32,
    window_height: u32,
) -> Option<silksurf_gui::WinitRetainedBufferUpdate> {
    let mut state = state.borrow_mut();
    if let Some(update) = take_focus_retained_buffer_update(&mut state, window_width, window_height)
    {
        return Some(update);
    }
    if state.focused_input.is_some() {
        if let Some(update) =
            take_navigation_start_retained_buffer_update(&mut state, window_width, window_height)
        {
            return Some(update);
        }
        if let Some(update) =
            take_current_view_retained_buffer_update(&mut state, window_width, window_height)
        {
            return Some(update);
        }
    } else {
        if let Some(update) =
            take_current_view_retained_buffer_update(&mut state, window_width, window_height)
        {
            return Some(update);
        }
        if let Some(update) =
            take_navigation_start_retained_buffer_update(&mut state, window_width, window_height)
        {
            return Some(update);
        }
    }
    if state.frame.focus_viewport_cache.is_some()
        || state.focused_input.is_some()
        || state.address_editing
    {
        state.frame.scroll_viewport_caches.clear();
        return None;
    }
    prepare_scroll_viewport_caches(&mut state, window_width, window_height);
    take_scroll_retained_buffer_update(&mut state, window_width, window_height)
}

pub(crate) fn take_focus_retained_buffer_update(
    state: &mut BrowserState,
    window_width: u32,
    window_height: u32,
) -> Option<silksurf_gui::WinitRetainedBufferUpdate> {
    if state.frame.focus_viewport_retained_sent || window_width != FRAME_WIDTH {
        return None;
    }
    let cache = state.frame.focus_viewport_cache.as_ref()?;
    if cache.bitmap_height != window_height {
        return None;
    }
    let pixel_count = surface_pixel_count(window_width, window_height)?;
    if cache.argb.len() < pixel_count {
        return None;
    }
    let pixels = cache.argb.clone();
    Some(silksurf_gui::WinitRetainedBufferUpdate {
        tag: FOCUS_VIEWPORT_RETAINED_TAG,
        width: window_width,
        height: window_height,
        pixels,
    })
}

pub(crate) fn take_current_view_retained_buffer_update(
    state: &mut BrowserState,
    window_width: u32,
    window_height: u32,
) -> Option<silksurf_gui::WinitRetainedBufferUpdate> {
    if state.frame.current_view_retained_sent
        || window_width != FRAME_WIDTH
        || state.frame.bitmap_height != window_height
    {
        return None;
    }
    let pixel_count = surface_pixel_count(window_width, window_height)?;
    if state.frame.argb.len() < pixel_count {
        return None;
    }
    Some(silksurf_gui::WinitRetainedBufferUpdate {
        tag: CURRENT_VIEW_RETAINED_TAG,
        width: window_width,
        height: window_height,
        pixels: state.frame.argb[..pixel_count].to_vec(),
    })
}

pub(crate) fn take_navigation_start_retained_buffer_update(
    state: &mut BrowserState,
    window_width: u32,
    window_height: u32,
) -> Option<silksurf_gui::WinitRetainedBufferUpdate> {
    if state.frame.navigation_start_retained_sent
        || window_width != FRAME_WIDTH
        || state.frame.bitmap_height != window_height
    {
        return None;
    }
    let pixel_count = surface_pixel_count(window_width, window_height)?;
    if state.frame.argb.len() < pixel_count {
        return None;
    }
    let mut pixels = state.frame.argb[..pixel_count].to_vec();
    draw_navigation_start_retained_chrome(&mut pixels, window_width, window_height);
    Some(silksurf_gui::WinitRetainedBufferUpdate {
        tag: NAVIGATION_START_RETAINED_TAG,
        width: window_width,
        height: window_height,
        pixels,
    })
}

pub(crate) fn prepare_scroll_viewport_caches(
    state: &mut BrowserState,
    window_width: u32,
    window_height: u32,
) {
    if window_width != FRAME_WIDTH || state.frame.bitmap_height != window_height {
        state.frame.scroll_viewport_caches.clear();
        return;
    }
    let Some(runtime) = state.runtime.as_ref() else {
        state.frame.scroll_viewport_caches.clear();
        return;
    };
    let max_scroll = max_browser_scroll_offset(
        state.frame.raster_height,
        window_height,
        BROWSER_CHROME_HEIGHT as u32,
    );
    let targets = scroll_retained_targets(state.frame.bitmap_scroll_y, max_scroll);
    if targets.is_empty() {
        state.frame.scroll_viewport_caches.clear();
        return;
    }
    if scroll_viewport_caches_cover_targets(
        &state.frame.scroll_viewport_caches,
        &targets,
        window_height,
    ) {
        return;
    }

    let mut caches = Vec::with_capacity(targets.len());
    let mut viewport_item_indices = Vec::new();
    for scroll_y in targets {
        let mut rgba = Vec::new();
        let mut argb = Vec::new();
        rasterize_browser_viewport_argb_preferred(
            &runtime.display_list,
            scroll_y,
            window_height,
            &mut rgba,
            &mut argb,
            &mut viewport_item_indices,
        );
        caches.push(ScrollViewportCache {
            scroll_y,
            bitmap_height: window_height,
            tag: scroll_retained_tag_for_scroll_y(scroll_y),
            argb,
            retained_sent: false,
        });
    }
    state.frame.scroll_viewport_caches = caches;
}

pub(crate) fn take_scroll_retained_buffer_update(
    state: &mut BrowserState,
    window_width: u32,
    window_height: u32,
) -> Option<silksurf_gui::WinitRetainedBufferUpdate> {
    let pixel_count = surface_pixel_count(window_width, window_height)?;
    let cache_index = state
        .frame
        .scroll_viewport_caches
        .iter()
        .position(|cache| {
            !cache.retained_sent
                && cache.bitmap_height == window_height
                && cache.argb.len() >= pixel_count
        })?;
    let cache = &mut state.frame.scroll_viewport_caches[cache_index];
    Some(silksurf_gui::WinitRetainedBufferUpdate {
        tag: cache.tag,
        width: window_width,
        height: window_height,
        pixels: cache.argb.clone(),
    })
}

pub(crate) fn scroll_retained_targets(current_scroll_y: u32, max_scroll: f32) -> Vec<u32> {
    let mut targets = Vec::with_capacity(2);
    for delta in [
        (BROWSER_WHEEL_LINE_PX * 2.0) as i32,
        -(BROWSER_WHEEL_LINE_PX as i32),
    ] {
        let target = clamp_scroll_offset(current_scroll_y as f32 + delta as f32, max_scroll);
        let target_scroll_y = target.round() as u32;
        if target_scroll_y != current_scroll_y && !targets.contains(&target_scroll_y) {
            targets.push(target_scroll_y);
        }
    }
    targets
}

pub(crate) fn scroll_viewport_caches_cover_targets(
    caches: &[ScrollViewportCache],
    targets: &[u32],
    bitmap_height: u32,
) -> bool {
    targets.iter().all(|target| {
        caches
            .iter()
            .any(|cache| cache.scroll_y == *target && cache.bitmap_height == bitmap_height)
    })
}

pub(crate) fn scroll_retained_tag_for_scroll_y(
    scroll_y: u32,
) -> silksurf_gui::WinitRetainedBufferTag {
    silksurf_gui::WinitRetainedBufferTag::new(
        SCROLL_VIEWPORT_RETAINED_TAG_BASE + u64::from(scroll_y),
    )
}

pub(crate) fn handle_browser_retained_buffer_prepared(
    state: &Rc<RefCell<BrowserState>>,
    tag: silksurf_gui::WinitRetainedBufferTag,
) {
    let mut state = state.borrow_mut();
    if tag == FOCUS_VIEWPORT_RETAINED_TAG {
        state.frame.focus_viewport_retained_sent = true;
        return;
    }
    if tag == CURRENT_VIEW_RETAINED_TAG {
        state.frame.current_view_retained_sent = true;
        return;
    }
    if tag == NAVIGATION_START_RETAINED_TAG {
        state.frame.navigation_start_retained_sent = true;
        return;
    }
    if let Some(cache) = state
        .frame
        .scroll_viewport_caches
        .iter_mut()
        .find(|cache| cache.tag == tag)
    {
        cache.retained_sent = true;
    }
}

pub(crate) fn surface_pixel_count(width: u32, height: u32) -> Option<usize> {
    usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)
}

pub(crate) fn handle_browser_presented_frame(
    state: &Rc<RefCell<BrowserState>>,
    last_width: &Cell<u32>,
    last_height: &Cell<u32>,
    frame: silksurf_gui::WinitPresentedFrame,
) {
    let Some(retained_tag) = frame.retained_tag else {
        return;
    };
    let mut state = state.borrow_mut();
    let retained_matches = state
        .retained_present
        .is_some_and(|retained| retained.tag == retained_tag);
    if !retained_matches {
        return;
    }
    if retained_tag == CURRENT_VIEW_RETAINED_TAG {
        state.frame.current_view_retained_sent = false;
    }
    state.retained_present = None;
    state.redraw_mode = BrowserRedrawMode::Clean;
    last_width.set(frame.width);
    last_height.set(frame.height);
}

pub(crate) fn handle_browser_wake(
    state_ref: &Rc<RefCell<BrowserState>>,
    navigation_rx: &Rc<RefCell<Option<mpsc::Receiver<NavigationMessage>>>>,
    scroll: &Cell<f32>,
    live_window_height: u32,
) -> bool {
    let result = navigation_rx
        .borrow_mut()
        .as_ref()
        .and_then(|rx| rx.try_recv().ok());
    let mut state = state_ref.borrow_mut();
    if let Some(result) = result {
        *navigation_rx.borrow_mut() = None;
        return apply_navigation_result(&mut state, result, scroll, live_window_height);
    }
    tick_browser_runtime(&mut state)
}

pub(crate) fn apply_navigation_result(
    state: &mut BrowserState,
    result: NavigationMessage,
    scroll: &Cell<f32>,
    live_window_height: u32,
) -> bool {
    let (generation, result) = result;
    if generation != state.navigation_generation {
        return false;
    }
    state.navigation_pending = false;
    match result {
        Ok(payload) => apply_navigation_payload(state, payload, scroll, live_window_height),
        Err(message) => {
            eprintln!("[SilkSurf] Navigation error: {message}");
            mark_navigation_error(state);
            true
        }
    }
}

pub(crate) fn apply_navigation_payload(
    state: &mut BrowserState,
    payload: BrowserPagePayload,
    scroll: &Cell<f32>,
    live_window_height: u32,
) -> bool {
    let render_config = payload.render_config.clone();
    let buffers = take_browser_frame_buffers(state);
    let live_window_height = (live_window_height > 0).then_some(live_window_height);
    match build_browser_page_with_buffers_for_height(payload, buffers, live_window_height) {
        Ok(page) => {
            eprintln!("[SilkSurf] Navigation complete: {}", page.frame.url);
            let modulepreload_urls = runtime_module_warm_urls(&page.runtime, &page.frame.url);
            let loaded_url = page.frame.url.clone();
            apply_history_success(state, loaded_url.as_str());
            state.frame = page.frame;
            state.runtime = Some(page.runtime);
            state.address_text = loaded_url;
            state.address_editing = false;
            state.address_select_all = false;
            clear_page_input_focus(state);
            set_browser_status(state, "ready");
            mark_redraw(state, BrowserRedrawMode::Full);
            scroll.set(0.0);
            preload_module_scripts(&modulepreload_urls, &render_config);
        }
        Err(err) => {
            let message = err.message;
            restore_browser_frame_buffers(state, err.buffers);
            eprintln!("[SilkSurf] Navigation render error: {message}");
            mark_navigation_error(state);
        }
    }
    true
}

pub(crate) fn runtime_module_warm_urls(
    runtime: &BrowserPageRuntime,
    base_url: &str,
) -> Vec<String> {
    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    extract_module_warm_urls(&dom, runtime.document, base_url)
}

pub(crate) fn take_browser_frame_buffers(state: &mut BrowserState) -> BrowserFrameBuffers {
    BrowserFrameBuffers {
        rgba: state
            .runtime
            .as_mut()
            .map(|runtime| std::mem::take(&mut runtime.rgba))
            .unwrap_or_default(),
        argb: std::mem::take(&mut state.frame.argb),
    }
}

pub(crate) fn restore_browser_frame_buffers(
    state: &mut BrowserState,
    buffers: BrowserFrameBuffers,
) {
    state.frame.argb = buffers.argb;
    if let Some(runtime) = state.runtime.as_mut() {
        runtime.rgba = buffers.rgba;
    }
}

pub(crate) fn mark_navigation_error(state: &mut BrowserState) {
    state.pending_history = None;
    set_browser_status(state, "error");
    mark_redraw(state, BrowserRedrawMode::Chrome);
}

// Geometry tests assert exact pixel-aligned f32 coordinates produced by
// exact float arithmetic; approximate comparison would weaken them.
#[allow(clippy::float_cmp)]
#[cfg(test)]
mod tests {
    // Module split from the former single-file binary; the crate root
    // re-exports every module so sibling items resolve by bare name.
    #[allow(clippy::wildcard_imports)]
    use crate::*;
    use silksurf_render::DisplayItem;

    #[test]
    fn browser_frame_blit_keeps_chrome_fixed_while_scrolling() {
        let frame_width = 3;
        let frame_height = 5;
        let chrome_height = 1;
        let frame = vec![
            10, 10, 10, // chrome
            20, 20, 20, // content row 0
            30, 30, 30, // content row 1
            40, 40, 40, // content row 2
            50, 50, 50, // content row 3
        ];
        let mut pixels = vec![0; 3 * 3];

        blit_browser_frame(
            &frame,
            frame_width,
            frame_height,
            chrome_height,
            2,
            3,
            3,
            &mut pixels,
        );

        assert_eq!(
            pixels,
            vec![
                10, 10, 10, // chrome remains pinned
                40, 40, 40, // content starts at chrome + scroll
                50, 50, 50,
            ]
        );
    }

    #[test]
    fn browser_frame_blit_fills_uncovered_window_margin() {
        let frame = vec![0xFF00_0001; 2 * 2];
        let mut pixels = vec![0x1234_5678; 3 * 3];

        blit_browser_frame(&frame, 2, 2, 0, 0, 3, 3, &mut pixels);

        assert_eq!(pixels[0], 0xFF00_0001);
        assert_eq!(pixels[1], 0xFF00_0001);
        assert_eq!(pixels[2], 0xFFFF_FFFF);
        assert_eq!(pixels[3], 0xFF00_0001);
        assert_eq!(pixels[4], 0xFF00_0001);
        assert_eq!(pixels[5], 0xFFFF_FFFF);
        assert_eq!(pixels[6], 0xFFFF_FFFF);
        assert_eq!(pixels[7], 0xFFFF_FFFF);
        assert_eq!(pixels[8], 0xFFFF_FFFF);
    }

    #[test]
    fn browser_frame_blit_copies_same_size_frame_contiguously() {
        let frame = vec![
            0xFF00_0001,
            0xFF00_0002,
            0xFF00_0003,
            0xFF00_0004,
            0xFF00_0005,
            0xFF00_0006,
        ];
        let mut pixels = vec![0xFFFF_FFFF; frame.len()];

        blit_browser_frame(&frame, 3, 2, 1, 0, 3, 2, &mut pixels);

        assert_eq!(pixels, frame);
    }

    #[test]
    fn browser_frame_height_allows_short_pages() {
        let items = vec![DisplayItem::SolidColor {
            rect: Rect {
                x: 0.0,
                y: BROWSER_CHROME_HEIGHT,
                width: 320.0,
                height: 80.0,
            },
            color: silksurf_css::Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        }];

        assert_eq!(
            browser_frame_height(&items, BROWSER_CHROME_HEIGHT as u32),
            124
        );
    }

    #[test]
    fn initial_browser_window_height_is_bounded() {
        assert_eq!(initial_browser_window_height(44), MIN_INITIAL_WINDOW_HEIGHT);
        assert_eq!(initial_browser_window_height(320), 320);
        assert_eq!(initial_browser_window_height(640), 640);
        assert_eq!(initial_browser_window_height(1200), FRAME_HEIGHT);
    }

    #[test]
    fn window_size_repaint_policy_skips_clean_shrinks() {
        assert!(window_size_exposes_unpainted_area(0, 0, 1280, 320));
        assert!(window_size_exposes_unpainted_area(1280, 320, 1281, 320));
        assert!(window_size_exposes_unpainted_area(1280, 320, 1280, 321));
        assert!(!window_size_exposes_unpainted_area(1280, 320, 1280, 319));
        assert!(!window_size_exposes_unpainted_area(1280, 320, 1279, 319));
    }

    #[test]
    fn address_chrome_present_damage_tracks_address_rect() {
        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::AddressChrome,
                320,
                BROWSER_CHROME_HEIGHT as u32,
                0,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: ADDRESS_BAR_X,
                y: ADDRESS_BAR_Y,
                width: ADDRESS_BAR_WIDTH,
                height: ADDRESS_BAR_HEIGHT,
            })
        );
    }

    #[test]
    fn address_focus_present_damage_tracks_address_rect() {
        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::AddressFocusChrome,
                320,
                BROWSER_CHROME_HEIGHT as u32,
                0,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: ADDRESS_BAR_X,
                y: ADDRESS_BAR_Y,
                width: ADDRESS_BAR_WIDTH,
                height: ADDRESS_BAR_HEIGHT,
            })
        );
    }

    #[test]
    fn address_text_present_damage_tracks_text_strip() {
        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::AddressTextChrome,
                320,
                BROWSER_CHROME_HEIGHT as u32,
                0,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: ADDRESS_BAR_X + 10,
                y: ADDRESS_BAR_Y + 7,
                width: ADDRESS_BAR_WIDTH - 22,
                height: ADDRESS_BAR_HEIGHT - 14,
            })
        );
    }

    #[test]
    fn address_full_text_present_damage_tracks_text_strip() {
        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::AddressFullTextChrome,
                320,
                BROWSER_CHROME_HEIGHT as u32,
                0,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: ADDRESS_BAR_X + 10,
                y: ADDRESS_BAR_Y + 7,
                width: ADDRESS_BAR_WIDTH - 22,
                height: ADDRESS_BAR_HEIGHT - 14,
            })
        );
    }

    #[test]
    fn status_present_damage_tracks_status_rect() {
        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::StatusChrome,
                320,
                BROWSER_CHROME_HEIGHT as u32,
                0,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: 1010,
                y: 14,
                width: 160,
                height: 7,
            })
        );
    }

    #[test]
    fn navigation_start_present_damage_tracks_active_chrome_parts() {
        let damage = browser_present_damage(
            BrowserRedrawMode::NavigationStartChrome,
            320,
            BROWSER_CHROME_HEIGHT as u32,
            0,
            1280,
            320,
        );
        let silksurf_gui::WinitPresentDamage::Rects(rects) = damage else {
            panic!("navigation start should present disjoint damage rects");
        };

        assert_eq!(
            rects.as_slice(),
            &[
                silksurf_gui::WinitDamageRect {
                    x: RELOAD_BUTTON_X,
                    y: NAV_BUTTON_Y,
                    width: NAV_BUTTON_WIDTH,
                    height: NAV_BUTTON_HEIGHT,
                },
                silksurf_gui::WinitDamageRect {
                    x: STOP_BUTTON_X,
                    y: NAV_BUTTON_Y,
                    width: NAV_BUTTON_WIDTH,
                    height: NAV_BUTTON_HEIGHT,
                },
                silksurf_gui::WinitDamageRect {
                    x: 1010,
                    y: 14,
                    width: 160,
                    height: 7,
                },
            ]
        );
    }

    #[test]
    fn content_present_damage_maps_scrolled_frame_rect_to_window_rect() {
        let damage = Rect {
            x: 12.0,
            y: BROWSER_CHROME_HEIGHT + 100.0,
            width: 30.0,
            height: 12.0,
        };

        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::Damage(damage),
                400,
                BROWSER_CHROME_HEIGHT as u32,
                80,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: 12,
                y: BROWSER_CHROME_HEIGHT as u32 + 20,
                width: 30,
                height: 12,
            })
        );
    }

    #[test]
    fn page_input_focus_present_damage_maps_scrolled_rect() {
        let damage = Rect {
            x: 12.0,
            y: BROWSER_CHROME_HEIGHT + 100.0,
            width: 30.0,
            height: 12.0,
        };

        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::PageInputFocus(damage),
                400,
                BROWSER_CHROME_HEIGHT as u32,
                80,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: 12,
                y: BROWSER_CHROME_HEIGHT as u32 + 20,
                width: 30,
                height: 12,
            })
        );
    }

    #[test]
    fn content_damage_with_chrome_unions_present_rects() {
        let damage = Rect {
            x: 12.0,
            y: BROWSER_CHROME_HEIGHT + 100.0,
            width: 30.0,
            height: 12.0,
        };

        assert_eq!(
            browser_present_damage(
                BrowserRedrawMode::DamageWithChrome(damage),
                400,
                BROWSER_CHROME_HEIGHT as u32,
                80,
                1280,
                320,
            ),
            silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                x: 0,
                y: 0,
                width: 1280,
                height: BROWSER_CHROME_HEIGHT as u32 + 32,
            })
        );
    }

    #[test]
    fn age_zero_partial_redraw_seeds_full_buffer() {
        let damage = Rect {
            x: 12.0,
            y: BROWSER_CHROME_HEIGHT,
            width: 30.0,
            height: 12.0,
        };

        assert!(browser_render_seeds_full_buffer(
            BrowserRedrawMode::AddressChrome,
            0
        ));
        assert!(browser_render_seeds_full_buffer(
            BrowserRedrawMode::AddressFocusChrome,
            0
        ));
        assert!(browser_render_seeds_full_buffer(
            BrowserRedrawMode::AddressTextChrome,
            0
        ));
        assert!(browser_render_seeds_full_buffer(
            BrowserRedrawMode::Chrome,
            0
        ));
        assert!(browser_render_seeds_full_buffer(
            BrowserRedrawMode::Damage(damage),
            0
        ));
        assert!(browser_render_seeds_full_buffer(
            BrowserRedrawMode::PageInputFocus(damage),
            0
        ));
        assert!(!browser_render_seeds_full_buffer(
            BrowserRedrawMode::AddressChrome,
            1
        ));
        assert!(!browser_render_seeds_full_buffer(
            BrowserRedrawMode::AddressFocusChrome,
            1
        ));
        assert!(!browser_render_seeds_full_buffer(
            BrowserRedrawMode::Full,
            0
        ));
        assert!(!browser_render_seeds_full_buffer(
            BrowserRedrawMode::Clean,
            0
        ));
    }

    #[test]
    fn scroll_offset_clamps_to_content_range() {
        assert_eq!(clamp_scroll_offset(-12.0, 100.0), 0.0);
        assert_eq!(clamp_scroll_offset(125.0, 100.0), 100.0);
        assert_eq!(clamp_scroll_offset(f32::NAN, 100.0), 0.0);
        assert_eq!(max_browser_scroll_offset(1200, 800, 44), 400.0);
    }

    #[test]
    fn scroll_exposed_document_rect_tracks_direction() {
        let down = scroll_exposed_document_rect(48, 100, 4);
        assert_eq!(down.y, 144.0);
        assert_eq!(down.height, 4.0);

        let up = scroll_exposed_document_rect(40, 100, -4);
        assert_eq!(up.y, 84.0);
        assert_eq!(up.height, 4.0);
    }

    #[test]
    fn scroll_reuse_only_handles_small_deltas() {
        assert!(!scroll_reuse_is_profitable(756, 0));
        assert!(scroll_reuse_is_profitable(756, 96));
        assert!(!scroll_reuse_is_profitable(756, 682));
        assert!(!scroll_reuse_is_profitable(756, 756));
    }

    #[test]
    fn scratch_damage_pack_matches_retained_rgba_pack() {
        let display_list = silksurf_render::DisplayList {
            items: vec![silksurf_render::DisplayItem::SolidColor {
                rect: Rect {
                    x: 1.0,
                    y: 1.0,
                    width: 2.0,
                    height: 2.0,
                },
                color: silksurf_css::Color {
                    r: 220,
                    g: 0,
                    b: 0,
                    a: 255,
                },
            }],
            tiles: None,
        };
        let damage = Rect {
            x: 1.0,
            y: 1.0,
            width: 2.0,
            height: 2.0,
        };
        let mut rgba = vec![255; 4 * 4 * 4];
        let mut scratch = silksurf_render::DamageScratch::default();
        silksurf_render::rasterize_skia_damage_into(
            &display_list,
            4,
            4,
            damage,
            &mut rgba,
            &mut scratch,
        );
        let mut from_rgba = vec![0xffff_ffff; 16];
        let mut from_scratch = from_rgba.clone();

        sync_argb_damage_from_rgba(&rgba, &mut from_rgba, 4, 4, damage);
        assert!(sync_argb_damage_from_scratch(
            &scratch,
            &mut from_scratch,
            4
        ));

        assert_eq!(from_scratch, from_rgba);
    }

    #[test]
    fn viewport_source_items_use_tile_index_without_duplicates() {
        let display_list = silksurf_render::DisplayList {
            items: vec![
                solid_item(0.0, 44.0, 1280.0, 200.0),
                solid_item(10.0, 260.0, 20.0, 20.0),
                solid_item(10.0, 1200.0, 20.0, 20.0),
            ],
            tiles: None,
        }
        .with_tiles(FRAME_WIDTH, 1400, DOCUMENT_TILE_SIZE);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: 756.0,
        };

        let mut item_indices = Vec::new();
        browser_viewport_source_item_indices(&display_list, viewport, &mut item_indices);

        assert_eq!(item_indices.len(), 2);
        assert_eq!(
            display_item_rect(&display_list.items[item_indices[0]]).y,
            44.0
        );
        assert_eq!(
            display_item_rect(&display_list.items[item_indices[1]]).y,
            260.0
        );
    }

    #[test]
    fn focus_viewport_cache_renders_on_demand() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: concat!(
                "<!doctype html><html><body>",
                "<div style=\"height:1200px\"></div>",
                "<input id=\"q\">",
                "</body></html>"
            )
            .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let scroll_y = first_focus_target_scroll(
            &page.frame.input_targets,
            page.frame.raster_height,
            page.frame.bitmap_height,
            BROWSER_CHROME_HEIGHT as u32,
        )
        .expect("offscreen input scroll exists");
        let mut state = test_browser_state_from_page(page);

        prepare_focus_viewport_cache(&mut state, scroll_y, FRAME_HEIGHT);

        let cache = state
            .frame
            .focus_viewport_cache
            .as_ref()
            .expect("cache renders");
        assert_eq!(cache.scroll_y, scroll_y);
        assert_eq!(cache.bitmap_height, FRAME_HEIGHT);
        assert_eq!(cache.argb.len(), (FRAME_WIDTH * FRAME_HEIGHT) as usize);
        assert!(!state.frame.focus_viewport_retained_sent);
        assert!(apply_focus_viewport_cache(
            &mut state,
            scroll_y,
            FRAME_HEIGHT
        ));
        assert_eq!(state.frame.bitmap_scroll_y, scroll_y);
        assert!(state.frame.focus_viewport_cache.is_none());
    }

    #[test]
    fn focus_viewport_cache_redraw_marks_visible_content_damage() {
        assert_eq!(
            focus_viewport_cache_redraw_mode(682, 800),
            BrowserRedrawMode::Damage(Rect {
                x: 0.0,
                y: BROWSER_CHROME_HEIGHT + 682.0,
                width: FRAME_WIDTH as f32,
                height: 756.0,
            })
        );
    }

    #[test]
    fn apply_focus_viewport_cache_swaps_cached_pixels_once() {
        let mut state = test_browser_state("https://example.com/");
        state.frame.focus_viewport_cache = Some(FocusViewportCache {
            scroll_y: 682,
            bitmap_height: 800,
            argb: vec![0x0102_0304, 0x0506_0708],
        });

        assert!(apply_focus_viewport_cache(&mut state, 682, 800));
        assert_eq!(state.frame.argb, vec![0x0102_0304, 0x0506_0708]);
        assert_eq!(state.frame.bitmap_scroll_y, 682);
        assert_eq!(state.frame.bitmap_height, 800);
        assert!(state.frame.focus_viewport_cache.is_none());
        assert!(!apply_focus_viewport_cache(&mut state, 682, 800));
    }

    #[test]
    fn focus_retained_buffer_update_sends_cache_once() {
        let mut state = test_browser_state("https://example.com/");
        state.frame.focus_viewport_cache = Some(FocusViewportCache {
            scroll_y: 682,
            bitmap_height: FRAME_HEIGHT,
            argb: vec![0x0102_0304; (FRAME_WIDTH * FRAME_HEIGHT) as usize],
        });
        let state = Rc::new(RefCell::new(state));

        let update =
            browser_retained_buffer_update(&state, FRAME_WIDTH, FRAME_HEIGHT).expect("cache sends");

        assert_eq!(update.tag, FOCUS_VIEWPORT_RETAINED_TAG);
        assert_eq!(update.width, FRAME_WIDTH);
        assert_eq!(update.height, FRAME_HEIGHT);
        assert_eq!(update.pixels.len(), (FRAME_WIDTH * FRAME_HEIGHT) as usize);
        assert!(!state.borrow().frame.focus_viewport_retained_sent);
        handle_browser_retained_buffer_prepared(&state, update.tag);
        assert!(state.borrow().frame.focus_viewport_retained_sent);
        assert!(browser_retained_buffer_update(&state, FRAME_WIDTH, FRAME_HEIGHT).is_none());
    }

    #[test]
    fn current_view_retained_buffer_update_feeds_page_focus() {
        let input_node = silksurf_dom::NodeId::from_raw(10);
        let mut state = test_browser_state("https://example.com/");
        let rect = Rect {
            x: 32.0,
            y: 443.0,
            width: 320.0,
            height: 22.0,
        };
        state.frame.argb = vec![0x0102_0304; (FRAME_WIDTH * FRAME_HEIGHT) as usize];
        state.frame.input_targets.push(InputTarget {
            rect,
            node: input_node,
        });
        let state = Rc::new(RefCell::new(state));

        let update = browser_retained_buffer_update(&state, FRAME_WIDTH, FRAME_HEIGHT)
            .expect("current view sends");

        assert_eq!(update.tag, CURRENT_VIEW_RETAINED_TAG);
        assert!(!state.borrow().frame.current_view_retained_sent);
        handle_browser_retained_buffer_prepared(&state, update.tag);
        assert!(state.borrow().frame.current_view_retained_sent);

        {
            let mut state = state.borrow_mut();
            assert!(focus_page_input(&mut state, input_node));
            assert_eq!(
                state.retained_present,
                Some(BrowserRetainedPresent {
                    tag: CURRENT_VIEW_RETAINED_TAG,
                    damage: silksurf_gui::WinitPresentDamage::Rect(silksurf_gui::WinitDamageRect {
                        x: 32,
                        y: 443,
                        width: 320,
                        height: 22,
                    },),
                })
            );
        }
    }

    #[test]
    fn navigation_start_retained_buffer_update_prepaints_loading_chrome() {
        let mut state = test_browser_state("https://example.com/");
        state.frame.argb = vec![0; (FRAME_WIDTH * FRAME_HEIGHT) as usize];
        state.frame.current_view_retained_sent = true;
        let state = Rc::new(RefCell::new(state));

        let update = browser_retained_buffer_update(&state, FRAME_WIDTH, FRAME_HEIGHT)
            .expect("navigation start sends");

        assert_eq!(update.tag, NAVIGATION_START_RETAINED_TAG);
        assert!(!state.borrow().frame.navigation_start_retained_sent);
        handle_browser_retained_buffer_prepared(&state, update.tag);
        assert!(state.borrow().frame.navigation_start_retained_sent);
        assert_ne!(
            update.pixels[NAV_BUTTON_Y as usize * FRAME_WIDTH as usize + RELOAD_BUTTON_X as usize],
            0
        );
        assert_ne!(
            update.pixels[14 * FRAME_WIDTH as usize + 1010],
            0,
            "loading status band should be prepainted"
        );
    }

    #[test]
    fn scroll_retained_targets_cover_wheel_probe_deltas() {
        assert_eq!(scroll_retained_targets(200, 1_000.0), vec![296, 152]);
        assert_eq!(scroll_retained_targets(0, 1_000.0), vec![96]);
    }

    #[test]
    fn scroll_retained_buffer_update_sends_cache_once() {
        let mut state = test_browser_state("https://example.com/");
        let tag = scroll_retained_tag_for_scroll_y(96);
        state
            .frame
            .scroll_viewport_caches
            .push(ScrollViewportCache {
                scroll_y: 96,
                bitmap_height: FRAME_HEIGHT,
                tag,
                argb: vec![0x0102_0304; (FRAME_WIDTH * FRAME_HEIGHT) as usize],
                retained_sent: false,
            });

        let update = take_scroll_retained_buffer_update(&mut state, FRAME_WIDTH, FRAME_HEIGHT)
            .expect("cache sends");

        assert_eq!(update.tag, tag);
        assert_eq!(update.width, FRAME_WIDTH);
        assert_eq!(update.height, FRAME_HEIGHT);
        assert_eq!(update.pixels.len(), (FRAME_WIDTH * FRAME_HEIGHT) as usize);
        assert!(!state.frame.scroll_viewport_caches[0].retained_sent);
        let state = Rc::new(RefCell::new(state));
        handle_browser_retained_buffer_prepared(&state, update.tag);
        assert!(state.borrow().frame.scroll_viewport_caches[0].retained_sent);
        assert!(browser_retained_buffer_update(&state, FRAME_WIDTH, FRAME_HEIGHT).is_none());
    }

    #[test]
    fn scroll_viewport_cache_apply_sets_retained_present_state() {
        let mut state = test_browser_state("https://example.com/");
        let tag = scroll_retained_tag_for_scroll_y(96);
        state
            .frame
            .scroll_viewport_caches
            .push(ScrollViewportCache {
                scroll_y: 96,
                bitmap_height: FRAME_HEIGHT,
                tag,
                argb: vec![0x0102_0304, 0x0506_0708],
                retained_sent: true,
            });

        let retained = apply_scroll_viewport_cache(
            &mut state,
            96,
            FRAME_HEIGHT,
            BROWSER_CHROME_HEIGHT as u32,
            FRAME_WIDTH,
            FRAME_HEIGHT,
        )
        .expect("retained present applies");

        assert_eq!(retained.tag, tag);
        assert_eq!(state.frame.argb, vec![0x0102_0304, 0x0506_0708]);
        assert_eq!(state.frame.bitmap_scroll_y, 96);
        assert_eq!(state.frame.bitmap_height, FRAME_HEIGHT);
        assert!(state.frame.scroll_viewport_caches.is_empty());
    }

    #[test]
    fn mark_redraw_clears_scroll_viewport_caches() {
        let mut state = test_browser_state("https://example.com/");
        state
            .frame
            .scroll_viewport_caches
            .push(ScrollViewportCache {
                scroll_y: 96,
                bitmap_height: FRAME_HEIGHT,
                tag: scroll_retained_tag_for_scroll_y(96),
                argb: Vec::new(),
                retained_sent: true,
            });

        mark_redraw(&mut state, BrowserRedrawMode::Chrome);

        assert!(state.frame.scroll_viewport_caches.is_empty());
    }

    #[test]
    fn retained_present_action_clears_after_matching_present() {
        let state = Rc::new(RefCell::new(test_browser_state("https://example.com/")));
        let last_width = Cell::new(FRAME_WIDTH);
        let last_height = Cell::new(FRAME_HEIGHT);
        let damage = silksurf_gui::WinitPresentDamage::rect(0, 44, FRAME_WIDTH, 756);
        {
            let mut state = state.borrow_mut();
            state.redraw_mode = BrowserRedrawMode::Damage(Rect {
                x: 0.0,
                y: BROWSER_CHROME_HEIGHT + 682.0,
                width: FRAME_WIDTH as f32,
                height: 756.0,
            });
            state.retained_present = Some(BrowserRetainedPresent {
                tag: FOCUS_VIEWPORT_RETAINED_TAG,
                damage,
            });
        }

        assert_eq!(
            browser_render_action(&state, &last_width, &last_height, FRAME_WIDTH, FRAME_HEIGHT),
            silksurf_gui::WinitRenderAction::Retained {
                tag: FOCUS_VIEWPORT_RETAINED_TAG,
                damage,
            }
        );

        handle_browser_presented_frame(
            &state,
            &last_width,
            &last_height,
            silksurf_gui::WinitPresentedFrame {
                width: FRAME_WIDTH,
                height: FRAME_HEIGHT,
                damage,
                retained_tag: Some(FOCUS_VIEWPORT_RETAINED_TAG),
            },
        );

        let state = state.borrow();
        assert_eq!(state.redraw_mode, BrowserRedrawMode::Clean);
        assert!(state.retained_present.is_none());
    }

    #[test]
    fn full_redraw_request_survives_later_chrome_request() {
        let mut state = test_browser_state("https://example.com/");

        mark_redraw(&mut state, BrowserRedrawMode::Full);
        mark_redraw(&mut state, BrowserRedrawMode::Chrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::Full);
    }

    #[test]
    fn damage_redraw_tracks_later_chrome_request() {
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Chrome;
        let damage = Rect {
            x: 8.0,
            y: BROWSER_CHROME_HEIGHT + 16.0,
            width: 32.0,
            height: 10.0,
        };

        mark_redraw(&mut state, BrowserRedrawMode::Damage(damage));
        mark_redraw(&mut state, BrowserRedrawMode::Chrome);

        assert_eq!(
            state.redraw_mode,
            BrowserRedrawMode::DamageWithChrome(damage)
        );
    }

    #[test]
    fn clean_redraw_accepts_next_dirty_request() {
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;

        mark_redraw(&mut state, BrowserRedrawMode::StatusChrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::StatusChrome);
    }

    #[test]
    fn navigation_start_redraw_stays_narrow_when_uncombined() {
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;

        mark_redraw(&mut state, BrowserRedrawMode::NavigationStartChrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::NavigationStartChrome);
    }

    #[test]
    fn status_redraw_promotes_when_address_also_changes() {
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;

        mark_redraw(&mut state, BrowserRedrawMode::StatusChrome);
        mark_redraw(&mut state, BrowserRedrawMode::AddressTextChrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::Chrome);
    }

    #[test]
    fn address_chrome_merges_without_status_redraw() {
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;

        mark_redraw(&mut state, BrowserRedrawMode::AddressChrome);
        mark_redraw(&mut state, BrowserRedrawMode::AddressTextChrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::AddressChrome);

        mark_redraw(&mut state, BrowserRedrawMode::Chrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::Chrome);
    }

    #[test]
    fn address_text_chrome_merges_until_larger_chrome_damage() {
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;

        mark_redraw(&mut state, BrowserRedrawMode::AddressTextChrome);
        mark_redraw(&mut state, BrowserRedrawMode::AddressTextChrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::AddressTextChrome);

        mark_redraw(&mut state, BrowserRedrawMode::AddressChrome);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::AddressChrome);
    }

    #[test]
    fn clean_redraw_does_not_downgrade_existing_damage() {
        let mut state = test_browser_state("https://example.com/");
        let damage = Rect {
            x: 8.0,
            y: BROWSER_CHROME_HEIGHT + 16.0,
            width: 32.0,
            height: 10.0,
        };
        state.redraw_mode = BrowserRedrawMode::Damage(damage);

        mark_redraw(&mut state, BrowserRedrawMode::Clean);

        assert_eq!(state.redraw_mode, BrowserRedrawMode::Damage(damage));
    }

    #[test]
    fn browser_frame_damage_blit_copies_visible_scrolled_rect() {
        let frame_width = 8;
        let frame_height = 12;
        let window_width = 8;
        let window_height = 8;
        let chrome_height = 2;
        let mut frame = vec![0_u32; (frame_width * frame_height) as usize];
        for y in 0..frame_height {
            for x in 0..frame_width {
                frame[(y * frame_width + x) as usize] = 0xAA00_0000 | (y << 8) | x;
            }
        }
        let mut pixels = vec![0xFFFF_FFFF; (window_width * window_height) as usize];

        blit_browser_frame_damage(
            &frame,
            frame_width,
            frame_height,
            chrome_height,
            3,
            window_width,
            window_height,
            Rect {
                x: 2.0,
                y: 6.0,
                width: 3.0,
                height: 2.0,
            },
            &mut pixels,
        );

        assert_eq!(pixels[(3 * window_width + 2) as usize], 0xAA00_0302);
        assert_eq!(pixels[(4 * window_width + 4) as usize], 0xAA00_0404);
        assert_eq!(pixels[(2 * window_width + 2) as usize], 0xFFFF_FFFF);
        assert_eq!(pixels[(3 * window_width + 1) as usize], 0xFFFF_FFFF);
        assert_eq!(pixels[(5 * window_width + 2) as usize], 0xFFFF_FFFF);
    }

    #[test]
    fn browser_frame_damage_blit_keeps_scrolled_damage_visible_below_viewport_height() {
        let frame_width = 8;
        let frame_height = 8;
        let window_width = 8;
        let window_height = 8;
        let chrome_height = 2;
        let scroll_y = 6;
        let mut frame = vec![0_u32; (frame_width * frame_height) as usize];
        for y in 0..frame_height {
            for x in 0..frame_width {
                frame[(y * frame_width + x) as usize] = 0xBB00_0000 | (y << 8) | x;
            }
        }
        let mut pixels = vec![0xFFFF_FFFF; (window_width * window_height) as usize];

        blit_browser_frame_damage(
            &frame,
            frame_width,
            frame_height,
            chrome_height,
            scroll_y,
            window_width,
            window_height,
            Rect {
                x: 2.0,
                y: 10.0,
                width: 3.0,
                height: 2.0,
            },
            &mut pixels,
        );

        assert_eq!(pixels[(4 * window_width + 2) as usize], 0xBB00_0402);
        assert_eq!(pixels[(5 * window_width + 4) as usize], 0xBB00_0504);
        assert_eq!(pixels[(3 * window_width + 2) as usize], 0xFFFF_FFFF);
        assert_eq!(pixels[(6 * window_width + 2) as usize], 0xFFFF_FFFF);
    }

    fn solid_item(x: f32, y: f32, width: f32, height: f32) -> DisplayItem {
        DisplayItem::SolidColor {
            rect: Rect {
                x,
                y,
                width,
                height,
            },
            color: silksurf_css::Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        }
    }
}
