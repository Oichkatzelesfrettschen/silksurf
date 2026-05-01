# silksurf-dom

Cleanroom DOM data structures and traversal APIs. Owns the node arena,
the per-DOM string interner, and the mutation-batch accounting that
drives the lock-free monotonic resolve table.

## Public API (high-level)

  * `Dom` -- top-level container. Owns `Vec<Node>`,
    `RwLock<SilkInterner>`, `resolve_table: Vec<SmallString>`, dirty-
    node tracking, mutation-batch depth, and the 64-bit `generation`
    counter.
  * `Node`, `NodeId`, `NodeKind`, `AttributeName`, `Attribute`.
  * `DomError` -- crate-local error; `From<DomError> for
    silksurf_core::SilkError` at the bottom of `lib.rs`.
  * `diff` module -- DOM-diff for incremental re-render.

## Key invariants

  * **Atoms are interner-bound.** Never share an `Atom` between two
    `Dom` instances; `Dom::resolve_fast` indexes the per-DOM
    `resolve_table` directly.
  * **Generation counter.** `Dom::generation()` = (instance_id << 32) |
    mutation_counter. The fused pipeline gates rebuilds on this; bump
    via `end_mutation_batch()` (or `materialize_resolve_table`).
  * **Resolve-table monotonicity.** Old atoms never move; new atoms
    extend the end. `resolve_fast(atom)` is a plain array index, no
    locks. The interner write path keeps the RwLock; the cascade read
    path is lock-free.
  * **`should_intern_identifier`** gates which strings get atoms. See
    `silksurf_core::should_intern_identifier`.

## Status

Stable. Recent work: persistent on-disk response cache integration
(via `silksurf-net`), generation-gated rebuild support for the fused
pipeline.
