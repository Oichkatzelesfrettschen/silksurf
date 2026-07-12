#[cfg(feature = "accessibility")]
// Module split from the former single-file binary; the crate root
// re-exports every module so sibling items resolve by bare name.
#[allow(clippy::wildcard_imports)]
use crate::*;

#[cfg(feature = "accessibility")]
pub(crate) fn log_accessibility_snapshot(state: &BrowserState) {
    let update = build_browser_accessibility_update(state);
    eprintln!(
        "[SilkSurf] Accessibility snapshot: nodes={}",
        update.nodes.len()
    );
}

#[cfg(feature = "accessibility")]
pub(crate) fn build_browser_accessibility_update(state: &BrowserState) -> accesskit::TreeUpdate {
    let mut nodes =
        Vec::with_capacity(8 + state.frame.link_targets.len() + state.frame.input_targets.len());
    let mut root = accesskit::Node::new(accesskit::Role::RootWebArea);
    root.set_label("SilkSurf");
    root.set_url(state.frame.url.as_str());
    root.set_bounds(accessibility_rect(
        0.0,
        0.0,
        FRAME_WIDTH as f32,
        state.frame.bitmap_height as f32,
    ));

    push_chrome_accessibility_nodes(state, &mut root, &mut nodes);
    push_link_accessibility_nodes(&state.frame.link_targets, &mut root, &mut nodes);
    push_input_accessibility_nodes(state, &mut root, &mut nodes);
    nodes.push((accesskit::NodeId(ACCESSIBILITY_ROOT_ID), root));

    accesskit::TreeUpdate {
        nodes,
        tree: Some(accesskit::Tree::new(accesskit::NodeId(
            ACCESSIBILITY_ROOT_ID,
        ))),
        tree_id: accesskit::TreeId::ROOT,
        focus: accessibility_focus_id(state),
    }
}

#[cfg(feature = "accessibility")]
pub(crate) fn push_chrome_accessibility_nodes(
    state: &BrowserState,
    root: &mut accesskit::Node,
    nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
) {
    push_accessibility_button(root, nodes, ACCESSIBILITY_BACK_ID, "Back", BACK_BUTTON_X);
    push_accessibility_button(
        root,
        nodes,
        ACCESSIBILITY_FORWARD_ID,
        "Forward",
        FORWARD_BUTTON_X,
    );
    push_accessibility_button(root, nodes, ACCESSIBILITY_HOME_ID, "Home", HOME_BUTTON_X);
    push_accessibility_button(
        root,
        nodes,
        ACCESSIBILITY_RELOAD_ID,
        "Reload",
        RELOAD_BUTTON_X,
    );
    push_accessibility_button(root, nodes, ACCESSIBILITY_STOP_ID, "Stop", STOP_BUTTON_X);
    push_address_accessibility_node(state, root, nodes);
    push_status_accessibility_node(state, root, nodes);
}

#[cfg(feature = "accessibility")]
pub(crate) fn push_accessibility_button(
    root: &mut accesskit::Node,
    nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
    id: u64,
    label: &str,
    x: u32,
) {
    let mut button = accesskit::Node::new(accesskit::Role::Button);
    button.set_label(label);
    button.set_bounds(accessibility_rect(
        x as f32,
        NAV_BUTTON_Y as f32,
        NAV_BUTTON_WIDTH as f32,
        NAV_BUTTON_HEIGHT as f32,
    ));
    button.add_action(accesskit::Action::Click);
    nodes.push((accesskit::NodeId(id), button));
    root.push_child(accesskit::NodeId(id));
}

#[cfg(feature = "accessibility")]
pub(crate) fn push_address_accessibility_node(
    state: &BrowserState,
    root: &mut accesskit::Node,
    nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
) {
    let mut address = accesskit::Node::new(accesskit::Role::UrlInput);
    address.set_label("Address");
    let address_value = if state.address_editing {
        state.address_text.as_str()
    } else {
        state.frame.url.as_str()
    };
    address.set_value(address_value);
    address.set_bounds(accessibility_rect(
        ADDRESS_BAR_X as f32,
        ADDRESS_BAR_Y as f32,
        ADDRESS_BAR_WIDTH as f32,
        ADDRESS_BAR_HEIGHT as f32,
    ));
    address.add_action(accesskit::Action::Focus);
    nodes.push((accesskit::NodeId(ACCESSIBILITY_ADDRESS_ID), address));
    root.push_child(accesskit::NodeId(ACCESSIBILITY_ADDRESS_ID));
}

#[cfg(feature = "accessibility")]
pub(crate) fn push_status_accessibility_node(
    state: &BrowserState,
    root: &mut accesskit::Node,
    nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
) {
    let mut status = accesskit::Node::new(accesskit::Role::Label);
    status.set_value(browser_status_text(state));
    status.set_bounds(accessibility_rect(
        (ADDRESS_BAR_X + ADDRESS_BAR_WIDTH + 12) as f32,
        17.0,
        96.0,
        14.0,
    ));
    nodes.push((accesskit::NodeId(ACCESSIBILITY_STATUS_ID), status));
    root.push_child(accesskit::NodeId(ACCESSIBILITY_STATUS_ID));
}

#[cfg(feature = "accessibility")]
pub(crate) fn push_link_accessibility_nodes(
    links: &[LinkTarget],
    root: &mut accesskit::Node,
    nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
) {
    for (index, target) in links.iter().enumerate() {
        let id = accesskit::NodeId(ACCESSIBILITY_LINK_BASE_ID + index as u64);
        let mut link = accesskit::Node::new(accesskit::Role::Link);
        link.set_label(target.href.as_str());
        link.set_url(target.href.as_str());
        link.set_bounds(accessibility_rect_from_layout(target.rect));
        link.add_action(accesskit::Action::Click);
        nodes.push((id, link));
        root.push_child(id);
    }
}

