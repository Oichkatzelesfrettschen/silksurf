/*
 * js_events synthesizes trusted DOM events from native GUI input.
 *
 * The dispatch contract: JS listeners fire first through the full
 * capture/target/bubble path (silksurf-js event_dispatch), then the native
 * default action runs unless a listener called preventDefault() on a
 * cancelable event. Pages with no registered listener for an event type pay
 * nothing: every synthesis site is gated on SilkContext::has_dom_listeners.
 *
 * The Dom mutex is never held across a dispatch call -- hit testing snapshots
 * what it needs and releases the lock before any listener runs. Listener DOM
 * mutations mark dirty nodes; each dispatch drains them through the same
 * incremental repaint path the host-callback tick uses.
 */

use crate::browser_types::{BrowserFrame, BrowserPageRuntime, BrowserRedrawMode, BrowserState};
use crate::dom_hit_test::rect_contains;
use crate::runtime_repaint::repaint_runtime_dirty_nodes;
use silksurf_js::{SyntheticEvent, SyntheticField};

/// Outcome of one synthetic dispatch, folded back into the input handler.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SyntheticDispatchOutcome {
    /// A listener asked to suppress the native default action.
    pub(crate) default_prevented: bool,
    /// Listener DOM mutations were repainted; the caller must redraw.
    pub(crate) redraw: Option<BrowserRedrawMode>,
}

/// Deepest laid-out element whose content rect contains the pointer.
///
/// Coordinate convention mirrors hit_test_link / hit_test_input: window x is
/// used as-is, window y translates to document space by adding the scroll
/// offset, and clicks above the chrome strip never reach the page.
pub(crate) fn hit_test_event_target(
    runtime: &BrowserPageRuntime,
    window_x: f32,
    window_y: f32,
    scroll_y: f32,
    chrome_height: u32,
) -> Option<silksurf_dom::NodeId> {
    if window_y < chrome_height as f32 {
        return None;
    }
    let document_y = window_y + scroll_y;
    let mut best: Option<(f32, silksurf_dom::NodeId)> = None;
    // The lock covers rect filtering only; no JS runs while it is held.
    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (idx, &node) in runtime.fused.table.bfs_order.iter().enumerate() {
        if !matches!(runtime.fused.styles.get(idx), Some(Some(_))) {
            continue;
        }
        // Event targets are elements; text nodes resolve to their parent
        // element's rect, which the BFS walk already visits.
        if dom.element_name(node).ok().flatten().is_none() {
            continue;
        }
        let Some(rect) = runtime.fused.node_rects.get(idx) else {
            continue;
        };
        if rect.width <= 0.0 || rect.height <= 0.0 {
            continue;
        }
        if !rect_contains(*rect, window_x, document_y) {
            continue;
        }
        let area = rect.width * rect.height;
        // Smallest containing rect wins; BFS order breaks ties toward the
        // deeper node because descendants appear after ancestors.
        match best {
            Some((best_area, _)) if area > best_area => {}
            _ => best = Some((area, node)),
        }
    }
    best.map(|(_, node)| node)
}

/// Dispatch one synthetic event and repaint any listener DOM mutations.
/// Returns None when no listener for the type exists (nothing ran).
pub(crate) fn dispatch_synthetic_event(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
    target: silksurf_dom::NodeId,
    event: &SyntheticEvent,
) -> Option<SyntheticDispatchOutcome> {
    if !runtime.js_ctx.has_dom_listeners(event.event_type.as_str()) {
        return None;
    }
    let outcome = match runtime.js_ctx.dispatch_dom_event(target, event) {
        Ok(outcome) => outcome,
        Err(err) => {
            eprintln!(
                "[SilkSurf] Event dispatch error ({}): {err}",
                event.event_type
            );
            return None;
        }
    };
    let dirty_nodes = {
        let mut dom = runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        dom.take_dirty_nodes()
    };
    let redraw = if dirty_nodes.is_empty() {
        None
    } else {
        repaint_runtime_dirty_nodes(runtime, frame, &dirty_nodes)
    };
    Some(SyntheticDispatchOutcome {
        default_prevented: outcome.default_prevented,
        redraw,
    })
}

fn pointer_event(event_type: &str, cancelable: bool, x: f32, y: f32) -> SyntheticEvent {
    SyntheticEvent::new(event_type, true, cancelable)
        .with_field("button", SyntheticField::Number(0.0))
        .with_field("clientX", SyntheticField::Number(f64::from(x)))
        .with_field("clientY", SyntheticField::Number(f64::from(y)))
}

