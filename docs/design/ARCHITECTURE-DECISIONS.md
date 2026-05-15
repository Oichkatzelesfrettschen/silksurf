# SilkSurf Architecture Decision Records (ADRs)

**Purpose**: Document key architectural decisions with rationale and alternatives
**Format**: Context → Decision → Rationale → Consequences → Alternatives
**Updated**: 2026-01-29

---

## AD-001: Cleanroom Implementation Strategy

**Status**: ✅ Accepted
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

✅ **Positive**:
- Clean IP, no licensing concerns
- Optimized for modern use cases, no legacy baggage
- Team gains deep spec knowledge
- Can make unconventional choices (arena allocators, pure XCB)

⚠️ **Negative**:
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

**Status**: ✅ Accepted
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

✅ **Positive**:
- Memory safety for JS engine (most attack surface)
- Can use battle-tested NetSurf libraries immediately
- Rust's zero-cost abstractions for performance
- C's simplicity reduces cognitive load for core rendering

⚠️ **Negative**:
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

- C ↔ Rust FFI via extern "C" ABI
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

**Status**: ✅ Accepted
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

✅ **Positive**:
- Fastest possible rendering path
- No GTK theme engine overhead
- Small memory footprint (<1MB for GUI layer)
- Direct access to X11 extensions (XShm, XDamage, XComposite)
- Perfect control over event handling

⚠️ **Negative**:
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
- `/diff-analysis/XCB_GUIDE.md`

---

## AD-004: Arena Allocator for DOM/Layout

**Status**: ✅ Accepted
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

✅ **Positive**:
- 10-100x faster allocation than malloc
- Zero fragmentation
- Simpler code (no individual cleanup)
- Better cache locality (30% speedup on traversals)
- Memory usage peaks are predictable

⚠️ **Negative**:
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
- Typical page: ~1000 DOM nodes × 128 bytes = 128KB
- Complex page: ~10,000 nodes × 128 bytes = 1.28MB
- Arena size: 2MB default (allows growth)

**References**:
- `/src/memory/allocator.c` - Arena implementation
- `SILKSURF-C-CORE-DESIGN.md` Section 2.1

---

## AD-005: Test262 95%+ Compliance Target

**Status**: ✅ Accepted
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

✅ **Positive**:
- Faster time to usable product
- Clear communication of limitations
- Can prioritize common features
- Realistic goal for small team

⚠️ **Negative**:
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

**Status**: 🟡 Experimental
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

✅ **Positive**:
- Potential 20-40% lexing speedup
- Novel research contribution
- Demonstrates ML integration in systems software
- Optional feature (can disable)

⚠️ **Negative**:
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
- `silksurf-specification/neural_bpe.md`

---

## AD-007: Damage Tracking for Rendering

**Status**: ✅ Accepted
**Date**: 2025-12-31
**Context**:

Full-screen redraws are expensive (1920×1080×4 bytes = 8MB per frame). Most changes are local.

**Decision**:

Implement damage tracking - record which screen regions changed, only redraw those.

**Rationale**:

1. **Performance**: 10x fewer pixel updates for typical interactions
2. **Power**: Reduced GPU/CPU usage, better battery life
3. **Responsiveness**: Faster redraws for small changes
4. **Standard**: X11 Damage extension is mature and well-supported

**Consequences**:

✅ **Positive**:
- 100+ FPS rendering (vs 10-20 FPS full redraw)
- Reduced power consumption
- Smoother scrolling and animations
- Better use of GPU bandwidth

⚠️ **Negative**:
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
- Text cursor blinking (10×20 pixel region)
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
**Deciders**: SNAZZY-WAFFLE roadmap (P0)

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
**Deciders**: SNAZZY-WAFFLE roadmap (P0)

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
**Deciders**: SNAZZY-WAFFLE roadmap (P0/P6)

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

## AD-016: Fused Render Pipeline (FusedWorkspace)

**Status**: Accepted
**Date**: 2026-04-30 (codifies design from `main` = `409356d`)
**Deciders**: SNAZZY-WAFFLE roadmap (P2.S3)

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
**Deciders**: SNAZZY-WAFFLE roadmap (P2.S3)

### Context

`Dom` holds a `RwLock<SilkInterner>`. The cascade matching path called
`dom.resolve(atom) -> SmallString` once per atom comparison (~29 atoms
per cascade); each call paid ~6 ns of `RwLock::read` acquisition
overhead, totalling ~168 ns per cascade just on lock traffic.

### Decision

Add a per-`Dom` `resolve_table: Vec<SmallString>`, materialised from
the interner's `values_slice()` at two phase boundaries:

  1. `TreeBuilder::into_dom()` -- after parse completes.
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
**Deciders**: SNAZZY-WAFFLE roadmap (P2.S3)

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
**Deciders**: SNAZZY-WAFFLE roadmap (P2.S3)

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
**Date**: 2026-04-30 (lands in SNAZZY-WAFFLE Wave 1 Batch 2)
**Deciders**: SNAZZY-WAFFLE roadmap (P1.S1)

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

**Status**: Accepted
**Date**: 2026-05-14
**Deciders**: SNAZZY-WAFFLE roadmap (P8.S4)

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

### See

  * `crates/silksurf-net/tests/idn.rs` -- IDN/Punycode round-trip test
  * AD-005 -- Test262 compliance target (Intl excluded from Phase 1)
  * https://docs.rs/unicode-segmentation
  * https://docs.rs/idna

---

## AD-022: Privacy and Site Isolation Skeleton -- Deferred

**Status**: Accepted (skeleton only; implementation deferred)
**Date**: 2026-05-14
**Deciders**: SNAZZY-WAFFLE roadmap (P8.S9)

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
  * SNAZZY-WAFFLE P8.S9 -- Privacy/sandboxing stream

---

## AD-023: Unicode BiDi and Line-Breaking Crate Adoption

**Status**: Adopted; full render-pipeline integration deferred to typography phase
**Date**: 2026-05-14
**Deciders**: Architecture Team
**SNAZZY-WAFFLE stream**: P8.S3

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
  * SNAZZY-WAFFLE P8.S3 -- BiDi / line-break adoption stream

---

## Decision Log

| ID | Title | Status | Date | Impact |
|----|-------|--------|------|--------|
| AD-001 | Cleanroom Implementation | Accepted | 2025-12-30 | High |
| AD-002 | Hybrid Rust + C | Accepted | 2025-12-30 | High |
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

---

## Future ADRs

Planned (renumbered after the 2026-04-30 batch):

  * AD-011: Wayland Support Strategy
  * AD-012: Multi-Process Architecture (browser vs renderer processes)
  * AD-013: Extension API Design
  * AD-014: Network Stack (libcurl vs custom)
  * AD-015: Image Decoding (libpng/libjpeg vs custom)
  * AD-016: Fused Render Pipeline (capturing the design now in main)
  * AD-017: Lock-free Monotonic Resolve Table
  * AD-018: Persistent On-Disk Response Cache
  * AD-019: tls-probe as Supported Diagnostic Surface
  * AD-020: Error-Type Unification (`silksurf_core::SilkError`)

The 2026-04-30 batch (AD-008..AD-010) covers foundations + GUI; AD-016..
AD-020 are queued for the documentation-baseline phase (P2) of the
SNAZZY-WAFFLE roadmap.

---

## See Also

  * `/CLAUDE.md` -- Engineering standards
  * `/CONTRIBUTING.md` -- Onboarding and gate discipline
  * `/docs/development/LOCAL-GATE.md` -- Local-gate reference
  * `/docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md` -- Implementation status
  * `/silksurf-specification/` -- Technical specifications
