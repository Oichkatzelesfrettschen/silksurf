# Legacy C Architecture (Archived)

## Legacy System Overview (C baseline)

```
┌─────────────────────────────────────────────────────┐
│                   SilkSurf Browser                   │
├─────────────────────────────────────────────────────┤
│ GUI Layer (XCB)                                     │
│ ├─ Window management (XCB)                          │
│ ├─ Event handling (keyboard, mouse)                 │
│ └─ Widget rendering (buttons, scrollbars)           │
├─────────────────────────────────────────────────────┤
│ Rendering Engine                                    │
│ ├─ Damage tracking (XDamage)                       │
│ ├─ Pixmap caching (pool + LRU)                     │
│ ├─ SIMD pixel ops (SSE2/AVX2)                      │
│ └─ XShm zero-copy upload                           │
├─────────────────────────────────────────────────────┤
│ Web Engine (from NeoSurf)                           │
│ ├─ HTML5 parser (libhubbub reference)             │
│ ├─ CSS engine (libcss reference)                   │
│ ├─ DOM tree (libdom reference)                     │
│ ├─ JavaScript (Duktape)                            │
│ └─ Fetch (HTTP, file, about)                       │
├─────────────────────────────────────────────────────┤
│ Memory Management                                   │
│ ├─ Arena allocator (20 MB pool)                    │
│ ├─ Object pooling (DOM nodes, styles)              │
│ └─ Reference counting                              │
└─────────────────────────────────────────────────────┘
```

---

## Legacy C File Organization (historical)

```
silksurf/
├── CMakeLists.txt                    # Build config
├── docs/
│   ├── MASTER_PLAN.md               # Project vision
│   ├── XCB_GUIDE.md                 # XCB reference
│   ├── OPTIMIZATION_STRATEGY.md      # Perf targets
│   ├── ARCHITECTURE.md               # This file
│   ├── PROFILING.md                 # Measurement
│   └── DESIGN_DECISIONS.md           # Trade-offs
├── include/silksurf/
│   ├── config.h                      # Configuration
│   ├── gui.h                         # XCB GUI API
│   ├── browser.h                     # Browser API
│   ├── renderer.h                    # Rendering API
│   ├── allocator.h                   # Memory management
│   └── util.h                        # Utilities
├── src/
│   ├── main.c                        # Entry point
│   ├── gui/
│   │   ├── xcb_wrapper.c             # XCB binding
│   │   ├── window.c                  # Window mgmt
│   │   ├── event.c                   # Event loop
│   │   └── widgets.c                 # Buttons, scrollbars
│   ├── rendering/
│   │   ├── renderer.c                # Main renderer
│   │   ├── damage_tracker.c          # Damage tracking
│   │   ├── pixmap_cache.c            # Pixmap LRU
│   │   ├── pixel_ops.c               # SIMD ops
│   │   └── buffer_pool.c             # Memory pools
│   ├── core/
│   │   ├── html_parser.c             # HTML5 (from NeoSurf)
│   │   ├── css_engine.c              # CSS (from NeoSurf)
│   │   ├── dom_tree.c                # DOM (from NeoSurf)
│   │   ├── js_engine.c               # Duktape binding
│   │   └── fetcher.c                 # HTTP/file/about
│   ├── memory/
│   │   ├── arena.c                   # Arena allocator
│   │   ├── pool.c                    # Object pools
│   │   └── refcount.c                # Reference counting
│   └── util/
│       ├── hash.c                    # Hash tables
│       ├── string.c                  # String interning
│       └── perf.c                    # Profiling
├── perf/
│   ├── benchmarks.c                  # Performance tests
│   ├── memory_trace.c                # Memory profiling
│   └── profile.sh                    # Profiling scripts
└── diff-analysis/                    # NetSurf vs NeoSurf
    └── COMPREHENSIVE_DIFF_REPORT.md
```

---

## Design Principles

### 1. Minimal Dependencies
- Only `libxcb` (no GTK, no Qt, no toolkit bloat)
- Duktape for JavaScript (already embedded in NeoSurf)
- Optional: pixman, libcurl (if not bundled)

### 2. Extreme Optimization Focus
- Every allocation tracked
- Every hot path profiled
- SIMD everywhere possible
- Cache-aware data layout

### 3. Cleanroom Implementation
- Architecture learned from NeoSurf/NetSurf
- Code written from scratch
- No wholesale copying (copyright/licensing)
- Better suited to our optimization goals

