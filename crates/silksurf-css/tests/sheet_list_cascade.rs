//! StyleIndex construction over an ordered list of sheets.

use silksurf_css::{StyleIndex, parse_stylesheet};

#[test]
fn a_sheet_list_cascades_in_list_order() {
    let first = parse_stylesheet("p { color: red }").expect("first sheet parses");
    let second = parse_stylesheet("p { color: blue }").expect("second sheet parses");
    let index = StyleIndex::for_viewport_sheets(&[first.clone(), second.clone()], 1280.0, 800.0);
    let concatenated =
        parse_stylesheet("p { color: red } p { color: blue }").expect("concatenation parses");
    let flat = StyleIndex::for_viewport(&concatenated, 1280.0, 800.0);
    assert_eq!(
        index.active_rules, flat.active_rules,
        "a sheet list flattens to what the same text concatenated produces"
    );
}

#[test]
fn one_layer_name_spans_the_sheet_list() {
    let first = parse_stylesheet("@layer base, theme; @layer theme { p { color: blue } }")
        .expect("first sheet parses");
    let second = parse_stylesheet("@layer base { p { color: red } }").expect("second sheet parses");
    let index = StyleIndex::for_viewport_sheets(&[first, second], 1280.0, 800.0);
    assert_eq!(
        index.active_rules.len(),
        2,
        "both layered rules stay active across the sheet boundary"
    );
}
