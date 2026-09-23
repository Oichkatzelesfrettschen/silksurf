use silksurf_css::{
    CssTokenizer, Display, compute_styles, matches_selector_list, parse_selector_list,
    parse_stylesheet,
};
use silksurf_dom::{Dom, NodeId};

fn fixture() -> (Dom, NodeId, NodeId, NodeId) {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let html = dom.create_element("html");
    let body = dom.create_element("body");
    let target = dom.create_element("main");
    dom.set_attribute(body, "class", "ancestor")
        .expect("ancestor class");
    dom.set_attribute(target, "class", "target")
        .expect("target class");
    dom.append_child(document, html).expect("html");
    dom.append_child(html, body).expect("body");
    dom.append_child(body, target).expect("target");
    (dom, document, html, target)
}

fn selectors(text: &str) -> silksurf_css::SelectorList {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed(text).expect("tokens");
    tokens.extend(tokenizer.finish().expect("complete tokens"));
    parse_selector_list(tokens)
}

#[test]
fn nested_function_boundaries_preserve_one_compound_selector() {
    let (dom, _, html, target) = fixture();
    for selector in [
        ".target:where(.ancestor:where(*) *):not(#never)",
        ".target:not(:is(.absent, :where(#missing))):not(#never)",
        ".target:is(:where(.target), :not(:is(*))):not(#never)",
    ] {
        let parsed = selectors(selector);
        assert_eq!(parsed.selectors.len(), 1, "{selector}: {parsed:?}");
        assert!(
            !matches_selector_list(&dom, html, &parsed),
            "root matches {selector}"
        );
        assert!(
            matches_selector_list(&dom, target, &parsed),
            "target misses {selector}"
        );
    }
}

#[test]
fn nested_display_rule_preserves_visible_ancestors_and_hides_only_target() {
    let (dom, document, html, target) = fixture();
    let stylesheet = parse_stylesheet("html, body, main { display:block } .absent:where(.ancestor:where(*) *):not(#never){display:none} .target:where(.ancestor:where(*) *):not(#never){display:none}").expect("stylesheet");
    let styles = compute_styles(&dom, document, &stylesheet);
    assert_eq!(styles[&html].display, Display::Block);
    let body = dom.parent(target).expect("parent query").expect("body");
    assert_eq!(styles[&body].display, Display::Block);
    assert_eq!(styles[&target].display, Display::None);
}

#[test]
fn unknown_nested_function_keeps_following_compounds_inside_one_selector() {
    let parsed = selectors(".absent:unknown(:where(*)):not(#never), .target");
    assert_eq!(parsed.selectors.len(), 2, "{parsed:?}");
    let (dom, _, html, target) = fixture();
    assert!(!matches_selector_list(&dom, html, &parsed));
    assert!(matches_selector_list(&dom, target, &parsed));
}

#[test]
fn stray_tokens_cannot_manufacture_a_selector_alternative() {
    for text in [".absent):not(#never)", ".absent,", ",:not(#never)"] {
        assert!(
            selectors(text).selectors.is_empty(),
            "invalid list admitted: {text}"
        );
    }
}
