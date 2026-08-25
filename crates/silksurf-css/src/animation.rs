//! `animation` and `transition` values, and the easing they share.
//!
//! Both properties are comma-separated lists whose components are read
//! positionally, so parsing splits on `CssToken::Comma` and then classifies
//! each component by its token shape. CSS Animations 1 orders the two time
//! values in the `animation` shorthand: the first is the duration and the
//! second the delay, whatever else surrounds them.
//!
//! The list types hold one component inline, because a page declares a single
//! animation far more often than several, while `transition` genuinely ships
//! lists -- the corpus under docs/roadmaps declares
//! `background-color .14s, box-shadow .14s, transform .14s`.

use crate::CssToken;
use smallvec::SmallVec;
use smol_str::SmolStr;

/// The easing an animation or transition applies to its progress.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimingFunction {
    /// Four control-point coordinates, `x1, y1, x2, y2`.
    ///
    /// The named curves are all cubic beziers, so they carry their control
    /// points here rather than as separate variants.
    CubicBezier(f32, f32, f32, f32),
    /// A step count and whether the jump happens at the start of each step.
    Steps(u32, bool),
}

impl Default for TimingFunction {
    /// `ease`, the initial value CSS Easing 1 gives both properties.
    fn default() -> Self {
        TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0)
    }
}

impl TimingFunction {
    /// Maps linear progress in [0, 1] onto eased progress.
    ///
    /// A cubic bezier used as an easing curve is a function of x rather than
    /// of its own parameter, so the parameter that reaches `progress` on the
    /// x axis is solved for first and the y coordinate there is the result.
    #[must_use]
    pub fn ease(self, progress: f32) -> f32 {
        let progress = progress.clamp(0.0, 1.0);
        match self {
            TimingFunction::CubicBezier(x1, y1, x2, y2) => {
                // The identity curve maps progress onto itself, and solving
                // for it costs the full Newton sweep to return the input.
                if (x1 - y1).abs() < f32::EPSILON && (x2 - y2).abs() < f32::EPSILON {
                    return progress;
                }
                bezier_axis(bezier_parameter_at_x(progress, x1, x2), y1, y2)
            }
            TimingFunction::Steps(count, jump_at_start) => {
                let count = count.max(1) as f32;
                let step = (progress * count).floor() + if jump_at_start { 1.0 } else { 0.0 };
                (step / count).clamp(0.0, 1.0)
            }
        }
    }
}

/// One coordinate of a cubic bezier whose outer control points are 0 and 1.
fn bezier_axis(t: f32, p1: f32, p2: f32) -> f32 {
    let inverse = 1.0 - t;
    3.0 * inverse * inverse * t * p1 + 3.0 * inverse * t * t * p2 + t * t * t
}

/// The derivative of `bezier_axis` with respect to the curve parameter.
fn bezier_slope(t: f32, p1: f32, p2: f32) -> f32 {
    let inverse = 1.0 - t;
    3.0 * inverse * inverse * p1 + 6.0 * inverse * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
}

/// Solves `bezier_axis(t, x1, x2) == x` for the curve parameter.
///
/// Newton-Raphson converges in a few steps over the monotonic x range an
/// easing curve is restricted to. A slope near zero leaves the step
/// undefined, so the search falls back to bisection, which cannot diverge.
fn bezier_parameter_at_x(x: f32, x1: f32, x2: f32) -> f32 {
    let mut t = x;
    for _ in 0..8 {
        let error = bezier_axis(t, x1, x2) - x;
        if error.abs() < 1e-6 {
            return t;
        }
        let slope = bezier_slope(t, x1, x2);
        if slope.abs() < 1e-6 {
            break;
        }
        t -= error / slope;
    }
    let (mut low, mut high) = (0.0f32, 1.0f32);
    let mut t = x.clamp(0.0, 1.0);
    for _ in 0..24 {
        let value = bezier_axis(t, x1, x2);
        if (value - x).abs() < 1e-6 {
            break;
        }
        if value < x {
            low = t;
        } else {
            high = t;
        }
        t = f32::midpoint(low, high);
    }
    t
}

/// Which iterations of an animation run in reverse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnimationDirection {
    #[default]
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

/// Which side of its active interval an animation holds a value on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnimationFillMode {
    #[default]
    None,
    Forwards,
    Backwards,
    Both,
}

/// One component of an `animation` list.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationSpec {
    /// The `@keyframes` rule this component names. Empty is `none`.
    pub name: SmolStr,
    /// Seconds. A zero duration holds the animation at its endpoint.
    pub duration: f32,
    /// Seconds; negative values start the animation partway through.
    pub delay: f32,
    pub timing: TimingFunction,
    /// `f32::INFINITY` for `infinite`.
    pub iteration_count: f32,
    pub direction: AnimationDirection,
    pub fill_mode: AnimationFillMode,
}

