#!/usr/bin/env bash
# lint_doc_links: verify that relative markdown links in live docs resolve.
#
# Scope = tracked *.md outside docs/archive/, docs/external_sources/,
# silksurf-extras/, vendor/, and diff-analysis/. Archived docs describe
# superseded states and legitimately reference removed files; external
# mirrors and reference checkouts are not authored here. Only inline
# [text](path) links with relative targets are checked -- http(s)/mailto
# links need the network and pure #fragment links need a markdown parser,
# so both stay out of a fast local gate.
#
# A broken relative link is a defect: it either points at a file that was
# moved without repairing inbound references, or it was wrong when written.

set -eu

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

fail=0
checked=0

while IFS= read -r file; do
    dir="$(dirname "$file")"
    # Extract inline link targets: [text](target). Reference-style links
    # and autolinks are rare in this tree and skipped.
    while IFS= read -r target; do
        case "$target" in
            http://*|https://*|mailto:*|\#*|"") continue ;;
            *" "*) continue ;;  # code fragments like ](&mut self), never a path
        esac
        # Strip a trailing #fragment from file targets.
        path="${target%%#*}"
        [ -n "$path" ] || continue
        if [ -e "$dir/$path" ] || [ -e "$path" ]; then
            checked=$((checked + 1))
        else
            echo "BROKEN: $file -> $target"
            fail=1
        fi
    done < <(grep -o '\](\([^)]*\))' "$file" 2>/dev/null | sed 's/^](//; s/)$//')
done < <(git ls-files '*.md' \
    | grep -vE '^(docs/archive/|docs/external_sources/|silksurf-extras/|vendor/|diff-analysis/)')

if [ "$fail" -ne 0 ]; then
    echo "lint_doc_links: FAIL -- broken relative links above"
    echo "  Fix: repair the link target or move the doc's reference"
    exit 1
fi
echo "lint_doc_links: OK ($checked relative links resolve)"
