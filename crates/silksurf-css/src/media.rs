/*
 * media.rs -- CSS @media query evaluator.
 *
 * WHY: @media rules gate entire blocks of style rules behind viewport or
 * capability conditions. Without evaluation they are silently dropped in
 * StyleIndex::new(), breaking responsive layouts on real-world pages.
 *
 * SCOPE: handles the common Level 3 and Level 4 query forms:
 *   media types: screen (true), print/speech/tty/tv/... (false), all (true)
 *   features:    max-width, min-width, max-height, min-height, width, height
 *                orientation, prefers-color-scheme, prefers-reduced-motion,
 *                prefers-contrast, color, color-gamut, display-mode
 *   connectors:  not, only (legacy), and
 *   list:        comma-separated -- any match wins
 *
 * UNKNOWN/COMPLEX forms: default to true (apply the rules). This is the
 * safe fallback for a static renderer: rendering too many rules is less
 * wrong than rendering too few.
 *
 * NOT HANDLED (default to true or false as noted):
 *   or, Level 4 range syntax (width >= 768px), @supports nesting.
 */

use crate::CssToken;

/// Evaluate a @media prelude (stored as `AtRule::prelude: Vec<CssToken>`)
/// against the given viewport dimensions (pixels).
///
/// Returns true when the media condition applies to this viewport.
pub fn evaluate_media_query(prelude: &[CssToken], viewport_w: f32, viewport_h: f32) -> bool {
    let toks: Vec<&CssToken> = prelude
        .iter()
        .filter(|t| !matches!(t, CssToken::Whitespace))
        .collect();
    if toks.is_empty() {
        return true; // bare @media {} = all
    }
    eval_list(&toks, viewport_w, viewport_h)
}

// ---------------------------------------------------------------------------
// Comma-separated list: any matching sub-query wins.
// ---------------------------------------------------------------------------

