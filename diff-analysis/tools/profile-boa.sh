#!/usr/bin/env bash
# Performance profiling script for Boa JavaScript Engine
# Purpose: Identify CPU hotspots and allocation patterns for cleanroom optimization

set -euo pipefail

BOA_DIR="/home/eirikr/Github/silksurf/silksurf-extras/boa"
OUTPUT_DIR="/home/eirikr/Github/silksurf/diff-analysis/tools-output/boa-profiling"
TEST262_DIR="/home/eirikr/Github/silksurf/silksurf-extras/boa/test262"
FLAMEGRAPH_DIR="/home/eirikr/Github/silksurf/silksurf-extras/FlameGraph"

# Add FlameGraph scripts to PATH
export PATH="$FLAMEGRAPH_DIR:$PATH"

# Create output directories
mkdir -p "$OUTPUT_DIR"/{perf,heaptrack,valgrind,benchmarks}

echo "=== Boa Performance Profiling Suite ==="
echo "Output: $OUTPUT_DIR"
echo ""

# ============================================================================
# BENCHMARK 1: Fibonacci(35) - Recursion Stress Test
# ============================================================================
echo "[1/5] Fibonacci(35) - Recursion benchmark..."

cat > /tmp/fib35.js <<'EOF'
function fib(n) {
    if (n <= 1) return n;
    return fib(n - 1) + fib(n - 2);
}
console.log(fib(35));
EOF

# Perf profiling
perf record -F 999 -g --call-graph dwarf \
    -o "$OUTPUT_DIR/perf/fib35.perf.data" \
    -- "$BOA_DIR/target/release/boa" /tmp/fib35.js \
    2>&1 | tee "$OUTPUT_DIR/benchmarks/fib35-perf.log"

# Generate flamegraph
perf script -i "$OUTPUT_DIR/perf/fib35.perf.data" \
    | stackcollapse-perf.pl \
    | flamegraph.pl > "$OUTPUT_DIR/perf/fib35-flamegraph.svg"

# Heaptrack profiling
heaptrack -o "$OUTPUT_DIR/heaptrack/fib35.heaptrack" \
    "$BOA_DIR/target/release/boa" /tmp/fib35.js \
    2>&1 | tee "$OUTPUT_DIR/benchmarks/fib35-heaptrack.log"

echo "✓ Fibonacci(35) complete"
echo ""

# ============================================================================
# BENCHMARK 2: Prime Sieve - Array/Loop Performance
# ============================================================================
echo "[2/5] Prime Sieve - Array performance..."

cat > /tmp/primes.js <<'EOF'
function sieve(n) {
    const primes = new Array(n + 1).fill(true);
    primes[0] = primes[1] = false;

    for (let i = 2; i * i <= n; i++) {
        if (primes[i]) {
            for (let j = i * i; j <= n; j += i) {
                primes[j] = false;
            }
        }
    }

    let count = 0;
    for (let i = 0; i <= n; i++) {
        if (primes[i]) count++;
    }
    console.log(count);
}
sieve(100000);
EOF

perf record -F 999 -g --call-graph dwarf \
    -o "$OUTPUT_DIR/perf/primes.perf.data" \
    -- "$BOA_DIR/target/release/boa" /tmp/primes.js \
    2>&1 | tee "$OUTPUT_DIR/benchmarks/primes-perf.log"

perf script -i "$OUTPUT_DIR/perf/primes.perf.data" \
    | stackcollapse-perf.pl \
    | flamegraph.pl > "$OUTPUT_DIR/perf/primes-flamegraph.svg"

heaptrack -o "$OUTPUT_DIR/heaptrack/primes.heaptrack" \
    "$BOA_DIR/target/release/boa" /tmp/primes.js \
    2>&1 | tee "$OUTPUT_DIR/benchmarks/primes-heaptrack.log"

echo "✓ Prime Sieve complete"
echo ""

# ============================================================================
# BENCHMARK 3: String Operations - Allocation Pressure
# ============================================================================
echo "[3/5] String Operations - Allocation benchmark..."

cat > /tmp/strings.js <<'EOF'
let result = "";
for (let i = 0; i < 10000; i++) {
    result += "benchmark_string_" + i + "_";
}
console.log(result.length);
EOF

heaptrack -o "$OUTPUT_DIR/heaptrack/strings.heaptrack" \
    "$BOA_DIR/target/release/boa" /tmp/strings.js \
    2>&1 | tee "$OUTPUT_DIR/benchmarks/strings-heaptrack.log"

echo "✓ String Operations complete"
echo ""

