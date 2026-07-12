// Module split from the former single-file binary; the crate root
// re-exports every module so sibling items resolve by bare name.
#[allow(clippy::wildcard_imports)]
use crate::*;

/// Extract href values from `<link rel="stylesheet">` tags.
pub(crate) fn extract_stylesheet_urls(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<String> {
    let mut urls = Vec::new();
    collect_link_resource_urls(dom, root, base_url, "stylesheet", &mut urls);
    urls
}

pub(crate) fn extract_modulepreload_urls(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<String> {
    let mut urls = Vec::new();
    collect_link_resource_urls(dom, root, base_url, "modulepreload", &mut urls);
    urls
}

pub(crate) fn extract_module_warm_urls(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<String> {
    let mut urls = extract_modulepreload_urls(dom, root, base_url);
    collect_module_script_warm_urls(dom, root, base_url, &mut urls);
    dedupe_resource_urls(&urls)
}

pub(crate) fn collect_module_script_warm_urls(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    urls: &mut Vec<String>,
) {
    if let Some(url) = module_script_external_url(dom, node, base_url) {
        urls.push(url);
    }
    if let Some(text) = inline_module_script_text(dom, node) {
        urls.extend(module_static_import_urls(base_url, &text));
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_module_script_warm_urls(dom, child, base_url, urls);
        }
    }
}

pub(crate) fn external_module_script_urls(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<String> {
    let mut urls = Vec::new();
    collect_external_module_script_urls(dom, root, base_url, &mut urls);
    dedupe_resource_urls(&urls)
}

pub(crate) fn collect_external_module_script_urls(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    urls: &mut Vec<String>,
) {
    if let Some(url) = module_script_external_url(dom, node, base_url) {
        urls.push(url);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_external_module_script_urls(dom, child, base_url, urls);
        }
    }
}

pub(crate) fn module_script_external_url(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
) -> Option<String> {
    if dom.element_name(node).ok().flatten()? != "script" {
        return None;
    }
    let attrs = dom.attributes(node).ok()?;
    if !script_type_is_module(Some(attrs)) {
        return None;
    }
    let src = script_src(Some(attrs))?;
    let resolved = resolve_resource_url(base_url, src);
    (!resolved.is_empty()).then_some(resolved)
}

pub(crate) fn module_path_for_url(module_url: &str) -> String {
    let Ok(parsed) = url::Url::parse(module_url) else {
        return module_url.to_string();
    };
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = parsed.query() {
        path.push('?');
        path.push_str(query);
    }
    path
}

pub(crate) fn inline_module_script_text(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<String> {
    if dom.element_name(node).ok().flatten()? != "script" {
        return None;
    }
    let attrs = dom.attributes(node).ok()?;
    if !script_type_is_module(Some(attrs)) || script_src(Some(attrs)).is_some() {
        return None;
    }
    let text = script_text_content(dom, node);
    (!text.trim().is_empty()).then_some(text)
}

pub(crate) fn collect_link_resource_urls(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    rel_token: &str,
    urls: &mut Vec<String>,
) {
    if let Some(url) = link_resource_url_for_node(dom, node, base_url, rel_token) {
        urls.push(url);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_link_resource_urls(dom, child, base_url, rel_token, urls);
        }
    }
}

pub(crate) fn link_resource_url_for_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    rel_token: &str,
) -> Option<String> {
    if dom.element_name(node).ok().flatten()? != "link" {
        return None;
    }
    let attrs = dom.attributes(node).ok()?;
    if !link_rel_contains(attrs, rel_token) {
        return None;
    }
    let href = link_href(attrs)?;
    let resolved = resolve_resource_url(base_url, href);
    (!resolved.is_empty()).then_some(resolved)
}

pub(crate) fn link_rel_contains(attrs: &[silksurf_dom::Attribute], token: &str) -> bool {
    let Some(rel) = attrs
        .iter()
        .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("rel"))
    else {
        return false;
    };
    rel.value
        .as_str()
        .split_ascii_whitespace()
        .any(|value| value.eq_ignore_ascii_case(token))
}

pub(crate) fn link_href(attrs: &[silksurf_dom::Attribute]) -> Option<&str> {
    attrs
        .iter()
        .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("href"))
        .map(|attr| attr.value.as_str())
        .filter(|href| !href.trim().is_empty())
}

