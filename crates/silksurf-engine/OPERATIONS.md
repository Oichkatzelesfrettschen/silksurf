# silksurf-engine OPERATIONS

## Runtime tunables

| Environment variable | Default | Description |
|---|---|---|
| `RUST_LOG` | `warn` | Log filter for tracing subscriber. `RUST_LOG=silksurf=debug` enables pipeline spans. |
| `SILKSURF_NO_CACHE` | unset | Set to `1` to disable the persistent HTTP response cache for a run. |

## Common failure modes

### High cascade latency (> 50 us on 50-node pages)

1. Run `make perf-baselines` to capture current numbers.
2. Check `scripts/check_perf_regression.sh` for >= 5% regression against history.
3. Profile with `cargo flamegraph -p silksurf-engine --bin bench_pipeline`.
4. Common culprits: `CascadeEntry` size drift past 40 bytes (check with `size_of::<CascadeEntry>()`), selector specificity sort on every node, `ComputedStyle::default()` heap alloc reintroduced.

### Stale style after DOM mutation

1. Confirm `Dom::end_mutation_batch()` is called after the mutation sequence.
2. Check `dom.generation()` before and after -- if unchanged, batch did not flush.
3. `FusedWorkspace::run()` reads the generation counter and rebuilds only when changed.

### Layout boxes mispositioned

1. Enable `RUST_LOG=silksurf_layout=debug` to log per-node rect computation.
2. Verify `VIEWPORT` rect is set correctly (default 1280x800 in bench, actual window size in GUI mode).
3. Check that all `build_layout_tree` calls use the same `LayoutTree` instance as the fused pass.

### Display list empty (blank render)

1. Confirm `document` NodeId passed to `fused_style_layout_paint` is the root document node (not `NodeId(0)` which is reserved).
2. Verify CSS rules loaded: `stylesheet.rules.len()` should be > 0.
3. Check `display_items` on the `FusedResult`; if empty, the cascade is likely marking all nodes `display: none`.

## Key metrics

| Metric | Target | Source |
|---|---|---|
| `fused_pipeline_us` | 9.5 us (50 nodes, steady-state) | `bench_pipeline --emit json` |
| `full_render_us` | < 500 us (50 nodes) | `bench_pipeline --emit json` |

Run `make perf-baselines` to append current values to `perf/history.ndjson`.

## DoS bounds

| Bound | Value | Location |
|---|---|---|
| Max CSS rules per stylesheet | `MAX_CSS_RULES` (silksurf-css) | `crates/silksurf-css/src/lib.rs` |
| Max HTML tokens per feed | `MAX_TOKENS_PER_FEED` (silksurf-html) | `crates/silksurf-html/src/lib.rs` |
| Max JS call stack depth | `MAX_CALL_STACK_DEPTH` (silksurf-js) | `silksurf-js/src/vm/mod.rs` |
| JS yield loop bound | 2^20 steps | `silksurf-js/src/vm/generator.rs` |
