#!/usr/bin/env bash
# conformance_run.sh runs the available silksurf conformance harnesses.
#
# Each harness emits its own JSON scorecard. The aggregated dashboard at
# docs/conformance/SCORECARD.md records the current published numbers.
#
# Available harnesses:
#   html5lib   -- HTML tokenizer corpus smoke through silksurf-html tests
#   css        -- external CSS corpus parser sweep through silksurf-css tests
#   test262    -- Boa parse/evaluate test262 runner (subset by default)
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
: "${PYTHON:?Set PYTHON to the intended Python executable}"
export RUSTFLAGS="${RUSTFLAGS:-} -D warnings"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

WPT_SCORECARD="${SCORECARD_DIR:+$SCORECARD_DIR/wpt-scorecard.json}"
WPT_SCORECARD="${WPT_SCORECARD:-crates/silksurf-engine/conformance/wpt-scorecard.json}"
SCORECARD_DIR="${SCORECARD_DIR:-docs/conformance}"
mkdir -p "$SCORECARD_DIR"

# A rate reproduces from a corpus revision plus the host and toolchain that
# produced it. The harnesses write their scorecard from Rust, so the envelope is
# embedded afterward from the one Python implementation rather than
# reimplemented per language.
#
# The capture happens once, before any harness runs, and every scorecard from
# this invocation carries it. Capturing per scorecard after its harness wrote it
# would report the artifact's own write as an uncommitted change, so every
# published record would read git.dirty true.
ENVIRONMENT_ENVELOPE="$(mktemp -t silksurf-measurement-environment.XXXXXX.json)"
trap 'rm -f "$ENVIRONMENT_ENVELOPE"' EXIT
"$PYTHON" scripts/measurement_environment.py --output "$ENVIRONMENT_ENVELOPE" >/dev/null

embed_environment() {
    local scorecard="$1"
    [ -f "$scorecard" ] || return 0
    "$PYTHON" scripts/measurement_environment.py \
        --from "$ENVIRONMENT_ENVELOPE" --inject "$scorecard" >/dev/null
}

# Each invocation owns fresh output; failed builds preserve published evidence.
run_scorecard() {
    local destination="$1" output_variable="$2"
    shift 2
    local evidence scorecard result=0 published
    mkdir -p target/conformance-runs "$(dirname "$destination")"
    evidence="$(mktemp -d target/conformance-runs/run.XXXXXX)"
    scorecard="$REPO_ROOT/$evidence/scorecard.json"
    if [ "$output_variable" = --scorecard ]; then
        "$@" --scorecard "$scorecard" >"$evidence/runner.log" 2>&1 || result=$?
    else
        env "$output_variable=$scorecard" "$@" >"$evidence/runner.log" 2>&1 || result=$?
    fi
    cat "$evidence/runner.log"
    printf '%s\n' "$result" >"$evidence/exit-status"
    if [ ! -f "$scorecard" ]; then
        echo "    runner produced no scorecard; evidence: $evidence" >&2
        if [ "$result" -eq 0 ]; then result=1; fi
        return "$result"
    fi
    embed_environment "$scorecard" || return 1
    published="$(mktemp "${destination}.XXXXXX")"
    cp "$scorecard" "$published" && mv "$published" "$destination" || return 1
    echo "    evidence: $evidence"
    return "$result"
}

run_html5lib() {
    echo "==> html5lib tokenizer corpus"
    if [ -z "${HTML5LIB_TESTS_DIR:-}" ]; then
        if [ -d "$REPO_ROOT/silksurf-extras/html5lib-tests/tokenizer" ]; then
            export HTML5LIB_TESTS_DIR="$REPO_ROOT/silksurf-extras/html5lib-tests/tokenizer"
        else
            echo "    html5lib corpus not present; run scripts/fetch_html_css_test_corpora.sh."
            return 1
        fi
    fi
    if [ -n "${HTML5LIB_TESTS_DIR:-}" ]; then
        echo "    HTML5LIB_TESTS_DIR=$HTML5LIB_TESTS_DIR"
    fi
    run_scorecard "$SCORECARD_DIR/html5lib-tokenizer-scorecard.json" HTML5LIB_SCORECARD \
        env HTML5LIB_FAIL_ON_XPASS=1 cargo test -p silksurf-html \
        --test html5lib_harness -- --nocapture
}

