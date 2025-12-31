# SilkSurfJS Dependency Synergy Analysis

**Last Updated**: 2025-12-30
**Phase**: 10 (Zero-Copy & Compile-Time Optimization)
**Tests**: 180 passing (with JIT feature)

## Overview

This document analyzes how dependencies in SilkSurfJS work together to achieve
performance and safety goals. Dependencies are organized into synergy groups
that complement each other.

## Synergy Groups

### Group 1: Zero-Copy Memory Access

**Dependencies**: `zerocopy`, `bytemuck`, `rkyv`

**Purpose**: Enable direct memory access without deserialization overhead.

| Crate | Role | Use Case |
|-------|------|----------|
| zerocopy | Safe byte-to-type conversion with validation | Opcode decoding, Instruction parsing |
| bytemuck | Zero-cost transmutes for Pod types | NaN-boxed values (all bit patterns valid) |
| rkyv | Zero-copy archive serialization | Bytecode chunk caching |

**Synergy Pattern**:
```
Raw Bytes --> zerocopy (validate) --> Structured Types --> rkyv (serialize)
         \--> bytemuck (transmute) --> Pod Types --------/
```

**Implementation Locations**:
- `src/bytecode/opcode.rs:270-272` - zerocopy TryFromBytes for Opcode
- `src/bytecode/instruction.rs:160-174` - zerocopy FromBytes for Instruction
- `src/vm/nanbox.rs:74-77` - bytemuck Pod/Zeroable for NanBoxedValue
- `src/bytecode/chunk.rs` - rkyv Archive for Chunk

**Decision Matrix**:
| Scenario | Use |
|----------|-----|
| Type has invalid bit patterns | zerocopy (TryFromBytes) |
| All bit patterns are valid | bytemuck (Pod) |
| Need serialization to disk/network | rkyv (Archive) |
| Need both validation and serialization | zerocopy + rkyv |

### Group 2: Compile-Time Verification

**Dependencies**: `static_assertions`, `zerocopy` (KnownLayout)

**Purpose**: Catch size/layout bugs at compile time, not runtime.

**Implementation Locations**:
- `src/bytecode/opcode.rs:261-262` - Opcode = 1 byte
- `src/bytecode/instruction.rs:36-37` - Instruction = 4 bytes
- `src/gc/heap.rs` - GcHeader = 16 bytes
- `src/vm/nanbox.rs:80-81` - NanBoxedValue = 8 bytes

**Pattern**:
```rust
use static_assertions::{assert_eq_size, const_assert_eq};

// Ensure struct matches expected primitive size
assert_eq_size!(Instruction, u32);
const_assert_eq!(std::mem::size_of::<Instruction>(), 4);
```

**Why Both Macros**:
- `assert_eq_size!` gives clearer error messages when types differ
- `const_assert_eq!` works with computed expressions

### Group 3: O(1) Lookup Structures

**Dependencies**: `phf`, `lasso`

**Purpose**: Constant-time lookups for hot paths.

| Crate | Role | Implementation |
|-------|------|----------------|
| phf | Perfect hash keyword lookup | `src/lexer/token.rs:427-483` (55 keywords) |
| lasso | Interned string comparison | `src/lexer/interner.rs` |

**Synergy Pattern**:
```
Source Text --> phf (keyword?) --> TokenKind::Keyword
          \--> lasso (intern) --> Symbol --> TokenKind::Identifier
```

**Hot Path Optimization**:
1. Check if token is keyword using phf (O(1))
2. If not keyword, intern identifier using lasso (O(1) lookup after first intern)
3. Token contains Symbol (cheap 32-bit index) not String

### Group 4: Memory-Efficient Allocation

**Dependencies**: `bumpalo`, `memchr`, `mimalloc` (optional)

**Purpose**: Minimize allocation overhead in hot paths.

| Crate | Role | Implementation |
|-------|------|----------------|
| bumpalo | Arena allocation for AST nodes | `src/parser/ast_arena.rs` |
| memchr | SIMD byte search in lexer | `src/lexer/mod.rs` |
| mimalloc | Fast global allocator | Feature flag `fast-alloc` |

