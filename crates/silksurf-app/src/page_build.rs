// Page-build entry points thread url, buffers, config, caches, and trace
// flags as explicit parameters between the navigation worker and builder.
#![allow(clippy::too_many_arguments)]

// Module split from the former single-file binary; the crate root
// re-exports every module so sibling items resolve by bare name.
#[allow(clippy::wildcard_imports)]
use crate::*;

pub(crate) fn append_static_external_stylesheets(
    renderer: &mut SpeculativeRenderer,
    dom: &silksurf_dom::Dom,
    doc_node: silksurf_dom::NodeId,
    url: &str,
    css_text: &mut String,
) {
    let stylesheet_urls = extract_stylesheet_urls(dom, doc_node, url);
    let css_accept_header = [("Accept".to_string(), "text/css,*/*".to_string())];
    let sheet_requests: Vec<(&str, &[(String, String)])> = stylesheet_urls
        .iter()
        .map(|u| (u.as_str(), css_accept_header.as_slice()))
        .collect();

    let sheet_results = renderer.fetch_all_or_speculate(&sheet_requests);
    for (result, sheet_url) in sheet_results.into_iter().zip(stylesheet_urls.iter()) {
        match result {
            Ok((resp, origin, elapsed)) if resp.status == 200 => {
                eprintln!(
                    "[SilkSurf] Stylesheet {sheet_url}: {} bytes ({:?} {:?})",
                    resp.body.len(),
                    origin,
                    elapsed
                );
                let sheet_css = String::from_utf8_lossy(&resp.body);
                css_text.push_str(&sheet_css);
                css_text.push('\n');
            }
            Ok((resp, _, _)) => {
                eprintln!("[SilkSurf] Stylesheet {sheet_url}: HTTP {}", resp.status);
            }
            Err(e) => eprintln!(
                "[SilkSurf] Stylesheet {sheet_url}: fetch error: {}",
                e.message
            ),
        }
    }
}

pub(crate) fn execute_static_inline_scripts(js_ctx: &mut SilkContext, scripts: &[String]) {
    for (i, script) in scripts.iter().enumerate() {
        const MAX_INLINE_SCRIPT: usize = 256 * 1024;
        if script.len() > MAX_INLINE_SCRIPT {
            eprintln!(
                "[SilkSurf] Script {i}: {} bytes (skipping -- bundle too large)",
                script.len()
            );
            continue;
        }
        log_static_script_start(i, script);
        let script_start = std::time::Instant::now();
        match js_ctx.eval(script) {
            Ok(()) => eprintln!(
                "[SilkSurf] Script {i} executed OK ({:?})",
                script_start.elapsed()
            ),
            Err(e) => eprintln!(
                "[SilkSurf] Script {i} error: {e} ({:?})",
                script_start.elapsed()
            ),
        }
    }
}

pub(crate) fn log_static_script_start(index: usize, script: &str) {
    if script.len() <= 1200 {
        eprintln!(
            "[SilkSurf] Script {index} FULL ({} bytes): {script}",
            script.len()
        );
        return;
    }
    let preview = &script[..script.len().min(80)];
    eprintln!(
        "[SilkSurf] Executing script {index} ({} bytes): {preview}...",
        script.len()
    );
}

pub(crate) fn load_navigation_payload(
    request: &BrowserNavigationRequest,
    config: &BrowserRenderConfig,
    image_cache: &Arc<Mutex<ImageResourceCache>>,
) -> NavigationResult {
    // The destination is the top-level document; its site keys the cookie
    // partition and drives SameSite enforcement for this navigation and its
    // subresources. Every fetch below uses this nav_config, so subresource
    // fetchers inherit the top-level site.
    let mut config = config.clone();
    config.top_level_site = url::Url::parse(&request.url)
        .as_ref()
        .map(silksurf_net::cookie::site_of_url)
        .unwrap_or_default();
    let config = &config;
    let mut renderer = renderer_from_config(config)?;
    let url = request.url.as_str();
    // The initiator site drives top-level-navigation SameSite enforcement; the
    // top-level document fetch is classified against it, not the subresource
    // rule. `None` (browser-initiated) is same-site.
    let initiator_site = request.initiator_site.as_deref();
    let (response, fetch_origin, fetch_elapsed) =
        if request.method == HttpMethod::Get && request.body.is_empty() {
            renderer
                .fetch_or_speculate(url, &request.headers, initiator_site)
                .map_err(|err| format!("{url}: fetch error: {}", err.message))?
        } else {
            let http_request = request.as_http_request();
            renderer
                .fetch_uncached_request(&http_request, initiator_site)
                .map_err(|err| format!("{url}: fetch error: {}", err.message))?
        };
    match fetch_origin {
        FetchOrigin::Cache => eprintln!(
            "[SilkSurf] Navigation cache hit: {} bytes in {:?}",
            response.body.len(),
            fetch_elapsed
        ),
        FetchOrigin::Fresh => match request.method {
            HttpMethod::Get => eprintln!(
                "[SilkSurf] Navigation fetched: {} bytes in {:?}",
                response.body.len(),
                fetch_elapsed
            ),
            HttpMethod::Post => eprintln!(
                "[SilkSurf] Navigation posted: {} bytes in {:?}",
                response.body.len(),
                fetch_elapsed
            ),
            _ => eprintln!(
                "[SilkSurf] Navigation fetched via {}: {} bytes in {:?}",
                http_method_label(request.method),
                response.body.len(),
                fetch_elapsed
            ),
        },
    }

    let html = String::from_utf8_lossy(&response.body).to_string();
    let document = parse_html(&html).map_err(|err| format!("{url}: parse error: {err:?}"))?;
    let doc_node = document.document;
    let dom = &document.dom;

    let inline_css = extract_inline_css(dom, doc_node);
    let mut css_text = stylesheet_text_with_user_agent_defaults(&inline_css);
    let stylesheet_urls = extract_stylesheet_urls(dom, doc_node, url);
    let css_accept_header = [("Accept".to_string(), "text/css,*/*".to_string())];
    let sheet_requests: Vec<(&str, &[(String, String)])> = stylesheet_urls
        .iter()
        .map(|sheet_url| (sheet_url.as_str(), css_accept_header.as_slice()))
        .collect();
    for (result, sheet_url) in renderer
        .fetch_all_or_speculate(&sheet_requests)
        .into_iter()
        .zip(stylesheet_urls.iter())
    {
        match result {
            Ok((resp, _, _)) if resp.status == 200 => {
                let sheet_css = String::from_utf8_lossy(&resp.body);
                css_text.push_str(&sheet_css);
                css_text.push('\n');
            }
            Ok((resp, _, _)) => {
                eprintln!(
                    "[SilkSurf] Navigation stylesheet {sheet_url}: HTTP {}",
                    resp.status
                );
            }
            Err(err) => {
                eprintln!(
                    "[SilkSurf] Navigation stylesheet {sheet_url}: fetch error: {}",
                    err.message
                );
            }
        }
    }

    let image_urls = extract_image_urls(dom, doc_node, url);
    let images = {
        let mut image_cache = image_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        fetch_decoded_images(&mut renderer, &mut image_cache, &image_urls)
    };
    let script_texts = load_document_script_texts(&mut renderer, dom, doc_node, url);
    let module_texts = load_document_module_texts(&mut renderer, dom, doc_node, url);

    Ok(BrowserPagePayload {
        url: url.to_string(),
        html,
        css_text,
        script_texts,
        module_texts,
        images,
        render_config: config.clone(),
        parsed_document: Some(document),
    })
}

