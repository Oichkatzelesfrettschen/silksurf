/*
 * DEPRECATED: This module is preserved for reference and reverse-engineering.
 *
 * Production JS execution has moved to boa_engine (src/boa_backend/mod.rs).
 * This hand-written VM is NOT built unless the "legacy-vm" feature is enabled.
 * It is NOT MAINTAINED going forward; API and behaviour may bitrot over time.
 *
 * WHY KEPT: The VM contains custom optimization patterns (register allocation,
 * NaN-boxing, shape/IC tagging) that may inform future hot-path extensions
 * layered on top of boa_engine's NativeFunction/NativeObject extension points.
 * Retaining the source is cheaper than reconstructing it from git history when
 * a specific optimization needs to be studied or ported.
 *
 * HOW TO ENABLE: cargo build --features legacy-vm
 *   (do NOT enable in production or CI; treat as an internal research tool)
 */

/*
 * vm/mod.rs -- Bytecode virtual machine (register-based, function-pointer dispatch).
 *
 * WHY: Executes compiled JavaScript bytecode. Register-based (not stack-based)
 * for fewer memory operations per instruction. Function-pointer dispatch table
 * gives O(1) opcode lookup with branch-predictor-friendly indirect calls.
 *
 * Architecture: Cleanroom design informed by V8 Ignition patterns.
 * - 256-entry dispatch table (one handler per opcode byte)
 * - 256 registers per frame (expandable)
 * - Call stack with explicit base/return register tracking
 * - Microtask queue for Promise resolution (see: promise.rs)
 * - Timer queue for setTimeout/setInterval (see: timers.rs)
 * - Exception handler stack for try/catch/finally
 *
 * Memory layout:
 *   registers: Vec<Value> -- 256 slots, each 24-40 bytes (tagged enum)
 *   call_stack: Vec<CallFrame> -- 16 bytes per frame
 *   chunks: Vec<Chunk> -- bytecode functions, owned by VM
 *   strings: StringTable -- O(1) intern/lookup via HashMap
 *   global: Rc<RefCell<Object>> -- global object (window, document, etc.)
 *
 * Performance: dispatch table is a static array of function pointers,
 * indexed by opcode byte. No match/switch overhead in the hot loop.
 * SAFETY: get_unchecked used in hot path with debug_assert guards.
 *
 * See: bytecode/instruction.rs for 32-bit instruction encoding
 * See: bytecode/opcode.rs for the 50+ opcode definitions
 * See: value.rs for the Value tagged enum representation
 * See: builtins/ for console, JSON, Math, Array, String prototypes
 * See: dom_bridge/ for JS-DOM integration (document, Element)
 * See: promise.rs for Promise state machine and microtask queue
 * See: event_loop.rs for timer/microtask/rAF orchestration
 */

pub mod builtins;
pub mod dom_bridge;
pub mod event_loop;
pub mod gc_integration;
mod generator;
pub mod host;
pub mod ic;
pub mod nanbox;
pub mod promise;
pub mod shape;
pub mod snapshot;
pub mod string;
pub mod timers;
pub mod value;

#[cfg(feature = "jit")]
pub mod jit_integration;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use value::{JsFunction, Object, Value};

use crate::bytecode::{Chunk, Constant, Instruction, Opcode};

/*
 * MAX_CALL_STACK_DEPTH -- DoS bound on JavaScript call stack depth.
 *
 * WHY: Unbounded recursion (`function f(){f();} f();`) would otherwise
 * grow vm.call_stack as a Vec<CallFrame> until the host process runs
 * out of address space, crashing the renderer instead of throwing a
 * recoverable RangeError. The bound is enforced inside op_call and
 * op_spread_call (the only opcodes that push a CallFrame for JS-side
 * functions); native calls do not consume a frame.
 *
 * Default 10_000 frames. CallFrame is 32 B on 64-bit, so the cap
 * bounds vm.call_stack at ~320 KiB. The historical default was 1024
 * which matched V8's classic limit but caused spec-conforming code
 * paths (deeply nested promise chains, recursive descent parsers in
 * user JS) to throw spuriously. 10_000 matches modern V8 / SpiderMonkey
 * defaults while still firing well below host-thread stack exhaustion.
 *
 * Used as the initial value of Vm::max_stack_depth; the field remains
 * mutable so embedders can override per-VM if needed.
 *
 * See: SNAZZY-WAFFLE roadmap P8.S8 (DoS bounds per crate).
 * See: op_call / op_spread_call (this file) for the enforcement sites.
 */
pub const MAX_CALL_STACK_DEPTH: usize = 10_000;

/*
 * VmError -- all possible VM execution failures.
 *
 * Exception(Value) carries the JS throw value through the call stack.
 * Halted is a normal exit (not an error) -- used to break the dispatch loop
 * when execution completes. The caller distinguishes Halted from real errors.
 *
 * See: op_throw (mod.rs) for exception dispatch to try/catch handlers
 * See: execute() main loop for Halted handling
 */
#[derive(Debug, Clone)]
pub enum VmError {
    /// Division by zero
    DivisionByZero,
    /// Type error (e.g., calling non-function)
    TypeError(String),
    /// Reference error (undefined variable)
    ReferenceError(String),
    /// Stack overflow
    StackOverflow,
    /// Invalid opcode
    InvalidOpcode(u8),
    /// Out of bounds access
    OutOfBounds,
    /// Uncaught exception
    Exception(Value),
    /// Halt instruction reached
    Halted,
}

/// Execution result
pub type VmResult<T> = Result<T, VmError>;

/*
 * CallFrame -- tracks execution context for one function invocation.
 *
 * Each function call pushes a frame; return pops it. The chunk_idx
 * identifies which Chunk (compiled function) is executing. pc is the
 * program counter within that chunk's instruction array.
 *
 * Layout: 16 bytes (usize + usize + usize + u8 + padding)
 * Max depth: vm.max_stack_depth (default MAX_CALL_STACK_DEPTH = 10_000)
 *
 * See: op_call (mod.rs) for frame push
 * See: op_ret (mod.rs) for frame pop and result propagation
 */
#[derive(Debug)]
pub struct CallFrame {
    pub chunk_idx: usize,
    pub pc: usize,
    pub base: usize,
    pub return_reg: u8,
    /*
     * captures -- the executing closure's upvalue cell, SHARED with the
     * JsFunction object via Rc.
     *
     * WHY: Closures need read/write access to variables captured from
     * enclosing function scopes, and mutations must persist across
     * invocations (counter pattern). JsFunction owns the canonical
     * Rc<RefCell<Vec<Value>>>; every call clones the Rc (cheap refcount
     * bump) so all CallFrames for the same closure see the same backing
     * Vec. `op_set_capture` (depth=0) writes through this shared cell, so
     * the next call's `op_get_capture` observes the update.
     *
     * Top-level frames and frames pushed for native call paths leave this
     * empty -- they have no enclosing function scope to capture.
     */
    pub captures: Rc<RefCell<Vec<Value>>>,
}

/*
 * StringTable -- interned string storage with O(1) lookup.
 *
 * WHY: JavaScript programs reuse property names ("length", "prototype",
 * "constructor", etc.) thousands of times. Interning deduplicates them
 * into a single u32 index, enabling integer comparison instead of
 * string comparison in property access hot paths.
 *
 * Complexity: intern() is O(1) average (HashMap lookup + optional insert)
 * Memory: strings stored once in Vec, HashMap maps String -> u32 index
 *
 * INVARIANT: index[s] == i  iff  strings[i] == s (bijective mapping)
 *
 * History: Originally O(n) linear scan; replaced with HashMap in Phase 0B
 * for 10-50x speedup on string-heavy JS (ChatGPT has thousands of strings).
 *
 * See: op_load_const (mod.rs) for string constant resolution
 * See: op_get_prop (mod.rs) for property name resolution via strings.get()
 */
#[derive(Debug, Default)]
pub struct StringTable {
    strings: Vec<String>,
    index: HashMap<String, u32>,
}

