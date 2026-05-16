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

## API Reference

Public types and functions exposed at crate-root lib.rs boundaries. Entries
here satisfy the `scripts/lint_glossary.sh` hard gate. Each entry is a
brief operator-facing description; deep rationale lives in the crate OPERATIONS.md
files and the relevant ADRs.

### append_child
**Type**: DOM mutation function
**Definition**: Attaches a node as the last child of a parent element in the DOM tree. Must be called inside a `with_mutation_batch` for the generation counter to advance.

### ArenaVec
**Type**: Arena-backed collection
**Definition**: Vec-like container whose backing memory comes from the `SilkArena` bump allocator. Elements are never individually freed; the whole arena is released at once. Used for short-lived collections in the parse and layout passes.

### Attribute
**Type**: DOM data structure
**Definition**: An HTML or XML element attribute: a name (`AttributeName`) plus a string value. Stored as a flat slice on the element node.

### AttributeName
**Type**: Interned atom type
**Definition**: An `Atom` identifying an HTML or XML attribute name (e.g., `class`, `href`, `id`). Interned via `SilkInterner` so equality is a pointer compare.

### BasicClient
**Type**: HTTP client variant
**Definition**: HTTP client without TLS configuration. Plain HTTP (port 80) only. Suitable for local testing; not for production fetches.

### begin_mutation_batch
**Type**: DOM API function
**Definition**: Opens a DOM mutation batch. Mutations made while the batch is open are buffered; the generation counter advances and dirty nodes are flushed when `end_mutation_batch` is called at depth zero. Prefer `with_mutation_batch` which handles pairing automatically.

### build_display_list
**Type**: Layout-to-render function
**Definition**: Converts a styled and laid-out DOM into a flat `Vec<DisplayItem>`. Each item is a draw command (background fill, border, text run, image). Consumed by the rasterizer.

### build_layout_tree
**Type**: Layout engine entry point
**Definition**: Constructs the `LayoutTree` from a `&Dom` and a computed-style map. Respects `Dom::generation()` for staleness detection. See silksurf-layout OPERATIONS.md.

### build_layout_tree_incremental
**Type**: Incremental layout function
**Definition**: Variant of `build_layout_tree` that reuses unchanged subtrees when only a subset of DOM nodes are dirty. Gated by the generation-gated rebuild mechanism in `FusedWorkspace`.

### child_elements
**Type**: DOM traversal function
**Definition**: Returns an iterator over the element-type children of a DOM node. Skips `Text`, `Comment`, and `DocumentType` nodes.

### create_comment
**Type**: DOM allocation function
**Definition**: Allocates a new `Comment` node in the DOM tree. Returns a `NodeId`. Must be wired into the tree with `append_child` or `insert_before`.

### create_doctype
**Type**: DOM allocation function
**Definition**: Allocates a new `DocumentType` node (the `<!DOCTYPE>` declaration) in the DOM. Typically called by the HTML tree builder.

### create_document
**Type**: DOM allocation function
**Definition**: Allocates the root `Document` node for a new DOM tree. The starting point for any parse.

### create_element
**Type**: DOM allocation function
**Definition**: Allocates a new `Element` node with a given tag name in the default (HTML) namespace. Returns a `NodeId`.

### create_element_ns
**Type**: DOM allocation function
**Definition**: Allocates a new `Element` node in a specified XML namespace (SVG, MathML, etc.). Used by the HTML5 tree builder for foreign content.

### CssError
**Type**: Error type
**Definition**: Error returned by CSS tokenizer, parser, and cascade operations. Variants cover `UnexpectedToken`, `InvalidValue`, `UnsupportedSelector`, and `Io`. Converts to `SilkError::Css` at workspace boundaries.

### CssToken
**Type**: Lexical unit
**Definition**: Single token produced by the CSS tokenizer. Variants: `Ident`, `Function`, `AtKeyword`, `String`, `Url`, `Delim`, `Number`, `Percentage`, `Dimension`, `Whitespace`, `Comment`, `CDO`, `CDC`, `Colon`, `Semicolon`, `Comma`, `OpenBrace`, `CloseBrace`, `OpenParen`, `CloseParen`, `OpenBracket`, `CloseBracket`, `EOF`.

### CssTokenizer
**Type**: Streaming lexer
**Definition**: Tokenizes CSS source text into a sequence of `CssToken`s following the CSS Syntax Level 3 specification. Advances by calling `next_token()`.

