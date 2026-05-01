# SilkSurf Rust Migration Plan + Implementation Status Map

> Updated 2026-04-30: expanded from a 70-line plan stub into the
> spec ↔ implementation map. The migration phases (1-8) below are
> historical; all phases land in the workspace as of `main` =
> `ac00472`. The current debt-reconciliation roadmap is the
> SNAZZY-WAFFLE plan at `/.claude/plans/`. See `docs/REPO-LAYOUT.md`
> for the directory inventory.

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
  * Legacy C is frozen; no new features in `src/` beyond migration
    needs (see ADR-007).

## Spec ↔ implementation map

The columns below show, for each design document, which crate(s)
implement the design and what status that implementation is in.

| Spec document | Implementing crate(s) | Status | Notes |
|---------------|------------------------|--------|-------|
| `SILKSURF-BUILD-SYSTEM-DESIGN.md` | root `Cargo.toml`, `CMakeLists.txt`, `Makefile`, `scripts/local_gate.sh` | partial | Rust workspace fully implemented; CMake legacy harness present per ADR-007; release-distribution work (cargo-dist) queued in roadmap P9. |
| `SILKSURF-C-CORE-DESIGN.md` | `src/`, `include/`, CMake build | legacy | The C core was the original design; Rust workspace under `crates/` is the active implementation. Deprecate-or-integrate decision pending (ADR-007). |
| `SILKSURF-JS-DESIGN.md` (1945 lines) | `silksurf-js/` | partial | Lexer + parser + bytecode compiler + register VM + NaN-boxing + GC heap implemented. Missing: try/catch/finally opcodes, async/await execution, generators, Proxy/Reflect, WeakMap/WeakSet, Promise.all/race, fetch from JS. All queued in SNAZZY-WAFFLE roadmap P7. test262 conformance harness queued in P5.S1. |
| `SILKSURF-NEURAL-INTEGRATION.md` | (none yet) | experimental | ADR-006 marked experimental; no production code. |
| `SILKSURF-XCB-GUI-DESIGN.md` (1019 lines) | `crates/silksurf-gui` | stub | Currently a one-line lib.rs. ADR-010 formalises XCB-only Linux-first; implementation queued in roadmap P6. |
| HTML5 tokenizer + tree builder | `crates/silksurf-html` | functional | WHATWG happy path. Foreign content (SVG/MathML), table-related insertion modes, template tag pending. Conformance harness queued in P5.S2 (WPT subset). |
| CSS tokenizer + parser + cascade + computed values | `crates/silksurf-css` | functional | Hot path = 9.5us steady-state. Three Phase-4.4 SoA TODOs queued in P4 (`ComputedStyle`, `Dimensions`, `DisplayList`). Conformance harness queued in P5.S2. |
| DOM tree + traversal + interner + mutation tracking | `crates/silksurf-dom` | functional | Lock-free monotonic resolve table + generation-gated rebuild + persistent cache integration all landed (ADR-017 / ADR-018). |
| Layout + box model | `crates/silksurf-layout` | functional | Block + inline + flex basics. Position absolute/relative/fixed and CSS Grid pending. |
| Rasterization + display list | `crates/silksurf-render` | functional | Solid-color rectangles. Tile-parallel rasterization with rayon. Image decode, gradient, text rendering pending. NEON SIMD path queued in P8.S7. |
| Networking (HTTP/1.1 + HTTP/2 + persistent cache) | `crates/silksurf-net` | functional | HTTP/3 deferred (RFC 9114). Max-body-size cap and max-connections cap queued in P8.S8. |
| TLS adapter | `crates/silksurf-tls` | functional | rustls 0.23, TLS 1.2/1.3, optional platform verifier, runtime CA injection (`--tls-ca-file`). OCSP stapling and HSTS enforcement queued in P5.S4. ADR-019 formalises `tls-probe` as the supported diagnostic surface. |
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
  6. JS integration. **Partial** (lexer/parser/compiler/VM; control flow + microtasks + builtins pending; see P7).
  7. Networking + TLS (rustls adapter). **Done** (HTTP/1.1 + HTTP/2; HTTP/3 deferred; OCSP + HSTS pending; see P5).
  8. Performance passes with benchmarks and regression guards. **Ongoing** (9.5us steady state achieved; SoA Phase-4.4 work queued in P4; rolling-history NDJSON queued in P3.S2).

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
    `Cargo.toml` `rust-version`. Bump in lockstep (ADR-008).
  * `cargo doc --workspace --no-deps --document-private-items` clean.
  * CMake/CTest 16/16 (ADR-007 legacy harness preserved until
    deprecation decision lands).

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
  * `/.claude/plans/elucidate-and-build-out-snazzy-waffle.md` --
    current debt-reconciliation roadmap (SNAZZY-WAFFLE).
