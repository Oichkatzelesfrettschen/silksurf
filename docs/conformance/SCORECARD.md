# silksurf Conformance Scorecard

> Aggregated dashboard of conformance harness results. Per-harness JSON
> sits alongside this file. Numbers update when a contributor runs
> `scripts/conformance_run.sh` and refreshes the JSON files.

## Last refresh

  * Date: 2026-05-14 (P9 release-infra wave; numbers re-pulled from
    `silksurf-js/conformance/test262-scorecard.json` and the in-repo
    copy at `docs/conformance/test262-scorecard.json`)
  * Baseline date: 2026-05-14
  * Reproducer: `scripts/conformance_run.sh`

## Harness summary

| Harness | Status | Coverage | Last result |
|---------|--------|----------|-------------|
| **test262** (lexer-only) | scaffolded | 157 of ~53 040 vendored .js files (numeric-literals subset) | 104 / 157 = 66.24 % at lexer level (2026-05-14 baseline). See `test262-scorecard.json` |
| **TLS loader sanity** (silksurf-tls) | functional | 4 unit tests covering empty PEM, malformed PEM, default-host loader, root-store diagnostics | 4 / 4 pass |
| **HTTP/2 (h2spec)** | scaffolded | external `h2spec` invocation flow documented; no in-tree HTTP/2 server harness yet | NOT YET MEASURED (P5.S3 -- needs local h2 server harness) |
| **HTML / CSS WPT** | DEFERRED | web-platform-tests not vendored | NOT YET MEASURED (P5.S2) |
| **TLS 1.3 RFC 8446 vectors** | DELEGATED | rustls owns protocol-level conformance; silksurf-tls only owns the loader / config / extra-CA surface | NOT YET MEASURED in-tree; relies on upstream rustls test suite. A first-party vector harness is queued (P5.S4 follow-on) |
| **OCSP stapling (RFC 6066)** | DEFERRED | not yet enforced in silksurf-net | NOT YET MEASURED (P5.S4) |
| **HSTS (RFC 6797)** | DEFERRED | not yet enforced in silksurf-net | NOT YET MEASURED (P5.S4) |

## Per-harness baseline (2026-05-14)

### test262 (JS tokeniser)

  * Source: `docs/conformance/test262-scorecard.json` (mirrored from
    `silksurf-js/conformance/test262-scorecard.json` after each run).
  * Subset: `language/literals/numeric` (157 files of ~53 040 in the
    vendored corpus).
  * Result: 104 passed / 53 failed / 0 skipped = 66.24 %.
  * Wall time: 0.031 s.
  * Runner kind: `lexer` (does NOT parse, compile, or evaluate).
  * Upgrade path: VM-based evaluation, queued as P5 + P7.

### HTML / CSS WPT

  * NOT YET MEASURED. WPT (web-platform-tests) is not vendored. Vendoring
    is queued for P5.S2; it requires a subset selection (URL + Fetch +
    Encoding first since they're bounded), a runner that mounts wpt via
    silksurf-engine, and per-test browser-state isolation.
  * Tracking: `silksurf-specification/SILKSURF-RUST-MIGRATION.md` (WPT row).

### h2spec

  * NOT YET MEASURED. We document how to invoke external `h2spec` against
    a future in-tree HTTP/2 server (`silksurf-net`), but the server side
    of that harness does not exist yet (queued P5.S3). Until it exists,
    we cannot generate a numeric scoreboard.
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
