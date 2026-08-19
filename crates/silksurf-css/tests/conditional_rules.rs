//! Cascade admission of conditional group rules: @layer, @supports, @media.
//!
//! A rule nested in a conditional group reaches the cascade only when
//! `StyleIndex` flattens that group. These cases pin the admission decision and
//! the layer precedence order that `flatten_active_rules` encodes.

use silksurf_css::{Color, Display, compute_styles, parse_stylesheet};
use silksurf_dom::{Dom, NodeId};

const RED: Color = Color {
    r: 255,
    g: 0,
    b: 0,
    a: 255,
};
const GREEN: Color = Color {
    r: 0,
    g: 128,
    b: 0,
    a: 255,
};
const BLUE: Color = Color {
    r: 0,
    g: 0,
    b: 255,
    a: 255,
};

/// Build `<html><body><div id="target">` and return the document and the div.
fn document_with_target() -> (Dom, NodeId, NodeId) {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let body = dom.create_element("body");
    dom.append_child(html, body).unwrap();
    let div = dom.create_element("div");
    dom.set_attribute(div, "id", "target").unwrap();
    dom.append_child(body, div).unwrap();
    (dom, doc, div)
}

fn target_color(css: &str) -> Color {
    let stylesheet = parse_stylesheet(css).expect("stylesheet parses");
    let (dom, doc, div) = document_with_target();
    let styles = compute_styles(&dom, doc, &stylesheet);
    styles.get(&div).expect("target style").color
}

fn target_display(css: &str) -> Display {
    let stylesheet = parse_stylesheet(css).expect("stylesheet parses");
    let (dom, doc, div) = document_with_target();
    let styles = compute_styles(&dom, doc, &stylesheet);
    styles.get(&div).expect("target style").display
}

#[test]
fn layer_block_rules_reach_the_cascade() {
    assert_eq!(target_color("@layer base { #target { color: red; } }"), RED);
}

#[test]
fn pseudo_class_selector_inside_a_layer_keeps_the_block_a_rule_list() {
    // `a:hover` presents `Ident Colon` at the head of the block; the block is
    // still a rule list, and the sibling rule still applies.
    assert_eq!(
        target_color("@layer base { a:hover { color: blue; } #target { color: red; } }"),
        RED
    );
}

#[test]
fn unlayered_rules_win_over_layered_rules_of_higher_specificity() {
    // CSS Cascade 5, 6.4.4: unlayered declarations beat layered ones in the
    // same origin regardless of the order they appear in the sheet.
    assert_eq!(
        target_color("div { color: green; } @layer base { #target { color: red; } }"),
        GREEN
    );
}

#[test]
fn later_declared_layers_win_over_earlier_ones() {
    assert_eq!(
        target_color(
            "@layer first { #target { color: red; } } @layer second { #target { color: blue; } }"
        ),
        BLUE
    );
}

#[test]
fn a_layer_statement_fixes_the_order_of_later_layer_blocks() {
    // `@layer second, first;` declares `first` last, so its rules win even
    // though its block appears first in the sheet.
    assert_eq!(
        target_color(
            "@layer second, first; \
             @layer first { #target { color: red; } } \
             @layer second { #target { color: blue; } }"
        ),
        RED
    );
}

#[test]
fn nested_layers_qualify_their_names() {
    assert_eq!(
        target_color(
            "@layer outer { @layer a { #target { color: blue; } } \
             @layer b { #target { color: red; } } }"
        ),
        RED
    );
}

#[test]
fn supported_condition_admits_its_block() {
    assert_eq!(
        target_display("@supports (display: flex) { #target { display: flex; } }"),
        Display::Flex
    );
}

#[test]
fn unsupported_condition_rejects_its_block() {
    assert_eq!(
        target_color("@supports (field-sizing: content) { #target { color: red; } }"),
        Color::black()
    );
}

#[test]
fn negated_unsupported_condition_admits_the_fallback_block() {
    assert_eq!(
        target_color("@supports not (height: anchor-size(height)) { #target { color: red; } }"),
        RED
    );
}

#[test]
fn media_query_inside_a_layer_still_evaluates() {
    assert_eq!(
        target_color("@layer base { @media (min-width: 1px) { #target { color: red; } } }"),
        RED
    );
}

#[test]
fn keyframe_blocks_stay_out_of_the_selector_cascade() {
    assert_eq!(
        target_color("@keyframes spin { from { color: red; } to { color: red; } }"),
        Color::black()
    );
}

/*
 * CSS Cascade 5, 6.4.4 reverses layer order for important declarations. These
 * cases pin the reversal against the normal-declaration order the tests above
 * fix, so a comparison that ignores importance fails one set or the other.
 */

#[test]
fn earlier_declared_layers_win_over_later_ones_for_important() {
    assert_eq!(
        target_color(
            "@layer first { #target { color: red !important; } } \
             @layer second { #target { color: blue !important; } }"
        ),
        RED
    );
}

#[test]
fn a_layer_statement_fixes_the_reversed_order_of_important_declarations() {
    // `@layer second, first;` declares `first` last, so for important
    // declarations `second` is the earlier layer and its rules win.
    assert_eq!(
        target_color(
            "@layer second, first; \
             @layer first { #target { color: red !important; } } \
             @layer second { #target { color: blue !important; } }"
        ),
        BLUE
    );
}

#[test]
fn a_layered_important_declaration_wins_over_an_unlayered_one() {
    // The mirror of unlayered_rules_win_over_layered_rules_of_higher_specificity:
    // UNLAYERED is the maximum rank, so its complement is the minimum.
    assert_eq!(
        target_color(
            "#target { color: green !important; } \
             @layer base { div { color: red !important; } }"
        ),
        RED
    );
}

#[test]
fn an_important_style_attribute_wins_over_an_important_layered_rule() {
    // The element-attached step of CSS Cascade 5, 6.4.3 sits outside the
    // layer reversal, so the attribute keeps its precedence.
    let stylesheet = parse_stylesheet("@layer base { #target { color: red !important; } }")
        .expect("stylesheet parses");
    let (mut dom, doc, div) = document_with_target();
    dom.set_attribute(div, "style", "color: blue !important")
        .unwrap();
    let styles = compute_styles(&dom, doc, &stylesheet);
    assert_eq!(styles.get(&div).expect("target style").color, BLUE);
}

#[test]
fn an_important_layered_custom_property_takes_the_earlier_layer() {
    let stylesheet = parse_stylesheet(
        "@layer first { :root { --ink: red; } } \
         @layer second { :root { --ink: blue; } } \
         @layer first { #target { color: var(--ink) !important; } }",
    )
    .expect("stylesheet parses");
    let (dom, doc, div) = document_with_target();
    let styles = compute_styles(&dom, doc, &stylesheet);
    // The custom property itself is a normal declaration, so `second` wins it.
    assert_eq!(styles.get(&div).expect("target style").color, BLUE);
}
