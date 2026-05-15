# Runbook -- h2spec HTTP/2 Conformance

> Operator guide for running `scripts/run_h2spec.sh` against a silksurf
> (or third-party) HTTP/2 server, refreshing
> `crates/silksurf-engine/conformance/h2spec-scorecard.json`, and
> updating the aggregated dashboard at `docs/conformance/SCORECARD.md`.

## WHY

HTTP/2 (RFC 9113) is a stateful binary protocol with HPACK header
compression, multiplexed streams, prioritised flow control, and
CONTINUATION sequencing. Hand-written tests cannot realistically cover
the spec; `h2spec` (https://github.com/summerwind/h2spec) is the
de-facto conformance suite. We treat it as an external oracle and feed
its summary into the same scoreboard schema as `test262` and the
synthetic WPT runner.

## WHAT

  * `scripts/run_h2spec.sh` -- invocation driver. Detects h2spec, optionally
    starts a local silksurf h2 server, runs the suite with a wall-clock
    cap, and parses the summary line into JSON.
  * `crates/silksurf-engine/conformance/h2spec-scorecard.json` -- machine-
    readable scorecard, schema:
        {total, pass, fail, skip, rate, timestamp, runner_version,
         runner_kind, h2_host, h2_port, raw_results, notes}
  * `crates/silksurf-engine/conformance/h2spec-results.txt` -- raw stdout
    capture from the most recent run (overwritten each run).
  * `docs/conformance/SCORECARD.md` -- dashboard; updated by hand from
    the scorecard JSON.

## HOW

### 1. Install h2spec

Pick whichever path matches your environment:

  * Arch Linux (AUR):
    ```sh
    yay -S h2spec
    ```
  * Go toolchain (works on any platform):
    ```sh
    go install github.com/summerwind/h2spec/cmd/h2spec@latest
    ```
  * Pre-built binaries:
    https://github.com/summerwind/h2spec/releases

Verify:

```sh
h2spec --version
```

### 2. Pick a server target

Two options.

#### Option A -- silksurf in-tree HTTP/2 server (preferred)

Tracking issue: SNAZZY-WAFFLE roadmap P5.S3.

Once the in-tree server lands as `cargo run -p silksurf-app --bin
silksurf-h2-server`, the script auto-spawns it. Until then, the script
exits 2 with a helpful pointer.

#### Option B -- external HTTP/2 server (sanity-check the toolchain only)

```sh
SILKSURF_H2_HOST=example.com SILKSURF_H2_PORT=443 \
    scripts/run_h2spec.sh
```

This validates that h2spec runs end-to-end and that the scoreboard JSON
schema is producible, but it does NOT measure silksurf's own HTTP/2
stack. Mark such runs in the scoreboard's `notes` field.

### 3. Run the script

From the repository root:

```sh
scripts/run_h2spec.sh
```

Optional environment knobs:

  * `SILKSURF_H2_HOST` -- default `localhost`; set to point at any
    HTTP/2 server you control.
  * `SILKSURF_H2_PORT` -- default `8443`.
  * `SILKSURF_H2_TIMEOUT` -- default `30` seconds; cap per-run wall
    clock so a hung server cannot stall CI.

Exit codes:

  * `0` -- run completed; scorecard emitted (regardless of pass rate).
  * `1` -- h2spec is not installed.
  * `2` -- no in-tree server and no `SILKSURF_H2_HOST` operator override.
  * `3` -- h2spec timed out or its summary line could not be parsed.

### 4. Update the dashboard

Open `docs/conformance/SCORECARD.md` and replace the h2spec row with the
freshly-minted numbers (the JSON file is the source of truth; the
markdown is a hand-curated summary).

### 5. Commit

The two files that change per run are:

  * `crates/silksurf-engine/conformance/h2spec-scorecard.json`
  * `crates/silksurf-engine/conformance/h2spec-results.txt`

Plus optionally:

  * `docs/conformance/SCORECARD.md`

Commit as `chore(conformance): refresh h2spec scorecard` so reviewers
recognise the run-output cadence.

## Common issues

  * `command -v h2spec` succeeds but the script reports it missing --
    check that h2spec is on the PATH for the same shell you ran the
    script from. Wrappers (asdf, mise, direnv) sometimes scope PATH
    additions per directory.
  * h2spec reports many `--- TIMEOUT` failures -- usually means the
    server is misconfigured (no h2 ALPN, wrong port). Curl the endpoint
    with `curl -v --http2 https://$HOST:$PORT/` to confirm h2 is
    advertised over ALPN before re-running.
  * `awk: command not found` on minimal containers -- install
    `gawk`/`busybox-awk`. The script uses `awk` to compute the pass
    rate as a float without dragging in `python3`.

## Related

  * `docs/conformance/SCORECARD.md` -- aggregated dashboard.
  * `crates/silksurf-net/` -- silksurf's HTTP/2 client (h2 server
    harness queued P5.S3).
  * `silksurf-specification/SILKSURF-RUST-MIGRATION.md` -- cleanroom
    migration tracker; h2spec row.