#[cfg(feature = "accessibility")]
pub(crate) fn push_input_accessibility_nodes(
    state: &BrowserState,
    root: &mut accesskit::Node,
    nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
) {
    for target in &state.frame.input_targets {
        let id = accessibility_input_id(target.node);
        let mut input = accesskit::Node::new(accesskit::Role::TextInput);
        input.set_label("Page input");
        let value = accessibility_input_value(state, target.node);
        input.set_value(value.as_str());
        input.set_bounds(accessibility_rect_from_layout(target.rect));
        input.add_action(accesskit::Action::Focus);
        nodes.push((id, input));
        root.push_child(id);
    }
}

#[cfg(feature = "accessibility")]
pub(crate) fn accessibility_focus_id(state: &BrowserState) -> accesskit::NodeId {
    if state.address_editing {
        return accesskit::NodeId(ACCESSIBILITY_ADDRESS_ID);
    }
    state.focused_input.map_or(
        accesskit::NodeId(ACCESSIBILITY_ROOT_ID),
        accessibility_input_id,
    )
}

#[cfg(feature = "accessibility")]
pub(crate) fn accessibility_input_id(node: silksurf_dom::NodeId) -> accesskit::NodeId {
    accesskit::NodeId(ACCESSIBILITY_INPUT_BASE_ID + node.raw() as u64)
}

#[cfg(feature = "accessibility")]
pub(crate) fn accessibility_input_value(
    state: &BrowserState,
    node: silksurf_dom::NodeId,
) -> String {
    let Some(runtime) = &state.runtime else {
        return String::new();
    };
    let dom = runtime
        .dom
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    input_value(&dom, node)
}

#[cfg(feature = "accessibility")]
pub(crate) fn accessibility_rect_from_layout(rect: Rect) -> accesskit::Rect {
    accessibility_rect(rect.x, rect.y, rect.width, rect.height)
}

#[cfg(feature = "accessibility")]
pub(crate) fn accessibility_rect(x: f32, y: f32, width: f32, height: f32) -> accesskit::Rect {
    accesskit::Rect::new(x as f64, y as f64, (x + width) as f64, (y + height) as f64)
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "accessibility")]
    // Module split from the former single-file binary; the crate root
    // re-exports every module so sibling items resolve by bare name.
    #[allow(clippy::wildcard_imports)]
    use crate::*;

    #[cfg(feature = "accessibility")]
    #[test]
    fn accessibility_snapshot_exposes_chrome_links_and_inputs() {
        let input_node = silksurf_dom::NodeId::from_raw(21);
        let mut state = test_browser_state("https://example.com/");
        state.frame.link_targets.push(LinkTarget {
            rect: Rect {
                x: 10.0,
                y: 80.0,
                width: 120.0,
                height: 20.0,
            },
            href: "https://example.com/docs".to_string(),
        });
        state.frame.input_targets.push(InputTarget {
            rect: Rect {
                x: 30.0,
                y: 120.0,
                width: 240.0,
                height: 24.0,
            },
            node: input_node,
        });
        state.focused_input = Some(input_node);

        let update = build_browser_accessibility_update(&state);
        let root = accessibility_node(&update, ACCESSIBILITY_ROOT_ID);
        let address = accessibility_node(&update, ACCESSIBILITY_ADDRESS_ID);
        let link = accessibility_node(&update, ACCESSIBILITY_LINK_BASE_ID);
        let input = accessibility_node(
            &update,
            ACCESSIBILITY_INPUT_BASE_ID + input_node.raw() as u64,
        );

        assert_eq!(
            update.tree.as_ref().expect("tree exists").root,
            accesskit::NodeId(ACCESSIBILITY_ROOT_ID)
        );
        assert_eq!(
            update.focus,
            accesskit::NodeId(ACCESSIBILITY_INPUT_BASE_ID + input_node.raw() as u64)
        );
        assert!(
            root.children()
                .contains(&accesskit::NodeId(ACCESSIBILITY_BACK_ID))
        );
        assert!(
            root.children()
                .contains(&accesskit::NodeId(ACCESSIBILITY_ADDRESS_ID))
        );
        assert_eq!(address.role(), accesskit::Role::UrlInput);
        assert_eq!(address.value(), Some("https://example.com/"));
        assert_eq!(link.role(), accesskit::Role::Link);
        assert_eq!(link.url(), Some("https://example.com/docs"));
        assert_eq!(input.role(), accesskit::Role::TextInput);
    }

    #[cfg(feature = "accessibility")]
    #[test]
    fn accessibility_snapshot_focuses_address_while_editing() {
        let mut state = test_browser_state("https://example.com/");
        state.address_editing = true;
        state.address_text = "https://example.com/search".to_string();

        let update = build_browser_accessibility_update(&state);
        let address = accessibility_node(&update, ACCESSIBILITY_ADDRESS_ID);

        assert_eq!(update.focus, accesskit::NodeId(ACCESSIBILITY_ADDRESS_ID));
        assert_eq!(address.value(), Some("https://example.com/search"));
    }

    #[cfg(feature = "accessibility")]
    fn accessibility_node(update: &accesskit::TreeUpdate, id: u64) -> &accesskit::Node {
        update
            .nodes
            .iter()
            .find_map(|(node_id, node)| (*node_id == accesskit::NodeId(id)).then_some(node))
            .expect("accessibility node exists")
    }
}
