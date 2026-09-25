#[allow(clippy::wildcard_imports)]
use crate::*;

static CHILD_FRAME_TRACE_EMITTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static CHILD_FRAME_TRACE_STATES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<(String, usize), String>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
pub(crate) fn composite_child_frame(
    parent: &mut [u32],
    parent_size: (u32, u32),
    child: &[u32],
    child_size: (u32, u32),
    child_content_y: u32,
    rect: Rect,
) {
    let (parent_width, parent_height) = parent_size;
    let (child_width, child_height) = child_size;
    if parent_width == 0
        || parent_height == 0
        || child_width == 0
        || child_height <= child_content_y
    {
        return;
    }
    let target_x = rect.x.floor() as i64;
    let target_y = rect.y.floor() as i64;
    let target_width = rect.width.ceil().max(0.0) as u32;
    let target_height = rect.height.ceil().max(0.0) as u32;
    if target_width == 0 || target_height == 0 {
        return;
    }
    let source_height = child_height - child_content_y;
    let left = target_x.max(0) as u32;
    let top = target_y.max(0) as u32;
    let right = (target_x + i64::from(target_width)).min(i64::from(parent_width));
    let bottom = (target_y + i64::from(target_height)).min(i64::from(parent_height));
    if i64::from(left) >= right || i64::from(top) >= bottom {
        return;
    }

    for destination_y in top..bottom as u32 {
        let relative_y = (i64::from(destination_y) - target_y) as u32;
        let source_y = child_content_y + relative_y.saturating_mul(source_height) / target_height;
        for destination_x in left..right as u32 {
            let relative_x = (i64::from(destination_x) - target_x) as u32;
            let source_x = relative_x.saturating_mul(child_width) / target_width;
            let source_index = (source_y * child_width + source_x) as usize;
            let destination_index = (destination_y * parent_width + destination_x) as usize;
            let Some(&source_pixel) = child.get(source_index) else {
                continue;
            };
            let Some(destination_pixel) = parent.get_mut(destination_index) else {
                continue;
            };
            *destination_pixel = composite_argb(source_pixel, *destination_pixel);
        }
    }
}