**Synergy Pattern**:
```
Source --> memchr (find token boundaries) --> bumpalo (alloc AST nodes)
                                         \--> mimalloc (general allocations)
```

### Group 5: Bytecode Caching

**Dependencies**: `rkyv`, `memmap2` (optional), `zerocopy`

**Purpose**: Persistent bytecode storage with zero-copy access.

**Flow**:
```
Bytecode Chunk --> rkyv serialize --> File
File --> memmap2 --> &[u8] --> zerocopy slice_from_bytes --> &[Instruction]
```

**Feature Flags**:
- `mmap` enables memmap2 for memory-mapped bytecode files

## Integration Metrics

### Size Guarantees (Compile-Time Verified)

| Type | Size | Assertion Location |
|------|------|-------------------|
| Opcode | 1 byte | `opcode.rs:261-262` |
| Instruction | 4 bytes | `instruction.rs:36-37` |
| NanBoxedValue | 8 bytes | `nanbox.rs:80-81` |
| GcHeader | 16 bytes | `heap.rs` |

### Performance Characteristics

| Operation | Complexity | Implementation |
|-----------|------------|----------------|
| Keyword lookup | O(1) | phf perfect hash |
| String comparison | O(1) | lasso Symbol equality |
| Opcode decode | O(1) | zerocopy or range check |
| Instruction read | O(1) | zerocopy from_bytes |
| Value transmute | O(1) | bytemuck cast |

### Safety Guarantees

| Risk | Mitigation |
|------|------------|
| Invalid opcode byte | zerocopy TryFromBytes validation |
| Misaligned instruction read | zerocopy alignment check |
| NaN-box corruption | bytemuck Pod (all patterns valid) |
| Size regression | static_assertions at compile time |

## Dependency Decision Tree

```
Need to read structured type from bytes?
├── Type has invalid bit patterns?
│   ├── Yes: Use zerocopy TryFromBytes
│   └── No: Use bytemuck Pod
│
├── Need to persist to disk?
│   └── Add rkyv Archive
│
├── Need memory-mapped access?
│   └── Add memmap2 + zerocopy slice_from_bytes
│
└── Need compile-time size check?
    └── Add static_assertions
```

## High-Priority Integration Points

### Tier 1: Immediate Performance Gains

| Dependency | Location | Current State | Improvement |
|------------|----------|---------------|-------------|
| `likely_stable` | `vm/mod.rs:225-251` | No branch hints | 5-15% dispatch speedup |
| `unchecked-index` | `vm/mod.rs:256-264` | Bounds-checked | Hot path optimization |
| `parking_lot` | `vm/value.rs:6,26` | `std::cell::RefCell` | Faster locking |
| `tinyvec` | `vm/mod.rs:91,92` | `Vec<Value>` | Stack allocation for small frames |
| `bitvec` | `gc/heap.rs:86-95` | `Color` enum per object | 8x memory reduction for marks |

### Tier 2: Architecture Improvements

| Dependency | Location | Current State | Improvement |
|------------|----------|---------------|-------------|
| `tracing` | All modules | No instrumentation | Structured logging + profiling |
| `thiserror` | `vm/mod.rs:23-41` | Manual Debug impl | Better error messages |
| `rayon` | `lexer/mod.rs` | Single-threaded | Parallel multi-file lexing |
| `ringbuf` | Future async | N/A | Lock-free event queues |

---

## Detailed Integration Examples

### VM Dispatch Optimization (`vm/mod.rs`)

**Current (lines 241-250):**
```rust
let handler = DISPATCH_TABLE[instr.opcode() as usize];
match handler(self, instr) {
    Ok(()) => continue,
    Err(VmError::Halted) => { /* ... */ }
    Err(e) => return Err(e),
}
```

**Optimized with `likely_stable`:**
```rust
use likely_stable::{likely, unlikely};

let handler = DISPATCH_TABLE[instr.opcode() as usize];
match handler(self, instr) {
    Ok(()) => continue,  // Most common - no hint needed
    Err(VmError::Halted) => {
        if unlikely(self.call_stack.is_empty()) {
            return Ok(self.registers[0].clone());
        }
    }
    Err(e) => return Err(e),  // Rare path
}
```

