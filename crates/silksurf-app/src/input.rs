// Module split from the former single-file binary; the crate root
// re-exports every module so sibling items resolve by bare name.
#[allow(clippy::wildcard_imports)]
use crate::*;

pub(crate) fn handle_browser_input(
    input: silksurf_gui::WinitInput,
    runtime: &BrowserInputRuntime<'_>,
) -> silksurf_gui::WinitInputResult {
    let frame_height = runtime.state.borrow().frame.raster_height;
    let max_scroll =
        max_browser_scroll_offset(frame_height, runtime.window_height, runtime.chrome_height);
    let current = runtime.scroll.get();
    if let silksurf_gui::WinitInput::CursorMoved { x, y } = input {
        let mut state = runtime.state.borrow_mut();
        let cursor = browser_cursor_shape_for_state(&state, runtime.chrome_height, x, y, current);
        let redraw = update_hover_status(&mut state, runtime.chrome_height, x, y, current);
        return silksurf_gui::WinitInputResult {
            redraw,
            cursor: Some(cursor),
        };
    }
    if let Some(changed) = handle_address_caret_input(input, runtime) {
        return changed.into();
    }
    if let Some(next) = browser_scroll_target(
        input,
        current,
        runtime.window_height,
        runtime.chrome_height,
        max_scroll,
    ) {
        return apply_browser_scroll(runtime, current, next, max_scroll).into();
    }
    handle_browser_command_input(input, runtime, current).into()
}

pub(crate) fn browser_scroll_target(
    input: silksurf_gui::WinitInput,
    current: f32,
    window_height: u32,
    chrome_height: u32,
    max_scroll: f32,
) -> Option<f32> {
    let page_delta = ((window_height.saturating_sub(chrome_height)) as f32
        * BROWSER_PAGE_SCROLL_FACTOR)
        .max(BROWSER_WHEEL_LINE_PX);
    match input {
        silksurf_gui::WinitInput::ScrollPixels(delta) => Some(current + delta),
        silksurf_gui::WinitInput::PageDown => Some(current + page_delta),
        silksurf_gui::WinitInput::PageUp => Some(current - page_delta),
        silksurf_gui::WinitInput::Home => Some(0.0),
        silksurf_gui::WinitInput::End => Some(max_scroll),
        _ => None,
    }
}

pub(crate) fn apply_browser_scroll(
    runtime: &BrowserInputRuntime<'_>,
    current: f32,
    next: f32,
    max_scroll: f32,
) -> bool {
    let scroll = clamp_scroll_offset(next, max_scroll);
    if (scroll - current).abs() < 0.5 {
        return false;
    }
    let bitmap_height = runtime.window_height.max(runtime.chrome_height);
    let scroll_y = scroll.round() as u32;
    let mut state = runtime.state.borrow_mut();
    runtime.scroll.set(scroll);
    if let Some(retained) = apply_scroll_viewport_cache(
        &mut state,
        scroll_y,
        bitmap_height,
        runtime.chrome_height,
        runtime.window_width,
        runtime.window_height,
    ) {
        mark_redraw(
            &mut state,
            scroll_viewport_cache_redraw_mode(scroll_y, bitmap_height),
        );
        state.retained_present = Some(retained);
    } else {
        mark_redraw(&mut state, BrowserRedrawMode::Scroll);
    }
    true
}

pub(crate) fn handle_address_caret_input(
    input: silksurf_gui::WinitInput,
    runtime: &BrowserInputRuntime<'_>,
) -> Option<bool> {
    let motion = match input {
        silksurf_gui::WinitInput::MoveCaretLeft => AddressCaretMotion::Backward,
        silksurf_gui::WinitInput::MoveCaretRight => AddressCaretMotion::Forward,
        silksurf_gui::WinitInput::Home => AddressCaretMotion::Start,
        silksurf_gui::WinitInput::End => AddressCaretMotion::End,
        _ => return None,
    };
    let mut state = runtime.state.borrow_mut();
    if !state.address_editing {
        return None;
    }
    if move_address_caret(&mut state, motion) {
        mark_redraw(&mut state, BrowserRedrawMode::AddressFullTextChrome);
        return Some(true);
    }
    Some(false)
}

pub(crate) fn handle_browser_command_input(
    input: silksurf_gui::WinitInput,
    runtime: &BrowserInputRuntime<'_>,
    current_scroll: f32,
) -> bool {
    match input {
        silksurf_gui::WinitInput::PrimaryClick { x, y } => {
            handle_browser_primary_click(runtime, x, y, current_scroll)
        }
        silksurf_gui::WinitInput::FocusAddress => focus_address_input(runtime),
        silksurf_gui::WinitInput::TextInput(ch) => handle_text_input(runtime, ch),
        silksurf_gui::WinitInput::SubmitAddress => submit_address_input(runtime),
        silksurf_gui::WinitInput::Backspace => handle_backspace_input(runtime),
        silksurf_gui::WinitInput::Copy => copy_address_input(runtime),
        silksurf_gui::WinitInput::Paste => paste_clipboard_into_address(runtime),
        silksurf_gui::WinitInput::Cut => cut_address_input(runtime),
        silksurf_gui::WinitInput::FocusNextPageInput => focus_next_page_input_from_runtime(runtime),
        silksurf_gui::WinitInput::MoveCaretLeft
        | silksurf_gui::WinitInput::MoveCaretRight
        | silksurf_gui::WinitInput::Home
        | silksurf_gui::WinitInput::End => false,
        silksurf_gui::WinitInput::Back => navigate_history_back(runtime),
        silksurf_gui::WinitInput::Forward => navigate_history_forward(runtime),
        silksurf_gui::WinitInput::BrowserHome => navigate_home_page(runtime),
        silksurf_gui::WinitInput::Reload => reload_current_page(runtime),
        silksurf_gui::WinitInput::Stop => stop_navigation(&mut runtime.state.borrow_mut()),
        _ => false,
    }
}

pub(crate) fn browser_cursor_shape_for_state(
    state: &BrowserState,
    chrome_height: u32,
    x: f32,
    y: f32,
    current_scroll: f32,
) -> silksurf_gui::WinitCursorShape {
    if browser_address_bar_contains(x, y)
        || hit_test_input(
            &state.frame.input_targets,
            x,
            y,
            current_scroll,
            chrome_height,
        )
        .is_some()
    {
        return silksurf_gui::WinitCursorShape::Text;
    }
    if hit_test_chrome_action(x, y).is_some_and(|action| chrome_action_enabled(state, action)) {
        return silksurf_gui::WinitCursorShape::Pointer;
    }
    if hit_test_link(
        &state.frame.link_targets,
        x,
        y,
        current_scroll,
        chrome_height,
    )
    .is_some()
    {
        return silksurf_gui::WinitCursorShape::Pointer;
    }
    silksurf_gui::WinitCursorShape::Default
}

pub(crate) fn update_hover_status(
    state: &mut BrowserState,
    chrome_height: u32,
    x: f32,
    y: f32,
    current_scroll: f32,
) -> bool {
    trace_link_hit_test(state, x, y, current_scroll);
    let next = (!state.navigation_pending)
        .then(|| {
            hit_test_link(
                &state.frame.link_targets,
                x,
                y,
                current_scroll,
                chrome_height,
            )
            .map(str::to_string)
        })
        .flatten();
    if state.hover_status_text == next {
        return false;
    }
    state.hover_status_text = next;
    mark_redraw(state, BrowserRedrawMode::StatusChrome);
    true
}

pub(crate) fn trace_link_hit_test(state: &BrowserState, x: f32, y: f32, scroll_y: f32) {
    if std::env::var_os("SILKSURF_TRACE_INPUT").is_none() {
        return;
    }
    eprintln!(
        "[SilkSurf] link hit-test: cursor=({x:.1},{y:.1}) scroll={scroll_y:.1} links={}",
        state.frame.link_targets.len()
    );
    for target in &state.frame.link_targets {
        eprintln!(
            "[SilkSurf] link target: href={} rect=({}, {}, {}, {})",
            target.href, target.rect.x, target.rect.y, target.rect.width, target.rect.height
        );
    }
}

pub(crate) fn handle_browser_primary_click(
    runtime: &BrowserInputRuntime<'_>,
    x: f32,
    y: f32,
    current_scroll: f32,
) -> bool {
    if let Some(action) = hit_test_chrome_action(x, y) {
        return handle_chrome_click(runtime, action);
    }
    if browser_address_bar_contains(x, y) {
        return focus_address_input(runtime);
    }
    let mut redraw_requested = blur_address_input(runtime);
    if let Some(input_node) = hit_test_page_input(runtime, x, y, current_scroll) {
        let mut state = runtime.state.borrow_mut();
        if activate_page_input_control(&mut state, input_node) {
            return true;
        }
        redraw_requested |= focus_page_input(&mut state, input_node);
        return redraw_requested;
    }
    redraw_requested | follow_hit_link(runtime, x, y, current_scroll)
}

pub(crate) fn handle_chrome_click(
    runtime: &BrowserInputRuntime<'_>,
    action: BrowserChromeAction,
) -> bool {
    let mut state = runtime.state.borrow_mut();
    clear_page_input_focus(&mut state);
    handle_chrome_action(
        &mut state,
        runtime.navigation_rx,
        action,
        runtime.wake_handle,
        runtime.render_config,
        runtime.image_cache,
    )
}

pub(crate) fn blur_address_input(runtime: &BrowserInputRuntime<'_>) -> bool {
    let mut state = runtime.state.borrow_mut();
    if !state.address_editing {
        return false;
    }
    state.address_editing = false;
    state.address_select_all = false;
    mark_redraw(&mut state, BrowserRedrawMode::AddressChrome);
    true
}

pub(crate) fn hit_test_page_input(
    runtime: &BrowserInputRuntime<'_>,
    x: f32,
    y: f32,
    current_scroll: f32,
) -> Option<silksurf_dom::NodeId> {
    let state = runtime.state.borrow();
    let input_node = hit_test_input(
        &state.frame.input_targets,
        x,
        y,
        current_scroll,
        runtime.chrome_height,
    );
    trace_input_hit_test(&state, x, y, current_scroll);
    input_node
}

pub(crate) fn follow_hit_link(
    runtime: &BrowserInputRuntime<'_>,
    x: f32,
    y: f32,
    current_scroll: f32,
) -> bool {
    {
        let mut state = runtime.state.borrow_mut();
        clear_page_input_focus(&mut state);
    }
    let href = {
        let state = runtime.state.borrow();
        hit_test_link(
            &state.frame.link_targets,
            x,
            y,
            current_scroll,
            runtime.chrome_height,
        )
        .map(str::to_string)
    };
    let Some(href) = href else {
        return false;
    };
    let mut state = runtime.state.borrow_mut();
    clear_page_input_focus(&mut state);
    if state.navigation_pending {
        return false;
    }
    eprintln!("[SilkSurf] Navigating: {href}");
    // A link click is page-initiated: the current page is the SameSite
    // initiator, so a cross-site link withholds the destination's Strict cookies.
    let request = BrowserNavigationRequest::get(href).initiated_by(&state.frame.url);
    start_navigation_worker(
        &mut state,
        runtime.navigation_rx,
        request,
        PendingHistoryAction::Push,
        runtime.wake_handle,
        runtime.render_config,
        runtime.image_cache,
    )
}

pub(crate) fn focus_address_input(runtime: &BrowserInputRuntime<'_>) -> bool {
    let mut state = runtime.state.borrow_mut();
    clear_page_input_focus(&mut state);
    let mode = address_focus_redraw_mode(&state);
    let changed = focus_address_bar(&mut state);
    if changed {
        mark_redraw(&mut state, mode);
    }
    changed
}

pub(crate) fn mark_address_edit_redraw(state: &mut BrowserState, full_address_damage: bool) {
    let mode = if full_address_damage {
        BrowserRedrawMode::AddressFullTextChrome
    } else {
        BrowserRedrawMode::AddressTextChrome
    };
    mark_redraw(state, mode);
}

pub(crate) fn handle_text_input(runtime: &BrowserInputRuntime<'_>, ch: char) -> bool {
    let mut state = runtime.state.borrow_mut();
    let full_address_damage = state.address_select_all;
    if push_address_char(&mut state, ch) {
        mark_address_edit_redraw(&mut state, full_address_damage);
        return true;
    }
    push_focused_input_char(&mut state, ch)
}

