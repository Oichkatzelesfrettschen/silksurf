# silksurf-js Operations

## Resource bounds (P8.S8)

| Constant                   | Default  | Enforcement site                                | Failure mode                |
|----------------------------|----------|--------------------------------------------------|-----------------------------|
| `vm::MAX_CALL_STACK_DEPTH` | `10_000` | `op_call`, `op_spread_call` (frame push guards) | Returns `VmError::StackOverflow` |

Embedders that need a tighter bound can override the per-VM
`max_stack_depth` field via a follow-up patch (the field is currently
private; a setter is tracked for the next API window). The constant is
the documented default.

## Memory layout

* `CallFrame` is 32 B on 64-bit (chunk_idx + pc + base + return_reg + padding).
* At the default cap the call stack is bounded at ~320 KiB.
* `Vec<Value>` (registers) grows on demand inside `op_call`; each new
  frame reserves up to 256 register slots (`Value` is ~24-40 B).

## Forensics test primitives

The deterministic [`silksurf_core::testing::Clock`] and
[`silksurf_core::testing::Rng`] are the recommended sources of "now"
and randomness for any silksurf-js unit test. Do not call
`std::time::Instant::now()` or `rand::random()` from inside VM tests
-- they re-introduce wall-clock flake.

## Stack growth strategy

`vm.call_stack` is created with `Vec::with_capacity(64)` and grows
geometrically up to `MAX_CALL_STACK_DEPTH`. Once the cap is reached
the next `op_call` or `op_spread_call` returns `VmError::StackOverflow`
and the dispatch loop unwinds via the standard exception path
(`try_handlers`); uncaught overflows propagate as
`SilkError::JsRuntime`.
