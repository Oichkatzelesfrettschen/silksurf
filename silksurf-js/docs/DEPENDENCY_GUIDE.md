# SilkSurfJS Dependency Guide

**Last Updated**: 2025-12-30
**Total Dependencies**: 50+ (275 with transitive)
**Tests**: 180 passing

This guide documents all dependencies, their purpose, and recommended usage patterns.

## Table of Contents

1. [Memory & Allocation](#memory--allocation)
2. [Zero-Copy & Serialization](#zero-copy--serialization)
3. [String & Text Processing](#string--text-processing)
4. [Compile-Time Utilities](#compile-time-utilities)
5. [Bit Manipulation](#bit-manipulation)
6. [Collections](#collections)
7. [Numeric & SIMD](#numeric--simd)
8. [Error Handling](#error-handling)
9. [Concurrency](#concurrency)
10. [Tracing & Diagnostics](#tracing--diagnostics)
11. [CLI & TUI](#cli--tui)
12. [JIT Compilation](#jit-compilation)
13. [Tooling Commands](#tooling-commands)

---

## Memory & Allocation

### bumpalo (3.16)
**Purpose**: Arena allocation for AST nodes and temporary allocations.

```rust
use bumpalo::Bump;

let arena = Bump::new();
let node = arena.alloc(AstNode::new());
// All allocations freed when arena is dropped
```

**When to use**: AST parsing, temporary buffers, batch allocations.

### mimalloc (0.1) - Feature: `fast-alloc`
**Purpose**: Fast global allocator replacement.

```rust
#[cfg(feature = "fast-alloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
```

**When to use**: Production builds where allocation is a bottleneck.

### memmap2 (0.9) - Feature: `mmap`
**Purpose**: Memory-mapped file I/O for bytecode caching.

```rust
use memmap2::Mmap;
use std::fs::File;

let file = File::open("bytecode.bin")?;
let mmap = unsafe { Mmap::map(&file)? };
let instructions = Instruction::slice_from_bytes(&mmap)?;
```

**When to use**: Loading cached bytecode files, large file processing.

---

## Zero-Copy & Serialization

### bytemuck (1.21)
**Purpose**: Safe transmutes for types where all bit patterns are valid.

```rust
use bytemuck::{Pod, Zeroable};

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(transparent)]
pub struct NanBoxedValue(u64);

// Safe transmute
let value: NanBoxedValue = bytemuck::cast(raw_bits);
let slice: &[NanBoxedValue] = bytemuck::cast_slice(bytes);
```

**When to use**: NaN-boxed values, any type where all bit patterns are valid.

### zerocopy (0.8)
**Purpose**: Safe byte conversion with validation for types with invalid patterns.

```rust
use zerocopy::{FromBytes, IntoBytes, TryFromBytes};

#[derive(TryFromBytes, IntoBytes)]
#[repr(u8)]
pub enum Opcode {
    Add = 0x10,
    // Only specific values are valid
}

// Safe decode with validation
let opcode = Opcode::try_read_from_bytes(&[0x10])?;
```

**When to use**: Opcodes, instructions, any type with invalid bit patterns.

### rkyv (0.8)
**Purpose**: Zero-copy archive serialization for bytecode persistence.

```rust
use rkyv::{Archive, Serialize, Deserialize};

#[derive(Archive, Serialize, Deserialize)]
pub struct Chunk {
    instructions: Vec<Instruction>,
    constants: Vec<Constant>,
}

// Serialize
let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&chunk)?;

// Zero-copy access (no deserialization)
let archived = rkyv::access::<ArchivedChunk, rkyv::rancor::Error>(&bytes)?;
```

**When to use**: Bytecode caching, snapshot serialization.

---

## String & Text Processing

### lasso (0.7)
**Purpose**: String interning for O(1) identifier comparison.

```rust
use lasso::{Rodeo, Spur};

let mut interner = Rodeo::default();
let symbol: Spur = interner.get_or_intern("identifier");

// O(1) comparison
if sym1 == sym2 { /* same string */ }
```

**When to use**: All identifier storage, variable names, property keys.

### memchr (2.7)
**Purpose**: SIMD-accelerated byte searching for lexer.

```rust
use memchr::{memchr, memchr2, memmem};

// Find single byte (3-6x faster than naive)
if let Some(pos) = memchr(b'\n', source) { }

// Find two possible bytes
if let Some(pos) = memchr2(b'"', b'\'', source) { }

// Find substring
let finder = memmem::Finder::new("function");
```

**When to use**: Lexer scanning, string searching.

### unicode-xid (0.2)
**Purpose**: Unicode identifier validation per ECMA-262.

```rust
use unicode_xid::UnicodeXID;

fn is_id_start(c: char) -> bool {
    c == '$' || c == '_' || UnicodeXID::is_xid_start(c)
}
```

**When to use**: Identifier parsing in lexer.

---

## Compile-Time Utilities

### phf (0.11)
**Purpose**: Perfect hash functions for O(1) keyword lookup.

```rust
use phf::phf_map;

static KEYWORDS: phf::Map<&'static str, TokenKind> = phf_map! {
    "function" => TokenKind::Function,
    "const" => TokenKind::Const,
    // 55 keywords...
};

pub fn keyword_lookup(s: &str) -> Option<TokenKind> {
    KEYWORDS.get(s).copied()
}
```

**When to use**: Keyword lookup, operator mapping, any static string->value map.

### static_assertions (1.1)
**Purpose**: Compile-time size and layout verification.

```rust
use static_assertions::{assert_eq_size, const_assert_eq};

assert_eq_size!(Instruction, u32);  // Clear error if wrong
const_assert_eq!(std::mem::size_of::<Opcode>(), 1);
```

**When to use**: All critical types (instructions, values, headers).

### const_format (0.2)
**Purpose**: Compile-time string formatting.

```rust
use const_format::formatcp;

const ERROR_MSG: &str = formatcp!("Expected {} bytes, got {}", 4, 8);
```

**When to use**: Static error messages, const strings.

### paste (1.0)
**Purpose**: Identifier concatenation in macros.

```rust
use paste::paste;

macro_rules! define_ops {
    ($($name:ident),*) => {
        paste! {
            $(
                fn [<execute_ $name>](&mut self) { }
            )*
        }
    }
}
```

**When to use**: Code generation macros, opcode dispatch.

### seq-macro (0.3)
**Purpose**: Sequential iteration in macros.

```rust
use seq_macro::seq;

seq!(N in 0..16 {
    fn get_reg_~N(&self) -> Value { self.registers[N] }
});
```

**When to use**: Unrolled loops, register access macros.

---

## Bit Manipulation

### bitvec (1.0)
**Purpose**: Efficient bit vectors for GC marking.

```rust
use bitvec::prelude::*;

let mut marks: BitVec = bitvec![0; object_count];
marks.set(index, true);  // Mark object
```

**When to use**: GC mark phase, dense boolean arrays.

### bitflags (2.6)
**Purpose**: Type-safe flag sets.

```rust
use bitflags::bitflags;

bitflags! {
    pub struct FunctionFlags: u8 {
        const ASYNC = 0b0001;
        const GENERATOR = 0b0010;
        const ARROW = 0b0100;
        const STRICT = 0b1000;
    }
}
```

**When to use**: Function attributes, object property flags.

### modular-bitfield (0.11)
**Purpose**: Bit-level struct packing.

```rust
use modular_bitfield::prelude::*;

#[bitfield]
pub struct PropertyDescriptor {
    writable: bool,
    enumerable: bool,
    configurable: bool,
    #[skip] __: B5,  // padding
}
```

**When to use**: Compact object headers, instruction encoding.

### u4 (0.1)
**Purpose**: 4-bit unsigned integer type.

```rust
use u4::U4;

let nibble = U4::new(0xF);  // Panics if > 15
let packed: u8 = (high.get() << 4) | low.get();
```

**When to use**: Packed register indices, nibble operations.

---

## Collections

### tinyvec (1.8)
**Purpose**: Inline/heap hybrid vector for small collections.

```rust
use tinyvec::{ArrayVec, TinyVec};

// Stack-only, max 8 elements
let mut args: ArrayVec<[Value; 8]> = ArrayVec::new();

// Spills to heap if needed
let mut params: TinyVec<[Value; 4]> = TinyVec::new();
```

**When to use**: Function arguments, small temporary buffers.

### ringbuf (0.4)
**Purpose**: Lock-free ring buffer for async communication.

```rust
use ringbuf::HeapRb;

let rb = HeapRb::<Event>::new(256);
let (mut prod, mut cons) = rb.split();

prod.push(event);
if let Some(e) = cons.pop() { }
```

**When to use**: Event queues, async I/O buffers.

### unchecked-index (0.2)
**Purpose**: Bounds-check-free indexing for hot paths.

```rust
use unchecked_index::UncheckedIndex;

let slice = slice.unchecked_index();
// SAFETY: bounds verified by caller
let value = unsafe { slice[known_valid_index] };
```

**When to use**: VM dispatch loop, proven-safe index access.

---

## Numeric & SIMD

### num-traits (0.2)
**Purpose**: Generic numeric traits.

```rust
use num_traits::{Zero, One, ToPrimitive};

fn add<T: num_traits::Num>(a: T, b: T) -> T {
    a + b
}
```

**When to use**: Generic numeric operations.

### likely_stable (0.1)
**Purpose**: Branch prediction hints on stable Rust.

```rust
use likely_stable::{likely, unlikely};

if likely(value.is_number()) {
    // Fast path - predicted taken
}

if unlikely(error_occurred) {
    // Cold path - predicted not taken
}
```

**When to use**: VM dispatch, error handling branches.

### simba (0.9) - Feature: `math`
**Purpose**: SIMD abstraction layer.

```rust
use simba::simd::f64x4;

let a = f64x4::from([1.0, 2.0, 3.0, 4.0]);
let b = f64x4::from([5.0, 6.0, 7.0, 8.0]);
let c = a + b;  // SIMD addition
```

**When to use**: Vectorized numeric operations.

### nalgebra (0.33) - Feature: `math`
**Purpose**: Linear algebra for graphics/math extensions.

```rust
use nalgebra::{Matrix4, Vector3};

let transform = Matrix4::new_translation(&Vector3::new(1.0, 2.0, 3.0));
```

**When to use**: TypedArray operations, WebGL bindings.

---

## Error Handling

### anyhow (1.0)
**Purpose**: Flexible error type for application code.

```rust
use anyhow::{Context, Result, bail};

fn parse_file(path: &str) -> Result<Ast> {
    let content = std::fs::read_to_string(path)
        .context("Failed to read source file")?;

    if content.is_empty() {
        bail!("Source file is empty");
    }

    Ok(parse(&content)?)
}
```

**When to use**: CLI tools, integration points, main functions.

### thiserror (1.0)
**Purpose**: Derive macro for custom error types.

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Unexpected token {0:?} at line {1}")]
    UnexpectedToken(TokenKind, usize),

    #[error("Unterminated string literal")]
    UnterminatedString,
}
```

**When to use**: Library error types, structured errors.

---

## Concurrency

### parking_lot (0.12)
**Purpose**: Fast synchronization primitives.

```rust
use parking_lot::{Mutex, RwLock};

static CACHE: Mutex<HashMap<Key, Value>> = Mutex::new(HashMap::new());

// Faster than std::sync::Mutex
let guard = CACHE.lock();
```

**When to use**: All synchronization (replace std::sync).

### once_cell (1.20)
**Purpose**: Lazy initialization.

```rust
use once_cell::sync::Lazy;

static RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    Runtime::new()
});
```

**When to use**: Global singletons, deferred initialization.

### rayon (1.10) - Feature: `parallel`
**Purpose**: Parallel iteration.

```rust
use rayon::prelude::*;

let results: Vec<_> = files.par_iter()
    .map(|f| parse_file(f))
    .collect();
```

**When to use**: Multi-file lexing, parallel compilation.

---

## Tracing & Diagnostics

### tracing (0.1)
**Purpose**: Structured logging and instrumentation.

```rust
use tracing::{info, debug, span, Level};

#[tracing::instrument]
fn compile(source: &str) -> Result<Chunk> {
    info!(len = source.len(), "Starting compilation");

    let span = span!(Level::DEBUG, "lexer");
    let _enter = span.enter();

    debug!("Tokenizing...");
    // ...
}
```

**When to use**: All logging, performance instrumentation.

### tracing-subscriber (0.3) - Feature: `tracing-full`
**Purpose**: Tracing output configuration.

```rust
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

tracing_subscriber::registry()
    .with(fmt::layer())
    .with(EnvFilter::from_default_env())
    .init();
```

**When to use**: CLI tools, test harnesses.

---

## CLI & TUI

### clap (4.5) - Feature: `cli`
**Purpose**: Command-line argument parsing.

```rust
use clap::Parser;

#[derive(Parser)]
struct Args {
    #[arg(short, long)]
    verbose: bool,

    file: PathBuf,
}
```

**When to use**: CLI binary entry points.

### ratatui (0.29) - Feature: `tui`
**Purpose**: Terminal UI framework.

```rust
use ratatui::{widgets::*, prelude::*};

let block = Block::default()
    .title("VM State")
    .borders(Borders::ALL);
```

**When to use**: Interactive debugger, REPL with TUI.

### crossterm (0.28) - Feature: `tui`
**Purpose**: Cross-platform terminal manipulation.

```rust
use crossterm::{terminal, cursor, event};

terminal::enable_raw_mode()?;
```

**When to use**: Terminal control for TUI.

---

## JIT Compilation

### cranelift-* (0.116) - Feature: `jit`
**Purpose**: Native code generation.

```rust
use cranelift_codegen::ir::*;
use cranelift_frontend::FunctionBuilder;

let mut builder = FunctionBuilder::new(&mut func, &mut ctx);
let block = builder.create_block();
builder.switch_to_block(block);
// ... generate IR
```

**When to use**: JIT compilation of hot functions.

---

## Tooling Commands

```bash
# Build with all features
cargo build --features full

# Run tests
cargo test --features jit

# Run clippy
cargo clippy --all-targets --all-features

# Format code
cargo fmt

# Check formatting
cargo fmt -- --check

# Audit dependencies
cargo deny check

# Run with miri (nightly)
cargo +nightly miri test

# Generate coverage
cargo llvm-cov --html

# Run benchmarks
cargo bench --features jit
```

---

## Feature Matrix

| Feature | Dependencies | Use Case |
|---------|-------------|----------|
| `default` | Core only | Library usage |
| `jit` | cranelift-* | Native JIT |
| `fast-alloc` | mimalloc | Production perf |
| `mmap` | memmap2 | Bytecode caching |
| `parallel` | rayon | Multi-file builds |
| `cli` | clap | Command-line tool |
| `tui` | ratatui, crossterm | Interactive debugger |
| `math` | nalgebra, simba | Numeric extensions |
| `wasm` | wasm-bindgen | Browser target |
| `full` | All of the above | Development |

---

## Dependency Decision Tree

```
Need to store/access data?
├── All bit patterns valid?
│   └── Yes: bytemuck (Pod, Zeroable)
│   └── No: zerocopy (TryFromBytes)
│
├── Need serialization?
│   └── Zero-copy access: rkyv
│   └── Text format: serde_json
│
├── String data?
│   └── Identifiers: lasso (interning)
│   └── Searching: memchr (SIMD)
│
├── Boolean flags?
│   └── Named flags: bitflags
│   └── Dense array: bitvec
│
└── Small collection?
    └── Fixed max size: tinyvec
    └── Ring buffer: ringbuf
```
