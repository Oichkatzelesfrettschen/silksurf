// Module split from the former single-file binary; the crate root
// re-exports every module so sibling items resolve by bare name.
#[allow(clippy::wildcard_imports)]
use crate::*;

pub(crate) fn collect_link_targets(
    dom: &silksurf_dom::Dom,
    items: &[silksurf_render::DisplayItem],
    base_url: &str,
) -> Vec<LinkTarget> {
    let mut targets = Vec::new();
    for item in items {
        let silksurf_render::DisplayItem::Text { rect, node, .. } = item else {
            continue;
        };
        if let Some(href) = href_for_node_anchor(dom, *node, base_url) {
            targets.push(LinkTarget { rect: *rect, href });
        }
    }
    targets
}

pub(crate) fn collect_input_targets(
    dom: &silksurf_dom::Dom,
    fused: &FusedResult,
) -> Vec<InputTarget> {
    let mut targets = Vec::new();
    let mut descendants = Vec::new();
    for (index, &node) in fused.table.bfs_order.iter().enumerate() {
        if !is_editable_input_node(dom, node) {
            continue;
        }
        let contents_editor = fused.styles[index].as_ref().is_some_and(|style| {
            style.display == silksurf_css::Display::Contents
                && is_text_content_editable_node(dom, node)
        });
        let rect = if contents_editor {
            contents_editor_rect(fused, index, &mut descendants)
        } else if box_is_exposed(fused, index) {
            fused_node_rect(fused, node)
        } else {
            None
        };
        let Some(rect) = rect else {
            continue;
        };
        if rect.width > 0.0 && rect.height > 0.0 {
            targets.push(InputTarget { rect, node });
        }
    }
    targets
}

fn contents_editor_rect(
    fused: &FusedResult,
    index: usize,
    descendants: &mut Vec<usize>,
) -> Option<Rect> {
    descendants.clear();
    descendants.push(index);
    let mut bounds = None;
    while let Some(parent) = descendants.pop() {
        let first = fused.table.child_start[parent];
        if first == u32::MAX {
            continue;
        }
        let start = first as usize;
        let end = start + usize::from(fused.table.child_count[parent]);
        for child in start..end {
            if !box_is_exposed(fused, child) {
                if fused.styles[child]
                    .as_ref()
                    .is_some_and(|style| style.display == silksurf_css::Display::Contents)
                {
                    descendants.push(child);
                }
                continue;
            }
            if let Some(rect) = fused.node_rects.get(child).copied()
                && rect.width > 0.0
                && rect.height > 0.0
            {
                bounds = Some(bounds.map_or(rect, |current| union_rect(current, rect)));
            }
            descendants.push(child);
        }
    }
    bounds
}

pub(crate) fn box_is_exposed(fused: &FusedResult, index: usize) -> bool {
    let mut current = index;
    loop {
        let Some(style) = fused.styles.get(current).and_then(Option::as_ref) else {
            return false;
        };
        if style.display == silksurf_css::Display::None
            || (current == index && style.display == silksurf_css::Display::Contents)
        {
            return false;
        }
        let parent = fused
            .table
            .parent_idx
            .get(current)
            .copied()
            .unwrap_or(u32::MAX);
        if parent == u32::MAX {
            return true;
        }
        current = parent as usize;
    }
}

