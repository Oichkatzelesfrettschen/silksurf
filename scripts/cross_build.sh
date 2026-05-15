#!/usr/bin/env bash
# scripts/cross_build.sh -- workspace cross-compile smoke test.
#
# WHY: we ship a Linux/x86_64 XCB GUI today, but several non-GUI workspace
#      crates (silksurf-core, silksurf-css, silksurf-engine, silksurf-js,
#      silksurf-net, ...) must keep building cleanly on additional targets
#      so we do not silently regress portability. Running this regularly
#      catches accidental host-only assumptions early.
#
# WHAT: for each requested rustc target triple, build the entire workspace
#      with `cargo build --workspace --target <triple>` and report PASS/FAIL.
#      When the `cross` tool (https://github.com/cross-rs/cross) is on PATH
#      and the target is non-native, prefer `cross build` (it provides a
#      pre-baked sysroot/toolchain inside Docker/Podman). Otherwise we use
#      the native toolchain and rely on whatever sysroot rustup has provided.
#
# HOW:
#      scripts/cross_build.sh
#          -- build the default targets (x86_64-unknown-linux-gnu,
#             aarch64-unknown-linux-gnu).
#
#      scripts/cross_build.sh --targets x86_64-unknown-linux-gnu \
#                                       aarch64-unknown-linux-gnu \
#                                       wasm32-unknown-unknown
#          -- build an explicit target list. All flags between --targets and
#             the next flag (or end of args) are treated as triples.
#
#      Exit code: 0 if every target built successfully, non-zero otherwise.
#      The script never installs anything (no `rustup target add`, no
#      `cargo install cross`); it only reports actionable hints when a
#      prerequisite is missing.

set -uo pipefail

# Locate repo root via this script's own path; cd there so cargo picks up
# the workspace Cargo.toml regardless of caller cwd.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

DEFAULT_TARGETS=(
    "x86_64-unknown-linux-gnu"
    "aarch64-unknown-linux-gnu"
)

usage() {
    cat <<'EOF'
Usage: scripts/cross_build.sh [--targets TRIPLE [TRIPLE ...]] [-h|--help]

Builds the SilkSurf workspace for each requested rustc target triple and
reports a PASS/FAIL summary.

Options:
  --targets TRIPLE...   Space-separated list of rustc target triples to build.
                        Defaults to:
                          x86_64-unknown-linux-gnu
                          aarch64-unknown-linux-gnu
  -h, --help            Show this help and exit.

Behavior:
  - When `cross` is on PATH and the target differs from the host triple,
    `cross build` is used instead of `cargo build` (provides a containerized
    sysroot/toolchain).
  - The script never installs missing toolchains, sysroots, or `cross`;
    failures print a hint and the corresponding target is marked FAIL.
  - Exit code is 0 only if every target built successfully.

See docs/development/CROSS-COMPILE.md for prerequisites and known limits.
EOF
}

# Parse args. Supported forms:
#   (no args)                                -> use DEFAULT_TARGETS
#   --targets t1 t2 t3                       -> use t1 t2 t3
#   --targets=t1                             -> use t1 (single triple form)
#   -h | --help                              -> usage; exit 0
TARGETS=()
parse_args() {
    if [[ $# -eq 0 ]]; then
        TARGETS=("${DEFAULT_TARGETS[@]}")
        return
    fi
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                usage
                exit 0
                ;;
            --targets)
                shift
                if [[ $# -eq 0 ]]; then
                    echo "cross_build: --targets requires at least one triple" >&2
                    exit 2
                fi
                while [[ $# -gt 0 && "$1" != --* ]]; do
                    TARGETS+=("$1")
                    shift
                done
                ;;
            --targets=*)
                TARGETS+=("${1#--targets=}")
                shift
                ;;
            *)
                echo "cross_build: unknown argument: $1" >&2
                usage >&2
                exit 2
                ;;
        esac
    done
    if [[ ${#TARGETS[@]} -eq 0 ]]; then
        TARGETS=("${DEFAULT_TARGETS[@]}")
    fi
}

# Detect the host target triple via `rustc -vV` so we can decide whether
# a request is "native" (use cargo) or "cross" (prefer the cross tool).
detect_host_triple() {
    if ! command -v rustc >/dev/null 2>&1; then
        echo ""
        return
    fi
    rustc -vV 2>/dev/null | awk -F': ' '/^host:/ { print $2; exit }'
}

# Returns 0 if `rustup` reports the given target as installed.
target_installed() {
    local triple="$1"
    if ! command -v rustup >/dev/null 2>&1; then
        # Without rustup we cannot inspect installed targets; assume yes
        # and let cargo emit a real error if the sysroot is missing.
        return 0
    fi
    rustup target list --installed 2>/dev/null | grep -Fxq "${triple}"
}

build_one_target() {
    local triple="$1"
    local host="$2"
    local use_cross="no"

    if command -v cross >/dev/null 2>&1 && [[ "${triple}" != "${host}" ]]; then
        use_cross="yes"
    fi

    echo
    echo "==> [${triple}] $( [[ "${use_cross}" == "yes" ]] && echo "cross build" || echo "cargo build" ) --workspace"

    if [[ "${use_cross}" == "no" ]] && [[ -n "${host}" ]] && [[ "${triple}" != "${host}" ]]; then
        if ! target_installed "${triple}"; then
            echo "    hint: rustup target add ${triple}" >&2
            echo "    hint: or install cross (cargo install cross) to use a containerized toolchain" >&2
        fi
    fi

    local cmd
    if [[ "${use_cross}" == "yes" ]]; then
        cmd=(cross build --workspace --target "${triple}")
    else
        cmd=(cargo build --workspace --target "${triple}")
    fi

    if "${cmd[@]}"; then
        return 0
    else
        return 1
    fi
}

main() {
    parse_args "$@"

    if ! command -v cargo >/dev/null 2>&1; then
        echo "cross_build: cargo not found on PATH" >&2
        exit 2
    fi

    local host
    host="$(detect_host_triple)"
    if [[ -n "${host}" ]]; then
        echo "cross_build: host triple detected: ${host}"
    else
        echo "cross_build: warning: could not detect host triple via rustc" >&2
    fi
    echo "cross_build: targets: ${TARGETS[*]}"
    if command -v cross >/dev/null 2>&1; then
        echo "cross_build: cross tool detected ($(command -v cross))"
    else
        echo "cross_build: cross tool not found (will use cargo for all targets)"
    fi

    local -a results=()
    local any_failed=0
    local triple
    for triple in "${TARGETS[@]}"; do
        if build_one_target "${triple}" "${host}"; then
            results+=("PASS  ${triple}")
        else
            results+=("FAIL  ${triple}")
            any_failed=1
        fi
    done

    echo
    echo "==> cross_build summary"
    local line
    for line in "${results[@]}"; do
        echo "    ${line}"
    done

    if [[ "${any_failed}" -ne 0 ]]; then
        echo
        echo "cross_build: one or more targets failed" >&2
        exit 1
    fi

    echo
    echo "cross_build: all targets built successfully"
    exit 0
}

main "$@"
