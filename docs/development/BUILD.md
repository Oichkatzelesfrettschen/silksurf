# SilkSurf Build Guide

The build is a Cargo workspace fronted by the root `Makefile`. Every
gate target denies warnings (`RUSTFLAGS='-D warnings'`); this is project
policy, enforced in the Makefile rather than `.cargo/config.toml` so
IDEs stay usable.

## Quick start

```sh
git clone <repo> silksurf && cd silksurf
scripts/install-git-hooks.sh   # one-time: pre-commit/pre-push gates

make check    # fast gate: rustfmt, clippy -D warnings, lint scripts
make test     # workspace tests, warnings denied
make full     # check + test + cargo deny + rustdoc (the merge gate)

cargo run -p silksurf-app -- <url>    # run the browser
```

## Requirements

- Rust: `rust-toolchain.toml` pins the stable channel (also the MSRV;
  see AD-008). `rustup` installs it automatically on first build.
- Components: clippy, rustfmt (installed with the pinned toolchain).
- Optional tools:
  - `cargo install cargo-deny` -- supply-chain policy (`make full`)
  - `cargo install cargo-fuzz` + nightly -- fuzz smoke (`make fuzz`)
  - `rustup toolchain install nightly --component miri` -- UB check
    (`make miri`)
- GUI targets need a Wayland or X11 session plus XCB client libraries
  at runtime (AD-003/AD-010); the winit/softbuffer backend covers hosts
  without XCB.

The legacy C/CMake harness is removed (AD-024); git history preserves
it and `docs/LEGACY_C_PORTING.md` maps each C module to its owning
crate.

## Build modes

```sh
cargo build                        # debug
cargo build --release              # release
scripts/gui_probe.sh --o0          # dev-o0 profile via the GUI probe
```

## Optimized builds

```sh
make riced-build                   # target-cpu=native release build
make pgo-train BIN=bench_pipeline  # profile-guided optimization
make bolt-opt BIN=bench_pipeline   # post-link BOLT optimization
make cross                         # x86_64 + aarch64 cross smoke
```

Implementation details live in `scripts/riced_build.sh`,
`scripts/pgo_build.sh`, `scripts/bolt_build.sh`, and
`scripts/cross_build.sh`; reproducibility notes in
`docs/development/REPRODUCIBLE-BUILD.md`.

## Testing

```sh
make test                          # full workspace
cargo test -p silksurf-css         # one crate
cargo test -p silksurf-layout --release   # release-profile contracts
make fuzz                          # 30s cargo-fuzz smoke per target
make miri                          # miri smoke on core + css
make bench                         # criterion benchmark suite
scripts/conformance_run.sh         # conformance harness set
```

Fuzz targets and corpora live under `fuzz/` (five targets:
html_tokenizer, html_tree_builder, css_tokenizer, css_parser,
js_runtime). Testing strategy: `docs/TESTING.md`. Gate reference:
`docs/development/LOCAL-GATE.md`.

## Performance measurement

```sh
make perf-guardrails               # regression guardrails vs baseline
make perf-baselines                # refresh local baselines
scripts/check_perf_regression.sh   # compare last two history rows
```

Baselines and history: `perf/baseline.json`, `perf/history.ndjson`.

## Troubleshooting

- **Toolchain mismatch**: `rustup show active-toolchain` inside the repo
  must print the pinned version; `rust-toolchain.toml` forces it.
- **Slow builds**: `docs/development/SCCACHE.md` covers compiler
  caching; `cargo build --timings` locates the long poles.
- **GUI probe fails to start**: the probe needs a live display session;
  see `scripts/gui_probe.sh --help` for backend selection.
