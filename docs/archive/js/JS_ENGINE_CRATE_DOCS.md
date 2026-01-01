# JS Crate Docs Highlights (Perf)

This summary captures the key performance-relevant points from docs.rs for
JS-critical crates.

## bumpalo
- Bump allocation is fast but deallocates en masse; ideal for phase-based data
  (parse/compile)
- `bumpalo::boxed::Box<T>` can run `Drop` without deallocating the arena
- `collections` feature provides arena-backed `Vec`/`String`

## lasso
- `Rodeo` for single-threaded interning; can freeze into `RodeoReader` or
  `RodeoResolver` for contention-free resolution
- Multi-threaded interning requires `multi-threaded` feature

## memchr
- SIMD-optimized byte search (`memchr`, `memchr2/3`, `memmem::Finder`)
- Best throughput for scanning; use `memmem::Finder` when reusing needles

## bytemuck
- Safe casting between plain data types with `NoUninit`/`AnyBitPattern`
- Derive `Pod`/`Zeroable` for nanbox/bytecode payloads
- `must_cast` feature can enforce static safety at compile time

## zerocopy
- Derivable traits: `TryFromBytes`, `FromBytes`, `IntoBytes`, `FromZeros`
- Marker traits: `KnownLayout`, `Immutable`, `Unaligned`
- `transmute_*` macros enforce compile-time size/alignment checks

## rkyv
- Zero-copy serialization with optional validation; supports in-place mutation
- Archived collections (`ArchivedHashMap`, `ArchivedBTreeMap`) built for speed
- Feature flags control format and validation overhead; choose explicitly

## phf
- Compile-time perfect hash maps/sets; good for keyword lookup tables
- Macro-based `phf_map!` or build-time `phf_codegen`

## bitvec
- Compact, bit-addressed storage; good for GC marking bitmaps
- Safe aliasing model avoids common bitfield races

## unchecked-index
- Wrapper for unchecked indexing with debug assertions
- Useful for hot loops when bounds checks dominate
