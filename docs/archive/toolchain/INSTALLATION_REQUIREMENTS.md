# Installation Requirements

This document lists toolchain and system dependencies for each module/crate.
Rust-only crates should build without additional native libraries unless noted.

## Global Toolchain
- Rust nightly pinned via `rust-toolchain.toml` (edition 2024).
  - `rustup toolchain install nightly-2026-01-01`
  - `rustup component add rustfmt clippy llvm-tools-preview --toolchain nightly-2026-01-01`
- MSRV baseline: 1.94.0 (aligned with current dependency requirements).
- `cargo` and standard build tools (bash, coreutils).
- `python3` for audit scripts and tooling.

Optional tooling:
- Perf: `perf`, `llvm-bolt`, `perf2bolt` (Linux only).
- Profiling: `cargo flamegraph`, `cargo-valgrind`, `iai-callgrind`.
- Fuzzing: `cargo fuzz` (Rust) and AFL++ (C harness).
- Guardrails: `/usr/bin/time` for RSS checks (Linux).

## Workspace Crates (Rust-Only by Default)

### crates/silksurf-core
- No external dependencies.

### crates/silksurf-dom
- No external dependencies.

### crates/silksurf-html
- No external dependencies.

### crates/silksurf-css
- No external dependencies.

### crates/silksurf-layout
- No external dependencies.

### crates/silksurf-render
- No external dependencies.

### crates/silksurf-engine
- No external dependencies.

### crates/silksurf-net
- No external dependencies (uses `rustls`).

### crates/silksurf-tls
- No external dependencies (uses `rustls`).

### crates/silksurf-gui
- No external dependencies (Rust-only stub today).

### crates/silksurf-app
- No external dependencies.

## silksurf-js (Optional Features)
Rust-only by default; optional features add runtime integrations:
- `--features jit`: Cranelift JIT (Rust-only).
- `--features fast-alloc`: `mimalloc` (bundled crate).
- `--features mmap`: `memmap2` (OS virtual memory).
- `--features napi`: Node.js required to build/run N-API bindings.
- `--features wasm`: wasm-bindgen toolchain for WebAssembly builds.
- `--features graphics`: `winit`/`pixels` may require X11/Wayland on Linux.
- `--features tui`: terminal UI dependencies only.

## Legacy C Targets (Makefile)
Building the legacy C GUI or fuzz harness requires:
- C toolchain (`gcc` or `clang`) and `cmake`.
- `pkg-config` and X11/XCB libs when using `make gui`:
  - `xcb`, `xcb-damage`, `xcb-composite`.

## Cleanroom Reference Repos
`silksurf-extras/*` are reference checkouts and not built as part of the
workspace. Treat them as read-only research inputs.
