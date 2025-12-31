#!/bin/bash
# SilkSurfJS Optimized Build Script
# Uses PGO (Profile-Guided Optimization) + BOLT for maximum performance
#
# Prerequisites:
# - rustup default nightly
# - llvm-bolt (pacman -S llvm-bolt)
# - perf (linux-tools)

set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PGO_DIR="/tmp/silksurf-pgo-data"
BOLT_DIR="/tmp/silksurf-bolt-data"

echo "=== SilkSurfJS Optimized Build ==="
echo "Project: $PROJECT_DIR"
echo ""

# Clean previous PGO data
rm -rf "$PGO_DIR" "$BOLT_DIR"
mkdir -p "$PGO_DIR" "$BOLT_DIR"

cd "$PROJECT_DIR"

# Step 1: Build with PGO instrumentation
echo "Step 1: Building with PGO instrumentation..."
RUSTFLAGS="-Cprofile-generate=$PGO_DIR -Ctarget-cpu=native" \
    cargo build --release --example parser_profile

# Step 2: Run workload to generate PGO data
echo "Step 2: Running workload for PGO profile..."
./target/release/examples/parser_profile

# Step 3: Merge PGO profile data
echo "Step 3: Merging PGO profile data..."
llvm-profdata merge -o "$PGO_DIR/merged.profdata" "$PGO_DIR"/*.profraw

# Step 4: Rebuild with PGO optimization
echo "Step 4: Rebuilding with PGO optimization..."
RUSTFLAGS="-Cprofile-use=$PGO_DIR/merged.profdata -Ctarget-cpu=native" \
    cargo build --release --example parser_profile

# Step 5: Try BOLT optimization (optional - may fail on some systems)
echo "Step 5: Attempting BOLT optimization..."
if command -v llvm-bolt &> /dev/null; then
    # Collect perf data for BOLT
    perf record -e cycles:u -o "$BOLT_DIR/perf.data" \
        -- ./target/release/examples/parser_profile 2>/dev/null || true

    # Convert to BOLT format (no-LBR mode for systems without LBR support)
    if perf2bolt -nl -p "$BOLT_DIR/perf.data" \
        -o "$BOLT_DIR/perf.fdata" \
        ./target/release/examples/parser_profile 2>/dev/null; then

        # Apply BOLT optimizations
        llvm-bolt ./target/release/examples/parser_profile \
            -o ./target/release/examples/parser_profile.bolt \
            -data="$BOLT_DIR/perf.fdata" \
            -reorder-blocks=ext-tsp \
            -dyno-stats 2>/dev/null && \
        mv ./target/release/examples/parser_profile.bolt \
           ./target/release/examples/parser_profile
        echo "BOLT optimization applied successfully!"
    else
        echo "BOLT optimization skipped (insufficient profile data)"
    fi
else
    echo "BOLT not available, skipping..."
fi

# Final benchmark
echo ""
echo "=== Final Benchmark ==="
./target/release/examples/parser_profile

echo ""
echo "Build complete!"
