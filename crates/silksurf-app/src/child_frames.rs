#[allow(clippy::wildcard_imports)]
use crate::*;

static CHILD_FRAME_TRACE_EMITTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static CHILD_FRAME_TRACE_STATES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<(String, usize), String>>,
> = std::sync::OnceLock::new();

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
        merge_deadline(deadline, embedded_frame_work_deadline(&child.page.runtime))
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
        for (owner, source_url, width, height) in candidates.into_iter().take(*remaining_frames) {
            *remaining_frames -= 1;
            let reusable_index = old_frames.iter().position(|child| {
                child.owner == owner
                    && child.source_url == source_url
                    && (child.page.runtime.viewport.width - width as f32).abs() < f32::EPSILON
                    && (child.page.runtime.viewport.height - height as f32).abs() < f32::EPSILON
            });
            let mut child = if let Some(index) = reusable_index {
                old_frames.remove(index)
            } else {
                changed = true;
                let mut child_config = runtime.render_config.clone();
                child_config.window_parent = runtime
                    .js_ctx
                    .window_context_id()
                    .map(|parent| (parent, owner.raw() as u64));
                match build_embedded_page(&source_url, &child_config, width, height) {
                    Ok(child) => EmbeddedBrowserFrame {
                        owner,
                        source_url,
                        page: Box::new(child),
                    },
                    Err(error) => {
                        eprintln!("[SilkSurf] Embedded frame load failed: {error}");
                        continue;
                    }
                }
            };
            if let Ok(Some(_)) =
                repaint_runtime_host_callbacks(&mut child.page.runtime, &mut child.page.frame)
            {
                changed = true;
            }
            changed |= sync_child_frames_inner(
                &mut child.page.runtime,
                &mut child.page.frame,
                depth + 1,
                remaining_frames,
            );
            next_frames.push(child);
        }
        if next_frames.len() != old_frame_count {
            changed = true;
        }
        runtime.child_frames = next_frames;
    }
    composite_child_frames(runtime, frame);
    changed
}

fn collect_child_frame_candidates(
    runtime: &BrowserPageRuntime,
    frame: &BrowserFrame,
) -> Vec<(silksurf_dom::NodeId, String, u32, u32)> {
    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let discovered = discover_iframes(&dom, runtime.document, &frame.url);
    trace_child_frame_states(&dom, &runtime.geometry.borrow(), &discovered, &frame.url);
    if std::env::var_os("SILKSURF_TRACE_CHILD_FRAMES").is_some()
        && !discovered.is_empty()
        && !CHILD_FRAME_TRACE_EMITTED.swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        for (owner, source_url) in &discovered {
            let attrs = dom
                .attributes(*owner)
                .map(|attrs| {
                    attrs
                        .iter()
                        .map(|attr| format!("{}={:?}", attr.name.as_str(), attr.value))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            eprintln!(
                "[SilkSurf] Child-frame candidate: owner={owner:?} source={source_url} attrs={attrs} bounds={:?}",
                runtime.geometry.borrow().get(*owner)
            );
        }
    }
    discovered
        .into_iter()
        .filter_map(|(owner, source_url)| {
            let bounds = runtime.geometry.borrow().get(owner)?;
            let rect = iframe_content_rect(bounds);
            let (mut width, mut height) = bounded_viewport(rect.width, rect.height);
            if width == 0 || height == 0 {
                let (intrinsic_width, intrinsic_height) = iframe_intrinsic_size(&dom, owner);
                (width, height) = bounded_viewport(intrinsic_width, intrinsic_height);
            }
            (width > 0 && height > 0).then_some((owner, source_url, width, height))
        })
        .collect()
}

fn trace_child_frame_states(
    dom: &silksurf_dom::Dom,
    geometry: &PageGeometry,
    frames: &[(silksurf_dom::NodeId, String)],
    document_url: &str,
) {
    if std::env::var_os("SILKSURF_TRACE_CHILD_FRAMES").is_none() {
        return;
    }
    let snapshots = CHILD_FRAME_TRACE_STATES.get_or_init(Default::default);
    let mut snapshots = snapshots
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (owner, source_url) in frames {
        let attributes = dom
            .attributes(*owner)
            .map(|attributes| {
                attributes
                    .iter()
                    .filter(|attribute| {
                        matches!(
                            attribute.name.as_str(),
                            "src" | "style" | "width" | "height"
                        )
                    })
                    .map(|attribute| format!("{}={:?}", attribute.name.as_str(), attribute.value))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let state = format!("{attributes} bounds={:?}", geometry.get(*owner));
        let key = (document_url.to_string(), owner.raw());
        if snapshots.get(&key) != Some(&state) {
            eprintln!("[SilkSurf] Child-frame state: owner={owner:?} source={source_url} {state}");
            snapshots.insert(key, state);
        }
    }
}

fn build_embedded_page(
    source_url: &str,
    config: &BrowserRenderConfig,
    width: u32,
    height: u32,
) -> Result<BrowserPage, String> {
    let image_cache = std::sync::Arc::new(std::sync::Mutex::new(ImageResourceCache::new()));
    let payload = load_embedded_navigation_payload(source_url, config, &image_cache)?;
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

pub(crate) fn composite_child_frames(runtime: &BrowserPageRuntime, frame: &mut BrowserFrame) {
    let geometry = runtime.geometry.borrow();
    for child in &runtime.child_frames {
        let Some(bounds) = geometry.get(child.owner) else {
            continue;
        };
        let mut rect = iframe_content_rect(bounds);
        rect.y -= frame.bitmap_scroll_y as f32;
        composite_child_frame(
            &mut frame.argb,
            (frame.raster_width, frame.bitmap_height),
            &child.page.frame.argb,
            (
                child.page.frame.raster_width,
                child.page.frame.bitmap_height,
            ),
            BROWSER_CHROME_HEIGHT as u32,
            rect,
        );
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
) -> Vec<(silksurf_dom::NodeId, String)> {
    let mut pending = vec![document];
    let mut frames = Vec::new();
    while let Some(node) = pending.pop() {
        if dom.element_name(node).ok().flatten() == Some("iframe") {
            let source = dom.attributes(node).ok().and_then(|attributes| {
                attributes
                    .iter()
                    .find(|attribute| attribute.name.as_str() == "src")
                    .map(|attribute| attribute.value.as_str())
                    .filter(|source| !source.is_empty())
            });
            if let Some(source) = source
                && let Some(url) = url::Url::parse(base_url)
                    .ok()
                    .and_then(|base| base.join(source).ok())
            {
                frames.push((node, url.to_string()));
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
        assert_eq!(page.runtime.child_frames.len(), 1);
        assert!(!Arc::ptr_eq(
            &page.runtime.child_frames[0].page.runtime.dom,
            &page.runtime.dom
        ));
        assert!(
            page.runtime.child_frames[0]
                .page
                .frame
                .argb
                .contains(&0xffff_0000),
            "child frame has no red paint"
        );
        let frame_box = page
            .runtime
            .geometry
            .borrow()
            .get(page.runtime.child_frames[0].owner)
            .expect("owner has a layout box");
        let pixel_x = frame_box[0].floor() as usize;
        let pixel_y = frame_box[1].floor() as usize;
        let pixel = page.frame.argb[pixel_y * page.frame.raster_width as usize + pixel_x];
        assert_eq!(pixel, 0xffff_0000);
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
}
