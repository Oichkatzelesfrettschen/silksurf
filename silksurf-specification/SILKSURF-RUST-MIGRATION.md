# SilkSurf Rust Migration Plan + Implementation Status Map

> Updated 2026-04-30: expanded from a 70-line plan stub into the
> spec <-> implementation map. The migration phases (1-8) below are
> historical; all phases land in the workspace as of `main` =
> `ac00472`. The current debt-reconciliation roadmap is
> `docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md`. See
> `docs/REPO-LAYOUT.md` for the directory inventory.

## Goals (unchanged from original)

  * Single cleanroom browser engine in Rust: HTML5, CSS, JS,
    networking, TLS.
  * Cross-platform build with minimal OS-specific code and a tiny
    runtime footprint.
  * Keep research artifacts separate; implementation lives only in
    `crates/` and `silksurf-js/`.
  * Prioritise correctness and performance: no warnings, no leaks,
    measurable speedups (9.5 us steady-state at 50 nodes; see
    `docs/PERFORMANCE.md`).

## Cleanroom boundaries

  * Reference analysis stays in `diff-analysis/` (read-only, no code
    reuse). No `use diff_analysis::*` ever appears in production code.
  * Specs live in `silksurf-specification/` and must be updated before
    code changes (CLAUDE.md rule).
  * Production code lives in `crates/` (Rust) and `silksurf-js/` (JS
    engine). The `silksurf-extras/` directory is vendored reference
    only; not linked into the workspace.
  * AD-024 retired the legacy C tree. `src/`, `include/`, and
    `CMakeLists.txt` left the repository with it; git history holds them.

## Spec <-> implementation map

The columns below show, for each design document, which crate(s)
implement the design and what status that implementation is in.

| Spec document | Implementing crate(s) | Status | Notes |
|---------------|------------------------|--------|-------|
| `SILKSURF-BUILD-SYSTEM-DESIGN.md` | root `Cargo.toml`, `Makefile`, `scripts/local_gate.sh` | partial | Cargo and the local gate carry the whole build; AD-024 retired the CMake surface. Release-distribution work (cargo-dist) stays open. |
| `SILKSURF-C-CORE-DESIGN.md` | (retired) | superseded | AD-024 retired the C core. `crates/` is the sole implementation and git history holds the removed tree. |
| `SILKSURF-JS-DESIGN.md` (1945 lines) | `silksurf-js/` | superseded | AD-025 confirmed `boa_engine` as the runtime and removed the hand-written lexer, parser, bytecode compiler, register VM, NaN-boxing, and GC heap. `silksurf-js/src/boa_backend/` holds the host surface. The `test262_boa` runner records 99.81% of executed and 69.38% of the total suite. |
| `SILKSURF-NEURAL-INTEGRATION.md` | (none yet) | experimental | ADR-006 marked experimental; no production code. |
| `SILKSURF-XCB-GUI-DESIGN.md` (1019 lines) | `crates/silksurf-gui` | functional | 3,770 lines across two backends selected by feature. `winit-backend` carries the winit event loop and softbuffer presentation and is what `silksurf-app` links; `xcb-backend` carries the XCB connection and `PutImage` path AD-010 formalised. |
| HTML5 tokenizer + tree builder | `crates/silksurf-html` | functional | WHATWG happy path. Foreign content (SVG/MathML), table-related insertion modes, template tag pending. Conformance harness runs the WPT tree-construction corpus; see docs/conformance/SCORECARD.md. |
| CSS tokenizer + parser + cascade + computed values | `crates/silksurf-css` | functional | Hot path = 9.5us steady-state. SoA work on `ComputedStyle`, `Dimensions`, and `DisplayList` stays open. `tests/css_harness.rs` measures parse robustness over the upstream WPT CSS subset; cascade and computed-value conformance stay unmeasured. |
| DOM tree + traversal + interner + mutation tracking | `crates/silksurf-dom` | functional | Lock-free monotonic resolve table + generation-gated rebuild + persistent cache integration all landed (ADR-017 / ADR-018). |
| Layout + box model | `crates/silksurf-layout` | functional | Block + inline + flex basics. Position absolute/relative/fixed and CSS Grid pending. |
| Rasterization + display list | `crates/silksurf-render` | functional | Solid-color rectangles. Tile-parallel rasterization with rayon. Image decode, gradient, text rendering pending. NEON SIMD path is open. |
| Networking (HTTP/1.1 + HTTP/2 + persistent cache) | `crates/silksurf-net` | functional | HTTP/3 deferred (RFC 9114). Max-body-size cap and max-connections cap are open. |
| TLS adapter | `crates/silksurf-tls` | functional | rustls 0.23, TLS 1.2/1.3, optional platform verifier, runtime CA injection (`--tls-ca-file`). OCSP stapling and HSTS enforcement are open. ADR-019 formalises `tls-probe` as the supported diagnostic surface. |
| Pipeline orchestration | `crates/silksurf-engine` | functional | Two paths: 3-pass legacy + fused (FusedWorkspace, ADR-016). |
| User-facing CLI + GUI demo | `crates/silksurf-app` | partial | Headless render works end-to-end. GUI window-and-paint queued in roadmap P6. |
| Foundation (errors, atoms, arenas, span) | `crates/silksurf-core` | stable | `SilkError` canonical (ADR-020); `SilkInterner` with monotonic resolve-table support. |

