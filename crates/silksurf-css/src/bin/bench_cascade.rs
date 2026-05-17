use silksurf_css::{compute_styles, parse_stylesheet_with_interner};
use silksurf_dom::Dom;
use std::fmt::Write as _;
use std::time::Instant;

fn main() {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let root = dom.create_element("div");
    dom.set_attribute(root, "id", "root").expect("id");
    dom.set_attribute(root, "class", "container")
        .expect("class");
    dom.append_child(document, root).expect("append root");

    for i in 0..128 {
        let child = dom.create_element("span");
        let class = if i % 2 == 0 {
            "item alpha"
        } else {
            "item beta"
        };
        dom.set_attribute(child, "class", class).expect("class");
        dom.append_child(root, child).expect("append child");
    }

    let mut css = String::new();
    writeln!(&mut css, "#root {{ display: block; padding: 2px; }}").unwrap();
    writeln!(&mut css, ".container span {{ margin: 1px; }}").unwrap();
    for i in 0..64 {
        writeln!(
            &mut css,
            "span.item.alpha[data-i=\"{}\"] {{ padding: {}px; }}",
            i,
            i % 8
        )
        .unwrap();
    }

    let stylesheet = dom
        .with_interner_mut(|interner| parse_stylesheet_with_interner(&css, interner))
        .expect("parse stylesheet");

    let iterations = 5_000;
    let start = Instant::now();
    let mut last_count = 0usize;
    for _ in 0..iterations {
        let styles = compute_styles(&dom, document, &stylesheet);
        last_count = styles.len();
    }
    let elapsed = start.elapsed();
    let per_iter = elapsed / iterations as u32;
    println!("cascade iterations: {iterations}");
    println!("total: {elapsed:?}, per-iter: {per_iter:?}");
    println!("styled nodes: {last_count}");
}