pub(crate) fn build_browser_page(payload: BrowserPagePayload) -> Result<BrowserPage, String> {
    build_browser_page_with_buffers(payload, BrowserFrameBuffers::default())
        .map_err(|err| err.message)
}

pub(crate) fn build_browser_page_with_buffers(
    payload: BrowserPagePayload,
    buffers: BrowserFrameBuffers,
) -> Result<BrowserPage, BrowserPageBuildError> {
    build_browser_page_with_buffers_for_height(payload, buffers, None)
}

pub(crate) fn build_browser_page_with_buffers_for_height(
    mut payload: BrowserPagePayload,
    buffers: BrowserFrameBuffers,
    live_window_height: Option<u32>,
) -> Result<BrowserPage, BrowserPageBuildError> {
    let trace_build = std::env::var_os("SILKSURF_TRACE_APP_FRAME").is_some()
        || std::env::var_os("SILKSURF_TRACE_NAV_BUILD").is_some();
    let build_start = std::time::Instant::now();
    let phase_start = std::time::Instant::now();
    let document = match payload.parsed_document.take() {
        Some(document) => document,
        None => match parse_html(&payload.html) {
            Ok(document) => document,
            Err(err) => {
                return Err(BrowserPageBuildError {
                    message: format!("{}: parse error: {err:?}", payload.url),
                    buffers,
                });
            }
        },
    };
    trace_navigation_build_phase(trace_build, &payload.url, "html", phase_start.elapsed());
    let doc_node = document.document;
    let dom = document.dom;
    let phase_start = std::time::Instant::now();
    let Some(stylesheet) = dom.with_interner_mut(|interner| {
        silksurf_css::parse_stylesheet_with_interner(&payload.css_text, interner).ok()
    }) else {
        return Err(BrowserPageBuildError {
            message: format!("{}: CSS parse failed", payload.url),
            buffers,
        });
    };
    trace_navigation_build_phase(trace_build, &payload.url, "css", phase_start.elapsed());
    let scripts = if payload.script_texts.is_empty() {
        extract_inline_scripts(&dom, doc_node)
    } else {
        payload.script_texts
    };
    let mut executed_script_nodes = initial_executed_script_nodes(&dom, doc_node, &payload.url);
    let viewport = Rect {
        x: 0.0,
        y: BROWSER_CHROME_HEIGHT,
        width: FRAME_WIDTH as f32,
        height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
    };
    let style_index = StyleIndex::for_viewport(&stylesheet, viewport.width, viewport.height);
    let dom_arc = Arc::new(Mutex::new(dom));
    let cookie_host = url::Url::parse(&payload.url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string))
        .unwrap_or_default();
    let mut js_ctx = SilkContext::with_dom_and_cookies(
        &dom_arc,
        &payload.render_config.cookie_jar,
        &payload.render_config.top_level_site,
        &cookie_host,
    );
    {
        let mut dom = dom_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = dom.take_dirty_nodes();
    }
    let phase_start = std::time::Instant::now();
    let script_phase_start = phase_start;
    let static_eval_start = std::time::Instant::now();
    for (idx, script) in scripts.iter().enumerate() {
        if script.len() > max_navigation_script_bytes() {
            eprintln!(
                "[SilkSurf] Navigation script {idx}: {} bytes skipped",
                script.len()
            );
            continue;
        }
        trace_navigation_script(trace_build, idx, script.len(), "start", None);
        let script_start = std::time::Instant::now();
        if let Err(err) = js_ctx.eval(script) {
            eprintln!("[SilkSurf] Navigation script {idx} error: {err}");
        }
        trace_navigation_script(
            trace_build,
            idx,
            script.len(),
            "done",
            Some(script_start.elapsed()),
        );
    }
    trace_navigation_script_phase(trace_build, "static-eval", static_eval_start.elapsed());
    let jobs_start = std::time::Instant::now();
    js_ctx.run_pending_jobs();
    trace_navigation_script_phase(trace_build, "static-jobs", jobs_start.elapsed());
    let host_callbacks_start = std::time::Instant::now();
    drain_initial_host_callbacks(&mut js_ctx);
    trace_navigation_script_phase(
        trace_build,
        "static-host-callbacks",
        host_callbacks_start.elapsed(),
    );
    let dirty_drain_start = std::time::Instant::now();
    let dynamic_dirty_nodes = take_dom_dirty_nodes(&dom_arc);
    trace_navigation_script_phase(trace_build, "dirty-drain", dirty_drain_start.elapsed());
    let dynamic_start = std::time::Instant::now();
    execute_dynamic_classic_scripts(
        &payload.url,
        &payload.render_config,
        &dom_arc,
        &mut js_ctx,
        &mut executed_script_nodes,
        dynamic_dirty_nodes,
        trace_build,
    );
    trace_navigation_script_phase(trace_build, "dynamic-total", dynamic_start.elapsed());
    let module_start = std::time::Instant::now();
    execute_static_module_scripts(
        &payload.url,
        &dom_arc,
        doc_node,
        &mut js_ctx,
        &payload.module_texts,
        trace_build,
    );
    trace_navigation_script_phase(trace_build, "module-total", module_start.elapsed());
    let trace_body_start = std::time::Instant::now();
    trace_navigation_body_data_fixture(trace_build, &dom_arc);
    trace_navigation_script_phase(trace_build, "trace-body", trace_body_start.elapsed());
    let trace_scripts_start = std::time::Instant::now();
    trace_navigation_script_nodes(trace_build, &dom_arc, &executed_script_nodes);
    trace_navigation_script_phase(
        trace_build,
        "trace-script-nodes",
        trace_scripts_start.elapsed(),
    );
    trace_navigation_build_phase(
        trace_build,
        &payload.url,
        "scripts",
        script_phase_start.elapsed(),
    );

    let phase_start = std::time::Instant::now();
    let dom_guard = dom_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let replaced_sizes =
        collect_image_replaced_sizes(&dom_guard, doc_node, &payload.url, &payload.images);
    let mut fused_workspace = FusedWorkspace::new();
    fused_workspace.run_with_replaced_sizes(
        &dom_guard,
        &stylesheet,
        &style_index,
        doc_node,
        viewport,
        &replaced_sizes,
    );
    let mut fused = fused_workspace.take_result();
    let mut display_list = silksurf_render::DisplayList {
        items: std::mem::take(&mut fused.display_items),
        tiles: None,
    };
    append_image_display_items(
        &dom_guard,
        &fused,
        &payload.url,
        &payload.images,
        &mut display_list.items,
    );
    let link_targets = collect_link_targets(&dom_guard, &display_list.items, &payload.url);
    let input_targets = collect_input_targets(&dom_guard, &fused);
    drop(dom_guard);
    trace_navigation_build_phase(
        trace_build,
        &payload.url,
        "layout-paint",
        phase_start.elapsed(),
    );

    let phase_start = std::time::Instant::now();
    let document_height = browser_frame_height(&display_list.items, BROWSER_CHROME_HEIGHT as u32);
    display_list = tile_browser_document_display_list(display_list, document_height);
    let bitmap_height = browser_page_bitmap_height(document_height, live_window_height);
    trace_navigation_build_phase(trace_build, &payload.url, "tiles", phase_start.elapsed());
    let BrowserFrameBuffers { mut rgba, mut argb } = buffers;
    let mut viewport_item_indices = Vec::new();
    let phase_start = std::time::Instant::now();
    if rasterize_browser_viewport_argb_direct(
        &display_list,
        0,
        bitmap_height,
        &mut argb,
        &mut viewport_item_indices,
    ) {
        trace_navigation_build_phase(
            trace_build,
            &payload.url,
            "argb-direct",
            phase_start.elapsed(),
        );
        trace_navigation_build_buffer(trace_build, &payload.url, "rgba", rgba.len());
    } else {
        trace_navigation_build_phase(
            trace_build,
            &payload.url,
            "argb-direct-miss",
            phase_start.elapsed(),
        );
        let phase_start = std::time::Instant::now();
        rasterize_browser_viewport_into(
            &display_list,
            0,
            bitmap_height,
            &mut rgba,
            &mut viewport_item_indices,
        );
        trace_navigation_build_phase(trace_build, &payload.url, "raster", phase_start.elapsed());
        trace_navigation_build_buffer(trace_build, &payload.url, "rgba", rgba.len());
        let phase_start = std::time::Instant::now();
        let (resize_elapsed, pack_elapsed) = rgba_bytes_to_argb_words_into_timed(&rgba, &mut argb);
        trace_navigation_build_phase(trace_build, &payload.url, "argb-resize", resize_elapsed);
        trace_navigation_build_phase(trace_build, &payload.url, "argb-pack", pack_elapsed);
        trace_navigation_build_phase(trace_build, &payload.url, "argb", phase_start.elapsed());
    }
    trace_navigation_build_buffer(trace_build, &payload.url, "argb", argb.len() * 4);
    let phase_start = std::time::Instant::now();
    trace_navigation_build_phase(
        trace_build,
        &payload.url,
        "focus-viewport-cache",
        phase_start.elapsed(),
    );
    {
        let mut dom = dom_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = dom.take_dirty_nodes();
    }
    trace_navigation_build_phase(trace_build, &payload.url, "total", build_start.elapsed());
    Ok(BrowserPage {
        frame: BrowserFrame {
            url: payload.url,
            argb,
            raster_height: document_height,
            bitmap_height,
            bitmap_scroll_y: 0,
            focus_viewport_cache: None,
            focus_viewport_retained_sent: false,
            current_view_retained_sent: false,
            navigation_start_retained_sent: false,
            scroll_viewport_caches: Vec::new(),
            link_targets,
            input_targets,
        },
        runtime: BrowserPageRuntime {
            dom: dom_arc,
            document: doc_node,
            stylesheet,
            style_index,
            viewport,
            js_ctx,
            fused,
            fused_workspace,
            display_list,
            images: payload.images,
            rgba,
            damage_scratch: silksurf_render::DamageScratch::default(),
            viewport_item_indices,
        },
    })
}

