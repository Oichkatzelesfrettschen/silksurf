# silksurf-dom OPERATIONS

## Runtime tunables

No environment variables are consumed at runtime. All configuration is via code.

## Key invariants

- `Dom::materialize_resolve_table()` must be called after every parse and after every `end_mutation_batch()` flush. Without it, `resolve_fast()` panics on atoms created after the last materialization.
- `dom.generation()` increments on every `end_mutation_batch()` call. Callers that cache styles against the DOM use this to detect staleness.
- `batch_depth` must reach 0 before any cascade or layout pass; nested batches are supported but the flush only runs at depth 0.

## Common failure modes

### `resolve_fast` panics with index out of bounds

Cause: an `Atom` was created after the last `materialize_resolve_table()` call.

Fix: ensure `materialize_resolve_table()` is called at the documented boundaries:
- After `silksurf-html::into_dom()` (tree builder calls it automatically).
- After every `Dom::end_mutation_batch()` (called automatically by `with_mutation_batch`).
- If atoms are interned directly via `dom.with_interner_mut(|i| i.intern(...))`, call `materialize_resolve_table()` immediately after.

### Generation counter not advancing

Cause: mutations are made outside a `begin_mutation_batch` / `end_mutation_batch` pair, so the generation is not incremented.

Fix: wrap all DOM writes in `dom.with_mutation_batch(|dom| { ... })`. Mutations outside a batch are legal but do not update the generation, so downstream caches (FusedWorkspace, CascadeView) may serve stale data.

### DOM tree inconsistent (orphaned nodes)

Cause: `remove_child` called without a matching `append_child` path for the detached subtree.

Fix: use `with_mutation_batch` to ensure `flush_dirty_batch` sees the full set of changes atomically.

## DoS bounds

| Bound | Enforced by |
|---|---|
| Node Vec grows without limit (no explicit cap) | Parser-level `MAX_TOKENS_PER_FEED` in silksurf-html prevents runaway growth |
| resolve_table grows to match interner | Same bound as interner atom count; no explicit limit |

For deep-nest protection, rely on the HTML tokenizer depth limit.