pub(crate) fn extract_image_urls(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<String> {
    let mut urls = Vec::new();
    collect_img_tags(dom, root, base_url, &mut urls);
    urls
}

pub(crate) fn collect_img_tags(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    urls: &mut Vec<String>,
) {
    if let Some(src) = image_src_for_node(dom, node) {
        let resolved = resolve_resource_url(base_url, &src);
        if !resolved.is_empty() {
            urls.push(resolved);
        }
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_img_tags(dom, child, base_url, urls);
        }
    }
}

pub(crate) fn fetch_decoded_images(
    renderer: &mut SpeculativeRenderer,
    image_cache: &mut ImageResourceCache,
    image_urls: &[String],
) -> Vec<DecodedPageImage> {
    let mut images = Vec::new();
    let mut missing_urls = Vec::new();
    for image_url in dedupe_resource_urls(image_urls) {
        if let Some(image) = image_cache.get(&image_url) {
            images.push(image);
        } else {
            missing_urls.push(image_url);
        }
    }
    if missing_urls.is_empty() {
        if !image_urls.is_empty() {
            eprintln!(
                "[SilkSurf] Image cache hit: {} entries, {} bytes",
                image_cache.len(),
                image_cache.bytes()
            );
        }
        return images;
    }

    let accept_header = [(
        "Accept".to_string(),
        "image/avif,image/webp,image/png,image/jpeg,*/*".to_string(),
    )];
    let requests: Vec<(&str, &[(String, String)])> = missing_urls
        .iter()
        .map(|image_url| (image_url.as_str(), accept_header.as_slice()))
        .collect();
    for image in renderer
        .fetch_all_or_speculate(&requests)
        .into_iter()
        .zip(missing_urls.iter())
        .filter_map(|(result, image_url)| decode_image_response(result, image_url))
    {
        image_cache.insert(image.clone());
        images.push(image);
    }
    eprintln!(
        "[SilkSurf] Image cache: {} entries, {} bytes",
        image_cache.len(),
        image_cache.bytes()
    );
    images
}

#[derive(Clone)]
pub(crate) enum DocumentScriptRef {
    Inline(String),
    External(String),
}

#[derive(Clone)]
pub(crate) struct DocumentScriptNode {
    pub(crate) node: silksurf_dom::NodeId,
    pub(crate) source: DocumentScriptRef,
}

