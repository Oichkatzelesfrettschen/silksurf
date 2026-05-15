//! C Foreign Function Interface for `SilkSurfJS`
//!
//! This module provides a C-compatible API for embedding `SilkSurfJS`
//! in non-Rust applications. The API follows a handle-based design
//! where opaque pointers represent engine state.
//!
//! # Thread Safety
//!
//! Currently single-threaded. Each engine instance must be used from
//! one thread only. Future versions may add thread-safe variants.
//!
//! # Error Handling
//!
//! Functions return status codes or null pointers on error.
//! Use `silksurf_last_error()` to get error details.

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char, c_int};
use std::ptr;

use crate::bytecode::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::parser::ast_arena::AstArena;
use crate::vm::Vm;

/// Opaque engine handle
pub struct SilkSurfEngine {
    vm: Vm,
}

/// Opaque compiled script handle
pub struct SilkSurfScript {
    chunk_idx: usize,
}

/// Result value from script execution
#[repr(C)]
pub struct SilkSurfValue {
    /// Type tag: 0=undefined, 1=null, 2=bool, 3=number, 4=string, 5=object
    pub tag: c_int,
    /// Numeric value (for bool: 0/1, for number: the value)
    pub number: f64,
    /// String value (null-terminated, owned by engine)
    pub string: *const c_char,
}

/// Status codes
#[repr(C)]
pub enum SilkSurfStatus {
    Ok = 0,
    ErrorParse = 1,
    ErrorCompile = 2,
    ErrorRuntime = 3,
    ErrorMemory = 4,
    ErrorInvalidArg = 5,
}

// Thread-local error storage
thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

fn set_error(msg: &str) {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = CString::new(msg).ok();
    });
}

/// Get the last error message, or null if no error.
/// The returned string is valid until the next API call.
#[unsafe(no_mangle)]
pub extern "C" fn silksurf_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ref().map_or(ptr::null(), |s| s.as_ptr()))
}

/// Get the library version string.
#[unsafe(no_mangle)]
pub extern "C" fn silksurf_version() -> *const c_char {
    static VERSION: &[u8] = b"0.1.0\0";
    VERSION.as_ptr().cast::<c_char>()
}

/// Create a new engine instance.
/// Returns null on failure.
#[unsafe(no_mangle)]
pub extern "C" fn silksurf_engine_new() -> *mut SilkSurfEngine {
    if let Ok(engine) = std::panic::catch_unwind(|| Box::new(SilkSurfEngine { vm: Vm::new() })) {
        Box::into_raw(engine)
    } else {
        set_error("Failed to create engine");
        ptr::null_mut()
    }
}

/// Destroy an engine instance.
/// Safe to call with null.
#[unsafe(no_mangle)]
pub extern "C" fn silksurf_engine_free(engine: *mut SilkSurfEngine) {
    if !engine.is_null() {
        // SAFETY: null-checked above; pointer must originate from a prior
        // silksurf_engine_new() Box::into_raw call (FFI contract).
        unsafe {
            drop(Box::from_raw(engine));
        }
    }
}

/// Compile JavaScript source code to a script handle.
/// Returns null on parse/compile error.
#[unsafe(no_mangle)]
pub extern "C" fn silksurf_compile(
    engine: *mut SilkSurfEngine,
    source: *const c_char,
) -> *mut SilkSurfScript {
    if engine.is_null() || source.is_null() {
        set_error("Null engine or source pointer");
        return ptr::null_mut();
    }

    // SAFETY: source null-checked above; caller must pass a valid
    // NUL-terminated C string per FFI contract on silksurf_compile.
    let Ok(source_str) = (unsafe { CStr::from_ptr(source) }).to_str() else {
        set_error("Invalid UTF-8 in source");
        return ptr::null_mut();
    };

    // Lex - check for errors
    let lexer = Lexer::new(source_str);
    for token in lexer {
        if let crate::lexer::TokenKind::Error(e) = &token.kind {
            set_error(&format!("Lexer error: {e}"));
            return ptr::null_mut();
        }
    }

    // Parse
    let ast_arena = AstArena::new();
    let parser = Parser::new(source_str, &ast_arena);
    let (ast, errors) = parser.parse();
    if !errors.is_empty() {
        set_error(&format!("Parse error: {:?}", errors[0]));
        return ptr::null_mut();
    }

    // Compile
    let compiler = Compiler::new();
    let chunk = match compiler.compile(&ast) {
        Ok(c) => c,
        Err(e) => {
            set_error(&format!("Compile error: {e:?}"));
            return ptr::null_mut();
        }
    };

    // SAFETY: engine null-checked at function entry; pointer must come from
    // silksurf_engine_new() and be exclusively owned by the caller (no aliasing).
    let engine = unsafe { &mut *engine };
    let chunk_idx = engine.vm.add_chunk(chunk);

    Box::into_raw(Box::new(SilkSurfScript { chunk_idx }))
}

/// Free a compiled script.
/// Safe to call with null.
#[unsafe(no_mangle)]
pub extern "C" fn silksurf_script_free(script: *mut SilkSurfScript) {
    if !script.is_null() {
        // SAFETY: null-checked above; pointer must originate from a prior
        // silksurf_compile() Box::into_raw call (FFI contract, called once).
        unsafe {
            drop(Box::from_raw(script));
        }
    }
}

