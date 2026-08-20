use silksurf_core::SilkArena;
use silksurf_css::{Color, Length, LengthOrAuto, parse_stylesheet_with_interner};
use silksurf_dom::{AttributeName, Dom, NodeId};
use silksurf_engine::{EnginePipeline, parse_html, render};
use silksurf_layout::{LayoutBox, Rect};

fn find_layout_box<'a>(layout: &'a LayoutBox<'a>, target: NodeId) -> Option<&'a LayoutBox<'a>> {
    if matches!(
        layout.box_type,
        silksurf_layout::BoxType::BlockNode(id) | silksurf_layout::BoxType::InlineNode(id)
            if id == target
    ) {
        return Some(layout);
    }
    for child in &layout.children {
        if let Some(found) = find_layout_box(child, target) {
            return Some(found);
        }
    }
    None
}

fn find_element_by_id(dom: &Dom, node: NodeId, id: &str) -> Option<NodeId> {
    if dom.element_name(node).ok().flatten().is_some()
        && let Ok(attrs) = dom.attributes(node)
        && attrs
            .iter()
            .any(|attr| attr.name == AttributeName::Id && attr.value.as_str() == id)
    {
        return Some(node);
    }
    let children = dom.children(node).ok()?;
    for child in children {
        if let Some(found) = find_element_by_id(dom, *child, id) {
            return Some(found);
        }
    }
    None
}

#[test]
fn renders_basic_pipeline() {
    let html = "<!doctype html><html><body><div>Hi</div></body></html>";
    let css = "div { display: block; background-color: red; }";
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    };

    let arena = SilkArena::new();
    let output = render(html, css, viewport, &arena).expect("render output");
    assert!(!output.display_list.items.is_empty());
}

#[test]
fn applies_styles_and_skips_display_none() {
    let html = "<html><body><div id='main'>Hi</div><span id='gone'>bye</span></body></html>";
    let css = "#main { margin: 12px; } #gone { display: none; }";
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    };

    let arena = SilkArena::new();
    let output = render(html, css, viewport, &arena).expect("render output");
    let main = find_element_by_id(&output.dom, output.document, "main").expect("main node");
    let gone = find_element_by_id(&output.dom, output.document, "gone").expect("gone node");
    let main_style = output.styles.get(&main).expect("main style");

    assert_eq!(
        main_style.margin.top,
        LengthOrAuto::Length(Length::Px(12.0))
    );
    assert!(find_layout_box(output.layout.root, gone).is_none());
}

#[test]
fn renders_incremental_after_dom_mutation() {
    let html = "<html><body><div id='main'>Hi</div></body></html>";
    let css = "#main { color: red; } #main.hot { color: blue; }";
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    };

    let mut pipeline = EnginePipeline::new();
    let arena = SilkArena::new();
    let document = parse_html(html).expect("parse html");
    let stylesheet = document
        .dom
        .with_interner_mut(|interner| parse_stylesheet_with_interner(css, interner))
        .expect("parse css");
    let output = pipeline
        .render_document(document, stylesheet.clone(), viewport, &arena)
        .expect("render output");

    let mut dom = output.dom;
    let document = output.document;
    let main = find_element_by_id(&dom, document, "main").expect("main node");
    let main_style = output.styles.get(&main).expect("main style");
    assert_eq!(
        main_style.color,
        Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255
        }
    );

    dom.with_mutation_batch(|dom| {
        dom.set_attribute(main, "class", "hot").expect("set class");
    });
    let output = pipeline
        .render_document_incremental_from_dom(dom, document, stylesheet, viewport, &arena)
        .expect("render incremental");
    let main_style = output.styles.get(&main).expect("main style");
    assert_eq!(
        main_style.color,
        Color {
            r: 0,
            g: 0,
            b: 255,
            a: 255
        }
    );
}

/// A `position` change reaches the retained taffy tree.
///
/// `FusedWorkspace` rebuilds that tree when the DOM's structure or style
/// generation moves, and the containing block each absolute box resolves
/// against is recorded during that rebuild. A guard that watched structure
/// alone would leave a box resolving against the block it had before a script
/// positioned its ancestor.
#[test]
fn positioning_an_ancestor_moves_its_absolute_descendant_on_the_retained_path() {
    use silksurf_css::{StyleIndex, parse_stylesheet};
    use silksurf_engine::fused_pipeline::FusedWorkspace;

    const HTML: &str = concat!(
        "<!DOCTYPE html><html><body>",
        "<div id=\"host\" style=\"margin-left:60px;width:300px;height:200px\">",
        "<div id=\"mid\" style=\"margin-left:15px\">",
        "<div id=\"probe\" style=\"position:absolute;left:20px;width:40px;height:40px\">",
        "</div></div></div></body></html>"
    );

    let mut parsed = parse_html(HTML).expect("the document parses");
    let stylesheet = parse_stylesheet("body { margin: 0; }").expect("the stylesheet parses");
    let style_index = StyleIndex::new(&stylesheet);
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 800.0,
    };
    let mut workspace = FusedWorkspace::new();

    let probe_x = |workspace: &FusedWorkspace, dom: &Dom| {
        let probe = find_element_by_id(dom, NodeId::from_raw(0), "probe").expect("#probe exists");
        let idx = *workspace
            .table()
            .node_to_bfs_idx
            .get(&probe)
            .expect("#probe reached layout") as usize;
        workspace.snapshot_result().node_rects[idx].x
    };

    // No ancestor is positioned, so the initial containing block at the
    // viewport origin supplies the 20 px offset.
    workspace.run(
        &parsed.dom,
        &stylesheet,
        &style_index,
        parsed.document,
        viewport,
    );
    assert!((probe_x(&workspace, &parsed.dom) - 20.0).abs() < 0.01);

    let host = find_element_by_id(&parsed.dom, parsed.document, "host").expect("#host exists");
    parsed
        .dom
        .set_attribute(
            host,
            "style",
            "margin-left:60px;width:300px;height:200px;position:relative",
        )
        .expect("#host takes a style attribute");

    // `#host` now supplies the containing block, so its own x joins the offset.
    workspace.run(
        &parsed.dom,
        &stylesheet,
        &style_index,
        parsed.document,
        viewport,
    );
    let moved = probe_x(&workspace, &parsed.dom);
    assert!(
        (moved - 80.0).abs() < 0.01,
        "expected 80.0 once #host is positioned, got {moved}"
    );
}
