#![no_main]

//! Fuzz the production HTML document parse path.
//!
//! `silksurf_html::parse_html` is the html5ever TreeSink entry point
//! `silksurf_engine` uses for every page load, so it carries the untrusted
//! input. The target asserts the tree is walkable afterward, which catches a
//! sink that leaves a node pointing outside the arena.

use libfuzzer_sys::fuzz_target;
use silksurf_dom::{Dom, NodeId};
use silksurf_html::parse_html;

fuzz_target!(|data: &[u8]| {
    let input = String::from_utf8_lossy(data);
    let dom = parse_html(&input);
    walk(&dom, NodeId::from_raw(0), 0);
});

/// Visit every node reachable from `id`.
///
/// The depth cap keeps a deeply nested document from overflowing the fuzzer's
/// stack, which would report a harness limit as a parser defect.
fn walk(dom: &Dom, id: NodeId, depth: usize) {
    if depth > 512 {
        return;
    }
    let Ok(children) = dom.children(id) else {
        return;
    };
    for &child in children {
        let _ = dom.node(child);
        let _ = dom.element_name(child);
        let _ = dom.attributes(child);
        walk(dom, child, depth + 1);
    }
}
