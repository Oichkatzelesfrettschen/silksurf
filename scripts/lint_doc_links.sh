#!/usr/bin/env bash
# lint_doc_links: verify that relative markdown links in live docs resolve.
#
# Scope = tracked *.md outside any archive/ directory, docs/external_sources/,
# silksurf-extras/, vendor/, and diff-analysis/. Archived docs describe
# superseded states and legitimately reference removed files; external
# mirrors and reference checkouts are not authored here. Only inline
# [text](path) links with relative targets are checked -- http(s)/mailto
# links need the network and pure #fragment links need a markdown parser,
# so both stay out of a fast local gate.
#
# A broken relative link is a defect: it either points at a file that was
# moved without repairing inbound references, or it was wrong when written.
#
# Backtick-quoted repository paths are checked on the same grounds. Prose
# cites a crate, module, or script far more often through `crates/foo/src/bar`
# than through a markdown link, so checking link syntax alone leaves most
# path claims unverified. A candidate qualifies when it starts with a tracked
# top-level directory, which keeps URL paths (`/results/?q=x`), corpus
# classification directories (`/invalid/`), and system paths (`/usr/lib/...`)
# out. A leading slash is stripped first: several docs anchor at the
# repository root that way, and the path underneath is what must resolve.

set -eu

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

fail=0
checked=0

# Paths a doc may name that the working tree legitimately lacks: harness
# output produced by a run, and the untracked reference checkouts AGENTS.md
# keeps out of the repository.
path_is_exempt() {
    case "$1" in
        silksurf-extras/*|silksurf-js/test262*|target/*) return 0 ;;
        crates/silksurf-engine/conformance/h2spec-results.txt) return 0 ;;
        *) return 1 ;;
    esac
}

# A backtick span names a repository path when it starts with a tracked
# top-level directory. Anything else in backticks is code, a command, a crate
# name, or a URL path, and carries no filesystem claim.
path_is_repo_rooted() {
    case "$1" in
        crates/*|docs/*|scripts/*|fuzz/*|silksurf-js/*|silksurf-specification/*) return 0 ;;
        perf/*|diff-analysis/*|vendor/*|silksurf-extras/*|.github/*) return 0 ;;
        *) return 1 ;;
    esac
}

while IFS= read -r file; do
    dir="$(dirname "$file")"

    # Backtick-quoted repository paths. Findings are exempt: AGENTS.md makes
    # them chronological records, so one names the file a defect lived in and
    # the wrong path a doc used to carry, both of which are gone by the time
    # the finding is written. Their markdown links stay checked below.
    case "$file" in
        docs/findings/*) span_check=skip ;;
        *) span_check=run ;;
    esac
    while IFS= read -r span; do
        [ "$span_check" = run ] || continue
        case "$span" in
            *" "*|*"*"*|*"?"*|*"="*|*"%"*|*"<"*|*">"*|*"|"*|*'$'*|*"("*|*"{"*) continue ;;
            http://*|https://*|"") continue ;;
        esac
        candidate="${span#/}"
        # Prose cites a symbol as `path::symbol` and a site as `path:line`;
        # the path is what must resolve in both.
        candidate="${candidate%%::*}"
        case "$candidate" in
            *:[0-9]*) candidate="${candidate%%:[0-9]*}" ;;
        esac
        case "$candidate" in */*) ;; *) continue ;; esac

        case "$candidate" in
            ./*|../*)
                # A traversal prefix inside backticks quotes a pattern far more
                # often than it claims a file: an ES module specifier
                # (`./mod.js`) or a lint's match pattern (`../diff-analysis/`).
                # Markdown links already carry the relative-link check.
                continue
                ;;
            .*/*)
                # Only three dot-directories belong in checked-in prose:
                # `.cargo` and `.github` are tracked, and `.git` is the
                # repository itself, which the hook runbooks cite. Every other
                # one is a local tool's state directory that a reader cloning
                # this repository does not have -- an editor profile, an agent
                # scratch tree. The allowlist is deliberate rather than a
                # filesystem test, because such a directory is usually present
                # on the author's machine and absent everywhere else.
                case "${candidate%%/*}" in
                    .cargo|.git|.github) checked=$((checked + 1)) ;;
                    *)
                        echo "PATH OUTSIDE REPOSITORY: $file -> \`$span\`"
                        fail=1
                        ;;
                esac
                continue
                ;;
        esac

        path_is_repo_rooted "$candidate" || continue
        path_is_exempt "$candidate" && continue
        # A trailing slash marks a directory; -e accepts either form.
        if [ -e "${candidate%/}" ]; then
            checked=$((checked + 1))
        else
            echo "BROKEN PATH: $file -> \`$span\`"
            fail=1
        fi
    done < <(grep -o '`[^`]*`' "$file" 2>/dev/null | tr -d '`')

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
    | grep -vE '(^|/)archive/' \
    | grep -vE '^(docs/external_sources/|silksurf-extras/|vendor/|diff-analysis/)')

if [ "$fail" -ne 0 ]; then
    echo "lint_doc_links: FAIL -- broken relative links above"
    echo "  Fix: repair the link target or move the doc's reference"
    exit 1
fi
echo "lint_doc_links: OK ($checked relative links resolve)"
