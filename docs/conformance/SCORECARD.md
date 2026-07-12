# silksurf Conformance Scorecard

> Aggregated dashboard of conformance harness results. Per-harness JSON
> sits alongside this file. Numbers update when a contributor runs
> `scripts/conformance_run.sh` and refreshes the JSON files.

## Last refresh

  * Date: 2026-07-02 (synthetic WPT runner covers HTML, CSS cascade,
    layout rects, and paint-list invariants. test262 and h2spec rows remain
    at their prior baselines.)
  * Baseline date: 2026-07-01
  * Reproducer: `cargo run -p silksurf-engine --bin wpt_runner -- --verbose`

## Harness summary

| Harness | Status | Coverage | Last result |
|---------|--------|----------|-------------|
| **test262** (boa runner, full eval) | functional | 47 703 tests, scope language+built-ins+annexB; skips at that baseline: Intl (no ICU data, AD-021), ESM modules, $DONE async, FinalizationRegistry -- async and static ESM now execute (2026-07-11/12), pending a corpus re-run | 33 098 pass / 62 fail / 14 543 skip = **99.81 % of executed**, **69.38 % of total** (2026-05-17 baseline). Both denominators are load-bearing: the executed rate excludes 30.5 % of the suite. See `silksurf-js/conformance/test262-boa-scorecard.json` |
| **test262** (lexer-only, legacy VM) | runner removed (AD-025) | 157 of ~53 040 .js files (numeric-literals subset) | 104 / 157 = 66.24 % at lexer level (2026-05-14 baseline, historical). JSON retained: `test262-scorecard.json` |
| **TLS loader sanity** (silksurf-tls) | functional | 4 unit tests covering empty PEM, malformed PEM, default-host loader, root-store diagnostics | 4 / 4 pass |
| **HTTP/2 (h2spec)** | scaffolded | `scripts/run_h2spec.sh` driver + JSON scorecard schema; in-tree h2 server still pending | 0 / 0 (stub -- needs in-tree server or operator-supplied `SILKSURF_H2_HOST`). See `crates/silksurf-engine/conformance/h2spec-scorecard.json` |
| **HTML / CSS / Layout / Paint WPT (synthetic)** | functional | 63 in-tree fixtures exercising HTML structure, CSS selectors and properties, inline style cascade, Taffy layout rects, and fused paint-list suppression | 63 / 0 / 0 (pass / fail / skip), 100.00 % @ 2026-07-02 refresh. See `crates/silksurf-engine/conformance/wpt-scorecard.json` |
| **TLS 1.3 RFC 8446 vectors** | DELEGATED | rustls owns protocol-level conformance; silksurf-tls only owns the loader / config / extra-CA surface | NOT YET MEASURED in-tree; relies on upstream rustls test suite. A first-party vector harness is queued (P5.S4 follow-on) |
| **OCSP stapling (RFC 6066)** | DEFERRED | not yet enforced in silksurf-net | NOT YET MEASURED (P5.S4) |
| **HSTS (RFC 6797)** | DEFERRED | not yet enforced in silksurf-net | NOT YET MEASURED (P5.S4) |

## Per-harness baseline (2026-05-15)

### test262 (boa runner, full evaluation)

  * Source: `silksurf-js/conformance/test262-boa-scorecard.json`
    (2026-05-17 run, retained verbatim as evidence).
  * Scope: language + built-ins + annexB; 47 703 tests total,
    33 160 executed (pass + fail), 14 543 skipped.
  * Result, two denominators, both always quoted together:
    - executed: 33 098 / 33 160 = 99.81 %
    - total (incl. skips): 33 098 / 47 703 = 69.38 %
  * Skip classes: all `Intl.*` (no ICU data bundled, AD-021),
    FinalizationRegistry, and stale generated Unicode-property suites.
    NOTE (2026-07-11): the `$DONE` async lane now EXECUTES (state-recording
    `$DONE` + run_jobs microtask drain). NOTE (2026-07-12): static ESM
    module tests now EXECUTE too -- the harness runs as a script and the
    test as a module through a `SimpleModuleLoader` rooted at the test's
    directory (only dynamic-import/import.meta/top-level-await/JSON-modules
    stay skipped by feature flag); and a per-test loop-iteration budget adds
    a distinct `limit_exceeded` tally (a probable infinite loop is neither a
    hang nor a silent FAIL). The 2026-05-17 totals above PREDATE all three
    and still count async + ESM as skipped. A --full re-run (which moves
    async + static-ESM tests from skip to executed, expanding coverage and
    lowering `rate_executed` as newly-run tests fail) is reported NOT RUN:
    the test262 corpus is absent locally. Local evidence for the async lane:
    built-ins/Promise runs 633 pass / 0 fail / 6 skip (358 async-flagged,
    all formerly skipped); for ESM + budget: a minimal test262-shaped
    fixture runs 5 pass / 0 fail / 2 limit (script + module infinite loops).
  * The runner (`silksurf-js/src/bin/test262_boa.rs`) emits
    `rate_executed`/`pass_pct_executed` and `rate_total`/`pass_pct_total`
    in the scorecard JSON; runs predating the dual-denominator fields
    carry a single ambiguous `rate` field computed over executed tests
    only.
  * Regeneration requires the tc39/test262 corpus at
    `silksurf-js/test262/` (not vendored; fetch before running).

