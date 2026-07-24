# Documentation Index

This directory is the canonical source of current project design, build,
security, testing, and cleanroom guidance. Historical material lives under
`docs/archive/` and must not override current code, scorecards, or this index.

## Current status and execution

- `docs/STATUS.md`: canonical current-state and evidence-scope summary.
- `docs/roadmaps/BROWSER-FUNCTIONALIZATION-ACTION-PLAN.md`: functional browser,
  engine isolation/backend, and native-chat program (tracked by issue #50).
- `docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md`: detailed debt inventory.
- `docs/roadmaps/SPA-CAPABILITY-ROADMAP.md`: native-engine SPA capability work.

## Core architecture

- `docs/ARCHITECTURE.md`: crate ownership, current process topology, data flow,
  and the target shell/engine boundary.
- `docs/JS_ENGINE.md`: production Boa integration, host surface, limitations,
  conformance, and performance scope.
- `docs/design/ARCHITECTURE-DECISIONS.md`: ADR ledger.
- `docs/design/THREAT-MODEL.md`: current security boundaries and gaps.

## Performance and tooling

- `docs/PERFORMANCE.md`: hot paths, baselines, guardrails, and retained findings.
- `docs/TOOLCHAIN.md`: toolchain pin, build/test commands, and tooling.
- `docs/development/LOCAL-GATE.md`: authoritative local merge gate.

## Governance and cleanroom

- `docs/CLEANROOM.md`: cleanroom policy, sources, and reference rules.
- `docs/DEPENDENCIES.md`: dependency rationale and utilization map.
- `docs/SECURITY.md`: security posture and reporting links.
- `docs/LOGGING.md`: logging expectations and diagnostics.
- `docs/TESTING.md`: testing strategy, conformance inputs, and fuzzing.

## Historical archive

See `docs/archive/README.md` for superseded phase plans, legacy C material, and
raw audits. Archived status statements are historical evidence, not current
project claims.
