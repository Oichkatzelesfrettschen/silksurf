use silksurf_core::SilkArena;
use silksurf_css::{Color, compute_styles, parse_stylesheet};
use silksurf_dom::{Dom, NodeKind};
use silksurf_layout::{Rect, build_layout_tree};
use silksurf_render::{DisplayItem, DisplayList, build_display_list};

#[test]
fn builds_display_list_for_backgrounds() {
    let stylesheet = parse_stylesheet("div { display: block; background-color: red; }").unwrap();

    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let body = dom.create_element("body");
    dom.append_child(html, body).unwrap();
    let div = dom.create_element("div");
    dom.append_child(body, div).unwrap();

    let styles = compute_styles(&dom, doc, &stylesheet);
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    };
    let arena = SilkArena::new();
    let layout = build_layout_tree(&arena, &dom, &styles, doc, viewport).expect("layout tree");
    let display_list = build_display_list(&dom, &styles, &layout);

    let expected = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
    assert!(display_list.items.iter().any(|item| matches!(
        item,
        DisplayItem::SolidColor { color, .. } if *color == expected
    )));
}

#[test]
fn builds_text_display_items() {
    let stylesheet = parse_stylesheet("body { color: blue; }").unwrap();

    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let body = dom.create_element("body");
    dom.append_child(html, body).unwrap();
    let text = dom.create_text("Hello");
    dom.append_child(body, text).unwrap();

    let styles = compute_styles(&dom, doc, &stylesheet);
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 100.0,
    };
    let arena = SilkArena::new();
    let layout = build_layout_tree(&arena, &dom, &styles, doc, viewport).expect("layout tree");
    let display_list = build_display_list(&dom, &styles, &layout);

    let expected = Color {
        r: 0,
        g: 0,
        b: 255,
        a: 255,
    };
    assert!(display_list.items.iter().any(|item| matches!(
        item,
        DisplayItem::Text { node, color, .. }
            if *color == expected
                && matches!(
                    dom.node(*node).ok().map(silksurf_dom::Node::kind),
                    Some(NodeKind::Text { text }) if text == "Hello"
                )
    )));
}

#[test]
fn rasterizes_solid_color_rect() {
    use silksurf_render::rasterize;

    let list = silksurf_render::DisplayList {
        items: vec![DisplayItem::SolidColor {
            rect: Rect {
                x: 1.0,
                y: 1.0,
                width: 2.0,
                height: 2.0,
            },
            color: Color {
                r: 10,
                g: 20,
                b: 30,
                a: 255,
            },
        }],
        tiles: None,
    };
    let buffer = rasterize(&list, 4, 4);
    let idx = (5 * 4) as usize;
    assert_eq!(buffer[idx], 10);
    assert_eq!(buffer[idx + 1], 20);
    assert_eq!(buffer[idx + 2], 30);
    assert_eq!(buffer[idx + 3], 255);
}

#[test]
fn rasterizes_damage_region() {
    use silksurf_render::rasterize_damage;

    let list = DisplayList {
        items: vec![
            DisplayItem::SolidColor {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 2.0,
                    height: 2.0,
                },
                color: Color {
                    r: 200,
                    g: 10,
                    b: 10,
                    a: 255,
                },
            },
            DisplayItem::SolidColor {
                rect: Rect {
                    x: 2.0,
                    y: 2.0,
                    width: 2.0,
                    height: 2.0,
                },
                color: Color {
                    r: 10,
                    g: 200,
                    b: 10,
                    a: 255,
                },
            },
        ],
        tiles: None,
    };
    let damage = Rect {
        x: 0.0,
        y: 0.0,
        width: 2.0,
        height: 2.0,
    };
    let buffer = rasterize_damage(&list, 4, 4, damage);
    let red_idx = 0usize;
    assert_eq!(buffer[red_idx], 200);
    assert_eq!(buffer[red_idx + 1], 10);
    assert_eq!(buffer[red_idx + 2], 10);
    assert_eq!(buffer[red_idx + 3], 255);

    let untouched_idx = ((3 * 4 + 3) * 4) as usize;
    assert_eq!(buffer[untouched_idx], 255);
    assert_eq!(buffer[untouched_idx + 1], 255);
    assert_eq!(buffer[untouched_idx + 2], 255);
    assert_eq!(buffer[untouched_idx + 3], 255);
}

#[test]
fn rasterizes_damage_region_with_tiles() {
    use silksurf_render::rasterize_damage;

    let list = DisplayList {
        items: vec![
            DisplayItem::SolidColor {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 2.0,
                    height: 2.0,
                },
                color: Color {
                    r: 50,
                    g: 60,
                    b: 70,
                    a: 255,
                },
            },
            DisplayItem::SolidColor {
                rect: Rect {
                    x: 2.0,
                    y: 2.0,
                    width: 2.0,
                    height: 2.0,
                },
                color: Color {
                    r: 10,
                    g: 20,
                    b: 200,
                    a: 255,
                },
            },
        ],
        tiles: None,
    }
    .with_tiles(4, 4, 2);
    let damage = Rect {
        x: 0.0,
        y: 0.0,
        width: 2.0,
        height: 2.0,
    };
    let buffer = rasterize_damage(&list, 4, 4, damage);
    let red_idx = 0usize;
    assert_eq!(buffer[red_idx], 50);
    assert_eq!(buffer[red_idx + 1], 60);
    assert_eq!(buffer[red_idx + 2], 70);
    assert_eq!(buffer[red_idx + 3], 255);

    let untouched_idx = ((3 * 4 + 3) * 4) as usize;
    assert_eq!(buffer[untouched_idx], 255);
    assert_eq!(buffer[untouched_idx + 1], 255);
    assert_eq!(buffer[untouched_idx + 2], 255);
    assert_eq!(buffer[untouched_idx + 3], 255);
}