run_css() {
    echo "==> css external corpus (parse robustness, not conformance)"
    if [ -z "${CSS_TESTS_DIR:-}" ]; then
        # The upstream WPT subset is the default corpus; the in-tree fixture
        # directory stands in only when the extras tree is absent.
        upstream_dir="$REPO_ROOT/silksurf-extras/wpt-css-parser-subset/css"
        if [ -d "$upstream_dir" ]; then
            export CSS_TESTS_DIR="$upstream_dir"
        else
            export CSS_TESTS_DIR="$REPO_ROOT/crates/silksurf-css/tests/fixtures/css_harness_corpus"
            echo "    upstream corpus absent; run scripts/fetch_html_css_test_corpora.sh."
        fi
        echo "    CSS_TESTS_DIR=$CSS_TESTS_DIR"
    fi
    run_scorecard "$SCORECARD_DIR/css-parse-robustness-scorecard.json" CSS_HARNESS_SCORECARD \
        env CSS_HARNESS_FAIL_ON_XPASS=1 cargo test -p silksurf-css --test css_harness -- --nocapture
}

run_test262() {
    # test262_boa parses, evaluates, and checks negative expectations
    # against boa_engine; the scorecard carries both denominators
    # (rate_executed and rate_total). TEST262_PATH selects a subdirectory
    # of the corpus; TEST262_FULL=1 widens scope to built-ins + annexB.
    local subset="${TEST262_PATH:-}"
    echo "==> test262 (boa runner)"
    if [ ! -d "silksurf-js/test262/test" ]; then
        echo "    test262 corpus absent at silksurf-js/test262/test."
        return 1
    fi
    set --
    if [ "${TEST262_FULL:-0}" = "1" ]; then
        set -- "$@" --full
    fi
    if [ -n "$subset" ]; then
        set -- "$@" --dir "silksurf-js/test262/test/$subset"
    fi
    run_scorecard "$SCORECARD_DIR/test262-boa-scorecard.json" --scorecard \
        cargo run --release -p silksurf-js --bin test262_boa --quiet -- "$@"
}

run_tls() {
    echo "==> silksurf-tls loader sanity"
    cargo test -p silksurf-tls --test loader_sanity -- --quiet
}

run_h2spec() {
    echo "==> h2spec (external)"
    if ! command -v h2spec >/dev/null 2>&1; then
        echo "    h2spec is required for the selected HTTP/2 lane."
        echo "    install: https://github.com/summerwind/h2spec"
        return 1
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
        2) echo "    set SILKSURF_H2_HOST to select an HTTP/2 server"; return 1 ;;
        *) echo "    h2spec driver exited $rc; see crates/silksurf-engine/conformance/h2spec-results.txt"; return "$rc" ;;
    esac
}

run_tree_construction() {
    echo "==> wpt tree construction (upstream corpus, production parse path)"
    if [ -z "${WPT_HTML_PARSING_DIR:-}" ]; then
        default_dir="$REPO_ROOT/silksurf-extras/wpt-css-parser-subset/html/syntax/parsing/resources"
        if [ -d "$default_dir" ]; then
            export WPT_HTML_PARSING_DIR="$default_dir"
        else
            echo "    corpus absent; run scripts/fetch_html_css_test_corpora.sh."
            return 1
        fi
    fi
    if [ -n "${WPT_HTML_PARSING_DIR:-}" ]; then
        echo "    WPT_HTML_PARSING_DIR=$WPT_HTML_PARSING_DIR"
    fi
    run_scorecard "$SCORECARD_DIR/html5lib-tree-construction-scorecard.json" HTML5LIB_TREE_SCORECARD \
        env HTML5LIB_TREE_FAIL_ON_XPASS=1 cargo test -p silksurf-html \
        --test html5lib_tree_construction -- --nocapture
}

run_wpt() {
    echo "==> wpt (synthetic in-tree subset)"
    run_scorecard "$WPT_SCORECARD" --scorecard \
        cargo run --release -p silksurf-engine --bin wpt_runner \
        --features js-conformance --quiet -- \
        --dir crates/silksurf-engine/conformance/wpt/fixtures
}

# Default: run everything available.
TARGETS=("$@")
if [ ${#TARGETS[@]} -eq 0 ]; then
    TARGETS=(html5lib tree-construction css test262 tls h2spec wpt)
fi

for target in "${TARGETS[@]}"; do
    case "$target" in
        html5lib) run_html5lib ;;
        tree-construction) run_tree_construction ;;
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
