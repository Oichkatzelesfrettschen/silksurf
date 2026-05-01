# Testing Strategy

This document defines the current test surface and near-term compliance work.
Legacy, long-form test plans live in `docs/archive/testing/`.

## Core Tests (Rust)
- Unit/integration tests live in `crates/*/tests` and `#[cfg(test)]` modules.
- Targeted runs:
  - `cargo test -p silksurf-html`
  - `cargo test -p silksurf-css`
  - `cargo test -p silksurf-engine`

## Compliance Inputs
- HTML tokenizer: html5lib JSON tests (smoke harness wired).
- CSS: WPT-oriented external corpus harness in `crates/silksurf-css/tests/css_harness.rs`.
  - Run with corpus path: `CSS_TESTS_DIR=/abs/path/to/css-tests cargo test -p silksurf-css --test css_harness -- --nocapture`
  - Default expectations file: `${CSS_TESTS_DIR}/silksurf-css-harness.expectations`
  - Override expectations file: `CSS_TEST_EXPECTATIONS=/abs/path/to/expectations`
  - Optional corpus narrowing:
    - `CSS_TEST_INCLUDE=<wildcard>` include only matching relative paths
    - `CSS_TEST_EXCLUDE=<wildcard>` exclude matching relative paths
    - `CSS_TEST_MAX_FILES=<N>` cap selected files after include/exclude filtering
  - Broad external sweep (including `support/` and `resources/` trees): put `expected-pass *` in an override expectations file and pass it via `CSS_TEST_EXPECTATIONS`.
  - Expectations format per line: `expected-pass <pattern>`, `expected-fail <pattern>`, `skip <pattern>`
  - Built-in conventions: `/invalid/` or `*.invalid.css` => expected-fail, `/support/` or `/resources/` => skip (metadata overrides conventions)
  - Strict mode for unexpected passes: `CSS_HARNESS_FAIL_ON_XPASS=1`
  - The harness now parses stylesheet bytes directly, including BOM/`@charset` decoding for non-UTF-8 corpus files.
- JS: `silksurf-js/test262` subset (planned).

## Fuzzing
Targets are under `fuzz/`:
- `cargo fuzz run html_tokenizer`
- `cargo fuzz run html_tree_builder`
- `cargo fuzz run css_tokenizer`
- `cargo fuzz run css_parser`
- `cargo fuzz run js_runtime`

## Warnings-as-Errors
Rust code is treated with `-D warnings`. Track exceptions in
`docs/archive/testing/WARNINGS_AUDIT.md`.

## Local Gate Policy (Primary)
Routine gating is local-first/local-only.

Use:
- `make local-gate-fast`
- `make local-gate-full`

Cloud workflow is manual-only (`workflow_dispatch`) and intentionally not used for routine gating.