/// mousedown -> mouseup -> click at the hit-tested node. The click outcome
/// decides whether the native default action (link follow, input focus)
/// proceeds; mousedown/mouseup run for listener visibility only.
pub(crate) fn dispatch_native_click(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
    window_x: f32,
    window_y: f32,
    scroll_y: f32,
    chrome_height: u32,
) -> SyntheticDispatchOutcome {
    let listens = runtime.js_ctx.has_dom_listeners("click")
        || runtime.js_ctx.has_dom_listeners("mousedown")
        || runtime.js_ctx.has_dom_listeners("mouseup");
    if !listens {
        return SyntheticDispatchOutcome::default();
    }
    let Some(target) = hit_test_event_target(runtime, window_x, window_y, scroll_y, chrome_height)
    else {
        return SyntheticDispatchOutcome::default();
    };
    let mut merged = SyntheticDispatchOutcome::default();
    for (event_type, cancelable) in [("mousedown", false), ("mouseup", false), ("click", true)] {
        let event = pointer_event(event_type, cancelable, window_x, window_y);
        if let Some(outcome) = dispatch_synthetic_event(runtime, frame, target, &event) {
            merged.default_prevented |= cancelable && outcome.default_prevented;
            merged.redraw = merge_redraw(merged.redraw, outcome.redraw);
        }
    }
    merged
}

fn key_event(event_type: &str, cancelable: bool, key: &str) -> SyntheticEvent {
    SyntheticEvent::new(event_type, true, cancelable)
        .with_field("key", SyntheticField::Text(key.to_string()))
}

/// keydown at the focused node; a preventDefault() suppresses the edit.
pub(crate) fn dispatch_native_keydown(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
    target: silksurf_dom::NodeId,
    key: &str,
) -> SyntheticDispatchOutcome {
    dispatch_synthetic_event(runtime, frame, target, &key_event("keydown", true, key))
        .unwrap_or_default()
}

/// input (after the edit landed) then keyup; neither is cancelable.
pub(crate) fn dispatch_native_post_edit(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
    target: silksurf_dom::NodeId,
    key: &str,
) -> SyntheticDispatchOutcome {
    let mut merged = SyntheticDispatchOutcome::default();
    for event in [
        SyntheticEvent::new("input", true, false),
        key_event("keyup", false, key),
    ] {
        if let Some(outcome) = dispatch_synthetic_event(runtime, frame, target, &event) {
            merged.redraw = merge_redraw(merged.redraw, outcome.redraw);
        }
    }
    merged
}

/// submit at the form node; a preventDefault() suppresses navigation.
pub(crate) fn dispatch_native_submit(
    runtime: &mut BrowserPageRuntime,
    frame: &mut BrowserFrame,
    form: silksurf_dom::NodeId,
) -> SyntheticDispatchOutcome {
    dispatch_synthetic_event(
        runtime,
        frame,
        form,
        &SyntheticEvent::new("submit", true, true),
    )
    .unwrap_or_default()
}

/// Run a dispatch closure against the page runtime without fighting the
/// BrowserState borrow: the runtime is taken out (the tick_browser_runtime
/// pattern), the closure gets runtime + frame, and the runtime goes back.
/// Returns None when no page runtime exists (chrome-only states).
pub(crate) fn with_page_runtime<R>(
    state: &mut BrowserState,
    dispatch: impl FnOnce(&mut BrowserPageRuntime, &mut BrowserFrame) -> R,
) -> Option<R> {
    let mut page_runtime = state.runtime.take()?;
    let result = dispatch(&mut page_runtime, &mut state.frame);
    state.runtime = Some(page_runtime);
    Some(result)
}

