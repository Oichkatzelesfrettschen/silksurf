//! Bytecode opcodes for SilkSurfJS VM
//!
//! Register-based instruction set with 50+ opcodes.
//! Derived from ECMA-262 specification requirements.

use static_assertions::{assert_eq_size, const_assert_eq};
use zerocopy::{Immutable, IntoBytes, KnownLayout, TryFromBytes};

/// Bytecode opcode enumeration
///
/// Each opcode is 8 bits, allowing 256 possible instructions.
/// Organized by category for clarity and cache locality.
///
/// # Safety
/// - `TryFromBytes` allows safe conversion from raw bytes with validation
/// - `IntoBytes` allows safe conversion to bytes
/// - The optimized `from_byte` method uses range-based validation for performance
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromBytes, IntoBytes, KnownLayout, Immutable)]
pub enum Opcode {
    // ========================================
    // Load/Store (0x00-0x0F)
    // ========================================
    /// Load constant: r[dst] = constants[idx]
    LoadConst = 0x00,
    /// Load true: r[dst] = true
    LoadTrue = 0x01,
    /// Load false: r[dst] = false
    LoadFalse = 0x02,
    /// Load null: r[dst] = null
    LoadNull = 0x03,
    /// Load undefined: r[dst] = undefined
    LoadUndefined = 0x04,
    /// Move: r[dst] = r[src]
    Mov = 0x05,
    /// Load small integer: r[dst] = imm16 (sign-extended)
    LoadSmi = 0x06,
    /// Load zero: r[dst] = 0
    LoadZero = 0x07,
    /// Load one: r[dst] = 1
    LoadOne = 0x08,
    /// Load minus one: r[dst] = -1
    LoadMinusOne = 0x09,

    // ========================================
    // Arithmetic (0x10-0x1F)
    // ========================================
    /// Add: r[dst] = r[src1] + r[src2]
    Add = 0x10,
    /// Subtract: r[dst] = r[src1] - r[src2]
    Sub = 0x11,
    /// Multiply: r[dst] = r[src1] * r[src2]
    Mul = 0x12,
    /// Divide: r[dst] = r[src1] / r[src2]
    Div = 0x13,
    /// Modulo: r[dst] = r[src1] % r[src2]
    Mod = 0x14,
    /// Exponentiation: r[dst] = r[src1] ** r[src2]
    Pow = 0x15,
    /// Negate: r[dst] = -r[src]
    Neg = 0x16,
    /// Increment: r[dst] = r[src] + 1
    Inc = 0x17,
    /// Decrement: r[dst] = r[src] - 1
    Dec = 0x18,

    // ========================================
    // Comparison (0x20-0x2F)
    // ========================================
    /// Loose equality: r[dst] = r[src1] == r[src2]
    Eq = 0x20,
    /// Strict equality: r[dst] = r[src1] === r[src2]
    StrictEq = 0x21,
    /// Loose inequality: r[dst] = r[src1] != r[src2]
    Ne = 0x22,
    /// Strict inequality: r[dst] = r[src1] !== r[src2]
    StrictNe = 0x23,
    /// Less than: r[dst] = r[src1] < r[src2]
    Lt = 0x24,
    /// Less than or equal: r[dst] = r[src1] <= r[src2]
    Le = 0x25,
    /// Greater than: r[dst] = r[src1] > r[src2]
    Gt = 0x26,
    /// Greater than or equal: r[dst] = r[src1] >= r[src2]
    Ge = 0x27,

