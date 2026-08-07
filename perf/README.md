# Benchmarks

## Matrix

| Area | Command | Primary signals |
| --- | --- | --- |
| Engine pipeline | `cargo run -p silksurf-engine --bin bench_pipeline` | total time, per-iteration time, display-list size |
| JS runtime queue | `cargo run -p silksurf-engine --bin bench_js` | total time for task enqueue/run |
| CSS parsing | `cargo run -p silksurf-css --bin bench_css` | total time, per-iteration time |
| Locality evidence | `python3 scripts/locality_probe.py --name <label> -- <command>` | wall-time distribution, max RSS, faults, context switches, perf counters |

## Files

| File | Role |
| --- | --- |
| `schema.json` | Shape of one `history.ndjson` record |
| `measurement-environment.schema.json` | Shape of the provenance envelope every artifact embeds |
| `baseline.json` | Latest captured snapshot `append_history.py` reads metrics from |
| `history.ndjson` | Append-only cross-commit trend, one JSON object per line |
| `locality-budget.json` | Cache-adaptive execution modes and required measurements |
| `results/` | Probe and baseline output, ignored by git |

`scripts/validate_measurement_artifacts.py` checks `history.ndjson` against
`schema.json` on every `make check`, so a record that drifts from the schema
fails the gate rather than accumulating.

## Provenance

Every record carries a `measurement_environment` object from
`scripts/measurement_environment.py`: the commit and whether the tree was
clean, the resolved rustc, and the host's CPU model, logical CPU count, process
affinity, load average, scaling governors, `perf_event_paranoid`, and full sysfs
cache topology. A timing does not reproduce from `git_sha` alone -- the same
commit on a different cache hierarchy or scaling governor produces a different
number -- and `docs/design/CACHE-LOCALITY-CONTRACT.md` requires the same facts
of every retained record.

`last_level_cache_bytes` is the nominal capacity of one instance as sysfs
reports it. Affinity, cache sharing, explicit partitions, virtualization, and
competing load all reduce the share a workload receives, which is why
`locality-budget.json` treats capacity as a runtime input rather than a target.

## Uncertainty

`schema.json` accepts an optional `distributions` object keyed the same as
`metrics`, carrying `n`, `min`, `median`, `p95`, `p99`, `max`, and `mean`. A
single scalar cannot distinguish a regression from run-to-run spread, so a
harness that runs more than one iteration reports both.
`scripts/locality_probe.py` computes exactly these statistics per sample set
through `summarize_numeric`, using nearest-rank percentiles.

`append_history.py --distribution` writes them, repeatable per metric:

```sh
python3 perf/append_history.py \
  --distribution 'fused_pipeline_us=20:188.4:191.7:198.2:203.9:205.1:192.3'
```

A spec naming a metric the record does not carry is rejected, because a spread
attached to an absent metric describes nothing and the schema cannot catch it:
`distributions` is keyed freely. `min <= median <= max` is checked for the same
reason.

## Appending a record

```sh
python3 perf/append_history.py --profile release --notes "<what changed>"
python3 perf/append_history.py --idle-cpu "$(sh scripts/measure_idle_cpu.sh)"
```

## Baseline script

```sh
./perf/run_baselines.sh
```

Outputs land under `perf/results/`, which git ignores.