impl Default for AnimationSpec {
    fn default() -> Self {
        Self {
            name: SmolStr::default(),
            duration: 0.0,
            delay: 0.0,
            timing: TimingFunction::default(),
            iteration_count: 1.0,
            direction: AnimationDirection::default(),
            fill_mode: AnimationFillMode::default(),
        }
    }
}

/// One component of a `transition` list.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionSpec {
    /// The property name this component transitions. `all` matches every one.
    pub property: SmolStr,
    pub duration: f32,
    pub delay: f32,
    pub timing: TimingFunction,
}

impl Default for TransitionSpec {
    fn default() -> Self {
        Self {
            property: SmolStr::new("all"),
            duration: 0.0,
            delay: 0.0,
            timing: TimingFunction::default(),
        }
    }
}

/// A parsed `animation` value. Empty is `none`, the initial value.
pub type AnimationList = SmallVec<[AnimationSpec; 1]>;

/// A parsed `transition` value. Empty is the initial value.
pub type TransitionList = SmallVec<[TransitionSpec; 1]>;

/// Splits a comma-separated value into its components at top level.
///
/// A comma inside a function's parentheses separates that function's
/// arguments rather than the list, so `cubic-bezier(.2, 0, 0, 1)` is one
/// component and splitting on every comma would cut it into four.
fn components(tokens: &[CssToken]) -> Vec<&[CssToken]> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token {
            CssToken::Function(_) | CssToken::ParenOpen => depth += 1,
            CssToken::ParenClose => depth = depth.saturating_sub(1),
            CssToken::Comma if depth == 0 => {
                out.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    out.push(&tokens[start..]);
    out
}

/// Reads a `<time>` in seconds from one token.
fn time_seconds(token: &CssToken) -> Option<f32> {
    let CssToken::Dimension { value, unit } = token else {
        return None;
    };
    let value = value.parse::<f32>().ok()?;
    if unit.eq_ignore_ascii_case("s") {
        return Some(value);
    }
    if unit.eq_ignore_ascii_case("ms") {
        return Some(value / 1000.0);
    }
    None
}

/// Reads a named or functional `<easing-function>`.
///
/// The named curves carry the control points CSS Easing 1 lists for them, so
/// every branch but `steps()` produces one cubic bezier.
pub fn timing_function(tokens: &[CssToken]) -> Option<TimingFunction> {
    let mut rest = tokens.iter().filter(|t| !matches!(t, CssToken::Whitespace));
    match rest.next()? {
        CssToken::Ident(name) => named_timing_function(name),
        CssToken::Function(name) if name.eq_ignore_ascii_case("cubic-bezier") => {
            let mut numbers = Vec::new();
            for token in rest {
                match token {
                    CssToken::Number(value) => numbers.push(value.parse::<f32>().ok()?),
                    CssToken::Comma => {}
                    CssToken::ParenClose => break,
                    _ => return None,
                }
            }
            let [x1, y1, x2, y2] = numbers[..] else {
                return None;
            };
            Some(TimingFunction::CubicBezier(x1, y1, x2, y2))
        }
        CssToken::Function(name) if name.eq_ignore_ascii_case("steps") => {
            let mut count = None;
            let mut jump_at_start = false;
            for token in rest {
                match token {
                    CssToken::Number(value) => count = value.parse::<f32>().ok(),
                    CssToken::Ident(position) => {
                        jump_at_start = position.eq_ignore_ascii_case("start")
                            || position.eq_ignore_ascii_case("jump-start");
                    }
                    CssToken::Comma | CssToken::ParenClose => {}
                    _ => return None,
                }
            }
            Some(TimingFunction::Steps(count? as u32, jump_at_start))
        }
        _ => None,
    }
}

/// The control points CSS Easing 1 assigns each named curve.
fn named_timing_function(name: &str) -> Option<TimingFunction> {
    let named = [
        ("linear", TimingFunction::CubicBezier(0.0, 0.0, 1.0, 1.0)),
        ("ease", TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0)),
        ("ease-in", TimingFunction::CubicBezier(0.42, 0.0, 1.0, 1.0)),
        ("ease-out", TimingFunction::CubicBezier(0.0, 0.0, 0.58, 1.0)),
        (
            "ease-in-out",
            TimingFunction::CubicBezier(0.42, 0.0, 0.58, 1.0),
        ),
        ("step-start", TimingFunction::Steps(1, true)),
        ("step-end", TimingFunction::Steps(1, false)),
    ];
    named
        .iter()
        .find(|(candidate, _)| name.eq_ignore_ascii_case(candidate))
        .map(|(_, function)| *function)
}

