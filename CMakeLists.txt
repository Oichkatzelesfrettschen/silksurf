cmake_minimum_required(VERSION 3.10)
project(silksurf C)

# Settings
set(CMAKE_C_STANDARD 11)
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -O3 -march=native -Wall -Wextra -Werror -fno-exceptions")

# Sanitizer support
set(SANITIZER "" CACHE STRING "Enable sanitizer(s): address, undefined, address,undefined, or leave empty")
if(SANITIZER)
    message(STATUS "Sanitizer enabled: ${SANITIZER}")
    set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -fsanitize=${SANITIZER} -fno-omit-frame-pointer")
    set(CMAKE_EXE_LINKER_FLAGS "${CMAKE_EXE_LINKER_FLAGS} -fsanitize=${SANITIZER}")
    # Disable optimizations for better stack traces in sanitizer builds
    if(CMAKE_BUILD_TYPE STREQUAL "Release")
        string(REPLACE "-O3" "-O1" CMAKE_C_FLAGS "${CMAKE_C_FLAGS}")
    endif()
endif()

# AFL++ fuzzing support
option(ENABLE_FUZZING "Build with AFL++ instrumentation for fuzzing" OFF)
if(ENABLE_FUZZING)
    find_program(AFL_CC afl-clang-fast)
    if(AFL_CC)
        message(STATUS "AFL++ fuzzing enabled with afl-clang-fast")
        set(CMAKE_C_COMPILER ${AFL_CC})
        set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -g")
    else()
        message(WARNING "AFL++ not found, fuzzing will not be instrumented")
    endif()
endif()

# Packages
find_package(PkgConfig REQUIRED)

# XCB (required)
pkg_check_modules(XCB REQUIRED xcb xcb-damage xcb-composite)

# NetSurf web engine libraries (required for Phase 4+)
pkg_check_modules(LIBHUBBUB REQUIRED libhubbub)
pkg_check_modules(LIBCSS REQUIRED libcss)
pkg_check_modules(LIBDOM REQUIRED libdom)
pkg_check_modules(LIBPARSERUTILS REQUIRED libparserutils)

# Optional: XCB-util for convenience
pkg_check_modules(XCB_UTIL xcb-util)
pkg_check_modules(XCB_SHM xcb-shm)
pkg_check_modules(PIXMAN pixman-1)

# Include directories
include_directories(
    ${CMAKE_SOURCE_DIR}/include
    ${CMAKE_SOURCE_DIR}/src
    ${XCB_INCLUDE_DIRS}
    ${XCB_SHM_INCLUDE_DIRS}
    ${LIBHUBBUB_INCLUDE_DIRS}
    ${LIBCSS_INCLUDE_DIRS}
    ${LIBDOM_INCLUDE_DIRS}
    ${LIBPARSERUTILS_INCLUDE_DIRS}
)

# Link directories
link_directories(
    ${XCB_LIBRARY_DIRS}
    ${XCB_SHM_LIBRARY_DIRS}
    ${LIBHUBBUB_LIBRARY_DIRS}
    ${LIBCSS_LIBRARY_DIRS}
    ${LIBDOM_LIBRARY_DIRS}
    ${LIBPARSERUTILS_LIBRARY_DIRS}
)

file(GLOB_RECURSE SOURCES "src/**/*.c")
file(GLOB CSS_SOURCES "src/css/*.c")
list(APPEND SOURCES ${CSS_SOURCES})
list(FILTER SOURCES EXCLUDE REGEX "tree_builder\\.c")
list(FILTER SOURCES EXCLUDE REGEX "fuzz_.*\\.c")
list(FILTER SOURCES EXCLUDE REGEX "main_gui\\.c")
list(FILTER SOURCES EXCLUDE REGEX "box_model\\.c")
list(FILTER SOURCES EXCLUDE REGEX "bpe_bench\\.c")
list(FILTER SOURCES EXCLUDE REGEX "main\\.c") # Exclude main.c from SOURCES as it is added explicitly

