use silksurf_css::parse_stylesheet;
use std::time::Instant;

fn main() {
    let css = "body { color: red; margin: 8px 12px; }\n@media screen { .hero { padding: 4px; } }";
    let iterations = 1000;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = parse_stylesheet(css).expect("parse css");
    }
    let elapsed = start.elapsed();
    let per_iter = elapsed / iterations as u32;
    println!("css parse iterations: {}", iterations);
    println!("total: {:?}, per-iter: {:?}", elapsed, per_iter);
}
