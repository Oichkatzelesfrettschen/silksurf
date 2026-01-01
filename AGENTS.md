# Repository Guidelines

## Project Structure & Module Organization
- `crates/`: Rust subsystem crates (`silksurf-html`, `silksurf-css`, `silksurf-dom`, `silksurf-layout`, `silksurf-render`, `silksurf-engine`, `silksurf-net`, `silksurf-tls`, plus app/gui).
- `silksurf-js/`: Rust JS engine crate and harnesses.
- `src/`, `include/`, `tests/`, `CMakeLists.txt`, `Makefile`: legacy C sources (migration-only).
- `docs/`: canonical architecture, performance, tooling, cleanroom, and testing docs (see `docs/README.md`).
- `silksurf-specification/`, `diff-analysis/`: cleanroom specs and design research.
- `silksurf-extras/`, `silksurf-js/test262/`: local reference checkouts (untracked).
- `fuzz/`, `perf/`, `logs/`: fuzzing and performance artifacts.

## Build, Test, and Development Commands
- `cargo build`: build the Rust workspace.
- `cargo run -p silksurf-app`: run the end-to-end pipeline driver.
- `cargo test`: run all Rust unit/integration tests.
- `cargo test -p silksurf-js`: run JS engine tests and harnesses.
- `cargo run -p silksurf-engine --bin bench_pipeline`: run pipeline benchmark binary.
- `cargo run -p silksurf-css --bin bench_css`: run CSS benchmark binary.
- `cd fuzz && cargo fuzz run html_tokenizer`: run fuzzing (requires `cargo-fuzz`).
- `cmake -B build && cmake --build build`: legacy C build (migration-only).

## Coding Style & Naming Conventions
- Rust: run `cargo fmt` and `cargo clippy`; use idiomatic `snake_case` for functions/vars and `CamelCase` for types; keep crate APIs cohesive.
- C (legacy): C11, 4-space indentation, `silk_*` functions/typedefs, `SILK_*` macros, and `SILKSURF_*_H` header guards.

## Testing Guidelines
- Prefer tests alongside each crate (`crates/*/tests/` and module `#[cfg(test)]`).
- Keep fixtures minimal and deterministic; store small HTML/CSS snippets in crate test dirs.
- Run targeted tests during development (`cargo test -p silksurf-html`, `cargo test -p silksurf-css`) plus `cargo test` before PRs.

## Commit & Pull Request Guidelines
- Recent commits use short, imperative summaries (e.g., `Handle CSS url tokens`); add a body when behavior changes.
- PRs should include: summary of changes, tests run (with commands), and screenshots for rendering changes.
- Document dependency changes and cleanroom rationale in `docs/`.

## Cleanroom & Reference Policy
- Treat `silksurf-extras/` and `silksurf-js/test262/` as reference-only; do not copy code.
- Base implementations on specs and documented reasoning in `silksurf-specification/` and `docs/`.
