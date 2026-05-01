# silksurf Conformance Scorecard

> Aggregated dashboard of conformance harness results. Per-harness JSON
> sits alongside this file. Numbers update when a contributor runs
> `scripts/conformance_run.sh` and refreshes the JSON files.

## Last refresh

  * Date: 2026-04-30
  * Reproducer: `scripts/conformance_run.sh`

## Harness summary

| Harness | Status | Coverage | Last result |
|---------|--------|----------|-------------|
| **test262** (lexer-only) | scaffolded | 157 of ~53 040 vendored .js files (numeric-literals subset) | 104 / 157 = 66.2 % at lexer level. See `test262-scorecard.json` |
| **TLS loader sanity** (silksurf-tls) | functional | 4 unit tests covering empty PEM, malformed PEM, default-host loader, root-store diagnostics | 4 / 4 pass |
| **HTTP/2 (h2spec)** | scaffolded | external `h2spec` invocation flow documented; no in-tree HTTP/2 server harness yet | n/a (P5.S3 -- needs local h2 server harness) |
| **WPT** | DEFERRED | web-platform-tests not vendored | n/a (P5.S2) |
| **TLS 1.3 RFC 8446 vectors** | DELEGATED | rustls owns protocol-level conformance; silksurf-tls only owns the loader / config / extra-CA surface | rustls upstream test suite |
| **OCSP stapling (RFC 6066)** | DEFERRED | not yet enforced in silksurf-net | n/a (P5.S4) |
| **HSTS (RFC 6797)** | DEFERRED | not yet enforced in silksurf-net | n/a (P5.S4) |

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
