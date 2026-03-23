/*
 * bench_pipeline -- compare 3-pass vs fused single-pass pipeline.
 *
 * WHY: The fused pipeline (fused_style_layout_paint) does style cascade,
 * layout, and display-list building in ONE BFS pass, reducing DOM traversals
 * from 3 to 1 and halving memory bandwidth (read each node once, not three
 * times). This benchmark measures the actual speedup on a representative page.
 *
 * Fixture: 50-node page (header, nav, main, aside, footer) with 13 CSS rules.
 * This is representative of a simple real-world page (not a trivial 1-div page
 * that fits in L1 cache and shows no difference).
 *
 * HTML and CSS are parsed ONCE outside the loop (both paths share the same
 * pre-parsed stylesheet so we measure cascade/layout/paint, not CSS parsing).
 *
 * See: fused_pipeline.rs for implementation details
 * See: neighbor_table.rs for BFS-level decomposition
 */

/*
 * mimalloc -- global allocator for bench_pipeline.
 * WHY: CSS tokenizer and SmolStr allocations are small and frequent.
 * mimalloc thread-local free lists reduce per-alloc cost by 2-4x.
 */
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use silksurf_core::SilkArena;
use silksurf_css::{compute_styles, parse_stylesheet_with_interner};
use silksurf_engine::fused_pipeline::fused_style_layout_paint;
use silksurf_engine::parse_html;
use silksurf_layout::Rect;
use silksurf_layout::{LayoutTree, build_layout_tree};
use silksurf_render::{build_display_list, rasterize_parallel, rasterize_parallel_into};
use std::time::Duration;
use std::time::Instant;

/*
 * BENCH_HTML -- representative multi-section page (~50 DOM nodes).
 *
 * WHY: A 1-div page fits in L1 cache so all three traversals are equally fast.
 * A 50-node page with layout structure exercises the BFS vs DFS difference and
 * the inter-pass data movement (ComputedStyle HashMap + LayoutBox arena).
 */
const BENCH_HTML: &str = concat!(
    "<!doctype html><html><head></head><body>",
    "<header id='hdr'><nav class='container'>",
    "<a href='/'>Logo</a>",
    "<ul class='menu'>",
    "<li><a href='/about'>About</a></li>",
    "<li><a href='/blog'>Blog</a></li>",
    "<li><a href='/contact'>Contact</a></li>",
    "</ul></nav></header>",
    "<main class='container'>",
    "<article>",
    "<h1 class='title'>Page Title</h1>",
    "<p class='body'>Paragraph one with some text content.</p>",
    "<p class='body'>Paragraph two with more text content here.</p>",
    "<p class='body'>Paragraph three closes the article.</p>",
    "<section class='cta'>",
    "<h2>Call to Action</h2>",
    "<p>Sign up today and get started.</p>",
    "<a href='/signup' class='btn'>Sign Up</a>",
    "</section>",
    "</article>",
    "<aside class='sidebar'>",
    "<div class='card'><h3>Widget A</h3><p>Widget A content.</p></div>",
    "<div class='card'><h3>Widget B</h3><p>Widget B content.</p></div>",
    "<div class='card'><h3>Widget C</h3><p>Widget C content.</p></div>",
    "</aside>",
    "</main>",
    "<footer class='container'>",
    "<p>Copyright 2025 SilkSurf. All rights reserved.</p>",
    "<nav><a href='/privacy'>Privacy</a> | <a href='/terms'>Terms</a></nav>",
    "</footer>",
    "</body></html>"
);

/*
 * BENCH_CSS -- 13 rules covering box model, flex, positioning, and color.
 *
 * WHY: A single rule only exercises one code path in apply_declaration.
 * 13 rules with class/tag/id selectors exercise the StyleIndex (tag + class
 * buckets) and the cascade specificity sort. Representative of real CSS.
 */
const BENCH_CSS: &str = concat!(
    "* { margin: 0; padding: 0; }",
    "body { display: block; background-color: #fff; color: #333; font-size: 16px; line-height: 24px; }",
    ".container { display: block; padding: 16px; }",
    "header { display: block; background-color: #222; color: #fff; padding: 16px; }",
    "nav { display: flex; justify-content: space-between; align-items: center; }",
    ".menu { display: flex; gap: 16px; }",
    "main { display: flex; gap: 24px; padding: 24px; }",
    "article { display: block; flex: 1; }",
    ".title { display: block; font-size: 24px; margin: 0 0 16px 0; }",
    ".body { display: block; line-height: 24px; margin: 0 0 12px 0; }",
    ".sidebar { display: block; }",
    ".card { display: block; padding: 16px; background-color: #f5f5f5; margin: 0 0 16px 0; }",
    "footer { display: block; background-color: #222; color: #fff; padding: 16px; }",
);

const ITERATIONS: u32 = 1000;
const VIEWPORT: Rect = Rect { x: 0.0, y: 0.0, width: 1280.0, height: 800.0 };

