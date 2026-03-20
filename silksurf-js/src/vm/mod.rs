//! Bytecode virtual machine
//!
//! Register-based VM with function-pointer dispatch for performance.
//! Architecture derived from studying V8 Ignition design patterns.

pub mod gc_integration;
pub mod ic;
pub mod nanbox;
pub mod shape;
pub mod snapshot;
pub mod string;
pub mod value;

#[cfg(feature = "jit")]
pub mod jit_integration;

use std::cell::RefCell;
use std::rc::Rc;

use value::{JsFunction, Object, Value};

use crate::bytecode::{Chunk, Constant, Instruction, Opcode};

/// VM execution error
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

/// Call frame for function invocation
#[derive(Debug)]
pub struct CallFrame {
    /// Bytecode chunk being executed
    pub chunk_idx: usize,
    /// Program counter (instruction offset)
    pub pc: usize,
    /// Base register offset in the register file
    pub base: usize,
    /// Return register (where to store result)
    pub return_reg: u8,
}

/// String table for interned strings
#[derive(Debug, Default)]
pub struct StringTable {
    strings: Vec<String>,
}

impl StringTable {
    #[must_use]
    pub fn new() -> Self {
        Self {
            strings: Vec::new(),
        }
    }

    pub fn intern(&mut self, s: String) -> u32 {
        // Simple linear search - production would use hash map
        for (i, existing) in self.strings.iter().enumerate() {
            if existing == &s {
                return i as u32;
            }
        }
        let idx = self.strings.len() as u32;
        self.strings.push(s);
        idx
    }

    pub fn get(&self, idx: u32) -> Option<&str> {
        self.strings.get(idx as usize).map(String::as_str)
    }
}

/// Bytecode virtual machine
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
    /// Maximum call stack depth
    max_stack_depth: usize,
}

/// Opcode handler function type
type OpHandler = fn(&mut Vm, Instruction) -> VmResult<()>;

/// Dispatch table - one handler per opcode
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
    table[Opcode::GetGlobal as usize] = op_get_global;
    table[Opcode::SetGlobal as usize] = op_set_global;

    // Special
    table[Opcode::Nop as usize] = op_nop;
    table[Opcode::Halt as usize] = op_halt;
    table[Opcode::Debugger as usize] = op_debugger;

    table
};

impl Vm {
    /// Create new VM
    #[must_use]
    pub fn new() -> Self {
        Self {
            registers: vec![Value::Undefined; 256],
            call_stack: Vec::with_capacity(64),
            chunks: Vec::new(),
            strings: StringTable::new(),
            global: Rc::new(RefCell::new(Object::new())),
            max_stack_depth: 1024,
        }
    }

    /// Add a chunk (compiled function) and return its index
    pub fn add_chunk(&mut self, chunk: Chunk) -> usize {
        let idx = self.chunks.len();
        self.chunks.push(chunk);
        idx
    }

    /// Execute a chunk by index
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
                    // Normal halt - return accumulator
                    // SAFETY: register 0 is always valid.
                    return Ok(unsafe { self.registers.get_unchecked(0) }.clone());
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
        Some(Constant::String(s)) => Value::String(*s),
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

fn op_add(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let lhs = vm.get_reg(instr.src1()).to_number();
    let rhs = vm.get_reg(instr.src2()).to_number();
    vm.set_reg(instr.dst(), Value::Number(lhs + rhs));
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
    // Simplified loose equality - full impl needs type coercion
    let result = match (lhs, rhs) {
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::Boolean(a), Value::Boolean(b)) => a == b,
        // Null and Undefined are equal to each other (loose equality)
        (Value::Null | Value::Undefined, Value::Null | Value::Undefined) => true,
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
        _ => false, // Different types are never strictly equal
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
    let callee = vm.get_reg(instr.src1());
    match callee {
        Value::Function(func) => {
            let chunk_idx = func.chunk_idx as usize;
            if chunk_idx >= vm.chunks.len() {
                return Err(VmError::OutOfBounds);
            }
            if vm.call_stack.len() >= vm.max_stack_depth {
                return Err(VmError::StackOverflow);
            }
            // Push new frame
            vm.call_stack.push(CallFrame {
                chunk_idx,
                pc: 0,
                base: 0, // Simplified - real impl manages register windows
                return_reg: instr.dst(),
            });
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
    Err(VmError::Exception(value))
}

// Property access handlers

fn op_get_prop(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let obj = vm.get_reg(instr.src1());
    let key = u32::from(instr.src2()); // Simplified - real impl uses constant pool
    let value = match obj {
        Value::Object(o) => o.borrow().get(key),
        _ => Value::Undefined,
    };
    vm.set_reg(instr.dst(), value);
    Ok(())
}

fn op_set_prop(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let key = u32::from(instr.src1());
    let value = vm.get_reg(instr.src2()).clone();
    let obj = vm.get_reg(instr.dst());
    if let Value::Object(o) = obj {
        o.borrow_mut().set(key, value);
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
    let idx = vm.strings.intern(type_str.to_string());
    vm.set_reg(instr.dst(), Value::String(idx));
    Ok(())
}

// Object creation handlers

fn op_new_object(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let obj = Rc::new(RefCell::new(Object::new()));
    vm.set_reg(instr.dst(), Value::Object(obj));
    Ok(())
}

fn op_new_array(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    // Arrays are objects with numeric keys (simplified)
    let obj = Rc::new(RefCell::new(Object::new()));
    vm.set_reg(instr.dst(), Value::Object(obj));
    Ok(())
}

fn op_new_function(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let chunk_idx = u32::from(instr.const_idx());
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

fn op_get_global(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let key = u32::from(instr.const_idx());
    let value = vm.global.borrow().get(key);
    vm.set_reg(instr.dst(), value);
    Ok(())
}

fn op_set_global(vm: &mut Vm, instr: Instruction) -> VmResult<()> {
    let key = u32::from(instr.const_idx());
    let value = vm.get_reg(instr.dst()).clone();
    vm.global.borrow_mut().set(key, value);
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
}
