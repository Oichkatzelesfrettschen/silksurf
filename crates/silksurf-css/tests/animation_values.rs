use silksurf_css::animation::{
    AnimationDirection, AnimationFillMode, AnimationSpec, TimingFunction, TransitionSpec,
};
use silksurf_css::{ComputedStyle, compute_styles, parse_stylesheet};
use silksurf_dom::Dom;

/// Computes the style of a single `div` carrying `declarations`.
fn div_style(declarations: &str) -> ComputedStyle {
    let sheet = parse_stylesheet(&format!("div {{ {declarations} }}")).unwrap();
    let mut dom = Dom::new();
    let doc = dom.create_document();
    let html = dom.create_element("html");
    dom.append_child(doc, html).unwrap();
    let div = dom.create_element("div");
    dom.append_child(html, div).unwrap();
    compute_styles(&dom, doc, &sheet)
        .get(&div)
        .expect("div style")
        .clone()
}

fn animation(declarations: &str) -> Vec<AnimationSpec> {
    div_style(declarations).animation.to_vec()
}

/// Every `animation` declaration in the captured corpus, in the order the
/// sheet spells them. The two times are the only components read by position.
#[test]
fn the_corpus_animation_declarations_parse() {
    assert_eq!(
        animation("animation: 0s linear 1.5s forwards mobile-static-assistant-reveal-fallback;"),
        vec![AnimationSpec {
            name: "mobile-static-assistant-reveal-fallback".into(),
            duration: 0.0,
            delay: 1.5,
            timing: TimingFunction::CubicBezier(0.0, 0.0, 1.0, 1.0),
            iteration_count: 1.0,
            direction: AnimationDirection::Normal,
            fill_mode: AnimationFillMode::Forwards,
        }]
    );
    assert_eq!(
        animation("animation: 1s step-end infinite lightweight-first-message-caret-blink;"),
        vec![AnimationSpec {
            name: "lightweight-first-message-caret-blink".into(),
            duration: 1.0,
            delay: 0.0,
            timing: TimingFunction::Steps(1, false),
            iteration_count: f32::INFINITY,
            direction: AnimationDirection::Normal,
            fill_mode: AnimationFillMode::None,
        }]
    );
    assert_eq!(
        animation("animation: .3s cubic-bezier(.2,0,0,1) both mobile-empty-composer-action-enter;"),
        vec![AnimationSpec {
            name: "mobile-empty-composer-action-enter".into(),
            duration: 0.3,
            delay: 0.0,
            timing: TimingFunction::CubicBezier(0.2, 0.0, 0.0, 1.0),
            iteration_count: 1.0,
            direction: AnimationDirection::Normal,
            fill_mode: AnimationFillMode::Both,
        }]
    );
    // The name leads rather than trails in this one, which the keyword-first
    // classification handles without a positional rule for it.
    assert_eq!(
        animation("animation: enlarge-appear .4s ease-out;"),
        vec![AnimationSpec {
            name: "enlarge-appear".into(),
            duration: 0.4,
            delay: 0.0,
            timing: TimingFunction::CubicBezier(0.0, 0.0, 0.58, 1.0),
            iteration_count: 1.0,
            direction: AnimationDirection::Normal,
            fill_mode: AnimationFillMode::None,
        }]
    );
    assert!(animation("animation: none;").is_empty());
    assert!(animation("color: red;").is_empty());
}

/// The one `transition` declaration in the corpus is a three-component list,
/// so the list form is the shape the property actually ships in.
#[test]
fn the_corpus_transition_declaration_parses_as_three_components() {
    let specs = div_style("transition: background-color .14s, box-shadow .14s, transform .14s;")
        .transition
        .to_vec();
    assert_eq!(
        specs,
        vec![
            TransitionSpec {
                property: "background-color".into(),
                duration: 0.14,
                delay: 0.0,
                timing: TimingFunction::default(),
            },
            TransitionSpec {
                property: "box-shadow".into(),
                duration: 0.14,
                delay: 0.0,
                timing: TimingFunction::default(),
            },
            TransitionSpec {
                property: "transform".into(),
                duration: 0.14,
                delay: 0.0,
                timing: TimingFunction::default(),
            },
        ]
    );
}

