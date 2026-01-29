# SilkSurf Build Guide

**Target**: New developer builds SilkSurf in <10 minutes
**Updated**: 2026-01-29

---

## Quick Start

```bash
# Clone repository
git clone https://github.com/your-org/silksurf.git
cd silksurf

# Build (Debug)
cmake -B build
cmake --build build

# Run tests
ctest --test-dir build

# Build (Release with optimizations)
cmake -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build
```

**Expected**: 3/4 tests passing, builds in ~2 minutes

---

## System Requirements

### Supported Platforms
- **Linux**: Ubuntu 22.04+, Arch Linux, Debian 12+
- **Architecture**: x86_64 (primary), ARM64 (experimental)
- **Compiler**: GCC 11+ or Clang 14+
- **CMake**: 3.10+

### Required Dependencies

**Core Libraries** (must install):
```bash
# Arch Linux
sudo pacman -S libxcb xcb-util libhubbub libcss libdom libparserutils pixman

# Ubuntu/Debian
sudo apt install libxcb1-dev libxcb-damage0-dev libxcb-composite0-dev \
                 libxcb-util-dev libxcb-shm0-dev libhubbub-dev libcss-dev \
                 libdom-dev libparserutils-dev libpixman-1-dev

# Fedora
sudo dnf install libxcb-devel xcb-util-devel netsurf-buildsystem \
                 libhubbub-devel libcss-devel libdom-devel libparserutils-devel \
                 pixman-devel
```

**Rust Toolchain** (for JavaScript engine):
```bash
# Install rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install nightly (required for some dependencies)
rustup install nightly
rustup default nightly
```

### Optional Dependencies

**Development Tools**:
- `valgrind` - Memory leak detection
- `gdb` - Debugging
- `afl++` - Fuzz testing
- `clang-tidy` - Static analysis
- `cppcheck` - Additional static analysis

```bash
# Arch Linux
sudo pacman -S valgrind gdb afl++ clang-tidy cppcheck

# Ubuntu/Debian
sudo apt install valgrind gdb afl++ clang-tidy cppcheck
```

---

## Build Modes

### Debug Build (Default)
```bash
cmake -B build
cmake --build build
```
- **Features**: Debug symbols, assertions enabled
- **Optimizations**: -O0
- **Use case**: Development, debugging
- **Size**: ~5MB binary

### Release Build
```bash
cmake -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build
```
- **Features**: No debug symbols, assertions disabled
- **Optimizations**: -O3 -march=native
- **Use case**: Performance testing, distribution
- **Size**: ~2MB binary (stripped)

### Sanitizer Builds

SilkSurf supports AddressSanitizer (ASAN) and UndefinedBehaviorSanitizer (UBSAN) for detecting memory errors and undefined behavior. Use the `SANITIZER` CMake option to enable them.

**AddressSanitizer (ASAN)** - Detect memory errors:
```bash
cmake -B build -DSANITIZER=address -DCMAKE_BUILD_TYPE=Debug
cmake --build build
./build/test_dom_parsing
```

**UndefinedBehaviorSanitizer (UBSAN)** - Detect undefined behavior:
```bash
cmake -B build -DSANITIZER=undefined -DCMAKE_BUILD_TYPE=Debug
cmake --build build
```

**Combined Sanitizers** - Both ASAN and UBSAN:
```bash
cmake -B build -DSANITIZER=address,undefined -DCMAKE_BUILD_TYPE=Debug
cmake --build build
```

**Notes**:
- Sanitizer builds automatically reduce optimization to -O1 for better stack traces
- Memory leaks from libdom (external library) are expected and documented
- Use sanitizers during development to catch issues early

### Profile-Guided Optimization (PGO)
```bash
# Step 1: Build with instrumentation
cmake -B build-pgo -DCMAKE_BUILD_TYPE=Release \
                   -DCMAKE_C_FLAGS="-O3 -march=native -fprofile-generate"
cmake --build build-pgo

# Step 2: Run benchmarks to generate profile data
./build-pgo/silksurf <typical-workload>

# Step 3: Build with profile data
cmake -B build -DCMAKE_BUILD_TYPE=Release \
               -DCMAKE_C_FLAGS="-O3 -march=native -fprofile-use"
cmake --build build
```
- **Expected**: 10-30% performance improvement on hot paths

---

## Testing

### Run All Tests
```bash
ctest --test-dir build
```

**Expected output**:
```
Test #1: parser_basic .......... Passed
Test #2: dom_parsing ........... Passed
Test #3: css_engine ............ Passed
Test #4: css_cascade ........... Failed

75% tests passed, 1 test failed
```

### Run Individual Tests
```bash
# HTML parser test
./build/test_parser

# DOM parsing test
./build/test_dom_parsing

# CSS engine test
./build/test_css_engine

# CSS cascade test (currently failing)
./build/test_css_cascade
```

### Verbose Test Output
```bash
ctest --test-dir build --output-on-failure
```

### Memory Leak Detection
```bash
valgrind --leak-check=full --show-leak-kinds=all \
         --track-origins=yes \
         ./build/test_dom_parsing
```

**Expected**: 0 errors, some leaks from libdom (external library)

### Fuzz Testing

SilkSurf includes AFL++ fuzzing harnesses for HTML and CSS parsers.

**Prerequisites**:
```bash
# Install AFL++
sudo pacman -S afl++        # Arch Linux
sudo apt install afl++      # Ubuntu/Debian
```

