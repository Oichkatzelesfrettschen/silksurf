# silksurf conformance

This directory holds the aggregated conformance dashboard (`SCORECARD.md`), the
per-harness JSON results, and the schema those results must satisfy.

## Layout

```
docs/conformance/
  README.md                                    # this file
  SCORECARD.md                                 # human-readable dashboard
  scorecard.schema.json                        # required shape of every scorecard below
  html5lib-tokenizer-scorecard.json            # html5lib tokenizer corpus
  html5lib-tree-construction-scorecard.json    # WPT html/syntax/parsing/resources
  css-parse-robustness-scorecard.json          # WPT CSS syntax and selector subset
  (live) ../../crates/silksurf-engine/conformance/wpt-scorecard.json
                                               # synthetic in-tree regression fixtures
  (live) ../../crates/silksurf-engine/conformance/h2spec-scorecard.json
  (live) ../../silksurf-js/conformance/test262-boa-scorecard.json
  (archived) ../archive/conformance/test262-lexer-scorecard.json
                                               # historical lexer-only result (AD-025)
```

The three scorecards in this directory validate against
`scorecard.schema.json`; `scripts/validate_measurement_artifacts.py` enforces
that in `make check`. The three live elsewhere predate the schema and carry
their own shapes.

## Running

```sh
scripts/conformance_run.sh                  # all available harnesses
scripts/conformance_run.sh html5lib css     # HTML/CSS parser harnesses
scripts/conformance_run.sh test262          # one named harness
TEST262_PATH=language scripts/conformance_run.sh test262
                                            # custom test262 subset
```

Upstream corpora live in the untracked `silksurf-extras/` tree.
`scripts/fetch_html_css_test_corpora.sh` clones them and writes the revision
each scorecard records. Re-running that fetch moves every corpus to the current
upstream HEAD, so it is a deliberate rebaseline rather than a setup step: every
recorded rate belongs to the revision beside it.

See `SCORECARD.md` for the current numbers and per-harness scope notes. Primary
HTML/CSS source material lives in
`docs/external_sources/html_css_conformance_2026-07-02/`.

## Writing a new harness

  1. Land the harness binary or test source in the appropriate crate.
  2. Add a `run_<harness>` function to `scripts/conformance_run.sh`, ending in
     `embed_environment` for its scorecard path.
  3. Emit a JSON scorecard to `docs/conformance/<harness>-scorecard.json`.
  4. Add a row to `SCORECARD.md` and a rate to `docs/STATUS.md`.

`scorecard.schema.json` is the normative shape. Three fields carry more weight
than the counts:

- `runner_kind` says what the run measures. A robustness sweep and a
  conformance run produce the same-looking percentage, and only this field
  separates them.
- `oracle` says what decides a pass, in the terms the harness applies. A rate
  whose oracle is "the parser returned without panicking" is not a correctness
  rate, and stating so is the difference between an honest number and a misread
  one.
- `measurement_environment` carries the host, toolchain, and revision the run
  used, defined by `perf/measurement-environment.schema.json`. A rate that names
  no compiler cannot separate an engine change from a codegen change.

Both denominators are published. `rate_executed` excludes skipped and
unsupported cases and is the higher number; `rate_total` charges them against
the result. Recorded gaps stay out of the numerator: folding an expectations-file
entry into `pass` would restate every tolerated failure as a handled case.

Example, with the envelope elided:

```json
{
  "runner": "html5lib_tokenizer",
  "runner_kind": "html5lib-tokenizer",
  "corpus": "html5lib-tests tokenizer",
  "corpus_revision": "224991ec10db04f056a89eed8b0bd8695fd2950e",
  "oracle": "the token stream Tokenizer emits equals the corpus #output after html5lib normalization; a case whose id the expectations file marks expected-fail counts as a recorded gap rather than a pass",
  "total": 6806,
  "executed": 6640,
  "pass": 3019,
  "expected_fail": 3621,
  "skip": 0,
  "unsupported": 166,
  "rate_executed": 0.4547,
  "rate_total": 0.4436,
  "measurement_environment": {}
}
```
