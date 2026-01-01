# Engine Performance Roadmap (DOM/CSS/Layout/Render)

This roadmap complements `docs/JS_ENGINE_PERF_ROADMAP.md` and tracks
non-JS engine performance work. Items are cleanroom and task-oriented.

## Phase 1: Identifiers + Matching (done)
- DONE: Tag/attribute enums + small-string storage in DOM.
- DONE: Selective interning for id/class values in DOM.
- DONE: Selector identifiers carry optional atoms for fast compare.
- DONE: Selector parsing + stylesheet parsing support interner reuse.

## Phase 2: Selector + Cascade Indexing
- DONE: Pre-index rules by tag/id/class to avoid full rule scans.
- DONE: Selector specificity cached; rule application iterates in rule order.
- DONE: StyleCache wired into engine; incremental invalidation hooked to DOM
  mutations with batching support.

## Phase 3: Layout Allocation + Math
- DONE: Layout nodes and child lists arena-allocated.
- DONE: Fixed-point width calculation for inline text.
- DONE: Expanded fixed-point usage for margins/padding/border and cursor/box metrics.

## Phase 4: Render Hot Paths
- DONE: Tile display list by damage region.
- DONE: SIMD row fill for solid colors (SSE2 fast path + scalar fallback).
- IN PROGRESS: Avoid text clones in display list; target borrowed or small-string
  storage for text items.

## Phase 5: Pipeline Benchmarks
- DONE: End-to-end `bench_pipeline` reports parse/style/layout/render timing.
- DONE: Micro-bench for selector matching + cascade cost.
- DONE: Perf guardrail script for timing/RSS thresholds, selector guard mode.

## Phase 6: HTML Tokenizer Hot Paths
- DONE: Delimiter-first scanning for tag/attr names + attribute value parsing.
- DONE: Memchr-driven raw-text scanning and character reference decode fast path.