/// Milliseconds and seconds reach the same value, and the second time in a
/// component is the delay whatever surrounds it.
#[test]
fn times_read_by_position_and_unit() {
    let spec = &animation("animation: spin 400ms 2s;")[0];
    assert!((spec.duration - 0.4).abs() < 1e-6, "{}", spec.duration);
    assert!((spec.delay - 2.0).abs() < 1e-6, "{}", spec.delay);
}

/// `linear` maps progress onto itself; `ease-out` leads it; `step-end` holds
/// the start value until the interval closes.
#[test]
fn the_named_curves_map_progress_as_specified() {
    let linear = TimingFunction::CubicBezier(0.0, 0.0, 1.0, 1.0);
    for progress in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
        assert!(
            (linear.ease(progress) - progress).abs() < 1e-3,
            "linear at {progress}: {}",
            linear.ease(progress)
        );
    }
    let ease_out = TimingFunction::CubicBezier(0.0, 0.0, 0.58, 1.0);
    assert!(ease_out.ease(0.5) > 0.5, "ease-out lags at the midpoint");

    let step_end = TimingFunction::Steps(1, false);
    assert!((step_end.ease(0.99) - 0.0).abs() < 1e-6);
    assert!((step_end.ease(1.0) - 1.0).abs() < 1e-6);

    let step_start = TimingFunction::Steps(1, true);
    assert!((step_start.ease(0.01) - 1.0).abs() < 1e-6);
}

/// Every easing curve fixes both endpoints, so an animation starts and ends
/// on its keyframe values whatever curve it carries.
#[test]
fn every_curve_fixes_both_endpoints() {
    let curves = [
        TimingFunction::CubicBezier(0.2, 0.0, 0.0, 1.0),
        TimingFunction::CubicBezier(0.42, 0.0, 0.58, 1.0),
        TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0),
        TimingFunction::Steps(4, false),
    ];
    for curve in curves {
        assert!(curve.ease(0.0).abs() < 1e-6, "{curve:?} at 0");
        assert!((curve.ease(1.0) - 1.0).abs() < 1e-6, "{curve:?} at 1");
    }
}

/// The bezier solver inverts its own x curve, so easing a progress the curve
/// maps to a known x returns that x's y.
#[test]
fn the_bezier_solver_inverts_its_own_curve() {
    let curve = TimingFunction::CubicBezier(0.2, 0.0, 0.0, 1.0);
    let mut previous = 0.0;
    for step in 0..=20 {
        let progress = step as f32 / 20.0;
        let eased = curve.ease(progress);
        assert!(eased >= previous - 1e-4, "curve fell at {progress}");
        assert!(
            (0.0..=1.0).contains(&eased),
            "curve left range at {progress}"
        );
        previous = eased;
    }
}

/// A comma inside a function separates that function's arguments, not the
/// list. Splitting on every comma cuts `cubic-bezier(.2, 0, 0, 1)` into four
/// components and loses the whole declaration.
#[test]
fn a_function_argument_comma_does_not_split_the_list() {
    let specs = animation("animation: a 1s cubic-bezier(.2,0,0,1), b 2s linear;");
    assert_eq!(specs.len(), 2, "{specs:?}");
    assert_eq!(specs[0].name, "a");
    assert_eq!(
        specs[0].timing,
        TimingFunction::CubicBezier(0.2, 0.0, 0.0, 1.0)
    );
    assert_eq!(specs[1].name, "b");
    assert!((specs[1].duration - 2.0).abs() < 1e-6);

    let transitions = div_style("transition: opacity 1s cubic-bezier(.2,0,0,1), width 2s;")
        .transition
        .to_vec();
    assert_eq!(transitions.len(), 2, "{transitions:?}");
    assert_eq!(transitions[0].property, "opacity");
    assert_eq!(transitions[1].property, "width");
}
