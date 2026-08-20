# SilkSurf Architecture Decision Records (ADRs)

**Purpose**: Document key architectural decisions with rationale and alternatives
**Format**: Context -> Decision -> Rationale -> Consequences -> Alternatives
**Updated**: 2026-01-29

---

## AD-001: Cleanroom Implementation Strategy

**Status**: Accepted
**Date**: 2025-12-30
**Deciders**: Architecture Team
**Context**:

Web browser implementations are complex and often reference existing codebases. We need to decide whether to:
1. Fork an existing browser (Chromium, Firefox)
2. Build on top of an existing engine (WebKit, Gecko)
3. Implement from scratch using only specifications

**Decision**:

Cleanroom implementation - build from specifications only, no code reference to existing browsers.

**Rationale**:

1. **Copyright Clarity**: No risk of inadvertent copyright violation
2. **Deep Understanding**: Forces thorough understanding of specifications
3. **Optimization Freedom**: Not constrained by legacy architectural decisions
4. **Learning Value**: Educational value for team and community
5. **Innovation Opportunity**: Can make novel design choices

**Consequences**:

**Positive**:
- Clean IP, no licensing concerns
- Optimized for modern use cases, no legacy baggage
- Team gains deep spec knowledge
- Can make unconventional choices (arena allocators, pure XCB)

**Negative**:
- Longer initial development time
- Must rediscover edge cases that existing browsers already handle
- Higher risk of spec misinterpretation
- Need extensive testing for compatibility

**Alternatives Considered**:

1. **Fork Chromium/Blink**
   - Pros: Mature, fast time-to-market, excellent compatibility
   - Cons: Massive codebase (25M+ LOC), hard to customize, heavyweight

2. **Build on WebKit**
   - Pros: Clean architecture, good performance, Apple backing
   - Cons: Still millions of LOC, C++ dependencies, license constraints

3. **Use Servo Components**
   - Pros: Modern Rust, parallel architecture, clean APIs
   - Cons: Project abandoned by Mozilla, uncertain future, still large

**Implementation Notes**:

- Use NetSurf libraries (libdom, libcss, libhubbub) as proven components
- These are cleanroom implementations themselves, well-documented
- Specifications used: WHATWG HTML, W3C CSS, ECMA-262
- Test against Test262, WPT (Web Platform Tests)

**References**:
- `/CLAUDE.md` - NO SHORTCUTS policy
- `/diff-analysis/` - NetSurf vs NeoSurf analysis

---

## AD-002: Hybrid Rust + C Architecture

**Status**: Accepted; C-side superseded by AD-024 (Legacy C Tree Retirement)
**Date**: 2025-12-30
**Context**:

Modern browsers use C++ (Chromium) or mix of languages (Firefox: C++/Rust). We need to choose our implementation language(s).

**Decision**:

Hybrid architecture:
- **Rust**: JavaScript engine, hot-path optimizations, future components
- **C**: DOM/HTML/CSS (via NetSurf libraries), GUI (XCB bindings)

**Rationale**:

1. **Rust for JS Engine**: Memory safety critical for untrusted code execution
2. **C for DOM/CSS**: Leverage proven NetSurf libraries (libdom, libcss, libhubbub)
3. **Best of Both**: Rust safety where needed, C simplicity where sufficient
4. **Performance**: Both compiled to native code, minimal FFI overhead
5. **Ecosystem**: NetSurf C libraries are mature, Rust tooling is excellent

**Consequences**:

**Positive**:
- Memory safety for JS engine (most attack surface)
- Can use battle-tested NetSurf libraries immediately
- Rust's zero-cost abstractions for performance
- C's simplicity reduces cognitive load for core rendering

**Negative**:
- FFI boundary requires careful design
- Two build systems (CMake + Cargo)
- Team needs both C and Rust expertise
- Debugging across language boundary can be tricky

**Alternatives Considered**:

1. **Pure Rust**
   - Pros: Memory safety everywhere, single language, modern tooling
   - Cons: Would need to rewrite libdom/libcss, massive effort

2. **Pure C**
   - Pros: Simple, single toolchain, proven NetSurf libraries
   - Cons: Memory safety burden for JS engine, no modern abstractions

3. **Pure C++**
   - Pros: OOP abstractions, STL, large ecosystem
   - Cons: Complexity, template bloat, still memory-unsafe

**Implementation Notes**:

- C <-> Rust FFI via extern "C" ABI
- Clear ownership boundaries (C owns DOM, Rust owns JS heap)
- Validation at FFI boundary (never trust foreign pointers)
- Arena allocators on both sides reduce FFI crossing frequency

**FFI Design**:
```c
// C calls Rust
extern JSValue js_eval(const char *code, size_t len);

// Rust calls C
extern "C" fn dom_node_get_attribute(node: *mut DOMNode, name: *const c_char) -> *const c_char;
```

**References**:
- `silksurf-specification/SILKSURF-JS-DESIGN.md` - Rust JS engine
- `silksurf-specification/SILKSURF-C-CORE-DESIGN.md` - C rendering core
- Task #33 - Complete Rust FFI integration

---

## AD-003: Pure XCB GUI (No GTK)

**Status**: Accepted
**Date**: 2025-12-31
**Context**:

Most Linux browsers use GTK (Firefox, Chromium via GTK3). We need to decide on GUI toolkit.

**Decision**:

Pure XCB (X C Binding) with no high-level toolkit (GTK, Qt).

**Rationale**:

1. **Performance**: Direct X11 access, no abstraction overhead (~30% less than GTK)
2. **Control**: Full control over rendering pipeline
3. **Size**: Minimal dependencies, smaller binary
4. **Efficiency**: Can optimize for browser-specific use cases
5. **Learning**: Deep understanding of X11 fundamentals

**Consequences**:

**Positive**:
- Fastest possible rendering path
- No GTK theme engine overhead
- Small memory footprint (<1MB for GUI layer)
- Direct access to X11 extensions (XShm, XDamage, XComposite)
- Perfect control over event handling

**Negative**:
- Must implement UI widgets ourselves (buttons, menus, dialogs)
- No automatic HiDPI scaling (must implement)
- Wayland support requires separate implementation
- Debugging X11 protocol can be challenging
- No native file picker (must use xdg-desktop-portal)

**Alternatives Considered**:

1. **GTK 4**
   - Pros: Modern, HiDPI support, native widgets, Wayland-ready
   - Cons: Heavy (~5MB), slow startup, theme engine overhead, complex API

2. **Qt**
   - Pros: Excellent cross-platform, modern C++, good documentation
   - Cons: Very heavy (~20MB), C++ complexity, licensing (LGPL)

3. **SDL2**
   - Pros: Simple, game-tested, cross-platform
   - Cons: Game-focused API, missing browser-specific features

**Implementation Notes**:

- Core XCB: `xcb_connect`, `xcb_create_window`, event loop
- Extensions: XCB-SHM (fast images), XCB-Damage (change tracking)
- Double buffering via pixmaps
- Manual widget toolkit (tabs, address bar, buttons)

**Performance Targets**:
- Window creation: <10ms
- Event handling: <1ms latency
- Image upload: 10x faster with XShm vs socket transport

**References**:
- `silksurf-specification/SILKSURF-XCB-GUI-DESIGN.md`
- `docs/XCB_GUIDE.md`

---

## AD-004: Arena Allocator for DOM/Layout

**Status**: Accepted
**Date**: 2025-12-31
**Context**:

DOM trees and layout boxes have short, synchronized lifetimes. Traditional malloc/free has overhead.

**Decision**:

Arena (bump) allocator for DOM nodes, layout boxes, and CSS computed styles.

**Rationale**:

1. **Performance**: O(1) allocation, batch deallocation
2. **Locality**: Better cache performance (sequential memory)
3. **Simplicity**: No individual free() calls
4. **Predictability**: No fragmentation
5. **Alignment**: All DOM nodes allocated together improves traversal

**Consequences**:

**Positive**:
- 10-100x faster allocation than malloc
- Zero fragmentation
- Simpler code (no individual cleanup)
- Better cache locality (30% speedup on traversals)
- Memory usage peaks are predictable

**Negative**:
- Cannot free individual nodes during page lifetime
- Memory "leaks" until page unload (acceptable)
- Requires upfront size estimate
- Not suitable for long-lived, sparse structures

**Alternatives Considered**:

1. **malloc/free per node**
   - Pros: Standard, flexible, can free individually
   - Cons: Slow, fragmentation, overhead (16-24 bytes per allocation)

2. **Object pools**
   - Pros: Reusable, type-specific
   - Cons: Complexity, fixed sizes, still fragmentation

3. **Generational GC**
   - Pros: Automatic, flexible
   - Cons: Pause times, complexity, unpredictable memory usage

**Implementation**:

```c
// Arena allocation
silk_arena_t *arena = silk_arena_create(1024 * 1024); // 1MB
silk_dom_node_t *node = silk_arena_alloc(arena, sizeof(silk_dom_node_t));
// ... use node ...
silk_arena_destroy(arena); // frees all nodes at once
```

**Memory Estimates**:
- Typical page: ~1000 DOM nodes x 128 bytes = 128KB
- Complex page: ~10,000 nodes x 128 bytes = 1.28MB
- Arena size: 2MB default (allows growth)

**References**:
- `/src/memory/allocator.c` - Arena implementation
- `SILKSURF-C-CORE-DESIGN.md` Section 2.1

---

## AD-005: Test262 95%+ Compliance Target

**Status**: Accepted
**Date**: 2025-12-31
**Context**:

JavaScript compliance is critical for web compatibility. Test262 has ~50,000 tests. Perfect compliance is difficult.

**Decision**:

Target 95%+ Test262 compliance, with explicit documentation of unsupported features.

**Rationale**:

