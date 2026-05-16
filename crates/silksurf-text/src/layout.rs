use cosmic_text::{Attrs, Buffer, Metrics, Shaping};

use crate::TEXT_STATE;

/// Measure shaped text dimensions in pixels.
///
/// Returns `(width, height)` where width is the longest line and height
/// covers all shaped lines. `max_width` constrains line wrapping; `None`
/// means no wrapping.
///
/// Uses cosmic-text shaping (harfbuzz) for correct Unicode/BiDi measurement.
/// Shares the process-wide FontSystem via TEXT_STATE to avoid re-scanning
/// system fonts on every call.
pub fn measure_text(text: &str, font_size: f32, max_width: Option<f32>) -> (f32, f32) {
    if text.is_empty() {
        return (0.0, 0.0);
    }

    let mut state = TEXT_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let font_system = &mut state.font_system;

    let line_height = font_size * 1.2;
    let metrics = Metrics::new(font_size, line_height);
    let mut buffer = Buffer::new(font_system, metrics);
    buffer.set_size(max_width, None);
    buffer.set_text(text, &Attrs::new(), Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);

    let mut max_width = 0.0f32;
    let mut max_bottom = 0.0f32;
    for run in buffer.layout_runs() {
        max_width = max_width.max(run.line_w);
        max_bottom = max_bottom.max(run.line_top + run.line_height);
    }

    (max_width, max_bottom)
}
