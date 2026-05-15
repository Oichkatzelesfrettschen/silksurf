#!/usr/bin/env bash
# scripts/run_h2spec.sh -- SNAZZY-WAFFLE P5.S3 h2spec conformance driver.
#
# WHY: HTTP/2 (RFC 9113) is a large stateful protocol. Validating frame
# semantics, flow control, HPACK encoding, and CONTINUATION sequencing
# by hand is impractical. h2spec (https://github.com/summerwind/h2spec)
# is the de-facto external conformance suite. This script gives us a
# reproducible invocation path so the resulting numbers land in the
# same scorecard schema as test262 / wpt.
#
# WHAT: invokes a locally-installed h2spec binary against a silksurf
# HTTP/2 server (or any server the operator names via $SILKSURF_H2_HOST
# / $SILKSURF_H2_PORT) and converts its summary line into a JSON
# scorecard at conformance/h2spec-scorecard.json.
#
# HOW:
#   scripts/run_h2spec.sh                    # localhost:8443, TLS
#   SILKSURF_H2_HOST=example.com SILKSURF_H2_PORT=443 scripts/run_h2spec.sh
#   SILKSURF_H2_TIMEOUT=60 scripts/run_h2spec.sh
#
# EXIT CODES:
#   0  -- h2spec ran and the scorecard was emitted (regardless of pass rate)
#   1  -- h2spec is not installed
#   2  -- the silksurf h2 server harness is not yet wired up AND no
#         operator-provided $SILKSURF_H2_HOST was set
#   3  -- h2spec timed out or its output could not be parsed
#
# See: docs/development/RUNBOOK-H2SPEC.md for the full runbook.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

CONFORMANCE_DIR="crates/silksurf-engine/conformance"
RESULTS_TXT="$CONFORMANCE_DIR/h2spec-results.txt"
SCORECARD_JSON="$CONFORMANCE_DIR/h2spec-scorecard.json"
RUNNER_VERSION="0.1.0"

H2_HOST="${SILKSURF_H2_HOST:-localhost}"
H2_PORT="${SILKSURF_H2_PORT:-8443}"
H2_TIMEOUT="${SILKSURF_H2_TIMEOUT:-30}"

mkdir -p "$CONFORMANCE_DIR"

# ---------------------------------------------------------------------------
# Step 1: ensure h2spec is installed.
# ---------------------------------------------------------------------------
if ! command -v h2spec >/dev/null 2>&1; then
    cat >&2 <<EOF
ERROR: h2spec is not installed or not on PATH.

Install instructions:
  - Arch: yay -S h2spec   (AUR)
  - Go:   go install github.com/summerwind/h2spec/cmd/h2spec@latest
  - Bin:  https://github.com/summerwind/h2spec/releases

After installation, ensure 'h2spec --version' prints a version string,
then re-run this script.
EOF
    exit 1
fi

# ---------------------------------------------------------------------------
# Step 2: locate or warn about the silksurf h2 server harness.
# ---------------------------------------------------------------------------
# silksurf-app does not yet ship a standalone HTTP/2 test server. Until
# that lands (tracked in SNAZZY-WAFFLE roadmap P5.S3), the operator must
# point us at an externally-running HTTP/2 endpoint via $SILKSURF_H2_HOST.
SERVER_PID=""
if [ "$H2_HOST" = "localhost" ] && [ -z "${SILKSURF_H2_HOST:-}" ]; then
    if cargo run -p silksurf-app --bin silksurf-h2-server --quiet -- --help \
        >/dev/null 2>&1; then
        echo "==> Starting silksurf-h2-server in the background"
        cargo run -p silksurf-app --bin silksurf-h2-server --quiet \
            -- --bind "$H2_HOST:$H2_PORT" >/dev/null 2>&1 &
        SERVER_PID=$!
        # Give the server a beat to bind. h2spec will retry connect
        # internally so we do not need a perfect synchronisation.
        sleep 1
    else
        cat >&2 <<EOF
ERROR: no in-tree silksurf HTTP/2 test server is available yet.

