# CLAUDE.md - Claude Code Entry Points

@AGENTS.md is the authoritative governance file: hard rules, engineering
posture, commit and PR doctrine, comment style, Rust workflow, and analysis
tooling all live there. This file carries only Claude-Code-specific entry
points and session habits.

## Build and Test Entry Points

- **Fast gate**: `make check` (rustfmt, clippy -D warnings, lint_unwrap,
  lint_unsafe, lint_glossary, lint_doc_links, lint_cleanroom, lint_ascii)
- **Test**: `make test` (workspace tests, warnings denied)
- **Full gate**: `make full` (check + test + cargo deny + rustdoc); required
  before merge-ready claims
- **Reference**: `docs/development/LOCAL-GATE.md`; CI is strict-local-only
  (AD-009)

## Session Habits (Claude-specific)

- For any task involving 2+ steps: create a task list with descriptive
  mechanism names (no session-local codes), track progress, and rescope as
  discoveries land.
- Report checks not run as `not run` with the reason.

## Orientation Pointers

- **Product**: Rust workspace (13 crates under `crates/` + `silksurf-js`);
  the legacy C tree is retired per AD-024.
- **Decisions**: `docs/design/ARCHITECTURE-DECISIONS.md`
- **Specs**: `silksurf-specification/`
- **Roadmaps**: `docs/roadmaps/SPA-CAPABILITY-ROADMAP.md` (forward),
  `docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md` (debt)