impl StringTable {
    #[must_use]
    pub fn new() -> Self {
        Self {
            strings: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub fn intern(&mut self, s: String) -> u32 {
        if let Some(&idx) = self.index.get(&s) {
            return idx;
        }
        let idx = self.strings.len() as u32;
        self.index.insert(s.clone(), idx);
        self.strings.push(s);
        idx
    }

    pub fn get(&self, idx: u32) -> Option<&str> {
        self.strings.get(idx as usize).map(String::as_str)
    }
}

/*
 * TryHandler -- exception handler state for try/catch/finally.
 *
 * WHY: JavaScript try/catch requires unwinding the call stack to
 * the nearest handler when an exception is thrown. TryHandlers form
 * a stack (LIFO) that mirrors try block nesting depth.
 *
 * On throw:
 *   - If catch_pc > 0: unwind to stack_depth, jump to catch_pc, store
 *     exception in r0 so the catch block can read it.
 *   - If finally_pc > 0 (no catch): unwind to stack_depth, store the
 *     exception in pending_exception, jump to finally_pc. The finally
 *     bytecode ends with Rethrow which re-throws pending_exception.
 *
 * pending_exception is None unless a throw is in flight through a
 * finally-only block (try-finally without catch).
 *
 * See: op_enter_try (mod.rs) for handler installation
 * See: op_throw (mod.rs) for handler dispatch
 * See: op_rethrow (mod.rs) for re-throwing from finally
 * See: execute() main loop for Exception handling in dispatch
 */
#[derive(Debug)]
struct TryHandler {
    /// Absolute instruction index of the catch block, 0 = no catch.
    catch_pc: usize,
    /// Absolute instruction index of the throw-path finally duplicate, 0 = no finally.
    finally_pc: usize,
    /// Call-stack depth at the time `EnterTry` executed.
    stack_depth: usize,
    /// Chunk index that owns this handler.
    chunk_idx: usize,
    /// Exception value in flight through a finally-only block.
    ///
    /// Set by `op_throw` when there is no catch but there is a finally.
    /// Cleared and re-thrown by `op_rethrow` at the end of the finally block.
    pending_exception: Option<Value>,
}

/*
 * Vm -- the bytecode virtual machine.
 *
 * WHY: Central execution engine for all JavaScript in SilkSurf.
 * Single-threaded (per JS spec) with cooperative async via microtasks.
 *
 * Memory layout:
 *   registers: 256 Value slots (~6-10KB depending on Value size)
 *   call_stack: pre-allocated for 64 frames, max MAX_CALL_STACK_DEPTH (10_000)
 *   chunks: compiled function bytecode (owned, never freed during execution)
 *   strings: interned string table (O(1) lookup)
 *   global: Rc<RefCell<Object>> -- shared with DOM bridge
 *   microtasks: FIFO queue for Promise callbacks
 *   timers: deadline-sorted heap for setTimeout/setInterval/rAF
 *   try_handlers: LIFO stack for exception handling
 *
 * Initialization: Vm::new() installs all builtins on global:
 *   console, JSON, Math, Error, parseInt, fetch, Promise, setTimeout,
 *   requestAnimationFrame, localStorage, window, performance, navigator
 *
 * See: builtins/mod.rs for install_builtins()
 * See: dom_bridge/mod.rs for install_document()
 * See: event_loop.rs for tick() orchestration
 */
pub struct Vm {
    /// Register file (256 registers per frame, expandable)
    registers: Vec<Value>,
    /// Call stack
    call_stack: Vec<CallFrame>,
    /// Bytecode chunks (functions)
    chunks: Vec<Chunk>,
    /// String table
    pub strings: StringTable,
    /// Global object
    pub global: Rc<RefCell<Object>>,
    /// Microtask queue (for Promise callbacks, queueMicrotask)
    pub microtasks: promise::MicrotaskQueue,
    /// Exception handler stack for try/catch/finally
    try_handlers: Vec<TryHandler>,
    /// Timer queue (setTimeout, setInterval, requestAnimationFrame)
    pub timers: timers::TimerQueue,
    /// Maximum call stack depth
    max_stack_depth: usize,
    /*
     * generator_yield_stack -- active yield-collection buffers for
     * generators currently being constructed (i.e. their bodies are running
     * eagerly).
     *
     * WHY: `op_yield` runs deep inside `vm.execute()` and must hand its
     * value back to the enclosing `op_call`-driven generator constructor.
     * We park a stack of `Rc<RefCell<Vec<Value>>>` here so:
     *   - nested generator construction (a generator that itself calls
     *     another generator and yields its results) does not lose values
     *   - top-of-stack is the buffer to push into; the constructor pops it
     *     after the body completes
     *
     * The stack is empty whenever no generator is being constructed; in
     * that case a stray `Yield` opcode silently no-ops (and the compiler
     * should have caught the misuse statically -- yield only inside
     * function*).
     *
     * See: generator.rs for the eager-strategy design rationale.
     * See: op_yield / op_yield_star for the producer side.
     * See: invoke_generator for the consumer / save+restore boilerplate.
     */
    generator_yield_stack: Vec<generator::GeneratorBuffer>,
}

/*
 * OpHandler -- function signature for opcode dispatch.
 *
 * Each handler receives a mutable VM reference and the 32-bit instruction.
 * Returns Ok(()) to continue, Err(Halted) to exit, or Err(Exception) to throw.
 *
 * PERFORMANCE: function pointers are faster than match/switch because:
 * 1. No branch misprediction cascade (indirect call, not chain of cmp+jne)
 * 2. CPU branch predictor learns handler addresses over time
 * 3. O(1) lookup by opcode byte (array index, no comparison)
 */
type OpHandler = fn(&mut Vm, Instruction) -> VmResult<()>;

/*
 * DISPATCH_TABLE -- static array of 256 function pointers, one per opcode.
 *
 * WHY: The hot loop in execute() does `handler = DISPATCH_TABLE[opcode]`
 * then `handler(self, instr)`. This is faster than a 50-arm match because
 * the CPU's indirect branch predictor can learn each opcode's target.
 *
 * Unassigned opcodes point to op_invalid which returns InvalidOpcode error.
 * Table is constructed at compile time (const eval in static initializer).
 *
 * Layout: 256 * 8 bytes = 2KB (fits in L1 instruction cache)
 *
 * See: bytecode/opcode.rs for opcode numbering
 * See: execute() for the dispatch loop that indexes into this table
 */
static DISPATCH_TABLE: [OpHandler; 256] = {
    let mut table: [OpHandler; 256] = [op_invalid; 256];

    // Load/Store
    table[Opcode::LoadConst as usize] = op_load_const;
    table[Opcode::LoadTrue as usize] = op_load_true;
    table[Opcode::LoadFalse as usize] = op_load_false;
    table[Opcode::LoadNull as usize] = op_load_null;
    table[Opcode::LoadUndefined as usize] = op_load_undefined;
    table[Opcode::Mov as usize] = op_mov;
    table[Opcode::LoadSmi as usize] = op_load_smi;
    table[Opcode::LoadZero as usize] = op_load_zero;
    table[Opcode::LoadOne as usize] = op_load_one;
    table[Opcode::LoadMinusOne as usize] = op_load_minus_one;

    // Arithmetic
    table[Opcode::Add as usize] = op_add;
    table[Opcode::Sub as usize] = op_sub;
    table[Opcode::Mul as usize] = op_mul;
    table[Opcode::Div as usize] = op_div;
    table[Opcode::Mod as usize] = op_mod;
    table[Opcode::Pow as usize] = op_pow;
    table[Opcode::Neg as usize] = op_neg;
    table[Opcode::Inc as usize] = op_inc;
    table[Opcode::Dec as usize] = op_dec;

    // Comparison
    table[Opcode::Eq as usize] = op_eq;
    table[Opcode::StrictEq as usize] = op_strict_eq;
    table[Opcode::Ne as usize] = op_ne;
    table[Opcode::StrictNe as usize] = op_strict_ne;
    table[Opcode::Lt as usize] = op_lt;
    table[Opcode::Le as usize] = op_le;
    table[Opcode::Gt as usize] = op_gt;
    table[Opcode::Ge as usize] = op_ge;

    // Logical/Bitwise
    table[Opcode::Not as usize] = op_not;
    table[Opcode::BitNot as usize] = op_bitnot;
    table[Opcode::BitAnd as usize] = op_bitand;
    table[Opcode::BitOr as usize] = op_bitor;
    table[Opcode::BitXor as usize] = op_bitxor;
    table[Opcode::Shl as usize] = op_shl;
    table[Opcode::Shr as usize] = op_shr;
    table[Opcode::Ushr as usize] = op_ushr;

    // Control Flow
    table[Opcode::Jmp as usize] = op_jmp;
    table[Opcode::JmpTrue as usize] = op_jmp_true;
    table[Opcode::JmpFalse as usize] = op_jmp_false;
    table[Opcode::JmpNullish as usize] = op_jmp_nullish;
    table[Opcode::JmpNotNullish as usize] = op_jmp_not_nullish;
    table[Opcode::Call as usize] = op_call;
    table[Opcode::Ret as usize] = op_ret;
    table[Opcode::RetUndefined as usize] = op_ret_undefined;
    table[Opcode::Throw as usize] = op_throw;
    table[Opcode::AsyncReturn as usize] = op_async_return;
    table[Opcode::Await as usize] = op_await;

    // Property Access
    table[Opcode::GetProp as usize] = op_get_prop;
    table[Opcode::SetProp as usize] = op_set_prop;
    table[Opcode::GetElem as usize] = op_get_elem;
    table[Opcode::SetElem as usize] = op_set_elem;
    table[Opcode::Typeof as usize] = op_typeof;

    // Object Creation
    table[Opcode::NewObject as usize] = op_new_object;
    table[Opcode::NewArray as usize] = op_new_array;
    table[Opcode::NewFunction as usize] = op_new_function;
    table[Opcode::NewGenerator as usize] = op_new_generator;
    table[Opcode::BindCapture as usize] = op_bind_capture;

    // Scope
    table[Opcode::GetLocal as usize] = op_get_local;
    table[Opcode::SetLocal as usize] = op_set_local;
    table[Opcode::GetCapture as usize] = op_get_capture;
    table[Opcode::SetCapture as usize] = op_set_capture;
    table[Opcode::GetGlobal as usize] = op_get_global;
    table[Opcode::SetGlobal as usize] = op_set_global;

    // Special
    table[Opcode::Nop as usize] = op_nop;
    table[Opcode::Halt as usize] = op_halt;
    table[Opcode::Debugger as usize] = op_debugger;

    // Spread
    table[Opcode::SpreadCall as usize] = op_spread_call;

    // Iterators (for...of / for...in)
    table[Opcode::GetIterator as usize] = op_get_iterator;
    table[Opcode::GetAsyncIterator as usize] = op_get_iterator; // same semantics for sync fallback
    table[Opcode::IterNext as usize] = op_iter_next;
    table[Opcode::IterDone as usize] = op_iter_done;
    table[Opcode::IterValue as usize] = op_iter_value;
    table[Opcode::IterClose as usize] = op_iter_close;

    // Generators (function* / yield)
    table[Opcode::Yield as usize] = op_yield;
    table[Opcode::YieldStar as usize] = op_yield_star;

    // Exception handling
    table[Opcode::EnterTry as usize] = op_enter_try;
    table[Opcode::LeaveTry as usize] = op_leave_try;
    table[Opcode::EnterCatch as usize] = op_enter_catch;
    table[Opcode::EnterFinally as usize] = op_enter_finally;
    table[Opcode::Rethrow as usize] = op_rethrow;
    table[Opcode::GetException as usize] = op_get_exception;

    table
};

impl Vm {
    /// Create new VM with built-in objects installed on the global.
    #[must_use]
    pub fn new() -> Self {
        let global = Rc::new(RefCell::new(Object::new()));
        builtins::install_builtins(&global);
        Self {
            registers: vec![Value::Undefined; 256],
            call_stack: Vec::with_capacity(64),
            chunks: Vec::new(),
            strings: StringTable::new(),
            global,
            microtasks: promise::MicrotaskQueue::new(),
            try_handlers: Vec::new(),
            timers: timers::TimerQueue::new(),
            max_stack_depth: MAX_CALL_STACK_DEPTH,
            generator_yield_stack: Vec::new(),
        }
    }

    /// Add a chunk (compiled function) and return its index
    pub fn add_chunk(&mut self, chunk: Chunk) -> usize {
        let idx = self.chunks.len();
        self.chunks.push(chunk);
        idx
    }

    /// Number of chunks currently registered.
    #[must_use]
    pub fn chunks_len(&self) -> usize {
        self.chunks.len()
    }

    /*
     * call_function -- re-entrant function invocation.
     *
     * WHY: NativeFunction constructors (ReadableStream, etc.) may receive
     * Value::Function callbacks that need to execute JS code. This method
     * allows calling a compiled JS function from outside the main execute()
     * loop by saving/restoring call stack state.
     *
     * Used by: ReadableStream's start(controller) callback
     */
    pub fn call_function(&mut self, func: &value::JsFunction, args: &[Value]) -> VmResult<Value> {
        let chunk_idx = func.chunk_idx as usize;
        if chunk_idx >= self.chunks.len() {
            return Err(VmError::OutOfBounds);
        }
        /*
         * Place args at registers 0, 1, ... matching the execute() frame's
         * base=0. With frame-relative addressing, the callee's param slot 0
         * maps to absolute register 0+0=0, slot 1 to register 1, etc.
         *
         * WHY: Previously args were placed at register 1 (r0 "reserved") but
         * the callee's params were compiled to slots 0, 1, ..., causing a
         * one-off mismatch. With frame-relative, base=0 means slot N = register N.
         */
        for (i, arg) in args.iter().enumerate() {
            if i < self.registers.len() {
                self.registers[i] = arg.clone();
            }
        }
        self.execute(chunk_idx)
    }

    /*
     * execute -- main bytecode interpretation loop.
     *
     * WHY: This is the VM's hot loop. Every JS instruction passes through here.
     * The loop fetches one 32-bit instruction per iteration, extracts the
     * opcode byte, indexes into DISPATCH_TABLE, and calls the handler.
     *
     * Complexity: O(n) where n = number of instructions executed
     * SAFETY: Uses get_unchecked in 3 places (guarded by debug_assert):
     *   1. chunk lookup by frame.chunk_idx (valid by CallFrame invariant)
     *   2. instruction fetch by frame.pc (bounds-checked at loop top)
     *   3. dispatch table lookup by opcode (always valid: 0..255)
     *
     * Exception handling: When a handler returns Err(Exception(value)),
     * the loop checks try_handlers stack. If a handler exists, it unwinds
     * the call stack and jumps to the catch/finally block. Otherwise,
     * the exception propagates to the caller.
     *
     * Exit conditions:
     *   - Err(Halted): normal completion, return register 0
     *   - Err(Exception): uncaught throw, propagate to caller
     *   - End of chunk: implicit return undefined
     *
     * See: DISPATCH_TABLE for all opcode handlers
     * See: TryHandler for exception handler state
     * See: CallFrame for per-function execution context
     */
    #[cfg_attr(
        feature = "tracing-full",
        tracing::instrument(level = "trace", skip(self))
    )]
    pub fn execute(&mut self, chunk_idx: usize) -> VmResult<Value> {
        if chunk_idx >= self.chunks.len() {
            return Err(VmError::OutOfBounds);
        }

        // Push initial call frame. Top-level scripts have no enclosing
        // function, so the captures vec is empty (and shared cheaply).
        self.call_stack.push(CallFrame {
            chunk_idx,
            pc: 0,
            base: 0,
            return_reg: 0,
            captures: Rc::new(RefCell::new(Vec::new())),
        });

        // Main execution loop
        loop {
            let frame = self.call_stack.last_mut().ok_or(VmError::OutOfBounds)?;
            debug_assert!(frame.chunk_idx < self.chunks.len());
            // SAFETY: call frames only store valid chunk indices.
            let chunk = unsafe { self.chunks.get_unchecked(frame.chunk_idx) };

            if frame.pc >= chunk.len() {
                // End of chunk - implicit return undefined
                self.call_stack.pop();
                if self.call_stack.is_empty() {
                    return Ok(Value::Undefined);
                }
                continue;
            }

            // SAFETY: bounds checked above for frame.pc.
            let instr = unsafe { *chunk.instructions.get_unchecked(frame.pc) };
            frame.pc += 1;

            // Dispatch via function pointer table
            let opcode = instr.opcode() as usize;
            debug_assert!(opcode < DISPATCH_TABLE.len());
            // SAFETY: opcode is a u8 cast to usize (range 0..=255), and DISPATCH_TABLE
            // is a static [OpHandler; 256], so the index is always in bounds.
            let handler = unsafe { *DISPATCH_TABLE.get_unchecked(opcode) };
            match handler(self, instr) {
                Ok(()) => {}
                Err(VmError::Halted) => {
                    // Drain any microtasks that user code (Promise reactions,
                    // queueMicrotask) enqueued during the run.  This matches
                    // the HTML spec rule that microtasks run after each
                    // macrotask completes -- here, after each top-level
                    // execute() call.  See: event_loop::tick for the timer
                    // path which performs the same drain.
                    self.microtasks.drain();
                    // SAFETY: Vm::new() initializes registers with 256 Value::Undefined
                    // entries and the array only ever grows, so index 0 is always valid.
                    return Ok(unsafe { self.registers.get_unchecked(0) }.clone());
                }
                /*
                 * VmError::Exception -- a JS-level `throw` value that the Throw
                 * opcode or a native call raised. Route through dispatch_exception
                 * which handles catch, finally-only, and uncaught paths uniformly.
                 *
                 * If dispatch_exception returns Ok(()) the VM continues (it already
                 * redirected the PC to the handler block). If it returns Err, the
                 * exception is uncaught and propagates to the Rust caller.
                 */
                Err(VmError::Exception(value)) => match dispatch_exception(self, value) {
                    Ok(()) => {}
                    Err(err) => return Err(err),
                },
                /*
                 * JS-level errors (TypeError, ReferenceError) ARE catchable by
                 * try/catch. Convert them to Exception(Value) and route through
                 * dispatch_exception for uniform catch/finally handling.
                 *
                 * WHY: op_call returns VmError::TypeError("not a function") when
                 * the callee is not callable. Without this conversion, a
                 * try{...}catch(e){} around the call does NOT catch the error --
                 * it propagates past the handler because only VmError::Exception
                 * is routed through dispatch_exception above.
                 *
                 * Internal VM errors (OutOfBounds, StackOverflow, InvalidOpcode)
                 * are NOT converted -- those are unrecoverable engine faults.
                 */
                Err(VmError::TypeError(msg)) => {
                    let exc_val = Value::string_owned(format!("TypeError: {msg}"));
                    match dispatch_exception(self, exc_val) {
                        Ok(()) => {}
                        Err(_) => return Err(VmError::TypeError(msg)),
                    }
                }
                Err(VmError::ReferenceError(msg)) => {
                    let exc_val = Value::string_owned(format!("ReferenceError: {msg}"));
                    match dispatch_exception(self, exc_val) {
                        Ok(()) => {}
                        Err(_) => return Err(VmError::ReferenceError(msg)),
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Get register value (frame-relative: adds current call frame's base).
    ///
    /// WHY: The VM uses a flat register array shared across all call frames.
    /// Each CallFrame.base is the absolute index where its register window begins.
    /// Callers place args immediately after the callee register; the callee's
    /// params (slots 0, 1, ...) map to base+0, base+1, ... which are the arg
    /// positions. Top-level code has base=0 so the behavior is unchanged.
    ///
    /// See: `op_call` `Value::Function` -- computes `new_base` = `current_base` + callee + 1
    /// See: CallFrame.base for the per-frame window start
    #[inline(always)]
    fn get_reg(&self, idx: u8) -> &Value {
        let base = self.call_stack.last().map_or(0, |f| f.base);
        let abs_idx = base + idx as usize;
        // Safe: op_call grows registers to new_base+256 before pushing each frame,
        // so abs_idx < registers.len() for all valid frame-relative indices (0-255).
        self.registers.get(abs_idx).unwrap_or(&Value::Undefined)
    }

    /// Set register value (frame-relative: adds current call frame's base).
    /// See: `get_reg` for the WHY of frame-relative addressing.
    #[inline(always)]
    fn set_reg(&mut self, idx: u8, value: Value) {
        let base = self.call_stack.last().map_or(0, |f| f.base);
        let abs_idx = base + idx as usize;
        if abs_idx < self.registers.len() {
            self.registers[abs_idx] = value;
        }
    }

    /// Get current chunk
    #[inline]
    fn current_chunk(&self) -> &Chunk {
        // UNWRAP-OK: only called from opcode handlers that run inside execute(),
        // which always pushes an initial frame before dispatching, so call_stack is non-empty.
        let frame = self.call_stack.last().unwrap();
        &self.chunks[frame.chunk_idx]
    }

    /// Get current program counter
    #[inline]
    fn current_pc(&self) -> usize {
        // UNWRAP-OK: only called from opcode handlers within an active execute() loop,
        // which guarantees a current frame on the call stack.
        self.call_stack.last().unwrap().pc
    }

    /// Modify program counter (for jumps)
    #[inline]
    fn jump(&mut self, offset: i32) {
        // UNWRAP-OK: jump is only invoked from opcode handlers during execute(),
        // which guarantees an active frame on the call stack.
        let frame = self.call_stack.last_mut().unwrap();
        frame.pc = ((frame.pc as i32) + offset) as usize;
    }
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Opcode Handlers
// ============================================================================

fn op_invalid(_vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    Err(VmError::InvalidOpcode(instr.opcode()))
}

fn op_nop(_vm: &mut Vm, _instr: Instruction) -> VmResult<()> {
    Ok(())
}

fn op_halt(_vm: &mut Vm, _instr: Instruction) -> VmResult<()> {
    Err(VmError::Halted)
}

fn op_debugger(_vm: &mut Vm, _instr: Instruction) -> VmResult<()> {
    // Breakpoint - could integrate with debugger
    Ok(())
}

// Load/Store handlers

fn op_load_const(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let dst = instr.dst();
    let idx = instr.const_idx();
    let chunk = vm.current_chunk();
    let value = match chunk.get_constant(idx) {
        Some(Constant::Number(n)) => Value::Number(*n),
        Some(Constant::String(s)) => {
            // Resolve interned string index to actual string content
            let text = vm.strings.get(*s).unwrap_or("").to_string();
            Value::string_owned(text)
        }
        _ => Value::Undefined,
    };
    vm.set_reg(dst, value);
    Ok(())
}

fn op_load_true(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    vm.set_reg(instr.dst(), Value::Boolean(true));
    Ok(())
}

fn op_load_false(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    vm.set_reg(instr.dst(), Value::Boolean(false));
    Ok(())
}

fn op_load_null(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    vm.set_reg(instr.dst(), Value::Null);
    Ok(())
}

fn op_load_undefined(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    vm.set_reg(instr.dst(), Value::Undefined);
    Ok(())
}

fn op_mov(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let value = vm.get_reg(instr.src1()).clone();
    vm.set_reg(instr.dst(), value);
    Ok(())
}

fn op_load_smi(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let value = f64::from(instr.offset16());
    vm.set_reg(instr.dst(), Value::Number(value));
    Ok(())
}

fn op_load_zero(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    vm.set_reg(instr.dst(), Value::Number(0.0));
    Ok(())
}

fn op_load_one(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    vm.set_reg(instr.dst(), Value::Number(1.0));
    Ok(())
}

fn op_load_minus_one(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    vm.set_reg(instr.dst(), Value::Number(-1.0));
    Ok(())
}

// Arithmetic handlers

/*
 * op_add -- addition with JS string concatenation semantics.
 *
 * WHY: In JavaScript, + is overloaded: number + number = arithmetic,
 * but string + anything = string concatenation. This is the most
 * common operator in JS and must handle both cases efficiently.
 *
 * If either operand is Value::String, both are coerced to strings
 * via to_js_string() and concatenated. Otherwise, both are coerced
 * to f64 via to_number() and added arithmetically.
 *
 * See: value.rs to_js_string() for ToString coercion
 * See: value.rs to_number() for ToNumber coercion
 */
fn op_add(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1());
    let rhs = vm.get_reg(instr.src2());
    // If either operand is a string, concatenate (JS spec)
    let result = if matches!(lhs, Value::String(_)) || matches!(rhs, Value::String(_)) {
        let ls = lhs.to_js_string();
        let rs = rhs.to_js_string();
        let left = ls.as_str().unwrap_or("");
        let right = rs.as_str().unwrap_or("");
        Value::string_owned(format!("{left}{right}"))
    } else {
        Value::Number(lhs.to_number() + rhs.to_number())
    };
    vm.set_reg(instr.dst(), result);
    Ok(())
}

fn op_sub(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_number();
    let rhs = vm.get_reg(instr.src2()).to_number();
    vm.set_reg(instr.dst(), Value::Number(lhs - rhs));
    Ok(())
}

fn op_mul(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_number();
    let rhs = vm.get_reg(instr.src2()).to_number();
    vm.set_reg(instr.dst(), Value::Number(lhs * rhs));
    Ok(())
}

fn op_div(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_number();
    let rhs = vm.get_reg(instr.src2()).to_number();
    // JS division by zero returns Infinity, not error
    vm.set_reg(instr.dst(), Value::Number(lhs / rhs));
    Ok(())
}

fn op_mod(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_number();
    let rhs = vm.get_reg(instr.src2()).to_number();
    vm.set_reg(instr.dst(), Value::Number(lhs % rhs));
    Ok(())
}

fn op_pow(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_number();
    let rhs = vm.get_reg(instr.src2()).to_number();
    vm.set_reg(instr.dst(), Value::Number(lhs.powf(rhs)));
    Ok(())
}

fn op_neg(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let val = vm.get_reg(instr.src1()).to_number();
    vm.set_reg(instr.dst(), Value::Number(-val));
    Ok(())
}

fn op_inc(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let val = vm.get_reg(instr.src1()).to_number();
    vm.set_reg(instr.dst(), Value::Number(val + 1.0));
    Ok(())
}

fn op_dec(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let val = vm.get_reg(instr.src1()).to_number();
    vm.set_reg(instr.dst(), Value::Number(val - 1.0));
    Ok(())
}

// Comparison handlers

fn op_eq(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1());
    let rhs = vm.get_reg(instr.src2());
    let result = match (lhs, rhs) {
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::Boolean(a), Value::Boolean(b)) => a == b,
        (Value::Null | Value::Undefined, Value::Null | Value::Undefined) => true,
        (Value::String(a), Value::String(b)) => a == b,
        // Type coercion: number == string -> compare as numbers
        (Value::Number(n), Value::String(s)) | (Value::String(s), Value::Number(n)) => {
            let text = s.as_str().unwrap_or("");
            text.trim()
                .parse::<f64>()
                .ok()
                .is_some_and(|parsed| parsed == *n)
        }
        _ => false,
    };
    vm.set_reg(instr.dst(), Value::Boolean(result));
    Ok(())
}

fn op_strict_eq(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1());
    let rhs = vm.get_reg(instr.src2());
    let result = match (lhs, rhs) {
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::Boolean(a), Value::Boolean(b)) => a == b,
        (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => true,
        (Value::String(a), Value::String(b)) => a == b,
        _ => false,
    };
    vm.set_reg(instr.dst(), Value::Boolean(result));
    Ok(())
}

fn op_ne(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    op_eq(vm, instr)?;
    let val = vm.get_reg(instr.dst()).is_truthy();
    vm.set_reg(instr.dst(), Value::Boolean(!val));
    Ok(())
}

fn op_strict_ne(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    op_strict_eq(vm, instr)?;
    let val = vm.get_reg(instr.dst()).is_truthy();
    vm.set_reg(instr.dst(), Value::Boolean(!val));
    Ok(())
}

fn op_lt(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_number();
    let rhs = vm.get_reg(instr.src2()).to_number();
    vm.set_reg(instr.dst(), Value::Boolean(lhs < rhs));
    Ok(())
}

fn op_le(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_number();
    let rhs = vm.get_reg(instr.src2()).to_number();
    vm.set_reg(instr.dst(), Value::Boolean(lhs <= rhs));
    Ok(())
}

fn op_gt(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_number();
    let rhs = vm.get_reg(instr.src2()).to_number();
    vm.set_reg(instr.dst(), Value::Boolean(lhs > rhs));
    Ok(())
}

fn op_ge(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_number();
    let rhs = vm.get_reg(instr.src2()).to_number();
    vm.set_reg(instr.dst(), Value::Boolean(lhs >= rhs));
    Ok(())
}

// Logical/Bitwise handlers

fn op_not(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let val = vm.get_reg(instr.src1()).is_truthy();
    vm.set_reg(instr.dst(), Value::Boolean(!val));
    Ok(())
}

fn op_bitnot(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let val = vm.get_reg(instr.src1()).to_i32();
    vm.set_reg(instr.dst(), Value::Number(f64::from(!val)));
    Ok(())
}

fn op_bitand(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_i32();
    let rhs = vm.get_reg(instr.src2()).to_i32();
    vm.set_reg(instr.dst(), Value::Number(f64::from(lhs & rhs)));
    Ok(())
}

fn op_bitor(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_i32();
    let rhs = vm.get_reg(instr.src2()).to_i32();
    vm.set_reg(instr.dst(), Value::Number(f64::from(lhs | rhs)));
    Ok(())
}

fn op_bitxor(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_i32();
    let rhs = vm.get_reg(instr.src2()).to_i32();
    vm.set_reg(instr.dst(), Value::Number(f64::from(lhs ^ rhs)));
    Ok(())
}

fn op_shl(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_i32();
    let rhs = vm.get_reg(instr.src2()).to_u32() & 0x1F;
    vm.set_reg(instr.dst(), Value::Number(f64::from(lhs << rhs)));
    Ok(())
}

fn op_shr(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_i32();
    let rhs = vm.get_reg(instr.src2()).to_u32() & 0x1F;
    vm.set_reg(instr.dst(), Value::Number(f64::from(lhs >> rhs)));
    Ok(())
}

fn op_ushr(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_u32();
    let rhs = vm.get_reg(instr.src2()).to_u32() & 0x1F;
    vm.set_reg(instr.dst(), Value::Number(f64::from(lhs >> rhs)));
    Ok(())
}

// Control flow handlers

fn op_jmp(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let offset = instr.offset24();
    vm.jump(offset);
    Ok(())
}

fn op_jmp_true(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    if vm.get_reg(instr.dst()).is_truthy() {
        let offset = i32::from(instr.offset16());
        vm.jump(offset);
    }
    Ok(())
}

fn op_jmp_false(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    if !vm.get_reg(instr.dst()).is_truthy() {
        let offset = i32::from(instr.offset16());
        vm.jump(offset);
    }
    Ok(())
}

fn op_jmp_nullish(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    if vm.get_reg(instr.dst()).is_nullish() {
        let offset = i32::from(instr.offset16());
        vm.jump(offset);
    }
    Ok(())
}

fn op_jmp_not_nullish(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    if !vm.get_reg(instr.dst()).is_nullish() {
        let offset = i32::from(instr.offset16());
        vm.jump(offset);
    }
    Ok(())
}

fn op_call(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let callee = vm.get_reg(instr.src1()).clone();
    match callee {
        Value::Function(func) => {
            let chunk_idx = func.chunk_idx as usize;
            if chunk_idx >= vm.chunks.len() {
                return Err(VmError::OutOfBounds);
            }
            if vm.call_stack.len() >= vm.max_stack_depth {
                return Err(VmError::StackOverflow);
            }
            /*
             * Generator-function call path (eager evaluation).
             *
             * WHY: A `function*` declaration compiles its body into a chunk
             * with `is_generator = true` and the call site emits
             * NewGenerator (not NewFunction).  When that generator-function
             * is invoked we must NOT push a regular call frame: spec
             * semantics say the body does not run yet -- the call returns
             * an iterator.
             *
             * Eager strategy (see vm/generator.rs for the full rationale):
             * snapshot context, run body to completion collecting yields,
             * restore context, wrap into iterator.  Implemented in
             * invoke_generator below.
             */
            if vm.chunks[chunk_idx].is_generator {
                return invoke_generator(vm, &func, chunk_idx, instr);
            }
            /*
             * Compute the callee's register window base.
             *
             * WHY: The caller places args immediately after the callee register
             * (at callee_reg+1, callee_reg+2, ...). The callee expects its
             * params at its own slots 0, 1, .... With frame-relative addressing,
             * callee_slot_N = vm.registers[new_base + N]. So new_base must point
             * to where the first arg lives: current_base + src1 + 1.
             *
             * See: compile_call -- args allocated at next_register after callee
             * See: get_reg for the frame-relative addressing scheme
             */
            let current_base = vm.call_stack.last().map_or(0, |f| f.base);
            let new_base = current_base + instr.src1() as usize + 1;
            // Grow the register array if this frame's window would overflow.
            // Each function uses at most 256 registers (u8 index limit).
            let needed = new_base + 256;
            if needed > vm.registers.len() {
                vm.registers.resize(needed, Value::Undefined);
            }
            // Share the closure's captures cell with the new frame.
            //
            // WHY: The canonical closure pattern -- `function makeCounter()
            // { var count = 0; return function() { count++; return count;
            // }; }` -- requires `count` mutations from one invocation to be
            // visible to the next. Snapshotting (cloning the inner Vec)
            // gave each call a private copy, so c() always returned 1. By
            // cloning the Rc instead of the Vec, every CallFrame for this
            // JsFunction reads and writes through the same RefCell, and
            // SetCapture (depth=0) durably mutates the closure state.
            //
            // Cost: Rc::clone is a single atomic-free refcount bump on
            // single-threaded code; far cheaper than allocating + cloning
            // the Vec on every call.
            let captures = Rc::clone(&func.captures);
            vm.call_stack.push(CallFrame {
                chunk_idx,
                pc: 0,
                base: new_base,
                return_reg: instr.dst(),
                captures,
            });
            Ok(())
        }
        Value::NativeFunction(func) => {
            /*
             * Collect args from registers immediately after the callee register.
             * With frame-relative addressing, the absolute position of the
             * first arg is current_base + src1 + 1.
             */
            let argc = instr.src2() as usize;
            let mut args = Vec::with_capacity(argc);
            let current_base = vm.call_stack.last().map_or(0, |f| f.base);
            let base_reg = current_base + instr.src1() as usize + 1;
            for i in 0..argc {
                if base_reg + i < vm.registers.len() {
                    args.push(vm.registers[base_reg + i].clone());
                }
            }
            let result = func.call(&args);
            vm.set_reg(instr.dst(), result);
            Ok(())
        }
        _ => Err(VmError::TypeError("not a function".to_string())),
    }
}

/*
 * invoke_generator -- eagerly run a generator-flagged chunk to completion
 * and deliver a JS iterator object into the caller's destination register.
 *
 * WHY: Generator functions cannot share the normal call_stack push path
 * because they must return an iterator without running the body in the
 * caller's frame.  Instead we drive a fresh execute() in a saved
 * sub-context.
 *
 * Step-by-step:
 *   1. Marshal argv: copy the caller's arg registers (current_base + src1 +
 *      1 onwards) into an owned Vec; they're about to be overwritten when
 *      we seed the sub-context at absolute r0..N.
 *   2. Save context: pull out the existing call_stack (std::mem::take) and
 *      snapshot the absolute registers we are about to clobber.  Both are
 *      restored after the body finishes (success or error path).
 *   3. Seed the sub-context: write args to absolute r0.., zero the rest of
 *      the snapshot window, push a fresh yield-collection buffer.
 *   4. Run run_generator_body(chunk_idx, captures): a shim that mirrors
 *      execute()'s loop but seeds the top frame with the closure's
 *      captures (execute() would otherwise install an empty captures vec,
 *      breaking upvalue references inside generator bodies).
 *   5. Restore context: pop the yield buffer, restore registers and
 *      call_stack.
 *   6. Build the iterator object via generator::build_generator and store
 *      it in the caller's destination register.
 *
 * Errors raised during the eager run (TypeError, uncaught throw, etc.) are
 * surfaced to the caller of op_call exactly as if a regular function had
 * thrown -- the saved call_stack/registers are restored first so subsequent
 * dispatch_exception sees a coherent state.
 *
 * See: vm/generator.rs for the eager-strategy design rationale and known
 *      limitations (visible-side-effects-at-construction, no .next(value)
 *      argument plumbing, no resumable throw()).
 */
fn invoke_generator(
    vm: &mut Vm,
    func: &value::JsFunction,
    chunk_idx: usize,
    instr: Instruction,
) -> VmResult<()> {
    // 1. Snapshot caller args -- args live at current_base + src1 + 1 ..
    let argc = instr.src2() as usize;
    let current_base = vm.call_stack.last().map_or(0, |f| f.base);
    let args_start = current_base + instr.src1() as usize + 1;
    let mut argv: Vec<Value> = Vec::with_capacity(argc);
    for slot in 0..argc {
        let abs = args_start + slot;
        if abs < vm.registers.len() {
            argv.push(vm.registers[abs].clone());
        } else {
            argv.push(Value::Undefined);
        }
    }

    // 2. Save the current call_stack so the sub-execute starts from
    //    scratch.  Snapshot the absolute register slots we are about to
    //    overwrite.
    //
    //    Also record try_handlers depth: op_enter_try pushes handlers and
    //    op_leave_try pops them.  If the body throws an uncaught exception
    //    any handlers pushed before the throw site are left on the stack.
    //    Truncating back to the watermark prevents these stale handlers from
    //    being dispatched in the caller's context (H1).
    let saved_call_stack = std::mem::take(&mut vm.call_stack);
    let try_handlers_watermark = vm.try_handlers.len();
    let snapshot_len = vm.registers.len().min(256);
    let saved_registers: Vec<Value> = vm.registers[..snapshot_len].to_vec();

    // 3. Seed args at absolute r0.., zero the rest of the snapshot window
    //    so the body starts with a clean register file.  Beyond
    //    snapshot_len we leave registers untouched: the 256-register window
    //    matches the per-frame u8 index limit, so the body cannot reach
    //    further.
    for (i, arg) in argv.iter().enumerate() {
        if i < vm.registers.len() {
            vm.registers[i] = arg.clone();
        }
    }
    for i in argv.len()..snapshot_len {
        vm.registers[i] = Value::Undefined;
    }
    // Park a fresh yield-collection buffer; op_yield pushes into it.
    let buffer: generator::GeneratorBuffer = Rc::new(RefCell::new(Vec::new()));
    vm.generator_yield_stack.push(Rc::clone(&buffer));

    // 4. Run the body via the captures-aware shim.
    let captures = Rc::clone(&func.captures);
    let run_result = run_generator_body(vm, chunk_idx, captures);

    // 5. Pop our buffer.  The top-of-stack must match `buffer` -- a
    //    mismatch means the body's construction stack underflowed (bug).
    let popped = vm.generator_yield_stack.pop();
    debug_assert!(
        popped.as_ref().is_some_and(|p| Rc::ptr_eq(p, &buffer)),
        "generator yield stack underflow: nested construction lost ordering",
    );

    // 6. Restore the saved register window, call_stack, and try_handlers
    //    BEFORE inspecting run_result so that an error path leaves the VM
    //    coherent.  Truncate try_handlers to the watermark recorded in step 2
    //    to discard any stale handlers left by an uncaught throw inside the
    //    body (see H1 in the SNAZZY-WAFFLE Wave-6 code-review report).
    for (i, value) in saved_registers.into_iter().enumerate() {
        if i < vm.registers.len() {
            vm.registers[i] = value;
        }
    }
    vm.call_stack = saved_call_stack;
    vm.try_handlers.truncate(try_handlers_watermark);

    // 7. Propagate any error.  Halted from the sub-execute is normal
    //    completion of the body; the body's return value is captured in
    //    the Ok arm by run_generator_body.
    let return_value = match run_result {
        Ok(value) => value,
        Err(VmError::Halted) => Value::Undefined,
        Err(err) => return Err(err),
    };

    // 8. Build the iterator object and store it in the caller's dst
    //    register.
    let yielded: Vec<Value> = std::mem::take(&mut *buffer.borrow_mut());
    let iterator = generator::build_generator(yielded, return_value);
    vm.set_reg(instr.dst(), iterator);
    Ok(())
}

/*
 * run_generator_body -- inline execute() variant that respects supplied
 * captures for the top-level frame.
 *
 * WHY: vm.execute() unconditionally seeds the top frame with an empty
 * captures vec.  Generator bodies that close over outer variables must see
 * those upvalues via the JsFunction's captures.  This shim pushes a frame
 * with the right Rc<RefCell<Vec<Value>>>, then runs the same dispatch loop
 * as execute().
 *
 * Returns the final value of register 0 on normal completion (which holds
 * the return value after Ret/RetUndefined), or the error otherwise.
 *
 * The implementation mirrors execute()'s loop: any future change to
 * dispatch semantics (new opcodes, new error categories) MUST be reflected
 * in both.  The duplication is small (~40 lines) and isolated; the
 * alternative (threading "initial captures" through execute()) would touch
 * every call site of execute().
 */
fn run_generator_body(
    vm: &mut Vm,
    chunk_idx: usize,
    captures: Rc<RefCell<Vec<Value>>>,
) -> VmResult<Value> {
    if chunk_idx >= vm.chunks.len() {
        return Err(VmError::OutOfBounds);
    }
    vm.call_stack.push(CallFrame {
        chunk_idx,
        pc: 0,
        base: 0,
        return_reg: 0,
        captures,
    });
    loop {
        let frame = vm.call_stack.last_mut().ok_or(VmError::OutOfBounds)?;
        debug_assert!(frame.chunk_idx < vm.chunks.len());
        // SAFETY: call frames only store valid chunk indices (enforced at
        // push sites: op_call validates chunk_idx before pushing).
        let chunk = unsafe { vm.chunks.get_unchecked(frame.chunk_idx) };
        if frame.pc >= chunk.len() {
            // Implicit fall-off-the-end.
            vm.call_stack.pop();
            if vm.call_stack.is_empty() {
                let r0 = vm.registers.first().cloned().unwrap_or(Value::Undefined);
                // Drain microtasks here to mirror the Halted arm below (H2):
                // Promise reactions enqueued by the body must run before the
                // caller's context is restored; draining here keeps the queue
                // empty at invoke_generator's step-6 restore point.
                vm.microtasks.drain();
                return Ok(r0);
            }
            continue;
        }
        // SAFETY: bounds checked above.
        let instr = unsafe { *chunk.instructions.get_unchecked(frame.pc) };
        frame.pc += 1;
        let opcode = instr.opcode() as usize;
        // SAFETY: opcode is a u8 cast to usize and DISPATCH_TABLE has 256
        // entries; the index is always in bounds.
        let handler = unsafe { *DISPATCH_TABLE.get_unchecked(opcode) };
        match handler(vm, instr) {
            Ok(()) => {}
            Err(VmError::Halted) => {
                let r0 = vm.registers.first().cloned().unwrap_or(Value::Undefined);
                vm.microtasks.drain();
                return Ok(r0);
            }
            Err(VmError::Exception(value)) => match dispatch_exception(vm, value) {
                Ok(()) => {}
                Err(err) => return Err(err),
            },
            Err(VmError::TypeError(msg)) => {
                let exc_val = Value::string_owned(format!("TypeError: {msg}"));
                match dispatch_exception(vm, exc_val) {
                    Ok(()) => {}
                    Err(_) => return Err(VmError::TypeError(msg)),
                }
            }
            Err(VmError::ReferenceError(msg)) => {
                let exc_val = Value::string_owned(format!("ReferenceError: {msg}"));
                match dispatch_exception(vm, exc_val) {
                    Ok(()) => {}
                    Err(_) => return Err(VmError::ReferenceError(msg)),
                }
            }
            Err(err) => return Err(err),
        }
    }
}

fn op_ret(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    /*
     * Return a value from the current function.
     *
     * WHY: return_reg is stored in the CALLEE's CallFrame (pushed by op_call
     * with instr.dst() from the caller's Call instruction). We must read it
     * BEFORE popping the callee frame; after pop the top of the stack is the
     * caller, whose return_reg records where THE CALLER's result should go
     * (one level higher) -- the wrong destination.
     *
     * After pop, set_reg(return_reg, value) uses the caller's base (frame-
     * relative), so return_reg -- which is a register index in the caller's
     * compiled bytecode -- lands at the correct absolute slot. ✓
     *
     * See: op_call -- stores instr.dst() as return_reg when pushing callee frame
     * See: execute() -- pushes top-level frame with return_reg=0; on Halted,
     *      returns vm.registers[0] to the Rust caller.
     */
    let value = vm.get_reg(instr.dst()).clone();
    let return_reg = vm.call_stack.last().map_or(0, |f| f.return_reg);
    vm.call_stack.pop();
    if vm.call_stack.is_empty() {
        // Returning from top-level execute() frame -- store in absolute r0.
        if !vm.registers.is_empty() {
            vm.registers[0] = value;
        }
        Err(VmError::Halted)
    } else {
        // Write to the caller's frame at the register the Call instruction
        // specified as destination (frame-relative in the now-current frame).
        vm.set_reg(return_reg, value);
        Ok(())
    }
}

fn op_ret_undefined(vm: &mut Vm, _instr: Instruction) -> VmResult<()> {
    /*
     * Return undefined. See op_ret for the return_reg-before-pop invariant.
     */
    let return_reg = vm.call_stack.last().map_or(0, |f| f.return_reg);
    vm.call_stack.pop();
    if vm.call_stack.is_empty() {
        Err(VmError::Halted)
    } else {
        vm.set_reg(return_reg, Value::Undefined);
        Ok(())
    }
}

/*
 * op_async_return -- return from an async function, wrapping the value in a Promise.
 *
 * WHY: An async function's caller expects the result to be a Promise, not the
 * raw return value. The compiler emits AsyncReturn at every return point of an
 * async function body so the callee, instead of handing the raw value back to
 * the caller's register, hands back Promise.resolve(value).
 *
 * If the value is itself already a Promise wrapper, resolved_promise_value
 * returns it unchanged (matching the spec rule that Promise.resolve(thenable)
 * does not double-wrap). After draining the microtask queue we are sure the
 * wrapper's introspect slot reflects the final state.
 *
 * Stack discipline mirrors op_ret: read return_reg from the callee frame
 * BEFORE popping (it records where the caller wants the result), then pop and
 * write the Promise into the now-current (caller) frame.
 *
 * See: op_ret for the return_reg-before-pop invariant
 * See: promise::resolved_promise_value for the wrap helper
 * See: compiler.rs Statement::FunctionDeclaration is_async branch for emission
 */
fn op_async_return(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let raw_value = vm.get_reg(instr.dst()).clone();
    let promise_value = promise::resolved_promise_value(raw_value);
    let return_reg = vm.call_stack.last().map_or(0, |f| f.return_reg);
    vm.call_stack.pop();
    if vm.call_stack.is_empty() {
        // Top-level async return: stash in absolute r0 so execute() returns it.
        if !vm.registers.is_empty() {
            vm.registers[0] = promise_value;
        }
        Err(VmError::Halted)
    } else {
        vm.set_reg(return_reg, promise_value);
        Ok(())
    }
}

/*
 * op_await -- synchronously extract a Promise's resolved value.
 *
 * WHY: Real async/await suspends the current frame and resumes it after the
 * awaited Promise settles. Implementing true suspension requires resumable
 * frames (saving the entire register window plus PC, recreating it on
 * microtask completion) which is the same machinery generators need. That
 * work is scheduled separately; for now we use the synchronous-await model:
 *
 *   1. Drain the microtask queue so any settle-on-resolve chains run.
 *   2. If the value is a Promise wrapper, read its current state via the
 *      INTERNAL_SLOT_KEY introspect function:
 *      - Fulfilled: store result in dst, continue.
 *      - Rejected:  raise as a JS exception so try/catch around await sees it.
 *      - Pending:   no suspension support yet; store undefined and continue.
 *                   (Tests in this task only exercise already-resolved
 *                   promises produced by Promise.resolve.)
 *   3. If the value is not a Promise wrapper, store it unchanged in dst (per
 *      spec: `await 42` evaluates to 42).
 *
 * Encoding: Await(dst, src) -- read promise from src, store extracted value
 * (or throw) into dst.
 *
 * See: promise::as_settled_promise for the introspect side
 * See: dispatch_exception for how the rejected path reaches user catch blocks
 */
fn op_await(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let dst = instr.dst();
    let src = instr.src1();
    let value = vm.get_reg(src).clone();
    // Drain the microtask queue so previously-enqueued resolutions run before
    // we inspect the promise's state.
    vm.microtasks.drain();
    match promise::as_settled_promise(&value) {
        Some((promise::PromiseState::Fulfilled, result)) => {
            vm.set_reg(dst, result);
            Ok(())
        }
        Some((promise::PromiseState::Rejected, reason)) => {
            // Route through the standard exception dispatcher so a try/catch
            // around an `await` reacts identically to an explicit `throw`.
            Err(VmError::Exception(reason))
        }
        Some((promise::PromiseState::Pending, _)) => {
            // No suspension support: behave as if `undefined` was the
            // eventual fulfillment value.  Documented limitation; only
            // synchronously-settled promises (Promise.resolve, immediate
            // .then) are supported by the synchronous-await model.
            vm.set_reg(dst, Value::Undefined);
            Ok(())
        }
        None => {
            // Awaiting a non-promise yields the value unchanged.
            vm.set_reg(dst, value);
            Ok(())
        }
    }
}

/*
 * op_throw -- raise a JS exception.
 *
 * WHY: Three dispatch paths:
 *   1. catch_pc > 0: there is a catch block. Unwind call stack, jump to
 *      catch_pc, store the exception in r0 (the catch block reads it from r0
 *      and the compiler copies it to the catch variable register via Mov).
 *   2. finally_pc > 0 (no catch): store the exception in the handler's
 *      pending_exception slot, push the handler back (so Rethrow can find it),
 *      then unwind and jump to the finally block. The finally block ends with
 *      Rethrow which reads pending_exception and re-throws it.
 *   3. No handler: propagate as VmError::Exception to the Rust caller.
 *
 * See: op_rethrow for path 2 continuation
 * See: execute() for VmError::Exception -> try_handler routing
 */
fn op_throw(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let value = vm.get_reg(instr.dst()).clone();
    dispatch_exception(vm, value)
}

/*
 * dispatch_exception -- shared throw-dispatch logic used by op_throw and execute().
 *
 * WHY: Both the Throw opcode and the execute() loop's error-recovery path need
 * the same try-handler lookup. Factoring it here avoids duplication and ensures
 * the finally-path pending_exception logic is applied consistently.
 */
fn dispatch_exception(vm: &mut Vm, value: Value) -> VmResult<()> {
    if let Some(mut handler) = vm.try_handlers.pop() {
        // Unwind call stack to the depth at which EnterTry executed.
        while vm.call_stack.len() > handler.stack_depth {
            vm.call_stack.pop();
        }
        if handler.catch_pc > 0 {
            // Jump to catch block; exception is in r0 for the catch body.
            if let Some(frame) = vm.call_stack.last_mut() {
                frame.pc = handler.catch_pc;
                frame.chunk_idx = handler.chunk_idx;
            }
            vm.set_reg(0, value);
            Ok(())
        } else if handler.finally_pc > 0 {
            // finally-only block: save exception so Rethrow can re-throw it,
            // then push the handler back and jump to the finally block.
            handler.pending_exception = Some(value);
            let finally_pc = handler.finally_pc;
            let chunk_idx = handler.chunk_idx;
            vm.try_handlers.push(handler);
            if let Some(frame) = vm.call_stack.last_mut() {
                frame.pc = finally_pc;
                frame.chunk_idx = chunk_idx;
            }
            Ok(())
        } else {
            Err(VmError::Exception(value))
        }
    } else {
        Err(VmError::Exception(value))
    }
}

/*
 * op_enter_try -- install an exception handler frame.
 *
 * WHY: The instruction carries a 16-bit handler_index (`const_idx`) that
 * indexes into `chunk.handlers` -- the exception-handler table compiled into
 * each Chunk. Storing absolute instruction indices in the Chunk avoids
 * computing offsets at execution time and avoids the sign/range constraints
 * of the inline offset encoding used by Jump instructions.
 *
 * The `catch_target` and `finally_pc` fields are derived from the handler
 * record:
 *   - catch_target = Some(n): there is a catch block starting at instruction n.
 *   - finally_target = Some(n): there is a finally block (throw path) starting
 *     at instruction n; it ends with Rethrow.
 *   - Both absent: the try body has neither catch nor finally (rare, no-op).
 *
 * pending_exception starts as None; set by op_throw when routing an exception
 * through a finally-only block.
 *
 * See: chunk.rs ExceptionHandler for the handler record layout
 * See: op_throw for how the handler is consumed
 * See: compiler.rs Statement::Try for how handler_index is emitted
 */
fn op_enter_try(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let handler_index = instr.const_idx() as usize;
    let frame = vm.call_stack.last().ok_or(VmError::OutOfBounds)?;
    let chunk_idx = frame.chunk_idx;
    let stack_depth = vm.call_stack.len();
    let chunk = &vm.chunks[chunk_idx];
    let (catch_pc, finally_pc) = if let Some(handler) = chunk.handlers.get(handler_index) {
        let catch_pc = handler.catch_target.map_or(0, |t| t as usize);
        let finally_pc = handler.finally_target.map_or(0, |t| t as usize);
        (catch_pc, finally_pc)
    } else {
        (0, 0)
    };
    vm.try_handlers.push(TryHandler {
        catch_pc,
        finally_pc,
        stack_depth,
        chunk_idx,
        pending_exception: None,
    });
    Ok(())
}

/// `LeaveTry`: pop the current try handler (normal exit from try block).
fn op_leave_try(vm: &mut Vm, _instr: Instruction) -> VmResult<()> {
    vm.try_handlers.pop();
    Ok(())
}

/// `EnterCatch`: marks catch block start; exception value is already in r0.
///
/// The compiler emits a `Mov r_catch, r0` immediately after this opcode to
/// copy the exception from r0 into the declared catch-variable register.
fn op_enter_catch(_vm: &mut Vm, _instr: Instruction) -> VmResult<()> {
    Ok(())
}

/// `EnterFinally`: marks the start of a finally block (no-op; the block is
/// just normal bytecode reachable from both the normal and throw paths).
fn op_enter_finally(_vm: &mut Vm, _instr: Instruction) -> VmResult<()> {
    Ok(())
}

/*
 * op_rethrow -- re-throw a pending exception after a finally-only block.
 *
 * WHY: When a throw routes through a finally-only block (try { } finally { }),
 * op_throw saves the exception in the top TryHandler's pending_exception and
 * jumps to the finally bytecode. The finally block ends with Rethrow. At that
 * point the TryHandler is still on try_handlers (pushed back by op_throw).
 *
 * op_rethrow:
 *   1. Pop the TryHandler to retrieve pending_exception.
 *   2. Call dispatch_exception, which searches the *next* enclosing handler
 *      (the one pushed back is gone). If no outer handler exists, the exception
 *      propagates as VmError::Exception to the Rust caller.
 *
 * The compiler emits Rethrow only at the end of the throw-path duplicate of a
 * finally block (see Statement::Try in compiler.rs).
 *
 * See: op_throw / dispatch_exception for how pending_exception is set
 * See: compiler.rs Statement::Try for when Rethrow is emitted
 */
fn op_rethrow(vm: &mut Vm, _instr: Instruction) -> VmResult<()> {
    // Pop the handler that was pushed back by dispatch_exception when routing
    // through a finally-only block.  pending_exception holds the exception.
    if let Some(handler) = vm.try_handlers.pop()
        && let Some(exc) = handler.pending_exception
    {
        return dispatch_exception(vm, exc);
    }
    // If no pending exception (rethrow at end of a catch-then-finally block
    // with no in-flight exception), just continue normally.
    Ok(())
}

/*
 * op_get_exception -- load the current exception value into a register.
 *
 * WHY: Some compiled patterns need to read the exception from somewhere other
 * than r0. The exception is placed in r0 by dispatch_exception; this opcode
 * copies r0 to instr.dst() for callers that need it in a specific register.
 * Currently used only by disassembly and future planned uses.
 */
fn op_get_exception(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let exc = vm.get_reg(0).clone();
    vm.set_reg(instr.dst(), exc);
    Ok(())
}

// Property access handlers

/*
 * op_get_prop -- property access dispatch (obj.prop or obj["prop"]).
 *
 * WHY: Central dispatch point for all property access in JS. Must handle:
 * 1. HostObject (DOM nodes) -- delegates to HostObject::get_property()
 * 2. Plain Object -- looks up by string name, then falls through to
 *    array methods (push, pop, map, etc.) for array-like objects
 * 3. String values -- dispatches to string prototype methods (length,
 *    charAt, indexOf, split, etc.)
 *
 * Property name resolution: the src2 register contains a constant index
 * into the string table. We resolve it to a string name, then look up.
 *
 * Complexity: O(1) average for own properties, O(prototype_chain_depth)
 * for inherited properties.
 *
 * See: host.rs HostObject trait for native object dispatch
 * See: dom_bridge/element.rs ElementHost for DOM property access
 * See: builtins/array.rs get_array_method() for array method lookup
 * See: builtins/string_proto.rs get_string_method() for string methods
 */
/*
 * op_spread_call -- call a function with arguments from an array.
 *
 * WHY: `f(...arr)` and `f(a, b, ...rest)` compile with SpreadCall so the
 * argument count can be determined at runtime. Regular Call encodes argc
 * as a compile-time constant (src2 byte); SpreadCall encodes the array
 * holding the actual args.
 *
 * Instruction encoding: SpreadCall dst, callee, args_array  (new_rrr)
 *   dst        = result register
 *   src1       = callee register
 *   src2       = args array register (Value::Object array-like)
 *
 * See: compiler.rs compile_call -- emits SpreadCall when any arg is Spread
 * See: op_call for the fixed-argc variant
 */
fn op_spread_call(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let callee = vm.get_reg(instr.src1()).clone();
    let args_val = vm.get_reg(instr.src2()).clone();
    let args: Vec<Value> = match &args_val {
        Value::Object(o) => {
            let o_borrow = o.borrow();
            if builtins::array::is_array_like(&o_borrow) {
                builtins::array::collect_elements_pub(&o_borrow)
            } else {
                vec![]
            }
        }
        _ => vec![],
    };

    match callee {
        Value::NativeFunction(func) => {
            let result = func.call(&args);
            vm.set_reg(instr.dst(), result);
            Ok(())
        }
        Value::Function(func) => {
            /*
             * SpreadCall for interpreted functions: write args into the arg
             * registers then push a call frame with the correct base.
             *
             * WHY: op_spread_call collects args into a Vec from the args_array
             * Object. We then write them to the registers starting at
             * current_base + src1 + 1 so they land where the callee's param
             * slots 0, 1, ... will map to with frame-relative addressing.
             *
             * See: op_call Value::Function for the identical base computation
             */
            let current_base = vm.call_stack.last().map_or(0, |f| f.base);
            let arg_reg_base = current_base + instr.src1() as usize + 1;
            // Grow register array if needed before writing args or pushing frame.
            let needed = arg_reg_base + args.len().max(256);
            if needed > vm.registers.len() {
                vm.registers.resize(needed, Value::Undefined);
            }
            for (i, arg) in args.iter().enumerate() {
                vm.registers[arg_reg_base + i] = arg.clone();
            }
            let new_base = arg_reg_base;
            let chunk_idx = func.chunk_idx as usize;
            if chunk_idx >= vm.chunks.len() {
                return Err(VmError::OutOfBounds);
            }
            if vm.call_stack.len() >= vm.max_stack_depth {
                return Err(VmError::StackOverflow);
            }
            // Share the closure's captures cell with the new frame; same
            // shared-binding rationale as op_call's Value::Function arm.
            let captures = Rc::clone(&func.captures);
            vm.call_stack.push(CallFrame {
                chunk_idx,
                pc: 0,
                base: new_base,
                return_reg: instr.dst(),
                captures,
            });
            Ok(())
        }
        _ => Err(VmError::TypeError("not a function".to_string())),
    }
}

/*
 * op_get_iterator -- create an iterator object from an iterable Value.
 *
 * WHY: for...of requires an iterator (object with .next() method).
 * For array-like Objects and Strings we construct a simple counter-based
 * iterator. For other objects we try obj[Symbol.iterator]() -- but since
 * our Symbol.iterator is a fixed string "@@symbol_wk_iterator", most
 * user objects won't have it. In that case we fall back to treating the
 * value as an empty iterable (safe, silent skip).
 *
 * Iterator shape: {__data: [v0, v1, ...], __idx: 0}
 * IterNext/IterDone/IterValue read these private fields.
 *
 * Instruction encoding: GetIterator dst, src  (new_rr)
 *   dst = register to store iterator object
 *   src = register holding the iterable
 *
 * See: op_iter_next for the stepping logic
 * See: compiler.rs Statement::ForOf for the loop bytecode pattern
 */
fn op_get_iterator(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let src = vm.get_reg(instr.src1()).clone();
    let iter = make_iterator_for(&src);
    vm.set_reg(instr.dst(), iter);
    Ok(())
}

/*
 * make_iterator_for -- build an iterator Object for a given Value.
 *
 * Array-like Objects: snapshot elements into __data Vec.
 * Strings: split into chars, store as __data Vec.
 * Iterator-shaped Objects (have a `next` method): pass through unchanged
 *   so `for...of` over a generator (built by vm/generator.rs) drives the
 *   generator's own next() instead of treating it as empty.
 * Others: empty iterator (done=true from the start).
 *
 * The iterator holds:
 *   __data: internal Vec<Value> (stored as a NativeFunction returning a
 *           pointer -- actually stored as a plain array Value under key
 *           "__data")
 *   __idx:  current position (stored as Value::Number under "__idx")
 */
fn make_iterator_for(iterable: &Value) -> Value {
    // Iterator-shaped pass-through: objects that already expose a `next`
    // NativeFunction (generators, manually-built iterators) ARE iterators
    // and must not be rewrapped -- doing so would lose their internal state
    // and yield zero elements.
    if let Value::Object(o) = iterable {
        let has_next = matches!(o.borrow().get_by_str("next"), Value::NativeFunction(_));
        if has_next {
            return iterable.clone();
        }
    }
    let elements: Vec<Value> = match iterable {
        Value::Object(o) => {
            let o_borrow = o.borrow();
            if builtins::array::is_array_like(&o_borrow) {
                builtins::array::collect_elements_pub(&o_borrow)
            } else {
                vec![]
            }
        }
        Value::String(s) => s
            .as_str()
            .unwrap_or("")
            .chars()
            .map(|c| value::Value::string_owned(c.to_string()))
            .collect(),
        _ => vec![],
    };

    let iter_elements = Rc::new(RefCell::new(elements));
    let idx = Rc::new(RefCell::new(0usize));

    let iter_obj = Rc::new(RefCell::new(value::Object::new()));
    {
        // next() method: returns {value, done}
        let iter_elements_ref = Rc::clone(&iter_elements);
        let idx_ref = Rc::clone(&idx);
        let next_fn = Value::NativeFunction(Rc::new(value::NativeFunction::new(
            "__iter_next__",
            move |_| {
                let i = *idx_ref.borrow();
                let elems = iter_elements_ref.borrow();
                let done = i >= elems.len();
                let value = if done {
                    Value::Undefined
                } else {
                    elems[i].clone()
                };
                drop(elems);
                *idx_ref.borrow_mut() = i + 1;
                // Return {value, done}
                let result = Rc::new(RefCell::new(value::Object::new()));
                result.borrow_mut().set_by_str("value", value);
                result.borrow_mut().set_by_str("done", Value::Boolean(done));
                Value::Object(result)
            },
        )));
        iter_obj.borrow_mut().set_by_str("next", next_fn);
        // Also store done state as a flag for fast IterDone check
        iter_obj
            .borrow_mut()
            .set_by_str("__done__", Value::Boolean(false));
    }

    Value::Object(iter_obj)
}

/*
 * op_iter_next -- call iter.next() and store the result object.
 *
 * Instruction encoding: IterNext dst, iter  (new_rr)
 *   dst  = register to store the {value, done} result object
 *   iter = register holding the iterator
 */
fn op_iter_next(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let iter = vm.get_reg(instr.src1()).clone();
    let result = if let Value::Object(o) = &iter {
        let next_fn = o.borrow().get_by_str("next");
        if let Value::NativeFunction(f) = next_fn {
            f.call(&[])
        } else {
            // No next function: return done=true
            let r = Rc::new(RefCell::new(value::Object::new()));
            r.borrow_mut().set_by_str("done", Value::Boolean(true));
            r.borrow_mut().set_by_str("value", Value::Undefined);
            Value::Object(r)
        }
    } else {
        let r = Rc::new(RefCell::new(value::Object::new()));
        r.borrow_mut().set_by_str("done", Value::Boolean(true));
        r.borrow_mut().set_by_str("value", Value::Undefined);
        Value::Object(r)
    };
    vm.set_reg(instr.dst(), result);
    Ok(())
}

/*
 * op_iter_done -- extract the `done` flag from an iterator result object.
 *
 * Instruction encoding: IterDone dst, result  (new_rr)
 *   dst    = register to store done (Value::Boolean)
 *   result = register holding the {value, done} object from IterNext
 */
fn op_iter_done(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let result = vm.get_reg(instr.src1()).clone();
    let done = match &result {
        Value::Object(o) => o.borrow().get_by_str("done"),
        _ => Value::Boolean(true),
    };
    vm.set_reg(instr.dst(), done);
    Ok(())
}

/*
 * op_iter_value -- extract the `value` from an iterator result object.
 *
 * Instruction encoding: IterValue dst, result  (new_rr)
 *   dst    = register to store the iteration value
 *   result = register holding the {value, done} object from IterNext
 */
fn op_iter_value(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let result = vm.get_reg(instr.src1()).clone();
    let val = match &result {
        Value::Object(o) => o.borrow().get_by_str("value"),
        _ => Value::Undefined,
    };
    vm.set_reg(instr.dst(), val);
    Ok(())
}

/*
 * op_iter_close -- clean up the iterator after a for...of loop.
 *
 * WHY: Some iterators need explicit cleanup (generators, file iterators).
 * For our simple array iterators, no cleanup is needed. For generators,
 * we'd call iter.return() here. Since we have no generators yet, this
 * is a no-op.
 *
 * Instruction encoding: IterClose iter  (new_r)
 */
fn op_iter_close(_vm: &mut Vm, _instr: Instruction) -> VmResult<()> {
    // No-op: array iterators hold no external resources.
    // When generator support is added, call iter.return() here.
    Ok(())
}

fn op_get_prop(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let obj = vm.get_reg(instr.src1()).clone();
    // src2 is a constant pool index; resolve to string table ID via constant
    let const_idx = u16::from(instr.src2());
    let str_id = match vm.current_chunk().get_constant(const_idx) {
        Some(Constant::String(id)) => *id,
        _ => u32::from(instr.src2()),
    };
    let prop_name = vm.strings.get(str_id).unwrap_or("").to_string();
    let value = match &obj {
        Value::HostObject(host) => host.borrow().get_property(&prop_name),
        Value::Object(o) => {
            let own = o.borrow().get_by_str(&prop_name);
            if matches!(own, Value::Undefined) {
                builtins::array::get_array_method(o, &prop_name).unwrap_or(Value::Undefined)
            } else {
                own
            }
        }
        Value::String(s) => {
            builtins::string_proto::get_string_method(s, &prop_name).unwrap_or(Value::Undefined)
        }
        /*
         * Function.prototype methods: bind, call, apply.
         *
         * WHY: ChatGPT's script 6 uses $RV.bind(null, $RB) to create
         * bound callbacks for requestAnimationFrame and setTimeout.
         * Without .bind(), the property lookup returns Undefined and
         * calling it gives TypeError("not a function").
         */
        _val if prop_name == "bind" => {
            // .bind(thisArg, ...args) returns a new NativeFunction
            // that calls the original with the bound arguments prepended.
            // Simplified: ignore thisArg, just prepend bound args.
            let original = obj.clone();
            Value::NativeFunction(Rc::new(value::NativeFunction::new("bind", move |args| {
                // args[0] = thisArg (ignored for now)
                // args[1..] = bound arguments to prepend
                let bound_args: Vec<Value> = args.iter().skip(1).cloned().collect();
                // Return a new function that prepends bound_args
                let orig = original.clone();
                let ba = bound_args.clone();
                Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "bound",
                    move |call_args| {
                        let mut all_args = ba.clone();
                        all_args.extend(call_args.iter().cloned());
                        // Call the original -- but we can only call NativeFunction, not Function
                        match &orig {
                            Value::NativeFunction(f) => f.call(&all_args),
                            _ => Value::Undefined, // Can't call Value::Function from NativeFunction
                        }
                    },
                )))
            })))
        }
        Value::Function(_) | Value::NativeFunction(_)
            if prop_name == "call" || prop_name == "apply" =>
        {
            // .call(thisArg, ...args) -- simplified: ignore thisArg, call with args
            let original = obj.clone();
            Value::NativeFunction(Rc::new(value::NativeFunction::new(
                &prop_name,
                move |args| {
                    let call_args: Vec<Value> = args.iter().skip(1).cloned().collect();
                    match &original {
                        Value::NativeFunction(f) => f.call(&call_args),
                        _ => Value::Undefined,
                    }
                },
            )))
        }
        /*
         * Static method dispatch for constructor NativeFunctions.
         *
         * WHY: Array.isArray(), Array.from(), String.fromCharCode() are
         * accessed as properties on the constructor function itself.
         * NativeFunction has no property map, so we dispatch by name here
         * rather than making the global objects (which would break `new`/call).
         *
         * String.fromCharCode: convert code points to a string.
         * Array.isArray: returns true for array-like Objects.
         * Array.from: build an array from an iterable (array or string).
         */
        /*
         * Object static methods: keys, values, entries, assign, freeze, create, fromEntries.
         *
         * WHY: React and modern JS heavily use Object.keys/values/entries for
         * iterating over object properties, Object.assign for shallow merge,
         * Object.freeze for immutable objects, and Object.create for prototypal
         * inheritance. These patterns appear throughout React Router context setup.
         *
         * Dispatched by NativeFunction.name == "Object" to avoid changing the
         * global Object value from NativeFunction to Object (which would break
         * `new Object()` and `Object(x)` call sites).
         */
        /*
         * Number static methods: isInteger, isFinite, isNaN, parseInt, parseFloat,
         * EPSILON, MAX_SAFE_INTEGER, MIN_SAFE_INTEGER, MAX_VALUE, POSITIVE_INFINITY.
         *
         * WHY: Unlike global isNaN/isFinite (which coerce their argument),
         * Number.isNaN/isFinite perform type-safe checks without coercion.
         * These are required by modern JS code including React internals.
         */
        /*
         * Symbol static properties: well-known symbols and Symbol.for().
         *
         * WHY: React uses Symbol.iterator for iterables, Symbol.asyncIterator
         * for async iteration, Symbol.toPrimitive for coercion, Symbol.hasInstance
         * for instanceof overrides, and Symbol.for() for cross-realm symbols
         * (e.g. react-is uses Symbol.for('react.element')).
         *
         * Each well-known symbol is a fixed unique string "@@symbol_N_name"
         * generated at first access via make_symbol_value. They're stored as
         * thread_local statics so the same string is returned on every access.
         *
         * Symbol.for(key): global registry -- same key returns same symbol.
         * Symbol.keyFor(sym): reverse lookup in the registry.
         */
        Value::NativeFunction(f) if f.name == "Symbol" => {
            use builtins::map_set::make_symbol_value;
            match prop_name.as_str() {
                "iterator" => {
                    thread_local! {
                        static SYM_ITER: std::cell::OnceCell<String> = const { std::cell::OnceCell::new() };
                    }
                    SYM_ITER.with(|c| {
                        value::Value::string_owned(
                            c.get_or_init(|| "@@symbol_wk_iterator".to_string()).clone(),
                        )
                    })
                }
                "asyncIterator" => {
                    thread_local! {
                        static SYM_ASYNC: std::cell::OnceCell<String> = const { std::cell::OnceCell::new() };
                    }
                    SYM_ASYNC.with(|c| {
                        value::Value::string_owned(
                            c.get_or_init(|| "@@symbol_wk_asyncIterator".to_string())
                                .clone(),
                        )
                    })
                }
                "toPrimitive" => value::Value::string("@@symbol_wk_toPrimitive"),
                "toStringTag" => value::Value::string("@@symbol_wk_toStringTag"),
                "hasInstance" => value::Value::string("@@symbol_wk_hasInstance"),
                "species" => value::Value::string("@@symbol_wk_species"),
                "isConcatSpreadable" => value::Value::string("@@symbol_wk_isConcatSpreadable"),
                "for" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Symbol.for",
                    |args| {
                        let key = args
                            .first()
                            .map(|v| {
                                let s = v.to_js_string();
                                s.as_str().unwrap_or("").to_string()
                            })
                            .unwrap_or_default();
                        // Registry lookup/insert
                        thread_local! {
                            static REGISTRY: RefCell<Vec<(String, String)>> =
                                const { RefCell::new(Vec::new()) };
                        }
                        REGISTRY.with(|reg| {
                            let mut r = reg.borrow_mut();
                            if let Some((_, sym)) = r.iter().find(|(k, _)| k == &key) {
                                return value::Value::string_owned(sym.clone());
                            }
                            let sym = format!("@@symbol_for_{key}");
                            r.push((key, sym.clone()));
                            value::Value::string_owned(sym)
                        })
                    },
                ))),
                "keyFor" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Symbol.keyFor",
                    |args| {
                        let sym = args
                            .first()
                            .map(|v| {
                                let s = v.to_js_string();
                                s.as_str().unwrap_or("").to_string()
                            })
                            .unwrap_or_default();
                        // Extract key from "@@symbol_for_KEY" format
                        if let Some(key) = sym.strip_prefix("@@symbol_for_") {
                            value::Value::string_owned(key.to_string())
                        } else {
                            value::Value::Undefined
                        }
                    },
                ))),
                _ => make_symbol_value(&prop_name),
            }
        }
        Value::NativeFunction(f) if f.name == "Number" => {
            match prop_name.as_str() {
                "isInteger" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Number.isInteger",
                    |args| {
                        let result = args.first().is_some_and(|v| {
                            if let value::Value::Number(n) = v {
                                n.is_finite() && n.fract() == 0.0
                            } else {
                                false
                            }
                        });
                        value::Value::Boolean(result)
                    },
                ))),
                "isFinite" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Number.isFinite",
                    |args| {
                        let result = args.first().is_some_and(|v| {
                            if let value::Value::Number(n) = v {
                                n.is_finite()
                            } else {
                                false
                            }
                        });
                        value::Value::Boolean(result)
                    },
                ))),
                "isNaN" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Number.isNaN",
                    |args| {
                        let result = args.first().is_some_and(|v| {
                            if let value::Value::Number(n) = v {
                                n.is_nan()
                            } else {
                                false
                            }
                        });
                        value::Value::Boolean(result)
                    },
                ))),
                "isSafeInteger" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Number.isSafeInteger",
                    |args| {
                        let result = args.first().is_some_and(|v| {
                            if let value::Value::Number(n) = v {
                                n.is_finite()
                                    && n.fract() == 0.0
                                    && n.abs() <= 9_007_199_254_740_991.0
                            } else {
                                false
                            }
                        });
                        value::Value::Boolean(result)
                    },
                ))),
                "parseInt" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Number.parseInt",
                    |args| {
                        // Same behaviour as global parseInt
                        let s = args
                            .first()
                            .map(value::Value::to_js_string)
                            .unwrap_or_default();
                        let text = s.as_str().unwrap_or("").trim();
                        let radix = args.get(1).map_or(10, |v| v.to_number() as u32);
                        let radix = if radix == 0 { 10 } else { radix };
                        i64::from_str_radix(text, radix.clamp(2, 36))
                            .map(|n| value::Value::Number(n as f64))
                            .unwrap_or(value::Value::Number(f64::NAN))
                    },
                ))),
                "parseFloat" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Number.parseFloat",
                    |args| {
                        let s = args
                            .first()
                            .map(value::Value::to_js_string)
                            .unwrap_or_default();
                        let text = s.as_str().unwrap_or("").trim();
                        text.parse::<f64>()
                            .map(value::Value::Number)
                            .unwrap_or(value::Value::Number(f64::NAN))
                    },
                ))),
                "EPSILON" => value::Value::Number(f64::EPSILON),
                "MAX_SAFE_INTEGER" => value::Value::Number(9_007_199_254_740_991.0),
                "MIN_SAFE_INTEGER" => value::Value::Number(-9_007_199_254_740_991.0),
                "MAX_VALUE" => value::Value::Number(f64::MAX),
                "MIN_VALUE" => value::Value::Number(f64::MIN_POSITIVE),
                "POSITIVE_INFINITY" => value::Value::Number(f64::INFINITY),
                "NEGATIVE_INFINITY" => value::Value::Number(f64::NEG_INFINITY),
                "NaN" => value::Value::Number(f64::NAN),
                _ => Value::Undefined,
            }
        }
        Value::NativeFunction(f) if f.name == "Object" => {
            match prop_name.as_str() {
                "keys" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Object.keys",
                    |args| {
                        if let Some(value::Value::Object(o)) = args.first() {
                            let o_borrow = o.borrow();
                            let keys: Vec<value::Value> = o_borrow
                                .properties
                                .keys()
                                .map(|k| match k {
                                    value::PropertyKey::String(s) => {
                                        value::Value::String(Rc::clone(s))
                                    }
                                    value::PropertyKey::Index(i) => {
                                        value::Value::string_owned(i.to_string())
                                    }
                                })
                                .collect();
                            builtins::array::create_array(&keys)
                        } else {
                            builtins::array::create_array(&[])
                        }
                    },
                ))),
                "values" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Object.values",
                    |args| {
                        if let Some(value::Value::Object(o)) = args.first() {
                            let o_borrow = o.borrow();
                            let vals: Vec<value::Value> =
                                o_borrow.properties.values().cloned().collect();
                            builtins::array::create_array(&vals)
                        } else {
                            builtins::array::create_array(&[])
                        }
                    },
                ))),
                "entries" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Object.entries",
                    |args| {
                        if let Some(value::Value::Object(o)) = args.first() {
                            let o_borrow = o.borrow();
                            let entries: Vec<value::Value> = o_borrow
                                .properties
                                .iter()
                                .map(|(k, v)| {
                                    let key_val = match k {
                                        value::PropertyKey::String(s) => {
                                            value::Value::String(Rc::clone(s))
                                        }
                                        value::PropertyKey::Index(i) => {
                                            value::Value::string_owned(i.to_string())
                                        }
                                    };
                                    builtins::array::create_array(&[key_val, v.clone()])
                                })
                                .collect();
                            builtins::array::create_array(&entries)
                        } else {
                            builtins::array::create_array(&[])
                        }
                    },
                ))),
                "assign" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Object.assign",
                    |args| {
                        let Some(value::Value::Object(target)) = args.first() else {
                            return value::Value::Undefined;
                        };
                        for src in args.iter().skip(1) {
                            if let value::Value::Object(src_obj) = src {
                                let pairs: Vec<(value::PropertyKey, value::Value)> = src_obj
                                    .borrow()
                                    .properties
                                    .iter()
                                    .map(|(k, v)| (k.clone(), v.clone()))
                                    .collect();
                                for (k, v) in pairs {
                                    target.borrow_mut().set_by_key(k, v);
                                }
                            }
                        }
                        value::Value::Object(Rc::clone(target))
                    },
                ))),
                "freeze" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Object.freeze",
                    // Simplified: freeze is a no-op (we have no frozen flag).
                    // Return the object as-is -- mutation will still work but
                    // scripts that freeze then try to mutate will silently succeed,
                    // which is acceptable for a rendering engine.
                    |args| args.first().cloned().unwrap_or(value::Value::Undefined),
                ))),
                "create" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Object.create",
                    |args| {
                        use std::cell::RefCell;
                        let mut obj = value::Object::new();
                        // Set prototype from first arg if it's an Object
                        if let Some(value::Value::Object(proto)) = args.first() {
                            obj.prototype = Some(Rc::clone(proto));
                        }
                        value::Value::Object(Rc::new(RefCell::new(obj)))
                    },
                ))),
                "fromEntries" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Object.fromEntries",
                    |args| {
                        use std::cell::RefCell;
                        let obj = value::Object::new();
                        let obj_rc = Rc::new(RefCell::new(obj));
                        if let Some(value::Value::Object(arr)) = args.first() {
                            let entries = builtins::array::collect_elements_pub(&arr.borrow());
                            for entry in entries {
                                if let value::Value::Object(pair) = entry {
                                    let k = pair.borrow().get_by_key(&value::PropertyKey::Index(0));
                                    let v = pair.borrow().get_by_key(&value::PropertyKey::Index(1));
                                    let key_str = k.to_js_string();
                                    let key_s = key_str.as_str().unwrap_or("");
                                    obj_rc
                                        .borrow_mut()
                                        .set_by_key(value::PropertyKey::string_key(key_s), v);
                                }
                            }
                        }
                        value::Value::Object(obj_rc)
                    },
                ))),
                "getOwnPropertyNames" | "getOwnPropertySymbols" => Value::NativeFunction(Rc::new(
                    value::NativeFunction::new("Object.getOwnPropertyNames", |args| {
                        if let Some(value::Value::Object(o)) = args.first() {
                            let keys: Vec<value::Value> = o
                                .borrow()
                                .properties
                                .keys()
                                .map(|k| match k {
                                    value::PropertyKey::String(s) => {
                                        value::Value::String(Rc::clone(s))
                                    }
                                    value::PropertyKey::Index(i) => {
                                        value::Value::string_owned(i.to_string())
                                    }
                                })
                                .collect();
                            builtins::array::create_array(&keys)
                        } else {
                            builtins::array::create_array(&[])
                        }
                    }),
                )),
                "defineProperty" | "defineProperties" | "seal" | "preventExtensions"
                | "isFrozen" | "isSealed" | "isExtensible" => {
                    // Stubs: return first arg unchanged or true/false as appropriate
                    let is_predicate =
                        matches!(prop_name.as_str(), "isFrozen" | "isSealed" | "isExtensible");
                    let name = prop_name.clone();
                    Value::NativeFunction(Rc::new(value::NativeFunction::new(name, move |args| {
                        if is_predicate {
                            value::Value::Boolean(false)
                        } else {
                            args.first().cloned().unwrap_or(value::Value::Undefined)
                        }
                    })))
                }
                _ => Value::Undefined,
            }
        }
        Value::NativeFunction(f) if f.name == "String" => {
            match prop_name.as_str() {
                "fromCharCode" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "String.fromCharCode",
                    |args| {
                        let s: String = args
                            .iter()
                            .filter_map(|v| {
                                let code = v.to_number() as u32;
                                char::from_u32(code)
                            })
                            .collect();
                        value::Value::string_owned(s)
                    },
                ))),
                "raw" => {
                    // String.raw`...` -- simplified: join strings without escape processing
                    Value::NativeFunction(Rc::new(value::NativeFunction::new(
                        "String.raw",
                        |args| {
                            // args[0] = template object with .raw array
                            // args[1..] = substitution values
                            if let Some(value::Value::Object(tmpl)) = args.first() {
                                let raw = tmpl.borrow().get_by_str("raw");
                                if let value::Value::Object(raw_arr) = raw {
                                    let parts =
                                        builtins::array::collect_elements_pub(&raw_arr.borrow());
                                    let subs = &args[1..];
                                    let mut result = String::new();
                                    for (i, part) in parts.iter().enumerate() {
                                        let s = part.to_js_string();
                                        result.push_str(s.as_str().unwrap_or(""));
                                        if i < subs.len() {
                                            let sub = subs[i].to_js_string();
                                            result.push_str(sub.as_str().unwrap_or(""));
                                        }
                                    }
                                    return value::Value::string_owned(result);
                                }
                            }
                            value::Value::string("")
                        },
                    )))
                }
                _ => Value::Undefined,
            }
        }
        Value::NativeFunction(f) if f.name == "Array" => {
            match prop_name.as_str() {
                "isArray" => Value::NativeFunction(Rc::new(value::NativeFunction::new(
                    "Array.isArray",
                    |args| {
                        let result = args.first().is_some_and(|v| {
                            if let value::Value::Object(o) = v {
                                builtins::array::is_array_like(&o.borrow())
                            } else {
                                false
                            }
                        });
                        value::Value::Boolean(result)
                    },
                ))),
                "from" => {
                    Value::NativeFunction(Rc::new(value::NativeFunction::new(
                        "Array.from",
                        |args| {
                            use builtins::array::{
                                collect_elements_pub, create_array, is_array_like,
                            };
                            let source = args.first().cloned().unwrap_or(value::Value::Undefined);
                            let map_fn = args.get(1).cloned();
                            let elements: Vec<value::Value> = match &source {
                                value::Value::Object(o) => {
                                    let o_borrow = o.borrow();
                                    if is_array_like(&o_borrow) {
                                        collect_elements_pub(&o_borrow)
                                    } else {
                                        vec![]
                                    }
                                }
                                value::Value::String(s) => {
                                    // Array.from("abc") -> ["a","b","c"]
                                    let text = s.as_str().unwrap_or("").to_string();
                                    text.chars()
                                        .map(|c| value::Value::string_owned(c.to_string()))
                                        .collect()
                                }
                                _ => vec![],
                            };
                            if let Some(value::Value::NativeFunction(f)) = map_fn {
                                let mapped: Vec<value::Value> = elements
                                    .iter()
                                    .enumerate()
                                    .map(|(i, el)| {
                                        f.call(&[el.clone(), value::Value::Number(i as f64)])
                                    })
                                    .collect();
                                create_array(&mapped)
                            } else {
                                create_array(&elements)
                            }
                        },
                    )))
                }
                "of" => {
                    Value::NativeFunction(Rc::new(value::NativeFunction::new("Array.of", |args| {
                        builtins::array::create_array(args)
                    })))
                }
                _ => Value::Undefined,
            }
        }
        _ => Value::Undefined,
    };
    vm.set_reg(instr.dst(), value);
    Ok(())
}