1. **Pragmatism**: 100% compliance requires years (even major browsers aren't 100%)
2. **Impact**: 95% covers all common features, last 5% is exotic
3. **Resources**: Focus on shipping a usable browser first
4. **Transparency**: Document what's not supported rather than hide it

**Consequences**:

**Positive**:
- Faster time to usable product
- Clear communication of limitations
- Can prioritize common features
- Realistic goal for small team

**Negative**:
- Some websites may break
- Need to track and document unsupported features
- May need to implement missing features later based on user needs
- Compatibility pressure from web developers

**Alternatives Considered**:

1. **100% Compliance**
   - Pros: Perfect compatibility
   - Cons: Unrealistic timeline (5+ years), diminishing returns

2. **80% Compliance**
   - Pros: Faster implementation
   - Cons: Too many broken sites, poor user experience

3. **No Target**
   - Pros: Flexible
   - Cons: No clear goal, hard to measure progress

**Phased Approach**:

**Phase 1** (MVP - 50% Test262):
- Variables, functions, basic objects
- Loops, conditionals, operators
- Arrays, strings, numbers

**Phase 2** (Beta - 80% Test262):
- Prototypes, inheritance
- Closures, scope chains
- Regular expressions
- JSON, Date, Math

**Phase 3** (Release - 95% Test262):
- Promises, async/await
- Generators, iterators
- Symbols, proxies
- WeakMap, WeakSet

**Explicitly Unsupported** (<5% of Test262):
- Esoteric Intl features
- Obscure RegExp flags
- Stage 3 proposals
- Tail call optimization

**References**:
- `SILKSURF-JS-DESIGN.md` - Phased compliance plan
- https://github.com/tc39/test262

---

## AD-006: Neural Integration (BPE + LSTM)

**Status**: [PARTIAL] Experimental
**Date**: 2025-12-31
**Context**:

JavaScript parsing/lexing is a hot path. Can we use neural optimization?

**Decision**:

Experimental integration of BPE (Byte Pair Encoding) for lexical optimization and LSTM for token prediction.

**Rationale**:

1. **Performance**: BPE can accelerate lexing by 20-40%
2. **Research Value**: Novel approach, potential publication
3. **Optional**: Can be disabled, no risk to correctness
4. **Learning**: Good ML integration case study

**Consequences**:

**Positive**:
- Potential 20-40% lexing speedup
- Novel research contribution
- Demonstrates ML integration in systems software
- Optional feature (can disable)

**Negative**:
- Complexity increase
- Model training required
- Unpredictable on unusual code
- Debugging is harder

**Implementation Status**:
- **Current**: BPE vocabulary built, not integrated
- **Next**: Token prediction model training
- **Future**: Runtime prediction (optional feature flag)

**Safety Considerations**:
- Models are deterministic (no runtime randomness)
- Fall back to standard lexing on prediction failure
- Predictions only used for prefetching, not correctness
- Optional feature flag: `-DENABLE_NEURAL_BPE=ON`

**Alternatives Considered**:

1. **No Neural Integration**
   - Pros: Simpler, predictable
   - Cons: Miss optimization opportunity

2. **JIT Compilation**
   - Pros: Proven technique, large speedups
   - Cons: Complexity, security concerns, code cache

3. **AOT Compilation**
   - Pros: Best performance
   - Cons: Not practical for web (need JIT or interpreter)

**References**:
- `SILKSURF-JS-DESIGN.md` Section 6
- `silksurf-specification/SILKSURF-NEURAL-INTEGRATION.md`

---

## AD-007: Damage Tracking for Rendering

**Status**: Accepted
**Date**: 2025-12-31
**Context**:

Full-screen redraws are expensive (1920x1080x4 bytes = 8MB per frame). Most changes are local.

**Decision**:

Implement damage tracking - record which screen regions changed, only redraw those.

**Rationale**:

1. **Performance**: 10x fewer pixel updates for typical interactions
2. **Power**: Reduced GPU/CPU usage, better battery life
3. **Responsiveness**: Faster redraws for small changes
4. **Standard**: X11 Damage extension is mature and well-supported

**Consequences**:

**Positive**:
- 100+ FPS rendering (vs 10-20 FPS full redraw)
- Reduced power consumption
- Smoother scrolling and animations
- Better use of GPU bandwidth

**Negative**:
- Additional complexity in tracking changes
- Must compute damage regions correctly (bugs = visual glitches)
- Not all operations benefit (full-page animations still expensive)
- Debugging is harder (partial redraws)

**Alternatives Considered**:

1. **Always Full Redraw**
   - Pros: Simple, no tracking overhead
   - Cons: Slow (10-20 FPS max), high power consumption

2. **Compositor-Based**
   - Pros: GPU acceleration, layer-based
   - Cons: Requires compositor, more complex, higher memory

**Implementation**:

```c
// Track damage
silk_damage_tracker_t *tracker = silk_damage_create();
silk_damage_add_rect(tracker, x, y, width, height);

// Render only damaged regions
silk_damage_region_t *regions = silk_damage_get_regions(tracker);
for (int i = 0; i < regions->count; i++) {
    render_rect(regions->rects[i]);
}
```

**Damage Sources**:
- Text cursor blinking (10x20 pixel region)
- Typing (variable-width character)
- Scrolling (vertical strip, can optimize with XCopyArea)
- Animations (bounding box of animated element)
- Mouse hover (element + cursor region)

**Optimizations**:
- Merge overlapping rectangles
- Skip tiny regions (<16 pixels)
- Use XShm for large damage regions
- Batch damage updates (reduce XCB round-trips)

**References**:
- `SILKSURF-XCB-GUI-DESIGN.md` Section 4
- `/src/rendering/damage_tracker.c`
- Task #26 - XShm acceleration

---

## AD-008: Stable-Rust Migration + MSRV Declaration

**Status**: Accepted
**Date**: 2026-04-30
**Deciders**: Architecture Team

### Context

Until 2026-04-30 the workspace pinned `nightly-2026-04-05` via
`rust-toolchain.toml`. The pin was load-bearing only for `[unstable] gc =
true` in `.cargo/config.toml` (a developer convenience that triggers Cargo's
target-directory garbage collection). A workspace-wide grep confirmed
**zero** `#![feature(...)]` directives in any crate.

The nightly pin had three negative consequences:

1. **Distribution blocker**: `cargo install` from crates.io requires stable.
   Nightly-only crates cannot be published without users opting into a
   nightly toolchain.
2. **MSRV theatre**: `Cargo.toml` declared `rust-version = "1.96.0"` even
   though that version did not exist as a stable release; the build was
   never actually verified against the declared MSRV.
3. **Reproducibility erosion**: nightly snapshots can change semantics
   between consecutive days; pinning to a single nightly date is a fragile
   reproducibility guarantee.

### Decision

Pin the workspace toolchain to a single, real stable Rust release. Match
`workspace.package.rust-version` to the same exact version in lockstep, and
propagate the value to every per-crate `Cargo.toml` `rust-version` field so
the per-crate MSRV does not drift from the workspace MSRV.

The current pin is **`1.94.1`** (released 2026-03-25). Bump in lockstep
across `rust-toolchain.toml`, `Cargo.toml` `workspace.package.rust-version`,
and every `crates/*/Cargo.toml` and `silksurf-js/Cargo.toml` per-crate
`rust-version`.

### Rationale

  * Edition 2024 stabilized in Rust 1.85, so any 1.85+ stable will build
    the workspace.
  * Removing `[unstable] gc = true` costs only the periodic auto-cleanup of
    `target/`; manual `cargo clean` or a contributor-side cron is a fine
    substitute.
  * The local-gate now has a dedicated MSRV verification step
    (`scripts/local_gate.sh full`) that prints the active toolchain and
    re-runs `cargo check --workspace --all-targets` so an MSRV violation
    is impossible to ship silently.

### Consequences

Positive: `cargo install` distribution becomes possible (P9 release work
unblocked); reproducibility tightens; MSRV theatre eliminated; Dependabot
and similar dependency-update agents work normally.

Negative: lose Cargo's nightly-only target-GC convenience; any future
nightly-only feature requires a deliberate ADR amendment.

### Alternatives Considered

  * Stay on nightly with explicit ADR justification -- rejected because
    the only justification was Cargo target-GC.
  * Dual toolchain (stable for CI, nightly for dev) -- rejected as
    unnecessary machinery; if a developer wants nightly tooling they can
    use `rustup` overrides locally.

---

## AD-009: Strict-Local-Only CI Policy

**Status**: Accepted
**Date**: 2026-04-30
**Deciders**: Architecture Team

### Context

Cloud CI on push and pull_request is currently disabled
(`.github/workflows/ci.yml` is `workflow_dispatch:`-only). The decision had
not been formally captured as an ADR; new contributors had no way to
distinguish "intentionally off" from "broken."

### Decision

Adopt strict-local-only CI as the canonical merge gate.
`scripts/local_gate.sh` is the single source of truth for merge readiness.
Pre-commit and pre-push git hooks (installed by
`scripts/install-git-hooks.sh`) wire the fast and full gate modes into the
everyday git flow. Cloud CI workflows remain `workflow_dispatch:`-only and
serve as discoverability surfaces, not gates.

### Rationale

  * **Latency**: local execution catches failures before the work leaves
    the machine; no GitHub Actions queue wait.
  * **Cost**: CI minutes are nontrivial for a workspace this size with
    LTO=fat release builds and a CMake/CTest pass.
  * **Reproducibility**: every contributor runs the exact same gate on the
    exact same toolchain (pinned by `rust-toolchain.toml`), so a green
    local-gate on one machine implies a green local-gate on another.
  * **No silent skip**: pre-push hook is mandatory; bypassing
    (`--no-verify`) requires explicit operator acknowledgement, and the
    bypass should be documented in the commit body.

### Consequences

Positive: fast feedback loop; deterministic gate; no CI minute spend;
fewer surprise failures on `main`.

Negative: outside contributors must install hooks before contributing;
green status is invisible from the GitHub UI; long-running checks (miri,
fuzz) become opt-in via `MIRI=1` / `FUZZ=1` rather than always-on.

### Alternatives Considered

  * Hybrid (local primary, cloud non-blocking) -- rejected for now
    because non-blocking informational scans tend to be ignored. Can be
    revisited if outside contributor friction grows.
  * Flip to push/PR cloud gating -- rejected as a regression of this
    policy; the local-gate is what this project chose deliberately.

---

## AD-010: GUI Backend Formalization -- XCB-Only, Linux-First

**Status**: Accepted (amends AD-003)
**Date**: 2026-04-30
**Deciders**: Architecture Team

### Context

AD-003 ("Pure XCB GUI") established the cleanroom XCB choice in 2025-12-31
but left the cross-platform posture implicit. `crates/silksurf-gui/src/
lib.rs` is currently a single doc-comment line; the implementation work
in roadmap P6 needs an explicit posture before code lands.

### Decision

Formalize XCB as the sole supported GUI backend for the v0.1 release line.
Linux is the only supported host platform for v0.1. Wayland, macOS, and
Windows are explicit future work tracked under separate ADRs.

The crate API will keep the backend behind a small trait
(`Window`, `EventLoop`) so a future Wayland or winit-based backend can be
introduced as a feature flag without an API break, but no second backend
ships in v0.1.

### Rationale

  * Cleanroom philosophy: XCB is a small, well-specified protocol; winit
    or SDL would pull a large dependency that obscures the engine's
    surface.
  * Smallest dep footprint matches the rest of the workspace (rustls,
    bumpalo, smallvec, etc.).
  * The XCB binding pattern is already documented in
    `docs/XCB_GUIDE.md`; we are codifying existing intent, not changing
    direction.

### Consequences

Positive: clear scope for P6; smaller surface to test; no cross-backend
abstraction tax during initial development.

Negative: no macOS or Windows v0.1; non-Linux contributors cannot run the
GUI demo locally (the headless engine + bench pipeline still work on any
Unix); Wayland-first users cannot use silksurf as a desktop browser until
a Wayland backend lands.

### Alternatives Considered

  * winit cross-platform -- rejected for v0.1 due to dep weight and
    cleanroom drift; reasonable choice for v0.2+.
  * Both XCB primary + winit feature flag in v0.1 -- rejected as
    premature; the trait abstraction in this ADR keeps that path open
    without paying the maintenance cost up-front.

---

## AD-011: Reserved -- Merged into AD-008 (Stable-Rust Migration)

**Status**: Superseded  
**Disposition**: The original ADR outline for MSRV toolchain formalization was
consolidated into AD-008. This number is
reserved to preserve the contiguous registry; the content lives in AD-008.

---

## AD-012: Reserved -- Merged into AD-009 (Strict-Local-Only CI)

**Status**: Superseded  
**Disposition**: The original ADR outline for pre-commit/pre-push hook policy was
consolidated into AD-009 during Wave 1. Content lives in AD-009.

---

## AD-013: Reserved -- Merged into AD-010 (XCB-Only GUI)

**Status**: Superseded  
**Disposition**: The original ADR outline for the XCB backend formalization was
consolidated into AD-010 during Wave 1. Content lives in AD-010.

---

## AD-014: Reserved -- Merged into AD-020 (SilkError)

**Status**: Superseded  
**Disposition**: The original ADR outline for the error-type unification strategy
was consolidated into AD-020 (Workspace-Wide Canonical Error) during Wave 1.

---

## AD-015: Reserved -- Pending (Proxy/Reflect JavaScript Semantics)

**Status**: Proposed  
**Disposition**: Covers the decision to defer JS Proxy and Reflect to a future
wave pending test262 conformance measurement. No implementation changes have
landed yet; the ADR will be filed when the implementation work begins.

---

## AD-016: Fused Render Pipeline (FusedWorkspace)

**Status**: Accepted
**Date**: 2026-04-30 (codifies design from `main` = `1066d3a`)
**Deciders**: Architecture Team

### Context

The legacy 3-pass pipeline (`EnginePipeline::render_document`) walked
the DOM three times: cascade, layout, paint. Each pass allocated its
own intermediate `HashMap` / `Vec`. Per-frame allocator pressure
dominated the steady state (~24 us at 50 nodes); the cascade was
fetching 168-byte `Node` rows when only ~36 bytes (tag, id_index,
class_*, parent_id) were actually needed.

### Decision

Adopt a single-BFS-walk fused pipeline that emits styles, layout
rects, and display-list items in one pass, backed by a `FusedWorkspace`
that owns all reusable per-frame buffers (`LayoutNeighborTable`,
`CascadeWorkspace`, output `Vec`s for styles / rects / cursors /
display items). After the first call, zero allocator traffic for
same-or-smaller DOMs.

Materialise a `CascadeView` SoA projection (40-byte `CascadeEntry`
rows, fits one cache line) once per render and consume it from the
matching hot path so `dom.node()` and per-call attribute scans
disappear.

### Rationale

  * 9.5 us steady-state at 50 nodes (1.69x over 3-pass workspace
    fused, 2.05x over 3-pass cold) -- measured in
    `bench_pipeline.rs`.
  * High-water-mark growth keeps the workspace warm across many
    page renders; fits cacheable-page workloads (404, wiki landing).
  * SoA layout gives 4.2x compression vs Node and exposes
    `parent_id` for combinator walks without `dom.parent()` (avoids
    the 168-byte fetch).

### Consequences

Positive: production-path is the fast-path, no behaviour switch
between bench and real workloads; the legacy 3-pass remains as a
parity test.

Negative: more state to keep coherent (the `generation`-gated
rebuild pattern, see AD-017). FusedWorkspace must be reused across
calls; passing a fresh `FusedWorkspace::new()` each call regresses to
cold cost.

### See

  * `crates/silksurf-engine/src/fused_pipeline.rs`
  * `docs/PERFORMANCE.md`
  * GLOSSARY -> CascadeView, FusedWorkspace, generation-gated rebuild

---

## AD-017: Lock-free Monotonic Resolve Table

**Status**: Accepted
**Date**: 2026-04-30 (codifies design from `main` = `662ddb9`)
**Deciders**: Architecture Team

### Context

`Dom` holds a `RwLock<SilkInterner>`. The cascade matching path called
`dom.resolve(atom) -> SmallString` once per atom comparison (~29 atoms
per cascade); each call paid ~6 ns of `RwLock::read` acquisition
overhead, totalling ~168 ns per cascade just on lock traffic.

### Decision

Add a per-`Dom` `resolve_table: Vec<SmallString>`, materialised from
the interner's `values_slice()` at two phase boundaries:

  1. `silksurf_html::treesink::SilkDomBuilder::finish()` -- after parse
     completes.
  2. `Dom::end_mutation_batch()` -- after JS / dynamic mutations.

`Dom::resolve_fast(atom)` is a plain array index by `atom.raw()`,
zero synchronisation. The table is monotonically growing: old atoms
never move, new atoms extend the end. The interner's `RwLock` is
retained on the write path (intern during parse / mutation), but the
read path (resolve during cascade) is entirely lock-free.

### Rationale

  * Eliminates ~168 ns of lock traffic per cascade.
  * Supports full dynamic DOM mutations without architectural
    penalty -- mutation batches mark a phase boundary, the table
    grows, and the cascade reads continue lock-free.
  * No two-tier lookup, no branch on the read path.

### Consequences

Positive: cascade write path becomes lock-free; the only remaining
synchronisation in the hot path is the rayon scope for tile
rasterisation.

Negative: callers must `end_mutation_batch()` after batched mutations
(or call `materialize_resolve_table()` explicitly) before the next
cascade can see new atoms. Document this discipline.

### See

  * `crates/silksurf-dom/src/lib.rs::materialize_resolve_table`
  * `crates/silksurf-core/src/interner.rs::values_slice`
  * GLOSSARY -> Lock-free monotonic resolve table, resolve_fast

---

## AD-018: Persistent On-Disk Response Cache

**Status**: Accepted
**Date**: 2026-04-30 (codifies design from `main` = `418ea00`)
**Deciders**: Architecture Team

### Context

The original `silksurf-net::ResponseCache` was in-memory only.
`FetchOrigin::Cache` therefore could not fire across process
invocations; the speculative-render revalidation path was unreachable
at the CLI boundary.

### Decision

Introduce `CachedResponseDisk` (serde-serializable, no `Instant`) for
on-disk JSON entries. `ResponseCache::with_disk(dir)` loads all
`*.json` from `dir` on construction; `put()` writes-through (silent on
I/O error -- the in-memory entry is still recorded). Filename =
`FxHash(url)` hex (16 chars; structurally path-traversal-safe).

`SpeculativeRenderer` constructors default to `with_disk()` rooted at
`$XDG_CACHE_HOME/silksurf/http` (or `~/.cache/silksurf/http`).

### Rationale

  * Second-run cache hit: ~9 us vs ~327 ms cold network fetch on
    chatgpt.com.
  * `Cache-Control: private` is not yet enforced on disk; documented
    as a threat-model gap (THREAT-MODEL.md Subsystem 7).
  * No URL bytes in the filename; the hash is collision-resistant
    enough for the workload.

### Consequences

Positive: speculative rendering finally has a write-through cache.
First-fetch creates the directory and writes 3 files (the response,
its conditional-GET headers, and the post-revalidation 304/200 result).

Negative: the cache grows unboundedly until manually cleared; SIZE-
bounded LRU is a future option. Disk encryption-at-rest discipline
becomes a user concern. Documented in `OPERATIONS.md`.

### See

  * `crates/silksurf-net/src/cache.rs`
  * `crates/silksurf-net/OPERATIONS.md`
  * `docs/design/THREAT-MODEL.md` Subsystem 7

---

## AD-019: tls-probe as Supported Diagnostic Surface

**Status**: Accepted
**Date**: 2026-04-30 (codifies design from `main` = `63e7551`)
**Deciders**: Architecture Team

### Context

TLS handshake failures were opaque -- no way to distinguish a
corporate-proxy CA injection from an incomplete server chain (e.g. a
Cloudflare host missing an intermediate) from a Nix env that simply
has no system roots. Each failure required an ad-hoc
`openssl s_client` session and manual cert-chain inspection.

### Decision

Adopt `tls-probe` (982 lines, lives at
`crates/silksurf-app/src/bin/tls_probe.rs`) as a first-class
diagnostic binary. Output sections:

  1. Root-store inventory (counts of native + webpki-roots + extra
     CAs, plus `SSL_CERT_*` env-var snapshot).
  2. TLS handshake (negotiated protocol + cipher + ALPN + leaf-cert
     chain in human-readable form, X.509 parsed via a pure-Rust ASN.1
     DER parser).
  3. DANE TLSA probe (DNSSEC-validated via hickory-resolver 0.26).
  4. RCA paragraph for the four canonical UnknownIssuer failure
     classes: Nix env / Cloudflare incomplete chain / corporate proxy
     / TLSA FQDN trailing-dot bug.

The runtime CA injection flag (`silksurf-app --tls-ca-file <path>`)
shares the same loader (`rustls-pemfile`).

### Rationale

  * Single command goes from "TLS broke" to a printable RCA.
  * The four canonical failure classes were observed during
    development; embedding them in the tool means the next contributor
    does not have to rediscover them.

### Consequences

Positive: handshake debugging is bounded to one tool. A 100-line
in-crate smoke variant remains under `silksurf-tls/src/bin/` for
silksurf-tls library development; consolidation tracked as a
follow-up task.

Negative: dependency on `hickory-resolver 0.26.0-beta.3` (unstable
version pin); migration to stable hickory release tracked.

### See

  * `crates/silksurf-app/src/bin/tls_probe.rs`
  * `docs/development/RUNBOOK-TLS-PROBE.md`

---

## AD-020: Workspace-Wide Canonical Error -- silksurf_core::SilkError

**Status**: Accepted
**Date**: 2026-04-30
**Deciders**: Architecture Team

### Context

Per-crate error types proliferated: `CssError`, `DomError`,
`TokenizeError`, `TreeBuildError`, `NetError`, `TlsConfigError`,
`EngineError`, `JsError`. Cross-crate APIs either matched 7 variants
or fell back to `Box<dyn Error>` with bad diagnostics. 184 unwrap /
expect sites had no annotated invariants.

### Decision

`silksurf_core::SilkError` is the canonical workspace error. It is
string-erased rather than generic-over-source-types, because
silksurf-core has no rev-deps on its dependents (which would create
cycles). Per-crate `From<MyError> for SilkError` impls live in the
leaf crates that own the source types.

`SilkError` variants: `InvalidInput(String)`,
`Unsupported(String)`, `Css { offset, message }`, `Dom(String)`,
`HtmlTokenize { offset, message }`, `HtmlTreeBuild(String)`,
`Net(String)`, `Tls(String)`, `Engine(String)`, `Js(String)`,
`Io(#[from] std::io::Error)`. `thiserror` provides the `Display`
impl.

The lint scripts `scripts/lint_unwrap.sh` and
`scripts/lint_unsafe.sh` enforce the matching annotation discipline:
every `.unwrap()`/`.expect(` site needs `// UNWRAP-OK: <invariant>`
within 7 lines above; every `unsafe { ... }` block needs
`// SAFETY: <invariant>` within 7 lines above. Both are wired into
the local-gate fast pass.

### Rationale

  * The cross-crate boundary becomes one type; callers do not match
    7 variants.
  * The lints make adding new bare unwraps or unsafe blocks
    impossible to merge accidentally.
  * Per-crate types remain visible inside each crate for richer
    pattern matching.

### Consequences

Positive: error-handling becomes mechanical at boundaries; the lint
discipline scales (annotate any new site at write-time, not later).

Negative: silksurf-html, silksurf-net, silksurf-tls grew a
silksurf-core dependency (lightweight: thiserror + a small enum). The
silksurf-js follow-up batch (~118 unannotated unwrap, ~40
unannotated unsafe) is documented as deferred and currently excluded
from the lint scope.

### See

  * `crates/silksurf-core/src/error.rs`
  * `scripts/lint_unwrap.sh`, `scripts/lint_unsafe.sh`
  * `docs/design/UNSAFE-CONTRACTS.md`
  * GLOSSARY -> SilkError, UNWRAP-OK / SAFETY annotations

---

## AD-021: Internationalization Posture -- Minimal Subset, ICU Deferred

**Status**: Accepted (amended 2026-07-11: direct `idna` dep for PSL)
**Date**: 2026-05-14 (amended 2026-07-11)
**Deciders**: Architecture Team

### Context

Correct internationalization (i18n) in a browser engine spans grapheme
clustering, Unicode normalization, bidirectional text (BiDi), collation,
number/date/time formatting, and IDNA (Internationalized Domain Names in
Applications).  Full ICU integration (icu4x or the system libicu) brings a
large dependency surface (icu4x alone is ~30 crates; system libicu is a
shared-library runtime dependency that varies by distribution).

The workspace already depends on:

  * `unicode-segmentation` -- grapheme cluster and word-boundary iteration
    (transitive via `silksurf-css` and `silksurf-dom`).
  * `url` -- RFC 3986 URL parsing with IDNA 2008 hostname processing via
    the `idna` crate (version 1.1.0, transitive via `url`).

### Decision

Adopt the **minimal-subset** path for the P8 release:

  1. Use `unicode-segmentation` for grapheme clustering wherever the engine
     needs to count user-visible characters (e.g. text layout, cursor
     positioning).  No new dep is introduced; the crate is already in the
     workspace.

  2. Rely on the `url` crate's built-in IDNA/Punycode handling for hostname
     canonicalization.  The `url` crate calls into `idna 1.1.0` (already
     in `Cargo.lock`) and produces ACE-encoded hostnames that survive
     round-trips through the network stack without additional code.

  3. Defer the following to a future ADR (target P10 or later):
       * ICU collation (locale-sensitive string sorting)
       * ICU number/date/time formatting (Intl.* JavaScript API surface)
       * Full BiDi algorithm (Unicode TR9)
       * Unicode normalization beyond what Rust's standard library covers
       * icu4x integration

### Rationale

  * The minimal subset covers the engine's current hot paths (text layout,
    hostname parsing, basic text comparison) with zero new dependencies.
  * ICU integration is a multi-week effort; deferring it keeps P8 scope
    manageable and avoids pulling a large transitive closure into the
    workspace before the dependency vetting process (P9) runs.
  * `unicode-segmentation` is MIT-licensed, audited, and tiny (~60 KB
    compiled); there is no security argument for replacing it sooner.
  * The `idna` crate (a dep of `url`) implements IDNA 2008 + UTS#46
    mapping tables; replacing it with a bespoke implementation would be a
    cleanroom violation and an unnecessary risk.

