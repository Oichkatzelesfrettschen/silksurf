# Git-LFS Payload and Repository History Weight

**Date**: 2026-08-07
**Last verified**: 2026-08-07
**Evidence class**: repository object measurement on this checkout (rank 1) plus
`.gitattributes` and roadmap source (rank 4).
**Mechanism**: `.gitattributes` routes three patterns through git-lfs:
`diff-analysis/tools-output/**`, `*.perf.data`, and `*.zst`.
`git lfs ls-files`, `git rev-list --objects --all` piped through `git cat-file
--batch-check`, and `du` over `.git` measure what those rules cost.
**Question**: a browser engine targeting a low-resource profile carries a
large-file extension. What does LFS hold, does anything consume it, and what
would removing it actually save?

## Verdict

LFS holds generated analysis output that nothing reads, and it is 69 percent of
the git directory. It is also not the largest thing in the history: compiled
binaries from the retired C tree are.

The two problems have different remedies, and only one of them requires
rewriting history.

## What LFS carries

155 objects, 104.5 MiB. All 155 are reachable from HEAD --
`git lfs ls-files` and `git lfs ls-files --all` both report 155, so no LFS
object exists only in history.

| Path | Objects |
| --- | --- |
| `diff-analysis/tools-output/afl-corpus/parser/seeds` | 100 |
| `diff-analysis/tools-output/lizard` | 15 |
| `diff-analysis/tools-output/semgrep` | 13 |
| `diff-analysis/tools-output/tokei` | 12 |
| `diff-analysis/tools-output/boa-profiling` | 10 |
| `diff-analysis/tools-output/test262-boa-baseline` | 3 |
| `diff-analysis/tools-output/infer` | 1 |
| `silksurf-js/heaptrack.parser_profile.1798871.zst` | 1 |

The last row is a blanket-rule accident rather than a decision: `*.zst` matches
anywhere, and a heaptrack profile left in `silksurf-js/` was captured by it.

`diff-analysis/` tracks 195 files. Exactly the 155 under `tools-output/` are
LFS; the other 40 are the hand-written analysis markdown that gives the
reference tree its value.

## Nothing consumes it

AGENTS.md walls production code and `silksurf-specification/` off from
`diff-analysis/`, and `scripts/lint_cleanroom.sh` enforces that as a gate.
Outside `diff-analysis/` itself, the only files naming `tools-output` are
`.gitattributes` and two documents describing the problem.

`docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md` already tracks the intent under
`tool-output-relocation`: generated evidence that belongs at an ignored path or
a dated evidence area outside the reference tree. The LFS store is the cost of a
relocation that has not happened.

## Footprint

| Measure | Bytes |
| --- | --- |
| `.git` | 147 MB |
| `.git/lfs` | 102 MB |
| `.git/objects/pack` | 42 MB |
| GitHub server-side repository size (LFS excluded) | 41.8 MB |
| checkout excluding `target/`, `.git/`, `silksurf-extras/`, `vendor/` | 209 MB |

The working tree is not part of that cost. This checkout has no LFS smudge
filter configured, so every tracked LFS path holds a 129-byte pointer rather
than content. The 93 MB `du` reports for `diff-analysis/` is almost entirely
three local `*.perf.data` captures that `.gitignore:23` excludes and git never
saw.

## The history weight is not LFS

Uncompressed blob bytes across every reachable object, grouped by top-level
path:

| Path | Uncompressed |
| --- | --- |
| `build-asan/` | 43.0 MiB |
| `diff-analysis/` plain blobs | 40.8 MiB |
| `docs/external_sources/` | 38.5 MiB |
| everything else | 45.2 MiB |

`build-asan/` holds compiled ELF: a 15.1 MB `silksurf` binary and roughly twelve
2.3 MB test binaries. It appears in two commits and left tracking at `8e6d201`
("chore: add build-*/ to .gitignore, remove build-asan from tracking"). It is
absent from the working tree and `.gitignore:6` covers it. It is dead weight in
the pack and nothing else.

The `diff-analysis/` plain blobs predate the LFS rules, so the largest files
exist twice: `tools-output/lizard/ladybird.csv` (9.7 MB) and
`tools-output/tokei/ladybird.json` (8.2 MB) are each a loose blob in history and
an LFS object today.

`docs/external_sources/` is 27 files tracked at HEAD, mostly specification PDFs
(`html-living-standard.pdf` at 15.6 MB, plus `css21.pdf` and `css22.pdf`). That
is provenance-managed source material with a verification script, so it is
weight the repository chose rather than accumulated.

## What each remedy actually buys

