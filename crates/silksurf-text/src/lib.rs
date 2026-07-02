/*
 * silksurf-text -- Unicode-aware text measurement and glyph rasterization.
 *
 * Layout needs text dimensions before taffy can resolve boxes. Rendering needs
 * glyph bitmaps for readable text. Layout uses deterministic metrics.
 * Glyph rasterization uses cosmic-text.
 *
 * cosmic-text owns FontSystem, Buffer, and SwashCache. Both public entry
 * points share one process-wide TextState behind LazyLock<Mutex<_>> so font
 * discovery occurs once per process.
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
