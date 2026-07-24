#!/usr/bin/env bash
# lint_ascii: verify that authored documentation is pure ASCII.
#
# Scope = tracked *.md outside docs/archive/, docs/external_sources/,
# silksurf-extras/, vendor/, and diff-analysis/. Archived docs are
# historical snapshots retained verbatim; external mirrors and reference
# checkouts are not authored here. The repo policy is ASCII-only authored
# prose: non-ASCII bytes are almost always smart quotes, em dashes, or
# emoji pasted from a rendering surface, and they break greps, diffs, and
# terminal review. The 2026-07 normalization sweep brought authored docs
# to zero non-ASCII bytes; this gate keeps them there.
#
# This is the final authored-documentation gate invoked by `make check`.
# After byte-level validation it runs the canonical status-consistency check,
# which compares current prose against manifests and scorecards.

set -eu

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

fail=0
checked=0

while IFS= read -r file; do
    checked=$((checked + 1))
    # grep -P '[^\x00-\x7F]' finds any byte outside the ASCII range.
    if hits="$(grep -nP '[^\x00-\x7F]' "$file" 2>/dev/null)"; then
        fail=1
        printf 'lint_ascii: non-ASCII in %s\n' "$file"
        printf '%s\n' "$hits" | head -5 | sed 's/^/    /'
    fi
done < <(git ls-files '*.md' \
    | grep -v -e '^docs/archive/' -e '^docs/external_sources/' \
              -e '^silksurf-extras/' -e '^vendor/' -e '^diff-analysis/')

if [ "$fail" -ne 0 ]; then
    echo "lint_ascii: FAIL (fix or move the file into an excluded tree)"
    exit 1
fi

echo "lint_ascii: OK ($checked authored markdown files are ASCII-clean)"
python3 scripts/check_status_consistency.py