pub(crate) fn load_document_script_texts(
    renderer: &mut SpeculativeRenderer,
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<String> {
    let scripts = extract_document_scripts(dom, root, base_url);
    let external_urls: Vec<String> = scripts
        .iter()
        .filter_map(|script| match script {
            DocumentScriptRef::External(url) => Some(url.clone()),
            DocumentScriptRef::Inline(_) => None,
        })
        .collect();
    let fetched = fetch_external_script_texts(renderer, &external_urls);
    scripts
        .into_iter()
        .filter_map(|script| match script {
            DocumentScriptRef::Inline(text) => Some(text),
            DocumentScriptRef::External(url) => fetched
                .iter()
                .find_map(|(fetched_url, text)| (fetched_url == &url).then(|| text.clone())),
        })
        .collect()
}

pub(crate) fn load_document_module_texts(
    renderer: &mut SpeculativeRenderer,
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<(String, String)> {
    let roots = external_module_script_urls(dom, root, base_url);
    let roots = dedupe_resource_urls(&roots);
    if roots.is_empty() {
        return Vec::new();
    }
    if roots.len() > MAX_NAVIGATION_MODULE_ROOTS {
        eprintln!(
            "[SilkSurf] Module execution skipped: {} roots exceed {} root cap",
            roots.len(),
            MAX_NAVIGATION_MODULE_ROOTS
        );
        return Vec::new();
    }
    let modules = fetch_module_graph_texts(renderer, &roots);
    let total_bytes: usize = modules.iter().map(|(_, text)| text.len()).sum();
    if total_bytes > MAX_NAVIGATION_MODULE_GRAPH_BYTES {
        eprintln!(
            "[SilkSurf] Module execution skipped: {total_bytes} bytes exceed {MAX_NAVIGATION_MODULE_GRAPH_BYTES} byte cap"
        );
        return Vec::new();
    }
    modules
        .into_iter()
        .map(|(module_url, text)| (module_path_for_url(&module_url), text))
        .collect()
}

pub(crate) fn fetch_module_graph_texts(
    renderer: &mut SpeculativeRenderer,
    roots: &[String],
) -> Vec<(String, String)> {
    let mut seen = HashSet::new();
    let mut pending = dedupe_resource_urls(roots);
    let mut fetched = Vec::new();
    for _round in 0..MAX_MODULE_GRAPH_ROUNDS {
        if pending.is_empty() || seen.len() >= MAX_MODULE_GRAPH_URLS {
            break;
        }
        let round_urls = take_module_graph_round_urls(&mut pending, &mut seen);
        if round_urls.is_empty() {
            break;
        }
        let round_fetched = fetch_module_round_texts(renderer, &round_urls, "Module");
        pending.extend(module_graph_child_urls(&round_fetched));
        fetched.extend(round_fetched);
    }
    fetched
}

pub(crate) fn fetch_external_script_texts(
    renderer: &mut SpeculativeRenderer,
    script_urls: &[String],
) -> Vec<(String, String)> {
    let urls = dedupe_resource_urls(script_urls);
    if urls.is_empty() {
        return Vec::new();
    }
    let accept_header = [(
        "Accept".to_string(),
        "text/javascript,application/javascript,*/*".to_string(),
    )];
    let requests: Vec<(&str, &[(String, String)])> = urls
        .iter()
        .map(|script_url| (script_url.as_str(), accept_header.as_slice()))
        .collect();
    renderer
        .fetch_all_or_speculate(&requests)
        .into_iter()
        .zip(urls.iter())
        .filter_map(|(result, script_url)| script_response_text(result, script_url))
        .collect()
}

pub(crate) fn preload_module_scripts(module_urls: &[String], config: &BrowserRenderConfig) {
    let urls = dedupe_resource_urls(module_urls);
    if urls.is_empty() {
        return;
    }
    for module_url in &urls {
        eprintln!("[SilkSurf] Modulepreload {module_url}: scheduled");
    }
    let config = config.clone();
    if let Err(err) = thread::Builder::new()
        .name("silksurf-modulepreload".to_string())
        .spawn(move || match renderer_from_config(&config) {
            Ok(mut renderer) => preload_module_scripts_with_renderer(&mut renderer, &urls),
            Err(message) => eprintln!("[SilkSurf] Modulepreload renderer: {message}"),
        })
    {
        eprintln!("[SilkSurf] Modulepreload thread: {err}");
    }
}

pub(crate) fn preload_module_scripts_with_renderer(
    renderer: &mut SpeculativeRenderer,
    urls: &[String],
) {
    let mut seen = HashSet::new();
    let mut pending = dedupe_resource_urls(urls);
    for round in 0..MAX_MODULE_GRAPH_ROUNDS {
        if pending.is_empty() || seen.len() >= MAX_MODULE_GRAPH_URLS {
            return;
        }
        let round_urls = take_module_graph_round_urls(&mut pending, &mut seen);
        if round_urls.is_empty() {
            return;
        }
        if !background_modulepreload_round_fits(round_urls.len()) {
            eprintln!(
                "[SilkSurf] Modulepreload graph stopped: {} round URLs exceed {} background cap",
                round_urls.len(),
                MAX_BACKGROUND_MODULEPRELOAD_ROUND_URLS
            );
            return;
        }
        let fetched = preload_module_round(renderer, &round_urls);
        pending.extend(module_graph_child_urls(&fetched));
        eprintln!(
            "[SilkSurf] Modulepreload graph round {round}: {} fetched, {} pending",
            fetched.len(),
            pending.len()
        );
    }
}

pub(crate) fn background_modulepreload_round_fits(round_url_count: usize) -> bool {
    round_url_count <= MAX_BACKGROUND_MODULEPRELOAD_ROUND_URLS
}

pub(crate) fn take_module_graph_round_urls(
    pending: &mut Vec<String>,
    seen: &mut HashSet<String>,
) -> Vec<String> {
    let mut round_urls = Vec::new();
    for url in std::mem::take(pending) {
        if seen.len() >= MAX_MODULE_GRAPH_URLS {
            break;
        }
        if seen.insert(url.clone()) {
            round_urls.push(url);
        }
    }
    round_urls
}

pub(crate) fn preload_module_round(
    renderer: &mut SpeculativeRenderer,
    urls: &[String],
) -> Vec<(String, String)> {
    fetch_module_round_texts(renderer, urls, "Modulepreload")
}

pub(crate) fn fetch_module_round_texts(
    renderer: &mut SpeculativeRenderer,
    urls: &[String],
    label: &str,
) -> Vec<(String, String)> {
    let accept_header = [(
        "Accept".to_string(),
        "text/javascript,application/javascript,*/*".to_string(),
    )];
    let requests: Vec<(&str, &[(String, String)])> = urls
        .iter()
        .map(|module_url| (module_url.as_str(), accept_header.as_slice()))
        .collect();
    renderer
        .fetch_all_or_speculate(&requests)
        .into_iter()
        .zip(urls.iter())
        .filter_map(|(result, module_url)| module_response_text(result, module_url, label))
        .collect()
}

pub(crate) fn module_graph_child_urls(fetched: &[(String, String)]) -> Vec<String> {
    fetched
        .iter()
        .flat_map(|(module_url, text)| module_static_import_urls(module_url, text))
        .collect()
}

pub(crate) fn module_static_import_urls(base_url: &str, source: &str) -> Vec<String> {
    module_static_import_specifiers(source)
        .into_iter()
        .map(|specifier| resolve_resource_url(base_url, &specifier))
        .filter(|url| !url.is_empty())
        .collect()
}

pub(crate) fn module_static_import_specifiers(source: &str) -> Vec<String> {
    let mut specifiers = Vec::new();
    for statement in source.split(';') {
        let trimmed = statement.trim_start();
        if trimmed.starts_with("import") {
            collect_import_statement_specifier(trimmed, &mut specifiers);
        } else if trimmed.starts_with("export") {
            collect_export_statement_specifier(trimmed, &mut specifiers);
        }
    }
    specifiers
}

pub(crate) fn collect_import_statement_specifier(statement: &str, specifiers: &mut Vec<String>) {
    let rest = statement.trim_start_matches("import").trim_start();
    if rest.starts_with('(') {
        return;
    }
    if let Some(specifier) = quoted_prefix(rest) {
        specifiers.push(specifier);
        return;
    }
    if let Some(from_index) = rest.rfind(" from ") {
        let after_from = rest[from_index + " from ".len()..].trim_start();
        if let Some(specifier) = quoted_prefix(after_from) {
            specifiers.push(specifier);
        }
    }
}

pub(crate) fn collect_export_statement_specifier(statement: &str, specifiers: &mut Vec<String>) {
    let Some(from_index) = statement.rfind(" from ") else {
        return;
    };
    let after_from = statement[from_index + " from ".len()..].trim_start();
    if let Some(specifier) = quoted_prefix(after_from) {
        specifiers.push(specifier);
    }
}

pub(crate) fn quoted_prefix(text: &str) -> Option<String> {
    let quote = text.as_bytes().first().copied()?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let rest = &text[1..];
    let end = rest.as_bytes().iter().position(|byte| *byte == quote)?;
    Some(rest[..end].to_string())
}

pub(crate) fn module_response_text(
    result: Result<
        (silksurf_net::HttpResponse, FetchOrigin, std::time::Duration),
        silksurf_net::NetError,
    >,
    module_url: &str,
    label: &str,
) -> Option<(String, String)> {
    match result {
        Ok((response, origin, elapsed)) if response.status == 200 => {
            eprintln!(
                "[SilkSurf] {label} {module_url}: {} bytes ({origin:?} {elapsed:?})",
                response.body.len()
            );
            if response.body.len() > MAX_MODULE_GRAPH_SCAN_BYTES {
                eprintln!(
                    "[SilkSurf] {label} {module_url}: graph scan skipped at {} bytes",
                    response.body.len()
                );
                return None;
            }
            Some((
                module_url.to_string(),
                String::from_utf8_lossy(&response.body).to_string(),
            ))
        }
        Ok((response, _, _)) => {
            eprintln!("[SilkSurf] {label} {module_url}: HTTP {}", response.status);
            None
        }
        Err(err) => {
            eprintln!(
                "[SilkSurf] {label} {module_url}: fetch error: {}",
                err.message
            );
            None
        }
    }
}

pub(crate) fn script_response_text(
    result: Result<
        (silksurf_net::HttpResponse, FetchOrigin, std::time::Duration),
        silksurf_net::NetError,
    >,
    script_url: &str,
) -> Option<(String, String)> {
    let (response, origin, elapsed) = match result {
        Ok(value) => value,
        Err(err) => {
            eprintln!(
                "[SilkSurf] Script {script_url}: fetch error: {}",
                err.message
            );
            return None;
        }
    };
    if response.status != 200 {
        eprintln!("[SilkSurf] Script {script_url}: HTTP {}", response.status);
        return None;
    }
    let text = String::from_utf8_lossy(&response.body).to_string();
    eprintln!(
        "[SilkSurf] Script {script_url}: {} bytes ({origin:?} {elapsed:?})",
        response.body.len()
    );
    Some((script_url.to_string(), text))
}

pub(crate) fn dedupe_resource_urls(urls: &[String]) -> Vec<String> {
    let mut deduped = Vec::new();
    for url in urls {
        if !deduped.iter().any(|existing| existing == url) {
            deduped.push(url.clone());
        }
    }
    deduped
}

pub(crate) fn decode_image_response(
    result: Result<
        (silksurf_net::HttpResponse, FetchOrigin, std::time::Duration),
        silksurf_net::NetError,
    >,
    image_url: &str,
) -> Option<DecodedPageImage> {
    let (response, origin, elapsed) = match result {
        Ok(value) => value,
        Err(err) => {
            eprintln!("[SilkSurf] Image {image_url}: fetch error: {}", err.message);
            return None;
        }
    };
    if response.status != 200 {
        eprintln!("[SilkSurf] Image {image_url}: HTTP {}", response.status);
        return None;
    }
    let content_type = response.header("content-type");
    let decoded = match silksurf_image::decode_image(&response.body, content_type) {
        Ok(decoded) => decoded,
        Err(err) => {
            eprintln!(
                "[SilkSurf] Image {image_url}: decode error: {}",
                err.message
            );
            return None;
        }
    };
    eprintln!(
        "[SilkSurf] Image {image_url}: {}x{} {} bytes ({origin:?} {elapsed:?})",
        decoded.width,
        decoded.height,
        response.body.len()
    );
    Some(DecodedPageImage {
        url: image_url.to_string(),
        surface: silksurf_render::ImageSurface {
            width: decoded.width,
            height: decoded.height,
            rgba: std::sync::Arc::from(decoded.rgba),
        },
    })
}

pub(crate) fn append_image_display_items(
    dom: &silksurf_dom::Dom,
    fused: &FusedResult,
    base_url: &str,
    images: &[DecodedPageImage],
    items: &mut Vec<silksurf_render::DisplayItem>,
) {
    for &node in &fused.table.bfs_order {
        if let Some(src) = image_src_for_node(dom, node) {
            let resolved = resolve_resource_url(base_url, &src);
            let Some(image) = images.iter().find(|image| image.url == resolved) else {
                continue;
            };
            let Some(rect) = fused_node_rect(fused, node) else {
                continue;
            };
            if rect.width <= 0.0 || rect.height <= 0.0 {
                continue;
            }
            items.push(silksurf_render::DisplayItem::Image {
                rect,
                image: image.surface.clone(),
            });
        } else if let Some(image) = canvas_surface_image(dom, node) {
            let Some(rect) = fused_node_rect(fused, node) else {
                continue;
            };
            if rect.width <= 0.0 || rect.height <= 0.0 {
                continue;
            }
            items.push(silksurf_render::DisplayItem::Image { rect, image });
        }
    }
}

/// Snapshot a canvas element's live backing store into an `ImageSurface`. The
/// pipeline is push-based, so this clones the current pixels on every render
/// pass -- the same by-value route decoded images travel.
fn canvas_surface_image(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<silksurf_render::ImageSurface> {
    let surface = dom.canvas_surface(node)?;
    Some(silksurf_render::ImageSurface {
        width: surface.width(),
        height: surface.height(),
        rgba: std::sync::Arc::from(surface.pixels()),
    })
}

pub(crate) fn collect_image_replaced_sizes(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
    images: &[DecodedPageImage],
) -> Vec<ReplacedSize> {
    let mut sizes = Vec::new();
    collect_image_replaced_sizes_for_node(dom, root, base_url, images, &mut sizes);
    sizes
}

pub(crate) fn collect_image_replaced_sizes_for_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    images: &[DecodedPageImage],
    sizes: &mut Vec<ReplacedSize>,
) {
    if let Some(size) = image_replaced_size_for_node(dom, node, base_url, images) {
        sizes.push(size);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_image_replaced_sizes_for_node(dom, child, base_url, images, sizes);
        }
    }
}

pub(crate) fn image_replaced_size_for_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    images: &[DecodedPageImage],
) -> Option<ReplacedSize> {
    if let Some(surface) = dom.canvas_surface(node) {
        let (attr_width, attr_height) = image_dimension_attrs(dom, node);
        let width = attr_width.unwrap_or(surface.width() as f32);
        let height = attr_height.unwrap_or(surface.height() as f32);
        return Some(ReplacedSize {
            node,
            width,
            height,
        });
    }
    let src = image_src_for_node(dom, node)?;
    let resolved = resolve_resource_url(base_url, &src);
    let image = images.iter().find(|image| image.url == resolved)?;
    let (attr_width, attr_height) = image_dimension_attrs(dom, node);
    let width = attr_width.unwrap_or(image.surface.width as f32);
    let height = attr_height.unwrap_or_else(|| inferred_image_height(attr_width, &image.surface));
    Some(ReplacedSize {
        node,
        width,
        height,
    })
}

