#!/usr/bin/env bash
# Reproduces the semgrep sweep behind
# docs/findings/browser-engine-size-and-complexity-comparison.md.
#
# p/owasp-top-ten is a web-application ruleset. Over browser engine sources it
# reports mostly plaintext-http-link on documentation URLs, so the output
# supports no memory-safety comparison; the finding records that rather than
# the counts. A ruleset that would is the open work.
#
# The reference checkouts live in the untracked silksurf-extras/ tree.
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
extras="${repo_root}/silksurf-extras"
output_dir="${1:-${repo_root}/diff-analysis/tools-output/semgrep}"
mkdir -p "$output_dir"

for browser in dillo Amaya-Editor ladybird elinks-0.13-20251230 links-links2 \
               lynx2.9.2 w3m tkhtml3 sciter servo netsurf-main neosurf-fork; do
    checkout="${extras}/${browser}"
    if [ ! -d "$checkout" ]; then
        echo "=== ${browser}: absent from ${extras}; skipping"
        continue
    fi
    echo "=== Scanning ${browser} ==="
    (cd "$checkout" && semgrep --config=p/owasp-top-ten --json \
        --output "${output_dir}/${browser}.json") 2>&1 | grep -A 7 "Scan Summary" || true
done
