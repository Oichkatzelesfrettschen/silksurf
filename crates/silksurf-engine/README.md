# silksurf-engine

The orchestration crate. Wires `silksurf-html` + `silksurf-css` +
`silksurf-dom` + `silksurf-layout` + `silksurf-render` + `silksurf-net`
+ `silksurf-js` into the rendering pipeline.

## Public API (high-level)

  * `EnginePipeline`, `RenderOutput`, `ParsedDocument` -- the top-level
    render flow (3-pass legacy path).
  * `parse_html`, `render_document`, `render` -- convenience wrappers.
  * `EngineError` -- crate-local error; `From<EngineError> for
    silksurf_core::SilkError` at the bottom of `lib.rs`.
  * `JsError`, `JsRuntime`, `JsTask`, `JsValue`, `NoopJsRuntime`
    (re-exports from `js` module).
  * `fused_pipeline` module -- the 9.5 us steady-state path:
    `FusedWorkspace`, `fused_style_layout_paint`,
    `fused_style_layout_paint_with_workspace`.
  * `speculative` module (gated on `net` feature) --
    `SpeculativeRenderer` with persistent on-disk response cache.

## Two pipelines

  * **3-pass (`EnginePipeline::render_document`)** -- the legacy path.
    Parse, cascade, layout, paint as separate passes with intermediate
    HashMaps. Easier to debug; ~24 us at 50 nodes.
  * **Fused (`fused_pipeline::*`)** -- the production path. One BFS
    walk that fuses cascade + layout + display-list emission, backed
    by a `FusedWorkspace` that retains all per-frame buffers. ~9.5 us
    at 50 nodes. See GLOSSARY -> FusedWorkspace, CascadeView.

## Bins

  * `bench_pipeline` -- the canonical fused-pipeline benchmark
    (`docs/development/RUNBOOK-BENCH.md`).
  * `bench_js` -- JS engine throughput.

## Status

Functional. The fused pipeline is the production path; the 3-pass path
is kept for parity testing and easier debugging. Three Phase-4.4 SoA
TODOs (`ComputedStyle`, `Dimensions`, `DisplayList`) are queued in
roadmap P4 and will further trim the steady state.