pub(crate) fn image_dimension_attrs(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> (Option<f32>, Option<f32>) {
    let Ok(attrs) = dom.attributes(node) else {
        return (None, None);
    };
    let width = attrs
        .iter()
        .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("width"))
        .and_then(|attr| parse_html_dimension(attr.value.as_str()));
    let height = attrs
        .iter()
        .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("height"))
        .and_then(|attr| parse_html_dimension(attr.value.as_str()));
    (width, height)
}

pub(crate) fn parse_html_dimension(value: &str) -> Option<f32> {
    let number = value.trim().trim_end_matches("px").parse::<f32>().ok()?;
    (number > 0.0).then_some(number)
}

pub(crate) fn inferred_image_height(
    attr_width: Option<f32>,
    surface: &silksurf_render::ImageSurface,
) -> f32 {
    let Some(width) = attr_width else {
        return surface.height as f32;
    };
    if surface.width == 0 {
        return surface.height as f32;
    }
    (width * surface.height as f32 / surface.width as f32).max(1.0)
}

pub(crate) fn image_src_for_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<String> {
    if dom.element_name(node).ok().flatten()? != "img" {
        return None;
    }
    let attrs = dom.attributes(node).ok()?;
    let src = attrs
        .iter()
        .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("src"))?
        .value
        .as_str();
    if src.trim().is_empty() {
        return None;
    }
    Some(src.to_string())
}

