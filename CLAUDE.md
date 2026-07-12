# CLAUDE.md - Engineering Standards & No Shortcuts Policy

`AGENTS.md` at the repo root is the authoritative instruction file; this
file carries the standing policy summary and the build/test entry points.

## CORE PRINCIPLES
1.  **NO SHORTCUTS**: Every implementation decision must be a genuine solution, not a workaround.
2.  **CLEANROOM**: Strict separation between reference analysis (`diff-analysis/`) and specification/implementation (`silksurf-specification/`, `crates/*/src`, `silksurf-js/src`). Production code never imports from `diff-analysis/`.
3.  **DOCUMENT FIRST**: Update specifications and ADRs before writing code. State the mechanism, then the consequence, then the evidence.
4.  **QUALITY GATES**: 0 warnings (RUSTFLAGS='-D warnings' everywhere), 0 memory leaks, 100% test passing.

## NO SHORTCUTS POLICY (Mandatory)
required behaviors:
-   **RESCOPE**: When blocked, reframe the problem. Don't hack around it.
-   **RESEARCH**: Investigate root causes; read docs; trace code. Don't guess.
-   **SANITY CHECK**: Verify alignment with architecture and performance targets.
-   **ASK**: When unsure, get clarification. Don't assume.
-   **DOCUMENT**: Explain the rationale.
-   **BUILD OUT**: Implement full solutions. No "TODO implement later" for core logic. Partial fixes accumulate debt.

## TASK PLANNING
For any task involving 2+ steps:
1.  Create a checklist with descriptive mechanism names (no session-local codes).
2.  Track progress.
3.  Update the plan as you discover new information.

## MEMORY & CONTEXT
-   **Product**: Rust workspace (13 crates under `crates/` + `silksurf-js`); the legacy C tree under `src/` is retired per AD-024 and removed incrementally.
-   **Decisions**: `docs/design/ARCHITECTURE-DECISIONS.md` (AD-001..AD-024).
-   **Specs**: `silksurf-specification/`.
-   **Debt plan**: `docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md`.

## BUILD & TEST
-   **Fast gate**: `make check` (rustfmt, clippy -D warnings, lint_unwrap, lint_unsafe, lint_glossary, lint_doc_links, lint_cleanroom)
-   **Test**: `make test` (workspace tests, warnings denied)
-   **Full gate**: `make full` (check + test + cargo deny + rustdoc); required before merge-ready claims
-   **Reference**: `docs/development/LOCAL-GATE.md`; CI is strict-local-only (AD-009)
