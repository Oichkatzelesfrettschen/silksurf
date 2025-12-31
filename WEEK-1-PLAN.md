# Week 1 Plan: Infrastructure & Foundation

**Status**: Active
**Phase**: Phase 3, Week 1

## Objectives
-   **Build System**: integrated CMake (C) + Cargo (Rust) build.
-   **Rust Engine**: Arena allocator & basic lexer types.
-   **C Core**: Arena allocator (C) & project skeleton.
-   **Graphics**: XCB window initialization.
-   **CI/CD**: GitHub Actions setup.

## Task List

### 1. Build System (Critical Path)
- [ ] Review/Update `CMakeLists.txt` to support Rust FFI.
- [ ] Ensure `silksurf-js/Cargo.toml` is valid.
- [ ] Create `silksurf-js/src/lib.rs` with C-compatible FFI.
- [ ] Verify `cmake` builds both C and Rust components and links them.

### 2. C Core Foundation
- [ ] Implement `src/memory/arena.c` and `include/silksurf/memory.h` (Arena Allocator).
- [ ] Create unit test for Arena Allocator in `tests/`.

### 3. Rust Engine Foundation
- [ ] Implement Arena Allocator in Rust (`silksurf-js/src/gc/arena.rs`).
- [ ] Define Token types in `silksurf-js/src/lexer/token.rs`.

### 4. Graphics Foundation
- [ ] Implement basic XCB window in `src/gui/window.c`.
- [ ] Ensure it compiles with `libxcb`.

### 5. Validation
- [ ] Run `cmake --build build` -> Success (No warnings).
- [ ] Run `./build/silksurf` -> Window opens (even if empty).
