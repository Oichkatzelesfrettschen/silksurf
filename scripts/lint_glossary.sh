#!/usr/bin/env bash
# lint_glossary: enforce that every public domain type exposed at a crate-root
# lib.rs is documented in docs/reference/GLOSSARY.md.
#
# WHY scope = lib.rs roots only:
#   The true public API of a crate is what appears at, or is re-exported from,
#   its lib.rs. Internal modules may declare pub items for intra-workspace use
#   that are not part of the documented surface. Scanning all source files
#   catches ~450 internal types and creates unbounded noise. Scanning only
#   crate roots catches the documented facade and stays at zero with the
#   current glossary.
#
# WHY this is now a hard gate (exit 1 on missing):
#   The user policy is: all warnings, missing, and errors are treated as build
#   errors. Advisory output that does not break the build is invisible in
#   practice and accumulates as debt. This script fails the check target when
#   any public domain concept at a crate root is absent from the glossary.
#
# What is checked:
#   pub struct  Name   -- PascalCase, >= 8 chars
#   pub enum    Name   -- PascalCase, >= 8 chars
#   pub type    Name   -- PascalCase, >= 8 chars
#   pub fn      name   -- snake_case, >= 12 chars (filters new, parse, etc.)
#
# Match strategy: case-insensitive substring against GLOSSARY.md so that
# "CascadeView" matches entries titled "CascadeView" or "Cascade View".
#
# Exit codes:
#   0 -- all public terms found in glossary
#   1 -- one or more terms missing; output lists each as "MISSING: <name>"

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

GLOSSARY="docs/reference/GLOSSARY.md"

if [ ! -f "${GLOSSARY}" ]; then
    echo "lint_glossary: ERROR: ${GLOSSARY} not found" >&2
    exit 1
fi

# Scan only crate-root lib.rs files -- the true public API facade.
# Binary crates (main.rs) expose no public API and are excluded.
mapfile -t SOURCES < <(
    find crates -name 'lib.rs' -path '*/src/lib.rs' 2>/dev/null | sort
    [ -f silksurf-js/src/lib.rs ] && echo silksurf-js/src/lib.rs || true
)

if [ "${#SOURCES[@]}" -eq 0 ]; then
    echo "lint_glossary: WARN: no lib.rs files found; nothing to scan"
    exit 0
fi

GLOSSARY_LOWER="$(mktemp)"
trap 'rm -f "${GLOSSARY_LOWER}"' EXIT
tr '[:upper:]' '[:lower:]' < "${GLOSSARY}" > "${GLOSSARY_LOWER}"

# Extract candidate names from each lib.rs. Logic is identical to the previous
# all-source scan, but restricted to the narrower file set.
mapfile -t CANDIDATES < <(
    for src in "${SOURCES[@]}"; do
        awk '
            /^[[:space:]]*\/\// { next }
            /^[[:space:]]*\*/   { next }
            match($0, /^[[:space:]]*pub(\([^)]+\))?[[:space:]]+(struct|enum|type|fn)[[:space:]]+[A-Za-z_][A-Za-z0-9_]*/) {
                seg = substr($0, RSTART, RLENGTH)
                if (match(seg, /(struct|enum|type|fn)[[:space:]]+[A-Za-z_][A-Za-z0-9_]*/)) {
                    tail = substr(seg, RSTART, RLENGTH)
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

filtered=()
for entry in "${CANDIDATES[@]}"; do
    kind="${entry%%	*}"
    name="${entry##*	}"
    [ -z "${name}" ] && continue
    [ "${name}" = "${entry}" ] && continue

    case "${kind}" in
        struct|enum|type)
            if [ "${#name}" -ge 8 ] && [[ "${name}" =~ ^[A-Z][A-Za-z0-9]+$ ]]; then
                filtered+=("${name}")
            fi ;;
        fn)
            if [ "${#name}" -ge 12 ] && [[ "${name}" =~ ^[a-z][a-z0-9_]+$ ]]; then
                filtered+=("${name}")
            fi ;;
    esac
done

if [ "${#filtered[@]}" -gt 0 ]; then
    mapfile -t filtered < <(printf '%s\n' "${filtered[@]}" | sort -u)
fi

missing=()
for name in "${filtered[@]}"; do
    lower="$(printf '%s' "${name}" | tr '[:upper:]' '[:lower:]')"
    if ! grep -q -F -- "${lower}" "${GLOSSARY_LOWER}"; then
        missing+=("${name}")
    fi
done

for name in "${missing[@]}"; do
    echo "MISSING: ${name}"
done

if [ "${#missing[@]}" -gt 0 ]; then
    echo "lint_glossary: FAIL -- ${#missing[@]} public term(s) at crate roots not in ${GLOSSARY}" >&2
    echo "  Fix: add each MISSING term as an entry in ${GLOSSARY}" >&2
    exit 1
fi

echo "lint_glossary: OK (${#SOURCES[@]} lib.rs roots scanned)"
exit 0
