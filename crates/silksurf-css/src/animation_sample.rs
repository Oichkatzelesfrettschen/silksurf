//! Sampling a `@keyframes` rule onto an element's own computed style.
//!
//! CSS Animations 1 gives each property its own keyframe list. A property a
//! block omits does not participate at that offset, and an endpoint no block
//! declares takes the element's own computed value -- which is what makes
//! `50% { opacity: 0 }` a blink rather than a fade from nothing. The sampler
//! therefore brackets per property rather than per offset.

use crate::Length;
use crate::animation::{AnimationDirection, AnimationFillMode, AnimationSpec};
use crate::property_id::PropertyId;
use crate::style::{
    Color, ComputedStyle, KeyframesRule, Transform, TransformFunction, Visibility,
    style_with_declarations,
};

/// The eased progress an animation stands at, or `None` when it contributes
/// nothing at this time.
///
/// A fill mode is what makes an animation contribute outside its active
/// interval, so the two are answered together: an animation before its delay
/// with no backwards fill, or past its last iteration with no forwards fill,
/// leaves the element at the style the cascade computed.
#[must_use]
pub fn animation_progress(spec: &AnimationSpec, elapsed_seconds: f32) -> Option<f32> {
    let fills_backwards = matches!(
        spec.fill_mode,
        AnimationFillMode::Backwards | AnimationFillMode::Both
    );
    let fills_forwards = matches!(
        spec.fill_mode,
        AnimationFillMode::Forwards | AnimationFillMode::Both
    );
    let active = elapsed_seconds - spec.delay;
    if active < 0.0 {
        return fills_backwards.then(|| spec.timing.ease(directed(0.0, 0.0, spec.direction)));
    }
    if spec.duration <= 0.0 {
        // A zero duration has no interval to run through, so the animation
        // stands at its end for as long as a forwards fill holds it.
        return fills_forwards.then(|| spec.timing.ease(directed(1.0, 0.0, spec.direction)));
    }
    let iterations = active / spec.duration;
    if iterations >= spec.iteration_count {
        let last = (spec.iteration_count - 1.0).max(0.0).floor();
        return fills_forwards.then(|| spec.timing.ease(directed(1.0, last, spec.direction)));
    }
    let iteration = iterations.floor();
    Some(
        spec.timing
            .ease(directed(iterations - iteration, iteration, spec.direction)),
    )
}

/// Applies `animation-direction` to one iteration's progress.
fn directed(progress: f32, iteration: f32, direction: AnimationDirection) -> f32 {
    let odd = (iteration as i64) % 2 != 0;
    let reversed = match direction {
        AnimationDirection::Normal => false,
        AnimationDirection::Reverse => true,
        AnimationDirection::Alternate => odd,
        AnimationDirection::AlternateReverse => !odd,
    };
    if reversed { 1.0 - progress } else { progress }
}

/// The properties this sampler interpolates.
///
/// Every property the captured corpus animates appears here, along with the
/// two its one `transition` declaration names that carry a numeric value.
/// The roadmap carries the rest as `animatable-property-set`.
const ANIMATABLE: [PropertyId; 7] = [
    PropertyId::Opacity,
    PropertyId::Color,
    PropertyId::BackgroundColor,
    PropertyId::BorderColor,
    PropertyId::Transform,
    PropertyId::Visibility,
    PropertyId::BorderRadius,
];

/// Copies one property's value from `source` onto `target`.
///
/// The sampler works in whole `ComputedStyle` values, so moving a single
/// property between two of them is how a keyframe block's declarations reach
/// the style without disturbing the properties it says nothing about.
pub(crate) fn copy_animatable(target: &mut ComputedStyle, source: &ComputedStyle, id: PropertyId) {
    match id {
        PropertyId::Opacity => target.opacity = source.opacity,
        PropertyId::Color => target.color = source.color,
        PropertyId::BackgroundColor => target.background_color = source.background_color,
        PropertyId::BorderColor => target.border_color = source.border_color,
        PropertyId::Transform => target.transform = source.transform.clone(),
        PropertyId::Visibility => target.visibility = source.visibility,
        PropertyId::BorderRadius => target.border_radius = source.border_radius,
        _ => {}
    }
}

