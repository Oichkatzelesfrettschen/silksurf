use silksurf_css::animation::{
    AnimationDirection, AnimationFillMode, AnimationSpec, TimingFunction,
};
use silksurf_css::animation_sample::{animation_progress, sample_keyframes};
use silksurf_css::{ComputedStyle, StyleIndex, TransformFunction, Visibility, parse_stylesheet};

const VIEWPORT: (f32, f32) = (800.0, 600.0);

/// Samples `name` from `css` at `progress` over a base style.
fn sample(css: &str, name: &str, base: &ComputedStyle, progress: f32) -> ComputedStyle {
    let sheet = parse_stylesheet(css).unwrap();
    let index = StyleIndex::for_viewport_sheets([&sheet], VIEWPORT.0, VIEWPORT.1);
    let rule = index.keyframes(name).expect("rule");
    sample_keyframes(base, rule, progress, 16.0, VIEWPORT)
}

fn spec(duration: f32) -> AnimationSpec {
    AnimationSpec {
        name: "x".into(),
        duration,
        delay: 0.0,
        // A curve that maps progress onto itself keeps these assertions about
        // the timeline rather than about the easing.
        timing: TimingFunction::CubicBezier(0.0, 0.0, 1.0, 1.0),
        iteration_count: 1.0,
        direction: AnimationDirection::Normal,
        fill_mode: AnimationFillMode::None,
    }
}

/// The corpus declares `mobile-empty-composer-action-enter` over two offsets.
/// Opacity runs the whole way and the translation moves with it.
#[test]
fn a_two_stop_rule_interpolates_between_its_own_offsets() {
    let css = "@keyframes enter { 0% { opacity: 0; transform: translate(-50%, 4px) } \
               to { opacity: 1; transform: translate(-50%, 0px) } }";
    let midpoint = sample(css, "enter", &ComputedStyle::default(), 0.5);
    assert!(
        (midpoint.opacity - 0.5).abs() < 1e-3,
        "opacity {}",
        midpoint.opacity
    );
    let TransformFunction::Translate { y, .. } = midpoint.transform.functions()[0] else {
        panic!("expected a translation, got {:?}", midpoint.transform);
    };
    assert_eq!(y, silksurf_css::Length::Px(2.0), "half of 4px");
}

/// `lightweight-first-message-caret-blink` declares only `50% { opacity: 0 }`.
/// Both endpoints are implicit, so they take the element's own opacity and
/// the rule reads as a blink rather than as a fade in from nothing.
#[test]
fn a_single_stop_rule_brackets_against_the_element_value() {
    let css = "@keyframes blink { 50% { opacity: 0 } }";
    let base = ComputedStyle {
        opacity: 1.0,
        ..Default::default()
    };
    let at_start = sample(css, "blink", &base, 0.0);
    let at_stop = sample(css, "blink", &base, 0.5);
    let at_end = sample(css, "blink", &base, 1.0);
    let quarter = sample(css, "blink", &base, 0.25);

    assert!(
        (at_start.opacity - 1.0).abs() < 1e-3,
        "{}",
        at_start.opacity
    );
    assert!(at_stop.opacity.abs() < 1e-3, "{}", at_stop.opacity);
    assert!((at_end.opacity - 1.0).abs() < 1e-3, "{}", at_end.opacity);
    assert!(
        (quarter.opacity - 0.5).abs() < 1e-3,
        "halfway to the stop: {}",
        quarter.opacity
    );
}

/// A property no block declares keeps the value the cascade computed, so a
/// rule animating opacity alone leaves the element's color where it was.
#[test]
fn an_undeclared_property_keeps_the_cascaded_value() {
    let css = "@keyframes fade { from { opacity: 0 } to { opacity: 1 } }";
    let base = ComputedStyle {
        color: silksurf_css::Color {
            r: 10,
            g: 20,
            b: 30,
            a: 255,
        },
        ..Default::default()
    };
    let midpoint = sample(css, "fade", &base, 0.5);
    assert_eq!(midpoint.color, base.color);
}

/// `mobile-static-assistant-stream-dot-pulse` shares one block across both
/// ends and holds a distinct value at the midpoint, so the sample returns to
/// its starting value.
#[test]
fn a_shared_end_block_returns_to_its_own_value() {
    let css = "@keyframes pulse { 0%, to { opacity: .68; transform: scale(.82) } \
               50% { opacity: 1; transform: scale(1) } }";
    let start = sample(css, "pulse", &ComputedStyle::default(), 0.0);
    let middle = sample(css, "pulse", &ComputedStyle::default(), 0.5);
    let end = sample(css, "pulse", &ComputedStyle::default(), 1.0);
    assert!((start.opacity - 0.68).abs() < 1e-3, "{}", start.opacity);
    assert!((middle.opacity - 1.0).abs() < 1e-3, "{}", middle.opacity);
    assert!((end.opacity - 0.68).abs() < 1e-3, "{}", end.opacity);
    assert_eq!(start.transform, end.transform);
}