# ============================================================================
# BENCHMARK 4: Object Property Access - Cache Performance
# ============================================================================
echo "[4/5] Object Property Access - Cache benchmark..."

cat > /tmp/objects.js <<'EOF'
const obj = {};
for (let i = 0; i < 1000; i++) {
    obj["prop_" + i] = i;
}

let sum = 0;
for (let j = 0; j < 10000; j++) {
    for (let i = 0; i < 1000; i++) {
        sum += obj["prop_" + i];
    }
}
console.log(sum);
EOF

perf record -F 999 -g --call-graph dwarf \
    -o "$OUTPUT_DIR/perf/objects.perf.data" \
    -- "$BOA_DIR/target/release/boa" /tmp/objects.js \
    2>&1 | tee "$OUTPUT_DIR/benchmarks/objects-perf.log"

perf script -i "$OUTPUT_DIR/perf/objects.perf.data" \
    | stackcollapse-perf.pl \
    | flamegraph.pl > "$OUTPUT_DIR/perf/objects-flamegraph.svg"

# Valgrind cachegrind (cache miss analysis)
valgrind --tool=cachegrind \
    --cachegrind-out-file="$OUTPUT_DIR/valgrind/objects.cachegrind" \
    "$BOA_DIR/target/release/boa" /tmp/objects.js \
    2>&1 | tee "$OUTPUT_DIR/benchmarks/objects-valgrind.log"

echo "✓ Object Property Access complete"
echo ""

# ============================================================================
# BENCHMARK 5: Test262 Subset - Realistic Workload
# ============================================================================
echo "[5/5] Test262 Subset (100 tests) - Realistic workload..."

# Sample 100 random tests for profiling
fd -e js . "$TEST262_DIR/test/language" | shuf | head -100 > /tmp/test262_sample.txt

# Create test runner script
cat > /tmp/run_test262_sample.sh <<'RUNNER'
#!/usr/bin/env bash
while IFS= read -r test; do
    timeout 5 "$1" "$test" >/dev/null 2>&1 || true
done < /tmp/test262_sample.txt
RUNNER
chmod +x /tmp/run_test262_sample.sh

perf record -F 999 -g --call-graph dwarf \
    -o "$OUTPUT_DIR/perf/test262.perf.data" \
    -- /tmp/run_test262_sample.sh "$BOA_DIR/target/release/boa" \
    2>&1 | tee "$OUTPUT_DIR/benchmarks/test262-perf.log"

perf script -i "$OUTPUT_DIR/perf/test262.perf.data" \
    | stackcollapse-perf.pl \
    | flamegraph.pl > "$OUTPUT_DIR/perf/test262-flamegraph.svg"

heaptrack -o "$OUTPUT_DIR/heaptrack/test262.heaptrack" \
    /tmp/run_test262_sample.sh "$BOA_DIR/target/release/boa" \
    2>&1 | tee "$OUTPUT_DIR/benchmarks/test262-heaptrack.log"

echo "✓ Test262 Subset complete"
echo ""

# ============================================================================
# GENERATE REPORTS
# ============================================================================
echo "=== Generating Analysis Reports ==="

# Perf report summaries
for perf_file in "$OUTPUT_DIR/perf"/*.perf.data; do
    base=$(basename "$perf_file" .perf.data)
    perf report -i "$perf_file" --stdio > "$OUTPUT_DIR/perf/${base}-report.txt"
done

# Heaptrack analysis
for heap_file in "$OUTPUT_DIR/heaptrack"/*.heaptrack; do
    base=$(basename "$heap_file" .heaptrack)
    heaptrack --analyze "$heap_file" > "$OUTPUT_DIR/heaptrack/${base}-analysis.txt" 2>&1
done

# Cachegrind annotation
if [ -f "$OUTPUT_DIR/valgrind/objects.cachegrind" ]; then
    cg_annotate "$OUTPUT_DIR/valgrind/objects.cachegrind" \
        > "$OUTPUT_DIR/valgrind/objects-annotated.txt"
fi

echo ""
echo "=== Profiling Complete ==="
echo "Results:"
echo "  Flamegraphs: $OUTPUT_DIR/perf/*-flamegraph.svg"
echo "  Perf Reports: $OUTPUT_DIR/perf/*-report.txt"
echo "  Heaptrack:   $OUTPUT_DIR/heaptrack/*-analysis.txt"
echo "  Cachegrind:  $OUTPUT_DIR/valgrind/objects-annotated.txt"
echo ""
echo "Next: Analyze results and document optimization opportunities"