/// Samples `rule` at `progress` over the style the cascade computed.
///
/// Each animatable property brackets against the stops that declare it, so a
/// rule declaring one property at one offset moves that property alone.
#[must_use]
pub fn sample_keyframes(
    base: &ComputedStyle,
    rule: &KeyframesRule,
    progress: f32,
    rem_base_px: f32,
    viewport: (f32, f32),
) -> ComputedStyle {
    let progress = progress.clamp(0.0, 1.0);
    let resolved: Vec<(f32, ComputedStyle)> = rule
        .stops
        .iter()
        .map(|(offset, declarations)| {
            (
                *offset,
                style_with_declarations(base, declarations, rem_base_px, viewport),
            )
        })
        .collect();
    let mut style = base.clone();
    for id in ANIMATABLE {
        let declaring: Vec<usize> = rule
            .stops
            .iter()
            .enumerate()
            .filter(|(_, (_, declarations))| declarations.iter().any(|d| d.property_id == id))
            .map(|(index, _)| index)
            .collect();
        if declaring.is_empty() {
            continue;
        }
        interpolate_property(&mut style, base, &resolved, &declaring, id, progress);
    }
    style
}

/// Writes one property's interpolated value onto `style`.
///
/// An offset before the first declaring stop or after the last takes the
/// element's own value, which is the implicit keyframe CSS Animations 1 adds
/// at each end.
fn interpolate_property(
    style: &mut ComputedStyle,
    base: &ComputedStyle,
    resolved: &[(f32, ComputedStyle)],
    declaring: &[usize],
    id: PropertyId,
    progress: f32,
) {
    // UNWRAP-OK: the caller returns early on an empty `declaring`.
    let first = declaring[0];
    // UNWRAP-OK: the same non-empty slice supplies the last index.
    let last = declaring[declaring.len() - 1];
    let (start, end, local) = match declaring
        .windows(2)
        .find(|pair| resolved[pair[0]].0 <= progress && progress <= resolved[pair[1]].0)
    {
        Some(pair) => {
            let (low, high) = (resolved[pair[0]].0, resolved[pair[1]].0);
            let span = high - low;
            let local = if span > 0.0 {
                (progress - low) / span
            } else {
                0.0
            };
            (pair[0], pair[1], local)
        }
        None if progress < resolved[first].0 => (first, first, 0.0),
        None if progress > resolved[last].0 => (last, last, 0.0),
        // A single declaring stop brackets against the element's own value on
        // whichever side the progress falls, which is the implicit keyframe.
        None => (first, first, 0.0),
    };
    let before = &resolved[start].1;
    let after = &resolved[end].1;
    if declaring.len() == 1 {
        let stop = resolved[first].0;
        let (from, to, local) = if progress <= stop {
            let span = stop;
            (base, before, if span > 0.0 { progress / span } else { 1.0 })
        } else {
            let span = 1.0 - stop;
            (
                before,
                base,
                if span > 0.0 {
                    (progress - stop) / span
                } else {
                    1.0
                },
            )
        };
        write_interpolated(style, from, to, local, id);
        return;
    }
    write_interpolated(style, before, after, local, id);
}

