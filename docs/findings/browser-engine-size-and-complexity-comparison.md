# Browser Engine Size and Complexity, and Where SilkSurf Sits

**Date**: 2026-08-07
**Last verified**: 2026-08-07
**Evidence class**: tool output over twelve upstream browser checkouts (rank 5,
static measurement) plus a `lizard` run over this workspace (rank 5). Static
counts bound source surface; they predict neither runtime cost nor conformance.
**Mechanism**: `tokei` counted lines per language and `lizard` computed
cyclomatic complexity per function across twelve browser codebases in
2025-12/2026-01. Those runs left 96.7 MiB of per-file JSON and per-function CSV
under `diff-analysis/tools-output/`, tracked through git-lfs. This finding is the
analysis those dumps existed to support; `docs/findings/git-lfs-payload-audit.md`
covers why the dumps themselves leave the repository.
**Question**: AGENTS.md holds touched Rust functions to cyclomatic complexity 16
or lower and calls for a low-resource browser profile. Where do those thresholds
sit against real browser engines, and where does SilkSurf sit?

## Verdict

The complexity gate is strict against the C-lineage browsers and roughly matches
the modern C++/Rust engines. The size target is met by three orders of
magnitude against Servo and one against the text browsers, which says more about
scope than about density.

## Source size

`tokei`, excluding its `Total` pseudo-language so each line is counted once:

| Project | Code lines | Files | Comments | Largest languages by code |
| --- | ---: | ---: | ---: | --- |
| servo | 7,221,477 | 107,169 | 404,602 | JSON 2783k, HTML 1474k, JavaScript 988k |
| ladybird | 858,560 | 17,274 | 314,167 | C++ 390k, C header 137k, JavaScript 136k |
| sciter | 596,818 | 1,648 | 40,096 | Bitbake 216k, SVG 172k, C header 50k |
| amaya | 557,599 | 1,298 | 106,086 | C 342k, C header 60k, HTML 33k |
| netsurf-main | 296,426 | 1,098 | 91,572 | C 249k, C header 19k, HTML 11k |
| neosurf-fork | 285,061 | 1,110 | 84,810 | C 235k, C header 30k, Bitbake 13k |
| elinks | 282,562 | 1,008 | 114,691 | PO file 136k, C 93k, HTML 16k |
| links-links2 | 271,147 | 206 | 6,270 | C 199k, Bitbake 58k, C header 10k |
| lynx 2.9.2 | 249,639 | 388 | 90,475 | C 123k, PO file 75k, C header 19k |
| w3m | 89,233 | 235 | 27,084 | C 55k, HTML 14k, C header 5k |
| tkhtml3 | 88,979 | 195 | 27,081 | C 49k, TCL 27k, HTML 7k |
| dillo | 82,758 | 454 | 19,750 | C++ 37k, C 20k, C header 9k |

Servo's total is dominated by vendored test data rather than engine code: JSON,
HTML, and JavaScript together outweigh its Rust. A line count over a checkout
measures the checkout, and only the language breakdown separates engine from
corpus. The same caution applies to sciter, whose largest bucket is Bitbake
recipes, and elinks and lynx, whose PO translation files exceed their C.

## Cyclomatic complexity

`lizard` per function, across the same checkouts. `ccn > 16` is the threshold
AGENTS.md applies to touched Rust:

| Project | Functions | Median CCN | p95 | Max | ccn > 16 | Share |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| ladybird | 38,601 | 1 | 9 | 473 | 837 | 2.2% |
| servo | 20,136 | 1 | 6 | 127 | 130 | 0.6% |
| amaya | 8,634 | 4 | 39 | 475 | 1,232 | 14.3% |
| netsurf-main | 7,190 | 3 | 17 | 317 | 361 | 5.0% |
| neosurf-fork | 7,120 | 2 | 18 | 317 | 413 | 5.8% |
| sciter | 3,851 | 2 | 13 | 743 | 122 | 3.2% |
| links-links2 | 3,729 | 4 | 26 | 171 | 365 | 9.8% |
| elinks | 3,487 | 3 | 20 | 164 | 255 | 7.3% |
| dillo | 3,249 | 2 | 15 | 128 | 134 | 4.1% |
| lynx 2.9.2 | 2,504 | 4 | 41 | 822 | 339 | 13.5% |
| w3m | 1,423 | 5 | 32 | 296 | 196 | 13.8% |
| tkhtml3 | 1,252 | 4 | 28 | 235 | 137 | 10.9% |

Two populations separate cleanly. The modern engines (ladybird, servo) sit at
median CCN 1 with p95 under 10 and put 0.6 to 2.2 percent of functions over 16.
The C-lineage browsers (amaya, lynx, w3m, tkhtml3, links) sit at median 4 to 5
with p95 26 to 41 and put 10 to 14 percent over 16.

## Where SilkSurf sits

`lizard -l rust crates silksurf-js/src` over this workspace:

| Functions | Median CCN | p95 | Max | ccn > 16 | Share |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 2,779 | 2 | 9 | 93 | 19 | 0.7% |

That places the distribution with the modern engines rather than the C-lineage
ones, and the AGENTS.md gate is what holds it there: the gate is not aspirational
against this codebase, it is roughly where ladybird and servo already are.

The nineteen functions over the threshold are the standing exceptions. The worst
five:

| CCN | Function | File |
| ---: | --- | --- |
| 93 | `apply_declaration` | `crates/silksurf-css/src/style.rs` |
| 37 | `feed` | `crates/silksurf-css/src/lib.rs` |
| 34 | `parse_meta` | `silksurf-js/src/bin/test262_boa.rs` |
| 28 | `layout_flex_container` | `crates/silksurf-layout/src/flex.rs` |
| 26 | `main` | `silksurf-js/src/bin/test262_boa.rs` |

`apply_declaration` at 93 is a property dispatch: one arm per CSS longhand, so
its complexity grows with `ComputedStyle`'s field count rather than with control
flow a reader must hold. That is the shape a table-driven dispatch would flatten,
and it is the single largest outlier in the workspace.

## What the static-analysis dumps contained

`semgrep` ran a generic ruleset over the same twelve checkouts. The result does
not support a memory-safety comparison: 211 of sciter's 211 findings and 201 of
amaya's 204 are `plaintext-http-link`, which flags `http://` URLs in text.
Servo's 45 errors are `run-shell-injection` in build tooling. Two projects that
differ by three orders of magnitude in unsafe surface differ by one finding here
(netsurf 1, links 0), because the ruleset never looked for the thing the
comparison wanted.

`infer/netsurf-cppcheck.txt` is cppcheck progress output over 23 files with no
findings recorded.

Both are named here so the absence is on the record rather than rediscovered.

## Falsifiers

- A `tokei` re-run over an engine-only subtree changes the ranking materially,
  which would mean the whole-checkout counts above measure vendored corpora more
  than engine source. The language breakdown already shows this for servo and
  sciter.
- `lizard` reports a different CCN for the same function under a newer version,
  which would mean the cross-project comparison mixes metric definitions.
- SilkSurf's `ccn > 16` share rises above the modern-engine band while the gate
  still passes, which would mean the gate covers touched functions only and
  untouched debt accumulates behind it.

## Evidence commands

```sh
lizard -l rust --csv crates silksurf-js/src
lizard -l rust -C 16 <touched paths>    # the gate AGENTS.md applies
```

The upstream checkouts are not vendored. The twelve `tokei` and `lizard` runs
came from `silksurf-extras/` reference clones in 2025-12 and 2026-01; the raw
per-file output left the repository with the git-lfs payload
(`docs/findings/git-lfs-payload-audit.md`), and these tables are what it
supported.