**Removing the paths at the tip.** `git clone` fetches LFS objects for the
checked-out ref. Every one of the 155 objects is reachable from HEAD, so a fresh
clone downloads all 104.5 MiB today. Deleting `diff-analysis/tools-output/` and
the stray `.zst` at the tip and dropping the `.gitattributes` rules takes a
fresh clone's LFS download to zero. Old commits keep their pointers and fetch on
demand only if someone checks one out. This needs no history rewrite and breaks
no commit identity.

**Rewriting history.** The prize is the roughly 84 MiB of `build-asan/` and
pre-LFS `diff-analysis/` blobs, of which about 42 MB survives packing, plus
server-side LFS storage. The cost is that every commit SHA changes, and this
repository binds identity to SHAs in several load-bearing places:

- the three conformance scorecards embed `measurement_environment.git.commit`,
  naming the commit that produced each rate;
- `docs/findings/` cite commits directly, which AGENTS.md requires: "landed in
  640166e (silksurf_core::engine_protocol wire framing)";
- commit trailers use `Fixes:` against earlier commits;
- 82 merged pull requests on the remote hold the old SHAs.

A rewrite makes every one of those references unreachable. The provenance
envelope in particular exists to let a reader reproduce a rate from the commit
that produced it, so rewriting history would falsify the artifacts that record
it.

## Measured rewrite result

The rewrite was built and verified in a scratch clone before any decision.
`git filter-repo --invert-paths` over `diff-analysis/tools-output/`,
`build-asan/`, and the stray `.zst` takes `.git` from 45 MB to 26 MB, a 42
percent reduction. All 350 commits survive -- none becomes empty -- and 308 of
them take new identities. The tree at HEAD is byte-identical
(`993fb385e7724ee75c61025a27c8ff1e49e515c3` before and after), so the working
copy does not move.

`scripts/reanchor_commit_citations.py` repoints the citations through the
`commit-map` that rewrite writes: 13 across 12 files, including the three
`measurement_environment.git.commit` values and the ADR, finding, and baseline
anchors. Each rewritten SHA resolves in the new history and the old one does
not, which is the check that the substitution happened rather than the text
merely changing.

Two limits stay after a force-push. GitHub keeps the pre-rewrite objects alive
through 65 `refs/pull/*` refs, so the server-side repository size does not drop
without support intervention; a fresh clone still gets the smaller history,
because clone fetches branch and tag refs alone. And `Fixes:` trailers inside
old commit bodies keep naming pre-rewrite SHAs, since a rewrite cannot repoint a
reference held in the object it rewrites.

## Three ADR anchors already dangle

`docs/design/ARCHITECTURE-DECISIONS.md` anchors four decisions with `codifies
design from main = <sha>`. Three of them -- `662ddb9` (AD-018), `418ea00`
(AD-019), and `63e7551` (AD-020) -- are not ancestors of `main`. They resolve
through the GitHub API, so some pull-request ref still reaches them, and they do
not resolve in a fresh clone. Only `1066d3a` is on the main line.

This predates any rewrite and survives one: a SHA outside the rewritten refs
carries no `commit-map` entry, so the tool reports it rather than guessing a
substitute. The condition is recorded rather than repaired because repointing
would change what each ADR claims to codify.

## Sequence taken

The tip-level removal landed first and on its own. It captures the whole
LFS clone cost -- 104.5 MiB to zero -- at no cost to commit identity, and it
closes `tool-output-relocation` rather than working around it. What the dumps
supported was extracted before they left: the size and complexity tables into
`docs/findings/browser-engine-size-and-complexity-comparison.md`, the boa
test262 baseline into
`docs/archive/conformance/boa-upstream-test262-scorecard.json`, 100
test262-derived seeds into `fuzz/corpus/js_runtime/`, and the small perf reports
and flamegraphs into `docs/findings/data/`.

The history rewrite follows as a separate, verified operation with its
re-anchoring step, on the measurement above.

## Falsifiers

- A fresh network clone after tip-level removal still downloads LFS bytes, which
  would mean `git clone` fetches beyond the checked-out ref.
- Some file outside `diff-analysis/` reads `tools-output/`, which would make it
  a live input rather than dead evidence.
- `git lfs ls-files --all` reports more objects than `git lfs ls-files` after a
  future commit, which would mean history-only LFS objects exist and tip removal
  no longer captures the whole payload.

## Evidence commands

```sh
git lfs ls-files | wc -l
git lfs ls-files --all | wc -l
du -sh .git .git/lfs .git/objects/pack
git rev-list --objects --all | git cat-file --batch-check='%(objecttype) %(objectname) %(objectsize) %(rest)'
git grep -ln 'tools-output' -- ':!diff-analysis/**'
```
