use silksurf_css::{CssTokenizer, matches_selector, parse_selector_list_with_interner};
use silksurf_dom::Dom;
use std::env;
use std::time::Instant;

fn main() {
    let guard = env::args().any(|arg| arg == "--guard");
    let workload = env::args().any(|arg| arg == "--workload");

    let selector = "div#main.item[data-role=hero] span.highlight";
    let mut tokenizer = CssTokenizer::new();
    let mut tokens = tokenizer.feed(selector).expect("tokenize selector");
    tokens.extend(tokenizer.finish().expect("finish tokenizer"));

    let mut dom = Dom::new();
    let document = dom.create_document();
    let container = dom.create_element("div");
    dom.set_attribute(container, "id", "main").expect("id");
    dom.set_attribute(container, "class", "item hero")
        .expect("class");
    dom.set_attribute(container, "data-role", "hero")
        .expect("data-role");
    let section = dom.create_element("section");
    dom.set_attribute(section, "class", "content")
        .expect("section class");
    let span = dom.create_element("span");
    dom.set_attribute(span, "class", "highlight")
        .expect("highlight");
    dom.append_child(document, container).expect("append div");
    dom.append_child(container, section)
        .expect("append section");
    dom.append_child(section, span).expect("append span");

    let mut workload_nodes = Vec::new();
    for i in 0..8 {
        let node = dom.create_element("span");
        let class = if i % 2 == 0 { "highlight" } else { "muted" };
        dom.set_attribute(node, "class", class).expect("class");
        if i % 3 == 0 {
            dom.set_attribute(node, "data-state", "active")
                .expect("data-state");
        }
        dom.append_child(section, node)
            .expect("append workload span");
        workload_nodes.push(node);
    }

    let selector_list =
        dom.with_interner_mut(|interner| parse_selector_list_with_interner(tokens, Some(interner)));
    let selector = selector_list.selectors.first().expect("selector parse");

    let iterations = if guard { 50_000 } else { 200_000 };
    let start = Instant::now();
    let mut matched = 0usize;
    for _ in 0..iterations {
        if matches_selector(&dom, span, selector) {
            matched += 1;
        }
    }
    let elapsed = start.elapsed();
    let per_iter = elapsed / iterations as u32;
    println!("selector match iterations: {iterations}");
    println!("total: {elapsed:?}, per-iter: {per_iter:?}");
    println!("matches: {matched}");

    if workload {
        let workload_selectors = [
            "section.content > span.muted",
            "span[data-state=active]",
            "div#main span",
            ".item .highlight",
        ];
        let mut parsed = Vec::new();
        for selector in workload_selectors {
            let mut tokenizer = CssTokenizer::new();
            let mut tokens = tokenizer
                .feed(selector)
                .expect("tokenize workload selector");
            tokens.extend(tokenizer.finish().expect("finish workload tokenizer"));
            let list = dom.with_interner_mut(|interner| {
                parse_selector_list_with_interner(tokens, Some(interner))
            });
            if let Some(first) = list.selectors.into_iter().next() {
                parsed.push(first);
            }
        }

        let workload_iterations = if guard { 5_000 } else { 50_000 };
        let start = Instant::now();
        let mut matched = 0usize;
        for _ in 0..workload_iterations {
            for selector in &parsed {
                for &node in &workload_nodes {
                    if matches_selector(&dom, node, selector) {
                        matched += 1;
                    }
                }
            }
        }
        let elapsed = start.elapsed();
        let per_iter = elapsed / workload_iterations as u32;
        println!("workload iterations: {workload_iterations}");
        println!("workload total: {elapsed:?}, per-iter: {per_iter:?}");
        println!("workload matches: {matched}");
    }
}