# Rust Integration
find_program(CARGO_EXECUTABLE cargo REQUIRED)

set(RUST_SOURCE_DIR "${CMAKE_SOURCE_DIR}/silksurf-js")
set(RUST_TARGET_DIR "${RUST_SOURCE_DIR}/target")

if(CMAKE_BUILD_TYPE STREQUAL "Release")
    set(CARGO_CMD ${CARGO_EXECUTABLE} build --release --manifest-path ${RUST_SOURCE_DIR}/Cargo.toml)
    set(RUST_LIB "${RUST_TARGET_DIR}/release/libsilksurf_js.a")
else()
    set(CARGO_CMD ${CARGO_EXECUTABLE} build --manifest-path ${RUST_SOURCE_DIR}/Cargo.toml)
    set(RUST_LIB "${RUST_TARGET_DIR}/debug/libsilksurf_js.a")
endif()

add_custom_target(silksurf_js_target ALL
    COMMAND ${CARGO_CMD}
    WORKING_DIRECTORY ${RUST_SOURCE_DIR}
    COMMENT "Building Rust component (silksurf-js)"
)

add_library(silksurf_js STATIC IMPORTED)
set_target_properties(silksurf_js PROPERTIES IMPORTED_LOCATION ${RUST_LIB})
add_dependencies(silksurf_js silksurf_js_target)

# Create executable
add_executable(silksurf
    src/main.c
    ${SOURCES}
)

# Link libraries
target_link_libraries(silksurf
    silksurf_js
    dl
    pthread
    ${XCB_LIBRARIES}
    ${LIBHUBBUB_LIBRARIES}
    ${LIBCSS_LIBRARIES}
    ${LIBDOM_LIBRARIES}
    ${LIBPARSERUTILS_LIBRARIES}
    m
)

# Optional: Link optional libraries
if(XCB_SHM_FOUND)
    target_link_libraries(silksurf ${XCB_SHM_LIBRARIES})
    add_definitions(-DSILK_HAS_XCB_SHM=1)
endif()

if(PIXMAN_FOUND)
    target_link_libraries(silksurf ${PIXMAN_LIBRARIES})
    add_definitions(-DSILK_HAS_PIXMAN=1)
endif()

# Test executable (use source files without main.c)
file(GLOB MEMORY_SOURCES "src/memory/*.c")
file(GLOB GUI_SOURCES "src/gui/*.c")
list(FILTER GUI_SOURCES EXCLUDE REGEX "main_gui\\.c")
file(GLOB RENDERING_SOURCES "src/rendering/*.c")
file(GLOB DOCUMENT_SOURCES "src/document/*.c")
list(FILTER DOCUMENT_SOURCES EXCLUDE REGEX "tree_builder\\.c")

add_executable(test_dom_parsing
    tests/test_dom_parsing.c
    ${MEMORY_SOURCES}
    ${GUI_SOURCES}
    ${RENDERING_SOURCES}
    ${DOCUMENT_SOURCES}
)

target_link_libraries(test_dom_parsing
    ${XCB_LIBRARIES}
    ${LIBHUBBUB_LIBRARIES}
    ${LIBCSS_LIBRARIES}
    ${LIBDOM_LIBRARIES}
    ${LIBPARSERUTILS_LIBRARIES}
    m
)

# Optional: Link optional libraries to test targets too
if(XCB_SHM_FOUND)
    target_link_libraries(test_dom_parsing ${XCB_SHM_LIBRARIES})
endif()

if(PIXMAN_FOUND)
    target_link_libraries(test_dom_parsing ${PIXMAN_LIBRARIES})
endif()

# Parser basic test
add_executable(test_parser_basic
    tests/test_parser_basic.c
)

target_link_libraries(test_parser_basic
    ${LIBHUBBUB_LIBRARIES}
)

# Simple parse test
add_executable(test_parse_simple
    tests/test_parse_simple.c
)