pub(crate) fn submit_address_input(runtime: &BrowserInputRuntime<'_>) -> bool {
    let target_url = {
        let state = runtime.state.borrow();
        state
            .address_editing
            .then(|| normalize_address_input(&state.address_text))
            .flatten()
    };
    match target_url {
        // Typed into the address bar: browser-initiated, no page initiator.
        Some(target_url) => navigate_address_target(runtime, target_url, None),
        None => {
            let mut state = runtime.state.borrow_mut();
            if state.address_editing {
                set_browser_status(&mut state, "error");
                mark_redraw(&mut state, BrowserRedrawMode::Chrome);
                return true;
            }
            if push_focused_textarea_newline(&mut state) {
                return true;
            }
            drop(state);
            submit_focused_form(runtime)
        }
    }
}

pub(crate) fn submit_focused_form(runtime: &BrowserInputRuntime<'_>) -> bool {
    // A form submission is page-initiated: the submitting page is the SameSite
    // initiator for both GET and POST targets.
    let (target, initiator) = {
        let state = runtime.state.borrow();
        (
            focused_form_submission_target(&state),
            state.frame.url.clone(),
        )
    };
    match target {
        Some(FormSubmissionTarget::Get(target_url)) => {
            navigate_address_target(runtime, target_url, Some(&initiator))
        }
        Some(FormSubmissionTarget::Post(request)) => {
            navigate_form_request(runtime, request, Some(&initiator))
        }
        Some(FormSubmissionTarget::UnsupportedMethod(method)) => {
            let mut state = runtime.state.borrow_mut();
            set_browser_status(&mut state, format!("unsupported form method {method}"));
            mark_redraw(&mut state, BrowserRedrawMode::Chrome);
            true
        }
        None => false,
    }
}

pub(crate) fn navigate_address_target(
    runtime: &BrowserInputRuntime<'_>,
    target_url: String,
    initiator: Option<&str>,
) -> bool {
    let mut state = runtime.state.borrow_mut();
    if state.navigation_pending {
        return false;
    }
    state.address_editing = false;
    state.address_select_all = false;
    clear_page_input_focus(&mut state);
    state.address_text.clone_from(&target_url);
    eprintln!("[SilkSurf] Navigating: {target_url}");
    let mut request = BrowserNavigationRequest::get(target_url);
    if let Some(initiator) = initiator {
        request = request.initiated_by(initiator);
    }
    start_navigation_worker(
        &mut state,
        runtime.navigation_rx,
        request,
        PendingHistoryAction::Push,
        runtime.wake_handle,
        runtime.render_config,
        runtime.image_cache,
    )
}

pub(crate) fn navigate_form_request(
    runtime: &BrowserInputRuntime<'_>,
    request: BrowserNavigationRequest,
    initiator: Option<&str>,
) -> bool {
    let mut state = runtime.state.borrow_mut();
    if state.navigation_pending {
        return false;
    }
    state.address_editing = false;
    state.address_select_all = false;
    clear_page_input_focus(&mut state);
    state.address_text.clone_from(&request.url);
    eprintln!("[SilkSurf] Navigating: {}", request.url);
    let request = match initiator {
        Some(initiator) => request.initiated_by(initiator),
        None => request,
    };
    start_navigation_worker(
        &mut state,
        runtime.navigation_rx,
        request,
        PendingHistoryAction::Push,
        runtime.wake_handle,
        runtime.render_config,
        runtime.image_cache,
    )
}

pub(crate) fn handle_backspace_input(runtime: &BrowserInputRuntime<'_>) -> bool {
    let mut state = runtime.state.borrow_mut();
    if edit_address_backspace(&mut state) {
        mark_address_edit_redraw(&mut state, true);
        return true;
    }
    if edit_focused_input_backspace(&mut state) {
        return true;
    }
    drop(state);
    navigate_history_back(runtime)
}

pub(crate) fn copy_address_input(runtime: &BrowserInputRuntime<'_>) -> bool {
    let state = runtime.state.borrow();
    if let Some(text) = address_clipboard_text(&state)
        && let Err(err) = write_clipboard_text(text)
    {
        eprintln!("[SilkSurf] Clipboard copy failed: {err}");
    }
    false
}

pub(crate) fn paste_clipboard_into_address(runtime: &BrowserInputRuntime<'_>) -> bool {
    let text = match read_clipboard_text() {
        Ok(text) => text,
        Err(err) => {
            eprintln!("[SilkSurf] Clipboard paste failed: {err}");
            return false;
        }
    };
    let mut state = runtime.state.borrow_mut();
    let full_address_damage = state.address_select_all;
    if !paste_address_text(&mut state, text.as_str()) {
        return false;
    }
    mark_address_edit_redraw(&mut state, full_address_damage);
    true
}

pub(crate) fn cut_address_input(runtime: &BrowserInputRuntime<'_>) -> bool {
    let mut state = runtime.state.borrow_mut();
    let copied = address_clipboard_text(&state)
        .map(write_clipboard_text)
        .transpose();
    if let Err(err) = copied {
        eprintln!("[SilkSurf] Clipboard cut failed: {err}");
    }
    if !cut_address_text(&mut state) {
        return false;
    }
    mark_address_edit_redraw(&mut state, true);
    true
}

pub(crate) fn focus_next_page_input_from_runtime(runtime: &BrowserInputRuntime<'_>) -> bool {
    let mut state = runtime.state.borrow_mut();
    let changed = focus_next_visible_page_input(
        &mut state,
        runtime.scroll.get(),
        runtime.chrome_height,
        runtime.window_height,
    );
    if !changed {
        return false;
    }
    let Some(focused) = state.focused_input else {
        return true;
    };
    let Some(target_rect) = state
        .frame
        .input_targets
        .iter()
        .find(|target| target.node == focused)
        .map(|target| target.rect)
    else {
        return true;
    };
    let max_scroll = max_browser_scroll_offset(
        state.frame.raster_height,
        runtime.window_height,
        runtime.chrome_height,
    );
    let next_scroll = scroll_to_show_input_target(
        runtime.scroll.get(),
        target_rect,
        max_scroll,
        runtime.chrome_height,
        runtime.window_height,
    );
    if (next_scroll - runtime.scroll.get()).abs() >= 0.5 {
        runtime.scroll.set(next_scroll);
        let bitmap_height = runtime.window_height.max(runtime.chrome_height);
        let scroll_y = next_scroll.round() as u32;
        let focus_cache_retained = state.frame.focus_viewport_retained_sent;
        prepare_focus_viewport_cache(&mut state, scroll_y, bitmap_height);
        if apply_focus_viewport_cache(&mut state, scroll_y, bitmap_height) {
            let redraw_mode = focus_viewport_cache_redraw_mode(scroll_y, bitmap_height);
            mark_redraw(&mut state, redraw_mode);
            if focus_cache_retained {
                state.retained_present = focus_viewport_retained_present(
                    &state,
                    redraw_mode,
                    runtime.chrome_height,
                    scroll_y,
                    runtime.window_width,
                    runtime.window_height,
                );
            }
        } else {
            mark_redraw(&mut state, BrowserRedrawMode::Scroll);
        }
    }
    true
}

pub(crate) fn focus_viewport_cache_redraw_mode(
    scroll_y: u32,
    bitmap_height: u32,
) -> BrowserRedrawMode {
    BrowserRedrawMode::Damage(scroll_visible_document_rect(scroll_y, bitmap_height))
}

pub(crate) fn focus_viewport_retained_present(
    state: &BrowserState,
    redraw_mode: BrowserRedrawMode,
    chrome_height: u32,
    scroll_y: u32,
    window_width: u32,
    window_height: u32,
) -> Option<BrowserRetainedPresent> {
    let damage = browser_present_damage(
        redraw_mode,
        state.frame.raster_height,
        chrome_height,
        scroll_y,
        window_width,
        window_height,
    );
    (damage != silksurf_gui::WinitPresentDamage::Clean).then_some(BrowserRetainedPresent {
        tag: FOCUS_VIEWPORT_RETAINED_TAG,
        damage,
    })
}

pub(crate) fn apply_focus_viewport_cache(
    state: &mut BrowserState,
    scroll_y: u32,
    bitmap_height: u32,
) -> bool {
    let cache_matches = state
        .frame
        .focus_viewport_cache
        .as_ref()
        .is_some_and(|cache| cache.scroll_y == scroll_y && cache.bitmap_height == bitmap_height);
    if !cache_matches {
        return false;
    }
    let Some(cache) = state.frame.focus_viewport_cache.take() else {
        return false;
    };
    state.frame.argb = cache.argb;
    state.frame.bitmap_scroll_y = cache.scroll_y;
    state.frame.bitmap_height = cache.bitmap_height;
    true
}

pub(crate) fn prepare_focus_viewport_cache(
    state: &mut BrowserState,
    scroll_y: u32,
    bitmap_height: u32,
) {
    if state
        .frame
        .focus_viewport_cache
        .as_ref()
        .is_some_and(|cache| cache.scroll_y == scroll_y && cache.bitmap_height == bitmap_height)
    {
        return;
    }
    let Some(runtime) = state.runtime.as_ref() else {
        return;
    };
    state.frame.focus_viewport_cache = Some(render_focus_viewport_cache(
        &runtime.display_list,
        scroll_y,
        bitmap_height,
    ));
    state.frame.focus_viewport_retained_sent = false;
}

pub(crate) fn scroll_viewport_cache_redraw_mode(
    scroll_y: u32,
    bitmap_height: u32,
) -> BrowserRedrawMode {
    BrowserRedrawMode::Damage(scroll_visible_document_rect(scroll_y, bitmap_height))
}

pub(crate) fn apply_scroll_viewport_cache(
    state: &mut BrowserState,
    scroll_y: u32,
    bitmap_height: u32,
    chrome_height: u32,
    window_width: u32,
    window_height: u32,
) -> Option<BrowserRetainedPresent> {
    let cache_index = state
        .frame
        .scroll_viewport_caches
        .iter()
        .position(|cache| {
            cache.retained_sent
                && cache.scroll_y == scroll_y
                && cache.bitmap_height == bitmap_height
        })?;
    let cache = state.frame.scroll_viewport_caches.swap_remove(cache_index);
    state.frame.argb = cache.argb;
    state.frame.bitmap_scroll_y = cache.scroll_y;
    state.frame.bitmap_height = cache.bitmap_height;
    scroll_viewport_retained_present(
        state,
        cache.tag,
        chrome_height,
        scroll_y,
        bitmap_height,
        window_width,
        window_height,
    )
}

pub(crate) fn scroll_viewport_retained_present(
    state: &BrowserState,
    tag: silksurf_gui::WinitRetainedBufferTag,
    chrome_height: u32,
    scroll_y: u32,
    bitmap_height: u32,
    window_width: u32,
    window_height: u32,
) -> Option<BrowserRetainedPresent> {
    let redraw_mode = scroll_viewport_cache_redraw_mode(scroll_y, bitmap_height);
    let damage = browser_present_damage(
        redraw_mode,
        state.frame.raster_height,
        chrome_height,
        scroll_y,
        window_width,
        window_height,
    );
    (damage != silksurf_gui::WinitPresentDamage::Clean)
        .then_some(BrowserRetainedPresent { tag, damage })
}

pub(crate) fn navigate_history_back(runtime: &BrowserInputRuntime<'_>) -> bool {
    let target = {
        let state = runtime.state.borrow();
        history_back_target(&state)
    };
    navigate_history_target(runtime, target)
}

pub(crate) fn navigate_history_forward(runtime: &BrowserInputRuntime<'_>) -> bool {
    let target = {
        let state = runtime.state.borrow();
        history_forward_target(&state)
    };
    navigate_history_target(runtime, target)
}

pub(crate) fn navigate_history_target(
    runtime: &BrowserInputRuntime<'_>,
    target: Option<(usize, String)>,
) -> bool {
    let Some((target_index, target_url)) = target else {
        return false;
    };
    let mut state = runtime.state.borrow_mut();
    clear_page_input_focus(&mut state);
    start_navigation_worker(
        &mut state,
        runtime.navigation_rx,
        BrowserNavigationRequest::get(target_url),
        PendingHistoryAction::MoveTo(target_index),
        runtime.wake_handle,
        runtime.render_config,
        runtime.image_cache,
    )
}

pub(crate) fn navigate_home_page(runtime: &BrowserInputRuntime<'_>) -> bool {
    let mut state = runtime.state.borrow_mut();
    clear_page_input_focus(&mut state);
    handle_chrome_action(
        &mut state,
        runtime.navigation_rx,
        BrowserChromeAction::Home,
        runtime.wake_handle,
        runtime.render_config,
        runtime.image_cache,
    )
}

pub(crate) fn reload_current_page(runtime: &BrowserInputRuntime<'_>) -> bool {
    let (target_url, history_index) = {
        let state = runtime.state.borrow();
        (state.frame.url.clone(), state.history_index)
    };
    let mut state = runtime.state.borrow_mut();
    clear_page_input_focus(&mut state);
    start_navigation_worker(
        &mut state,
        runtime.navigation_rx,
        BrowserNavigationRequest::get(target_url),
        PendingHistoryAction::MoveTo(history_index),
        runtime.wake_handle,
        runtime.render_config,
        runtime.image_cache,
    )
}

