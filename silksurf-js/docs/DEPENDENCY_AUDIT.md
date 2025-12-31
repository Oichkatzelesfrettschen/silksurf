# SilkSurfJS Dependency Audit

**Status**: Phase 10 Complete (2025-12-30)
**Tests**: 180 passing (with JIT feature)

## Current Dependencies

| Crate | Version | Usage | Status |
|-------|---------|-------|--------|
| bumpalo | 3.16 | Arena allocation | Active |
| bytemuck | 1.21 | NaN-boxed value transmutes | Active |
| lasso | 0.7 | String interning | Active |
| memchr | 2.7 | SIMD byte search | Active |
| rkyv | 0.8 | Zero-copy serialization | Active |
| unicode-xid | 0.2 | JS identifier validation | Active |
| libc | 0.2 | C FFI | Active |
| phf | 0.11 | O(1) keyword lookup | Active (Phase 10) |
| static_assertions | 1.1 | Compile-time size checks | Active (Phase 10) |
| bitvec | 1.0 | Efficient bit vectors | Ready (Phase 10) |
| zerocopy | 0.8 | Safe byte conversion | Active (Phase 10) |

## Optional Dependencies

| Crate | Feature | Usage | Status |
|-------|---------|-------|--------|
| mimalloc | fast-alloc | Fast global allocator | Ready |
| memmap2 | mmap | Memory-mapped bytecode | Ready |
| rayon | parallel | Multi-file processing | Ready |
| cranelift-* | jit | Native code generation | Active (18 tests) |
| wasm-bindgen | wasm | Browser/WASM support | Ready |
| napi | napi | Node.js bindings | Ready |

## Phase 10 Implementation Summary

### Implemented

| Crate | Implementation | Location |
|-------|----------------|----------|
| phf | Perfect hash keyword lookup (55 keywords) | `src/lexer/token.rs:427-483` |
| static_assertions | Instruction=4B, Opcode=1B, GcHeader=16B | `src/bytecode/*.rs`, `src/gc/heap.rs` |
| zerocopy | TryFromBytes/IntoBytes for Opcode enum | `src/bytecode/opcode.rs:19` |
| mimalloc | Feature `fast-alloc` | Cargo.toml feature gate |
| memmap2 | Feature `mmap` | Cargo.toml feature gate |
| rayon | Feature `parallel` | Cargo.toml feature gate |

### Ready for Future Use

| Crate | Prepared For | Notes |
|-------|--------------|-------|
| bitvec | GC marking optimization | Added to Cargo.toml, awaiting GC refactor |

### Not Needed (Corrected Assessment)

| Original Recommendation | Actual State |
|------------------------|--------------|
| GC uses Vec<bool> for marks | **Incorrect**: Uses inline `mark_bits: u8` in GcHeader |
| Value should be 64-bit | **Future**: Current Phase 4 Value is enum with Rc; NaN-boxing in Phase 5 |

## Implementation Details

### Opcode Safe Byte Conversion (opcode.rs)

```rust
// Phase 10: Using zerocopy for safe byte-to-enum conversion
#[derive(TryFromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(u8)]
pub enum Opcode { ... }

// Safe path using zerocopy validation
pub fn from_byte(byte: u8) -> Option<Self> {
    Self::try_read_from_bytes(&[byte]).ok()
}

// Fast path with optimized range checking (for hot paths)
pub fn from_byte_fast(byte: u8) -> Option<Self> {
    match byte {
        0x00..=0x09 | 0x10..=0x18 | ... => Some(unsafe { transmute(byte) }),
        _ => None,
    }
}
```

### Keyword Lookup (token.rs)

```rust
// Phase 10: O(1) perfect hash lookup (was 55-arm match)
static KEYWORDS: phf::Map<&'static str, KeywordId> = phf_map! {
    "await" => KeywordId::Await,
    "break" => KeywordId::Break,
    // ... 55 keywords
};

pub fn keyword_lookup(s: &str) -> Option<TokenKind<'static>> {
    KEYWORDS.get(s).map(|id| id.to_token_kind())
}
```

### Compile-Time Size Verification

```rust
// src/bytecode/opcode.rs
assert_eq_size!(Opcode, u8);
const_assert_eq!(std::mem::size_of::<Opcode>(), 1);

// src/bytecode/instruction.rs
assert_eq_size!(Instruction, u32);
const_assert_eq!(std::mem::size_of::<Instruction>(), 4);

// src/gc/heap.rs
const_assert_eq!(std::mem::size_of::<GcHeader>(), HEADER_SIZE); // 16 bytes
```

## Future Considerations

| Crate | Purpose | Priority | Notes |
|-------|---------|----------|-------|
| soa-derive | Struct-of-arrays for objects | Medium | Better cache locality |
| paste | Macro identifier concat | Low | Compile-time string concat |
| seq_macro | Sequential macros | Low | Loop unrolling |
| const_format | Compile-time formatting | Low | Static error messages |

## Synergy Analysis

### Active Synergies

1. **zerocopy + static_assertions**: Compile-time size verification ensures zerocopy derives work correctly
2. **phf + lasso**: phf provides O(1) keyword lookup, lasso provides O(1) identifier comparison
3. **rkyv + zerocopy**: Both enable zero-copy patterns for bytecode serialization
4. **bumpalo + memchr**: Arena allocation for AST, SIMD search for lexer

### Potential Synergies (Ready)

1. **bitvec + GC**: Replace Color enum tracking with dense bit vectors
2. **rayon + lexer**: Parallel multi-file lexing for large projects
3. **mimalloc + VM**: Fast allocation for hot object creation paths
4. **memmap2 + rkyv**: Memory-mapped bytecode cache files

## Related Documentation

- [SYNERGY_ANALYSIS.md](SYNERGY_ANALYSIS.md) - Detailed integration analysis
- [DEPENDENCY_GUIDE.md](DEPENDENCY_GUIDE.md) - Usage patterns for all dependencies

## Verification

```bash
# All tests pass
cargo test --features jit  # 180 passed

# Build succeeds with all optional features
cargo build --features "jit,fast-alloc,mmap,parallel"
```
