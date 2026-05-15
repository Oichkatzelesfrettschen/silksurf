//! JIT Compiler using Cranelift
//!
//! Translates bytecode chunks to native machine code.

use std::collections::HashMap;

use cranelift_codegen::Context;
use cranelift_codegen::ir::{AbiParam, UserFuncName};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

use super::code_cache::CodeCache;
use super::ir_builder::IrBuilder;
use crate::bytecode::{Chunk, Opcode};

/// Error during JIT compilation
#[derive(Debug, Clone)]
pub enum JitError {
    /// Failed to create JIT module
    ModuleCreation(String),
    /// Failed to compile function
    Compilation(String),
    /// Failed to finalize code
    Finalization(String),
    /// Unsupported opcode
    UnsupportedOpcode(Opcode),
    /// Code cache full
    CacheFull,
}

impl std::fmt::Display for JitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JitError::ModuleCreation(s) => write!(f, "Module creation failed: {}", s),
            JitError::Compilation(s) => write!(f, "Compilation failed: {}", s),
            JitError::Finalization(s) => write!(f, "Finalization failed: {}", s),
            JitError::UnsupportedOpcode(op) => write!(f, "Unsupported opcode: {:?}", op),
            JitError::CacheFull => write!(f, "Code cache full"),
        }
    }
}

impl std::error::Error for JitError {}

/// A compiled native function
pub struct CompiledFunction {
    /// Function pointer
    pub ptr: *const u8,
    /// Size in bytes
    pub size: usize,
    /// Source chunk index
    pub chunk_idx: usize,
}

impl CompiledFunction {
    /// Execute the compiled function
    ///
    /// # Safety
    ///
    /// The function pointer must be valid and the calling convention
    /// must match what was compiled.
    pub unsafe fn call(&self) -> i64 {
        let func: fn() -> i64 = std::mem::transmute(self.ptr);
        func()
    }
}

/// JIT compiler instance
pub struct JitCompiler {
    /// Cranelift JIT module
    module: JITModule,
    /// Function builder context (reusable)
    builder_ctx: FunctionBuilderContext,
    /// Cranelift context (reusable)
    ctx: Context,
    /// Compiled function cache
    pub cache: CodeCache,
    /// Function ID counter
    next_func_id: u32,
    /// Compiled function map (chunk_idx -> func_id)
    func_map: HashMap<usize, FuncId>,
}

impl JitCompiler {
    /// Create a new JIT compiler
    pub fn new() -> Result<Self, JitError> {
        // Configure for the native target
        let mut flag_builder = settings::builder();
        flag_builder
            .set("use_colocated_libcalls", "false")
            .map_err(|e| JitError::ModuleCreation(e.to_string()))?;
        flag_builder
            .set("is_pic", "false")
            .map_err(|e| JitError::ModuleCreation(e.to_string()))?;
        flag_builder
            .set("opt_level", "speed")
            .map_err(|e| JitError::ModuleCreation(e.to_string()))?;

        let isa_builder =
            cranelift_native::builder().map_err(|e| JitError::ModuleCreation(e.to_string()))?;

        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| JitError::ModuleCreation(e.to_string()))?;

        let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        let module = JITModule::new(builder);