    // ========================================
    // Logical/Bitwise (0x30-0x3F)
    // ========================================
    /// Logical NOT: r[dst] = !r[src]
    Not = 0x30,
    /// Bitwise NOT: r[dst] = ~r[src]
    BitNot = 0x31,
    /// Bitwise AND: r[dst] = r[src1] & r[src2]
    BitAnd = 0x32,
    /// Bitwise OR: r[dst] = r[src1] | r[src2]
    BitOr = 0x33,
    /// Bitwise XOR: r[dst] = r[src1] ^ r[src2]
    BitXor = 0x34,
    /// Shift left: r[dst] = r[src1] << r[src2]
    Shl = 0x35,
    /// Signed right shift: r[dst] = r[src1] >> r[src2]
    Shr = 0x36,
    /// Unsigned right shift: r[dst] = r[src1] >>> r[src2]
    Ushr = 0x37,

    // ========================================
    // Control Flow (0x40-0x4F)
    // ========================================
    /// Unconditional jump
    Jmp = 0x40,
    /// Jump if truthy: if r[cond] then pc += offset
    JmpTrue = 0x41,
    /// Jump if falsy: if !r[cond] then pc += offset
    JmpFalse = 0x42,
    /// Jump if nullish: if r[cond] == null || r[cond] == undefined
    JmpNullish = 0x43,
    /// Jump if not nullish
    JmpNotNullish = 0x44,
    /// Function call: r[dst] = r[callee](r[base]..r[base+argc-1])
    Call = 0x45,
    /// Method call: r[dst] = r[obj].name(args)
    CallMethod = 0x46,
    /// Tail call optimization
    TailCall = 0x47,
    /// Return value: return r[src]
    Ret = 0x48,
    /// Return undefined
    RetUndefined = 0x49,
    /// Throw exception: throw r[src]
    Throw = 0x4A,

    // ========================================
    // Property Access (0x50-0x5F)
    // ========================================
    /// Get property: r[dst] = r[obj].name (inline cache slot)
    GetProp = 0x50,
    /// Set property: r[obj].name = r[val] (inline cache slot)
    SetProp = 0x51,
    /// Get element: r[dst] = r[obj][r[key]]
    GetElem = 0x52,
    /// Set element: r[obj][r[key]] = r[val]
    SetElem = 0x53,
    /// Delete property: r[dst] = delete r[obj].name
    DeleteProp = 0x54,
    /// Delete element: r[dst] = delete r[obj][r[key]]
    DeleteElem = 0x55,
    /// In operator: r[dst] = r[key] in r[obj]
    In = 0x56,
    /// Instanceof: r[dst] = r[obj] instanceof r[ctor]
    Instanceof = 0x57,
    /// Typeof: r[dst] = typeof r[src]
    Typeof = 0x58,

    // ========================================
    // Object/Array Creation (0x60-0x6F)
    // ========================================
    /// Create empty object: r[dst] = {}
    NewObject = 0x60,
    /// Create array: r[dst] = new Array(len)
    NewArray = 0x61,
    /// Create function: r[dst] = Function(func_idx)
    NewFunction = 0x62,
    /// Create arrow function
    NewArrow = 0x63,
    /// Create generator function
    NewGenerator = 0x64,
    /// Create async function
    NewAsync = 0x65,
    /// Create class
    NewClass = 0x66,
    /// Create RegExp: r[dst] = /pattern/flags
    NewRegExp = 0x67,
    /// Define property with attributes
    DefineProperty = 0x68,
    /// Define getter
    DefineGetter = 0x69,
    /// Define setter
    DefineSetter = 0x6A,
    /// Spread into array: [...r[src]]
    SpreadArray = 0x6B,
    /// Spread into call: f(...r[src])
    SpreadCall = 0x6C,

    // ========================================
    // Scope/Environment (0x70-0x7F)
    // ========================================
    /// Get local: r[dst] = locals[slot]
    GetLocal = 0x70,
    /// Set local: locals[slot] = r[src]
    SetLocal = 0x71,
    /// Get captured variable: r[dst] = captures[depth][slot]
    GetCapture = 0x72,
    /// Set captured variable: captures[depth][slot] = r[src]
    SetCapture = 0x73,
    /// Get global: r[dst] = global.name
    GetGlobal = 0x74,
    /// Set global: global.name = r[src]
    SetGlobal = 0x75,
    /// Create binding (TDZ-aware)
    CreateBinding = 0x76,
    /// Check TDZ: throw if uninitialized
    CheckTdz = 0x77,
    /// Push scope
    PushScope = 0x78,
    /// Pop scope
    PopScope = 0x79,