### test262 (JS tokeniser, legacy VM -- runner removed)

  * Source: `docs/conformance/test262-scorecard.json` (historical
    artifact; the lexer-only runner and the hand-written VM are removed
    per AD-025 and live in git history).
  * Subset: `language/literals/numeric` (157 files of ~53 040).
  * Result: 104 passed / 53 failed / 0 skipped = 66.24 %.
  * Runner kind: `lexer` (did NOT parse, compile, or evaluate).

### HTML / CSS / Layout / Paint WPT (synthetic)

  * 63 pass / 0 fail / 0 skip = 100.00 % across 63 in-tree fixtures.
  * Reproducer:
    ```sh
    cargo run -p silksurf-engine --bin wpt_runner -- --verbose
    ```
  * Source: `crates/silksurf-engine/conformance/wpt-scorecard.json`.
  * Fixture set: `crates/silksurf-engine/conformance/wpt/fixtures/`
    (63 self-contained HTML files; each has a registered check inside
    `wpt_runner.rs`).
  * Scope: synthetic WPT-style checks over the in-tree engine. The runner
    validates produced DOM, CSS selector/property behavior, inline style
    cascade ordering, flex layout rects, and fused display-list output. It does
    not execute JavaScript or vendor the upstream WPT corpus.
  * Paint coverage: `paint_visible_text_and_hidden_metadata.html` requires
    visible body text and background color to reach `DisplayItem` output while
    script, style, head metadata, and `display:none` body text stay out of the
    paint list.
  * Upgrade path: vendor a real WPT subset after the fixture runner grows URL,
    Fetch, Encoding, DOM mutation, and JavaScript execution hooks.

### h2spec

  * 0 / 0 stub. The driver script is in place; numbers will populate
    once either the in-tree HTTP/2 server lands (preferred) or an
    operator points the script at an external endpoint via
    `SILKSURF_H2_HOST` for toolchain validation.
  * Reproducer:
    ```sh
    scripts/run_h2spec.sh
    ```
  * Source: `crates/silksurf-engine/conformance/h2spec-scorecard.json`.
  * Runbook: `docs/development/RUNBOOK-H2SPEC.md` (install h2spec,
    pick a server target, interpret exit codes).
  * Tracking: `silksurf-specification/SILKSURF-RUST-MIGRATION.md`
    (HTTP/2 row).

### TLS conformance vectors

  * NOT YET MEASURED in-tree. silksurf-tls deliberately defers protocol
    conformance to rustls (delegation rationale documented in
    `docs/NETWORK_TLS.md`). A first-party RFC 8446 vector harness that
    re-exercises rustls through silksurf-tls's loader is queued behind
    OCSP / HSTS work.

## test262 scope

One runner exists: `test262_boa` (default build) parses, evaluates, and
checks negative expectations against boa_engine -- its numbers are real
language conformance, quoted with both denominators above.
`scripts/conformance_run.sh test262` drives it (TEST262_FULL=1 widens
scope; TEST262_PATH selects a corpus subdirectory). The former
lexer-only runner is removed with the hand-written VM (AD-025); its
2026-05-14 scorecard JSON remains as a historical artifact.

Any quoted test262 percentage names its denominator. "99.81 %" without
"of executed" is a misquote; the all-tests figure is 69.38 %.

Running a wider subset:

```sh
TEST262_PATH=language scripts/conformance_run.sh test262
TEST262_PATH=built-ins scripts/conformance_run.sh test262
TEST262_PATH=harness scripts/conformance_run.sh test262
```

Running everything (53 040 files, several minutes wall time):

```sh
TEST262_PATH= scripts/conformance_run.sh test262
```

## How harnesses get added

  1. Land the harness binary or the test source under the appropriate
     crate (e.g. `silksurf-js/src/bin/test262.rs` for test262).
  2. Add a `run_<harness>` function to `scripts/conformance_run.sh`.
  3. Update this file's "Harness summary" table.
  4. Update the harness's row in `silksurf-specification/SILKSURF-RUST-MIGRATION.md`.

## Why some harnesses are deferred

  * **WPT** -- the web-platform-tests corpus is large (~150 MB submoduled).
    Vendoring is queued for SNAZZY-WAFFLE P5.S2; it requires (a) a subset
    selection (probably URL + Fetch + Encoding first since they're bounded),
    (b) a runner that mounts wpt via the silksurf engine, and (c) browser-
    isolation for tests that mutate DOM state.
  * **HTTP/3** -- RFC 9114; not started. silksurf-net does not currently
    support HTTP/3 transport.
  * **WAI-ARIA / a11y** -- silksurf does not yet expose an accessibility
    tree; queued for P8.S5.
  * **CSS Color 4** -- only the basic color slice is implemented; full
    CSS-Color-4 conformance (Display P3, color()) is queued for P8.S2.

See `/.claude/plans/elucidate-and-build-out-snazzy-waffle.md` for the
complete debt-reconciliation roadmap.

## Related

  * `/silksurf-specification/SILKSURF-RUST-MIGRATION.md` -- spec to
    implementation map with status per crate.
  * `/docs/development/RUNBOOK-BENCH.md` -- bench (not conformance)
    reproducibility.
  * `/docs/design/THREAT-MODEL.md` -- security posture vs threats.
