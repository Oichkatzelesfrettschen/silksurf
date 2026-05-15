#!/usr/bin/env bash
# silksurf SBOM generator (P9.S2).
#
# WHY: Supply-chain transparency. A CycloneDX SBOM enumerates every crate
#      version that lands in the released binary, which lets downstream
#      consumers (perf-lab provisioners, security auditors, packagers)
#      diff the dependency closure between releases and run vulnerability
#      scans against a stable, machine-readable manifest.
#
# WHAT: Wraps `cargo cyclonedx`. Writes to releases/sbom.cdx.json relative
#      to the repo root. The output format is CycloneDX 1.5 JSON, the
#      current de-facto interchange format for SBOM tooling (Grype,
#      Dependency-Track, Trivy, Syft).
#
# HOW: scripts/generate_sbom.sh
#      OUT=path/to/sbom.json scripts/generate_sbom.sh    # custom output
#
# Exit codes:
#   0  -- SBOM written.
#   1  -- cargo-cyclonedx missing, or generation failed.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

log() { printf '==> %s\n' "$*"; }
err() { printf 'ERROR: %s\n' "$*" >&2; }

# --- Tool discovery ---------------------------------------------------------
# cargo-cyclonedx exposes itself via `cargo cyclonedx --version`. The exit
# code is 101 if the subcommand is missing (cargo's "no such subcommand"
# response) and 0 if it succeeds. We treat any non-zero as missing.
if ! cargo cyclonedx --version >/dev/null 2>&1; then
    err "cargo-cyclonedx is not installed."
    err ""
    err "Install with:"
    err "    cargo install cargo-cyclonedx"
    err ""
    err "After install, re-run: scripts/generate_sbom.sh"
    exit 1
fi

CYCLONEDX_VERSION="$(cargo cyclonedx --version 2>&1 | head -1)"
log "found ${CYCLONEDX_VERSION}"

# --- Output destination -----------------------------------------------------
# Default location is releases/sbom.cdx.json (the dotted-cdx convention is
# how the CycloneDX project marks the file format inside an SBOM-bundle
# directory). Override with OUT=...
OUT="${OUT:-releases/sbom.cdx.json}"
OUT_DIR="$(dirname "$OUT")"
mkdir -p "$OUT_DIR"

# --- Generate ---------------------------------------------------------------
# cargo-cyclonedx writes per-package files by default; we want a single
# aggregate. The flags below evolved across versions; we probe and adapt.
#
# 0.5.x and later: `--format json --all` produces one bom.json per crate
#                  in target/. We then merge with `--top-level` if available,
#                  otherwise we copy the workspace-root one.
log "generating CycloneDX SBOM into ${OUT}"

# Probe for the --override-filename flag (introduced in 0.5.5). When
# available, we get exactly one file at the path we want. When not, we
# fall back to the per-package output and copy the workspace-root SBOM
# into place.
if cargo cyclonedx --help 2>&1 | grep -q -- '--override-filename'; then
    cargo cyclonedx \
        --format json \
        --override-filename "$(basename "$OUT" .cdx.json)" \
        --target-in-filename
    # Move the generated workspace-root SBOM to the requested path. The
    # tool writes to ./<name>.cdx.json; relocate it.
    GENERATED="$(basename "$OUT" .cdx.json).cdx.json"
    if [ -f "$GENERATED" ] && [ "$GENERATED" != "$OUT" ]; then
        mv "$GENERATED" "$OUT"
    fi
else
    # Older releases: aggregate output goes to ./bom.json.
    cargo cyclonedx --format json
    if [ -f "bom.json" ]; then
        mv bom.json "$OUT"
    elif [ -f "silksurf.cdx.json" ]; then
        mv silksurf.cdx.json "$OUT"
    else
        err "cargo-cyclonedx ran but produced no recognisable output file."
        err "Check the cyclonedx documentation for your installed version:"
        err "    cargo cyclonedx --help"
        exit 1
    fi
fi

if [ ! -s "$OUT" ]; then
    err "SBOM file ${OUT} is missing or empty after generation."
    exit 1
fi

# --- Report -----------------------------------------------------------------
SIZE_BYTES="$(wc -c < "$OUT")"
log "wrote ${OUT} (${SIZE_BYTES} bytes)"
log "verify with:"
log "    jq '.components | length' ${OUT}      # component count"
log "    jq '.metadata.timestamp' ${OUT}       # generation timestamp"
