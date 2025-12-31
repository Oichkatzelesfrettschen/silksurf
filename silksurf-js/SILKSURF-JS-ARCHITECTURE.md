# SilkSurfJS Architecture Document

**Version**: 0.2.0 (Draft)
**Date**: 2025-12-30
**Status**: Design Phase

This document scopes the comprehensive architecture for SilkSurfJS, a cleanroom JavaScript
engine implementation in Rust. All design decisions are derived from the ECMA-262 specification
and publicly documented high-level patterns from production engines.

## Table of Contents

1. [Design Principles](#design-principles)
2. [Phase Overview](#phase-overview)
3. [Front-End: Lexer and Parser](#front-end-lexer-and-parser)
4. [Bytecode VM Design](#bytecode-vm-design)
5. [Value Representation (NaN Boxing)](#value-representation-nan-boxing)
6. [Object Model: Shapes and Inline Caches](#object-model-shapes-and-inline-caches)
7. [String Optimization](#string-optimization)
8. [Garbage Collection](#garbage-collection)
9. [ES2025 Feature Implementation](#es2025-feature-implementation)
10. [Module System](#module-system)
11. [Embedding and FFI](#embedding-and-ffi)
12. [AOT Compilation and Snapshots](#aot-compilation-and-snapshots)
13. [Testing Strategy](#testing-strategy)
14. [Security Considerations](#security-considerations)
15. [Performance Targets](#performance-targets)
16. [References](#references)

---

## Design Principles

### Cleanroom Implementation
- **No code copying**: All implementations derived from specification text
- **Public patterns only**: Use documented high-level architectures
- **Citation required**: Each major technique must cite its origin

### Memory Efficiency ("Femto" Philosophy)
- Minimize peak memory usage
- Parse -> compile -> drop AST immediately
- Arena allocation for short-lived objects
- Compact bytecode representation

### Correctness First
- Specification compliance verified via test262
- Formal verification with Kani where tractable
- Property-based fuzzing for invariants

---

## Phase Overview

| Phase | Component | Status | Target |
|-------|-----------|--------|--------|
| 1-3 | Lexer + Parser | **Complete** | 175/57 MB/s |
| 4 | Bytecode VM | Planned | Week 1-2 |
| 5 | Object Model | Planned | Week 3-4 |
| 6 | GC Integration | Planned | Week 5-6 |
| 7 | ES2025 Built-ins | Planned | Week 7-10 |
| 8 | test262 Integration | Planned | Week 11-12 |
| 9 | FFI / Embedding | Planned | Week 13-14 |
| 10 | AOT / Snapshots | Planned | Week 15-16 |

---

## Front-End: Lexer and Parser

### Current Implementation (Complete)

**Lexer** (`src/lexer/`)
- Zero-copy tokenization with source references via `Span`
- BPE (Byte Pair Encoding) pattern matching for common tokens
- String interning via `lasso` crate for O(1) identifier comparison
- Throughput: **175 MB/s** (PGO+BOLT optimized)

**Parser** (`src/parser/`)
- Recursive descent for statements
- Pratt precedence climbing for expressions
- Error recovery with synchronization points
- ESTree-compatible AST structure
- Throughput: **57 MB/s** (PGO+BOLT optimized)

### Context-Sensitive Parsing

JavaScript requires context-sensitive parsing for:

```
ASI (Automatic Semicolon Insertion)
- Track line terminators
- Apply insertion rules per ECMA-262 12.9

yield/await Context
- Track function context (generator/async)
- Modify parsing based on context

import Statement
- Valid only at module top-level
- Detect script vs module mode
```

### Arena-Allocated AST (Phase 6.2)

Currently AST nodes use `Box<T>`. Phase 6.2 will migrate to arena allocation:

```rust
// Current (heap-allocated)
pub struct BinaryExpression<'src> {
    pub left: Box<Expression<'src>>,
    pub right: Box<Expression<'src>>,
    // ...
}

// Future (arena-allocated)
pub struct BinaryExpression<'arena, 'src> {
    pub left: &'arena Expression<'arena, 'src>,
    pub right: &'arena Expression<'arena, 'src>,
    // ...
}
```

Benefits:
- Single allocation for entire AST
- Bulk deallocation after bytecode compilation
- Reduced memory fragmentation

---

## Bytecode VM Design

### Architecture: Register-Based

**Rationale** (cite: V8 Ignition documentation):
> "V8's interpreter is a fast low-level register-based interpreter."

Register-based is preferred for small engines because:
- Fewer instructions than stack-based (no push/pop overhead)
- Better locality for small programs
- Natural fit for Rust's ownership model

### Instruction Format

Fixed-width 32-bit instructions for cache efficiency:

```
+--------+--------+--------+--------+
| opcode |  dst   |  src1  |  src2  |
| 8 bits | 8 bits | 8 bits | 8 bits |
+--------+--------+--------+--------+

Alternative for constants:
+--------+--------+------------------+
| opcode |  dst   |    constant_idx  |
| 8 bits | 8 bits |     16 bits      |
+--------+--------+------------------+
```

### Core Opcode Set (50+ instructions)

**Load/Store**
```
LOAD_CONST      r0, #idx      ; r0 = constants[idx]
LOAD_TRUE       r0            ; r0 = true
LOAD_FALSE      r0            ; r0 = false
LOAD_NULL       r0            ; r0 = null
LOAD_UNDEFINED  r0            ; r0 = undefined
MOV             r0, r1        ; r0 = r1
```

**Arithmetic**
```
ADD             r0, r1, r2    ; r0 = r1 + r2
SUB             r0, r1, r2    ; r0 = r1 - r2
MUL             r0, r1, r2    ; r0 = r1 * r2
DIV             r0, r1, r2    ; r0 = r1 / r2
MOD             r0, r1, r2    ; r0 = r1 % r2
POW             r0, r1, r2    ; r0 = r1 ** r2
NEG             r0, r1        ; r0 = -r1
INC             r0            ; r0++
DEC             r0            ; r0--
```

**Comparison**
```
EQ              r0, r1, r2    ; r0 = r1 == r2
STRICT_EQ       r0, r1, r2    ; r0 = r1 === r2
LT              r0, r1, r2    ; r0 = r1 < r2
LE              r0, r1, r2    ; r0 = r1 <= r2
GT              r0, r1, r2    ; r0 = r1 > r2
GE              r0, r1, r2    ; r0 = r1 >= r2
```

**Logical/Bitwise**
```
NOT             r0, r1        ; r0 = !r1
BITNOT          r0, r1        ; r0 = ~r1
BITAND          r0, r1, r2    ; r0 = r1 & r2
BITOR           r0, r1, r2    ; r0 = r1 | r2
BITXOR          r0, r1, r2    ; r0 = r1 ^ r2
SHL             r0, r1, r2    ; r0 = r1 << r2
SHR             r0, r1, r2    ; r0 = r1 >> r2
USHR            r0, r1, r2    ; r0 = r1 >>> r2
```

**Control Flow**
```
JMP             offset        ; unconditional jump
JMP_TRUE        r0, offset    ; jump if r0 truthy
JMP_FALSE       r0, offset    ; jump if r0 falsy
JMP_NULLISH     r0, offset    ; jump if r0 null/undefined
CALL            r0, r1, argc  ; r0 = r1(...args)
CALL_METHOD     r0, r1, #name, argc
RET             r0            ; return r0
THROW           r0            ; throw r0
```

**Property Access**
```
GET_PROP        r0, r1, #name ; r0 = r1.name (with IC)
SET_PROP        r0, #name, r1 ; r0.name = r1 (with IC)
GET_ELEM        r0, r1, r2    ; r0 = r1[r2]
SET_ELEM        r0, r1, r2    ; r0[r1] = r2
DELETE_PROP     r0, r1, #name ; delete r1.name
IN              r0, r1, r2    ; r0 = r1 in r2
INSTANCEOF      r0, r1, r2    ; r0 = r1 instanceof r2
```

**Object/Array Creation**
```
NEW_OBJECT      r0            ; r0 = {}
NEW_ARRAY       r0, len       ; r0 = []
NEW_FUNCTION    r0, #func_idx ; r0 = function
NEW_CLASS       r0, #class_idx
```

**Scope/Environment**
```
GET_LOCAL       r0, slot      ; r0 = locals[slot]
SET_LOCAL       slot, r0      ; locals[slot] = r0
GET_CAPTURE     r0, depth, slot
SET_CAPTURE     depth, slot, r0
GET_GLOBAL      r0, #name
SET_GLOBAL      #name, r0
```

**Iterators/Generators**
```
GET_ITERATOR    r0, r1        ; r0 = r1[Symbol.iterator]()
ITER_NEXT       r0, r1        ; r0 = r1.next()
ITER_DONE       r0, r1        ; r0 = r1.done
YIELD           r0            ; yield r0
AWAIT           r0            ; await r0
```

### Dispatch Implementation

Options ranked by performance:

1. **Direct threading** - Not portable in stable Rust
2. **Function pointer table** - Good balance
3. **Match dispatch** - Simplest, decent with PGO

Recommended: Function pointer dispatch with computed goto optimization hints:

```rust
type OpHandler = fn(&mut Vm, Instruction) -> Result<(), JsError>;

static DISPATCH_TABLE: [OpHandler; 256] = [
    op_load_const,
    op_add,
    op_sub,
    // ...
];

fn execute(vm: &mut Vm) -> Result<Value, JsError> {
    loop {
        let instr = vm.fetch();
        let handler = DISPATCH_TABLE[instr.opcode as usize];
        handler(vm, instr)?;
    }
}
```

### Compiler: AST to Bytecode

Key algorithms:

**Scope Analysis**
```
1. Walk AST to identify all declarations
2. Classify: var (function-scoped) vs let/const (block-scoped)
3. Detect TDZ (Temporal Dead Zone) boundaries
4. Calculate closure captures
5. Assign stack slots and environment indices
```

**Lowering Constructs**
```
for (init; test; update) body
-->
  init
loop_start:
  test
  JMP_FALSE loop_end
  body
continue_target:
  update
  JMP loop_start
loop_end:

try { ... } catch (e) { ... } finally { ... }
-->
  ENTER_TRY handler_offset, finally_offset
  try_body
  LEAVE_TRY
  JMP end
handler:
  catch_body
finally:
  finally_body
end:
```

---

## Value Representation (NaN Boxing)

### Encoding Scheme

Use IEEE 754 quiet NaN payload bits to encode non-float values:

```
64-bit layout:
+------------------+------------------+
|     NaN header   |     payload      |
|     16 bits      |     48 bits      |
+------------------+------------------+

Float64 (normal):  Any non-NaN IEEE754 double
Tagged values:     0x7FF8_xxxx_xxxx_xxxx (quiet NaN with tag)

Tags (in bits 48-50):
  000 = object pointer
  001 = string pointer
  010 = symbol pointer
  011 = BigInt pointer
  100 = small integer (SMI, 32-bit signed in payload)
  101 = boolean (payload: 0=false, 1=true)
  110 = null
  111 = undefined
```

### Rust Implementation

```rust
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Value(u64);

const NAN_QUIET: u64 = 0x7FF8_0000_0000_0000;
const TAG_MASK: u64  = 0x0007_0000_0000_0000;
const TAG_OBJECT: u64 = 0x0000_0000_0000_0000;
const TAG_STRING: u64 = 0x0001_0000_0000_0000;
const TAG_SMI: u64    = 0x0004_0000_0000_0000;
const TAG_BOOL: u64   = 0x0005_0000_0000_0000;
const TAG_NULL: u64   = 0x0006_0000_0000_0000;
const TAG_UNDEF: u64  = 0x0007_0000_0000_0000;
const PTR_MASK: u64   = 0x0000_FFFF_FFFF_FFFF;

impl Value {
    #[inline]
    pub fn from_f64(n: f64) -> Self {
        Self(n.to_bits())
    }

    #[inline]
    pub fn from_i32(n: i32) -> Self {
        Self(NAN_QUIET | TAG_SMI | (n as u32 as u64))
    }

    #[inline]
    pub fn is_number(&self) -> bool {
        self.0 & NAN_QUIET != NAN_QUIET || self.0 == f64::NAN.to_bits()
    }

    #[inline]
    pub fn is_smi(&self) -> bool {
        (self.0 & (NAN_QUIET | TAG_MASK)) == (NAN_QUIET | TAG_SMI)
    }

    #[inline]
    pub fn as_object_ptr(&self) -> Option<*mut Object> {
        if (self.0 & (NAN_QUIET | TAG_MASK)) == (NAN_QUIET | TAG_OBJECT) {
            Some((self.0 & PTR_MASK) as *mut Object)
        } else {
            None
        }
    }
}
```

---

## Object Model: Shapes and Inline Caches

### Hidden Classes (Shapes)

**Concept** (cite: V8 documentation):
> "Hidden classes are the principal mechanism by which V8 exploits
> the structure that JavaScript objects typically have."

Each object points to a Shape that describes:
- Property names and their offsets
- Property attributes (writable, enumerable, configurable)
- Prototype chain reference
- Transition links to other shapes

```rust
pub struct Shape {
    /// Unique shape ID for fast comparison
    pub id: ShapeId,
    /// Property table (name -> PropertyInfo)
    pub properties: PropertyTable,
    /// Prototype shape (for inheritance)
    pub prototype: Option<ShapeId>,
    /// Transitions to child shapes (property_name -> ShapeId)
    pub transitions: TransitionTable,
    /// Number of in-object slots
    pub inline_slots: u8,
}

pub struct PropertyInfo {
    /// Offset in object's property storage
    pub offset: u16,
    /// Property attributes
    pub attributes: PropertyAttributes,
}

pub struct Object {
    /// Shape describing this object's structure
    pub shape: ShapeId,
    /// Inline property slots (fast access)
    pub inline: [Value; 4],
    /// Overflow properties (heap-allocated)
    pub overflow: Option<Box<[Value]>>,
}
```

### Shape Transitions

```
{}                          Shape A (empty)
  |
  v  add "x"
{x: ...}                    Shape B (x at offset 0)
  |
  v  add "y"
{x: ..., y: ...}            Shape C (x at 0, y at 1)
```

Transitions are cached in a hash table for O(1) lookup.

### Inline Caches (ICs)

**Concept** (cite: V8 documentation):
> "Inline caches are the partner optimization that makes hidden classes pay off."

At each property access site in bytecode, cache the last seen shape:

```rust
pub struct InlineCache {
    /// Last seen shape ID
    shape: ShapeId,
    /// Property offset in that shape
    offset: u16,
    /// Cache state
    state: IcState,
}

pub enum IcState {
    Uninitialized,
    Monomorphic,      // 1 shape seen
    Polymorphic,      // 2-4 shapes seen
    Megamorphic,      // >4 shapes, fallback to hash lookup
}

impl InlineCache {
    #[inline]
    pub fn get(&self, obj: &Object, heap: &Heap) -> Option<Value> {
        if obj.shape == self.shape && self.state == IcState::Monomorphic {
            // Fast path: shape matches, use cached offset
            Some(obj.get_property_by_offset(self.offset))
        } else {
            None // Fall back to slow path
        }
    }
}
```

---

## String Optimization

### Requirements

JavaScript strings are immutable, Unicode (UTF-16 semantically), and heavily used.

### Optimization Strategies

**1. Small String Optimization (SSO)**
```rust
const SSO_CAPACITY: usize = 23; // 24 bytes - 1 tag

pub enum JsString {
    /// Inline storage for strings <= 23 bytes UTF-8
    Inline {
        len: u8,
        data: [u8; SSO_CAPACITY],
    },
    /// Heap-allocated for longer strings
    Heap(Arc<str>),
    /// Interned (deduplicated) string
    Interned(Symbol),
    /// Rope for concatenation (lazy flattening)
    Concat(Arc<JsString>, Arc<JsString>),
}
```

**2. Aggressive Interning**
- All property keys are interned
- All identifier-like strings interned
- Use `lasso` crate's `Rodeo` interner

**3. Rope Representation**
For `a + b` concatenation:
- Create Concat node instead of immediate copy
- Flatten lazily when string content is accessed
- Prevents O(n^2) repeated concatenation

**4. UTF-8 Internal, UTF-16 Indexing Cache**
```rust
pub struct HeapString {
    /// UTF-8 encoded content
    data: Box<str>,
    /// Cached UTF-16 length
    utf16_len: u32,
    /// Index cache for O(1) charAt after first access
    index_cache: Option<IndexCache>,
}
```

---

## Garbage Collection

### Strategy: Non-Moving Mark-Sweep with Arenas

**Rationale**:
- Moving GC in Rust requires handle indirection everywhere
- Non-moving is simpler and sufficient for embedded use
- Arena groups provide bulk deallocation for short-lived objects

### GC Architecture

```rust
pub struct Heap {
    /// Arena for AST nodes (reset after compilation)
    ast_arena: Arena,
    /// Arena for bytecode constants (persistent)
    const_arena: Arena,
    /// GC-managed object storage
    gc_heap: GcHeap,
    /// Root set for GC
    roots: RootSet,
}

pub struct GcHeap {
    /// Object pages (non-moving)
    pages: Vec<Page>,
    /// Free list per size class
    free_lists: [FreeList; NUM_SIZE_CLASSES],
    /// Mark bits (separate from objects for cache efficiency)
    mark_bits: MarkBitmap,
}
```

### Mark-Sweep Algorithm

```
MARK PHASE:
  1. Clear all mark bits
  2. For each root in root_set:
       mark_recursive(root)

  mark_recursive(obj):
    if obj.is_marked():
      return
    obj.set_marked()
    for each ref in obj.references():
      mark_recursive(ref)

SWEEP PHASE:
  for each page in pages:
    for each slot in page:
      if not slot.is_marked():
        free_list.add(slot)
```

### WeakRef and FinalizationRegistry (ES2025)

```rust
pub struct WeakRef {
    /// Target object (cleared if collected)
    target: Option<GcPtr<Object>>,
}

pub struct FinalizationRegistry {
    /// Cleanup callback
    cleanup: GcPtr<Function>,
    /// Registered targets with held values
    cells: Vec<FinalizationCell>,
}

// GC integration:
// 1. During mark: weak refs do NOT prevent collection
// 2. After sweep: clear dead weak refs, queue finalizers
// 3. After GC: run HostEnqueueFinalizationRegistryCleanupJob
```

### Future: Incremental / Generational (Optional)

For larger applications:
1. Incremental marking with write barriers
2. Generational: nursery (copying) + old gen (mark-compact)
3. Stop-the-world simplicity first, incremental later

---

## ES2025 Feature Implementation

### New ES2025 Features (ECMA-262 16th Edition)

| Feature | Specification | Priority |
|---------|--------------|----------|
| Iterator helpers | tc39/proposal-iterator-helpers | High |
| Set methods | tc39/proposal-set-methods | High |
| JSON modules | tc39/proposal-json-modules | Medium |
| Import attributes | tc39/proposal-import-attributes | Medium |
| RegExp.escape | tc39/proposal-regex-escaping | Medium |
| RegExp modifiers | tc39/proposal-regexp-modifiers | Medium |
| Promise.try | tc39/proposal-promise-try | High |
| Float16Array | tc39/proposal-float16array | Low |

### Iterator Helpers Implementation

```javascript
// New Iterator global with prototype methods:
Iterator.prototype.map(fn)
Iterator.prototype.filter(fn)
Iterator.prototype.take(n)
Iterator.prototype.drop(n)
Iterator.prototype.flatMap(fn)
Iterator.prototype.reduce(fn, init)
Iterator.prototype.toArray()
Iterator.prototype.forEach(fn)
Iterator.prototype.some(fn)
Iterator.prototype.every(fn)
Iterator.prototype.find(fn)
Iterator.from(iterable)
```

### Set Methods Implementation

```javascript
// New Set prototype methods:
Set.prototype.union(other)
Set.prototype.intersection(other)
Set.prototype.difference(other)
Set.prototype.symmetricDifference(other)
Set.prototype.isSubsetOf(other)
Set.prototype.isSupersetOf(other)
Set.prototype.isDisjointFrom(other)
```

### Promise.try Implementation

```javascript
// Wraps sync/async function in Promise
Promise.try(fn, ...args)
// Equivalent to:
new Promise(resolve => resolve(fn(...args)))
```

---

## Module System

### ModuleLoader Trait

```rust
pub trait ModuleLoader {
    /// Resolve module specifier to canonical path
    fn resolve(&self, specifier: &str, referrer: &str) -> Result<String, JsError>;

    /// Fetch module source code
    fn fetch(&self, path: &str) -> Result<String, JsError>;

    /// Determine module type (JS, JSON, WASM)
    fn module_type(&self, path: &str) -> ModuleType;
}

pub enum ModuleType {
    JavaScript,
    Json,        // ES2025 JSON modules
    WebAssembly, // Future
}
```

### Module Record Lifecycle

```
1. PARSE: Source -> ModuleRecord
   - Parse imports/exports
   - Build dependency graph

2. INSTANTIATE:
   - Link imports to exports
   - Create module environment
   - TDZ for uninitialized bindings

3. EVALUATE:
   - Execute module body
   - Return completion value
```

### Import Attributes (ES2025)

```javascript
import data from "./data.json" with { type: "json" };
import("./config.json", { with: { type: "json" } });
```

---

## Embedding and FFI

### C FFI Design

```rust
// src/ffi/mod.rs

/// Opaque handle to JS runtime
#[repr(C)]
pub struct SilkSurfJsRuntime {
    _private: [u8; 0],
}

/// Opaque handle to JS value
#[repr(C)]
pub struct SilkSurfJsValue {
    _private: [u8; 0],
}

#[no_mangle]
pub extern "C" fn silksurfjs_runtime_new() -> *mut SilkSurfJsRuntime {
    let runtime = Box::new(Runtime::new());
    Box::into_raw(runtime) as *mut SilkSurfJsRuntime
}

#[no_mangle]
pub extern "C" fn silksurfjs_runtime_free(rt: *mut SilkSurfJsRuntime) {
    if !rt.is_null() {
        unsafe { drop(Box::from_raw(rt as *mut Runtime)) };
    }
}

#[no_mangle]
pub extern "C" fn silksurfjs_eval(
    rt: *mut SilkSurfJsRuntime,
    source: *const c_char,
    source_len: usize,
) -> *mut SilkSurfJsValue {
    // ...
}

#[no_mangle]
pub extern "C" fn silksurfjs_value_is_number(val: *const SilkSurfJsValue) -> bool {
    // ...
}
```

### C Header Generation

Use `cbindgen` to generate `silksurfjs.h`:

```c
// silksurfjs.h (auto-generated)
typedef struct SilkSurfJsRuntime SilkSurfJsRuntime;
typedef struct SilkSurfJsValue SilkSurfJsValue;

SilkSurfJsRuntime* silksurfjs_runtime_new(void);
void silksurfjs_runtime_free(SilkSurfJsRuntime* rt);
SilkSurfJsValue* silksurfjs_eval(SilkSurfJsRuntime* rt, const char* source, size_t len);
bool silksurfjs_value_is_number(const SilkSurfJsValue* val);
// ...
```

---

## AOT Compilation and Snapshots

### AOT Bytecode Compiler

Hermes-inspired approach:
- Compile JS to bytecode ahead of time
- Ship bytecode bundles instead of source
- Skip parse + compile on device

```
silksurfjs-compile input.js -o output.ssbc

Bundle format:
+------------------+
| Magic: "SSBC"    |  4 bytes
| Version          |  4 bytes
| Flags            |  4 bytes
| Num Functions    |  4 bytes
| Constant Pool    |  variable
| Function Table   |  variable
| Bytecode         |  variable
| Source Map       |  optional
+------------------+
```

### Snapshot Support

Serialize heap state for fast startup:

```rust
pub struct Snapshot {
    /// Serialized global object graph
    heap_data: Vec<u8>,
    /// Serialized shape table
    shapes: Vec<u8>,
    /// Serialized string table
    strings: Vec<u8>,
    /// Bytecode for built-ins
    builtins: Vec<u8>,
}

impl Runtime {
    pub fn from_snapshot(snapshot: &Snapshot) -> Self {
        // Deserialize directly into memory
        // Skip built-in initialization
    }

    pub fn create_snapshot(&self) -> Snapshot {
        // Serialize current heap state
    }
}
```

---

## Testing Strategy

### test262 Integration

```rust
// tests/test262.rs
use silksurf_js::Runtime;
use test262_harness::{Test, TestOutcome};

fn run_test262() {
    let harness = test262_harness::Harness::new("./test262");

    for test in harness.tests() {
        let mut rt = Runtime::new();
        let result = rt.eval(&test.source);

        match (result, test.expected) {
            (Ok(_), TestOutcome::Pass) => { /* pass */ }
            (Err(e), TestOutcome::Throw(expected)) => {
                assert!(e.matches_expected(&expected));
            }
            _ => panic!("Test failed: {}", test.path),
        }
    }
}
```

### Differential Testing

Compare against QuickJS for behavior verification:

```rust
fn differential_test(source: &str) {
    let our_result = silksurfjs_eval(source);
    let quickjs_result = quickjs_eval(source);

    assert_eq!(
        our_result.to_canonical_json(),
        quickjs_result.to_canonical_json(),
        "Behavior mismatch for: {}",
        source
    );
}
```

### Property-Based Fuzzing

```rust
// fuzz/fuzz_targets/parser.rs
#![no_main]
use libfuzzer_sys::fuzz_target;
use silksurf_js::Parser;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        // Parser should never panic on any input
        let _ = Parser::new(source).parse();
    }
});
```

---

## Security Considerations

### Restricted Execution

```rust
pub struct SecurityPolicy {
    /// Disable eval() and Function() constructor
    pub no_dynamic_code: bool,
    /// Maximum execution steps (prevent infinite loops)
    pub max_steps: Option<u64>,
    /// Maximum heap size
    pub max_heap_bytes: Option<usize>,
    /// Maximum call stack depth
    pub max_stack_depth: Option<u32>,
    /// Restricted global objects
    pub restricted_globals: HashSet<String>,
}
```

### Resource Quotas

```rust
impl Runtime {
    pub fn step(&mut self) -> Result<(), JsError> {
        self.step_count += 1;
        if let Some(max) = self.policy.max_steps {
            if self.step_count > max {
                return Err(JsError::quota_exceeded("execution steps"));
            }
        }
        // ...
    }
}
```

### Avoiding Code Injection

- `eval()` disabled by default
- `Function()` constructor disabled by default
- Template literal tags sanitized
- No `with` statement in strict mode

---

## Performance Targets

| Metric | Target | Current |
|--------|--------|---------|
| Lexer throughput | 150 MB/s | 175 MB/s |
| Parser throughput | 80 MB/s | 57 MB/s |
| VM dispatch | <50ns/op | TBD |
| Property access (IC hit) | <10ns | TBD |
| GC pause (1MB heap) | <1ms | TBD |
| Startup from snapshot | <5ms | TBD |
| Memory per object | <64 bytes | TBD |

---

## References

### Specifications
1. ECMA-262, 16th Edition (ES2025) - https://tc39.es/ecma262/
2. TC39 Proposals - https://github.com/tc39/proposals
3. test262 Conformance Suite - https://github.com/tc39/test262

### Engine Documentation (High-Level Patterns Only)
4. V8 Design Overview - https://v8.dev/docs
5. V8 Hidden Classes - https://v8.dev/docs/hidden-classes
6. V8 Ignition Interpreter - https://v8.dev/docs/ignition
7. SpiderMonkey Internals - https://firefox-source-docs.mozilla.org/js/
8. Hermes Architecture - https://hermesengine.dev/

### Academic Papers
9. "Efficient Implementation of the Smalltalk-80 System" (Deutsch & Schiffman, 1984)
10. "SELF: The Power of Simplicity" (Ungar & Smith, 1987)
11. "Fast Property Access in Javascript" (V8 Blog)

### Cleanroom Notice
All implementations are independently derived from the ECMA-262 specification.
No code has been copied from any existing JavaScript engine.
Design patterns are drawn from publicly documented architectures and academic literature.

---

*Document maintained by SilkSurf Project*
*Last updated: 2025-12-30*
