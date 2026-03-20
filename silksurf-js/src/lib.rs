//! `SilkSurfJS` - Pure Rust JavaScript Engine
//!
//! Cleanroom implementation with:
//! - Zero-copy lexer (tokens reference source directly)
//! - Arena-based allocation (minimal heap pressure)
//! - Generational GC with reference counting for cycles
//! - Bytecode VM (register-based for performance)
//! - C FFI for integration with `SilkSurf` C core
//!
//! Design informed by studying Boa, `QuickJS`, and Elk patterns.
//! No code copied - independent implementation per `SILKSURF-JS-DESIGN.md`.

// Lint configuration
//
// This crate is a bytecode JavaScript VM. Several clippy lints that are
// correct guidance for general code are either intentional design choices
// here, or would require invasive API-breaking changes:
//
// - cast_possible_truncation/wrap/sign_loss/precision_loss: NaN-boxing
//   (nanbox.rs) and instruction encoding (instruction.rs) perform
//   deliberate bit-level casts. Truncation is accounted for by the value
//   representation invariants established at those call sites.
//
// - unnecessary_wraps: Some pub API functions return Result/Option for
//   forward-compatibility; changing the return type breaks C FFI callers.
//
// - items_after_statements: Local helper closures/fns defined inside
//   method bodies are idiomatic in compiler and VM hot paths.
//
// - not_unsafe_ptr_arg_deref: extern "C" FFI functions guard every raw
//   pointer with an is_null() check before dereferencing. Marking them
//   `unsafe` in Rust would have no effect on the C-visible signature.
//
// - missing_panics_doc/missing_errors_doc/multiple_crate_versions/
//   cargo_common_metadata/module_inception/too_many_lines/inline_always:
//   Documentation and cosmetic lints deferred until API stabilizes.
#![allow(unknown_lints)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(dead_code)] // GC module is prepared for future phases
// Bytecode VM / NaN-boxing: intentional bit-level casts
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
// API choices requiring breaking changes to alter
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
// Documentation and cosmetic lints -- deferred
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::multiple_crate_versions)]
#![allow(clippy::cargo_common_metadata)]
#![allow(clippy::module_inception)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::inline_always)]

#[cfg(all(feature = "fast-alloc", not(target_arch = "wasm32")))]
#[global_allocator]
static GLOBAL_ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub mod bytecode;
pub mod ffi;
pub mod gc;
pub mod lexer;
pub mod parser;
#[cfg(feature = "tracing-full")]
pub mod tracing_support;
pub mod verification;
pub mod vm;

#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
pub mod wasm;

#[cfg(feature = "napi")]
pub mod napi;

#[cfg(feature = "jit")]
pub mod jit;

// Re-exports for convenience
pub use bytecode::{Chunk, ChunkDeserializeError, Instruction, Opcode};
pub use gc::Arena;
pub use lexer::{Lexer, Span, Token, TokenKind};
pub use parser::{Expression, ParseError, Parser, Program, Statement};
pub use vm::snapshot::{SnapshotError, VmSnapshot};
pub use vm::{Vm, VmError, VmResult};
