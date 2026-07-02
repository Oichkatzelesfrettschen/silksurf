/// Measure text dimensions in pixels.
///
/// Returns `(width, height)` where width is the longest line and height
/// covers all shaped lines. `max_width` constrains line wrapping; `None`
/// means no wrapping.
///
/// Uses deterministic font metrics so layout never scans system fonts.
pub fn measure_text(text: &str, font_size: f32, max_width: Option<f32>) -> (f32, f32) {
    if text.is_empty() {
        return (0.0, 0.0);
    }
    let line_height = font_size * 1.2;
    let max_width = max_width.filter(|width| width.is_finite() && *width > 0.0);
    let mut current_width = 0.0_f32;
    let mut widest_width = 0.0_f32;
    let mut line_count = 1_u32;

    for ch in text.chars() {
        if ch == '\n' {
            widest_width = widest_width.max(current_width);
            current_width = 0.0;
            line_count = line_count.saturating_add(1);
            continue;
        }
        let char_width = metric_advance(ch, font_size);
        if let Some(limit) = max_width
            && current_width > 0.0
            && current_width + char_width > limit
        {
            widest_width = widest_width.max(current_width);
            current_width = 0.0;
            line_count = line_count.saturating_add(1);
        }
        current_width += char_width;
    }

    widest_width = widest_width.max(current_width);
    (widest_width, line_height * line_count as f32)
}

fn metric_advance(ch: char, font_size: f32) -> f32 {
    if ch == ' ' {
        font_size * 0.33
    } else if ch.is_ascii() || is_combining_mark(ch) {
        if is_combining_mark(ch) {
            0.0
        } else {
            font_size * 0.55
        }
    } else if is_wide_cjk(ch) {
        font_size
    } else {
        font_size * 0.62
    }
}

fn is_combining_mark(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f
    )
}

fn is_wide_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x115f
            | 0x2329..=0x232a
            | 0x2e80..=0xa4cf
            | 0xac00..=0xd7a3
            | 0xf900..=0xfaff
            | 0xfe10..=0xfe19
            | 0xfe30..=0xfe6f
            | 0xff00..=0xff60
            | 0xffe0..=0xffe6
            | 0x1f300..=0x1faff
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= f32::EPSILON,
            "actual={actual}, expected={expected}"
        );
    }

    #[test]
    fn ascii_measurement_uses_deterministic_advances() {
        let (width, height) = measure_text("abc", 10.0, None);

        assert_close(width, 16.5);
        assert_close(height, 12.0);
    }

    #[test]
    fn ascii_measurement_wraps_to_width_limit() {
        let (width, height) = measure_text("abcd", 10.0, Some(12.0));

        assert_close(width, 11.0);
        assert_close(height, 24.0);
    }

    #[test]
    fn non_ascii_measurement_uses_metric_path() {
        let (width, height) = measure_text("Hi cafe\u{0301}", 10.0, None);

        assert!(width > 0.0);
        assert_close(height, 12.0);
    }
}
