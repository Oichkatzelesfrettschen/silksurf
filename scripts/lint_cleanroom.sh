#!/usr/bin/env bash
# lint_cleanroom: enforce the cleanroom boundary around diff-analysis/.
#
# diff-analysis/ is the reference-analysis tree: study of reference browsers,
# baselines, and generated evidence. The cleanroom rule (docs/CLEANROOM.md,
# CLAUDE.md) is that the boundary is one-directional -- production and the
# specification study reference material, but never depend on it. This gate
# locks that in so the boundary cannot silently erode.
#
# Two rules, both about a durable DEPENDENCY across the boundary, not a prose
# mention of it (describing the boundary is exactly what CLEANROOM.md does):
#
#   1. Production isolation. No production Rust source (crates/*/src,
#      silksurf-js/src) and no Cargo manifest may name diff-analysis at all --
#      an import, include_str!, path dependency, or build reference across the
#      boundary is a cleanroom violation.
#
#   2. Specification isolation. No file under silksurf-specification/ may point
#      at a SPECIFIC FILE inside diff-analysis (e.g. `diff-analysis/X.md`): the
#      spec is derived from first principles, not from a reference-analysis
#      document it cites as a dependency. A bare `../diff-analysis/` directory
#      mention that describes the boundary is allowed.
#
# Both currently hold; this gate keeps them holding.

set -eu

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

fail=0

# Rule 1: production source + manifests must not name diff-analysis.
while IFS= read -r file; do
    case "$file" in
        crates/*/src/*.rs | silksurf-js/src/*.rs | */Cargo.toml | Cargo.toml) ;;
        *) continue ;;
    esac
    if grep -nE 'diff[-_]analysis' "$file" >/dev/null 2>&1; then
        echo "PRODUCTION-CROSSING: $file references diff-analysis:"
        grep -nE 'diff[-_]analysis' "$file" | sed 's/^/  /'
        fail=1
    fi
done < <(git ls-files 'crates/*/src/*.rs' 'silksurf-js/src/*.rs' '*Cargo.toml' 'Cargo.toml')

# Rule 2: the specification tree must not depend on a file inside diff-analysis.
# The pattern requires a path segment and an extension after diff-analysis/,
# so `../diff-analysis/` (the boundary description) does not match but
# `../diff-analysis/PHASE-2-RESEARCH-SYNTHESIS.md` (a content pointer) does.
while IFS= read -r file; do
    if grep -nE 'diff-analysis/[^ )"`'"'"']+\.[A-Za-z0-9]+' "$file" >/dev/null 2>&1; then
        echo "SPEC-CROSSING: $file points at a file inside diff-analysis:"
        grep -nE 'diff-analysis/[^ )"`'"'"']+\.[A-Za-z0-9]+' "$file" | sed 's/^/  /'
        fail=1
    fi
done < <(git ls-files 'silksurf-specification/*')

if [ "$fail" -ne 0 ]; then
    echo "lint_cleanroom: FAIL -- a dependency crosses the diff-analysis boundary above"
    echo "  Fix: production/spec study reference analysis but must not depend on it;"
    echo "       move the needed content out of diff-analysis or drop the pointer."
    exit 1
fi
echo "lint_cleanroom: OK (production and specification do not depend on diff-analysis)"