### Consequences

Positive: zero new deps; hostname round-trip correctness guaranteed by
the existing `url` dep; grapheme cursor logic is correct for Latin and CJK
scripts; P8 ships on time.

Negative: `Intl.*` JS APIs are unimplemented (already documented in
AD-005 as out of scope for Phase 1); full BiDi layout is absent (right-
to-left rendering will be visually broken); locale-sensitive collation is
absent (JS `Array.sort` with locale comparator degrades to byte order).

### Future ADR Hook

A follow-on ADR (target AD-025 or later) will evaluate icu4x vs system
libicu at the point where `Intl.Collator`, `Intl.DateTimeFormat`, or RTL
layout becomes a tracked gap rather than a known limitation.

### Amendment (2026-07-11): direct `idna` dependency for PSL normalization

`silksurf-core` now takes a **direct** `idna` dependency (workspace-pinned
`idna 1.1.0`). This is the "direct idna dep" the ignored test in
`crates/silksurf-net/tests/idn.rs` anticipated. It adds **no new crate to
the compiled closure** -- `idna` was already there transitively via `url`
(Decision point 2) -- it only makes the dependency explicit where core's
Public Suffix List matcher (`silksurf_core::psl`) needs it. The matcher
normalizes the list's U-label (Unicode) rules to their A-label (Punycode)
form so they match the Punycode hostnames `url::host_str` yields; without
that step IDN hosts would fall through to the default `*` rule and be
over-grouped into one site (a silent isolation loss). This stays inside
the minimal-subset posture: no ICU, no bespoke IDNA (the crate's UTS#46
tables are reused, honoring the cleanroom point in the Rationale).

