# SilkSurf Dependency Strategy (Rust)

## Intent
- Align workspace dependencies with `silksurf-js` for shared primitives.
- Keep cleanroom implementation; dependencies are tools, not engines.
- Prefer small, focused crates with clear performance profiles.

## Workspace Alignment
Use `workspace = true` in crate `Cargo.toml` for shared crates:
- `bumpalo`: arena allocation for AST/DOM/layout scratch memory.
- `lasso`: string interning for tag names, attributes, identifiers.
- `memchr`: SIMD byte scanning in tokenizers.
- `bytemuck`, `zerocopy`, `rkyv`: zero-copy serialization where safe.
- `bitflags`, `bitvec`: compact state and feature flags.
- `parking_lot`, `once_cell`: lightweight synchronization.
- `anyhow`, `thiserror`: error surfaces with context.
- `tracing`: structured diagnostics and performance spans.

## Design Guidelines
- HTML/CSS tokenizers should be byte-first and avoid regex.
- DOM and layout should use arena-backed allocations where possible.
- Use interning for identifiers that are compared frequently.
- Keep optional crates behind feature flags until needed.

## Cleanroom Guardrails
- No code reuse from reference repos.
- Specs must justify dependency usage with a performance/correctness reason.