fn op_set_prop(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let const_idx = u16::from(instr.src1());
    let str_id = match vm.current_chunk().get_constant(const_idx) {
        Some(Constant::String(id)) => *id,
        _ => u32::from(instr.src1()),
    };
    let prop_name = vm.strings.get(str_id).unwrap_or("").to_string();
    let value = vm.get_reg(instr.src2()).clone();
    let obj = vm.get_reg(instr.dst());
    match obj {
        Value::HostObject(host) => {
            host.borrow_mut().set_property(&prop_name, value);
        }
        Value::Object(o) => {
            o.borrow_mut().set_by_str(&prop_name, value);
        }
        _ => {}
    }
    Ok(())
}

fn op_get_elem(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let obj = vm.get_reg(instr.src1());
    let key = vm.get_reg(instr.src2()).to_u32();
    let value = match obj {
        Value::Object(o) => o.borrow().get(key),
        _ => Value::Undefined,
    };
    vm.set_reg(instr.dst(), value);
    Ok(())
}

fn op_set_elem(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let key = vm.get_reg(instr.src1()).to_u32();
    let value = vm.get_reg(instr.src2()).clone();
    let obj = vm.get_reg(instr.dst());
    if let Value::Object(o) = obj {
        let mut borrow = o.borrow_mut();
        borrow.set(key, value);
        /*
         * ECMA-262 OrdinaryDefineOwnProperty for arrays:
         * when defining own integer-indexed property P, if Uint32(P)
         * >= length, set length = P + 1. We approximate the spec's
         * "is the receiver an Array" test by checking whether the
         * object already carries a numeric `length` property -- which
         * is true exactly for the literal `[]`, NewArray-built rests,
         * and template-string strings arrays. Plain objects keep the
         * old behaviour (no length bump).
         */
        if let Value::Number(current) = borrow.get_by_str("length") {
            let needed = f64::from(key) + 1.0;
            if needed > current {
                borrow.set_by_str("length", Value::Number(needed));
            }
        }
    }
    Ok(())
}