### See

  * `crates/silksurf-core/src/psl.rs` -- registrable-domain matcher (uses `idna`)
  * `crates/silksurf-net/tests/idn.rs` -- IDN/Punycode round-trip test
  * AD-005 -- Test262 compliance target (Intl excluded from Phase 1)
  * AD-022 (fourth amendment) -- site = eTLD+1 via the Public Suffix List
  * https://docs.rs/unicode-segmentation
  * https://docs.rs/idna

---

## AD-022: Privacy and Site Isolation Skeleton -- Deferred

**Status**: Accepted (skeleton); partition + cookie substrate partially
implemented 2026-07-11..12 -- see Amendments
**Date**: 2026-05-14 (amended 2026-07-11, 2026-07-12)
**Deciders**: Architecture Team

### Context

A production browser engine must address four interrelated privacy and
security concerns before it can be trusted with user data:

  1. **Cookie jar partitioning**: cookies scoped to (site, top-level-site)
     tuples prevent cross-site tracking via cookies.
  2. **Third-party storage partitioning**: localStorage, IndexedDB, and
     Cache Storage must be partitioned per top-level origin so embedded
     third-party frames cannot correlate user state across sites.
  3. **Fingerprinting surface audit**: JS-visible APIs (canvas, AudioContext
     timing, font enumeration, WebGL renderer string, navigator.*) expose
     entropy that trackers aggregate into stable identifiers.
  4. **Site isolation / process model**: running each site in a separate OS
     process (or at minimum a separate sandboxed thread) limits the blast
     radius of a compromised renderer.

None of these are implemented in the current codebase.  The networking
and storage layers are too immature to carry the partitioning semantics
correctly; adding partial implementations now would create false confidence
and debt that is harder to remove than absent code.

### Decision

Introduce a skeleton module (`crates/silksurf-engine/src/privacy.rs`) that
reserves the API surface and documents the deferral.  The module exposes:

  * `CookieJar` -- empty struct; implementation deferred (see below).
  * `StoragePartition` -- empty struct; implementation deferred.
  * `partition_key(origin: &str) -> String` -- placeholder that returns the
    origin unchanged.  When partitioning is implemented, this function will
    return a (site, top-level-site) key tuple serialised as a string.

All four concerns are deferred:

  * **Cookie jar partitioning**: deferred to the networking maturity phase
    (P9+).  The `CookieJar` struct will acquire fields and methods when the
    HTTP layer has a stable Set-Cookie parser and a session model.
  * **Third-party storage partitioning**: deferred to the storage layer
    (P10+).  No localStorage or IndexedDB implementation exists yet;
    partitioning will be designed in when storage lands.
  * **Fingerprinting surface audit**: deferred to P10.  A structured audit
    requires a working JS engine with Intl and canvas; the audit will be
    documented in `docs/design/THREAT-MODEL.md` once the surface exists.
  * **Site isolation**: deferred to the process model ADR (future AD-012).
    A multi-process architecture requires IPC design, sandbox integration
    (seccomp/Landlock on Linux), and a shared-memory protocol for the
    display list; none of these are in scope for P8.

### Rationale

  * Skeleton-first avoids the dual failure modes of (a) shipping nothing
    and (b) shipping a partial implementation that gives false assurance.
    The empty structs and TODO comments are honest: they say "this is where
    the work belongs, it is not done yet."
  * `partition_key` as a passthrough is the correct placeholder: callers
    that use it today will get correct behaviour once the real implementation
    lands, because all call sites already pass `origin` and the only change
    will be the return value.
  * Deferring fingerprinting audit to P10 matches the dependency on a
    working JS engine; auditing non-existent APIs is not useful.

### Consequences

Positive: the module exists as a hook for P9/P10 work; the deferral is
explicit and findable; no false assurance that privacy is implemented.

Negative: the engine has no cookie isolation, no storage partitioning,
no fingerprinting mitigations, and no process isolation.  It should not
be used with untrusted web content until these are addressed.  This
limitation is documented in `docs/design/THREAT-MODEL.md`.

### Amendment (2026-07-11): partition + cookie substrate

The skeleton is partly implemented. What is NOW real (so this ADR no
longer reads as "nothing done"):

  * **Cookie primitives + store** (`silksurf-net::cookie`): a real
    `Cookie` with domain/path/expiry/Secure/HttpOnly/SameSite, an RFC
    6265 `parse_set_cookie`, an HTTP-date parser, and a `CookieStore`
    that produces `Cookie` request headers (domain/path/Secure/expiry/
    SameSite filtered) and the `document.cookie` string. Homed in net,
    not the engine, because `silksurf-js` (document.cookie) cannot reach
    `silksurf-engine`; net is the one crate both consume.
  * **document.cookie is wired to it**: the JS bridge now stores
    attribute-aware cookies, respects expiry, and refuses to set
    HttpOnly cookies from script. This is the real consumer.
  * **Partition key** (`privacy::partition_key`): NO LONGER a passthrough.
    Signature CHANGED from `partition_key(origin) -> String` to
    `partition_key(top_level_origin, resource_origin) -> String`,
    returning `"<resource-site>^<top-level-site>"`. This supersedes the
    Decision's claim that "all call sites already pass origin and the
    only change will be the return value" -- there were no call sites
    (dead code), so the signature change is safe.
  * **StoragePartition**: real `{ key }` with `for_context` /`from_key`.
  * **PartitionedCookieStore** (net): `HashMap<key, CookieStore>` giving
    cookie isolation across partitions; tested.
  * **Origin/Site classification** (`sandbox`): real same-origin /
    same-site logic replacing the fake `SiteIsolation` registry (which
    enforced nothing -- exactly the false assurance this ADR warns of).

Further amendment (2026-07-11): the **HTTP round-trip** is now LANDED.
`BasicClient` attaches the `Cookie` header on requests and stores
`Set-Cookie` from responses; one `Arc<Mutex<CookieStore>>` per session
(on `BrowserRenderConfig`) is shared by the worker-thread fetch client
(`SpeculativeRenderer::attach_cookie_jar`) and the main-thread
`SilkContext::with_dom_and_cookies` (host-scoped to the document). The
worker/main thread split forced `Arc<Mutex>`. Verified by net round-trip
tests, a JS shared-jar bridge test, and a headless end-to-end smoke.

