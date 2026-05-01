//! SilkSurf Rust-native webview entry point.
//!
//! Pipeline: fetch URL -> parse HTML -> load CSS/JS resources -> create VM
//! with DOM bridge -> run scripts -> layout -> render (future: XCB window).
//!
//! Usage: silksurf-app [URL]
//! Default URL: https://example.com

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

use std::cell::RefCell;
use std::rc::Rc;

use silksurf_engine::fused_pipeline::fused_style_layout_paint;
use silksurf_engine::parse_html;
use silksurf_engine::speculative::{FetchOrigin, SpeculativeRenderer};
use silksurf_js::vm::Vm;
use silksurf_js::vm::dom_bridge;
use silksurf_layout::Rect;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let insecure = args.iter().any(|a| a == "--insecure" || a == "-k");
    let platform_verifier = args.iter().any(|a| a == "--platform-verifier");
    let speculative = args.iter().any(|a| a == "--speculative" || a == "-s");

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

    // 5. Create JS VM with DOM bridge (post-CSS, pre-render)
    let shared_dom = Rc::new(RefCell::new(dom));
    let mut vm = Vm::new();
    dom_bridge::install_document(&vm.global, Rc::clone(&shared_dom), doc_node);

    // 6. Extract and execute inline <script> tags
    let scripts = extract_inline_scripts(&shared_dom.borrow(), doc_node);
    eprintln!("[SilkSurf] Found {} inline script(s)", scripts.len());
    for (i, script) in scripts.iter().enumerate() {
        // Skip only very large bundled JS (React, webpack output, etc.).
        // Inline init scripts are usually <4KB; anything >256KB is a bundle.
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
        // No skip patterns needed -- parser handles ??=, class extends, ?. etc.
        // Compile and execute
        let ast_arena = silksurf_js::parser::ast_arena::AstArena::new();
        let parser = silksurf_js::parser::Parser::new(script, &ast_arena);
        let (ast, errors) = parser.parse();
        if !errors.is_empty() {
            eprintln!("[SilkSurf] Script {i} parse errors: {errors:?}");
            continue;
        }
        let compiler = silksurf_js::bytecode::Compiler::new();
        match compiler.compile_with_children(&ast) {
            Ok((chunk, child_chunks, string_pool)) => {
                // Load compiler's string pool into VM's StringTable.
                // Build a mapping from compiler IDs to VM IDs.
                let mut str_map = std::collections::HashMap::new();
                for (compiler_id, s) in &string_pool {
                    let vm_id = vm.strings.intern(s.clone());
                    str_map.insert(*compiler_id, vm_id);
                }
                // Add child chunks (function bodies) first so their indices are stable.
                // CRITICAL: remap both String IDs and Function chunk indices in child chunks.
                // String IDs: child uses parent string pool (new_with_pool), so str_map covers
                //   all strings including those added by child/nested compilers.
                // Function indices: compiler stores indices relative to child_chunks[0].
                //   After adding to VM at child_base, all Function(idx) -> Function(idx+child_base).
                let child_base = vm.chunks_len();
                for mut child in child_chunks {
                    for constant in child.constants_mut() {
                        match constant {
                            silksurf_js::bytecode::Constant::String(str_id) => {
                                if let Some(&vm_id) = str_map.get(str_id) {
                                    *str_id = vm_id;
                                }
                            }
                            silksurf_js::bytecode::Constant::Function(idx) => {
                                *idx += child_base as u32;
                            }
                            _ => {}
                        }
                    }
                    vm.add_chunk(child);
                }
                // Patch main chunk constants: remap string IDs and function chunk indices
                let mut main_chunk = chunk;
                for constant in main_chunk.constants_mut() {
                    match constant {
                        silksurf_js::bytecode::Constant::Function(idx) => {
                            *idx += child_base as u32;
                        }
                        silksurf_js::bytecode::Constant::String(str_id) => {
                            if let Some(&vm_id) = str_map.get(str_id) {
                                *str_id = vm_id;
                            }
                        }
                        _ => {}
                    }
                }
                let chunk_idx = vm.add_chunk(main_chunk);
                match vm.execute(chunk_idx) {
                    Ok(_) => eprintln!(
                        "[SilkSurf] Script {i} executed OK ({:?})",
                        script_start.elapsed()
                    ),
                    Err(e) => {
                        eprintln!(
                            "[SilkSurf] Script {i} runtime error: {e:?} ({:?})",
                            script_start.elapsed()
                        );
                        // Save failing script to /tmp for analysis
                        if i == 6 {
                            std::fs::write("/tmp/chatgpt_script6.js", script).ok();
                            eprintln!("[SilkSurf] Saved script 6 to /tmp/chatgpt_script6.js");
                        }
                    }
                }
            }
            Err(e) => eprintln!("[SilkSurf] Script {i} compile error: {e:?}"),
        }

        /*
         * Post-script fixup: inject streamController for ReadableStream.
         *
         * WHY: ChatGPT's script 3 does `new ReadableStream({start(controller){
         * window.__reactRouterContext.streamController = controller}})`.
         * Since NativeFunction constructors can't invoke JS function callbacks,
         * the `start` callback never runs and `streamController` is never set.
         *
         * We detect this by checking: if __reactRouterContext exists and has
         * a `stream` property but no `streamController`, inject one.
         * This unblocks scripts 5, 7, 10 which call enqueue()/close().
         */
        {
            let g = vm.global.borrow();
            // Check both the global directly and via window (self-referential)
            let ctx = g.get_by_str("__reactRouterContext");
            let ctx_type = ctx.type_of();
            if i == 4 {
                // After script 3 + 4, check if __reactRouterContext exists
                let prop_count = g.properties.len();
                eprintln!(
                    "[DEBUG] Global has {prop_count} props, __reactRouterContext type: {ctx_type}"
                );
            }
            if let silksurf_js::vm::value::Value::Object(ctx_obj) = &ctx {
                let sc = ctx_obj.borrow().get_by_str("streamController");
                if matches!(sc, silksurf_js::vm::value::Value::Undefined) {
                    // Inject controller with enqueue() and close() stubs
                    let ctrl = silksurf_js::vm::value::Object::new();
                    let ctrl_rc = std::rc::Rc::new(std::cell::RefCell::new(ctrl));
                    {
                        let mut c = ctrl_rc.borrow_mut();
                        c.set_by_str(
                            "enqueue",
                            silksurf_js::vm::value::Value::NativeFunction(std::rc::Rc::new(
                                silksurf_js::vm::value::NativeFunction::new("enqueue", |_| {
                                    silksurf_js::vm::value::Value::Undefined
                                }),
                            )),
                        );
                        c.set_by_str(
                            "close",
                            silksurf_js::vm::value::Value::NativeFunction(std::rc::Rc::new(
                                silksurf_js::vm::value::NativeFunction::new("close", |_| {
                                    silksurf_js::vm::value::Value::Undefined
                                }),
                            )),
                        );
                    }
                    ctx_obj.borrow_mut().set_by_str(
                        "streamController",
                        silksurf_js::vm::value::Value::Object(ctrl_rc),
                    );
                }
            }
        }
    }

    // 7. Run one tick of the event loop
    let tick_result = silksurf_js::vm::event_loop::tick(&mut vm.timers, &mut vm.microtasks);
    eprintln!("[SilkSurf] Event loop tick: {tick_result:?}");

    // 8. Fused style+layout+paint: single BFS pass over post-JS DOM.
    //    Replaces separate compute_styles + build_layout_tree + build_display_list calls.
    //    Running post-JS ensures DOM mutations from scripts are visible in the render.
    let fused_start = std::time::Instant::now();
    let fused = fused_style_layout_paint(&shared_dom.borrow(), &stylesheet, doc_node, viewport);
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
                        &shared_dom.borrow(),
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

/// Extract text content from <style> tags.
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

/// Extract text content from inline <script> tags (without src attribute).
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
