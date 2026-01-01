#!/usr/bin/env bash
set -euo pipefail

BIN=${1:-bench_pipeline}
shift || true

PROFILE=${PROFILE:-release-riced}
CRATE=${CRATE:-silksurf-engine}
BOLT_DIR=${BOLT_DIR:-target/bolt}
TARGET_CPU=${TARGET_CPU:-native}
EXTRA_RUSTFLAGS=${EXTRA_RUSTFLAGS:-}
PERF_OPTS=${PERF_OPTS:-"-e cycles:u -j any,u"}
PERF2BOLT_OPTS=${PERF2BOLT_OPTS:-}
BOLT_OPTS=${BOLT_OPTS:-"-reorder-blocks=ext-tsp -reorder-functions=cdsort -split-functions -icf -use-gnu-stack"}

mkdir -p "${BOLT_DIR}"

RUSTFLAGS="-C target-cpu=${TARGET_CPU} -C force-frame-pointers=yes -C link-arg=-Wl,--emit-relocs ${EXTRA_RUSTFLAGS}" \
  cargo build --profile "${PROFILE}" -p "${CRATE}" --bin "${BIN}"

BIN_PATH="./target/${PROFILE}/${BIN}"

if ! perf record ${PERF_OPTS} -o "${BOLT_DIR}/perf.data" -- "${BIN_PATH}" "$@"; then
  echo "BOLT error: perf record failed. Set PERF_OPTS to a supported branch-sampling mode." >&2
  exit 1
fi
perf2bolt "${BIN_PATH}" -p "${BOLT_DIR}/perf.data" -o "${BOLT_DIR}/perf.fdata" ${PERF2BOLT_OPTS}
llvm-bolt "${BIN_PATH}" -o "${BOLT_DIR}/${BIN}.bolt" -data="${BOLT_DIR}/perf.fdata" ${BOLT_OPTS}
