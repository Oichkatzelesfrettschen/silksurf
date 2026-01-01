# SilkSurf Optimization Strategy

**Goal**: <10 MB memory baseline, 60+ FPS rendering, <500ms startup

---

## 1. Memory Optimization

### 1.1 Target Breakdown (10 MB)
```
Web engine core:     2.5 MB  (HTML/CSS parser, DOM)
JavaScript (Duktape): 0.8 MB
Rendering pipeline:  0.7 MB  (buffers, caches)
XCB/GUI:             0.5 MB
Resources/fonts:     0.3 MB
Overhead:            5.2 MB  (code, data, glibc, etc.)
────────────────────────
TOTAL:              10.0 MB
```

### 1.2 Techniques

**Arena Allocators**: 
- One large allocation, subdivide
- Reduces fragmentation
- Single free() at exit
- ~30% savings vs malloc

**Object Pooling**:
- Reuse DOM nodes, styles, etc.
- Pre-allocate typical workloads
- Reduces alloc/free overhead

**Compact Data Layout**:
- Minimize padding
- Pack bitfields
- Pointer packing (32-bit pointers where possible)

**Copy-on-Write**:
- Shared strings (intern frequently used strings)
- Shared stylesheets
- Shared font data

---

## 2. CPU Optimization

### 2.1 Target Metrics
```
Startup:          <500 ms (vs Firefox: 2-3s)
Page render:      <100 ms for average page
Idle CPU:         <5% (vs GTK: 15-20%)
Scroll:           60+ FPS (vs GTK: 30-40 FPS)
Zoom:             120+ FPS (interactive)
```

### 2.2 Techniques

**Profiling-Guided**:
1. Find hot paths with `perf`
2. Optimize 20% of code that uses 80% of time
3. Inline critical functions
4. SIMD for pixel operations

**Branch Prediction**:
- Likely/unlikely hints
- Order branches by frequency
- Avoid data-dependent branches

**Cache Optimization**:
- Data layout for cache lines (64 bytes)
- Temporal locality (reuse soon)
- Spatial locality (access nearby memory)

**SIMD Optimization**:
- SSE2 for memory operations (memcpy, memset)
- AVX2 for color conversion
- Intrinsics for manual unrolling

---

## 3. Framebuffer/VRAM Optimization

### 3.1 Target
- Typical webpage: <50 MB VRAM
- Complex page: <100 MB VRAM
- Streaming video: <200 MB VRAM

### 3.2 Techniques

**Pixmap Pooling**:
```c
struct pixmap_pool {
    xcb_pixmap_t pixmaps[1000];
    int width[1000], height[1000];
    bool in_use[1000];
    int count;
};

// Request pixmap (reuse if available)
xcb_pixmap_t pool_acquire(int w, int h) {
    // Search for matching size
    // If found unused, reuse
    // If not, allocate new
}
```

**Lazy Pixmap Allocation**:
- Only allocate when rendered
- Free when off-screen
- Track reference count

**Mipmap Generation**:
- Downscaled versions for CSS zoom
- Store once, reuse

**Compressed Formats**:
- JPEG for photos
- PNG for graphics
- WebP if available

---

## 4. Rendering Optimization

### 4.1 Damage Tracking
```
1. User scrolls 100px
2. XDamage reports dirty region
3. Only redraw 100px height
4. Saves 90% of pixels
```

**Implementation**:
- XDamage extension
- Track bounding rectangles
- Batch updates

### 4.2 Dirty Region Merging
```c
// Merge overlapping regions to reduce work
struct region {
    int x, y, width, height;
};

void merge_regions(struct region *regions, int *count) {
    // If region A and B overlap >50%, merge
    // Reduces from 1000 rects to 10
}
```

### 4.3 Incremental Rendering
```
Frame 1: Render div.box (changed)
Frame 2: Render p.text (changed)
Frame 3: Render img (changed)
→ Spread work across frames
→ Maintain 60 FPS
```

---

## 5. XCB Optimization Tricks

### 5.1 Request Batching
```c
// Collect all draw commands
struct draw_cmd {
    int type;
    union { xcb_rectangle_t rect; ... } data;
};

struct draw_cmd cmds[1000];
int cmd_count = 0;

// Execute all at once
xcb_flush(conn);  // Single round-trip!
```

### 5.2 XShm Zero-Copy
```c
// CPU renders directly to shared memory
// No copy to X server
uint8_t *shm_data = attach_shared_memory();
render_directly_to(shm_data);
xcb_shm_put_image(conn, ...);  // Upload
```

### 5.3 Pixmap Caching
```c
// Cache rendered content
// Reuse if page hasn't changed
hash = compute_content_hash(dom);
cached = pixmap_cache_lookup(hash);
if (cached) {
    xcb_copy_area(conn, cached, ...);
} else {
    render_to_pixmap(...);
    pixmap_cache_store(hash, pixmap);
}
```

---

## 6. Profiling Workflow

### 6.1 Startup Time
```bash
# Profile startup
time ./silksurf https://example.com

# Flamegraph
perf record -F 99 ./silksurf https://example.com
perf script | flamegraph.pl > graph.svg

# Find hot functions
perf report  # Sort by CPU%
```

### 6.2 Memory Usage
```bash
# Track memory over time
valgrind --tool=massif ./silksurf
massif-visualizer massif.out

# Find memory leaks
valgrind --leak-check=full ./silksurf
```

### 6.3 Rendering Performance
```bash
# Custom benchmark
./silksurf --benchmark pages.txt --fps-target 60
```

---

## 7. Incremental Optimization Plan

### Phase 1: Baseline
- [ ] Get it working
- [ ] Measure baseline: 50 MB, 500ms startup, 30 FPS

### Phase 2: Low-Hanging Fruit
- [ ] Arena allocators (-15 MB)
- [ ] Object pooling (-5 MB)
- [ ] Request batching (+100% FPS)
- [ ] Damage tracking (+100% FPS)

### Phase 3: Deep Optimization
- [ ] SIMD pixel ops (+50% render speed)
- [ ] Cache optimization (-20% memory)
- [ ] Inline critical paths (+20% speed)

### Phase 4: Advanced
- [ ] GPU acceleration (+200% video)
- [ ] JIT for JavaScript (+500% speed)
- [ ] Parallel rendering (+2x speed on 4-core)

---

## 8. Comparison Targets

| Metric | Firefox | Chrome | NetSurf | SilkSurf Target |
|--------|---------|--------|---------|-----------------|
| Binary | 100 MB | 200 MB | 20 MB | 5 MB |
| Memory | 400 MB | 500 MB | 55 MB | 10 MB |
| Startup | 2-3s | 2-4s | 1-2s | <500ms |
| Page load | 500ms-2s | 400ms-2s | 500ms-3s | 300ms-1s |
| FPS (scroll) | 60 | 60 | 30-40 | 60+ |
| CPU (idle) | 10-15% | 15-20% | 5% | <3% |

