# Phase 4c: Parallel Layout Engine - Granular Roadmap

**Goal**: Implement Flow-inspired parallel layout with hybrid threading, multi-platform rendering, and comprehensive SIMD optimization

**Status**: Planning Complete | Implementation Starting
**Timeline**: 4-6 weeks for complete implementation
**Performance Target**: 50-60% layout speedup on quad-core, 2x animation FPS

---

## Architecture Decisions

### Threading: Hybrid TBB + OpenMP + pthreads
- **Intel TBB**: Work-stealing scheduler, parallel algorithms
- **OpenMP**: Simple parallelization with #pragma directives
- **pthreads**: Fine-grained control where needed
- **Fallback**: Graceful degradation to single-threaded

### Rendering: Triple-Path Architecture
- **XCB**: Pure X11 rasterization (primary, always available)
- **OpenGL**: GPU-accelerated rendering (performance)
- **Wayland**: Native compositor integration (modern)
- **Runtime selection**: Detect display server and GPU capabilities

### SIMD: Multi-ISA with Clean Fallbacks
- **x86**: SSE2 (baseline), SSE4.1, AVX, AVX2
- **ARM**: NEON (ARMv7+), SVE (ARMv8+)
- **PowerPC**: Altivec/VMX
- **Generic**: Portable C fallback for all operations
- **Runtime dispatch**: CPU feature detection at startup

### Core Scaling: Dynamic 1-16 Cores
- Detect CPU topology at runtime (`sysconf`, `hwloc`)
- Scale thread pool to available cores
- Work-stealing for load balancing
- NUMA-aware scheduling (future enhancement)

---

## Phase Breakdown

### Phase 4b (PREREQUISITE) - CSS Cascade & Selector Matching

**Status**: In Progress
**Time**: 3-5 days

#### Tasks:
1. **Selector Matching Engine** (2 days)
   - [ ] Implement selector parser (tag, class, ID, attribute, pseudo)
   - [ ] Build selector matching against DOM nodes
   - [ ] Handle combinators (descendant, child, sibling, general sibling)
   - [ ] Optimize with Bloom filters for fast rejection

2. **Specificity Calculation** (1 day)
   - [ ] Implement (inline, IDs, classes, elements) tuple
   - [ ] Compare specificity for conflict resolution
   - [ ] Handle !important flag
   - [ ] Test specificity edge cases

3. **Cascade Resolution** (1-2 days)
   - [ ] Implement cascade algorithm (origin, specificity, order)
   - [ ] Handle inheritance (font-family, color, etc.)
   - [ ] Compute final styles for each element
   - [ ] Cache computed styles per element

4. **Style Application** (1 day)
   - [ ] Extract CSS properties to silk_computed_style_t
   - [ ] Handle default values and initial values
   - [ ] Implement currentColor and inherit keywords
   - [ ] Test style application end-to-end

**Test Suite**:
- [ ] Selector matching tests (100 test cases)
- [ ] Specificity calculation tests (50 cases)
- [ ] Cascade resolution tests (75 cases)
- [ ] Style inheritance tests (30 cases)

---

### Phase 4c.0 - Foundation (Week 1)

**Dependencies**: Phase 4b complete
**Focus**: Threading infrastructure, SIMD abstraction, core detection

#### 4c.0.1: SIMD Abstraction Layer (2-3 days)

**Architecture**:
```c
/* Generic SIMD operations with ISA-specific implementations */
typedef struct {
    void (*memcpy_simd)(void *dst, const void *src, size_t len);
    void (*memset_simd)(void *dst, int val, size_t len);
    void (*blend_alpha)(uint32_t *dst, uint32_t *src, size_t count);
    void (*scale_image)(uint32_t *dst, uint32_t *src, int scale);
    /* ... more operations ... */
} simd_ops_t;

/* Runtime dispatch based on CPU features */
extern simd_ops_t *silk_simd_ops;
```

**Tasks**:
- [ ] Create `src/simd/simd_dispatch.c` - CPU feature detection
- [ ] Implement `src/simd/simd_sse2.c` - SSE2 baseline (x86)
- [ ] Implement `src/simd/simd_avx2.c` - AVX2 optimizations
- [ ] Implement `src/simd/simd_neon.c` - ARM NEON
- [ ] Implement `src/simd/simd_altivec.c` - PowerPC Altivec
- [ ] Implement `src/simd/simd_generic.c` - Portable C fallback
- [ ] Add compile-time ISA detection (CMake)
- [ ] Add runtime CPU feature detection (CPUID, getauxval)
- [ ] Test SIMD dispatch on multiple architectures

