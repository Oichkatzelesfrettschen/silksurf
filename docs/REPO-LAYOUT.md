# Repository Layout

> A new contributor opening the silksurf repo for the first time encounters
> a mix of Rust workspace, C/CMake legacy, formal models, fuzz infra,
> reference checkouts, and several conventions that are obvious only after
> reading the ADRs. This document is the map.

## Top-level directories

| Path | Purpose | Tracked? |
|------|---------|----------|
| `crates/` | Rust workspace member crates -- the active engine code | yes |
| `silksurf-js/` | Rust workspace member -- JS engine, intentionally a sibling of `crates/` (see "Why silksurf-js is outside crates/" below) | yes |
| `silksurf-specification/` | Living technical specifications; the source of truth for "what should the code do" (see CLAUDE.md) | yes |
| `silksurf-extras/` | Third-party reference checkouts (servo, etc.) for cleanroom comparison only -- NOT linked into the workspace | yes (vendored) |
| `diff-analysis/` | Cleanroom reference analysis surface; strict no-import-from policy from `crates/*/src` | yes (analysis only) |
| `docs/` | Human-readable documentation, runbooks, ADRs | yes |
| `scripts/` | Build, gate, and developer scripts (`local_gate.sh`, `install-git-hooks.sh`, lints, hooks) | yes |
| `fuzz/` | libfuzzer-sys harness crate (separate from workspace) | yes |
| `fuzz_corpus/` | Seed inputs for the legacy AFL++ harnesses (CMake-built `silksurf_fuzz` and `silksurf_css_fuzz`) | yes |
| `perf/` | Benchmark baselines and runner scripts | yes |
| `src/` | **Legacy C/C++ harness sources** built by `CMakeLists.txt`; pending the deprecate-or-integrate decision recorded in ADR-007 | yes |
| `include/` | C/C++ headers paired with `src/` | yes |
| `tests/` | Top-level CMake-driven CTest sources (16 tests) | yes |
| `target/`, `build/`, `build-asan/`, `build-fuzz/`, `build-webview/` | Build output trees; ignored | no |
| `fuzz_in/`, `fuzz_in_css/`, `fuzz_out*/`, `infer-out/`, `logs/`, `states/` | Tool output; ignored | no |
| `~/` (literal tilde) | Accidental artifact from a tool that did not expand `~` -- safe to delete | no (gitignored) |

## Top-level files

| File | Purpose |
|------|---------|
| `README.md` | Project overview |
| `CLAUDE.md` | Engineering standards, NO SHORTCUTS policy |
| `CONTRIBUTING.md` | Onboarding, gate discipline, hook setup |
| `CODE_OF_CONDUCT.md` | Contributor Covenant 2.1 |
| `SECURITY.md` | Security policy and reporting (canonical at root; `docs/SECURITY.md` is a redirect) |
| `LICENSE-MIT`, `LICENSE-APACHE` | Dual MIT / Apache-2.0 license |
| `.editorconfig` | Whitespace conventions |
| `Cargo.toml`, `Cargo.lock` | Rust workspace manifest + pinned dependency graph |
| `rust-toolchain.toml` | Pinned stable Rust 1.94.1 (see ADR-008) |
| `deny.toml` | cargo-deny supply-chain policy |
| `Makefile` | Convenience wrapper around CMake + Cargo (see ADR-007) |
| `CMakeLists.txt` | Legacy C/AFL++ harness (see ADR-007) |
| `BrowserLoader.tla`, `BrowserLoader.cfg` | TLA+ formal model of the browser-loader state machine -- precedent for the formal-models phase (P8.S12 in the SNAZZY-WAFFLE roadmap) |
| `BrowserLoader.old` | Earlier revision of the TLA+ model, kept for diff history |
| `lsan.supp` | LeakSanitizer suppressions used when building with `-DCMAKE_BUILD_TYPE=ASAN` |
| `generate_diffs.sh` | Diff-analysis helper (cleanroom comparison artefact) |

## Why silksurf-js is outside crates/

Convention: every workspace member that is part of the engine pipeline
lives under `crates/`. silksurf-js is the deliberate exception. It is the
JavaScript runtime -- a self-contained subsystem that compiles standalone
to FFI (libsilksurf-js.so) for embedding outside the engine, and it has
its own clippy.toml, rustfmt.toml, and feature-gate matrix (NaN-boxing
modes, GC backends, FFI dispatch). Putting it under `crates/` would
imply it is consumed *only* through the engine, which is not the
intent.

## Why CMakeLists.txt + Makefile + src/ + include/ + tests/

The repo predates the all-Rust pivot. The C harness is currently kept
green by the local-gate full pass (`scripts/local_gate.sh full`) so the
deprecate-or-integrate decision (ADR-007) can be made without surprise
breakage. New work should not extend the C harness.

## Conventions a new contributor must know

  * **Hook-installed by default.** Run `scripts/install-git-hooks.sh`
    once. From then on every commit and push runs the appropriate
    local-gate pass.
  * **Cloud CI is intentionally off** for push and PR (ADR-009). The
    only authoritative gate is local. Expect no merge-blocking signal
    from GitHub Actions.
  * **MSRV moves in lockstep.** `rust-toolchain.toml` and
    `Cargo.toml` `workspace.package.rust-version` and every per-crate
    `Cargo.toml` `rust-version` must all match the same exact patch
    version. The bump is a single commit (see ADR-008).
  * **Errors funnel through `silksurf_core::SilkError`** at workspace
    boundaries; per-crate types stay private to their crate's API.
  * **`unsafe` and `unwrap` carry annotations.**
    `// SAFETY: <invariant>` (within 7 lines above) and
    `// UNWRAP-OK: <invariant>` (same window). Lints in the gate enforce.
  * **Specs precede code.** New behavior lands in
    `silksurf-specification/` before it lands in `crates/*/src`.
  * **No imports from `diff-analysis/`** in production code -- it is
    cleanroom reference, not a dependency.
