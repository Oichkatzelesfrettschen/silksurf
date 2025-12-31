# Repository Guidelines

## Project Structure & Module Organization
- `Cargo.toml`: Rust workspace root.
- `crates/`: Rust implementation crates for engine subsystems.
- `silksurf-js/`: Rust JS engine crate.
- `silksurf-specification/`: cleanroom specs and migration plans.
- `src/`, `include/`, `tests/`, `CMakeLists.txt`, `Makefile`: legacy C sources (migration-only).
- `docs/`, `diff-analysis/`, `silksurf-extras/`: research, analysis, and vendor/reference material.

## Build, Test, and Development Commands
- `cargo build`: build the Rust workspace.
- `cargo test`: run Rust tests across crates.
- `cargo run -p silksurf-app`: run the Rust binary.
- `cargo test -p silksurf-js`: run JS engine tests.
- `cargo bench -p silksurf-js`: run JS engine benchmarks (if enabled).
- `cmake -B build && cmake --build build`: legacy C build (migration-only).
- `ctest --test-dir build`: legacy C tests.
- `make build` / `make clean`: legacy Makefile wrapper.
- `make gui`, `make fuzz-build`, `make fuzz-run`: legacy GUI and AFL++ workflows.

## Coding Style & Naming Conventions
- C11, 4-space indentation, braces on the same line.
- Functions/types use `silk_*` and `silk_*_t`; macros/constants use `SILK_*`.
- Header guards use `SILKSURF_*_H`; keep includes sorted with local headers from `include/silksurf/`.
- Rust code uses `rustfmt` (`cargo fmt`) and `clippy` (`cargo clippy`) with warnings treated as errors where practical.

## Testing Guidelines
- Prefer Rust tests in each crate (`cargo test`); add integration tests under each crate's `tests/` folder.
- Keep tests deterministic; prefer small HTML/CSS fixtures checked into the crate test dirs.
- Run `cargo test` (and optionally `cargo nextest run`) before PRs.
- Legacy C tests live in `tests/` and run via `ctest --test-dir build`.

## Commit & Pull Request Guidelines
- Git history is not available in this workspace; no repo-specific commit convention found.
- Use a concise, imperative subject (e.g., `css: fix cascade order`) and add a brief body if needed.
- PRs should include: summary of changes, tests run (with commands), and screenshots for GUI/rendering changes. Link related issues when available.

## Documentation & Cleanroom Policy
- Keep research in `diff-analysis/`, specs in `silksurf-specification/`, and implementation in `crates/` + `silksurf-js/`.
- Update specs/docs before large code changes and explain the why/what/how (see `CLAUDE.md`).
- Treat `silksurf-extras/` and `silksurf-js/test262/` as local, untracked reference checkouts.