const MAX_EMBEDDED_FRAME_DEPTH: u8 = 8;
const MAX_EMBEDDED_VIEWPORT_DIMENSION: u32 = 4096;
const MAX_EMBEDDED_VIEWPORT_PIXELS: u64 = 1_048_576;
const MAX_EMBEDDED_FRAME_COUNT: usize = 16;
const MAX_EMBEDDED_FETCH_JOBS: usize = 16;
static ACTIVE_EMBEDDED_FETCH_JOBS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct IframeSandboxPolicy {
    pub(crate) present: bool,
    pub(crate) allow_scripts: bool,
    pub(crate) allow_same_origin: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IframeCandidate {
    owner: silksurf_dom::NodeId,
    source_url: String,
    sandbox: IframeSandboxPolicy,
}

struct EmbeddedFetchPermit;

impl EmbeddedFetchPermit {
    fn acquire() -> Option<Self> {
        ACTIVE_EMBEDDED_FETCH_JOBS
            .fetch_update(
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
                |active| (active < MAX_EMBEDDED_FETCH_JOBS).then_some(active + 1),
            )
            .ok()
            .map(|_| Self)
    }
}

impl Drop for EmbeddedFetchPermit {
    fn drop(&mut self) {
        ACTIVE_EMBEDDED_FETCH_JOBS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

pub(crate) fn embedded_frame_work_deadline(
    runtime: &BrowserPageRuntime,
) -> Option<std::time::Instant> {
    let now = std::time::Instant::now();
    let own = if runtime.js_ctx.layout_observation_pending()
        || runtime.js_ctx.performance_delivery_pending()
    {
        Some(now)
    } else {
        let animation = runtime
            .fused_workspace
            .animations_advance()
            .then(|| now + ANIMATION_FRAME_INTERVAL);
        let callback = runtime.js_ctx.next_host_callback_deadline();
        let callback = merge_deadline(callback, animation);
        if !runtime.sheets.has_pending_fetches() && !runtime.preloads.has_pending_fetches() {
            callback
        } else {
            let poll = now + STYLESHEET_FETCH_POLL_INTERVAL;
            Some(callback.map_or(poll, |deadline| deadline.min(poll)))
        }
    };
    runtime.child_frames.iter().fold(own, |deadline, child| {
        let child_deadline = match &child.state {
            EmbeddedFrameState::Pending | EmbeddedFrameState::Loading(_) => {
                Some(now + STYLESHEET_FETCH_POLL_INTERVAL)
            }
            EmbeddedFrameState::Ready { page, .. } => embedded_frame_work_deadline(&page.runtime),
            EmbeddedFrameState::Failed => None,
        };
        merge_deadline(deadline, child_deadline)
    })
}

pub(crate) fn sync_child_frames(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
    depth: u8,
) -> bool {
    let mut remaining_frames = MAX_EMBEDDED_FRAME_COUNT;
    sync_child_frames_inner(runtime, frame, depth, &mut remaining_frames)
}

fn sync_child_frames_inner(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
    depth: u8,
    remaining_frames: &mut usize,
) -> bool {
    let mut changed = false;
    if depth < MAX_EMBEDDED_FRAME_DEPTH && *remaining_frames > 0 {
        let candidates = collect_child_frame_candidates(runtime, frame);
        let mut old_frames = std::mem::take(&mut runtime.child_frames);
        let old_frame_count = old_frames.len();
        let mut next_frames = Vec::with_capacity(candidates.len().min(*remaining_frames));
        let accepted_count = candidates.len().min(*remaining_frames);
        for candidate in candidates.iter().take(accepted_count).cloned() {
            let Some((child, child_changed)) =
                update_child_frame(runtime, candidate, &mut old_frames, depth, remaining_frames)
            else {
                continue;
            };
            next_frames.push(child);
            changed |= child_changed;
        }
        for candidate in candidates.iter().skip(accepted_count) {
            dispatch_iframe_event(runtime, candidate.owner, "error");
            changed = true;
        }
        if next_frames.len() != old_frame_count {
            changed = true;
        }
        runtime.child_frames = next_frames;
    }
    changed | dispatch_initial_load_when_settled(runtime)
}

fn update_child_frame(
    runtime: &mut BrowserPageRuntime,
    candidate: IframeCandidate,
    old_frames: &mut Vec<EmbeddedBrowserFrame>,
    depth: u8,
    remaining_frames: &mut usize,
) -> Option<(EmbeddedBrowserFrame, bool)> {
    let IframeCandidate {
        owner,
        source_url,
        sandbox,
    } = candidate;
    let (width, height) = child_frame_viewport(runtime, owner)?;
    *remaining_frames -= 1;
    let child_config = child_render_config(runtime, owner, sandbox);
    let mut child = take_reusable_child(
        old_frames,
        owner,
        &source_url,
        sandbox,
        width,
        height,
        &child_config,
    );
    let mut changed = resize_child_frame(&mut child, width, height);
    child.width = width;
    child.height = height;
    changed |= advance_child_fetch(runtime, &mut child, &child_config);
    changed |= advance_ready_child(runtime, owner, &mut child, depth, remaining_frames);
    Some((child, changed))
}

fn child_render_config(
    runtime: &BrowserPageRuntime,
    owner: silksurf_dom::NodeId,
    sandbox: IframeSandboxPolicy,
) -> BrowserRenderConfig {
    let mut config = runtime.render_config.clone();
    config.scripts_disabled |= sandbox.present && !sandbox.allow_scripts;
    config.origin_sandboxed |= sandbox.present && !sandbox.allow_same_origin;
    config.defer_initial_load = true;
    config.window_parent = runtime
        .js_ctx
        .window_context_id()
        .map(|parent| (parent, owner.raw() as u64));
    config
}

fn take_reusable_child(
    old_frames: &mut Vec<EmbeddedBrowserFrame>,
    owner: silksurf_dom::NodeId,
    source_url: &str,
    sandbox: IframeSandboxPolicy,
    width: u32,
    height: u32,
    config: &BrowserRenderConfig,
) -> EmbeddedBrowserFrame {
    let reusable_index = old_frames.iter().position(|child| {
        child.owner == owner
            && child.source_url == source_url
            && child.sandbox == sandbox
            && child.scripts_disabled == config.scripts_disabled
            && child.origin_sandboxed == config.origin_sandboxed
    });
    reusable_index.map_or_else(
        || EmbeddedBrowserFrame {
            owner,
            source_url: source_url.to_string(),
            sandbox,
            width,
            height,
            scripts_disabled: config.scripts_disabled,
            origin_sandboxed: config.origin_sandboxed,
            state: EmbeddedFrameState::Pending,
        },
        |index| old_frames.remove(index),
    )
}

fn resize_child_frame(child: &mut EmbeddedBrowserFrame, width: u32, height: u32) -> bool {
    if child.width == width && child.height == height {
        return false;
    }
    let EmbeddedFrameState::Ready { page, surface, .. } = &mut child.state else {
        return false;
    };
    let chrome_height = BROWSER_CHROME_HEIGHT as u32;
    let viewport = browser_layout_viewport(Some((width, height.saturating_add(chrome_height))));
    page.frame.bitmap_height = height.saturating_add(chrome_height);
    reflow_runtime_for_viewport(&mut page.runtime, &mut page.frame, viewport);
    if let Err(error) = page.runtime.js_ctx.dispatch_window_event("resize") {
        eprintln!("[SilkSurf] Embedded frame resize event failed: {error}");
    }
    *surface = embedded_frame_surface(&page.frame);
    true
}

fn advance_child_fetch(
    runtime: &mut BrowserPageRuntime,
    child: &mut EmbeddedBrowserFrame,
    config: &BrowserRenderConfig,
) -> bool {
    let width = child.width;
    let height = child.height;
    match &mut child.state {
        EmbeddedFrameState::Pending => match start_embedded_fetch(&child.source_url, config) {
            Ok(Some(receiver)) => {
                child.state = EmbeddedFrameState::Loading(receiver);
                true
            }
            Ok(None) => false,
            Err(error) => fail_child_frame(runtime, child, "worker", &error),
        },
        EmbeddedFrameState::Loading(receiver) => match receiver.try_recv() {
            Ok(Ok(payload)) => match build_embedded_page(payload, width, height) {
                Ok(page) => {
                    child.state = EmbeddedFrameState::Ready {
                        surface: embedded_frame_surface(&page.frame),
                        page: Box::new(page),
                        load_event_dispatched: false,
                    };
                    true
                }
                Err(error) => fail_child_frame(runtime, child, "build", &error),
            },
            Ok(Err(error)) => fail_child_frame(runtime, child, "load", &error),
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => fail_child_frame(
                runtime,
                child,
                "load",
                "worker exited before returning a payload",
            ),
        },
        EmbeddedFrameState::Failed | EmbeddedFrameState::Ready { .. } => false,
    }
}

fn fail_child_frame(
    runtime: &mut BrowserPageRuntime,
    child: &mut EmbeddedBrowserFrame,
    stage: &str,
    error: &str,
) -> bool {
    eprintln!("[SilkSurf] Embedded frame {stage} failed: {error}");
    child.state = EmbeddedFrameState::Failed;
    dispatch_iframe_event(runtime, child.owner, "error");
    true
}

fn advance_ready_child(
    runtime: &mut BrowserPageRuntime,
    owner: silksurf_dom::NodeId,
    child: &mut EmbeddedBrowserFrame,
    depth: u8,
    remaining_frames: &mut usize,
) -> bool {
    let EmbeddedFrameState::Ready {
        page,
        surface,
        load_event_dispatched,
    } = &mut child.state
    else {
        return false;
    };
    let mut changed = repaint_runtime_host_callbacks(&mut page.runtime, &mut page.frame)
        .is_ok_and(|redraw| redraw.is_some());
    changed |= sync_child_frames_inner(
        &mut page.runtime,
        &mut page.frame,
        depth + 1,
        remaining_frames,
    );
    if changed {
        *surface = embedded_frame_surface(&page.frame);
    }
    if !*load_event_dispatched && !page.runtime.initial_document_load_pending {
        dispatch_iframe_event(runtime, owner, "load");
        *load_event_dispatched = true;
        changed = true;
    }
    changed
}

fn dispatch_initial_load_when_settled(runtime: &mut BrowserPageRuntime) -> bool {
    if !runtime.initial_document_load_pending
        || !runtime.child_frames.iter().all(embedded_frame_load_settled)
    {
        return false;
    }
    if let Err(error) = dispatch_document_load(&mut runtime.js_ctx) {
        eprintln!("[SilkSurf] Document load event failed: {error}");
    }
    runtime.initial_document_load_pending = false;
    true
}

fn embedded_frame_load_settled(child: &EmbeddedBrowserFrame) -> bool {
    match &child.state {
        EmbeddedFrameState::Pending | EmbeddedFrameState::Loading(_) => false,
        EmbeddedFrameState::Failed => true,
        EmbeddedFrameState::Ready {
            page,
            load_event_dispatched,
            ..
        } => !page.runtime.initial_document_load_pending && *load_event_dispatched,
    }
}

fn dispatch_iframe_event(
    runtime: &mut BrowserPageRuntime,
    owner: silksurf_dom::NodeId,
    event_name: &str,
) {
    let event = silksurf_js::SyntheticEvent::new(event_name, false, false);
    if let Err(error) = runtime.js_ctx.dispatch_dom_event(owner, &event) {
        eprintln!("[SilkSurf] Iframe {event_name} event failed: {error}");
    }
}

fn start_embedded_fetch(
    source_url: &str,
    config: &BrowserRenderConfig,
) -> Result<Option<std::sync::mpsc::Receiver<NavigationResult>>, String> {
    let Some(permit) = EmbeddedFetchPermit::acquire() else {
        return Ok(None);
    };
    let source_url = source_url.to_string();
    let config = config.clone();
    let image_cache = std::sync::Arc::new(std::sync::Mutex::new(ImageResourceCache::new()));
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("silksurf-embedded-fetch".to_string())
        .spawn(move || {
            let _permit = permit;
            let result = load_embedded_navigation_payload(&source_url, &config, &image_cache);
            let _ = sender.send(result);
        })
        .map_err(|error| format!("cannot start embedded fetch worker: {error}"))?;
    Ok(Some(receiver))
}

fn collect_child_frame_candidates(
    runtime: &BrowserPageRuntime,
    frame: &BrowserFrame,
) -> Vec<IframeCandidate> {
    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let discovered = discover_iframes(&dom, runtime.document, &frame.url);
    trace_child_frame_states(
        &dom,
        &runtime.geometry.borrow(),
        &runtime.fused,
        &discovered,
        &frame.url,
    );
    if std::env::var_os("SILKSURF_TRACE_CHILD_FRAMES").is_some()
        && !discovered.is_empty()
        && !CHILD_FRAME_TRACE_EMITTED.swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        for candidate in &discovered {
            let owner = candidate.owner;
            let source_url = &candidate.source_url;
            let attrs = dom
                .attributes(owner)
                .map(|attrs| {
                    attrs
                        .iter()
                        .map(|attr| format!("{}={:?}", attr.name.as_str(), attr.value))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            let computed_style = runtime
                .fused
                .table
                .node_to_bfs_idx
                .get(&owner)
                .and_then(|index| runtime.fused.styles.get(*index as usize))
                .and_then(Option::as_ref)
                .map_or_else(
                    || "computed_style=unavailable".to_string(),
                    |style| {
                        format!(
                            "computed_width={:?} computed_height={:?}",
                            style.width, style.height
                        )
                    },
                );
            eprintln!(
                "[SilkSurf] Child-frame candidate: owner={owner:?} source={source_url} attrs={attrs} bounds={:?} {computed_style}",
                runtime.geometry.borrow().get(owner),
            );
        }
    }
    discovered
}

fn child_frame_viewport(
    runtime: &BrowserPageRuntime,
    owner: silksurf_dom::NodeId,
) -> Option<(u32, u32)> {
    let geometry = runtime.geometry.borrow();
    let Some(bounds) = geometry.get(owner) else {
        drop(geometry);
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (width, height) = iframe_intrinsic_size(&dom, owner);
        let (width, height) = bounded_viewport(width, height);
        return (width > 0 && height > 0).then_some((width, height));
    };
    let rect = iframe_content_rect(bounds);
    let (mut width, mut height) = bounded_viewport(rect.width, rect.height);
    if width == 0 || height == 0 {
        let dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (intrinsic_width, intrinsic_height) = iframe_intrinsic_size(&dom, owner);
        (width, height) = bounded_viewport(intrinsic_width, intrinsic_height);
    }
    (width > 0 && height > 0).then_some((width, height))
}

fn trace_child_frame_states(
    dom: &silksurf_dom::Dom,
    geometry: &PageGeometry,
    fused: &FusedResult,
    frames: &[IframeCandidate],
    document_url: &str,
) {
    if std::env::var_os("SILKSURF_TRACE_CHILD_FRAMES").is_none() {
        return;
    }
    let snapshots = CHILD_FRAME_TRACE_STATES.get_or_init(Default::default);
    let mut snapshots = snapshots
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for candidate in frames {
        let owner = &candidate.owner;
        let source_url = &candidate.source_url;
        let attributes = dom
            .attributes(*owner)
            .map(|attributes| {
                attributes
                    .iter()
                    .filter(|attribute| {
                        matches!(
                            attribute.name.as_str(),
                            "src" | "style" | "width" | "height" | "sandbox"
                        )
                    })
                    .map(|attribute| format!("{}={:?}", attribute.name.as_str(), attribute.value))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let computed_style = fused
            .table
            .node_to_bfs_idx
            .get(owner)
            .and_then(|index| fused.styles.get(*index as usize))
            .and_then(Option::as_ref)
            .map_or_else(
                || "computed_style=unavailable".to_string(),
                |style| {
                    format!(
                        "computed_width={:?} computed_height={:?}",
                        style.width, style.height
                    )
                },
            );
        let state = format!(
            "{attributes} bounds={:?} {computed_style}",
            geometry.get(*owner)
        );
        let key = (document_url.to_string(), owner.raw());
        if snapshots.get(&key) != Some(&state) {
            eprintln!("[SilkSurf] Child-frame state: owner={owner:?} source={source_url} {state}");
            snapshots.insert(key, state);
        }
    }
}

fn build_embedded_page(
    payload: BrowserPagePayload,
    width: u32,
    height: u32,
) -> Result<BrowserPage, String> {
    let chrome_height = BROWSER_CHROME_HEIGHT as u32;
    let page = build_browser_page_with_buffers_for_window(
        payload,
        BrowserFrameBuffers::default(),
        Some((width, height.saturating_add(chrome_height))),
    )
    .map_err(|error| error.message)?;
    eprintln!(
        "[SilkSurf] Embedded frame ready: {} ({}x{}, {} paint items)",
        page.frame.url,
        width,
        height,
        page.runtime.display_list.items.len()
    );
    Ok(page)
}

pub(crate) fn resolve_child_frame_display_items(
    runtime: &BrowserPageRuntime,
    items: &mut Vec<silksurf_render::DisplayItem>,
) {
    for item in items {
        let silksurf_render::DisplayItem::EmbeddedFrame { rect, node } = item else {
            continue;
        };
        if let Some(EmbeddedBrowserFrame {
            state: EmbeddedFrameState::Ready { surface, .. },
            ..
        }) = runtime
            .child_frames
            .iter()
            .find(|child| child.owner == *node)
        {
            *item = silksurf_render::DisplayItem::Image {
                rect: *rect,
                image: surface.clone(),
            };
        }
    }
}

fn embedded_frame_surface(frame: &BrowserFrame) -> silksurf_render::ImageSurface {
    let content_top = BROWSER_CHROME_HEIGHT as u32;
    let width = frame.raster_width;
    let height = frame.bitmap_height.saturating_sub(content_top);
    let mut rgba = Vec::with_capacity((u64::from(width) * u64::from(height) * 4) as usize);
    for pixel in frame
        .argb
        .iter()
        .skip((u64::from(width) * u64::from(content_top)) as usize)
    {
        rgba.extend_from_slice(&[
            (pixel >> 16) as u8,
            (pixel >> 8) as u8,
            *pixel as u8,
            (pixel >> 24) as u8,
        ]);
    }
    silksurf_render::ImageSurface {
        width,
        height,
        rgba: Arc::from(rgba.into_boxed_slice()),
    }
}

fn iframe_content_rect(bounds: silksurf_js::ElementBox) -> Rect {
    let left = bounds[7] + bounds[11];
    let top = bounds[4] + bounds[8];
    let right = bounds[5] + bounds[9];
    let bottom = bounds[6] + bounds[10];
    Rect {
        x: bounds[0] + left,
        y: bounds[1] + top,
        width: (bounds[2] - left - right).max(0.0),
        height: (bounds[3] - top - bottom).max(0.0),
    }
}

fn discover_iframes(
    dom: &silksurf_dom::Dom,
    document: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<IframeCandidate> {
    let mut pending = vec![document];
    let mut frames = Vec::new();
    while let Some(node) = pending.pop() {
        if dom.element_name(node).ok().flatten() == Some("iframe") {
            let attributes = dom.attributes(node).ok();
            let source = attributes.as_ref().and_then(|attributes| {
                attributes
                    .iter()
                    .find(|attribute| attribute.name.as_str() == "src")
            });
            if let Some(source) = source
                .map(|attribute| attribute.value.as_str())
                .filter(|source| !source.is_empty())
                && let Some(url) = url::Url::parse(base_url)
                    .ok()
                    .and_then(|base| base.join(source).ok())
            {
                frames.push(IframeCandidate {
                    owner: node,
                    source_url: url.to_string(),
                    sandbox: attributes.map_or_else(IframeSandboxPolicy::default, |attributes| {
                        parse_iframe_sandbox(attributes)
                    }),
                });
            }
        }
        if let Ok(children) = dom.children(node) {
            pending.extend(children.iter().rev().copied());
        }
        if let Some(shadow_root) = dom.shadow_root(node)
            && let Ok(children) = dom.children(shadow_root)
        {
            pending.extend(children.iter().rev().copied());
        }
    }
    frames
}

fn parse_iframe_sandbox(attributes: &[silksurf_dom::Attribute]) -> IframeSandboxPolicy {
    let Some(value) = attributes
        .iter()
        .find(|attribute| attribute.name.as_str().eq_ignore_ascii_case("sandbox"))
        .map(|attribute| attribute.value.as_str())
    else {
        return IframeSandboxPolicy::default();
    };
    let tokens = value
        .split_ascii_whitespace()
        .map(str::to_ascii_lowercase)
        .collect::<std::collections::HashSet<_>>();
    IframeSandboxPolicy {
        present: true,
        allow_scripts: tokens.contains("allow-scripts"),
        allow_same_origin: tokens.contains("allow-same-origin"),
    }
}

fn bounded_viewport(width: f32, height: f32) -> (u32, u32) {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return (0, 0);
    }
    let width = width.ceil().min(MAX_EMBEDDED_VIEWPORT_DIMENSION as f32) as u32;
    let height = height.ceil().min(MAX_EMBEDDED_VIEWPORT_DIMENSION as f32) as u32;
    let area = u64::from(width) * u64::from(height);
    if area <= MAX_EMBEDDED_VIEWPORT_PIXELS {
        return (width, height);
    }
    let scale = (MAX_EMBEDDED_VIEWPORT_PIXELS as f64 / area as f64).sqrt();
    (
        (f64::from(width) * scale).floor().max(1.0) as u32,
        (f64::from(height) * scale).floor().max(1.0) as u32,
    )
}

fn iframe_intrinsic_size(dom: &silksurf_dom::Dom, owner: silksurf_dom::NodeId) -> (f32, f32) {
    let dimensions = dom.attributes(owner).ok().map(|attributes| {
        let dimension = |name: &str, fallback: f32| {
            attributes
                .iter()
                .find(|attribute| attribute.name.as_str() == name)
                .and_then(|attribute| attribute.value.parse::<f32>().ok())
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or(fallback)
        };
        (dimension("width", 300.0), dimension("height", 150.0))
    });
    dimensions.unwrap_or((300.0, 150.0))
}

#[cfg(test)]
fn composite_argb(source: u32, destination: u32) -> u32 {
    let alpha = (source >> 24) & 0xff;
    if alpha == 0xff {
        return source;
    }
    if alpha == 0 {
        return destination;
    }
    let inverse_alpha = 0xff - alpha;
    let channel = |shift: u32| {
        let foreground = (source >> shift) & 0xff_u32;
        let background = (destination >> shift) & 0xff_u32;
        (foreground * alpha + background * inverse_alpha + 127) / 255
    };
    (0xff << 24) | (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    #[test]
    fn iframe_sandbox_tokens_only_clear_script_and_origin_restrictions() {
        let mut dom = silksurf_dom::Dom::new();
        let frame = dom.create_element("iframe");
        let empty = parse_iframe_sandbox(dom.attributes(frame).expect("attributes exist"));
        assert_eq!(empty, IframeSandboxPolicy::default());

        dom.set_attribute(frame, "sandbox", "")
            .expect("sandbox sets");
        let sandboxed = parse_iframe_sandbox(dom.attributes(frame).expect("attributes exist"));
        assert_eq!(
            sandboxed,
            IframeSandboxPolicy {
                present: true,
                allow_scripts: false,
                allow_same_origin: false,
            }
        );

        dom.set_attribute(frame, "sandbox", "allow-SCRIPTS unknown allow-same-origin")
            .expect("sandbox tokens update");
        let allowed = parse_iframe_sandbox(dom.attributes(frame).expect("attributes exist"));
        assert_eq!(
            allowed,
            IframeSandboxPolicy {
                present: true,
                allow_scripts: true,
                allow_same_origin: true,
            }
        );
    }

    #[test]
    fn child_surface_composites_clipped_into_owner_box() {
        let mut parent = vec![0xff00_0000; 4 * 3];
        let child = vec![
            0xff00_0000,
            0xffff_0000,
            0xff00_0000,
            0xff00_0000,
            0xff00_ff00,
            0xff00_0000,
        ];
        composite_child_frame(
            &mut parent,
            (4, 3),
            &child,
            (3, 2),
            0,
            Rect {
                x: -1.0,
                y: 1.0,
                width: 3.0,
                height: 2.0,
            },
        );
        assert_eq!(parent[4], 0xffff_0000);
        assert_eq!(parent[5], 0xff00_0000);
        assert_eq!(parent[8], 0xff00_ff00);
        assert_eq!(parent[0], 0xff00_0000);
    }

    #[test]
    fn transparent_child_pixels_preserve_the_parent_surface() {
        let mut parent = vec![0xff12_3456; 1];
        composite_child_frame(
            &mut parent,
            (1, 1),
            &[0x0000_0000],
            (1, 1),
            0,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
        );
        assert_eq!(parent, [0xff12_3456]);
    }

    #[test]
    fn dynamically_inserted_frame_paints_its_child_document_into_owner_box() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("local listener binds");
        let address = listener.local_addr().expect("listener address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("child request arrives");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("request reads");
            let body = "<!doctype html><html><body><div style='position:absolute;left:0;top:0;width:6px;height:4px;background-color:red'></div></body></html>";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("response writes");
        });
        let url = format!("http://{address}/parent");
        let config = BrowserRenderConfig::default();
        let mut page = build_browser_page(BrowserPagePayload {
            url: url.clone(),
            html: "<!doctype html><html><body><script>setTimeout(function () { var host = document.createElement('div'); host.style.position = 'absolute'; host.style.left = '2px'; host.style.top = '5px'; host.style.width = '6px'; host.style.height = '4px'; var root = host.attachShadow({ mode: 'closed' }); var frame = document.createElement('iframe'); frame.src = '/child'; frame.style.position = 'absolute'; frame.style.inset = '0'; frame.style.width = '100%'; frame.style.height = '100%'; frame.style.border = '0'; root.appendChild(frame); document.body.appendChild(host); }, 0);</script></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults("body { margin: 0; }"),
            sheet_bodies: Vec::new(),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: config,
            parsed_document: None,
        })
        .expect("parent page builds");
        std::thread::sleep(std::time::Duration::from_millis(5));
        repaint_runtime_host_callbacks(&mut page.runtime, &mut page.frame)
            .expect("parent timer callback paints");

        assert!(sync_child_frames(&mut page.runtime, &mut page.frame, 0));
        server.join().expect("child server exits");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !matches!(
            page.runtime.child_frames.first().map(|child| &child.state),
            Some(EmbeddedFrameState::Ready { .. })
        ) && std::time::Instant::now() < deadline
        {
            sync_child_frames(&mut page.runtime, &mut page.frame, 0);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(page.runtime.child_frames.len(), 1);
        let EmbeddedFrameState::Ready { page: child, .. } = &page.runtime.child_frames[0].state
        else {
            panic!("child frame fetch did not reach Ready");
        };
        assert!(!Arc::ptr_eq(&child.runtime.dom, &page.runtime.dom));
        assert!(
            child.frame.argb.contains(&0xffff_0000),
            "child frame has no red paint"
        );
        repaint_runtime_full_document(&mut page.runtime, &mut page.frame);
        assert!(
            page.runtime.display_list.items.iter().any(|item| matches!(
                item,
                silksurf_render::DisplayItem::Image { image, .. }
                    if image.rgba.chunks_exact(4).any(|pixel| pixel == [255, 0, 0, 255])
            )),
            "parent display list omitted the ready child surface"
        );
        assert!(
            page.frame.argb.contains(&0xffff_0000),
            "parent raster omitted the ready iframe surface"
        );
    }

    #[test]
    fn hidden_iframe_loads_once_and_retains_its_context_when_shown() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("local listener binds");
        let address = listener.local_addr().expect("listener address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("child request arrives");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("request reads");
            std::thread::sleep(std::time::Duration::from_millis(400));
            let body = "<!doctype html><html><body>child</body></html>";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("response writes");
        });
        let url = format!("http://{address}/parent");
        let mut page = build_browser_page(BrowserPagePayload {
            url: url.clone(),
            html: "<!doctype html><html><body><iframe src='/child' style='display:none'></iframe></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults("body { margin: 0; }"),
            sheet_bodies: Vec::new(),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        })
        .expect("parent page builds");

        let started = std::time::Instant::now();
        assert!(sync_child_frames(&mut page.runtime, &mut page.frame, 0));
        assert!(
            started.elapsed() < std::time::Duration::from_millis(250),
            "hidden child fetch blocked the page tick for {:?}",
            started.elapsed()
        );
        server.join().expect("child server exits");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !matches!(
            page.runtime.child_frames.first().map(|child| &child.state),
            Some(EmbeddedFrameState::Ready { .. })
        ) && std::time::Instant::now() < deadline
        {
            sync_child_frames(&mut page.runtime, &mut page.frame, 0);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let owner = page.runtime.child_frames[0].owner;
        let context_id = match &page.runtime.child_frames[0].state {
            EmbeddedFrameState::Ready { page, .. } => page.runtime.js_ctx.window_context_id(),
            _ => panic!("hidden child frame did not finish loading"),
        };
        assert!(
            !page.runtime.display_list.items.iter().any(|item| matches!(
                item,
                silksurf_render::DisplayItem::EmbeddedFrame { node, .. } if *node == owner
            )),
            "display:none iframe contributed a paint item"
        );

        page.runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .set_attribute(owner, "style", "display:block;width:300px;height:150px")
            .expect("iframe becomes visible");
        repaint_runtime_full_document(&mut page.runtime, &mut page.frame);
        sync_child_frames(&mut page.runtime, &mut page.frame, 0);
        assert!(matches!(
            page.runtime.child_frames[0].state,
            EmbeddedFrameState::Ready { .. }
        ));
        let EmbeddedFrameState::Ready { page: child, .. } = &page.runtime.child_frames[0].state
        else {
            panic!("shown child frame lost its runtime");
        };
        assert_eq!(child.runtime.js_ctx.window_context_id(), context_id);
        assert!(
            page.runtime
                .display_list
                .items
                .iter()
                .any(|item| matches!(item, silksurf_render::DisplayItem::Image { .. }))
        );
    }

    #[test]
    fn parent_load_waits_for_child_load_and_fires_after_iframe_load() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("local listener binds");
        let address = listener.local_addr().expect("listener address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("child request arrives");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("request reads");
            std::thread::sleep(std::time::Duration::from_millis(50));
            let body = "<!doctype html><html><body><script>window.addEventListener('message', event => { globalThis.receivedMessage = event.data; }); window.addEventListener('resize', () => { globalThis.resizedViewport = innerWidth + 'x' + innerHeight; });</script>child</body></html>";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("response writes");
        });
        let url = format!("http://{address}/parent");
        let render_config = BrowserRenderConfig {
            defer_initial_load: true,
            ..BrowserRenderConfig::default()
        };
        let mut page = build_browser_page(BrowserPagePayload {
            url,
            html: "<!doctype html><html><body><script>globalThis.loadOrder=[]; document.querySelector('iframe').addEventListener('load', function () { loadOrder.push('iframe'); }); window.addEventListener('load', function () { loadOrder.push('parent'); });</script><iframe src='/child' style='width:20px;height:20px'></iframe></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults("body { margin: 0; }"),
            sheet_bodies: Vec::new(),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config,
            parsed_document: None,
        })
        .expect("parent page builds");

        assert!(page.runtime.initial_document_load_pending);
        sync_child_frames(&mut page.runtime, &mut page.frame, 0);
        assert!(page.runtime.initial_document_load_pending);
        server.join().expect("child server exits");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while page.runtime.initial_document_load_pending && std::time::Instant::now() < deadline {
            sync_child_frames(&mut page.runtime, &mut page.frame, 0);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!page.runtime.initial_document_load_pending);
        page.runtime
            .js_ctx
            .eval("if (loadOrder.join(',') !== 'iframe,parent') throw new Error(loadOrder.join(','));")
            .expect("iframe load precedes the parent's load event");
        let frame_context_id = match &page.runtime.child_frames[0].state {
            EmbeddedFrameState::Ready { page, .. } => page.runtime.js_ctx.window_context_id(),
            _ => panic!("loaded child frame retains its runtime"),
        };
        page.runtime
            .js_ctx
            .eval("document.querySelector('iframe').contentWindow.postMessage('execute', '*');")
            .expect("parent queues a message to the loaded child");
        let owner = page.runtime.child_frames[0].owner;
        page.runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .set_attribute(owner, "style", "width:30px;height:10px")
            .expect("iframe dimensions update");
        repaint_runtime_full_document(&mut page.runtime, &mut page.frame);
        sync_child_frames(&mut page.runtime, &mut page.frame, 0);

        assert_eq!(
            (
                page.runtime.child_frames[0].width,
                page.runtime.child_frames[0].height
            ),
            (30, 10)
        );
        let EmbeddedFrameState::Ready { page: child, .. } = &mut page.runtime.child_frames[0].state
        else {
            panic!("resized child frame retains its runtime");
        };
        assert_eq!(child.runtime.js_ctx.window_context_id(), frame_context_id);
        child
            .runtime
            .js_ctx
            .run_host_callbacks(64)
            .expect("child delivers the queued execute message");
        child.runtime.js_ctx.eval(
            "if (receivedMessage !== 'execute') throw new Error('message lost across resize'); if (resizedViewport !== '30x10') throw new Error(resizedViewport);",
        ).expect("resize preserves message target and updates child viewport");
    }

    #[test]
    fn embedded_viewport_budget_preserves_small_boxes_and_bounds_large_boxes() {
        assert_eq!(bounded_viewport(300.0, 150.0), (300, 150));
        let (width, height) = bounded_viewport(4096.0, 4096.0);
        assert!(u64::from(width) * u64::from(height) <= MAX_EMBEDDED_VIEWPORT_PIXELS);
    }

    #[test]
    fn iframe_without_size_attributes_uses_html_intrinsic_dimensions() {
        let page = build_browser_page(BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><iframe></iframe></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            sheet_bodies: Vec::new(),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        })
        .expect("parent page builds");
        let dom = page
            .runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut pending = vec![page.runtime.document];
        let owner = loop {
            let node = pending.pop().expect("iframe exists");
            if dom.element_name(node).ok().flatten() == Some("iframe") {
                break node;
            }
            pending.extend(dom.children(node).expect("children read").iter().copied());
        };
        let bounds = page
            .runtime
            .geometry
            .borrow()
            .get(owner)
            .expect("iframe receives a layout box");
        assert!((bounds[2] - 300.0).abs() < f32::EPSILON);
        assert!((bounds[3] - 150.0).abs() < f32::EPSILON);
    }

    #[test]
    fn iframe_css_height_overrides_html_intrinsic_dimensions() {
        let page = build_browser_page(BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><iframe style='width:300px;height:65px;border:0'></iframe></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults("body { margin: 0; }"),
            sheet_bodies: Vec::new(),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        })
        .expect("parent page builds");
        let dom = page
            .runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut pending = vec![page.runtime.document];
        let owner = loop {
            let node = pending.pop().expect("iframe exists");
            if dom.element_name(node).ok().flatten() == Some("iframe") {
                break node;
            }
            pending.extend(dom.children(node).expect("children read").iter().copied());
        };
        let bounds = page
            .runtime
            .geometry
            .borrow()
            .get(owner)
            .expect("iframe receives a layout box");
        assert!((bounds[2] - 300.0).abs() < f32::EPSILON);
        assert!((bounds[3] - 65.0).abs() < f32::EPSILON);
    }
}
