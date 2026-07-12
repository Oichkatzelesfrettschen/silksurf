// Module split from the former single-file binary; the crate root
// re-exports every module so sibling items resolve by bare name.
#[allow(clippy::wildcard_imports)]
use crate::*;

pub(crate) fn mark_redraw(state: &mut BrowserState, mode: BrowserRedrawMode) {
    state.retained_present = None;
    if mode != BrowserRedrawMode::Clean {
        state.frame.navigation_start_retained_sent = false;
        if !matches!(mode, BrowserRedrawMode::PageInputFocus(_)) {
            state.frame.current_view_retained_sent = false;
        }
        state.frame.scroll_viewport_caches.clear();
    }
    state.redraw_mode = combine_redraw_mode(state.redraw_mode, mode);
}

pub(crate) fn combine_redraw_mode(
    current: BrowserRedrawMode,
    next: BrowserRedrawMode,
) -> BrowserRedrawMode {
    match (current, next) {
        (BrowserRedrawMode::Clean, mode) | (mode, BrowserRedrawMode::Clean) => mode,
        (BrowserRedrawMode::Scroll, mode) | (mode, BrowserRedrawMode::Scroll) => mode,
        (BrowserRedrawMode::Full, _) | (_, BrowserRedrawMode::Full) => BrowserRedrawMode::Full,
        (BrowserRedrawMode::Damage(a), BrowserRedrawMode::Damage(b)) => {
            BrowserRedrawMode::Damage(union_rect(a, b))
        }
        (
            BrowserRedrawMode::PageInputFocus(a) | BrowserRedrawMode::Damage(a),
            BrowserRedrawMode::PageInputFocus(b),
        )
        | (BrowserRedrawMode::PageInputFocus(a), BrowserRedrawMode::Damage(b)) => {
            BrowserRedrawMode::Damage(union_rect(a, b))
        }
        (
            BrowserRedrawMode::DamageWithChrome(a),
            BrowserRedrawMode::Damage(b)
            | BrowserRedrawMode::PageInputFocus(b)
            | BrowserRedrawMode::DamageWithChrome(b),
        )
        | (
            BrowserRedrawMode::Damage(a) | BrowserRedrawMode::PageInputFocus(a),
            BrowserRedrawMode::DamageWithChrome(b),
        ) => BrowserRedrawMode::DamageWithChrome(union_rect(a, b)),
        (
            BrowserRedrawMode::Chrome,
            BrowserRedrawMode::AddressChrome
            | BrowserRedrawMode::AddressFocusChrome
            | BrowserRedrawMode::AddressFullTextChrome
            | BrowserRedrawMode::AddressTextChrome
            | BrowserRedrawMode::NavigationStartChrome
            | BrowserRedrawMode::StatusChrome,
        )
        | (
            BrowserRedrawMode::AddressChrome
            | BrowserRedrawMode::AddressFocusChrome
            | BrowserRedrawMode::AddressFullTextChrome
            | BrowserRedrawMode::AddressTextChrome
            | BrowserRedrawMode::NavigationStartChrome
            | BrowserRedrawMode::StatusChrome,
            BrowserRedrawMode::Chrome,
        ) => BrowserRedrawMode::Chrome,
        (
            BrowserRedrawMode::AddressChrome,
            BrowserRedrawMode::AddressFocusChrome
            | BrowserRedrawMode::AddressFullTextChrome
            | BrowserRedrawMode::AddressTextChrome,
        )
        | (
            BrowserRedrawMode::AddressFocusChrome
            | BrowserRedrawMode::AddressFullTextChrome
            | BrowserRedrawMode::AddressTextChrome,
            BrowserRedrawMode::AddressChrome,
        ) => BrowserRedrawMode::AddressChrome,
        (
            BrowserRedrawMode::AddressFocusChrome,
            BrowserRedrawMode::AddressFullTextChrome | BrowserRedrawMode::AddressTextChrome,
        )
        | (
            BrowserRedrawMode::AddressFullTextChrome | BrowserRedrawMode::AddressTextChrome,
            BrowserRedrawMode::AddressFocusChrome,
        ) => BrowserRedrawMode::AddressFullTextChrome,
        (
            BrowserRedrawMode::Damage(damage) | BrowserRedrawMode::PageInputFocus(damage),
            BrowserRedrawMode::Chrome
            | BrowserRedrawMode::AddressChrome
            | BrowserRedrawMode::AddressFocusChrome
            | BrowserRedrawMode::AddressFullTextChrome
            | BrowserRedrawMode::AddressTextChrome
            | BrowserRedrawMode::StatusChrome
            | BrowserRedrawMode::NavigationStartChrome,
        )
        | (
            BrowserRedrawMode::Chrome
            | BrowserRedrawMode::AddressChrome
            | BrowserRedrawMode::AddressFocusChrome
            | BrowserRedrawMode::AddressFullTextChrome
            | BrowserRedrawMode::AddressTextChrome
            | BrowserRedrawMode::StatusChrome
            | BrowserRedrawMode::NavigationStartChrome,
            BrowserRedrawMode::Damage(damage) | BrowserRedrawMode::PageInputFocus(damage),
        ) => BrowserRedrawMode::DamageWithChrome(damage),
        (
            BrowserRedrawMode::DamageWithChrome(damage),
            BrowserRedrawMode::Chrome
            | BrowserRedrawMode::AddressChrome
            | BrowserRedrawMode::AddressFocusChrome
            | BrowserRedrawMode::AddressFullTextChrome
            | BrowserRedrawMode::AddressTextChrome
            | BrowserRedrawMode::StatusChrome
            | BrowserRedrawMode::NavigationStartChrome,
        )
        | (
            BrowserRedrawMode::Chrome
            | BrowserRedrawMode::AddressChrome
            | BrowserRedrawMode::AddressFocusChrome
            | BrowserRedrawMode::AddressFullTextChrome
            | BrowserRedrawMode::AddressTextChrome
            | BrowserRedrawMode::StatusChrome
            | BrowserRedrawMode::NavigationStartChrome,
            BrowserRedrawMode::DamageWithChrome(damage),
        ) => BrowserRedrawMode::DamageWithChrome(damage),
        (BrowserRedrawMode::Chrome, BrowserRedrawMode::Chrome) => BrowserRedrawMode::Chrome,
        (BrowserRedrawMode::NavigationStartChrome, BrowserRedrawMode::NavigationStartChrome) => {
            BrowserRedrawMode::NavigationStartChrome
        }
        (BrowserRedrawMode::StatusChrome, BrowserRedrawMode::StatusChrome) => {
            BrowserRedrawMode::StatusChrome
        }
        (BrowserRedrawMode::StatusChrome, _) | (_, BrowserRedrawMode::StatusChrome) => {
            BrowserRedrawMode::Chrome
        }
        (BrowserRedrawMode::NavigationStartChrome, _)
        | (_, BrowserRedrawMode::NavigationStartChrome) => BrowserRedrawMode::Chrome,
        (BrowserRedrawMode::AddressChrome, BrowserRedrawMode::AddressChrome) => {
            BrowserRedrawMode::AddressChrome
        }
        (BrowserRedrawMode::AddressFocusChrome, BrowserRedrawMode::AddressFocusChrome) => {
            BrowserRedrawMode::AddressFocusChrome
        }
        (
            BrowserRedrawMode::AddressFullTextChrome | BrowserRedrawMode::AddressTextChrome,
            BrowserRedrawMode::AddressFullTextChrome,
        )
        | (BrowserRedrawMode::AddressFullTextChrome, BrowserRedrawMode::AddressTextChrome) => {
            BrowserRedrawMode::AddressFullTextChrome
        }
        (BrowserRedrawMode::AddressTextChrome, BrowserRedrawMode::AddressTextChrome) => {
            BrowserRedrawMode::AddressTextChrome
        }
    }
}

