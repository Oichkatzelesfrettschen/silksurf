# SilkSurf Documentation Index

**Last Updated**: 2026-01-29
**Status**: Phase 3 Week 1-2 Complete (14/34 tasks done, 75% test pass rate)

---

## Quick Navigation

**New to SilkSurf?** → README.md → BUILD.md → GLOSSARY.md
**Building?** → docs/development/BUILD.md
**Implementing?** → docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md
**Understanding Decisions?** → docs/design/ARCHITECTURE-DECISIONS.md
**Contributing?** → CLAUDE.md (NO SHORTCUTS policy)

---

## Project Root Files

### Essential Documentation

**📄 README.md**
- **Purpose**: Project overview and current status
- **Status**: Phase 3 (Parallel Implementation - In Progress)
- **Progress**: 75% test pass rate, core rendering pipeline established
- **Audience**: Everyone

**📄 CLAUDE.md**
- **Purpose**: Engineering standards and policies (applies globally)
- **Key Sections**:
  - NO SHORTCUTS POLICY (mandatory)
  - TASK PLANNING WITH TODOWRITE (required for 2+ steps)
  - Quality gates and tests
  - Git workflow
- **Audience**: All contributors
- **Status**: Authoritative source of standards

**📄 DOCUMENTATION-INDEX.md** (this file)
- **Purpose**: Master navigation for all documentation
- **Audience**: Everyone looking for information

---

## Documentation Structure

```
silksurf/
├── README.md                          # Project overview
├── CLAUDE.md                          # Engineering standards
├── DOCUMENTATION-INDEX.md             # This file
├── CMakeLists.txt                     # Build configuration
│
├── docs/
│   ├── development/                   # Build & development guides
│   │   ├── BUILD.md                   # 📘 Build instructions & troubleshooting
│   │   ├── AGENTS.md                  # AI assistant coordination
│   │   └── AI-ASSISTANTS.md           # AI tool usage guide
│   │
│   ├── design/                        # Architecture & decisions
│   │   └── ARCHITECTURE-DECISIONS.md  # 📘 7 key ADRs (cleanroom, Rust+C, XCB, etc.)
│   │
│   ├── reference/                     # Technical reference
│   │   └── GLOSSARY.md                # 📘 Complete technical glossary
│   │
│   ├── roadmaps/                      # Planning & status
│   │   ├── PHASE-3-IMPLEMENTATION-ROADMAP.md  # 📘 Current roadmap (12 weeks)
│   │   └── PHASE-2-COMPLETION-SUMMARY.md      # Phase 2 results
│   │
│   └── archive/                       # Historical documentation
│       ├── DEPRECATED.md              # Archive explanation
│       ├── roadmaps/                  # Old planning docs
│       │   ├── PHASE-3-5-MILESTONES.md
│       │   ├── PHASE-3-SCOPE-AND-READINESS.md
│       │   └── WEEK-1-PLAN.md
│       └── legacy/
│           └── SILKSURF_MASTER_PLAN_v1.md
│
├── silksurf-specification/            # Frozen specifications (6500+ lines)
│   ├── SILKSURF-JS-DESIGN.md          # JavaScript engine (Rust)
│   ├── SILKSURF-C-CORE-DESIGN.md      # C rendering core (1400 lines)
│   ├── SILKSURF-XCB-GUI-DESIGN.md     # XCB GUI layer (1200 lines)
│   ├── SILKSURF-NEURAL-INTEGRATION.md # BPE + LSTM optimization
│   ├── SILKSURF-BUILD-SYSTEM-DESIGN.md # CMake + Cargo integration
│   └── archive/
│       └── rust-stubs/                # Future Rust tokenizers
│
├── diff-analysis/                     # Research & cleanroom artifacts
│   ├── PHASE-0-COMPLETE.md            # Test262 baseline validation
│   ├── PROJECT-STATUS.md              # Browser archaeology (Phase 1)
│   ├── CLEANROOM-PROGRESS.md          # Cleanroom strategy validation
│   └── ... (25+ analysis documents)
│
├── src/                               # Implementation
├── tests/                             # Test suite
└── build/                             # Build artifacts
```

---

## Documentation by Task

### Building SilkSurf

1. **Install Dependencies**: `docs/development/BUILD.md` → System Requirements
2. **Configure Build**: `CMakeLists.txt` (CMake + Cargo)
3. **Build**: `docs/development/BUILD.md` → Quick Start
4. **Test**: `docs/development/BUILD.md` → Testing
5. **Troubleshooting**: `docs/development/BUILD.md` → Troubleshooting

