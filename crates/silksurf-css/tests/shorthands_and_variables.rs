//! Shorthand properties, logical box properties, and `var()` substitution.
//!
//! Each case computes a style for one element and reads the property the
//! declaration should have set, so a value that parses but never reaches the
//! computed style fails here rather than silently painting nothing.

use silksurf_css::{ComputedStyle, Length, LengthOrAuto, compute_style_for_node, parse_stylesheet};
use silksurf_dom::{Dom, NodeId};

fn computed(css: &str, style_attribute: Option<&str>) -> ComputedStyle {
    let stylesheet = parse_stylesheet(css).expect("stylesheet parses");
    let mut dom = Dom::new();
    let document = dom.create_document();
    let html = dom.create_element("html");
    let body = dom.create_element("body");
    let div = dom.create_element("div");
    dom.set_attribute(div, "class", "box").expect("class sets");
    if let Some(text) = style_attribute {
        dom.set_attribute(div, "style", text).expect("style sets");
    }
    dom.append_child(document, html).expect("html attaches");
    dom.append_child(html, body).expect("body attaches");
    dom.append_child(body, div).expect("div attaches");
    chain_computed(&dom, &stylesheet, &[html, body, div])
}

/// Compute down the ancestor chain so inherited values -- custom properties
/// among them -- reach the last element the way the cascade delivers them.
fn chain_computed(
    dom: &Dom,
    stylesheet: &silksurf_css::Stylesheet,
    chain: &[NodeId],
) -> ComputedStyle {
    let mut parent: Option<ComputedStyle> = None;
    for &node in chain {
        let style = compute_style_for_node(dom, node, stylesheet, parent.as_ref());
        parent = Some(style);
    }
    parent.expect("chain holds at least one element")
}

#[test]
fn the_background_shorthand_sets_the_colour() {
    let style = computed(".box { background: #cc0000; }", None);
    assert_eq!(
        (
            style.background_color.r,
            style.background_color.g,
            style.background_color.b,
            style.background_color.a
        ),
        (204, 0, 0, 255)
    );
}

#[test]
fn the_background_shorthand_clears_an_earlier_colour() {
    let style = computed(
        ".box { background-color: #cc0000; } .box { background: transparent; }",
        None,
    );
    assert_eq!(style.background_color.a, 0);
}

#[test]
fn the_background_shorthand_carries_a_gradient() {
    let style = computed(
        ".box { background: linear-gradient(to right, #ff0000, #0000ff); }",
        None,
    );
    assert!(style.background_image.is_some());
}

#[test]
fn a_later_flat_background_replaces_a_gradient() {
    let style = computed(
        ".box { background: linear-gradient(to right, #f00, #00f); } \
         .box { background: #00ff00; }",
        None,
    );
    assert!(style.background_image.is_none());
    assert_eq!(style.background_color.g, 255);
}

#[test]
fn inline_size_and_block_size_resolve_to_width_and_height() {
    let style = computed(".box { inline-size: 200px; block-size: 100px; }", None);
    assert_eq!(style.width, LengthOrAuto::Length(Length::Px(200.0)));
    assert_eq!(style.height, LengthOrAuto::Length(Length::Px(100.0)));
}

#[test]
fn logical_min_and_max_sizes_resolve_to_their_physical_names() {
    let style = computed(
        ".box { min-inline-size: 10px; max-inline-size: 20px; \
                min-block-size: 30px; max-block-size: 40px; }",
        None,
    );
    assert_eq!(style.min_width, Length::Px(10.0));
    assert_eq!(style.max_width, Some(Length::Px(20.0)));
    assert_eq!(style.min_height, Length::Px(30.0));
    assert_eq!(style.max_height, Some(Length::Px(40.0)));
}

#[test]
fn logical_margin_edges_resolve_under_horizontal_writing_mode() {
    let style = computed(
        ".box { margin-block-start: 1px; margin-block-end: 2px; \
                margin-inline-start: 3px; margin-inline-end: 4px; }",
        None,
    );
    assert_eq!(style.margin.top, LengthOrAuto::Length(Length::Px(1.0)));
    assert_eq!(style.margin.bottom, LengthOrAuto::Length(Length::Px(2.0)));
    assert_eq!(style.margin.left, LengthOrAuto::Length(Length::Px(3.0)));
    assert_eq!(style.margin.right, LengthOrAuto::Length(Length::Px(4.0)));
}

#[test]
fn the_two_value_logical_shorthands_cover_both_edges() {
    let style = computed(".box { margin-inline: 5px 6px; padding-block: 7px; }", None);
    assert_eq!(style.margin.left, LengthOrAuto::Length(Length::Px(5.0)));
    assert_eq!(style.margin.right, LengthOrAuto::Length(Length::Px(6.0)));
    assert_eq!(style.padding.top, Length::Px(7.0));
    assert_eq!(style.padding.bottom, Length::Px(7.0));
}

