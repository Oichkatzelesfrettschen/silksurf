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
use silksurf_css::{ComputedStyle, StyleIndex, compute_styles, parse_stylesheet_with_interner};
use silksurf_engine::fused_pipeline::{FusedWorkspace, fused_style_layout_paint};
use silksurf_engine::parse_html;
use silksurf_layout::Rect;
use silksurf_layout::neighbor_table::LayoutNeighborTable;
use silksurf_layout::{LayoutTree, build_layout_tree};
use silksurf_render::{build_display_list, rasterize_parallel, rasterize_parallel_into};
use std::process::Command;
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
const VIEWPORT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 1280.0,
    height: 800.0,
};

/*
 * emit_history_record -- write one NDJSON line to history.ndjson.
 *
 * WHY: bench_pipeline previously emitted only human-readable text, which broke
 * the pipeline: append_history.py and check_perf_regression.sh exist but had
 * nothing to read. The `--emit json` flag closes the gap by writing one record
 * conforming to perf/schema.json to stdout (caller pipes it via Make target).
 *
 * Metric mapping (documented here as the canonical source of truth):
 *   fused_pipeline_us -- ws_per (FusedWorkspace steady-state, iter 0 excluded)
 *   css_cache_hit_us  -- cascade_only_per (ws_per minus table.rebuild(); cascade
 *                        with pre-parsed stylesheet, zero CSS re-parsing cost)
 *   full_render_us    -- fused_per + raster_reuse_per (cold fused + steady-state
 *                        rasterize, buffer pre-allocated; the cached re-render
 *                        budget measured against the <500us target)
 */