Third amendment (2026-07-11): **top-level-site partitioning** landed,
unlocking two features. The session jar is now `PartitionedCookieStore`
keyed by `(top_level_site, resource_site)`; the top-level site is
threaded per navigation (`BrowserRenderConfig.top_level_site`, set from
the destination URL). (1) **Partition-keyed jars**: a resource embedded
under two top-level sites gets two isolated cookie stores; document.cookie
uses the first-party partition. (2) **SameSite subresource enforcement**:
cross-site subresources withhold Strict/Lax cookies; same-site send them.
An empty top-level site degrades to `Unknown` + `UNPARTITIONED` (no
enforcement, one store) so an unplumbed path never silently drops
cookies. STILL deferred within SameSite: top-level-NAVIGATION Strict
withholding (needs a navigation initiator/referrer, not tracked).

Fourth amendment (2026-07-11): **site = registrable domain (eTLD+1)**
landed. Both site derivations -- `sandbox::Origin::site` and
`silksurf_net::cookie::site_of_url` -- now reduce the host to its
registrable domain through one entry point, `silksurf_core::psl::
registrable_domain`, backed by a vendored Public Suffix List
(`crates/silksurf-core/data/public_suffix_list.dat`, MPL-2.0, both ICANN
and PRIVATE sections). So `a.example.com` and `b.example.com` share a
site while `a.co.uk` and `b.co.uk` do not, and `a.github.io` /
`b.github.io` stay separate via the PRIVATE section. A host with no
registrable domain (IP literal, bare public suffix, `localhost`) keeps
its full host (maximally partitioned). U-label rules are normalized to
Punycode via `idna` so IDN hosts are not over-grouped -- see the AD-021
amendment for the direct-`idna` dependency this takes.

Fifth amendment (2026-07-12): **SameSite top-level-navigation
enforcement** landed. `BrowserNavigationRequest` now carries an
`initiator_site` -- the site of the page that started the navigation --
set on page-initiated navigations (link click, form GET/POST) from the
current page's URL (which is still the OLD page at request-build time;
`state.frame` is replaced only on completion), and `None` for
browser-initiated ones (address bar, bookmark, history, initial load).
`silksurf_net::cookie::navigation_same_site_context(initiator,
destination, safe_method)` classifies the top-level document fetch:
`None`/same-site initiator is `SameSite` (Strict sent); a cross-site
initiator is `CrossSiteTopLevel` for a safe method (Strict withheld,
Lax rides) and `CrossSiteSubresource` for an unsafe method (Lax withheld
too, RFC 6265bis). `BasicClient::fetch_navigation` computes the context
once and applies it across the redirect chain; the renderer routes the
top-level document (only) through it. This closes the pre-fix CSRF
exposure where a cross-site link click sent the destination's Strict
cookies (the top-level site equals the destination, so the subresource
rule saw it as same-site). `CrossSiteTopLevel` is now produced.

What is STILL deferred (so partial reads as partial):

  * **Redirect-hop SameSite reclassification**: the navigation context is
    frozen from the initiator and the original destination, so a
    cross-site redirect reached from a same-site navigation is not
    re-flagged. The partition also stays keyed on the original
    destination across a cross-site redirect (pre-existing).
  * **Third-party storage partitioning** (localStorage/IndexedDB/Cache):
    still no storage layer to partition (P10+).
  * **Fingerprinting audit** (P10) and **process-level site isolation**
    (future process-model ADR: IPC, seccomp/Landlock): unchanged.

### Alternatives Considered

  * Implement a basic in-memory `CookieJar` now -- rejected because without
    a `SameSite` parser, a `Set-Cookie` tokenizer, and a session model, an
    in-memory jar would be a leaky abstraction that callers would rely on
    before it is safe.
  * Skip the skeleton entirely -- rejected because then the deferral is
    invisible; future contributors would have to rediscover that these APIs
    are missing.

### See

  * `crates/silksurf-engine/src/privacy.rs` -- skeleton implementation
  * `docs/design/THREAT-MODEL.md` -- fingerprinting gap, cookie gap
  * AD-012 (future) -- Multi-Process Architecture / site isolation
  * Privacy and sandboxing stream

---

## AD-023: Unicode BiDi and Line-Breaking Crate Adoption

**Status**: Adopted; full render-pipeline integration deferred to typography phase
**Date**: 2026-05-14
**Deciders**: Architecture Team
**Stream**: BiDi and line-break adoption

### Context

SilkSurf already carries `unicode-segmentation` as a workspace dependency
(used for grapheme-cluster-aware text measurement).  Correct inline layout
also requires:

  * **UAX #9** (Unicode Bidirectional Algorithm) -- determines the paragraph
    embedding level and run directionality for mixed LTR/RTL text.
  * **UAX #14** (Unicode Line Breaking Algorithm) -- determines the byte
    positions where the layout engine may legally break a line of text.

Without these two algorithms the engine can only handle left-to-right
Latin text in a single line; all other cases produce incorrect results
or crash.

The Rust ecosystem provides two mature, minimal crates that implement
exactly these two standards:

  * `unicode-bidi` -- UAX #9, no unsafe, `#![no_std]`-compatible.
  * `unicode-linebreak` -- UAX #14, generated from the Unicode data tables,
    no unsafe.

HarfBuzz (full text shaping, glyph-level layout) is a larger scope and
is deferred to a future ADR once the font-loading pipeline exists.

### Decision

Adopt `unicode-bidi = "0.3"` and `unicode-linebreak = "0.1"` as
workspace dependencies.  Wire them into `crates/silksurf-layout` via
two public utility functions:

  * `bidi_level(text: &str) -> u8` -- returns the UAX #9 paragraph
    embedding level (0 = LTR, 1 = RTL).
  * `linebreak_opportunities(text: &str) -> Vec<usize>` -- returns the
    byte offsets of all Allowed and Mandatory break positions per UAX #14.

These functions are the adoption boundary; they prevent the crates from
being dead dependencies and define the interface that the render pipeline
will call once full typography integration begins.

### Rationale

  * `unicode-segmentation` is already present; adding `unicode-bidi` and
    `unicode-linebreak` completes the minimal i18n triad with no new
    transitive dependencies of note.
  * Both crates are pure Rust, `#![no_std]`-compatible, and have no unsafe
    code -- consistent with the workspace's safety posture.
  * Scoping adoption to two stub functions keeps the diff surgical and the
    PR reviewable; it does not touch the hot render path yet.
  * Deferring HarfBuzz avoids a C FFI dependency before the font pipeline
    is ready.  A future ADR will cover that boundary.
  * The stub functions give test coverage (three tests in
    `crates/silksurf-layout/tests/typography.rs`) so the adoption is
    verifiable from day one.

### Consequences

Positive:
  * The workspace now officially supports UAX #9 and UAX #14; the scope
    is visible to all contributors.
  * `bidi_level` and `linebreak_opportunities` are stable entry points
    for the typography phase; the render pipeline can call them without
    importing the raw crates.
  * Three integration tests act as a regression fence for the algorithms.

Negative:
  * Two additional crate dependencies increase compile time slightly
    (measured at <1 s for a cold `cargo test -p silksurf-layout` build).
  * Full bidirectional and line-breaking behaviour is not yet wired into
    the render pipeline -- pages with RTL text or long lines will still
    render incorrectly until the typography phase completes.

### Alternatives Considered

  * **Roll our own BiDi / line-break logic** -- rejected.  The Unicode
    algorithms are large and subtle; bugs would be silent and hard to
    detect.  The two crates are small, well-tested, and cleanroom.
  * **Adopt HarfBuzz now** -- rejected.  HarfBuzz requires a font-loading
    pipeline that does not exist yet.  Adding a large C dependency with
    no call sites would be dead weight.
  * **Defer entirely** -- rejected.  Deferring leaves the workspace without
    any UAX #9 / #14 coverage and lets incorrect inline layout accumulate
    callers that assume LTR-only behaviour.

### See

  * `crates/silksurf-layout/src/lib.rs` -- `bidi_level`, `linebreak_opportunities`
  * `crates/silksurf-layout/tests/typography.rs` -- adoption tests
  * `Cargo.toml` -- `unicode-bidi`, `unicode-linebreak` workspace entries
  * AD-021 -- Internationalization Posture (Minimal Subset, ICU Deferred)
  * BiDi and line-break adoption stream

---

## AD-024: Legacy C Tree Retirement

**Status**: Accepted
**Date**: 2026-07-09
**Deciders**: Architecture Team
**Supersedes**: the C-side implementation assignment of AD-002

### Context

AD-002 (2025-12-30) assigned DOM/HTML/CSS parsing and the GUI to a C
implementation under `src/` and `include/`, built by `CMakeLists.txt`.
The stable-Rust migration (AD-008) and the crate build-out since then
moved every one of those subsystems into owning Rust crates:

| C module | Owning Rust crate |
|---|---|
| `src/document/html_tokenizer.c`, `tree_builder.c` | `silksurf-html` |
| `src/document/dom_node.c`, `document.c` | `silksurf-dom` |
| `src/css/*`, `src/document/css_*.c` | `silksurf-css` |
| `src/layout/box_model.c`, `inline.c` | `silksurf-layout` |
| `src/rendering/*` | `silksurf-render` |
| `src/memory/arena.c`, `pool.c`, `refcount.c` | `silksurf-core` |
| `src/gui/*` (XCB, SHM, event loop) | `silksurf-gui` |

`src/README_LEGACY.md` already states the tree is a historical
cleanroom reference and not part of the Rust build, but no ADR records
that decision. Three documents (the Makefile legacy section,
`docs/development/LOCAL-GATE.md`, `docs/REPO-LAYOUT.md`) cite AD-007
for a "deprecate or integrate" decision; AD-007 is Damage Tracking and
contains no such decision. The C GUI build path is already broken: the
Makefile `gui` target references `src/css/cascade.c`, which no longer
exists, so that target has not compiled since the file's removal.

### Decision

The legacy C tree (`src/`, `include/`, `CMakeLists.txt`, and the C
sources under `tests/`) is retired. It is a historical cleanroom
reference, not a build target. Retirement executes incrementally, in
dependency order, with `make full` green after every step:

1. Code that no target builds is deleted immediately
   (`src/document/tree_builder.c`, the empty `src/core/`, the broken
   Makefile `gui` target).
2. Capabilities with no Rust equivalent are re-homed before their C
   module is deleted. The single such capability is the BPE tokenizer
   (`src/neural/bpe.c`, `src/neural/bpe_bench.c`,
   `include/silksurf/neural_bpe.h`), whose fate follows AD-006
   (Neural Integration, Experimental): port to a Rust crate if AD-006
   proceeds, archive with the C tree if AD-006 is abandoned.
3. `src/ffi/js_engine_wrapper.c` exists only to serve the C binaries
   and is deleted together with them.
4. Duplicated C modules (table above), their CMake targets, the C
   tests, and the AFL seed trees (`fuzz_in/`, `fuzz_in_css/`,
   `fuzz_corpus/`) are removed once steps 1-3 land. Rust fuzzing
   continues under `fuzz/` (cargo-fuzz).

Until removal completes, no new work extends the C tree.

### Rationale

  * Every C subsystem has an owning Rust crate; the duplication is
    pure carrying cost (~10.9k LOC) that doubles search, metric, and
    audit surface without executing in any default build.
  * The broken `gui` target demonstrates the tree receives no
    maintenance; keeping unbuildable code invites false confidence.
  * The cleanroom reference value is preserved by git history and by
    `docs/LEGACY_C_PORTING.md` (the porting map), not by keeping the
    tree checked out.

