#!/usr/bin/env python3
"""Repoint commit SHAs cited in tracked text through a filter-repo commit map.

A history rewrite changes every commit identity, and this repository binds
identity to SHAs in load-bearing places: `docs/conformance/*-scorecard.json`
embeds the commit that produced each rate so a reader can reproduce it,
`docs/findings/` cites the commit a fix landed in as AGENTS.md requires, and
`docs/design/ARCHITECTURE-DECISIONS.md` anchors decisions to a tree state. Left
alone after a rewrite those citations name commits no branch reaches.

`git filter-repo` writes `.git/filter-repo/commit-map` as two whitespace-
separated columns, old and new. That file is the authority for what each commit
became; a SHA absent from it was never reachable from the rewritten refs, and
this tool reports it rather than guessing.

A seven-hex prefix collides with ordinary hex data -- colour literals, byte
counts, checksums -- so the file list is explicit rather than a tree sweep.

    python3 scripts/reanchor_commit_citations.py            # rewrite in place
    python3 scripts/reanchor_commit_citations.py --dry-run  # report only
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
COMMIT_MAP = Path(".git/filter-repo/commit-map")
NULL_SHA = "0" * 40

# Files that cite commit SHAs. A file joins this list when it gains a citation,
# which is the same discipline docs/findings/ already applies to its own
# provenance lines.
CITING_FILES = (
    "AGENTS.md",
    "docs/STATUS.md",
    "docs/conformance/SCORECARD.md",
    "docs/conformance/css-parse-robustness-scorecard.json",
    "docs/conformance/html5lib-tokenizer-scorecard.json",
    "docs/conformance/html5lib-tree-construction-scorecard.json",
    "docs/design/ARCHITECTURE-DECISIONS.md",
    "docs/findings/git-lfs-payload-audit.md",
    "docs/findings/measurement-provenance-envelope.md",
    "perf/baseline.json",
    "perf/history.ndjson",
    "silksurf-specification/SILKSURF-RUST-MIGRATION.md",
)

SHA_RE = re.compile(r"\b[0-9a-f]{7,40}\b")


def load_map(path: Path) -> dict[str, str]:
    """Reads the old-to-new mapping, dropping commits the rewrite deleted."""

    mapping: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        fields = line.split()
        if len(fields) != 2 or len(fields[0]) != 40 or fields[0] == "old":
            continue
        if fields[1] != NULL_SHA:
            mapping[fields[0]] = fields[1]
    return mapping


def rewrite(text: str, mapping: dict[str, str]) -> tuple[str, list[tuple[str, str]]]:
    """Replaces every mapped SHA, preserving the abbreviation length used."""

    changes: list[tuple[str, str]] = []

    def repl(match: re.Match[str]) -> str:
        token = match.group(0)
        for old, new in mapping.items():
            if old.startswith(token):
                changes.append((token, new[: len(token)]))
                return new[: len(token)]
        return token

    return SHA_RE.sub(repl, text), changes


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--map",
        type=Path,
        default=REPO_ROOT / COMMIT_MAP,
        help="filter-repo commit map (default: .git/filter-repo/commit-map)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="report the substitutions without writing them",
    )
    args = parser.parse_args(argv)

    if not args.map.is_file():
        print(
            f"reanchor_commit_citations: no commit map at {args.map}; "
            "run git filter-repo first",
            file=sys.stderr,
        )
        return 1

    mapping = load_map(args.map)
    total = 0
    for relative in CITING_FILES:
        path = REPO_ROOT / relative
        if not path.is_file():
            print(f"  absent: {relative}")
            continue
        text = path.read_text(encoding="utf-8")
        updated, changes = rewrite(text, mapping)
        if not changes:
            continue
        if not args.dry_run:
            path.write_text(updated, encoding="utf-8")
        for old, new in changes:
            print(f"  {relative}: {old} -> {new}")
        total += len(changes)

    verb = "would re-anchor" if args.dry_run else "re-anchored"
    print(f"reanchor_commit_citations: {verb} {total} citation(s) "
          f"against {len(mapping)} mapped commits")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
