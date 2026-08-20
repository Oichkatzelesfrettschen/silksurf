//! CSS text serialization: the text form CSSOM hands back for a parsed rule.

use silksurf_css::{
    Rule, declarations_to_css, parse_declaration_list, parse_stylesheet, rule_to_css,
    selector_list_to_css,
};

fn only_rule(source: &str) -> Rule {
    let sheet = parse_stylesheet(source).expect("stylesheet parses");
    assert_eq!(sheet.rules.len(), 1, "fixture declares one rule");
    sheet.rules[0].clone()
}

#[test]
fn a_style_rule_serializes_selector_and_declarations() {
    let text = rule_to_css(&only_rule("a.link { color: red; margin-top: 4px }"));
    assert_eq!(text, "a.link { color: red; margin-top: 4px; }");
}

#[test]
fn an_important_declaration_keeps_its_flag() {
    let text = rule_to_css(&only_rule("p { display: none !important }"));
    assert_eq!(text, "p { display: none !important; }");
}

#[test]
fn a_selector_list_serializes_every_combinator() {
    let Rule::Style(rule) = only_rule("a > b + c ~ d e, #id[data-x=\"1\"] { color: red }") else {
        panic!("fixture declares a style rule");
    };
    assert_eq!(
        selector_list_to_css(&rule.selectors),
        "a > b + c ~ d e, #id[data-x=\"1\"]"
    );
}

#[test]
fn a_functional_pseudo_class_serializes_its_argument() {
    let Rule::Style(rule) = only_rule("li:nth-child(2n+1) { color: red }") else {
        panic!("fixture declares a style rule");
    };
    assert_eq!(selector_list_to_css(&rule.selectors), "li:nth-child(2n+1)");
}

#[test]
fn a_function_value_keeps_its_operators_and_spacing() {
    let declarations = parse_declaration_list("width: calc(100vw - 2 * 20px); color: rgb(1, 2, 3)")
        .expect("declarations parse");
    assert_eq!(
        declarations_to_css(&declarations),
        "width: calc(100vw - 2 * 20px); color: rgb(1, 2, 3);"
    );
}

#[test]
fn an_at_rule_serializes_its_prelude_and_nested_rules() {
    let text = rule_to_css(&only_rule("@media (min-width: 40em) { a { color: red } }"));
    assert_eq!(text, "@media (min-width: 40em) { a { color: red; } }");
}

#[test]
fn a_serialized_stylesheet_reparses_to_the_same_rules() {
    let source = "\
        a.link > span { color: red; margin: 0 auto }\
        @media screen and (min-width: 40em) { .box { width: calc(50% - 8px) !important } }\
        @font-face { font-family: \"Silk Sans\"; src: url(/f.woff2) }\
        li:not(.skip):nth-of-type(3n) { content: \"a\\\"b\" }";
    let parsed = parse_stylesheet(source).expect("source parses");
    let serialized: String = parsed
        .rules
        .iter()
        .map(rule_to_css)
        .collect::<Vec<_>>()
        .join(" ");
    let reparsed = parse_stylesheet(&serialized).expect("serialized text reparses");
    assert_eq!(
        reparsed.rules, parsed.rules,
        "serialization round-trips through the parser\nserialized: {serialized}"
    );
}