/// Widening merge: two damage rects collapse to Full rather than growing a
/// rect-union vocabulary here; per-event dispatch rarely repaints twice.
fn merge_redraw(
    a: Option<BrowserRedrawMode>,
    b: Option<BrowserRedrawMode>,
) -> Option<BrowserRedrawMode> {
    match (a, b) {
        (None, other) | (other, None) => other,
        (Some(_), Some(_)) => Some(BrowserRedrawMode::Full),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser_types::{BrowserPage, BrowserPagePayload, BrowserRenderConfig};
    use crate::page_build::{build_browser_page, stylesheet_text_with_user_agent_defaults};

    fn page_with_script(html: &str, script: &str) -> BrowserPage {
        let payload = BrowserPagePayload {
            url: "https://example.com/".to_string(),
            html: html.to_string(),
            css_text: stylesheet_text_with_user_agent_defaults(""),
            script_texts: vec![script.to_string()],
            module_texts: Vec::new(),
            images: Vec::new(),
            render_config: BrowserRenderConfig::default(),
            parsed_document: None,
        };
        build_browser_page(payload).expect("payload builds page")
    }

    fn node_by_id(page: &BrowserPage, id: &str) -> silksurf_dom::NodeId {
        let dom = page
            .runtime
            .dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *page
            .runtime
            .fused
            .table
            .bfs_order
            .iter()
            .find(|&&node| {
                dom.attributes(node).ok().is_some_and(|attrs| {
                    attrs
                        .iter()
                        .any(|a| a.name.as_str() == "id" && a.value.as_str() == id)
                })
            })
            .expect("node with id present")
    }

    #[test]
    fn click_prevent_default_suppresses_native_action() {
        let mut page = page_with_script(
            "<!doctype html><html><body><a id=\"go\" href=\"https://example.org/\">go</a></body></html>",
            "document.getElementById('go').addEventListener('click', function (e) { e.preventDefault(); });",
        );
        let target = node_by_id(&page, "go");
        let outcome = dispatch_synthetic_event(
            &mut page.runtime,
            &mut page.frame,
            target,
            &silksurf_js::SyntheticEvent::new("click", true, true),
        )
        .expect("listener registered, dispatch runs");
        assert!(outcome.default_prevented);
    }

    #[test]
    fn dispatch_without_listeners_reports_none() {
        let mut page = page_with_script(
            "<!doctype html><html><body><a id=\"go\" href=\"https://example.org/\">go</a></body></html>",
            "",
        );
        let target = node_by_id(&page, "go");
        let outcome = dispatch_synthetic_event(
            &mut page.runtime,
            &mut page.frame,
            target,
            &silksurf_js::SyntheticEvent::new("click", true, true),
        );
        assert!(outcome.is_none());
    }

    #[test]
    fn listener_dom_mutation_triggers_incremental_repaint() {
        let mut page = page_with_script(
            "<!doctype html><html><body><p id=\"msg\">Hello</p></body></html>",
            "document.getElementById('msg').addEventListener('poke', function () { \
               document.getElementById('msg').firstChild.textContent = 'Poked'; \
             });",
        );
        let target = node_by_id(&page, "msg");
        let outcome = dispatch_synthetic_event(
            &mut page.runtime,
            &mut page.frame,
            target,
            &silksurf_js::SyntheticEvent::new("poke", false, false),
        )
        .expect("listener registered, dispatch runs");
        assert!(outcome.redraw.is_some(), "DOM mutation must repaint");
    }

    #[test]
    fn hit_test_event_target_finds_deepest_node() {
        let page = page_with_script(
            "<!doctype html><html><body><div id=\"outer\"><p id=\"inner\">text</p></div></body></html>",
            "",
        );
        let inner = node_by_id(&page, "inner");
        let inner_rect = {
            let idx = *page
                .runtime
                .fused
                .table
                .node_to_bfs_idx
                .get(&inner)
                .expect("inner in table") as usize;
            page.runtime.fused.node_rects[idx]
        };
        let hit = hit_test_event_target(
            &page.runtime,
            inner_rect.x + inner_rect.width / 2.0,
            inner_rect.y + inner_rect.height / 2.0,
            0.0,
            0,
        )
        .expect("point inside inner hits a node");
        assert_eq!(hit, inner);
    }

    #[test]
    fn keydown_prevent_default_reports_prevented() {
        let mut page = page_with_script(
            "<!doctype html><html><body><input id=\"field\" value=\"\"></body></html>",
            "document.getElementById('field').addEventListener('keydown', function (e) { \
               if (e.key === 'x') { e.preventDefault(); } \
             });",
        );
        let field = node_by_id(&page, "field");
        let prevented = dispatch_native_keydown(&mut page.runtime, &mut page.frame, field, "x");
        assert!(prevented.default_prevented);
        let allowed = dispatch_native_keydown(&mut page.runtime, &mut page.frame, field, "y");
        assert!(!allowed.default_prevented);
    }
}