    // ========================================
    // Iterators/Generators (0x80-0x8F)
    // ========================================
    /// Get iterator: r[dst] = r[obj][Symbol.iterator]()
    GetIterator = 0x80,
    /// Get async iterator: r[dst] = r[obj][Symbol.asyncIterator]()
    GetAsyncIterator = 0x81,
    /// Iterator next: r[dst] = r[iter].next()
    IterNext = 0x82,
    /// Check iterator done: r[dst] = r[result].done
    IterDone = 0x83,
    /// Get iterator value: r[dst] = r[result].value
    IterValue = 0x84,
    /// Close iterator
    IterClose = 0x85,
    /// Yield value
    Yield = 0x86,
    /// Yield delegate: yield* r[src]
    YieldStar = 0x87,
    /// Await promise
    Await = 0x88,

    // ========================================
    // Exception Handling (0x90-0x9F)
    // ========================================
    /// Enter try block
    EnterTry = 0x90,
    /// Leave try block
    LeaveTry = 0x91,
    /// Enter catch block
    EnterCatch = 0x92,
    /// Enter finally block
    EnterFinally = 0x93,
    /// Rethrow exception
    Rethrow = 0x94,
    /// Get exception: r[dst] = current exception
    GetException = 0x95,

    // ========================================
    // Special (0xF0-0xFF)
    // ========================================
    /// No operation
    Nop = 0xF0,
    /// Debugger statement
    Debugger = 0xF1,
    /// Wide instruction prefix (16-bit operands follow)
    Wide = 0xFE,
    /// Halt execution
    Halt = 0xFF,
}

// Compile-time size verification - opcode must be exactly 1 byte
assert_eq_size!(Opcode, u8);
const_assert_eq!(std::mem::size_of::<Opcode>(), 1);

impl Opcode {
    /// Decode opcode from byte using zerocopy's safe validation
    ///
    /// This method leverages zerocopy's `TryFromBytes` derive to safely
    /// convert a byte to an Opcode without unsafe code.
    #[inline]
    pub fn from_byte(byte: u8) -> Option<Self> {
        Self::try_read_from_bytes(&[byte]).ok()
    }

    /// Fast path decode using optimized range checking
    ///
    /// This is an optimized version that uses range-based validation
    /// instead of checking each variant individually. Useful for hot paths.
    ///
    /// # Safety
    /// Safe because we validate the byte is in a valid opcode range before transmuting.
    /// The Opcode enum is `#[repr(u8)]` and all bytes in the validated ranges
    /// correspond to valid enum discriminants.
    #[inline]
    pub fn from_byte_fast(byte: u8) -> Option<Self> {
        match byte {
            0x00..=0x09 | 0x10..=0x18 | 0x20..=0x27 | 0x30..=0x37
            | 0x40..=0x4A | 0x50..=0x58 | 0x60..=0x6C | 0x70..=0x79
            | 0x80..=0x88 | 0x90..=0x95 | 0xF0..=0xF1 | 0xFE..=0xFF => {
                // SAFETY: All bytes in these ranges correspond to valid Opcode discriminants.
                // The enum is #[repr(u8)] so transmute is sound for valid discriminants.
                Some(unsafe { std::mem::transmute(byte) })
            }
            _ => None,
        }
    }

