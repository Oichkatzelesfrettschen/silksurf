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
run_bench "bench_pipeline" cargo run -p silksurf-engine --bin bench_pipeline
run_bench "bench_js" cargo run -p silksurf-engine --bin bench_js
run_bench "bench_css" cargo run -p silksurf-css --bin bench_css