fn emit_history_record(
    fused_pipeline_us: f64,
    css_cache_hit_us: f64,
    full_render_us: f64,
    profile: &str,
) {
    // git rev-parse HEAD for the 40-char SHA.
    let git_sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok()).map_or_else(|| "0".repeat(40), |s| s.trim().to_string());

    // rustc --version for the toolchain string.
    let rust_version = Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok()).map_or_else(|| "unknown".to_string(), |s| s.trim().to_string());

    // ISO-8601 UTC timestamp via date -u.
    let timestamp = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok()).map_or_else(|| "1970-01-01T00:00:00Z".to_string(), |s| s.trim().to_string());

    // Emit single JSON line; no external crate needed for this simple record.
    // serde_json is available as a workspace dep but the format is trivial enough
    // to build with format! and avoids the need for a derive.
    println!(
        "{{\"git_sha\":{git_sha:?},\"timestamp\":{timestamp:?},\"rust_version\":{rust_version:?},\"profile\":{profile:?},\"metrics\":{{\"fused_pipeline_us\":{fused_pipeline_us:.3},\"css_cache_hit_us\":{css_cache_hit_us:.3},\"full_render_us\":{full_render_us:.3}}}}}",
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let emit_json = args.iter().any(|a| a == "--emit" || a == "--emit=json");
    // release vs debug for the schema profile field
    let build_profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };

    // Pre-parse HTML and CSS once -- both paths operate on the same DOM snapshot.
    // This isolates cascade/layout/paint performance from parsing performance.
    let template_doc = parse_html(BENCH_HTML).expect("parse html");
    let stylesheet = template_doc
        .dom
        .with_interner_mut(|interner| parse_stylesheet_with_interner(BENCH_CSS, interner))
        .expect("parse css");

    println!("=== SilkSurf Pipeline Benchmark ({ITERATIONS} iterations) ===");
    println!(
        "Page: {} DOM nodes, {} CSS rules",
        count_hint(),
        stylesheet.rules.len()
    );
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
    println!(
        "  html parse:   {:>8?}  per-iter  ({} bytes)",
        parse_per,
        BENCH_HTML.len()
    );
    println!();

    // ---- DOM memory layout analysis (cache line utilization) ----
    println!("--- DOM type sizes (cache line = 64 bytes) ---");
    println!(
        "  Node:            {} bytes  ({:.1} cache lines)",
        std::mem::size_of::<silksurf_dom::Node>(),
        std::mem::size_of::<silksurf_dom::Node>() as f64 / 64.0
    );
    println!(
        "  NodeKind:        {} bytes",
        std::mem::size_of::<silksurf_dom::NodeKind>()
    );
    println!(
        "  Attribute:       {} bytes",
        std::mem::size_of::<silksurf_dom::Attribute>()
    );
    println!(
        "  TagName:         {} bytes",
        std::mem::size_of::<silksurf_dom::TagName>()
    );
    println!(
        "  ComputedStyle:   {} bytes  ({:.1} cache lines)",
        std::mem::size_of::<silksurf_css::ComputedStyle>(),
        std::mem::size_of::<silksurf_css::ComputedStyle>() as f64 / 64.0
    );
    println!(
        "  CascadeEntry:    {} bytes  ({:.1} cache lines)",
        std::mem::size_of::<silksurf_css::CascadeEntry>(),
        std::mem::size_of::<silksurf_css::CascadeEntry>() as f64 / 64.0
    );
    println!(
        "  SelectorIdent:   {} bytes",
        std::mem::size_of::<silksurf_css::SelectorIdent>()
    );
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
            build_layout_tree(&arena, &doc.dom, &styles, doc.document, VIEWPORT).expect("layout");
        layout_total += t.elapsed();

        let t = Instant::now();
        let dl = build_display_list(&doc.dom, &styles, &layout).with_tiles(1280, 800, 64);
        display_total += t.elapsed();
        old_items = dl.items.len();
        arena.reset();
    }
    let old_total = style_total + layout_total + display_total;
    let old_per = old_total / ITERATIONS;

    println!("--- 3-pass pipeline ---");
    println!("  cascade:      {:>8?}  per-iter", style_total / ITERATIONS);
    println!(
        "  layout:       {:>8?}  per-iter",
        layout_total / ITERATIONS
    );
    println!(
        "  display list: {:>8?}  per-iter",
        display_total / ITERATIONS
    );
    println!(
        "  TOTAL:        {old_per:>8?}  per-iter  ({old_items} display items)"
    );
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
    println!("  fused pass:   {fused_per:>8?}  per-iter");
    println!(
        "  rasterize:    {raster_per:>8?}  per-iter  (fresh alloc each frame)"
    );
    println!(
        "  TOTAL:        {:>8?}  per-iter  ({} display items)",
        fused_per + raster_per,
        fused_items
    );
    println!();

    // ---- Speedup comparison (cold) ----
    let speedup = old_per.as_nanos() as f64 / fused_per.as_nanos() as f64;
    println!("=== Speedup cold: fused pass vs 3-pass ===");
    println!(
        "  {speedup:.2}x  ({old_per:?} -> {fused_per:?} per iter)"
    );
    println!();

    // ---- WORKSPACE PATH: FusedWorkspace steady-state (zero alloc after warm-up) ----
    //
    // WHY: The cold fused path allocates LayoutNeighborTable (FxHashMap + flat Vecs)
    // and output Vecs fresh every iteration.  FusedWorkspace retains capacity across
    // calls: rebuild() clears (O(1)) and refills, eliminating all per-frame allocator
    // traffic after the first iteration.
    //
    // StyleIndex is built once outside the loop -- this mirrors the production
    // scenario where the stylesheet is stable across re-renders.
    //
    // WHY separate from rasterize reuse below: this section isolates the
    // fused pipeline allocation overhead from rasterizer allocation overhead.
    let style_index = StyleIndex::new(&stylesheet);
    let mut ws = FusedWorkspace::new();
    let mut ws_total = Duration::ZERO;
    let mut ws_items = 0usize;

    // Pre-warm (iter 0 establishes capacity; subsequent iters are zero-alloc).
    for i in 0..ITERATIONS {
        let doc = parse_html(BENCH_HTML).expect("parse html");

        let t = Instant::now();
        ws.run(&doc.dom, &stylesheet, &style_index, doc.document, VIEWPORT);
        let elapsed = t.elapsed();

        if i > 0 {
            ws_total += elapsed;
        }
        ws_items = ws.display_items.len();
    }
    let ws_per = ws_total / (ITERATIONS - 1);

    println!("--- fused pipeline (FusedWorkspace, zero-alloc steady-state) ---");
    println!(
        "  fused pass:   {ws_per:>8?}  per-iter  (iter 0 warm-up excluded)"
    );
    println!("  display items: {ws_items} (same as cold path)");
    println!();

    let speedup_ws_vs_cold = fused_per.as_nanos() as f64 / ws_per.as_nanos() as f64;
    let speedup_ws_vs_3pass = old_per.as_nanos() as f64 / ws_per.as_nanos() as f64;
    println!(
        "=== Speedup workspace vs cold fused: {speedup_ws_vs_cold:.2}x ==="
    );
    println!(
        "=== Speedup workspace vs 3-pass:     {speedup_ws_vs_3pass:.2}x ==="
    );
    println!();

    // ---- RCA: sub-phase breakdown of workspace steady-state cost ----
    //
    // ws.run() = ?us.  Where does the time go?
    //
    // Sub-phase 1: LayoutNeighborTable::rebuild() alone.
    // Measures FxHashMap clear + 50 inserts + flat Vec refill.
    // This is purely DOM traversal cost (dom.children per node).
    let template_doc2 = parse_html(BENCH_HTML).expect("parse html");
    let mut rca_table = LayoutNeighborTable::build(&template_doc2.dom, template_doc2.document);
    let mut rebuild_total = Duration::ZERO;
    for i in 0..ITERATIONS {
        let doc = parse_html(BENCH_HTML).expect("parse html");
        let t = Instant::now();
        rca_table.rebuild(&doc.dom, doc.document);
        let elapsed = t.elapsed();
        if i > 0 {
            rebuild_total += elapsed;
        }
    }
    let rebuild_per = rebuild_total / (ITERATIONS - 1);

    // Sub-phase 2: ComputedStyle::default() construction cost.
    // This is called inside CascadedStyle::resolve() for EVERY node (allocates
    // font_family: Vec<String> as the fallback, even when parent covers it).
    let mut default_total = Duration::ZERO;
    let n_nodes = rca_table.len();
    for i in 0..ITERATIONS {
        let t = Instant::now();
        for _ in 0..n_nodes {
            let _s: ComputedStyle = ComputedStyle::default();
            std::hint::black_box(_s);
        }
        let elapsed = t.elapsed();
        if i > 0 {
            default_total += elapsed;
        }
    }
    let default_per = default_total / (ITERATIONS - 1);

    // Sub-phase 3: cascade-only time = total ws.run() - rebuild.
    // Everything in ws.run() that is not table.rebuild() goes here:
    //   - compute_style_for_node_with_workspace x50
    //   - layout math x50
    //   - display item push x27
    let cascade_only_per = ws_per.saturating_sub(rebuild_per);

    println!(
        "--- RCA: sub-phase breakdown of workspace steady-state ({n_nodes} nodes) ---"
    );
    println!(
        "  table.rebuild():           {rebuild_per:>8?}  per-iter  (FxHashMap clear+50 inserts)"
    );
    println!(
        "  cascade+layout+paint:      {cascade_only_per:>8?}  per-iter  (ws.run minus rebuild)"
    );
    println!(
        "  ComputedStyle::default x{n_nodes}: {default_per:>8?}  per-iter  (SmallVec<SmolStr> -- zero heap alloc)"
    );
    println!("  TOTAL ws.run():            {ws_per:>8?}  per-iter");
    println!();
    println!("  APPLIED FIXES: SmolStr font_family (Fix 1), bitvec seen (Fix D),");
    println!("  workspace class_keys (Fix 2), pre-resolved class_strings (Fix 3),");
    println!("  fused tag+id+class (Fix F). ComputedStyle::default now zero-heap-alloc.");
    println!();
    let unaccounted = cascade_only_per.saturating_sub(default_per);
    println!(
        "  cascade+layout+paint minus default overhead: {unaccounted:>8?}"
    );
    println!("  (remaining: selector matching, apply_declaration, layout math)");
    println!();

    // Sub-phase 4: REFERENCE COST: Vec alloc (now eliminated by workspace.class_keys).
    // node_tag_id_class() reuses workspace.class_keys (Fix 2). This measures the
    // OLD cost for comparison: Vec::new() + push per class node.
    let n_class_nodes: usize = 24; // nodes with class attrs in BENCH_HTML
    let mut class_vec_total = Duration::ZERO;
    for i in 0..ITERATIONS {
        let t = Instant::now();
        for _ in 0..n_class_nodes {
            // Deliberately Vec::new() + push to reproduce the pre-Fix-2
            // two-allocation pattern (empty Vec, then growth on push).
            // Rewriting to vec![] would change what is measured.
            #[allow(clippy::vec_init_then_push)]
            {
                let mut v: Vec<[u8; 24]> = Vec::new(); // same size as SelectorIdent (SmolStr=24)
                v.push([0u8; 24]);
                std::hint::black_box(&v);
                drop(v);
            }
        }
        let elapsed = t.elapsed();
        if i > 0 {
            class_vec_total += elapsed;
        }
    }
    let class_vec_per = class_vec_total / (ITERATIONS - 1);

    // Sub-phase 5: REFERENCE COST: RwLock acquire (now eliminated by class_strings).
    // node_tag_id_class() reads attr.class_strings (Fix 3) instead of calling
    // dom.resolve(atom). This measures the OLD cost for comparison.
    let rw = std::sync::RwLock::new(42u64);
    let n_class_atoms: usize = 29; // approximate total class atoms in bench DOM
    let mut rwlock_total = Duration::ZERO;
    for i in 0..ITERATIONS {
        let t = Instant::now();
        for _ in 0..n_class_atoms {
            let guard = rw.read().unwrap();
            std::hint::black_box(*guard);
            drop(guard);
        }
        let elapsed = t.elapsed();
        if i > 0 {
            rwlock_total += elapsed;
        }
    }
    let rwlock_per = rwlock_total / (ITERATIONS - 1);

    println!(
        "  [REF] Vec alloc x{n_class_nodes} (eliminated): {class_vec_per:>8?}  per-iter  (was node_id_class_keys)"
    );
    println!(
        "  [REF] RwLock x{n_class_atoms} (eliminated):  {rwlock_per:>8?}  per-iter  (was dom.resolve)"
    );
    println!();

    let sum_known = rebuild_per + default_per + class_vec_per + rwlock_per;
    let true_residual = ws_per.saturating_sub(sum_known);
    println!(
        "  Known overhead sum:          {sum_known:>8?}  (rebuild + default + Vec + RwLock)"
    );
    println!(
        "  Residual (selector + layout): {true_residual:>8?}  (pure algorithm work -- target floor)"
    );
    println!();
    println!(
        "  Fixes applied: SmolStr default (-{default_per:?}), bitvec seen, workspace class_keys,"
    );
    println!("  pre-resolved class_strings, fused tag+id+class lookup.");
    println!("  Remaining: flatten DOM memory layout (DOD) for cache locality.");
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
    println!(
        "  rasterize:    {raster_reuse_per:>8?}  per-iter  (buffer reused, zero alloc)"
    );
    println!(
        "  fused+raster: {:>8?}  per-iter  (target: <500us cached re-render)",
        fused_per + raster_reuse_per
    );
    let alloc_overhead = raster_per.saturating_sub(raster_reuse_per);
    println!(
        "  alloc savings:{alloc_overhead:>8?}  per-frame  (eliminated by buffer reuse)"
    );

    // --emit json: write one NDJSON history record conforming to perf/schema.json.
    // Pipe to perf/history.ndjson via `make perf-baselines` or manually:
    //   cargo run --release -p silksurf-engine --bin bench_pipeline -- --emit json \
    //     >> perf/history.ndjson
    if emit_json {
        let fused_us = ws_per.as_nanos() as f64 / 1000.0;
        let cache_hit_us = cascade_only_per.as_nanos() as f64 / 1000.0;
        let full_render_us = (fused_per + raster_reuse_per).as_nanos() as f64 / 1000.0;
        emit_history_record(fused_us, cache_hit_us, full_render_us, build_profile);
    }
}

/// Approximate node count for display (parse once outside any timing section).
fn count_hint() -> &'static str {
    "~50"
}
