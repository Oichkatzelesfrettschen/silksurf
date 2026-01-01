# JS Hot Path Map (Initial)

This map lists likely hot paths in `silksurf-js` based on code structure and
allocation markers (see `docs/JS_ENGINE_ALLOCATIONS.md`). It is a starting
point for profiling.

## Primary Hot Paths
- Lexer: `src/lexer/lexer.rs`
  - Uses `memchr` for SIMD scanning; main byte traversal loop.
- Parser: `src/parser/parser.rs`
  - High `Box::new` usage; candidate for arena allocation and AST pooling.
- VM Dispatch: `src/vm/mod.rs`, `src/vm/bytecode` paths
  - Opcode loop, nanbox conversions; sensitive to bounds checks and branch
    prediction.
- GC Mark/Sweep: `src/gc/heap.rs`, `src/gc/trace.rs`
  - Potential for `bitvec`/bitmap acceleration and tighter object layout.
- Interning: `src/lexer/interner.rs`
  - `lasso` usage; consider freezing to resolver for fast reads post-parse.

## Allocation Pressure Candidates
- AST nodes: `src/parser/parser.rs` (heavy `Box::new` usage)
- Bytecode chunks: `src/bytecode/chunk.rs`
- VM strings: `src/vm/string.rs`

## Profiling Targets
- `cargo bench -p silksurf-js --bench lexer_throughput`
- `cargo bench -p silksurf-js --bench vm_throughput`
- (future) `cargo flamegraph -p silksurf-js --bench lexer_throughput`