pub(crate) fn handle_chrome_action(
    state: &mut BrowserState,
    navigation_rx: &Rc<RefCell<Option<mpsc::Receiver<NavigationMessage>>>>,
    action: BrowserChromeAction,
    wake_handle: &silksurf_gui::WinitWakeHandle,
    render_config: &BrowserRenderConfig,
    image_cache: &Arc<Mutex<ImageResourceCache>>,
) -> bool {
    if !chrome_action_enabled(state, action) {
        return false;
    }
    state.address_editing = false;
    state.address_select_all = false;
    match action {
        BrowserChromeAction::Back => history_back_target(state).is_some_and(|(index, url)| {
            start_navigation_worker(
                state,
                navigation_rx,
                BrowserNavigationRequest::get(url),
                PendingHistoryAction::MoveTo(index),
                wake_handle,
                render_config,
                image_cache,
            )
        }),
        BrowserChromeAction::Forward => {
            history_forward_target(state).is_some_and(|(index, url)| {
                start_navigation_worker(
                    state,
                    navigation_rx,
                    BrowserNavigationRequest::get(url),
                    PendingHistoryAction::MoveTo(index),
                    wake_handle,
                    render_config,
                    image_cache,
                )
            })
        }
        BrowserChromeAction::Home => start_navigation_worker(
            state,
            navigation_rx,
            BrowserNavigationRequest::get(browser_home_url()),
            PendingHistoryAction::Push,
            wake_handle,
            render_config,
            image_cache,
        ),
        BrowserChromeAction::Reload => start_navigation_worker(
            state,
            navigation_rx,
            BrowserNavigationRequest::get(state.frame.url.clone()),
            PendingHistoryAction::MoveTo(state.history_index),
            wake_handle,
            render_config,
            image_cache,
        ),
        BrowserChromeAction::Stop => stop_navigation(state),
    }
}

pub(crate) fn chrome_action_enabled(state: &BrowserState, action: BrowserChromeAction) -> bool {
    match action {
        BrowserChromeAction::Back => {
            !state.navigation_pending && history_back_target(state).is_some()
        }
        BrowserChromeAction::Forward => {
            !state.navigation_pending && history_forward_target(state).is_some()
        }
        BrowserChromeAction::Home | BrowserChromeAction::Reload => !state.navigation_pending,
        BrowserChromeAction::Stop => state.navigation_pending,
    }
}

pub(crate) fn stop_navigation(state: &mut BrowserState) -> bool {
    if !state.navigation_pending {
        return false;
    }
    state.navigation_generation = state.navigation_generation.saturating_add(1);
    state.navigation_pending = false;
    state.pending_history = None;
    set_browser_status(state, "ready");
    mark_redraw(state, BrowserRedrawMode::Chrome);
    eprintln!("[SilkSurf] Navigation stopped");
    true
}

pub(crate) fn history_back_target(state: &BrowserState) -> Option<(usize, String)> {
    let target_index = state.history_index.checked_sub(1)?;
    let target_url = state.history.get(target_index)?.clone();
    Some((target_index, target_url))
}

pub(crate) fn history_forward_target(state: &BrowserState) -> Option<(usize, String)> {
    let target_index = state.history_index.checked_add(1)?;
    let target_url = state.history.get(target_index)?.clone();
    Some((target_index, target_url))
}

pub(crate) fn apply_history_success(state: &mut BrowserState, loaded_url: &str) {
    match state.pending_history.take() {
        Some(PendingHistoryAction::Push) => {
            let keep = state.history_index.saturating_add(1);
            state.history.truncate(keep);
            if state
                .history
                .last()
                .is_some_and(|current_url| current_url == loaded_url)
            {
                state.history_index = state.history.len().saturating_sub(1);
            } else {
                state.history.push(loaded_url.to_string());
                state.history_index = state.history.len().saturating_sub(1);
            }
        }
        Some(PendingHistoryAction::MoveTo(index)) => {
            if index < state.history.len() {
                state.history_index = index;
            }
        }
        None => {}
    }
}

pub(crate) fn focus_address_bar(state: &mut BrowserState) -> bool {
    let next_text = state.frame.url.clone();
    let changed =
        !state.address_editing || !state.address_select_all || state.address_text != next_text;
    state.address_editing = true;
    state.address_select_all = true;
    state.address_text = next_text;
    state.address_cursor = state.address_text.len();
    changed
}

pub(crate) fn address_focus_redraw_mode(state: &BrowserState) -> BrowserRedrawMode {
    if !state.address_editing || state.address_text == state.frame.url {
        BrowserRedrawMode::AddressFocusChrome
    } else {
        BrowserRedrawMode::AddressChrome
    }
}

pub(crate) fn push_address_char(state: &mut BrowserState, ch: char) -> bool {
    if !state.address_editing || !(ch.is_ascii_graphic() || ch == ' ') {
        return false;
    }
    if state.address_select_all {
        state.address_text.clear();
        state.address_select_all = false;
        state.address_cursor = 0;
    }
    if state.address_text.len() >= ADDRESS_TEXT_MAX_CHARS {
        return false;
    }
    let cursor = clamp_address_cursor(&state.address_text, state.address_cursor);
    state.address_text.insert(cursor, ch);
    state.address_cursor = cursor + ch.len_utf8();
    true
}

pub(crate) fn edit_address_backspace(state: &mut BrowserState) -> bool {
    if !state.address_editing {
        return false;
    }
    if state.address_select_all {
        state.address_text.clear();
        state.address_select_all = false;
        state.address_cursor = 0;
        return true;
    }
    let cursor = clamp_address_cursor(&state.address_text, state.address_cursor);
    let previous = previous_address_cursor(&state.address_text, cursor);
    if previous == cursor {
        state.address_cursor = cursor;
        return false;
    }
    state.address_text.replace_range(previous..cursor, "");
    state.address_cursor = previous;
    true
}

pub(crate) fn address_clipboard_text(state: &BrowserState) -> Option<&str> {
    state
        .address_editing
        .then_some(state.address_text.as_str())
        .filter(|text| !text.is_empty())
}

pub(crate) fn paste_address_text(state: &mut BrowserState, text: &str) -> bool {
    if !state.address_editing {
        return false;
    }
    let mut changed = false;
    if state.address_select_all {
        state.address_text.clear();
        state.address_select_all = false;
        state.address_cursor = 0;
        changed = true;
    }
    for ch in text.chars().filter(|ch| address_paste_char_allowed(*ch)) {
        if state.address_text.len() >= ADDRESS_TEXT_MAX_CHARS {
            break;
        }
        let cursor = clamp_address_cursor(&state.address_text, state.address_cursor);
        state.address_text.insert(cursor, ch);
        state.address_cursor = cursor + ch.len_utf8();
        changed = true;
    }
    changed
}

pub(crate) fn cut_address_text(state: &mut BrowserState) -> bool {
    if !state.address_editing || !state.address_select_all || state.address_text.is_empty() {
        return false;
    }
    state.address_text.clear();
    state.address_select_all = false;
    state.address_cursor = 0;
    true
}

pub(crate) fn address_paste_char_allowed(ch: char) -> bool {
    ch.is_ascii_graphic() || ch == ' '
}

pub(crate) fn move_address_caret(state: &mut BrowserState, motion: AddressCaretMotion) -> bool {
    if !state.address_editing {
        return false;
    }
    let current = if state.address_select_all {
        selected_address_caret(&state.address_text, motion)
    } else {
        let cursor = clamp_address_cursor(&state.address_text, state.address_cursor);
        match motion {
            AddressCaretMotion::Backward => previous_address_cursor(&state.address_text, cursor),
            AddressCaretMotion::Forward => next_address_cursor(&state.address_text, cursor),
            AddressCaretMotion::Start => 0,
            AddressCaretMotion::End => state.address_text.len(),
        }
    };
    let changed = state.address_select_all || state.address_cursor != current;
    state.address_select_all = false;
    state.address_cursor = current;
    changed
}

pub(crate) fn selected_address_caret(text: &str, motion: AddressCaretMotion) -> usize {
    match motion {
        AddressCaretMotion::Backward | AddressCaretMotion::Start => 0,
        AddressCaretMotion::Forward | AddressCaretMotion::End => text.len(),
    }
}

pub(crate) fn clamp_address_cursor(text: &str, cursor: usize) -> usize {
    if cursor >= text.len() {
        return text.len();
    }
    if text.is_char_boundary(cursor) {
        return cursor;
    }
    previous_address_cursor(text, cursor)
}

pub(crate) fn previous_address_cursor(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    text.char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index < cursor)
        .last()
        .unwrap_or(0)
}

pub(crate) fn next_address_cursor(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    text.char_indices()
        .map(|(index, ch)| index + ch.len_utf8())
        .find(|index| *index > cursor)
        .unwrap_or(text.len())
}

pub(crate) fn read_clipboard_text() -> Result<String, arboard::Error> {
    arboard::Clipboard::new()?.get_text()
}

pub(crate) fn write_clipboard_text(text: &str) -> Result<(), arboard::Error> {
    arboard::Clipboard::new()?.set_text(text.to_owned())
}