pub(crate) fn resolve_resource_url(base_url: &str, resource_url: &str) -> String {
    if resource_url.starts_with("http://") || resource_url.starts_with("https://") {
        return resource_url.to_string();
    }
    url::Url::parse(base_url)
        .and_then(|base| base.join(resource_url))
        .map(|url| url.to_string())
        .unwrap_or_default()
}

pub(crate) fn extract_inline_css(dom: &silksurf_dom::Dom, root: silksurf_dom::NodeId) -> String {
    let mut css = String::new();
    collect_style_tags(dom, root, &mut css);
    css
}

pub(crate) fn collect_style_tags(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    css: &mut String,
) {
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

pub(crate) fn extract_document_scripts(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
    base_url: &str,
) -> Vec<DocumentScriptRef> {
    let mut scripts = Vec::new();
    collect_document_script_refs(dom, root, base_url, &mut scripts);
    scripts
}

pub(crate) fn collect_document_script_refs(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    scripts: &mut Vec<DocumentScriptRef>,
) {
    if let Some(script) = script_ref_for_node(dom, node, base_url) {
        scripts.push(script);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_document_script_refs(dom, child, base_url, scripts);
        }
    }
}

pub(crate) fn collect_classic_script_nodes(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
    scripts: &mut HashSet<silksurf_dom::NodeId>,
) {
    if script_ref_for_node(dom, node, base_url).is_some() {
        scripts.insert(node);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_classic_script_nodes(dom, child, base_url, scripts);
        }
    }
}