### DisplayItem
**Type**: Render command
**Definition**: A single draw command in a display list. Variants: `Rect` (filled rectangle for background or border), `Text` (positioned text run), `Image` (decoded image at a rect). Consumed by the rasterizer.

### DisplayListTiles
**Type**: Tiled display list partition
**Definition**: A `DisplayList` that has been subdivided into 64x64 pixel tiles for parallel rasterization. Built by `DisplayList::with_tiles(width, height, 64)`. Each tile holds a clipped subset of `DisplayItem`s.

### DomError
**Type**: Error type
**Definition**: Error returned by DOM tree manipulation operations (node-not-found, invalid parent, detached node, etc.). Converts to `SilkError::Html` at workspace boundaries.

### EdgeSizes
**Type**: Layout geometry type
**Definition**: Four-sided inset values `{ top, right, bottom, left }` used to represent margin, padding, and border widths in the layout engine. Values are floating-point pixels in the viewport coordinate space.

### element_name
**Type**: DOM query function
**Definition**: Returns the tag name string of an element node (e.g., `"div"`, `"span"`). Returns `None` for non-element nodes.

### EngineError
**Type**: Error type
**Definition**: Error type returned by the engine-level fused pipeline. Wraps `SilkError` with additional pipeline context (which stage failed).

### EnginePipeline
**Type**: Fused pipeline type
**Definition**: The integrated style+layout+paint pipeline in `silksurf-engine`. Accepts a `&Dom` and `&Stylesheet`; returns `FusedOutput` (styles, layout geometry, display list). See `fused_pipeline.rs`.

### fetch_parallel
**Type**: HTTP client function
**Definition**: Issues multiple HTTP requests concurrently, optionally multiplexed over a single HTTP/2 connection when requests share the same host. Returns results in request order.

### fixed_from_f32
**Type**: Layout unit conversion
**Definition**: Converts an `f32` pixel value to a fixed-point layout unit (26.6 or similar sub-pixel format). Used at the boundary between CSS length computation and the layout engine.

### fixed_to_f32
**Type**: Layout unit conversion
**Definition**: Converts a fixed-point layout unit back to `f32` pixels. Used when passing layout geometry to the rasterizer.

### HttpMethod
**Type**: HTTP vocabulary enum
**Definition**: Enum of HTTP request methods: `Get`, `Post`, `Put`, `Delete`, `Head`, `Options`, `Patch`. Used in `HttpRequest`.

### HttpRequest
**Type**: HTTP request structure
**Definition**: Typed HTTP request: method (`HttpMethod`), URL string, header map, optional body bytes. Consumed by the network client.

### HttpResponse
**Type**: HTTP response structure
**Definition**: Typed HTTP response: status code (u16), header map, body bytes (`Vec<u8>`). Produced by the network client after a successful fetch.

### insert_before
**Type**: DOM mutation function
**Definition**: Inserts a node into the DOM tree immediately before an existing sibling node. Must be called inside a `with_mutation_batch`.

### LayoutBox
**Type**: Layout tree node
**Definition**: A single positioned rectangular box in the layout tree. Carries a reference to its DOM `NodeId`, the computed `Rect` (position and size), and the box type (block, inline, text). Linked into `LayoutTree`.

### LayoutTree
**Type**: Layout output structure
**Definition**: The full tree of `LayoutBox` nodes produced by the layout engine. Root corresponds to the document node. Used by `build_display_list` to generate render commands.

### length_to_px
**Type**: CSS unit conversion function
**Definition**: Converts a CSS length value in any unit (em, rem, px, %, vh, vw, pt, etc.) to an absolute pixel float given the current font metrics and viewport dimensions.

### linear_to_srgb
**Type**: Color conversion function
**Definition**: Converts a linear-light float in [0, 1] to an sRGB-encoded u8 [0, 255] using the IEC 61966-2-1 transfer function. Inverse of `srgb_to_linear`. Applied when writing composited pixels back to the framebuffer.

### linebreak_opportunities
**Type**: Unicode text function
**Definition**: Returns the set of Unicode line-break opportunity positions in a text run according to UAX #14. Used by the inline layout engine to determine where to wrap text.

### materialize_resolve_table
**Type**: DOM phase-boundary function
**Definition**: Snapshots the `SilkInterner` into the lock-free monotonic resolve table (`Vec<SmallString>`). Must be called after every `into_dom()` and every `end_mutation_batch()`. Without it, `resolve_fast` panics. See silksurf-dom OPERATIONS.md.