pub(crate) fn browser_page_bitmap_height(
    document_height: u32,
    live_window_height: Option<u32>,
) -> u32 {
    live_window_height.map_or_else(
        || initial_browser_window_height(document_height),
        |height| height.max(BROWSER_CHROME_HEIGHT as u32),
    )
}

pub(crate) fn trace_navigation_build_phase(
    enabled: bool,
    url: &str,
    phase: &str,
    elapsed: std::time::Duration,
) {
    if enabled {
        eprintln!("[SilkSurf] Navigation build {phase}: {elapsed:?} for {url}");
    }
}

pub(crate) fn trace_navigation_build_buffer(enabled: bool, url: &str, name: &str, bytes: usize) {
    if enabled {
        eprintln!("[SilkSurf] Navigation build {name} buffer: {bytes} bytes for {url}");
    }
}

pub(crate) fn trace_navigation_script(
    enabled: bool,
    index: usize,
    bytes: usize,
    state: &str,
    elapsed: Option<std::time::Duration>,
) {
    if !enabled {
        return;
    }
    if let Some(elapsed) = elapsed {
        eprintln!("[SilkSurf] Navigation script {index} {state}: {bytes} bytes in {elapsed:?}");
    } else {
        eprintln!("[SilkSurf] Navigation script {index} {state}: {bytes} bytes");
    }
}

