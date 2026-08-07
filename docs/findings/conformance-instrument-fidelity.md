# Conformance Instrument Fidelity

**Date**: 2026-08-06
**Mechanism**: Three harnesses that reported success were measured against the
upstream corpora they name. `crates/silksurf-html/tests/html5lib_harness.rs`
ran the full html5lib tokenizer corpus instead of a nine-description
allowlist; a new `crates/silksurf-html/tests/html5lib_tree_construction.rs`
ran WPT `html/syntax/parsing/resources` through `silksurf_html::parse_html`;
and `crates/silksurf-css/tests/css_harness.rs` ran the WPT CSS subset already
present in `silksurf-extras/`.
**Question**: the conjecture under test was "SilkSurf is a fully standards
compliant browser/engine for embedded standards with a fully functional UI
with elegant UX". `README.md` and `docs/STATUS.md` already refute every clause
in the project's own words, so the question that earns work is the
second-order one: does the instrument that would detect drift survive the same
scrutiny?

## Verdict

It does not. Three gates reported success while asserting nothing, and one of
them reproduced its own defect inside the repair.

The strongest single item is self-inflicted. The new tree-construction
harness first printed `conformance=100.00% of executed` because `rate_executed`
summed genuine passes and recorded known failures into one numerator. The
metric defect this work exists to remove reproduced itself in the instrument
built to remove it, within an hour of the original being named. Gate status
and conformance now print as separate lines: the gate is green whenever no
unexpected failure appears, at any conformance rate.

### What the instrument said, and what it says now

| Harness | Claimed | Measured |
|---|---|---|
| html5lib tokenizer | 9/9 hand-picked descriptions | 3,019 / 6,640 executed = **45.47%** |
| HTML tree construction | not run | 1,440 / 1,726 executed = **83.43%** |
| CSS over upstream WPT | 6 in-tree fixtures | 603 / 603 accepted, robustness oracle only |

`crates/silksurf-html/tests/html5lib_harness.rs` returned early -- and
therefore passed -- when the corpus was absent, then asserted over the nine
entries of `HTML5LIB_TEST1_PASS_DESCRIPTIONS`. A green `make test` proved
nothing about HTML tokenization. Both harnesses now skip with a remedy when
their corpus is absent and fail hard when an operator names a path that does
not resolve.

### Where the failures concentrate

Tree construction: 199 of 286 recorded gaps reach one cause. `silksurf_dom::NodeKind`
carries neither a template-content fragment nor a processing-instruction
variant, so `crates/silksurf-html/src/treesink.rs` drops template children
(109 cases) and renders a processing instruction as a comment (90 cases).
html5ever itself is spec-grounded; the losses sit in the SilkSurf adapter.

Tokenization: 2,283 of 3,621 recorded gaps are named character references left
as source text, and 1,238 are `test3` state permutations. The `State` enum in
`crates/silksurf-html/src/lib.rs` carries 8 states against roughly 80 in the
standard.

The CSS number needs its oracle stated to be honest. `parse_case` accepts a
file when `parse_stylesheet_bytes` returns without an error or a panic, and
discards the parsed stylesheet. 603 / 603 measures parser robustness over the
corpus and carries no claim about cascade or computed-value correctness.

### Corpus provenance

html5lib retired its own `tree-construction/` directory. The pinned revision
`224991ec` carries the subject "Tree construction tests have moved to WPT", so
no revision bump recovers it; `scripts/fetch_html_css_test_corpora.sh` now
sparse-checks out `html/syntax/parsing/resources`, where the `.dat` files
live. The sibling `html5lib_*.html` wrappers drive those files through
testharness.js, which an engine-level runner does not need.

Each upstream scorecard records its corpus revision, and
`scripts/check_status_consistency.py` rejects a scorecard whose revision
drifts from `silksurf-extras/html-css-test-corpora-revisions.txt` or whose
rate is absent from `docs/STATUS.md`. Both rejections were confirmed by
mutating a scorecard and observing the failure.

## Predictions that failed

Recording these because each deviation was the finding.

**"6 of 23 insertion modes proves HTML non-compliance" -- void.** The 5-mode
`TreeBuilder` in `crates/silksurf-html/src/tree_builder.rs` was unreachable
from production. `silksurf-engine` imports `silksurf_html::parse_html` as
`html5ever_parse`. The observation was a shadow-implementation debt finding,
not a compliance measurement.

**"The 70/70 harness exercises the shadow parser" -- void.**
`crates/silksurf-engine/src/bin/wpt_runner.rs` calls the production
`parse_html`; the hand-rolled `Tokenizer` appears only in
`extract_inline_style`. Tooling does lift `<style>` contents with a tokenizer
separate from the production path, which is a narrower harness-fidelity limit
than predicted.

**"`SilkError::HtmlTreeBuild` would break on removal" -- void.** The variant
holds a `String`, not a `TreeBuildError`.

## Second-order defects the repair surfaced

Removing the dead builder and widening `scripts/lint_doc_links.sh` exposed
defects that the narrow gates had hidden:

