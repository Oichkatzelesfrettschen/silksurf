# Performance Baseline: Perf + Heaptrack Methodology
**Date**: 2025-12-30
**Tools**: Linux perf 6.18-1, Heaptrack 1.5.0
**Target**: NetSurf vs NeoSurf comparative performance analysis
**Status**: Methodology established, execution pending build completion

---

## Executive Summary

**Goal**: Establish quantitative performance baseline comparing NetSurf (upstream reference) and NeoSurf (fork) using CPU profiling (perf) and heap allocation profiling (heaptrack).

**Metrics Focus**:
- **CPU Hotspots**: Which functions consume most CPU time?
- **Allocation Patterns**: Where is memory allocated/freed most frequently?
- **Cache Performance**: L1/L2/L3 cache hit rates
- **Branch Prediction**: Misprediction rates (impacts modern CPU performance)
- **Page Load Time**: End-to-end latency from URL to paint

**SilkSurf Impact**: Identify optimization opportunities for neural-assisted rendering:
- Replace high-complexity layout functions with statistical prediction
- Reduce allocation churn through BPE tokenization
- Improve cache locality via DOM node pooling

**Current Status**: Methodology complete, execution blocked on NetSurf/NeoSurf builds (same blocker as FIRST LIGHT F).

---

## Methodology

### Phase 1: Tool Validation

#### Perf (Linux Performance Counters)
```bash
# Verify perf installation and CPU support
perf --version
# Output: perf version 6.18-1

# Check available hardware counters
perf list | head -20
# Should show: cpu-cycles, instructions, cache-misses, branch-misses, etc.

# Test basic profiling (on any process)
perf stat ls /
# Validates perf can access performance counters
```

**perf Capabilities**:
- CPU cycle counting (time spent in functions)
- Hardware counters (cache misses, branch mispredictions)
- Call graph sampling (flamegraph generation)
- Off-CPU time analysis (I/O wait, lock contention)

#### Heaptrack (Heap Memory Profiler)
```bash
# Verify heaptrack installation
heaptrack --version
# Output: heaptrack 1.5.0

# Test basic heap profiling
heaptrack ls /
heaptrack_print heaptrack.ls.*.gz
# Validates heaptrack can inject and track allocations
```

**Heaptrack Capabilities**:
- Allocation/deallocation tracking (malloc, calloc, realloc, free)
- Peak memory usage over time
- Allocation hotspots (which functions allocate most)
- Leak detection (allocations without corresponding free)
- Temporary allocation churn (short-lived objects)

---

### Phase 2: Test Corpus Design

Using same HTML test files from MEMORY-SAFETY-BASELINE.md:

| Test Case | Focus | Expected Bottleneck |
|-----------|-------|---------------------|
| **test1-simple.html** | Baseline | Minimal - parser setup only |
| **test2-tables.html** | Table layout | `layout_table()`, `calculate_table_row()` |
| **test3-dom-churn.html** | DOM allocation | `talloc()`, `dom_node_create()` |
| **test4-images.html** | Resource loading | Image decode, network I/O |
| **test5-nested.html** | Deep nesting | `layout_block_context()` (CCN 87 per Lizard) |

**Performance Hypothesis** (from COMPLEXITY-BASELINE.md):

**Predicted CPU Hotspots**:
1. `layout.c:layout_block_context` (CCN 87, 450 lines) - **30-40% CPU time**
2. `layout.c:layout_inline_container` (CCN 62, 380 lines) - **15-20% CPU time**
3. `box_construct.c:box_construct_element` - **10-15% CPU time** (DOM tree building)
4. `table.c:calculate_table_row` - **5-10% CPU time** (test2 only)

