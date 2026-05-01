#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

run_fast_gate() {
    echo "==> Rust format check"
    cargo fmt --all -- --check

    echo "==> Rust clippy strict deny set"
    cargo clippy --workspace --all-targets -- \
        -D clippy::correctness \
        -D clippy::suspicious \
        -D clippy::perf \
        -D clippy::complexity
}

run_full_gate() {
    run_fast_gate

    echo "==> Rust warnings-as-errors check"
    RUSTFLAGS='-D warnings' cargo check --workspace --all-targets

    echo "==> Rust workspace tests"
    cargo test --workspace

    echo "==> Cargo deny policy checks"
    cargo deny check advisories bans licenses sources

    echo "==> C/C++ configure and build"
    cmake -B build
    cmake --build build

    echo "==> CTest"
    ctest --test-dir build --output-on-failure
}

usage() {
    echo "Usage: $0 [fast|full]"
}

MODE="${1:-fast}"
case "$MODE" in
    fast)
        run_fast_gate
        ;;
    full)
        run_full_gate
        ;;
    *)
        usage
        exit 1
        ;;
esac
