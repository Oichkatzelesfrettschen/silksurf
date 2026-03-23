use silksurf_core::SilkArena;
use silksurf_css::{Color, Length, parse_stylesheet_with_interner};
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
    if dom.element_name(node).ok().flatten().is_some() {
        if let Ok(attrs) = dom.attributes(node) {
            if attrs
                .iter()
                .any(|attr| attr.name == AttributeName::Id && attr.value.as_str() == id)
            {
                return Some(node);
            }
        }
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

    assert_eq!(main_style.margin.top, Length::Px(12.0));
    assert!(find_layout_box(&output.layout.root, gone).is_none());
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