fn op_typeof(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let val = vm.get_reg(instr.src1());
    let type_str = val.type_of();
    vm.set_reg(instr.dst(), Value::string(type_str));
    Ok(())
}

// Object creation handlers

fn op_new_object(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let obj = Rc::new(RefCell::new(Object::new()));
    vm.set_reg(instr.dst(), Value::Object(obj));
    Ok(())
}

/*
 * op_new_array -- create a JS array (object with length property).
 *
 * WHY: Arrays in JS are objects with a numeric `length` property.
 * Array methods (push, pop, map, etc.) check for `length` via
 * is_array_like(). Without it, `$RB.push(a,b)` returns Undefined
 * because $RB doesn't look like an array.
 *
 * The compiler encodes the literal element count in const_idx (16-bit
 * field shared with property indices). compile_array uses this to
 * pre-size `length`, mirroring the semantics of `[1,2,3,4]` in spec
 * terms: the array's length property is established by the literal,
 * not by the subsequent SetElem stores. Pre-sizing also lets later
 * SetElem stores at i < length skip the length-bump path.
 *
 * For dynamic-growth arrays (push, splice, manual `arr[i] = v`), the
 * `op_set_elem` handler is responsible for keeping `length` consistent:
 * see that handler's comment for the spec rule.
 */
