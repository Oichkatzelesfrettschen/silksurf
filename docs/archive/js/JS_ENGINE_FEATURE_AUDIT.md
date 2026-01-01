# JS Feature/Dependency Wiring Audit

## Feature Flags
cfg(feature=...) found in code: cli, fast-alloc, jit, mmap, napi, tracing-full, wasm
Declared but not referenced via cfg: constrained, default, full, neural

## Optional Dependencies (heuristic)
Optional deps wired and referenced in code: clap, console_error_panic_hook, cranelift-codegen, cranelift-frontend, cranelift-jit, cranelift-module, cranelift-native, memmap2, mimalloc, napi, napi-derive, tracing, tracing-subscriber, wasm-bindgen
Optional deps with no code usage yet: none

## Notes
- Usage detection is heuristic; derive/proc-macro usage is not always visible via `dep::`.