pub(crate) fn focus_page_input(state: &mut BrowserState, node: silksurf_dom::NodeId) -> bool {
    let changed = state.focused_input != Some(node) || state.address_editing;
    let target_rect = state
        .frame
        .input_targets
        .iter()
        .find(|target| target.node == node)
        .map(|target| target.rect);
    let redraw_mode = if state.address_editing {
        Some(BrowserRedrawMode::AddressChrome)
    } else {
        target_rect.map(BrowserRedrawMode::PageInputFocus)
    };
    state.address_editing = false;
    state.address_select_all = false;
    state.focused_input = Some(node);
    if changed {
        if let Some(redraw_mode) = redraw_mode {
            mark_redraw(state, redraw_mode);
            if let BrowserRedrawMode::PageInputFocus(_) = redraw_mode
                && state.frame.current_view_retained_sent
            {
                let damage = browser_present_damage(
                    redraw_mode,
                    state.frame.raster_height,
                    BROWSER_CHROME_HEIGHT as u32,
                    state.frame.bitmap_scroll_y,
                    FRAME_WIDTH,
                    state.frame.bitmap_height,
                );
                if damage != silksurf_gui::WinitPresentDamage::Clean {
                    state.retained_present = Some(BrowserRetainedPresent {
                        tag: CURRENT_VIEW_RETAINED_TAG,
                        damage,
                    });
                }
            }
        }
        eprintln!("[SilkSurf] Page input focused: node={}", node.raw());
        trace_page_input_focus(node, target_rect);
    }
    changed
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputControlKind {
    Checkbox,
    Radio,
    Select,
}

pub(crate) fn activate_page_input_control(
    state: &mut BrowserState,
    node: silksurf_dom::NodeId,
) -> bool {
    let Some(mut runtime) = state.runtime.take() else {
        return false;
    };
    let edit_result = {
        let mut dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        input_control_kind(&dom, node).and_then(|kind| {
            let changed = dom.with_mutation_batch(|dom| match kind {
                InputControlKind::Checkbox => toggle_checkbox_control(dom, node),
                InputControlKind::Radio => check_radio_control(dom, runtime.document, node),
                InputControlKind::Select => cycle_select_control(dom, node),
            });
            changed.ok().filter(|changed| *changed).map(|_| {
                if kind == InputControlKind::Select {
                    let _ = dom.take_dirty_nodes();
                    return vec![node];
                }
                dom.take_dirty_nodes()
            })
        })
    };
    let Some(dirty_nodes) = edit_result else {
        state.runtime = Some(runtime);
        return false;
    };
    let redraw_mode = repaint_runtime_dirty_nodes(&mut runtime, &mut state.frame, &dirty_nodes);
    state.runtime = Some(runtime);
    state.focused_input = Some(node);
    if let Some(redraw_mode) = redraw_mode {
        mark_redraw(state, redraw_mode);
        eprintln!(
            "[SilkSurf] Page input toggled: node={} dirty_nodes={}",
            node.raw(),
            dirty_nodes.len()
        );
        return true;
    }
    false
}

pub(crate) fn input_control_kind(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<InputControlKind> {
    match input_node_kind(dom, node)? {
        silksurf_dom::TagName::Input => match input_type(dom, node).as_str() {
            "checkbox" => Some(InputControlKind::Checkbox),
            "radio" => Some(InputControlKind::Radio),
            _ => None,
        },
        silksurf_dom::TagName::Select => Some(InputControlKind::Select),
        _ => None,
    }
}

pub(crate) fn cycle_select_control(
    dom: &mut silksurf_dom::Dom,
    select: silksurf_dom::NodeId,
) -> Result<bool, silksurf_dom::DomError> {
    let options = enabled_select_options(dom, select);
    let Some(next_option) = next_select_option(dom, &options) else {
        return Ok(false);
    };
    set_single_selected_option(dom, select, next_option)
}

pub(crate) fn enabled_select_options(
    dom: &silksurf_dom::Dom,
    select: silksurf_dom::NodeId,
) -> Vec<silksurf_dom::NodeId> {
    let mut options = Vec::new();
    collect_enabled_option_nodes(dom, select, &mut options);
    options
}

pub(crate) fn collect_enabled_option_nodes(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    options: &mut Vec<silksurf_dom::NodeId>,
) {
    if node_tag_name(dom, node) == Some(silksurf_dom::TagName::Option)
        && element_attribute(dom, node, "disabled").is_none()
    {
        options.push(node);
    }
    let Ok(children) = dom.children(node) else {
        return;
    };
    for &child in children {
        collect_enabled_option_nodes(dom, child, options);
    }
}

pub(crate) fn next_select_option(
    dom: &silksurf_dom::Dom,
    options: &[silksurf_dom::NodeId],
) -> Option<silksurf_dom::NodeId> {
    if options.is_empty() {
        return None;
    }
    let selected_index = options
        .iter()
        .position(|&option| option_selected(dom, option));
    Some(options[(selected_index.map_or(0, |index| index + 1)) % options.len()])
}

pub(crate) fn set_single_selected_option(
    dom: &mut silksurf_dom::Dom,
    select: silksurf_dom::NodeId,
    selected: silksurf_dom::NodeId,
) -> Result<bool, silksurf_dom::DomError> {
    let mut changed = false;
    for option in enabled_select_options(dom, select) {
        if option == selected {
            if !option_selected(dom, option) {
                dom.set_attribute(option, "selected", "")?;
                changed = true;
            }
        } else if dom.remove_attribute(option, "selected")? {
            changed = true;
        }
    }
    Ok(changed)
}

pub(crate) fn toggle_checkbox_control(
    dom: &mut silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Result<bool, silksurf_dom::DomError> {
    if input_checked(dom, node) {
        dom.remove_attribute(node, "checked")
    } else {
        dom.set_attribute(node, "checked", "")?;
        Ok(true)
    }
}

pub(crate) fn check_radio_control(
    dom: &mut silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    node: silksurf_dom::NodeId,
) -> Result<bool, silksurf_dom::DomError> {
    let mut changed = false;
    if !input_checked(dom, node) {
        dom.set_attribute(node, "checked", "")?;
        changed = true;
    }
    let Some(name) = element_attribute(dom, node, "name").map(str::to_string) else {
        return Ok(changed);
    };
    if name.is_empty() {
        return Ok(changed);
    }
    let group_root = nearest_form_node(dom, node).unwrap_or(root);
    let mut radios = Vec::new();
    collect_radio_group_nodes(dom, group_root, name.as_str(), &mut radios);
    for radio in radios {
        if radio != node && dom.remove_attribute(radio, "checked")? {
            changed = true;
        }
    }
    Ok(changed)
}

pub(crate) fn collect_radio_group_nodes(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    name: &str,
    radios: &mut Vec<silksurf_dom::NodeId>,
) {
    if input_control_kind(dom, node) == Some(InputControlKind::Radio)
        && element_attribute(dom, node, "name").is_some_and(|value| value == name)
    {
        radios.push(node);
    }
    let Ok(children) = dom.children(node) else {
        return;
    };
    for &child in children {
        collect_radio_group_nodes(dom, child, name, radios);
    }
}

pub(crate) fn focus_next_page_input(state: &mut BrowserState) -> bool {
    let Some(next_node) =
        next_text_editable_input_target(state).or_else(|| next_input_target(state))
    else {
        return false;
    };
    focus_page_input(state, next_node)
}

pub(crate) fn next_text_editable_input_target(
    state: &BrowserState,
) -> Option<silksurf_dom::NodeId> {
    let runtime = state.runtime.as_ref()?;
    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut first = None;
    let mut take_next = state.focused_input.is_none();
    for target in &state.frame.input_targets {
        if !is_text_editable_input_node(&dom, target.node) {
            continue;
        }
        first.get_or_insert(target.node);
        if take_next {
            return Some(target.node);
        }
        if state.focused_input == Some(target.node) {
            take_next = true;
        }
    }
    first
}

pub(crate) fn next_input_target(state: &BrowserState) -> Option<silksurf_dom::NodeId> {
    let next_index = state
        .focused_input
        .and_then(|focused| {
            state
                .frame
                .input_targets
                .iter()
                .position(|target| target.node == focused)
        })
        .map_or(0, |index| (index + 1) % state.frame.input_targets.len());
    state
        .frame
        .input_targets
        .get(next_index)
        .map(|target| target.node)
}

pub(crate) fn focus_next_visible_page_input(
    state: &mut BrowserState,
    scroll_y: f32,
    chrome_height: u32,
    window_height: u32,
) -> bool {
    if state.frame.input_targets.is_empty() {
        return false;
    }
    let visible_count = state
        .frame
        .input_targets
        .iter()
        .filter(|target| {
            input_target_intersects_viewport(target, scroll_y, chrome_height, window_height)
        })
        .count();
    if visible_count == 0 {
        return focus_next_page_input(state);
    }
    let next_visible_index = state
        .focused_input
        .and_then(|focused| {
            state
                .frame
                .input_targets
                .iter()
                .filter(|target| {
                    input_target_intersects_viewport(target, scroll_y, chrome_height, window_height)
                })
                .position(|target| target.node == focused)
        })
        .map_or(0, |index| (index + 1) % visible_count);
    let Some(next_node) = state
        .frame
        .input_targets
        .iter()
        .filter(|target| {
            input_target_intersects_viewport(target, scroll_y, chrome_height, window_height)
        })
        .nth(next_visible_index)
        .map(|target| target.node)
    else {
        return false;
    };
    trace_visible_page_input_focus(next_node, scroll_y, chrome_height, window_height);
    focus_page_input(state, next_node)
}

pub(crate) fn input_target_intersects_viewport(
    target: &InputTarget,
    scroll_y: f32,
    chrome_height: u32,
    window_height: u32,
) -> bool {
    let viewport_top = scroll_y + chrome_height as f32;
    let viewport_bottom = scroll_y + window_height as f32;
    let target_top = target.rect.y;
    let target_bottom = target.rect.y + target.rect.height;
    target.rect.width > 0.0
        && target.rect.height > 0.0
        && target_bottom > viewport_top
        && target_top < viewport_bottom
}

pub(crate) fn trace_visible_page_input_focus(
    node: silksurf_dom::NodeId,
    scroll_y: f32,
    chrome_height: u32,
    window_height: u32,
) {
    if std::env::var_os("SILKSURF_TRACE_INPUT").is_none() {
        return;
    }
    eprintln!(
        "[SilkSurf] input focus visible: node={} scroll={} chrome={} height={}",
        node.raw(),
        scroll_y,
        chrome_height,
        window_height
    );
}

pub(crate) fn trace_page_input_focus(node: silksurf_dom::NodeId, rect: Option<Rect>) {
    if std::env::var_os("SILKSURF_TRACE_INPUT").is_none() {
        return;
    }
    if let Some(rect) = rect {
        eprintln!(
            "[SilkSurf] input focus target: node={} rect=({}, {}, {}, {})",
            node.raw(),
            rect.x,
            rect.y,
            rect.width,
            rect.height
        );
    }
}

pub(crate) fn clear_page_input_focus(state: &mut BrowserState) {
    state.focused_input = None;
}

pub(crate) fn push_focused_input_char(state: &mut BrowserState, ch: char) -> bool {
    if !(ch.is_ascii_graphic() || ch == ' ') {
        return false;
    }
    edit_focused_input_value(state, |value| {
        if value.len() >= PAGE_INPUT_TEXT_MAX_CHARS {
            return false;
        }
        value.push(ch);
        true
    })
}

pub(crate) fn push_focused_textarea_newline(state: &mut BrowserState) -> bool {
    let Some(node) = state.focused_input else {
        return false;
    };
    let is_textarea = {
        let Some(runtime) = state.runtime.as_ref() else {
            return false;
        };
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        is_text_content_editable_input_node(&dom, node)
    };
    if !is_textarea {
        return false;
    }
    edit_focused_input_value(state, |value| {
        if value.len() >= PAGE_INPUT_TEXT_MAX_CHARS {
            return false;
        }
        value.push('\n');
        true
    })
}

pub(crate) fn edit_focused_input_backspace(state: &mut BrowserState) -> bool {
    edit_focused_input_value(state, |value| value.pop().is_some())
}

pub(crate) fn edit_focused_input_value(
    state: &mut BrowserState,
    edit: impl FnOnce(&mut String) -> bool,
) -> bool {
    let Some(node) = state.focused_input else {
        return false;
    };
    let Some(mut runtime) = state.runtime.take() else {
        return false;
    };

    let edit_result = {
        let mut dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if is_text_editable_input_node(&dom, node) {
            let mut value = input_value(&dom, node);
            if edit(&mut value) {
                let value_before_edit = input_value(&dom, node);
                let value_after_edit = value.clone();
                let result =
                    dom.with_mutation_batch(|dom| set_editable_input_value(dom, node, value));
                if result.is_err() {
                    None
                } else {
                    Some((dom.take_dirty_nodes(), value_before_edit, value_after_edit))
                }
            } else {
                None
            }
        } else {
            None
        }
    };
    let Some((dirty_nodes, value_before_edit, value_after_edit)) = edit_result else {
        state.runtime = Some(runtime);
        return false;
    };

    let empty_insert_damage =
        focused_empty_insert_damage(&state.frame, node, &value_before_edit, &value_after_edit);
    let redraw_mode =
        repaint_focused_input_value(&mut runtime, &mut state.frame, node, &value_after_edit)
            .or_else(|| {
                repaint_runtime_dirty_nodes(&mut runtime, &mut state.frame, &dirty_nodes)
                    .map(|mode| empty_insert_damage.map_or(mode, BrowserRedrawMode::Damage))
            });
    state.runtime = Some(runtime);
    if let Some(redraw_mode) = redraw_mode {
        mark_redraw(state, redraw_mode);
        eprintln!(
            "[SilkSurf] Page input updated: node={} dirty_nodes={}",
            node.raw(),
            dirty_nodes.len()
        );
        return true;
    }
    false
}

pub(crate) fn input_value(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> String {
    if is_text_content_editable_input_node(dom, node) {
        return textarea_text(dom, node);
    }
    dom.attributes(node)
        .ok()
        .and_then(|attrs| {
            attrs
                .iter()
                .find(|attr| attr.name.as_str() == "value")
                .map(|attr| attr.value.to_string())
        })
        .unwrap_or_default()
}

pub(crate) fn set_editable_input_value(
    dom: &mut silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    value: String,
) -> Result<(), silksurf_dom::DomError> {
    if is_text_content_editable_input_node(dom, node) {
        dom.set_text_content(node, value)
    } else {
        dom.set_attribute(node, "value", value)
    }
}

pub(crate) fn textarea_text(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> String {
    let mut text = String::new();
    append_text_descendants(dom, node, &mut text);
    text
}

pub(crate) fn append_text_descendants(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    text: &mut String,
) {
    let Ok(children) = dom.children(node) else {
        return;
    };
    for &child in children {
        if let Ok(dom_node) = dom.node(child)
            && let silksurf_dom::NodeKind::Text { text: child_text } = dom_node.kind()
        {
            text.push_str(child_text);
            continue;
        }
        append_text_descendants(dom, child, text);
    }
}

pub(crate) fn focused_form_submission_target(state: &BrowserState) -> Option<FormSubmissionTarget> {
    let focused = state.focused_input?;
    let runtime = state.runtime.as_ref()?;
    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if is_textarea_node(&dom, focused) {
        return None;
    }
    let form = nearest_form_node(&dom, focused)?;
    form_submission_target(&dom, form, &state.frame.url)
}

pub(crate) fn form_submission_target(
    dom: &silksurf_dom::Dom,
    form: silksurf_dom::NodeId,
    base_url: &str,
) -> Option<FormSubmissionTarget> {
    let method = element_attribute(dom, form, "method")
        .unwrap_or("get")
        .to_ascii_lowercase();
    let action = element_attribute(dom, form, "action").unwrap_or("");
    let mut target = url::Url::parse(base_url).ok()?.join(action).ok()?;
    let pairs = form_submission_pairs(dom, form);
    match method.as_str() {
        "get" => {
            for (name, value) in pairs {
                target.query_pairs_mut().append_pair(&name, &value);
            }
            browser_supported_url(&target).map(FormSubmissionTarget::Get)
        }
        "post" => {
            let body = encode_form_submission_body(&pairs);
            browser_supported_url(&target).map(|url| {
                FormSubmissionTarget::Post(BrowserNavigationRequest::post_form(
                    url,
                    body.into_bytes(),
                ))
            })
        }
        _ => Some(FormSubmissionTarget::UnsupportedMethod(method)),
    }
}

pub(crate) fn nearest_form_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<silksurf_dom::NodeId> {
    let mut current = Some(node);
    while let Some(id) = current {
        if dom.element_name(id).ok().flatten().is_some_and(|name| {
            silksurf_dom::TagName::from_str(name) == silksurf_dom::TagName::Form
        }) {
            return Some(id);
        }
        current = dom.parent(id).ok().flatten();
    }
    None
}

pub(crate) fn form_submission_pairs(
    dom: &silksurf_dom::Dom,
    form: silksurf_dom::NodeId,
) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    collect_form_submission_pairs(dom, form, &mut pairs);
    pairs
}

pub(crate) fn encode_form_submission_body(pairs: &[(String, String)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in pairs {
        serializer.append_pair(name, value);
    }
    serializer.finish()
}

pub(crate) fn collect_form_submission_pairs(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    pairs: &mut Vec<(String, String)>,
) {
    if let Some(pair) = form_control_submission_pair(dom, node) {
        pairs.push(pair);
    }
    let Ok(children) = dom.children(node) else {
        return;
    };
    for &child in children {
        collect_form_submission_pairs(dom, child, pairs);
    }
}

pub(crate) fn form_control_submission_pair(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<(String, String)> {
    if element_attribute(dom, node, "disabled").is_some() {
        return None;
    }
    let name = element_attribute(dom, node, "name")?.to_string();
    if name.is_empty() {
        return None;
    }
    let element_name = dom.element_name(node).ok().flatten()?;
    match silksurf_dom::TagName::from_str(element_name) {
        silksurf_dom::TagName::Input => {
            input_submission_value(dom, node).map(|value| (name, value))
        }
        silksurf_dom::TagName::Select => {
            select_submission_value(dom, node).map(|value| (name, value))
        }
        silksurf_dom::TagName::Textarea => Some((name, textarea_text(dom, node))),
        _ => None,
    }
}

pub(crate) fn select_submission_value(
    dom: &silksurf_dom::Dom,
    select: silksurf_dom::NodeId,
) -> Option<String> {
    selected_select_option(dom, select).map(|option| option_value(dom, option))
}

pub(crate) fn selected_select_option(
    dom: &silksurf_dom::Dom,
    select: silksurf_dom::NodeId,
) -> Option<silksurf_dom::NodeId> {
    let options = enabled_select_options(dom, select);
    options
        .iter()
        .copied()
        .find(|&option| option_selected(dom, option))
        .or_else(|| options.first().copied())
}

pub(crate) fn option_value(dom: &silksurf_dom::Dom, option: silksurf_dom::NodeId) -> String {
    element_attribute(dom, option, "value")
        .map_or_else(|| textarea_text(dom, option), ToOwned::to_owned)
}

pub(crate) fn input_submission_value(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<String> {
    let input_type = element_attribute(dom, node, "type")
        .unwrap_or("text")
        .to_ascii_lowercase();
    match input_type.as_str() {
        "button" | "file" | "image" | "reset" | "submit" => None,
        "checkbox" | "radio" => input_checked(dom, node).then(|| checkbox_radio_value(dom, node)),
        _ => Some(input_value(dom, node)),
    }
}

pub(crate) fn checkbox_radio_value(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> String {
    element_attribute(dom, node, "value")
        .filter(|value| !value.is_empty())
        .unwrap_or("on")
        .to_string()
}

pub(crate) fn input_checked(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> bool {
    element_attribute(dom, node, "checked").is_some()
}

pub(crate) fn option_selected(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> bool {
    element_attribute(dom, node, "selected").is_some()
}

pub(crate) fn input_type(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> String {
    element_attribute(dom, node, "type")
        .unwrap_or("text")
        .to_ascii_lowercase()
}

pub(crate) fn element_attribute<'a>(
    dom: &'a silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    name: &str,
) -> Option<&'a str> {
    dom.attributes(node).ok()?.iter().find_map(|attr| {
        attr.name
            .as_str()
            .eq_ignore_ascii_case(name)
            .then_some(attr.value.as_str())
    })
}

pub(crate) fn http_method_label(method: HttpMethod) -> &'static str {
    match method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Delete => "DELETE",
    }
}

pub(crate) fn normalize_address_input(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return None;
    }
    if let Ok(url) = url::Url::parse(trimmed) {
        return browser_supported_url(&url);
    }
    let with_scheme = format!("https://{trimmed}");
    let url = url::Url::parse(&with_scheme).ok()?;
    browser_supported_url(&url)
}

pub(crate) fn browser_home_url() -> String {
    std::env::var("SILKSURF_HOME_URL")
        .ok()
        .and_then(|value| normalize_address_input(&value))
        .unwrap_or_else(|| HOME_URL.to_string())
}

pub(crate) fn browser_address_bar_contains(x: f32, y: f32) -> bool {
    x >= ADDRESS_BAR_X as f32
        && x < (ADDRESS_BAR_X + ADDRESS_BAR_WIDTH) as f32
        && y >= ADDRESS_BAR_Y as f32
        && y < (ADDRESS_BAR_Y + ADDRESS_BAR_HEIGHT) as f32
}

pub(crate) fn hit_test_chrome_action(x: f32, y: f32) -> Option<BrowserChromeAction> {
    if nav_button_contains(BACK_BUTTON_X, x, y) {
        return Some(BrowserChromeAction::Back);
    }
    if nav_button_contains(FORWARD_BUTTON_X, x, y) {
        return Some(BrowserChromeAction::Forward);
    }
    if nav_button_contains(HOME_BUTTON_X, x, y) {
        return Some(BrowserChromeAction::Home);
    }
    if nav_button_contains(RELOAD_BUTTON_X, x, y) {
        return Some(BrowserChromeAction::Reload);
    }
    if nav_button_contains(STOP_BUTTON_X, x, y) {
        return Some(BrowserChromeAction::Stop);
    }
    None
}

pub(crate) fn nav_button_contains(button_x: u32, x: f32, y: f32) -> bool {
    x >= button_x as f32
        && x < (button_x + NAV_BUTTON_WIDTH) as f32
        && y >= NAV_BUTTON_Y as f32
        && y < (NAV_BUTTON_Y + NAV_BUTTON_HEIGHT) as f32
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
    use silksurf_dom::NodeId;
    use silksurf_render::DisplayItem;

    #[test]
    fn link_targets_resolve_anchor_text_rects() {
        let document = parse_html(
            "<!doctype html><html><body><a href=\"/docs/start\">Example</a></body></html>",
        )
        .expect("html parses");
        let text = find_text_node(&document.dom, document.document, "Example").expect("text node");
        let rect = Rect {
            x: 12.0,
            y: 64.0,
            width: 90.0,
            height: 18.0,
        };
        let items = vec![DisplayItem::Text {
            rect,
            node: text,
            text_len: 7,
            text: "Example".to_string(),
            font_size: 16.0,
            color: rgba(0, 0, 0, 255),
        }];

        let targets = collect_link_targets(&document.dom, &items, "https://example.com/root/");

        assert_eq!(
            targets,
            vec![LinkTarget {
                rect,
                href: "https://example.com/docs/start".to_string(),
            }]
        );
    }

    #[test]
    fn address_focus_overlay_preserves_existing_text_pixels() {
        let mut pixels = vec![0; 1100 * 44];
        draw_browser_address_overlay(
            &mut pixels,
            1100,
            44,
            "https://example.com",
            "https://example.com".len(),
            false,
        );
        let text_color = argb(31, 41, 55, 255);
        let text_pixels_before = pixels.iter().filter(|pixel| **pixel == text_color).count();

        draw_browser_address_focus_overlay(
            &mut pixels,
            1100,
            44,
            "https://example.com",
            "https://example.com".len(),
        );

        assert_eq!(
            pixels[ADDRESS_BAR_Y as usize * 1100 + ADDRESS_BAR_X as usize],
            argb(37, 99, 235, 255)
        );
        assert!(pixels.iter().filter(|pixel| **pixel == text_color).count() >= text_pixels_before);
    }

    #[test]
    fn hit_test_link_accounts_for_scroll_and_chrome() {
        let targets = vec![LinkTarget {
            rect: Rect {
                x: 20.0,
                y: 244.0,
                width: 80.0,
                height: 20.0,
            },
            href: "https://example.com/next".to_string(),
        }];

        assert_eq!(
            hit_test_link(&targets, 30.0, 144.0, 100.0, 44),
            Some("https://example.com/next")
        );
        assert_eq!(hit_test_link(&targets, 30.0, 24.0, 100.0, 44), None);
        assert_eq!(hit_test_link(&targets, 8.0, 144.0, 100.0, 44), None);
    }

    #[test]
    fn input_targets_resolve_empty_controls_from_layout_rects() {
        let document = parse_html("<!doctype html><html><body><input id=\"q\"></body></html>")
            .expect("html parses");
        let stylesheet = test_stylesheet(&document.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let fused =
            fused_style_layout_paint(&document.dom, &stylesheet, document.document, viewport);

        let targets = collect_input_targets(&document.dom, &fused);

        assert_eq!(targets.len(), 1);
        assert!(targets[0].rect.width > 0.0);
        assert!(targets[0].rect.height > 0.0);
    }

    #[test]
    fn input_targets_include_contenteditable_controls() {
        let document = parse_html(
            "<!doctype html><html><body><div id=\"composer\" contenteditable=\"true\">Hi</div></body></html>",
        )
        .expect("html parses");
        let stylesheet = test_stylesheet(&document.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let fused =
            fused_style_layout_paint(&document.dom, &stylesheet, document.document, viewport);

        let targets = collect_input_targets(&document.dom, &fused);

        assert_eq!(targets.len(), 1);
        assert!(is_text_content_editable_node(
            &document.dom,
            targets[0].node
        ));
    }

    #[test]
    fn checkbox_and_radio_submission_uses_checked_controls() {
        let document = parse_html(concat!(
            "<!doctype html><html><body>",
            "<form>",
            "<input name=\"q\" value=\"silk\">",
            "<input type=\"checkbox\" name=\"opt\" checked>",
            "<input type=\"checkbox\" name=\"skip\" value=\"no\">",
            "<input type=\"radio\" name=\"tier\" value=\"basic\">",
            "<input type=\"radio\" name=\"tier\" value=\"pro\" checked>",
            "</form>",
            "</body></html>"
        ))
        .expect("html parses");
        let form =
            first_element_by_name(&document.dom, document.document, "form").expect("form exists");

        assert_eq!(
            form_submission_pairs(&document.dom, form),
            vec![
                ("q".to_string(), "silk".to_string()),
                ("opt".to_string(), "on".to_string()),
                ("tier".to_string(), "pro".to_string()),
            ]
        );
    }

    #[test]
    fn checkbox_toggle_marks_control_dirty() {
        let mut document = parse_html(
            "<!doctype html><html><body><form><input type=\"checkbox\" name=\"opt\"></form></body></html>",
        )
        .expect("html parses");
        let checkbox =
            first_element_by_name(&document.dom, document.document, "input").expect("input exists");
        let _ = document.dom.take_dirty_nodes();

        assert!(toggle_checkbox_control(&mut document.dom, checkbox).expect("checkbox toggles"));
        assert!(input_checked(&document.dom, checkbox));
        assert_eq!(document.dom.take_dirty_nodes(), vec![checkbox]);

        assert!(toggle_checkbox_control(&mut document.dom, checkbox).expect("checkbox untoggles"));
        assert!(!input_checked(&document.dom, checkbox));
        assert_eq!(document.dom.take_dirty_nodes(), vec![checkbox]);
    }

    #[test]
    fn radio_check_unchecks_same_named_group() {
        let mut document = parse_html(concat!(
            "<!doctype html><html><body>",
            "<form>",
            "<input type=\"radio\" name=\"tier\" value=\"basic\" checked>",
            "<input type=\"radio\" name=\"tier\" value=\"pro\">",
            "</form>",
            "</body></html>"
        ))
        .expect("html parses");
        let basic = element_by_attr(&document.dom, document.document, "input", "value", "basic")
            .expect("basic radio exists");
        let pro = element_by_attr(&document.dom, document.document, "input", "value", "pro")
            .expect("pro radio exists");
        let _ = document.dom.take_dirty_nodes();

        assert!(
            document
                .dom
                .with_mutation_batch(|dom| check_radio_control(dom, document.document, pro))
                .expect("radio group updates")
        );

        assert!(!input_checked(&document.dom, basic));
        assert!(input_checked(&document.dom, pro));
        assert_eq!(document.dom.take_dirty_nodes(), vec![basic, pro]);
    }

    #[test]
    fn checkbox_is_interactive_but_not_text_editable() {
        let document = parse_html(concat!(
            "<!doctype html><html><body>",
            "<input type=\"text\" name=\"q\">",
            "<input type=\"checkbox\" name=\"opt\">",
            "<textarea name=\"note\">Hi</textarea>",
            "</body></html>"
        ))
        .expect("html parses");
        let text = element_by_attr(&document.dom, document.document, "input", "name", "q")
            .expect("text input exists");
        let checkbox = element_by_attr(&document.dom, document.document, "input", "name", "opt")
            .expect("checkbox exists");
        let textarea = first_element_by_name(&document.dom, document.document, "textarea")
            .expect("textarea exists");

        assert!(is_editable_input_node(&document.dom, checkbox));
        assert!(is_text_editable_input_node(&document.dom, text));
        assert!(is_text_editable_input_node(&document.dom, textarea));
        assert!(!is_text_editable_input_node(&document.dom, checkbox));
    }

    #[test]
    fn select_submission_uses_selected_option_value() {
        let document = parse_html(concat!(
            "<!doctype html><html><body>",
            "<form>",
            "<select name=\"sort\">",
            "<option value=\"recent\">Recent</option>",
            "<option value=\"popular\" selected>Popular</option>",
            "</select>",
            "</form>",
            "</body></html>"
        ))
        .expect("html parses");
        let form =
            first_element_by_name(&document.dom, document.document, "form").expect("form exists");

        assert_eq!(
            form_submission_pairs(&document.dom, form),
            vec![("sort".to_string(), "popular".to_string())]
        );
    }

    #[test]
    fn select_submission_defaults_to_first_enabled_option_text() {
        let document = parse_html(concat!(
            "<!doctype html><html><body>",
            "<form>",
            "<select name=\"sort\">",
            "<option value=\"skip\" disabled>Skip</option>",
            "<option>Recent</option>",
            "<option value=\"popular\">Popular</option>",
            "</select>",
            "</form>",
            "</body></html>"
        ))
        .expect("html parses");
        let form =
            first_element_by_name(&document.dom, document.document, "form").expect("form exists");

        assert_eq!(
            form_submission_pairs(&document.dom, form),
            vec![("sort".to_string(), "Recent".to_string())]
        );
    }

    #[test]
    fn select_cycle_marks_changed_options_dirty() {
        let mut document = parse_html(concat!(
            "<!doctype html><html><body>",
            "<form>",
            "<select name=\"sort\">",
            "<option value=\"recent\" selected>Recent</option>",
            "<option value=\"popular\">Popular</option>",
            "</select>",
            "</form>",
            "</body></html>"
        ))
        .expect("html parses");
        let select = first_element_by_name(&document.dom, document.document, "select")
            .expect("select exists");
        let recent = element_by_attr(
            &document.dom,
            document.document,
            "option",
            "value",
            "recent",
        )
        .expect("recent option exists");
        let popular = element_by_attr(
            &document.dom,
            document.document,
            "option",
            "value",
            "popular",
        )
        .expect("popular option exists");
        let _ = document.dom.take_dirty_nodes();

        assert!(cycle_select_control(&mut document.dom, select).expect("select cycles"));

        assert!(!option_selected(&document.dom, recent));
        assert!(option_selected(&document.dom, popular));
        assert_eq!(document.dom.take_dirty_nodes(), vec![recent, popular]);
    }

    #[test]
    fn select_is_interactive_but_not_text_editable() {
        let document = parse_html(concat!(
            "<!doctype html><html><body>",
            "<select name=\"sort\"><option>Recent</option></select>",
            "</body></html>"
        ))
        .expect("html parses");
        let stylesheet = test_stylesheet(&document.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let fused =
            fused_style_layout_paint(&document.dom, &stylesheet, document.document, viewport);
        let select = first_element_by_name(&document.dom, document.document, "select")
            .expect("select exists");

        assert!(is_editable_input_node(&document.dom, select));
        assert!(!is_text_editable_input_node(&document.dom, select));
        assert_eq!(collect_input_targets(&document.dom, &fused).len(), 1);
    }

    #[test]
    fn focused_input_typing_updates_value_with_damage_redraw() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><input id=\"q\" value=\"Hi\"></body></html>"
                .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let input_node = page.frame.input_targets[0].node;
        let mut state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
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
            focused_input: Some(input_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert!(push_focused_input_char(&mut state, '!'));
        assert!(matches!(
            state.redraw_mode,
            BrowserRedrawMode::Damage(_) | BrowserRedrawMode::DamageWithChrome(_)
        ));
        let runtime = state.runtime.as_ref().expect("runtime stays attached");
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(input_value(&dom, input_node), "Hi!");
    }

    #[test]
    fn focused_textarea_typing_updates_text_content_with_damage_redraw() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><textarea id=\"q\">Hi</textarea></body></html>"
                .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let textarea_node = page.frame.input_targets[0].node;
        let mut state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
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
            focused_input: Some(textarea_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert!(push_focused_input_char(&mut state, '!'));
        assert!(matches!(
            state.redraw_mode,
            BrowserRedrawMode::Damage(_) | BrowserRedrawMode::DamageWithChrome(_)
        ));
        let runtime = state.runtime.as_ref().expect("runtime stays attached");
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(input_value(&dom, textarea_node), "Hi!");
        assert!(find_text_node(&dom, textarea_node, "Hi!").is_some());
    }

    #[test]
    fn focused_contenteditable_typing_updates_text_content_with_damage_redraw() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><div id=\"q\" contenteditable=\"plaintext-only\">Hi</div></body></html>"
                .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let editable_node = page.frame.input_targets[0].node;
        let mut state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
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
            focused_input: Some(editable_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert!(push_focused_input_char(&mut state, '!'));
        assert!(matches!(
            state.redraw_mode,
            BrowserRedrawMode::Damage(_) | BrowserRedrawMode::DamageWithChrome(_)
        ));
        let runtime = state.runtime.as_ref().expect("runtime stays attached");
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(input_value(&dom, editable_node), "Hi!");
        assert!(find_text_node(&dom, editable_node, "Hi!").is_some());
    }

    #[test]
    fn focused_textarea_enter_appends_newline_to_text_content() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><textarea id=\"q\">Hi</textarea></body></html>"
                .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let textarea_node = page.frame.input_targets[0].node;
        let mut state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
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
            focused_input: Some(textarea_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert!(push_focused_textarea_newline(&mut state));
        let runtime = state.runtime.as_ref().expect("runtime stays attached");
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(input_value(&dom, textarea_node), "Hi\n");
        assert!(find_text_node(&dom, textarea_node, "Hi\n").is_some());
    }

    #[test]
    fn focused_textarea_enter_ignores_plain_input_controls() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><input id=\"q\" value=\"Hi\"></body></html>"
                .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let input_node = page.frame.input_targets[0].node;
        let mut state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
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
            focused_input: Some(input_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert!(!push_focused_textarea_newline(&mut state));
        let runtime = state.runtime.as_ref().expect("runtime stays attached");
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(input_value(&dom, input_node), "Hi");
    }

    #[test]
    fn focused_input_enter_builds_get_form_target() {
        let payload = BrowserPagePayload {
            url: "https://example.com/base/page.html".to_string(),
            html: concat!(
                "<!doctype html><html><body>",
                "<form action=\"/search?source=fixture\">",
                "<input name=\"q\" value=\"rust gui\">",
                "<textarea name=\"note\">fast path</textarea>",
                "<input name=\"skip\" value=\"ignored\" disabled>",
                "<input value=\"unnamed\">",
                "</form>",
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
        let input_node = page.frame.input_targets[0].node;
        let state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
            navigation_pending: false,
            status_text: "ready".to_string(),
            hover_status_text: None,
            history: vec!["https://example.com/base/page.html".to_string()],
            history_index: 0,
            pending_history: None,
            navigation_generation: 0,
            address_editing: false,
            address_select_all: false,
            address_text: "https://example.com/base/page.html".to_string(),
            address_cursor: 0,
            focused_input: Some(input_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert_eq!(
            focused_form_submission_target(&state),
            Some(FormSubmissionTarget::Get(
                "https://example.com/search?source=fixture&q=rust+gui&note=fast+path".to_string()
            ))
        );
    }

    #[test]
    fn focused_input_enter_builds_post_form_target() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: concat!(
                "<!doctype html><html><body>",
                "<form method=\"POST\" action=\"/submit\">",
                "<input name=\"q\" value=\"rust\">",
                "<textarea name=\"note\">fast path</textarea>",
                "</form>",
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
        let input_node = page.frame.input_targets[0].node;
        let state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
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
            focused_input: Some(input_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert_eq!(
            focused_form_submission_target(&state),
            Some(FormSubmissionTarget::Post(
                BrowserNavigationRequest::post_form(
                    "https://example.com/submit".to_string(),
                    b"q=rust&note=fast+path".to_vec()
                )
            ))
        );
    }

    #[test]
    fn focused_input_enter_reports_unsupported_dialog_form() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: concat!(
                "<!doctype html><html><body>",
                "<form method=\"dialog\" action=\"/submit\">",
                "<input name=\"q\" value=\"rust\">",
                "</form>",
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
        let input_node = page.frame.input_targets[0].node;
        let state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
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
            focused_input: Some(input_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert_eq!(
            focused_form_submission_target(&state),
            Some(FormSubmissionTarget::UnsupportedMethod(
                "dialog".to_string()
            ))
        );
    }

    #[test]
    fn focused_textarea_enter_does_not_submit_parent_form() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: concat!(
                "<!doctype html><html><body>",
                "<form action=\"/search\">",
                "<textarea name=\"q\">rust</textarea>",
                "</form>",
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
        let textarea_node = page.frame.input_targets[0].node;
        let state = BrowserState {
            frame: page.frame,
            runtime: Some(page.runtime),
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
            focused_input: Some(textarea_node),
            redraw_mode: BrowserRedrawMode::Clean,
            retained_present: None,
        };

        assert_eq!(focused_form_submission_target(&state), None);
    }

    #[test]
    fn focused_input_text_damage_tracks_changed_suffix() {
        let item = silksurf_render::DisplayItem::Text {
            rect: Rect {
                x: 100.0,
                y: 200.0,
                width: 400.0,
                height: 64.0,
            },
            node: silksurf_dom::NodeId::from_raw(1),
            text_len: 5,
            text: "Hello".to_string(),
            font_size: 16.0,
            color: rgba(0, 0, 0, 255),
        };

        let damage =
            focused_input_text_damage_rect(&item, "Hello!").expect("text item gives damage");

        assert!(damage.x > 140.0);
        assert!(damage.width < 32.0);
        assert!(damage.height < 32.0);
    }

    #[test]
    fn focused_input_text_damage_tracks_next_line_suffix() {
        let item = silksurf_render::DisplayItem::Text {
            rect: Rect {
                x: 100.0,
                y: 200.0,
                width: 400.0,
                height: 64.0,
            },
            node: silksurf_dom::NodeId::from_raw(1),
            text_len: 6,
            text: "Hello\n".to_string(),
            font_size: 16.0,
            color: rgba(0, 0, 0, 255),
        };

        let damage =
            focused_input_text_damage_rect(&item, "Hello\n!").expect("text item gives damage");

        assert_eq!(damage.x, 100.0);
        assert!(damage.y > 220.0);
        assert!(damage.width < 32.0);
    }

    #[test]
    fn focused_empty_insert_damage_marks_first_text_cells() {
        let input_node = silksurf_dom::NodeId::from_raw(10);
        let mut state = test_browser_state("https://example.com/");
        state.frame.input_targets = vec![InputTarget {
            rect: Rect {
                x: 8.0,
                y: 1436.0,
                width: 1264.0,
                height: 22.0,
            },
            node: input_node,
        }];

        let damage =
            focused_empty_insert_damage(&state.frame, input_node, "", "!").expect("damage exists");

        assert_eq!(damage.x, 8.0);
        assert_eq!(damage.y, 1436.0);
        assert!(damage.width < 32.0);
        assert_eq!(damage.height, 22.0);
    }

    #[test]
    fn focus_next_page_input_cycles_targets() {
        let first = silksurf_dom::NodeId::from_raw(10);
        let second = silksurf_dom::NodeId::from_raw(11);
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;
        state.frame.input_targets = vec![
            InputTarget {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT,
                    width: 20.0,
                    height: 20.0,
                },
                node: first,
            },
            InputTarget {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT + 24.0,
                    width: 20.0,
                    height: 20.0,
                },
                node: second,
            },
        ];

        assert!(focus_next_page_input(&mut state));
        assert_eq!(state.focused_input, Some(first));
        assert!(matches!(
            state.redraw_mode,
            BrowserRedrawMode::PageInputFocus(_)
        ));
        state.redraw_mode = BrowserRedrawMode::Clean;
        assert!(focus_next_page_input(&mut state));
        assert_eq!(state.focused_input, Some(second));
        assert!(focus_next_page_input(&mut state));
        assert_eq!(state.focused_input, Some(first));
    }

    #[test]
    fn focus_next_page_input_prefers_text_editable_targets() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html:
                "<!doctype html><html><body><input type=\"checkbox\"><input id=\"q\"></body></html>"
                    .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let checkbox_node = page.frame.input_targets[0].node;
        let text_node = page.frame.input_targets[1].node;
        let mut state = test_browser_state_from_page(page);
        state.redraw_mode = BrowserRedrawMode::Clean;

        assert!(focus_next_page_input(&mut state));

        assert_ne!(state.focused_input, Some(checkbox_node));
        assert_eq!(state.focused_input, Some(text_node));
    }

    #[test]
    fn prepared_focus_target_matches_text_editable_focus_order() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html:
                "<!doctype html><html><body><input type=\"checkbox\"><input id=\"q\"></body></html>"
                    .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };
        let page = build_browser_page(payload).expect("payload builds page");
        let checkbox_node = page.frame.input_targets[0].node;
        let text_node = page.frame.input_targets[1].node;
        let dom = page
            .runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let target =
            first_prepared_focus_target(&dom, &page.frame.input_targets).expect("target exists");

        assert_ne!(target.node, checkbox_node);
        assert_eq!(target.node, text_node);
    }

    #[test]
    fn focus_next_visible_page_input_prefers_viewport_targets() {
        let hidden = silksurf_dom::NodeId::from_raw(10);
        let visible = silksurf_dom::NodeId::from_raw(11);
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;
        state.frame.input_targets = vec![
            InputTarget {
                rect: Rect {
                    x: 0.0,
                    y: 2_000.0,
                    width: 20.0,
                    height: 20.0,
                },
                node: hidden,
            },
            InputTarget {
                rect: Rect {
                    x: 0.0,
                    y: BROWSER_CHROME_HEIGHT + 24.0,
                    width: 20.0,
                    height: 20.0,
                },
                node: visible,
            },
        ];

        assert!(focus_next_visible_page_input(
            &mut state,
            0.0,
            BROWSER_CHROME_HEIGHT as u32,
            FRAME_HEIGHT
        ));
        assert_eq!(state.focused_input, Some(visible));
    }

    #[test]
    fn scroll_to_show_input_target_keeps_visible_target_stable() {
        let rect = Rect {
            x: 8.0,
            y: 120.0,
            width: 200.0,
            height: 24.0,
        };

        assert_eq!(
            scroll_to_show_input_target(0.0, rect, 2_000.0, BROWSER_CHROME_HEIGHT as u32, 800),
            0.0
        );
    }

    #[test]
    fn scroll_to_show_input_target_reveals_below_viewport_target() {
        let rect = Rect {
            x: 8.0,
            y: 1436.0,
            width: 1264.0,
            height: 22.0,
        };

        assert_eq!(
            scroll_to_show_input_target(0.0, rect, 2_000.0, BROWSER_CHROME_HEIGHT as u32, 800),
            682.0
        );
    }

    #[test]
    fn scroll_to_show_input_target_clamps_to_page_end() {
        let rect = Rect {
            x: 8.0,
            y: 3_000.0,
            width: 1264.0,
            height: 22.0,
        };

        assert_eq!(
            scroll_to_show_input_target(0.0, rect, 900.0, BROWSER_CHROME_HEIGHT as u32, 800),
            900.0
        );
    }

    #[test]
    fn first_focus_target_scroll_tracks_offscreen_input() {
        let input_node = silksurf_dom::NodeId::from_raw(10);
        let targets = vec![InputTarget {
            rect: Rect {
                x: 8.0,
                y: 1436.0,
                width: 1264.0,
                height: 22.0,
            },
            node: input_node,
        }];

        assert_eq!(
            first_focus_target_scroll(&targets, 1500, 800, BROWSER_CHROME_HEIGHT as u32),
            Some(682)
        );
    }

    #[test]
    fn page_input_focus_only_repaints_address_when_address_was_editing() {
        let input_node = silksurf_dom::NodeId::from_raw(15);
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;
        state.address_editing = true;

        assert!(focus_page_input(&mut state, input_node));
        assert_eq!(state.focused_input, Some(input_node));
        assert!(!state.address_editing);
        assert_eq!(state.redraw_mode, BrowserRedrawMode::AddressChrome);
    }

    #[test]
    fn page_input_focus_uses_copy_free_focus_damage() {
        let input_node = silksurf_dom::NodeId::from_raw(15);
        let mut state = test_browser_state("https://example.com/");
        let rect = Rect {
            x: 32.0,
            y: 443.0,
            width: 320.0,
            height: 22.0,
        };
        state.frame.input_targets.push(InputTarget {
            rect,
            node: input_node,
        });
        state.redraw_mode = BrowserRedrawMode::Clean;

        assert!(focus_page_input(&mut state, input_node));
        assert_eq!(state.focused_input, Some(input_node));
        assert_eq!(state.redraw_mode, BrowserRedrawMode::PageInputFocus(rect));
    }

    #[test]
    fn browser_cursor_shape_uses_chrome_and_page_targets() {
        let input_node = silksurf_dom::NodeId::from_raw(21);
        let mut state = test_browser_state("https://example.com/");
        state.history = vec![
            "https://example.com/start".to_string(),
            "https://example.com/".to_string(),
        ];
        state.history_index = 1;
        state.frame.link_targets.push(LinkTarget {
            rect: Rect {
                x: 16.0,
                y: 90.0,
                width: 120.0,
                height: 20.0,
            },
            href: "https://example.com/docs".to_string(),
        });
        state.frame.input_targets.push(InputTarget {
            rect: Rect {
                x: 24.0,
                y: 130.0,
                width: 180.0,
                height: 24.0,
            },
            node: input_node,
        });

        assert_eq!(
            browser_cursor_shape_for_state(&state, BROWSER_CHROME_HEIGHT as u32, 15.0, 22.0, 0.0),
            silksurf_gui::WinitCursorShape::Pointer
        );
        assert_eq!(
            browser_cursor_shape_for_state(
                &state,
                BROWSER_CHROME_HEIGHT as u32,
                ADDRESS_BAR_X as f32 + 8.0,
                ADDRESS_BAR_Y as f32 + 8.0,
                0.0
            ),
            silksurf_gui::WinitCursorShape::Text
        );
        assert_eq!(
            browser_cursor_shape_for_state(&state, BROWSER_CHROME_HEIGHT as u32, 32.0, 90.0, 0.0),
            silksurf_gui::WinitCursorShape::Pointer
        );
        assert_eq!(
            browser_cursor_shape_for_state(&state, BROWSER_CHROME_HEIGHT as u32, 32.0, 130.0, 0.0),
            silksurf_gui::WinitCursorShape::Text
        );
        assert_eq!(
            browser_cursor_shape_for_state(&state, BROWSER_CHROME_HEIGHT as u32, 400.0, 400.0, 0.0),
            silksurf_gui::WinitCursorShape::Default
        );
    }

    #[test]
    fn link_hover_status_updates_only_when_target_changes() {
        let mut state = test_browser_state("https://example.com/");
        state.redraw_mode = BrowserRedrawMode::Clean;
        state.frame.link_targets.push(LinkTarget {
            rect: Rect {
                x: 16.0,
                y: 90.0,
                width: 120.0,
                height: 20.0,
            },
            href: "https://example.com/docs".to_string(),
        });

        assert!(update_hover_status(
            &mut state,
            BROWSER_CHROME_HEIGHT as u32,
            32.0,
            90.0,
            0.0
        ));
        assert_eq!(browser_status_text(&state), "https://example.com/docs");
        assert_eq!(state.redraw_mode, BrowserRedrawMode::StatusChrome);

        state.redraw_mode = BrowserRedrawMode::Clean;
        assert!(!update_hover_status(
            &mut state,
            BROWSER_CHROME_HEIGHT as u32,
            32.0,
            90.0,
            0.0
        ));
        assert_eq!(state.redraw_mode, BrowserRedrawMode::Clean);

        assert!(update_hover_status(
            &mut state,
            BROWSER_CHROME_HEIGHT as u32,
            400.0,
            400.0,
            0.0
        ));
        assert_eq!(browser_status_text(&state), "ready");
        assert_eq!(state.redraw_mode, BrowserRedrawMode::StatusChrome);
    }

    #[test]
    fn link_targets_ignore_unsupported_schemes() {
        assert_eq!(
            resolve_page_url("mailto:ops@example.com", "https://example.com/"),
            None
        );
        assert_eq!(
            resolve_page_url("#top", "https://example.com/docs/page"),
            Some("https://example.com/docs/page#top".to_string())
        );
    }

    #[test]
    fn browser_status_overlay_updates_chrome_pixels() {
        let mut pixels = vec![0; 1100 * 44];

        draw_browser_status(&mut pixels, 1100, 44, "loading");

        let rect = browser_status_text_band_rect(1100, 44).expect("status rect exists");
        assert_eq!(
            pixels[rect.y as usize * 1100 + (rect.x + rect.width - 1) as usize],
            argb(243, 244, 246, 255)
        );
        assert!(
            pixels.iter().any(|pixel| *pixel == argb(75, 85, 99, 255)),
            "status glyph should write foreground pixels"
        );
    }

    #[test]
    fn chrome_action_enabled_tracks_history_and_pending_navigation() {
        let mut state = test_browser_state("https://example.com/b");
        state.history = vec![
            "https://example.com/a".to_string(),
            "https://example.com/b".to_string(),
        ];
        state.history_index = 1;

        assert!(chrome_action_enabled(&state, BrowserChromeAction::Back));
        assert!(!chrome_action_enabled(&state, BrowserChromeAction::Forward));
        assert!(chrome_action_enabled(&state, BrowserChromeAction::Home));
        assert!(chrome_action_enabled(&state, BrowserChromeAction::Reload));
        assert!(!chrome_action_enabled(&state, BrowserChromeAction::Stop));

        state.navigation_pending = true;
        assert!(!chrome_action_enabled(&state, BrowserChromeAction::Back));
        assert!(!chrome_action_enabled(&state, BrowserChromeAction::Forward));
        assert!(!chrome_action_enabled(&state, BrowserChromeAction::Home));
        assert!(!chrome_action_enabled(&state, BrowserChromeAction::Reload));
        assert!(chrome_action_enabled(&state, BrowserChromeAction::Stop));
    }

    #[test]
    fn chrome_action_hit_test_tracks_button_bounds() {
        assert_eq!(
            hit_test_chrome_action(BACK_BUTTON_X as f32 + 2.0, NAV_BUTTON_Y as f32 + 2.0),
            Some(BrowserChromeAction::Back)
        );
        assert_eq!(
            hit_test_chrome_action(FORWARD_BUTTON_X as f32 + 2.0, NAV_BUTTON_Y as f32 + 2.0),
            Some(BrowserChromeAction::Forward)
        );
        assert_eq!(
            hit_test_chrome_action(HOME_BUTTON_X as f32 + 2.0, NAV_BUTTON_Y as f32 + 2.0),
            Some(BrowserChromeAction::Home)
        );
        assert_eq!(
            hit_test_chrome_action(RELOAD_BUTTON_X as f32 + 2.0, NAV_BUTTON_Y as f32 + 2.0),
            Some(BrowserChromeAction::Reload)
        );
        assert_eq!(
            hit_test_chrome_action(STOP_BUTTON_X as f32 + 2.0, NAV_BUTTON_Y as f32 + 2.0),
            Some(BrowserChromeAction::Stop)
        );
        assert_eq!(
            hit_test_chrome_action(ADDRESS_BAR_X as f32 + 2.0, ADDRESS_BAR_Y as f32 + 2.0),
            None
        );
    }

    #[test]
    fn disabled_chrome_buttons_do_not_request_pointer_cursor() {
        let state = test_browser_state("https://example.com/");

        assert_eq!(
            browser_cursor_shape_for_state(
                &state,
                BROWSER_CHROME_HEIGHT as u32,
                BACK_BUTTON_X as f32 + 2.0,
                NAV_BUTTON_Y as f32 + 2.0,
                0.0,
            ),
            silksurf_gui::WinitCursorShape::Default
        );
    }

    #[test]
    fn enabled_chrome_buttons_request_pointer_cursor() {
        let mut state = test_browser_state("https://example.com/b");
        state.history = vec![
            "https://example.com/a".to_string(),
            "https://example.com/b".to_string(),
        ];
        state.history_index = 1;

        assert_eq!(
            browser_cursor_shape_for_state(
                &state,
                BROWSER_CHROME_HEIGHT as u32,
                BACK_BUTTON_X as f32 + 2.0,
                NAV_BUTTON_Y as f32 + 2.0,
                0.0,
            ),
            silksurf_gui::WinitCursorShape::Pointer
        );
    }

    #[test]
    fn navigation_button_overlay_shows_disabled_state() {
        let state = test_browser_state("https://example.com/");
        let mut pixels = vec![0; 1100 * 44];

        draw_browser_navigation_buttons(&state, &mut pixels, 1100, 44);

        assert_eq!(
            pixels[NAV_BUTTON_Y as usize * 1100 + BACK_BUTTON_X as usize],
            argb(209, 213, 219, 255)
        );
        assert!(
            pixels
                .iter()
                .any(|pixel| *pixel == argb(156, 163, 175, 255)),
            "disabled button glyph should use disabled foreground"
        );
    }

    #[test]
    fn address_input_normalizes_browser_urls() {
        assert_eq!(
            normalize_address_input("example.com/path"),
            Some("https://example.com/path".to_string())
        );
        assert_eq!(
            normalize_address_input("http://example.com/"),
            Some("http://example.com/".to_string())
        );
        assert_eq!(normalize_address_input("mailto:ops@example.com"), None);
        assert_eq!(normalize_address_input("example .com"), None);
    }

    #[test]
    fn address_editing_updates_buffer_without_navigation() {
        let mut state = test_browser_state("https://example.com/");

        assert!(focus_address_bar(&mut state));
        assert!(state.address_editing);
        assert!(state.address_select_all);
        assert_eq!(state.address_text, "https://example.com/");
        assert_eq!(state.address_cursor, "https://example.com/".len());
        assert!(push_address_char(&mut state, 'x'));
        assert!(!state.address_select_all);
        assert_eq!(state.address_text, "x");
        assert_eq!(state.address_cursor, 1);
        assert!(push_address_char(&mut state, 'y'));
        assert_eq!(state.address_text, "xy");
        assert_eq!(state.address_cursor, 2);
        assert!(move_address_caret(&mut state, AddressCaretMotion::Backward));
        assert_eq!(state.address_cursor, 1);
        assert!(push_address_char(&mut state, 'z'));
        assert_eq!(state.address_text, "xzy");
        assert_eq!(state.address_cursor, 2);
        assert!(edit_address_backspace(&mut state));
        assert_eq!(state.address_text, "xy");
        assert_eq!(state.address_cursor, 1);
        assert!(focus_address_bar(&mut state));
        assert!(edit_address_backspace(&mut state));
        assert_eq!(state.address_text, "");
        assert_eq!(state.address_cursor, 0);
    }

    #[test]
    fn address_caret_home_end_and_selection_collapse() {
        let mut state = test_browser_state("https://example.com/");

        assert!(focus_address_bar(&mut state));
        assert!(move_address_caret(&mut state, AddressCaretMotion::Start));
        assert_eq!(state.address_cursor, 0);
        assert!(!state.address_select_all);
        assert!(push_address_char(&mut state, 'x'));
        assert_eq!(state.address_text, "xhttps://example.com/");
        assert!(move_address_caret(&mut state, AddressCaretMotion::End));
        assert_eq!(state.address_cursor, state.address_text.len());
    }

    #[test]
    fn address_clipboard_helpers_follow_selection_model() {
        let mut state = test_browser_state("https://example.com/");

        assert_eq!(address_clipboard_text(&state), None);
        assert!(focus_address_bar(&mut state));
        assert_eq!(address_clipboard_text(&state), Some("https://example.com/"));
        assert!(paste_address_text(
            &mut state,
            "https://chat.example/\nignored"
        ));
        assert_eq!(state.address_text, "https://chat.example/ignored");
        assert!(!state.address_select_all);
        assert!(!paste_address_text(&mut state, "\n\t"));
        assert!(!cut_address_text(&mut state));

        assert!(focus_address_bar(&mut state));
        assert!(cut_address_text(&mut state));
        assert_eq!(state.address_text, "");
        assert!(!state.address_select_all);
        assert_eq!(address_clipboard_text(&state), None);
    }

    #[test]
    fn address_paste_respects_text_limit() {
        let mut state = test_browser_state("https://example.com/");

        assert!(focus_address_bar(&mut state));
        let pasted = "x".repeat(ADDRESS_TEXT_MAX_CHARS + 32);
        assert!(paste_address_text(&mut state, pasted.as_str()));
        assert_eq!(state.address_text.len(), ADDRESS_TEXT_MAX_CHARS);
    }

    #[test]
    fn address_overlay_draws_border_and_text_pixels() {
        let mut pixels = vec![0; 1100 * 44];

        draw_browser_address_overlay(
            &mut pixels,
            1100,
            44,
            "https://example.com",
            "https://example.com".len(),
            true,
        );

        assert_eq!(
            pixels[ADDRESS_BAR_Y as usize * 1100 + ADDRESS_BAR_X as usize],
            argb(37, 99, 235, 255)
        );
        assert!(
            pixels.iter().any(|pixel| *pixel == argb(31, 41, 55, 255)),
            "address overlay should write foreground pixels"
        );
    }

    #[test]
    fn chrome_overlay_leaves_content_pixels_untouched() {
        let mut pixels = vec![0x1234_5678; 1100 * 120];
        let mut state = test_browser_state("https://example.com/");
        state.address_editing = true;
        state.address_text = "https://example.com/edit".to_string();
        state.status_text = "loading".to_string();

        draw_browser_chrome_overlays(&state, &mut pixels, 1100, 120);

        assert_eq!(pixels[60 * 1100 + 20], 0x1234_5678);
        assert_ne!(
            pixels[ADDRESS_BAR_Y as usize * 1100 + ADDRESS_BAR_X as usize],
            0x1234_5678
        );
    }

    #[test]
    fn address_text_strip_leaves_border_pixels_untouched() {
        let mut pixels = vec![0x1234_5678; 1100 * 44];

        draw_browser_address_text_strip(&mut pixels, 1100, 44, "abc", 3);

        assert_eq!(
            pixels[ADDRESS_BAR_Y as usize * 1100 + ADDRESS_BAR_X as usize],
            0x1234_5678
        );
        assert!(
            pixels.iter().any(|pixel| *pixel == argb(31, 41, 55, 255)),
            "address text strip should write foreground pixels"
        );
        assert_eq!(
            pixels[(ADDRESS_BAR_Y as usize + 10) * 1100 + (ADDRESS_BAR_X as usize + 240)],
            0x1234_5678
        );
    }

    #[test]
    fn address_cursor_x_uses_text_prefix() {
        let text_x = ADDRESS_BAR_X + 10;
        let max_x = ADDRESS_BAR_X + ADDRESS_BAR_WIDTH - 12;

        assert_eq!(bitmap_text_prefix_end_x(text_x, "abc", 0, max_x), text_x);
        assert_eq!(
            bitmap_text_prefix_end_x(text_x, "abc", 1, max_x),
            text_x + 6
        );
        assert_eq!(
            bitmap_text_prefix_end_x(text_x, "abc", 3, max_x),
            text_x + 18
        );
    }

    #[test]
    fn chrome_overlay_microbench_reports_cost() {
        let mut pixels = vec![0xFFFF_FFFF; 1280 * 800];
        let mut state = test_browser_state("https://example.com/");
        state.address_editing = true;
        state.address_text = "https://example.com/search?q=latency".to_string();
        state.status_text = "loading".to_string();
        let chrome_iters = 10_000_u32;
        let chrome_start = std::time::Instant::now();
        for _ in 0..chrome_iters {
            draw_browser_chrome_overlays(&state, &mut pixels, 1280, 800);
        }
        let chrome_avg = chrome_start.elapsed() / chrome_iters;
        assert!(pixels.iter().any(|pixel| *pixel == argb(31, 41, 55, 255)));

        let frame = vec![0xFFAA_AAAA; 1280 * 800];
        let full_iters = 200_u32;
        let full_start = std::time::Instant::now();
        for _ in 0..full_iters {
            blit_browser_frame(&frame, 1280, 800, 44, 0, 1280, 800, &mut pixels);
        }
        let full_avg = full_start.elapsed() / full_iters;

        eprintln!("[SilkSurf] chrome overlay avg: {chrome_avg:?}; full blit avg: {full_avg:?}");
    }

    #[test]
    fn address_typing_microbench_reports_cost() {
        let mut pixels = vec![0xFFFF_FFFF; 1280 * 320];
        let mut state = test_browser_state("https://example.com/");
        assert!(focus_address_bar(&mut state));
        let chars = b"chatgpt.com/?q=latency";
        let iterations = 10_000_u32;
        let start = std::time::Instant::now();
        for idx in 0..iterations {
            let ch = chars[(idx as usize) % chars.len()] as char;
            if push_address_char(&mut state, ch) {
                draw_browser_address_from_state(&state, &mut pixels, 1280, 320);
            }
            if state.address_text.len() > 128 {
                state.address_text.clear();
            }
        }
        let avg = start.elapsed() / iterations;

        assert!(pixels.iter().any(|pixel| *pixel == argb(31, 41, 55, 255)));
        let strip_start = std::time::Instant::now();
        for idx in 0..iterations {
            let ch = chars[(idx as usize) % chars.len()] as char;
            if push_address_char(&mut state, ch) {
                draw_browser_address_text_from_state(&state, &mut pixels, 1280, 320);
            }
            if state.address_text.len() > 128 {
                state.address_text.clear();
            }
        }
        let strip_avg = strip_start.elapsed() / iterations;

        eprintln!("[SilkSurf] address typing full avg: {avg:?}; text strip avg: {strip_avg:?}");
    }

    #[test]
    fn history_targets_track_back_and_forward_urls() {
        let mut state = test_browser_state("https://example.com/a");
        state.history = vec![
            "https://example.com/a".to_string(),
            "https://example.com/b".to_string(),
            "https://example.com/c".to_string(),
        ];
        state.history_index = 1;

        assert_eq!(
            history_back_target(&state),
            Some((0, "https://example.com/a".to_string()))
        );
        assert_eq!(
            history_forward_target(&state),
            Some((2, "https://example.com/c".to_string()))
        );
    }

    #[test]
    fn push_history_truncates_forward_entries() {
        let mut state = test_browser_state("https://example.com/a");
        state.history = vec![
            "https://example.com/a".to_string(),
            "https://example.com/b".to_string(),
            "https://example.com/c".to_string(),
        ];
        state.history_index = 1;
        state.pending_history = Some(PendingHistoryAction::Push);

        apply_history_success(&mut state, "https://example.com/d");

        assert_eq!(
            state.history,
            vec![
                "https://example.com/a".to_string(),
                "https://example.com/b".to_string(),
                "https://example.com/d".to_string(),
            ]
        );
        assert_eq!(state.history_index, 2);
    }

    #[test]
    fn move_history_changes_cursor_without_rewriting_entries() {
        let mut state = test_browser_state("https://example.com/a");
        state.history = vec![
            "https://example.com/a".to_string(),
            "https://example.com/b".to_string(),
        ];
        state.history_index = 1;
        state.pending_history = Some(PendingHistoryAction::MoveTo(0));

        apply_history_success(&mut state, "https://example.com/a");

        assert_eq!(state.history_index, 0);
        assert_eq!(state.history.len(), 2);
    }

    fn element_by_attr(
        dom: &silksurf_dom::Dom,
        root: NodeId,
        tag: &str,
        attr_name: &str,
        attr_value: &str,
    ) -> Option<NodeId> {
        if dom
            .element_name(root)
            .ok()
            .flatten()
            .is_some_and(|element| element.eq_ignore_ascii_case(tag))
            && element_attribute(dom, root, attr_name).is_some_and(|value| value == attr_value)
        {
            return Some(root);
        }
        for &child in dom.children(root).ok()? {
            if let Some(found) = element_by_attr(dom, child, tag, attr_name, attr_value) {
                return Some(found);
            }
        }
        None
    }
}