pub(crate) fn script_ref_for_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
) -> Option<DocumentScriptRef> {
    if dom.element_name(node).ok().flatten()? != "script" {
        return None;
    }
    let attrs = dom.attributes(node).ok();
    if !script_type_is_classic(attrs) {
        return None;
    }
    if let Some(src) = script_src(attrs) {
        let resolved = resolve_resource_url(base_url, src);
        return (!resolved.is_empty()).then_some(DocumentScriptRef::External(resolved));
    }
    let text = script_text_content(dom, node);
    (!text.trim().is_empty()).then_some(DocumentScriptRef::Inline(text))
}

pub(crate) fn script_type_is_classic(attrs: Option<&[silksurf_dom::Attribute]>) -> bool {
    let script_type = script_type_value(attrs);
    matches!(
        script_type,
        None | Some("" | "text/javascript" | "application/javascript")
    )
}

pub(crate) fn script_type_is_module(attrs: Option<&[silksurf_dom::Attribute]>) -> bool {
    script_type_value(attrs).is_some_and(|script_type| script_type.eq_ignore_ascii_case("module"))
}

pub(crate) fn script_type_value(attrs: Option<&[silksurf_dom::Attribute]>) -> Option<&str> {
    attrs.and_then(|attrs| {
        attrs
            .iter()
            .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("type"))
            .map(|attr| attr.value.as_str())
    })
}

