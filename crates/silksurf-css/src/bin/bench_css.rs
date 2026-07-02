#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use silksurf_core::SilkInterner;
use silksurf_css::parse_stylesheet;
use std::time::Instant;

fn main() {
    if let Some(path) = std::env::args().nth(1) {
        let css = std::fs::read_to_string(&path).expect("read css file");
        let mut interner = SilkInterner::new();
        let start = Instant::now();
        let sheet = silksurf_css::parse_stylesheet_with_interner(&css, &mut interner)
            .expect("parse css file");
        let elapsed = start.elapsed();
        println!("css file: {path}");
        println!("bytes: {}", css.len());
        println!("rules: {}", sheet.rules.len());
        println!("total: {elapsed:?}");
        return;
    }

    let css = "body { color: red; margin: 8px 12px; }\n@media screen { .hero { padding: 4px; } }";
    let iterations = 1000;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = parse_stylesheet(css).expect("parse css");
    }
    let elapsed = start.elapsed();
    let per_iter = elapsed / iterations as u32;
    println!("css parse iterations: {iterations}");
    println!("total: {elapsed:?}, per-iter: {per_iter:?}");
}
