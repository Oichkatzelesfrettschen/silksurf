use silksurf_css::{
    CssTokenizer, Specificity, matches_selector, matches_selector_list, parse_selector_list,
    selector_specificity,
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

#[test]
fn matches_nth_child() {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let body = dom.create_element("body");
    dom.append_child(html, body).unwrap();
    // body has four li children (positions 1-4)
    let items: Vec<_> = (0..4)
        .map(|_| {
            let li = dom.create_element("li");
            dom.append_child(body, li).unwrap();
            li
        })
        .collect();

    // :nth-child(odd) -- positions 1, 3
    let odd = selector_from(":nth-child(odd)");
    assert!(matches_selector(&dom, items[0], &odd));
    assert!(!matches_selector(&dom, items[1], &odd));
    assert!(matches_selector(&dom, items[2], &odd));
    assert!(!matches_selector(&dom, items[3], &odd));

    // :nth-child(even) -- positions 2, 4
    let even = selector_from(":nth-child(even)");
    assert!(!matches_selector(&dom, items[0], &even));
    assert!(matches_selector(&dom, items[1], &even));
    assert!(!matches_selector(&dom, items[2], &even));
    assert!(matches_selector(&dom, items[3], &even));

    // :nth-child(2n+1) == odd
    let two_n_plus_one = selector_from(":nth-child(2n+1)");
    assert!(matches_selector(&dom, items[0], &two_n_plus_one));
    assert!(!matches_selector(&dom, items[1], &two_n_plus_one));

    // :nth-child(3) -- only position 3
    let third = selector_from(":nth-child(3)");
    assert!(!matches_selector(&dom, items[0], &third));
    assert!(!matches_selector(&dom, items[1], &third));
    assert!(matches_selector(&dom, items[2], &third));
    assert!(!matches_selector(&dom, items[3], &third));
}

#[test]
fn matches_not_is_where() {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let body = dom.create_element("body");
    dom.append_child(html, body).unwrap();
    let div = dom.create_element("div");
    dom.set_attribute(div, "class", "active").unwrap();
    dom.append_child(body, div).unwrap();
    let span = dom.create_element("span");
    dom.append_child(body, span).unwrap();

    // :not(.active) matches span, not div
    let not_active = selector_from(":not(.active)");
    assert!(!matches_selector(&dom, div, &not_active));
    assert!(matches_selector(&dom, span, &not_active));

    // :is(div, span) matches both
    let is_div_or_span = selector_from(":is(div, span)");
    assert!(matches_selector(&dom, div, &is_div_or_span));
    assert!(matches_selector(&dom, span, &is_div_or_span));

    // :where(div) has 0 specificity but still matches
    let where_div = selector_from(":where(div)");
    assert!(matches_selector(&dom, div, &where_div));
    assert!(!matches_selector(&dom, span, &where_div));
}

#[test]
fn matches_of_type_pseudo_classes() {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let body = dom.create_element("body");
    dom.append_child(html, body).unwrap();
    // body: div, span, div
    let div1 = dom.create_element("div");
    dom.append_child(body, div1).unwrap();
    let span = dom.create_element("span");
    dom.append_child(body, span).unwrap();
    let div2 = dom.create_element("div");
    dom.append_child(body, div2).unwrap();

    // div1 is first-of-type for div; span is both first and last of its type
    assert!(matches_selector(
        &dom,
        div1,
        &selector_from(":first-of-type")
    ));
    assert!(!matches_selector(
        &dom,
        div2,
        &selector_from(":first-of-type")
    ));
    assert!(matches_selector(
        &dom,
        div2,
        &selector_from(":last-of-type")
    ));
    assert!(!matches_selector(
        &dom,
        div1,
        &selector_from(":last-of-type")
    ));
    assert!(matches_selector(
        &dom,
        span,
        &selector_from(":only-of-type")
    ));
    assert!(!matches_selector(
        &dom,
        div1,
        &selector_from(":only-of-type")
    ));
}

#[test]
fn where_contributes_zero_specificity() {
    // :where() contributes 0 to specificity while :is() uses max of its args.
    let where_sel = selector_from("div:where(.active)");
    let is_sel = selector_from("div:is(.active)");

    let where_spec = selector_specificity(&where_sel);
    let is_spec = selector_specificity(&is_sel);

    // div:where(.active) -> (0, 0, 1) -- element only
    assert_eq!(where_spec, Specificity { ids: 0, classes: 0, elements: 1 });
    // div:is(.active) -> (0, 1, 1) -- element + class from .active arg
    assert_eq!(is_spec, Specificity { ids: 0, classes: 1, elements: 1 });
}

#[test]
fn matches_nth_last_child() {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let body = dom.create_element("body");
    dom.append_child(html, body).unwrap();
    let items: Vec<_> = (0..4)
        .map(|_| {
            let li = dom.create_element("li");
            dom.append_child(body, li).unwrap();
            li
        })
        .collect();

    // :nth-last-child(1) == :last-child
    let last = selector_from(":nth-last-child(1)");
    assert!(!matches_selector(&dom, items[0], &last));
    assert!(matches_selector(&dom, items[3], &last));

    // :nth-last-child(odd) -- positions from end: 1, 3 -> items[3], items[1]
    let nth_last_odd = selector_from(":nth-last-child(odd)");
    assert!(!matches_selector(&dom, items[0], &nth_last_odd));
    assert!(matches_selector(&dom, items[1], &nth_last_odd));
    assert!(!matches_selector(&dom, items[2], &nth_last_odd));
    assert!(matches_selector(&dom, items[3], &nth_last_odd));
}

#[test]
fn matches_has_pseudo_class() {
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let body = dom.create_element("body");
    dom.append_child(html, body).unwrap();
    let article = dom.create_element("article");
    dom.append_child(body, article).unwrap();
    let _h1 = {
        let h1 = dom.create_element("h1");
        dom.append_child(article, h1).unwrap();
        h1
    };
    let section = dom.create_element("section");
    dom.append_child(body, section).unwrap();

    // article:has(h1) matches; section:has(h1) does not
    let list = parse_selector_list({
        let mut t = CssTokenizer::new();
        let mut tok = t.feed(":has(h1)").unwrap();
        tok.extend(t.finish().unwrap());
        tok
    });
    assert!(matches_selector_list(&dom, article, &list));
    assert!(!matches_selector_list(&dom, section, &list));
}
