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

## Low-Latency Evidence Ladder
- Complexity: `/home/eirikr/.local/bin/lizard -l rust -C 16 <touched-rust-files>`
  is the gate for every touched Rust path.
- Source shape: `rust-analyzer`, `rg`, `fd`, `cargo tree`, `cargo machete`,
  `scc`, and `cloc` explain code and dependency surface before profiling.
- Legacy C/XCB shape: `cflow`, `cscope`, `global`, `ctags`, and `readtags`
  map C call and symbol flow. They do not prove Rust call graphs.
- Binary size: `cargo bloat` and `cargo llvm-lines` identify codegen and
  monomorphization pressure.
- Command timing: `hyperfine` measures repeatable CLI paths. GUI address input
  uses `scripts/gui_probe.sh` trace metrics instead.
- Runtime timing: `perf stat`, `perf record`, `cargo flamegraph`, `hotspot`,
  `sysprof-cli`, `uftrace`, and `valgrind --tool=callgrind` localize CPU,
  indirect-call, branch, cache, and scheduler cost.
- Allocation and RSS: `heaptrack` and Valgrind explain heap growth and
  allocator churn when built-in counters do not isolate it.
- Syscall and compositor waits: `strace`, `ltrace`, `bpftrace`,
  `wayland-info`, `wev`, `xprop`, `xwininfo`, `Xvfb`, and `xvfb-run`
  separate app work from display-backend behavior.
- Host counters: `likwid-topology` and `likwid-perfctr` ground cache,
  bandwidth, and CPU-topology claims.

## Local Tool Inventory (2026-06-30)

The current workstation exposes the full low-latency evidence ladder:

| Surface | Available tools |
|---------|-----------------|
| Source and metrics | `rg`, `fd`, `rust-analyzer`, `lizard`, `scc`, `cloc` |
| Rust dependency and size pressure | `cargo tree`, `cargo machete`, `cargo udeps`, `cargo deny`, `cargo bloat`, `cargo llvm-lines` |
| C and legacy symbol flow | `cflow`, `ctags`, `readtags`, `gtags`, `global` |
| Repeatable timing | `hyperfine`, `scripts/gui_probe.sh` |
| CPU and timeline profiling | `perf`, `sysprof-cli`, `uftrace`, `valgrind`, `flamegraph` |
| Allocation and syscall evidence | `heaptrack`, `strace`, `ltrace`, `bpftrace` |
| Display backend evidence | `wayland-info`, `wev`, `xprop`, `xwininfo`, `Xvfb`, `xvfb-run` |
| Host counter evidence | `likwid-topology`, `likwid-perfctr` |

`rg` resolves through the Codex vendor path in this shell. Use `/usr/bin/rg`
when package ownership or system-path evidence matters.

`scripts/gui_probe.sh --backend auto` treats a live Wayland socket as the first
display truth surface and falls back to `DISPLAY` only when Wayland is absent.
`xvfb-run` supplies `DISPLAY` for X11-only probe runs; it does not replace a
live Wayland evidence run.

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
