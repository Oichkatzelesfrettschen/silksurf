# SilkSurf: Ultra-Optimized Browser Project

**Vision**: Fastest, leanest, most resource-efficient browser using XCB + optimized web engine

**Core Philosophy**:
- Cleanroom implementation (no wholesale copying, architectural learning only)
- NeoSurf web engine as reference (libhubbub, libcss, libdom, Duktape)
- Pure XCB for GUI (no GTK, no dependencies)
- Novel XCB acceleration tricks
- Extreme optimization: CPU, RAM, VRAM, framebuffer

---

## PHASE 1: RESEARCH & DESIGN (NOW)

### 1.1 Documentation & Analysis
- [x] Full NetSurf vs NeoSurf diff (completed)
- [ ] Download XCB documentation
- [ ] Analyze performance bottlenecks
- [ ] Design cleanroom architecture
- [ ] Prototype memory/performance strategy

### 1.2 Directory Structure
```
silksurf/
├── docs/                      # Architecture & design docs
│   ├── XCB_GUIDE.md          # XCB programmer's reference
│   ├── ARCHITECTURE.md        # System design
│   ├── OPTIMIZATION.md        # Performance strategy
│   └── MEMORY_MODEL.md        # RAM management
├── src/
│   ├── core/                  # Web engine (from NeoSurf)
│   │   ├── content/           # HTML/CSS/JS handlers
│   │   ├── desktop/           # Desktop abstraction
│   │   └── utils/             # Generic utilities
│   ├── gui/                   # XCB GUI framework
│   │   ├── xcb_wrapper.c      # Low-level XCB bindings
│   │   ├── window.c           # Window management
│   │   ├── widgets.c          # Minimal widget set
│   │   └── render.c           # Rendering pipeline
│   ├── rendering/             # Graphics & optimization
│   │   ├── pixel_ops.c        # Optimized pixel operations
│   │   ├── damage_tracking.c  # Partial update tracking
│   │   ├── buffer_pool.c      # Memory pooling
│   │   └── shader_accel.c     # GPU acceleration (DRI, DRI2, DRI3)
│   └── main.c                 # Entry point
├── include/
│   ├── silksurf/
│   │   ├── browser.h
│   │   ├── gui.h
│   │   ├── renderer.h
│   │   └── config.h
├── CMakeLists.txt             # Build system
├── Makefile                   # Fast rebuild
└── perf/
    ├── benchmarks.c           # Performance tests
    ├── memory_trace.c         # Memory profiling
    └── profile.sh             # Profiling scripts
```

---

## PHASE 2: SETUP & DOCUMENTATION

### 2.1 XCB Foundation
**Key Documents to Acquire**:
- X11 Programmer's Manual (Xlib vs XCB comparison)
- XCB API reference
- Composite extension (for buffer management)
- XFixes, XDamage (damage tracking)
- GLX/DRI for GPU acceleration

**Optimization Strategies**:
1. Batched protocol requests (reduce round-trips)
2. Damage tracking (only redraw changed regions)
3. Pixmap caching (avoid redraws)
4. Double/triple buffering (flicker-free)
5. Shared memory XShm (zero-copy rendering)
6. GPU acceleration via DRI3/Wayland

### 2.2 Memory & CPU Optimization
**From Analysis**:
- NeoSurf: 28 MB baseline, 1026 C/H files
- NetSurf: 55 MB (bloated by platforms)
- **Target**: <10 MB baseline (67% reduction)

**Strategies**:
1. Static allocation where possible
2. Object pooling (reuse objects)
3. Lazy loading (features on demand)
4. Minimal GTK → XCB (eliminate dependency bloat)
5. Inline critical paths (cache-friendly)
6. SIMD optimizations (SSE2/AVX for rendering)

---

## PHASE 3: CORE IMPLEMENTATION

### 3.1 Web Engine (80% from NeoSurf reference)
- Extract/rewrite libhubbub (HTML5 parser)
- Extract/rewrite libcss (CSS parser)
- Extract/rewrite libdom (DOM tree)
- Use Duktape (JavaScript, already small)
- Custom fetcher (HTTP/file/about)
- Minimal but correct rendering