### Namespace
**Type**: XML/HTML namespace type
**Definition**: Identifies the namespace of a DOM element or attribute: `Html`, `Svg`, `MathMl`, `Xml`, or `XmlNs`. Used by `create_element_ns` and the tree builder's foreign-content handling.

### NetError
**Type**: Error type
**Definition**: Error returned by network fetch, connection, DNS resolution, and TLS operations. Variants cover `Io`, `Tls`, `Http`, `Timeout`, `DnsResolution`. Converts to `SilkError::Net`.

### new_h2_with_extra_ca_file
**Type**: HTTP/2 client constructor
**Definition**: Constructs an HTTP/2 client that appends a PEM CA bundle from a file to the default Mozilla trust store. For private PKI environments.

### new_insecure
**Type**: HTTP client constructor
**Definition**: Constructs an HTTP/1.1 client with TLS certificate verification disabled. Intended for local development and testing only.

### new_insecure_h2
**Type**: HTTP/2 client constructor
**Definition**: Constructs an HTTP/2 client with TLS certificate verification disabled. Intended for local development and testing only.

### new_platform_verifier
**Type**: HTTP client constructor
**Definition**: Constructs an HTTP/1.1 client that delegates certificate verification to the OS trust store (rustls-platform-verifier). Required by the `platform-verifier` feature flag.

### new_platform_verifier_h2
**Type**: HTTP/2 client constructor
**Definition**: HTTP/2 variant of `new_platform_verifier`.

### new_platform_verifier_h2_with_extra_ca_file
**Type**: HTTP/2 client constructor
**Definition**: HTTP/2 client using the OS trust store plus an additional PEM CA bundle.

### new_platform_verifier_with_extra_ca_file
**Type**: HTTP client constructor
**Definition**: HTTP/1.1 client using the OS trust store plus an additional PEM CA bundle.

### new_with_extra_ca_file
**Type**: HTTP client constructor
**Definition**: Constructs an HTTP/1.1 client that appends a PEM CA bundle from a file to the default Mozilla trust store. Accepts path forms `--tls-ca-file /path` and `--tls-ca-file=/path`.

### next_sibling
**Type**: DOM traversal function
**Definition**: Returns the `NodeId` of the next sibling in the DOM tree, or `None` if this node is the last child.

### NodeKind
**Type**: DOM discriminant enum
**Definition**: Discriminates DOM node types: `Element { tag, attrs }`, `Text { text }`, `Comment { data }`, `Document`, `DocumentType { name, public_id, system_id }`. Returned by `Dom::node(id)`.

### ParsedDocument
**Type**: HTML parse output
**Definition**: Output of `silksurf_engine::parse_html()`: the `Dom` tree plus the `NodeId` of the root document node. The dom has `resolve_table` already materialized.

### previous_sibling
**Type**: DOM traversal function
**Definition**: Returns the `NodeId` of the preceding sibling in the DOM tree, or `None` if this node is the first child.

### rasterize_damage
**Type**: Rasterizer function
**Definition**: Rasterizes only the tiles of a `DisplayListTiles` that intersect a given damage `Rect`. Skips tiles outside the damage area. Used for incremental screen updates.

### rasterize_parallel
**Type**: Rasterizer function
**Definition**: Rasterizes all tiles in a `DisplayListTiles` in parallel using Rayon. Returns a newly allocated `Vec<u8>` of `width * height * 4` bytes (ARGB). See silksurf-render OPERATIONS.md.

### rasterize_parallel_into
**Type**: Rasterizer function
**Definition**: Rasterizes all tiles into a caller-supplied `Vec<u8>`. Avoids the 4 MB allocation on subsequent frames by reusing the same buffer. Preferred over `rasterize_parallel` in interactive render loops.

### rasterize_skia
**Type**: Rasterizer function
**Definition**: Rasterizes a `DisplayList` into a newly-allocated RGBA `Vec<u8>` using tiny-skia for anti-aliased path rendering and cosmic-text for glyph shaping. Returns `width * height * 4` bytes.

### rasterize_skia_into
**Type**: Rasterizer function
**Definition**: Rasterizes a `DisplayList` into a caller-supplied `Vec<u8>` using tiny-skia. Reuses the buffer across frames to avoid repeated large allocations. Preferred over `rasterize_skia` in render loops.

### remove_child
**Type**: DOM mutation function
**Definition**: Detaches a child node from its parent in the DOM tree. The node is retained in the arena; call within a `with_mutation_batch` for correct generation tracking.

