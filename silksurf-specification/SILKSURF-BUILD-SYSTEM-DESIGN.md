================================================================================
SILKSURF MODULAR CMAKE BUILD SYSTEM DESIGN
================================================================================
Version: 1.0
Date: 2025-12-31
Audience: Build engineers (Phase 2-3)
Status: Architecture Freeze

EXECUTIVE SUMMARY
================================================================================

The SilkSurf build system uses CMake 3.16+ to support:
- **Multiple interfaces**: CLI, TUI, Curses, XCB (selectable at configure time)
- **Rust/C FFI integration**: cargo + cmake coordination
- **Modular linking**: Each interface is a separate target; shared core
- **Feature flags**: SilkSurf_ENABLE_XCB, etc. for fine-grained control
- **Cross-platform**: Linux primary (x86_64, ARM, RISC-V); macOS/Windows future

Key design goals:
- Zero linking bloat (don't link GTK unless XCB is enabled)
- Hermetic builds (reproducible, pinned dependencies)
- Parallel compilation (ninja, multiple cores)
- Fast incremental builds (separate targets, ccache integration)

================================================================================
PART 1: CMAKE PROJECT STRUCTURE
================================================================================

### 1.1 Root CMakeLists.txt

```cmake
# silksurf/CMakeLists.txt

cmake_minimum_required(VERSION 3.16)
project(
    SilkSurf
    VERSION 0.1.0
    DESCRIPTION "Cleanroom HTML5/CSS/JS browser engine"
    LANGUAGES C CXX Rust
)

# ============================================================================
# Build Options
# ============================================================================

option(SilkSurf_ENABLE_CLI "Build CLI interface" ON)
option(SilkSurf_ENABLE_TUI "Build TUI interface" OFF)
option(SilkSurf_ENABLE_CURSES "Build Curses interface" OFF)
option(SilkSurf_ENABLE_XCB "Build XCB GUI interface" ON)
option(SilkSurf_ENABLE_TESTS "Build tests and benchmarks" ON)
option(SilkSurf_USE_CLANG "Use Clang compiler" OFF)
option(SilkSurf_USE_CCACHE "Use ccache for compilation" ON)
option(SilkSurf_LTO "Enable Link Time Optimization" ON)
option(SilkSurf_SANITIZERS "Enable ASAN/UBSAN (debug)" OFF)

# ============================================================================
# Compiler Configuration
# ============================================================================

if(SilkSurf_USE_CLANG)
    set(CMAKE_C_COMPILER clang)
    set(CMAKE_CXX_COMPILER clang++)
else()
    # Default to GCC
    set(CMAKE_C_COMPILER gcc)
    set(CMAKE_CXX_COMPILER g++)
endif()

# Use ccache for faster rebuilds
if(SilkSurf_USE_CCACHE)
    find_program(CCACHE_PROGRAM ccache)
    if(CCACHE_PROGRAM)
        set(CMAKE_C_COMPILER_LAUNCHER "${CCACHE_PROGRAM}")
        set(CMAKE_CXX_COMPILER_LAUNCHER "${CCACHE_PROGRAM}")
    endif()
endif()

# Compiler flags (strict warnings)
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -Wall -Wextra -Werror")
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -std=c11 -fPIC")
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -Wall -Wextra -Werror")

# Optimization flags
if(CMAKE_BUILD_TYPE STREQUAL "Release")
    set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -O3 -march=native")
    if(SilkSurf_LTO)
        set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -flto=auto")
    endif()
elseif(CMAKE_BUILD_TYPE STREQUAL "Debug")
    set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -g -O0 -DDEBUG")
    if(SilkSurf_SANITIZERS)
        set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -fsanitize=address,undefined")
    endif()
endif()

# ============================================================================
# Dependencies
# ============================================================================

# C core
add_subdirectory(silksurf-core)

# Rust engine (via cargo)
add_subdirectory(silksurf-js)

# Interfaces
if(SilkSurf_ENABLE_CLI)
    add_subdirectory(silksurf-cli)
endif()

if(SilkSurf_ENABLE_TUI)
    add_subdirectory(silksurf-tui)
endif()

if(SilkSurf_ENABLE_CURSES)
    add_subdirectory(silksurf-curses)
endif()

if(SilkSurf_ENABLE_XCB)
    add_subdirectory(silksurf-gui)
endif()

# Tests
if(SilkSurf_ENABLE_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()

# ============================================================================
# Summary
# ============================================================================

message(STATUS "SilkSurf Build Configuration:")
message(STATUS "  Compiler: ${CMAKE_C_COMPILER}")
message(STATUS "  Build type: ${CMAKE_BUILD_TYPE}")
message(STATUS "  CLI enabled: ${SilkSurf_ENABLE_CLI}")
message(STATUS "  TUI enabled: ${SilkSurf_ENABLE_TUI}")
message(STATUS "  Curses enabled: ${SilkSurf_ENABLE_CURSES}")
message(STATUS "  XCB enabled: ${SilkSurf_ENABLE_XCB}")
message(STATUS "  Tests enabled: ${SilkSurf_ENABLE_TESTS}")
message(STATUS "  LTO enabled: ${SilkSurf_LTO}")
```

### 1.2 C Core CMakeLists.txt

```cmake
# silksurf/silksurf-core/CMakeLists.txt

add_library(silksurf-core STATIC
    src/html/tokenizer.c
    src/html/tree_constructor.c
    src/html/parser.c
    src/css/tokenizer.c
    src/css/parser.c
    src/css/cascade.c
    src/dom/node.c
    src/dom/tree.c
    src/layout/engine.c
    src/layout/box_model.c
    src/render/damage.c
    src/render/buffer.c
    src/render/paint.c
    src/neural/model.c
    src/neural/bpe_vocab.c
    src/memory/arena.c
)

target_include_directories(silksurf-core PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include
)

# Warnings as errors
target_compile_options(silksurf-core PRIVATE
    -Wall -Wextra -Werror
)

# C11 standard
set_target_properties(silksurf-core PROPERTIES
    C_STANDARD 11
    C_STANDARD_REQUIRED ON
)

# ============================================================================
# Optional XShm/XCB support
# ============================================================================

if(SilkSurf_ENABLE_XCB)
    find_package(XCB REQUIRED)

    target_compile_definitions(silksurf-core PRIVATE HAVE_XCB=1)
    target_include_directories(silksurf-core PRIVATE ${XCB_INCLUDE_DIRS})
    target_link_libraries(silksurf-core PRIVATE ${XCB_LIBRARIES})

    # Add XCB-specific sources
    target_sources(silksurf-core PRIVATE
        src/render/xshm.c
    )
endif()

# ============================================================================
# Neural model support
# ============================================================================

# Link with TensorFlow Lite (optional, Phase 3+)
if(FALSE)  # Disabled for Phase 2
    find_package(TensorFlowLite REQUIRED)
    target_link_libraries(silksurf-core PRIVATE tensorflow-lite)
endif()

# ============================================================================
# Testing (unit tests for C core)
# ============================================================================

if(SilkSurf_ENABLE_TESTS)
    add_executable(test-silksurf-core
        tests/test_tokenizer.c
        tests/test_parser.c
        tests/test_cascade.c
        tests/test_layout.c
    )

    target_link_libraries(test-silksurf-core PRIVATE silksurf-core)
    target_include_directories(test-silksurf-core PRIVATE
        ${CMAKE_CURRENT_SOURCE_DIR}/include
    )

    add_test(NAME SilkSurf-Core COMMAND test-silksurf-core)
endif()
```

================================================================================
PART 2: RUST FFI INTEGRATION
================================================================================

### 2.1 Rust Subproject CMakeLists.txt

```cmake
# silksurf/silksurf-js/CMakeLists.txt

# ============================================================================
# Cargo Build Integration
# ============================================================================

set(CARGO_MANIFEST_DIR "${CMAKE_CURRENT_SOURCE_DIR}")
set(CARGO_BUILD_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo")
set(CARGO_TARGET_DIR "${CARGO_BUILD_DIR}/target")

# Determine build type for cargo
if(CMAKE_BUILD_TYPE STREQUAL "Release")
    set(CARGO_RELEASE "--release")
    set(CARGO_BUILD_SUBDIR "release")
else()
    set(CARGO_RELEASE "")
    set(CARGO_BUILD_SUBDIR "debug")
endif()

# ============================================================================
# Custom Cargo Build Target
# ============================================================================

add_custom_target(cargo-build ALL
    COMMAND ${CMAKE_COMMAND} -E echo "Building SilkSurfJS with cargo..."
    COMMAND
        cargo build
        ${CARGO_RELEASE}
        --target-dir=${CARGO_TARGET_DIR}
        --manifest-path=${CARGO_MANIFEST_DIR}/Cargo.toml
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
    USES_TERMINAL
)

# ============================================================================
# Create Library Target from Cargo Output
# ============================================================================

add_library(silksurf-js STATIC IMPORTED)

# Path to built library
set_target_properties(silksurf-js PROPERTIES
    IMPORTED_LOCATION
    ${CARGO_TARGET_DIR}/${CARGO_BUILD_SUBDIR}/libsilksurf_js.a
)

add_dependencies(silksurf-js cargo-build)

# ============================================================================
# FFI Header
# ============================================================================

target_include_directories(silksurf-js INTERFACE
    ${CMAKE_CURRENT_SOURCE_DIR}/src/ffi
)

# Copy FFI headers to build directory
add_custom_command(TARGET cargo-build POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_directory
        ${CMAKE_CURRENT_SOURCE_DIR}/src/ffi
        ${CMAKE_CURRENT_BINARY_DIR}/include/silksurf-js
)

# ============================================================================
# Testing
# ============================================================================

if(SilkSurf_ENABLE_TESTS)
    add_custom_target(cargo-test
        COMMAND cargo test --manifest-path=${CARGO_MANIFEST_DIR}/Cargo.toml
        WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
        USES_TERMINAL
    )

    add_test(
        NAME SilkSurfJS
        COMMAND cargo test --manifest-path=${CARGO_MANIFEST_DIR}/Cargo.toml
    )
endif()
```

### 2.2 Cargo.toml Configuration

```toml
[package]
name = "silksurf-js"
version = "0.1.0"
edition = "2021"

[lib]
name = "silksurf_js"
crate-type = ["staticlib"]

[dependencies]
[dev-dependencies]
criterion = "0.5"

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1

[profile.dev]
opt-level = 0
```

================================================================================
PART 3: INTERFACE TARGETS
================================================================================

### 3.1 XCB GUI Target

```cmake
# silksurf/silksurf-gui/CMakeLists.txt

add_executable(silksurf-gui
    src/main.c
    src/window.c
    src/widget.c
    src/button.c
    src/textinput.c
)

# Link against shared components
target_link_libraries(silksurf-gui PRIVATE
    silksurf-core
    silksurf-js
)

# XCB dependencies
find_package(XCB REQUIRED)
target_link_libraries(silksurf-gui PRIVATE
    ${XCB_LIBRARIES}
)
target_include_directories(silksurf-gui PRIVATE
    ${XCB_INCLUDE_DIRS}
)

# Cairo for rendering (optional)
find_package(Cairo)
if(CAIRO_FOUND)
    target_link_libraries(silksurf-gui PRIVATE ${CAIRO_LIBRARIES})
    target_compile_definitions(silksurf-gui PRIVATE HAVE_CAIRO=1)
endif()

set_target_properties(silksurf-gui PROPERTIES
    OUTPUT_NAME silksurf
    C_STANDARD 11
)
```

### 3.2 CLI Target

```cmake
# silksurf/silksurf-cli/CMakeLists.txt

add_executable(silksurf-cli
    src/main.c
    src/parser.c
    src/repl.c
)

target_link_libraries(silksurf-cli PRIVATE
    silksurf-core
    silksurf-js
)

set_target_properties(silksurf-cli PROPERTIES
    OUTPUT_NAME silksurf-cli
    C_STANDARD 11
)
```

### 3.3 TUI Target (Future)

```cmake
# silksurf/silksurf-tui/CMakeLists.txt

# Planned for Phase 3+
# Dependencies: ncurses (optional), libvterm (maybe)
```

================================================================================
PART 4: TESTING & VALIDATION
================================================================================

### 4.1 Test Configuration

```cmake
# silksurf/tests/CMakeLists.txt

# Unit tests
add_executable(test-parser
    unit/test_tokenizer.c
    unit/test_parser.c
)
target_link_libraries(test-parser PRIVATE silksurf-core)

add_test(NAME Parser COMMAND test-parser)

# Integration tests
add_executable(test-integration
    integration/test_html_parsing.c
    integration/test_css_cascade.c
    integration/test_layout.c
)
target_link_libraries(test-integration PRIVATE
    silksurf-core
    silksurf-js
)

add_test(NAME Integration COMMAND test-integration)

# Benchmarks
add_executable(bench-parsing
    benchmarks/bench_tokenizer.c
)
target_link_libraries(bench-parsing PRIVATE silksurf-core)

# Test262 compliance
add_custom_target(test262
    COMMAND python3 ${CMAKE_SOURCE_DIR}/test262/runner.py
    WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
)
```

### 4.2 CTest Integration

```cmake
# Enable dashboard submission
include(CTest)

set(CTEST_SITE "silksurf-ci")
set(CTEST_BUILD_NAME "phase-2-build")
set(CTEST_UPDATE_COMMAND "git")

# Run all tests
enable_testing()
```

================================================================================
PART 5: CI/CD PIPELINE HOOKS
================================================================================

### 5.1 GitHub Actions (Phase 3+)

```yaml
name: Build & Test

on: [push, pull_request]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - name: Install dependencies
        run: |
          apt-get update
          apt-get install -y libxcb-dev rustc cargo
      - name: Build
        run: |
          mkdir build && cd build
          cmake -DCMAKE_BUILD_TYPE=Release ..
          cmake --build . --parallel
      - name: Test
        run: |
          cd build
          ctest --output-on-failure
      - name: Upload coverage
        uses: codecov/codecov-action@v2
```

### 5.2 Build Verification Script

```bash
#!/bin/bash
# silksurf/verify_build.sh

set -e

BUILD_DIR=${BUILD_DIR:-build}
BUILD_TYPE=${BUILD_TYPE:-Release}

echo "=== SilkSurf Build Verification ==="
echo "Build directory: $BUILD_DIR"
echo "Build type: $BUILD_TYPE"

# Clean and configure
rm -rf $BUILD_DIR
mkdir -p $BUILD_DIR
cd $BUILD_DIR

cmake \
    -DCMAKE_BUILD_TYPE=$BUILD_TYPE \
    -DSilkSurf_ENABLE_CLI=ON \
    -DSilkSurf_ENABLE_XCB=ON \
    -DSilkSurf_ENABLE_TESTS=ON \
    ..

# Build
cmake --build . --parallel $(nproc)

# Run tests
ctest --output-on-failure

echo "=== Build Successful ==="
```

================================================================================
END OF CMAKE BUILD SYSTEM DESIGN
================================================================================

**Status**: Complete (All build configuration documented)
**Next**: Phase 3 implementation can begin with this CMake foundation
**Validation**: Verify build succeeds for all enabled interfaces