**Build with AFL++ instrumentation**:
```bash
# Configure with AFL++ compiler
cmake -B build-fuzz -DENABLE_FUZZING=ON
cmake --build build-fuzz

# Verify instrumentation
afl-showmap -o /dev/null -- ./build-fuzz/silksurf_fuzz < fuzz_corpus/html/input/basic.html
```

**Run fuzzing campaigns**:
```bash
# 5-minute smoke test
cd build-fuzz && make fuzz_quick

# Full HTML fuzzing (run until interrupted with Ctrl+C)
cd build-fuzz && make fuzz_html

# Full CSS fuzzing
cd build-fuzz && make fuzz_css

# Manual fuzzing with custom options
afl-fuzz -i ../fuzz_corpus/html/input \
         -o ../fuzz_corpus/html/output \
         -m none \
         -- ./silksurf_fuzz
```

**Check results**:
```bash
# View statistics
afl-whatsup fuzz_corpus/html/output

# Examine crashes
ls fuzz_corpus/html/output/default/crashes/
cat fuzz_corpus/html/output/default/crashes/id:000000*

# Reproduce crash
./build-fuzz/silksurf_fuzz < fuzz_corpus/html/output/default/crashes/id:000000*
```

**Expected**: 24 hours with zero crashes for production readiness

---

## Troubleshooting

### Issue: "libhubbub not found"

**Symptom**:
```
CMake Error: Could not find libhubbub
```

**Solution**:
```bash
# Arch Linux - Install NetSurf libraries
sudo pacman -S libhubbub libcss libdom libparserutils

# Ubuntu - Install from PPA
sudo add-apt-repository ppa:netsurf/ppa
sudo apt update
sudo apt install libhubbub-dev libcss-dev libdom-dev
```

### Issue: "Rust compilation fails with unsafe attribute error"

**Symptom**:
```
error: unsafe attribute used without unsafe
```

**Solution**: This is a known issue with Rust nightly. The JavaScript engine integration is work-in-progress (Task #33). For now, build only C targets:
```bash
cmake --build build --target test_dom_parsing test_css_engine test_css_cascade
```

### Issue: "X11/xcb.h not found"

**Symptom**:
```
fatal error: xcb/xcb.h: No such file or directory
```

**Solution**:
```bash
# Install XCB development headers
sudo pacman -S libxcb xcb-util      # Arch
sudo apt install libxcb1-dev        # Ubuntu
```

### Issue: Build is slow (>5 minutes)

**Symptom**: CMake takes forever to configure or compile

**Solutions**:
```bash
# Use parallel builds
cmake --build build -j$(nproc)

# Use Ninja instead of Make
cmake -B build -G Ninja
ninja -C build
```

### Issue: Tests segfault

**Symptom**: Tests crash with SIGSEGV

**Solution**:
1. Run with gdb to get backtrace:
   ```bash
   gdb ./build/test_css_cascade
   (gdb) run
   (gdb) bt
   ```

2. Check if running with sanitizers reveals the issue:
   ```bash
   cmake -B build -DCMAKE_C_FLAGS="-fsanitize=address"
   cmake --build build
   ./build/test_css_cascade
   ```

---

## Feature Flags

Currently all features are enabled by default. Future versions will support:

```bash
# Disable JavaScript engine (build C core only)
cmake -B build -DENABLE_JAVASCRIPT=OFF

# Enable neural BPE optimization
cmake -B build -DENABLE_NEURAL_BPE=ON

# Enable XShm acceleration
cmake -B build -DENABLE_XSHM=ON
```

---

## Development Workflow

### 1. Make Code Changes
```bash
# Edit src/document/dom_node.c
vim src/document/dom_node.c
```

### 2. Incremental Build
```bash
# Only rebuilds changed files
cmake --build build
```

### 3. Run Tests
```bash
ctest --test-dir build --output-on-failure
```

### 4. Check for Warnings
```bash
# Build with -Werror enabled (treat warnings as errors)
# This is already enabled by default in CMakeLists.txt
cmake --build build 2>&1 | grep -i warning
```
**Expected**: No warnings

### 5. Memory Check (Optional)
```bash
valgrind ./build/test_dom_parsing
```

### 6. Commit
```bash
git add src/document/dom_node.c
git commit -m "Fix DOM node attribute handling

- Implement silk_dom_node_get_attribute
- Add NULL checks and bounds validation
- Add tests for edge cases"
```

---

## Performance Benchmarking

### Startup Time
```bash
time ./build/silksurf --benchmark startup
```
**Target**: <500ms

### Memory Usage
```bash
/usr/bin/time -v ./build/silksurf test.html
```
**Target**: <10MB per tab

### Rendering FPS
```bash
./build/silksurf --benchmark render
```
**Target**: 60 FPS layout, 100+ FPS damage-tracked rendering

---

## CI/CD Integration

### GitHub Actions (planned)
```yaml
# .github/workflows/ci.yml
name: CI

on: [push, pull_request]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - name: Install dependencies
        run: sudo apt install libxcb1-dev libhubbub-dev libcss-dev libdom-dev
      - name: Build
        run: cmake -B build && cmake --build build
      - name: Test
        run: ctest --test-dir build --output-on-failure
```

---

## Clean Build

```bash
# Remove build directory
rm -rf build

# Clean git working directory
git clean -fdx

# Fresh build
cmake -B build
cmake --build build
```

---

## References

- **CMakeLists.txt** - Build configuration
- **CLAUDE.md** - Engineering standards (NO SHORTCUTS policy)
- **README.md** - Project overview
- **docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md** - Implementation status