pub(crate) fn script_src(attrs: Option<&[silksurf_dom::Attribute]>) -> Option<&str> {
    attrs?
        .iter()
        .find(|attr| attr.name == silksurf_dom::AttributeName::from_str("src"))
        .map(|attr| attr.value.as_str())
        .filter(|src| !src.trim().is_empty())
}

pub(crate) fn script_text_content(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> String {
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
    text
}

/// Extract text content from inline `<script>` tags.
pub(crate) fn extract_inline_scripts(
    dom: &silksurf_dom::Dom,
    root: silksurf_dom::NodeId,
) -> Vec<String> {
    let mut scripts = Vec::new();
    collect_script_tags(dom, root, &mut scripts);
    scripts
}

pub(crate) fn collect_script_tags(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    scripts: &mut Vec<String>,
) {
    if let Some(DocumentScriptRef::Inline(text)) = script_ref_for_node(dom, node, "") {
        scripts.push(text);
    }
    if let Ok(children) = dom.children(node) {
        for &child in children {
            collect_script_tags(dom, child, scripts);
        }
    }
}

#[cfg(test)]
mod tests {
    // Module split from the former single-file binary; the crate root
    // re-exports every module so sibling items resolve by bare name.
    #[allow(clippy::wildcard_imports)]
    use crate::*;

    #[test]
    fn canvas_element_composites_as_image_display_item() {
        // Build a document with a drawn-on canvas, run the real fused pipeline,
        // and confirm append_image_display_items emits an Image for the canvas
        // sized to its width/height attributes -- the Layer-3 compositing path.
        let mut dom = silksurf_dom::Dom::new();
        let document = dom.create_document();
        let html = dom.create_element("html");
        let body = dom.create_element("body");
        let canvas = dom.create_element("canvas");
        dom.set_attribute(canvas, "width", "40")
            .expect("width attr sets");
        dom.set_attribute(canvas, "height", "20")
            .expect("height attr sets");
        dom.append_child(document, html).expect("html attaches");
        dom.append_child(html, body).expect("body attaches");
        dom.append_child(body, canvas).expect("canvas attaches");

        let surface = dom.ensure_canvas_surface(canvas, 40, 20);
        surface.set_fill_style([255, 0, 0, 255]);
        surface.fill_rect(0.0, 0.0, 40.0, 20.0);

        let stylesheet = test_stylesheet(&dom);
        let viewport = silksurf_layout::Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let replaced = collect_image_replaced_sizes(&dom, document, "http://example.test/", &[]);
        assert!(
            replaced.iter().any(|size| size.node == canvas
                && (size.width - 40.0).abs() < 0.5
                && (size.height - 20.0).abs() < 0.5),
            "canvas should contribute a 40x20 replaced size"
        );

        let fused = silksurf_engine::fused_pipeline::fused_style_layout_paint_with_replaced_sizes(
            &dom,
            &stylesheet,
            document,
            viewport,
            &replaced,
        );
        let mut items = fused.display_items.clone();
        append_image_display_items(&dom, &fused, "http://example.test/", &[], &mut items);

        let canvas_image = items.iter().find_map(|item| match item {
            silksurf_render::DisplayItem::Image { rect, image } => Some((rect, image)),
            _ => None,
        });
        let (rect, image) = canvas_image.expect("canvas emits an Image display item");
        assert_eq!(image.width, 40);
        assert_eq!(image.height, 20);
        assert!(rect.width > 0.0 && rect.height > 0.0);
        // The snapshot carries the drawn red pixels.
        assert_eq!(&image.rgba[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn background_modulepreload_caps_large_rounds() {
        assert!(background_modulepreload_round_fits(
            MAX_BACKGROUND_MODULEPRELOAD_ROUND_URLS
        ));
        assert!(!background_modulepreload_round_fits(
            MAX_BACKGROUND_MODULEPRELOAD_ROUND_URLS + 1
        ));
    }

    #[test]
    fn image_nodes_emit_image_display_items() {
        let document = parse_html(
            "<!doctype html><html><body><img src=\"/asset.webp\" width=\"40\" height=\"20\" alt=\"demo\"></body></html>",
        )
        .expect("html parses");
        let stylesheet = test_stylesheet(&document.dom);
        let viewport = Rect {
            x: 0.0,
            y: BROWSER_CHROME_HEIGHT,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32 - BROWSER_CHROME_HEIGHT,
        };
        let images = vec![DecodedPageImage {
            url: "https://example.com/asset.webp".to_string(),
            surface: silksurf_render::ImageSurface {
                width: 1,
                height: 1,
                rgba: Arc::<[u8]>::from(vec![255, 0, 0, 255]),
            },
        }];
        let replaced_sizes = collect_image_replaced_sizes(
            &document.dom,
            document.document,
            "https://example.com/page",
            &images,
        );
        let mut fused = fused_style_layout_paint_with_replaced_sizes(
            &document.dom,
            &stylesheet,
            document.document,
            viewport,
            &replaced_sizes,
        );
        let mut items = std::mem::take(&mut fused.display_items);

        append_image_display_items(
            &document.dom,
            &fused,
            "https://example.com/page",
            &images,
            &mut items,
        );

        assert!(items.iter().any(|item| matches!(
            item,
            silksurf_render::DisplayItem::Image { rect, .. }
                if (rect.width - 40.0).abs() < f32::EPSILON && (rect.height - 20.0).abs() < f32::EPSILON
        )));
    }

    #[test]
    fn image_resource_cache_returns_decoded_surface() {
        let mut cache = ImageResourceCache::with_capacity(4, 1024);
        let image = DecodedPageImage {
            url: "https://example.com/asset.webp".to_string(),
            surface: silksurf_render::ImageSurface {
                width: 2,
                height: 1,
                rgba: Arc::<[u8]>::from(vec![255, 0, 0, 255, 0, 255, 0, 255]),
            },
        };

        cache.insert(image.clone());
        let cached = cache
            .get("https://example.com/asset.webp")
            .expect("image cache returns inserted image");

        assert_eq!(cached.url, image.url);
        assert_eq!(cached.surface.width, 2);
        assert_eq!(cached.surface.rgba.as_ref(), image.surface.rgba.as_ref());
        assert_eq!(cache.len(), 1);
        assert!(cache.bytes() >= image.surface.rgba.len() as u64);
    }

    #[test]
    fn dedupe_resource_urls_preserves_first_seen_order() {
        let urls = vec![
            "https://example.com/a.png".to_string(),
            "https://example.com/b.png".to_string(),
            "https://example.com/a.png".to_string(),
        ];

        assert_eq!(
            dedupe_resource_urls(&urls),
            vec![
                "https://example.com/a.png".to_string(),
                "https://example.com/b.png".to_string(),
            ]
        );
    }

    #[test]
    fn extracts_modulepreload_links_as_rel_tokens() {
        let document = parse_html(
            "<!doctype html><html><head>\
             <link rel=\"modulepreload\" href=\"/app.mjs\">\
             <link rel=\"stylesheet modulepreload\" href=\"/shared.js\">\
             <link rel=\"stylesheet\" href=\"/style.css\">\
             </head></html>",
        )
        .expect("fixture parses");

        assert_eq!(
            extract_modulepreload_urls(&document.dom, document.document, "https://example.com/"),
            vec![
                "https://example.com/app.mjs".to_string(),
                "https://example.com/shared.js".to_string(),
            ]
        );
        assert_eq!(
            extract_stylesheet_urls(&document.dom, document.document, "https://example.com/"),
            vec![
                "https://example.com/shared.js".to_string(),
                "https://example.com/style.css".to_string(),
            ]
        );
    }

    #[test]
    fn extracts_module_warm_urls_from_module_scripts_and_inline_imports() {
        let document = parse_html(
            "<!doctype html><html><head>\
             <link rel=\"modulepreload\" href=\"/preload.mjs\">\
             <script type=\"module\" src=\"/entry.mjs\"></script>\
             <script type=\"module\">import './inline-dep.mjs';</script>\
             <script src=\"/classic.js\"></script>\
             </head></html>",
        )
        .expect("fixture parses");

        assert_eq!(
            extract_module_warm_urls(&document.dom, document.document, "https://example.com/app/"),
            vec![
                "https://example.com/preload.mjs".to_string(),
                "https://example.com/entry.mjs".to_string(),
                "https://example.com/app/inline-dep.mjs".to_string(),
            ]
        );
    }

    #[test]
    fn extracts_static_module_import_specifiers() {
        let source = r#"
            import "./side-effect.mjs";
            import value from './value.mjs';
            import { x as y } from "./named.mjs";
            import * as ns from "./namespace.mjs";
            export { y } from "./reexport.mjs";
            export * from './all.mjs';
            import("./dynamic.mjs");
        "#;

        assert_eq!(
            module_static_import_specifiers(source),
            vec![
                "./side-effect.mjs".to_string(),
                "./value.mjs".to_string(),
                "./named.mjs".to_string(),
                "./namespace.mjs".to_string(),
                "./reexport.mjs".to_string(),
                "./all.mjs".to_string(),
            ]
        );
    }
}
