# Benchmarks

## Matrix
| Area | Command | Primary signals |
| --- | --- | --- |
| Engine pipeline | `cargo run -p silksurf-engine --bin bench_pipeline` | total time, per-iteration time, display-list size |
| JS runtime queue | `cargo run -p silksurf-engine --bin bench_js` | total time for task enqueue/run |
| CSS parsing | `cargo run -p silksurf-css --bin bench_css` | total time, per-iteration time |

## Baseline Script
Use the helper script to capture stdout into timestamped files:

```
./perf/run_baselines.sh
```

Outputs are saved under `perf/results/` (ignored by git).