/// Interpolates one property between two whole styles.
fn write_interpolated(
    style: &mut ComputedStyle,
    from: &ComputedStyle,
    to: &ComputedStyle,
    local: f32,
    id: PropertyId,
) {
    match id {
        PropertyId::Opacity => style.opacity = lerp(from.opacity, to.opacity, local),
        PropertyId::Color => style.color = lerp_color(from.color, to.color, local),
        PropertyId::BackgroundColor => {
            style.background_color = lerp_color(from.background_color, to.background_color, local);
        }
        PropertyId::BorderColor => {
            style.border_color = lerp_color(from.border_color, to.border_color, local);
        }
        PropertyId::BorderRadius => {
            style.border_radius = lerp(from.border_radius, to.border_radius, local);
        }
        PropertyId::Transform => {
            style.transform = lerp_transform(&from.transform, &to.transform, local);
        }
        PropertyId::Visibility => {
            // CSS Transitions 1 interpolates visibility so that `visible` on
            // either endpoint holds through the interval, which is what lets
            // an element appear at the start of a reveal and disappear only
            // once it ends.
            style.visibility =
                if from.visibility == Visibility::Visible || to.visibility == Visibility::Visible {
                    if local >= 1.0 {
                        to.visibility
                    } else {
                        Visibility::Visible
                    }
                } else {
                    to.visibility
                };
        }
        _ => {}
    }
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

/// Interpolates each channel, including alpha.
fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    let channel = |a: u8, b: u8| {
        lerp(f32::from(a), f32::from(b), t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color {
        r: channel(from.r, to.r),
        g: channel(from.g, to.g),
        b: channel(from.b, to.b),
        a: channel(from.a, to.a),
    }
}

/// Interpolates two transform lists.
///
/// CSS Transforms 1 interpolates componentwise when both lists carry the same
/// functions in the same order, which every transform pair in the captured
/// corpus does. Lists that disagree have no componentwise correspondence, so
/// the value steps at the midpoint; the roadmap carries the matrix
/// decomposition as `transform-matrix-interpolation`.
fn lerp_transform(from: &Transform, to: &Transform, t: f32) -> Transform {
    let (before, after) = (from.functions(), to.functions());
    if before.is_empty() || after.is_empty() || before.len() != after.len() {
        return if t < 0.5 { from.clone() } else { to.clone() };
    }
    let mut out = Vec::with_capacity(before.len());
    for (a, b) in before.iter().zip(after.iter()) {
        let Some(mixed) = lerp_transform_function(a, b, t) else {
            return if t < 0.5 { from.clone() } else { to.clone() };
        };
        out.push(mixed);
    }
    Transform::from_functions(out)
}

/// Interpolates two transform functions of the same kind.
fn lerp_transform_function(
    from: &TransformFunction,
    to: &TransformFunction,
    t: f32,
) -> Option<TransformFunction> {
    match (from, to) {
        (
            TransformFunction::Translate { x: ax, y: ay },
            TransformFunction::Translate { x: bx, y: by },
        ) => Some(TransformFunction::Translate {
            x: lerp_length(*ax, *bx, t)?,
            y: lerp_length(*ay, *by, t)?,
        }),
        (TransformFunction::Scale { x: ax, y: ay }, TransformFunction::Scale { x: bx, y: by }) => {
            Some(TransformFunction::Scale {
                x: lerp(*ax, *bx, t),
                y: lerp(*ay, *by, t),
            })
        }
        (TransformFunction::Rotate { degrees: a }, TransformFunction::Rotate { degrees: b }) => {
            Some(TransformFunction::Rotate {
                degrees: lerp(*a, *b, t),
            })
        }
        (
            TransformFunction::Skew {
                x_degrees: ax,
                y_degrees: ay,
            },
            TransformFunction::Skew {
                x_degrees: bx,
                y_degrees: by,
            },
        ) => Some(TransformFunction::Skew {
            x_degrees: lerp(*ax, *bx, t),
            y_degrees: lerp(*ay, *by, t),
        }),
        _ => None,
    }
}

/// Interpolates two lengths of the same unit.
///
/// A percentage resolves against the element's border box, which the cascade
/// does not know, so a pair mixing units has no common scale here and the
/// caller steps the whole list instead.
fn lerp_length(from: Length, to: Length, t: f32) -> Option<Length> {
    match (from, to) {
        (Length::Px(a), Length::Px(b)) => Some(Length::Px(lerp(a, b, t))),
        (Length::Percent(a), Length::Percent(b)) => Some(Length::Percent(lerp(a, b, t))),
        (Length::Em(a), Length::Em(b)) => Some(Length::Em(lerp(a, b, t))),
        (Length::Rem(a), Length::Rem(b)) => Some(Length::Rem(lerp(a, b, t))),
        _ => None,
    }
}
