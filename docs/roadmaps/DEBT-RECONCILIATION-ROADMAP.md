# Debt Reconciliation Roadmap

**Date**: 2026-07-09
**Scope**: full-repository debt inventory and reconciliation plan
**Evidence base**: cflow/cscope call-graph capture of the legacy C GUI path,
cargo-modules structure of silksurf-engine, lizard complexity census of
crates/silksurf-app/src/main.rs, five bounded scans (Rust workspace, JS
runtime, legacy C tree, documentation coherence, build/test/fuzz
infrastructure), and a tracked-file non-ASCII sweep. Every finding below
carries a file or file:line citation. Line numbers reflect the tree at the
date above; the file and symbol names are the durable anchors.

This roadmap sequences the paydown of debt that SNAZZY-WAFFLE-COMPLETION.md
left open and that the scans surface. Each workstream carries a mechanism
name; each task carries a descriptive slug, an evidence gate, and explicit
dependencies. Tasks marked **LANDED** are complete with the gate evidence
noted.

---

## Debt taxonomy

The inventory classifies every finding into one of these classes:

| Class | Definition | Primary carrier in this repo |
|---|---|---|
| structural | code organized against its own architecture | 14319-line main.rs; two dormant parallel implementations |
| shadow-implementation | complete retired implementations still shipping in-tree | src/ C tree (~10.9k LOC); silksurf-js legacy VM (~10k LOC) |
| governance-pointer | build files and docs citing decisions that do not exist where cited | phantom deprecation citations (repaired); missing porting map (written) |
| code | markers, panic paths, suppressions, unsafe surface | layout unresolved-dimension panics (funneled); 17-lint allow in main.rs |
| test | coverage gaps, orphaned harnesses, unexercised claims | tracked ELF in tests/ (removed); MSRV never exercised; miri/fuzz opt-in |
| metric | numbers framed to overstate what evidence supports | test262 executed-vs-total denominators (now both published) |
| documentation | stale, contradictory, or broken authored prose | CLAUDE.md CMake claim; two competing indexes; broken links |
| build/workspace | dual build systems, dead targets, orphan config | broken make gui (removed); orphan vendor/; ignore-rule inversion (repaired) |
| security | deferred protection mechanisms and accepted advisories | privacy/sandbox stubs (AD-022); 2 RUSTSEC ignores |
| conformance | spec-coverage gaps behind the passing suites | Intl/ICU deferred (AD-021); async, static ESM, and a per-test loop budget now execute in test262 (dynamic-import/import.meta/TLA still skipped) |
| verification | formal models that assert nothing | BrowserLoader.cfg declares no INVARIANT or PROPERTY |
| hygiene | stray artifacts and policy violations in tracked files | non-ASCII in 34 authored docs; cleanroom-tree accumulation |

Three cross-cutting observations shape the sequencing:

1. **Shadow-implementation multiplier.** Every browser subsystem exists two
   or three times (live Rust crate, legacy C module, and for JS a dormant
   custom VM). Searches, line counts, marker sweeps, and complexity metrics
   over the tree overstate the live surface by roughly 2x. 33 of the 39 C
   markers and all 20 custom-VM markers sit in code no default build
   executes. Excising the shadows is the highest-leverage single move:
   it shrinks the audit surface of every later workstream.
2. **Aggregation, not tangle.** main.rs holds 404 functions at average
   cyclomatic complexity 3.0 with zero functions above the CC-16 gate
   (lizard). The god-file is an aggregation problem; the split is mechanical
   file surgery, not a risky refactor.
3. **Pointer rot outranks prose rot.** Three documents cited AD-007 (Damage
   Tracking) for a C-tree deprecation decision that no ADR recorded. AD-024
   (Legacy C Tree Retirement) now records it and the citations are
   repaired. Fixing pointers is cheap and unblocks the largest excisions.

---

## governance-pointer-repair

Restores a single truthful chain from governance docs to build reality.
No dependency; runs first.

- **legacy-c-retirement-adr** -- LANDED. AD-024 recorded in
  docs/design/ARCHITECTURE-DECISIONS.md with ledger row; AD-002 marked
  C-side-superseded. Gate: ADR ledger lists AD-024.
- **phantom-deprecation-citation-repair** -- LANDED. Makefile,
  docs/development/LOCAL-GATE.md, docs/REPO-LAYOUT.md repointed from
  AD-007 to AD-024; LOCAL-GATE.md additionally corrected: the gate never
  ran cmake/ctest, so the "kept green by the gate" claim is gone.
  Gate: `rg 'ADR-007'` returns only damage-tracking contexts.
- **legacy-c-porting-map** -- LANDED. docs/LEGACY_C_PORTING.md written
  (C module -> owning Rust crate, per-module status); resolves the
  dangling src/README_LEGACY.md reference.