### 4. Progressive Enhancement
- Phase 1: Basic rendering
- Phase 2: Full HTML/CSS/JS
- Phase 3: Performance optimization
- Phase 4: Advanced features (tabs, history, etc.)

---

## Memory Layout (Target: <10 MB)

### Allocation Strategy
```
┌─────────────────────────────────┐
│  20 MB Arena Allocator          │
├─────────────────────────────────┤
│  DOM Tree (reused)              │  ~1 MB
│  Style cache                    │  ~0.5 MB
│  Rendering buffers              │  ~2 MB
│  String pool (interned)         │  ~0.3 MB
│  Object pools                   │  ~1 MB
│  Pixmap cache                   │  ~5 MB
│  Free space (fragmentation)     │  ~9.7 MB
└─────────────────────────────────┘
```

### Key Optimization: String Interning
```
Before: "hello" allocated 6 times = 36 bytes
After:  "hello" allocated once, 6 pointers = 6 bytes + 6 * 8 = 54 bytes
        (but: shared font names, CSS keywords = huge savings)
```

---

## Rendering Pipeline

### Frame Rendering Flow
```
1. User input (click, scroll, key)
   ↓
2. Update DOM/layout
   ↓
3. Collect damaged regions (XDamage)
   ↓
4. Check pixmap cache (hit = skip rendering)
   ↓
5. SIMD render to shared memory (XShm)
   ↓
6. Batch X requests (minimize round-trips)
   ↓
7. Upload via XCB (single flush)
   ↓
8. Present to screen (Composite if available)
```

### Damage Tracking Benefit
```
Before: Scroll 100px → redraw entire 1024x768 = 786,432 pixels
After:  Scroll 100px → redraw 1024x100 = 102,400 pixels
Savings: 87%
```

---

## Performance Targets (Phase 5)

| Metric | Phase 1 | Phase 2 | Phase 3 | Phase 5 | Target |
|--------|---------|---------|---------|---------|--------|
| Binary | 1 MB | 3 MB | 4 MB | 5 MB | <5 MB |
| Memory | 50 MB | 30 MB | 15 MB | 10 MB | <10 MB |
| Startup | 2s | 800ms | 600ms | 400ms | <500ms |
| FPS | 30 | 45 | 55 | 65+ | 60+ |
| CPU (idle) | 10% | 7% | 5% | 3% | <5% |

---

## Diff Strategy (NeoSurf vs NetSurf)

### What We're Taking from NeoSurf
1. **Architecture**: CMake build, bundled deps, `src/` organization
2. **Cleanliness**: Removed 75% of legacy platform code
3. **Codebase**: 1026 C/H files vs NetSurf's 866 (refactored, not padded)
4. **Bundled libs**: libhubbub, libcss, libdom (guaranteed compatibility)

### What We're Rejecting
1. GTK dependency (50+ MB)
2. Bloated frontends (riscos, amiga, atari, windows)
3. Resource bundles (2+ MB of icons, localization)
4. C++ code (faster to optimize pure C with SIMD)

### What We're Innovating
1. XCB rendering (instead of GTK Cairo)
2. Arena allocator (instead of malloc fragmentation)
3. Damage tracking (XDamage for partial updates)
4. Pixmap pooling (VRAM reuse)
5. SIMD optimization (manual vectorization)

---

## Success Metrics (Phase 5)

**Minimal Viable Product (Phase 1)**:
- [x] XCB window creation
- [x] Event handling
- [x] Simple rectangle drawing
- Target: 1 MB binary, loads trivial HTML

**Feature Complete (Phase 3)**:
- [ ] Full HTML5 parsing
- [ ] CSS rendering
- [ ] JavaScript execution
- [ ] Image rendering
- Target: 4 MB binary, loads real websites

**Production Ready (Phase 5)**:
- [ ] <10 MB memory baseline
- [ ] 60+ FPS rendering
- [ ] <500ms startup
- [ ] <3% idle CPU
- [ ] Full web compatibility

---

## Risk Mitigation

| Risk | Mitigation |
|------|-----------|
| XCB learning curve | XCB_GUIDE.md + reference code |
| Memory fragmentation | Arena allocator from Phase 1 |
| Rendering bugs | Damage tracking tests |
| Performance regressions | Continuous profiling (perf, valgrind) |
| SIMD portability | Fallback C implementations |
| X11 dependencies | Graceful degradation of features |
