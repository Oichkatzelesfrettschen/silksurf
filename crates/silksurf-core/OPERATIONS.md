# silksurf-core OPERATIONS

## Runtime tunables

No environment variables are consumed at runtime.

## Key types for operators

- `SilkArena` -- bump allocator for short-lived allocations (parsing scratch space). Thread-local; not `Send`.
- `SilkInterner` -- string internment table; accessed via `Dom::with_interner_mut`. Atoms are stable as long as the `Dom` lives.
- `resolve_table` -- lock-free copy of the interner built by `Dom::materialize_resolve_table()`. `resolve_fast(atom)` reads from here; panics if called before materialization.
- `testing::Clock` / `testing::Rng` -- deterministic clock and seedable PRNG for reproducible tests. Not for production use.

## Common failure modes

### `resolve_fast` panics on atom created after last materialization

See silksurf-dom OPERATIONS.md. The resolution table is in the DOM; core exports the `Atom` type only.

### Arena overflow (stack overflow in bump path)

`SilkArena` panics if a single allocation exceeds its backing store. Increase the arena's initial capacity or switch to the heap for large allocations.

### `SilkError` variants

| Variant | Cause |
|---|---|
| `Css(CssError)` | CSS parse failure |
| `Html(TreeBuildError)` | HTML parse / tree-build failure |
| `Net(NetError)` | Network I/O failure |
| `Js(String)` | JavaScript runtime error |
| `Io(std::io::Error)` | File or pipe I/O |

All crate boundaries convert their domain error to `SilkError` via `From` impl. Callers at the engine boundary handle `SilkError` only.
