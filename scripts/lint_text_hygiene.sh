#!/usr/bin/env bash
# lint_text_hygiene: reject emoji and typographic substitutes in authored docs.
#
# Scope = tracked *.md outside docs/archive/, docs/external_sources/,
# silksurf-extras/, vendor/, and diff-analysis/. Archived docs are historical
# snapshots retained verbatim; external mirrors and reference checkouts are not
# authored here.
#
# The gate rejects two classes. An emoji carries no information its word does
# not, and it breaks greps, widens diffs, and renders as a box or a double-width
# cell wherever the glyph is missing. A typographic substitute -- a curly quote,
# an en or em dash, an ellipsis glyph, a non-breaking space -- changes bytes
# without changing meaning, so it buys diff noise and grep misses for nothing.
#
# Symbols that carry meaning pass: mathematical operators, Greek letters, arrows
# in state transitions, box-drawing in diagrams, the degree and micro signs, and
# CJK. A name keeps the spelling its owner uses, so accented characters in author
# and copyright lines pass. crates/silksurf-core/src/psl.rs handles IDN public
# suffixes such as the .公司 TLD, and prose describing that code quotes the
# suffix it handles.
#
# This is the final authored-documentation gate invoked by `make check`. After
# text validation it runs the canonical status-consistency check, which compares
# current prose against manifests and scorecards.

set -eu

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# Denied ranges, in order: emoji and pictographs; miscellaneous symbols;
# dingbats; emoji variation selectors; zero-width joiner; curly quotes; en and
# em dash; ellipsis; non-breaking space; non-breaking hyphen.
DENY='[\x{1F000}-\x{1FAFF}\x{2600}-\x{27BF}\x{FE0E}\x{FE0F}\x{200D}\x{2018}-\x{201F}\x{2013}\x{2014}\x{2026}\x{00A0}\x{2011}]'

fail=0
checked=0

while IFS= read -r file; do
    checked=$((checked + 1))
    if hits="$(grep -nP "$DENY" "$file" 2>/dev/null)"; then
        fail=1
        printf 'lint_text_hygiene: emoji or typographic substitute in %s\n' "$file"
        printf '%s\n' "$hits" | head -5 | sed 's/^/    /'
    fi
done < <(git ls-files '*.md' \
    | grep -v -e '^docs/archive/' -e '^docs/external_sources/' \
              -e '^silksurf-extras/' -e '^vendor/' -e '^diff-analysis/')

if [ "$fail" -ne 0 ]; then
    echo "lint_text_hygiene: FAIL (use the plain word, straight quotes, -- and ...)"
    exit 1
fi

echo "lint_text_hygiene: OK ($checked authored markdown files carry no emoji or typographic substitutes)"
python3 scripts/check_status_consistency.py