/// Parses an `animation` shorthand list.
///
/// Every component but the two times is identified by keyword, so the two
/// `<time>` values are the only ones read by position: CSS Animations 1
/// takes the first as the duration and the second as the delay. A bare
/// identifier that matches no keyword is the `@keyframes` name.
pub fn parse_animation(tokens: &[CssToken]) -> Option<AnimationList> {
    let mut list = AnimationList::new();
    for component in components(tokens) {
        let spec = animation_component(component)?;
        // `none` is a valid fill mode and the animation-name that means no
        // animation. CSS Animations 1 gives the name the whole declaration,
        // so a component naming nothing else drops the list.
        if spec.name.is_empty() || spec.name.eq_ignore_ascii_case("none") {
            return Some(AnimationList::new());
        }
        list.push(spec);
    }
    Some(list)
}

/// Reads one component of an `animation` list.
fn animation_component(tokens: &[CssToken]) -> Option<AnimationSpec> {
    let mut spec = AnimationSpec::default();
    let mut times = 0;
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        index += 1;
        if matches!(token, CssToken::Whitespace) {
            continue;
        }
        if let Some(seconds) = time_seconds(token) {
            if times == 0 {
                spec.duration = seconds.max(0.0);
            } else if times == 1 {
                spec.delay = seconds;
            }
            times += 1;
            continue;
        }
        if let CssToken::Number(value) = token {
            spec.iteration_count = value.parse::<f32>().ok()?.max(0.0);
            continue;
        }
        if let CssToken::Function(_) = token {
            let end = index
                + tokens[index..]
                    .iter()
                    .position(|t| matches!(t, CssToken::ParenClose))
                    .map_or(tokens.len() - index, |offset| offset + 1);
            spec.timing = timing_function(&tokens[index - 1..end])?;
            index = end;
            continue;
        }
        let CssToken::Ident(name) = token else {
            return None;
        };
        if !animation_keyword(&mut spec, name) {
            spec.name = name.clone();
        }
    }
    Some(spec)
}

/// Applies one `animation` keyword, reporting whether it named a component.
///
/// A name that matches no keyword falls through to the `@keyframes` name,
/// which is why this reports a match rather than failing.
fn animation_keyword(spec: &mut AnimationSpec, name: &str) -> bool {
    if let Some(timing) = named_timing_function(name) {
        spec.timing = timing;
        return true;
    }
    let direction = [
        ("normal", AnimationDirection::Normal),
        ("reverse", AnimationDirection::Reverse),
        ("alternate", AnimationDirection::Alternate),
        ("alternate-reverse", AnimationDirection::AlternateReverse),
    ];
    if let Some((_, value)) = direction.iter().find(|(k, _)| name.eq_ignore_ascii_case(k)) {
        spec.direction = *value;
        // `normal` is also the initial fill mode's spelling in `none`, and the
        // direction grammar claims it first.
        return true;
    }
    let fill = [
        ("none", AnimationFillMode::None),
        ("forwards", AnimationFillMode::Forwards),
        ("backwards", AnimationFillMode::Backwards),
        ("both", AnimationFillMode::Both),
    ];
    if let Some((_, value)) = fill.iter().find(|(k, _)| name.eq_ignore_ascii_case(k)) {
        spec.fill_mode = *value;
        return !name.eq_ignore_ascii_case("none");
    }
    if name.eq_ignore_ascii_case("infinite") {
        spec.iteration_count = f32::INFINITY;
        return true;
    }
    false
}

/// Parses a `transition` shorthand list.
///
/// One `<time>` is the duration and a second is the delay, matching the
/// `animation` grammar. A bare identifier that names no easing curve is the
/// transitioned property.
pub fn parse_transition(tokens: &[CssToken]) -> Option<TransitionList> {
    let mut list = TransitionList::new();
    for component in components(tokens) {
        let spec = transition_component(component)?;
        if spec.property.is_empty() {
            return Some(TransitionList::new());
        }
        list.push(spec);
    }
    Some(list)
}

/// Reads one component of a `transition` list.
fn transition_component(tokens: &[CssToken]) -> Option<TransitionSpec> {
    let mut spec = TransitionSpec::default();
    let mut times = 0;
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        index += 1;
        if matches!(token, CssToken::Whitespace) {
            continue;
        }
        if let Some(seconds) = time_seconds(token) {
            if times == 0 {
                spec.duration = seconds.max(0.0);
            } else if times == 1 {
                spec.delay = seconds;
            }
            times += 1;
            continue;
        }
        if let CssToken::Function(_) = token {
            let end = index
                + tokens[index..]
                    .iter()
                    .position(|t| matches!(t, CssToken::ParenClose))
                    .map_or(tokens.len() - index, |offset| offset + 1);
            spec.timing = timing_function(&tokens[index - 1..end])?;
            index = end;
            continue;
        }
        let CssToken::Ident(name) = token else {
            return None;
        };
        if name.eq_ignore_ascii_case("none") {
            spec.property = SmolStr::default();
        } else if let Some(timing) = named_timing_function(name) {
            spec.timing = timing;
        } else {
            spec.property = name.clone();
        }
    }
    Some(spec)
}
