#!/usr/bin/env bash
set -euo pipefail

BIN=${1:-bench_pipeline}
shift || true

PROFILE=${PROFILE:-release-riced}
CRATE=${CRATE:-silksurf-engine}
PGO_DIR=${PGO_DIR:-target/pgo}
TARGET_CPU=${TARGET_CPU:-native}
EXTRA_RUSTFLAGS=${EXTRA_RUSTFLAGS:-}
PGO_WARN=${PGO_WARN:-0}

mkdir -p "${PGO_DIR}"
PGO_DIR="$(cd "${PGO_DIR}" && pwd -P)"

RUSTFLAGS="-C profile-generate=${PGO_DIR} -C target-cpu=${TARGET_CPU} ${EXTRA_RUSTFLAGS}" \
  cargo build --profile "${PROFILE}" -p "${CRATE}" --bin "${BIN}"

"./target/${PROFILE}/${BIN}" "$@"

shopt -s nullglob
profraw_files=("${PGO_DIR}"/*.profraw)
if [ ${#profraw_files[@]} -eq 0 ]; then
  echo "PGO error: no .profraw files found in ${PGO_DIR}" >&2
  exit 1
fi

llvm-profdata merge -o "${PGO_DIR}/merged.profdata" "${PGO_DIR}"/*.profraw

PGO_WARN_ARGS=""
if [ "${PGO_WARN}" = "1" ]; then
  PGO_WARN_ARGS="-C llvm-args=-pgo-warn-missing-function"
fi

RUSTFLAGS="-C profile-use=${PGO_DIR}/merged.profdata ${PGO_WARN_ARGS} -C target-cpu=${TARGET_CPU} ${EXTRA_RUSTFLAGS}" \
  cargo build --profile "${PROFILE}" -p "${CRATE}" --bin "${BIN}"
