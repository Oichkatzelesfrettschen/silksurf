# SilkSurf Rust Migration Plan

## Goals
- Single cleanroom browser engine in Rust: HTML5, CSS, JS, networking, TLS.
- Cross-platform build with minimal OS-specific code and a tiny runtime footprint.
- Keep research artifacts separate; implementation lives only in `crates/`.
- Prioritize correctness and performance: no warnings, no leaks, measurable speedups.

## Cleanroom Boundaries
- Reference analysis stays in `diff-analysis/` (read-only, no code reuse).
- Specs live in `silksurf-specification/` and must be updated before code changes.
- Production code lives in `crates/` (Rust) and `silksurf-js/` (JS engine).
- Legacy C is frozen; no new features or fixes in `src/` beyond migration needs.

## Reference Inputs (Cleanroom Only)
- `silksurf-extras/Amaya-Editor`: layout/editor behaviors (concepts only).
- `silksurf-extras/boa`: JS engine architecture patterns.
- `silksurf-extras/servo`: HTML/CSS/DOM/layout patterns.
- `silksurf-js/test262`: JS conformance tests.

## Target Workspace Layout (Root = Project Root)
- `Cargo.toml` (workspace)
- `crates/silksurf-app` (binary entrypoint)
- `crates/silksurf-engine` (orchestrator: pipeline + scheduling)
- `crates/silksurf-html` (HTML5 tokenizer/parser)
- `crates/silksurf-css` (CSS syntax, cascade, computed values)
- `crates/silksurf-dom` (DOM tree, node storage, traversal)
- `crates/silksurf-layout` (layout + box model)
- `crates/silksurf-render` (rasterization + display list)
- `crates/silksurf-gui` (windowing/event loop)
- `crates/silksurf-net` (fetch, caching, HTTP)
- `crates/silksurf-tls` (TLS adapter over rustls)
- `silksurf-js` (JS engine; integrated via Rust API, no C FFI)

## Build/Test/Bench Baseline
- `cargo build` / `cargo test` / `cargo bench` from the repo root.
- `cargo test -p silksurf-js` for JS engine only.
- `cargo run -p silksurf-app` for the binary.

## Rust Tooling Candidates (Verified on crates.io)
- Perf/size: `cargo-bloat`, `cargo-llvm-lines`, `cargo-asm`.
- Profiling: `flamegraph` (cargo subcommand).
- Memory: `cargo-valgrind`, `iai-callgrind`.
- Test orchestration: `cargo-nextest`.
- Benchmarks: `criterion`.

See `SILKSURF-DEPENDENCY-STRATEGY.md` for workspace dependency alignment.

## Parser/Support Crates (Optional, Not Full Engines)
- HTML: `html5ever` (HTML5 parser) or custom tokenizer + tree builder.
- CSS: `cssparser` (syntax), `selectors` (matching).
- TLS: `rustls` (TLS implementation).

These are support packages; use only if they do not violate cleanroom goals.
If we use them, we wrap their APIs behind SilkSurf-owned abstractions.

## Migration Phases
1. Workspace setup, crate layout, CI, lint/format hooks.
2. Core data structures: DOM nodes, strings, arenas, interning.
3. HTML5 tokenizer/parser (cleanroom).
4. CSS tokenizer/parser + cascade + selector matching.
5. Layout + display list + raster backend.
6. JS integration (silksurf-js API surface + bindings).
7. Networking + TLS (rustls adapter).
8. Performance passes with benchmarks and regression guards.

## Acceptance Gates
- Zero warnings in Rust builds; clippy clean at least on core crates.
- Deterministic tests for HTML/CSS/DOM.
- Benchmarks for parser throughput, layout time, and render timing.
