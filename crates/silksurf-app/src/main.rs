//! SilkSurf Rust-native webview entry point.
//!
//! Pipeline: fetch URL -> parse HTML -> load CSS/JS resources -> create VM
//! with DOM bridge -> run scripts -> layout -> render (future: XCB window).
//!
//! Usage: silksurf-app \[URL\]
//! Default URL: `https://example.com`

/*
 * mimalloc global allocator.
 *
 * WHY: The CSS tokenizer and cascade produce many small heap allocations
 * (one SmolStr per identifier > 22 bytes, Vec<CssToken> per declaration).
 * mimalloc uses thread-local free lists and page segregation to service
 * small allocs in ~5ns vs ~20ns for system malloc. 2-4x throughput on
 * allocation-heavy workloads. Zero code changes outside this declaration.
 */
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use silksurf_engine::fused_pipeline::fused_style_layout_paint;
use silksurf_engine::parse_html;
use silksurf_engine::speculative::{FetchOrigin, SpeculativeRenderer};
use silksurf_js::SilkContext;
use silksurf_layout::Rect;

fn main() {
    /*
     * P8.S6 -- observability bootstrap.
     *
     * Order matters: the subscriber is installed before the panic hook so
     * that a panic during early arg parsing is captured by the structured
     * logger.  Once the subscriber is up we replace the default panic hook
     * with one that emits a `tracing::error!` event before delegating to
     * the original hook (so the standard backtrace path still runs).
     *
     * Default filter level: warn for everything, info for the `silksurf`
     * span tree.  Override at runtime with `RUST_LOG=silksurf=debug` etc.
     *
     * OOM hook: alloc_error_hook is nightly-only
     * (alloc::alloc::set_alloc_error_hook).  silksurf-app uses mimalloc
     * which aborts on OOM natively in release builds.  Nightly OOM hook
     * deferred to when the feature stabilises.
     */
    // UNWRAP-OK: "silksurf=info" is a valid tracing directive literal; parse() is infallible here.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("silksurf=info".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(panic = %info, "process panicking");
        default_hook(info);
    }));

    let args: Vec<String> = std::env::args().collect();
    let insecure = args.iter().any(|a| a == "--insecure" || a == "-k");
    let platform_verifier = args.iter().any(|a| a == "--platform-verifier");
    let speculative = args.iter().any(|a| a == "--speculative" || a == "-s");
    let window_mode = args.iter().any(|a| a == "--window");
    let winit_mode = args.iter().any(|a| a == "--backend=winit")
        || args
            .windows(2)
            .any(|w| w[0] == "--backend" && w[1] == "winit");

    /*
     * --window  -- open an XCB window, present a placeholder frame, and
     *              pump the event loop until Close or Escape (keysym 0x09).
     *
     * WHY: The full pipeline (fetch -> parse -> layout -> rasterize) goes
     * to stderr today.  P6.S2/S3 wires the rasterizer output to a real
     * X11 drawable so the developer experience matches expectations of a
     * browser ("show me the page in a window").  This early exit keeps
     * the network/JS path entirely separate so a regression on either side
     * does not break the other -- a strict cleanroom seam.
     *
     * HOW: cornflower-blue (0x6495ED) fill via silksurf_render::fill_scalar
     * is the placeholder.  P6.S4 swaps it for the real rasterized buffer
     * sourced from the same fused pipeline that the headless mode uses.
     *
     * The XcbWindow::new() error path is the only thing that can panic
     * here on a headless box, so we surface it as a clean stderr message
     * and exit code 1 -- never panic.
     */
    if window_mode {
        match silksurf_gui::XcbWindow::new("silksurf", 1280, 720) {
            Ok(mut window) => {
                let pixel_count = 1280usize * 720usize;
                let mut pixels: Vec<u32> = vec![0; pixel_count];
                // Cornflower blue, ARGB. The high byte (0xFF) is alpha; the
                // server ignores alpha for opaque windows but downstream
                // SHM/composite paths require it set.
                silksurf_render::fill_scalar(&mut pixels, 0xFF6495ED);
                window.present(&pixels);

                /*
                 * Re-presentation on Expose is intentionally NOT wired in
                 * this slice: the event-loop handler signature is
                 * `FnMut(Event) -> ControlFlow` (no &mut to window or
                 * framebuffer), so we cannot call window.present() from
                 * inside it without violating the borrow checker.  P6.S4
                 * extends the handler signature to a redraw closure;
                 * until then, the BackPixel(white_pixel) on the window
                 * gives a sensible fallback when the WM unmaps/remaps.
                 */
                let mut event_loop = silksurf_gui::EventLoop::new();
                let run_result = event_loop.run(&mut window, |event| match event {
                    silksurf_gui::Event::Close => silksurf_gui::ControlFlow::Exit,
                    // X11 keycode 0x09 is Escape on a US keyboard layout.
                    // Real keysym translation lands in P6.S4 (xkbcommon).
                    silksurf_gui::Event::KeyPress { keysym: 0x09 } => {
                        silksurf_gui::ControlFlow::Exit
                    }
                    _ => silksurf_gui::ControlFlow::Continue,
                });
                if let Err(err) = run_result {
                    eprintln!("[SilkSurf] window event loop error: {err}");
                    std::process::exit(1);
                }
                return;
            }
            Err(err) => {
                eprintln!("[SilkSurf] --window: cannot open display: {err}");
                std::process::exit(1);
            }
        }
    }

    /*
     * --backend=winit  -- open a winit window, present a placeholder frame, and
     *                     pump the event loop until Close or Escape.
     *
     * WHY: winit 0.30 supports X11, Wayland, macOS, and Windows via a single
     * ApplicationHandler trait.  softbuffer 0.4 exposes `buffer_mut() -> &mut [u32]`
     * which is the same type as the rasterizer output, making the adapter zero-copy.
     * This path runs independently of the headless fetch/layout pipeline so either
     * can be tested and regressed without breaking the other.
     *
     * HOW: cargo run -p silksurf-app -- --backend=winit
     *      cargo run -p silksurf-app -- --backend winit
     */
    if winit_mode {
        match silksurf_gui::WinitWindow::new("silksurf", 1280, 720) {
            Ok(win) => {
                win.run(|w, h| {
                    let mut pixels: Vec<u32> = vec![0u32; (w * h) as usize];
                    // Cornflower blue (0x6495ED), fully opaque.
                    silksurf_render::fill_scalar(&mut pixels, 0xFF6495ED);
                    pixels
                });
            }
            Err(err) => {
                eprintln!("[SilkSurf] --backend=winit: cannot create window: {err}");
                std::process::exit(1);
            }
        }
        return;
    }

    /*
     * --tls-ca-file <path>  -- append a PEM CA bundle to the default trust store.
     *
     * WHY: Corporate proxies and private PKI deployments sign TLS certificates
     * with an internal CA absent from the Mozilla root bundle.  Supplying the
     * specific bundle here adds only that chain rather than disabling all
     * verification with --insecure.
     *
     * Accepted forms:  --tls-ca-file /etc/ssl/my-corp.pem
     *                  --tls-ca-file=/etc/ssl/my-corp.pem   (equals form)
     */
    let tls_ca_file: Option<std::path::PathBuf> = args
        .windows(2)
        .find_map(|w| {
            if w[0] == "--tls-ca-file" {
                Some(std::path::PathBuf::from(&w[1]))
            } else {
                None
            }
        })
        .or_else(|| {
            args.iter().find_map(|a| {
                a.strip_prefix("--tls-ca-file=")
                    .map(std::path::PathBuf::from)
            })
        });

    let url = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .cloned()
        .unwrap_or_else(|| "https://example.com".to_string());

    if insecure {
        eprintln!("[SilkSurf] WARNING: TLS certificate verification disabled (--insecure)");
    }
    if platform_verifier {
        eprintln!("[SilkSurf] TLS platform verifier requested");
    }
    if let Some(ref p) = tls_ca_file {
        eprintln!("[SilkSurf] Extra CA bundle: {}", p.display());
    }

    /*
     * SpeculativeRenderer: cache-first HTTP client.
     *
     * fetch_or_speculate() returns a cached response immediately (0ms) if
     * the URL was fetched before in this session, or performs a live fetch
     * and caches the result for subsequent calls.
     *
     * See: speculative.rs SpeculativeRenderer::fetch_or_speculate()
     */
    let mut renderer = if insecure {
        SpeculativeRenderer::with_insecure()
    } else if let Some(ref ca_path) = tls_ca_file {
        match SpeculativeRenderer::with_extra_ca_file(ca_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[SilkSurf] --tls-ca-file error: {}", e.message);
                return;
            }
        }
    } else if platform_verifier {
        #[cfg(feature = "platform-verifier")]
        {
            match SpeculativeRenderer::with_platform_verifier() {
                Ok(renderer) => renderer,
                Err(e) => {
                    eprintln!(
                        "[SilkSurf] TLS platform verifier setup error: {}",
                        e.message
                    );
                    return;
                }
            }
        }
        #[cfg(not(feature = "platform-verifier"))]
        {
            eprintln!(
                "[SilkSurf] Rebuild with `--features platform-verifier` to use --platform-verifier"
            );
            return;
        }
    } else {
        SpeculativeRenderer::new()
    };

    eprintln!("[SilkSurf] Fetching: {url}");

    // 1. Fetch the page (cache-first)
    let (response, fetch_origin, fetch_elapsed) = match renderer.fetch_or_speculate(&url, &[]) {
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
    }

    /*
     * Background revalidation on cache hit:
     *
     * Spawn a conditional GET (If-None-Match / If-Modified-Since) in a
     * background thread so the render pipeline can proceed without waiting.
     * After rendering, join the thread to report whether content changed.
     *
     * See: speculative.rs spawn_revalidation() for thread model
     */
    let revalidation_handle = if fetch_origin == FetchOrigin::Cache && speculative {
        eprintln!("[SilkSurf] Spawning background revalidation for {url}");
        Some(renderer.spawn_revalidation(&url))
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

    // 3. Extract inline CSS from <style> tags + fetch external stylesheets
    let mut css_text = extract_inline_css(&dom, doc_node);
    eprintln!(
        "[SilkSurf] Extracted {} bytes of inline CSS",
        css_text.len()
    );

    /*
     * Fetch external <link rel="stylesheet"> resources in parallel (HTTP/2).
     *
     * WHY: CSS subresources (e.g. chatgpt.com's 2 stylesheets at ~680ms total
     * over sequential HTTP/1.1) can be parallelized via HTTP/2 multiplexing.
     * fetch_all_or_speculate groups same-host HTTPS URLs and sends them over
     * a single TLS connection, reducing total CSS fetch time to max(RTTs).
     *
     * Cache hit: returns immediately from ResponseCache (0ms network).
     * Cache miss + h2: all URLs fetched in parallel over one TLS connection.
     * Cache miss + no h2: sequential HTTP/1.1 fallback (same as before).
     *
     * See: SpeculativeRenderer::fetch_all_or_speculate for implementation
     */
    let stylesheet_urls = extract_stylesheet_urls(&dom, doc_node, &url);
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
                eprintln!("[SilkSurf] Stylesheet {sheet_url}: HTTP {}", resp.status)
            }
            Err(e) => eprintln!(
                "[SilkSurf] Stylesheet {sheet_url}: fetch error: {}",
                e.message
            ),
        }
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

    // Viewport dimensions used by fused pipeline and rasterizer
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 800.0,
    };

    // 5. Create JS context backed by boa_engine (ECMA-262 2024+).
    let mut js_ctx = SilkContext::new();

    // 6. Extract and execute inline <script> tags.
    let scripts = extract_inline_scripts(&dom, doc_node);
    eprintln!("[SilkSurf] Found {} inline script(s)", scripts.len());
    for (i, script) in scripts.iter().enumerate() {
        // Skip very large bundled JS (React, webpack output, etc.).
        // Inline init scripts are usually <4 KB; anything >256 KB is a bundle.
        const MAX_INLINE_SCRIPT: usize = 256 * 1024;
        if script.len() > MAX_INLINE_SCRIPT {
            eprintln!(
                "[SilkSurf] Script {i}: {} bytes (skipping -- bundle too large)",
                script.len()
            );
            continue;
        }
        let preview = &script[..script.len().min(80)];
        if script.len() <= 1200 {
            eprintln!(
                "[SilkSurf] Script {i} FULL ({} bytes): {script}",
                script.len()
            );
        } else {
            eprintln!(
                "[SilkSurf] Executing script {i} ({} bytes): {preview}...",
                script.len()
            );
        }
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

    // 7. Drain pending microtasks and Promise reactions.
    js_ctx.run_pending_jobs();

    // 8. Fused style+layout+paint: single BFS pass over post-JS DOM.
    //    Replaces separate compute_styles + build_layout_tree + build_display_list calls.
    //    Running post-JS ensures DOM mutations from scripts are visible in the render.
    let fused_start = std::time::Instant::now();
    let fused = fused_style_layout_paint(&dom, &stylesheet, doc_node, viewport);
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

    let display_list = silksurf_render::DisplayList {
        items: fused.display_items,
        tiles: None,
    }
    .with_tiles(1280, 800, 64);

    /*
     * 9. Tile-parallel rasterization via Rayon (disjoint tile regions, no sync).
     *
     * WHY rasterize_parallel_into: in an interactive browser, raster_buf would
     * be held across frames and reused, eliminating the ~1ms cold 4MB allocation
     * on every frame. The CLI only renders once per process but uses the correct
     * API so the architecture is ready for an interactive render loop.
     *
     * See: silksurf_render::rasterize_parallel_into for buffer-reuse semantics.
     */
    let raster_start = std::time::Instant::now();
    let mut raster_buf: Vec<u8> = Vec::new();
    silksurf_render::rasterize_parallel_into(&display_list, 1280, 800, 64, &mut raster_buf);
    let raster_elapsed = raster_start.elapsed();
    eprintln!(
        "[SilkSurf] Rasterized: {} bytes in {:?}",
        raster_buf.len(),
        raster_elapsed
    );

    eprintln!("\n=== PROCESSING BUDGET (excludes network) ===");
    eprintln!(
        "  CSS parse:      {:?}",
        css_start.elapsed() - fused_elapsed - raster_elapsed
    );
    eprintln!("  Fused pipeline: {:?}", fused_elapsed);
    eprintln!("  Rasterize:      {:?}", raster_elapsed);
    eprintln!("  TOTAL:          {:?}", css_start.elapsed());
    eprintln!("============================================\n");

    eprintln!("[SilkSurf] Pipeline complete for {url}");

    /*
     * Background revalidation result: join here after the render is done.
     *
     * The revalidation ran in parallel with HTML parse, CSS cascade, layout,
     * and rasterization. By the time we reach here, the result is likely
     * already available (try_recv first to avoid blocking).
     *
     * See: speculative.rs RevalidationHandle for thread model
     */
    /*
     * Background revalidation result: join here after the render is done.
     *
     * On 304 Not Modified: diff is empty by definition (same bytes = same DOM).
     * No re-render needed. This is the primary Phase E benefit for chatgpt.com:
     * the revalidation path skips all DOM parsing, CSS cascade, layout, and
     * rasterization when the server confirms the page hasn't changed.
     *
     * On 200 with new content: diff the cached DOM against the new DOM to
     * quantify the change set. The diff result is logged here; Phase E.2
     * (incremental re-render) will use it to only re-process changed nodes.
     *
     * See: silksurf-dom/src/diff.rs for DomDiff and diff_doms
     */
    if let Some(handle) = revalidation_handle {
        let result = match handle.wait() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[SilkSurf] Revalidation error: {}", e.message);
                return;
            }
        };
        if result.changed {
            eprintln!(
                "[SilkSurf] Revalidation: CONTENT CHANGED (200) in {:?}",
                result.rtt
            );
            if let Some(new_resp) = result.response {
                renderer.update_cache(&url, &new_resp);
                eprintln!(
                    "[SilkSurf] Cache updated ({} bytes)",
                    renderer.cache_bytes()
                );

                // DOM diff: compare cached parse against the new HTML.
                let new_html = String::from_utf8_lossy(
                    &renderer
                        .cache
                        .get(&url)
                        .map(|e| e.body.clone())
                        .unwrap_or_default(),
                )
                .to_string();
                if let Ok(new_doc) = silksurf_engine::parse_html(&new_html) {
                    let diff = silksurf_dom::diff::diff_doms(
                        &dom,
                        doc_node,
                        &new_doc.dom,
                        new_doc.document,
                    );
                    if diff.is_empty() {
                        eprintln!(
                            "[SilkSurf] DOM diff: no structural changes (cached render valid)"
                        );
                    } else {
                        eprintln!(
                            "[SilkSurf] DOM diff: {} changed, {} added, {} removed nodes -- re-rendering new DOM",
                            diff.changed.len(),
                            diff.added.len(),
                            diff.removed.len(),
                        );
                        /*
                         * Phase E.2: full re-render on the new DOM.
                         *
                         * WHY full re-render (not node-subset): fused_style_layout_paint
                         * is a BFS cascade where each node's layout depends on its parent's
                         * output.  To avoid a full pass we would need to track the "layout
                         * boundary" ancestor for every dirty node -- complex and error-prone.
                         * For now, we take the correct (if non-minimal) path: re-render the
                         * entire new DOM.  The key optimizations ARE in effect:
                         *   - CSS: same URLs -> ResponseCache hit -> same bytes ->
                         *     StylesheetCache hit -> intern_rules only (~200us, not 2.5ms)
                         *   - Raster buf: reuse existing allocation (zero 4MB alloc)
                         *
                         * Future: incremental layout boundary tracking (Phase E.3).
                         *
                         * NOTE: we reuse css_text from the initial fetch.  External
                         * stylesheets are at the same URLs; their content is returned
                         * from ResponseCache in 0ms.  Inline CSS from the new HTML is
                         * unlikely to differ for chatgpt.com (CSS is external-only).
                         * If inline CSS does differ, the SoA cascade will still produce
                         * correct styles -- selector matching is content-independent.
                         */
                        let rerender_t0 = std::time::Instant::now();

                        // CSS: cache hit path -- intern_rules against new DOM's interner.
                        let css_t0 = std::time::Instant::now();
                        let new_stylesheet = new_doc
                            .dom
                            .with_interner_mut(|interner| {
                                renderer.get_or_parse_stylesheet(&css_text, interner)
                            })
                            .unwrap_or_else(|| {
                                // UNWRAP-OK: parsing the empty string cannot fail; canonical
                                // empty-stylesheet construction.
                                silksurf_css::parse_stylesheet_with_interner(
                                    "",
                                    &mut silksurf_core::SilkInterner::new(),
                                )
                                .unwrap()
                            });
                        let css_elapsed = css_t0.elapsed();

                        // Fused pipeline on new DOM.
                        let fused_t0 = std::time::Instant::now();
                        let new_fused = fused_style_layout_paint(
                            &new_doc.dom,
                            &new_stylesheet,
                            new_doc.document,
                            viewport,
                        );
                        let fused_elapsed = fused_t0.elapsed();

                        // Rasterize (reuse existing raster_buf allocation).
                        let raster_t0 = std::time::Instant::now();
                        let new_display_list = silksurf_render::DisplayList {
                            items: new_fused.display_items,
                            tiles: None,
                        }
                        .with_tiles(1280, 800, 64);
                        silksurf_render::rasterize_parallel_into(
                            &new_display_list,
                            1280,
                            800,
                            64,
                            &mut raster_buf,
                        );
                        let raster_elapsed = raster_t0.elapsed();

                        let total = rerender_t0.elapsed();
                        let new_styled = new_fused.styles.iter().filter(|s| s.is_some()).count();
                        eprintln!(
                            "[SilkSurf] Re-render ({} styled nodes): CSS {:?} + fused {:?} + raster {:?} = {:?}",
                            new_styled, css_elapsed, fused_elapsed, raster_elapsed, total,
                        );
                    }
                }
            }
        } else {
            eprintln!(
                "[SilkSurf] Revalidation: 304 NOT MODIFIED in {:?} -- cached render is current, no re-render",
                result.rtt
            );
        }
    }
}

