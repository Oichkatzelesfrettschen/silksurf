# SilkSurf Cleanroom Specification

**Purpose**: Technical specifications produced by cleanroom synthesis of
reference material (`../diff-analysis/`, `../silksurf-extras/`). They
define implementation targets from first principles, never copies of
existing code.

**Governance**: These documents are design inputs. Binding decisions live
in `../docs/design/ARCHITECTURE-DECISIONS.md` (AD-001..AD-026); where a
spec and an ADR disagree, the ADR governs. The C-core, XCB-GUI, and
CMake build specs describe the original hybrid design whose C side is
superseded (AD-002 -> AD-024); the concepts they specify (tokenizer state
machines, cascade, damage tracking, box model) are implemented by the
Rust workspace crates.

## Cleanroom boundary

This folder contains ONLY architecture and design decisions, algorithm
specifications, data-structure layouts, interface contracts, and
performance targets with acceptance criteria. It does NOT contain code
from reference browsers (that analysis lives in `../diff-analysis/`),
dependencies on specific browser implementations, or reverse-engineered
implementation details. Production code never imports from
`../diff-analysis/` (see `../docs/CLEANROOM.md`).

## Specification documents

- **SILKSURF-JS-DESIGN.md** -- lexer, parser, bytecode VM, GC, FFI,
  test262 roadmap. The hand-written VM this spec produced is gated
  behind the silksurf-js `legacy-vm` feature; production execution
  delegates to boa_engine (L7).
- **SILKSURF-C-CORE-DESIGN.md** -- HTML5 tokenizer, CSS engine, DOM,
  layout, rendering. Historical: the owning crates are silksurf-html,
  -css, -dom, -layout, -render (`../docs/LEGACY_C_PORTING.md`).
- **SILKSURF-XCB-GUI-DESIGN.md** -- windowing, double-buffering,
  damage tracking. Implemented by silksurf-gui (XCB and winit backends;
  AD-003, AD-010).
- **SILKSURF-NEURAL-INTEGRATION.md** -- BPE vocabulary, parser
  prediction (AD-006, Experimental). The BPE tokenizer lives at
  `silksurf_core::bpe::BpeTokenizer`.
- **SILKSURF-BUILD-SYSTEM-DESIGN.md** -- historical CMake architecture;
  the canonical build is the root Makefile wrapping Cargo
  (`../docs/development/LOCAL-GATE.md`).
- **formal/** -- TLA+ models (`resolve_table.tla`, `cache_coherence.tla`,
  `BrowserLoader.tla`) with TLC configurations and checked invariants.

Consolidated reference-analysis findings -- reference implementations
studied, baselines, and formal-verification strategies -- live in the
reference tree governed by `../docs/CLEANROOM.md`. They are inputs the
cleanroom study drew on, not a dependency of these specs: the boundary is
one-directional, so the specification names the reference tree but does
not point into a document inside it.

## Cleanroom principles

1. **Study, don't copy**: reference materials are studied for concepts,
   patterns, and algorithms. No direct code reuse.
2. **Independent implementation**: specifications define the target from
   first principles, informed by cleanroom research but architecturally
   independent.
3. **Full solutions**: every specification includes complete algorithm
   pseudocode, data structures, and acceptance criteria.
4. **Documented tradeoffs**: decisions and rationale are recorded, in the
   spec or in an ADR.
5. **Verification ready**: all specs include measurable acceptance
   criteria -- throughput/latency/memory targets, conformance suites,
   warnings-as-errors, deterministic builds.
