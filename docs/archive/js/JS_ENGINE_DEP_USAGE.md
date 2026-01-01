# JS Dependency Macro/Feature Usage (Heuristic)

Scan for derive/macro use beyond `crate::` paths.

- bitvec: src/gc/heap.rs:61
- bumpalo: src/gc/arena.rs:9
- bytemuck: src/vm/nanbox.rs:31
- clap: src/bin/main.rs:6
- console_error_panic_hook: src/wasm.rs:40
- cranelift-codegen: src/jit/compiler.rs:7, src/jit/ir_builder.rs:5
- cranelift-frontend: src/jit/compiler.rs:10, src/jit/ir_builder.rs:7
- cranelift-jit: src/jit/compiler.rs:11
- cranelift-module: src/jit/compiler.rs:12, src/jit/ir_builder.rs:8
- cranelift-native: src/jit/compiler.rs:99
- lasso: src/lexer/interner.rs:6
- memchr: src/lexer/lexer.rs:16
- memmap2: src/vm/snapshot.rs:11
- mimalloc: src/lib.rs:23
- napi: src/napi.rs:24
- napi-derive: src/napi.rs:25
- phf: src/lexer/token.rs:8
- rkyv: src/bytecode/chunk.rs:12, src/bytecode/instruction.rs:31, src/vm/snapshot.rs:18
- static_assertions: src/bytecode/opcode.rs:6, src/bytecode/instruction.rs:7, src/gc/heap.rs:62, src/vm/nanbox.rs:32
- tracing: src/vm/mod.rs:211, src/lexer/lexer.rs:220
- tracing-subscriber: src/tracing_support.rs:5
- unicode-xid: src/lexer/lexer.rs:748
- wasm-bindgen: src/wasm.rs:19
- zerocopy: src/bytecode/opcode.rs:7, src/bytecode/instruction.rs:8
