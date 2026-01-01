# Rust Tooling and Support Crates

## Toolchain
- Pinned nightly via `rust-toolchain.toml` (edition 2024, rustc 1.94.0-nightly).
- Use `cargo +nightly-2026-01-01` if your shell overrides the toolchain.

## Performance and Analysis (cargo subcommands)
- `cargo-valgrind`: run Valgrind via Cargo.
- `flamegraph`: generate flamegraphs (cargo subcommand).
- `cargo-bloat`: find binary size hotspots.
- `cargo-llvm-lines`: report LLVM IR line counts for size/opt analysis.
- `cargo-asm`: inspect generated assembly for hot functions.
- `cargo-udeps`: detect unused dependencies.
- `cargo-nextest`: faster, parallel test runner.

## Concrete Commands (with expected outputs)
- `cargo valgrind run -p silksurf-engine --bin bench_pipeline`
  -> Valgrind summary in stdout, leak report at end.
- `cargo flamegraph -p silksurf-engine --bin bench_pipeline`
  -> `flamegraph.svg` in the workspace root.
- `cargo bloat -p silksurf-engine --bin bench_pipeline -n 20`
  -> top-20 symbols with size percentages.
- `cargo llvm-lines -p silksurf-engine --bin bench_pipeline -n 20`
  -> top-20 functions by LLVM IR lines.
- `cargo asm -p silksurf-js --lib silksurf_js::vm::Vm --rust`
  -> annotated assembly for hot code paths.
- `cargo nextest run -p silksurf-css`
  -> parallel test output with per-test timing.
- `cargo run -p silksurf-css --bin bench_selectors`
  -> selector match iterations + per-iter timing.
- `cargo run -p silksurf-css --bin bench_selectors -- --guard`
  -> lighter selector guardrail run (fewer iterations).
- `cargo run -p silksurf-css --bin bench_selectors -- --workload`
  -> extended selector mix for real-workload sampling.
- `cargo run -p silksurf-css --bin bench_cascade`
  -> cascade iterations + per-iter timing.
- `cargo run -p silksurf-css --bin bench_cascade_guard`
  -> lightweight cascade timing for guardrails.

## PGO + BOLT (Linux)
- `./scripts/pgo_build.sh bench_pipeline`
  -> writes `target/pgo/merged.profdata`, rebuilds with PGO data.
- `./scripts/bolt_build.sh bench_pipeline`
  -> writes `target/bolt/perf.data`, `target/bolt/perf.fdata`, and `.bolt` binary.
Note: keep PGO/BOLT out of CI unless the runners provide `llvm-profdata`,
`perf`, and `llvm-bolt` consistently.
The BOLT script enables `--emit-relocs` and frame pointers for better profiles.
Optional env vars:
- `EXTRA_RUSTFLAGS="-D warnings"` to fail on warnings.
- `PGO_WARN=1` to enable missing-function warnings during PGO use.
- `PERF_OPTS="-e cycles:u -j any,u"` for LBR (preferred). If unsupported, try
  `PERF2BOLT_OPTS="-nl"` to allow no-LBR mode (less effective).
- `BOLT_OPTS="..."` to override bolt passes.

Makefile wrappers:
- `make riced-build BIN=bench_pipeline`
- `make pgo-train BIN=bench_pipeline`
- `make bolt-opt BIN=bench_pipeline`
- `make perf-guardrails`

Perf guardrails (thresholds via env vars):
- `PIPELINE_US=15 SELECTORS_NS=200 CASCADE_US=30 make perf-guardrails`
- Optional RSS check: `MAX_RSS_KB=26000 make perf-guardrails`
Guardrails use `bench_cascade_guard` for cascade timing and run the compiled
`target/debug/bench_pipeline` for RSS measurement.

## Project Bench and Fuzz
- `cargo run -p silksurf-engine --bin bench_pipeline`
- `cargo run -p silksurf-engine --bin bench_js`
- `cargo run -p silksurf-css --bin bench_css`
- `cargo run -p silksurf-css --bin bench_selectors`
- `cargo run -p silksurf-css --bin bench_cascade`
- `cargo run -p silksurf-css --bin bench_cascade_guard`
- `cargo fuzz run html_tokenizer` (from `fuzz/`)
- `cargo fuzz run html_tree_builder` (from `fuzz/`)
- `cargo fuzz run css_tokenizer` (from `fuzz/`)
- `cargo fuzz run css_parser` (from `fuzz/`)
- `cargo fuzz run js_runtime` (from `fuzz/`)

## Perf/Flamegraph Examples
- `cargo flamegraph -p silksurf-engine --bin bench_pipeline`
- `perf record -g -- cargo run -p silksurf-engine --bin bench_pipeline`
- `perf report` for interactive call stacks.

## Valgrind Examples
- `cargo valgrind run -p silksurf-engine --bin bench_pipeline`

## Benchmarks
- `criterion`: statistics-driven micro-benchmarks.
- `iai-callgrind`: instruction-precise benchmarking via Callgrind.

## HTML/CSS/TLS Support (not full engines)
- `html5ever`: HTML5 tokenizer/parser.
- `cssparser`: CSS syntax parsing.
- `selectors`: CSS selector matching.
- `rustls`: TLS implementation.

These crates are optional; use only if they align with cleanroom goals.
