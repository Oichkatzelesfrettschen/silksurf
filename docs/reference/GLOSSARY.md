# SilkSurf Technical Glossary

**Purpose**: Define all technical terms, acronyms, and jargon used in SilkSurf documentation
**Updated**: 2026-01-29

---

## Project-Specific Terms

### Arena Allocator
**Type**: Memory Management Pattern
**Definition**: Bump allocator that allocates memory in large chunks ("arenas") and frees all allocations at once. Extremely fast for temporary allocations.
**SilkSurf Usage**: DOM tree, layout boxes, CSS computed styles
**Benefits**: Zero fragmentation, O(1) allocation, batch deallocation
**Trade-off**: Cannot free individual allocations

### BPE (Byte Pair Encoding)
**Type**: Compression/Tokenization Algorithm
**Definition**: Iterative algorithm that merges frequent byte pairs into single tokens
**SilkSurf Usage**: JavaScript lexer optimization, neural token prediction
**Performance**: 50+ MB/s tokenization target
**Reference**: `SILKSURF-JS-DESIGN.md`

### Cleanroom Implementation
**Type**: Development Methodology
**Definition**: Writing code from scratch using only specifications, without referencing existing implementations
**SilkSurf Usage**: JavaScript engine, layout algorithms
**Purpose**: Avoid copyright issues, ensure deep understanding
**Policy**: See CLAUDE.md "NO SHORTCUTS" section

### Damage Tracking
**Type**: Rendering Optimization
**Definition**: Recording which screen regions changed to avoid redrawing unchanged pixels
**SilkSurf Usage**: Rendering pipeline for 100+ FPS performance
**Mechanism**: XCB damage extension, rectangular region tracking
**Benefit**: 10x fewer pixel updates

### Phase 3
**Type**: Project Milestone
**Definition**: Parallel implementation phase (12 weeks)
**Goal**: Functional browser, 95% Test262 compliance, 60 FPS layout
**Status**: Week 1-2 (in progress)
**Teams**: Rust Engine, C Core, Graphics, Build/DevOps

---

## Architecture Terms

