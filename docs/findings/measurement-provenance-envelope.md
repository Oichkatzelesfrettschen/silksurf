# Measurement Provenance Across Every Published Artifact

**Date**: 2026-08-07
**Last verified**: 2026-08-07
**Evidence class**: artifact contents and schema validation on this checkout
(rank 1) plus script source (rank 4). The conformance rates quoted here are the
existing rank-2 test-oracle results, unchanged; this finding is about what
travels with them.
**Mechanism**: `scripts/measurement_environment.py` captures the commit, the
resolved rustc, and the host's CPU model, logical CPU count, process affinity,
load average, scaling governors, `perf_event_paranoid`, and full sysfs cache
topology. `perf/measurement-environment.schema.json` defines that object.
`scripts/conformance_run.sh` embeds it in every scorecard, `perf/append_history.py`
embeds it in every history record, and `scripts/locality_probe.py` -- which held
the only implementation -- now imports it.
**Question**: `docs/design/CACHE-LOCALITY-CONTRACT.md` requires every retained
record to name the commit, build profile, cache topology, governor, affinity,
competing load, and command. One instrument honored that. What does the rest of
the published evidence carry, and what would a reader need to reproduce it?

## Verdict

The requirement existed as prose and as one implementation. Three conformance
scorecards carried a corpus revision and nothing else; `perf/history.ndjson`
carried a commit, a timestamp, a rustc version, and a profile, across one
record. No artifact in the tree named the CPU, the cache hierarchy, the kernel,
or the governor, while `perf/locality-budget.json` treats cache capacity as a
measurement input and lists 8, 16, 32, 64, and 96 MiB as sweep coordinates.

Every artifact now carries the same object under the same key, from one
implementation, and `scripts/validate_measurement_artifacts.py` fails
`make check` when one does not.

## What a rate needs beyond its number

Three fields decide whether a percentage is interpretable, and only one of the
three was present everywhere.

`corpus_revision` was. It is the one that had a gate:
`scripts/check_status_consistency.py` rejects a scorecard whose revision drifts
from the pin in `silksurf-extras/html-css-test-corpora-revisions.txt`.

`oracle` was present on one of three. A robustness sweep and a conformance run
produce the same-looking percentage. `css-parse-robustness-scorecard.json`
reports 603 of 603 accepted, and only its oracle line -- `parse_stylesheet_bytes`
returns without error or panic, and the parsed stylesheet is discarded --
prevents that from reading as CSS conformance. The two html5lib scorecards now
state theirs in the same terms, including that a case the expectations file
marks expected-fail counts as a recorded gap rather than a pass.

`measurement_environment` was present on none. A rate that names no compiler
cannot separate an engine change from a codegen change, and a timing that names
no host cannot be compared to another machine's at all.

## Capturing before the run, not after

The first implementation injected the envelope after each harness wrote its
scorecard. Every record then read `git.dirty: true`, because the harness's own
write is an uncommitted change at the moment of capture. `conformance_run.sh`
now captures once before any harness runs and embeds that one envelope in every
scorecard from the invocation, which is also the truthful grouping: three
harnesses in one session ran against one host state.

Regenerating the three scorecards from a clean tree at `8376272` reproduced
every count exactly -- 3,019 of 6,640 executed, 1,440 of 1,726 executed, and 603
of 603 accepted, against the same two corpus revisions. That reproduction is the
first evidence in the tree that a published rate is reproducible rather than
merely recorded.

## A schema nothing validated

`perf/schema.json` declared `additionalProperties: false` and a five-field
required set. `perf/append_history.py` builds each record by hand and imports no
validator, and no target ran one. The schema described an intention.

`scripts/validate_measurement_artifacts.py` reads `perf/history.ndjson` line by
line against that schema and every `docs/conformance/*-scorecard.json` against
the new `docs/conformance/scorecard.schema.json`, reporting the file, the record
index, and the failing JSON pointer. Schemas resolve each other from disk
through their `$id`, so no reference reaches the network. The step exits 0 when
`jsonschema` is absent so the merge gate stays runnable on a host without it,
and `--require-validator` makes the absence an error.

Verified by falsification: a `rate_executed` of 1.4, a removed
`measurement_environment`, and a `corpus_revision` truncated to eight characters
each fail with the pointer that rejected them, and the restored artifacts
validate.

`scripts/test_locality_probe.py` was in the same position -- nine tests that no
Makefile target and no gate ran. `make check` now runs
`python3 -m unittest discover -s scripts -p 'test_*.py'`.

## The single-host limit

Every measurement in this repository comes from one machine: an AMD Ryzen 5
5600X3D, 12 logical CPUs, 96 MiB last-level cache, Linux 7.1.6, `performance`
governor. That is now recorded rather than implicit, which is the improvement;
it is not a substitute for a second host.

Two consequences follow and neither is repairable by better instrumentation.

A capacity sweep needs hosts at its coordinates. `perf/locality-budget.json`
lists 8, 16, 32, 64, and 96 MiB. This host can supply the last one. It cannot
establish behavior at the smaller capacities without hosts that have them or a
verified cache partition, so no latency or miss-rate knee below 96 MiB is
measurable here. `docs/design/CACHE-LOCALITY-CONTRACT.md` already states this
about its own first run; the envelope makes the constraint machine-readable in
every artifact rather than a paragraph in one document.

A single-host result cannot separate host-specific behavior from general
behavior. The parser conformance rates are deterministic in the corpus and the
code, so this does not threaten them. It bounds every timing claim in
`docs/PERFORMANCE.md` and every finding under `docs/findings/` that reports a
latency: those are measurements of this workload on this machine, and the
envelope is what lets a second machine's run be compared rather than merely
placed beside them.

## Falsifiers

- A scorecard regenerated from a clean tree at the same corpus revision produces
  a different count, which would mean the rate depends on something the envelope
  does not record.
- `validate_measurement_artifacts.py` passes an artifact missing a required
  field, which would mean the schema registry resolves the wrong document.
- A second host reproduces a `docs/PERFORMANCE.md` timing within run-to-run
  spread, which would weaken the single-host limit for that measurement rather
  than confirm it.
- A `measurement_environment` object validates while carrying a
  `last_level_cache_bytes` that contradicts `lscpu -C` on the same host, which
  would mean the sysfs derivation picks the wrong level.

## Evidence commands

```sh
python3 scripts/measurement_environment.py
python3 scripts/validate_measurement_artifacts.py --require-validator
scripts/conformance_run.sh html5lib tree-construction css
lscpu -C
```
