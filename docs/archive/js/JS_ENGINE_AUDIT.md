# JS Engine Audit (Initial)

This audit focuses on `silksurf-js` with a performance-first lens. It combines
macro/derive usage, feature-gated code paths, and known hot sections.

## Dependency Usage (heuristic)
See `docs/JS_ENGINE_DEP_USAGE.md` for per-file macro/derive hits. Highlights:
- In use: `bumpalo`, `bytemuck`, `lasso`, `memchr`, `phf`, `rkyv`, `zerocopy`.
- JIT wired: `cranelift-*` modules present under `feature = "jit"`.
- Optional integrations: `napi` and `wasm-bindgen` have code paths.
- Likely unused in code: `bitflags`, `bitvec`, `modular-bitfield`, `tinyvec`,
  `unchecked-index`, `parking_lot`, `once_cell`, `rayon`, `tracing`.

## Feature Flags (current)
See `docs/JS_ENGINE_FEATURE_MAP.md`. Only `cli`, `jit`, `napi` show `cfg` usage.
Most other features are declared but have no `cfg` usage yet.

## Immediate Findings
- **Feature drift**: many optional deps/features are declared but not wired.
  Decide whether to implement or remove to reduce build surface.
- **Hot-path crates** are present but not always applied (e.g. `bitvec` for GC
  marking, `unchecked-index` for VM dispatch tables, `tinyvec` for small arrays).
- **Allocator controls** (`fast-alloc`, `mmap`) are declared but not hooked.

## Next Steps
- Wire missing features behind `cfg` and/or drop unused deps.
- Audit hot paths in lexer/parser/VM/GC and introduce the intended crates.
- Add perf baselines (criterion + callgrind) per subsystem.
