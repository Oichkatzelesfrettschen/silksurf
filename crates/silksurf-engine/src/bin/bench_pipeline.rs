use silksurf_core::SilkArena;
use silksurf_css::{compute_styles, parse_stylesheet_with_interner};
use silksurf_engine::parse_html;
use silksurf_layout::Rect;
use silksurf_layout::{build_layout_tree, LayoutTree};
use silksurf_render::build_display_list;
use std::time::Duration;
use std::time::Instant;

fn main() {
    let html = "<!doctype html><html><body><div class='box'>Hello</div></body></html>";
    let css = "div { display: block; margin: 8px; padding: 4px; background-color: #ff0000; }";
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    };

    let iterations = 200;
    let mut arena = SilkArena::new();
    let mut parse_total = Duration::from_secs(0);
    let mut css_total = Duration::from_secs(0);
    let mut style_total = Duration::from_secs(0);
    let mut layout_total = Duration::from_secs(0);
    let mut render_total = Duration::from_secs(0);
    let mut last_items = 0usize;
    for _ in 0..iterations {
        let start = Instant::now();
        let document = parse_html(html).expect("parse html");
        parse_total += start.elapsed();

        let start = Instant::now();
        let stylesheet = document
            .dom
            .with_interner_mut(|interner| parse_stylesheet_with_interner(css, interner))
            .expect("parse css");
        css_total += start.elapsed();

        let start = Instant::now();
        let styles = compute_styles(&document.dom, document.document, &stylesheet);
        style_total += start.elapsed();

        let start = Instant::now();
        let layout: LayoutTree<'_> =
            build_layout_tree(&arena, &document.dom, &styles, document.document, viewport)
                .expect("layout");
        layout_total += start.elapsed();

        let start = Instant::now();
        let width = viewport.width.max(0.0).ceil() as u32;
        let height = viewport.height.max(0.0).ceil() as u32;
        let display_list = build_display_list(&document.dom, &styles, &layout)
            .with_tiles(width, height, 64);
        render_total += start.elapsed();
        last_items = display_list.items.len();
        arena.reset();
    }
    let elapsed = parse_total + css_total + style_total + layout_total + render_total;
    let per_iter = elapsed / iterations as u32;
    println!("parse total: {:?}, per-iter: {:?}", parse_total, parse_total / iterations as u32);
    println!("css total: {:?}, per-iter: {:?}", css_total, css_total / iterations as u32);
    println!("style total: {:?}, per-iter: {:?}", style_total, style_total / iterations as u32);
    println!(
        "layout total: {:?}, per-iter: {:?}",
        layout_total,
        layout_total / iterations as u32
    );
    println!(
        "render total: {:?}, per-iter: {:?}",
        render_total,
        render_total / iterations as u32
    );
    println!("pipeline iterations: {}", iterations);
    println!("total: {:?}, per-iter: {:?}", elapsed, per_iter);
    println!("display list items: {}", last_items);
}
