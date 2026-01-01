#!/usr/bin/env bash
set -euo pipefail

PROFILE=${PROFILE:-release-riced}
TARGET_CPU=${TARGET_CPU:-native}
EXTRA_RUSTFLAGS=${EXTRA_RUSTFLAGS:-}

RUSTFLAGS="-C target-cpu=${TARGET_CPU} ${EXTRA_RUSTFLAGS}" \
  cargo build --profile "${PROFILE}" "$@"
