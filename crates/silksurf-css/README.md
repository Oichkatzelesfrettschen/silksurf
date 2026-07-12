# silksurf-css

CSS tokenizer, parser, selector matching, cascade, and computed-style
machinery. The hot path of the render pipeline.

## Public API (high-level)

  * `parse_stylesheet`, `parse_stylesheet_with_interner` -- entry
    points returning a `Stylesheet`.
  * `Stylesheet`, `Rule`, `StyleRule`, `AtRule`, `Declaration`.
  * `Selector`, `SelectorList`, `SelectorIdent`, `Combinator`,
    `Specificity`, `matches_selector`, `matches_selector_list`,
    `selector_specificity`.
  * `ComputedStyle`, `Display`, `Position`, `Length`, `Color`, ...
    (the full computed-value vocabulary; see `style.rs`).
  * `CascadeWorkspace`, `StyleIndex`, `StyleCache` -- the per-frame
    scratch and the cascade cache.
  * `CascadeView`, `CascadeEntry` -- the SoA projection used by the
    fused pipeline (see GLOSSARY -> CascadeView).
  * `compute_style_for_node`, `compute_style_for_node_with_index`,
    `compute_style_for_node_with_workspace`, `compute_styles`.
  * `CssError` -- crate-local error; `From<CssError> for
    silksurf_core::SilkError` lives at the bottom of `lib.rs`.

## Hot-path shape

  1. `Stylesheet` parsed once and cached (see `silksurf-engine::
     SpeculativeRenderer`).
  2. `CascadeView::rebuild(dom)` builds a 40-byte-per-node SoA
     projection (cache-line aligned).
  3. `compute_styles(...)` walks the DOM in BFS order, populating
     `ComputedStyle` per node, with cascade workspace dedup via the
     `IndexedSelector.pair_id` bitvec.

The 9.5 us steady-state benchmark exercises this entire path (see
`docs/PERFORMANCE.md`).

## Bins (microbenches)

  * `bench_cascade`, `bench_selectors`, `bench_cascade_guard`,
    `bench_css` -- see `docs/development/RUNBOOK-BENCH.md`.

## Status

Functional, fast, and well-fuzzed (`fuzz/css_tokenizer`,
`fuzz/css_parser`). The speculative post-cascade SoA surfaces
(`StyleSoA`, `DimensionsSoA`, `DisplayListBatched`) are removed:
measured construction cost erased the fused-pipeline win, and
`CascadeView` already carries the column layout on the cascade hot
path.