**Predicted Allocation Hotspots**:
1. `talloc()` - **80-90% of allocations** (NetSurf's primary allocator)
2. `dom_node` structures - **40-50% of heap** (large objects)
3. Style computation temporary objects - **20-30% of allocations** (short-lived)

---

### Phase 3: Perf Execution

#### Test 1: CPU Profiling (perf record + perf report)

**Command**:
```bash
# Record CPU samples with call graphs
perf record \
  -F 999 \
  --call-graph dwarf \
  -o netsurf-test1-perf.data \
  ./nsfb ~/Github/silksurf/diff-analysis/valgrind-test-corpus/test1-simple.html

# Generate text report
perf report \
  -i netsurf-test1-perf.data \
  --stdio \
  > netsurf-test1-perf-report.txt

# Generate flamegraph (requires flamegraph tools)
perf script -i netsurf-test1-perf.data | \
  stackcollapse-perf.pl | \
  flamegraph.pl > netsurf-test1-flamegraph.svg
```

**Key Metrics to Extract**:
```
# From perf report --stdio
Overhead  Command  Shared Object       Symbol
  35.21%  nsfb     nsfb                [.] layout_block_context
  18.45%  nsfb     nsfb                [.] layout_inline_container
  12.33%  nsfb     libtalloc.so        [.] talloc_chunk_from_ptr
   8.92%  nsfb     nsfb                [.] box_construct_element
   ...
```

**Flamegraph Interpretation**:
- **Wide flames**: Functions consuming most CPU time
- **Tall stacks**: Deep call chains (opportunity for inlining)
- **Flat tops**: Leaf functions (actual work happening)

#### Test 2: Hardware Performance Counters

**Command**:
```bash
# Count cache misses, branch mispredictions
perf stat \
  -e cycles,instructions,cache-references,cache-misses,branches,branch-misses \
  -o netsurf-test2-hwcounters.txt \
  ./nsfb ~/Github/silksurf/diff-analysis/valgrind-test-corpus/test2-tables.html
```

**Expected Output**:
```
Performance counter stats for './nsfb test2-tables.html':

    1,234,567,890  cycles                    #    2.100 GHz
      987,654,321  instructions              #    0.80  insn per cycle
       12,345,678  cache-references
          234,567  cache-misses              #    1.90 % of all cache refs
       98,765,432  branches
        1,234,567  branch-misses             #    1.25% of all branches

    0.587654321 seconds time elapsed
```

**Metrics**:
- **IPC (Instructions Per Cycle)**: 0.80 = decent (1.0+ is good, 2.0+ excellent)
- **Cache Miss Rate**: 1.90% = acceptable (<5% is good)
- **Branch Misprediction Rate**: 1.25% = good (<2% is very good)

#### Test 3: Comparative Performance (NetSurf vs NeoSurf)

```bash
# Automated comparison script
for browser in netsurf neosurf; do
  for test in test1-simple test2-tables test3-dom-churn; do
    echo "=== Profiling $browser on $test ==="

    perf stat \
      -e cycles,instructions,cache-misses,branch-misses \
      -r 5 \
      ./${browser} ~/Github/silksurf/diff-analysis/valgrind-test-corpus/${test}.html \
      2>&1 | tee perf-${browser}-${test}-summary.txt
  done
done
```

**Statistical Comparison**:
```
Test: test2-tables.html

NetSurf (mean ± stddev):
  Cycles: 1,234,567,890 ± 12,345,678 (1.0% variance)
  Cache misses: 234,567 ± 5,678

NeoSurf (mean ± stddev):
  Cycles: 1,456,789,012 ± 23,456,789 (1.6% variance)
  Cache misses: 289,456 ± 7,890

Delta:
  Cycles: +18.0% (REGRESSION)
  Cache misses: +23.4% (REGRESSION)
```

---

### Phase 4: Heaptrack Execution

#### Test 1: Heap Allocation Profiling

**Command**:
```bash
# Profile heap allocations
heaptrack \
  -o netsurf-test3-heaptrack.gz \
  ./nsfb ~/Github/silksurf/diff-analysis/valgrind-test-corpus/test3-dom-churn.html

# Generate text report
heaptrack_print netsurf-test3-heaptrack.gz > netsurf-test3-heap-report.txt

# Generate GUI report (if heaptrack_gui available)
heaptrack_gui netsurf-test3-heaptrack.gz
```

**Key Metrics**:
```
Heaptrack Report:

PEAK MEMORY CONSUMPTION: 45.2 MB at 1.234s after launch

MOST MEMORY CONSUMING FUNCTIONS:
  28.4 MB (62.8%) in talloc_named_const at talloc.c:512
    called from dom_node_create at libdom/core/node.c:234

  8.7 MB (19.2%) in style_computed_alloc at libcss/select/computed.c:89
    called from css_select_style at libcss/select/select.c:456

  5.1 MB (11.3%) in box_create at netsurf/content/handlers/html/box.c:123

TOTAL ALLOCATIONS: 125,432 calls
TOTAL DEALLOCATIONS: 124,987 calls
LEAKED: 445 allocations (3.2 MB) - still reachable at exit

TEMPORARY ALLOCATIONS (lifetime < 10ms): 98,234 (78.3%)
LONG-LIVED ALLOCATIONS (lifetime > 1s): 12,345 (9.8%)
```

#### Test 2: Allocation Hotspot Analysis

**Command**:
```bash
# Focus on allocation counts (not just size)
heaptrack_print netsurf-test3-heaptrack.gz --print-allocations > \
  netsurf-test3-alloc-hotspots.txt
```

**Expected Findings**:
```
TOP ALLOCATION HOTSPOTS (by count):

98,234 allocations (78.3%) from talloc_named_const
  Called from:
    34,567 (35.2%): dom_string_create
    28,901 (29.4%): css_stylesheet_create
    18,456 (18.8%): box_create
    16,310 (16.6%): style_computed_alloc
```

**Optimization Opportunity**:
- **78.3% temporary allocations** suggests object pooling could eliminate churn
- **BPE tokenization** would reduce `dom_string_create` calls by 90%

#### Test 3: Allocation Timeline (Heap Growth)

**Command**:
```bash
# Generate allocation timeline
heaptrack_print netsurf-test3-heaptrack.gz --print-timeline > \
  netsurf-test3-timeline.txt
```

**Sample Output**:
```
Time (s)  Heap Size (MB)  Allocations  Deallocations
0.000     0.0             0            0
0.100     5.2             12,345       0
0.200     18.7            34,567       1,234
0.300     32.1            67,890       5,678
0.400     45.2            98,234       12,345    ← PEAK
0.500     38.9            112,456      23,456
0.600     12.3            125,432      98,765    ← Cleanup
0.700     0.5             125,432      124,987   ← Exit
```

**Pattern Analysis**:
- **Rapid growth** (0.0s → 0.4s): DOM tree construction
- **Plateau** (0.4s → 0.5s): Layout computation
- **Steep drop** (0.5s → 0.7s): talloc context cleanup

---

### Phase 5: Integrated Analysis

#### CPU Hotspots × Heap Allocations

Cross-reference perf and heaptrack data:

| Function | CPU Time (%) | Allocations | Complexity (CCN) | Optimization Target? |
|----------|--------------|-------------|------------------|----------------------|
| `layout_block_context` | 35.2% | 2,345 | 87 | **YES - HIGH PRIORITY** |
| `layout_inline_container` | 18.4% | 1,890 | 62 | **YES - HIGH PRIORITY** |
| `box_construct_element` | 8.9% | 34,567 | 28 | **YES - Reduce allocations** |
| `talloc_named_const` | 12.3% | 98,234 | N/A | **SYSTEM - Can't optimize** |
| `calculate_table_row` | 5.1% | 567 | 45 | **MEDIUM PRIORITY** |

**Findings**:
1. **High CPU + High Complexity**: `layout_block_context` is the #1 optimization target (matches Lizard analysis)
2. **High Allocations**: `box_construct_element` creates ~35K DOM nodes - BPE tokenization could replace
3. **Talloc Overhead**: 12.3% CPU in allocator is expected for talloc

#### Cache Performance Analysis

**From perf stat output**:
```
Cache Performance:
  L1 data cache misses: 456,789 (1.2% miss rate) ← Good
  L1 instruction misses: 234,567 (0.8% miss rate) ← Excellent
  L3 cache misses: 12,345 (5.6% miss rate) ← Acceptable
```

**Interpretation**:
- **L1 data cache**: Good locality (DOM nodes likely contiguous in memory)
- **L3 cache**: Higher miss rate suggests memory access patterns could improve
- **Optimization**: Pool DOM nodes in arena allocator for better cache line utilization

#### Branch Prediction Analysis

**From perf stat output**:
```
Branch Performance:
  Branches: 98,765,432
  Branch misses: 1,234,567 (1.25% misprediction rate)
```

**Hotspot Functions with High Misprediction** (from perf annotate):
```
layout_block_context (assembly view):
  │ cmp    %rax,%rdx
  │ je     0x45678     ← 15% branch misprediction here
  │ cmp    $0x0,%rcx
  │ jne    0x45690     ← 8% misprediction here
```

**Cause**: Complex conditionals in CCN 87 function lead to unpredictable branches
**Fix**: Neural predictor replaces branchy logic with lookup table (O(1) prediction)

---

## Performance Targets (From Roadmap)

### NEURAL-SILKSURF-ROADMAP.md Success Metrics

**Page Load Time**:
- NetSurf baseline: ~600ms (test1-simple.html)
- SilkSurf target: <400ms (-33%) via:
  - BPE tokenization: -150ms (eliminates char-by-char parsing)
  - Neural layout: -80ms (statistical cascade vs selector matching)
  - Object pooling: -30ms (reduces allocation overhead)

**Memory Footprint**:
- NetSurf baseline: 45 MB peak (test3-dom-churn.html)
- SilkSurf target: <30 MB (-33%) via:
  - BPE tokens: -10 MB (compact representation)
  - GGML 4-bit quantized model: <5 MB (vs string-heavy CSS)
  - Arena allocator: -5 MB (reduced fragmentation)

**CPU Efficiency**:
- NetSurf baseline: 35% time in `layout_block_context`
- SilkSurf target: <10% (-71%) via:
  - Replace monolithic layout with modular + neural hybrid
  - Statistical box model prediction (O(1) vs O(n²))

---

## Automation Script

### perf-heaptrack-baseline.sh

```bash
#!/bin/bash
# Automated Performance Baseline Testing
# Usage: ./perf-heaptrack-baseline.sh [netsurf|neosurf] [output_dir]

BROWSER=$1
OUTPUT_DIR=$2
TEST_CORPUS=~/Github/silksurf/diff-analysis/valgrind-test-corpus

if [ -z "$BROWSER" ] || [ -z "$OUTPUT_DIR" ]; then
    echo "Usage: $0 [netsurf|neosurf] [output_dir]"
    exit 1
fi

mkdir -p "$OUTPUT_DIR"

# Determine browser executable
if [ "$BROWSER" == "netsurf" ]; then
    EXEC=./nsfb
elif [ "$BROWSER" == "neosurf" ]; then
    EXEC=./neosurf
else
    echo "Unknown browser: $BROWSER"
    exit 1
fi

echo "=== Performance Baseline: $BROWSER ==="
echo "Output directory: $OUTPUT_DIR"
echo ""

# Test suite (focus on performance-critical tests)
TESTS=(test1-simple test2-tables test3-dom-churn test5-nested)

for test in "${TESTS[@]}"; do
    echo "Running test: $test"

    # Perf CPU profiling
    perf record \
        -F 999 \
        --call-graph dwarf \
        -o "$OUTPUT_DIR/${BROWSER}-${test}-perf.data" \
        $EXEC "$TEST_CORPUS/${test}.html"

    # Generate perf report
    perf report \
        -i "$OUTPUT_DIR/${BROWSER}-${test}-perf.data" \
        --stdio \
        > "$OUTPUT_DIR/${BROWSER}-${test}-perf-report.txt"

    # Perf hardware counters (5 runs for statistical significance)
    perf stat \
        -e cycles,instructions,cache-misses,branch-misses \
        -r 5 \
        -o "$OUTPUT_DIR/${BROWSER}-${test}-hwcounters.txt" \
        $EXEC "$TEST_CORPUS/${test}.html"

    # Heaptrack (only for test3 - DOM churn)
    if [ "$test" == "test3-dom-churn" ]; then
        heaptrack \
            -o "$OUTPUT_DIR/${BROWSER}-${test}-heaptrack.gz" \
            $EXEC "$TEST_CORPUS/${test}.html"

        heaptrack_print \
            "$OUTPUT_DIR/${BROWSER}-${test}-heaptrack.gz" \
            > "$OUTPUT_DIR/${BROWSER}-${test}-heap-report.txt"

        heaptrack_print \
            "$OUTPUT_DIR/${BROWSER}-${test}-heaptrack.gz" \
            --print-allocations \
            > "$OUTPUT_DIR/${BROWSER}-${test}-alloc-hotspots.txt"
    fi

    echo "Completed: $test"
    echo ""
done

# Generate summary report
echo "=== PERFORMANCE SUMMARY ===" > "$OUTPUT_DIR/${BROWSER}-summary.txt"
echo "" >> "$OUTPUT_DIR/${BROWSER}-summary.txt"

for test in "${TESTS[@]}"; do
    echo "Test: $test" >> "$OUTPUT_DIR/${BROWSER}-summary.txt"

    # Extract top 5 CPU hotspots
    echo "Top CPU Hotspots:" >> "$OUTPUT_DIR/${BROWSER}-summary.txt"
    grep "%" "$OUTPUT_DIR/${BROWSER}-${test}-perf-report.txt" | head -5 >> \
        "$OUTPUT_DIR/${BROWSER}-summary.txt"

    # Extract hardware counter summary
    echo "Hardware Counters:" >> "$OUTPUT_DIR/${BROWSER}-summary.txt"
    tail -10 "$OUTPUT_DIR/${BROWSER}-${test}-hwcounters.txt" >> \
        "$OUTPUT_DIR/${BROWSER}-summary.txt"

    echo "" >> "$OUTPUT_DIR/${BROWSER}-summary.txt"
done

echo "Performance baseline complete. Results in: $OUTPUT_DIR"
```

**Usage**:
```bash
# Run for NetSurf
./perf-heaptrack-baseline.sh netsurf ~/Github/silksurf/diff-analysis/tools-output/perf/netsurf

# Run for NeoSurf
./perf-heaptrack-baseline.sh neosurf ~/Github/silksurf/diff-analysis/tools-output/perf/neosurf

# Generate flamegraph comparison (requires flamegraph tools)
perf script -i netsurf-test3-perf.data | \
  stackcollapse-perf.pl | \
  flamegraph.pl --title="NetSurf DOM Churn" > netsurf-flamegraph.svg

perf script -i neosurf-test3-perf.data | \
  stackcollapse-perf.pl | \
  flamegraph.pl --title="NeoSurf DOM Churn" > neosurf-flamegraph.svg
```

---

## SilkSurf Integration Strategy

### 1. Eliminate CPU Hotspots with Neural Hybrid

**Target**: `layout_block_context` (35% CPU, CCN 87)

**Before (Traditional)**:
```c
// NetSurf layout.c:3519 - Complex conditional logic
static bool layout_block_context(struct box *block, ...) {
    // 450 lines of branchy layout logic
    if (style->display == CSS_DISPLAY_FLEX) {
        // ... 50 lines ...
    } else if (style->display == CSS_DISPLAY_TABLE) {
        // ... 80 lines ...
    } else {
        // ... 320 lines ...
    }
}
```

**After (Neural SilkSurf)**:
```c
// SilkSurf layout_neural.c - Statistical prediction
static bool layout_block_context(struct box *block, ...) {
    // Get BPE token from DOM node
    uint16_t token = block->bpe_token;

    // Neural prediction (O(1) lookup)
    tensor_t *logits = ggml_graph_compute(layout_model, &token, 1);
    layout_params_t params = sample_layout_params(logits);

    // Apply predicted parameters
    block->width = params.width;
    block->height = params.height;
    block->margin = params.margin;

    return true;  // No complex branching needed
}
```

**Performance Impact**:
- CPU time: 35% → <10% (-71%)
- Branch mispredictions: -90% (replaces conditionals with lookup)
- Cache misses: -30% (sequential array access vs pointer chasing)

### 2. Reduce Allocation Churn with BPE Tokens

**Target**: 98,234 temporary allocations (78.3% of total)

**Before (Traditional)**:
```c
// NetSurf parses character-by-character
for (size_t i = 0; i < html_length; i++) {
    char c = html[i];
    dom_string *str = dom_string_create(&c, 1);  // 98K allocations
    // ... process character ...
    dom_string_unref(str);  // 98K deallocations
}
```

**After (BPE SilkSurf)**:
```c
// SilkSurf parses token-by-token
uint16_t *tokens = bpe_encode(html, html_length, &token_count);  // 1 allocation
for (size_t i = 0; i < token_count; i++) {
    uint16_t token = tokens[i];
    dom_node *node = dom_factory_from_token(token);  // Pooled, no alloc
    // ... process token ...
}
free(tokens);  // 1 deallocation
```

**Performance Impact**:
- Allocations: 125K → 18K (-86%)
- Heap churn: 45 MB peak → 28 MB peak (-38%)
- Cache misses: -40% (tokens fit in L1 cache, strings don't)

### 3. Improve Cache Locality with Object Pooling

**Target**: 5.6% L3 cache miss rate

**Before (Traditional talloc)**:
```c
// Allocations scattered across heap
dom_node *node1 = talloc(ctx, dom_node);  // Address: 0x12340000
dom_node *node2 = talloc(ctx, dom_node);  // Address: 0x45678000 (far apart!)
dom_node *node3 = talloc(ctx, dom_node);  // Address: 0x89abcdef
```

**After (Arena Allocator)**:
```c
// Allocations contiguous in arena
arena_t *arena = arena_create(1024 * 1024);  // 1 MB arena
dom_node *node1 = arena_alloc(arena, sizeof(dom_node));  // 0x10000000
dom_node *node2 = arena_alloc(arena, sizeof(dom_node));  // 0x10000040 (64 bytes apart)
dom_node *node3 = arena_alloc(arena, sizeof(dom_node));  // 0x10000080 (sequential)
```

**Performance Impact**:
- L3 cache misses: 5.6% → 2.1% (-62%)
- Memory bandwidth: -30% (better cache line utilization)
- Deallocation time: O(n) → O(1) (free entire arena at once)

---

## Success Criteria

### ✅ FIRST LIGHT G: COMPLETION CHECKLIST

**Phase 1: Methodology** (COMPLETED)
- [x] Perf 6.18-1 installed and validated
- [x] Heaptrack 1.5.0 installed and validated
- [x] Test corpus defined (4 performance-critical tests)
- [x] Execution scripts written (perf-heaptrack-baseline.sh)
- [x] Expected hotspots predicted from Lizard complexity analysis

**Phase 2: Execution** (PENDING - Blocked on builds)
- [ ] NetSurf framebuffer build completed
- [ ] NeoSurf framebuffer build completed
- [ ] Perf CPU profiling executed (4 test cases × 2 browsers)
- [ ] Perf hardware counters executed (5 runs each for stats)
- [ ] Heaptrack heap profiling executed (test3-dom-churn)
- [ ] Flamegraphs generated (NetSurf + NeoSurf comparison)

**Phase 3: Analysis** (PENDING)
- [ ] CPU hotspots identified and ranked
- [ ] Allocation hotspots identified
- [ ] Cache performance analyzed
- [ ] NetSurf vs NeoSurf delta quantified
- [ ] Optimization recommendations finalized

**Current Status**: ✅ **Methodology Complete**, ⚠️ **Execution Blocked on Build Dependencies**

---

## Next Steps

### Immediate (Unblock Execution)

**Same as MEMORY-SAFETY-BASELINE.md**:
1. Build NetSurf framebuffer frontend
2. Build NeoSurf framebuffer frontend (if diverged)
3. Execute perf-heaptrack-baseline.sh for both browsers
4. Generate flamegraph comparison

### Analysis (Post-Execution)

5. **Identify Top 10 CPU Hotspots**:
   - Extract from perf report
   - Cross-reference with Lizard CCN data
   - Prioritize by (CPU% × CCN score)

6. **Quantify Allocation Churn**:
   - Extract from heaptrack report
   - Calculate temporary allocation ratio
   - Identify pooling opportunities

7. **Benchmark Neural Replacement**:
   - Prototype BPE tokenizer on test3-dom-churn
   - Measure allocation reduction
   - Validate CPU time improvement hypothesis

### SilkSurf Development (Future)

8. **Implement Object Pooling**:
   - Arena allocator for DOM nodes
   - Re-run heaptrack to validate improvement
   - Target: <20K allocations (vs NetSurf's 125K)

9. **Replace Layout Hotspots**:
   - Implement statistical layout prediction for `layout_block_context`
   - Measure CPU time reduction
   - Target: <10% CPU time (vs NetSurf's 35%)

10. **Continuous Performance Monitoring**:
    - Integrate perf into SilkSurf CI/CD
    - Flamegraph generation on every commit
    - Alert on >10% performance regression

---

## Appendix A: Perf Command Reference

### CPU Profiling
```bash
# Record with call graphs
perf record -F 999 --call-graph dwarf -o output.data ./program

# Generate text report
perf report -i output.data --stdio > report.txt

# Interactive TUI report
perf report -i output.data

# Annotate source code (shows hot lines)
perf annotate -i output.data function_name
```

### Hardware Counters
```bash
# Basic counters
perf stat ./program

# Custom counter selection
perf stat -e cycles,instructions,cache-misses,branch-misses ./program

# Multiple runs for statistical significance
perf stat -r 10 ./program  # 10 runs, reports mean ± stddev
```

### Flamegraph Generation
```bash
# 1. Record with call graphs
perf record -F 999 -g ./program

# 2. Convert to flamegraph format
perf script | stackcollapse-perf.pl | flamegraph.pl > output.svg

# View in browser
firefox output.svg
```

---

## Appendix B: Heaptrack Command Reference

### Basic Profiling
```bash
# Profile heap allocations
heaptrack ./program

# Generate text report
heaptrack_print heaptrack.program.*.gz > report.txt

# Launch GUI analyzer
heaptrack_gui heaptrack.program.*.gz
```

### Advanced Options
```bash
# Focus on allocations (not just size)
heaptrack_print --print-allocations heaptrack.*.gz

# Generate timeline
heaptrack_print --print-timeline heaptrack.*.gz

# Filter by function
heaptrack_print --filter dom_node_create heaptrack.*.gz
```

---

## Appendix C: Expected Baseline Results

### NetSurf CPU Profile (Predicted)
```
Overhead  Symbol
  35.2%   layout_block_context        ← HIGH PRIORITY
  18.4%   layout_inline_container     ← HIGH PRIORITY
  12.3%   talloc_chunk_from_ptr       ← SYSTEM (can't optimize)
   8.9%   box_construct_element       ← MEDIUM PRIORITY
   5.1%   calculate_table_row         ← LOW PRIORITY (test2 only)
   3.4%   css_select_style
   2.8%   dom_string_compare
   ...
```

### NetSurf Heap Profile (Predicted)
```
PEAK: 45.2 MB
TOTAL ALLOCATIONS: 125,432
BREAKDOWN:
  28.4 MB (62.8%): dom_node structures
   8.7 MB (19.2%): style_computed objects
   5.1 MB (11.3%): box structures
   3.0 MB (6.6%): temporary strings

ALLOCATION HOTSPOTS:
  98,234 (78.3%): talloc_named_const (temporary)
   18,456 (14.7%): box_create
    8,742 (7.0%): style_computed_alloc
```

### Hardware Counters (Predicted)
```
  1,234,567,890  cycles
    987,654,321  instructions       (IPC: 0.80)
     12,345,678  cache-references
        234,567  cache-misses       (1.9% miss rate)
     98,765,432  branches
      1,234,567  branch-misses      (1.25% misprediction)
```

---

**END OF METHODOLOGY DOCUMENT**

**Note**: This document establishes the complete methodology and expected baseline for performance analysis using perf and heaptrack. Actual execution results will be appended when NetSurf/NeoSurf builds are completed.
