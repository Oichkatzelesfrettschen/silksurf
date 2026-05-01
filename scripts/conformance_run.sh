#!/usr/bin/env bash
# scripts/conformance_run.sh
#
# Run all available conformance harnesses and emit per-harness JSON
# scorecards under docs/conformance/. The aggregated dashboard at
# docs/conformance/SCORECARD.md is updated by hand from the JSON files.
#
# WHY: the SNAZZY-WAFFLE roadmap (P5) tracks web/network/spec
# conformance as scoreboards. This script is the single entry point so
# numbers are reproducible.
#
# Available harnesses:
#   test262    -- silksurf-js lexer-only test262 runner (subset by default)
#   tls        -- silksurf-tls loader sanity unit tests
#   h2spec     -- HTTP/2 conformance via the external `h2spec` binary
#                 (skipped if not installed)
#   wpt        -- DEFERRED (not vendored yet)
#
# Usage:
#   scripts/conformance_run.sh                    # run all available
#   scripts/conformance_run.sh test262            # run a single harness
#   scripts/conformance_run.sh test262 tls        # run named harnesses
#   TEST262_PATH=language scripts/conformance_run.sh test262
#                                                 # custom test262 subset

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

SCORECARD_DIR="docs/conformance"
mkdir -p "$SCORECARD_DIR"

run_test262() {
    local subset="${TEST262_PATH:-language/literals/numeric}"
    echo "==> test262 [subset: $subset]"
    if [ ! -d "silksurf-js/test262/test" ]; then
        echo "    test262 corpus not present at silksurf-js/test262/test; skipping."
        return 0
    fi
    cargo build --release -p silksurf-js --bin test262 --quiet
    ./target/release/test262 \
        --test262 silksurf-js/test262 \
        --scorecard "$SCORECARD_DIR/test262-scorecard.json" \
        "$subset" || true
}

run_tls() {
    echo "==> silksurf-tls loader sanity"
    cargo test -p silksurf-tls --test loader_sanity -- --quiet
}

run_h2spec() {
    echo "==> h2spec (external)"
    if ! command -v h2spec >/dev/null 2>&1; then
        echo "    h2spec not installed; skipping."
        echo "    install: https://github.com/summerwind/h2spec"
        return 0
    fi
    echo "    h2spec invocation against silksurf-net::H2Client is queued in"
    echo "    SNAZZY-WAFFLE P5.S3 (requires a local h2 server harness)."
    echo "    Until that lands, point h2spec at any external HTTP/2 server"
    echo "    you control to validate the tool installation:"
    echo "        h2spec -h <hostname> -p 443 -t"
}

# Default: run everything available.
TARGETS=("$@")
if [ ${#TARGETS[@]} -eq 0 ]; then
    TARGETS=(test262 tls h2spec)
fi

for target in "${TARGETS[@]}"; do
    case "$target" in
        test262) run_test262 ;;
        tls)     run_tls ;;
        h2spec)  run_h2spec ;;
        wpt)
            echo "==> wpt: DEFERRED (web-platform-tests not vendored)."
            echo "    Tracked in SNAZZY-WAFFLE roadmap P5.S2."
            ;;
        *) echo "unknown target: $target" >&2; exit 1 ;;
    esac
done

echo
echo "Conformance run complete."
echo "Scorecards under: $SCORECARD_DIR"