**Expected**: 3/4 tests passing, builds in ~2 minutes

### Understanding Architecture

1. **High-Level**: `README.md` → Vision & What's Complete
2. **Key Decisions**: `docs/design/ARCHITECTURE-DECISIONS.md` (7 ADRs)
3. **Detailed Specs**: `silksurf-specification/*.md` (6500+ lines)
4. **Terms**: `docs/reference/GLOSSARY.md`

**Key ADRs**:
- AD-001: Cleanroom Implementation
- AD-002: Hybrid Rust + C Architecture
- AD-003: Pure XCB GUI (No GTK)
- AD-004: Arena Allocator for DOM/Layout
- AD-005: Test262 95%+ Compliance Target
- AD-006: Neural Integration (BPE + LSTM)
- AD-007: Damage Tracking for Rendering

### Contributing Code

1. **Read Standards**: `CLAUDE.md` (NO SHORTCUTS policy)
2. **Check Roadmap**: `docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md`
3. **Build**: `docs/development/BUILD.md`
4. **Implement**: Follow specifications in `silksurf-specification/`
5. **Test**: Run ctest, memory check with Valgrind
6. **Commit**: Follow git workflow in `CLAUDE.md`

**Quality Gates**:
- ✅ 0 compiler warnings (with -Werror)
- ✅ All tests pass
- ✅ 0 memory leaks
- ✅ Code follows specifications

### Reviewing Code

1. **Standards**: `CLAUDE.md` → Quality gates
2. **Specs**: Relevant `silksurf-specification/*.md` file
3. **Roadmap**: `docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md` → Acceptance criteria
4. **Tests**: Verify test coverage and pass rate

---

## Content by Audience

### For New Contributors

**Start here**:
1. `README.md` - Project vision
2. `docs/development/BUILD.md` - Get building in <10 minutes
3. `docs/reference/GLOSSARY.md` - Learn the terminology
4. `CLAUDE.md` - Understand standards

### For Implementers

**Rust JavaScript Engine Team**:
- Spec: `silksurf-specification/SILKSURF-JS-DESIGN.md` (1500 lines)
- Status: Planned (Phase 3 Week 9-10)
- Roadmap: `docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md`

**C Core Team** (current focus):
- Spec: `silksurf-specification/SILKSURF-C-CORE-DESIGN.md` (1400 lines)
- Status: In progress (Week 1-2 complete, Week 3-4 current)
- Current Tasks: CSS cascade completion, layout engine
- Roadmap: `docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md`

**Graphics/GUI Team**:
- Spec: `silksurf-specification/SILKSURF-XCB-GUI-DESIGN.md` (1200 lines)
- Status: Partial (basic XCB setup, double-buffer pending)
- Roadmap: `docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md`