- **claude-md-build-reality-rewrite** -- LANDED. Root CLAUDE.md rewritten:
  make check/test/full entry points, crates/*/src implementation target,
  AD-024 retirement noted, no phase labels; names AGENTS.md as
  authoritative.
- **readme-front-door-rewrite** -- LANDED. README.md rewritten to the
  Rust workspace (crate map, gate commands, dual-denominator conformance
  pointers, actual MIT/Apache dual license); every link resolves to an
  existing file.
- **single-index-consolidation** -- LANDED. DOCUMENTATION-INDEX.md is now
  a pointer to docs/README.md (the canonical index).
- **phase-roadmap-archival** -- LANDED. Both phase-labeled roadmaps moved
  to docs/archive/roadmaps/ with supersession banners; inbound references
  in BUILD.md, GLOSSARY.md, and ARCHITECTURE-DECISIONS.md repointed; the
  SNAZZY-WAFFLE item-21 archival claim is now true.
- **cross-reference-link-repair** -- LANDED for the live tree:
  silksurf-specification/README.md rewritten (link fixed to
  diff-analysis/PHASE-2-RESEARCH-SYNTHESIS.md, C-era framing dropped,
  ADR governance stated); the SILKSURF-OPTIMIZATION-STRATEGY.md dangling
  reference now lives only in the archived Phase-3 roadmap, which
  carries a supersession banner. The automated fence is
  docs-link-checker-gate below.
- **gate-docstring-alignment** -- LANDED. Makefile header and
  local_gate.sh usage both name lint_glossary.

## repo-hygiene-excision

Deletes stray artifacts and makes ignore rules truthful. No dependency.

- **dead-plusCal-source-removal** -- LANDED. BrowserLoader.old deleted;
  docs/REPO-LAYOUT.md row updated.
- **tracked-elf-artifact-removal** -- LANDED. tests/test_tokenizer (82 KB
  ELF) deleted and ignore rule added; `git check-ignore` confirms.
- **ignore-rule-inversion-repair** -- LANDED. The no-op /fuzz_in/ and
  /fuzz_in_css/ ignore rules are removed; the tracked seeds stay
  (fuzz_in/css.dict is consumed by css-fuzz-run and not regenerated) and
  the trees retire wholesale with the C tree.
- **fuzz-corpus-consolidation**: fuzz/corpus/ (live cargo-fuzz) stays;
  fuzz_in/, fuzz_in_css/, and fuzz_corpus/ are legacy AFL seed sets for
  the C fuzzers and follow the C tree's retirement.
- **stray-database-file-removal** -- LANDED: both empty mydatabase.db
  files removed (root + diff-analysis); *.db ignore rule confirmed.
- **vendor-dir-wiring-verified** -- LANDED (audit correction: NOT
  orphaned). vendor/wayland-scanner-0.31.10 is a workspace member wired
  through a Cargo [patch] path dependency pinning quick-xml
  (Cargo.toml); the audit had looked only for a .cargo vendored-sources
  stanza. Kept as-is.
- **perf-history-churn-relocation** -- LANDED: run_baselines.sh appends
  to the ignored perf/results/history.local.ndjson;
  check_perf_regression.sh compares the newest local row against the
  newest curated row in the tracked perf/history.ndjson; milestone
  promotion is an explicit documented step (tail -n 1 local >> tracked).
- **authored-docs-ascii-normalization** -- LANDED: 41 authored docs
  normalized via a mapping script (box drawing, arrows, check/cross
  marks, emoji status chips, smart punctuation, math glyphs); tracked
  sweep now returns only external_sources, vendored, and corpus paths.

## legacy-c-tree-retirement

Executes AD-024. Depends on governance-pointer-repair's first three
tasks (all LANDED). The module -> crate map lives in
docs/LEGACY_C_PORTING.md.

- **dead-c-code-deletion** -- LANDED. src/document/tree_builder.c and its
  orphaned include/silksurf/tree_builder.h deleted (built by no target,
  included by nothing); empty src/core/ removed; broken Makefile `gui`
  target (referenced nonexistent src/css/cascade.c) removed with its
  .PHONY entry; both CMake exclusion regexes cleaned. Gate: CMake
  configure passes post-deletion.
- **bpe-tokenizer-re-home** -- LANDED. Ported to
  silksurf_core::bpe::BpeTokenizer (Vec-backed byte trie, no unsafe,
  10 unit tests) with a criterion bench mirroring the C fixture
  (crates/silksurf-core/benches/bpe.rs); src/neural/ and
  include/silksurf/neural_bpe.h deleted, bpe-bench Makefile target and
  CMake exclusion removed; porting map updated. AD-024 step 2 executed.
- **c-ffi-shim-retirement** -- LANDED (AD-024 step 3 executed).
- **duplicated-c-module-removal** -- LANDED (AD-024 step 4 executed):
  src/, include/, tests/ (C sources and orphaned fixtures),
  CMakeLists.txt, and lsan.supp deleted -- 106 files; the dead
  measure_performance.sh (ctest-driven) went with them.
- **c-build-target-removal** -- LANDED: cmake-build/cmake-clean/
  layout-test/fuzz-build/fuzz-run/css-fuzz-run/infer* targets and the
  RICING_FLAGS/GUI_LIBS/CMAKE_FLAGS variables removed; AFL seed trees
  (fuzz_in/, fuzz_in_css/, fuzz_corpus/) deleted
  (fuzz-corpus-consolidation executed with them).
- **c-marker-closure** -- LANDED by deletion; the owning crates already
  implement the behavior.
- **c-doc-reality-updates** -- LANDED: REPO-LAYOUT.md, LOCAL-GATE.md,
  BUILD.md (rewritten to the Rust build), TOOLCHAIN.md, nested
  docs/development/AGENTS.md, .github/README.md (rewritten: the old one
  described a cmake CI pipeline that never existed under AD-009),
  GLOSSARY.md CMake entry, and the porting map's removal record.
- Gate met: make check green and 582 workspace tests pass post-excision.

## legacy-vm-quarantine-or-removal

silksurf-js ships a complete dormant JS engine beside the live boa
backend. The live path is SilkContext (src/lib.rs re-export;
boa_backend/mod.rs); the custom VM, bytecode compiler, lexer, parser, GC,
and JIT are gated behind the non-default `legacy-vm` feature that
Cargo.toml itself labels "NOT MAINTAINED ... expect bitrot".

- **legacy-vm-removal** -- LANDED (owner decision, recorded as AD-025
  with the engine audit: boa_engine confirmed as the runtime). Deleted:
  src/vm/, src/bytecode/, src/lexer/, src/parser/, src/gc/ (unsound mark
  phase gone with it), src/jit/ and the Cranelift set, src/ffi.rs,
  src/wasm.rs, src/napi.rs (both imported the gated VM and could not
  compile), src/verification.rs, the lexer-only test262 runner and
  harness, VM benches and examples, and features legacy-vm/jit/wasm/
  napi/mmap/neural/constrained with ~15 dependencies. Crate builds as a
  plain lib; SilkContext is the declared embedding surface.
- **conformance-lane-rewire** -- LANDED with the removal:
  scripts/conformance_run.sh test262 now drives test262_boa
  (TEST262_FULL=1 widens scope, TEST262_PATH selects a subset) and
  writes the dual-denominator scorecard.
- Gate: default and --features full builds green; 38 silksurf-js tests
  pass; Cranelift absent from cargo tree.

## panic-path-and-unsafe-hardening

Converts crash-on-invariant into recoverable error on the live path, and
shrinks/encapsulates the unsafe surface. Independent of other
workstreams.

- **layout-unresolved-dimension-funnel** -- LANDED. Twelve
  `unreachable!("em/rem ...")` sites (ten in taffy_layout.rs, one in
  flex.rs, one in lib.rs length_to_px -- one more than the audit's first
  count) now funnel through `unresolved_font_relative_px()`: debug and
  test builds fail loudly via debug_assert! so the gate catches cascade
  regressions; release builds degrade to a deterministic 0 px instead of
  aborting the frame. Both contracts covered by profile-gated tests
  (debug should_panic test passes in `cargo test`; release degradation
  test passes in `cargo test --release`).
- **interner-atom-expect-justification** -- LANDED (verified, no change
  needed): the site already carries a complete UNWRAP-OK annotation, and
  the invariant it claims holds -- Atom's field is private with only a
  read-only raw() accessor, so Atoms originate solely from intern(),
  which inserts before returning.
- **h2-batch-url-parse-once** -- LANDED (audit correction: the
  localhost-fallback unwrap lived in the fetch_parallel HTTP/2 batch
  path, not redirect handling). same_https_host now takes pre-parsed
  URLs; fetch_parallel parses each URL exactly once and a malformed URL
  sends the batch down the HTTP/1.1 path where fetch() returns a proper
  per-request error. The silent localhost substitution is gone;
  regression test in crates/silksurf-net/tests/client.rs.
- **treebuilder-document-expect-annotation**:
  silksurf-html/src/tree_builder.rs `expect("document node present")` --
  justify with an UNWRAP-OK annotation (lint_unwrap.sh convention) or
  make fallible.
- **wayland-shm-safe-wrapper**: crates/silksurf-gui/src/wayland_shm.rs
  carries ~11 unsafe blocks (mmap of the shm fd, foreign wl_display
  handle, raw mapped-buffer slices) -- the largest unsafe concentration in
  the tree. Extract a minimal safe wrapper type owning the mapping
  lifetime; document each remaining block against
  docs/design/UNSAFE-CONTRACTS.md.
- **sendptr-send-sync-soundness-proof**: `unsafe impl Send/Sync for
  SendPtr` (crates/silksurf-render/src/lib.rs) shares a raw pixel pointer
  across rayon threads; sound only under non-overlapping row partitions.
  Encode the partition proof in the type (split_at_mut-derived slices) or
  document the aliasing argument precisely.
- **audited-unsafe-singles-watchlist**: treesink QualName pointer
  extension (silksurf-html/src/treesink.rs, rationale in file header),
  from_utf8_unchecked in css tokenization (silksurf-css/src/lib.rs), SIMD
  fill/pack intrinsics (render/lib.rs; app/main.rs packers). These carry
  rationale today; the app-monolith split moves the packers into a module
  where the SAFETY comments are reviewable.
- Gate: make check green (lint_unsafe.sh enforces SAFETY comments);
  a cascade-regression fuzz input no longer aborts layout.

## suppression-paydown

Makes the lint surface honest. Depends on app-monolith-decomposition for
the main.rs items.

- **app-blanket-allow-retirement** -- LANDED. The 17-lint (in practice
  16 firing) crate-wide allow in main.rs is gone. Twelve lints fixed at
  the source (clippy --fix plus hand fixes: clone_from, let-else,
  by-reference parameters, char-by-value, union_rect returning Rect
  instead of an always-Some Option, exact-epsilon test comparison,
  underscored hex literals). Four remain as narrowly scoped allows with
  in-file rationale: too_many_arguments (argb_raster, page_build,
  window_frame -- explicit pixel-geometry parameters),
  cast_ptr_alignment (argb_raster SIMD, Vec<u32> alignment), float_cmp
  (input/window_frame test mods, exact pixel-aligned assertions), and
  wildcard_imports (the module-split crate-root re-export idiom,
  annotated at every use site).
- **workspace-lint-allowlist-triage**: [workspace.lints.clippy]
  blanket-allows ~35 pedantic/style lints; triage into (a)
  genuinely-wanted policy recorded with one-line rationale each, (b)
  droppable now, (c) droppable after the split and the SoA decision.
- **per-crate-allow-shrink**: per-crate #![allow] headers (css, html,
  render, net) shrink to what still fires; eight too_many_arguments
  allows (render/lib.rs, gui/winit_backend.rs) convert to parameter
  structs.
- **deny-policy-hardening**: deny.toml flips multiple-versions from
  "warn" to "deny" once the tree is clean; the two RUSTSEC ignores
  (RUSTSEC-2024-0436 paste, RUSTSEC-2026-0192 ttf-parser) track the
  boa/cosmic-text upgrades (security-substrate-buildout).
- **crate-metadata-provenance**: add license.workspace = true and
  description to all 13 crate manifests (publish=false today, provenance
  still owed).
- Gate: `rg '#\!\[allow' crates/` output shrinks to the recorded policy
  set; cargo deny check passes with deny-level multiple-versions.

## app-monolith-decomposition -- LANDED

main.rs (14319 lines, 404 functions) is now 539 lines: the module
declarations, the mimalloc allocator, main(), the winit event-loop
glue, and the headless static-render path. Twelve modules carry the
rest, cut by a deterministic extraction script driven by a symbol
outline, a cross-module call-edge map, and a per-test ownership table
(144 tests reassigned):

- browser_types (shared structs/enums/constants), app_options (CLI,
  observability install, legacy window modes), page_build (navigation
  payload, page build, script execution, revalidation), runtime_repaint
  (runtime tick, dirty-node/text repaint), window_frame (frame render,
  retained buffers, scroll caches), input (pointer/keyboard, address
  bar, forms, history), redraw_geometry (redraw-mode merge, damage-rect
  union -- the cluster four modules call), dom_hit_test (node
  classification, link/input hit tests), accessibility (accesskit tree,
  feature-gated as a whole), argb_raster (SIMD packers with SAFETY
  contracts, argb fills/blits, bitmap text, chrome drawing),
  page_resources (script/module/image URL extraction and fetching),
  test_support (#[cfg(test)] shared fixtures).
- Every planned sub-extraction (argb-simd-packer, cli-and-tracing-init,
  browser-shell-glue, in-file-test-relocation, script-tag-helper) landed
  inside this split.
- Gate evidence: main.rs 539 lines (< 1500); make full green (62 test
  suites, 145 app tests -- 141 default + 4 integration; 143 with
  --features accessibility); headless smoke exits 0 with "Pipeline
  complete"; --backend=winit smoke presents frames until timeout.
- app-blanket-allow-retirement landed with the split (see
  suppression-paydown).

## soa-migration-decision

- **soa-wire-or-delete** -- LANDED (delete). Investigation corrected two
  audit premises: the fused_pipeline "coupling" to StyleSoA was a doc
  comment, not a code edge, and that comment already carried the
  benchmark evidence the decision needed -- building StyleSoA costs ~4us
  at 50 nodes and erases the fused pipeline's win over the 3-pass
  baseline, which is why it was left as an on-demand API. No caller ever
  materialized for any of the three surfaces (grep: zero consumers
  outside self + own tests). All three are deleted: style_soa.rs
  (cs-soa), dimensions_soa.rs (dim-soa), display_list_batched.rs
  (batched-raster), their tests, features, and doc mentions. The SoA
  idea lives where it measurably pays: CascadeView (cascade_view.rs) is
  the shipped column-oriented view on the cascade hot path. Revival
  source: git history plus the measured rationale retained in
  fused_pipeline.rs.
- Gate met: zero dead_code allows remain (by deletion); workspace check
  green; perf guardrails untouched (no runtime path changed).

## security-substrate-buildout

Turns the security placeholders into mechanisms. AD-022 already scopes
the skeleton; this schedules the build-out.

- **cookie-jar-partitioned-store** -- LANDED (store + document.cookie;
  HTTP round-trip is the named follow-on). Cookie primitives are homed in
  `silksurf-net::cookie` (not the engine) because `silksurf-js`
  (document.cookie) cannot reach `silksurf-engine`; net is the one crate
  both consume. Real `Cookie` (domain/path/expiry/Secure/HttpOnly/
  SameSite), RFC 6265 `parse_set_cookie`, an HTTP-date parser, a
  `CookieStore` producing filtered `Cookie` headers and the
  `document.cookie` string, and `PartitionedCookieStore`
  (HashMap<key, store>). document.cookie now stores attribute-aware
  cookies, respects expiry, and refuses HttpOnly-from-script.
  `privacy::partition_key` is real (signature changed to
  `(top_level_origin, resource_origin) -> "<resource-site>^<top-level-
  site>"`) and `StoragePartition { key }` carries it; both have engine
  tests. 11 net cookie tests + 4 engine partition/sandbox tests.
  (SameSite enforcement and partition-keyed jars now landed -- see
  top-level-site-partitioning; eTLD+1 sites landed -- see
  public-suffix-list-etld1-sites.)
- **cookie-http-round-trip** -- LANDED (2026-07-11). `BasicClient` carries
  the `Cookie` request header (`with_cookie_jar`, `request_cookie_header`)
  and stores response `Set-Cookie` headers (`store_response_cookies`);
  `SpeculativeRenderer::attach_cookie_jar` rebuilds its client with the
  jar. One `Arc<Mutex<CookieStore>>` per session lives on
  `BrowserRenderConfig` (created once), attached to the worker-thread
  renderer AND handed to the main-thread `SilkContext::with_dom_and_cookies`
  (with the document host, so document.cookie is host-scoped and coherent
  with HTTP cookies) -- the thread split (fetch on worker, JS on main)
  forced `Arc<Mutex>`. Verified: 3 net round-trip tests (a local server's
  Set-Cookie is sent on the next request; a script-set cookie is sent; no
  jar sends nothing), a JS shared-jar bridge test, and a headless
  end-to-end smoke (server Set-Cookie `session=srv123` read back via
  `document.cookie` in a real page load).
- **top-level-site-partitioning** -- LANDED (2026-07-11). The session
  jar is now `PartitionedCookieStore` (per `(top_level_site,
  resource_site)`), and the top-level site is threaded per navigation so
  two features unlock at once:
  - **Partition-keyed jars**: `BasicClient` holds a `CookieContext { jar,
    top_level_site }`; each request keys its partition by
    `partition_key(top_level_site, site_of_url(request))`. A resource
    embedded under two different top-level sites gets two isolated
    stores. document.cookie reads/writes the first-party partition
    (`partition_key(top, top)`).
  - **SameSite subresource enforcement**: cross-site subresources
    withhold `SameSite=Strict` and `Lax` cookies; same-site subresources
    send them (`subresource_same_site_context`). Verified by a
    withhold/send test pair.
  - **Graceful degradation** (the safety net): an empty top-level site
    (an unplumbed path) maps to `SameSiteContext::Unknown` + the
    `UNPARTITIONED` store -- cookies still round-trip, just unenforced
    and unpartitioned (batch-11 behavior), never silently dropped.
  - Threading: `BrowserRenderConfig` gained `top_level_site`, set per
    navigation from the destination URL in `load_navigation_payload`
    (and once from the initial URL in `main`); it flows to
    `renderer.attach_cookie_context` and
    `SilkContext::with_dom_and_cookies`. Single-source: net's
    `partition_key` is the canonical live keyer (with the empty->
    unpartitioned rule); `privacy::partition_key` is the origin-based
    derivation with the same format for non-empty sites.
  - Verified: 6 net round-trip/enforcement tests (isolation by
    top-level site, cross-site withhold, same-site send, empty->
    unpartitioned, no-context), updated JS first-party-partition bridge
    test, and a headless e2e smoke (server Set-Cookie read back via
    document.cookie through the first-party partition).
  - Top-level-NAVIGATION SameSite now landed too -- see
    navigation-initiator-samesite.
- **navigation-initiator-samesite** -- LANDED (2026-07-12). Top-level
  navigations now enforce SameSite from the initiator site.
  `BrowserNavigationRequest` gained `initiator_site`, set on
  page-initiated navigations (link click, form GET/POST) from the current
  page URL -- which is still the OLD page when the request is built, since
  `state.frame` is replaced only on navigation completion -- and `None`
  for browser-initiated ones (address bar, bookmark, history, initial
  load). `silksurf_net::cookie::navigation_same_site_context(initiator,
  destination, safe_method)` classifies the top-level document fetch:
  `None`/same-site => SameSite (Strict sent); cross-site + safe method
  (GET) => CrossSiteTopLevel (Strict withheld, Lax rides); cross-site +
  unsafe method (POST) => CrossSiteSubresource (Lax withheld too, RFC
  6265bis). `BasicClient::fetch_navigation` computes it once and applies
  across the redirect chain; the renderer routes only the top-level
  document through it (subresources keep the subresource rule; the h2
  subresource path applies no cookies, pre-existing). Closes the CSRF
  exposure where a cross-site link click sent the destination's Strict
  cookies. Verified: classifier unit test + 3 net round-trip tests
  (same-site nav sends Strict; cross-site GET withholds Strict/sends Lax;
  cross-site POST withholds Lax) alongside the retained subresource pair.
  STILL deferred: redirect-hop reclassification (context frozen from
  initiator + original destination) and the Domain=<public-suffix>
  attribute rejection. AD-022 fifth amendment.
- **public-suffix-list-etld1-sites** -- LANDED (2026-07-11). Sites are now
  scheme + registrable domain (eTLD+1), not scheme + host. One entry point,
  `silksurf_core::psl::registrable_domain`, backs both site derivations
  (`sandbox::Origin::site` and `silksurf_net::cookie::site_of_url`), so
  classification and live cookie keying agree. Backed by a vendored Public
  Suffix List (`crates/silksurf-core/data/public_suffix_list.dat`, MPL-2.0,
  ICANN + PRIVATE sections). The matcher implements normal/wildcard/exception
  rules with longest-match-wins, exception priority, and the default `*` rule;
  U-label rules are Punycode-normalized via a now-direct `idna` dep (no new
  crate in the closure -- see AD-021 amendment) so IDN hosts are not
  over-grouped. `a.example.com`/`b.example.com` share a site; `a.co.uk`/
  `b.co.uk` and `a.github.io`/`b.github.io` do not; an IP/bare-suffix/
  `localhost` host keeps its full host (maximally partitioned). Verified: 7
  core matcher tests (incl. the a.co.uk/b.co.uk separation and IDN
  over-grouping guard), flipped net/sandbox site assertions, and a new
  sandbox registrable-domain-separation test. AD-021 + AD-022 amended.
- **site-isolation-registry** -- RE-SCOPED (the roadmap's "or re-scope"
  branch, per AD-022). The fake `SiteIsolation` registry (enforced
  nothing = false assurance AD-022 warns against) is replaced by real
  same-origin/same-site *classification* in `sandbox::Origin` (scheme/
  host/port, port-independent site). Enforcement (process model, IPC,
  seccomp/Landlock) stays deferred to the future process-model ADR. The
  duplicate `StoragePartition` (was in both privacy.rs and sandbox.rs) is
  folded into one in privacy.rs. Site is now scheme + registrable domain
  (eTLD+1) via `silksurf_core::psl` -- see public-suffix-list-etld1-sites.
- **rustsec-unblock-upgrades** -- RESOLVED as documented deferral (no
  upgrade exists). Both advisories are `informational = "unmaintained"`
  with `patched = []` -- not CVEs -- and boa_engine 0.21.1 / cosmic-text
  0.19.0 are the LATEST published versions (verified 2026-07-11). No
  upgrade clears them; the fix requires upstream (boa/fontdb) to drop the
  transitive dep, or migrating engines. deny.toml ignore rationale
  strengthened accordingly. GATE RE-SCOPED: "zero ignores" is unreachable
  without an upstream/engine migration; `cargo deny` already passes WITH
  the two informational ignores, which is the honest steady state.
- **a11y-substrate-scheduling** -- DEFERRED with ADR (AD-026). The
  page-content accessibility tree (`build_a11y_tree` DOM walker + AT-SPI)
  is deferred; browser *chrome* a11y already ships via AccessKit
  (crates/silksurf-app/src/accessibility.rs). a11y.rs module doc points
  at AD-026.
- Reconciliation (blocking, DONE): AD-022 amended (2026-07-11) and
  docs/design/THREAT-MODEL.md updated so the partial cookie/partition
  landing reads as partial, not as "privacy implemented."
- Gate: `make full` green (cargo deny passes with the two justified
  informational ignores); net cookie tests + engine partition tests land.

## benchmark-runnability-buildout

Removes the architectural blockers between the engine and standard
browser benchmarks (Speedometer, JetStream, Kraken, Octane,
MotionMark), diagnosed 2026-07-10 with per-suite failure points.

- **windowed-default-launch** -- LANDED. The winit browser is the
  default entry point; --headless selects the one-shot static render
  (previous default). --backend=winit stays accepted. AppOptions drops
  the dead winit_mode field; integration tests unaffected (invalid
  hosts fail at fetch before a window opens).
- **navigation-script-cap-parametrization** -- LANDED.
  max_navigation_script_bytes() defaults to 8 MiB (was a 256 KiB
  const; JetStream/Octane bundles run 2-5 MiB) with a
  SILKSURF_MAX_SCRIPT_BYTES override.
- **deadline-ordered-host-timers** -- LANDED. HostScheduler stores
  (id, Instant) timeouts and re-arming IntervalEntry periods;
  setTimeout/setInterval read the delay argument (clamped to 0ms per
  HTML); take_timer_callbacks drains due-first by deadline and
  requeues budget overflow; SilkContext::next_host_callback_deadline
  exposes the earliest deadline. New silksurf-js/tests/host_timers.rs
  pins defer/order/re-arm/clamp semantics.
- **event-loop-timer-self-wake** -- LANDED. WinitWindow::
  with_host_work_deadline registers a deadline source; about_to_wait
  arms ControlFlow::WaitUntil at min(redraw pacing, next JS timer), so
  the loop wakes and drains due callbacks without external events.
  Verified live: a 300ms setTimeout DOM mutation fires and repaints in
  the windowed browser (SILKSURF_TRACE_HOST_CALLBACKS trace).
- **xhr-compatibility-shim** -- LANDED. XMLHttpRequest is a global
  constructor over the blocking net path (silksurf-js boa_backend):
  open/setRequestHeader/send, status/statusText/responseText/response,
  readyState progression (UNSENT..DONE constants on the constructor),
  getResponseHeader/getAllResponseHeaders, and both the on-property
  handlers (onreadystatechange/onload/onerror) and addEventListener
  listener arrays. The request runs synchronously inside send() (there
  is no async transfer to cancel, so abort() is a post-hoc no-op),
  which matches synchronous XHR and serves harnesses that pull resource
  files. Five tests in silksurf-js/tests/xhr.rs drive a local HTTP echo
  server (GET/POST/headers/listeners) plus the connection-failure path.
- **performance-now-monotonic-timer** -- LANDED. performance.now() was
  a hard-coded 0.0 stub that made all self-timing meaningless; it now
  returns fractional milliseconds from a process-wide monotonic epoch
  (OnceLock<Instant>). Discovered while timing the Kraken run.
- **canvas-2d-substrate** -- LANDED. A working HTML canvas 2D context
  spanning three layers:
  - Rasterizer (silksurf-dom canvas2d.rs): `CanvasSurface`, a
    straight-alpha RGBA8 framebuffer with affine transforms,
    fill/clear/stroke rects, path construction + nonzero-winding fill,
    arcs and Bezier flattening, stroking, drawImage scaling, and
    getImageData/putImageData. Pure Rust, 9 unit tests. It lives on the
    `Dom` (a canvas element owns its bitmap), which the JS bridge and
    the paint pipeline already share -- so no registry threading.
  - JS bindings (boa_backend dom_bridge.rs): `element.getContext('2d')`
    returns a `CanvasRenderingContext2D` with fillStyle/strokeStyle/
    lineWidth/globalAlpha accessors (CSS color parsing: hex, rgb()/
    rgba(), named), the full draw-op method set, and getImageData/
    putImageData/drawImage. 10 integration tests verify real pixels
    through getImageData.
  - Compositing (fused_pipeline is_image_element + app page_resources):
    `TagName::Canvas` is a replaced element sized from width/height
    attributes; append_image_display_items snapshots the live surface
    into a render `ImageSurface` and emits `DisplayItem::Image`, reusing
    the entire existing image blit path with no new display-list
    variant. A page_resources unit test runs the real fused pipeline on
    a drawn canvas and confirms a correctly-sized Image item carrying
    the drawn pixels; a headless render of a canvas page composites
    without error.
  - Deferred follow-ons (documented, not blocking): fillText/measureText
    (needs a canvas font path), drawImage from `<img>` sources (their
    decoded pixels are URL-keyed in the app cache, not on the DOM node),
    and true `Uint8ClampedArray`/`ImageData` (getImageData returns a
    plain-array-backed object today).
- **async-done-completion-hook** -- LANDED. `$DONE` is a reusable async
  completion signal on `SilkContext` (boa_backend): `$DONE()` (or a
  falsy argument) passes, a truthy argument fails with a `name: message`
  string, a second call fails. `AsyncCompletion` (Pending/Passed/Failed)
  reports state; `drive_until_done(max_wall)` pumps the microtask queue
  and host timer callbacks (waiting out timer deadlines) until `$DONE`
  fires or no runnable work remains -- the synchronous driver an
  embedder without a live event loop uses to run a promise/`setTimeout`
  async run to completion. 9 unit tests cover sync, promise-chained,
  timer-driven, chained-timer+promise, error, truthy-non-error,
  double-call, never-called (returns Pending, no hang), and reset paths.
  test262_boa now runs the `[async]` lane (previously skipped): its raw
  context records `$DONE` state and `run_jobs` drains the microtask-based
  test262 async reactions. Evidence: built-ins/Promise runs 633 pass /
  0 fail / 6 skip (99.06% total, 358 async-flagged tests that were all
  skipped before); Promise/prototype 123/123.
- Evidence (2026-07-10): the real Kraken 1.1 ai-astar subtest
  (ai-astar.js + ai-astar-data.js, unmodified algorithm) executes to
  completion through the CLI SilkContext, producing the correct
  379-length A* path. Release Boa runs ~14.2s/iter (non-JIT tree
  interpreter, ~350x slower than V8); debug does not finish one
  iteration in 40s. This confirms correctness and measurability; the
  gap to real benchmark scores is interpreter throughput, not missing
  APIs. The run is an evidence artifact, not a gate test (too slow for
  make test).
- Gate: a local Kraken/Octane subset executes to completion (met for
  Kraken ai-astar); timer and XHR tests stay green in make test.

## conformance-honesty-and-expansion

Makes the conformance numbers mean what they say, then widens them.

- **scorecard-dual-denominator** -- LANDED. test262_boa now emits
  executed, rate_executed/pass_pct_executed, and
  rate_total/pass_pct_total; console summary prints both rates;
  docs/conformance/SCORECARD.md quotes 99.81% of executed and 69.38% of
  total together and documents the skip classes. The 2026-05-17 JSON
  artifact is retained verbatim as evidence; regeneration reported not
  run (test262 corpus absent locally).
- **module-and-async-execution** -- LANDED (2026-07-12). Async ($DONE +
  run_jobs microtask drain), static ESM, and a per-test budget all execute
  now.
  - ESM: module-flagged tests are no longer skipped. The harness runs as a
    script (installing globals like assert/$DONE), then the test runs as an
    ES module through a `SimpleModuleLoader` rooted at the test's directory;
    the entry module is parsed with its own path (`Source::with_path`) so a
    relative `./x_FIXTURE.js` import resolves. A rejected evaluation promise
    maps to the "threw" branch, so module negative parse/runtime tests judge
    correctly. Tests needing dynamic import, import.meta, top-level-await, or
    JSON modules stay skipped by feature flag (SimpleModuleLoader is
    static-import only).
  - Per-test budget: a deterministic loop-iteration budget (boa
    `set_loop_iteration_limit`, default 1e8, `--loop-limit`), NOT a wall-clock
    timeout (rejected as nondeterministic across machines). It converts an
    infinite JS loop into a catchable error so a runaway test cannot hang the
    collector (which waits on every worker's result); recursion is bounded by
    boa's default 512-frame limit. Budget hits are a distinct `LIMIT` tally
    (scorecard `limit_exceeded`), never folded into FAIL, so a too-low limit
    is visible instead of silently depressing the pass rate.
  - Panic-safety: per-test evaluation runs inside `catch_unwind` with a quiet
    panic hook, so no single test crashes a worker. This is load-bearing for
    modules: a module whose loop hits the budget makes boa PANIC converting
    its RuntimeLimit error to a promise-rejection value; the catch maps that
    to `LimitExceeded`. Any other panic becomes a visible FAIL.
  - Verified end-to-end against a minimal test262-shaped fixture (pass
    script, static ESM import + `_FIXTURE`, module negative parse, module
    negative runtime TypeError, async, infinite-loop script AND module):
    5 PASS / 0 FAIL / 2 LIMIT, `limit_exceeded: 2` in the scorecard.
  - A full `--full` re-run to refresh the JSON artifact is STILL reported not
    run: the test262 corpus is absent locally. Flipping module tests from
    Skip to executed will drop `rate_executed` (newly-run tests that fail)
    while dropping `skip` -- that is coverage expansion, not regression, and
    the dual-denominator scorecard already frames it.
- **strict-mode-variant-execution**: the runner executes onlyStrict tests
  as normal scripts; run both modes per test262 metadata.
- **finalization-registry-host-gap**: implement FinalizationRegistry /
  WeakRef in the boa host layer or record the deferral with AD-021-style
  rationale.
- **icu-intl-posture**: Intl stays deferred per AD-021; the
  dual-denominator scorecard carries the honesty until that ADR reopens.
- Gate: skip count drops with each landed item; conformance_run.sh output
  references the new fields.

## measurement-and-observability

Closes the gap between measurement claims and exercised evidence.

- **tracing-rollout-or-drop**: the workspace-level tracing dependency is
  wired only into silksurf-engine (root Cargo.toml comment says the
  rollout is "queued"). Either instrument net/gui/render spans on the
  AGENTS.md hot-path budget boundaries or drop the workspace-wide
  declaration.
- **msrv-exercise-policy**: rust-toolchain.toml pins 1.94.1 equal to
  rust-version, so the MSRV claim is asserted, never tested below the
  daily toolchain. Add a `make msrv` target or document that
  channel==MSRV is the deliberate policy (LOCAL-GATE.md already argues
  the by-construction position; make the argument an explicit policy
  statement).
- **miri-fuzz-cadence**: promote miri and fuzz from opt-in flags to a
  scheduled cadence (weekly `MIRI=1 FUZZ=1 make full`), recorded in
  docs/development/LOCAL-GATE.md; the strict-local-only CI policy
  (AD-009) makes cadence discipline the only enforcement available.
- **docs-link-checker-gate** -- LANDED: scripts/lint_doc_links.sh walks
  tracked markdown outside archive/external/vendored trees and verifies
  relative link targets exist; wired into make check and the gate
  docstrings.
- Gate: make full output shows the new targets; link checker green.

## formal-model-consolidation

- **browserloader-model-relocation** -- LANDED (2026-07-12).
  BrowserLoader.tla + BrowserLoader.cfg moved from the repository root into
  silksurf-specification/formal/ beside resolve_table.tla and
  cache_coherence.tla; formal/README.md documents all three; the spec
  README's formal/ bullet lists BrowserLoader. TLC's generated states/ dir
  is gitignored.
- **browserloader-invariant-authoring** -- LANDED (2026-07-12). The old
  BrowserLoader.cfg declared only `SPECIFICATION Spec` (verified nothing).
  The model is rewritten as checkable TLA+ with a ghost `used_after_free`
  flag and now checks, via BrowserLoader.cfg: TypeOK, NoUseAfterFree (the
  network never commits a DOM update to a freed node -- the use-after-free
  the model exists to rule out), CommitOnlyAfterLoad, and the Termination
  property. `tlc BrowserLoader.tla -config BrowserLoader.cfg` passes (8
  distinct states, no error). Non-vacuity confirmed by mutation: injecting
  an unconditional commit makes TLC report "Invariant NoUseAfterFree is
  violated".
- (The superseded BrowserLoader.old is already deleted under
  repo-hygiene-excision.)
- Gate: `tlc BrowserLoader.tla -config BrowserLoader.cfg` checks four
  properties and passes; the invariant is non-vacuous (mutation-tested).

## cleanroom-boundary-restoration

diff-analysis/ is documented as reference-analysis-only
(docs/CLEANROOM.md, docs/REPO-LAYOUT.md), yet it accumulates
non-reference material, and silksurf-specification/README.md links into
it -- bidirectional coupling across the boundary the policy separates.

- **boundary-enforcement-gate** -- LANDED (2026-07-12). The load-bearing
  cleanroom invariant is now gated: `scripts/lint_cleanroom.sh` (wired into
  `make check`) FAILS if (1) any production Rust source (`crates/*/src`,
  `silksurf-js/src`) or Cargo manifest names `diff-analysis`, or (2) any
  file under `silksurf-specification/` points at a specific file inside
  `diff-analysis` (a bare `../diff-analysis/` directory mention is allowed --
  describing the boundary is what CLEANROOM.md does). Audit at landing:
  production was already CLEAN (no crate/src or Cargo.toml crossing); the one
  spec crossing was `silksurf-specification/README.md` pointing at
  `../diff-analysis/PHASE-2-RESEARCH-SYNTHESIS.md`, reconciled by reframing it
  as a reference-side input governed by CLEANROOM.md, not a spec dependency.
  Documented in docs/CLEANROOM.md "Boundary enforcement". The gate makes the
  boundary a regression-tested invariant rather than a prose aspiration.
- STILL open (physical hygiene, distinct from the dependency boundary the
  gate enforces): the relocations below move generated/planning artifacts OUT
  of the reference tree. They do not affect the one-directional dependency
  boundary (nothing production/spec depends on them), so they are tracked
  hygiene rather than a cleanroom-integrity risk.

- **tool-output-relocation**: diff-analysis/tools-output/ (afl-corpus,
  semgrep, ctags, doxygen, tokei, infer results) is generated evidence;
  relocate to an ignored path or a dated evidence area outside the
  reference tree.
- **generated-artifact-relocation**: move generated visualizations/ and
  logs/ (mcp-puppeteer log, .audit.json) likewise; delete the
  diff-analysis mydatabase.db (stray-database-file-removal).
- **planning-doc-re-homing**: re-home NEURAL-SILKSURF-ROADMAP.md,
  WEEK-2-PLAN.md, PHASE-0-COMPLETE.md to docs/archive/;
  PHASE-2-RESEARCH-SYNTHESIS.md either moves to docs/ (fixing the spec
  README link) or the link inverts.
- **external-reference-deduplication**: diff-analysis/references/{papers,
  specs,docs} overlaps docs/external_sources/ (the provenance-managed
  tree); one home, with the fetch/verify scripts as the mechanism.
- **spec-readme-reframing**: silksurf-specification/README.md front
  matter drops its "Phase 2, C-core + CMake" framing (couples to
  governance-pointer-repair).
- Gate: the one-directional dependency boundary is enforced by
  `scripts/lint_cleanroom.sh` in `make check` (LANDED). The remaining
  physical-relocation items above are hygiene, not dependency crossings, and
  do not gate cleanroom integrity.

---

## Sequencing and dependencies

```
governance-pointer-repair ---+--> legacy-c-tree-retirement --> residual hygiene
  (adr + citations + map     |      (dead-c-code LANDED;
   LANDED; docs rewrites     |       bpe re-home next)
   remain)                   |
