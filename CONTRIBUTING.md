# Contributing to SilkSurf

Welcome. silksurf is a from-scratch Rust browser engine. This document covers
the policies and gates a contribution must clear before it can land on `main`.
For deeper engineering principles see [`CLAUDE.md`](CLAUDE.md) (the
no-shortcuts policy and CORE PRINCIPLES are non-negotiable).

## TL;DR

```sh
git clone <repo>
cd silksurf
rustup show                          # picks up rust-toolchain.toml (1.94.1)
cargo build --workspace              # ~1-2 min cold, seconds warm
cargo test --workspace               # full suite
scripts/install-git-hooks.sh         # wire pre-commit + pre-push hooks
scripts/local_gate.sh fast           # ~30s -- run before every commit
scripts/local_gate.sh full           # several minutes -- run before every push
```

## Local-gate is the merge gate

silksurf has a deliberate **strict-local-only CI policy**: cloud CI on push
and PR is intentionally disabled (see
[ADR-009](docs/design/ARCHITECTURE-DECISIONS.md#ad-009-strict-local-only-ci-policy)).
The local-gate script is what enforces merge readiness.

`scripts/install-git-hooks.sh` installs the two hooks that run the gate
automatically:

  * `pre-commit` -> `scripts/local_gate.sh fast` (rustfmt, clippy strict,
    lint_unwrap, lint_unsafe). Target: ~30s warm.
  * `pre-push` -> `scripts/local_gate.sh full` (the fast gate plus
    warnings-as-errors check, full test suite, cargo deny, MSRV verification,
    cargo doc, optional miri/fuzz, CMake/CTest).

Run them by hand any time. See
[`docs/development/LOCAL-GATE.md`](docs/development/LOCAL-GATE.md) for the
canonical reference, including the `MIRI=1` and `FUZZ=1` opt-in modes.

Do **not** use `git push --no-verify` or `git commit --no-verify` casually.
If you must bypass a hook, document why in the commit body.

## Branch and commit conventions

  * Short-lived feature branches: `feature/<topic>`, `fix/<topic>`,
    `docs/<topic>`. Rebase on top of `main`; do not let branches diverge
    significantly.
  * Conventional Commits: `feat:`, `fix:`, `perf:`, `refactor:`, `docs:`,
    `test:`, `chore:` ... Imperative mood, present tense, ~70-char subject
    line, then a blank line, then a body that explains WHY first, WHAT
    second, HOW third.
  * One topic per commit; one topic per PR.

## Code conventions

  * MSRV: `1.94.1` stable (see `rust-toolchain.toml` and
    `Cargo.toml` `workspace.package.rust-version`). Bump in lockstep across
    all per-crate `Cargo.toml` `rust-version` fields.
  * `rustfmt` is the spec. CI rejects unformatted code via the local-gate.
  * `clippy::correctness`, `suspicious`, `perf`, `complexity` are enforced
    as deny-level lints.
  * Every `unwrap`/`expect` must be annotated `// UNWRAP-OK: <invariant>`
    on a line within 3 above the call. Bare `unwrap()` is a bug. The
    `lint_unwrap` script enforces this.
  * Every `unsafe { ... }` block must be preceded within 5 lines by a
    `// SAFETY:` comment that explains the invariant. The `lint_unsafe`
    script enforces this. The full unsafe-block index lives at
    `docs/design/UNSAFE-CONTRACTS.md`.

## Documentation

  * Every architectural decision lands as an ADR in
    `docs/design/ARCHITECTURE-DECISIONS.md`.
  * Every new public type lands in `docs/reference/GLOSSARY.md` if its
    name is not self-documenting.
  * Every crate has `README.md`, `INSTALL.md`, `OPERATIONS.md` (work in
    progress; see roadmap P2.S1).

## Submitting changes

  1. Fork, branch from `main`, make focused commits.
  2. `scripts/local_gate.sh full` must pass. With `MIRI=1` if your change
     touches `unsafe`. With `FUZZ=1` if it touches a parser surface.
  3. Open a PR. Title = the conventional-commit subject of the change.
     Body = WHY then WHAT then HOW, plus a checklist confirming the gate
     ran locally.
  4. The maintainers will review for spec correctness, performance impact,
     and conformance to the no-shortcuts policy in `CLAUDE.md`.

## Reporting bugs and vulnerabilities

Functional bugs: open a GitHub issue with a minimal reproducer.

Security vulnerabilities: see [`SECURITY.md`](SECURITY.md). Do not file
public issues for unfixed exploitable bugs.