### Box Model
**Type**: Layout Concept
**Definition**: CSS visual layout model where each element is a rectangular box
**Components**: content, padding, border, margin
**SilkSurf Status**: Implementation pending (Task #25)
**Formula**: `total_width = content + 2×(padding + border + margin)`

### Cascade
**Type**: CSS Algorithm
**Definition**: Process of resolving multiple conflicting CSS rules to compute final styles
**Factors**: Specificity, source order, !important, inheritance
**SilkSurf Status**: 90% complete (Task #22 in progress)
**Library**: libcss

### DOM (Document Object Model)
**Type**: API/Data Structure
**Definition**: Tree-structured representation of HTML document
**SilkSurf Implementation**: libdom (C library from NetSurf)
**Nodes**: Element, Text, Comment, Document
**Operations**: Traversal, manipulation, querying

### Layout Engine
**Type**: Browser Component
**Definition**: Converts DOM + styles into positioned boxes
**Algorithms**: Block layout, inline layout, flex, grid
**SilkSurf Status**: Stub implementation (Tasks #25, #21, #30 pending)
**Target**: 60 FPS

### Rendering Pipeline
**Type**: System Architecture
**Definition**: HTML → Parse → DOM → Style → Layout → Paint → Display
**SilkSurf Stages**:
1. HTML Parse (libhubbub → libdom)
2. CSS Parse (libcss)
3. Style Computation (cascade algorithm)
4. Layout (box model, positioning)
5. Paint (XCB rendering, SIMD ops)
6. Display (double-buffer, XShm)

---

## Browser Terminology

### CSS Selector
**Type**: CSS Query Syntax
**Examples**: `div.class`, `#id`, `p > a`, `[attr="value"]`
**Purpose**: Match elements in DOM for styling
**SilkSurf Status**: Implemented via libcss callbacks

### HTML5 Tokenizer
**Type**: Parser Component
**Definition**: Converts HTML byte stream into tokens (StartTag, EndTag, Character, etc.)
**SilkSurf Implementation**: libhubbub
**Specification**: WHATWG HTML Standard
**Performance**: Streaming, state-machine based

### Quirks Mode
**Type**: Browser Compatibility Mode
**Definition**: Legacy rendering mode for pre-standards HTML
**Modes**: Quirks, Limited Quirks, Standards
**SilkSurf Handling**: libdom handles doctype-based detection

### Tree Builder
**Type**: Parser Component
**Definition**: Constructs DOM tree from HTML tokens
**SilkSurf Implementation**: libdom
**Features**: Error recovery, implicit tags, foster parenting

---

## JavaScript Terms

### AST (Abstract Syntax Tree)
**Type**: Data Structure
**Definition**: Tree representation of program structure
**SilkSurf Usage**: JavaScript parser output
**Phases**: Tokens → AST → Bytecode
**Status**: Planned (Rust implementation)

### Bytecode
**Type**: Intermediate Representation
**Definition**: Low-level instruction set for VM execution
**SilkSurf Design**: 50+ instructions, stack-based
**Target**: Single-pass compilation, no heap during parse
**Reference**: `SILKSURF-JS-DESIGN.md`

### GC (Garbage Collection)
**Type**: Memory Management
**Definition**: Automatic reclamation of unused memory
**SilkSurf Strategy**: Hybrid (arena + generational + reference counting)
**Target**: 99% allocation reduction vs traditional GC
**Status**: Planned (Rust implementation)

### Test262
**Type**: Compliance Test Suite
**Definition**: Official ECMAScript conformance tests (~50,000 tests)
**SilkSurf Target**: 95%+ compliance
**Status**: Planned (JavaScript engine not yet implemented)
**Repository**: tc39/test262

---

## Performance Terms

### FPS (Frames Per Second)
**Type**: Performance Metric
**Definition**: Number of complete frames rendered per second
**SilkSurf Targets**:
- Layout: 60 FPS
- Rendering (damage-tracked): 100+ FPS
**Bottlenecks**: Layout > Style Computation > Paint

### PGO (Profile-Guided Optimization)
**Type**: Compiler Optimization
**Definition**: Compiler uses runtime profiling data to optimize hot paths
**SilkSurf Usage**: Optional build mode for 10-30% speedup
**Process**: Build with instrumentation → Run workload → Rebuild with profile
**Reference**: `docs/development/BUILD.md`

### SIMD (Single Instruction, Multiple Data)
**Type**: CPU Instruction Set
**Definition**: Process multiple data elements in parallel
**SilkSurf Usage**: Pixel operations, memcpy, alpha blending
**Extensions**: SSE2, AVX2
**Detection**: CPUID at runtime (Task #24)
**Speedup**: 2-4x on supported hardware

### Zero-Copy
**Type**: Optimization Pattern
**Definition**: Avoid copying data between buffers
**SilkSurf Usage**: Tokenization, string interning
**Mechanism**: References, slices, memory mapping
**Benefit**: Reduced memory allocations and bandwidth

---

## Build Terms

### CMake
**Type**: Build System
**Definition**: Cross-platform build configuration tool
**SilkSurf Usage**: C/C++ components, test targets
**Files**: `CMakeLists.txt`
**Commands**: `cmake -B build`, `cmake --build build`

### Cargo
**Type**: Rust Build Tool
**Definition**: Rust package manager and build system
**SilkSurf Usage**: JavaScript engine (`silksurf-js` crate)
**Files**: `Cargo.toml`, `rust-toolchain.toml`
**Integration**: Called from CMake via custom target

### FFI (Foreign Function Interface)
**Type**: Interop Mechanism
**Definition**: Calling functions across language boundaries
**SilkSurf Usage**: C ↔ Rust for JavaScript engine integration
**Safety**: Type marshalling, validation at boundary
**Status**: Incomplete (Task #33)

### Fuzzing
**Type**: Testing Technique
**Definition**: Automated testing with random/mutated inputs
**SilkSurf Tool**: AFL++ (American Fuzzy Lop)
**Targets**: HTML parser, CSS parser
**Status**: Harness implemented, not integrated (Task #31)
**Goal**: 24 hours with zero crashes

---

## GUI/Graphics Terms

### Compositor
**Type**: Graphics Component
**Definition**: Combines multiple layers into final image
**X11 Extension**: XComposite
**SilkSurf Usage**: Planned for transparency effects
**Alternative**: Manual alpha blending

### Damage Extension
**Type**: X11 Extension
**Definition**: Notifies applications of changed screen regions
**X11 Name**: XDamage
**SilkSurf Usage**: Avoid redrawing unchanged areas
**Performance**: 10x reduction in pixel updates

### Double Buffering
**Type**: Graphics Technique
**Definition**: Render to off-screen buffer, then swap to visible
**Purpose**: Prevent tearing, smooth animation
**SilkSurf Implementation**: XCB pixmaps
**Cost**: 2x memory per window

### Pixmap
**Type**: X11 Object
**Definition**: Off-screen image buffer
**SilkSurf Usage**: Double buffering, caching
**Operations**: Draw operations, XShmPutImage
**Memory**: VRAM or system RAM

### XCB (X C Binding)
**Type**: X11 Library
**Definition**: Low-level C library for X Window System
**Alternative**: Xlib (higher-level, deprecated)
**SilkSurf Choice**: Direct XCB for minimal overhead
**Performance**: ~30% less overhead than GTK

### XShm (X Shared Memory)
**Type**: X11 Extension
**Definition**: Zero-copy image transfer using shared memory
**Benefit**: 10x faster than socket-based image uploads
**SilkSurf Status**: Planned (Task #26)
**Requirement**: XCB-SHM extension

---

## Quality Assurance Terms

### ASAN (AddressSanitizer)
**Type**: Dynamic Analysis Tool
**Definition**: Detects memory errors (buffer overflow, use-after-free)
**SilkSurf Usage**: Build mode for testing
**Overhead**: 2x slowdown, 2-3x memory
**Command**: `cmake -DCMAKE_C_FLAGS="-fsanitize=address"`

### UBSAN (UndefinedBehaviorSanitizer)
**Type**: Dynamic Analysis Tool
**Definition**: Detects undefined behavior (integer overflow, NULL dereference)
**SilkSurf Usage**: Build mode for testing
**Command**: `cmake -DCMAKE_C_FLAGS="-fsanitize=undefined"`

### Valgrind
**Type**: Dynamic Analysis Tool
**Definition**: Detects memory leaks, invalid accesses
**SilkSurf Usage**: Memory leak detection
**Overhead**: 10-50x slowdown
**Status**: 0 errors in core paths
**Command**: `valgrind --leak-check=full ./test_dom_parsing`

### -Werror
**Type**: Compiler Flag
**Definition**: Treat all warnings as errors
**SilkSurf Policy**: Enabled by default (enforces 0 warnings)
**Purpose**: Prevent drift, catch issues early
**Status**: ✓ All code compiles with -Werror

---

## Library/Dependency Terms

### libcss
**Type**: C Library
**Origin**: NetSurf Project
**Purpose**: CSS parsing, selector matching, cascade algorithm
**SilkSurf Integration**: ✓ Complete (handler callbacks implemented)
**Features**: Full CSS 2.1, partial CSS3

### libdom
**Type**: C Library
**Origin**: NetSurf Project
**Purpose**: DOM tree construction, manipulation, traversal
**SilkSurf Integration**: ✓ Complete
**API**: W3C DOM Core Level 3 compatible

### libhubbub
**Type**: C Library
**Origin**: NetSurf Project
**Purpose**: HTML5 tokenization and parsing
**SilkSurf Integration**: ✓ Complete
**Compliance**: HTML5 tokenizer specification

### libparserutils
**Type**: C Library
**Origin**: NetSurf Project
**Purpose**: Common parsing utilities (character encoding, input streams)
**SilkSurf Integration**: ✓ Dependency of libhubbub/libdom

### libpixman
**Type**: C Library
**Origin**: Cairo/X.org
**Purpose**: Low-level pixel manipulation
**SilkSurf Integration**: ✓ Used for alpha blending
**Features**: Porter-Duff compositing, antialiasing

---

## Acronyms Reference

| Acronym | Full Name | Category |
|---------|-----------|----------|
| AFL | American Fuzzy Lop | Testing |
| API | Application Programming Interface | General |
| ASAN | AddressSanitizer | Testing |
| AST | Abstract Syntax Tree | Compiler |
| BPE | Byte Pair Encoding | Optimization |
| CI/CD | Continuous Integration/Deployment | DevOps |
| CPU | Central Processing Unit | Hardware |
| CSS | Cascading Style Sheets | Web Standard |
| DOM | Document Object Model | Web Standard |
| FFI | Foreign Function Interface | Interop |
| FPS | Frames Per Second | Performance |
| GC | Garbage Collection | Memory |
| GUI | Graphical User Interface | Interface |
| HTML | HyperText Markup Language | Web Standard |
| JIT | Just-In-Time (compilation) | Compiler |
| LSTM | Long Short-Term Memory | ML |
| MVP | Minimum Viable Product | Product |
| PGO | Profile-Guided Optimization | Compiler |
| POSIX | Portable Operating System Interface | Standard |
| PR | Pull Request | Git |
| SIMD | Single Instruction Multiple Data | Hardware |
| SSE | Streaming SIMD Extensions | Hardware |
| TCB | Trusted Code Base | Security |
| TODO | To-Do (task marker) | Development |
| UBSAN | UndefinedBehaviorSanitizer | Testing |
| UI | User Interface | Interface |
| VM | Virtual Machine | Runtime |
| VRAM | Video Random Access Memory | Hardware |
| XCB | X C Binding | Graphics |
| XShm | X Shared Memory | Graphics |

---

## Phase-3 Engine Pipeline Terms (added 2026-04-30)

### CascadeView
**Aliases**: CascadeView SoA, cascade SoA projection
**Type**: Structure-of-Arrays projection
**Definition**: SoA materialization of cascade-relevant DOM fields (tag, id_index, class_start, class_count, parent_id) in a flat 40-byte-per-node array. Built once per render and consumed by the cascade pass; replaces the 168-byte `Node` fetch inside the matching hot path. Fits 1 cache line per entry; gives ~4.2x compression vs the AoS Node read. The term "CascadeView SoA" is used throughout ADRs and runbooks to refer to this structure and its SoA layout strategy.
**SilkSurf Usage**: `crates/silksurf-css/src/cascade_view.rs`; consumed by `silksurf-engine::fused_pipeline::FusedWorkspace`.
**Why it matters**: drove the 9.5us steady-state benchmark (see `docs/PERFORMANCE.md`).

### CascadeEntry
**Type**: 40-byte SoA row
**Definition**: One entry in a `CascadeView`. Carries the tag, the interned id atom index, the class slice (start + count into the flat ident array), and the parent-node index. The `parent_id` field is `pub(crate)` -- external readers use `CascadeView::parent_of()` which hides the NO_PARENT sentinel and the >65534-node fallback.

### FusedWorkspace
**Type**: Reusable per-frame scratch buffer container
**Definition**: Holds the `LayoutNeighborTable`, `CascadeWorkspace`, and the output Vecs (styles, node_rects, block_cursors, display_items) as owned fields. After the first render, zero allocator traffic for same-or-smaller DOMs. High-water-mark growth (containers grow to peak node count and never shrink). See `crates/silksurf-engine/src/fused_pipeline.rs`.

### Generation-gated rebuild
**Type**: Cache-coherence pattern
**Definition**: `Dom::generation()` = (instance_id << 32) | mutation_counter. The fused pipeline skips `table.rebuild()` and `cascade_view.rebuild()` when the cached generation matches the current DOM. Saves ~2us on hover/resize/media-query re-renders over an unchanged DOM.

### IndexedSelector.pair_id
**Type**: Sequential u32 identifier
**Definition**: Assigned by `StyleIndex` to each (rule, selector) pair. Replaces a `FxHashSet<(usize, usize)>` dedup with a `Vec<u64>` bitvec; dedup becomes a branchless shift+mask (3 u64 words for 159 pairs) and clearing is `fill(0)` instead of hash table reset. See `crates/silksurf-css/src/style.rs`.

### Lock-free monotonic resolve table
**Type**: Concurrency pattern
**Definition**: `Dom::resolve_table` (a `Vec<SmallString>`) is materialized from the interner at two phase boundaries (`TreeBuilder::into_dom()` and `Dom::end_mutation_batch()`). `Dom::resolve_fast(atom)` is a plain array index by `atom.raw()`, zero synchronization. Replaces the prior `RwLock<SilkInterner>` read-lock-per-cascade-call. The interner write path retains the RwLock; the read hot path is lock-free.

### resolve_fast
**Type**: Lock-free atom resolution
**Definition**: `dom.resolve_fast(atom) -> &SmallString`. Reads from the monotonic resolve table by raw index. Used in the cascade matching path where the prior `dom.resolve(atom)` cost a RwLock read per call (~6ns * 29 atoms = 168ns per cascade, eliminated).

### Static FALLBACK
**Type**: Lazy-allocated default-value cache
**Definition**: `LazyLock<ComputedStyle>` in `CascadedStyle::resolve()`. Constructed once per process and reused via reference. Eliminates ~61 `ComputedStyle::default()` constructions per render (each building SmallVec + SmolStr). Non-Copy fields clone only when needed (rare).

### Phase-4.4 SoA TODOs
**Type**: Scheduled performance work
**Definition**: Three documented-in-code TODOs to convert `ComputedStyle`, `Dimensions` (silksurf-layout), and the `DisplayList` to SoA layout. Tracked in the SNAZZY-WAFFLE roadmap P4. Expected to extend the 9.5us steady-state further by improving cache reuse during the per-node loop.

### `silksurf_core::SilkError`
**Type**: Workspace-wide canonical error
**Definition**: String-erased `enum` (variants for Css, Dom, HtmlTokenize, HtmlTreeBuild, Net, Tls, Engine, Js, Io, plus generic InvalidInput / Unsupported). Per-crate error types implement `From<MyError> for SilkError` in their own crate (silksurf-core has no rev-deps). Public APIs at the workspace boundary funnel through this type. See `crates/silksurf-core/src/error.rs`.

### UNWRAP-OK / SAFETY annotations
**Type**: Lint-enforced documentation requirement
**Definition**: Every `.unwrap()` or `.expect(` in production code must be preceded within 7 lines by a `// UNWRAP-OK: <invariant>` comment. Every `unsafe { ... }` block must be preceded within 7 lines by `// SAFETY: <invariant>`. Enforced by `scripts/lint_unwrap.sh` and `scripts/lint_unsafe.sh`, both wired into the local-gate fast pass. Cross-crate index of unsafe blocks: `docs/design/UNSAFE-CONTRACTS.md`.

---

## See Also

- `/README.md` - Project overview
- `/CLAUDE.md` - Engineering standards
- `/CONTRIBUTING.md` - Onboarding flow with hook setup
- `/docs/REPO-LAYOUT.md` - Repository directory and file inventory
- `/docs/development/BUILD.md` - Build instructions
- `/docs/development/LOCAL-GATE.md` - Local-gate reference
- `/docs/design/UNSAFE-CONTRACTS.md` - Unsafe-block index
- `/docs/design/ARCHITECTURE-DECISIONS.md` - ADRs (incl. AD-008 stable Rust, AD-009 local-only CI, AD-010 XCB-only GUI)
- `/docs/PERFORMANCE.md` - Bench reproducibility and steady-state results
- `/docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md` - Current milestones
- `/silksurf-specification/` - Technical specifications