#[test]
fn a_variable_declared_on_the_element_substitutes() {
    let style = computed(
        ".box { --tone: #cc0000; background-color: var(--tone); }",
        None,
    );
    assert_eq!(style.background_color.r, 204);
}

#[test]
fn a_variable_inherits_from_an_ancestor() {
    let style = computed(
        "html { --tone: #00cc00; } .box { background-color: var(--tone); }",
        None,
    );
    assert_eq!(style.background_color.g, 204);
}

#[test]
fn the_root_pseudo_class_declares_an_inheritable_variable() {
    let style = computed(
        ":root { --tone: #0000cc; } .box { background-color: var(--tone); }",
        None,
    );
    assert_eq!(style.background_color.b, 204);
}

#[test]
fn a_variable_fallback_applies_when_the_name_is_undeclared() {
    let style = computed(".box { background-color: var(--absent, #cc0000); }", None);
    assert_eq!(style.background_color.r, 204);
}

#[test]
fn an_undeclared_variable_without_a_fallback_leaves_the_property_unset() {
    let style = computed(
        ".box { background-color: #00ff00; } .box { background-color: var(--absent); }",
        None,
    );
    assert_eq!(style.background_color.g, 255);
}

#[test]
fn a_more_specific_declaration_wins_the_variable() {
    let style = computed(
        "div { --tone: #ff0000; } .box { --tone: #0000ff; } \
         .box { background-color: var(--tone); }",
        None,
    );
    assert_eq!(style.background_color.b, 255);
}

#[test]
fn an_inline_style_declares_and_reads_a_variable() {
    let style = computed(
        ".box { }",
        Some("--tone: #cc0000; background-color: var(--tone)"),
    );
    assert_eq!(style.background_color.r, 204);
}

#[test]
fn a_variable_holding_a_variable_resolves_through() {
    let style = computed(
        ":root { --base: #cc0000; --tone: var(--base); } \
         .box { background-color: var(--tone); }",
        None,
    );
    assert_eq!(style.background_color.r, 204);
}

#[test]
fn the_background_shorthand_reads_a_variable() {
    let style = computed(
        ":root { --tone: #cc0000; } .box { background: var(--tone); }",
        None,
    );
    assert_eq!(style.background_color.r, 204);
}

#[test]
fn the_font_shorthand_sets_size_and_family() {
    let style = computed(".box { font: bold 20px/28px Georgia, serif; }", None);
    assert_eq!(style.font_size, Length::Px(20.0));
    assert_eq!(style.line_height, Length::Px(28.0));
    assert_eq!(
        style.font_family.first().map(smol_str::SmolStr::as_str),
        Some("Georgia")
    );
}

#[test]
fn place_items_sets_both_axes() {
    let style = computed(".box { display: flex; place-items: center; }", None);
    assert_eq!(
        style.flex_container.align_items,
        silksurf_css::AlignItems::Center
    );
    assert_eq!(
        style.flex_container.justify_content,
        silksurf_css::JustifyContent::Center
    );
}

#[test]
fn inset_sets_all_four_offsets() {
    let style = computed(".box { position: absolute; inset: 1px 2px 3px 4px; }", None);
    assert_eq!(style.top, LengthOrAuto::Length(Length::Px(1.0)));
    assert_eq!(style.right, LengthOrAuto::Length(Length::Px(2.0)));
    assert_eq!(style.bottom, LengthOrAuto::Length(Length::Px(3.0)));
    assert_eq!(style.left, LengthOrAuto::Length(Length::Px(4.0)));
}

#[test]
fn transform_translate_reaches_the_computed_style() {
    let style = computed(".box { transform: translate(200px, 30px); }", None);
    assert_eq!(style.transform.x, Length::Px(200.0));
    assert_eq!(style.transform.y, Length::Px(30.0));
}

#[test]
fn transform_axis_functions_set_one_component_each() {
    let x_only = computed(".box { transform: translateX(12px); }", None);
    assert_eq!(x_only.transform.x, Length::Px(12.0));
    assert_eq!(x_only.transform.y, Length::Px(0.0));
    let y_only = computed(".box { transform: translateY(34px); }", None);
    assert_eq!(y_only.transform.x, Length::Px(0.0));
    assert_eq!(y_only.transform.y, Length::Px(34.0));
}

#[test]
fn a_transform_list_sums_its_translations() {
    let style = computed(
        ".box { transform: translateX(10px) rotate(45deg) translateX(5px); }",
        None,
    );
    assert_eq!(style.transform.x, Length::Px(15.0));
}

#[test]
fn a_transform_naming_no_translation_leaves_the_element_in_place() {
    let style = computed(".box { transform: rotate(45deg) scale(2); }", None);
    assert_eq!(style.transform.x, Length::Px(0.0));
    assert_eq!(style.transform.y, Length::Px(0.0));
}

#[test]
fn translate3d_contributes_its_two_dimensional_part() {
    let style = computed(".box { transform: translate3d(7px, 8px, 9px); }", None);
    assert_eq!(style.transform.x, Length::Px(7.0));
    assert_eq!(style.transform.y, Length::Px(8.0));
}