## Build / test / bench baseline (current)

```sh
cargo build --workspace                          # ~2 min cold, seconds warm
cargo test --workspace                           # full suite
cargo run -p silksurf-engine --bin bench_pipeline # 9.5 us steady-state
cargo run --release --bin tls-probe -- chatgpt.com
scripts/local_gate.sh full                       # canonical merge gate
```

## Migration phase status (historical)

  1. Workspace setup, crate layout, CI, lint/format hooks. **Done.**
  2. Core data structures: DOM nodes, strings, arenas, interning. **Done.**
  3. HTML5 tokenizer/parser (cleanroom). **Done.**
  4. CSS tokenizer/parser + cascade + selector matching. **Done.**
  5. Layout + display list + raster backend. **Done** (block/inline/flex; absolute/grid pending).
  6. JS integration. **Done** through `boa_engine` per AD-025; the host surface, DOM bridge, and microtask pump live in `silksurf-js/src/boa_backend/`.
  7. Networking + TLS (rustls adapter). **Done** (HTTP/1.1 + HTTP/2; HTTP/3 deferred; OCSP + HSTS pending; see P5).
  8. Performance passes with benchmarks and regression guards. **Ongoing** (9.5us steady state achieved; SoA Phase-4.4 work is open; rolling-history NDJSON in perf/history.ndjson is open).

## Acceptance gates (current)

  * Zero warnings in Rust builds (`RUSTFLAGS='-D warnings'` enforced
    by `local_gate.sh full`).
  * `lint_unwrap.sh` enforces `// UNWRAP-OK: <invariant>` annotation
    above every `.unwrap()` / `.expect(` site.
  * `lint_unsafe.sh` enforces `// SAFETY: <invariant>` above every
    `unsafe { ... }` block; cross-crate index at
    `docs/design/UNSAFE-CONTRACTS.md`.
  * `cargo deny check advisories bans licenses sources` clean (one
    documented exception: RUSTSEC-2025-0134 rustls-pemfile
    unmaintained -- migration tracked).
  * MSRV = stable 1.94.1, pinned in `rust-toolchain.toml` and every
    `Cargo.toml` `rust-version`. Bump in lockstep (AD-008).
  * `cargo doc --workspace --no-deps --document-private-items` clean.

## Reference inputs (cleanroom only)

  * `silksurf-extras/Amaya-Editor` -- layout/editor behaviors
    (concepts only).
  * `silksurf-extras/boa` -- JS engine architecture patterns.
  * `silksurf-extras/servo` -- HTML/CSS/DOM/layout patterns.
  * `silksurf-js/test262` -- JS conformance tests (vendored).

## Related

  * `/CLAUDE.md` -- engineering standards (NO SHORTCUTS, specs first).
  * `/docs/design/ARCHITECTURE-DECISIONS.md` -- ADR record.
  * `/docs/REPO-LAYOUT.md` -- directory inventory.
  * `/docs/PERFORMANCE.md` -- bench reproducibility.
  * `docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md` --
    current debt-reconciliation roadmap.
