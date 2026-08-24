use silksurf_css::{StyleIndex, parse_stylesheet};

/// Builds an index over one sheet at an 800x600 viewport.
fn index(css: &str) -> StyleIndex {
    let sheet = parse_stylesheet(css).unwrap();
    StyleIndex::for_viewport_sheets([&sheet], 800.0, 600.0)
}

/// A keyframe selector is its own grammar, so a percentage offset survives
/// where the selector parser would have dropped it: `50%` reaches the
/// selector parser as an empty list and `0%, to` keeps only the `to`.
#[test]
fn percentage_offsets_survive_the_keyframe_grammar() {
    let index = index("@keyframes spin { 0% { opacity: 0 } 50% { opacity: 1 } to { opacity: 0 } }");
    let rule = index.keyframes("spin").expect("spin");
    let offsets: Vec<f32> = rule.stops.iter().map(|(offset, _)| *offset).collect();
    assert_eq!(offsets, vec![0.0, 0.5, 1.0]);
}

/// `from` and `to` are the spelled forms of the two endpoints.
#[test]
fn from_and_to_are_the_endpoints() {
    let index = index("@keyframes fade { from { opacity: 0 } to { opacity: 1 } }");
    let rule = index.keyframes("fade").expect("fade");
    let offsets: Vec<f32> = rule.stops.iter().map(|(offset, _)| *offset).collect();
    assert_eq!(offsets, vec![0.0, 1.0]);
}

/// A selector list spreads one block's declarations across every offset it
/// names. The corpus declares `0%, to { opacity: .68; transform: scale(.82) }`.
#[test]
fn a_selector_list_spreads_one_block_across_its_offsets() {
    let index = index("@keyframes pulse { 0%, to { opacity: .68 } 50% { opacity: 1 } }");
    let rule = index.keyframes("pulse").expect("pulse");
    let offsets: Vec<f32> = rule.stops.iter().map(|(offset, _)| *offset).collect();
    assert_eq!(offsets, vec![0.0, 0.5, 1.0]);
    // The two ends share the block, so they carry identical declarations.
    assert_eq!(rule.stops[0].1, rule.stops[2].1);
    assert_ne!(rule.stops[0].1, rule.stops[1].1);
}

/// A later rule of the same name replaces the earlier one.
#[test]
fn a_later_rule_of_one_name_replaces_the_earlier() {
    let index = index("@keyframes x { from { opacity: 0 } } @keyframes x { 50% { opacity: 1 } }");
    let rule = index.keyframes("x").expect("x");
    assert_eq!(rule.stops.len(), 1);
    assert!((rule.stops[0].0 - 0.5).abs() < f32::EPSILON);
}

/// An offset outside [0%, 100%] makes its selector invalid, so the block it
/// introduces is dropped rather than clamped.
#[test]
fn an_out_of_range_offset_drops_its_block() {
    let index = index("@keyframes x { 150% { opacity: 0 } 25% { opacity: 1 } }");
    let rule = index.keyframes("x").expect("x");
    let offsets: Vec<f32> = rule.stops.iter().map(|(offset, _)| *offset).collect();
    assert_eq!(offsets, vec![0.25]);
}

/// A document declaring no keyframes reports so, and an undeclared name
/// resolves to nothing.
#[test]
fn an_undeclared_name_resolves_to_nothing() {
    let index = index("div { color: red }");
    assert!(!index.declares_keyframes());
    assert!(index.keyframes("spin").is_none());
}
