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

That list is the weak point: a file gaining its first citation is invisible to
the rewrite pass until someone adds it, and the omission is silent. `--verify`
closes it from the other side by asking git what each hex token in tracked
authored text actually is. Two rules follow, each decidable:

- a token that prefixes an old commit-map entry and resolves to nothing is a
  citation the rewrite pass missed;
- a file holding a token that resolves to a commit is a citing file, so it
  belongs on `CITING_FILES` before the next rewrite.

Ordinary hex data satisfies neither and goes unreported. Colour literals and
checksums outnumber real citations by roughly 18 to 1 here, so a rule requiring
every hex token to resolve would be noise rather than a gate.

    python3 scripts/reanchor_commit_citations.py            # rewrite in place
    python3 scripts/reanchor_commit_citations.py --dry-run  # report only
    python3 scripts/reanchor_commit_citations.py --verify   # gate: no stale citation
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
COMMIT_MAP = Path(".git/filter-repo/commit-map")
NULL_SHA = "0" * 40

# Anchors that name no reachable commit and are kept as written. Repointing one
# changes what its ADR claims to codify, so `adr-anchor-reachability` in
# docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md owns the repair and this map is
# the exemption it earns. Resolving that entry empties this dict.
KNOWN_DANGLING = {
    "662ddb9": "AD-018 anchor; names no commit reachable from main",
    "418ea00": "AD-019 anchor; names no commit reachable from main",
    "63e7551": "AD-020 anchor; names no commit reachable from main",
}

# Authored trees. Reference checkouts and vendored sources cite their own
# upstream histories, which this repository does not rewrite.
EXCLUDED_PREFIXES = (
    "docs/external_sources/",
    "silksurf-extras/",
    "vendor/",
    "diff-analysis/",
)

SWEEP_GLOBS = ("*.md", "*.json", "*.ndjson", "*.toml", "*.sh", "*.py", "*.rs")

# Files that cite commit SHAs. A file joins this list when it gains a citation,
# which is the same discipline docs/findings/ already applies to its own
# provenance lines.
CITING_FILES = (
    "AGENTS.md",
    "docs/STATUS.md",
    # An archived document records a superseded state, and a SHA is part of what
    # it records. Re-anchoring keeps that citation naming the same content;
    # exempting the tree would leave it naming nothing.
    "docs/archive/roadmaps/SNAZZY-WAFFLE-COMPLETION.md",
    "docs/conformance/SCORECARD.md",
    "docs/conformance/css-parse-robustness-scorecard.json",
    "docs/conformance/html5lib-tokenizer-scorecard.json",
    "docs/conformance/html5lib-tree-construction-scorecard.json",
    "docs/design/ARCHITECTURE-DECISIONS.md",
    "docs/findings/git-lfs-payload-audit.md",
    "docs/findings/local-gate-reachability.md",
    "docs/findings/measurement-provenance-envelope.md",
    "docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md",
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


def git(*args: str) -> str:
    """Runs git in the repository and returns stdout."""

    return subprocess.run(
        ["git", *args],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout


def sweep_tokens() -> dict[str, set[str]]:
    """Maps each hex token in tracked authored text to the files carrying it."""

    tokens: dict[str, set[str]] = {}
    for relative in git("ls-files", *SWEEP_GLOBS).splitlines():
        if relative.startswith(EXCLUDED_PREFIXES):
            continue
        path = REPO_ROOT / relative
        # A symbolic link carries no bytes of its own; the citation lives in
        # its target, which the sweep reaches under the target's own path.
        if path.is_symlink():
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        for token in SHA_RE.findall(text):
            tokens.setdefault(token, set()).add(relative)
    return tokens


def resolving_commits(tokens: list[str]) -> set[str]:
    """Returns the tokens git resolves to a commit, in one batch-check pass."""

    if not tokens:
        return set()
    query = "".join(f"{token}^{{commit}}\n" for token in tokens)
    completed = subprocess.run(
        ["git", "cat-file", "--batch-check"],
        cwd=REPO_ROOT,
        input=query,
        capture_output=True,
        text=True,
        check=False,
    )
    lines = completed.stdout.splitlines()
    # batch-check answers every query on its own line, so a short reply means
    # git failed rather than that nothing resolved. Pairing by position would
    # turn that into an empty result set and a silently passing gate.
    if len(lines) != len(tokens):
        raise RuntimeError(
            f"git cat-file --batch-check answered {len(lines)} of {len(tokens)} "
            f"queries: {completed.stderr.strip()}"
        )
    resolved: set[str] = set()
    for token, line in zip(tokens, lines):
        # A resolvable object prints "<sha> commit <size>"; anything else
        # prints "<query> missing" or "<query> ambiguous".
        fields = line.split()
        if len(fields) == 3 and fields[1] == "commit":
            resolved.add(token)
    return resolved


def verify() -> int:
    """Reports citations a rewrite left stale and citing files off the list.

    Two rules, each sound on its own evidence. A token prefixing an *old*
    commit-map entry and resolving to nothing is a citation the rewrite pass
    missed, which is decidable only while the map exists. A file carrying a
    token that resolves to a commit is a citing file, so it belongs on
    CITING_FILES before the next rewrite whether or not it is stale today.
    Ordinary hex data -- colour literals, checksums, byte counts -- dangles
    under both rules and neither reports it.
    """

    tokens = sweep_tokens()
    resolved = resolving_commits(sorted(tokens))
    failures = 0

    map_path = REPO_ROOT / COMMIT_MAP
    if map_path.is_file():
        old_shas = set(load_map(map_path))
        for token, files in sorted(tokens.items()):
            if token in resolved or token in KNOWN_DANGLING:
                continue
            if any(old.startswith(token) for old in old_shas):
                for relative in sorted(files):
                    print(f"STALE CITATION: {relative} -> {token} (pre-rewrite)")
                    failures += 1
    else:
        # A clone has never had a commit map, so this is the common path for
        # everyone but the operator who ran the rewrite.
        print(
            "reanchor_commit_citations: stale-citation rule not run "
            "(no .git/filter-repo/commit-map to compare against)"
        )

    registered = set(CITING_FILES)
    for token in sorted(resolved):
        for relative in sorted(tokens[token]):
            if relative not in registered:
                print(f"UNREGISTERED CITER: {relative} cites {token}")
                print("  add it to CITING_FILES so the next rewrite re-anchors it")
                failures += 1

    for token, reason in sorted(KNOWN_DANGLING.items()):
        if token in resolved:
            print(f"KNOWN_DANGLING is stale: {token} now resolves ({reason})")
            failures += 1

    if failures:
        print(f"reanchor_commit_citations: FAIL -- {failures} citation defect(s)")
        return 1
    print(
        f"reanchor_commit_citations: OK ({len(resolved)} commit citations across "
        f"{len(registered)} registered files, {len(KNOWN_DANGLING)} accepted dangling)"
    )
    return 0


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
    parser.add_argument(
        "--verify",
        action="store_true",
        help="report stale citations and citing files absent from CITING_FILES",
    )
    args = parser.parse_args(argv)

    if args.verify:
        return verify()

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
