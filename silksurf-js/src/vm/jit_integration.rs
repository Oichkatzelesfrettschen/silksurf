//! JIT Integration for VM
//!
//! Provides tiered compilation support - functions start in the interpreter
//! and are JIT-compiled after reaching the call threshold.

#[cfg(feature = "jit")]
use std::collections::HashMap;

#[cfg(feature = "jit")]
use crate::bytecode::Chunk;
#[cfg(feature = "jit")]
use crate::jit::{JIT_THRESHOLD, JitCompiler, JitError, should_jit_compile};

/// Hot function tracker for tiered compilation
#[cfg(feature = "jit")]
pub struct HotFunctionTracker {
    /// Call counts per function (chunk_idx -> count)
    call_counts: HashMap<usize, u32>,
    /// JIT compiler instance
    compiler: Option<JitCompiler>,
}

#[cfg(feature = "jit")]
impl HotFunctionTracker {
    /// Create a new tracker
    pub fn new() -> Self {
        Self {
            call_counts: HashMap::new(),
            compiler: None,
        }
    }

    /// Create tracker with JIT compiler
    pub fn with_jit() -> Result<Self, JitError> {
        Ok(Self {
            call_counts: HashMap::new(),
            compiler: Some(JitCompiler::new()?),
        })
    }

    /// Record a function call and check if it should be JIT compiled
    ///
    /// Returns true if the function was just compiled
    pub fn record_call(&mut self, chunk_idx: usize, chunk: &Chunk) -> bool {
        let count = self.call_counts.entry(chunk_idx).or_insert(0);
        *count = count.saturating_add(1);

        // Check if should compile
        if should_jit_compile(*count, chunk.instructions.len()) {
            // Only compile once
            if *count == JIT_THRESHOLD {
                return self.try_compile(chunk_idx, chunk);
            }
        }

        false
    }

    /// Attempt to JIT compile a function
    fn try_compile(&mut self, chunk_idx: usize, chunk: &Chunk) -> bool {
        let Some(compiler) = &mut self.compiler else {
            return false;
        };

        if compiler.is_compiled(chunk_idx) {
            return false;
        }

        match compiler.compile(chunk, chunk_idx) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    /// Check if a function is JIT compiled
    pub fn is_compiled(&self, chunk_idx: usize) -> bool {
        self.compiler
            .as_ref()
            .is_some_and(|c| c.is_compiled(chunk_idx))
    }

    /// Execute a JIT-compiled function if available
    ///
    /// # Safety
    ///
    /// The chunk must match what was compiled
    pub unsafe fn try_execute(&self, chunk_idx: usize) -> Option<i64> {
        let compiler = self.compiler.as_ref()?;
        let compiled = compiler.cache.get(chunk_idx)?;
        Some(compiled.call())
    }

    /// Get call count for a function
    pub fn call_count(&self, chunk_idx: usize) -> u32 {
        self.call_counts.get(&chunk_idx).copied().unwrap_or(0)
    }

    /// Get JIT statistics
    pub fn stats(&self) -> JitStats {
        JitStats {
            tracked_functions: self.call_counts.len(),
            compiled_functions: self
                .compiler
                .as_ref()
                .map(|c| c.stats().compiled_functions)
                .unwrap_or(0),
            total_calls: self.call_counts.values().sum(),
        }
    }
}

#[cfg(feature = "jit")]
impl Default for HotFunctionTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// JIT statistics
#[cfg(feature = "jit")]
#[derive(Debug, Clone, Copy)]
pub struct JitStats {
    /// Number of functions being tracked
    pub tracked_functions: usize,
    /// Number of JIT-compiled functions
    pub compiled_functions: usize,
    /// Total call count across all functions
    pub total_calls: u32,
}

#[cfg(all(test, feature = "jit"))]
mod tests {
    use super::*;
    use crate::bytecode::{Instruction, Opcode};

    fn make_hot_chunk() -> Chunk {
        let mut chunk = Chunk::default();
        // Add enough instructions to be considered for JIT
        for _ in 0..15 {
            chunk.instructions.push(Instruction::new_r(Opcode::Nop, 0));
        }
        chunk
            .instructions
            .push(Instruction::new_ri(Opcode::LoadSmi, 0, 42));
        chunk.instructions.push(Instruction::new_r(Opcode::Ret, 0));
        chunk.register_count = 1;
        chunk
    }

    #[test]
    fn test_tracker_creation() {
        let tracker = HotFunctionTracker::new();
        assert_eq!(tracker.stats().tracked_functions, 0);
    }

    #[test]
    fn test_tracker_with_jit() {
        let tracker = HotFunctionTracker::with_jit();
        assert!(tracker.is_ok());
    }

    #[test]
    fn test_call_counting() {
        let mut tracker = HotFunctionTracker::new();
        let chunk = make_hot_chunk();

        for _ in 0..50 {
            tracker.record_call(0, &chunk);
        }

        assert_eq!(tracker.call_count(0), 50);
    }

    #[test]
    fn test_jit_threshold_trigger() {
        // UNWRAP-OK: with_jit() only fails if Cranelift backend init fails, which is
        // tested separately; this happy-path test asserts the backend is available.
        let mut tracker = HotFunctionTracker::with_jit().unwrap();
        let chunk = make_hot_chunk();

        // Call up to threshold - 1
        for _ in 0..(JIT_THRESHOLD - 1) {
            let compiled = tracker.record_call(0, &chunk);
            assert!(!compiled, "Should not compile before threshold");
        }

        // This call should trigger compilation
        let compiled = tracker.record_call(0, &chunk);
        assert!(compiled, "Should compile at threshold");
        assert!(tracker.is_compiled(0));
    }

    #[test]
    fn test_jit_execute() {
        // UNWRAP-OK: with_jit() only fails if Cranelift backend init fails, which is
        // tested separately; this happy-path test asserts the backend is available.
        let mut tracker = HotFunctionTracker::with_jit().unwrap();
        let chunk = make_hot_chunk();

        // Warm up to threshold
        for _ in 0..JIT_THRESHOLD {
            tracker.record_call(0, &chunk);
        }

        // SAFETY: chunk was just compiled via record_call up to JIT_THRESHOLD above,
        // so try_execute's contract (chunk matches what was compiled) is satisfied.
        let result = unsafe { tracker.try_execute(0) };
        assert_eq!(result, Some(42));
    }

    #[test]
    fn test_stats() {
        // UNWRAP-OK: with_jit() only fails if Cranelift backend init fails, which is
        // tested separately; this happy-path test asserts the backend is available.
        let mut tracker = HotFunctionTracker::with_jit().unwrap();
        let chunk = make_hot_chunk();

        for _ in 0..150 {
            tracker.record_call(0, &chunk);
        }

        let stats = tracker.stats();
        assert_eq!(stats.tracked_functions, 1);
        assert_eq!(stats.compiled_functions, 1);
        assert_eq!(stats.total_calls, 150);
    }
}
