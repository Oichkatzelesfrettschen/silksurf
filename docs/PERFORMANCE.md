# Performance Guide

This doc consolidates non-JS and JS performance guidance. JS-specific details
are expanded in `docs/JS_ENGINE.md`.

## Goals
- CPU: prioritize lowest cycles and cache misses, allow larger binaries.
- Memory: target <26 MB RSS, stretch goal <10 MB.
- Determinism: stable timings across builds; guardrails in CI/local runs.

## Fused Pipeline Results (2026-04-13, 50-node benchmark DOM, 13 CSS rules)

The fused pipeline (`FusedWorkspace::run()`) performs style cascade, layout, and
display-list construction in a single BFS pass. Steady-state re-render on an
unchanged DOM: **9.5us** (rebuild skipped via generation check).

| Metric | Value | Notes |
|--------|-------|-------|
| ws.run() cold (fresh DOM) | 11.3-11.6us | Includes table+view rebuild |
| ws.run() warm (same DOM) | ~9.5us | Rebuild skipped via generation check |
| 3-pass baseline | ~22us | compute_styles + layout + display list |
| Speedup vs 3-pass | 2.0x | |
| Per-node cost | ~190ns (600 cycles @ 3GHz) | Hash, match, cascade, layout |

### Architecture (SoA cascade path)

The cascade hot path operates entirely on the CascadeView SoA layout.
No `dom.node()` (168 bytes) or `dom.attributes()` calls during cascade.

| Component | Size | Cache lines | Purpose |
|-----------|------|------------|---------|
| Node (AoS, avoided) | 168B | 2.6 | Full DOM node with topology |
| CascadeEntry (SoA) | 40B | 0.6 | tag + id_index + class_start/count + parent_id |
| SelectorIdent | 32B | 0.5 | SmolStr + Option<Atom>, pre-constructed |
| ComputedStyle | 264B | 4.1 | Full computed style (stack alloc) |

Key optimizations applied (in dependency order):
1. **FusedWorkspace** -- single-object reusable scratch for all pipeline state
2. **LayoutNeighborTable** -- flat BFS-level decomposition, rebuild() reuses capacity
3. **CascadeWorkspace** -- bitvec seen (Fix D), workspace class_keys (Fix 2)
4. **SmolStr font_family** -- ComputedStyle::default() zero-heap-alloc (Fix 1)
5. **Pre-resolved class_strings** -- set_attribute populates SmallStrings (Fix 3)
6. **Fused tag+id+class** -- single dom.node() call per node (Fix F)
7. **Monotonic resolve table** -- lock-free Atom resolution, materialized at phase boundaries
8. **CascadeView SoA** -- 40-byte per-node entry, flat SelectorIdent array
9. **Zero-alloc matches_selector** -- reverse index arithmetic, no Vec allocation
10. **CascadeView in matching** -- tag/id/class/parent from SoA, not 168-byte Node
11. **Generation-gated rebuild** -- skip table+view rebuild on unchanged DOM
12. **Static FALLBACK** -- LazyLock<ComputedStyle> eliminates per-node default construction

### Phase boundaries (DOM lifecycle)

The DOM operates in strictly phased mode:
- **Parse phase**: TreeBuilder calls set_attribute (interner RwLock, write path)
- **Materialize**: into_dom() builds resolve_table + increments generation
- **Render phase**: cascade reads CascadeView + resolve_fast() (lock-free, read-only)
- **Mutate**: with_mutation_batch() allows new atoms via RwLock (cold path)
- **Re-materialize**: end_mutation_batch() extends resolve_table + increments generation

FusedWorkspace detects DOM changes via `Dom::generation()` (unique instance ID +
mutation counter). Same-DOM re-renders skip table.rebuild() + cascade_view.rebuild().

### Remaining cost breakdown (50 nodes, cold path)

| Component | Cost | % of total |
|-----------|------|-----------|
| table.rebuild() | ~1.0us | 9% |
| cascade_view.rebuild() | ~1.0us | 9% |
| Cascade (hash + match + apply) | ~7.5us | 65% |
| Layout math | ~1.5us | 13% |
| Display list push | ~0.5us | 4% |

The cascade algorithm (7.5us) is now instruction-bound, not memory-bound.
Further compression would require JIT compilation of CSS selectors.

## Hot Paths (Summary)
DOM/HTML:
- HTML tokenizer (`crates/silksurf-html/src/lib.rs`): delimiter-first scans,
  memchr fast paths, character reference decoding.
- Tree builder (`crates/silksurf-html/src/tree_builder.rs`): batch node creation.
- DOM mutation (`crates/silksurf-dom/src/lib.rs`): batching + dirty-node flush.

CSS:
- Cascade view (`crates/silksurf-css/src/cascade_view.rs`): SoA materialized view.
- Selector matching (`crates/silksurf-css/src/matching.rs`): CascadeView-accelerated.
- Cascade (`crates/silksurf-css/src/style.rs`): indexed rule buckets + bitvec dedup.

Layout/Render:
- Fused pipeline (`crates/silksurf-engine/src/fused_pipeline.rs`): single BFS pass.
- Raster fill (`crates/silksurf-render/src/lib.rs`): SIMD row fill.

JS:
- Lexer/parser/VM/GC (see `docs/JS_ENGINE.md`).

## Plan (Next)
- ChatGPT-scale benchmark (397 nodes) to validate L1 cache pressure predictions.
- HTTP/2 parallel fetch for first-render network latency reduction.
- In-process stylesheet cache (Arc<Stylesheet> keyed by CSS text hash).
- rkyv zero-copy stylesheet archive for cross-process cold start.
- SoA DOM conversion for layout pass (separate from cascade SoA).

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
