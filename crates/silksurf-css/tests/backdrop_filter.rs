use silksurf_css::{FilterFunction, compute_styles, parse_stylesheet};
use silksurf_dom::Dom;

/// Computes the style of a single `div` carrying `declarations`.
fn div_filter(declarations: &str) -> Vec<FilterFunction> {
    let sheet = parse_stylesheet(&format!("div {{ {declarations} }}")).unwrap();
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let div = dom.create_element("div");
    dom.append_child(html, div).unwrap();
    let styles = compute_styles(&dom, doc, &sheet);
    styles
        .get(&div)
        .expect("div style")
        .backdrop_filter
        .to_vec()
}

/// The value every backdrop-filter declaration in the captured corpus carries,
/// in the order the pipeline applies it.
#[test]
fn the_corpus_declaration_parses_as_a_blur_then_a_saturate() {
    assert_eq!(
        div_filter("backdrop-filter: blur(25px) saturate(1.12);"),
        vec![FilterFunction::Blur(25.0), FilterFunction::Saturate(1.12)]
    );
}

/// Five of the six declarations in the corpus reach the value through the
/// `-webkit-` name alone, so the alias carries the property rather than
/// decorating it.
#[test]
fn the_webkit_alias_reaches_the_same_property() {
    assert_eq!(
        div_filter("-webkit-backdrop-filter: blur(25px) saturate(1.12);"),
        vec![FilterFunction::Blur(25.0), FilterFunction::Saturate(1.12)]
    );
}

#[test]
fn none_and_an_absent_declaration_both_resolve_to_an_empty_pipeline() {
    assert!(div_filter("backdrop-filter: none;").is_empty());
    assert!(div_filter("color: red;").is_empty());
}

/// A percentage resolves against 1.0, so both spellings of one amount agree.
#[test]
fn a_percentage_argument_matches_its_number_form() {
    assert_eq!(
        div_filter("backdrop-filter: saturate(50%);"),
        div_filter("backdrop-filter: saturate(0.5);")
    );
    assert_eq!(
        div_filter("backdrop-filter: saturate(50%);"),
        vec![FilterFunction::Saturate(0.5)]
    );
}

/// An unrecognized function rejects the whole declaration, so the cascade
/// keeps the inherited-or-initial value rather than painting the prefix of a
/// pipeline the author did not ask for.
#[test]
fn an_unsupported_function_rejects_the_whole_declaration() {
    assert!(div_filter("backdrop-filter: blur(4px) hue-rotate(90deg);").is_empty());
}

/// An omitted argument takes the function's own identity value.
#[test]
fn an_empty_argument_list_takes_the_function_initial_value() {
    assert_eq!(
        div_filter("backdrop-filter: blur() saturate();"),
        vec![FilterFunction::Blur(0.0), FilterFunction::Saturate(1.0)]
    );
}