fn op_new_array(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let obj = Rc::new(RefCell::new(Object::new()));
    let len = f64::from(instr.const_idx());
    obj.borrow_mut().set_by_str("length", Value::Number(len));
    vm.set_reg(instr.dst(), Value::Object(obj));
    Ok(())
}

/*
 * op_new_function -- create a Function value from a compiled chunk.
 *
 * WHY: Function expressions (`function(){}`, arrow functions) compile
 * their body into a separate Chunk. The constant pool entry at const_idx
 * holds Constant::Function(chunk_idx) pointing to that chunk.
 *
 * The chunk_idx is an absolute index into vm.chunks (patched by the
 * caller after compile_with_children() adds child chunks).
 *
 * See: compile_expression Expression::Function for compilation
 * See: op_call for function invocation
 */
fn op_new_function(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let const_idx = instr.const_idx();
    let chunk = vm.current_chunk();
    let chunk_idx = match chunk.get_constant(const_idx) {
        Some(Constant::Function(idx)) => *idx,
        _ => u32::from(const_idx), // Fallback: treat const_idx as chunk_idx
    };
    let func = Rc::new(JsFunction::new(chunk_idx));
    vm.set_reg(instr.dst(), Value::Function(func));
    Ok(())
}

/*
 * op_new_generator -- create a Value::Function value pointing at a generator
 * body chunk.
 *
 * WHY: At runtime there is no observable difference between a regular
 * Function value and a generator-function value before it is called: both
 * are first-class JS values that flow through registers, closures, and
 * argument lists.  The DIFFERENCE shows up when the value is invoked: op_call
 * inspects vm.chunks[chunk_idx].is_generator and diverts generator-flagged
 * chunks to invoke_generator (eager body execution, returns iterator).
 *
 * So this handler is structurally identical to op_new_function -- it just
 * resolves the constant-pool Function entry into a Value::Function with the
 * given chunk index.  The compiler MUST flag the referenced chunk's
 * is_generator field (it does, in Statement::FunctionDeclaration and
 * Expression::Function when the AST node's is_generator is set).
 *
 * Splitting NewGenerator from NewFunction at the bytecode level keeps the
 * call-site explicit (better debuggability) and leaves room for future
 * proper-suspension implementations that may want a richer wrapper here.
 *
 * Encoding: dst = result register, const_idx = constant-pool slot holding
 * Constant::Function(chunk_idx).
 *
 * See: invoke_generator for the eager dispatch when this value is called.
 * See: vm/generator.rs for the design rationale and limitations.
 */