pub(crate) fn trace_navigation_script_phase(
    enabled: bool,
    name: &str,
    elapsed: std::time::Duration,
) {
    if enabled {
        eprintln!("[SilkSurf] Navigation script phase {name}: {elapsed:?}");
    }
}

pub(crate) fn initial_executed_script_nodes(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> HashSet<silksurf_dom::NodeId> {
    let mut nodes = HashSet::new();
    collect_classic_script_nodes(dom, root, base_url, &mut nodes);
    nodes
}

pub(crate) fn execute_dynamic_classic_scripts(
    base_url: &str,
    config: &BrowserRenderConfig,
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    js_ctx: &mut SilkContext,
    executed_nodes: &mut HashSet<silksurf_dom::NodeId>,
    mut dirty_nodes: Vec<silksurf_dom::NodeId>,
    trace_build: bool,
) {
    for round in 0..MAX_DYNAMIC_SCRIPT_ROUNDS {
        let round_start = std::time::Instant::now();
        let find_start = std::time::Instant::now();
        let scripts = dynamic_classic_script_refs(base_url, dom_arc, executed_nodes, &dirty_nodes);
        trace_navigation_dynamic_phase(trace_build, round, "find", find_start.elapsed());
        if scripts.is_empty() {
            return;
        }
        execute_dynamic_script_round(base_url, config, js_ctx, trace_build, round, &scripts);
        for script in scripts {
            executed_nodes.insert(script.node);
        }
        let jobs_start = std::time::Instant::now();
        js_ctx.run_pending_jobs();
        trace_navigation_dynamic_phase(trace_build, round, "jobs", jobs_start.elapsed());
        let callbacks_start = std::time::Instant::now();
        drain_initial_host_callbacks(js_ctx);
        trace_navigation_dynamic_phase(
            trace_build,
            round,
            "host-callbacks",
            callbacks_start.elapsed(),
        );
        let dirty_start = std::time::Instant::now();
        dirty_nodes = take_dom_dirty_nodes(dom_arc);
        trace_navigation_dynamic_phase(trace_build, round, "dirty-drain", dirty_start.elapsed());
        trace_navigation_dynamic_phase(trace_build, round, "total", round_start.elapsed());
    }
    eprintln!(
        "[SilkSurf] Navigation dynamic scripts stopped after {MAX_DYNAMIC_SCRIPT_ROUNDS} rounds"
    );
}

pub(crate) fn execute_static_module_scripts(
    base_url: &str,
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    root: silksurf_dom::NodeId,
    js_ctx: &mut SilkContext,
    module_texts: &[(String, String)],
    trace_build: bool,
) {
    if module_texts.is_empty() {
        return;
    }
    let root_urls = {
        let dom = dom_arc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        external_module_script_urls(&dom, root, base_url)
    };
    for (idx, root_url) in dedupe_resource_urls(&root_urls).iter().enumerate() {
        let root_path = module_path_for_url(root_url);
        let root_len = module_texts
            .iter()
            .find_map(|(path, text)| (path == &root_path).then_some(text.len()))
            .unwrap_or(0);
        let module_start = std::time::Instant::now();
        match js_ctx.eval_module_graph(&root_path, module_texts) {
            Ok(()) => trace_navigation_script(
                trace_build,
                idx,
                root_len,
                "module-done",
                Some(module_start.elapsed()),
            ),
            Err(err) => eprintln!("[SilkSurf] Module {root_url} error: {err}"),
        }
    }
    js_ctx.run_pending_jobs();
}

pub(crate) fn take_dom_dirty_nodes(
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
) -> Vec<silksurf_dom::NodeId> {
    let mut dom = dom_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    dom.take_dirty_nodes()
}

pub(crate) fn dynamic_classic_script_refs(
    base_url: &str,
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    executed_nodes: &HashSet<silksurf_dom::NodeId>,
    dirty_nodes: &[silksurf_dom::NodeId],
) -> Vec<DocumentScriptNode> {
    let dom = dom_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut scripts = Vec::new();
    let mut seen_nodes = HashSet::new();
    for &node in dirty_nodes {
        if !seen_nodes.insert(node) {
            continue;
        }
        if let Some(source) = script_ref_for_node(&dom, node, base_url) {
            scripts.push(DocumentScriptNode { node, source });
        }
    }
    scripts
        .into_iter()
        .filter(|script| !executed_nodes.contains(&script.node))
        .collect()
}

pub(crate) fn execute_dynamic_script_round(
    base_url: &str,
    config: &BrowserRenderConfig,
    js_ctx: &mut SilkContext,
    trace_build: bool,
    round: usize,
    scripts: &[DocumentScriptNode],
) {
    let urls_start = std::time::Instant::now();
    let external_urls = dynamic_external_script_urls(base_url, scripts);
    trace_navigation_dynamic_phase(trace_build, round, "urls", urls_start.elapsed());
    let fetch_start = std::time::Instant::now();
    let fetched = fetch_dynamic_external_script_texts(config, &external_urls, trace_build, round);
    trace_navigation_dynamic_phase(trace_build, round, "fetch-total", fetch_start.elapsed());
    let eval_start = std::time::Instant::now();
    for (idx, script) in scripts.iter().enumerate() {
        let Some(text) = dynamic_script_text(base_url, script, &fetched) else {
            continue;
        };
        execute_dynamic_script_text(js_ctx, trace_build, round, idx, text);
    }
    trace_navigation_dynamic_phase(trace_build, round, "eval-total", eval_start.elapsed());
}

pub(crate) fn dynamic_external_script_urls(
    base_url: &str,
    scripts: &[DocumentScriptNode],
) -> Vec<String> {
    scripts
        .iter()
        .filter_map(|script| match &script.source {
            DocumentScriptRef::External(url) => Some(resolve_resource_url(base_url, url)),
            DocumentScriptRef::Inline(_) => None,
        })
        .filter(|url| !url.is_empty())
        .collect()
}

pub(crate) fn fetch_dynamic_external_script_texts(
    config: &BrowserRenderConfig,
    urls: &[String],
    trace_build: bool,
    round: usize,
) -> Vec<(String, String)> {
    if urls.is_empty() {
        return Vec::new();
    }
    let renderer_start = std::time::Instant::now();
    let mut renderer = match ephemeral_renderer_from_config(config) {
        Ok(renderer) => renderer,
        Err(message) => {
            eprintln!("[SilkSurf] Navigation dynamic script renderer: {message}");
            return Vec::new();
        }
    };
    trace_navigation_dynamic_phase(trace_build, round, "renderer", renderer_start.elapsed());
    let request_start = std::time::Instant::now();
    let texts = fetch_external_script_texts(&mut renderer, urls);
    trace_navigation_dynamic_phase(
        trace_build,
        round,
        "fetch-requests",
        request_start.elapsed(),
    );
    texts
}

pub(crate) fn dynamic_script_text<'a>(
    base_url: &str,
    script: &'a DocumentScriptNode,
    fetched: &'a [(String, String)],
) -> Option<&'a str> {
    match &script.source {
        DocumentScriptRef::Inline(text) => Some(text.as_str()),
        DocumentScriptRef::External(url) => {
            let resolved = resolve_resource_url(base_url, url);
            fetched
                .iter()
                .find_map(|(fetched_url, text)| (fetched_url == &resolved).then_some(text.as_str()))
        }
    }
}

