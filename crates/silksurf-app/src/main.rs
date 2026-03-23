//! SilkSurf Rust-native webview entry point.
//!
//! Pipeline: fetch URL -> parse HTML -> load CSS/JS resources -> create VM
//! with DOM bridge -> run scripts -> layout -> render (future: XCB window).
//!
//! Usage: silksurf-app [URL]
//! Default URL: https://example.com

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
    let speculative = args.iter().any(|a| a == "--speculative" || a == "-s");

    let url = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .cloned()
        .unwrap_or_else(|| "https://example.com".to_string());

    if insecure {
        eprintln!("[SilkSurf] WARNING: TLS certificate verification disabled (--insecure)");
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
    } else {
        SpeculativeRenderer::new()
    };

    eprintln!("[SilkSurf] Fetching: {url}");

    // 1. Fetch the page (cache-first)
    let (response, fetch_origin, fetch_elapsed) =
        match renderer.fetch_or_speculate(&url, &[]) {
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

    // Fetch external <link rel="stylesheet"> resources (cache-first via SpeculativeRenderer)
    let stylesheet_urls = extract_stylesheet_urls(&dom, doc_node, &url);
    for sheet_url in &stylesheet_urls {
        eprintln!("[SilkSurf] Fetching stylesheet: {sheet_url}");
        let css_headers = [("Accept".to_string(), "text/css,*/*".to_string())];
        match renderer.fetch_or_speculate(sheet_url, &css_headers) {
            Ok((resp, origin, elapsed)) if resp.status == 200 => {
                eprintln!(
                    "[SilkSurf] Stylesheet {} bytes ({:?} {:?})",
                    resp.body.len(),
                    origin,
                    elapsed
                );
                let sheet_css = String::from_utf8_lossy(&resp.body);
                css_text.push_str(&sheet_css);
                css_text.push('\n');
            }
            Ok((resp, _, _)) => eprintln!("[SilkSurf] Stylesheet HTTP {}", resp.status),
            Err(e) => eprintln!("[SilkSurf] Stylesheet fetch error: {}", e.message),
        }
    }

    eprintln!("[SilkSurf] Total CSS to parse: {} bytes", css_text.len());

    // 4. Parse CSS
    let css_start = std::time::Instant::now();
    let stylesheet = match dom.with_interner_mut(|interner| {
        silksurf_css::parse_stylesheet_with_interner(&css_text, interner)
    }) {
        Ok(ss) => ss,
        Err(e) => {
            eprintln!("[SilkSurf] CSS parse error: {e:?}");
            silksurf_css::parse_stylesheet_with_interner(
                "",
                &mut silksurf_core::SilkInterner::new(),
            )
            .unwrap()
        }
    };
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
            eprintln!("[SilkSurf] Script {i} FULL ({} bytes): {script}", script.len());
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
                eprintln!("[DEBUG] Global has {prop_count} props, __reactRouterContext type: {ctx_type}");
            }
            if let silksurf_js::vm::value::Value::Object(ctx_obj) = &ctx {
                let sc = ctx_obj.borrow().get_by_str("streamController");
                if matches!(sc, silksurf_js::vm::value::Value::Undefined) {
                    // Inject controller with enqueue() and close() stubs
                    let ctrl = silksurf_js::vm::value::Object::new();
                    let ctrl_rc = std::rc::Rc::new(std::cell::RefCell::new(ctrl));
                    {
                        let mut c = ctrl_rc.borrow_mut();
                        c.set_by_str("enqueue", silksurf_js::vm::value::Value::NativeFunction(std::rc::Rc::new(
                            silksurf_js::vm::value::NativeFunction::new("enqueue", |_| silksurf_js::vm::value::Value::Undefined),
                        )));
                        c.set_by_str("close", silksurf_js::vm::value::Value::NativeFunction(std::rc::Rc::new(
                            silksurf_js::vm::value::NativeFunction::new("close", |_| silksurf_js::vm::value::Value::Undefined),
                        )));
                    }
                    ctx_obj.borrow_mut().set_by_str("streamController", silksurf_js::vm::value::Value::Object(ctrl_rc));
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

    // 9. Tile-parallel rasterization via Rayon (disjoint tile regions, no sync)
    let raster_start = std::time::Instant::now();
    let buffer = silksurf_render::rasterize_parallel(&display_list, 1280, 800, 64);
    let raster_elapsed = raster_start.elapsed();
    eprintln!("[SilkSurf] Rasterized: {} bytes in {:?}", buffer.len(), raster_elapsed);

    eprintln!("\n=== PROCESSING BUDGET (excludes network) ===");
    eprintln!("  CSS parse:      {:?}", css_start.elapsed() - fused_elapsed - raster_elapsed);
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
                "[SilkSurf] Revalidation: CONTENT CHANGED (200) in {:?} -- re-render needed",
                result.rtt
            );
            if let Some(new_resp) = result.response {
                renderer.update_cache(&url, &new_resp);
                eprintln!(
                    "[SilkSurf] Cache updated ({} bytes)",
                    renderer.cache_bytes()
                );
            }
        } else {
            eprintln!(
                "[SilkSurf] Revalidation: 304 NOT MODIFIED in {:?} -- cached render is current",
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
    if let Ok(name) = dom.element_name(node) {
        if name == Some("link") {
            if let Ok(attrs) = dom.attributes(node) {
                let is_stylesheet = attrs.iter().any(|a| {
                    a.name == silksurf_dom::AttributeName::from_str("rel")
                        && a.value.as_str() == "stylesheet"
                });
                if is_stylesheet {
                    if let Some(href) = attrs
                        .iter()
                        .find(|a| a.name == silksurf_dom::AttributeName::from_str("href"))
                    {
                        let href_str = href.value.as_str();
                        // Resolve relative URLs
                        let resolved = if href_str.starts_with("http://")
                            || href_str.starts_with("https://")
                        {
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
    if let Ok(name) = dom.element_name(node) {
        if name == Some("style") {
            if let Ok(children) = dom.children(node) {
                for &child in children {
                    if let Ok(n) = dom.node(child) {
                        if let silksurf_dom::NodeKind::Text { text } = n.kind() {
                            css.push_str(text);
                            css.push('\n');
                        }
                    }
                }
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
    if let Ok(name) = dom.element_name(node) {
        if name == Some("script") {
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
            let is_js = match script_type.as_deref() {
                None | Some("") | Some("text/javascript") | Some("application/javascript") => true,
                _ => false,
            };

            if !has_src && is_js {
                let mut text = String::new();
                if let Ok(children) = dom.children(node) {
                    for &child in children {
                        if let Ok(n) = dom.node(child) {
                            if let silksurf_dom::NodeKind::Text { text: t } = n.kind() {
                                text.push_str(t);
                            }
                        }
                    }
                }
                if !text.trim().is_empty() {
                    scripts.push(text);
                }
            }
        }
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_script_tags(dom, child, scripts);
        }
    }
}