fn eval_list(toks: &[&CssToken], w: f32, h: f32) -> bool {
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut found = false;
    for (i, tok) in toks.iter().enumerate() {
        match tok {
            CssToken::ParenOpen | CssToken::Function(_) => depth += 1,
            CssToken::ParenClose => depth = depth.saturating_sub(1),
            CssToken::Comma if depth == 0 => {
                if eval_single(&toks[start..i], w, h) {
                    found = true;
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    found || eval_single(&toks[start..], w, h)
}

// ---------------------------------------------------------------------------
// Single media query: [not|only] media-type? [and (...)]*.
// ---------------------------------------------------------------------------

fn eval_single(toks: &[&CssToken], w: f32, h: f32) -> bool {
    if toks.is_empty() {
        return true;
    }
    let mut i = 0usize;

    // Optional NOT / ONLY prefix.
    let negated = match toks.get(i) {
        Some(CssToken::Ident(kw)) if kw.eq_ignore_ascii_case("not") => {
            i += 1;
            true
        }
        Some(CssToken::Ident(kw)) if kw.eq_ignore_ascii_case("only") => {
            i += 1; // legacy hint for old browsers; no semantic effect
            false
        }
        _ => false,
    };

    // Optional media type: the first Ident that is not "and".
    let media_ok = match toks.get(i) {
        Some(CssToken::Ident(mt)) if !mt.eq_ignore_ascii_case("and") => {
            i += 1;
            media_type_matches(mt)
        }
        // No media type token (query starts with a paren condition or "and").
        _ => true,
    };

    // Skip a bare "and" that appears between the type and the first condition.
    if matches!(toks.get(i), Some(CssToken::Ident(kw)) if kw.eq_ignore_ascii_case("and")) {
        i += 1;
    }

    // Collect parenthesized feature conditions; all must match (AND semantics).
    let mut features_ok = true;
    while i < toks.len() {
        match toks.get(i) {
            Some(CssToken::ParenOpen) => {
                let end = find_close_paren(toks, i);
                let inner = &toks[(i + 1)..end];
                if !eval_feature(inner, w, h) {
                    features_ok = false;
                }
                i = end + 1;
                // Skip "and" connector between conditions.
                if matches!(toks.get(i), Some(CssToken::Ident(kw)) if kw.eq_ignore_ascii_case("and")) {
                    i += 1;
                }
            }
            // Skip any remaining unexpected tokens (e.g., "or", unknown types).
            _ => {
                i += 1;
            }
        }
    }

    let result = media_ok && features_ok;
    if negated { !result } else { result }
}

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

fn media_type_matches(mt: &str) -> bool {
    // "screen" and "all" are the display-rendering targets.
    // Everything else is a non-screen medium.
    let lower = mt.to_ascii_lowercase();
    !matches!(
        lower.as_str(),
        "print"
            | "speech"
            | "aural"
            | "tty"
            | "tv"
            | "projection"
            | "handheld"
            | "braille"
            | "embossed"
    )
}

/// Find the closing ParenClose that matches the ParenOpen at `open_at`.
/// Returns the index of the closing token, or `toks.len() - 1` if not found
/// (graceful degradation for malformed queries).
fn find_close_paren(toks: &[&CssToken], open_at: usize) -> usize {
    let mut depth = 0usize;
    for (j, t) in toks[open_at..].iter().enumerate() {
        match t {
            CssToken::ParenOpen | CssToken::Function(_) => depth += 1,
            CssToken::ParenClose => {
                depth -= 1;
                if depth == 0 {
                    return open_at + j;
                }
            }
            _ => {}
        }
    }
    toks.len().saturating_sub(1)
}

/// Evaluate a feature condition (the token sequence inside a `(...)`).
/// Grammar: feature-name : value  or  just feature-name (boolean feature).
fn eval_feature(inner: &[&CssToken], w: f32, h: f32) -> bool {
    // Split on the first Colon.
    let colon_pos = inner.iter().position(|t| matches!(t, CssToken::Colon));

    let name_toks = colon_pos.map_or(inner, |c| &inner[..c]);
    let value_toks = colon_pos.map_or(&[] as &[_], |c| &inner[(c + 1)..]);

    // Feature name: first Ident in the name section.
    let feature = name_toks
        .iter()
        .find_map(|t| {
            if let CssToken::Ident(s) = t {
                Some(s.as_str())
            } else {
                None
            }
        })
        .unwrap_or("")
        .to_ascii_lowercase();

    // Boolean feature (no colon).
    if colon_pos.is_none() {
        return match feature.as_str() {
            "color" => true,
            "monochrome" => false,
            _ => true, // unknown boolean feature: true
        };
    }

    // Extract dimension value (px) and/or ident value from the value section.
    let px_value = value_toks.iter().find_map(|t| match t {
        CssToken::Dimension { value, unit } if unit.eq_ignore_ascii_case("px") => {
            value.parse::<f32>().ok()
        }
        CssToken::Number(n) => n.parse::<f32>().ok(),
        _ => None,
    });

    let ident_value = value_toks.iter().find_map(|t| {
        if let CssToken::Ident(s) = t {
            Some(s.as_str())
        } else {
            None
        }
    });

    match feature.as_str() {
        "max-width" | "max-device-width" => px_value.is_none_or(|v| w <= v),
        "min-width" | "min-device-width" => px_value.is_none_or(|v| w >= v),
        "max-height" | "max-device-height" => px_value.is_none_or(|v| h <= v),
        "min-height" | "min-device-height" => px_value.is_none_or(|v| h >= v),
        "width" => px_value.is_none_or(|v| (w - v).abs() < 1.0),
        "height" => px_value.is_none_or(|v| (h - v).abs() < 1.0),
        "orientation" => match ident_value {
            Some(s) if s.eq_ignore_ascii_case("landscape") => w > h,
            Some(s) if s.eq_ignore_ascii_case("portrait") => w <= h,
            _ => true,
        },
        // prefers-color-scheme: assume the user has no special scheme set
        // (i.e., light mode). "light" -> true; "dark" -> false.
        "prefers-color-scheme" => match ident_value {
            Some(s) if s.eq_ignore_ascii_case("light") | s.eq_ignore_ascii_case("no-preference") => true,
            Some(s) if s.eq_ignore_ascii_case("dark") => false,
            _ => true,
        },
        // prefers-reduced-motion: assume no-preference (motion not reduced).
        "prefers-reduced-motion" => match ident_value {
            Some(s) if s.eq_ignore_ascii_case("no-preference") => true,
            Some(s) if s.eq_ignore_ascii_case("reduce") => false,
            _ => true,
        },
        // prefers-contrast: assume no special contrast preference.
        "prefers-contrast" => {
            matches!(ident_value, Some(s) if s.eq_ignore_ascii_case("no-preference"))
        }
        // Color capability.
        "color" => true,
        "color-gamut" => match ident_value {
            Some(s) if s.eq_ignore_ascii_case("srgb") => true,
            _ => false, // p3, rec2020: we don't report those capabilities
        },
        // Display mode: we render as a plain browser tab.
        "display-mode" => matches!(ident_value, Some(s) if s.eq_ignore_ascii_case("browser")),
        // forced-colors, inverted-colors, pointer, hover, any-hover,
        // any-pointer, resolution, update, overflow-block, etc.:
        // unknown -- default to true (safe fallback: apply the rules).
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CssTokenizer;

    fn parse_prelude(s: &str) -> Vec<CssToken> {
        let mut tok = CssTokenizer::new();
        let mut v = tok.feed(s).unwrap_or_default();
        v.extend(tok.finish().unwrap_or_default());
        v
    }

    fn eval(s: &str, w: f32, h: f32) -> bool {
        evaluate_media_query(&parse_prelude(s), w, h)
    }

    #[test]
    fn media_type_screen() {
        assert!(eval("screen", 1280.0, 800.0));
    }

    #[test]
    fn media_type_all() {
        assert!(eval("all", 1280.0, 800.0));
    }

    #[test]
    fn media_type_print_is_false() {
        assert!(!eval("print", 1280.0, 800.0));
    }

    #[test]
    fn media_not_print_is_true() {
        assert!(eval("not print", 1280.0, 800.0));
    }

    #[test]
    fn media_not_screen_is_false() {
        assert!(!eval("not screen", 1280.0, 800.0));
    }

    #[test]
    fn media_max_width_below() {
        assert!(eval("(max-width: 768px)", 640.0, 480.0));
    }

    #[test]
    fn media_max_width_above() {
        assert!(!eval("(max-width: 768px)", 1280.0, 800.0));
    }

    #[test]
    fn media_min_width_above() {
        assert!(eval("(min-width: 768px)", 1280.0, 800.0));
    }

    #[test]
    fn media_min_width_below() {
        assert!(!eval("(min-width: 1400px)", 1280.0, 800.0));
    }

    #[test]
    fn media_screen_and_max_width() {
        assert!(eval("screen and (max-width: 1920px)", 1280.0, 800.0));
        assert!(!eval("screen and (max-width: 1024px)", 1280.0, 800.0));
    }

    #[test]
    fn media_orientation_landscape() {
        assert!(eval("(orientation: landscape)", 1280.0, 800.0));
        assert!(!eval("(orientation: portrait)", 1280.0, 800.0));
    }

    #[test]
    fn media_prefers_color_scheme() {
        assert!(eval("(prefers-color-scheme: light)", 1280.0, 800.0));
        assert!(!eval("(prefers-color-scheme: dark)", 1280.0, 800.0));
    }

    #[test]
    fn media_comma_list_any_match() {
        // print is false but screen is true; comma = OR.
        assert!(eval("print, screen", 1280.0, 800.0));
        assert!(!eval("print, tty", 1280.0, 800.0));
    }

    #[test]
    fn media_empty_prelude_is_true() {
        assert!(evaluate_media_query(&[], 1280.0, 800.0));
    }
}