### Register Access with `unchecked-index` (`vm/mod.rs:255-264`)

**Current:**
```rust
fn get_reg(&self, idx: u8) -> &Value {
    &self.registers[idx as usize]  // Bounds check on every access
}
```

**Optimized:**
```rust
use unchecked_index::UncheckedIndex;

#[inline]
fn get_reg(&self, idx: u8) -> &Value {
    // SAFETY: VM guarantees idx < 256 (register file size)
    unsafe { self.registers.unchecked_index(idx as usize) }
}
```

### GC Mark Bits with `bitvec` (`gc/heap.rs`)

**Current:**
```rust
pub enum Color {
    White = 0,
    Gray = 1,
    Black = 2,
}
// Each object header stores 1 byte for Color
```

**Optimized:**
```rust
use bitvec::prelude::*;

pub struct Heap {
    /// 2 bits per object: white=00, gray=01, black=10
    /// For 1M objects: 256KB vs 1MB with inline Color
    mark_bits: BitVec<u64, Lsb0>,
}
```

### Call Stack with `tinyvec` (`vm/mod.rs:92`)

**Current:**
```rust
call_stack: Vec<CallFrame>,
```

**Optimized:**
```rust
use tinyvec::TinyVec;

// Inline up to 16 frames, spill to heap for deep recursion
call_stack: TinyVec<[CallFrame; 16]>,
```

### VmError with `thiserror` (`vm/mod.rs:23-41`)

**Current:**
```rust
#[derive(Debug, Clone)]
pub enum VmError {
    DivisionByZero,
    TypeError(String),
    // ...
}
```

**Optimized:**
```rust
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum VmError {
    #[error("division by zero")]
    DivisionByZero,

    #[error("TypeError: {0}")]
    TypeError(String),

    #[error("ReferenceError: {0} is not defined")]
    ReferenceError(String),

    #[error("invalid opcode: 0x{0:02X}")]
    InvalidOpcode(u8),
}
```

---

## Synergy Combinations

### 1. VM Fast Path Combo
```
likely_stable + unchecked-index + tinyvec
```
**Estimated Impact**: 15-25% faster bytecode execution

### 2. GC Memory Efficiency Combo
```
bitvec + tinyvec + parking_lot
```
**Estimated Impact**: 30-50% less GC overhead

### 3. Zero-Copy Serialization Combo
```
rkyv + memmap2 + zerocopy
```
**Estimated Impact**: Near-instant cold start for cached bytecode

### 4. Observability Combo
```
tracing + tracing-subscriber + anyhow + thiserror
```
**Estimated Impact**: 10x easier debugging, production-ready logging

---

## Implementation Priority

### Phase 1: Quick Wins (1-2 days)
1. Add `likely_stable` to VM dispatch (`vm/mod.rs`)
2. Add `thiserror` to VmError (`vm/mod.rs`)
3. Add `tracing` instrumentation (all modules)

### Phase 2: Medium Effort (3-5 days)
4. Replace register access with `unchecked-index` (`vm/mod.rs`)
5. Add `tinyvec` for small collections (`vm/mod.rs`, `gc/heap.rs`)
6. Implement `bitvec` for GC marks (`gc/heap.rs`)

### Phase 3: Architecture (1-2 weeks)
7. NaN-boxing with `bytemuck` (`vm/value.rs`, `vm/nanbox.rs`)
8. Parallel lexing with `rayon` (`lexer/mod.rs`)
9. Bytecode caching with `rkyv` + `memmap2` (`bytecode/chunk.rs`)

---

## Future Synergies

### bitvec + GC (Ready)
Dense bit vectors for GC mark tracking when scaling to large heaps.

### rayon + Lexer (Ready)
Parallel multi-file lexing for project-wide analysis.

### soa-derive + Object Model (Future)
Struct-of-arrays for better cache locality in object property access.

---

## Verification

```bash
# All tests pass
cargo test --features jit  # 180 passed

# All features build
cargo build --features "jit,fast-alloc,mmap,parallel"

# Check for UB with miri (after implementing unsafe optimizations)
cargo +nightly miri test
```
