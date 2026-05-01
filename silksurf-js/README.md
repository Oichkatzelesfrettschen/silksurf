# silksurf-js

Pure-Rust JavaScript engine. Cleanroom implementation: zero-copy lexer,
arena-based allocation, generational GC with reference counting for
cycles, register-based bytecode VM, C FFI for embedding outside the
silksurf workspace.

## Why outside crates/

silksurf-js is a workspace member at the repo root (sibling of
`crates/`), not under `crates/`. This is deliberate: the JS engine is a
self-contained subsystem that compiles standalone to a `.so` for FFI
embedding outside the engine. It has its own clippy.toml and rustfmt.toml.
See `docs/REPO-LAYOUT.md`.

## Public API (high-level)

  * `Engine` -- the top-level VM handle.
  * `Value` -- NaN-boxed 64-bit value.
  * `Lexer`, `Parser`, `Compiler`, `Vm` -- the four pipeline stages.
  * `gc::Heap` -- generational GC (mark + sweep).
  * `ffi` -- the C ABI surface for embedding (functions defined in
    `src/ffi.rs`; export prefix `silksurf_js_*`).

## Bins

  * `test262` -- partial Test262 harness (in-crate; full conformance
    runner is queued in SNAZZY-WAFFLE roadmap P5.S1).

## Status

Foundational. Compiler implements most ES2015+ syntax incl. spread,
template literals, destructuring patterns. VM dispatch covers the
common-case ops. Major gaps:

  * `try/catch/finally` opcodes -- queued in P7.S1.
  * `async/await` execution -- queued in P7.S2 (microtask queue
    skeleton present, dispatch not wired).
  * Generators / yield -- not started.
  * `Proxy`, `Reflect`, `WeakMap`, `WeakSet` -- not started.
  * `Promise.all` / `Promise.race` -- not started.
  * `fetch` from JS -- queued in P7.S4.

## Known issues

  * **FFI bug at `src/ffi.rs:271`**: `unwrap()` inside `unsafe {
    CStr::from_ptr(version) }.to_str()` panics across the FFI boundary
    on non-UTF-8 input. Tracked in the silksurf-js unwrap/unsafe
    follow-up batch (~118 unwrap + ~40 unsafe sites total).
  * `lint_unwrap.sh` and `lint_unsafe.sh` currently EXCLUDE silksurf-js;
    the dedicated annotation batch is queued.

## Bench

```sh
cargo bench -p silksurf-js --bench interner
cargo bench -p silksurf-js --bench lexer_throughput
cargo bench -p silksurf-js --bench vm_throughput
```

## Features

  * `tracing-full` -- enables `tracing` + `tracing-subscriber`
    structured logging (default off). Workspace-wide observability
    adoption is queued in P8.S6.