        Ok(Self {
            module,
            builder_ctx: FunctionBuilderContext::new(),
            ctx: Context::new(),
            cache: CodeCache::new(),
            next_func_id: 0,
            func_map: HashMap::new(),
        })
    }

    /// Compile a bytecode chunk to native code
    pub fn compile(
        &mut self,
        chunk: &Chunk,
        chunk_idx: usize,
    ) -> Result<&CompiledFunction, JitError> {
        // Check if already compiled
        if self.cache.get(chunk_idx).is_some() {
            return self.cache.get(chunk_idx).ok_or(JitError::CacheFull);
        }

        // Create function signature: () -> i64
        let mut sig = self.module.make_signature();
        sig.returns
            .push(AbiParam::new(cranelift_codegen::ir::types::I64));

        // Declare function
        let func_name = format!("chunk_{}", chunk_idx);
        let func_id = self
            .module
            .declare_function(&func_name, Linkage::Local, &sig)
            .map_err(|e| JitError::Compilation(e.to_string()))?;

        // Build function IR
        self.ctx.func.signature = sig;
        self.ctx.func.name = UserFuncName::user(0, self.next_func_id);
        self.next_func_id += 1;

        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_ctx);
            let ir_builder = IrBuilder::new(&mut builder, &self.module);
            ir_builder.build_function(chunk)?;
            builder.finalize();
        }

        // Compile to native code
        self.module
            .define_function(func_id, &mut self.ctx)
            .map_err(|e| JitError::Compilation(e.to_string()))?;

        // Finalize
        self.module
            .finalize_definitions()
            .map_err(|e| JitError::Finalization(e.to_string()))?;

        // Get function pointer
        let code = self.module.get_finalized_function(func_id);

        // Cache the compiled function
        let compiled = CompiledFunction {
            ptr: code,
            size: 0, // Size not easily available from JITModule
            chunk_idx,
        };

        self.func_map.insert(chunk_idx, func_id);
        self.cache.insert(chunk_idx, compiled)?;

        // Clear context for next compilation
        self.module.clear_context(&mut self.ctx);

        self.cache.get(chunk_idx).ok_or(JitError::CacheFull)
    }

    /// Check if a chunk is already compiled
    pub fn is_compiled(&self, chunk_idx: usize) -> bool {
        self.cache.get(chunk_idx).is_some()
    }

    /// Get compilation statistics
    pub fn stats(&self) -> JitStats {
        JitStats {
            compiled_functions: self.cache.len(),
            total_code_size: self.cache.total_size(),
        }
    }
}

impl Default for JitCompiler {
    fn default() -> Self {
        // UNWRAP-OK: Default::default() has no Result return; if the host
        // platform cannot construct a Cranelift JIT module (no native ISA,
        // failed flag config), there is nothing sensible to fall back to,
        // so we panic loudly. Production callers should use JitCompiler::new()
        // directly to handle the Result.
        Self::new().expect("Failed to create JIT compiler")
    }
}

/// JIT compilation statistics
#[derive(Debug, Clone, Copy)]
pub struct JitStats {
    /// Number of compiled functions
    pub compiled_functions: usize,
    /// Total size of generated code
    pub total_code_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Instruction;

    #[test]
    fn test_jit_compiler_creation() {
        let compiler = JitCompiler::new();
        assert!(compiler.is_ok());
    }

    #[test]
    fn test_jit_stats() {
        // UNWRAP-OK: native test host always has a working ISA + Cranelift
        // JIT module; failure indicates a broken developer environment.
        let compiler = JitCompiler::new().unwrap();
        let stats = compiler.stats();
        assert_eq!(stats.compiled_functions, 0);
    }

    #[test]
    fn test_jit_compile_simple() {
        // UNWRAP-OK: native test host always has a working ISA + Cranelift
        // JIT module; failure indicates a broken developer environment.
        let mut compiler = JitCompiler::new().unwrap();

        // Create a simple chunk that returns 42
        // LoadSmi r0, 42
        // Ret r0
        let mut chunk = Chunk::default();
        chunk
            .instructions
            .push(Instruction::new_ri(Opcode::LoadSmi, 0, 42));
        chunk.instructions.push(Instruction::new_r(Opcode::Ret, 0));
        chunk.register_count = 1;

        let result = compiler.compile(&chunk, 0);
        assert!(result.is_ok(), "Compilation should succeed");

        // UNWRAP-OK: assert!(result.is_ok()) on the previous line guarantees Ok.
        let compiled = result.unwrap();
        assert_eq!(compiled.chunk_idx, 0);
        assert!(!compiled.ptr.is_null());
    }