pub(crate) fn execute_dynamic_script_text(
    js_ctx: &mut SilkContext,
    trace_build: bool,
    round: usize,
    index: usize,
    script: &str,
) {
    if script.len() > max_navigation_script_bytes() {
        eprintln!(
            "[SilkSurf] Navigation dynamic script {round}.{index}: {} bytes skipped",
            script.len()
        );
        return;
    }
    trace_navigation_dynamic_script(trace_build, round, index, script.len(), "start", None);
    let script_start = std::time::Instant::now();
    if let Err(err) = js_ctx.eval(script) {
        eprintln!("[SilkSurf] Navigation dynamic script {round}.{index} error: {err}");
    }
    trace_navigation_dynamic_script(
        trace_build,
        round,
        index,
        script.len(),
        "done",
        Some(script_start.elapsed()),
    );
}

pub(crate) fn trace_navigation_dynamic_script(
    enabled: bool,
    round: usize,
    index: usize,
    bytes: usize,
    state: &str,
    elapsed: Option<std::time::Duration>,
) {
    if !enabled {
        return;
    }
    if let Some(elapsed) = elapsed {
        eprintln!(
            "[SilkSurf] Navigation dynamic script {round}.{index} {state}: {bytes} bytes in {elapsed:?}"
        );
    } else {
        eprintln!("[SilkSurf] Navigation dynamic script {round}.{index} {state}: {bytes} bytes");
    }
}

pub(crate) fn trace_navigation_dynamic_phase(
    enabled: bool,
    round: usize,
    name: &str,
    elapsed: std::time::Duration,
) {
    if enabled {
        eprintln!("[SilkSurf] Navigation dynamic phase {round} {name}: {elapsed:?}");
    }
}

pub(crate) fn trace_navigation_body_data_fixture(
    enabled: bool,
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
) {
    if !enabled {
        return;
    }
    let dom = dom_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(body) = first_element_by_name(&dom, silksurf_dom::NodeId::from_raw(0), "body") else {
        return;
    };
    if let Some(value) = element_attribute(&dom, body, "data-fixture") {
        eprintln!("[SilkSurf] Navigation DOM body data-fixture={value}");
    }
    if let Some(value) = element_attribute(&dom, body, "data-dynamic-script") {
        eprintln!("[SilkSurf] Navigation DOM body data-dynamic-script={value}");
    }
    if let Some(value) = element_attribute(&dom, body, "data-module-graph") {
        eprintln!("[SilkSurf] Navigation DOM body data-module-graph={value}");
    }
}

pub(crate) fn trace_navigation_script_nodes(
    enabled: bool,
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    script_nodes: &HashSet<silksurf_dom::NodeId>,
) {
    if !enabled {
        return;
    }
    let dom = dom_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for &node in script_nodes {
        if let Some(src) = element_attribute(&dom, node, "src") {
            let text_bytes = script_text_content(&dom, node).len();
            eprintln!("[SilkSurf] Navigation DOM script src={src} text_bytes={text_bytes}");
        }
    }
}