pub(crate) fn is_editable_input_node(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> bool {
    input_node_kind(dom, node).is_some() || is_text_content_editable_node(dom, node)
}

pub(crate) fn is_text_editable_input_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> bool {
    if is_textarea_node(dom, node) || is_text_content_editable_node(dom, node) {
        return true;
    }
    if input_node_kind(dom, node) != Some(silksurf_dom::TagName::Input) {
        return false;
    }
    !matches!(
        input_type(dom, node).as_str(),
        "button"
            | "checkbox"
            | "color"
            | "file"
            | "hidden"
            | "image"
            | "radio"
            | "range"
            | "reset"
            | "submit"
    )
}

pub(crate) fn is_textarea_node(dom: &silksurf_dom::Dom, node: silksurf_dom::NodeId) -> bool {
    input_node_kind(dom, node) == Some(silksurf_dom::TagName::Textarea)
}

pub(crate) fn is_text_content_editable_input_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> bool {
    is_textarea_node(dom, node) || is_text_content_editable_node(dom, node)
}

pub(crate) fn is_text_content_editable_node(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> bool {
    dom.attributes(node)
        .ok()
        .and_then(|attrs| {
            attrs
                .iter()
                .find(|attr| attr.name.as_str() == "contenteditable")
        })
        .is_some_and(|attr| contenteditable_value_is_editable(attr.value.as_str()))
}

pub(crate) fn contenteditable_value_is_editable(value: &str) -> bool {
    let value = value.trim();
    value.is_empty()
        || value.eq_ignore_ascii_case("true")
        || value.eq_ignore_ascii_case("plaintext-only")
}

pub(crate) fn input_node_kind(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<silksurf_dom::TagName> {
    let tag = node_tag_name(dom, node)?;
    matches!(
        tag,
        silksurf_dom::TagName::Input
            | silksurf_dom::TagName::Textarea
            | silksurf_dom::TagName::Select
    )
    .then_some(tag)
}

pub(crate) fn node_tag_name(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
) -> Option<silksurf_dom::TagName> {
    dom.element_name(node)
        .ok()
        .flatten()
        .map(silksurf_dom::TagName::from_str)
}

pub(crate) fn href_for_node_anchor(
    dom: &silksurf_dom::Dom,
    node: silksurf_dom::NodeId,
    base_url: &str,
) -> Option<String> {
    let mut current = Some(node);
    while let Some(node_id) = current {
        if dom
            .element_name(node_id)
            .ok()
            .flatten()
            .is_some_and(|name| name.eq_ignore_ascii_case("a"))
            && let Ok(attrs) = dom.attributes(node_id)
            && let Some(href) = attrs
                .iter()
                .find(|attr| attr.name == silksurf_dom::AttributeName::Href)
        {
            return resolve_page_url(href.value.as_str(), base_url);
        }
        current = dom.parent(node_id).ok().flatten();
    }
    None
}

pub(crate) fn resolve_page_url(href: &str, base_url: &str) -> Option<String> {
    let trimmed = href.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(url) = url::Url::parse(trimmed) {
        return browser_supported_url(&url);
    }
    let base = url::Url::parse(base_url).ok()?;
    let joined = base.join(trimmed).ok()?;
    browser_supported_url(&joined)
}

pub(crate) fn browser_supported_url(url: &url::Url) -> Option<String> {
    match url.scheme() {
        "http" | "https" => Some(url.to_string()),
        _ => None,
    }
}

pub(crate) fn hit_test_link(
    targets: &[LinkTarget],
    window_x: f32,
    window_y: f32,
    scroll_y: f32,
    chrome_height: u32,
) -> Option<&str> {
    if window_y < chrome_height as f32 {
        return None;
    }
    let document_y = window_y + scroll_y;
    targets
        .iter()
        .rev()
        .find(|target| rect_contains(target.rect, window_x, document_y))
        .map(|target| target.href.as_str())
}

pub(crate) fn hit_test_input(
    targets: &[InputTarget],
    window_x: f32,
    window_y: f32,
    scroll_y: f32,
    chrome_height: u32,
) -> Option<silksurf_dom::NodeId> {
    if window_y < chrome_height as f32 {
        return None;
    }
    let document_y = window_y + scroll_y;
    targets
        .iter()
        .rev()
        .find(|target| rect_contains(target.rect, window_x, document_y))
        .map(|target| target.node)
}

pub(crate) fn trace_input_hit_test(state: &BrowserState, x: f32, y: f32, scroll_y: f32) {
    if std::env::var_os("SILKSURF_TRACE_INPUT").is_none() {
        return;
    }
    eprintln!(
        "[SilkSurf] input hit-test: click=({x:.1},{y:.1}) scroll={scroll_y:.1} inputs={} links={}",
        state.frame.input_targets.len(),
        state.frame.link_targets.len()
    );
    for target in &state.frame.link_targets {
        eprintln!(
            "[SilkSurf] link target: href={} rect=({}, {}, {}, {})",
            target.href, target.rect.x, target.rect.y, target.rect.width, target.rect.height
        );
    }
    eprintln!(
        "[SilkSurf] input target count: {}",
        state.frame.input_targets.len()
    );
    for target in &state.frame.input_targets {
        eprintln!(
            "[SilkSurf] input target: node={} rect=({}, {}, {}, {})",
            target.node.raw(),
            target.rect.x,
            target.rect.y,
            target.rect.width,
            target.rect.height
        );
    }
}

pub(crate) fn rect_contains(rect: Rect, x: f32, y: f32) -> bool {
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}