### Consequences

Positive:
  * Single build system (Cargo + Makefile wrapper) once complete.
  * Marker, line-count, and complexity metrics describe the live
    product instead of a 2x shadow surface.

Negative:
  * The BPE benchmark surface disappears until the AD-006 decision
    lands (step 2 blocks bulk deletion of `src/neural/`).
  * Contributors lose in-tree C reference reading; the porting map
    and git history replace it.

### Alternatives Considered

  * **Integrate (finish the C browser)** -- rejected. AD-008 committed
    the project to stable Rust; the C tree lost its build integrity
    (broken `gui` target) without anyone noticing, which measures its
    real maintenance level.
  * **Keep indefinitely as reference** -- rejected. Git history
    preserves the reference; a checked-out shadow tree distorts every
    repository metric and marker sweep.

### See

  * `src/README_LEGACY.md` -- prior informal statement of this decision
  * `docs/LEGACY_C_PORTING.md` -- C module -> Rust crate porting map
  * AD-002 -- Hybrid Rust + C (C-side superseded by this ADR)
  * AD-006 -- Neural Integration (governs the BPE re-home decision)
  * AD-008 -- Stable-Rust Migration
  * `docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md` -- retirement tasks

---

## AD-025: boa_engine Confirmed as the JS Runtime; Hand-Written VM Removed

**Status**: Accepted
**Date**: 2026-07-09
**Deciders**: Architecture Team (engine audit); owner decision on VM removal
**Extends**: the L7 boa_engine adoption; supersedes L7's preserve-the-VM clause

### Context

L7 adopted boa_engine 0.21 as the production JS runtime and preserved
the hand-written VM (~10k LOC: lexer, parser, bytecode compiler,
register VM, tri-color GC, Cranelift JIT scaffolding) behind the
non-default `legacy-vm` feature, labeled "NOT MAINTAINED ... expect
bitrot" in the crate manifest. The preserved tree accumulated exactly
the predicted debt: an unsound GC mark phase (roots marked, children
never traced), a JIT shim with no caller justifying five Cranelift
dependencies, a duplicate DOM bridge, platform-binding modules (wasm,
napi) that could not compile because they imported the gated VM without
its feature, and ten crate dependencies serving no reachable code.

### Engine audit

The requirement is a memory-safe, lightweight, embeddable engine for a
low-resource pure-Rust browser. Candidates:

  * **boa_engine (adopted)** -- pure Rust, no C toolchain or FFI
    boundary, memory-safe by construction, active upstream. Measured
    in-tree: 99.81% of executed test262 (69.38% of total; Intl, ESM,
    async, FinalizationRegistry skips are recorded and scheduled in the
    debt roadmap). Weakest on raw throughput, acceptable for the
    browser's script profile; the pending boa upgrade also clears
    RUSTSEC-2024-0436.
  * **rusty_v8 / deno_core** -- fastest and most conformant, but a
    multi-hundred-MB build, tens of MB of binary, and a C++ engine
    behind unsafe bindings; contradicts the low-resource profile and
    the single-toolchain build.
  * **rquickjs (QuickJS)** -- small and highly conformant, but a C
    engine behind an unsafe FFI boundary; reintroduces the C toolchain
    AD-024 just removed and moves JS memory safety outside Rust.
  * **Duktape / mujs** -- ES5-era; fail modern-web requirements.
  * **Hand-written VM (removed)** -- full ownership and the original
    cleanroom goal, but test262 parity is a multi-year effort; the
    2026-05 lexer-only baseline was 66% at tokenizer level.

boa_engine remains the correct engine: it is the only candidate that is
simultaneously pure-Rust, memory-safe, embeddable through a typed API,
and close enough to full conformance that the remaining gaps are
host-layer work (module loader, event loop, Intl data) rather than
engine replacement.

### Decision

boa_engine stays the production runtime, accessed exclusively through
`silksurf_js::SilkContext`. The hand-written VM is removed: `src/vm/`,
`src/bytecode/`, `src/lexer/`, `src/parser/`, `src/gc/`, `src/jit/`,
`src/ffi.rs`, `src/wasm.rs`, `src/napi.rs`, `src/verification.rs`, the
lexer-only test262 runner, the VM benches and examples, and the
`legacy-vm`, `jit`, `wasm`, `napi`, `mmap`, `neural`, and `constrained`
features with their dependencies (cranelift x5, wasm-bindgen,
console_error_panic_hook, napi x2, memmap2, bumpalo, bytemuck,
zerocopy, rkyv, unicode-xid, memchr, regress, phf, static_assertions,
bitvec). Git history preserves the sources;
`silksurf-specification/SILKSURF-JS-DESIGN.md` preserves the design.
The crate builds as a plain `lib`; embedders use the `SilkContext` Rust
API (a deliberate FFI surface, if ever needed, is a future ADR).

### Consequences

Positive: ~10k dormant LOC and ~15 dependencies leave the tree; the
duplicate DOM bridge and the unsound GC no longer exist to confuse
audits; JS conformance work concentrates on one engine's host layer.

Negative: reviving a custom VM means starting from git history and the
design spec rather than compiling code; wasm/napi embedding surfaces
disappear until rebuilt against boa (they did not compile anyway).

### See

  * `silksurf-js/src/boa_backend/mod.rs` -- SilkContext host layer
  * `docs/conformance/SCORECARD.md` -- dual-denominator test262 status
  * AD-005 (test262 target), AD-021 (Intl posture), AD-024 (C tree)
  * `docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md` --
    conformance-honesty-and-expansion workstream

---

## AD-026: Page-Content Accessibility Tree Deferred

**Status**: Accepted (deferral recorded; chrome a11y ships, page-content
tree deferred)
**Date**: 2026-07-11
**Deciders**: security-substrate-buildout workstream

### Context

`crates/silksurf-dom/src/a11y.rs` is a data-only skeleton
(`AccessibilityTree`, `AccessibilityNode`, an 8-variant
`AccessibilityRole`) with no builder: nothing walks the DOM to produce an
accessibility tree, and there is no AT-SPI bridge. The
`a11y-substrate-scheduling` roadmap item said to schedule it behind the
security substrate and record the deferral in an ADR if it slips again.
It has slipped again, so this ADR records the deferral.

The browser is NOT accessibility-blind at the chrome level: the app ships
a working AccessKit integration (`crates/silksurf-app/src/accessibility.rs`,
`accessibility` feature) exposing the address bar, navigation buttons,
status, links, and page inputs as an AccessKit tree. What is missing is a
*page-content* accessibility tree derived from the rendered DOM.

### Decision

Defer the page-content accessibility tree. Keep the `a11y.rs` skeleton as
the reserved surface. The deferred work is: `build_a11y_tree(dom, styles,
layout)` (role derivation from tag + `role=`, WAI-ARIA accessible-name
computation, focus/state capture, the full role set) plus an AT-SPI
exposure path in `silksurf-gui`. Prerequisite ordering: the security
substrate (cookies/partitioning) precedes it per the roadmap.

### Rationale

Chrome-level a11y already gives the shipped UI screen-reader reachability.
Page-content a11y is a large, self-contained subsystem (name computation
and the ~80-role set are non-trivial and independently testable). Bundling
it into the security batch would dilute both; a dated deferral is the
honest record the roadmap asked for.

### Consequences

Positive: the deferral is explicit and findable; chrome a11y is not
mistaken for page a11y. Negative: assistive technology cannot read page
*content* (only browser chrome) until `build_a11y_tree` lands. AT-SPI on
Linux (ADR-010) remains the long-term target.

---

## AD-027: Engine Protocol v1 -- Process-Neutral Shell/Engine Boundary