pub(crate) fn first_element_by_name(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    name: &str,
) -> Option<silksurf_dom::NodeId> {
    if dom
        .element_name(node)
        .ok()
        .flatten()
        .is_some_and(|element| element.eq_ignore_ascii_case(name))
    {
        return Some(node);
    }
    for &child in dom.children(node).ok()? {
        if let Some(found) = first_element_by_name(dom, child, name) {
            return Some(found);
        }
    }
    None
}

pub(crate) fn handle_revalidation(
    handle: silksurf_engine::speculative::RevalidationHandle,
    renderer: &mut SpeculativeRenderer,
    url: &str,
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    doc_node: silksurf_dom::NodeId,
    css_text: &str,
    viewport: Rect,
    fused: &FusedResult,
    raster_buf: &mut Vec<u8>,
) -> Result<(), String> {
    let result = handle
        .wait()
        .map_err(|err| format!("Revalidation error: {}", err.message))?;
    if !result.changed {
        eprintln!(
            "[SilkSurf] Revalidation: 304 NOT MODIFIED in {:?} -- cached render is current, no re-render",
            result.rtt
        );
        return Ok(());
    }
    apply_changed_revalidation(
        result, renderer, url, dom_arc, doc_node, css_text, viewport, fused, raster_buf,
    )
}

pub(crate) fn apply_changed_revalidation(
    result: silksurf_engine::speculative::RevalidationResult,
    renderer: &mut SpeculativeRenderer,
    url: &str,
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    doc_node: silksurf_dom::NodeId,
    css_text: &str,
    viewport: Rect,
    fused: &FusedResult,
    raster_buf: &mut Vec<u8>,
) -> Result<(), String> {
    eprintln!(
        "[SilkSurf] Revalidation: CONTENT CHANGED (200) in {:?}",
        result.rtt
    );
    let Some(response) = result.response else {
        return Ok(());
    };
    renderer.update_cache(url, &response);
    eprintln!(
        "[SilkSurf] Cache updated ({} bytes)",
        renderer.cache_bytes()
    );
    rerender_revalidated_cache(
        renderer, url, dom_arc, doc_node, css_text, viewport, fused, raster_buf,
    )
}

pub(crate) fn rerender_revalidated_cache(
    renderer: &mut SpeculativeRenderer,
    url: &str,
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    doc_node: silksurf_dom::NodeId,
    css_text: &str,
    viewport: Rect,
    fused: &FusedResult,
    raster_buf: &mut Vec<u8>,
) -> Result<(), String> {
    let new_html = String::from_utf8_lossy(
        &renderer
            .cache
            .get(url)
            .map(|entry| entry.body.clone())
            .unwrap_or_default(),
    )
    .to_string();
    let new_doc = silksurf_engine::parse_html(&new_html)
        .map_err(|err| format!("Revalidation parse error: {err:?}"))?;
    let diff = revalidation_dom_diff(dom_arc, doc_node, &new_doc);
    if diff.is_empty() {
        eprintln!("[SilkSurf] DOM diff: no structural changes (cached render valid)");
        return Ok(());
    }
    rerender_revalidation_diff(
        renderer, css_text, viewport, fused, raster_buf, &new_doc, &diff,
    );
    Ok(())
}

pub(crate) fn revalidation_dom_diff(
    dom_arc: &Arc<Mutex<silksurf_dom::Dom>>,
    doc_node: silksurf_dom::NodeId,
    new_doc: &silksurf_engine::ParsedDocument,
) -> silksurf_dom::diff::DomDiff {
    let orig_dom = dom_arc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    silksurf_dom::diff::diff_doms(&orig_dom, doc_node, &new_doc.dom, new_doc.document)
}

pub(crate) fn rerender_revalidation_diff(
    renderer: &mut SpeculativeRenderer,
    css_text: &str,
    viewport: Rect,
    fused: &FusedResult,
    raster_buf: &mut Vec<u8>,
    new_doc: &silksurf_engine::ParsedDocument,
    diff: &silksurf_dom::diff::DomDiff,
) {
    eprintln!(
        "[SilkSurf] DOM diff: {} changed, {} added, {} removed nodes -- re-rendering new DOM",
        diff.changed.len(),
        diff.added.len(),
        diff.removed.len(),
    );
    let rerender_t0 = std::time::Instant::now();
    let (css_elapsed, new_stylesheet) = parse_revalidation_stylesheet(renderer, css_text, new_doc);
    let fused_t0 = std::time::Instant::now();
    let mut new_fused =
        fused_style_layout_paint(&new_doc.dom, &new_stylesheet, new_doc.document, viewport);
    let fused_elapsed = fused_t0.elapsed();
    let raster_elapsed = raster_revalidation_diff(diff, fused, &mut new_fused, raster_buf);
    let total = rerender_t0.elapsed();
    let new_styled = new_fused
        .styles
        .iter()
        .filter(|style| style.is_some())
        .count();
    eprintln!(
        "[SilkSurf] Re-render ({new_styled} styled nodes): CSS {css_elapsed:?} + fused {fused_elapsed:?} + raster {raster_elapsed:?} = {total:?}",
    );
}

pub(crate) fn parse_revalidation_stylesheet(
    renderer: &mut SpeculativeRenderer,
    css_text: &str,
    new_doc: &silksurf_engine::ParsedDocument,
) -> (std::time::Duration, silksurf_css::Stylesheet) {
    let css_t0 = std::time::Instant::now();
    let stylesheet = new_doc
        .dom
        .with_interner_mut(|interner| renderer.get_or_parse_stylesheet(css_text, interner))
        .unwrap_or_else(|| {
            silksurf_css::parse_stylesheet_with_interner(
                "",
                &mut silksurf_core::SilkInterner::new(),
            )
            // UNWRAP-OK: empty CSS is always a valid stylesheet.
            .unwrap()
        });
    (css_t0.elapsed(), stylesheet)
}

