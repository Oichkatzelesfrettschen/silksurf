# Runbook -- benchmarks

> Reproducing the 9.5 us steady-state fused-pipeline result, the CSS
> microbenchmarks, the JS engine throughput, and the persistent baseline
> tracking. All paths are absolute relative to the repo root.

## One-shot fused pipeline (the 9.5 us number)

```sh
cargo run --release -p silksurf-engine --bin bench_pipeline
```

Output: cold cost breakdown per component (`table.rebuild`, cascade
candidate-collection, cascade matching, layout, display-list build) plus
the steady-state warm-cache mean over 1000 iterations.

The 9.5 us number is the warm-cache median at 50 nodes. Cold-cache cost
on a 1280x800 viewport is ~24 us. See `docs/PERFORMANCE.md` for the full
table including the 397-node ChatGPT.com page measurement.

## CSS microbenches

```sh
cargo run --release -p silksurf-css --bin bench_cascade        # cascade only
cargo run --release -p silksurf-css --bin bench_selectors      # selector matching
cargo run --release -p silksurf-css --bin bench_cascade_guard  # guard-mode (--guard)
cargo run --release -p silksurf-css --bin bench_css            # end-to-end
```

The `--guard` mode exits non-zero on regression vs `perf/baseline.json`.
Wired through `make perf-guardrails`.

## JS engine

```sh
cargo run --release -p silksurf-engine --bin bench_js
cargo bench -p silksurf-js                    # criterion: interner, lexer, vm
```

## Criterion benches (silksurf-js + silksurf-core)

```sh
cargo bench -p silksurf-js --bench interner
cargo bench -p silksurf-js --bench lexer_throughput
cargo bench -p silksurf-js --bench vm_throughput
cargo bench -p silksurf-core --bench interner
```

Output goes to `target/criterion/` with HTML reports.

## Baseline tracking

`perf/baseline.json` is the committed snapshot used by the guardrail
script. The schema is currently informal; a formal JSON schema lands in
the SNAZZY-WAFFLE roadmap P3.S2.

```sh
make perf-baselines              # regenerate baseline (overwrites)
make perf-guardrails             # check current vs baseline
perf/run_baselines.sh            # equivalent to `make perf-baselines`
```

A rolling-history file (`perf/history.ndjson`, append-only one row per
run with git SHA + timestamp + metrics) is planned in P3.S2 so trend
analysis is possible. Until then the baseline is a single-snapshot
pass/fail check.

## Local cron

For continuous tracking, install a personal cron:

```cron
# Append a perf data point every night at 03:00.
0 3 * * * cd /path/to/silksurf && /usr/bin/cargo run --release --quiet \
    -p silksurf-engine --bin bench_pipeline >> ~/.silksurf-bench.log 2>&1
```

The format will stabilize once the JSON schema lands; until then the log
contains human-readable fused-pipeline output.

## What can regress 9.5 us

In rough sensitivity order:

  1. **CascadeView SoA layout drift.** Bumping `CascadeEntry` past 40
     bytes pushes it across the cache line boundary; expect ~3-4 us
     regression at 397 nodes. Verify with `bench_pipeline` cold-cache.
  2. **Resolve-table eager build.** Re-introducing a per-call
     `interner.read().unwrap()` in the cascade hot path costs ~168 ns
     per cascade.
  3. **Allocator pressure in the cascade loop.** New `Vec::new()` or
     `FxHashMap::new()` in matching costs ~50-300 ns per call. The
     `lint_unwrap.sh` lint will catch any new unannotated unwraps that
     might mask such regressions.
  4. **mimalloc opt-out.** `silksurf-app/src/main.rs` and
     `silksurf-engine/src/bin/bench_pipeline.rs` set `#[global_allocator]
     = MiMalloc;`. Reverting to the system allocator on Linux costs
     ~5-10% on small alloc churn.

## Reproduction caveat

Benchmark numbers in `docs/PERFORMANCE.md` were taken on a specific
machine (recorded in the doc). Don't expect bit-exact reproduction; do
expect proportional results within ~10%. The guardrail threshold is
generous to avoid alerting on per-machine variance.
