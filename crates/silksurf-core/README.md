# silksurf-core

Foundation crate. Provides the workspace's canonical error type, atom
interner, arena allocator, and span/source-location primitive. Has zero
workspace-internal dependencies (only `bumpalo`, `smol_str`, `thiserror`)
so every other crate can depend on it without cycle risk.

## Public API

  * `SilkError`, `SilkResult<T>` -- workspace-wide canonical error
    (string-erased; per-crate errors implement `From<MyError> for
    SilkError` in their own crate). See
    `docs/reference/GLOSSARY.md` and `crates/silksurf-core/src/error.rs`.
  * `SilkInterner`, `Atom`, `should_intern_identifier` -- string
    interner used by HTML/CSS/DOM. `Atom::raw()` exposes the u32 index
    for the lock-free monotonic resolve table pattern (see GLOSSARY).
  * `SilkArena` (`bumpalo::Bump` newtype) -- bump arena for per-frame
    allocations.
  * `Span` -- byte-range source location, used by parser error
    reporting.
  * `SmallString = smol_str::SmolStr` -- workspace-wide short-string
    type alias.
  * `ArenaVec<'a, T>` -- `bumpalo::collections::Vec` alias for
    arena-allocated vectors.

## Conventions

  * Atoms are tied to the `SilkInterner` instance that created them.
    Sharing Atoms across interners is undefined; `resolve()` panics on
    out-of-range index (see `UNWRAP-OK` annotation at
    `interner.rs:53`).

## Status

Stable. The lock-free monotonic resolve table pattern (Phase 3) is
documented in `docs/reference/GLOSSARY.md` -> `Lock-free monotonic
resolve table`.
