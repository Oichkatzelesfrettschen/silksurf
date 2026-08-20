//! The `transform` property's retained function list.

use silksurf_css::{Length, TransformFunction, compute_style_for_node, parse_stylesheet};
use silksurf_dom::Dom;

fn functions(value: &str) -> Vec<TransformFunction> {
    let mut dom = Dom::new();
    let document = dom.create_document();
    let html = dom.create_element("html");
    let target = dom.create_element("div");
    dom.set_attribute(target, "id", "t").expect("id attaches");
    dom.append_child(document, html).expect("html attaches");
    dom.append_child(html, target).expect("div attaches");
    dom.materialize_resolve_table();
    let sheet = parse_stylesheet(&format!("#t {{ font-size: 16px; transform: {value} }}"))
        .expect("fixture parses");
    let style = compute_style_for_node(&dom, target, &sheet, None);
    style.transform.functions().to_vec()
}

#[test]
fn a_percentage_translation_stays_a_percentage() {
    assert_eq!(
        functions("translate(-50%)"),
        vec![TransformFunction::Translate {
            x: Length::Percent(-50.0),
            y: Length::Px(0.0),
        }],
        "the basis is the element's own border box, which paint supplies"
    );
}

#[test]
fn a_rem_translation_resolves_against_the_root_font_size() {
    assert_eq!(
        functions("translate(-50%, .25rem)"),
        vec![TransformFunction::Translate {
            x: Length::Percent(-50.0),
            y: Length::Px(4.0),
        }]
    );
}

#[test]
fn a_one_argument_scale_is_uniform() {
    assert_eq!(
        functions("scale(.82)"),
        vec![TransformFunction::Scale { x: 0.82, y: 0.82 }]
    );
}

#[test]
fn a_zero_scale_is_distinct_from_none() {
    assert_eq!(
        functions("scale(0)"),
        vec![TransformFunction::Scale { x: 0.0, y: 0.0 }]
    );
    assert!(
        functions("none").is_empty(),
        "transform: none names no function"
    );
}

#[test]
fn a_percentage_scale_is_a_factor() {
    assert_eq!(
        functions("scale(100%)"),
        vec![TransformFunction::Scale { x: 1.0, y: 1.0 }]
    );
}

#[test]
fn a_list_keeps_the_order_the_author_wrote() {
    assert_eq!(
        functions("scale(75%) rotate(-90deg)"),
        vec![
            TransformFunction::Scale { x: 0.75, y: 0.75 },
            TransformFunction::Rotate { degrees: -90.0 },
        ]
    );
}

#[test]
fn every_angle_unit_reaches_degrees() {
    assert_eq!(
        functions("rotate(.25turn)"),
        vec![TransformFunction::Rotate { degrees: 90.0 }]
    );
    assert_eq!(
        functions("rotate(200grad)"),
        vec![TransformFunction::Rotate { degrees: 180.0 }]
    );
    assert_eq!(
        functions("rotate(0)"),
        vec![TransformFunction::Rotate { degrees: 0.0 }],
        "CSS Values 4 makes a bare zero an angle"
    );
}

#[test]
fn a_matrix_keeps_all_six_terms() {
    assert_eq!(
        functions("matrix(1, 0, 0, 1, 10, 20)"),
        vec![TransformFunction::Matrix {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 10.0,
            f: 20.0,
        }],
        "the translation terms a matrix carries were dropped before"
    );
}

#[test]
fn a_skew_reads_both_axes() {
    assert_eq!(
        functions("skew(10deg, 20deg)"),
        vec![TransformFunction::Skew {
            x_degrees: 10.0,
            y_degrees: 20.0,
        }]
    );
    assert_eq!(
        functions("skewY(20deg)"),
        vec![TransformFunction::Skew {
            x_degrees: 0.0,
            y_degrees: 20.0,
        }]
    );
}

#[test]
fn an_unreadable_function_leaves_the_element_where_layout_placed_it() {
    assert!(functions("perspective(500px)").is_empty());
}