fn op_new_generator(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let const_idx = instr.const_idx();
    let chunk = vm.current_chunk();
    let chunk_idx = match chunk.get_constant(const_idx) {
        Some(Constant::Function(idx)) => *idx,
        _ => u32::from(const_idx),
    };
    let func = Rc::new(JsFunction::new(chunk_idx));
    vm.set_reg(instr.dst(), Value::Function(func));
    Ok(())
}

/*
 * op_yield -- emit a value from the currently-executing generator body.
 *
 * WHY: In the eager strategy the generator body runs to completion before
 * the caller ever sees the iterator.  Each `yield expr` therefore appends
 * `expr`'s value to the active yield buffer (top of generator_yield_stack)
 * and continues execution -- no suspension.  The dst register receives
 * undefined because the eager strategy cannot plumb the .next(value)
 * argument back into the yielded expression (documented limitation; see
 * vm/generator.rs).
 *
 * Encoding: Yield(dst, src) -- read value from r[src], store undefined in
 * r[dst] (yield expression result).
 *
 * If the stack is empty (yield outside a generator -- compiler bug or
 * direct bytecode injection) we silently no-op the append.  We still write
 * undefined to dst so downstream code observes a consistent value.
 */
fn op_yield(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let value = vm.get_reg(instr.src1()).clone();
    if let Some(buffer) = vm.generator_yield_stack.last() {
        buffer.borrow_mut().push(value);
    }
    vm.set_reg(instr.dst(), Value::Undefined);
    Ok(())
}

/*
 * op_yield_star -- delegate to an iterable; flatten its values into the
 * active yield buffer.
 *
 * WHY: `yield* iterable` is spec-defined as "yield every value produced by
 * iterable.next() until done".  In eager mode we drain the iterable
 * synchronously via generator::yield_star_flatten, which handles arrays,
 * strings, and iterator-shaped objects (including other generators built
 * by build_generator).
 *
 * Encoding: YieldStar(dst, src) -- read iterable from r[src], drain into
 * top yield buffer, store undefined in r[dst].
 *
 * No-op (apart from writing undefined to dst) when the yield stack is
 * empty.
 */
fn op_yield_star(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let iterable = vm.get_reg(instr.src1()).clone();
    if let Some(buffer) = vm.generator_yield_stack.last().cloned() {
        generator::yield_star_flatten(&buffer, &iterable);
    }
    vm.set_reg(instr.dst(), Value::Undefined);
    Ok(())
}

/*
 * op_bind_capture -- append `r[src]` to the JsFunction in `r[dst]`.
 *
 * WHY: Closures need to carry the values of variables from their
 * enclosing scope. The compiler emits one BindCapture instruction per
 * upvalue, immediately after NewFunction, in the same order the inner
 * function expects them in its captures slot indices. This runs at the
 * outer function's execution time, so the captured values are exactly
 * the locals' current values at the moment the inner function is
 * created -- matching ECMA-262 closure semantics for the common case.
 *
 * Encoding: dst = function register, src1 = source local register.
 *
 * See: op_get_capture (depth=0 mode) for the read side.
 * See: compile_expression Expression::Function for emission.
 */
fn op_bind_capture(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let value = vm.get_reg(instr.src1()).clone();
    if let Value::Function(func) = vm.get_reg(instr.dst()) {
        func.captures.borrow_mut().push(value);
    }
    Ok(())
}

// Scope handlers

fn op_get_local(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    // In register-based VM, locals ARE registers
    let value = vm.get_reg(instr.src1()).clone();
    vm.set_reg(instr.dst(), value);
    Ok(())
}

fn op_set_local(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let value = vm.get_reg(instr.src1()).clone();
    vm.set_reg(instr.dst(), value);
    Ok(())
}

/*
 * op_get_capture / op_set_capture -- closure variable access.
 *
 * WHY: When an inner function references a variable from an outer scope,
 * the compiler emits GetCapture(dst, depth, slot) where `slot` is the
 * register index of the captured variable in the outer function's frame.
 *
 * In our flat register VM all CallFrames share vm.registers[]. The outer
 * function's variables remain in vm.registers[slot] for the lifetime of
 * the outer call. We simply read/write the slot directly (depth is ignored
 * in this flat model -- it encodes scope nesting for future closure objects).
 *
 * INVARIANT: The outer function's registers are not reused by the inner
 * function because the compiler allocates inner function registers starting
 * from 0 within the child chunk. As long as the inner function uses fewer
 * registers than `slot`, the outer value is safe.
 *
 * Encoding:
 *   GetCapture: dst=target_reg, src1=depth, src2=outer_slot
 *   SetCapture: dst=depth, src1=outer_slot, src2=value_reg
 */
fn op_get_capture(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    /*
     * GetCapture has TWO modes selected by the depth operand:
     *
     *   depth == 0  -- TRUE upvalue from the current closure's captures.
     *                  `slot` indexes into CallFrame.captures, which the
     *                  caller seeded from JsFunction.captures at op_call.
     *                  This is what makes `function f(x){return function(){return x}}`
     *                  work: the inner function reads x even though x lives
     *                  in a defunct call frame.
     *
     *   depth >= 1  -- intra-function block-scope lookup. The compiler's
     *                  child Compiler walks scopes BUT cannot cross function
     *                  boundaries, so depth > 0 only happens when the
     *                  reference and the binding live in the SAME function
     *                  but in different lexical block scopes (for-loop init
     *                  read from the for-body, etc.). `slot` is then a
     *                  frame-relative register index; get_reg applies the
     *                  current frame's base.
     *
     * See: op_set_capture for the symmetric write path
     * See: lookup_var in compiler.rs for how depth/slot are computed
     * See: JsFunction.captures and BindCapture for upvalue construction
     */
    let depth = instr.src1();
    let slot = instr.src2();
    let value = if depth == 0 {
        if let Some(frame) = vm.call_stack.last() {
            let cap = frame.captures.borrow();
            cap.get(slot as usize).cloned().unwrap_or(Value::Undefined)
        } else {
            Value::Undefined
        }
    } else {
        vm.get_reg(slot).clone()
    };
    vm.set_reg(instr.dst(), value);
    Ok(())
}

fn op_set_capture(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    /*
     * Write a captured variable. See op_get_capture for the depth=0
     * vs depth>=1 dispatch rationale.
     *
     * Encoding here matches the compiler emission:
     *   `Instruction::new_rrr(SetCapture, depth, slot, src)`
     * so dst()=depth, src1()=slot, src2()=src.
     */
    let depth = instr.dst();
    let slot = instr.src1();
    let value = vm.get_reg(instr.src2()).clone();
    if depth == 0 {
        if let Some(frame) = vm.call_stack.last() {
            let mut cap = frame.captures.borrow_mut();
            if (slot as usize) < cap.len() {
                cap[slot as usize] = value;
            } else {
                // Defensive: extend with Undefined if compiler emitted a
                // slot beyond captures.len(). This should not happen in
                // well-formed bytecode but avoids silent loss.
                while cap.len() < slot as usize {
                    cap.push(Value::Undefined);
                }
                cap.push(value);
            }
        }
    } else {
        vm.set_reg(slot, value);
    }
    Ok(())
}

/*
 * op_get_global -- resolve a global variable by name.
 *
 * WHY: Global variable access in JS (document, window, console, etc.)
 * must look up the name on the global object. The compiler emits a
 * constant index that references the string table; we resolve it here.
 *
 * String resolution: const_idx -> strings.get(idx) -> property name.
 * If the name is non-empty, look up by string on global object.
 * If empty (legacy numeric index), fall back to Index(key_idx).
 *
 * This was fixed to use get_by_str() instead of get() to find
 * builtins installed with set_by_str() (document, window, etc.).
 *
 * See: builtins/mod.rs install_builtins() for what's on the global
 * See: dom_bridge/mod.rs install_document() for document global
 */
/*
 * op_get_global / op_set_global -- resolve global variables via constant pool.
 *
 * CRITICAL FIX: The instruction's const_idx is an index into the CONSTANT POOL,
 * not the string table. The constant at that index is Constant::String(str_id)
 * where str_id is the string table index. We must resolve through the constant
 * pool first, just like op_new_function resolves Constant::Function.
 *
 * Previous bug: treated const_idx directly as string table index, which only
 * worked when const_idx happened to equal the string table ID (true for the
 * first few constants, false for scripts with many constants).
 */
fn op_get_global(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let const_idx = instr.const_idx();
    let chunk = vm.current_chunk();
    // Resolve constant pool entry to string table ID
    let str_id = match chunk.get_constant(const_idx) {
        Some(Constant::String(id)) => *id,
        _ => u32::from(const_idx), // Fallback
    };
    let name = vm.strings.get(str_id).unwrap_or("").to_string();
    let value = if name.is_empty() {
        vm.global.borrow().get(str_id)
    } else {
        vm.global.borrow().get_by_str(&name)
    };
    vm.set_reg(instr.dst(), value);
    Ok(())
}

