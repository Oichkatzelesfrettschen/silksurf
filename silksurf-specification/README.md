================================================================================
SILKSURF CLEANROOM SPECIFICATION
================================================================================

**Purpose**: Authoritative technical specification for the SilkSurf browser engine.
Built from cleanroom synthesis of reference material (located in `../diff-analysis/`
and `../silksurf-extras/`), these documents define the IMPLEMENTATION TARGET,
not direct copies of existing code.

**Cleanroom Boundary**: This folder contains ONLY:
- Architecture & design decisions
- Algorithm specifications
- Data structure layouts
- Interface contracts (C FFI, CMake targets)
- Performance targets & acceptance criteria

This folder does NOT contain:
- Direct code from reference browsers (see `../diff-analysis/`)
- Dependency on specific browser implementations
- Reverse-engineered implementation details

**References**: Material studied during cleanroom design process is documented
in `../diff-analysis/` (Phase 1 analysis) and `../PHASE-2-RESEARCH-SYNTHESIS.md`.

================================================================================
SPECIFICATION DOCUMENTS
================================================================================

### Core Engine

**SILKSURF-JS-DESIGN.md** (1500 lines)
- Lexer: Token recognition, BPE optimization, zero-copy design
- Parser: Recursive descent grammar, AST construction, error recovery
- Bytecode: Stack-based VM, 50+ instruction set, compilation strategy
- GC: Hybrid approach (arena + generational + reference counting)
- FFI: Safe C boundary, serialization, validation
- Test262: Compliance roadmap, phased approach, gap analysis

**SILKSURF-C-CORE-DESIGN.md** (1400 lines)
- HTML5 Tokenizer: State machine, BPE patterns, error modes
- CSS Engine: Cascade algorithm, specificity, media queries
- DOM Tree: Streaming construction, node types, traversal
- Layout: Box model, constraint resolution, inline/block/flex
- Rendering: Damage tracking, double-buffering, color management
- Acceleration: XShm extension, DRI3 preparation

**SILKSURF-XCB-GUI-DESIGN.md** (1200 lines)
- Window Management: XCB initialization, event loop, lifecycle
- Double-Buffering: Pixmap management, copy-on-write, blitting
- Widget System: Base class, standard widgets, event dispatch
- Damage Tracking: Incremental rendering, rect merging, optimization
- Acceleration: XShm 10x faster, DRI3/GPU (Phase 3+)

### Advanced Features

**SILKSURF-NEURAL-INTEGRATION.md** (700 lines)
- BPE Vocabulary: 256+ patterns per language (JS/HTML/CSS)
- Parser Prediction: LSTM model, quantized weights, training data
- Speculative Parsing: Pre-allocation, error hints, fallback recovery
- Performance: +5-8% speedup, <1MB model size, <1ms inference

**SILKSURF-BUILD-SYSTEM-DESIGN.md** (600 lines)
- CMake Architecture: Modular targets, feature flags
- Interface Selection: CLI, TUI, Curses, XCB (selectable)
- Rust FFI: Cargo integration, linking strategy, type safety
- Testing: Unit, integration, Test262, benchmarks
- CI/CD: GitHub Actions, ctest, coverage tracking

### Synthesis & Research

**PHASE-2-RESEARCH-SYNTHESIS.md** (1260 lines, in `../`)
- Consolidated research findings from 8 investigation areas
- Reference implementations analyzed (Boa, QuickJS, NetSurf, etc.)
- Performance baselines & improvement targets
- Formal verification strategies (TLA+, Z3 specs)
- 30+ authoritative 2025-current sources

================================================================================
CLEANROOM PRINCIPLES (From CLAUDE.md)
================================================================================

1. **Study, Don't Copy**: Reference materials (`diff-analysis/`, `silksurf-extras/`)
   are studied for CONCEPTS, PATTERNS, and ALGORITHMS. No direct code reuse.

2. **Independent Implementation**: Specifications in this folder define the target
   from first principles, informed by cleanroom research but architecturally
   independent.

3. **Full Solutions**: Every specification includes complete algorithm pseudocode,
   data structures, and acceptance criteria. No placeholders or "fill in later."

4. **Documented Tradeoffs**: When choosing between multiple valid approaches, the
   decision and rationale are documented (see each spec's intro sections).

5. **Verification Ready**: All specs include measurable acceptance criteria:
   - Performance: Throughput, latency, memory targets
   - Correctness: Test262 compliance, formal verification specs
   - Quality: No warnings, deterministic behavior, repeatable builds

================================================================================
IMPLEMENTATION WORKFLOW
================================================================================

**Phase 2 (Specification)** ← YOU ARE HERE
→ Generate authoritative specifications from research
→ Create CMake build system, organize folder structure

**Phase 3 (Implementation)**
→ Implement each module per specification
→ Verify against acceptance criteria
→ Run Test262 & benchmarks continuously
→ No deviations from spec without design review

**Phase 4 (Optimization)**
→ Profile real workloads (Top 1M websites)
→ Implement performance improvements (arena, SIMD, etc.)
→ Measure before/after (record baselines)
→ Formal verification (TLA+, Z3)

**Phase 5 (Production)**
→ Final hardening & security audit
→ Cross-platform support (macOS, Windows, BSD)
→ Deployment packaging

================================================================================
DIRECTORY STRUCTURE
================================================================================

```
/silksurf/
├── silksurf-specification/        ← YOU ARE HERE (Cleanroom specs)
│   ├── README.md                   (This file)
│   ├── SILKSURF-JS-DESIGN.md
│   ├── SILKSURF-C-CORE-DESIGN.md
│   ├── SILKSURF-XCB-GUI-DESIGN.md
│   ├── SILKSURF-NEURAL-INTEGRATION.md
│   └── SILKSURF-BUILD-SYSTEM-DESIGN.md
│
├── diff-analysis/                  ← Reference material (browser archaeology)
│   ├── PHASE-0-COMPLETE.md
│   ├── PROJECT-STATUS.md
│   ├── PHASE-2-RESEARCH-SYNTHESIS.md
│   └── tools-output/               (Analysis results)
│
├── silksurf-extras/                ← Reference implementations
│   ├── boa/                        (JS engine)
│   ├── quickjs/                    (JS engine)
│   ├── netsurf/                    (HTML/CSS/Layout)
│   └── ... (other references)
│
├── silksurf-js/                    ← Implementation (Rust JS engine)
│   ├── Cargo.toml
│   └── src/
│
├── silksurf-core/                  ← Implementation (C HTML5/CSS/DOM)
│   ├── CMakeLists.txt
│   └── src/
│
└── silksurf-gui/                   ← Implementation (XCB GUI)
    ├── CMakeLists.txt
    └── src/
```

================================================================================
NEXT STEPS
================================================================================

Phase 2 Remaining Tasks:
- [ ] Reconcile documentation (merge insights from all sources)
- [ ] Create Phase 2-3 implementation roadmap (acceptance criteria per task)
- [ ] Create Phase 3-5 milestone timeline (weekly sprints)
- [ ] Validate all specs against CLAUDE.md (no shortcuts, full solutions)
- [ ] Scope Phase 3 (architecture freeze, team assignments, CI/CD)

Each specification is COMPLETE and ready for implementation. No gaps or TODOs.

================================================================================
