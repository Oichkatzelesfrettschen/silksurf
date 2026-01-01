use silksurf_css::{compute_styles, parse_stylesheet_with_interner};
use silksurf_dom::Dom;
use std::time::Instant;

fn main() {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let root = dom.create_element("div");
    dom.set_attribute(root, "id", "root").expect("id");
    dom.append_child(document, root).expect("append root");

    for i in 0..16 {
        let child = dom.create_element("span");
        let class = if i % 2 == 0 { "item" } else { "item alt" };
        dom.set_attribute(child, "class", class).expect("class");
        dom.append_child(root, child).expect("append child");
    }

    let css = "#root { display: block; } .item { margin: 1px; } .alt { color: blue; }";
    let stylesheet = dom
        .with_interner_mut(|interner| parse_stylesheet_with_interner(css, interner))
        .expect("parse stylesheet");

    let iterations = 1000;
    let start = Instant::now();
    let mut last_count = 0usize;
    for _ in 0..iterations {
        let styles = compute_styles(&dom, document, &stylesheet);
        last_count = styles.len();
    }
    let elapsed = start.elapsed();
    let per_iter = elapsed / iterations as u32;
    println!("cascade_guard iterations: {}", iterations);
    println!("total: {:?}, per-iter: {:?}", elapsed, per_iter);
    println!("styled nodes: {}", last_count);
}
