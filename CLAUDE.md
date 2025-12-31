# CLAUDE.md - Engineering Standards & No Shortcuts Policy

## CORE PRINCIPLES
1.  **NO SHORTCUTS**: Every implementation decision must be a genuine solution, not a workaround.
2.  **CLEANROOM**: Strict separation between reference analysis (`diff-analysis/`) and specification/implementation (`silksurf-specification/`, `src/`).
3.  **DOCUMENT FIRST**: Update specifications before writing code. Explain WHY, then WHAT, then HOW.
4.  **QUALITY GATES**: 0 warnings, 0 memory leaks, 100% test passing.

## NO SHORTCUTS POLICY (Mandatory)
required behaviors:
-   **RESCOPE**: When blocked, reframe the problem. Don't hack around it.
-   **RESEARCH**: Investigate root causes; read docs; trace code. Don't guess.
-   **SANITY CHECK**: Verify alignment with architecture and performance targets.
-   **ASK**: When unsure, get clarification. Don't assume.
-   **DOCUMENT**: Explain the rationale.
-   **BUILD OUT**: Implement full solutions. No "TODO implement later" for core logic. Partial fixes accumulate debt.

## TASK PLANNING WITH TODOWRITE
For any task involving 2+ steps:
1.  Create a checklist.
2.  Track progress.
3.  Update the plan as you discover new information.

## MEMORY & CONTEXT
-   **Status**: Phase 3 (Parallel Implementation)
-   **Architecture**: Frozen (see `PHASE-2-COMPLETION-SUMMARY.md`)
-   **Specs**: Located in `silksurf-specification/`

## BUILD & TEST
-   **Build**: `cmake -B build && cmake --build build`
-   **Test**: `ctest --test-dir build`
-   **Lint**: Ensure no warnings are introduced.
