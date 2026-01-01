# JS Runtime Integration

This document describes the integration surface between the Rust engine and the
JavaScript runtime in `silksurf-js`. The goal is to keep the runtime embeddable,
deterministic, and safe to drive from the engine.

## Engine-facing API
`crates/silksurf-engine/src/js.rs` defines the host interface:
- `JsRuntime::bind_dom(&Dom, NodeId)`: provide DOM access by handle.
- `JsRuntime::evaluate(&str) -> JsValue`: execute script source.
- `JsRuntime::enqueue_task(JsTask)`: schedule a task (script/microtask).
- `JsRuntime::run_microtasks()`: drain microtasks after script execution.
- `JsTask::Script(String)`: queued work item (placeholder until richer task
  kinds are defined).
- `JsValue`/`JsError`: host-visible value + error wrappers (minimal today).
- `NoopJsRuntime`: stub used by pipeline tests.

## Execution Model
- HTML/CSS parse is synchronous.
- JS runs after DOM construction (or per script tag), producing tasks.
- Tasks can mutate the DOM and trigger style/layout recomputation.
- The engine owns scheduling (future: timers, network callbacks, render ticks).
- DOM mutations should record dirty nodes and allow incremental style/layout
  recompute via `EnginePipeline::render_document_incremental_from_dom` (DOM
  batching) or `render_document_incremental` (explicit dirty list).

## Runtime Module Boundaries
`silksurf-js` is organized to keep hot paths tight and embeddable:
- `lexer/`: zero-copy tokenization (`Lexer`, `TokenKind`, `Span`).
- `parser/`: AST construction (`Program`, `Statement`, `Expression`).
- `bytecode/`: register-based instruction stream (`Chunk`, `Opcode`).
- `vm/`: execution engine + snapshots (`Vm`, `VmSnapshot`).
- `gc/`: arena + GC hooks (`Arena` and allocation strategy).
- `jit/`: optional Cranelift backend (`cfg(feature = "jit")`).
- `ffi/`, `napi/`, `wasm/`: host bindings (C FFI, Node, wasm).
- `verification/`: bytecode/VM validation helpers.

## JS Data Flow (Current)
Source bytes → `lexer` tokens → `parser` AST → `bytecode` chunk → `vm` execute
→ optional snapshot via `vm::snapshot` (mmap when enabled).

## Data Contracts
- `Dom` and `NodeId` are the only host handles; JS should not retain borrowed
  references beyond the host call boundary.
- The JS runtime should use arena allocation + interning for hot paths
  (`bumpalo`, `lasso`) and prefer ID-based host calls to avoid large copies.

## Integration Points (current)
- The core pipeline (`render`/`render_document`) is JS-agnostic today.
- Embedding points live in `crates/silksurf-engine/src/js.rs` and are used by
  the test stub; production runtime wiring is planned for the event loop.

## Host Bindings (planned)
- DOM bindings: query/select, create/append nodes, attributes.
- Console/logging: forwards to `tracing`.
- Fetch/timers: wired through `silksurf-net` and engine scheduler.

## Performance Notes
- The runtime should support arena allocation and zero-copy bytecode caching
  (via `bumpalo`, `rkyv`, `zerocopy` in `silksurf-js`).
- Keep the host API thin: pass IDs/handles rather than large structures.