/// Execute a compiled script.
/// Returns status code.
#[unsafe(no_mangle)]
pub extern "C" fn silksurf_run(
    engine: *mut SilkSurfEngine,
    script: *mut SilkSurfScript,
) -> SilkSurfStatus {
    if engine.is_null() || script.is_null() {
        set_error("Null engine or script pointer");
        return SilkSurfStatus::ErrorInvalidArg;
    }

    // SAFETY: both pointers null-checked above; engine/script must originate
    // from silksurf_engine_new()/silksurf_compile() and be uniquely owned.
    let engine = unsafe { &mut *engine };
    // SAFETY: script null-checked above; pointer comes from silksurf_compile()
    // and remains valid until silksurf_script_free() is called.
    let script = unsafe { &*script };

    match engine.vm.execute(script.chunk_idx) {
        Ok(_) => SilkSurfStatus::Ok,
        Err(e) => {
            set_error(&format!("Runtime error: {e:?}"));
            SilkSurfStatus::ErrorRuntime
        }
    }
}

/// Evaluate JavaScript source code directly.
/// Convenience wrapper around compile + run.
#[unsafe(no_mangle)]
pub extern "C" fn silksurf_eval(
    engine: *mut SilkSurfEngine,
    source: *const c_char,
) -> SilkSurfStatus {
    let script = silksurf_compile(engine, source);
    if script.is_null() {
        return SilkSurfStatus::ErrorParse;
    }

    let status = silksurf_run(engine, script);
    silksurf_script_free(script);
    status
}

/// Get the number of instructions in a compiled script.
/// Note: Returns chunk index, not instruction count (requires engine access).
#[unsafe(no_mangle)]
pub extern "C" fn silksurf_script_instruction_count(script: *const SilkSurfScript) -> c_int {
    if script.is_null() {
        return -1;
    }
    // Return chunk index as a proxy - full instruction count requires engine access
    // SAFETY: null-checked above; script pointer must come from silksurf_compile()
    // and remain valid (not yet freed) per FFI contract.
    unsafe { (*script).chunk_idx as c_int }
}

/// Trigger garbage collection.
/// Currently a no-op; GC runs automatically when needed.
#[unsafe(no_mangle)]
pub extern "C" fn silksurf_gc(_engine: *mut SilkSurfEngine) {
    // GC is automatic in current implementation
    // Future: Add explicit GC trigger via engine.vm.collect()
}

/// Get heap statistics.
#[repr(C)]
pub struct SilkSurfHeapStats {
    pub bytes_allocated: usize,
    pub bytes_threshold: usize,
    pub gc_count: usize,
}

#[unsafe(no_mangle)]
pub extern "C" fn silksurf_heap_stats(
    _engine: *const SilkSurfEngine,
    stats: *mut SilkSurfHeapStats,
) -> SilkSurfStatus {
    if stats.is_null() {
        return SilkSurfStatus::ErrorInvalidArg;
    }

    // TODO: Wire up actual heap stats when GC tracking is exposed
    // SAFETY: stats null-checked above; pointer must reference a valid,
    // properly aligned, writable SilkSurfHeapStats per FFI contract.
    unsafe {
        (*stats).bytes_allocated = 0;
        (*stats).bytes_threshold = 0;
        (*stats).gc_count = 0;
    }

    SilkSurfStatus::Ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_lifecycle() {
        let engine = silksurf_engine_new();
        assert!(!engine.is_null());
        silksurf_engine_free(engine);
    }

    #[test]
    fn test_version() {
        let version = silksurf_version();
        // SAFETY: silksurf_version() returns a static, NUL-terminated, valid
        // pointer with 'static lifetime; never null and never invalidated.
        // Use unwrap_or to avoid panic if the static were ever non-UTF-8.
        let version_str = unsafe { CStr::from_ptr(version) }
            .to_str()
            .unwrap_or("unknown");
        assert_eq!(version_str, "0.1.0");
    }

    #[test]
    fn test_compile_simple() {
        let engine = silksurf_engine_new();
        // UNWRAP-OK: literal "1 + 2" contains no interior NUL bytes, so
        // CString::new cannot return Err(NulError) here.
        let source = CString::new("1 + 2").unwrap();
        let script = silksurf_compile(engine, source.as_ptr());
        assert!(!script.is_null());

        let count = silksurf_script_instruction_count(script);
        assert!(count >= 0); // chunk_idx starts at 0

        silksurf_script_free(script);
        silksurf_engine_free(engine);
    }

    #[test]
    fn test_null_safety() {
        silksurf_engine_free(ptr::null_mut());
        silksurf_script_free(ptr::null_mut());
        assert_eq!(silksurf_script_instruction_count(ptr::null()), -1);
    }

    #[test]
    fn test_heap_stats() {
        let engine = silksurf_engine_new();
        let mut stats = SilkSurfHeapStats {
            bytes_allocated: 0,
            bytes_threshold: 0,
            gc_count: 0,
        };
        let status = silksurf_heap_stats(engine, &mut stats);
        assert!(matches!(status, SilkSurfStatus::Ok));
        silksurf_engine_free(engine);
    }
}