    #[test]
    fn test_jit_compile_arithmetic() {
        // UNWRAP-OK: native test host always has a working ISA + Cranelift
        // JIT module; failure indicates a broken developer environment.
        let mut compiler = JitCompiler::new().unwrap();

        // LoadSmi r0, 10
        // LoadSmi r1, 5
        // Add r2, r0, r1
        // Ret r2
        let mut chunk = Chunk::default();
        chunk
            .instructions
            .push(Instruction::new_ri(Opcode::LoadSmi, 0, 10));
        chunk
            .instructions
            .push(Instruction::new_ri(Opcode::LoadSmi, 1, 5));
        chunk
            .instructions
            .push(Instruction::new_rrr(Opcode::Add, 2, 0, 1));
        chunk.instructions.push(Instruction::new_r(Opcode::Ret, 2));
        chunk.register_count = 3;

        let result = compiler.compile(&chunk, 1);
        assert!(result.is_ok(), "Arithmetic compilation should succeed");
    }

    #[test]
    fn test_jit_execute_simple() {
        // UNWRAP-OK: native test host always has a working ISA + Cranelift
        // JIT module; failure indicates a broken developer environment.
        let mut compiler = JitCompiler::new().unwrap();

        // LoadSmi r0, 42
        // Ret r0
        let mut chunk = Chunk::default();
        chunk
            .instructions
            .push(Instruction::new_ri(Opcode::LoadSmi, 0, 42));
        chunk.instructions.push(Instruction::new_r(Opcode::Ret, 0));
        chunk.register_count = 1;

        // UNWRAP-OK: chunk uses only LoadSmi + Ret (both supported); compile
        // never returns Err for this trivial input on a working host.
        let compiled = compiler.compile(&chunk, 2).unwrap();

        // SAFETY: compiled.ptr was just produced by JitCompiler::compile() above
        // and lives inside compiler's JITModule (not yet dropped). The chunk
        // returns i64 and takes no args, matching the fn() -> i64 transmute
        // inside CompiledFunction::call.
        // Execute the compiled function
        let result = unsafe { compiled.call() };
        assert_eq!(result, 42, "JIT should return 42");
    }

    #[test]
    fn test_jit_execute_arithmetic() {
        // UNWRAP-OK: native test host always has a working ISA + Cranelift
        // JIT module; failure indicates a broken developer environment.
        let mut compiler = JitCompiler::new().unwrap();

        // LoadSmi r0, 10
        // LoadSmi r1, 32
        // Add r2, r0, r1
        // Ret r2
        let mut chunk = Chunk::default();
        chunk
            .instructions
            .push(Instruction::new_ri(Opcode::LoadSmi, 0, 10));
        chunk
            .instructions
            .push(Instruction::new_ri(Opcode::LoadSmi, 1, 32));
        chunk
            .instructions
            .push(Instruction::new_rrr(Opcode::Add, 2, 0, 1));
        chunk.instructions.push(Instruction::new_r(Opcode::Ret, 2));
        chunk.register_count = 3;

        // UNWRAP-OK: chunk uses only LoadSmi + Add + Ret (all supported);
        // compile cannot return Err for this trivial input on a working host.
        let compiled = compiler.compile(&chunk, 3).unwrap();

        // SAFETY: compiled.ptr was just produced by JitCompiler::compile() above
        // and lives inside compiler's JITModule. The chunk returns i64 and takes
        // no args, matching the fn() -> i64 transmute in CompiledFunction::call.
        let result = unsafe { compiled.call() };
        assert_eq!(result, 42, "10 + 32 should equal 42");
    }

    #[test]
    fn test_jit_cache() {
        // UNWRAP-OK: native test host always has a working ISA + Cranelift
        // JIT module; failure indicates a broken developer environment.
        let mut compiler = JitCompiler::new().unwrap();

        let mut chunk = Chunk::default();
        chunk
            .instructions
            .push(Instruction::new_ri(Opcode::LoadSmi, 0, 1));
        chunk.instructions.push(Instruction::new_r(Opcode::Ret, 0));
        chunk.register_count = 1;

        // UNWRAP-OK: trivial chunk (LoadSmi + Ret), supported opcodes only.
        // First compile
        compiler.compile(&chunk, 10).unwrap();
        assert!(compiler.is_compiled(10));

        // UNWRAP-OK: second call hits the cache and returns Ok unconditionally
        // (existing entry path in JitCompiler::compile).
        // Second compile should hit cache
        compiler.compile(&chunk, 10).unwrap();
        assert_eq!(compiler.stats().compiled_functions, 1);
    }
}