target_link_libraries(test_parse_simple
    ${LIBHUBBUB_LIBRARIES}
)

# CSS engine test
add_executable(test_css_engine
    tests/test_css_engine.c
    ${MEMORY_SOURCES}
    ${DOCUMENT_SOURCES}
)

target_link_libraries(test_css_engine
    ${XCB_LIBRARIES}
    ${LIBHUBBUB_LIBRARIES}
    ${LIBCSS_LIBRARIES}
    ${LIBDOM_LIBRARIES}
    ${LIBPARSERUTILS_LIBRARIES}
    m
)

# CSS cascade test
add_executable(test_css_cascade
    tests/test_css_cascade.c
    ${MEMORY_SOURCES}
    ${DOCUMENT_SOURCES}
)

target_link_libraries(test_css_cascade
    ${XCB_LIBRARIES}
    ${LIBHUBBUB_LIBRARIES}
    ${LIBCSS_LIBRARIES}
    ${LIBDOM_LIBRARIES}
    ${LIBPARSERUTILS_LIBRARIES}
    m
)

# SIMD detection test
add_executable(test_simd_detection
    tests/test_simd_detection.c
    src/rendering/pixel_ops.c
)

target_link_libraries(test_simd_detection
    m
)

# Native CSS cascade algorithm test (no libcss dependency)
add_executable(test_css_cascade_native
    tests/test_css_cascade_native.c
    ${MEMORY_SOURCES}
    src/document/css_cascade.c
    src/document/css_property_spec.c
)

target_link_libraries(test_css_cascade_native
    m
)

# CSS cascade integration test (with LibCSS selector matching)
add_executable(test_css_cascade_integration
    tests/test_css_cascade_integration.c
    ${MEMORY_SOURCES}
    ${DOCUMENT_SOURCES}
)

target_link_libraries(test_css_cascade_integration
    ${XCB_LIBRARIES}
    ${LIBHUBBUB_LIBRARIES}
    ${LIBCSS_LIBRARIES}
    ${LIBDOM_LIBRARIES}
    ${LIBPARSERUTILS_LIBRARIES}
    m
)

# End-to-end rendering pipeline test
add_executable(test_e2e_rendering
    tests/test_e2e_rendering.c
    ${MEMORY_SOURCES}
    ${DOCUMENT_SOURCES}
    src/rendering/pixel_ops.c
    src/rendering/paint.c
    src/layout/box_model.c
)

target_link_libraries(test_e2e_rendering
    ${XCB_LIBRARIES}
    ${LIBHUBBUB_LIBRARIES}
    ${LIBCSS_LIBRARIES}
    ${LIBDOM_LIBRARIES}
    ${LIBPARSERUTILS_LIBRARIES}
    m
)

# Inline layout test
add_executable(test_inline_layout
    tests/test_inline_layout.c
    src/layout/inline.c
    src/layout/box_model.c
    ${MEMORY_SOURCES}
)

target_link_libraries(test_inline_layout
    m
)

# Replaced elements test
add_executable(test_replaced_elements
    tests/test_replaced_elements.c
    src/layout/box_model.c
    ${MEMORY_SOURCES}
)

target_link_libraries(test_replaced_elements
    m
)

# XCB Shared Memory test
add_executable(test_xcb_shm
    tests/test_xcb_shm.c
)

target_link_libraries(test_xcb_shm
    m
)

# Fuzzing harness
add_executable(silksurf_fuzz
    src/fuzz_harness.c
    ${MEMORY_SOURCES}
    ${DOCUMENT_SOURCES}
)

target_link_libraries(silksurf_fuzz
    ${LIBHUBBUB_LIBRARIES}
    ${LIBCSS_LIBRARIES}
    ${LIBDOM_LIBRARIES}
    ${LIBPARSERUTILS_LIBRARIES}
    m
)