fn main() {
    // Pre-parse HTML and CSS once -- both paths operate on the same DOM snapshot.
    // This isolates cascade/layout/paint performance from parsing performance.
    let template_doc = parse_html(BENCH_HTML).expect("parse html");
    let stylesheet = template_doc
        .dom
        .with_interner_mut(|interner| parse_stylesheet_with_interner(BENCH_CSS, interner))
        .expect("parse css");

    println!("=== SilkSurf Pipeline Benchmark ({ITERATIONS} iterations) ===");
    println!("Page: {} DOM nodes, {} CSS rules", count_hint(), stylesheet.rules.len());
    println!();

    // ---- HTML PARSE cost (needed for full cached-re-render budget) ----
    // WHY: The speculative pre-render scenario fetches from HTTP cache (0ms)
    // but still re-parses the HTML body each render. This measures the parse
    // cost so we know whether DOM caching is needed to hit the <500us target.
    let mut parse_total = Duration::ZERO;
    for _ in 0..ITERATIONS {
        let html_bytes = BENCH_HTML.as_bytes();
        let t = Instant::now();
        let _doc = parse_html(std::str::from_utf8(html_bytes).unwrap()).expect("parse html");
        parse_total += t.elapsed();
    }
    let parse_per = parse_total / ITERATIONS;
    println!("--- HTML parse cost (input to cached re-render) ---");
    println!("  html parse:   {:>8?}  per-iter  ({} bytes)", parse_per, BENCH_HTML.len());
    println!();

    // ---- OLD PATH: 3-pass (compute_styles + build_layout_tree + build_display_list) ----
    let mut style_total = Duration::ZERO;
    let mut layout_total = Duration::ZERO;
    let mut display_total = Duration::ZERO;
    let mut old_items = 0usize;

    let mut arena = SilkArena::new();
    for _ in 0..ITERATIONS {
        let doc = parse_html(BENCH_HTML).expect("parse html");

        let t = Instant::now();
        let styles = compute_styles(&doc.dom, doc.document, &stylesheet);
        style_total += t.elapsed();

        let t = Instant::now();
        let layout: LayoutTree<'_> =
            build_layout_tree(&arena, &doc.dom, &styles, doc.document, VIEWPORT)
                .expect("layout");
        layout_total += t.elapsed();

        let t = Instant::now();
        let dl = build_display_list(&doc.dom, &styles, &layout)
            .with_tiles(1280, 800, 64);
        display_total += t.elapsed();
        old_items = dl.items.len();
        arena.reset();
    }
    let old_total = style_total + layout_total + display_total;
    let old_per = old_total / ITERATIONS;

    println!("--- 3-pass pipeline ---");
    println!("  cascade:      {:>8?}  per-iter", style_total / ITERATIONS);
    println!("  layout:       {:>8?}  per-iter", layout_total / ITERATIONS);
    println!("  display list: {:>8?}  per-iter", display_total / ITERATIONS);
    println!("  TOTAL:        {:>8?}  per-iter  ({} display items)", old_per, old_items);
    println!();

    // ---- NEW PATH: fused single BFS pass + Rayon rasterize (fresh buffer each iter) ----
    let mut fused_total = Duration::ZERO;
    let mut raster_total = Duration::ZERO;
    let mut fused_items = 0usize;

    for _ in 0..ITERATIONS {
        let doc = parse_html(BENCH_HTML).expect("parse html");

        let t = Instant::now();
        let result = fused_style_layout_paint(&doc.dom, &stylesheet, doc.document, VIEWPORT);
        fused_total += t.elapsed();

        let dl = silksurf_render::DisplayList {
            items: result.display_items,
            tiles: None,
        }
        .with_tiles(1280, 800, 64);

        let t = Instant::now();
        let _buf = rasterize_parallel(&dl, 1280, 800, 64);
        raster_total += t.elapsed();

        fused_items = dl.items.len();
    }
    let fused_per = fused_total / ITERATIONS;
    let raster_per = raster_total / ITERATIONS;

    println!("--- fused pipeline (style+layout+paint in 1 BFS pass) ---");
    println!("  fused pass:   {:>8?}  per-iter", fused_per);
    println!("  rasterize:    {:>8?}  per-iter  (fresh alloc each frame)", raster_per);
    println!("  TOTAL:        {:>8?}  per-iter  ({} display items)", fused_per + raster_per, fused_items);
    println!();

    // ---- Speedup comparison ----
    let speedup = old_per.as_nanos() as f64 / fused_per.as_nanos() as f64;
    println!("=== Speedup (fused pass vs 3-pass cascade+layout+display) ===");
    println!("  {:.2}x  ({:?} -> {:?} per iter)", speedup, old_per, fused_per);
    println!();

    // ---- REUSE PATH: rasterize_parallel_into with pre-allocated buffer ----
    // WHY: In an interactive browser, the raster buffer persists across frames.
    // This measures the amortized cost of rasterization after the first frame,
    // which is the relevant number for cached re-render latency.
    let mut raster_reuse_total = Duration::ZERO;
    let mut reuse_buf: Vec<u8> = Vec::new();
    // Pre-warm: first iter allocates the buffer, subsequent iters reuse it.
    for i in 0..ITERATIONS {
        let doc = parse_html(BENCH_HTML).expect("parse html");
        let result = fused_style_layout_paint(&doc.dom, &stylesheet, doc.document, VIEWPORT);
        let dl = silksurf_render::DisplayList {
            items: result.display_items,
            tiles: None,
        }
        .with_tiles(1280, 800, 64);

        let t = Instant::now();
        rasterize_parallel_into(&dl, 1280, 800, 64, &mut reuse_buf);
        let elapsed = t.elapsed();
        // Skip iter 0 (cold allocation) to measure steady-state cost
        if i > 0 {
            raster_reuse_total += elapsed;
        }
    }
    let raster_reuse_per = raster_reuse_total / (ITERATIONS - 1);

    println!("=== Buffer reuse (steady-state, pre-allocated 4MB buffer) ===");
    println!("  rasterize:    {:>8?}  per-iter  (buffer reused, zero alloc)", raster_reuse_per);
    println!("  fused+raster: {:>8?}  per-iter  (target: <500us cached re-render)", fused_per + raster_reuse_per);
    let alloc_overhead = raster_per.saturating_sub(raster_reuse_per);
    println!("  alloc savings:{:>8?}  per-frame  (eliminated by buffer reuse)", alloc_overhead);
}

/// Approximate node count for display (parse once outside any timing section).
fn count_hint() -> &'static str { "~50" }
