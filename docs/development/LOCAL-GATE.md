# Local Gate -- silksurf merge-readiness reference

> Cloud CI is intentionally disabled for push/PR (see
> [ADR-009](../design/ARCHITECTURE-DECISIONS.md#ad-009-strict-local-only-ci-policy)).
> `scripts/local_gate.sh` is the merge gate. Wire it into your git flow with
> `scripts/install-git-hooks.sh` so it runs automatically on commit and push.

## Quick reference

```sh
scripts/install-git-hooks.sh        # one-time: symlink .git/hooks/{pre-commit,pre-push}
scripts/local_gate.sh fast          # ~30s -- pre-commit equivalent
scripts/local_gate.sh full          # several minutes -- pre-push equivalent
make gui-probe                     # live GUI probe; requires Wayland or X11
make gui-probe GUI_PROBE_ARGS="--release --backend x11 --max-input-ns 20000"
make gui-probe GUI_PROBE_ARGS="--release --backend x11 --max-any-input-ns 20000"
make gui-probe GUI_PROBE_ARGS="--release --backend x11 --max-buffer-ns 10000"
make gui-probe GUI_PROBE_ARGS="--release --backend x11 --max-any-buffer-ns 10000"
make gui-probe GUI_PROBE_ARGS="--release --backend x11 --max-render-ns 10000"
make gui-probe GUI_PROBE_ARGS="--release --backend x11 --runs 5"
make gui-probe GUI_PROBE_ARGS="--release --backend x11 --runs 5 --max-overhead-ns 10000"
MIRI=1 scripts/local_gate.sh full   # add miri smoke (~3-5 min)
FUZZ=1 scripts/local_gate.sh full   # add fuzz smoke (30s/target * 5 targets)
```

## What runs

### `fast` (pre-commit)

| Check                                                          | Why |
|----------------------------------------------------------------|-----|
| `cargo fmt --all -- --check`                                   | Prevent style drift (rustfmt is the spec) |
| `cargo clippy --workspace --all-targets -- -D clippy::{correctness,suspicious,perf,complexity}` | Catch bugs and obvious slop |
| `scripts/lint_unwrap.sh` (when present)                        | Every `unwrap`/`expect` site must be annotated `// UNWRAP-OK: <invariant>` (see ADR for error policy) |
| `scripts/lint_unsafe.sh` (when present)                        | Every `unsafe { ... }` block must be preceded within 5 lines by `// SAFETY:` (see `docs/design/UNSAFE-CONTRACTS.md`) |

Target wall time: under 30s on a warm `target/` cache.

### `full` (pre-push) -- includes `fast` plus

| Check                                                          | Why |
|----------------------------------------------------------------|-----|
| `RUSTFLAGS='-D warnings' cargo check --workspace --all-targets` | Warnings-as-errors gate |
| `cargo test --workspace`                                       | Run the full test suite |
| `cargo deny check advisories bans licenses sources`            | Supply-chain policy (`deny.toml`); skipped if cargo-deny missing |
| MSRV verification (`rustup show active-toolchain` + `cargo check --workspace --all-targets`) | Explicitly confirm the build still passes on the pinned MSRV (1.94.1; see ADR-008) |
| `cargo doc --workspace --no-deps --document-private-items`     | Catches missing/broken doc links and rustdoc warnings |
| (opt-in `MIRI=1`) `cargo +nightly miri test -p silksurf-core -p silksurf-css --lib` | UB and aliasing-rule check on the unsafe-heavy crates |
| (opt-in `FUZZ=1`) 30s/target on each of the 5 fuzz targets    | Cheap regression check that fuzzers still build and run |
| `cmake -B build && cmake --build build && ctest --test-dir build` | Legacy C/C++ harness (see ADR-007 for deprecation tracking) |

## Pre-flight requirements

```sh
rustup toolchain install 1.94.1 --component clippy --component rustfmt --component llvm-tools-preview
cargo install cargo-deny       # for `cargo deny check`
rustup toolchain install nightly --component miri  # only if you use MIRI=1
cargo install cargo-fuzz       # only if you use FUZZ=1
```

The `rust-toolchain.toml` at the repo root forces the right stable channel
automatically; you do not need to switch toolchains by hand.

## What "MSRV verification" actually means here

We pin both `rust-toolchain.toml` and `Cargo.toml` `workspace.package.
rust-version` to the same exact stable patch (`1.94.1`). That makes the
ordinary `cargo check` an MSRV check by construction: there is no
"developer-only" toolchain that masks an MSRV violation. The local-gate
prints `rustup show active-toolchain` so the active version is visible
in the gate output for forensics.

Bumping the MSRV is a two-line change:

  1. `rust-toolchain.toml`   `channel = "X.Y.Z"`
  2. `Cargo.toml`            `rust-version = "X.Y.Z"`
  3. (also bump every per-crate `Cargo.toml` `rust-version` field --
     `for f in crates/*/Cargo.toml silksurf-js/Cargo.toml; do
        sed -i 's/^rust-version = ".*"$/rust-version = "X.Y.Z"/' "$f"
      done`)

The two-line change should land in its own commit with an ADR amendment.

## Why the C/C++ build still runs

ADR-007 catalogues the legacy C/AFL++ harness under `src/`, `include/`,
`silksurf-extras/`, and `CMakeLists.txt`. Until the deprecate-or-integrate
decision lands, the local-gate keeps it green so accidental drift is caught
immediately.

## Why miri and fuzz are opt-in

Miri is slow (~3-5 minutes per pass) and the ten well-justified `unsafe`
blocks change rarely; running it on every push has bad cost-benefit. The
`MIRI=1` toggle is the right place to invoke it explicitly when touching
unsafe code or the resolve table.

Fuzz smoke (30s/target) is also opt-in via `FUZZ=1`. The point isn't to find
new crashes (that requires hours per target); it's to catch fuzzer build
breakage. Run it whenever you touch parser surface.

## Live GUI probe

`make gui-probe` runs `scripts/gui_probe.sh --release --backend auto`. The
probe opens the winit browser window, renders the default page, feeds address
input through the native event loop, waits for the final input frame to present,
and exits from inside the app. It is opt-in because it requires a live Wayland
or X11 session. Backend `auto` checks for a live `WAYLAND_DISPLAY` socket first
and uses X11 only when Wayland is absent. Explicit `--backend wayland` requires
the Wayland socket. Explicit `--backend x11` requires `DISPLAY`; `xvfb-run`
therefore acts as an X11 provider when no live desktop session exists. On
Wayland, presenter `auto` selects the SHM presenter.

`GUI_PROBE_ARGS` replaces the default argument list when a run needs a specific
backend, URL, presenter, or latency threshold.

Useful direct forms:

```sh
scripts/gui_probe.sh --release --backend auto --fixture ai-chat --probe page-input
scripts/gui_probe.sh --debug --backend wayland
scripts/gui_probe.sh --release --backend x11 https://example.com
scripts/gui_probe.sh --release --backend wayland --presenter shm
scripts/gui_probe.sh --release --backend wayland --presenter softbuffer
SILKSURF_TRACE_SHM_PHASES=1 scripts/gui_probe.sh --release --backend wayland --presenter shm
scripts/gui_probe.sh --release --backend auto --fixture ai-chat --probe page-input --runs 3 --max-total-ns 10000
scripts/gui_probe.sh --release --backend x11 --max-input-ns 20000
scripts/gui_probe.sh --release --backend x11 --max-any-input-ns 20000
scripts/gui_probe.sh --release --backend x11 --max-buffer-ns 10000
scripts/gui_probe.sh --release --backend x11 --max-any-buffer-ns 10000
scripts/gui_probe.sh --release --backend x11 --max-render-ns 10000
scripts/gui_probe.sh --release --backend x11 --runs 5
scripts/gui_probe.sh --release --backend x11 --runs 5 --max-overhead-ns 10000
make gui-probe-o0-ai-chat
```

`SILKSURF_TRACE_SHM_PHASES=1` adds Wayland SHM pump, buffer, seed, render,
attach/damage, flush, and preseed timings to the frame trace. Use it for
diagnosis only; the extra `Instant` calls perturb sub-0.01ms totals.
`--trace-app-frame` adds app-frame blit and chrome logs inside the measured
render callback. Use it for diagnosis only; the stderr write perturbs
sub-0.01ms totals.

## HTML/CSS source and conformance lanes

`make verify-conformance-sources` verifies the retained W3C and WHATWG source
bundle without network access. `make fetch-conformance-sources` refreshes the
same bundle with the recorded Mozilla user agent and rewrites `SHA256SUMS`.
`make fetch-conformance-test-corpora` fetches ignored external test corpora
under `silksurf-extras/` and records the fetched revisions there.

`make conformance-html-css` runs the HTML tokenizer harness, CSS corpus harness,
and synthetic WPT-style engine runner. The lane exercises the embedded
`html5ever` tree builder, the local CSS parser and cascade, Taffy-backed
layout, and fused paint-list checks. If `silksurf-extras/html5lib-tests` is
present, the html5lib harness reads that tokenizer corpus. If `CSS_TESTS_DIR`
is unset, the CSS harness runs the checked-in fixture corpus; set
`CSS_TESTS_DIR=/path/to/css-corpus` for a broader external sweep.

## Pre-commit/pre-push hook semantics

`scripts/install-git-hooks.sh` symlinks:

  - `.git/hooks/pre-commit` -> `scripts/hooks/pre-commit`  (runs `fast`)
  - `.git/hooks/pre-push`   -> `scripts/hooks/pre-push`    (runs `full`)

If a hook fails, the commit/push is rejected. To bypass once (rarely
appropriate; document the reason in the commit message):

```sh
git commit --no-verify -m "..."
git push --no-verify
```

`--no-verify` should be a flag in your CONTRIBUTING.md sentinel filter.
