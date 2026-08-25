//! `SilkSurf` Rust-native webview entry point.
//!
//! Pipeline: fetch URL -> parse HTML -> load CSS/JS resources -> create VM
//! with DOM bridge -> run scripts -> layout -> render.
//!
//! Usage: silksurf-app \[--headless\] \[--screenshot PATH\]
//!          \[--display-backend=auto|wayland|x11\] \[URL\]
//! Default URL: `https://example.com`. The windowed browser is the default;
//! `--headless` runs the one-shot static render pipeline instead.

/*
 * mimalloc global allocator.
 *
 * The CSS tokenizer and cascade produce many small heap allocations. mimalloc
 * uses thread-local free lists and page segregation, so the allocation-heavy
 * CSS path runs through a low-latency allocator without changing call sites.
 */
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use quick_cache::{Weighter, unsync::Cache};
use silksurf_css::StyleIndex;
use silksurf_dom::diff::{ChangeKind, DomDiff};
use silksurf_engine::fused_pipeline::{
    FusedResult, FusedWorkspace, ReplacedSize, fused_style_layout_paint,
    fused_style_layout_paint_with_replaced_sizes,
};
use silksurf_engine::speculative::{FetchOrigin, SpeculativeRenderer};
use silksurf_engine::{ParsedDocument, parse_html};
use silksurf_js::SilkContext;
use silksurf_layout::Rect;
use silksurf_net::{HttpMethod, HttpRequest};

/// How long the event loop sleeps between frames of a running animation.
///
/// A 60 Hz cadence is the finest a sampled animation can show on a display
/// refreshing at that rate, so a shorter sleep costs CPU for frames nothing
/// presents. Only an animation whose value still changes reaches this.
const ANIMATION_FRAME_INTERVAL: std::time::Duration = std::time::Duration::from_millis(16);

/// The earlier of two optional deadlines.
///
/// The loop wakes for whichever source needs it first, and a source with no
/// deadline never delays one that has it.
fn merge_deadline(
    left: Option<std::time::Instant>,
    right: Option<std::time::Instant>,
) -> Option<std::time::Instant> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    }
}

mod accessibility;
mod app_options;
mod argb_raster;
mod browser_types;
mod dom_hit_test;
mod engine_process;
mod input;
mod js_events;
mod link_preload;
mod page_build;
mod page_resources;
mod profile;
mod redraw_geometry;
mod runtime_repaint;
mod stylesheet_set;
mod window_frame;
#[cfg(feature = "accessibility")]
#[allow(clippy::wildcard_imports)]
pub(crate) use accessibility::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use app_options::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use argb_raster::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use browser_types::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use dom_hit_test::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use input::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use link_preload::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use page_build::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use page_resources::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use redraw_geometry::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use runtime_repaint::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use stylesheet_set::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use window_frame::*;
#[cfg(test)]
mod test_support;
#[cfg(test)]
#[allow(clippy::wildcard_imports)]
pub(crate) use test_support::*;

/// Print the monitors the display server reports, then exit. winit reports
/// them only from inside an active event loop, so this opens one and lets
/// `resumed` print before it stops.
fn list_monitors_and_exit(display_backend: silksurf_gui::WinitDisplayBackend) -> ! {
    match silksurf_gui::WinitWindow::new("silksurf", 1, 1) {
        Ok(window) => {
            window
                .with_display_backend(display_backend)
                .with_monitor(silksurf_gui::WinitMonitorChoice::List)
                .run(|_width, _height, _pixels| {});
            std::process::exit(0);
        }
        Err(err) => {
            eprintln!("[SilkSurf] --list-monitors: cannot open display: {err}");
            std::process::exit(1);
        }
    }
}