**Files to Create**:
```
include/silksurf/simd.h          (public SIMD API)
src/simd/simd_dispatch.c         (runtime dispatcher)
src/simd/simd_sse2.c             (x86 SSE2)
src/simd/simd_avx2.c             (x86 AVX2)
src/simd/simd_neon.c             (ARM NEON)
src/simd/simd_altivec.c          (PowerPC)
src/simd/simd_generic.c          (C fallback)
tests/test_simd.c                (unit tests)
```

#### 4c.0.2: Hybrid Threading Infrastructure (2-3 days)

**Architecture**:
```c
/* Thread pool with work-stealing */
typedef struct {
    /* TBB task scheduler (primary) */
    void *tbb_scheduler;

    /* OpenMP fallback */
    int use_openmp;

    /* pthread fallback */
    pthread_t *threads;
    int thread_count;

    /* Work-stealing queue */
    work_queue_t *work_queues;
} silk_thread_pool_t;
```

**Dependencies**:
- Intel TBB library (`libtbb-dev`)
- OpenMP support (`-fopenmp`)
- pthreads (built-in)

**Tasks**:
- [ ] Check for TBB availability (CMake `find_package(TBB)`)
- [ ] Implement TBB task scheduler wrapper
- [ ] Implement OpenMP parallel regions (fallback #1)
- [ ] Implement pthread pool (fallback #2)
- [ ] Implement work-stealing queue (lock-free)
- [ ] Add thread pool lifecycle (create, destroy, resize)
- [ ] Add dynamic thread count adjustment
- [ ] Test thread pool under load

**Files to Create**:
```
include/silksurf/thread_pool.h   (public API)
src/threading/thread_pool_tbb.c  (TBB implementation)
src/threading/thread_pool_omp.c  (OpenMP fallback)
src/threading/thread_pool_pthread.c (pthread fallback)
src/threading/work_queue.c       (work-stealing queue)
tests/test_thread_pool.c         (unit tests)
```

#### 4c.0.3: Dynamic Core Detection (1 day)

**Tasks**:
- [ ] Implement CPU topology detection
  - `sysconf(_SC_NPROCESSORS_ONLN)` (POSIX)
  - `/proc/cpuinfo` parsing (Linux)
  - `hwloc` library integration (optional, advanced)
- [ ] Detect logical vs physical cores
- [ ] Detect NUMA nodes (future: NUMA-aware scheduling)
- [ ] Create `silk_cpu_info_t` structure
- [ ] Test on 1, 2, 4, 8, 16 core systems

**Files to Create**:
```
include/silksurf/cpu_info.h
src/platform/cpu_detect_linux.c
src/platform/cpu_detect_bsd.c
tests/test_cpu_detect.c
```

---

### Phase 4c.1 - Parallel DOM Traversal (Week 2)

**Dependencies**: 4c.0 complete
**Focus**: Identify independent layout contexts

#### 4c.1.1: Layout Context Analysis (2 days)

**Algorithm**:
1. Traverse DOM tree
2. Identify block formatting contexts (BFC)
3. Identify independent layout subtrees:
   - Paragraphs (`<p>`)
   - Flex containers (`display: flex`)
   - Grid containers (`display: grid`)
   - Table cells (`<td>`)
   - Absolutely positioned elements
4. Build dependency graph
5. Queue independent contexts for parallel processing

**Tasks**:
- [ ] Create `layout_context_t` structure
- [ ] Implement BFC detection
- [ ] Implement independence analysis (no vertical overlap)
- [ ] Build layout context queue
- [ ] Handle nested contexts (flex inside flex)
- [ ] Visualize layout context tree (debug mode)

**Files to Create**:
```
include/silksurf/layout_context.h
src/layout/layout_context.c
src/layout/bfc_detector.c
tests/test_layout_context.c
```

#### 4c.1.2: Work Queue Population (1 day)

**Tasks**:
- [ ] Create work items for each layout context
- [ ] Assign priority (viewport-visible contexts first)
- [ ] Distribute to thread pool work queues
- [ ] Handle dependencies (parent must complete before child)
- [ ] Test queue population with complex DOM

**Files to Create**:
```
src/layout/work_scheduler.c
tests/test_work_scheduler.c
```

---

### Phase 4c.2 - Multi-Pass Layout Algorithm (Week 3)

**Dependencies**: 4c.1 complete
**Focus**: Parallel intrinsic size calculation, flex resolution

#### 4c.2.1: Pass 1 - Intrinsic Sizes (Parallel) (3 days)

**Algorithm**:
```
parallel_for each layout_context:
    calculate_min_content_width(context)
    calculate_max_content_width(context)
    store in context->intrinsic_sizes
```

**Tasks**:
- [ ] Implement `calculate_min_content_width()` for:
  - [ ] Text (word wrapping, hyphenation)
  - [ ] Images (intrinsic width)
  - [ ] Flex containers (min of flex items)
  - [ ] Grid containers (min of tracks)
  - [ ] Tables (min of cells)
- [ ] Implement `calculate_max_content_width()` for all above
- [ ] Parallelize with TBB `parallel_for` or OpenMP
- [ ] Test with various content types
- [ ] Benchmark speedup (target: 40-50% faster on quad-core)

**Files to Create**:
```
src/layout/intrinsic_sizes.c
src/layout/intrinsic_text.c
src/layout/intrinsic_flex.c
src/layout/intrinsic_grid.c
tests/test_intrinsic_sizes.c
```

#### 4c.2.2: Pass 2 - Flex Layout Resolution (Parallel) (2 days)

**Algorithm**:
```
parallel_for each flexbox in layout_contexts:
    resolve_flex_base_sizes()
    distribute_remaining_space()
    align_flex_items()
```

**Tasks**:
- [ ] Implement flexbox algorithm (CSS Flexbox spec)
- [ ] Handle flex-grow, flex-shrink, flex-basis
- [ ] Handle flex-direction (row, column)
- [ ] Handle flex-wrap
- [ ] Parallelize independent flexboxes
- [ ] Test flexbox layouts

**Files to Create**:
```
src/layout/flex_layout.c
tests/test_flex_layout.c
```

#### 4c.2.3: Pass 3 - Final Positioning (Sequential/Hybrid) (2 days)

**Algorithm**:
```
/* Sequential for vertical positioning */
for each layout_context in document_order:
    position_vertically(context, current_y_offset)
    current_y_offset += context->height

/* Parallel for horizontal positioning */
parallel_for each layout_context:
    position_horizontally(context)
```

**Tasks**:
- [ ] Implement vertical positioning (sequential - vertical dependencies)
- [ ] Implement horizontal positioning (parallel - independent)
- [ ] Handle absolutely positioned elements (out of flow)
- [ ] Handle fixed positioning
- [ ] Handle z-index stacking contexts
- [ ] Test final positioning accuracy

**Files to Create**:
```
src/layout/positioning.c
src/layout/stacking_context.c
tests/test_positioning.c
```

---

### Phase 4c.3 - Rendering Backends (Week 4-5)

**Dependencies**: 4c.2 complete
**Focus**: Triple-path rendering (XCB, OpenGL, Wayland)

#### 4c.3.1: XCB Rasterization with SIMD (2-3 days)

**Tasks**:
- [ ] Implement XCB pixmap rendering
- [ ] Use SIMD for pixel operations (memcpy, blend, scale)
- [ ] Implement XShm zero-copy rendering
- [ ] Implement damage tracking (XDamage)
- [ ] Implement double buffering (XComposite)
- [ ] Test on various X11 servers
- [ ] Benchmark SIMD speedup

**Files to Create**:
```
src/rendering/xcb_renderer.c
src/rendering/xcb_simd_blit.c
tests/test_xcb_rendering.c
```

#### 4c.3.2: OpenGL GPU Rendering (3-4 days)

**Tasks**:
- [ ] Create OpenGL 3.3+ context (GLX or EGL)
- [ ] Implement GPU texture upload
- [ ] Implement shader pipeline:
  - [ ] Vertex shader (quad rendering)
  - [ ] Fragment shader (texturing, alpha blending)
  - [ ] Text rendering shader (SDF or bitmap fonts)
- [ ] Implement GPU text rendering
- [ ] Implement GPU canvas/SVG operations
- [ ] Test on Intel/AMD/NVIDIA GPUs
- [ ] Benchmark vs CPU rendering

**Files to Create**:
```
src/rendering/opengl_renderer.c
src/rendering/gl_context.c
src/rendering/gl_shaders.c
src/rendering/gl_text.c
shaders/quad.vert
shaders/texture.frag
shaders/text_sdf.frag
tests/test_opengl_rendering.c
```

#### 4c.3.3: Wayland Compositor Integration (2-3 days)

**Tasks**:
- [ ] Create Wayland surface
- [ ] Implement `wl_surface` protocol
- [ ] Implement buffer submission
- [ ] Implement damage tracking (wl_surface.damage)
- [ ] Implement subsurfaces (for overlay UI)
- [ ] Test on Sway, GNOME Wayland, KDE Wayland
- [ ] Handle DPI scaling

**Files to Create**:
```
src/rendering/wayland_renderer.c
src/rendering/wayland_surface.c
tests/test_wayland_rendering.c
```

#### 4c.3.4: Runtime Renderer Selection (1 day)

**Algorithm**:
```c
/* Priority: OpenGL > Wayland > XCB */
if (has_opengl && gpu_capable)
    use_opengl_renderer()
else if (wayland_display)
    use_wayland_renderer()
else if (x11_display)
    use_xcb_renderer()
else
    fallback_software_renderer()
```

**Tasks**:
- [ ] Detect display server (Wayland vs X11)
- [ ] Detect GPU capabilities (OpenGL version, extensions)
- [ ] Select optimal renderer
- [ ] Allow user override (env var: `SILKSURF_RENDERER`)
- [ ] Test renderer switching

**Files to Create**:
```
src/rendering/renderer_select.c
tests/test_renderer_select.c
```

---

### Phase 4c.4 - Work-Stealing & Load Balancing (Week 6)

**Dependencies**: All rendering backends complete
**Focus**: Optimize parallel efficiency

#### 4c.4.1: Work-Stealing Queue Implementation (2 days)

**Algorithm**: Chase-Lev lock-free deque

**Tasks**:
- [ ] Implement lock-free double-ended queue
- [ ] Implement work stealing protocol:
  - Local thread: pop from bottom (LIFO - cache locality)
  - Stealing thread: steal from top (FIFO - breadth-first)
- [ ] Handle ABA problem with hazard pointers
- [ ] Test under high contention
- [ ] Benchmark vs locked queue

**Files to Create**:
```
src/threading/work_stealing_deque.c
tests/test_work_stealing.c
```

#### 4c.4.2: Dynamic Load Balancing (2 days)

**Tasks**:
- [ ] Implement work imbalance detection
- [ ] Trigger work stealing when thread idle
- [ ] Implement backoff strategy (reduce stealing overhead)
- [ ] Track thread utilization
- [ ] Visualize load distribution (debug mode)
- [ ] Test with unbalanced workloads

**Files to Create**:
```
src/threading/load_balancer.c
tests/test_load_balancing.c
```

#### 4c.4.3: NUMA-Aware Scheduling (Future) (2 days)

**Tasks** (Optional - Advanced):
- [ ] Detect NUMA topology (`libnuma`)
- [ ] Allocate memory on local NUMA node
- [ ] Schedule threads on local CPU cores
- [ ] Benchmark NUMA vs non-NUMA

---

## Testing & Benchmarking

### Unit Tests (Throughout Development)
- [ ] SIMD operations correctness
- [ ] Thread pool scalability
- [ ] Layout context detection
- [ ] Intrinsic size calculation
- [ ] Rendering output correctness

### Integration Tests
- [ ] End-to-end parallel layout with real HTML/CSS
- [ ] Multi-threaded rendering stress test
- [ ] Cross-renderer compatibility (XCB, OpenGL, Wayland)

### Performance Benchmarks
- [ ] Layout performance: 1 core vs 4 cores vs 8 cores
- [ ] Animation FPS: XCB vs OpenGL
- [ ] SIMD speedup: SSE2 vs AVX2 vs NEON
- [ ] Memory usage scaling
- [ ] Cache efficiency (perf stat)

**Target Metrics**:
- Layout: 50-60% faster on quad-core
- Animation: 2x FPS improvement
- Core scaling: Linear up to 4-8 cores
- Memory: <15 MB for typical page

---

## Reference Browser Analysis

### Clone & Analyze (Background Task)
- [x] Sciter SDK - Lightweight HTML/CSS engine
- [ ] Servo - Parallel rendering in Rust
- [ ] Ladybird - SerenityOS browser
- [ ] Flow - Proprietary (architecture analysis only)

### Analysis Focus
1. **Threading Architecture**:
   - How do they parallelize layout?
   - Work distribution strategies
   - Synchronization primitives

2. **SIMD Usage**:
   - Which operations are SIMD-accelerated?
   - Multi-ISA support patterns
   - Fallback strategies

3. **Rendering Pipeline**:
   - GPU vs CPU rasterization
   - Display server integration
   - Damage tracking approaches

---

## Risk Mitigation

### Technical Risks
1. **TBB Availability**: Fallback to OpenMP/pthreads (mitigated)
2. **GPU Driver Issues**: XCB fallback always available (mitigated)
3. **Lock-Free Bugs**: Extensive testing with ThreadSanitizer
4. **Performance Regression**: Continuous benchmarking

### Schedule Risks
1. **Complexity Underestimation**: Break tasks into smaller pieces
2. **Dependency Delays**: Parallelize independent work streams
3. **Testing Overhead**: Write tests concurrently with implementation

---

## Success Criteria

Phase 4c is complete when:
- [x] All unit tests passing (coverage >80%)
- [x] Layout 50%+ faster on quad-core vs single-threaded
- [x] Animation 2x FPS vs single-threaded
- [x] All three rendering backends working (XCB, OpenGL, Wayland)
- [x] SIMD dispatch working on x86, ARM, PowerPC
- [x] Dynamic core scaling (1-16 cores)
- [x] Comprehensive documentation

---

**Next**: Complete Phase 4b CSS cascade, then begin Phase 4c.0 SIMD abstraction
