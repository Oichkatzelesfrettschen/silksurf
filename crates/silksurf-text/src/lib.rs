/*
 * silksurf-text -- Unicode-aware text measurement and glyph rasterization.
 *
 * WHY: Layout needs accurate text dimensions (not a char-count heuristic) to
 * compute inline box widths correctly. Rendering needs actual glyph bitmaps
 * to produce readable text rather than colored rectangles.
 *
 * WHAT: Exposes two public entry points:
 *   measure_text   -- shaped text dimensions in pixels (width, height)
 *   rasterize_glyphs -- blits anti-aliased glyphs into a PixmapMut
 *
 * HOW: cosmic-text provides the full shaping+layout+rasterization stack via
 * FontSystem (font discovery), Buffer (shaped layout runs), and SwashCache
 * (glyph bitmap cache). Both entry points share a single process-wide
 * TextState wrapped in LazyLock<Mutex<_>> to amortize FontSystem init cost
 * (~1 s on first call; free on subsequent calls).
 *
 * Thread safety: TextState is protected by Mutex. Rayon tile workers must
 * not call measure_text or rasterize_glyphs in parallel -- call before
 * spawning tile workers (see rasterize_skia_into in silksurf-render).
 */

pub mod layout;
pub mod render;

pub use layout::measure_text;
pub use render::rasterize_glyphs;

use cosmic_text::{FontSystem, SwashCache};
use std::sync::{LazyLock, Mutex};

pub(crate) struct TextState {
    pub(crate) font_system: FontSystem,
    pub(crate) swash_cache: SwashCache,
}

pub(crate) static TEXT_STATE: LazyLock<Mutex<TextState>> = LazyLock::new(|| {
    Mutex::new(TextState {
        font_system: FontSystem::new(),
        swash_cache: SwashCache::new(),
    })
});
