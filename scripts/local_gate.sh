#!/usr/bin/env bash
# silksurf local-gate: the canonical merge-readiness check.
#
# WHY: cloud CI is intentionally disabled for push/PR (see ADR-009 in
# docs/design/ARCHITECTURE-DECISIONS.md). The local-gate is the merge gate;
# pre-commit / pre-push git hooks (scripts/install-git-hooks.sh) wire fast
# and full modes into the everyday git flow.
#
# WHAT:
#   fast  -- rustfmt + clippy strict deny set + lint_unwrap + lint_unsafe.
#            Invoked by pre-commit hook. Target: under ~30s on a warm cache.
#   full  -- fast + warnings-as-errors check + tests + cargo deny + MSRV
#            verification + doc build + (optional miri smoke, opt-in via
#            MIRI=1) + (optional fuzz smoke, opt-in via FUZZ=1) + C/C++
#            CMake build + CTest. Invoked by pre-push hook.
#
# HOW: scripts/local_gate.sh [fast|full]
#   MIRI=1 scripts/local_gate.sh full    # add miri smoke (~3-5 min)
#   FUZZ=1 scripts/local_gate.sh full    # add fuzz smoke (30s per target)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# Read MSRV from workspace.package.rust-version. Single source of truth.
MSRV="$(awk -F'"' '/^rust-version =/ {print $2; exit}' Cargo.toml)"

run_fast_gate() {
    echo "==> rustfmt format check"
    cargo fmt --all -- --check

    echo "==> clippy strict deny set"
    cargo clippy --workspace --all-targets -- \
        -D clippy::correctness \
        -D clippy::suspicious \
        -D clippy::perf \
        -D clippy::complexity

    if [ -x "scripts/lint_unwrap.sh" ]; then
        echo "==> lint_unwrap (unannotated unwrap/expect detector)"
        scripts/lint_unwrap.sh
    fi

    if [ -x "scripts/lint_unsafe.sh" ]; then
        echo "==> lint_unsafe (SAFETY-comment requirement)"
        scripts/lint_unsafe.sh
    fi

    if [ -x "scripts/lint_glossary.sh" ]; then
        echo "==> lint_glossary (advisory: public names vs GLOSSARY.md)"
        scripts/lint_glossary.sh
    fi
}

run_full_gate() {
    run_fast_gate

    echo "==> warnings-as-errors check"
    RUSTFLAGS='-D warnings' cargo check --workspace --all-targets

    echo "==> workspace tests"
    cargo test --workspace

    echo "==> cargo deny policy"
    if command -v cargo-deny >/dev/null 2>&1; then
        cargo deny check advisories bans licenses sources
    else
        echo "    (cargo-deny not installed; skipping. Install via: cargo install cargo-deny)"
    fi

    echo "==> MSRV verification (workspace pinned to ${MSRV})"
    rustup show active-toolchain
    cargo check --workspace --all-targets

    echo "==> cargo doc (no_deps, document private items)"
    cargo doc --workspace --no-deps --document-private-items

    if [ "${MIRI:-0}" = "1" ]; then
        echo "==> miri smoke (opt-in via MIRI=1)"
        if rustup +nightly component list --installed 2>/dev/null | grep -q '^miri'; then
            cargo +nightly miri test \
                -p silksurf-core \
                -p silksurf-css \
                --lib
        else
            echo "    miri not installed on nightly toolchain. Install via:"
            echo "      rustup toolchain install nightly --component miri"
            exit 1
        fi
    fi

    if [ "${FUZZ:-0}" = "1" ]; then
        echo "==> fuzz smoke (opt-in via FUZZ=1, 30s per target)"
        if command -v cargo-fuzz >/dev/null 2>&1; then
            for target in html_tokenizer html_tree_builder css_tokenizer css_parser js_runtime; do
                echo "    -- $target"
                cargo +nightly fuzz run "$target" -- \
                    -max_total_time=30 -runs=200000 || true
            done
        else
            echo "    cargo-fuzz not installed. Install via: cargo install cargo-fuzz"
            exit 1
        fi
    fi

    if [ -f "CMakeLists.txt" ]; then
        echo "==> C/C++ configure and build (legacy harness, see ADR-007)"
        cmake -B build -DCMAKE_BUILD_TYPE=Release
        cmake --build build --parallel

        echo "==> CTest"
        ctest --test-dir build --output-on-failure
    fi
}

usage() {
    cat <<EOF
Usage: $0 [fast|full]

  fast  -- pre-commit gate: fmt + clippy + lint_unwrap + lint_unsafe.
  full  -- pre-push gate:   fast + warnings-as-errors + tests + deny +
                            MSRV + cargo doc + (opt-in miri / fuzz) +
                            CMake + CTest.

Environment:
  MIRI=1   add miri smoke to full gate (requires nightly + miri component)
  FUZZ=1   add fuzz smoke to full gate (requires cargo-fuzz)

See docs/development/LOCAL-GATE.md for full reference.
EOF
}

MODE="${1:-fast}"
case "$MODE" in
    fast)
        run_fast_gate
        ;;
    full)
        run_full_gate
        ;;
    -h|--help|help)
        usage
        ;;
    *)
        usage
        exit 1
        ;;
esac

echo
echo "OK: local-gate ${MODE} passed."