The silksurf-app crate does not ship a 'silksurf-h2-server' binary at
this time. Either:

  (a) point this script at an external HTTP/2 endpoint:
        SILKSURF_H2_HOST=example.com SILKSURF_H2_PORT=443 \\
            scripts/run_h2spec.sh
      (this validates the toolchain but NOT silksurf's own h2 stack);

  (b) wait for SNAZZY-WAFFLE P5.S3 to land an in-tree h2 test server
      (tracked in silksurf-specification/SILKSURF-RUST-MIGRATION.md).
EOF
        exit 2
    fi
fi

# Always clean up our backgrounded server on exit.
cleanup() {
    if [ -n "$SERVER_PID" ]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Step 3: run h2spec and capture full stdout to disk.
# ---------------------------------------------------------------------------
echo "==> Running h2spec against $H2_HOST:$H2_PORT (timeout ${H2_TIMEOUT}s)"
set +e
timeout "${H2_TIMEOUT}s" h2spec -h "$H2_HOST" -p "$H2_PORT" -t \
    > "$RESULTS_TXT" 2>&1
H2SPEC_RC=$?
set -e

if [ $H2SPEC_RC -eq 124 ]; then
    echo "ERROR: h2spec timed out after ${H2_TIMEOUT}s" >&2
    exit 3
fi

# h2spec returns non-zero when any test fails; that is normal. We only
# treat parse failure or timeout as a script-level error.

# ---------------------------------------------------------------------------
# Step 4: parse the summary line.
#
# Recent h2spec emits a final line of the form:
#     "X tests, Y passed, Z skipped, W failed"
# Older builds emit:
#     "X tests, Y passed, Z failed"
# We accept either.
# ---------------------------------------------------------------------------
SUMMARY="$(grep -E '[0-9]+ tests, [0-9]+ passed' "$RESULTS_TXT" | tail -n1 || true)"

if [ -z "$SUMMARY" ]; then
    echo "ERROR: could not find a summary line in h2spec output" >&2
    echo "       see $RESULTS_TXT for the captured run" >&2
    exit 3
fi

TOTAL="$(echo "$SUMMARY" | grep -oE '[0-9]+ tests' | grep -oE '[0-9]+')"
PASS="$(echo "$SUMMARY" | grep -oE '[0-9]+ passed' | grep -oE '[0-9]+')"
SKIP="$(echo "$SUMMARY" | grep -oE '[0-9]+ skipped' | grep -oE '[0-9]+' || echo 0)"
FAIL="$(echo "$SUMMARY" | grep -oE '[0-9]+ failed' | grep -oE '[0-9]+' || echo 0)"

# Default empty values to 0 for the JSON write.
TOTAL="${TOTAL:-0}"
PASS="${PASS:-0}"
SKIP="${SKIP:-0}"
FAIL="${FAIL:-0}"

# rate = pass / (pass + fail). Skips do not count towards the denominator,
# matching the test262 / wpt convention.
DENOM=$((PASS + FAIL))
if [ "$DENOM" -gt 0 ]; then
    # awk for portable float division (printf %.4f on integers).
    RATE="$(awk -v p="$PASS" -v d="$DENOM" 'BEGIN { printf "%.4f", p / d }')"
else
    RATE="0.0000"
fi

TIMESTAMP="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

# ---------------------------------------------------------------------------
# Step 5: emit the scorecard.
# ---------------------------------------------------------------------------
cat > "$SCORECARD_JSON" <<EOF
{
  "total": $TOTAL,
  "pass": $PASS,
  "fail": $FAIL,
  "skip": $SKIP,
  "rate": $RATE,
  "timestamp": "$TIMESTAMP",
  "runner_version": "$RUNNER_VERSION",
  "runner_kind": "h2spec",
  "h2_host": "$H2_HOST",
  "h2_port": $H2_PORT,
  "raw_results": "$RESULTS_TXT",
  "notes": "Generated by scripts/run_h2spec.sh. See docs/development/RUNBOOK-H2SPEC.md."
}
EOF

echo "==> h2spec summary: $SUMMARY"
echo "==> Scorecard:      $SCORECARD_JSON"
echo "==> Raw output:     $RESULTS_TXT"
exit 0
