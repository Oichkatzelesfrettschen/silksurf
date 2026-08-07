# Local-Gate Reachability Across Hooks, Features, and Citations

**Date**: 2026-08-06
**Last verified**: 2026-08-06
**Evidence class**: observed tool behavior on this checkout (rank 1) plus script
and crate source (rank 4). The gate runs measured here are build and lint
evidence; no conformance, live-run, or GUI-frame claim rests on them.
**Mechanism**: AD-009 keeps cloud CI off, so `scripts/local_gate.sh full` --
reached as `make full` -- is the only automated gate a change passes. Two paths
carry it to a change: `scripts/install-git-hooks.sh` puts it in the pre-push
slot, and the Makefile's `check` target selects which crates and features the
compiler sees.
**Question**: PR #67 recorded its gates as not run, and PR #68 failed rustfmt at
four sites and clippy at three lints when the gate was finally run against it.
Two consecutive changes reached review ungated under a policy that has no second
gate. What made the gate skippable?

## Verdict

Three independent reachability failures, each one silent.

## The gate hook declines to install on an LFS checkout

`install_hook` in `scripts/install-git-hooks.sh` printed a warning and returned
0 when the hook slot already held a foreign script:

```sh
echo "WARN: .git/hooks/${name} exists and is not our script."
echo "      Use --force to overwrite, or install manually."
return
```

Git runs one script per hook name, and `git lfs install` writes `pre-push`,
`post-checkout`, `post-commit`, and `post-merge`. On 2026-08-06 this repository
tracked 155 LFS files under `diff-analysis/tools-output/`, `*.perf.data`, and
`*.zst`, so the slot the gate wants was the slot LFS occupied. All four LFS
hooks were present, `.git/hooks/pre-push` held the stock `git lfs pre-push`
dispatcher, and `scripts/hooks/pre-push` was installed nowhere. `make hooks`
reports success on that path, so the absence produced no output a developer
would read.

Reproduced in a clone: with LFS's script copied into the pre-push slot, the
previous installer prints the warning, exits 0, and leaves the gate hook
uninstalled.

The repair chains rather than replaces. An occupying hook moves to
`<name>.local` and the installed gate hook runs it first with arguments and
stdin intact -- git-lfs reads the ref updates it uploads from stdin, so ordering
the gate second costs the preserved hook nothing. A foreign hook arriving when
`<name>.local` is already taken exits 1 rather than choosing between them. The
verification replays arguments and stdin to the preserved hook, confirms the
gate runs after it, confirms a nonzero exit from the preserved hook aborts
before the gate, and confirms a second install is idempotent.

The occupant is gone on 2026-08-07: `docs/findings/git-lfs-payload-audit.md`
records the removal of every `filter=lfs` rule and of the LFS store, and the
four stock hooks went with it. A repository carrying no LFS object gains nothing
from chaining `git lfs pre-push`, so `--force` is the disposition here. The
chaining mechanism stays because it is what makes the installer correct on a
checkout that does hold a foreign hook.

Installed and confirmed by the falsifier the earlier state would have failed: a
staged text-hygiene violation drives `make check` through rustfmt, three clippy
configurations, six lints, and the artifact validators, `lint_text_hygiene`
rejects the planted emoji, `git commit` exits 1, and HEAD does not move.

## The gate does not typecheck the xcb backend

`crates/silksurf-gui` declares no default features, and `crates/silksurf-app`
depends on it with `default-features = false, features = ["winit-backend"]`.
Cargo therefore resolves `xcb-backend` off for `cargo clippy --workspace
--all-targets`, and `window.rs` and `event_loop.rs` never reach the compiler.

The decisive check: a deliberate type error appended to `window.rs` passes
`make check` at exit 0, and fails `cargo check -p silksurf-gui --features
xcb-backend` at exit 101. What sits outside the gate is the XCB connection, the
X11 event loop, and the `unsafe` reinterpretation of the ARGB `[u32]` frame as
`PutImage` wire bytes. Lints that select by file path rather than by compilation
-- `lint_unsafe.sh` scans `crates/silksurf-gui/src` directly -- still cover
those files, which is why the gap left no trace in lint output.

