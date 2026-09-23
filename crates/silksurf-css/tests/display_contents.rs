use silksurf_css::{Display, compute_styles, parse_stylesheet};
use silksurf_dom::Dom;

#[test]
fn contents_keeps_inheritance_and_adjusts_root_and_replaced_elements() {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let html = dom.create_element("html");
    let body = dom.create_element("body");
    let wrapper = dom.create_element("div");
    let child = dom.create_element("span");
    let image = dom.create_element("img");
    let input = dom.create_element("input");
    dom.set_attribute(wrapper, "id", "wrapper")
        .expect("wrapper id attaches");
    dom.append_child(document, html).expect("html attaches");
    dom.append_child(html, body).expect("body attaches");
    dom.append_child(body, wrapper).expect("wrapper attaches");
    dom.append_child(wrapper, child).expect("child attaches");
    dom.append_child(body, image).expect("image attaches");
    dom.append_child(body, input).expect("input attaches");
    let stylesheet = parse_stylesheet(
        "html, #wrapper, img, input { display: contents } \
         #wrapper { color: red }",
    )
    .expect("stylesheet parses");
    let styles = compute_styles(&dom, document, &stylesheet);
    assert_eq!(styles[&html].display, Display::Block);
    assert_eq!(styles[&wrapper].display, Display::Contents);
    assert_eq!(styles[&image].display, Display::None);
    assert_eq!(styles[&input].display, Display::None);
    assert_eq!(styles[&child].color.r, 255);
}
