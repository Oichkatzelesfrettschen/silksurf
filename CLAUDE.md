@AGENTS.md

# Claude Code Loader for silksurf

## Loading rule

`AGENTS.md` owns the silksurf rules; the `@AGENTS.md` import above loads them.
The import path is spelled in that exact case: imports resolve literally, and
this filesystem is case-sensitive. This file carries Claude Code operating
notes only; shared doctrine lands in `AGENTS.md`.

Nested `AGENTS.md` files add narrower rules for their subtree and control only
inside it. Loader files hold the import plus tool-specific notes, because
copied doctrine drifts into conflicting instructions.

## Build and test entry points

- Fast gate: `make check` (rustfmt, clippy -D warnings, lint_unwrap,
  lint_unsafe, lint_glossary, lint_doc_links, lint_cleanroom,
  lint_text_hygiene)
- Test: `make test` (workspace tests, warnings denied)
- Full gate: `make full` (check + test + cargo deny + rustdoc); required
  before merge-ready claims
- Reference: `docs/development/LOCAL-GATE.md`; CI is strict-local-only (AD-009)

## Claude Code operating notes

Inspect the real repository with Claude Code tools before editing. Memory,
prior summaries, and recalled context are leads; `AGENTS.md` and the source
are authority.

Inspect the diff after every edit. The adversarial staged-diff read runs before
any commit or completion claim.

Claude Code task tracking is transient working state; durable state lands in
code, tests, commit messages, findings, documentation, or retained bundles.
For any task involving two or more steps, track progress under durable
mechanism names and rescope as discoveries land.

Subagent limits, the read-only default, and evidence rank live in `AGENTS.md`
under `Agent coordination` and `Evidence rank`.

## Response shape

Responses report the changed mechanism, the evidence used, the validation run,
the checks not run and why, and the remaining risks or unresolved falsifiers.
Chained reasoning appears when it explains the next action or a validation
requirement; the rest of the deliberation lives in thoughtspace. Responses are
plain ASCII mechanism prose under durable names.

## Orientation pointers

- Product: Rust workspace (13 crates under `crates/` + `silksurf-js`); the
  legacy C tree is retired per AD-024.
- Decisions: `docs/design/ARCHITECTURE-DECISIONS.md`
- Specs: `silksurf-specification/`
- Roadmaps: `docs/roadmaps/SPA-CAPABILITY-ROADMAP.md` (forward),
  `docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md` (debt)