### render_document
**Type**: Engine pipeline function
**Definition**: Runs the full fused style+layout+paint+raster pipeline on a `Dom`. Returns `RenderOutput` with the pixel buffer and layout metrics.

### render_document_incremental
**Type**: Engine pipeline function
**Definition**: Re-renders only the dirty subtree of a DOM after a mutation batch. Uses `Dom::generation()` and the dirty-node set to skip unchanged subtrees.

### render_document_incremental_from_dom
**Type**: Engine pipeline function
**Definition**: Incremental render starting from a pre-built `Dom` reference rather than a URL. Used when the caller already holds the parsed DOM (e.g., after a revalidation diff).

### RenderOutput
**Type**: Engine output structure
**Definition**: Output of a render pass: `pixels: Vec<u8>` (ARGB framebuffer, `width * height * 4` bytes), `layout: LayoutTree`, `display_list: DisplayListTiles`, `styled_count: usize`.

### root_store_diagnostics
**Type**: TLS diagnostic function
**Definition**: Inspects the rustls root certificate store and returns a `RootStoreDiagnostics` record. Used by `tls-probe` to report trust-store composition.

### RootStoreDiagnostics
**Type**: TLS diagnostic structure
**Definition**: Diagnostic record for the TLS root certificate store: `count` (number of trusted root CAs), `source` (Mozilla bundle, OS store, or extra file), `errors` (malformed certificates).

### RustlsProvider
**Type**: Cryptography provider enum
**Definition**: Selects the cryptography backend for rustls: `Ring` (default, Google ring library) or `AwsLcRs` (aws-lc-rs, FIPS-capable). Set at client construction time.

### set_attribute
**Type**: DOM mutation function
**Definition**: Sets or replaces an attribute on a DOM element node. Creates the attribute if absent; overwrites the value if present. Must be called inside a `with_mutation_batch`.

### srgb_to_linear
**Type**: Color conversion function
**Definition**: Converts an sRGB-encoded u8 [0, 255] to a linear-light f32 [0, 1] using the IEC 61966-2-1 transfer function. Applied before blending in the compositor.

### style_generation
**Type**: Cache coherence counter
**Definition**: Monotonic u64 counter embedded in the `Dom` generation word. Advances on every `end_mutation_batch()`. Downstream caches (CascadeView, FusedWorkspace) compare their stored generation against this value to detect staleness.

### take_dirty_nodes
**Type**: DOM change detection function
**Definition**: Returns the set of `NodeId`s marked dirty since the last call, and clears the set. Used by the incremental render pipeline to determine which subtrees to re-cascade.

### TextState
**Type**: Text rendering state structure
**Definition**: Holds the `FontSystem` and `SwashCache` used by cosmic-text for glyph shaping and rasterization. Stored as a `LazyLock<Mutex<TextState>>` and initialized once per process. Accessed only from the main render thread.

### TlsConfig
**Type**: TLS configuration structure
**Definition**: rustls `ClientConfig` wrapper: specifies cipher suites, certificate verifier (Mozilla bundle, OS store, or insecure), and ALPN protocol list (`["h2", "http/1.1"]` for H2-capable clients).

### TlsConfigError
**Type**: Error type
**Definition**: Error returned when constructing a `TlsConfig` fails: malformed CA file, missing platform verifier support, or invalid cipher suite selection.

### TokenizeError
**Type**: Error type
**Definition**: Error returned by the CSS or HTML tokenizer. Variants: `UnexpectedEof`, `InvalidUtf8`, `UnterminatedString`, `InvalidEscape`. Converts to `SilkError::Css` or `SilkError::Html`.

### unpremultiply
**Type**: Color conversion function
**Definition**: Reverses alpha-premultiplication on a linear-light pixel: divides each RGB channel by the alpha value. Applied after compositing, before writing back to the sRGB framebuffer. A no-op when alpha is 255 (fully opaque).

### with_interner_mut
**Type**: DOM accessor function
**Definition**: Acquires a mutable reference to the `Dom`'s `SilkInterner` within a closure. Safe to call at any time; the returned atoms are valid until the next `materialize_resolve_table()`. See silksurf-dom OPERATIONS.md.

### with_mutation_batch
**Type**: DOM mutation helper
**Definition**: Opens a mutation batch, runs the supplied closure with mutable DOM access, then calls `end_mutation_batch()`. Advances `dom.generation()` and flushes the dirty-node set at depth zero. Preferred over manual `begin_mutation_batch` / `end_mutation_batch` pairing.

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
