cmake_minimum_required(VERSION 3.10)
project(silksurf C)

# Settings
set(CMAKE_C_STANDARD 11)
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -O3 -march=native -Wall -Wextra -Werror -fno-exceptions")

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
    ${LIBHUBBUB_INCLUDE_DIRS}
    ${LIBCSS_INCLUDE_DIRS}
    ${LIBDOM_INCLUDE_DIRS}
    ${LIBPARSERUTILS_INCLUDE_DIRS}
)

# Link directories
link_directories(
    ${XCB_LIBRARY_DIRS}
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
    src/css/selector.c
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

# Install
install(TARGETS silksurf DESTINATION bin)

# Print config
message(STATUS "SilkSurf Configuration:")
message(STATUS "  XCB Found: ${XCB_FOUND}")
message(STATUS "  libhubbub Found: ${LIBHUBBUB_FOUND}")
message(STATUS "  libcss Found: ${LIBCSS_FOUND}")
message(STATUS "  libdom Found: ${LIBDOM_FOUND}")
message(STATUS "  XCB SHM Found: ${XCB_SHM_FOUND}")
message(STATUS "  Pixman Found: ${PIXMAN_FOUND}")
message(STATUS "  C Flags: ${CMAKE_C_FLAGS}")

