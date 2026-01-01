# Cleanroom Sources and Distillation

These repositories are reference-only. Do not copy code; only distill concepts
into SilkSurf-owned specs and tests.

## Reference Sources (Local Checkouts)
- `silksurf-extras/Amaya-Editor`: layout/editor UI ideas and rendering behaviors.
- `silksurf-extras/boa`: JS engine architecture patterns.
- `silksurf-extras/servo`: HTML/CSS/DOM/layout architecture patterns.
- `silksurf-js/test262`: JavaScript conformance tests.

## Distillation Process
1. Read source material and write a cleanroom spec in `silksurf-specification/`.
2. Record the problem statement, invariants, and expected behaviors.
3. Derive tests from the spec (no code reuse) and add them to Rust crates.
4. Implement the Rust module guided only by the spec + tests.

## Guardrails
- No source files from reference repos are included in the Rust workspace.
- Keep research notes separate from implementation code.
- Record dependency choices and rationale in `docs/DEPENDENCY_RATIONALE.md`.
- Log each intake in `docs/CLEANROOM_INTAKE_LOG.md`.