pub(crate) fn raster_revalidation_diff(
    diff: &silksurf_dom::diff::DomDiff,
    fused: &FusedResult,
    new_fused: &mut FusedResult,
    raster_buf: &mut Vec<u8>,
) -> std::time::Duration {
    let raster_t0 = std::time::Instant::now();
    let damage = text_only_diff_damage_rect(diff, fused, new_fused);
    let new_display_list = silksurf_render::DisplayList {
        items: std::mem::take(&mut new_fused.display_items),
        tiles: None,
    };
    if let Some(damage) = damage {
        let mut damage_scratch = silksurf_render::DamageScratch::default();
        silksurf_render::rasterize_skia_damage_into(
            &new_display_list,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            damage,
            raster_buf,
            &mut damage_scratch,
        );
        eprintln!(
            "[SilkSurf] Re-render damage rect: {}x{} at ({}, {})",
            damage.width, damage.height, damage.x, damage.y
        );
    } else {
        silksurf_render::rasterize_skia_into(
            &new_display_list,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            raster_buf,
        );
    }
    raster_t0.elapsed()
}

pub(crate) fn renderer_from_config(
    config: &BrowserRenderConfig,
) -> Result<SpeculativeRenderer, String> {
    let mut renderer = build_renderer_from_config(config)?;
    renderer.attach_cookie_context(config.cookie_jar.clone(), config.top_level_site.clone());
    Ok(renderer)
}

fn build_renderer_from_config(config: &BrowserRenderConfig) -> Result<SpeculativeRenderer, String> {
    if config.insecure {
        return Ok(SpeculativeRenderer::with_insecure());
    }
    if let Some(ref ca_path) = config.tls_ca_file {
        return SpeculativeRenderer::with_extra_ca_file(ca_path)
            .map_err(|err| format!("--tls-ca-file: {}", err.message));
    }
    if config.platform_verifier {
        #[cfg(feature = "platform-verifier")]
        {
            return SpeculativeRenderer::with_platform_verifier()
                .map_err(|err| format!("TLS platform verifier: {}", err.message));
        }
        #[cfg(not(feature = "platform-verifier"))]
        {
            return Err("rebuild with --features platform-verifier".to_string());
        }
    }
    Ok(SpeculativeRenderer::new())
}

pub(crate) fn ephemeral_renderer_from_config(
    config: &BrowserRenderConfig,
) -> Result<SpeculativeRenderer, String> {
    let mut renderer = build_ephemeral_renderer_from_config(config)?;
    renderer.attach_cookie_context(config.cookie_jar.clone(), config.top_level_site.clone());
    Ok(renderer)
}

fn build_ephemeral_renderer_from_config(
    config: &BrowserRenderConfig,
) -> Result<SpeculativeRenderer, String> {
    if config.insecure {
        return Ok(SpeculativeRenderer::with_insecure_ephemeral());
    }
    if let Some(ref ca_path) = config.tls_ca_file {
        return SpeculativeRenderer::with_extra_ca_file_ephemeral(ca_path)
            .map_err(|err| format!("--tls-ca-file: {}", err.message));
    }
    if config.platform_verifier {
        #[cfg(feature = "platform-verifier")]
        {
            return SpeculativeRenderer::with_platform_verifier_ephemeral()
                .map_err(|err| format!("TLS platform verifier: {}", err.message));
        }
        #[cfg(not(feature = "platform-verifier"))]
        {
            return Err("rebuild with --features platform-verifier".to_string());
        }
    }
    Ok(SpeculativeRenderer::new_ephemeral())
}

pub(crate) fn stylesheet_text_with_user_agent_defaults(document_css: &str) -> String {
    let mut css_text =
        String::with_capacity(DEFAULT_USER_AGENT_STYLESHEET.len() + document_css.len() + 1);
    css_text.push_str(DEFAULT_USER_AGENT_STYLESHEET);
    css_text.push('\n');
    css_text.push_str(document_css);
    css_text
}

#[cfg(test)]
mod tests {
    // Module split from the former single-file binary; the crate root
    // re-exports every module so sibling items resolve by bare name.
    #[allow(clippy::wildcard_imports)]
    use crate::*;
    use silksurf_render::DisplayItem;

    #[test]
    fn document_css_follows_user_agent_defaults() {
        let css = stylesheet_text_with_user_agent_defaults("body { margin: 0; }");
        let ua_pos = css.find("body {").expect("ua body rule");
        let document_pos = css
            .rfind("body { margin: 0; }")
            .expect("document body rule");

        assert!(ua_pos < document_pos);
    }