pub(crate) fn text_only_diff_damage_rect(
    diff: &DomDiff,
    old_fused: &FusedResult,
    new_fused: &FusedResult,
) -> Option<Rect> {
    if !diff.added.is_empty()
        || !diff.removed.is_empty()
        || diff.changed.is_empty()
        || diff
            .changed
            .iter()
            .any(|(_, kind)| *kind != ChangeKind::TextContent)
    {
        return None;
    }

    let mut damage = None;
    for &(node, _) in &diff.changed {
        let old_rect = fused_node_rect(old_fused, node)?;
        let new_rect = fused_node_rect(new_fused, silksurf_dom::NodeId::from_raw(node.raw()))?;
        damage = Some(match damage {
            Some(current) => union_rect(union_rect(current, old_rect), new_rect),
            None => union_rect(old_rect, new_rect),
        });
    }
    damage
}

pub(crate) fn dirty_nodes_damage_rect(
    dom: &silksurf_dom::Dom,
    dirty_nodes: &[silksurf_dom::NodeId],
    old_fused: &FusedResult,
    new_fused: &FusedResult,
) -> Option<Rect> {
    if dirty_nodes.is_empty() {
        return None;
    }

    let mut damage = None;
    for &node in dirty_nodes {
        let is_text_node = matches!(
            dom.node(node).ok().map(silksurf_dom::Node::kind),
            Some(silksurf_dom::NodeKind::Text { .. })
        );
        if !is_text_node && !is_editable_input_node(dom, node) {
            return None;
        }
        let old_rect = fused_node_rect(old_fused, node)?;
        let new_rect = fused_node_rect(new_fused, node)?;
        damage = Some(match damage {
            Some(current) => union_rect(union_rect(current, old_rect), new_rect),
            None => union_rect(old_rect, new_rect),
        });
    }
    damage
}

pub(crate) fn fused_node_rect(fused: &FusedResult, node: silksurf_dom::NodeId) -> Option<Rect> {
    let bfs_idx = *fused.table.node_to_bfs_idx.get(&node)? as usize;
    fused.node_rects.get(bfs_idx).copied()
}

pub(crate) fn union_rect(a: Rect, b: Rect) -> Rect {
    if a.width <= 0.0 || a.height <= 0.0 {
        return b;
    }
    if b.width <= 0.0 || b.height <= 0.0 {
        return a;
    }
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.width).max(b.x + b.width);
    let y1 = (a.y + a.height).max(b.y + b.height);
    Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}