repo-hygiene-excision -------+    legacy-vm-quarantine-or-removal
  (deletions LANDED;         |      --> finalization-registry-host-gap
   corpus/vendor/perf        |
   remain)                   |
suppression triage plan -----+--> app-monolith-decomposition --> suppression-paydown
soa-migration-decision ------+
panic-path-and-unsafe-hardening   [funnel LANDED; rest independent]
security-substrate-buildout, conformance-honesty-and-expansion
  [scorecard-dual-denominator LANDED],
measurement-and-observability, formal-model-consolidation,
cleanroom-boundary-restoration    [independent of each other;
                                   spec-readme-reframing follows
                                   governance-pointer-repair]
```

Next landings in order: cross-reference-link-repair (spec README link),
gate-docstring-alignment, c-ffi-shim-retirement plus
duplicated-c-module-removal (completing AD-024 steps 3-4),
legacy-vm-preserve-or-delete-decision, interner-atom-expect-justification.
Each is a small, separately-verifiable change gated by `make full`.

## Verification checklist (applies to every workstream)

- `make check` and `make test` green with RUSTFLAGS='-D warnings'.
- `make full` (deny + doc) green before any merge-ready claim.
- lint_unwrap.sh / lint_unsafe.sh / lint_glossary.sh pass.
- Behavior-affecting changes carry a bench or probe delta
  (scripts/perf_guardrails.py, make gui-probe) per AGENTS.md evidence
  classes.
- Checks not run are reported as `not run` with the reason.
