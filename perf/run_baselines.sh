#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "$0")/.." && pwd)"
out_dir="${root_dir}/perf/results/$(date +%Y%m%d_%H%M%S)"
mkdir -p "${out_dir}"

run_bench() {
  local name="$1"
  shift
  {
    echo "== ${name} =="
    "$@"
  } 2>&1 | tee "${out_dir}/${name}.txt"
}

cd "${root_dir}"

# Run bench_pipeline and capture both human-readable output (for the results
# directory) and a machine-readable NDJSON record (for perf/history.ndjson).
# The --emit json flag writes one schema-conforming JSON line to stdout, which
# we split via tee: human copy goes to the results dir, machine copy appends
# to history.ndjson so check_perf_regression.sh and append_history.py see it.
run_bench "bench_pipeline" \
  cargo run --release -p silksurf-engine --bin bench_pipeline

# Also emit the NDJSON record in a separate run (release profile required for
# valid us numbers; the human-readable run above may be debug in local mode).
if cargo run --release -p silksurf-engine --bin bench_pipeline -- --emit json \
     >> "${root_dir}/perf/history.ndjson" 2>/dev/null; then
  echo "==> perf/history.ndjson updated"
else
  echo "==> WARN: --emit json run failed; history.ndjson not updated"
fi

run_bench "bench_js" cargo run -p silksurf-engine --bin bench_js
run_bench "bench_css" cargo run -p silksurf-css --bin bench_css
