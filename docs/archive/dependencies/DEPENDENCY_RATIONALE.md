# Dependency Rationale

This document records third-party crates used by the Rust workspace and the
reasoning behind them. The goal is to keep the cleanroom boundary explicit and
avoid drifting into full engine reuse.

## Principles
- Prefer small, focused crates (parsing, encoding, data structures).
- Avoid embedding full engines or large frameworks.
- Record spec references and the concrete problem each crate solves.
- Keep licenses compatible (MIT/Apache-2.0) and document exceptions.

## Current Dependencies (selected)
- `rustls`: TLS implementation for `crates/silksurf-tls`.
- `tracing` / `tracing-subscriber`: structured logging.
- `serde` / `serde_json`: test fixtures and debug serialization.
- `memchr`, `phf`, `bitflags`: low-level utilities.

## Candidates to Evaluate (not adopted)
- `html5ever`, `markup5ever`, `tendril`: HTML tokenization helpers (evaluate
  cleanroom risk).
- `cssparser`, `selectors`: CSS syntax and selector helpers.
- `encoding_rs`: character encoding support.
- `unicode-segmentation`, `unicode-normalization`: text processing.

## Support Crates for HTML/CSS (Rationale)
Use these as helpers or references, not as full-engine imports.

### In use (current)
- `memchr`: SIMD-accelerated byte scanning in HTML/CSS tokenizers.
- `smol_str`: small-string storage for identifiers without heap churn.
- `phf`: planned for keyword tables (tags, attributes, CSS keywords).

### Reference or optional helpers
- `html5ever` / `markup5ever`: spec-aligned tokenization/tree building;
  useful as a behavior reference, not a drop-in engine.
- `tendril`: efficient byte/string buffers for parsing pipelines.
- `cssparser`: compact CSS token stream parsing; useful for edge-case handling.
- `selectors`: selector parsing/matching primitives to validate our behavior.
- `encoding_rs`: standards-compliant encoding tables for HTML input streams.

Use these only as optional helpers; the default path is cleanroom-owned logic
and tests in `crates/silksurf-html` and `crates/silksurf-css`.

## Performance Tooling
See `docs/RUST_TOOLING.md` for `cargo-valgrind`, `flamegraph`, `criterion`, and
`iai-callgrind` references.
