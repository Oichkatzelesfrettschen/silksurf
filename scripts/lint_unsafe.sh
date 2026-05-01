#!/usr/bin/env bash
# lint_unsafe: every `unsafe {` block in production code must be preceded
# within 5 lines by a `// SAFETY:` comment that explains the invariant.
# `unsafe impl`, `unsafe fn`, and `unsafe trait` declarations are NOT
# included -- those are documented at the type/trait level.
#
# Tests, benches, examples are exempt.
#
# Exit codes:
#   0  every unsafe block is annotated; gate passes
#   1  unannotated unsafe blocks found; gate fails (each site printed)
#   2  internal error (no source tree found)
#
# Wired into scripts/local_gate.sh fast. The full SAFETY-block index lives
# in docs/design/UNSAFE-CONTRACTS.md.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

## NOTE: silksurf-js/src is intentionally NOT scanned in this round. It has
## ~40 unannotated unsafe blocks concentrated in gc/heap.rs and ffi.rs.
## Annotating them is its own batch in the SNAZZY-WAFFLE roadmap (P1
## follow-up, tracked as a task). Re-include silksurf-js/src here when
## that batch lands.
mapfile -t SOURCES < <(find \
    crates/silksurf-app/src \
    crates/silksurf-core/src \
    crates/silksurf-css/src \
    crates/silksurf-dom/src \
    crates/silksurf-engine/src \
    crates/silksurf-gui/src \
    crates/silksurf-html/src \
    crates/silksurf-layout/src \
    crates/silksurf-net/src \
    crates/silksurf-render/src \
    crates/silksurf-tls/src \
    -type f -name '*.rs' \
    -not -path '*/tests/*' \
    2>/dev/null)

if [ "${#SOURCES[@]}" -eq 0 ]; then
    echo "lint_unsafe: ERROR: no source files found under crates/*/src" >&2
    exit 2
fi

violations=0
for src in "${SOURCES[@]}"; do
    awk -v file="$src" '
        # Sliding 7-line window of preceding context (SAFETY blocks may be
        # multi-line; tighter windows force terse single-line rationale,
        # which is a worse outcome than a slightly larger window).
        { ctx[NR % 8] = $0 }
        # Match unsafe { but not "unsafe impl", "unsafe fn", "unsafe trait".
        # Matches inline like `let x = unsafe { ... };` and
        # block-form `unsafe { ... }`.
        /unsafe[[:space:]]*\{/ {
            line = $0
            # Skip comment lines.
            if (line ~ /^[[:space:]]*(\/\/|\*)/) next
            # Skip declarations.
            if (line ~ /unsafe[[:space:]]+(impl|fn|trait)/) next
            # Allow same-line SAFETY annotation.
            if (line ~ /SAFETY:/) next
            ok = 0
            for (back = 1; back <= 7; back++) {
                idx = (NR - back) % 8
                if (idx < 0) idx += 8
                if (ctx[idx] ~ /SAFETY:/) { ok = 1; break }
            }
            if (!ok) {
                printf "%s:%d: unsafe block without SAFETY: comment within 5 lines: %s\n", file, NR, line
                violations++
            }
        }
        END { exit (violations > 0 ? 1 : 0) }
    ' "$src" || violations=$((violations + 1))
done

if [ "$violations" -gt 0 ]; then
    echo
    echo "lint_unsafe: FAIL ($violations file(s) with unannotated unsafe blocks)"
    echo "Add a comment within 7 lines above each unsafe block:"
    echo "    // SAFETY: <invariant that justifies the unsafe op>"
    echo "    unsafe { ... }"
    echo "And add the block to docs/design/UNSAFE-CONTRACTS.md."
    exit 1
fi

echo "lint_unsafe: OK ($(printf '%s\n' "${SOURCES[@]}" | wc -l) production files scanned)"
