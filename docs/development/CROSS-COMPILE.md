# Cross-compile runbook

## Why
The XCB GUI is Linux/x86_64-only, but the rest of the workspace
(silksurf-core, silksurf-css, silksurf-engine, silksurf-js, silksurf-net,
...) must keep building cleanly on additional rustc target triples so we do
not silently regress portability. `scripts/cross_build.sh` is the canonical
smoke test for that.

## Prerequisites
- `rustup` and a stable toolchain (the workspace MSRV in `Cargo.toml`).
- For each non-host target you want to build natively:
  - `rustup target add <triple>`
  - matching system linker / sysroot if you are not using `cross`
    (e.g. `gcc-aarch64-linux-gnu` on Debian/Ubuntu, `aarch64-linux-gnu-gcc`
    on Arch via the `aarch64-linux-gnu-gcc` package).
- For containerized cross-compilation (recommended for non-native targets):
  - `cargo install cross`
  - Docker or Podman daemon reachable by your user
    (`docker info` or `podman info` succeeds without sudo).
- The script never installs anything on its own. If a prerequisite is
  missing it prints a hint and marks the target FAIL.

## How to run
From the repo root:

```
scripts/cross_build.sh
```

This builds the default targets:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

To build a custom set of targets, pass `--targets` followed by a
space-separated list of rustc target triples:

```
scripts/cross_build.sh --targets x86_64-unknown-linux-gnu \
                                 aarch64-unknown-linux-gnu \
                                 wasm32-unknown-unknown
```

The script auto-detects the host triple via `rustc -vV`. For any requested
target that differs from the host, if the `cross` tool is on PATH it is
used in place of `cargo build` (containerized sysroot, more reliable).
Otherwise the script falls back to plain `cargo build --target <triple>`.

Exit code is 0 iff every requested target built successfully; the final
summary lists `PASS  <triple>` / `FAIL  <triple>` for each one.

## How to add a new target
1. Pick the rustc triple (see `rustc --print target-list`).
2. Native path: `rustup target add <triple>` and install the matching
   linker/sysroot for your distro.
3. `cross` path: ensure Docker/Podman is running; no extra rustup step
   needed -- `cross` brings its own sysroot.
4. Smoke-test it:
   `scripts/cross_build.sh --targets <triple>`.
5. Once green, add the triple to `DEFAULT_TARGETS` in
   `scripts/cross_build.sh` so it is exercised by default.

## Known limitations
- The XCB GUI binary (`silksurf-gui` / XCB integration) is Linux/x86_64
  only. Cross-compiled binaries will not link xcb on non-Linux targets and
  may fail to link on Linux targets without an xcb sysroot. Today the GUI
  crate is gated by host platform; treat any GUI-target failure on a
  non-x86_64-linux triple as expected and out of scope for this script.
- `wasm32-unknown-unknown` and other no_std-ish targets currently surface
  workspace crates that pull in std-only deps (e.g. networking). They are
  not part of `DEFAULT_TARGETS` for this reason; opt in explicitly when
  you want to track that surface.
- `cross` requires a working container runtime; on hosts without
  Docker/Podman the script transparently falls back to `cargo`.