fn run_winit_browser_page(
    display_backend: silksurf_gui::WinitDisplayBackend,
    monitor: silksurf_gui::WinitMonitorChoice,
    render_config: &BrowserRenderConfig,
    image_cache: &Arc<Mutex<ImageResourceCache>>,
    page: BrowserPage,
) {
    let url = page.frame.url.clone();
    let initial_modulepreload_urls = runtime_module_warm_urls(&page.runtime, &url);
    let initial_window_height = initial_browser_window_height(page.frame.raster_height);
    let browser_state = Rc::new(RefCell::new(BrowserState {
        frame: page.frame,
        runtime: Some(page.runtime),
        navigation_pending: false,
        status_text: "ready".to_string(),
        hover_status_text: None,
        history: vec![url.clone()],
        history_index: 0,
        pending_history: None,
        navigation_generation: 0,
        address_editing: false,
        address_select_all: false,
        address_text: url,
        address_cursor: 0,
        focused_input: None,
        redraw_mode: BrowserRedrawMode::Full,
        retained_present: None,
    }));
    #[cfg(feature = "accessibility")]
    log_accessibility_snapshot(&browser_state.borrow());
    let navigation_rx: Rc<RefCell<Option<mpsc::Receiver<NavigationMessage>>>> =
        Rc::new(RefCell::new(None));
    let scroll_y = Rc::new(Cell::new(0.0f32));
    let last_render_width = Rc::new(Cell::new(0u32));
    let last_render_height = Rc::new(Cell::new(0u32));
    let chrome_height = BROWSER_CHROME_HEIGHT as u32;
    let trace_app_frame = std::env::var_os("SILKSURF_TRACE_APP_FRAME").is_some();
    let resolved_display_backend = display_backend.resolve_for_current_environment();
    let window =
        match silksurf_gui::WinitWindow::new("silksurf", FRAME_WIDTH, initial_window_height) {
            Ok(window) => window
                .with_display_backend(display_backend)
                .with_monitor(monitor),
            Err(err) => {
                eprintln!("[SilkSurf] winit: cannot open display: {err}");
                std::process::exit(1);
            }
        };
    eprintln!(
        "[SilkSurf] Display backend: configured={display_backend:?} resolved={resolved_display_backend:?}"
    );

    /*
     * JS timers drive the event-loop sleep: the backend waits until the
     * earliest setTimeout/setInterval deadline, then the wake callback drains
     * the due callbacks through tick_browser_runtime.
     *
     * A stylesheet fetch completes on a worker thread and posts no wake, so
     * an outstanding one arms its own short poll and the earlier deadline
     * wins.
     */
    let deadline_state = Rc::clone(&browser_state);
    let window = window.with_host_work_deadline(move || {
        let state = deadline_state.borrow();
        let runtime = state.runtime.as_ref()?;
        // A scroll marks the observation checkpoint and arms no timer, so an
        // otherwise idle page wakes now rather than sleeping through the
        // delivery its observers are waiting for.
        if runtime.js_ctx.layout_observation_pending() {
            return Some(std::time::Instant::now());
        }
        // An animation whose value still changes with time needs the next
        // frame, and one held at a fill mode does not. A page whose animated
        // selectors match nothing reaches neither, so it keeps the idle cost
        // it had before any animation was sampled.
        let animation = runtime
            .fused_workspace
            .animations_advance()
            .then(|| std::time::Instant::now() + ANIMATION_FRAME_INTERVAL);
        let timer = merge_deadline(runtime.js_ctx.next_host_callback_deadline(), animation);
        if !runtime.sheets.has_pending_fetches() && !runtime.preloads.has_pending_fetches() {
            return timer;
        }
        let poll = std::time::Instant::now() + STYLESHEET_FETCH_POLL_INTERVAL;
        Some(timer.map_or(poll, |timer| timer.min(poll)))
    });

    let render_state = Rc::clone(&browser_state);
    let render_scroll = Rc::clone(&scroll_y);
    let render_last_width = Rc::clone(&last_render_width);
    let render_last_height = Rc::clone(&last_render_height);
    let render_modulepreload = Rc::new(RefCell::new(Some((
        initial_modulepreload_urls,
        render_config.clone(),
    ))));
    let render_modulepreload_state = Rc::clone(&render_modulepreload);
    let ready_state = Rc::clone(&browser_state);
    let ready_last_width = Rc::clone(&last_render_width);
    let ready_last_height = Rc::clone(&last_render_height);
    let action_state = Rc::clone(&browser_state);
    let action_last_width = Rc::clone(&last_render_width);
    let action_last_height = Rc::clone(&last_render_height);
    let retained_update_state = Rc::clone(&browser_state);
    let retained_prepared_state = Rc::clone(&browser_state);
    let presented_state = Rc::clone(&browser_state);
    let presented_last_width = Rc::clone(&last_render_width);
    let presented_last_height = Rc::clone(&last_render_height);
    let input_state = Rc::clone(&browser_state);
    let input_navigation_rx = Rc::clone(&navigation_rx);
    let input_scroll = Rc::clone(&scroll_y);
    let input_render_config = render_config.clone();
    let input_image_cache = Arc::clone(image_cache);
    let wake_state = Rc::clone(&browser_state);
    let wake_navigation_rx = Rc::clone(&navigation_rx);
    let wake_scroll = Rc::clone(&scroll_y);
    let wake_last_width = Rc::clone(&last_render_width);
    let wake_last_height = Rc::clone(&last_render_height);

    window.run_with_input_wake_and_render_actions(
        move |width, height, buffer_age, pixels| {
            let damage = render_browser_window_frame(
                &render_state,
                &render_scroll,
                &render_last_width,
                &render_last_height,
                chrome_height,
                trace_app_frame,
                width,
                height,
                buffer_age,
                pixels,
            );
            if let Some((urls, config)) = render_modulepreload_state.borrow_mut().take() {
                preload_module_scripts(&urls, &config);
            }
            damage
        },
        move |width, height| {
            browser_render_ready(
                &ready_state,
                &ready_last_width,
                &ready_last_height,
                width,
                height,
            )
        },
        move |width, height| {
            browser_render_action(
                &action_state,
                &action_last_width,
                &action_last_height,
                width,
                height,
            )
        },
        move |width, height| browser_retained_buffer_update(&retained_update_state, width, height),
        move |tag| handle_browser_retained_buffer_prepared(&retained_prepared_state, tag),
        move |frame| {
            handle_browser_presented_frame(
                &presented_state,
                &presented_last_width,
                &presented_last_height,
                frame,
            );
        },
        move |input, window_width, window_height, wake_handle| {
            handle_browser_input(
                input,
                &BrowserInputRuntime {
                    state: &input_state,
                    navigation_rx: &input_navigation_rx,
                    scroll: &input_scroll,
                    chrome_height,
                    window_width,
                    window_height,
                    wake_handle,
                    render_config: &input_render_config,
                    image_cache: &input_image_cache,
                },
            )
        },
        move || {
            handle_browser_wake(
                &wake_state,
                &wake_navigation_rx,
                &wake_scroll,
                (wake_last_width.get(), wake_last_height.get()),
            )
        },
    );
}

