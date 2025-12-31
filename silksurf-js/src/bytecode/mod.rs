//! Bytecode VM for JavaScript execution
//!
//! Register-based virtual machine with:
//! - 50+ instruction set (ECMA-262 coverage)
//! - Fixed-width 32-bit instructions for cache efficiency
//! - Function-pointer dispatch for performance
//! - Inline cache slots for property access
//!
//! Architecture derived from publicly documented V8 Ignition design.
//! No code copied - independent implementation per SILKSURF-JS-DESIGN.md.

pub mod opcode;
pub mod instruction;
pub mod chunk;
pub mod compiler;

// Re-exports
pub use opcode::Opcode;
pub use instruction::{Instruction, Register, InstructionBuilder};
pub use chunk::{Chunk, Constant, SourceLocation, DebugInfo, ExceptionHandler, ChunkDeserializeError};
pub use compiler::{Compiler, CompileError, CompileResult};