/// Two scales interpolate componentwise, which every transform pair in the
/// captured corpus admits.
#[test]
fn matching_transform_lists_interpolate_componentwise() {
    let css = "@keyframes grow { 0% { transform: scale(75%) rotate(-90deg) } \
               to { transform: scale(100%) rotate(0deg) } }";
    let midpoint = sample(css, "grow", &ComputedStyle::default(), 0.5);
    let functions = midpoint.transform.functions();
    assert_eq!(functions.len(), 2, "{functions:?}");
    let TransformFunction::Scale { x, y } = functions[0] else {
        panic!("expected a scale, got {:?}", functions[0]);
    };
    assert!((x - 0.875).abs() < 1e-3, "scale x {x}");
    assert!((y - 0.875).abs() < 1e-3, "scale y {y}");
    let TransformFunction::Rotate { degrees } = functions[1] else {
        panic!("expected a rotation, got {:?}", functions[1]);
    };
    assert!((degrees + 45.0).abs() < 1e-3, "rotation {degrees}");
}

/// `mobile-static-assistant-reveal-fallback` declares `to { visibility:
/// visible }` over a hidden element. Visibility holds visible through the
/// interval once either endpoint is visible.
#[test]
fn visibility_holds_visible_across_the_interval() {
    let css = "@keyframes reveal { to { visibility: visible } }";
    let base = ComputedStyle {
        visibility: Visibility::Hidden,
        ..Default::default()
    };
    let midpoint = sample(css, "reveal", &base, 0.5);
    assert_eq!(midpoint.visibility, Visibility::Visible);
    let at_end = sample(css, "reveal", &base, 1.0);
    assert_eq!(at_end.visibility, Visibility::Visible);
}

/// The timeline answers where an animation stands, and a fill mode is what
/// makes it contribute outside its own interval.
#[test]
fn the_timeline_reports_progress_and_fill() {
    let mut running = spec(2.0);
    assert_eq!(animation_progress(&running, 0.0), Some(0.0));
    assert_eq!(animation_progress(&running, 1.0), Some(0.5));
    // Past the single iteration with no fill, the animation contributes
    // nothing and the element keeps its cascaded style.
    assert_eq!(animation_progress(&running, 2.5), None);

    running.fill_mode = AnimationFillMode::Forwards;
    assert_eq!(animation_progress(&running, 2.5), Some(1.0));

    running.delay = 1.0;
    running.fill_mode = AnimationFillMode::None;
    assert_eq!(animation_progress(&running, 0.5), None);
    assert_eq!(animation_progress(&running, 2.0), Some(0.5));

    running.fill_mode = AnimationFillMode::Backwards;
    assert_eq!(animation_progress(&running, 0.5), Some(0.0));
}

/// A `both` fill over a 0% keyframe of `opacity: 0` hides the element before
/// its delay elapses. Advancing time is what brings it back, so the fill and
/// the clock are one mechanism rather than two.
#[test]
fn a_both_fill_holds_the_zero_offset_through_the_delay() {
    let css = "@keyframes enter { 0% { opacity: 0 } to { opacity: 1 } }";
    let mut running = spec(0.3);
    running.delay = 0.5;
    running.fill_mode = AnimationFillMode::Both;
    let base = ComputedStyle {
        opacity: 1.0,
        ..Default::default()
    };

    let before = animation_progress(&running, 0.1).expect("backwards fill");
    assert!(sample(css, "enter", &base, before).opacity.abs() < 1e-3);

    let during = animation_progress(&running, 0.65).expect("active");
    let mid = sample(css, "enter", &base, during).opacity;
    assert!((0.1..0.9).contains(&mid), "midpoint opacity {mid}");

    let after = animation_progress(&running, 2.0).expect("forwards fill");
    assert!((sample(css, "enter", &base, after).opacity - 1.0).abs() < 1e-3);
}

/// An infinite animation never leaves its active interval, and an alternating
/// one runs odd iterations backwards.
#[test]
fn infinite_and_alternating_iterations_walk_the_timeline() {
    let mut running = spec(1.0);
    running.iteration_count = f32::INFINITY;
    assert_eq!(animation_progress(&running, 10.25), Some(0.25));

    running.direction = AnimationDirection::Alternate;
    assert_eq!(animation_progress(&running, 0.25), Some(0.25));
    // The second iteration runs backwards.
    assert_eq!(animation_progress(&running, 1.25), Some(0.75));
}
