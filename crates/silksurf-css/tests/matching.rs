use silksurf_css::{
    CssTokenizer, Specificity, matches_selector, parse_selector_list, selector_specificity,
};
use silksurf_dom::Dom;

fn selector_from(input: &str) -> silksurf_css::Selector {
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed(input).unwrap();
    tokens.extend(tokenizer.finish().unwrap());
    let list = parse_selector_list(tokens);
    list.selectors.into_iter().next().expect("selector")
}

#[test]
fn matches_basic_selectors() {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let body = dom.create_element("body");
    dom.append_child(html, body).unwrap();
    let div = dom.create_element("div");
    dom.set_attribute(div, "id", "main").unwrap();
    dom.set_attribute(div, "class", "item hero").unwrap();
    dom.append_child(body, div).unwrap();
    let span = dom.create_element("span");
    dom.append_child(body, span).unwrap();

    let sel_item = selector_from(".item");
    let sel_id = selector_from("#main");
    let sel_adjacent = selector_from("div + span");

    assert!(matches_selector(&dom, div, &sel_item));
    assert!(!matches_selector(&dom, span, &sel_item));
    assert!(matches_selector(&dom, div, &sel_id));
    assert!(matches_selector(&dom, span, &sel_adjacent));
}

#[test]
fn computes_specificity() {
    let selector = selector_from("div#main.item");
    let specificity = selector_specificity(&selector);
    assert_eq!(
        specificity,
        Specificity {
            ids: 1,
            classes: 1,
            elements: 1,
        }
    );
}

#[test]
fn matches_attribute_operators() {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let body = dom.create_element("body");
    dom.append_child(html, body).unwrap();
    let link = dom.create_element("a");
    dom.set_attribute(link, "href", "https://example.com")
        .unwrap();
    dom.set_attribute(link, "rel", "hero nofollow").unwrap();
    dom.set_attribute(link, "lang", "en-us").unwrap();
    dom.set_attribute(link, "title", "superhero").unwrap();
    dom.set_attribute(link, "name", "userid").unwrap();
    dom.append_child(body, link).unwrap();

    assert!(matches_selector(
        &dom,
        link,
        &selector_from("a[href^=https]")
    ));
    assert!(matches_selector(
        &dom,
        link,
        &selector_from("a[rel~=nofollow]")
    ));
    assert!(matches_selector(&dom, link, &selector_from("a[lang|=en]")));
    assert!(matches_selector(
        &dom,
        link,
        &selector_from("a[title*=hero]")
    ));
    assert!(matches_selector(&dom, link, &selector_from("a[name$=id]")));
}

#[test]
fn matches_pseudo_classes() {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let body = dom.create_element("body");
    dom.append_child(html, body).unwrap();
    let first = dom.create_element("div");
    dom.append_child(body, first).unwrap();
    let empty = dom.create_element("p");
    dom.append_child(body, empty).unwrap();
    let only_parent = dom.create_element("section");
    dom.append_child(body, only_parent).unwrap();
    let only_child = dom.create_element("em");
    dom.append_child(only_parent, only_child).unwrap();
    let last = dom.create_element("span");
    dom.append_child(body, last).unwrap();

    assert!(matches_selector(&dom, html, &selector_from(":root")));
    assert!(matches_selector(
        &dom,
        first,
        &selector_from(":first-child")
    ));
    assert!(matches_selector(&dom, last, &selector_from(":last-child")));
    assert!(matches_selector(
        &dom,
        only_child,
        &selector_from(":only-child")
    ));
    assert!(matches_selector(&dom, empty, &selector_from(":empty")));
}
