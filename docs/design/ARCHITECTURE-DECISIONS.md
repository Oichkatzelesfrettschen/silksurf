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

## Decision Log

| ID | Title | Status | Date | Impact |
|----|-------|--------|------|--------|
| AD-001 | Cleanroom Implementation | ✅ Accepted | 2025-12-30 | High |
| AD-002 | Hybrid Rust + C | ✅ Accepted | 2025-12-30 | High |
| AD-003 | Pure XCB GUI | ✅ Accepted | 2025-12-31 | High |
| AD-004 | Arena Allocator | ✅ Accepted | 2025-12-31 | Medium |
| AD-005 | Test262 95% Target | ✅ Accepted | 2025-12-31 | Medium |
| AD-006 | Neural Integration | 🟡 Experimental | 2025-12-31 | Low |
| AD-007 | Damage Tracking | ✅ Accepted | 2025-12-31 | High |

---

## Future ADRs

**Planned**:
- AD-008: Wayland Support Strategy
- AD-009: Multi-Process Architecture (browser vs renderer processes)
- AD-010: Extension API Design
- AD-011: Network Stack (libcurl vs custom)
- AD-012: Image Decoding (libpng/libjpeg vs custom)

---

## See Also

- `/CLAUDE.md` - Engineering standards
- `/docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md` - Implementation status
- `/silksurf-specification/` - Technical specifications
