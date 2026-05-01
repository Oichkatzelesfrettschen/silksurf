# Toolchain and Build

## Toolchain Pin and MSRV
- Toolchain: `nightly-2026-04-05` (rustc 1.96.0-nightly).
- MSRV baseline: 1.94.0 (aligned with current dependency requirements).
- Install:
  - `rustup toolchain install nightly-2026-04-05`
  - `rustup component add rustfmt clippy llvm-tools-preview --toolchain nightly-2026-04-05`
- Use `cargo +nightly-2026-04-05` if your shell overrides the toolchain.

## Core Commands
- Build: `cargo build` / `cargo build --release`
- Run pipeline: `cargo run -p silksurf-app`
- Tests: `cargo test` (targeted: `cargo test -p silksurf-html`)

## Performance Tooling (Rust)
- `cargo flamegraph -p silksurf-engine --bin bench_pipeline`
- `cargo valgrind run -p silksurf-engine --bin bench_pipeline`
- `cargo bloat -p silksurf-engine --release`
- `cargo llvm-lines -p silksurf-engine`
- `cargo nextest run -p silksurf-css`

## PGO + BOLT (Linux)
- `./scripts/pgo_build.sh bench_pipeline`
- `./scripts/bolt_build.sh bench_pipeline`
- Optional env:
  - `PERF_OPTS="-e cycles:u -j any,u"`
  - `PERF2BOLT_OPTS="-nl"` (no-LBR fallback)
  - `EXTRA_RUSTFLAGS="-D warnings"`

## Warnings-as-Errors
Default policy: treat warnings as errors for Rust code.
- Local: `EXTRA_RUSTFLAGS="-D warnings" ./scripts/riced_build.sh -p silksurf-engine --bin bench_pipeline`
- Track known warnings in `docs/archive/testing/WARNINGS_AUDIT.md`.

## Legacy C Targets
Rust crates use Cargo; legacy C targets are in the root `Makefile` only.
- `cmake -B build && cmake --build build` (legacy C build)

## Core Dump Handling (Repo-Local)
We do not change system-wide `kernel.core_pattern`.
- If `core.*` appears in the repo, run `make core-dumps` to move it to `logs/cores/`.
- `make clean` resets the folder.
