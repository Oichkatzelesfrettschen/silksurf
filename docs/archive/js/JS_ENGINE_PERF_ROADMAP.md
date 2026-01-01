# JS Engine Performance Roadmap (Draft)

This roadmap is JS-first and intentionally granular. It stays cleanroom
compliant (no code copying), and focuses on cycle reduction, cache locality,
and memory footprint.

## Phase 0: Wiring + Hygiene (build correctness)
1. DONE: Gate `console_error_panic_hook` under `wasm` feature.
2. DONE: `cfg(feature = "fast-alloc")` global allocator (`mimalloc`).
3. DONE: `cfg(feature = "mmap")` bytecode cache hooks (`memmap2`).
4. DROPPED: `cfg(feature = "parallel")` multi-file lexing pass (`rayon` removed).
5. DONE: Gate `tracing` + `tracing-subscriber` under `tracing-full`.
6. DONE: Optional TUI/graphics deps removed; revisit only with explicit UI scope.
7. TODO: Add a `features.md` doc explaining feature → code path wiring.

## Phase 1: Lexer/Parser Hot Paths
8. Lexing: use `memchr` / `memchr3` for delimiter scans and skip loops.
9. Lexing: replace per-char branching with fast ASCII table classification.
10. Token buffers: switch to arena-backed `bumpalo::collections::Vec`.
11. Token strings: intern identifiers via `lasso` (key-based tokens).
12. Parser: replace `Box` AST allocation with bump arena nodes.
13. DEFERRED: `tinyvec` removed; revisit with arena-backed small lists if needed.
14. Parser: reduce `String` clones; pass slices where possible.

## Phase 2: Bytecode + VM Dispatch
15. Instruction storage: use `bytemuck`/`zerocopy` for packed instruction words.
16. Opcode decode: precompute opcode metadata tables (size, stack effect).
17. Stack/register indexing: introduce `unchecked-index` for hot loops.
18. Value representation: validate NaN-boxing invariants with `static_assertions`.
19. Inline caches: tighten IC layout, prefer SoA for hot fields.

## Phase 3: GC + Interning
20. Mark bits: adopt `bitvec` for compact mark/gray sets.
21. Heap blocks: align object headers to cache lines; pack header fields.
22. Interning: freeze `Rodeo` → `RodeoResolver` after parse for fast resolve.
23. String storage: ensure SSO or inline string fast path; avoid rehashing.
24. Weak refs: batch finalization queues; reduce churn in `Vec` growth.

## Phase 4: Optional JIT + FFI
25. JIT: reduce IR allocations; reuse `cranelift` contexts between functions.
26. JIT: cache compiled stubs; persist with `rkyv` where safe.
27. FFI: use `bytemuck` for POD structs; avoid copy in FFI boundary.

## Phase 5: Bench + Regression Harness
28. Add criterion benches for lexer/parser/VM/GC microbenchmarks.
29. Add callgrind/iai baselines for opcode dispatch + GC mark.
30. Add perf CI thresholds for regressions (time + alloc counts).
31. Add allocator profiling (jemalloc/mimalloc stats) to perf reports.

## Phase 6: Standards + Stability
32. Expand spec tests (test262 subset) gated by perf budget.
33. Run fuzzing passes on lexer/parser with sanitized builds.
34. Validate determinism across build profiles.
35. Document cleanroom sources for each module in `docs/`.
