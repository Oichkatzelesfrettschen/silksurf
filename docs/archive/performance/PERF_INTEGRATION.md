# Performance Integration Plan

This document scopes how `silksurf-core` (arena + interner) will be wired into
DOM/CSS/layout for measurable speedups without breaking cleanroom boundaries.

## Goals
- Reduce string allocations for tag/class/attribute names.
- Speed up selector matching and style lookup.
- Keep DOM and CSS data structures cache-friendly.

## Implemented/Planned Changes
1. Tag/attribute enums + `SmallString` storage for DOM identifiers (implemented).
2. Selective interning for high-frequency values (id/class) (implemented).
3. Selector matching now uses enum comparisons and interner fast paths (implemented).
4. Layout boxes allocated in `SilkArena` with lifetime-aware APIs (implemented).
5. Fixed-point cursor for inline flow to reduce rounding drift (implemented).
6. DOM mutation batching feeds dirty-node lists into incremental style/layout (implemented).
7. HTML tokenizer uses delimiter-first scans + memchr fast paths (implemented).

## Follow-ups
- Consider compact storage for class tokens (smallvec/tinyvec).
- Evaluate enum expansion for more HTML tags/attributes.
- Remove remaining text clones in display list rendering.

## Performance Tooling (Commands)
- `cargo run -p silksurf-engine --bin bench_pipeline`: runs the end-to-end
  pipeline benchmark; prints timing summary in stdout.
- `cargo run -p silksurf-css --bin bench_css`: runs CSS parsing/cascade timing;
  prints iterations/throughput in stdout.
- `cargo flamegraph -p silksurf-engine --bin bench_pipeline`: generates a
  `flamegraph.svg` for CPU hot paths (requires `cargo-flamegraph`).
- `cargo valgrind run -p silksurf-engine --bin bench_pipeline`: runs under
  Valgrind; produces `target/valgrind/` logs (requires `cargo-valgrind`).
- `cargo bloat -p silksurf-engine --release`: shows top code-size offenders
  (requires `cargo-bloat`).
- `cargo llvm-lines -p silksurf-engine`: reports line-level code size
  (requires `cargo-llvm-lines`).

## Dependencies
- `silksurf-core` provides `SilkInterner` and `SilkArena`.
- `silksurf-css`/`silksurf-dom` will depend on `silksurf-core`.

## Risks
- API churn across crates; plan for staged refactors and tests.
- Must keep public APIs stable for harnesses and test fixtures.
