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

/*
 * PageGeometry -- the border boxes the layout-reading DOM accessors answer
 * from.
 *
 * The fused pipeline writes rects in document coordinates and the frame
 * scrolls the presented bitmap, so a viewport rect is the document rect less
 * the scroll offset -- the same conversion `viewport_damage_rect` performs for
 * damage. `getBoundingClientRect` reports viewport coordinates, so the
 * conversion happens on read and the map stays valid while the page scrolls.
 *
 * The map refreshes when a fused run completes rather than when a script asks,
 * because layout runs after script. A read in the same script as the mutation
 * that invalidated it therefore reports the last completed layout.
 */
#[derive(Default)]
pub(crate) struct PageGeometry {
    boxes: std::collections::HashMap<silksurf_dom::NodeId, silksurf_js::ElementBox>,
    scroll_y: f32,
}

/// Shared handle: the repaint path writes it, the JS geometry provider reads it.
pub(crate) type PageGeometryRef = std::rc::Rc<std::cell::RefCell<PageGeometry>>;

impl PageGeometry {
    /// Replace the map from a completed layout. A node the layout produced no
    /// box for is absent, which the JS half reports as a zero rect.
    pub(crate) fn refresh(&mut self, fused: &FusedResult) {
        self.boxes.clear();
        self.boxes.reserve(fused.table.bfs_order.len());
        for (index, node) in fused.table.bfs_order.iter().enumerate() {
            let (Some(rect), Some(border), Some(padding)) = (
                fused.node_rects.get(index),
                fused.node_borders.get(index),
                fused.node_paddings.get(index),
            ) else {
                continue;
            };
            self.boxes.insert(
                *node,
                [
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    border.top,
                    border.right,
                    border.bottom,
                    border.left,
                    padding.top,
                    padding.right,
                    padding.bottom,
                    padding.left,
                ],
            );
        }
    }

    /// Record the scroll offset the frame presents at, which the read
    /// subtracts to reach viewport coordinates.
    pub(crate) fn set_scroll(&mut self, scroll_y: f32) {
        self.scroll_y = scroll_y;
    }

    /// The node's border box in viewport coordinates.
    pub(crate) fn get(&self, node: silksurf_dom::NodeId) -> Option<silksurf_js::ElementBox> {
        let mut found = *self.boxes.get(&node)?;
        found[1] -= self.scroll_y;
        Some(found)
    }
}

/// Install the geometry provider backing `getBoundingClientRect`,
/// `getClientRects`, `offsetWidth`, `offsetHeight`, `clientWidth`, and
/// `clientHeight`.
pub(crate) fn install_geometry_provider(js_ctx: &mut SilkContext, geometry: &PageGeometryRef) {
    let geometry = std::rc::Rc::clone(geometry);
    js_ctx.set_geometry_provider(std::rc::Rc::new(move |node| geometry.borrow().get(node)));
}

#[cfg(test)]
mod page_geometry_tests {
    use super::*;

    /// A one-element document plus the layout result for it.
    fn fixture(rect: Rect) -> (silksurf_dom::NodeId, FusedResult) {
        let mut dom = silksurf_dom::Dom::new();
        let root = dom.create_document();
        let div = dom.create_element("div");
        let _ = dom.append_child(root, div);
        dom.materialize_resolve_table();
        let table = silksurf_layout::neighbor_table::LayoutNeighborTable::build(&dom, root);
        // UNWRAP-OK: the fixture appended the div under the root the table was built from.
        let at = table
            .bfs_order
            .iter()
            .position(|node| *node == div)
            .expect("the div is in the table");
        let mut node_rects = vec![Rect::default(); table.len()];
        let mut node_borders = vec![silksurf_layout::EdgeSizes::default(); table.len()];
        let mut node_paddings = vec![silksurf_layout::EdgeSizes::default(); table.len()];
        node_rects[at] = rect;
        node_borders[at] = silksurf_layout::EdgeSizes {
            top: 1.0,
            right: 2.0,
            bottom: 3.0,
            left: 4.0,
        };
        node_paddings[at] = silksurf_layout::EdgeSizes {
            top: 5.0,
            right: 6.0,
            bottom: 7.0,
            left: 8.0,
        };
        (
            div,
            FusedResult {
                styles: vec![None; table.len()],
                display_items: Vec::new(),
                node_rects,
                node_borders,
                node_paddings,
                table,
            },
        )
    }

    #[test]
    fn a_refresh_publishes_the_border_box_and_its_insets() {
        let (div, fused) = fixture(Rect {
            x: 10.0,
            y: 20.0,
            width: 300.0,
            height: 100.0,
        });
        let mut geometry = PageGeometry::default();
        geometry.refresh(&fused);
        // UNWRAP-OK: the fixture wrote a rect at the div's BFS slot before the refresh.
        let found = geometry.get(div).expect("the div has a box");
        let want = [
            10.0, 20.0, 300.0, 100.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0,
        ];
        for (index, (got, expected)) in found.iter().zip(want).enumerate() {
            assert!((got - expected).abs() < f32::EPSILON, "slot {index}");
        }
    }

    #[test]
    fn a_read_reports_viewport_coordinates_while_the_page_scrolls() {
        // The fused pipeline writes document coordinates and the frame scrolls
        // the presented bitmap, so a viewport rect is the document rect less
        // the scroll offset -- the conversion viewport_damage_rect performs.
        let (div, fused) = fixture(Rect {
            x: 0.0,
            y: 500.0,
            width: 10.0,
            height: 10.0,
        });
        let mut geometry = PageGeometry::default();
        geometry.refresh(&fused);
        // UNWRAP-OK: the fixture wrote a rect at the div's BFS slot before the refresh.
        let unscrolled = geometry.get(div).expect("box");
        assert!((unscrolled[1] - 500.0).abs() < f32::EPSILON, "unscrolled");
        geometry.set_scroll(120.0);
        // UNWRAP-OK: the same slot the unscrolled read above resolved.
        let scrolled = geometry.get(div).expect("box");
        assert!((scrolled[1] - 380.0).abs() < f32::EPSILON, "scrolled");
        assert!(scrolled[0].abs() < f32::EPSILON, "x is unaffected");
    }

    #[test]
    fn a_node_the_layout_skipped_has_no_box() {
        let (_div, fused) = fixture(Rect::default());
        let mut geometry = PageGeometry::default();
        geometry.refresh(&fused);
        assert!(
            geometry.get(silksurf_dom::NodeId::from_raw(999)).is_none(),
            "a node outside the layout answers nothing"
        );
    }
}
