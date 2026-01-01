# Build Notes (Cross-Platform)

## Toolchains
- Rust nightly-2026-01-01 (`rustup show`).
- Linux: standard build tools for any native dependencies (GUI work will use X11).
- macOS/Windows: Rust-only crates build without extra native dependencies today.

## Common Commands
- `cargo build` (debug)
- `cargo build --release` (optimized)

## Build System Notes
- Cargo is the canonical build system for Rust crates.
- The root `Makefile` remains for legacy C targets and script wrappers; prefer
  Cargo or `scripts/*.sh` for Rust-only builds.
- There is no `justfile` today; keep task automation in Cargo, Make, or scripts.

## Riced Release Builds
Use the workspace profile `release-riced` for max throughput and allow larger
binaries for lower CPU cycles:
```
cargo build --profile release-riced
```
All profile blocks live in the workspace `Cargo.toml` (not per-crate).

For CPU-specific tuning, use `scripts/riced_build.sh`:
```
TARGET_CPU=native ./scripts/riced_build.sh -p silksurf-engine --bin bench_pipeline
```
On nightly, check `rustc -C help` and `rustc -Z help` for new codegen flags.

Strict warnings:
```
EXTRA_RUSTFLAGS="-D warnings" ./scripts/riced_build.sh -p silksurf-engine --bin bench_pipeline
```

Warnings-as-errors policy:
- Local builds and CI should use `-D warnings` for Rust crates.
- When a warning appears, either fix it or add a targeted lint allow with a
  justification comment.
- Current warning inventory lives in `docs/WARNINGS_AUDIT.md`.

## PGO + BOLT (Linux)
PGO: build an instrumented binary, run training, then rebuild with profiles:
```
./scripts/pgo_build.sh bench_pipeline
```

BOLT: requires `perf`, `perf2bolt`, and `llvm-bolt`:
```
./scripts/bolt_build.sh bench_pipeline
```

## Memory Target Notes
- Target RAM: <26 MB (stretch goal <10 MB).
- Biggest drivers today: DOM/tree + styles, layout tree, JS heap, and the
  RGBA framebuffer (`width * height * 4` bytes).

## Cross Compilation
```
rustup target add x86_64-unknown-linux-gnu
cargo build --release --target x86_64-unknown-linux-gnu
```

## Notes
- Keep feature flags minimal and deterministic for CI builds.
- When enabling optional allocators (e.g., `mimalloc` in `silksurf-js`), document
  the chosen allocator in PRs.
- Core dumps are collected into `logs/cores/` via `make core-dumps`; `make clean`
  resets the folder.

## Core Dump Routing (repo-local)
Linux `kernel.core_pattern` is system-wide, so we do not modify it for this
project. Instead:

- If `core.*` appears in the repo root, run `make core-dumps` to move them into
  `logs/cores/`.
- If your system uses `systemd-coredump`, you can export a single dump into the
  repo with `coredumpctl dump --output logs/cores/core.%e.%p`.
- Use `ulimit -c unlimited` (enable) or `ulimit -c 0` (disable) in your shell
  session to control whether cores are generated.
- Do not change `kernel.core_pattern` system-wide for this repo; keep routing
  repo-local via `make core-dumps` or `coredumpctl dump`.