/// Extract text content from `<style>` tags.
/// Extract href values from <link rel="stylesheet"> tags, resolved against base URL.
fn extract_stylesheet_urls(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<String> {
    let mut urls = Vec::new();
    collect_link_tags(dom, root, base_url, &mut urls);
    urls
}

fn collect_link_tags(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    urls: &mut Vec<String>,
) {
    if let Ok(name) = dom.element_name(node)
        && name == Some("link")
        && let Ok(attrs) = dom.attributes(node)
    {
        let is_stylesheet = attrs.iter().any(|a| {
            a.name == silksurf_dom::AttributeName::from_str("rel")
                && a.value.as_str() == "stylesheet"
        });
        if is_stylesheet
            && let Some(href) = attrs
                .iter()
                .find(|a| a.name == silksurf_dom::AttributeName::from_str("href"))
        {
            let href_str = href.value.as_str();
            // Resolve relative URLs
            let resolved = if href_str.starts_with("http://") || href_str.starts_with("https://") {
                href_str.to_string()
            } else if let Ok(base) = url::Url::parse(base_url) {
                base.join(href_str)
                    .map(|u| u.to_string())
                    .unwrap_or_default()
            } else {
                href_str.to_string()
            };
            if !resolved.is_empty() {
                urls.push(resolved);
            }
        }
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_link_tags(dom, child, base_url, urls);
        }
    }
}

