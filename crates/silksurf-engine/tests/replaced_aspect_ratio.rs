use silksurf_css::{StyleIndex, parse_stylesheet};
use silksurf_dom::{Dom, NodeId};
use silksurf_engine::fused_pipeline::{
    FusedWorkspace, ReplacedSize, fused_style_layout_paint_with_replaced_sizes,
};
use silksurf_engine::parse_html;
use silksurf_layout::Rect;

fn find_svg(dom: &Dom, node: NodeId) -> Option<NodeId> {
    if dom.element_name(node).ok().flatten() == Some("svg") {
        return Some(node);
    }
    dom.children(node)
        .ok()?
        .iter()
        .find_map(|&child| find_svg(dom, child))
}

fn find_image(dom: &Dom, node: NodeId) -> Option<NodeId> {
    if dom.element_name(node).ok().flatten() == Some("img") {
        return Some(node);
    }
    dom.children(node)
        .ok()?
        .iter()
        .find_map(|&child| find_image(dom, child))
}

#[test]
fn svg_percent_width_uses_intrinsic_ratio_for_auto_height() {
    let parsed = parse_html(
        "<html><body><div><svg viewBox='0 0 357 62'><path d='M0 0'/></svg></div></body></html>",
    )
    .expect("fixture parses");
    let stylesheet = parse_stylesheet(
        "body { margin: 0 } div { width: 110px } svg { width: 100%; height: auto }",
    )
    .expect("stylesheet parses");
    let svg = find_svg(&parsed.dom, parsed.document).expect("svg exists");
    let fused = fused_style_layout_paint_with_replaced_sizes(
        &parsed.dom,
        &stylesheet,
        parsed.document,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 300.0,
        },
        &[ReplacedSize {
            node: svg,
            width: 357.0,
            height: 62.0,
        }],
    );
    let rect = fused.node_rects[fused.table.node_to_bfs_idx[&svg] as usize];
    assert!((rect.width - 110.0).abs() < 0.1, "width: {}", rect.width);
    assert!(
        (rect.height - 110.0 * 62.0 / 357.0).abs() < 0.1,
        "height: {}",
        rect.height
    );
}

#[test]
fn changed_intrinsic_ratio_rebuilds_cached_layout() {
    let parsed = parse_html("<html><body><svg viewBox='0 0 357 62'></svg></body></html>")
        .expect("fixture parses");
    let stylesheet = parse_stylesheet("body { margin: 0 } svg { width: 110px; height: auto }")
        .expect("stylesheet parses");
    let index = StyleIndex::new(&stylesheet);
    let svg = find_svg(&parsed.dom, parsed.document).expect("svg exists");
    let mut workspace = FusedWorkspace::default();
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 300.0,
    };
    let first_size = ReplacedSize {
        node: svg,
        width: 357.0,
        height: 62.0,
    };
    workspace.run_with_replaced_sizes(
        &parsed.dom,
        &stylesheet,
        &index,
        parsed.document,
        viewport,
        &[first_size],
    );
    let svg_index = workspace.table().node_to_bfs_idx[&svg] as usize;
    let first_height = workspace.node_rects[svg_index].height;
    workspace.run_with_replaced_sizes(
        &parsed.dom,
        &stylesheet,
        &index,
        parsed.document,
        viewport,
        &[ReplacedSize {
            height: 124.0,
            ..first_size
        }],
    );
    assert!((first_height - 110.0 * 62.0 / 357.0).abs() < 0.1);
    assert!((workspace.node_rects[svg_index].height - first_height * 2.0).abs() < 0.1);
}

#[test]
fn image_explicit_height_uses_intrinsic_ratio_for_auto_width() {
    let parsed =
        parse_html("<html><body><img src='portrait.png'></body></html>").expect("fixture parses");
    let stylesheet = parse_stylesheet("body { margin: 0 } img { width: auto; height: 40px }")
        .expect("stylesheet parses");
    let image = find_image(&parsed.dom, parsed.document).expect("image exists");
    let fused = fused_style_layout_paint_with_replaced_sizes(
        &parsed.dom,
        &stylesheet,
        parsed.document,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 300.0,
        },
        &[ReplacedSize {
            node: image,
            width: 100.0,
            height: 200.0,
        }],
    );
    let rect = fused.node_rects[fused.table.node_to_bfs_idx[&image] as usize];
    assert!((rect.width - 20.0).abs() < 0.1, "width: {}", rect.width);
    assert!((rect.height - 40.0).abs() < 0.1, "height: {}", rect.height);
}
