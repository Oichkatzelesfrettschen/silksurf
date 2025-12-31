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

// Treat warnings as errors for production quality
#![deny(warnings)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(dead_code)] // GC module is prepared for future phases

pub mod gc;
pub mod lexer;
pub mod parser;
pub mod bytecode;
pub mod vm;
pub mod ffi;
pub mod verification;

#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
pub mod wasm;

#[cfg(feature = "napi")]
pub mod napi;

#[cfg(feature = "jit")]
pub mod jit;

// Re-exports for convenience
pub use gc::Arena;
pub use lexer::{Lexer, Token, TokenKind, Span};
pub use parser::{Parser, Program, Statement, Expression, ParseError};
pub use vm::{Vm, VmError, VmResult};
pub use bytecode::{Chunk, Instruction, Opcode, ChunkDeserializeError};
pub use vm::snapshot::{VmSnapshot, SnapshotError};
