# JS Engine Guide

This document consolidates JS runtime integration, performance notes, and
feature wiring for `silksurf-js`.

## Runtime Integration (Host Boundary)
Host interface lives in `crates/silksurf-engine/src/js.rs`:
- `JsRuntime::bind_dom(&Dom, NodeId)`
- `JsRuntime::evaluate(&str) -> JsValue`
- `JsRuntime::enqueue_task(JsTask)`
- `JsRuntime::run_microtasks()`

DOM mutations should batch changes and trigger incremental render via
`EnginePipeline::render_document_incremental_from_dom`.

## Module Boundaries
- `boa_backend/`: the production runtime. `SilkContext` wraps
  boa_engine and installs the browser host layer (DOM bridge in
  `boa_backend/dom_bridge.rs`, document/location/navigator, storage,
  crypto, fetch, timers, console).
- `bin/test262_boa.rs`: conformance runner (dual-denominator scorecard).
- The hand-written lexer/parser/bytecode/VM modules are removed per
  AD-025; git history and `SILKSURF-JS-DESIGN.md` preserve them.
- `gc/`: arena/heap + tracing hooks.
- `jit/`: optional Cranelift backend (`feature = "jit"`).
- `ffi/`, `napi/`, `wasm/`: host bindings.

## Feature Flags (Current)
Cfg-used: `cli`, `fast-alloc`, `jit`, `mmap`, `napi`, `tracing-full`, `wasm`.
Feature defaults are minimal; optional integrations are opt-in.

## Hot Paths (Summary)
- Lexer: byte scan loop (memchr).
- Parser: AST allocation pressure (Box-heavy).
- VM: opcode dispatch and nanbox conversions.
- GC: mark/sweep tracing and heap layout.

## Allocation Strategy
- Arena allocations for parse/compile stages (`bumpalo`).
- Intern identifiers via `lasso` for fast compares.
- Prefer zero-copy bytecode snapshots (`rkyv`, `zerocopy`) under `mmap`.

## Performance Roadmap (JS)
- Replace parser `Box` allocations with arena-backed nodes.
- Introduce packed bytecode instruction formats (bytemuck/zerocopy).
- Add GC mark-bitmaps (`bitvec`) and compact object headers.
- Add microbenchmarks (criterion/callgrind) for lexer/VM/GC.

## Plan (Current)
- Integrate `silksurf-js` behind `silksurf-engine` `js` feature flag.
- Draft the FFI boundary (minimal DOM bindings + task queue).
- Add a feature wiring doc (feature -> cfg -> code paths).

## Dependency Highlights
Hot-path crates: `memchr`, `bumpalo`, `lasso`, `bytemuck`, `rkyv`, `zerocopy`.
Optional integrations: `cranelift-*`, `napi`, `wasm-bindgen`, `mimalloc`.

## References
Detailed scans and audits live in `docs/archive/js/`.