- The HTML fuzz target exercised the dead `TreeBuilder` while
  `parse_html` -- the code that handles every untrusted page -- was unfuzzed.
  `fuzz/fuzz_targets/html_parse.rs` now drives the production path.
- `fuzz/` declares its own `[workspace]`, so `make check`'s
  `cargo clippy --workspace` never reached it. A lint the main workspace
  rejects therefore landed in the new fuzz target and survived
  `cargo check`. `make check` now runs clippy inside `fuzz/` as a second
  invocation.
- `crates/silksurf-gui/README.md` described the crate as "**Currently a
  stub** (one doc-comment line in `lib.rs`)" and listed
  `src/{window,event_loop,input}.rs` as work to be authored. The crate is
  3,874 lines across those files plus a 2,450-line winit backend, which
  `crates/silksurf-app` selects by default. `docs/design/THREAT-MODEL.md`
  likewise credited the removed `TreeBuilder` for bounded recursion and
  described the retired hand-written VM.
- `docs/design/UNSAFE-CONTRACTS.md` and `docs/design/THREAT-MODEL.md`
  documented a KNOWN BUG at `silksurf-js/src/ffi.rs:271`, in a file AD-025
  deleted. The current surface is 74 `unsafe` blocks, every one a
  `boa_engine::NativeFunction::from_closure` call whose obligation is that the
  closure captures owned host handles rather than GC-managed values.
  `scripts/lint_unsafe.sh` excludes `silksurf-js/src`, so none are gated.
- `docs/development/RUNBOOK-TLS-PROBE.md` documented a smoke binary at
  `crates/silksurf-tls/src/bin/tls_probe.rs`. That crate declares no binary
  target and has no `bin` directory.
- `docs/conformance/SCORECARD.md` and
  `silksurf-specification/SILKSURF-RUST-MIGRATION.md` both cited
  `/.claude/plans/elucidate-and-build-out-snazzy-waffle.md`, an untracked path
  outside the repository. `scripts/lint_doc_links.sh` extracted only markdown
  inline-link syntax, so it saw neither. It now checks 486 paths against 8,
  and rejects a dot-directory outside a deliberate `.cargo`/`.git`/`.github`
  allowlist. A filesystem test was insufficient: `.claude/` exists untracked on
  the authoring machine, so the first version of the rule passed the very
  reference it was written to catch. Widening it surfaced eleven further stale
  references, among them a `KNOWN BUG` citation into a deleted file, a runbook
  documenting a binary target that does not exist, and seven doc paths in a
  "Key docs" list.
- `docs/conformance/SCORECARD.md` reported that silksurf exposes no
  accessibility tree. `crates/silksurf-app/src/accessibility.rs` builds an
  `accesskit::TreeUpdate` over six roles. The claim was right about exposure
  and wrong about construction: `Cargo.lock` carries `accesskit` with no
  platform adapter, and the only consumer prints a node count to stderr.

## Scope cuts

- `SilkError::HtmlTreeBuild(String)` is retained and currently unconstructible.
  `crates/silksurf-html/src/tree_builder.rs` held its only construction site,
  and `parse_html` discards parse errors. It stays as the home for `treesink`
  parse errors once that path surfaces them.
- 192 fragment cases skip in tree construction; `parse_html` is document-mode
  only, and `parse_fragment_into` takes a context element the `.dat` harness
  does not yet thread through.
- 166 tokenizer cases need `initialStates` beyond the data state or a
  `lastStartTag`, neither of which `silksurf_html::Tokenizer` exposes.

## Resolved and evolved conjecture

The conjecture is false on every clause. What survives falsification: SilkSurf
is a low-latency retained-rendering browser shell and integration layer over
third-party standards engines -- html5ever for tree construction, boa for
ECMAScript, Taffy for layout, rustls for TLS, tiny-skia for raster. Its
original contribution is the damage-driven repaint pipeline and the fused
style-layout-paint path, carrying rank-1 live evidence at roughly 100 us text
repaint and 190-260 us fused relayout.

The successor conjecture, stated to be falsifiable: **SilkSurf can carry HTML
4.8 embedded content on a real upstream-corpus instrument without surrendering
its retained-repaint latency budget.** Three falsifiers, in the order they
would bite:

1. A nested browsing context needs its own document, style tree, layout root,
   and paint subtree, while the shell owns one `BrowserPageRuntime`. If a
   second document forces full-page repaint, the latency claim and the
   capability claim are in direct conflict.
2. The tree-construction rate does not move when embedded-content elements
   land, which would mean the corpus does not reach them and a different
   instrument is needed.
3. `video` and `audio` need a media stack the workspace does not have; they
   stay named scope cuts in `docs/roadmaps/SPA-CAPABILITY-ROADMAP.md`.

`picture` and `srcset` are tractable inside the existing
`crates/silksurf-image` surface and are the cheapest real embedded-content
capability. The measured entry point for falsifier 1 is whether the damage
model survives a second document, which is a question to answer before writing
element support.

## Reproduce

```sh
scripts/fetch_html_css_test_corpora.sh
scripts/conformance_run.sh html5lib tree-construction css
python3 scripts/check_status_consistency.py
```