    #[test]
    fn build_browser_page_reuses_supplied_frame_buffer_capacity() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><p>Hello</p></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };
        let rgba_capacity = (FRAME_WIDTH * FRAME_HEIGHT * 4) as usize;
        let argb_capacity = (FRAME_WIDTH * FRAME_HEIGHT) as usize;
        let buffers = BrowserFrameBuffers {
            rgba: Vec::with_capacity(rgba_capacity),
            argb: Vec::with_capacity(argb_capacity),
        };

        let page = build_browser_page_with_buffers(payload, buffers).expect("payload builds page");

        assert!(page.runtime.rgba.capacity() >= rgba_capacity);
        assert!(page.frame.argb.capacity() >= argb_capacity);
    }

    #[test]
    fn browser_page_build_uses_supplied_parsed_document() {
        let parsed_html = concat!(
            "<!doctype html><html><body>",
            "<p>parsed handoff document</p>",
            "</body></html>"
        );
        let document = parse_html(parsed_html).expect("html parses");
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><p>fallback html</p></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: Some(document),
        };

        let page = build_browser_page(payload).expect("payload builds page");
        let text_items: Vec<&str> = page
            .runtime
            .display_list
            .items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert!(
            text_items
                .iter()
                .any(|text| text.contains("parsed handoff document"))
        );
        assert!(!text_items.iter().any(|text| text.contains("fallback html")));
    }

    #[test]
    fn browser_page_suppresses_style_and_script_metadata_text() {
        let inline_css = concat!(
            "body{background:#eee;width:60vw;margin:15vh auto;",
            "font-family:system-ui,sans-serif}",
            "h1{font-size:1.5em}",
            "div{opacity:0.8}",
            "a:link,a:visited{color:#348}"
        );
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: format!(
                concat!(
                    "<!doctype html><html><head><style>{}</style>",
                    "<script type=\"application/json\">hidden-script-text</script>",
                    "</head><body><div><h1>Example Domain</h1>",
                    "<p>This domain is for use in documentation examples ",
                    "without needing permission.</p>",
                    "<a href=\"https://www.iana.org/domains/example\">Learn more</a>",
                    "</div></body></html>"
                ),
                inline_css
            ),
            css_text: stylesheet_text_with_user_agent_defaults(inline_css),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };

        let page = build_browser_page(payload).expect("payload builds page");
        let text_items: Vec<&str> = page
            .runtime
            .display_list
            .items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert!(
            text_items
                .iter()
                .any(|text| text.contains("Example Domain"))
        );
        assert!(
            text_items
                .iter()
                .any(|text| text.contains("documentation examples"))
        );
        assert!(!text_items.iter().any(|text| text.contains("body{")));
        assert!(
            !text_items
                .iter()
                .any(|text| text.contains("hidden-script-text"))
        );
    }

    #[test]
    fn navigation_page_build_uses_live_window_height() {
        let payload = BrowserPagePayload {
            url: "https://example.com/results/".to_string(),
            html: "<!doctype html><html><body><p>Result</p></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };

        let page = build_browser_page_with_buffers_for_height(
            payload,
            BrowserFrameBuffers::default(),
            Some(FRAME_HEIGHT),
        )
        .expect("payload builds page");

        assert_eq!(page.frame.bitmap_height, FRAME_HEIGHT);
        assert_eq!(page.frame.argb.len(), (FRAME_WIDTH * FRAME_HEIGHT) as usize);
    }

    #[test]
    fn navigation_build_defers_offscreen_focus_cache() {
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

        assert!(!page.frame.input_targets.is_empty());
        assert!(
            first_focus_target_scroll(
                &page.frame.input_targets,
                page.frame.raster_height,
                page.frame.bitmap_height,
                BROWSER_CHROME_HEIGHT as u32,
            )
            .is_some()
        );
        assert!(page.frame.focus_viewport_cache.is_none());
        assert!(!page.frame.focus_viewport_retained_sent);
    }

    #[test]
    fn navigation_generation_marks_stale_results() {
        let mut state = test_browser_state("https://example.com/a");
        state.navigation_generation = 7;
        state.navigation_pending = true;
        state.pending_history = Some(PendingHistoryAction::Push);
        state.status_text = "loading".to_string();

        state.navigation_generation = state.navigation_generation.saturating_add(1);
        state.navigation_pending = false;
        state.pending_history = None;
        state.status_text = "ready".to_string();

        assert_ne!(7, state.navigation_generation);
        assert!(!state.navigation_pending);
        assert_eq!(state.pending_history, None);
        assert_eq!(state.status_text, "ready");
    }

    #[test]
    fn browser_page_payload_builds_retained_runtime() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><p id=\"msg\">Hello</p><script>requestAnimationFrame(function(){setTimeout(function(){document.getElementById('msg').firstChild.textContent='Runtime';},0);});</script></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };

        let mut page = build_browser_page(payload).expect("payload builds page");
        assert_eq!(page.frame.url, "https://example.com/");
        assert!(page.runtime.js_ctx.has_pending_host_callbacks());

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
        assert!(find_text_node(&dom, page.runtime.document, "Runtime").is_some());
    }

    #[test]
    fn browser_page_payload_executes_external_script_text() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><p id=\"msg\">Hello</p><script src=\"/app.js\"></script></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: vec![
                "document.getElementById('msg').firstChild.textContent='External';".to_string(),
            ],
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };

        let page = build_browser_page(payload).expect("payload builds page");
        let dom = page
            .runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(find_text_node(&dom, page.runtime.document, "External").is_some());
    }

    #[test]
    fn browser_page_payload_executes_external_module_graph() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><script type=\"module\" src=\"/module.js\"></script></body></html>".to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: vec![
                (
                    "/module.js".to_string(),
                    "import { fixtureGraph } from '/module-child.js'; document.body.setAttribute('data-module-graph', fixtureGraph);".to_string(),
                ),
                (
                    "/module-child.js".to_string(),
                    "export const fixtureGraph = 'module-child';".to_string(),
                ),
            ],
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };

        let page = build_browser_page(payload).expect("payload builds page");
        let dom = page
            .runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let body = first_element_by_name(&dom, page.runtime.document, "body").expect("body exists");
        let attrs = dom.attributes(body).expect("body has attributes");
        assert!(attrs.iter().any(|attr| {
            attr.name.as_str() == "data-module-graph" && attr.value.as_str() == "module-child"
        }));
    }

    #[test]
    fn browser_page_payload_executes_dynamic_inline_script() {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: "<!doctype html><html><body><p id=\"msg\">Hello</p><script>\
                   var script = document.createElement('script');\
                   script.innerHTML = \"document.getElementById('msg').firstChild.textContent='Dynamic';\";\
                   document.head.appendChild(script);\
                   </script></body></html>"
                .to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: Vec::new(),
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };

        let page = build_browser_page(payload).expect("payload builds page");
        let dom = page
            .runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(find_text_node(&dom, page.runtime.document, "Dynamic").is_some());
    }
}
