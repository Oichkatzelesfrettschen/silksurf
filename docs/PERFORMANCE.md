# Performance Guide

This doc consolidates non-JS and JS performance guidance. JS-specific details
are expanded in `docs/JS_ENGINE.md`.

## Goals
- CPU: prioritize lowest cycles and cache misses, allow larger binaries.
- Memory: target <26 MB RSS, stretch goal <10 MB.
- Determinism: stable timings across builds; guardrails in CI/local runs.

## Hot Paths (Summary)
DOM/HTML:
- HTML tokenizer (`crates/silksurf-html/src/lib.rs`): delimiter-first scans,
  memchr fast paths, character reference decoding.
- Tree builder (`crates/silksurf-html/src/tree_builder.rs`): batch node creation.
- DOM mutation (`crates/silksurf-dom/src/lib.rs`): batching + dirty-node flush.

CSS:
- Selector parsing/matching (`crates/silksurf-css/src/selector.rs`,
  `crates/silksurf-css/src/matching.rs`).
- Cascade (`crates/silksurf-css/src/style.rs`): indexed rule buckets.

Layout/Render:
- Layout tree build (`crates/silksurf-layout/src/lib.rs`).
- Raster fill (`crates/silksurf-render/src/lib.rs`): SIMD row fill.

JS:
- Lexer/parser/VM/GC (see `docs/JS_ENGINE.md`).

## Plan (Current)
- Finish display-list text storage without clones (borrowed/small-string path).
- Tighten CSS/DOM incremental invalidation cost (batch dedup + minimal cascades).
- Extend SIMD/fixed-point passes through CSS/layout/render hot paths.
- Add RSS guardrails and refresh perf baselines after refactors.
- Run PGO training + BOLT no-LBR refresh and document results.
- Introduce riced build targets for LTO/PGO/BOLT in build tooling.

## Benchmarks and Guardrails
Core benchmarks:
- `cargo run -p silksurf-engine --bin bench_pipeline`
- `cargo run -p silksurf-css --bin bench_selectors -- --guard`
- `cargo run -p silksurf-css --bin bench_selectors -- --workload`
- `cargo run -p silksurf-css --bin bench_cascade_guard`

Guardrails:
- `make perf-guardrails` (thresholds via `PIPELINE_US`, `SELECTORS_NS`, `CASCADE_US`)
- Optional RSS check: `MAX_RSS_KB=26000 make perf-guardrails`

### Interner Microbenchmarks (local interners)
- `cargo bench -p silksurf-core --bench interner`
- `cargo bench -p silksurf-js --bench interner`

Representative medians from one local run:

| Crate | Scenario | Median |
|---|---|---:|
| `silksurf-core` | insert-heavy (10k unique keys) | `944 µs` |
| `silksurf-core` | resolve path (10k symbols) | `13.48 µs` |
| `silksurf-core` | repeated-key hit (100k hits) | `2.016 ms` |
| `silksurf-js` | insert-heavy (10k unique keys) | `1.445 ms` |
| `silksurf-js` | lookup `get` path (10k existing keys) | `305.6 µs` |
| `silksurf-js` | resolve path (10k symbols) | `9.700 µs` |
| `silksurf-js` | repeated-key hit (100k hits) | `2.685 ms` |

Notes:
- This establishes a baseline for the post-`lasso` local interners in both crates.
- No low-risk optimization was applied in this pass: no benchmark indicated a clear regression requiring code changes.
- Scope boundary: this benchmark/documentation pass adds no CI schedule-trigger changes.

## Optimization Tooling
- PGO: `./scripts/pgo_build.sh bench_pipeline`
- BOLT: `./scripts/bolt_build.sh bench_pipeline`
- `cargo flamegraph`, `cargo valgrind`, `cargo bloat`, `cargo llvm-lines`

## Notes
- Use `release-riced` profile for max throughput (see `Cargo.toml`).
- Keep profile-level changes in the workspace `Cargo.toml` only.
