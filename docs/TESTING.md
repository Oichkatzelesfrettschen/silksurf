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
- CSS: WPT subset planned (syntax/selectors/cascade first).
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
