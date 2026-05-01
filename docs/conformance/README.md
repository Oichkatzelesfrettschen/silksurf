# silksurf conformance

This directory holds the aggregated conformance dashboard
(`SCORECARD.md`) and per-harness JSON result files.

## Layout

```
docs/conformance/
  README.md                        # this file
  SCORECARD.md                     # human-readable dashboard
  test262-scorecard.json           # silksurf-js test262 result
  (future) wpt-scorecard.json      # web-platform-tests subset
  (future) h2spec-scorecard.json   # HTTP/2 conformance
```

## Running

```sh
scripts/conformance_run.sh                  # all available harnesses
scripts/conformance_run.sh test262          # one named harness
TEST262_PATH=language scripts/conformance_run.sh test262
                                            # custom test262 subset
```

See `SCORECARD.md` for the current numbers and per-harness scope notes.

## Writing a new harness

  1. Land the harness binary or test source in the appropriate crate.
  2. Add a `run_<harness>` function to `scripts/conformance_run.sh`.
  3. Emit a JSON scorecard to `docs/conformance/<harness>-scorecard.json`.
  4. Add a row to `SCORECARD.md`.

JSON shape (forward-compatible; new fields appended, never removed):

```json
{
  "runner": "<short identifier>",
  "runner_version": "<semver>",
  "unix_timestamp": 1234567890,
  "test_root": "<vendored corpus root>",
  "path_filter": "<subset path or '<all>'>",
  "total": 0,
  "passed": 0,
  "failed": 0,
  "skipped": 0,
  "pass_rate_pct": 0.0,
  "duration_secs": 0.0,
  "runner_kind": "<lexer | parser | vm | network | etc.>",
  "notes": "<one paragraph clarifying what counts as a pass>"
}
```
