// Module split from the former single-file binary; the crate root
// re-exports every module so sibling items resolve by bare name.
#[allow(clippy::wildcard_imports)]
use crate::*;
use silksurf_dom::{NodeId, NodeKind};

#[cfg(test)]
pub(crate) fn rgba(r: u8, g: u8, b: u8, a: u8) -> silksurf_css::Color {
    silksurf_css::Color { r, g, b, a }
}

pub(crate) fn find_text_node(dom: &silksurf_dom::Dom, root: NodeId, text: &str) -> Option<NodeId> {
    let node = dom.node(root).ok()?;
    if let NodeKind::Text { text: candidate } = node.kind()
        && candidate == text
    {
        return Some(root);
    }
    for &child in dom.children(root).ok()? {
        if let Some(found) = find_text_node(dom, child, text) {
            return Some(found);
        }
    }
    None
}

pub(crate) fn test_stylesheet(dom: &silksurf_dom::Dom) -> silksurf_css::Stylesheet {
    let css = stylesheet_text_with_user_agent_defaults("");
    // UNWRAP-OK: the user-agent default stylesheet is a compile-time constant
    // that always parses; test fixture construction has no error path.
    dom.with_interner_mut(|interner| silksurf_css::parse_stylesheet_with_interner(&css, interner))
        .expect("stylesheet parses")
}

pub(crate) fn test_browser_state(url: &str) -> BrowserState {
    BrowserState {
        frame: BrowserFrame {
            url: url.to_string(),
            argb: Vec::new(),
            raster_height: FRAME_HEIGHT,
            bitmap_height: FRAME_HEIGHT,
            bitmap_scroll_y: 0,
            focus_viewport_cache: None,
            focus_viewport_retained_sent: false,
            current_view_retained_sent: false,
            navigation_start_retained_sent: false,
            scroll_viewport_caches: Vec::new(),
            link_targets: Vec::new(),
            input_targets: Vec::new(),
        },
        runtime: None,
        navigation_pending: false,
        status_text: "ready".to_string(),
        hover_status_text: None,
        history: vec![url.to_string()],
        history_index: 0,
        pending_history: None,
        navigation_generation: 0,
        address_editing: false,
        address_select_all: false,
        address_text: url.to_string(),
        address_cursor: 0,
        focused_input: None,
        redraw_mode: BrowserRedrawMode::Full,
        retained_present: None,
    }
}

pub(crate) fn test_browser_state_from_page(page: BrowserPage) -> BrowserState {
    BrowserState {
        frame: page.frame,
        runtime: Some(page.runtime),
        navigation_pending: false,
        status_text: "ready".to_string(),
        hover_status_text: None,
        history: vec!["https://example.com/".to_string()],
        history_index: 0,
        pending_history: None,
        navigation_generation: 0,
        address_editing: false,
        address_select_all: false,
        address_text: "https://example.com/".to_string(),
        address_cursor: 0,
        focused_input: None,
        redraw_mode: BrowserRedrawMode::Full,
        retained_present: None,
    }
}
