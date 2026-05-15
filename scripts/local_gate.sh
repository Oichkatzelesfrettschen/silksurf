#!/usr/bin/env bash
# local_gate.sh -- backward-compatible wrapper around make check / make full.
#
# WHY: The canonical entry points are now `make check` (fast) and `make full`.
# This script is kept so that any existing docs or muscle-memory still work.
# Prefer invoking make directly for new workflows.
#
# HOW: scripts/local_gate.sh [fast|full]
#   MIRI=1 scripts/local_gate.sh full    # add miri smoke
#   FUZZ=1 scripts/local_gate.sh full    # add fuzz smoke (30s per target)
#
# See docs/development/LOCAL-GATE.md for full reference.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

usage() {
    cat <<EOF
Usage: $0 [fast|full]

  fast  -- pre-commit gate: make check (fmt + clippy -D warnings + lint_unwrap + lint_unsafe)
  full  -- pre-push gate:   make full  (fast + tests + deny + doc + opt-in miri/fuzz)

Environment (passed through to make):
  MIRI=1   add miri smoke to full gate (requires nightly + miri component)
  FUZZ=1   add fuzz smoke to full gate (requires cargo-fuzz)

See docs/development/LOCAL-GATE.md for full reference.
EOF
}

MODE="${1:-fast}"
case "$MODE" in
    fast)
        exec make -C "${REPO_ROOT}" check
        ;;
    full)
        exec make -C "${REPO_ROOT}" full MIRI="${MIRI:-0}" FUZZ="${FUZZ:-0}"
        ;;
    -h|--help|help)
        usage
        ;;
    *)
        usage
        exit 1
        ;;
esac