### 3.2 XCB GUI Framework (Novel)
**No GTK = No 50MB dependency**

```c
// Minimal XCB abstraction
struct xcb_window {
    xcb_window_t id;
    xcb_gcontext_t gc;
    xcb_pixmap_t backbuffer;
    int width, height;
    void (*on_expose)(struct xcb_window *);
};

struct xcb_button {
    xcb_rectangle_t rect;
    void (*on_click)(void *);
    char *label;
};
```

**Components** (minimal):
- Window manager (single window, no fancy decorations)
- Buttons, text input, scrollbars (pure XCB)
- Menu bar (XCB drawing)
- Status bar
- Tab bar (optional)

### 3.3 Rendering Pipeline (Novel Acceleration)
**Damage-tracked rendering**:
```
1. Track dirty regions via XDamage
2. Only redraw damaged areas
3. Cache rendered content
4. Use pixmaps for immutable content
5. Batch X protocol requests
```

**VRAM optimization**:
```
1. Pixmap reuse pool
2. Lazy pixmap allocation
3. Mipmaps for downscaled images
4. Compressed texture formats (DXT)
5. GPU-side format conversion
```

**CPU optimization**:
```
1. SIMD pixel ops (memcpy, blending)
2. Lookup tables (gamma, color convert)
3. Inline hot paths
4. Cache-friendly data layout
5. Prefetch hints for large ops
```

---

## PHASE 4: OPTIMIZATION TRICKS

### 4.1 Novel XCB Acceleration
1. **Batch requests**: Queue 50+ X calls, send once
2. **Pixmap pooling**: Pre-allocate, reuse pixmaps
3. **XShm extensions**: Zero-copy shared memory transport
4. **Composite**: Off-screen rendering, atomic updates
5. **Damage extension**: Track dirty regions precisely
6. **GPU acceleration**: DRI3 + EGL for video codec acceleration

### 4.2 Memory Tricks
1. **Arena allocators**: Single large allocation, subdivide
2. **String interning**: Single copy of repeated strings
3. **Object pooling**: Allocate once, recycle
4. **Compact data layout**: Minimize padding/pointers
5. **Copy-on-write**: Share data until modified
6. **Reference counting**: Track object lifetime

### 4.3 CPU Tricks
1. **Instruction cache**: Keep hot code in L1/L2
2. **Branch prediction**: Avoid mispredicts
3. **Loop unrolling**: Manual for critical paths
4. **Vectorization**: SIMD for pixel ops
5. **Prefetching**: Hints for memory access
6. **Inline assembly**: Critical inner loops

---

## PHASE 5: BENCHMARKING & PROFILING

### 5.1 Metrics
- **Startup time**: Target <500ms
- **Memory**: Target <10 MB baseline
- **VRAM**: Target <50 MB for complex pages
- **Frame rate**: Target 60 FPS, 120 FPS stretch
- **CPU**: <5% idle, <20% on scroll

### 5.2 Tools
- `perf` - CPU profiling
- `valgrind` - Memory profiling
- Custom benchmarks (load time, scroll FPS)

---

## Expected Outcomes

**vs NetSurf (55 MB)**:
- Binary size: -70%
- Memory: -70%
- Startup: -50%
- GUI responsiveness: +100%

**vs NeoSurf GTK (28 MB)**:
- Binary size: -60%
- Memory: -60%
- GUI responsiveness: +50%
- Startup: -30%

**vs Firefox (200+ MB)**:
- Binary size: -95%
- Memory: -90%
- Startup: -80%

---

## Success Criteria

1. ✓ Builds cleanly with CMake
2. ✓ Renders complex HTML/CSS correctly
3. ✓ Executes JavaScript
4. ✓ Handles images, media
5. ✓ Fast XCB GUI (no flicker, smooth scroll)
6. ✓ <10 MB RAM baseline
7. ✓ <500ms startup
8. ✓ 60+ FPS rendering

