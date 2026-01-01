# Performance Baselines

Date: 2025-12-31 (local)

## Dev profile (RUSTFLAGS="-D warnings")
- bench_pipeline: iterations 200, total 2.016662ms, per-iter 10.083us, items 3
  - parse: total 458.15us, per-iter 2.29us
  - css: total 621.19us, per-iter 3.105us
  - style: total 455.241us, per-iter 2.276us
  - layout: total 125.82us, per-iter 629ns
  - render: total 356.261us, per-iter 1.781us
- bench_selectors: iterations 200000, total 10.45882ms, per-iter 52ns, matches 200000
- bench_cascade: iterations 5000, total 4.815890569s, per-iter 963.178us, styled nodes 130
- bench_cascade_guard: iterations 1000, total 10.349627ms, per-iter 10.349us, styled nodes 18

## Release-riced (PGO training run)
- bench_pipeline: total 1.320376ms, per-iter 6.601us, items 3
Note: PGO training runs are instrumented and not strictly comparable to release-only builds.

## Release-riced (BOLT run without LBR, -nl)
- bench_pipeline: total 1.035821ms, per-iter 5.179us, items 3
- perf2bolt ran in no-LBR mode with low samples and mismatch warnings; output is weak.

## LBR BOLT attempt (failed)
- perf record with `-j any,u` failed: PMU does not support branch stack sampling.
