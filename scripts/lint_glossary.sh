#!/usr/bin/env bash
# lint_glossary: surface public domain types/functions that have not yet
# been documented in docs/reference/GLOSSARY.md. Advisory only -- this
# is a hint to the author, not a CI gate. The intent is to keep the
# glossary in lockstep with the public surface as the codebase grows,
# without blocking PRs on documentation churn.
#
# What we look for:
#   pub struct  Name { ... }   -- PascalCase, >=8 chars
#   pub enum    Name { ... }   -- PascalCase, >=8 chars
#   pub type    Name = ... ;   -- PascalCase, >=8 chars
#   pub fn      name(...) {... -- snake_case, >=12 chars (filters out
#                                 trivial helpers like `pub fn new`,
#                                 `pub fn from_str`, etc.)
#
# Why those length thresholds: short PascalCase names (Dom, Css, Box) are
# usually unambiguous re-exports of well-known concepts; short snake_case
# names (new, len, push, parse) are conventional and would just create
# noise. The thresholds were tuned by sampling the existing surface and
# picking values where the false-positive rate dropped sharply.
#
# Match strategy: case-insensitive substring against GLOSSARY.md. We use
# substring (not whole-word) on purpose so that "CascadeView" matches a
# glossary entry titled "CascadeView" or even "Cascade View"; we use
# case-insensitive so contributors don't have to match snake_case vs
# PascalCase rendering exactly.
#
# Scope: same production-source file set as scripts/lint_unwrap.sh --
# crates/<crate>/src/ and silksurf-js/src/, excluding tests/, benches/,
# examples/, and bin/. Bin trees are CLI entry points whose internal
# types are not part of the documented surface.
#
# Exit code: ALWAYS 0. This script is advisory by design; CI must not
# fail on missing glossary entries (writers iterate faster than docs).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

GLOSSARY="docs/reference/GLOSSARY.md"

if [ ! -f "${GLOSSARY}" ]; then
    echo "lint_glossary: ERROR: ${GLOSSARY} not found" >&2
    # Still exit 0: we promised advisory-only behaviour, and a missing
    # glossary is an issue for a different gate to catch.
    exit 0
fi

# Same source-tree scope as lint_unwrap.sh. Keeping these in sync
# manually (rather than via a shared helper) is acceptable until a third
# script needs the same scope -- then extract.
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
    -not -path '*/benches/*' \
    -not -path '*/examples/*' \
    2>/dev/null)

if [ "${#SOURCES[@]}" -eq 0 ]; then
    echo "lint_glossary: WARN: no source files found; nothing to scan"
    echo "lint_glossary: 0 term(s) not in GLOSSARY.md (advisory)"
    exit 0
fi

# Lower-cased glossary text held in a temp file so we can grep it
# repeatedly without re-reading from disk per name. tr does the
# downcasing once.
GLOSSARY_LOWER="$(mktemp)"
trap 'rm -f "${GLOSSARY_LOWER}"' EXIT
tr '[:upper:]' '[:lower:]' < "${GLOSSARY}" > "${GLOSSARY_LOWER}"

# Collect candidate names. Awk strips the identifier from each pub line;
# we then sort -u to dedupe (same name may appear in multiple files via
# re-exports).
#
# Awk regex anchored at start-of-line (allowing leading whitespace)
# rejects lines like `// pub struct X` (comment) and `(pub struct Y)`
# (parameter list). The strip step takes everything up to the first
# non-identifier character (`<`, `(`, `{`, whitespace, `;`, `=`).
mapfile -t CANDIDATES < <(
    for src in "${SOURCES[@]}"; do
        awk '
            # Skip comment lines so we do not pick up names inside doc
            # comments like /// pub struct Foo.
            /^[[:space:]]*\/\// { next }
            /^[[:space:]]*\*/   { next }

            # Match: optional whitespace, "pub", optional visibility
            # qualifier in parens (pub(crate) etc), then the kind.
            match($0, /^[[:space:]]*pub(\([^)]+\))?[[:space:]]+(struct|enum|type|fn)[[:space:]]+[A-Za-z_][A-Za-z0-9_]*/) {
                # Extract just the identifier. The matched span includes
                # the leading "pub ... struct " prefix; we slice past it.
                seg = substr($0, RSTART, RLENGTH)
                # Find the kind keyword and take the word after it.
                if (match(seg, /(struct|enum|type|fn)[[:space:]]+[A-Za-z_][A-Za-z0-9_]*/)) {
                    tail = substr(seg, RSTART, RLENGTH)
                    # Strip the kind keyword + whitespace.
                    sub(/^(struct|enum|type|fn)[[:space:]]+/, "", tail)
                    kind_match = seg
                    sub(/[[:space:]]+[A-Za-z_][A-Za-z0-9_]*$/, "", kind_match)
                    sub(/^.*[[:space:]]/, "", kind_match)
                    print kind_match "\t" tail
                }
            }
        ' "${src}"
    done | sort -u
)

# Filter to items meeting our domain-concept thresholds.
filtered=()
for entry in "${CANDIDATES[@]}"; do
    # entry is "kind<TAB>name"
    kind="${entry%%	*}"
    name="${entry##*	}"
    # Defensive: if the split failed, skip.
    [ -z "${name}" ] && continue
    [ "${name}" = "${entry}" ] && continue

    case "${kind}" in
        struct|enum|type)
            # Require PascalCase (starts with uppercase, contains no
            # underscore in the middle of identifier-style names) and
            # length >= 8 to filter out short re-exports.
            if [ "${#name}" -ge 8 ] \
               && [[ "${name}" =~ ^[A-Z][A-Za-z0-9]+$ ]]; then
                filtered+=("${name}")
            fi
            ;;
        fn)
            # Require snake_case (lowercase + underscores + digits) and
            # length >= 12 to filter trivial helpers (new, from_str,
            # build, parse_one). 12 chars is the empirical threshold
            # below which most fn names are language-convention
            # boilerplate rather than domain concepts.
            if [ "${#name}" -ge 12 ] \
               && [[ "${name}" =~ ^[a-z][a-z0-9_]+$ ]]; then
                filtered+=("${name}")
            fi
            ;;
    esac
done

# Dedupe filtered list (the candidate sort -u already deduped on
# kind+name, but two kinds can share a name in extreme cases).
if [ "${#filtered[@]}" -gt 0 ]; then
    mapfile -t filtered < <(printf '%s\n' "${filtered[@]}" | sort -u)
fi

# Check each name against the lowercased glossary as a substring.
missing=()
for name in "${filtered[@]}"; do
    lower="$(printf '%s' "${name}" | tr '[:upper:]' '[:lower:]')"
    if ! grep -q -F -- "${lower}" "${GLOSSARY_LOWER}"; then
        missing+=("${name}")
    fi
done

# Report. Each missing name on its own line, prefixed with "MISSING:"
# so editor jump-to-error pickers and grep -E '^MISSING:' both work.
for name in "${missing[@]}"; do
    echo "MISSING: ${name}"
done

echo "lint_glossary: ${#missing[@]} term(s) not in GLOSSARY.md (advisory)"
exit 0