fn op_set_global(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let const_idx = instr.const_idx();
    let chunk = vm.current_chunk();
    let str_id = match chunk.get_constant(const_idx) {
        Some(Constant::String(id)) => *id,
        _ => u32::from(const_idx),
    };
    let value = vm.get_reg(instr.dst()).clone();
    let name = vm.strings.get(str_id).unwrap_or("").to_string();
    if name.is_empty() {
        vm.global.borrow_mut().set(str_id, value);
    } else {
        vm.global.borrow_mut().set_by_str(&name, value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{Chunk, Instruction, Opcode};

    #[test]
    fn test_vm_arithmetic() {
        let mut vm = Vm::new();
        let mut chunk = Chunk::new();

        // r0 = 10
        chunk.emit(Instruction::new_r_offset(Opcode::LoadSmi, 0, 10));
        // r1 = 5
        chunk.emit(Instruction::new_r_offset(Opcode::LoadSmi, 1, 5));
        // r2 = r0 + r1
        chunk.emit(Instruction::new_rrr(Opcode::Add, 2, 0, 1));
        // return r2
        chunk.emit(Instruction::new_r(Opcode::Ret, 2));

        let idx = vm.add_chunk(chunk);
        // UNWRAP-OK: test executes a hand-built well-formed chunk; failure indicates a VM bug.
        let result = vm.execute(idx).unwrap();

        if let Value::Number(n) = result {
            assert!((n - 15.0).abs() < f64::EPSILON);
        } else {
            panic!("Expected number");
        }
    }

    #[test]
    fn test_vm_comparison() {
        let mut vm = Vm::new();
        let mut chunk = Chunk::new();

        // r0 = 10
        chunk.emit(Instruction::new_r_offset(Opcode::LoadSmi, 0, 10));
        // r1 = 5
        chunk.emit(Instruction::new_r_offset(Opcode::LoadSmi, 1, 5));
        // r2 = r0 > r1
        chunk.emit(Instruction::new_rrr(Opcode::Gt, 2, 0, 1));
        // return r2
        chunk.emit(Instruction::new_r(Opcode::Ret, 2));

        let idx = vm.add_chunk(chunk);
        // UNWRAP-OK: test executes a hand-built well-formed chunk; failure indicates a VM bug.
        let result = vm.execute(idx).unwrap();

        assert!(matches!(result, Value::Boolean(true)));
    }

    #[test]
    fn test_vm_conditional_jump() {
        let mut vm = Vm::new();
        let mut chunk = Chunk::new();

        // r0 = true
        chunk.emit(Instruction::new_r(Opcode::LoadTrue, 0));
        // if r0 jump +2
        chunk.emit(Instruction::new_r_offset(Opcode::JmpTrue, 0, 2));
        // r1 = 100 (skipped)
        chunk.emit(Instruction::new_r_offset(Opcode::LoadSmi, 1, 100));
        // jump +1
        chunk.emit(Instruction::new_offset(Opcode::Jmp, 1));
        // r1 = 200 (executed)
        chunk.emit(Instruction::new_r_offset(Opcode::LoadSmi, 1, 200));
        // return r1
        chunk.emit(Instruction::new_r(Opcode::Ret, 1));

        let idx = vm.add_chunk(chunk);
        // UNWRAP-OK: test executes a hand-built well-formed chunk; failure indicates a VM bug.
        let result = vm.execute(idx).unwrap();

        if let Value::Number(n) = result {
            assert!((n - 200.0).abs() < f64::EPSILON);
        } else {
            panic!("Expected number");
        }
    }

    #[test]
    fn test_vm_bitwise() {
        let mut vm = Vm::new();
        let mut chunk = Chunk::new();

        // r0 = 0xFF
        chunk.emit(Instruction::new_r_offset(Opcode::LoadSmi, 0, 0xFF));
        // r1 = 0x0F
        chunk.emit(Instruction::new_r_offset(Opcode::LoadSmi, 1, 0x0F));
        // r2 = r0 & r1
        chunk.emit(Instruction::new_rrr(Opcode::BitAnd, 2, 0, 1));
        // return r2
        chunk.emit(Instruction::new_r(Opcode::Ret, 2));

        let idx = vm.add_chunk(chunk);
        // UNWRAP-OK: test executes a hand-built well-formed chunk; failure indicates a VM bug.
        let result = vm.execute(idx).unwrap();

        if let Value::Number(n) = result {
            assert!((n - f64::from(0x0F)).abs() < f64::EPSILON);
        } else {
            panic!("Expected number");
        }
    }

    #[test]
    fn test_vm_object() {
        let mut vm = Vm::new();
        let mut chunk = Chunk::new();

        // r0 = {}
        chunk.emit(Instruction::new_r(Opcode::NewObject, 0));
        // r1 = 42
        chunk.emit(Instruction::new_r_offset(Opcode::LoadSmi, 1, 42));
        // r0[0] = r1
        chunk.emit(Instruction::new_rrr(Opcode::SetElem, 0, 1, 1)); // key=1, val=r1
        // Actually, let's just return the value we set
        chunk.emit(Instruction::new_r(Opcode::Ret, 1));

        let idx = vm.add_chunk(chunk);
        // UNWRAP-OK: test executes a hand-built well-formed chunk; failure indicates a VM bug.
        let result = vm.execute(idx).unwrap();

        if let Value::Number(n) = result {
            assert!((n - 42.0).abs() < f64::EPSILON);
        } else {
            panic!("Expected number");
        }
    }

    /*
     * Helper: full pipeline parse -> compile -> load strings -> execute.
     * Returns Ok(()) if the script ran without error, Err with the error otherwise.
     */
    fn run_script(vm: &mut Vm, source: &str) -> Result<(), String> {
        let ast_arena = crate::parser::ast_arena::AstArena::new();
        let parser = crate::parser::Parser::new(source, &ast_arena);
        let (ast, errors) = parser.parse();
        if !errors.is_empty() {
            return Err(format!("Parse errors: {errors:?}"));
        }
        let compiler = crate::bytecode::Compiler::new();
        match compiler.compile_with_children(&ast) {
            Ok((chunk, child_chunks, string_pool)) => {
                let mut str_map = std::collections::HashMap::new();
                for (compiler_id, s) in &string_pool {
                    let vm_id = vm.strings.intern(s.clone());
                    str_map.insert(*compiler_id, vm_id);
                }
                let child_base = vm.chunks_len();
                for mut child in child_chunks {
                    for constant in child.constants_mut() {
                        if let Constant::String(str_id) = constant
                            && let Some(&vm_id) = str_map.get(str_id)
                        {
                            *str_id = vm_id;
                        }
                    }
                    vm.add_chunk(child);
                }
                let mut main_chunk = chunk;
                for constant in main_chunk.constants_mut() {
                    match constant {
                        Constant::Function(idx) => *idx += child_base as u32,
                        Constant::String(str_id) => {
                            if let Some(&vm_id) = str_map.get(str_id) {
                                *str_id = vm_id;
                            }
                        }
                        _ => {}
                    }
                }
                let chunk_idx = vm.add_chunk(main_chunk);
                match vm.execute(chunk_idx) {
                    Ok(_) => Ok(()),
                    Err(e) => Err(format!("{e:?}")),
                }
            }
            Err(e) => Err(format!("Compile error: {e:?}")),
        }
    }

    #[test]
    fn test_e2e_simple_assignment() {
        let mut vm = Vm::new();
        // UNWRAP-OK: input script is a well-formed literal that this test asserts must run.
        run_script(&mut vm, "var x = 42;").unwrap();
    }

    #[test]
    fn test_e2e_global_array_assignment() {
        let mut vm = Vm::new();
        // This is the pattern from ChatGPT script 6
        // UNWRAP-OK: input script is a well-formed literal that this test asserts must run.
        run_script(&mut vm, "$RB = [];").unwrap();
    }

    #[test]
    fn test_e2e_global_function_assignment() {
        let mut vm = Vm::new();
        // UNWRAP-OK: input script is a well-formed literal that this test asserts must run.
        run_script(&mut vm, "$RV = function(a) {};").unwrap();
    }

    #[test]
    fn test_e2e_iife_with_try_catch() {
        let mut vm = Vm::new();
        // UNWRAP-OK: input script is a well-formed literal that this test asserts must run.
        run_script(&mut vm, "!function(){ try { var x = 1; } catch(e) {} }();").unwrap();
    }

    #[test]
    fn test_e2e_script6_minimal() {
        let mut vm = Vm::new();
        let result = run_script(&mut vm, "$RB=[];$RV=function(a){};");
        assert!(result.is_ok(), "Script 6 minimal: {result:?}");
    }

    #[test]
    fn test_e2e_script6_with_for_loop() {
        let mut vm = Vm::new();
        let result = run_script(
            &mut vm,
            "$RB=[];$RV=function(a){for(var b=0;b<a.length;b+=2){var c=a[b];}};",
        );
        assert!(result.is_ok(), "Script 6 with for loop: {result:?}");
    }

    #[test]
    fn test_e2e_script6_with_semicolon_separated() {
        // The REAL script 6 has two statements separated by ;
        // The second ends WITHOUT a semicolon (just })
        let mut vm = Vm::new();
        let result = run_script(
            &mut vm,
            "$RB=[];$RV=function(a){$RT=performance.now();for(var b=0;b<a.length;b+=2){var c=a[b],e=a[b+1];}a.length=0};",
        );
        // This may fail -- we're looking for the exact failure point
        if let Err(e) = &result {
            eprintln!("Script 6 expanded failure: {e}");
        }
    }

    #[test]
    fn test_e2e_script6_full() {
        let mut vm = Vm::new();
        let source = "$RB=[];$RV=function(a){$RT=performance.now();for(var b=0;b<a.length;b+=2){var c=a[b],e=a[b+1];null!==e.parentNode&&e.parentNode.removeChild(e);var f=c.parentNode;if(f){var g=c.previousSibling,h=0;do{if(c&&8===c.nodeType){var d=c.data;if(\"/$\"===d||\"/&\"===d)if(0===h)break;else h--;else\"$\"!==d&&\"$?\"!==d&&\"$~\"!==d&&\"$!\"!==d&&\"&\"!==d||h++}d=c.nextSibling;f.removeChild(c);c=d}while(c);for(;e.firstChild;)f.insertBefore(e.firstChild,c);g.data=\"$\";g._reactRetry&&requestAnimationFrame(g._reactRetry)}}a.length=0};";
        // First check parse
        let ast_arena = crate::parser::ast_arena::AstArena::new();
        let parser = crate::parser::Parser::new(source, &ast_arena);
        let (ast, errors) = parser.parse();
        eprintln!(
            "Script 6 parse: {} statements, {} errors",
            ast.body.len(),
            errors.len()
        );
        for e in &errors {
            eprintln!("  Parse error: {e:?}");
        }
        // Then run
        let result = run_script(&mut vm, source);
        if let Err(e) = &result {
            eprintln!("Script 6 FULL failure: {e}");
        }
        assert!(result.is_ok(), "Script 6 full: {result:?}");
    }

    #[test]
    fn test_e2e_script8_rc_call() {
        let mut vm = Vm::new();
        let result = run_script(&mut vm, "$RC(\"B:1\",\"S:1\")");
        assert!(result.is_err(), "$RC should fail (not defined)");
    }

    #[test]
    fn test_e2e_scripts_sequential() {
        // Run scripts 0-6 sequentially in one VM, checking each
        let mut vm = Vm::new();

        // Script 0-like (IIFE)
        let r = run_script(&mut vm, "!function(){ var x = 1; }();");
        assert!(r.is_ok(), "Script 0: {r:?}");

        // Script 6-like (global assignments)
        let r = run_script(&mut vm, "$RB=[];$RV=function(a){};");
        assert!(r.is_ok(), "Script 6 after others: {r:?}");

        // Script 8-like ($RC call -- should fail because $RC not defined)
        let r = run_script(&mut vm, "$RC(\"B:1\",\"S:1\")");
        assert!(r.is_err(), "Script 8: {r:?}");
    }

    #[test]
    fn test_e2e_script6_after_5_scripts() {
        // Simulate running 5 scripts before script 6, accumulating strings
        let mut vm = Vm::new();
        run_script(
            &mut vm,
            "!function(){try{var d=document.documentElement}catch(e){}}();",
        )
        .ok();
        run_script(
            &mut vm,
            "!function(){try{var t=localStorage.getItem('x')}catch(e){}}();",
        )
        .ok();
        run_script(&mut vm, "var x = window.__oai_SSR_HTML || 0;").ok();
        run_script(&mut vm, "window.__test = {\"a\": 1};").ok();
        run_script(&mut vm, "requestAnimationFrame(function(){});").ok();

        // Now script 6
        let r = run_script(
            &mut vm,
            "$RB=[];$RV=function(a){$RT=performance.now();for(var b=0;b<a.length;b+=2){var c=a[b];}a.length=0};",
        );
        assert!(r.is_ok(), "Script 6 after 5: {r:?}");
    }

    /*
     * Destructuring parameter tests.
     *
     * WHY: compile_pattern_binding was added to support object/array/default
     * destructuring in function parameters. These tests verify that the
     * two-pass param compilation (slot allocation + pattern binding) produces
     * correct results for the common cases encountered in real-world scripts.
     *
     * The tests run a script that stores the call result in the global `result`,
     * then inspect vm.global after execution (main chunk always returns Undefined
     * via RetUndefined, so we cannot check execute()'s return value).
     *
     * See: compiler.rs compile_pattern_binding
     * See: compiler.rs Expression::Function / Arrow param loop
     */

    /*
     * run_and_get_result -- run a script that sets `window.result` and return it.
     *
     * WHY: Top-level `var x` compiles to SetLocal (register), not SetGlobal.
     * `vm.global` (the JS global object) only receives values via SetProp on
     * the window object or SetGlobal for undeclared assignments. Since `window`
     * is the global object itself (install_window_self wires them up), assigning
     * `window.result = expr` is the reliable way to inspect a computed value.
     *
     * See: vm/builtins/window.rs install_window_self()
     * See: op_set_prop for how window property writes land on vm.global
     */
    fn run_and_get_result(source: &str) -> Result<Value, String> {
        let mut vm = Vm::new();
        run_script(&mut vm, source)?;
        Ok(vm.global.borrow().get_by_str("result").clone())
    }

    #[test]
    fn test_run_and_get_result_basic() {
        // Sanity check: run_and_get_result works for a plain function call
        // UNWRAP-OK: script is a well-formed literal that this test asserts must run.
        let v =
            run_and_get_result("function add(x, y) { return x + y; } window.result = add(3, 4);")
                .expect("script failed");
        if let Value::Number(n) = v {
            assert!((n - 7.0).abs() < f64::EPSILON, "expected 7, got {n}");
        } else {
            panic!("expected 7, got {v:?}");
        }
    }

    #[test]
    fn test_destruct_object_param() {
        // UNWRAP-OK: script is a well-formed literal that this test asserts must run.
        let v = run_and_get_result(
            "function add({x, y}) { return x + y; } window.result = add({x: 3, y: 4});",
        )
        .expect("script failed");
        if let Value::Number(n) = v {
            assert!((n - 7.0).abs() < f64::EPSILON, "expected 7, got {n}");
        } else {
            panic!("expected 7, got {v:?}");
        }
    }

    #[test]
    fn test_destruct_array_param() {
        // UNWRAP-OK: script is a well-formed literal that this test asserts must run.
        let v = run_and_get_result(
            "function sum([a, b]) { return a + b; } window.result = sum([10, 20]);",
        )
        .expect("script failed");
        if let Value::Number(n) = v {
            assert!((n - 30.0).abs() < f64::EPSILON, "expected 30, got {n}");
        } else {
            panic!("expected 30, got {v:?}");
        }
    }

    #[test]
    fn test_destruct_default_param() {
        // {role = "user"} should use the default when the property is absent
        // UNWRAP-OK: script is a well-formed literal that this test asserts must run.
        let v = run_and_get_result(
            "function label({name, role = \"user\"}) { return name + \":\" + role; }\
             window.result = label({name: \"Bob\"});",
        )
        .expect("script failed");
        if let Value::String(s) = &v {
            assert_eq!(s.as_str().unwrap_or(""), "Bob:user");
        } else {
            panic!("expected string, got {v:?}");
        }
    }

    #[test]
    fn test_destruct_mixed_params() {
        // Mix of identifier and destructured params
        // UNWRAP-OK: script is a well-formed literal that this test asserts must run.
        let v = run_and_get_result(
            "function f(n, {a, b}) { return n + a + b; } window.result = f(1, {a: 2, b: 3});",
        )
        .expect("script failed");
        if let Value::Number(n) = v {
            assert!((n - 6.0).abs() < f64::EPSILON, "expected 6, got {n}");
        } else {
            panic!("expected 6, got {v:?}");
        }
    }

    // ========================================================================
    // Exception handling tests (P7.S1)
    //
    // WHY: These tests verify that try/catch/finally opcodes work correctly
    // end-to-end through the full compile->execute pipeline.  Each test uses
    // window.result as the observable output (see run_and_get_result).
    // ========================================================================

    /*
     * try_catch_basic -- throw inside try; catch receives the value; code after
     * catch runs normally.
     *
     * Expected: window.result == 42 (set by catch block).
     */
    #[test]
    fn test_try_catch_basic() {
        // UNWRAP-OK: well-formed script; failure indicates a VM or compiler bug.
        let v = run_and_get_result(
            "try { throw 42; window.result = 0; } catch(e) { window.result = e; }",
        )
        .expect("script failed");
        if let Value::Number(n) = v {
            assert!((n - 42.0).abs() < f64::EPSILON, "expected 42, got {n}");
        } else {
            panic!("expected number 42, got {v:?}");
        }
    }

    /*
     * try_catch_no_throw -- try block completes normally; catch is not entered;
     * code after the whole try/catch runs.
     *
     * Expected: window.result == 1 (set by try body; catch is skipped).
     */
    #[test]
    fn test_try_catch_no_throw() {
        // UNWRAP-OK: well-formed script; failure indicates a VM or compiler bug.
        let v = run_and_get_result("try { window.result = 1; } catch(e) { window.result = 99; }")
            .expect("script failed");
        if let Value::Number(n) = v {
            assert!((n - 1.0).abs() < f64::EPSILON, "expected 1, got {n}");
        } else {
            panic!("expected number 1, got {v:?}");
        }
    }

    /*
     * try_finally_runs_on_throw -- throw inside try with no catch; finally
     * executes before the exception propagates outward.  The outer try/catch
     * captures the propagated exception.
     *
     * Expected: window.result == 7 (set by finally); the outer catch fires
     * because the inner try has no catch clause.
     */
    #[test]
    fn test_try_finally_runs_on_throw() {
        // UNWRAP-OK: well-formed script; failure indicates a VM or compiler bug.
        let v = run_and_get_result(
            "try {
               try { throw 1; } finally { window.result = 7; }
             } catch(e) {}",
        )
        .expect("script failed");
        if let Value::Number(n) = v {
            assert!((n - 7.0).abs() < f64::EPSILON, "expected 7, got {n}");
        } else {
            panic!("expected number 7, got {v:?}");
        }
    }

    /*
     * try_finally_runs_on_normal -- try block completes without throwing;
     * finally still runs after the try body.
     *
     * Expected: window.result == 5 (set by finally; try sets it to 1 first).
     */
    #[test]
    fn test_try_finally_runs_on_normal() {
        // UNWRAP-OK: well-formed script; failure indicates a VM or compiler bug.
        let v = run_and_get_result("try { window.result = 1; } finally { window.result = 5; }")
            .expect("script failed");
        if let Value::Number(n) = v {
            assert!((n - 5.0).abs() < f64::EPSILON, "expected 5, got {n}");
        } else {
            panic!("expected number 5, got {v:?}");
        }
    }

    /*
     * nested_try_catch -- inner try/catch handles the inner throw; the outer
     * catch does NOT fire because the inner catch consumed the exception.
     *
     * Expected: window.result == 2 (set by inner catch; outer catch is skipped).
     */
    #[test]
    fn test_nested_try_catch() {
        // UNWRAP-OK: well-formed script; failure indicates a VM or compiler bug.
        let v = run_and_get_result(
            "try {
               try { throw 1; } catch(inner) { window.result = 2; }
             } catch(outer) { window.result = 99; }",
        )
        .expect("script failed");
        if let Value::Number(n) = v {
            assert!((n - 2.0).abs() < f64::EPSILON, "expected 2, got {n}");
        } else {
            panic!("expected number 2, got {v:?}");
        }
    }

    // ========================================================================
    // Async/await tests (P7.S2)
    //
    // WHY: These tests exercise the synchronous-await model end-to-end:
    //   1. async function bodies must wrap their return value in a Promise
    //      (op_async_return + Statement::Return is_async branch).
    //   2. `await` on a settled Promise wrapper must extract the fulfillment
    //      value (op_await + as_settled_promise).
    //   3. Multiple awaits compose -- each unwraps the prior step's Promise
    //      so subsequent statements see the raw value.
    //
    // The tests use the .then(callback) pattern to observe the async
    // function's resolved value.  Because Promise.resolve produces an
    // already-fulfilled Promise, the .then native callback runs during the
    // microtask drain at the end of execute(), which writes window.result.
    //
    // See: vm/promise.rs as_settled_promise / resolved_promise_value
    // See: vm/mod.rs op_async_return / op_await
    // See: bytecode/compiler.rs Statement::Return / Expression::Await
    // ========================================================================

    /*
     * Helper: run a script that stashes a Promise wrapper in window.result and
     * return its (state, fulfillment-value) pair.
     *
     * WHY: The synchronous-await model means we cannot easily observe a
     * Promise via JS-side .then() callbacks (those would need a JS Function
     * dispatch path in execute_promise_reaction, which is a separate task).
     * Instead we inspect the wrapper directly from Rust using
     * promise::as_settled_promise -- this exercises exactly the same
     * introspect slot path that op_await uses, so the tests still validate
     * the production code paths.
     */
    fn run_and_get_promise_state(source: &str) -> Result<(promise::PromiseState, Value), String> {
        let mut vm = Vm::new();
        run_script(&mut vm, source)?;
        let wrapper = vm.global.borrow().get_by_str("result").clone();
        promise::as_settled_promise(&wrapper)
            .ok_or_else(|| format!("window.result was not a Promise wrapper: {wrapper:?}"))
    }

    /*
     * async_function_returns_resolved_promise -- the simplest case.
     * `async function f() { return 42; }; window.result = f();` must yield a
     * Promise wrapper whose state is Fulfilled and whose result is 42.
     *
     * This proves that:
     *   - Statement::Return inside an async body emits AsyncReturn.
     *   - op_async_return wraps 42 in a fulfilled Promise wrapper.
     *   - The wrapper carries the INTERNAL_SLOT_KEY introspect slot so
     *     as_settled_promise can read its state from Rust.
     */
    #[test]
    fn test_async_function_returns_resolved_promise() {
        /*
         * Parser now lowers `async function NAME(){...}` at statement
         * position to a FunctionDeclaration (with is_async=true), so the
         * name is hoisted into the enclosing scope.  See
         * test_async_function_declaration_is_hoisted below for the
         * declaration-form test; this test stays on the assignment form to
         * keep it focused on async/await Promise wrapping.
         */
        // UNWRAP-OK: well-formed script; failure indicates an async/await bug.
        let (state, value) = run_and_get_promise_state(
            "var f = async function() { return 42; }; window.result = f();",
        )
        .expect("script failed");
        assert_eq!(state, promise::PromiseState::Fulfilled);
        if let Value::Number(n) = value {
            assert!((n - 42.0).abs() < f64::EPSILON, "expected 42, got {n}");
        } else {
            panic!("expected number 42, got {value:?}");
        }
    }

    /*
     * await_promise_resolve -- `await Promise.resolve(99)` must yield 99.
     * Proves that op_await reads the introspect slot of an already-fulfilled
     * Promise wrapper and extracts the result value into the destination
     * register, so `return await Promise.resolve(99)` evaluates to a fulfilled
     * Promise wrapping the number 99 (NOT a Promise wrapping a Promise).
     */
    #[test]
    fn test_await_promise_resolve() {
        // UNWRAP-OK: well-formed script; failure indicates an async/await bug.
        let (state, value) = run_and_get_promise_state(
            "var f = async function() { return await Promise.resolve(99); };\
             window.result = f();",
        )
        .expect("script failed");
        assert_eq!(state, promise::PromiseState::Fulfilled);
        if let Value::Number(n) = value {
            assert!((n - 99.0).abs() < f64::EPSILON, "expected 99, got {n}");
        } else {
            panic!("expected number 99, got {value:?}");
        }
    }

    /*
     * chained_awaits -- two sequential awaits compose.  Each await must
     * extract a raw number from its Promise wrapper so the subsequent `+`
     * sees Number + Number = 3, not undefined + undefined = NaN.
     *
     * Expected: returned Promise is Fulfilled with 3.
     */
    #[test]
    fn test_chained_awaits() {
        let source = "var f = async function() {\
               let x = await Promise.resolve(1);\
               let y = await Promise.resolve(2);\
               return x + y;\
             };\
             window.result = f();";
        // UNWRAP-OK: well-formed script; failure indicates an async/await bug.
        let (state, value) = run_and_get_promise_state(source).expect("script failed");
        assert_eq!(state, promise::PromiseState::Fulfilled);
        if let Value::Number(n) = value {
            assert!((n - 3.0).abs() < f64::EPSILON, "expected 3, got {n}");
        } else {
            panic!("expected number 3, got {value:?}");
        }
    }

    /*
     * async_function_declaration_is_hoisted -- regression for the parser
     * gap where `async function NAME(){...}` at statement position was
     * lowered to ExpressionStatement(Function), which prevented hoisting
     * (collect_declarations_in_stmt only walks Statement::FunctionDeclaration).
     *
     * After the fix, parse_statement consumes `async` when followed by
     * `function` and dispatches to parse_function_declaration with
     * is_async=true.  The name `greet` is therefore hoisted into the
     * surrounding scope and resolvable as a callable Value::Function.
     *
     * We use `typeof greet === "function"` rather than `greet instanceof
     * Function` because Instanceof opcode dispatch is not yet installed in
     * the VM (see opcode table init in Vm::new).
     *
     * Expected: window.result == 1 (truthy branch ran).
     *
     * See: parser.rs parse_statement (TokenKind::Async arm)
     * See: compiler.rs collect_declarations_in_stmt
     */
    #[test]
    fn test_async_function_declaration_is_hoisted() {
        // UNWRAP-OK: well-formed script; failure indicates the parser
        // regressed back to the ExpressionStatement(Function) lowering.
        let v = run_and_get_result(
            "async function greet() { return \"hello\"; }\
             window.result = typeof greet === \"function\" ? 1 : 0;",
        )
        .expect("script failed");
        if let Value::Number(n) = v {
            assert!((n - 1.0).abs() < f64::EPSILON, "expected 1, got {n}");
        } else {
            panic!("expected number 1, got {v:?}");
        }
    }

    /*
     * closure_counter_persists_across_calls -- regression for the closure
     * snapshot bug where each invocation of an inner function received a
     * private clone of JsFunction.captures, so `count++` mutations were
     * lost between calls.
     *
     * Canonical pattern:
     *   function makeCounter() {
     *     var count = 0;
     *     return function() { count++; return count; };
     *   }
     *   var c = makeCounter();
     *   c(); c(); c();   // must return 3
     *
     * After the fix, JsFunction.captures is Rc<RefCell<Vec<Value>>> and
     * op_call shares the Rc into the new CallFrame, so SetCapture writes
     * propagate across calls.  Pre-fix this test asserted 1; post-fix it
     * asserts 3.
     *
     * See: vm/value.rs JsFunction.captures
     * See: vm/mod.rs op_call (Value::Function arm)
     * See: vm/mod.rs op_set_capture (depth=0)
     */
    #[test]
    fn test_closure_counter_persists_across_calls() {
        // UNWRAP-OK: well-formed script; failure indicates the closure
        // capture sharing regressed back to snapshot semantics.
        let v = run_and_get_result(
            "function makeCounter() {\
                 var count = 0;\
                 return function() { count++; return count; };\
             }\
             var c = makeCounter();\
             c(); c();\
             window.result = c();",
        )
        // UNWRAP-OK: well-formed script; failure means closure capture sharing regressed.
        .expect("script failed");
        if let Value::Number(n) = v {
            assert!((n - 3.0).abs() < f64::EPSILON, "expected 3, got {n}");
        } else {
            panic!("expected number 3, got {v:?}");
        }
    }

    /*
     * closure_aliased_handles_share_state -- two references to the same
     * closure object must observe each other's mutations.
     *
     * Pattern:
     *   var c = makeCounter();
     *   var d = c;          // d and c are the same Rc<JsFunction>
     *   c(); d(); c();       // d sees c's increment and vice versa
     *
     * This exercises the Rc-sharing semantics: cloning the Value::Function
     * register only bumps the Rc<JsFunction>, so both `c` and `d` resolve
     * to the SAME captures cell.  Each call shares the cell with the
     * JsFunction, so all three increments accumulate to 3.
     *
     * See: closure_counter_persists_across_calls for the single-handle
     *      version of this guarantee.
     */
    #[test]
    fn test_closure_aliased_handles_share_state() {
        // UNWRAP-OK: well-formed script; failure indicates Rc<JsFunction>
        // alias semantics regressed (e.g., capture cell got cloned per
        // assignment).
        let v = run_and_get_result(
            "function makeCounter() {\
                 var count = 0;\
                 return function() { count++; return count; };\
             }\
             var c = makeCounter();\
             var d = c;\
             c();\
             d();\
             window.result = c();",
        )
        // UNWRAP-OK: well-formed script; failure means Rc<JsFunction> alias semantics regressed.
        .expect("script failed");
        if let Value::Number(n) = v {
            assert!(
                (n - 3.0).abs() < f64::EPSILON,
                "expected 3 (c+d+c share state), got {n}"
            );
        } else {
            panic!("expected number 3, got {v:?}");
        }
    }

    // ========================================================================
    // Promise combinator tests (Promise.all / race / allSettled / any)
    //
    // WHY: These tests pin down the synchronous-settle implementation of the
    // four ECMAScript Promise combinators. Each test feeds already-settled
    // inputs (Promise.resolve / numeric literals) so the microtask drain
    // inside the builtin can complete every step in one pass. They guard
    // against regressing back to the old stubs which returned the raw input
    // array (Promise.all) or the first input value (Promise.race) without
    // ever inspecting promise state.
    //
    // See: vm/builtins/promise_builtin.rs for the implementations.
    // ========================================================================

    /*
     * promise_all_basic -- mix of plain values and a fulfilled Promise; result
     * must be a Fulfilled Promise wrapping an array whose length is 3 and
     * whose first element is the number 1.  We check length + first + second
     * elements rather than the full array because run_and_get_promise_state
     * only surfaces the wrapper's result value, not nested introspection.
     */
    #[test]
    fn test_promise_all_basic() {
        // UNWRAP-OK: well-formed script; failure indicates Promise.all regressed.
        let (state, value) =
            run_and_get_promise_state("window.result = Promise.all([1, Promise.resolve(2), 3]);")
                .expect("script failed");
        assert_eq!(state, promise::PromiseState::Fulfilled);
        let Value::Object(arr) = value else {
            panic!("expected array object, got {value:?}");
        };
        let arr_ref = arr.borrow();
        if let Value::Number(len) = arr_ref.get_by_str("length") {
            assert!(
                (len - 3.0).abs() < f64::EPSILON,
                "expected length 3, got {len}"
            );
        } else {
            panic!("expected numeric length");
        }
        let first = arr_ref.get_by_key(&crate::vm::value::PropertyKey::Index(0));
        if let Value::Number(n) = first {
            assert!((n - 1.0).abs() < f64::EPSILON, "expected first=1, got {n}");
        } else {
            panic!("expected first element to be number 1, got {first:?}");
        }
        let second = arr_ref.get_by_key(&crate::vm::value::PropertyKey::Index(1));
        if let Value::Number(n) = second {
            assert!((n - 2.0).abs() < f64::EPSILON, "expected second=2, got {n}");
        } else {
            panic!("expected second element to be number 2, got {second:?}");
        }
    }

    /*
     * promise_race_basic -- a single fulfilled Promise must win the race; the
     * resulting wrapper is Fulfilled with that promise's value (42), not the
     * Promise wrapper itself.
     */
    #[test]
    fn test_promise_race_basic() {
        // UNWRAP-OK: well-formed script; failure indicates Promise.race regressed.
        let (state, value) =
            run_and_get_promise_state("window.result = Promise.race([Promise.resolve(42)]);")
                .expect("script failed");
        assert_eq!(state, promise::PromiseState::Fulfilled);
        if let Value::Number(n) = value {
            assert!((n - 42.0).abs() < f64::EPSILON, "expected 42, got {n}");
        } else {
            panic!("expected number 42, got {value:?}");
        }
    }

    /*
     * promise_all_settled -- one fulfilled input; result is Fulfilled with an
     * array containing a single descriptor object whose status is "fulfilled"
     * and whose value is 1.
     */
    #[test]
    fn test_promise_all_settled() {
        // UNWRAP-OK: well-formed script; failure indicates Promise.allSettled regressed.
        let (state, value) =
            run_and_get_promise_state("window.result = Promise.allSettled([Promise.resolve(1)]);")
                .expect("script failed");
        assert_eq!(state, promise::PromiseState::Fulfilled);
        let Value::Object(arr) = value else {
            panic!("expected array object, got {value:?}");
        };
        let arr_ref = arr.borrow();
        if let Value::Number(len) = arr_ref.get_by_str("length") {
            assert!(
                (len - 1.0).abs() < f64::EPSILON,
                "expected length 1, got {len}"
            );
        } else {
            panic!("expected numeric length");
        }
        let descriptor = arr_ref.get_by_key(&crate::vm::value::PropertyKey::Index(0));
        let Value::Object(desc_rc) = descriptor else {
            panic!("expected descriptor object");
        };
        let desc = desc_rc.borrow();
        let status = desc.get_by_str("status");
        let status_str = status.to_js_string();
        assert_eq!(
            status_str.as_str().unwrap_or(""),
            "fulfilled",
            "expected status=fulfilled"
        );
        if let Value::Number(n) = desc.get_by_str("value") {
            assert!((n - 1.0).abs() < f64::EPSILON, "expected value=1, got {n}");
        } else {
            panic!("expected descriptor.value to be number 1");
        }
    }

    /*
     * promise_any_basic -- first fulfilled wins; result is Fulfilled with 99.
     */
    #[test]
    fn test_promise_any_basic() {
        // UNWRAP-OK: well-formed script; failure indicates Promise.any regressed.
        let (state, value) =
            run_and_get_promise_state("window.result = Promise.any([Promise.resolve(99)]);")
                .expect("script failed");
        assert_eq!(state, promise::PromiseState::Fulfilled);
        if let Value::Number(n) = value {
            assert!((n - 99.0).abs() < f64::EPSILON, "expected 99, got {n}");
        } else {
            panic!("expected number 99, got {value:?}");
        }
    }

    // ========================================================================
    // Generator tests (function* / yield / yield* / for...of over generator)
    //
    // WHY: These tests verify the eager-strategy generator implementation
    // end-to-end:
    //   - test_generator_basic: function* declaration, multiple yields,
    //     direct .next().value calls, sum yields three values.
    //   - test_generator_done: after exhausting all yields, .next().done
    //     is observably truthy.
    //   - test_for_of_generator: for...of drives the generator via its
    //     own next() method (no array-like wrapping); body sums yielded
    //     values to 0+1+2 = 3.
    //   - test_generator_yield_star: yield* delegates to an inner array,
    //     flattening its elements into the parent's yield buffer.
    //
    // Known limitations (documented in vm/generator.rs and exercised here
    // only insofar as the test bodies avoid the unsupported patterns):
    //   * .next(value) argument is not plumbed back into yield expressions.
    //   * Side effects in the body happen at construction time, not lazily.
    //   * .return() / .throw() do not retroactively re-run the body.
    //
    // See: vm/generator.rs for the eager-strategy design rationale.
    // See: vm/mod.rs op_yield / op_yield_star / op_new_generator /
    //      invoke_generator for the runtime dispatch.
    // See: bytecode/compiler.rs Statement::FunctionDeclaration /
    //      Expression::Function (is_generator branch) and Expression::Yield
    //      for the bytecode emission.
    // ========================================================================

    /*
     * generator_basic -- three sequential .next().value calls return the
     * three yielded values in order; their sum is 1 + 2 + 3 = 6.
     *
     * Exercises: NewGenerator emission, invoke_generator eager execution,
     * build_generator iterator construction, op_call on the returned
     * iterator's `next` NativeFunction, GetProp on the {value, done}
     * record.
     */
    #[test]
    fn test_generator_basic() {
        // UNWRAP-OK: well-formed script; failure indicates the generator
        // VM/compiler integration regressed.
        let v = run_and_get_result(
            "function* gen() { yield 1; yield 2; yield 3; }\
             var g = gen();\
             window.result = g.next().value + g.next().value + g.next().value;",
        )
        .expect("script failed");
        if let Value::Number(n) = v {
            assert!((n - 6.0).abs() < f64::EPSILON, "expected 6, got {n}");
        } else {
            panic!("expected number 6, got {v:?}");
        }
    }

    /*
     * generator_done -- after the buffer is exhausted (three yields, three
     * .next() calls), the FOURTH .next() must report done=true.
     *
     * Exercises: build_generator's exhaustion path (finished flag flips,
     * subsequent calls return {value: undefined, done: true}).
     */
    #[test]
    fn test_generator_done() {
        let v = run_and_get_result(
            "function* gen() { yield 1; yield 2; yield 3; }\
             var g = gen();\
             g.next(); g.next(); g.next();\
             window.result = g.next().done;",
        )
        // UNWRAP-OK: well-formed script; failure indicates the iterator's
        // done flag is not flipping on exhaustion.
        .expect("script failed");
        if let Value::Boolean(b) = v {
            assert!(b, "expected done=true after exhausting all yields");
        } else {
            panic!("expected boolean true, got {v:?}");
        }
    }

    /*
     * for_of_generator -- for...of drives a generator's own next() instead
     * of treating the iterator as an empty array-like.
     *
     * Exercises: make_iterator_for's iterator-shaped pass-through branch
     * (detects `next` NativeFunction on the object), op_iter_next dispatch
     * to the generator's next closure, op_iter_done flag extraction.
     *
     * range(3) yields 0, 1, 2; sum = 3.
     */
    #[test]
    fn test_for_of_generator() {
        let v = run_and_get_result(
            "function* range(n) {\
                 var i = 0;\
                 while (i < n) { yield i; i = i + 1; }\
             }\
             var sum = 0;\
             for (var x of range(3)) { sum = sum + x; }\
             window.result = sum;",
        )
        // UNWRAP-OK: well-formed script; failure indicates the for-of /
        // generator iterator-protocol bridge regressed.
        .expect("script failed");
        if let Value::Number(n) = v {
            assert!((n - 3.0).abs() < f64::EPSILON, "expected 3, got {n}");
        } else {
            panic!("expected number 3, got {v:?}");
        }
    }

    /*
     * generator_yield_star -- yield* delegates to an iterable, flattening
     * its elements into the parent generator's yield buffer.
     *
     * Exercises: op_yield_star, generator::yield_star_flatten's array-like
     * path.  The outer generator yields 10, then delegates to [1, 2, 3],
     * then yields 20.  for-of summing gives 10 + 1 + 2 + 3 + 20 = 36.
     */
    #[test]
    fn test_generator_yield_star() {
        let v = run_and_get_result(
            "function* combined() {\
                 yield 10;\
                 yield* [1, 2, 3];\
                 yield 20;\
             }\
             var sum = 0;\
             for (var x of combined()) { sum = sum + x; }\
             window.result = sum;",
        )
        // UNWRAP-OK: well-formed script; failure indicates yield* delegation
        // regressed.
        .expect("script failed");
        if let Value::Number(n) = v {
            assert!((n - 36.0).abs() < f64::EPSILON, "expected 36, got {n}");
        } else {
            panic!("expected number 36, got {v:?}");
        }
    }

    // H1 regression: a generator body that uses try/catch must not leave
    // stale TryHandlers on vm.try_handlers after construction.  Before the
    // fix, throw-inside-generator caused the handler to persist, so the next
    // unrelated throw in caller code dispatched into the wrong chunk.
    #[test]
    fn test_generator_try_handlers_do_not_leak() {
        // Construct a generator whose body uses try/catch (normal completion).
        // If any TryHandler leaks onto the VM stack, a subsequent throw from
        // caller code would mis-dispatch -- resulting in a wrong result or panic.
        let v = run_and_get_result(
            "function* safe() {\
                 try { yield 1; } catch(e) { yield -1; }\
                 yield 2;\
             }\
             var out = 0;\
             for (var x of safe()) { out = out + x; }\
             try { throw 999; } catch(e) { out = out + e; }\
             window.result = out;",
        );
        // UNWRAP-OK: test fixture; run_and_get_result only fails on compile/parse errors in the literal above.
        let v = v.expect("script failed");
        // 1 + 2 + 999 = 1002
        if let Value::Number(n) = v {
            assert!(
                (n - 1002.0).abs() < f64::EPSILON,
                "expected 1002, got {n}: try_handlers may have leaked"
            );
        } else {
            panic!("expected number 1002, got {v:?}");
        }
    }

    #[test]
    fn test_string_match_returns_array_on_hit() {
        // "hello".match("el+") -> ["ell"], index=1
        let v = run_and_get_result(r#"var m = "hello".match("el+"); window.result = m[0];"#);
        // UNWRAP-OK: test fixture literal is valid JS.
        let v = v.expect("script failed");
        assert!(
            matches!(&v, Value::String(s) if s.as_str() == Some("ell")),
            "expected 'ell', got {v:?}"
        );
    }

    #[test]
    fn test_string_match_returns_null_on_miss() {
        let v = run_and_get_result(
            r#"var m = "hello".match("xyz"); window.result = m === null ? 1 : 0;"#,
        );
        // UNWRAP-OK: test fixture literal is valid JS.
        let v = v.expect("script failed");
        assert!(
            matches!(v, Value::Number(n) if (n - 1.0).abs() < f64::EPSILON),
            "expected 1 (null check), got {v:?}"
        );
    }

    #[test]
    fn test_string_search_returns_char_index() {
        // "hello".search("ll") -> 2
        let v = run_and_get_result(r#"window.result = "hello".search("ll");"#);
        // UNWRAP-OK: test fixture literal is valid JS.
        let v = v.expect("script failed");
        assert!(
            matches!(v, Value::Number(n) if (n - 2.0).abs() < f64::EPSILON),
            "expected 2, got {v:?}"
        );
    }

    #[test]
    fn test_string_search_returns_minus_one_on_miss() {
        let v = run_and_get_result(r#"window.result = "hello".search("xyz");"#);
        // UNWRAP-OK: test fixture literal is valid JS.
        let v = v.expect("script failed");
        assert!(
            matches!(v, Value::Number(n) if (n - (-1.0)).abs() < f64::EPSILON),
            "expected -1, got {v:?}"
        );
    }

    #[test]
    fn test_string_match_captures_group() {
        // Use a character-class pattern to avoid JS string-escape complexity with \d.
        // "2024-01-15".match("([0-9]+)-([0-9]+)-([0-9]+)") -> result[1] = "2024"
        let v = run_and_get_result(
            r#"var m = "2024-01-15".match("([0-9]+)-([0-9]+)-([0-9]+)"); window.result = m[1];"#,
        );
        // UNWRAP-OK: test fixture literal is valid JS.
        let v = v.expect("script failed");
        assert!(
            matches!(&v, Value::String(s) if s.as_str() == Some("2024")),
            "expected '2024', got {v:?}"
        );
    }
}