# CSS Fuzzing harness
add_executable(silksurf_css_fuzz
    src/css/fuzz_css.c
    src/document/css_engine.c
    src/css/css_tokenizer.c
    src/css/selector.c
    src/css/cascade.c
    ${MEMORY_SOURCES}
    ${DOCUMENT_SOURCES}
)

target_link_libraries(silksurf_css_fuzz
    ${LIBHUBBUB_LIBRARIES}
    ${LIBCSS_LIBRARIES}
    ${LIBDOM_LIBRARIES}
    ${LIBPARSERUTILS_LIBRARIES}
    m
)

# Enable testing
enable_testing()
add_test(NAME parser_basic COMMAND test_parser_basic)
add_test(NAME dom_parsing COMMAND test_dom_parsing)
add_test(NAME css_engine COMMAND test_css_engine)
add_test(NAME css_cascade COMMAND test_css_cascade)
add_test(NAME css_cascade_native COMMAND test_css_cascade_native)
add_test(NAME css_cascade_integration COMMAND test_css_cascade_integration)
add_test(NAME simd_detection COMMAND test_simd_detection)
add_test(NAME e2e_rendering COMMAND test_e2e_rendering)
add_test(NAME inline_layout COMMAND test_inline_layout)
add_test(NAME replaced_elements COMMAND test_replaced_elements)
add_test(NAME xcb_shm COMMAND test_xcb_shm)

# Fuzzing custom targets (requires AFL++ and ENABLE_FUZZING=ON)
if(ENABLE_FUZZING AND AFL_CC)
    # HTML fuzzing target
    add_custom_target(fuzz_html
        COMMAND afl-fuzz -i ${CMAKE_SOURCE_DIR}/fuzz_corpus/html/input
                         -o ${CMAKE_SOURCE_DIR}/fuzz_corpus/html/output
                         -m none
                         -- $<TARGET_FILE:silksurf_fuzz>
        DEPENDS silksurf_fuzz
        COMMENT "Running AFL++ HTML fuzzing campaign"
        USES_TERMINAL
    )

    # CSS fuzzing target
    add_custom_target(fuzz_css
        COMMAND afl-fuzz -i ${CMAKE_SOURCE_DIR}/fuzz_corpus/css/input
                         -o ${CMAKE_SOURCE_DIR}/fuzz_corpus/css/output
                         -m none
                         -- $<TARGET_FILE:silksurf_css_fuzz>
        DEPENDS silksurf_css_fuzz
        COMMENT "Running AFL++ CSS fuzzing campaign"
        USES_TERMINAL
    )

    # Quick smoke test fuzzing (5 minutes)
    add_custom_target(fuzz_quick
        COMMAND timeout 300 afl-fuzz -i ${CMAKE_SOURCE_DIR}/fuzz_corpus/html/input
                                      -o ${CMAKE_SOURCE_DIR}/fuzz_corpus/html/output
                                      -m none
                                      -- $<TARGET_FILE:silksurf_fuzz> || true
        DEPENDS silksurf_fuzz
        COMMENT "Running 5-minute AFL++ smoke test"
        USES_TERMINAL
    )
endif()

# Install
install(TARGETS silksurf DESTINATION bin)

# Print config
message(STATUS "SilkSurf Configuration:")
message(STATUS "  Build Type: ${CMAKE_BUILD_TYPE}")
message(STATUS "  Sanitizer: ${SANITIZER}")
message(STATUS "  XCB Found: ${XCB_FOUND}")
message(STATUS "  libhubbub Found: ${LIBHUBBUB_FOUND}")
message(STATUS "  libcss Found: ${LIBCSS_FOUND}")
message(STATUS "  libdom Found: ${LIBDOM_FOUND}")
message(STATUS "  XCB SHM Found: ${XCB_SHM_FOUND}")
message(STATUS "  Pixman Found: ${PIXMAN_FOUND}")
message(STATUS "  C Flags: ${CMAKE_C_FLAGS}")

