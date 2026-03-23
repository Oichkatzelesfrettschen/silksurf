//! SilkSurf Rust-native webview entry point.
//!
//! Pipeline: fetch URL -> parse HTML -> load CSS/JS resources -> create VM
//! with DOM bridge -> run scripts -> layout -> render (future: XCB window).
//!
//! Usage: silksurf-app [URL]
//! Default URL: https://example.com

use std::cell::RefCell;
use std::rc::Rc;

use silksurf_core::SilkArena;
use silksurf_css::compute_styles;
use silksurf_engine::parse_html;
use silksurf_js::vm::Vm;
use silksurf_js::vm::dom_bridge;
use silksurf_layout::Rect;
use silksurf_net::{BasicClient, HttpMethod, HttpRequest, NetClient};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let insecure = args.iter().any(|a| a == "--insecure" || a == "-k");
    let url = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .cloned()
        .unwrap_or_else(|| "https://example.com".to_string());

    if insecure {
        eprintln!("[SilkSurf] WARNING: TLS certificate verification disabled (--insecure)");
    }
    eprintln!("[SilkSurf] Fetching: {url}");

    // 1. Fetch the page
    let client = if insecure {
        use silksurf_net::BasicClient as BC;
        use silksurf_tls::RustlsProvider;
        BC::with_tls(std::sync::Arc::new(RustlsProvider::new_insecure()))
    } else {
        BasicClient::new()
    };
    let request = HttpRequest {
        method: HttpMethod::Get,
        url: url.clone(),
        headers: vec![
            ("Accept".to_string(), "text/html,*/*".to_string()),
            (
                "User-Agent".to_string(),
                "SilkSurf/0.1 (X11; Linux x86_64)".to_string(),
            ),
        ],
        body: Vec::new(),
    };

    let response = match client.fetch(&request) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[SilkSurf] Fetch error: {}", e.message);
            return;
        }
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

    // Fetch external <link rel="stylesheet"> resources
    let stylesheet_urls = extract_stylesheet_urls(&dom, doc_node, &url);
    for sheet_url in &stylesheet_urls {
        eprintln!("[SilkSurf] Fetching stylesheet: {sheet_url}");
        let sheet_req = HttpRequest {
            method: HttpMethod::Get,
            url: sheet_url.clone(),
            headers: vec![("Accept".to_string(), "text/css,*/*".to_string())],
            body: Vec::new(),
        };
        match client.fetch(&sheet_req) {
            Ok(resp) if resp.status == 200 => {
                eprintln!("[SilkSurf] Fetched {} bytes of CSS", resp.body.len());
                // Limit total CSS to prevent slow parsing
                const MAX_TOTAL_CSS: usize = 128 * 1024;
                if css_text.len() < MAX_TOTAL_CSS {
                    let sheet_css = String::from_utf8_lossy(&resp.body);
                    let remaining = MAX_TOTAL_CSS - css_text.len();
                    let to_add = sheet_css.len().min(remaining);
                    // Truncate at rule boundary
                    let safe = sheet_css[..to_add]
                        .rfind('}')
                        .map(|p| p + 1)
                        .unwrap_or(to_add);
                    css_text.push_str(&sheet_css[..safe]);
                    css_text.push('\n');
                } else {
                    eprintln!("[SilkSurf] Skipping (CSS budget exceeded)");
                }
            }
            Ok(resp) => eprintln!("[SilkSurf] Stylesheet HTTP {}", resp.status),
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

    // 5. Compute styles
    let styles = compute_styles(&dom, doc_node, &stylesheet);
    eprintln!("[SilkSurf] Computed styles for {} nodes", styles.len());

    // 6. Build layout tree
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 800.0,
    };
    let arena = SilkArena::new();
    let layout = silksurf_layout::build_layout_tree(&arena, &dom, &styles, doc_node, viewport);

    match &layout {
        Some(tree) => {
            let dims = tree.root.dimensions();
            eprintln!(
                "[SilkSurf] Layout complete: {}x{} at ({}, {})",
                dims.content.width, dims.content.height, dims.content.x, dims.content.y
            );
        }
        None => {
            eprintln!("[SilkSurf] Layout failed (no root box)");
        }
    }

    // 7. Create JS VM with DOM bridge
    let shared_dom = Rc::new(RefCell::new(dom));
    let mut vm = Vm::new();
    dom_bridge::install_document(&vm.global, Rc::clone(&shared_dom), doc_node);

    // 8. Extract and execute inline <script> tags
    let scripts = extract_inline_scripts(&shared_dom.borrow(), doc_node);
    eprintln!("[SilkSurf] Found {} inline script(s)", scripts.len());
    for (i, script) in scripts.iter().enumerate() {
        // Skip large scripts for now (bundled React app, etc.)
        if script.len() > 1_000 {
            eprintln!(
                "[SilkSurf] Script {i}: {} bytes (skipping -- too large for interpreter)",
                script.len()
            );
            continue;
        }
        let preview = &script[..script.len().min(80)];
        eprintln!(
            "[SilkSurf] Executing script {i} ({} bytes): {preview}...",
            script.len()
        );
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
            Ok((chunk, child_chunks)) => {
                // Add child chunks (function bodies) first so their indices are stable
                let child_base = vm.chunks_len();
                for child in child_chunks {
                    vm.add_chunk(child);
                }
                // Patch function constants to use absolute chunk indices
                let mut main_chunk = chunk;
                for constant in main_chunk.constants_mut() {
                    if let silksurf_js::bytecode::Constant::Function(idx) = constant {
                        *idx += child_base as u32;
                    }
                }
                let chunk_idx = vm.add_chunk(main_chunk);
                match vm.execute(chunk_idx) {
                    Ok(_) => eprintln!(
                        "[SilkSurf] Script {i} executed OK ({:?})",
                        script_start.elapsed()
                    ),
                    Err(e) => eprintln!(
                        "[SilkSurf] Script {i} runtime error: {e:?} ({:?})",
                        script_start.elapsed()
                    ),
                }
            }
            Err(e) => eprintln!("[SilkSurf] Script {i} compile error: {e:?}"),
        }
    }

    // 9. Run one tick of the event loop
    let tick_result = silksurf_js::vm::event_loop::tick(&mut vm.timers, &mut vm.microtasks);
    eprintln!("[SilkSurf] Event loop tick: {tick_result:?}");

    // 10. Build display list
    if let Some(layout_tree) = &layout {
        let display_list =
            silksurf_render::build_display_list(&shared_dom.borrow(), &styles, layout_tree);
        eprintln!(
            "[SilkSurf] Display list: {} items",
            display_list.items.len()
        );

        // Rasterize to buffer (headless)
        let buffer = silksurf_render::rasterize(&display_list, 1280, 800);
        eprintln!("[SilkSurf] Rasterized: {} bytes", buffer.len());
    }

    eprintln!("[SilkSurf] Pipeline complete for {url}");
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
