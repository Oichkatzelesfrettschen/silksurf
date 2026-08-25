use silksurf_css::{StyleIndex, parse_stylesheet};
use silksurf_dom::{AttributeName, Dom, NodeId};
use silksurf_engine::fused_pipeline::FusedWorkspace;
use silksurf_engine::parse_html;
use silksurf_layout::Rect;

const VIEWPORT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 1280.0,
    height: 800.0,
};

/// Finds an element by its id attribute.
fn element_by_id(dom: &Dom, node: NodeId, id: &str) -> Option<NodeId> {
    if dom.element_name(node).ok().flatten().is_some()
        && let Ok(attrs) = dom.attributes(node)
        && attrs
            .iter()
            .any(|attr| attr.name == AttributeName::Id && attr.value.as_str() == id)
    {
        return Some(node);
    }
    for child in dom.children(node).ok()? {
        if let Some(found) = element_by_id(dom, *child, id) {
            return Some(found);
        }
    }
    None
}

/// Runs the pipeline at `seconds` and reports the opacity of `#target` and
/// whether any animation still advances.
fn run_at(html: &str, css: &str, seconds: f32) -> (f32, bool) {
    let parsed = parse_html(html).expect("the document parses");
    let stylesheet = parse_stylesheet(css).expect("the stylesheet parses");
    let style_index = StyleIndex::new(&stylesheet);
    let mut workspace = FusedWorkspace::new();
    workspace.set_timeline_seconds(seconds);
    workspace.run(
        &parsed.dom,
        &stylesheet,
        &style_index,
        NodeId::from_raw(0),
        VIEWPORT,
    );
    let target = element_by_id(&parsed.dom, NodeId::from_raw(0), "target").expect("#target");
    let index = *workspace
        .table()
        .node_to_bfs_idx
        .get(&target)
        .expect("#target reached the cascade") as usize;
    let opacity = workspace.snapshot_result().styles[index]
        .as_ref()
        .expect("#target has a style")
        .opacity;
    (opacity, workspace.animations_advance())
}

const HTML: &str = "<!DOCTYPE html><html><body><div id=\"target\">x</div></body></html>";

/// The pipeline samples a running animation at the timeline it was given, so
/// the same document at two times paints two different opacities.
#[test]
fn the_pipeline_samples_a_running_animation_at_its_timeline() {
    let css = "@keyframes fade { from { opacity: 0 } to { opacity: 1 } } \
               #target { animation: fade 2s linear; }";
    let (start, _) = run_at(HTML, css, 0.0);
    let (middle, _) = run_at(HTML, css, 1.0);
    assert!(start.abs() < 1e-3, "at 0s: {start}");
    assert!((middle - 0.5).abs() < 1e-3, "at 1s: {middle}");
}

/// An animation whose selector matches nothing costs the frame loop nothing.
///
/// This is the gate that keeps the profile's idle cost: the corpus declares
/// two infinite animations, and neither of their selectors matches the
/// captured document.
#[test]
fn an_animation_matching_no_element_never_advances_the_loop() {
    let css = "@keyframes blink { 50% { opacity: 0 } } \
               #absent { animation: blink 1s linear infinite; }";
    let (opacity, advances) = run_at(HTML, css, 0.5);
    assert!(
        (opacity - 1.0).abs() < 1e-3,
        "an unmatched animation moved the element: {opacity}"
    );
    assert!(!advances, "an unmatched animation scheduled a frame");
}

/// A document declaring no animation at all leaves the loop alone.
#[test]
fn a_document_without_animations_never_advances_the_loop() {
    let (_, advances) = run_at(HTML, "#target { color: red }", 5.0);
    assert!(!advances);
}

/// An infinite animation always needs the next frame.
#[test]
fn an_infinite_animation_advances_the_loop() {
    let css = "@keyframes blink { 50% { opacity: 0 } } \
               #target { animation: blink 1s linear infinite; }";
    let (opacity, advances) = run_at(HTML, css, 100.5);
    assert!(
        opacity.abs() < 1e-3,
        "the blink reached its stop: {opacity}"
    );
    assert!(advances, "an infinite animation stopped scheduling frames");
}

/// An animation held at a forwards fill contributes a value and never a
/// different one, so it stops asking for frames while keeping its effect.
#[test]
fn a_filled_animation_holds_its_value_without_advancing_the_loop() {
    let css = "@keyframes fade { from { opacity: 0 } to { opacity: 1 } } \
               #target { animation: fade 1s linear forwards; }";
    let (during, advancing) = run_at(HTML, css, 0.5);
    assert!((during - 0.5).abs() < 1e-3, "midpoint: {during}");
    assert!(advancing, "a running animation stopped scheduling frames");

    let (after, still_advancing) = run_at(HTML, css, 10.0);
    assert!((after - 1.0).abs() < 1e-3, "held at the end: {after}");
    assert!(!still_advancing, "a filled animation kept the loop awake");
}

/// A `both` fill over a 0% keyframe of `opacity: 0` hides the element through
/// its delay. Advancing time is what brings it back, so shipping the fill
/// without the clock would hide an element that renders correctly today.
#[test]
fn a_both_fill_hides_through_the_delay_and_time_restores_it() {
    let css = "@keyframes enter { 0% { opacity: 0 } to { opacity: 1 } } \
               #target { animation: .3s linear 1s both enter; }";
    let (delayed, _) = run_at(HTML, css, 0.2);
    assert!(
        delayed.abs() < 1e-3,
        "the backwards fill applied: {delayed}"
    );
    let (arrived, _) = run_at(HTML, css, 2.0);
    assert!(
        (arrived - 1.0).abs() < 1e-3,
        "time did not restore it: {arrived}"
    );
}
