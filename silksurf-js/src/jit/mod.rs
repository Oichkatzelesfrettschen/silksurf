//! JIT Compiler for SilkSurfJS
//!
//! Provides native code generation using Cranelift for hot functions.
//!
//! # Architecture
//!
//! ```text
//! Bytecode -> IR Builder -> Cranelift IR -> Native Code
//!                |                              |
//!                v                              v
//!         Call Analysis              Code Cache (mmap'd)
//! ```
//!
//! # Tiered Compilation
//!
//! 1. Functions start in interpreter
//! 2. Call counter tracks invocations
//! 3. Hot functions (>100 calls) get JIT compiled
//! 4. Very hot functions may get further optimized

#![cfg(feature = "jit")]

mod code_cache;
mod compiler;
mod ir_builder;

pub use code_cache::CodeCache;
pub use compiler::{CompiledFunction, JitCompiler, JitError};
pub use ir_builder::IrBuilder;

/// Threshold for JIT compilation (number of calls)
pub const JIT_THRESHOLD: u32 = 100;

/// Maximum functions to keep in code cache
pub const MAX_CACHED_FUNCTIONS: usize = 1024;

/// JIT compilation result
pub type JitResult<T> = Result<T, JitError>;

/// Check if a chunk is worth JIT compiling
pub fn should_jit_compile(call_count: u32, instruction_count: usize) -> bool {
    // Only compile hot functions with enough instructions
    call_count >= JIT_THRESHOLD && instruction_count >= 10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jit_threshold() {
        assert!(!should_jit_compile(50, 100));
        assert!(!should_jit_compile(100, 5));
        assert!(should_jit_compile(100, 100));
        assert!(should_jit_compile(1000, 50));
    }
}
