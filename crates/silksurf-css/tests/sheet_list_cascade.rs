//! StyleIndex construction over an ordered list of sheets.

use silksurf_css::{StyleIndex, parse_stylesheet};

#[test]
fn a_sheet_list_cascades_in_list_order() {
    let first = parse_stylesheet("p { color: red }").expect("first sheet parses");
    let second = parse_stylesheet("p { color: blue }").expect("second sheet parses");
    let index = StyleIndex::for_viewport_sheets(&[first.clone(), second.clone()], 1280.0, 800.0);
    let concatenated =
        parse_stylesheet("p { color: red } p { color: blue }").expect("concatenation parses");
    let flat = StyleIndex::for_viewport(&concatenated, 1280.0, 800.0);
    assert_eq!(
        index.active_rules, flat.active_rules,
        "a sheet list flattens to what the same text concatenated produces"
    );
}

#[test]
fn one_layer_name_spans_the_sheet_list() {
    let first = parse_stylesheet("@layer base, theme; @layer theme { p { color: blue } }")
        .expect("first sheet parses");
    let second = parse_stylesheet("@layer base { p { color: red } }").expect("second sheet parses");
    let index = StyleIndex::for_viewport_sheets(&[first, second], 1280.0, 800.0);
    assert_eq!(
        index.active_rules.len(),
        2,
        "both layered rules stay active across the sheet boundary"
    );
}

/// A rule list that grew must not carry the previous node's match cache.
///
/// `CascadeWorkspace::prepare` runs per node and retains its buffers across
/// runs. `Vec::resize` appends the fill value and leaves existing elements
/// alone, so a rule list that grew -- a CSSOM insertRule, a `<style>` the
/// document gained -- kept the entries the previous run's last node wrote.
/// The first node of the new run then took whatever rule those indices named.
#[test]
fn a_grown_rule_list_drops_the_previous_match_cache() {
    use silksurf_css::{CascadeWorkspace, LengthOrAuto, compute_style_for_node_with_workspace};
    use silksurf_dom::Dom;

    let mut dom = Dom::new();
    let document = dom.create_document();
    let html = dom.create_element("html");
    let body = dom.create_element("body");
    let target = dom.create_element("div");
    dom.set_attribute(target, "id", "box").expect("id attaches");
    dom.append_child(document, html).expect("html attaches");
    dom.append_child(html, body).expect("body attaches");
    dom.append_child(body, target).expect("div attaches");
    dom.materialize_resolve_table();

    let mut workspace = CascadeWorkspace::new(2);
    let viewport = (1280.0, 800.0);

    // The div matches the rule at the tail of the list, which writes that
    // rule's index into the workspace cache.
    let small = parse_stylesheet("html { color: red } #box { width: 100px }").expect("parses");
    let small_index = StyleIndex::for_viewport(&small, viewport.0, viewport.1);
    let _ = compute_style_for_node_with_workspace(
        &dom,
        target,
        &small,
        &small_index,
        None,
        &mut workspace,
        None,
        16.0,
        viewport,
    );

    // html is the first node of a run against a longer list, through the same
    // workspace. It declares no width whatever the list holds.
    let grown = parse_stylesheet(
        "html { color: red } #box { width: 100px } #box { width: 250px } p { color: blue }",
    )
    .expect("parses");
    let grown_index = StyleIndex::for_viewport(&grown, viewport.0, viewport.1);
    let style = compute_style_for_node_with_workspace(
        &dom,
        html,
        &grown,
        &grown_index,
        None,
        &mut workspace,
        None,
        16.0,
        viewport,
    );
    assert_eq!(
        style.width,
        LengthOrAuto::Auto,
        "html takes no width from a rule that selects #box"
    );
}
