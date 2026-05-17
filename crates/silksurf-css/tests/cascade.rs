use silksurf_css::{Color, Display, Length, compute_styles, parse_stylesheet};
use silksurf_dom::Dom;

#[test]
fn cascades_and_inherits() {
    let stylesheet = parse_stylesheet(
        "body { color: green; } div { color: red; display: block; margin: 4px 8px; } #main { color: blue; }",
    )
    .unwrap();

    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let body = dom.create_element("body");
    dom.append_child(html, body).unwrap();
    let div = dom.create_element("div");
    dom.set_attribute(div, "id", "main").unwrap();
    dom.append_child(body, div).unwrap();
    let span = dom.create_element("span");
    dom.append_child(body, span).unwrap();

    let styles = compute_styles(&dom, doc, &stylesheet);
    let div_style = styles.get(&div).expect("div style");
    let span_style = styles.get(&span).expect("span style");

    assert_eq!(div_style.display, Display::Block);
    assert_eq!(
        div_style.color,
        Color {
            r: 0,
            g: 0,
            b: 255,
            a: 255
        }
    );
    assert_eq!(
        div_style.margin,
        silksurf_css::Edges {
            top: Length::Px(4.0),
            right: Length::Px(8.0),
            bottom: Length::Px(4.0),
            left: Length::Px(8.0),
        }
    );
    assert_eq!(span_style.display, Display::Inline);
    assert_eq!(
        span_style.color,
        Color {
            r: 0,
            g: 128,
            b: 0,
            a: 255
        }
    );
}

#[test]
fn cascades_line_height_and_border() {
    let stylesheet = parse_stylesheet("p { line-height: 18px; border-width: 2px 4px; }").unwrap();

    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let body = dom.create_element("body");
    dom.append_child(html, body).unwrap();
    let p = dom.create_element("p");
    dom.append_child(body, p).unwrap();

    let styles = compute_styles(&dom, doc, &stylesheet);
    let p_style = styles.get(&p).expect("p style");

    assert_eq!(p_style.line_height, Length::Px(18.0));
    assert_eq!(
        p_style.border,
        silksurf_css::Edges {
            top: Length::Px(2.0),
            right: Length::Px(4.0),
            bottom: Length::Px(2.0),
            left: Length::Px(4.0),
        }
    );
}