fn main() {
    /*
     * The default browser runtime keeps startup observability small: status
     * lines go to stderr directly and the panic hook adds a [SilkSurf] prefix
     * before delegating to Rust's default hook. The structured tracing
     * subscriber is available through the structured-tracing feature.
     *
     * mimalloc aborts on OOM natively in release builds. The nightly-only
     * alloc_error_hook API is not part of the stable runtime surface.
     */
    install_observability();

    let args: Vec<String> = std::env::args().collect();
    // The engine worker re-execs this binary, so the process modes claim their
    // flags before parse_app_options, which ignores unrecognized `-` arguments.
    if let Some(exit_code) = engine_process::run_internal_engine_process_mode(&args) {
        std::process::exit(exit_code);
    }
    let mut options = match parse_app_options(&args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("[SilkSurf] {message}");
            return;
        }
    };
    // The initial URL is the top-level document; its site keys the cookie
    // partition for the static-render path and the initial fetch. The winit
    // navigation worker re-derives it per navigation (load_navigation_payload).
    options.render_config.top_level_site = url::Url::parse(&options.url)
        .as_ref()
        .map(silksurf_net::cookie::site_of_url)
        .unwrap_or_default();

    /*
     * --window opens the XCB backend, presents a placeholder frame, and pumps
     * events until Close or Escape. This legacy backend isolates XCB window
     * setup from the fetch, JS, layout, and raster paths.
     *
     * XcbWindow::new() reports headless display failures as SilkError. The app
     * converts that error into stderr plus exit code 1.
     */
    if options.window_mode {
        run_legacy_window_mode();
    }

    // --list-monitors reports the display server's monitor names and exits
    // without loading a page; the names it prints are what --monitor matches.
    if options.monitor == silksurf_gui::WinitMonitorChoice::List {
        list_monitors_and_exit(options.display_backend);
    }

    let image_cache = Arc::new(Mutex::new(ImageResourceCache::new()));
    let mut renderer = match renderer_from_config(&options.render_config) {
        Ok(renderer) => renderer,
        Err(message) => {
            eprintln!("[SilkSurf] {message}");
            return;
        }
    };

    // The windowed browser is the default entry point; --headless selects
    // the one-shot static render pipeline (fetch -> parse -> raster -> exit).
    if !options.headless {
        match load_navigation_payload(
            &BrowserNavigationRequest::get(options.url.clone()),
            &options.render_config,
            &image_cache,
        )
        .and_then(build_browser_page)
        {
            Ok(page) => {
                run_winit_browser_page(
                    options.display_backend,
                    options.monitor.clone(),
                    &options.render_config,
                    &image_cache,
                    page,
                );
            }
            Err(message) => eprintln!("[SilkSurf] {message}"),
        }
        return;
    }

    run_static_browser_render(&options, &mut renderer, &image_cache);
    eprintln!(
        "[SilkSurf] Headless static render finished; run without --headless for the windowed browser."
    );
}