fn extract_inline_css(dom: &silksurf_dom::Dom, root: silksurf_dom::NodeId) -> String {
    let mut css = String::new();
    collect_style_tags(dom, root, &mut css);
    css
}

fn collect_style_tags(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId, css: &mut String) {
    if let Ok(name) = dom.element_name(node)
        && name == Some("style")
        && let Ok(children) = dom.children(node)
    {
        for &child in children {
            if let Ok(n) = dom.node(child)
                && let silksurf_dom::NodeKind::Text { text } = n.kind()
            {
                css.push_str(text);
                css.push('\n');
            }
        }
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_style_tags(dom, child, css);
        }
    }
}

/// Extract text content from inline `<script>` tags (without src attribute).
fn extract_inline_scripts(dom: &silksurf_dom::Dom, root: silksurf_dom::NodeId) -> Vec<String> {
    let mut scripts = Vec::new();
    collect_script_tags(dom, root, &mut scripts);
    scripts
}

fn collect_script_tags(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    scripts: &mut Vec<String>,
) {
    if let Ok(name) = dom.element_name(node)
        && name == Some("script")
    {
        let attrs = dom.attributes(node).ok();
        // Skip external scripts (src="...") and non-JS types
        let has_src = attrs
            .as_ref()
            .map(|a| {
                a.iter()
                    .any(|a| a.name == silksurf_dom::AttributeName::from_str("src"))
            })
            .unwrap_or(false);
        let script_type = attrs.as_ref().and_then(|a| {
            a.iter()
                .find(|a| a.name == silksurf_dom::AttributeName::from_str("type"))
                .map(|a| a.value.to_string())
        });
        // Skip JSON-LD, importmap, and other non-JS types
        let is_js = matches!(
            script_type.as_deref(),
            None | Some("") | Some("text/javascript") | Some("application/javascript")
        );

        if !has_src && is_js {
            let mut text = String::new();
            if let Ok(children) = dom.children(node) {
                for &child in children {
                    if let Ok(n) = dom.node(child)
                        && let silksurf_dom::NodeKind::Text { text: t } = n.kind()
                    {
                        text.push_str(t);
                    }
                }
            }
            if !text.trim().is_empty() {
                scripts.push(text);
            }
        }
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_script_tags(dom, child, scripts);
        }
    }
}