    /// Get the number of operands this opcode uses
    #[inline]
    pub const fn operand_count(self) -> u8 {
        use Opcode::*;
        match self {
            // No operands
            Nop | Debugger | Halt | RetUndefined | LeaveTry | Rethrow => 0,

            // 1 operand (dst or src)
            LoadTrue | LoadFalse | LoadNull | LoadUndefined | LoadZero | LoadOne
            | LoadMinusOne | Ret | Throw | PushScope | PopScope => 1,

            // 2 operands (dst + src or dst + immediate)
            Mov | Neg | Inc | Dec | Not | BitNot | Typeof | LoadConst | LoadSmi
            | Jmp | NewObject | NewArray | GetLocal | SetLocal | GetGlobal
            | SetGlobal | GetIterator | GetAsyncIterator | IterNext | IterDone
            | IterValue | IterClose | Yield | YieldStar | Await | EnterTry
            | EnterCatch | EnterFinally | GetException | NewFunction | NewArrow
            | NewGenerator | NewAsync | NewClass | NewRegExp | CheckTdz
            | CreateBinding | Wide => 2,

            // 3 operands (dst + src1 + src2)
            Add | Sub | Mul | Div | Mod | Pow | Eq | StrictEq | Ne | StrictNe
            | Lt | Le | Gt | Ge | BitAnd | BitOr | BitXor | Shl | Shr | Ushr
            | JmpTrue | JmpFalse | JmpNullish | JmpNotNullish | GetProp | SetProp
            | GetElem | SetElem | DeleteProp | DeleteElem | In | Instanceof
            | Call | CallMethod | TailCall | GetCapture | SetCapture
            | DefineProperty | DefineGetter | DefineSetter | SpreadArray
            | SpreadCall => 3,
        }
    }

    /// Check if this opcode can branch
    #[inline]
    pub const fn is_branch(self) -> bool {
        use Opcode::*;
        matches!(
            self,
            Jmp | JmpTrue | JmpFalse | JmpNullish | JmpNotNullish
        )
    }

    /// Check if this opcode terminates a basic block
    #[inline]
    pub const fn is_terminator(self) -> bool {
        use Opcode::*;
        matches!(
            self,
            Ret | RetUndefined | Throw | Halt | Jmp | TailCall | Rethrow
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcode_encoding() {
        assert_eq!(Opcode::LoadConst as u8, 0x00);
        assert_eq!(Opcode::Add as u8, 0x10);
        assert_eq!(Opcode::Halt as u8, 0xFF);
    }

    #[test]
    fn test_opcode_decode() {
        assert_eq!(Opcode::from_byte(0x00), Some(Opcode::LoadConst));
        assert_eq!(Opcode::from_byte(0x10), Some(Opcode::Add));
        assert_eq!(Opcode::from_byte(0x99), None); // Invalid
    }

    #[test]
    fn test_opcode_decode_fast() {
        // Test fast path gives same results as zerocopy path
        assert_eq!(Opcode::from_byte_fast(0x00), Some(Opcode::LoadConst));
        assert_eq!(Opcode::from_byte_fast(0x10), Some(Opcode::Add));
        assert_eq!(Opcode::from_byte_fast(0xFF), Some(Opcode::Halt));
        assert_eq!(Opcode::from_byte_fast(0x99), None); // Invalid

        // Verify both methods agree on all valid opcodes
        for byte in 0u8..=255 {
            assert_eq!(
                Opcode::from_byte(byte),
                Opcode::from_byte_fast(byte),
                "Mismatch at byte {:#x}",
                byte
            );
        }
    }

    #[test]
    fn test_opcode_into_bytes() {
        use zerocopy::IntoBytes;
        // Test that IntoBytes derive works correctly
        let opcode = Opcode::Add;
        let bytes = opcode.as_bytes();
        assert_eq!(bytes, &[0x10]);

        let opcode = Opcode::Halt;
        let bytes = opcode.as_bytes();
        assert_eq!(bytes, &[0xFF]);
    }

    #[test]
    fn test_opcode_properties() {
        assert!(Opcode::Jmp.is_branch());
        assert!(!Opcode::Add.is_branch());
        assert!(Opcode::Ret.is_terminator());
        assert!(!Opcode::Add.is_terminator());
    }
}