fn run_static_browser_render(
    options: &AppOptions,
    renderer: &mut SpeculativeRenderer,
    image_cache: &Arc<Mutex<ImageResourceCache>>,
) {
    eprintln!("[SilkSurf] Fetching: {}", options.url);
    let (response, fetch_origin, fetch_elapsed) =
        // Initial load is browser-initiated (no page initiator) -> same-site.
        match renderer.fetch_or_speculate(&options.url, &[], None) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[SilkSurf] Fetch error: {}", e.message);
                return;
            }
        };

    match fetch_origin {
        FetchOrigin::Cache => eprintln!(
            "[SilkSurf] CACHE HIT: {} bytes in {:?}",
            response.body.len(),
            fetch_elapsed
        ),
        FetchOrigin::Fresh => eprintln!(
            "[SilkSurf] FETCHED: {} bytes in {:?} (now cached)",
            response.body.len(),
            fetch_elapsed
        ),
        FetchOrigin::Revalidated => eprintln!(
            "[SilkSurf] REVALIDATED 304: {} stored bytes in {:?} (no body transferred)",
            response.body.len(),
            fetch_elapsed
        ),
    }

    /*
     * Background revalidation sends conditional GET headers from a worker
     * thread on cache hits. Rendering proceeds against cached bytes and later
     * consumes the revalidation result.
     */
    let revalidation_handle = if fetch_origin == FetchOrigin::Cache && options.speculative {
        eprintln!(
            "[SilkSurf] Spawning background revalidation for {}",
            options.url
        );
        Some(renderer.spawn_revalidation(&options.url))
    } else {
        None
    };

    eprintln!(
        "[SilkSurf] Response: {} ({} bytes)",
        response.status,
        response.body.len()
    );

    let html = String::from_utf8_lossy(&response.body).to_string();

    // 2. Parse HTML into DOM
    let document = match parse_html(&html) {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!("[SilkSurf] Parse error: {e:?}");
            return;
        }
    };

    let doc_node = document.document;
    let dom = document.dom;
    eprintln!("[SilkSurf] DOM parsed successfully");

    /*
     * The document's stylesheets are the <style> elements and the
     * <link rel=stylesheet> hrefs in tree order. fetch_all_or_speculate loads
     * the links through the cache-first resource path, so same-host HTTPS
     * requests share HTTP/2 multiplexing and a cached sheet returns without
     * network delay.
     */
    let mut sheets = document_stylesheet_set(
        renderer,
        &dom,
        doc_node,
        &options.url,
        &options.render_config,
        "Stylesheet",
    );
    let css_text = sheets.css_text();
    eprintln!(
        "[SilkSurf] Assembled {} bytes of CSS from {} source(s)",
        css_text.len(),
        sheets.source_count()
    );

    let image_urls = extract_image_urls(&dom, doc_node, &options.url);
    let decoded_images = {
        let mut image_cache = image_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        fetch_decoded_images(renderer, &mut image_cache, &image_urls)
    };
    if !image_urls.is_empty() {
        eprintln!(
            "[SilkSurf] Images decoded: {}/{}",
            decoded_images.len(),
            image_urls.len()
        );
    }

    eprintln!("[SilkSurf] Total CSS to parse: {} bytes", css_text.len());

    // 4. Parse CSS -- cache-first via StylesheetCache in SpeculativeRenderer.
    // On first render: full tokenize+parse (~2.5ms for ChatGPT's 128KB CSS).
    // On subsequent renders with same CSS bytes: clone Arc + intern_rules (~200us).
    let css_start = std::time::Instant::now();
    let stylesheet = dom
        .with_interner_mut(|interner| renderer.get_or_parse_stylesheet(&css_text, interner))
        .unwrap_or_else(|| {
            // UNWRAP-OK: parse_stylesheet_with_interner on the empty string can only fail on
            // tokenizer errors; the empty input has none. This is the canonical empty-stylesheet
            // construction.
            silksurf_css::parse_stylesheet_with_interner(
                "",
                &mut silksurf_core::SilkInterner::new(),
            )
            .unwrap()
        });
    eprintln!("[SilkSurf] CSS parsed in {:?}", css_start.elapsed());

    /*
     * The scripts carry their own `<script>` element so the eval loop can name
     * it as document.currentScript, which pages read to reach the tag they sit
     * beside.
     */
    let scripts: Vec<(Option<silksurf_dom::NodeId>, String)> =
        extract_document_script_nodes(&dom, doc_node, &options.url)
            .into_iter()
            .filter_map(|script| match script.source {
                DocumentScriptRef::Inline(text) => Some((Some(script.node), text)),
                DocumentScriptRef::External(_) => None,
            })
            .collect();
    eprintln!("[SilkSurf] Found {} inline script(s)", scripts.len());

    // Viewport dimensions used by fused pipeline and rasterizer
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: FRAME_WIDTH as f32,
        height: FRAME_HEIGHT as f32,
    };

    // 6. Create JS context with live DOM bridge (boa_engine + silksurf_dom).
    //    Arc<Mutex<Dom>> lets the JS context read/write the same DOM that the
    //    HTML parser built, so getElementById and friends work on real content.
    let dom_arc = Arc::new(Mutex::new(dom));
    let cookie_host = url::Url::parse(&options.url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string))
        .unwrap_or_default();
    let cookie_top_level_site = url::Url::parse(&options.url)
        .as_ref()
        .map(silksurf_net::cookie::site_of_url)
        .unwrap_or_default();
    let mut js_ctx = SilkContext::with_dom_and_cookies(
        &dom_arc,
        &options.render_config.cookie_jar,
        &cookie_top_level_site,
        &cookie_host,
    );
    // location.href backs every same-origin URL a page builds; page script runs
    // after the document address is in place.
    js_ctx.set_document_url(&options.url);
    // matchMedia answers from this size, and a startup script that branches on
    // it -- chatgpt.com sets data-desktop-layout from `(min-width: 48rem)` --
    // selects which shell the document renders.
    js_ctx.set_viewport(viewport.width, viewport.height);

    // 7. Execute inline <script> tags.
    execute_static_inline_scripts(&mut js_ctx, &scripts);

    // 7. Drain pending microtasks and Promise reactions.
    js_ctx.run_pending_jobs();
    drain_initial_host_callbacks(&mut js_ctx);

    /*
     * Converge on the document a windowed load reaches: a preload that lands
     * after the scripts run fires its load event, a handler may upgrade a link
     * to a stylesheet, and the new sheet reparses into the cascade.
     */
    let mut preloads = PreloadLinks::new(&options.url, &options.render_config);
    let stylesheet = match settle_static_document(
        &mut js_ctx,
        &dom_arc,
        doc_node,
        &mut sheets,
        &mut preloads,
        STATIC_SETTLE_BUDGET,
    ) {
        Some(settled_css) => {
            eprintln!(
                "[SilkSurf] Settled CSS: {} bytes from {} source(s)",
                settled_css.len(),
                sheets.source_count()
            );
            let dom = dom_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            dom.with_interner_mut(|interner| {
                renderer.get_or_parse_stylesheet(&settled_css, interner)
            })
            .unwrap_or(stylesheet)
        }
        None => stylesheet,
    };

    // 8. Fused style+layout+paint: single BFS pass over post-JS DOM.
    //    Replaces separate compute_styles + build_layout_tree + build_display_list calls.
    //    Running post-JS ensures DOM mutations from scripts are visible in the render.
    let fused_start = std::time::Instant::now();
    let dom_guard = dom_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let replaced_sizes =
        collect_image_replaced_sizes(&dom_guard, doc_node, &options.url, &decoded_images);
    let mut fused = fused_style_layout_paint_with_replaced_sizes(
        &dom_guard,
        &stylesheet,
        doc_node,
        viewport,
        &replaced_sizes,
    );
    let fused_elapsed = fused_start.elapsed();
    let styled_count = fused.styles.iter().filter(|s| s.is_some()).count();
    eprintln!(
        "[SilkSurf] Fused style+layout+paint: {} items, {} styled nodes in {:?}",
        fused.display_items.len(),
        styled_count,
        fused_elapsed
    );
    if let Some(&bfs_idx) = fused.table.node_to_bfs_idx.get(&doc_node) {
        let root_rect = &fused.node_rects[bfs_idx as usize];
        eprintln!(
            "[SilkSurf] Root: {}x{} at ({}, {})",
            root_rect.width, root_rect.height, root_rect.x, root_rect.y
        );
    }

    let mut display_list = silksurf_render::DisplayList {
        items: std::mem::take(&mut fused.display_items),
        tiles: None,
    };
    let mut svg_cache = crate::page_resources::SvgSurfaceCache::default();
    append_image_display_items(
        &dom_guard,
        &fused,
        &options.url,
        &decoded_images,
        &mut svg_cache,
        &mut display_list.items,
    );
    drop(dom_guard);
    let bitmap_height = FRAME_HEIGHT;

    /*
     * rasterize_skia_into provides anti-aliased paths, gradients,
     * rounded-corner arcs, box shadows, and cosmic-text glyph compositing.
     * The cosmic-text FontSystem uses shared state, so this path keeps text
     * rasterization single-threaded.
     */
    let raster_start = std::time::Instant::now();
    let mut raster_buf: Vec<u8> = Vec::new();
    silksurf_render::rasterize_skia_into(
        &display_list,
        FRAME_WIDTH,
        bitmap_height,
        &mut raster_buf,
    );
    let raster_elapsed = raster_start.elapsed();
    eprintln!(
        "[SilkSurf] Rasterized: {} bytes in {:?}",
        raster_buf.len(),
        raster_elapsed
    );

    if let Some(path) = options.screenshot.as_ref() {
        write_static_screenshot(&display_list, viewport.width as u32, path);
    }

    eprintln!("\n=== PROCESSING BUDGET (excludes network) ===");
    eprintln!(
        "  CSS parse:      {:?}",
        css_start
            .elapsed()
            .saturating_sub(fused_elapsed)
            .saturating_sub(raster_elapsed)
    );
    eprintln!("  Fused pipeline: {fused_elapsed:?}");
    eprintln!("  Rasterize:      {raster_elapsed:?}");
    eprintln!("  TOTAL:          {:?}", css_start.elapsed());
    eprintln!("============================================\n");

    eprintln!("[SilkSurf] Pipeline complete for {}", options.url);

    /*
     * Revalidation completes after the initial render. A 304 response keeps
     * the cached render valid. A 200 response updates the cache and diffs the
     * cached DOM against the new DOM so the changed surface is observable.
     */
    if let Some(handle) = revalidation_handle
        && let Err(message) = handle_revalidation(
            handle,
            renderer,
            &options.url,
            &dom_arc,
            doc_node,
            &css_text,
            viewport,
            &fused,
            &mut raster_buf,
        )
    {
        eprintln!("[SilkSurf] {message}");
    }
}
