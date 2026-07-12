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
# directory) and a machine-readable NDJSON record. Local runs append to the
# ignored perf/results/history.local.ndjson so measuring never churns a
# tracked file; check_perf_regression.sh compares the newest local row
# against the newest curated row in the tracked perf/history.ndjson.
# Promote a milestone row explicitly:
#
#   tail -n 1 perf/results/history.local.ndjson >> perf/history.ndjson
#
run_bench "bench_pipeline" \
  cargo run --release -p silksurf-engine --bin bench_pipeline

# Emit the NDJSON record in a separate run (release profile required for
# valid us numbers; the human-readable run above may be debug in local mode).
local_history="${root_dir}/perf/results/history.local.ndjson"
if cargo run --release -p silksurf-engine --bin bench_pipeline -- --emit json \
     >> "${local_history}" 2>/dev/null; then
  echo "==> ${local_history#"${root_dir}"/} updated"
  echo "    (promote a milestone: tail -n 1 ${local_history#"${root_dir}"/} >> perf/history.ndjson)"
else
  echo "==> WARN: --emit json run failed; local history not updated"
fi

run_bench "bench_js" cargo run -p silksurf-engine --bin bench_js
run_bench "bench_css" cargo run -p silksurf-css --bin bench_css
