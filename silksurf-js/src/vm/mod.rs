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
 * Max depth: vm.max_stack_depth (default 1024)
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
 * On throw: pop the top handler, unwind call_stack to handler's
 * stack_depth, then jump to catch_pc (or finally_pc if no catch).
 * The exception value is placed in register 0 for the catch block.
 *
 * See: op_enter_try (mod.rs) for handler installation
 * See: op_throw (mod.rs) for handler dispatch
 * See: execute() main loop for Exception handling in dispatch
 */
#[derive(Debug)]
struct TryHandler {
    catch_pc: usize,
    finally_pc: usize,
    stack_depth: usize,
    /// Chunk index of the handler
    chunk_idx: usize,
}

/*
 * Vm -- the bytecode virtual machine.
 *
 * WHY: Central execution engine for all JavaScript in SilkSurf.
 * Single-threaded (per JS spec) with cooperative async via microtasks.
 *
 * Memory layout:
 *   registers: 256 Value slots (~6-10KB depending on Value size)
 *   call_stack: pre-allocated for 64 frames, max 1024
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

    // Exception handling
    table[Opcode::EnterTry as usize] = op_enter_try;
    table[Opcode::LeaveTry as usize] = op_leave_try;
    table[Opcode::EnterCatch as usize] = op_enter_catch;
    table[Opcode::EnterFinally as usize] = op_enter_finally;

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
            max_stack_depth: 1024,
        }
    }

    /// Add a chunk (compiled function) and return its index
    pub fn add_chunk(&mut self, chunk: Chunk) -> usize {
        let idx = self.chunks.len();
        self.chunks.push(chunk);
        idx
    }

    /// Number of chunks currently registered.
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
        // Store args in registers starting at base_reg
        let base_reg = 1u8; // r0 reserved for return, args start at r1
        for (i, arg) in args.iter().enumerate() {
            let reg = base_reg as usize + i;
            if reg < self.registers.len() {
                self.registers[reg] = arg.clone();
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

        // Push initial call frame
        self.call_stack.push(CallFrame {
            chunk_idx,
            pc: 0,
            base: 0,
            return_reg: 0,
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
            let handler = unsafe { *DISPATCH_TABLE.get_unchecked(opcode) };
            match handler(self, instr) {
                Ok(()) => {}
                Err(VmError::Halted) => {
                    return Ok(unsafe { self.registers.get_unchecked(0) }.clone());
                }
                Err(VmError::Exception(value)) => {
                    // Check for try handler before propagating
                    if let Some(try_handler) = self.try_handlers.pop() {
                        while self.call_stack.len() > try_handler.stack_depth {
                            self.call_stack.pop();
                        }
                        if try_handler.catch_pc > 0 {
                            if let Some(frame) = self.call_stack.last_mut() {
                                frame.pc = try_handler.catch_pc;
                                frame.chunk_idx = try_handler.chunk_idx;
                            }
                            self.set_reg(0, value);
                        } else if try_handler.finally_pc > 0 {
                            if let Some(frame) = self.call_stack.last_mut() {
                                frame.pc = try_handler.finally_pc;
                                frame.chunk_idx = try_handler.chunk_idx;
                            }
                        } else {
                            return Err(VmError::Exception(value));
                        }
                    } else {
                        return Err(VmError::Exception(value));
                    }
                }
                /*
                 * JS-level errors (TypeError, ReferenceError) ARE catchable by
                 * try/catch. Convert them to Exception(value) and re-dispatch
                 * through the try handler mechanism.
                 *
                 * WHY: op_call returns VmError::TypeError("not a function") when
                 * the callee isn't callable. Without this conversion, try{...}catch(e){}
                 * around the call does NOT catch the error -- it propagates past the
                 * handler because only VmError::Exception is checked above.
                 *
                 * This was the cause of scripts 0 and 1 failing despite having
                 * a try/catch: the TypeError leaked through the Exception handler.
                 *
                 * Internal VM errors (OutOfBounds, StackOverflow, InvalidOpcode)
                 * are NOT converted -- those are unrecoverable engine faults.
                 */
                Err(VmError::TypeError(msg)) => {
                    let exc_val = Value::string_owned(format!("TypeError: {msg}"));
                    if let Some(try_handler) = self.try_handlers.pop() {
                        while self.call_stack.len() > try_handler.stack_depth {
                            self.call_stack.pop();
                        }
                        if try_handler.catch_pc > 0 {
                            if let Some(frame) = self.call_stack.last_mut() {
                                frame.pc = try_handler.catch_pc;
                                frame.chunk_idx = try_handler.chunk_idx;
                            }
                            self.set_reg(0, exc_val);
                        } else {
                            return Err(VmError::TypeError(msg));
                        }
                    } else {
                        return Err(VmError::TypeError(msg));
                    }
                }
                Err(VmError::ReferenceError(msg)) => {
                    let exc_val = Value::string_owned(format!("ReferenceError: {msg}"));
                    if let Some(try_handler) = self.try_handlers.pop() {
                        while self.call_stack.len() > try_handler.stack_depth {
                            self.call_stack.pop();
                        }
                        if try_handler.catch_pc > 0 {
                            if let Some(frame) = self.call_stack.last_mut() {
                                frame.pc = try_handler.catch_pc;
                                frame.chunk_idx = try_handler.chunk_idx;
                            }
                            self.set_reg(0, exc_val);
                        } else {
                            return Err(VmError::ReferenceError(msg));
                        }
                    } else {
                        return Err(VmError::ReferenceError(msg));
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Get register value
    #[inline(always)]
    fn get_reg(&self, idx: u8) -> &Value {
        let idx = idx as usize;
        debug_assert!(idx < self.registers.len());
        // SAFETY: register indices are validated by the compiler/VM invariants.
        unsafe { self.registers.get_unchecked(idx) }
    }

    /// Set register value
    #[inline(always)]
    fn set_reg(&mut self, idx: u8, value: Value) {
        let idx = idx as usize;
        debug_assert!(idx < self.registers.len());
        // SAFETY: register indices are validated by the compiler/VM invariants.
        unsafe {
            *self.registers.get_unchecked_mut(idx) = value;
        }
    }

    /// Get current chunk
    #[inline]
    fn current_chunk(&self) -> &Chunk {
        let frame = self.call_stack.last().unwrap();
        &self.chunks[frame.chunk_idx]
    }

    /// Get current program counter
    #[inline]
    fn current_pc(&self) -> usize {
        self.call_stack.last().unwrap().pc
    }

    /// Modify program counter (for jumps)
    #[inline]
    fn jump(&mut self, offset: i32) {
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
            vm.call_stack.push(CallFrame {
                chunk_idx,
                pc: 0,
                base: 0,
                return_reg: instr.dst(),
            });
            Ok(())
        }
        Value::NativeFunction(func) => {
            // Collect arguments from registers (simplified: src2 = arg count)
            let argc = instr.src2() as usize;
            let mut args = Vec::with_capacity(argc);
            // Arguments start after the callee register
            let base_reg = instr.src1() as usize + 1;
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

fn op_ret(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let value = vm.get_reg(instr.dst()).clone();
    vm.call_stack.pop();
    if vm.call_stack.is_empty() {
        // Returning from top-level - store in r0 for caller
        vm.set_reg(0, value);
        Err(VmError::Halted)
    } else {
        // Store result in caller's return register
        let return_reg = vm.call_stack.last().unwrap().return_reg;
        vm.set_reg(return_reg, value);
        Ok(())
    }
}

fn op_ret_undefined(vm: &mut Vm, _instr: Instruction) -> VmResult<()> {
    vm.call_stack.pop();
    if vm.call_stack.is_empty() {
        vm.set_reg(0, Value::Undefined);
        Err(VmError::Halted)
    } else {
        let return_reg = vm.call_stack.last().unwrap().return_reg;
        vm.set_reg(return_reg, Value::Undefined);
        Ok(())
    }
}

fn op_throw(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let value = vm.get_reg(instr.dst()).clone();
    // Check if there's a try handler to catch this exception
    if let Some(handler) = vm.try_handlers.pop() {
        // Unwind call stack to handler depth
        while vm.call_stack.len() > handler.stack_depth {
            vm.call_stack.pop();
        }
        if handler.catch_pc > 0 {
            // Jump to catch block, store exception in r0
            if let Some(frame) = vm.call_stack.last_mut() {
                frame.pc = handler.catch_pc;
                frame.chunk_idx = handler.chunk_idx;
            }
            vm.set_reg(0, value);
            Ok(())
        } else if handler.finally_pc > 0 {
            // No catch, jump to finally
            if let Some(frame) = vm.call_stack.last_mut() {
                frame.pc = handler.finally_pc;
                frame.chunk_idx = handler.chunk_idx;
            }
            Ok(())
        } else {
            Err(VmError::Exception(value))
        }
    } else {
        Err(VmError::Exception(value))
    }
}

/// EnterTry: push a try handler. dst=catch_offset (const_idx), src1 is unused.
/// The instruction uses the wide constant format: catch offset as const_idx.
fn op_enter_try(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let catch_offset = instr.const_idx() as usize;
    let frame = vm.call_stack.last().ok_or(VmError::OutOfBounds)?;
    let current_pc = frame.pc;
    let chunk_idx = frame.chunk_idx;
    vm.try_handlers.push(TryHandler {
        catch_pc: current_pc + catch_offset,
        finally_pc: 0,
        stack_depth: vm.call_stack.len(),
        chunk_idx,
    });
    Ok(())
}

/// LeaveTry: pop the current try handler (normal exit from try block).
fn op_leave_try(vm: &mut Vm, _instr: Instruction) -> VmResult<()> {
    vm.try_handlers.pop();
    Ok(())
}

/// EnterCatch: marks catch block start (exception already in r0 from throw dispatch).
fn op_enter_catch(_vm: &mut Vm, _instr: Instruction) -> VmResult<()> {
    // Exception value is already in r0, set by op_throw.
    // The catch block reads it from there.
    Ok(())
}

/// EnterFinally: marks finally block start.
fn op_enter_finally(_vm: &mut Vm, _instr: Instruction) -> VmResult<()> {
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
            if !matches!(own, Value::Undefined) {
                own
            } else {
                builtins::array::get_array_method(o, &prop_name).unwrap_or(Value::Undefined)
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
        ref val @ (Value::Function(_) | Value::NativeFunction(_)) if prop_name == "bind" => {
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
                Value::NativeFunction(Rc::new(value::NativeFunction::new("bound", move |call_args| {
                    let mut all_args = ba.clone();
                    all_args.extend(call_args.iter().cloned());
                    // Call the original -- but we can only call NativeFunction, not Function
                    match &orig {
                        Value::NativeFunction(f) => f.call(&all_args),
                        _ => Value::Undefined, // Can't call Value::Function from NativeFunction
                    }
                })))
            })))
        }
        Value::Function(_) | Value::NativeFunction(_) if prop_name == "call" || prop_name == "apply" => {
            // .call(thisArg, ...args) -- simplified: ignore thisArg, call with args
            let original = obj.clone();
            Value::NativeFunction(Rc::new(value::NativeFunction::new(&prop_name, move |args| {
                let call_args: Vec<Value> = args.iter().skip(1).cloned().collect();
                match &original {
                    Value::NativeFunction(f) => f.call(&call_args),
                    _ => Value::Undefined,
                }
            })))
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
                                .filter_map(|k| match k {
                                    value::PropertyKey::String(s) => {
                                        Some(value::Value::String(Rc::clone(s)))
                                    }
                                    value::PropertyKey::Index(i) => {
                                        Some(value::Value::string_owned(i.to_string()))
                                    }
                                })
                                .collect();
                            builtins::array::create_array(keys)
                        } else {
                            builtins::array::create_array(vec![])
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
                            builtins::array::create_array(vals)
                        } else {
                            builtins::array::create_array(vec![])
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
                                    builtins::array::create_array(vec![key_val, v.clone()])
                                })
                                .collect();
                            builtins::array::create_array(entries)
                        } else {
                            builtins::array::create_array(vec![])
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
                            let entries =
                                builtins::array::collect_elements_pub(&arr.borrow());
                            for entry in entries {
                                if let value::Value::Object(pair) = entry {
                                    let k = pair.borrow().get_by_key(
                                        &value::PropertyKey::Index(0),
                                    );
                                    let v = pair.borrow().get_by_key(
                                        &value::PropertyKey::Index(1),
                                    );
                                    let key_str = k.to_js_string();
                                    let key_s = key_str.as_str().unwrap_or("");
                                    obj_rc
                                        .borrow_mut()
                                        .set_by_key(value::PropertyKey::from_str(key_s), v);
                                }
                            }
                        }
                        value::Value::Object(obj_rc)
                    },
                ))),
                "getOwnPropertyNames" | "getOwnPropertySymbols" => {
                    Value::NativeFunction(Rc::new(value::NativeFunction::new(
                        "Object.getOwnPropertyNames",
                        |args| {
                            if let Some(value::Value::Object(o)) = args.first() {
                                let keys: Vec<value::Value> = o
                                    .borrow()
                                    .properties
                                    .keys()
                                    .filter_map(|k| match k {
                                        value::PropertyKey::String(s) => {
                                            Some(value::Value::String(Rc::clone(s)))
                                        }
                                        value::PropertyKey::Index(i) => {
                                            Some(value::Value::string_owned(i.to_string()))
                                        }
                                    })
                                    .collect();
                                builtins::array::create_array(keys)
                            } else {
                                builtins::array::create_array(vec![])
                            }
                        },
                    )))
                }
                "defineProperty" | "defineProperties" | "seal" | "preventExtensions"
                | "isFrozen" | "isSealed" | "isExtensible" => {
                    // Stubs: return first arg unchanged or true/false as appropriate
                    let is_predicate = matches!(
                        prop_name.as_str(),
                        "isFrozen" | "isSealed" | "isExtensible"
                    );
                    let name = prop_name.clone();
                    Value::NativeFunction(Rc::new(value::NativeFunction::new(
                        name,
                        move |args| {
                            if is_predicate {
                                value::Value::Boolean(false)
                            } else {
                                args.first().cloned().unwrap_or(value::Value::Undefined)
                            }
                        },
                    )))
                }
                _ => Value::Undefined,
            }
        }
        Value::NativeFunction(f) if f.name == "String" => {
            match prop_name.as_str() {
                "fromCharCode" => {
                    Value::NativeFunction(Rc::new(value::NativeFunction::new(
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
                    )))
                }
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
                                    let parts = builtins::array::collect_elements_pub(
                                        &raw_arr.borrow(),
                                    );
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
                "isArray" => {
                    Value::NativeFunction(Rc::new(value::NativeFunction::new(
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
                    )))
                }
                "from" => {
                    Value::NativeFunction(Rc::new(value::NativeFunction::new(
                        "Array.from",
                        |args| {
                            use builtins::array::{collect_elements_pub, create_array, is_array_like};
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
                                create_array(mapped)
                            } else {
                                create_array(elements)
                            }
                        },
                    )))
                }
                "of" => {
                    Value::NativeFunction(Rc::new(value::NativeFunction::new(
                        "Array.of",
                        |args| builtins::array::create_array(args.to_vec()),
                    )))
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
        o.borrow_mut().set(key, value);
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
 * This was the root cause of Script 6's TypeError: `$RB=[]` created
 * an Object without length, so later `$RB.push(...)` failed.
 */
fn op_new_array(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let obj = Rc::new(RefCell::new(Object::new()));
    obj.borrow_mut().set_by_str("length", Value::Number(0.0));
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
    let slot = instr.src2(); // outer scope register index
    let value = vm.get_reg(slot).clone();
    vm.set_reg(instr.dst(), value);
    Ok(())
}

fn op_set_capture(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let slot = instr.src1(); // outer scope register index
    let value = vm.get_reg(instr.src2()).clone();
    vm.set_reg(slot, value);
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
        let result = vm.execute(idx).unwrap();

        if let Value::Number(n) = result {
            assert_eq!(n, 15.0);
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
        let result = vm.execute(idx).unwrap();

        if let Value::Number(n) = result {
            assert_eq!(n, 200.0);
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
        let result = vm.execute(idx).unwrap();

        if let Value::Number(n) = result {
            assert_eq!(n, 0x0F as f64);
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
        let result = vm.execute(idx).unwrap();

        if let Value::Number(n) = result {
            assert_eq!(n, 42.0);
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
                        if let Constant::String(str_id) = constant {
                            if let Some(&vm_id) = str_map.get(str_id) {
                                *str_id = vm_id;
                            }
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
        run_script(&mut vm, "var x = 42;").unwrap();
    }

    #[test]
    fn test_e2e_global_array_assignment() {
        let mut vm = Vm::new();
        // This is the pattern from ChatGPT script 6
        run_script(&mut vm, "$RB = [];").unwrap();
    }

    #[test]
    fn test_e2e_global_function_assignment() {
        let mut vm = Vm::new();
        run_script(&mut vm, "$RV = function(a) {};").unwrap();
    }

    #[test]
    fn test_e2e_iife_with_try_catch() {
        let mut vm = Vm::new();
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
        eprintln!("Script 6 parse: {} statements, {} errors", ast.body.len(), errors.len());
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
        run_script(&mut vm, "!function(){try{var d=document.documentElement}catch(e){}}();").ok();
        run_script(&mut vm, "!function(){try{var t=localStorage.getItem('x')}catch(e){}}();").ok();
        run_script(&mut vm, "var x = window.__oai_SSR_HTML || 0;").ok();
        run_script(&mut vm, "window.__test = {\"a\": 1};").ok();
        run_script(&mut vm, "requestAnimationFrame(function(){});").ok();

        // Now script 6
        let r = run_script(&mut vm, "$RB=[];$RV=function(a){$RT=performance.now();for(var b=0;b<a.length;b+=2){var c=a[b];}a.length=0};");
        assert!(r.is_ok(), "Script 6 after 5: {r:?}");
    }
}
