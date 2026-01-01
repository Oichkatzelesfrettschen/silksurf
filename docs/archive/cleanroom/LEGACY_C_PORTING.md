# Legacy C Porting Inventory

## Overview
This inventory maps legacy C modules under `src/` and `include/` to their Rust
replacements in the workspace. C sources are not part of the Rust build and are
kept for cleanroom reference only.

## Status Legend
- implemented: ported with working code and basic tests.
- partial: core scaffolding exists, missing features or tests.
- pending: not ported yet.

## Document + DOM
- `src/document/html_tokenizer.c` -> `crates/silksurf-html` tokenizer (character
  refs, raw text, basic tokens) [implemented]
- `src/document/tree_builder.c` -> `crates/silksurf-html` tree builder (basic
  insertion modes, foster parenting) [implemented]
- `src/document/dom_node.c` -> `crates/silksurf-dom` nodes/attributes/comments/
  doctypes (TagName/AttributeName enums, `SmallString`, selective id/class
  interning) [implemented]
- `src/document/document.c` -> `crates/silksurf-engine` orchestration pipeline
  (parse → style → layout → render, JS hooks stubbed) [partial]
- `src/document/css_select_handler.c` -> `crates/silksurf-css` selector matching
  (`SelectorIdent` + interner fast paths) [implemented]
- `src/document/css_engine.c` -> `crates/silksurf-css` cascade/computed styles
  [partial]

## CSS
- `src/css/css_tokenizer.c` -> `crates/silksurf-css` tokenizer [implemented]
- `src/css/selector.c` -> `crates/silksurf-css` selector parsing (`SelectorIdent`
  identifiers) [implemented]
- `src/css/cascade.c` -> `crates/silksurf-css` cascade + computed values [partial]
- `src/css/fuzz_css.c` -> `fuzz/` targets (`css_tokenizer`) [partial]

## Layout + Rendering
- `src/layout/box_model.c` -> `crates/silksurf-layout` layout tree + block/inline
  (`SilkArena` allocations, arena-backed child lists, fixed-point inline width)
  [partial]
- `src/rendering/renderer.c` -> `crates/silksurf-render` display list + raster
  [partial]
- `src/rendering/paint.c` -> `crates/silksurf-render` paint primitives [pending]
- `src/rendering/pixel_ops.c` -> `crates/silksurf-render` SIMD pixel ops [pending]
- `src/rendering/damage_tracker.c` -> `crates/silksurf-render` damage tracking
  [pending]
- `src/rendering/pixmap_cache.c` -> `crates/silksurf-render` pixmap caching
  [pending]

## GUI + Platform
- `src/gui/main_gui.c` -> `crates/silksurf-gui` app/window integration [pending]
- `src/gui/xcb_wrapper.c`, `window.c` -> `crates/silksurf-gui` platform layer
  [pending]
- `src/gui/events.c`, `event_loop.c`, `event.c` -> `crates/silksurf-gui` event
  loop/input dispatch [pending]

## Memory + Core
- `src/memory/arena.c` -> `crates/silksurf-core` arena allocator
  (`SilkArena`/`bumpalo`) [implemented]
- `src/memory/refcount.c` -> `crates/silksurf-core` refcounting [pending]
- `src/memory/pool.c` -> `crates/silksurf-core` object pools [pending]

## JS Runtime + ABI
- `include/silksurf/js_engine.h` -> `silksurf-js` public header + FFI boundary
  (`silksurf-js/include/silksurf.h`) [partial]
- Legacy JS embedding (no C source in repo; cleanroom reference only) -> Rust-first
  `silksurf-js` runtime + `crates/silksurf-engine/src/js.rs` host bridge
  [implemented]

## Entry, Fuzz, and Neural
- `src/main.c` -> `crates/silksurf-app` CLI entry [partial]
- `src/fuzz_harness.c` -> `fuzz/` targets (`html_tokenizer`, `css_tokenizer`,
  `js_runtime`) [partial]
- `src/neural/bpe.c`, `src/neural/bpe_bench.c` -> new Rust crate or module
  (TBD) [pending]

## Notes
- JS engine is Rust-first in `silksurf-js`; no C equivalent in `src/`.
- Networking/TLS are Rust-first in `crates/silksurf-net` and `crates/silksurf-tls`.
