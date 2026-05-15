# silksurf Conformance Scorecard

> Aggregated dashboard of conformance harness results. Per-harness JSON
> sits alongside this file. Numbers update when a contributor runs
> `scripts/conformance_run.sh` and refreshes the JSON files.

## Last refresh

  * Date: 2026-05-15 (P5.S2 + P5.S3 wave; WPT synthetic harness +
    h2spec scaffold landed. test262 numbers unchanged from 2026-05-14.)
  * Baseline date: 2026-05-15
  * Reproducer: `scripts/conformance_run.sh`

## Harness summary

| Harness | Status | Coverage | Last result |
|---------|--------|----------|-------------|
| **test262** (lexer-only) | scaffolded | 157 of ~53 040 vendored .js files (numeric-literals subset) | 104 / 157 = 66.24 % at lexer level (2026-05-14 baseline). See `test262-scorecard.json` |
| **TLS loader sanity** (silksurf-tls) | functional | 4 unit tests covering empty PEM, malformed PEM, default-host loader, root-store diagnostics | 4 / 4 pass |
| **HTTP/2 (h2spec)** | scaffolded | `scripts/run_h2spec.sh` driver + JSON scorecard schema; in-tree h2 server still pending | 0 / 0 (stub -- needs in-tree server or operator-supplied `SILKSURF_H2_HOST`). See `crates/silksurf-engine/conformance/h2spec-scorecard.json` |
| **HTML / CSS WPT (synthetic)** | scaffolded | 16 in-tree fixtures exercising HTML structure, attributes, void elements, lists, tables, forms, scripts, anchors, entities, CSS class / id / type selectors | 9 / 7 / 0 (pass / fail / skip), 56.25 % @ 2026-05-15 baseline. See `crates/silksurf-engine/conformance/wpt-scorecard.json` |
| **TLS 1.3 RFC 8446 vectors** | DELEGATED | rustls owns protocol-level conformance; silksurf-tls only owns the loader / config / extra-CA surface | NOT YET MEASURED in-tree; relies on upstream rustls test suite. A first-party vector harness is queued (P5.S4 follow-on) |
| **OCSP stapling (RFC 6066)** | DEFERRED | not yet enforced in silksurf-net | NOT YET MEASURED (P5.S4) |
| **HSTS (RFC 6797)** | DEFERRED | not yet enforced in silksurf-net | NOT YET MEASURED (P5.S4) |

## Per-harness baseline (2026-05-15)

### test262 (JS tokeniser)

  * Source: `docs/conformance/test262-scorecard.json` (mirrored from
    `silksurf-js/conformance/test262-scorecard.json` after each run).
  * Subset: `language/literals/numeric` (157 files of ~53 040 in the
    vendored corpus).
  * Result: 104 passed / 53 failed / 0 skipped = 66.24 %.
  * Wall time: 0.031 s.
  * Runner kind: `lexer` (does NOT parse, compile, or evaluate).
  * Upgrade path: VM-based evaluation, queued as P5 + P7.

### HTML / CSS WPT (synthetic)

  * 9 pass / 7 fail / 0 skip = 56.25 % across 16 in-tree fixtures.
  * Reproducer:
    ```sh
    cargo run -p silksurf-engine --bin wpt_runner -- --verbose
    ```
  * Source: `crates/silksurf-engine/conformance/wpt-scorecard.json`.
  * Fixture set: `crates/silksurf-engine/conformance/wpt/fixtures/`
    (15+ self-contained HTML files; each has a hard-coded structural
    check inside `wpt_runner.rs`).
  * Scope: parser-only -- the runner does not lay out, paint, or run
    JavaScript. It validates the produced DOM and (for three CSS
    fixtures) selector matching against the parsed DOM.
  * Known failures expose real silksurf-html gaps tracked in
    `silksurf-specification/SILKSURF-RUST-MIGRATION.md`:
    void-element handling (`<br>`, `<hr>`, `<img>` open scope they
    should not), implicit body insertion when text appears inside
    `<title>`, and subsequent `</body>` misclosure. These are baseline
    failures, not test-runner bugs; they motivate the upgrade path
    toward a spec-fidelity tree builder.
  * Upgrade path: vendor a real WPT subset (URL + Fetch + Encoding
    first) once the engine boots through the rendering pipeline end-to-
    end; track in `silksurf-specification/SILKSURF-RUST-MIGRATION.md`.

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

The current runner is **lexer-only**. It validates that each test262
file lexes without a tokeniser error. It does NOT parse, compile, or
evaluate -- so the pass/fail counts reflect tokeniser conformance, not
language conformance.

Realistic numbers will only appear once the runner upgrades to full
VM-based evaluation (queued in SNAZZY-WAFFLE roadmap P7 + P5.S1
evaluation work). Until then, this baseline tracks tokeniser regressions.

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
