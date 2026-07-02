#!/usr/bin/env bash
# conformance_run.sh runs the available silksurf conformance harnesses.
#
# Each harness emits its own JSON scorecard. The aggregated dashboard at
# docs/conformance/SCORECARD.md records the current published numbers.
#
# Available harnesses:
#   html5lib   -- HTML tokenizer corpus smoke through silksurf-html tests
#   css        -- external CSS corpus parser sweep through silksurf-css tests
#   test262    -- silksurf-js lexer-only test262 runner (subset by default)
#   tls        -- silksurf-tls loader sanity unit tests
#   h2spec     -- HTTP/2 conformance via the external `h2spec` binary
#                 (skipped if not installed)
#   wpt        -- synthetic in-tree HTML/CSS/layout/paint fixture subset
#
# Usage:
#   scripts/conformance_run.sh                    # run all available
#   scripts/conformance_run.sh test262            # run a single harness
#   scripts/conformance_run.sh html5lib css      # run HTML/CSS harnesses
#   scripts/conformance_run.sh test262 tls       # run named harnesses
#   TEST262_PATH=language scripts/conformance_run.sh test262
#                                                 # custom test262 subset

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

SCORECARD_DIR="docs/conformance"
mkdir -p "$SCORECARD_DIR"

run_html5lib() {
    echo "==> html5lib tokenizer corpus"
    if [ -z "${HTML5LIB_TESTS_DIR:-}" ] \
        && [ ! -d "silksurf-extras/html5lib-tests/tokenizer" ]; then
        echo "    html5lib corpus not present; test emits a skip notice."
    fi
    RUSTFLAGS='-D warnings' cargo test -p silksurf-html --test html5lib_harness -- --nocapture
}

run_css() {
    echo "==> css external corpus"
    if [ -z "${CSS_TESTS_DIR:-}" ]; then
        echo "    CSS_TESTS_DIR not set; test emits a skip notice."
    fi
    RUSTFLAGS='-D warnings' cargo test -p silksurf-css --test css_harness -- --nocapture
}

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
    # Delegate to the dedicated driver. It writes the scorecard JSON
    # itself, so we do not need to capture stdout. Exit 2 means "no
    # server available" -- benign in the default workspace.
    set +e
    scripts/run_h2spec.sh
    local rc=$?
    set -e
    case "$rc" in
        0) ;;
        2) echo "    no in-tree h2 server yet; skipping (set SILKSURF_H2_HOST to override)" ;;
        *) echo "    h2spec driver exited $rc; see crates/silksurf-engine/conformance/h2spec-results.txt" ;;
    esac
}

run_wpt() {
    echo "==> wpt (synthetic in-tree subset)"
    cargo build --release -p silksurf-engine --bin wpt_runner --quiet
    ./target/release/wpt_runner \
        --dir crates/silksurf-engine/conformance/wpt/fixtures \
        --scorecard crates/silksurf-engine/conformance/wpt-scorecard.json \
        || true
}

# Default: run everything available.
TARGETS=("$@")
if [ ${#TARGETS[@]} -eq 0 ]; then
    TARGETS=(html5lib css test262 tls h2spec wpt)
fi

for target in "${TARGETS[@]}"; do
    case "$target" in
        html5lib) run_html5lib ;;
        css)      run_css ;;
        test262) run_test262 ;;
        tls)     run_tls ;;
        h2spec)  run_h2spec ;;
        wpt)     run_wpt ;;
        *) echo "unknown target: $target" >&2; exit 1 ;;
    esac
done

echo
echo "Conformance run complete."
echo "Dashboard: $SCORECARD_DIR/SCORECARD.md"
