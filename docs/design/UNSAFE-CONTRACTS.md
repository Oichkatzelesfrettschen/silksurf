# Unsafe Block Index

Every `unsafe { ... }` block in the production code (crates/*/src,
silksurf-js/src) must be preceded within 7 lines by a `// SAFETY:` comment
that explains the invariant. `scripts/lint_unsafe.sh` enforces this; it is
wired into the local-gate fast pass.

This document is the cross-crate index of every annotated block, with
file:line, the operation, the invariant, and verification status (miri
clean, loom clean, fuzz clean, manually reviewed).

## Index

### `crates/silksurf-css/src/lib.rs`

| line | op | invariant | verify |
|------|-----|-----------|--------|
| ~436 | `str::from_utf8_unchecked(&bytes[start..scan_end])` | `NameParse` validated the byte range to be ASCII before this point | manually reviewed; fuzzed via `css_tokenizer` |

### `crates/silksurf-render/src/lib.rs`

| line | op | invariant | verify |
|------|-----|-----------|--------|
| 275 | `slice::from_raw_parts_mut(ptr as *mut u32, len_u32)` | Vec<u8> alignment >= 4 (matches `alignof::<u32>`); `len_u32 = buffer.len() / 4` exact-fit; `&mut buffer` borrow held for the alias scope | manually reviewed |
| 295 | `unsafe { fill_row_sse2(...) }` (call) | gated by `is_x86_feature_detected!("sse2")`; SSE2 intrinsics in the body are valid whenever SSE2 is | manually reviewed |
| 318 | `_mm_set1_epi32(...)` | no preconditions beyond SSE2 (already verified by caller) | manually reviewed |
| 322 | `ptr.add(idx) as *mut __m128i` (pointer arithmetic) | `idx + 4 <= len` invariant from the loop guard; `*mut __m128i` storeu tolerates unalignment | manually reviewed |
| 325 | `_mm_storeu_si128(dst, value)` | dst points to 16 valid writable bytes; storeu does not require alignment | manually reviewed |
| 333 | `*ptr.add(idx) = pixel` (tail loop) | `idx < len` from the loop guard; exclusive `&mut` on `row` held by caller | manually reviewed |
| ~444 | `slice::from_raw_parts_mut(shared.0.add(row_offset), row_len)` | rayon scope guarantees disjoint tile regions; the SendPtr documents the no-mutations invariant | manually reviewed |

### `crates/silksurf-render/src/lib.rs` -- impl blocks

| op | invariant | verify |
|----|-----------|--------|
| `unsafe impl Send for SendPtr` | pointer is valid for the rayon scope only; we never mutate beyond the disjoint tile a thread owns | manually reviewed |
| `unsafe impl Sync for SendPtr` | same as Send; threads write to disjoint tile regions | manually reviewed |

### `silksurf-js/src` -- DEFERRED

silksurf-js has ~40 `unsafe` blocks concentrated in `gc/heap.rs`, `ffi.rs`,
`vm/string.rs`, and the bytecode chunk machinery. Annotating them is its
own batch (tracked in the SNAZZY-WAFFLE roadmap; the lint currently
excludes silksurf-js/src for that reason).

The known-suspect site is **`silksurf-js/src/ffi.rs:271`** -- an
`unwrap()` inside an `unsafe { CStr::from_ptr(version) }.to_str()` chain
where the failure mode is "FFI caller passed a non-UTF-8 version string."
The migration must turn that into a defensive return rather than a
process-aborting panic across the FFI boundary.

## Verification methodology

For each block:

  * **Manual review.** Author writes the SAFETY: comment; reviewer
    independently reasons about every reachable callsite.
  * **Fuzz.** Where the invariant depends on parser output (e.g. the
    silksurf-css ASCII-range claim), the relevant fuzz target
    (`css_tokenizer`, `html_tokenizer`) exercises the path. `FUZZ=1
    scripts/local_gate.sh full` runs each target for 30s.
  * **Miri.** `MIRI=1 scripts/local_gate.sh full` runs the unit tests of
    silksurf-core and silksurf-css under miri. The render SIMD blocks
    are gated behind a `cfg(target_arch = "x86_64")` so miri (which
    runs on the test machine) does exercise them when applicable.
  * **Loom.** No loom coverage today. Once `silksurf-core::resolve_table`
    grows formal concurrent semantics (P8.S12), the relevant atomics
    will be wrapped in a loom-aware abstraction.

## Bumping or adding an unsafe block

  1. Write the `// SAFETY: ...` comment immediately above the `unsafe`
     keyword (within 7 lines; multi-line OK).
  2. Add the entry to this file under the appropriate crate.
  3. Run `scripts/lint_unsafe.sh` (or the local-gate fast pass).
  4. If the block is in a hot path, also fuzz the relevant input surface
     for at least 30s and confirm no crashes.
