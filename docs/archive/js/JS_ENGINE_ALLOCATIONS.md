# JS Allocation Marker Scan (Heuristic)

Counts of common allocation markers per file.

- src/parser/parser.rs: total=84 (Vec::new=14, Vec::with_capacity=5, Box::new=65)
- src/bin/test262.rs: total=9 (String::new=2, to_string=7)
- src/bytecode/chunk.rs: total=8 (Vec::new=3, String::new=1, to_string=4)
- src/bytecode/compiler.rs: total=8 (Vec::new=8)
- src/jit/compiler.rs: total=8 (to_string=8)
- src/vm/string.rs: total=8 (Vec::new=1, Vec::with_capacity=1, to_string=4, Box::new=2)
- src/gc/heap.rs: total=5 (Vec::new=3, Vec::with_capacity=2)
- src/vm/mod.rs: total=5 (Vec::new=2, Vec::with_capacity=1, to_string=2)
- src/gc/weakref.rs: total=4 (Vec::new=1, Vec::with_capacity=1, to_string=2)
- src/vm/gc_integration.rs: total=4 (Vec::new=2, to_string=2)
- src/vm/ic.rs: total=4 (Vec::new=1, Vec::with_capacity=3)
- src/napi.rs: total=2 (to_string=2)
- src/ffi.rs: total=2 (Box::new=2)
- src/vm/shape.rs: total=2 (Vec::new=1, Vec::with_capacity=1)
- src/parser/ast_arena.rs: total=2 (Vec::new=1, Vec::with_capacity=1)
- src/wasm.rs: total=1 (to_string=1)
- src/bytecode/instruction.rs: total=1 (Vec::with_capacity=1)
- src/gc/arena.rs: total=1 (bumpalo=1)
- src/gc/trace.rs: total=1 (Vec::new=1)
- src/jit/code_cache.rs: total=1 (Vec::new=1)

## Allocator + GC Strategy Notes
- Keep arena allocations (`gc::Arena`) scoped per parse/compile pass; reset between scripts to cap RSS.
- Enable `mimalloc` only when needed for host integration; default to system allocator for lowest footprint.
- Prioritize zero-copy bytecode (`rkyv`, `zerocopy`) and reuse `Vec` buffers in `vm`/`bytecode` hot paths.
- Treat snapshot mmap as optional (`feature = "mmap"`) to avoid mapping overhead on constrained runs.