`check` gains a clippy step for that feature, on the same grounds as the fuzz
step above it: `fuzz/` declares its own `[workspace]`, so `--workspace` never
reaches it either. Both modules were already clean under `-D warnings`, so the
step closes a reachability hole rather than paying down accumulated warnings.

## A load-bearing target assumption cited a decision that does not make it

`XcbWindow::present` reinterprets the ARGB `[u32]` frame as bytes and hands them
to X11 `PutImage`, which reads them as little-endian BGRA. The doc comment
attributed that constraint to ADR-008. AD-008 records the stable-Rust migration
and the MSRV declaration; grepping the whole ledger for `x86_64`, `aarch64`, and
`little-endian` returns nothing, so no entry decides target architecture at all.
`scripts/cross_build.sh` is the authority that exists: `DEFAULT_TARGETS` is
`x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`, both little-endian.

Prose alone left the assumption unenforced. No `cfg(target_endian)` guard exists
anywhere in `crates/`, so a big-endian build compiled and presented every channel
reversed. A module-scope `compile_error!` under `cfg(target_endian = "big")` now
makes that a build failure; inverting the guard to `little` fails the build with
the intended message, which proves it reachable.

## The failures compose

Each defect hides the next. The hook gap means the gate never runs, so the
feature gap never surfaces; the feature gap means the compiler never reads
`window.rs`, so the endianness assumption is checked by nothing; and the
citation makes the unchecked assumption look decided. A reader following
ADR-008 to the ledger finds a decision about toolchains and stops.

## Second-order findings recorded elsewhere

The `phantom-deprecation-citation-repair` item in
`docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md` was marked LANDED while its own
stated gate failed: four `ADR-007` citations remained in
`silksurf-specification/SILKSURF-RUST-MIGRATION.md`, all naming the CMake legacy
harness that AD-024 retired. That file also credited the register VM, NaN-boxing,
and GC heap AD-025 removed, reported `crates/silksurf-gui` as a one-line
`lib.rs` against 3,770 lines, called two conformance harnesses open that now
run, and listed `CMake/CTest 16/16` as an acceptance gate -- a passing result
from a harness that cannot run. Those rows now name mechanisms that exist and
the roadmap gate holds.

Two classes stay open with counts and enumerating commands in the same roadmap
section: `adr-prefix-normalization` (20 citations across 11 live files spelling
`ADR-NNN` where the ledger spells `AD-NNN`, every number and anchor correct) and
`mechanism-comment-conversion` (60 comments across 22 live files opening with a
`WHY`, `WHAT`, or `HOW` label).

`cargo deny` reports duplicate `thiserror` 1.0.69 and 2.0.18 and duplicate
`thiserror-impl`. The warning is advisory, predates this review, and does not
fail the gate.

## Falsifiers

The conclusions change when any of the following holds.

- A checkout without git-lfs installed shows the gate hook absent anyway, which
  would mean the slot conflict is not the cause and something else declines the
  install.
- A deliberate error in `crates/silksurf-gui/src/event_loop.rs` fails `make
  check` after this change, which would mean the added step is redundant rather
  than closing a gap.
- An entry in `docs/design/ARCHITECTURE-DECISIONS.md` is found to decide target
  architecture, which would make the ADR-008 citation a prefix error rather than
  a phantom one.
- A big-endian target builds `silksurf-gui` with `--features xcb-backend` after
  this change, which would mean the guard is placed where the feature resolve
  cannot see it.

## Evidence commands

```sh
scripts/install-git-hooks.sh              # preserves an occupying hook as <name>.local
git lfs ls-files | wc -l                  # 155 on 2026-08-06, 0 after the payload removal
make check                                # now includes the xcb-backend clippy step
cargo check -p silksurf-gui --features xcb-backend
git grep -n 'ADR-007' -- '*.md' ':!docs/archive/**'
```
