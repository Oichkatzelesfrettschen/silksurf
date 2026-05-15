#!/usr/bin/env bash
# silksurf release driver (P9.S1).
#
# WHY: A release is a high-stakes, hard-to-undo action (annotated tags get
#      promoted to public artifacts and consumed by downstream packagers).
#      The release path therefore has to gate aggressively: clean tree,
#      green local-gate full, successful workspace release build. We script
#      the gate so it is reproducible and never skipped under time pressure.
#
# WHAT: Validates working-tree cleanliness, builds the workspace in release
#      mode, runs scripts/local_gate.sh full, and creates an annotated git
#      tag `v$VERSION` -- but does NOT push it. Pushing is left as an
#      explicit follow-up command the operator must type by hand. SBOM
#      generation and cosign signing are also flagged as manual follow-ups.
#
# HOW: VERSION=0.2.0 scripts/release.sh
#      VERSION=0.2.0 SKIP_GATE=1 scripts/release.sh   # emergency hotfix
#                                                    # (discouraged; sets
#                                                    # SKIP_GATE marker in
#                                                    # tag message)
#
# Exit codes:
#   0  -- tag created locally, ready for manual push.
#   1  -- precondition failure (dirty tree, missing VERSION, gate failed,
#         build failed, tag already exists).
#
# This script never runs `git push`. That is intentional. See "MANUAL FOLLOW-UPS"
# at the end of a successful run.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

log() { printf '==> %s\n' "$*"; }
err() { printf 'ERROR: %s\n' "$*" >&2; }

# --- Precondition 1: VERSION env var present and well-formed ----------------
if [ -z "${VERSION:-}" ]; then
    err "VERSION env var is required. Example: VERSION=0.2.0 scripts/release.sh"
    exit 1
fi

# Accept SemVer 2.0.0 core (MAJOR.MINOR.PATCH) plus optional pre-release
# (-alpha.1, -rc.2, etc). Reject anything else so we do not produce tags
# that cargo-dist or downstream packagers will refuse.
if ! printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'; then
    err "VERSION '$VERSION' is not a valid SemVer string."
    err "Examples: 0.1.0, 0.2.0-rc.1, 1.0.0-alpha.3"
    exit 1
fi

TAG="v${VERSION}"
log "Preparing release ${TAG}"

# --- Precondition 2: working tree is clean ---------------------------------
# Why both `status --porcelain` AND `diff --quiet`: the former catches
# untracked files, the latter catches staged-but-uncommitted edits with
# explicit exit codes for chained checks.
DIRTY="$(git status --porcelain)"
if [ -n "$DIRTY" ]; then
    err "working tree is not clean. Stash or commit first."
    err "----- git status --porcelain -----"
    printf '%s\n' "$DIRTY" >&2
    exit 1
fi

if ! git diff --quiet HEAD; then
    err "uncommitted changes detected via 'git diff HEAD'."
    exit 1
fi

# --- Precondition 3: tag does not already exist ----------------------------
# Refuse to clobber an existing tag silently. Operator must delete it
# explicitly with `git tag -d v$VERSION` before re-running.
if git rev-parse --verify "refs/tags/${TAG}" >/dev/null 2>&1; then
    err "tag ${TAG} already exists locally. Delete it explicitly:"
    err "  git tag -d ${TAG}"
    exit 1
fi

# --- Step 1: workspace release build ---------------------------------------
log "cargo build --workspace --release"
if ! cargo build --workspace --release; then
    err "release build failed. Aborting before gate run."
    exit 1
fi

# --- Step 2: local-gate full ------------------------------------------------
# Per ADR-009 the local-gate is the canonical merge gate. We require the
# full variant (fmt + clippy + warnings-as-errors + tests + deny + MSRV +
# doc + CMake + CTest) so a release tag implies the same artefact has
# already passed every check that lands code on main.
if [ "${SKIP_GATE:-0}" = "1" ]; then
    log "SKIP_GATE=1 set -- bypassing local_gate.sh full (DISCOURAGED)"
    log "    Reason will be embedded in the tag message for forensics."
    GATE_NOTE="SKIP_GATE=1 was used during tagging; gate was NOT run."
else
    log "scripts/local_gate.sh full"
    if ! scripts/local_gate.sh full; then
        err "local_gate.sh full failed. Fix the violations and re-run."
        exit 1
    fi
    GATE_NOTE="scripts/local_gate.sh full passed."
fi

# --- Step 3: cargo-dist plan dry-run ---------------------------------------
# We intentionally do NOT run `cargo dist build` here -- that is a separate
# step the operator runs after pushing the tag, because the artifact
# directory it populates (target/distrib/) is large and the tag is the
# upstream-of-truth for the artifact set.
if command -v cargo-dist >/dev/null 2>&1 || cargo dist --version >/dev/null 2>&1; then
    log "cargo dist plan (dry-run, validates release config)"
    if ! cargo dist plan >/dev/null; then
        err "cargo dist plan failed. Fix workspace.metadata.dist before tagging."
        exit 1
    fi
else
    log "cargo-dist not installed -- skipping plan dry-run."
    log "    Install via: cargo install cargo-dist --version 0.31.0"
fi

# --- Step 4: create annotated tag ------------------------------------------
# Annotated (not lightweight) tags carry a message and are GPG-signable.
# We do not auto-sign here; signing decisions belong to the operator's
# git config (commit.gpgsign / tag.gpgsign).
COMMIT_SHA="$(git rev-parse HEAD)"
log "creating annotated tag ${TAG} on ${COMMIT_SHA}"
git tag -a "${TAG}" -m "silksurf ${TAG}

Commit:  ${COMMIT_SHA}
Built:   $(date -u '+%Y-%m-%dT%H:%M:%SZ')
Gate:    ${GATE_NOTE}

See docs/development/REPRODUCIBLE-BUILD.md for byte-for-byte reproduction
instructions and docs/development/RUNBOOK-TLS-PROBE.md for downstream
verification flows."

log "tag ${TAG} created locally."

# --- Step 5: print manual follow-ups ---------------------------------------
cat <<RELEASE_FOLLOWUP

================================================================================
MANUAL FOLLOW-UPS (this script intentionally does NOT do these for you)
================================================================================

1. Verify the tag locally:
       git show ${TAG}

2. Build release artifacts (writes to target/distrib/):
       cargo dist build

3. Generate the SBOM:
       scripts/generate_sbom.sh
       # writes releases/sbom.cdx.json

4. (Optional) Sign artifacts and SBOM with cosign keyless:
       cosign sign-blob target/distrib/*.tar.gz \\
              --output-signature target/distrib/sig.bundle
       cosign sign-blob releases/sbom.cdx.json \\
              --output-signature releases/sbom.sig

5. Push the tag to the canonical remote (this is the point of no return):
       git push origin ${TAG}

6. After the tag is on the remote, attach release artifacts via your
   chosen mechanism (manual upload to GitHub Releases, internal mirror,
   or perf-lab provisioning rsync).

If you need to abort BEFORE step 5, delete the local tag:
       git tag -d ${TAG}

================================================================================
RELEASE_FOLLOWUP