**Build/DevOps Team**:
- Spec: `silksurf-specification/SILKSURF-BUILD-SYSTEM-DESIGN.md`
- Guide: `docs/development/BUILD.md`
- Status: CMake functional, Rust FFI incomplete (Task #33)

### For Architects

**Decision Making**:
1. `docs/design/ARCHITECTURE-DECISIONS.md` - Rationale for key choices
2. `silksurf-specification/SILKSURF-C-CORE-DESIGN.md` - Core architecture
3. `CLAUDE.md` - Engineering principles

**Research Context**:
1. `diff-analysis/PROJECT-STATUS.md` - Browser archaeology
2. `diff-analysis/CLEANROOM-PROGRESS.md` - Cleanroom validation
3. `docs/roadmaps/PHASE-2-COMPLETION-SUMMARY.md` - Phase 2 results

### For Project Managers

**Status**:
1. `README.md` - Current phase and progress
2. `docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md` - Timeline & milestones
3. Task list: 14/34 completed (41%)

**Metrics**:
- Tests: 3/4 passing (75%)
- Warnings: 0 (100% compliance)
- Memory leaks: 0
- Phase: Week 1-2 of 12

---

## Frozen Specifications (No Changes Without Review)

**Status**: Architecture Freeze (2025-12-31)

These specifications are **frozen** - changes require design review:

1. **SILKSURF-JS-DESIGN.md** (1500 lines)
   - JavaScript engine: lexer, parser, bytecode, VM, GC
   - Target: 95%+ Test262 compliance

2. **SILKSURF-C-CORE-DESIGN.md** (1400 lines)
   - HTML/CSS parsing, DOM, Layout, Rendering
   - Current implementation base

3. **SILKSURF-XCB-GUI-DESIGN.md** (1200 lines)
   - Window management, widgets, events, damage tracking

4. **SILKSURF-NEURAL-INTEGRATION.md** (700 lines)
   - BPE optimization, LSTM token prediction

5. **SILKSURF-BUILD-SYSTEM-DESIGN.md** (600 lines)
   - CMake architecture, Rust FFI, CI/CD

**Total**: 6500+ lines of frozen architecture

---

## Active Documents (Updated Regularly)

**Living Roadmaps**:
- `docs/roadmaps/PHASE-3-IMPLEMENTATION-ROADMAP.md` - Weekly updates
- `README.md` - Phase status updates

**Always Current**:
- `CLAUDE.md` - Engineering standards
- `DOCUMENTATION-INDEX.md` - This navigation file

---

## Archived Documentation

See `docs/archive/DEPRECATED.md` for explanation of archived content.

**Archived Roadmaps** (superseded by Phase 3 Implementation Roadmap):
- `docs/archive/roadmaps/PHASE-3-5-MILESTONES.md` (416 lines)
- `docs/archive/roadmaps/PHASE-3-SCOPE-AND-READINESS.md` (756 lines)
- `docs/archive/roadmaps/WEEK-1-PLAN.md` (36 lines)

**Archived Specs** (Rust tokenizers - future work):
- `silksurf-specification/archive/rust-stubs/SILKSURF-CSS-DESIGN.md`
- `silksurf-specification/archive/rust-stubs/SILKSURF-HTML-DESIGN.md`
- `silksurf-specification/archive/rust-stubs/SILKSURF-DEPENDENCY-STRATEGY.md`

**Legacy Plans**:
- `docs/archive/legacy/SILKSURF_MASTER_PLAN_v1.md`

---

## Key Metrics & Targets

### Current Status (Week 1-2)
- ✅ Tests: 3/4 passing (75%)
- ✅ Compiler: 0 warnings with -Werror
- ✅ Memory: 0 leaks (Valgrind verified)
- ✅ Segfaults: 0 (was 1, now fixed)
- 🚧 CSS Cascade: 90% complete

### Phase 3 Targets (Week 12)
- Tests: 100% passing
- Test262: 95%+ compliance
- Layout: 60 FPS
- Rendering: 100+ FPS (damage-tracked)
- Memory: <10MB per tab
- Startup: <500ms

---

## Anti-Patterns to Avoid

❌ **Don't**: Copy code from diff-analysis/ (reference only)
✅ **Do**: Implement from specifications

❌ **Don't**: Skip NO SHORTCUTS policy in CLAUDE.md
✅ **Do**: Read and follow policy for every task

❌ **Don't**: Make architectural changes without updating specs
✅ **Do**: Specs first, implementation second

❌ **Don't**: Leave TODOs or placeholders in core logic
✅ **Do**: Complete implementation fully or document as future work

❌ **Don't**: Commit with warnings
✅ **Do**: Build with -Werror, fix all warnings

---

## Next Steps

### Immediate (Week 3-4)
- [ ] Complete CSS cascade algorithm (Task #22)
- [ ] Implement CSS selector callbacks (Task #27)
- [ ] Achieve 100% test pass rate

### Short Term (Week 5-8)
- [ ] Layout engine (box model, block/inline) (Tasks #25, #21)
- [ ] Rendering pipeline integration (Task #29)
- [ ] Add sanitizer builds (Task #2)

### Medium Term (Week 9-12)
- [ ] JavaScript engine core (Rust)
- [ ] Full pipeline integration (HTML → JS → render)
- [ ] Performance optimization
- [ ] CI/CD automation (Task #34)

---

## Getting Help

**Questions About**:
- **Building**: See `docs/development/BUILD.md` troubleshooting section
- **Architecture**: See `docs/design/ARCHITECTURE-DECISIONS.md`
- **Terms**: See `docs/reference/GLOSSARY.md`
- **Standards**: See `CLAUDE.md`
- **Implementation**: See relevant spec in `silksurf-specification/`

**File Issues**: https://github.com/your-org/silksurf/issues

---

## Document Maintenance

**Update Triggers**:
- Phase transition → Update README.md, roadmaps
- New documentation → Update this index
- Architecture decision → Update ARCHITECTURE-DECISIONS.md
- Task completion → Update roadmap

**Last Updated**: 2026-01-29
**Version**: 2.0
**Status**: Phase 3 Week 1-2 Complete
