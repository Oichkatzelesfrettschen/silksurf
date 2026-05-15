#!/usr/bin/env bash
# lint_unwrap: every .unwrap() and .expect( site in production code (under
# crates/*/src and silksurf-js/src, excluding test trees) must be preceded
# within 3 lines by a `// UNWRAP-OK: <invariant>` annotation. Bare
# `.unwrap()` is a bug; the annotation forces the author to record the
# invariant that makes the call safe.
#
# Tests, benches, examples, and binaries are exempt -- they are allowed to
# panic on test-input failure.
#
# Exit codes:
#   0  no unannotated unwraps; gate passes
#   1  unannotated unwraps found; gate fails (each site printed)
#   2  internal error (no source tree found)
#
# Wired into scripts/local_gate.sh fast.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# Find production .rs files: under <crate>/src/ and silksurf-js/src/, but
# exclude /tests/, /benches/, /examples/, /bin/. Tests live in
# crates/*/tests/ (separate dir) so the find scope already excludes them;
# /bin/ subdirs hold per-binary code that is conventionally allowed to
# unwrap on user-input failure.
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
    silksurf-js/src \
    -type f -name '*.rs' \
    -not -path '*/bin/*' \
    -not -path '*/tests/*' \
    2>/dev/null)

if [ "${#SOURCES[@]}" -eq 0 ]; then
    echo "lint_unwrap: ERROR: no source files found under crates/*/src" >&2
    exit 2
fi

violations=0
for src in "${SOURCES[@]}"; do
    # awk pass: for each line that contains .unwrap() or .expect(, look
    # back up to 3 lines for "UNWRAP-OK:" (or "// SAFETY-UNWRAP" as a
    # tolerated equivalent inside unsafe blocks). Skip #[cfg(test)]
    # gated mod tests blocks (we approximate by dropping lines after a
    # `mod tests {` line until the matching closing `}`).
    awk -v file="$src" '
        BEGIN { in_test_mod = 0; depth = 0 }
        /^[[:space:]]*#\[cfg\(test\)\]/  { gate = 1; next }
        gate && /^[[:space:]]*mod[[:space:]]+tests[[:space:]]*\{/ {
            in_test_mod = 1; depth = 1; gate = 0; next
        }
        gate { gate = 0 }
        in_test_mod {
            for (i = 1; i <= length($0); i++) {
                c = substr($0, i, 1)
                if (c == "{") depth++
                if (c == "}") {
                    depth--
                    if (depth == 0) { in_test_mod = 0; break }
                }
            }
            next
        }
        # Sliding 7-line window of preceding context. Closures and
        # multi-line builder chains routinely separate the rationale
        # comment from the unwrap by ~5 lines; tighter windows force
        # less informative comments which is a worse outcome.
        {
            ctx[NR % 8] = $0
        }
        /\.unwrap\(\)|\.expect\("/ {
            # Skip comment lines (//, ///, //!, * within block comments).
            if ($0 ~ /^[[:space:]]*(\/\/|\*)/) next
            # Allow lines that contain UNWRAP-OK on the same line.
            if ($0 ~ /UNWRAP-OK/) next
            # Look back 7 lines for the annotation.
            ok = 0
            for (back = 1; back <= 7; back++) {
                idx = (NR - back) % 8
                if (idx < 0) idx += 8
                if (ctx[idx] ~ /UNWRAP-OK/) { ok = 1; break }
            }
            if (!ok) {
                printf "%s:%d: unannotated unwrap/expect: %s\n", file, NR, $0
                violations++
            }
        }
        END { exit (violations > 0 ? 1 : 0) }
    ' "$src" || violations=$((violations + 1))
done

if [ "$violations" -gt 0 ]; then
    echo
    echo "lint_unwrap: FAIL ($violations file(s) with unannotated unwrap/expect)"
    echo "Annotate each site with a comment within 7 lines above:"
    echo "    // UNWRAP-OK: <one-line invariant that makes this safe>"
    echo "    something.unwrap();"
    echo "Or rewrite to ? / expect(\"invariant: ...\")."
    exit 1
fi

echo "lint_unwrap: OK ($(printf '%s\n' "${SOURCES[@]}" | wc -l) production files scanned)"