**Status**: Accepted (control plane specified and implemented; frame transport
deferred to the extraction spike)
**Date**: 2026-07-23
**Deciders**: Browser functionalization program (issue #50, P1)

### Context

The shell and the page runtime share one process. `BrowserState` holds the
chrome, history, and focused input directly beside `BrowserPageRuntime`, which
owns the DOM, `SilkContext`, layout, display list, and pixel buffers. A page
hang or exploit is therefore a shell hang or exploit. AD-022 named site
isolation as a deferred concern and pointed at a "future process-model ADR";
the "Future ADRs" list reserved AD-012 for "Multi-Process Architecture." Neither
was written. The verified audit (issue #50) confirmed there is no engine-process
boundary and no message contract.

The program must also keep the compatibility-backend verdict open: WPE, Wry,
Servo, and CEF are candidates (DG-1..DG-3), and none of them can be forced
through a boundary that leaks SilkSurf's `Dom`, `NodeId`, Boa values, CSS
structures, Taffy nodes, or display-list entries.

### Decision

Introduce engine protocol v1: a process-neutral, view-oriented message contract
in `silksurf_core::engine_protocol`, specified in
`docs/design/ENGINE-PROTOCOL-V1.md`. The boundary carries browser-view
operations only -- create/close view, navigate/reload/stop, resize/visibility,
input, permission/download/file-chooser/new-view requests, load-state and
metadata events, and a frame handle plus damage. It never carries engine
internals.

The protocol separates a control plane from a frame plane. The control plane
(commands, events, input) is fully serialized, decoded, and validated by this
crate now; malformed control messages are rejected with a typed `ProtocolError`
and never panic the receiver. The frame plane is an abstract `FrameHandle`
carrying a view id, a monotonic `FrameGeneration`, and an opaque transport
token plus a byte length; the concrete transport (sealed shared memory first,
DMA-BUF later) is bound at the native-runtime extraction spike (issue #53).

Version negotiation uses `ProtocolVersion { major, minor }`: majors are
incompatible by construction, and the agreed version is the highest common
minor within a shared major. Capabilities negotiate as an intersection; an
unmet capability is answered with `Event::CapabilityMismatch`, not a panic.
Lifecycle state machines for engine, view, and frame validate transitions
through explicit tables and return `IllegalTransition` rather than mutating on
an illegal edge.

The module homes in `silksurf-core` because both `silksurf-app` and
`silksurf-engine` already depend on it, so the protocol reaches both sides
without dragging engine internals into the shell. It is a split candidate: once
#53 measures ownership, it extracts to `silksurf-engine-protocol`. No new crate
topology is frozen by this ADR.

### Rationale

- Control-plane-first (not envelope-first) makes the anti-panic property real:
  the malformed-message tests exercise body decode, which is the exact path
  #53's "protocol errors cannot panic the shell" invariant depends on.
- Abstracting only the frame handle avoids committing to a wire format that
  #53's transport choice may discard, while still fixing the generation and
  release ownership that prevents a stale engine from overwriting a presented
  frame.
- A `u64`-newtype id space and a length-prefixed, discriminant-tagged envelope
  give unknown-message skip-ahead for forward minor compatibility and bounded
  decode for hostile input.

### Consequences

Positive: the shell/engine contract exists and is testable before any process
is spawned; the backend verdict stays open because the contract is
engine-neutral; AD-022's deferred process-model boundary has a concrete first
realization.

Negative: no process is extracted yet (issue #53), so the isolation benefit is
not yet delivered; the frame transport is unspecified until #53 measures it.

### Alternatives Considered

- A generic `BrowserEngine` trait exposing DOM/CSS/JS/layout objects -- rejected
  because it forces incompatible engines into a lowest-common-denominator
  pseudo-browser model and leaks internals the boundary exists to hide.
- Deferring all message-body serialization to #53 behind an opaque envelope --
  rejected because an envelope with no decoded bodies cannot test the anti-panic
  property, which is the boundary's whole purpose.

### See

  * `docs/design/ENGINE-PROTOCOL-V1.md` -- full message and state-machine spec
  * `crates/silksurf-core/src/engine_protocol/` -- implementation
  * AD-022 -- Privacy and Site Isolation Skeleton (deferred the process model)
  * issue #50 -- browser functionalization program; #52 (this spec), #53
    (native-runtime extraction and frame transport)

---

## Decision Log

| ID | Title | Status | Date | Impact |
|----|-------|--------|------|--------|
| AD-001 | Cleanroom Implementation | Accepted | 2025-12-30 | High |
| AD-002 | Hybrid Rust + C | C-side superseded by AD-024 | 2025-12-30 | High |
| AD-003 | Pure XCB GUI | Accepted | 2025-12-31 | High |
| AD-004 | Arena Allocator | Accepted | 2025-12-31 | Medium |
| AD-005 | Test262 95% Target | Accepted | 2025-12-31 | Medium |
| AD-006 | Neural Integration | Experimental | 2025-12-31 | Low |
| AD-007 | Damage Tracking | Accepted | 2025-12-31 | High |
| AD-008 | Stable-Rust Migration + MSRV Declaration | Accepted | 2026-04-30 | High |
| AD-009 | Strict-Local-Only CI Policy | Accepted | 2026-04-30 | High |
| AD-010 | GUI Backend Formalization (XCB-Only, Linux-First) | Accepted | 2026-04-30 | High |
| AD-016 | Fused Render Pipeline (FusedWorkspace) | Accepted | 2026-04-30 | High |
| AD-017 | Lock-free Monotonic Resolve Table | Accepted | 2026-04-30 | High |
| AD-018 | Persistent On-Disk Response Cache | Accepted | 2026-04-30 | Medium |
| AD-019 | tls-probe as Supported Diagnostic Surface | Accepted | 2026-04-30 | Medium |
| AD-020 | Workspace-Wide Canonical Error (SilkError) | Accepted | 2026-04-30 | High |
| AD-021 | Internationalization Posture (Minimal Subset, ICU Deferred) | Accepted | 2026-05-14 | Medium |
| AD-022 | Privacy and Site Isolation Skeleton (Deferred) | Accepted | 2026-05-14 | High |
| AD-023 | Unicode BiDi and Line-Breaking Crate Adoption | Adopted | 2026-05-14 | Medium |
| AD-024 | Legacy C Tree Retirement | Accepted | 2026-07-09 | High |
| AD-025 | boa_engine Confirmed; Hand-Written VM Removed | Accepted | 2026-07-09 | High |
| AD-026 | Page-Content Accessibility Tree Deferred | Accepted | 2026-07-11 | Medium |
| AD-027 | Engine Protocol v1 -- Process-Neutral Shell/Engine Boundary | Accepted | 2026-07-23 | High |

---

## AD-028: Document Resources as Live State -- Stylesheets, Preloads, Reflection

**Status**: Accepted (implemented on branch live-document-stylesheets)
**Date**: 2026-08-19
**Deciders**: Public web page load repairs

### Context

The page pipeline treated the document's resources as a parse-time snapshot.
`extract_stylesheet_urls` ran once on the freshly parsed DOM and produced a
flat `css_text` String before any script executed. `<link rel=preload>` was
collected for module warming alone and fired no `load` event. Element wrappers
carried live accessors for `id`, `className`, `value`, and `src` and nothing
else, so a page's `element.rel = "stylesheet"` wrote a plain JavaScript
property that no engine stage observed.

Each of the three is enough on its own to leave a real page unstyled.
chatgpt.com serves its 79 KB stylesheet as `<link rel=preload as=style>` and
upgrades the rel from the link's load handler: the preload was never fetched,
the handler was never registered because `document.currentScript` was a
constant null, the rel assignment would not have reached the DOM, and a
changed rel would not have re-entered the cascade. Measured before the change,
the document's entire author CSS was its inline `<style>`: 100,831 bytes over
one source.

### Decision

Model the document's resources as live state the engine re-collects from the
DOM.

`StyleSheetSet` (crates/silksurf-app/src/stylesheet_set.rs) holds the ordered
stylesheet list, one entry per `<style>` element and per `<link
rel=stylesheet>`, keyed by owning NodeId plus resolved href. `refresh`
recollects when the tree shape moved or a dirty node is a `<style>` or
`<link>`, and re-reads inline text when only `Dom::generation` moved. A
changed list reparses the concatenated text and rebuilds `StyleIndex` against
it, which forces a full repaint because a cascade input that changed
document-wide has no damage rect to ride. Link bodies fetch on worker threads
and arrive over an mpsc channel, so the repaint tick never blocks.

`PreloadLinks` (crates/silksurf-app/src/link_preload.rs) fetches each `<link
rel=preload>` with the Accept header its `as` attribute selects and dispatches
`load` or `error` at the element through `SilkContext::dispatch_dom_event`.
The body is discarded; the warmed HTTP cache is what the follow-on fetch
reads.

IDL attribute reflection (silksurf-js/src/boa_backend/dom_interfaces.rs,
REFLECTION_BOOTSTRAP) defines accessors on the interface prototypes rather
than per wrapper, reaching the node through `this.nodeId`. Four reflection
kinds cover the HTML table: string, URL resolved against `document.URL`,
boolean by attribute presence, and long with a zero default.

`SilkContext::set_current_script` brackets each classic script evaluation with
the `<script>` element that carries it, which HTML defines and which pages
read to reach their own tag.

### Consequences

A page that appends a `<style>`, rewrites a link's rel, swaps an href, or sets
any reflected property now moves the engine. Measured on chatgpt.com the
author CSS rises to 180,449 bytes over two sources and the cascade from 345 to
496 rules.

The cost is a tree walk per stylesheet-affecting mutation. Gating it on
`Dom::structure_generation` plus a dirty-node check keeps a React reconcile
off that path: `make gui-probe-page-click` measures 113 us average against a
recorded 112, and the reconcile probes 210 us against a 190-260 band.

`document.styleSheets` and the CSSOM stay absent: the set is engine state, not
a scripted object model, so a page cannot enumerate or mutate a sheet through
script. The payload keeps `css_text` beside `sheet_bodies` so the first frame
paints from the text the navigation worker assembled while the runtime set
converges.

---

## AD-029: CSS Custom Properties Resolved in the Cascade

**Status**: Accepted (implemented on branch live-document-stylesheets)
**Date**: 2026-08-19
**Deciders**: Public web page load repairs

### Context

`crates/silksurf-css/src/custom_properties.rs` supplied `CustomPropertyMap`
and `resolve_var_references` and was exported from the crate root, and no
caller ever constructed either. Every `var()` reference therefore resolved to
nothing and the declaration holding it was discarded. Modern CSS puts each
colour, radius, and spacing value behind a token, so the cascade dropped most
of what a page declares: a headless fixture declaring
`background-color: var(--tone)` painted white whether the property was
declared at `:root`, on `html`, or on the element itself.

### Decision

Resolve custom properties in the cascade, in the two passes CSS Custom
Properties 1 5 requires.

`resolve_custom_properties` runs first and settles which custom-property
declaration wins for each name by importance, then specificity, then document
order -- the same precedence `apply_property` enforces for typed properties --
seeded from the parent element's map, because custom properties inherit.
`apply_substituted_declaration` then substitutes each remaining declaration's
value against that map before applying it; a value holding no `var()` applies
from its own tokens and allocates nothing.

`ComputedStyle` carries the map behind an `Arc`. An element that declares none
shares its parent's allocation, so the map allocates once per element that
actually declares one. `StyleIndex::declares_custom_properties` is computed
once at index build, and a document that declares none anywhere skips the pass
on one bool per element.

### Consequences

`var()` resolves through inheritance, fallbacks, nesting, inline style
attributes, and both the longhand and the shorthand forms; twenty tests in
`crates/silksurf-css/tests/shorthands_and_variables.rs` pin those cases.

Registered custom properties are not modelled: `@property` declares a syntax,
an inherits flag, and an initial value, and the parser treats the at-rule as
a declaration block it discards. An unregistered property therefore has no
initial value, so `var(--unset)` with no fallback leaves the declaration
unapplied rather than falling back to a registered initial. chatgpt.com opens
its sheet with four `@property` declarations, which is where this surfaces
first.

Custom properties resolve at computed-value time, so a value that changes
between elements re-resolves per element. There is no dependency graph and no
invalidation short-cut: a changed custom property invalidates through the
ordinary cascade rebuild.

---

## AD-030: CSSOM Style Sheets as the Scripted View of Live Sheet State

**Status**: Accepted (implemented on branch document-stylesheets-cssom)
**Date**: 2026-08-19
**Deciders**: Public web page load repairs

### Context

AD-028 made the document's stylesheets live engine state and named the
remaining gap: "`document.styleSheets` and the CSSOM stay absent: the set is
engine state, not a scripted object model." The roadmap entry
`document-stylesheets-cssom` reads that gap as a page contributing nothing to
the cascade when it installs styles through script rather than through a
`<style>` element.

The measured failure is sharper than the roadmap states. chatgpt.com's module
graph is 1163 chunks reachable from `/cdn/assets/manifest-56d12409.js`; seven
touch the CSSOM. One of them decides whether the page paints its component
styles at all. Chunk `e0691314-j25je0r3ld54d0o7.js` is Emotion's style sheet,
whose `insert` reaches its target sheet through

```js
function r(e){ if(e.sheet) return e.sheet;
  for(var t=0;t<document.styleSheets.length;t++)
    if(document.styleSheets[t].ownerNode===e) return document.styleSheets[t]; }
```

and whose speedy mode is on by default: `this.isSpeedy = e.speedy === void 0
|| e.speedy`. The `try { n.insertRule(e, n.cssRules.length) } catch {}` that
follows guards the insertion alone; the accessor call `var n = r(t)` sits
outside it. A `SilkContext` probe over a document carrying a `<style>` element
measures `document.styleSheets` undefined, `CSSStyleSheet` undefined,
`styleElement.sheet` undefined, and `document.adoptedStyleSheets` undefined,
so the accessor raises `TypeError: cannot convert 'null' or 'undefined' to
object` and the throw leaves `insert` rather than being swallowed. Every
Emotion rule the page writes is lost, and the loss is a thrown exception in
the page's own style path, not a silent degradation.

The other six consumers rank differently. Chunk
`bcae0416-e2x9v208ubq1vkey.js` is CodeMirror's StyleModule, which gates on
`e.adoptedStyleSheets && r.CSSStyleSheet` and otherwise builds `<style>` text
that AD-028 already collects; it works today. Chunk
`2340486e-eab5bn2wcgxcv5rd.js` wraps `CSSStyleSheet.prototype.insertRule`,
`CSSMediaRule`, and `CSSSupportsRule` and reads `cssText`, `parentStyleSheet`,
and `ownerNode` to emit `StyleSheetRule` mutation records; it is a session
recorder and moves no pixels. Chunk `27597608-biyt09iz6nivrt6s.js` calls
`attachShadow`, `new CSSStyleSheet`, and `replaceSync`, which need Shadow DOM.

Feature detection makes a partial implementation actively worse than none.
CodeMirror takes its working text path only while `adoptedStyleSheets` stays
undefined, so defining the constructed-sheet surface without the adoption
plumbing moves a working consumer onto a broken path.

Three engine facts bound the design. `StyleSheetSet`
(crates/silksurf-app/src/stylesheet_set.rs:171) derives its sources by walking
the DOM and exposes `css_text()` alone, so a sheet has no identity a script
could hold. `refresh` (stylesheet_set.rs:291) watches
`Dom::structure_generation`, `Dom::generation`, and dirty nodes matching
`is_style_source_element`, and an `insertRule` call moves none of them.
`install_computed_style_provider` (crates/silksurf-app/src/page_build.rs:663)
clones the `Stylesheet` into its closure and is called once at page build
(page_build.rs:227), so `getComputedStyle` answers from a parse-time snapshot
and already fails to observe the AD-028 rebuild.

### Decision

Model the CSSOM as the scripted view of the sheet state AD-028 already keeps
live, rather than as a second store script mutates behind the engine's back.

The concatenated `Stylesheet` becomes an ordered list carrying origin.
`StyleIndex::for_viewport_sheets(&[Stylesheet], w, h)` flattens the list in
document order, which is what walking one concatenation already did, and the
single-sheet `for_viewport` wraps it so the seven construction sites change by
one line each. Origin tagging keeps `DEFAULT_USER_AGENT_STYLESHEET`
(stylesheet_set.rs:257) out of the scripted collection.

Sheet state moves behind a handle both sides hold. silksurf-js already depends
on silksurf-css, so the shared type needs no new crate edge. The app runtime
and `SilkContext` share it, and `document.styleSheets` enumerates the author
entries with `ownerNode`, `href`, `media`, `disabled`, `cssRules`,
`insertRule`, and `deleteRule`. `HTMLStyleElement.sheet` and
`HTMLLinkElement.sheet` resolve to the same object, which is the branch
Emotion takes first and the one that avoids the enumeration entirely.

`insertRule` splices the sheet's `Vec<Rule>` and raises a dirty flag the
repaint tick drains, on the precedent of `SilkContext::storage_dirty`
(silksurf-js/src/boa_backend/mod.rs:853). Draining it enters the same
`StyleIndex` rebuild and full repaint that `refresh_runtime_stylesheets`
(crates/silksurf-app/src/runtime_repaint.rs:169) runs when sheet text changes,
without reparsing the text a script never touched. Emotion reads
`cssRules.length` before every insert, so the retained rule list is what keeps
that read O(1) instead of a reparse per inserted rule.

`cssText` and `selectorText` need `Rule`, `SelectorList`, and `Declaration`
serialization, which silksurf-css does not have in any form today. The
serializer lands in silksurf-css beside the parser so the parse and the
serialization of a rule stay one review away from each other.

`install_computed_style_provider` takes the shared handle instead of a cloned
`Stylesheet`. This repairs the AD-028 snapshot defect on its own and is the
only way a rule inserted through script can reach `getComputedStyle` at all.

### Consequences

A page that inserts rules through `CSSStyleSheet.insertRule` reaches the
cascade, and chatgpt.com's Emotion styles stop throwing out of the page's own
style path. The rebuild cost per drained tick is the existing `StyleIndex`
rebuild and full repaint that AD-028 measured at 113 us on
`make gui-probe-page-click`; the CSSOM path avoids AD-028's reparse of the
concatenated text, which the parse of chatgpt's 180,449 bytes dominates.

Three pieces are cut by name. `constructed-stylesheets-and-adoption` leaves
`new CSSStyleSheet`, `replaceSync`, and `adoptedStyleSheets` undefined, which
needs Shadow DOM for its only observed consumer and keeps CodeMirror on its
working text fallback. `cssom-synchronous-restyle` leaves an `insertRule`
followed by a `getComputedStyle` read in the same script observing the
pre-insert cascade, because the rebuild rides the tick. `cssom-grouping-rules`
leaves `CSSMediaRule` and `CSSSupportsRule` without the `parentRule` and
`parentStyleSheet` walk the session recorder performs, which records telemetry
and paints nothing.

Each cut is a defined surface rather than a partial one: the constructed-sheet
API stays absent so feature detection keeps choosing the path that works.

---

## AD-031: Transforms Baked Into the Paint Rect as an Axis-Aligned Affine

**Status**: Accepted (implemented on branch css-transform-affine-subset)
**Date**: 2026-08-20
**Deciders**: Public web page load repairs

### Context

`parse_translation` (crates/silksurf-css/src/style.rs:4548) matches
`translate`, `translate3d`, `translateX`, and `translateY` and drops every
other function through its `_ => {}` arm, so `ComputedStyle::transform` is a
`Translation { x: Length, y: Length }` and nothing else survives the cascade.
Measured against the tree:

```
translate(-50%)              -> Translation { x: Percent(-50.0), y: Px(0.0) }
scale(0)                     -> Translation { x: Px(0.0), y: Px(0.0) }
scale(.82)                   -> Translation { x: Px(0.0), y: Px(0.0) }
rotate(45deg)                -> Translation { x: Px(0.0), y: Px(0.0) }
matrix(1, 0, 0, 1, 10, 20)   -> Translation { x: Px(0.0), y: Px(0.0) }
none                         -> Translation { x: Px(0.0), y: Px(0.0) }
```

`scale(0)` is byte-identical to `none`, so an element the page collapses to
nothing paints at full size. `matrix()` drops the translation component it
carries, which is the one transform kind the engine claims to support.

chatgpt.com's author CSS names six distinct transform values across its two
sheets: `translate(-50%)`, `translate(-50%, .25rem)`, `translateY(var(--...))`,
`scale(0)`, `scale(.82)`, and `scale(100%) rotate(-90deg)`. There is no
`matrix()`, no `skew()`, and no 3D. The load is percentage translate, which
`translation_px` (crates/silksurf-engine/src/fused_pipeline.rs:1086) already
resolves against the node's own border box, plus scale, which contributes
nothing today.

Six surfaces assume an axis-aligned rect. `DisplayItem`
(crates/silksurf-render/src/lib.rs:72) carries a `Rect` on every variant and no
matrix. Three independent rasterizers draw those rects: `paint_skia_item`
(crates/silksurf-render/src/lib.rs:1246) through tiny-skia with
`Transform::identity()` at every call, the scalar `rasterize`
(crates/silksurf-render/src/lib.rs:228), and the ARGB-word path
(crates/silksurf-app/src/argb_raster.rs:769) with its own 5x7 bitmap font.
`build_tiles` (crates/silksurf-render/src/lib.rs:311) buckets by rect,
`rect_contains` (crates/silksurf-app/src/dom_hit_test.rs:238) is a
point-in-rect test, and `union_rect`
(crates/silksurf-app/src/redraw_geometry.rs:217) unions damage.

### Decision

Retain the parsed function list and compose it at paint into the affine that
keeps a rect a rect.

`TransformFunction` records `translate`, `scale`, `rotate`, `skew`, and
`matrix` as the parser reads them, and `ComputedStyle` holds the list behind an
`Arc` so an element declaring no transform shares one empty list.
Percentage translate stays a `Length` in the retained form because CSS
Transforms 1, 3 resolves it against the element's own border box, which the
cascade does not know.

The paint pass composes each node's list about its border-box centre into
`(scale_x, scale_y, dx, dy)` and multiplies it into the accumulated parent
value, replacing the `(dx, dy)` addition that `apply_transform_offsets`
(crates/silksurf-engine/src/fused_pipeline.rs:1045) performs today.
Composition is `sx = parent.sx * child.sx` and `dx = parent.dx + parent.sx *
child.dx`, so a child of a `scale(.5)` parent that declares `translate(100px)`
moves 50 px. `transformed_rect` scales the width and height it already
offsets, and a text item's `font_size` scales with the vertical factor, which
re-rasterizes the run through cosmic-text rather than magnifying a coverage
bitmap.

Baking into the rect is what leaves the six axis-aligned surfaces untouched. A
matrix per `DisplayItem` reaches all three rasterizers, the tiling, the
hit-test, and the damage union; a scale folded into `Rect` reaches none of
them, because that is what the translation offset already does.
`scale(0)` yields a zero-area rect, which `sk_rect`
(crates/silksurf-render/src/lib.rs:1474) and `pixel_rect_from_rect`
(crates/silksurf-app/src/runtime_repaint.rs:867) both answer `None` for, so
the element paints nothing.

### Consequences

A page that collapses a popover with `scale(0)` stops painting it at full size,
and one that shrinks a control with `scale(.82)` paints it at the size it asks
for. A `matrix()` contributes its translation and scale components rather than
nothing.

Three pieces are cut by name. `transform-rotation-and-skew` leaves `rotate`,
`skew`, and a `matrix` with a non-zero `b` or `c` term contributing nothing:
such a matrix's `a` and `d` terms are cosines rather than scale factors, so
reading them as scale would shrink the box toward zero at 90 degrees, and
painting a rotated element as its axis-aligned bounding box would put a
wrong-sized box on screen for anything non-square. `transform-origin-property` leaves
the origin at the `50% 50%` default CSS Transforms 1, 6 specifies, because the
property parses nowhere in the workspace today.
`transformed-rect-hit-test-and-damage` records that `LinkTarget` reads the
transformed paint rect (crates/silksurf-app/src/dom_hit_test.rs:6) while
`InputTarget` and both damage rects read the untransformed `node_rects`
(crates/silksurf-app/src/redraw_geometry.rs:212), which is a pre-existing
inconsistency that a scale widens.

---

## Future ADRs

Planned (renumbered after the 2026-04-30 batch):

  * AD-011: Wayland Support Strategy
  * AD-012: Multi-Process Architecture (browser vs renderer processes) --
    first realized by AD-027 (engine protocol v1); process extraction tracked
    in issue #53
  * AD-013: Extension API Design
  * AD-014: Network Stack (libcurl vs custom)
  * AD-015: Image Decoding (libpng/libjpeg vs custom)
  * AD-016: Fused Render Pipeline (capturing the design now in main)
  * AD-017: Lock-free Monotonic Resolve Table
  * AD-018: Persistent On-Disk Response Cache
  * AD-019: tls-probe as Supported Diagnostic Surface
  * AD-020: Error-Type Unification (`silksurf_core::SilkError`)

The 2026-04-30 batch (AD-008..AD-010) covers foundations + GUI; AD-016..
AD-020 are queued for the documentation-baseline work in
`docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md`.

---

## See Also

  * `/CLAUDE.md` -- Engineering standards
  * `/CONTRIBUTING.md` -- Onboarding and gate discipline
  * `/docs/development/LOCAL-GATE.md` -- Local-gate reference
  * `/docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md` -- Debt inventory and reconciliation plan
  * `/silksurf-specification/` -- Technical specifications
